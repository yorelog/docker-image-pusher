use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bolt_lite::{Bolt, Bucket, Tx};

use crate::types::{Descriptor, ImageEntry, ResolvedImage, Result, StoreError};

#[cfg(feature = "bucket-logging")]
static BUCKET_MATCH_LOGGER: OnceLock<Box<dyn Fn(BucketMatch) + Send + Sync>> = OnceLock::new();

static SIZE_WARN_ONCE: OnceLock<()> = OnceLock::new();

/// Category of bucket being resolved so layout differences can be traced.
#[derive(Clone, Copy, Debug)]
pub enum BucketKind {
    Images,
    Content,
    Leases,
    Snapshots,
}

/// Information sent to the optional bucket-match logger.
#[cfg(feature = "bucket-logging")]
#[derive(Clone, Debug)]
pub struct BucketMatch {
    pub kind: BucketKind,
    pub path: Vec<String>,
}

const META_DIR: &str = "io.containerd.metadata.v1.bolt";
const META_DB: &str = "meta.db";

pub struct ContainerdStore {
    root: PathBuf,
    namespace: String,
    db_path: PathBuf,
}

#[cfg(feature = "bucket-logging")]
pub fn set_bucket_match_logger<F>(logger: F) -> Result<(), &'static str>
where
    F: Fn(BucketMatch) + Send + Sync + 'static,
{
    BUCKET_MATCH_LOGGER
        .set(Box::new(logger))
        .map_err(|_| "bucket match logger already set")
}

impl ContainerdStore {
    pub fn open<P: AsRef<Path>>(root: P, namespace: &str) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let db_path = root.join(META_DIR).join(META_DB);
        if !db_path.exists() {
            return Err(StoreError::DbOpen(format!(
                "metadata DB not found at {}",
                db_path.display()
            )));
        }
        Bolt::open_ro(&db_path).map_err(|e| {
            StoreError::DbOpen(format!(
                "failed to open metadata DB at {}: {}",
                db_path.display(),
                e
            ))
        })?;
        Ok(Self {
            root,
            namespace: namespace.to_string(),
            db_path,
        })
    }

    pub fn content_root(&self) -> PathBuf {
        self.root
            .join("io.containerd.content.v1.content")
            .join("blobs")
    }

    pub fn meta_db_path(&self) -> PathBuf {
        self.db_path.clone()
    }

    pub fn root_path(&self) -> PathBuf {
        self.root.clone()
    }

    pub fn list_images(&self) -> Result<Vec<ImageEntry>> {
        let namespace = self.namespace.clone();
        let mut images = Vec::new();

        let db = Bolt::open_ro(&self.db_path).map_err(|e| StoreError::Db(format!("{}", e)))?;
        let tx = db.begin().map_err(|e| StoreError::Db(format!("{}", e)))?;

        let images_bucket = find_images_bucket(&tx, &namespace)
            .ok_or_else(|| StoreError::ImagesBucketMissing(namespace.clone()))?;

        for (name_bytes, bucket) in images_bucket.iter_buckets() {
            let name = std::str::from_utf8(&name_bytes)
                .map_err(|e| StoreError::Utf8(e.to_string()))?
                .to_string();
            if let Some(entry) = parse_image_bucket(&bucket, &name)? {
                images.push(entry);
            }
        }

        Ok(images)
    }

    pub fn resolve_image(&self, name: &str) -> Result<ResolvedImage> {
        let content_root = self.content_root();
        let namespace = self.namespace.clone();

        let db = Bolt::open_ro(&self.db_path).map_err(|e| StoreError::Db(format!("{}", e)))?;
        let tx = db.begin().map_err(|e| StoreError::Db(format!("{}", e)))?;

        let images_bucket = find_images_bucket(&tx, &namespace)
            .ok_or_else(|| StoreError::ImagesBucketMissing(namespace.clone()))?;

        let image_bucket = images_bucket
            .bucket(name.as_bytes())
            .ok_or_else(|| StoreError::ImageNotFound(name.to_string()))?;
        let entry = parse_image_bucket(&image_bucket, name)?
            .ok_or_else(|| StoreError::DescriptorMissing(name.to_string()))?;

        let manifest_digest = &entry.target.digest;
        let digest_ref = crate::types::DigestRef::parse(manifest_digest)?;
        let manifest_path = digest_ref.path_under(&content_root);
        Ok(ResolvedImage {
            entry,
            manifest_path,
        })
    }
}

fn parse_image_bucket(bucket: &Bucket<'_>, name: &str) -> Result<Option<ImageEntry>> {
    // Prefer nested "target" bucket; fallback to current bucket keys.
    if let Some(target) = bucket.bucket(b"target") {
        if let Some(desc) = read_descriptor_bucket(&target)? {
            let created_at = read_str_entry(bucket, b"createdat");
            let updated_at = read_str_entry(bucket, b"updatedat");
            return Ok(Some(ImageEntry {
                name: name.to_string(),
                target: desc,
                created_at,
                updated_at,
            }));
        }
    }

    if let Some(desc) = read_descriptor_bucket(bucket)? {
        let created_at = read_str_entry(bucket, b"createdat");
        let updated_at = read_str_entry(bucket, b"updatedat");
        return Ok(Some(ImageEntry {
            name: name.to_string(),
            target: desc,
            created_at,
            updated_at,
        }));
    }

    Ok(None)
}

