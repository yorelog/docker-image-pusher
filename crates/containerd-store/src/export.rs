use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::ContainerdStore;
use crate::types::{DigestRef, ManifestInfo, PortableImageExport, Result, StoreError};

pub fn export_image_to_dir(
    store: &ContainerdStore,
    image: &str,
    out: impl AsRef<Path>,
) -> Result<PortableImageExport> {
    let mut all = export_images_to_dir(store, &[image], out)?;
    Ok(all.remove(0))
}

pub fn export_images_to_dir(
    store: &ContainerdStore,
    images: &[&str],
    out: impl AsRef<Path>,
) -> Result<Vec<PortableImageExport>> {
    let out_root = out.as_ref().to_path_buf();
    let mirror_root = out_root
        .join("io.containerd.content.v1.content")
        .join("blobs");
    fs::create_dir_all(&mirror_root)?;

    // copy meta.db so the bundle is self-contained
    let meta_src = store.meta_db_path();
    let meta_dst = out_root
        .join("io.containerd.metadata.v1.bolt")
        .join("meta.db");
    if let Some(parent) = meta_dst.parent() {
        fs::create_dir_all(parent)?;
    }
    if meta_src.exists() {
        fs::copy(&meta_src, &meta_dst)?;
    }

    let content_root = store.content_root();
    let mut copied = HashSet::new();
    let mut results = Vec::new();

    for image in images {
        let resolved = store.resolve_image(image)?;
        let manifest_digest = resolved.entry.target.digest.clone();
        let mut this_copied = Vec::new();
        copy_manifest_tree(
            &content_root,
            &mirror_root,
            &manifest_digest,
            &mut copied,
            &mut this_copied,
        )?;
        results.push(PortableImageExport {
            manifest_digest,
            blobs_root: mirror_root.clone(),
            copied: this_copied,
        });
    }

    Ok(results)
}

fn copy_if_needed(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Err(StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("missing blob: {}", src.display()),
        )));
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    if !dst.exists() {
        fs::copy(src, dst)?;
    }
    Ok(())
}

fn load_manifest(path: &Path) -> Result<ManifestInfo> {
    let bytes = fs::read(path)?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
    let config_digest = manifest["config"]["digest"].as_str().map(|s| s.to_string());
    let mut layers = Vec::new();
    if let Some(arr) = manifest["layers"].as_array() {
        for layer in arr {
            if let Some(d) = layer["digest"].as_str() {
                layers.push(d.to_string());
            }
        }
    }
    Ok(ManifestInfo {
        config_digest,
        layer_digests: layers,
        raw: bytes,
    })
}

enum ManifestKind {
    Image(ManifestInfo),
    Index(String), // selected manifest digest
}

fn parse_manifest_or_index(path: &Path) -> Result<ManifestKind> {
    let bytes = fs::read(path)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;
    if json.get("manifests").is_some() {
        // Treat as OCI index / Docker manifest list
        let manifests = json["manifests"].as_array().cloned().unwrap_or_default();
        let pick = select_platform_manifest(&manifests)
            .ok_or_else(|| StoreError::ImageNotFound("no manifests in index".into()))?;
        let digest = pick
            .get("digest")
            .and_then(|v| v.as_str())
            .ok_or_else(|| StoreError::ImageNotFound("manifest digest missing in index".into()))?;
        return Ok(ManifestKind::Index(digest.to_string()));
    }
    Ok(ManifestKind::Image(load_manifest(path)?))
}

fn select_platform_manifest(manifests: &[serde_json::Value]) -> Option<serde_json::Value> {
    let mut fallback: Option<serde_json::Value> = None;
    for m in manifests {
        if let Some(p) = m.get("platform") {
            let arch = p.get("architecture").and_then(|v| v.as_str()).unwrap_or("");
            let os = p.get("os").and_then(|v| v.as_str()).unwrap_or("");
            if os == "linux" && arch == "amd64" {
                return Some(m.clone());
            }
            if fallback.is_none() {
                fallback = Some(m.clone());
            }
        } else if fallback.is_none() {
            fallback = Some(m.clone());
        }
    }
    fallback
}

fn copy_manifest_tree(
    content_root: &PathBuf,
    mirror_root: &PathBuf,
    manifest_digest: &str,
    copied: &mut HashSet<String>,
    record: &mut Vec<String>,
) -> Result<()> {
    let manifest_ref = DigestRef::parse(manifest_digest)?;
    let manifest_src = manifest_ref.path_under(content_root);
    if !manifest_src.exists() {
        return Err(StoreError::ImageNotFound(format!(
            "manifest blob missing at {}",
            manifest_src.display()
        )));
    }
    let manifest_dst = manifest_ref.path_under(mirror_root);
    copy_if_needed(&manifest_src, &manifest_dst)?;
    if copied.insert(manifest_digest.to_string()) {
        record.push(manifest_digest.to_string());
    }

    let parsed = parse_manifest_or_index(&manifest_dst)?;
    match parsed {
        ManifestKind::Image(manifest) => {
            if let Some(cfg) = manifest.config_digest.as_ref() {
                let cfg_ref = DigestRef::parse(cfg)?;
                let cfg_src = cfg_ref.path_under(content_root);
                let cfg_dst = cfg_ref.path_under(mirror_root);
                copy_if_needed(&cfg_src, &cfg_dst)?;
                if copied.insert(cfg.to_string()) {
                    record.push(cfg.to_string());
                }
            }
            for layer in &manifest.layer_digests {
                let lr = DigestRef::parse(layer)?;
                let ls = lr.path_under(content_root);
                let ld = lr.path_under(mirror_root);
                copy_if_needed(&ls, &ld)?;
                if copied.insert(layer.clone()) {
                    record.push(layer.clone());
                }
            }
            Ok(())
        }
        ManifestKind::Index(next_digest) => {
            // Ensure nested manifest is also copied.
            copy_manifest_tree(content_root, mirror_root, &next_digest, copied, record)
        }
    }
}

// Utility to compute digest of a file (sha256) if needed later
#[allow(dead_code)]
fn digest_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let hex = format!("{:x}", hasher.finalize());
    Ok(format!("sha256:{}", hex))
}
