use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("BoltDB open error: {0}")]
    DbOpen(String),
    #[error("BoltDB access error: {0}")]
    Db(String),
    #[error("Containerd images bucket not found (namespace: {0})")] 
    ImagesBucketMissing(String),
    #[error("Image not found: {0}")]
    ImageNotFound(String),
    #[error("Descriptor missing for image: {0}")]
    DescriptorMissing(String),
    #[error("Invalid UTF-8 in key/value: {0}")]
    Utf8(String),
    #[error("Invalid digest format: {0}")]
    Digest(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Descriptor {
    pub media_type: String,
    pub digest: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageEntry {
    pub name: String,
    pub target: Descriptor,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedImage {
    pub entry: ImageEntry,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ManifestInfo {
    pub config_digest: Option<String>,
    pub layer_digests: Vec<String>,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PortableImageExport {
    pub manifest_digest: String,
    pub blobs_root: PathBuf,
    pub copied: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DigestRef {
    pub algo: String,
    pub hex: String,
}

impl fmt::Display for DigestRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algo, self.hex)
    }
}

impl DigestRef {
    pub fn parse(d: &str) -> Result<Self> {
        let mut parts = d.splitn(2, ':');
        let algo = parts
            .next()
            .ok_or_else(|| StoreError::Digest("missing algo".into()))?;
        let hex = parts
            .next()
            .ok_or_else(|| StoreError::Digest("missing hex".into()))?;
        if algo.is_empty() || hex.is_empty() {
            return Err(StoreError::Digest("empty digest parts".into()));
        }
        Ok(DigestRef {
            algo: algo.to_string(),
            hex: hex.to_string(),
        })
    }

    pub fn path_under(&self, root: &PathBuf) -> PathBuf {
        root.join(&self.algo).join(&self.hex)
    }
}
