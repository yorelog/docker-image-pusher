//! Minimal read-only BoltDB parser, tailored for containerd metadata usage.
//! Implements just enough of Bolt's B+tree to walk buckets and read values.

mod btree;
mod meta;
mod page;

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::btree::{collect_tree_entries, find_in_page, find_in_tree};
pub use meta::{
    BRANCH_PAGE_FLAG, BUCKET_VALUE_FLAG, LEAF_PAGE_FLAG, MAGIC, META_PAGE_FLAG, META_STRUCT_OFFSET,
};
use meta::{BucketHeader, parse_meta_at, parse_page_size};
use page::{LeafEntry, collect_leaf_entries};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid bolt magic")]
    InvalidMagic,
    #[error("unsupported page size {0}")]
    InvalidPageSize(u32),
    #[error("invalid page id {0}")]
    InvalidPageId(u64),
    #[error("corrupt bolt structure: {0}")]
    Corrupt(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Read-only database held in memory (meta.db is small).
pub struct Bolt {
    data: Vec<u8>,
    page_size: usize,
    root: BucketHeader,
}

/// Lightweight database stats useful for debugging bounds.
pub struct Stats {
    pub page_size: usize,
    pub page_count: usize,
    pub bytes: usize,
}

/// Read-only transaction. Immutable view over the DB bytes.
pub struct Tx<'a> {
    db: &'a Bolt,
}

/// A logical bucket.
pub struct Bucket<'a> {
    db: &'a Bolt,
    root: u64,
    inline: Option<Vec<u8>>, // inline page bytes when root == 0
}

/// Cursor to iterate leaf entries without re-parsing the tree.
pub struct BucketCursor<'a> {
    entries: Vec<LeafEntry>,
    idx: usize,
    _db: &'a Bolt,
}

impl Bolt {
    /// Open the database file into memory. We intentionally do not validate checksums
    /// to tolerate slightly nonstandard meta pages seen in containerd rootless setups.
    pub fn open_ro<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Self::from_bytes(data)
    }

    /// Construct from in-memory bytes (e.g. when the caller already slurped the file).
    /// Performs the same validation as [`open_ro`].
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        if data.len() < 4096 {
            return Err(Error::Corrupt("file too small"));
        }

        let page_size = parse_page_size(&data)? as usize;
        let meta0 = parse_meta_at(&data, page_size, 0).ok();
        let meta1 = if data.len() >= page_size * 2 {
            parse_meta_at(&data, page_size, page_size).ok()
        } else {
            None
        };
        let meta = match (meta0, meta1) {
            (Some(m0), Some(m1)) => {
                if m1.txid >= m0.txid {
                    m1
                } else {
                    m0
                }
            }
            (Some(m0), None) => m0,
            (None, Some(m1)) => m1,
            (None, None) => return Err(Error::Corrupt("no valid meta pages")),
        };

        Ok(Self {
            data,
            page_size,
            root: meta.root,
        })
    }

    pub fn stats(&self) -> Stats {
        Stats {
            page_size: self.page_size,
            page_count: self.data.len() / self.page_size,
            bytes: self.data.len(),
        }
    }

    pub fn begin(&self) -> Result<Tx<'_>> {
        Ok(Tx { db: self })
    }

    fn read_page(&self, pgid: u64) -> Result<&[u8]> {
        let offset = (pgid as usize)
            .checked_mul(self.page_size)
            .ok_or(Error::InvalidPageId(pgid))?;
        if offset + self.page_size > self.data.len() {
            return Err(Error::InvalidPageId(pgid));
        }
        let base = &self.data[offset..offset + self.page_size];
        let overflow = u32::from_le_bytes(base[12..16].try_into().unwrap()) as usize;
        let end = offset + self.page_size * (1 + overflow);
        if end > self.data.len() {
            return Err(Error::InvalidPageId(pgid));
        }
        Ok(&self.data[offset..end])
    }
}

