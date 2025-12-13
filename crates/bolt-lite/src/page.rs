use crate::{Error, Result};

#[derive(Clone, Debug)]
pub(crate) struct LeafEntry {
    pub(crate) key: Vec<u8>,
    pub(crate) value: Vec<u8>,
    pub(crate) flags: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct BranchElem {
    pub(crate) key: Vec<u8>,
    pub(crate) pgid: u64,
}

pub(crate) fn page_flags(page: &[u8]) -> u16 {
    u16::from_le_bytes(page[8..10].try_into().unwrap())
}

pub(crate) fn branch_elements(_page_size: usize, page: &[u8]) -> Result<Vec<BranchElem>> {
    let count = u16::from_le_bytes(page[10..12].try_into().unwrap()) as usize;
    let elem_size = 16; // pos u32, ksize u32, pgid u64
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = 16 + i * elem_size;
        let pos = u32::from_le_bytes(page[base..base + 4].try_into().unwrap()) as usize;
        let ksize = u32::from_le_bytes(page[base + 4..base + 8].try_into().unwrap()) as usize;
        let pgid = u64::from_le_bytes(page[base + 8..base + 16].try_into().unwrap());
        let key_start = base + pos;
        let key_end = key_start + ksize;
        if key_end > page.len() {
            return Err(Error::Corrupt("branch key out of bounds"));
        }
        let key = page[key_start..key_end].to_vec();
        out.push(BranchElem { key, pgid });
    }
    Ok(out)
}

pub(crate) fn leaf_elements(_page_size: usize, page: &[u8]) -> Result<Vec<LeafEntry>> {
    let count = u16::from_le_bytes(page[10..12].try_into().unwrap()) as usize;
    let elem_size = 16; // flags u32, pos u32, ksize u32, vsize u32
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = 16 + i * elem_size;
        if base + elem_size > page.len() {
            return Err(Error::Corrupt("leaf element out of bounds"));
        }
        let flags = u32::from_le_bytes(page[base..base + 4].try_into().unwrap());
        let pos = u32::from_le_bytes(page[base + 4..base + 8].try_into().unwrap()) as usize;
        let ksize = u32::from_le_bytes(page[base + 8..base + 12].try_into().unwrap()) as usize;
        let vsize = u32::from_le_bytes(page[base + 12..base + 16].try_into().unwrap()) as usize;
        let key_start = base + pos;
        let key_end = key_start + ksize;
        let val_end = key_end + vsize;
        if val_end > page.len() {
            return Err(Error::Corrupt("leaf key/value out of bounds"));
        }
        let key = page[key_start..key_end].to_vec();
        let value = page[key_end..val_end].to_vec();
        out.push(LeafEntry { key, value, flags });
    }
    Ok(out)
}

pub(crate) fn collect_leaf_entries(page_size: usize, page: &[u8]) -> Result<Vec<LeafEntry>> {
    leaf_elements(page_size, page)
}