fn read_descriptor_bucket(bucket: &Bucket<'_>) -> Result<Option<Descriptor>> {
    let mut digest = read_str_entry(bucket, b"digest");
    let mut media_type = read_str_entry(bucket, b"mediatype");
    let mut size = parse_size(bucket.get(b"size"));

    // If any field is missing, try parsing a JSON descriptor stored directly under "target".
    if digest.is_none() || media_type.is_none() || size.is_none() {
        if let Some(raw) = bucket.get(b"target") {
            if let Ok(json_desc) = serde_json::from_slice::<self::json_models::Target>(&raw) {
                if digest.is_none() {
                    digest = Some(json_desc.digest);
                }
                if media_type.is_none() {
                    media_type = Some(json_desc.media_type);
                }
                if size.is_none() {
                    size = json_desc.size;
                }
            }
        }
    }

    if let (Some(digest), Some(media_type)) = (digest, media_type) {
        let size = size.unwrap_or_else(|| {
            warn_size_default();
            0
        });
        return Ok(Some(Descriptor {
            media_type,
            digest,
            size,
        }));
    }

    Ok(None)
}

fn read_str_entry(bucket: &Bucket<'_>, key: &[u8]) -> Option<String> {
    bucket.get(key).and_then(|v| String::from_utf8(v).ok())
}

fn parse_size(raw: Option<Vec<u8>>) -> Option<i64> {
    match raw {
        Some(bytes) if bytes.len() == 8 => {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes);
            Some(i64::from_le_bytes(buf))
        }
        Some(bytes) => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|s| s.parse::<i64>().ok()),
        None => None,
    }
}

fn warn_size_default() {
    SIZE_WARN_ONCE.get_or_init(|| {
        eprintln!("containerd-store: missing or invalid descriptor size; defaulting to 0");
    });
}

fn find_bucket_for<'a>(
    tx: &'a Tx<'a>,
    namespace: &str,
    leaf: &[u8],
    kind: BucketKind,
) -> Option<Bucket<'a>> {
    let ns = namespace.as_bytes();
    for path in candidate_bucket_paths(ns, leaf) {
        let bucket = if path.len() == 1 {
            tx.bucket(path[0])
        } else {
            tx.bucket_path(&path)
        };
        if let Some(b) = bucket {
            log_bucket_match(kind, &path);
            return Some(b);
        }
    }
    None
}

fn candidate_bucket_paths<'a>(namespace: &'a [u8], leaf: &'a [u8]) -> Vec<Vec<&'a [u8]>> {
    vec![
        vec![b"v1".as_ref(), namespace, leaf],
        vec![
            b"metadata".as_ref(),
            b"namespaces".as_ref(),
            namespace,
            leaf,
        ],
        vec![b"metadata".as_ref(), leaf],
        vec![b"v1".as_ref(), leaf],
        vec![leaf],
    ]
}

fn find_images_bucket<'a>(tx: &'a Tx<'a>, namespace: &str) -> Option<Bucket<'a>> {
    find_bucket_for(tx, namespace, b"images", BucketKind::Images)
}

#[allow(dead_code)]
fn find_content_bucket<'a>(tx: &'a Tx<'a>, namespace: &str) -> Option<Bucket<'a>> {
    find_bucket_for(tx, namespace, b"content", BucketKind::Content)
}

#[allow(dead_code)]
fn find_leases_bucket<'a>(tx: &'a Tx<'a>, namespace: &str) -> Option<Bucket<'a>> {
    find_bucket_for(tx, namespace, b"leases", BucketKind::Leases)
}

#[allow(dead_code)]
fn find_snapshots_bucket<'a>(tx: &'a Tx<'a>, namespace: &str) -> Option<Bucket<'a>> {
    find_bucket_for(tx, namespace, b"snapshots", BucketKind::Snapshots)
}

#[cfg(feature = "bucket-logging")]
fn log_bucket_match(kind: BucketKind, path: &[&[u8]]) {
    if let Some(logger) = BUCKET_MATCH_LOGGER.get() {
        let rendered = path
            .iter()
            .map(|segment| String::from_utf8_lossy(segment).to_string())
            .collect();
        logger(BucketMatch {
            kind,
            path: rendered,
        });
    }
}

#[cfg(not(feature = "bucket-logging"))]
fn log_bucket_match(_kind: BucketKind, _path: &[&[u8]]) {}

// Lightweight JSON models for fallback parsing when bucket stores JSON blobs.
pub(crate) mod json_models {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    pub struct Target {
        #[serde(rename = "mediaType")]
        pub media_type: String,
        pub digest: String,
        pub size: Option<i64>,
    }
}