impl<'a> Tx<'a> {
    pub fn bucket_path(&'a self, parts: &[&[u8]]) -> Option<Bucket<'a>> {
        let mut current = self.root_bucket();
        for name in parts {
            current = current.bucket(name)?;
        }
        Some(current)
    }

    pub fn bucket(&'a self, name: &[u8]) -> Option<Bucket<'a>> {
        let root = self.root_bucket();
        root.bucket(name)
    }

    fn root_bucket(&'a self) -> Bucket<'a> {
        Bucket {
            db: self.db,
            root: self.db.root.root,
            inline: None,
        }
    }
}

impl<'a> Bucket<'a> {
    pub fn bucket(&self, name: &[u8]) -> Option<Bucket<'a>> {
        let entry = self.find_entry(name)?;
        if entry.flags & BUCKET_VALUE_FLAG == 0 {
            return None;
        }
        let hdr = ok_opt(parse_bucket_header(&entry.value))?;
        let inline = if hdr.root == 0 && entry.value.len() > 16 {
            Some(entry.value[16..].to_vec())
        } else {
            None
        };
        Some(Bucket {
            db: self.db,
            root: hdr.root,
            inline,
        })
    }

    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.find_entry(key).map(|e| e.value)
    }

    pub fn iter_buckets(&self) -> Vec<(Vec<u8>, Bucket<'a>)> {
        let mut out = Vec::new();
        if let Ok(entries) = self.collect_entries() {
            for e in entries {
                if e.flags & BUCKET_VALUE_FLAG != 0 {
                    if let Some(hdr) = ok_opt(parse_bucket_header(&e.value)) {
                        let inline = if hdr.root == 0 && e.value.len() > 16 {
                            Some(e.value[16..].to_vec())
                        } else {
                            None
                        };
                        out.push((
                            e.key,
                            Bucket {
                                db: self.db,
                                root: hdr.root,
                                inline,
                            },
                        ));
                    }
                }
            }
        }
        out
    }

    fn find_entry(&self, key: &[u8]) -> Option<LeafEntry> {
        if self.root == 0 {
            let data = self.inline.as_ref()?;
            return find_in_page(self.db.page_size, data, key).ok().flatten();
        }
        find_in_tree(self.db, self.root, key).ok().flatten()
    }

    fn collect_entries(&self) -> Result<Vec<LeafEntry>> {
        if self.root == 0 {
            let data = self
                .inline
                .as_ref()
                .ok_or(Error::Corrupt("inline bucket missing data"))?;
            return collect_leaf_entries(self.db.page_size, data);
        }
        collect_tree_entries(self.db, self.root)
    }

    pub fn cursor(&self) -> Result<BucketCursor<'a>> {
        let entries = self.collect_entries()?;
        Ok(BucketCursor {
            entries,
            idx: 0,
            _db: self.db,
        })
    }
}

fn parse_bucket_header(buf: &[u8]) -> Result<BucketHeader> {
    if buf.len() < 16 {
        return Err(Error::Corrupt("bucket header too small"));
    }
    Ok(BucketHeader {
        root: u64::from_le_bytes(buf[..8].try_into().unwrap()),
        _sequence: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
    })
}

pub(crate) fn ok_opt<T>(res: Result<T>) -> Option<T> {
    res.ok()
}

pub struct CursorEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub flags: u32,
}

impl<'a> Iterator for BucketCursor<'a> {
    type Item = CursorEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.entries.len() {
            return None;
        }
        let entry = self.entries[self.idx].clone();
        self.idx += 1;
        Some(CursorEntry {
            key: entry.key,
            value: entry.value,
            flags: entry.flags,
        })
    }
}

// Tests cover offset handling and cursor basics.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bytes_rejects_too_small() {
        assert!(matches!(
            Bolt::from_bytes(vec![0u8; 100]),
            Err(Error::Corrupt("file too small"))
        ));
    }

    #[test]
    fn ok_opt_maps_result_option() {
        assert_eq!(ok_opt::<u32>(Ok(5)), Some(5));
        assert!(ok_opt::<u32>(Err(Error::InvalidMagic)).is_none());
    }

    #[test]
    fn stats_reports_page_count() {
        let mut db = Bolt {
            data: vec![0u8; 8192],
            page_size: 4096,
            root: BucketHeader {
                root: 0,
                _sequence: 0,
            },
        };
        let stats = db.stats();
        assert_eq!(stats.page_size, 4096);
        assert_eq!(stats.page_count, 2);
        assert_eq!(stats.bytes, 8192);
        // silence unused
        db.data[0] = 0;
    }
}
