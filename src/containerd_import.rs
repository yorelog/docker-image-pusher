use std::fs;
use std::path::PathBuf;

use containerd_store::ContainerdStore;
use oci_core::auth::RegistryAuth;
use oci_core::blobs::{LayerUploadOptions, LayerUploadPool, LocalLayer, ProgressOptions};
use oci_core::client::Client;
use oci_core::manifest::OciImageManifest;
use oci_core::reference::Reference;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::{
    CHUNKED_LAYER_SIZE_BYTES, ESTIMATED_SPEED_MBPS, LARGE_LAYER_PROGRESS_INTERVAL_SECS,
    LARGE_LAYER_THRESHOLD_BYTES, LARGE_LAYER_THRESHOLD_MB, MAX_CHUNKED_LAYER_SIZE_BYTES,
    NORMAL_LAYER_PROGRESS_INTERVAL_SECS, PusherError, RATE_LIMIT_DELAY_MS, state,
};

pub async fn run_push_containerd(
    client: &Client,
    root: Option<&str>,
    namespace: &str,
    image: &str,
    target: Option<String>,
    username: Option<String>,
    password: Option<String>,
    registry: Option<String>,
    blob_chunk: Option<usize>,
) -> Result<(), PusherError> {
    let root_path = resolve_root(root)?;
    let store = ContainerdStore::open(&root_path, namespace)
        .map_err(|e| PusherError::push_error(format!("Failed to open containerd store: {e}")))?;

    let resolved = store.resolve_image(image).map_err(|e| {
        PusherError::push_error(format!("Failed to resolve image in containerd: {e}"))
    })?;

    let target_image = select_target(&resolved.entry.name, target, registry);
    let target_ref: Reference = target_image
        .parse()
        .map_err(|e| PusherError::push_error(format!("Invalid target reference: {e}")))?;

    let (user, pass) = resolve_credentials(&target_ref, username, password).await?;
    let auth = std::sync::Arc::new(RegistryAuth::basic(&user, &pass));

    let manifest_payload = load_image_manifest(&store, &resolved.entry.target)
        .map_err(|e| PusherError::push_error(format!("Failed to load manifest: {e}")))?;
    let manifest = manifest_payload.manifest.clone();

    // Gather layers
    let mut layers = Vec::new();
    for layer in &manifest.layers {
        let digest = layer.digest.clone();
        let path = digest_to_path(&store, &digest)?;
        let meta = fs::metadata(&path).map_err(|e| {
            PusherError::push_error(format!(
                "Missing layer {} at {}: {e}",
                digest,
                path.display()
            ))
        })?;
        layers.push(LocalLayer {
            digest,
            media_type: layer.media_type.clone(),
            size: meta.len(),
            path,
        });
    }

    // Upload layers with bounded concurrency.
    let chunk_size_bytes = blob_chunk
        .map(|mb| (mb * 1024 * 1024).min(MAX_CHUNKED_LAYER_SIZE_BYTES))
        .unwrap_or(CHUNKED_LAYER_SIZE_BYTES)
        .max(1);

    let upload_options = LayerUploadOptions {
        chunk_size_bytes,
        large_layer_threshold_bytes: LARGE_LAYER_THRESHOLD_BYTES,
        medium_layer_threshold_mb: 250.0,
        rate_limit_delay_ms: RATE_LIMIT_DELAY_MS,
        concurrency: 3,
        progress: ProgressOptions {
            large_layer_threshold_mb: LARGE_LAYER_THRESHOLD_MB,
            large_interval_secs: LARGE_LAYER_PROGRESS_INTERVAL_SECS,
            normal_interval_secs: NORMAL_LAYER_PROGRESS_INTERVAL_SECS,
            estimated_speed_mbps: ESTIMATED_SPEED_MBPS,
        },
        progress_reporter: Some(crate::progress_display::docker_like_progress_reporter()),
    };

    println!("📤 Uploading layers for {}", target_ref);
    let (tx, rx) = mpsc::channel::<LocalLayer>(upload_options.concurrency * 2);
    for layer in layers {
        tx.send(layer)
            .await
            .map_err(|e| PusherError::push_error(format!("Failed to enqueue layer: {e}")))?;
    }
    drop(tx);

    let uploader = LayerUploadPool::new(client, &target_ref, auth.clone(), upload_options);
    let upload_summary = uploader
        .upload_stream(rx)
        .await
        .map_err(|e| PusherError::push_error(format!("Layer upload failed: {e}")))?;
    if upload_summary.skipped > 0 {
        println!("💡 Skipped {} existing layer(s)", upload_summary.skipped);
    }

    // Push config blob
    let config_digest = &manifest.config.digest;
    let config_path = digest_to_path(&store, config_digest)?;
    let config_bytes = fs::read(&config_path).map_err(|e| {
        PusherError::push_error(format!("Failed to read config {}: {e}", config_digest))
    })?;
    println!("⚙️  Uploading config {}", config_digest);
    client
        .push_blob(&target_ref, auth.as_ref(), &config_bytes, config_digest)
        .await
        .map_err(|e| PusherError::push_error(format!("Failed to upload config: {e}")))?;

    println!("📋 Pushing manifest to registry: {}", target_image);
    client
        .push_manifest_bytes(
            &target_ref,
            &manifest_payload.manifest_media_type,
            &manifest_payload.manifest_bytes,
            auth.as_ref(),
        )
        .await
        .map_err(|e| PusherError::push_error(format!("Failed to push manifest: {e}")))?;

    if let Some(index) = manifest_payload.index {
        println!("📦 Pushing index for tag (platform-limited)");
        client
            .push_manifest_bytes(&target_ref, &index.media_type, &index.bytes, auth.as_ref())
            .await
            .map_err(|e| PusherError::push_error(format!("Failed to push index: {e}")))?;
    }

    state::record_push_target(&target_image).await?;
    println!("✅ Pushed image {} from containerd store", target_image);
    Ok(())
}

/// List images recorded in containerd's metadata.db.
pub async fn list_images(root: Option<&str>, namespace: &str) -> Result<(), PusherError> {
    let root_path = resolve_root(root)?;
    let store = ContainerdStore::open(&root_path, namespace)
        .map_err(|e| PusherError::push_error(format!("Failed to open containerd store: {e}")))?;
    let images = store
        .list_images()
        .map_err(|e| PusherError::push_error(format!("Failed to read images: {e}")))?;

    if images.is_empty() {
        println!("(no images found in namespace {})", namespace);
        return Ok(());
    }

    println!("📚 Images in namespace '{}':", namespace);
    for img in images {
        println!(
            "- {} -> {} (mediaType={}, size={})",
            img.name, img.target.digest, img.target.media_type, img.target.size
        );
    }
    Ok(())
}

/// Export a specific image's blobs/config from containerd into a portable directory
/// that mirrors containerd's content store layout, for easy copying.
/// If `digest` is provided, we skip metadata lookup and copy blobs under that manifest digest.
pub async fn export_images(
    root: Option<&str>,
    namespace: &str,
    images: &[String],
    out: &str,
    digest: Option<&str>,
) -> Result<(), PusherError> {
    let root_path = resolve_root(root)?;
    let store = ContainerdStore::open(&root_path, namespace)
        .map_err(|e| PusherError::push_error(format!("Failed to open containerd store: {e}")))?;

    if digest.is_some() && images.len() != 1 {
        return Err(PusherError::push_error(
            "--digest can only be used with a single image",
        ));
    }

    println!("🧳 Exporting image(s) {:?} (ns: {})", images, namespace);
    if let Some(manifest_digest) = digest {
        export_by_manifest(&store, manifest_digest, out)?;
    } else {
        let refs: Vec<&str> = images.iter().map(|s| s.as_str()).collect();
        containerd_store::export_images_to_dir(&store, &refs, out)
            .map_err(|e| PusherError::push_error(format!("Export failed: {e}")))?;
    }
    println!("📦 Export complete at {}", out);
    Ok(())
}

fn resolve_root(root: Option<&str>) -> Result<PathBuf, PusherError> {
    if let Some(dir) = root {
        return Ok(PathBuf::from(dir));
    }

    let home = std::env::var("HOME").map_err(|_| {
        PusherError::push_error("HOME not set; please provide --root for containerd directory")
    })?;
    let rootless = PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("containerd");
    let rootless_meta = rootless
        .join("io.containerd.metadata.v1.bolt")
        .join("meta.db");
    if rootless_meta.exists() {
        return Ok(rootless);
    }

    Err(PusherError::push_error(
        "Could not find containerd root (checked ~/.local/share/containerd); pass --root explicitly",
    ))
}

fn export_by_manifest(store: &ContainerdStore, digest: &str, out: &str) -> Result<(), PusherError> {
    let digest_ref = containerd_store::DigestRef::parse(digest)
        .map_err(|e| PusherError::push_error(format!("Invalid digest: {e}")))?;
    let content_root = store.content_root();
    let manifest_src = digest_ref.path_under(&content_root);
    if !manifest_src.exists() {
        return Err(PusherError::push_error(format!(
            "Manifest blob not found: {}",
            manifest_src.display()
        )));
    }
    let out_root = PathBuf::from(out);
    copy_meta_db(store, &out_root)?;

    let mirror_root = out_root
        .join("io.containerd.content.v1.content")
        .join("blobs");
    std::fs::create_dir_all(&mirror_root)
        .map_err(|e| PusherError::push_error(format!("Failed to create output: {e}")))?;

    let manifest_dst = digest_ref.path_under(&mirror_root);
    copy_file(&manifest_src, &manifest_dst)?;

    let manifest_bytes = fs::read(&manifest_dst)
        .map_err(|e| PusherError::push_error(format!("Failed to read manifest: {e}")))?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| PusherError::push_error(format!("Failed to parse manifest JSON: {e}")))?;

    if let Some(cfg) = manifest["config"]["digest"].as_str() {
        copy_blob_from_store(store, &mirror_root, cfg)?;
    }
    if let Some(layers) = manifest["layers"].as_array() {
        for layer in layers {
            if let Some(d) = layer["digest"].as_str() {
                copy_blob_from_store(store, &mirror_root, d)?;
            }
        }
    }
    Ok(())
}

fn copy_meta_db(store: &ContainerdStore, out_root: &PathBuf) -> Result<(), PusherError> {
    let meta_src = store.meta_db_path();
    let meta_dst = out_root
        .join("io.containerd.metadata.v1.bolt")
        .join("meta.db");
    if let Some(parent) = meta_dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            PusherError::push_error(format!("Failed to create dir {}: {e}", parent.display()))
        })?;
    }
    if meta_src.exists() {
        std::fs::copy(&meta_src, &meta_dst)
            .map_err(|e| PusherError::push_error(format!("Failed to copy meta.db: {e}")))?;
    }
    Ok(())
}

fn copy_blob_from_store(
    store: &ContainerdStore,
    mirror_root: &PathBuf,
    digest: &str,
) -> Result<(), PusherError> {
    let digest_ref = containerd_store::DigestRef::parse(digest)
        .map_err(|e| PusherError::push_error(format!("Invalid digest: {e}")))?;
    let src = digest_ref.path_under(&store.content_root());
    let dst = digest_ref.path_under(mirror_root);
    copy_file(&src, &dst)
}

fn copy_file(src: &PathBuf, dst: &PathBuf) -> Result<(), PusherError> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            PusherError::push_error(format!("Failed to create dir {}: {e}", parent.display()))
        })?;
    }
    std::fs::copy(src, dst).map_err(|e| {
        PusherError::push_error(format!(
            "Failed to copy {} -> {}: {e}",
            src.display(),
            dst.display()
        ))
    })?;
    Ok(())
}

fn digest_to_path(store: &ContainerdStore, digest: &str) -> Result<PathBuf, PusherError> {
    let digest_ref = containerd_store::DigestRef::parse(digest)
        .map_err(|e| PusherError::push_error(format!("Invalid digest {}: {}", digest, e)))?;
    Ok(digest_ref.path_under(&store.content_root()))
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct IndexDescriptor {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
    size: i64,
    #[serde(default)]
    platform: Option<IndexPlatform>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[allow(dead_code)]
struct IndexPlatform {
    #[serde(default)]
    architecture: Option<String>,
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    variant: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct OciIndex {
    #[serde(default)]
    #[serde(rename = "mediaType")]
    media_type: Option<String>,
    #[serde(default = "default_schema_version")]
    schema_version: i32,
    #[serde(default)]
    manifests: Vec<IndexDescriptor>,
}

fn default_schema_version() -> i32 {
    2
}

#[derive(Debug, Clone)]
struct ManifestPayload {
    manifest: OciImageManifest,
    manifest_bytes: Vec<u8>,
    manifest_media_type: String,
    index: Option<IndexPayload>,
}

#[derive(Debug, Clone)]
struct IndexPayload {
    media_type: String,
    bytes: Vec<u8>,
}

fn load_image_manifest(
    store: &ContainerdStore,
    desc: &containerd_store::Descriptor,
) -> Result<ManifestPayload, String> {
    let content_root = store.content_root();
    load_manifest_from_desc(&content_root, desc, 0)
}

fn load_manifest_from_desc(
    content_root: &PathBuf,
    desc: &containerd_store::Descriptor,
    depth: usize,
) -> Result<ManifestPayload, String> {
    if depth > 4 {
        return Err("manifest index nesting too deep".into());
    }

    let digest_ref = containerd_store::DigestRef::parse(&desc.digest)
        .map_err(|e| format!("invalid manifest digest {}: {}", desc.digest, e))?;
    let path = digest_ref.path_under(content_root);
    let bytes = fs::read(&path)
        .map_err(|e| format!("failed to read manifest blob {}: {}", path.display(), e))?;

    if is_index_media_type(&desc.media_type) {
        let index: OciIndex = serde_json::from_slice(&bytes)
            .map_err(|e| format!("failed to parse index JSON: {}", e))?;
        let chosen = select_platform_descriptor(&index.manifests)
            .ok_or_else(|| "no manifests found in index".to_string())?;
        let next_desc = containerd_store::Descriptor {
            media_type: chosen.media_type.clone(),
            digest: chosen.digest.clone(),
            size: chosen.size,
        };
        let mut payload = load_manifest_from_desc(content_root, &next_desc, depth + 1)?;
        payload.index = Some(IndexPayload {
            media_type: desc.media_type.clone(),
            bytes,
        });
        return Ok(payload);
    }

    if !is_manifest_media_type(&desc.media_type) {
        return Err(format!(
            "unsupported manifest mediaType: {}",
            desc.media_type
        ));
    }

    let manifest: OciImageManifest = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Failed to parse manifest JSON: {}", e))?;

    Ok(ManifestPayload {
        manifest_media_type: desc.media_type.clone(),
        manifest_bytes: bytes,
        manifest,
        index: None,
    })
}

fn select_platform_descriptor(manifests: &[IndexDescriptor]) -> Option<IndexDescriptor> {
    let mut selected: Option<IndexDescriptor> = None;
    for m in manifests {
        if let Some(p) = &m.platform {
            let arch = p.architecture.as_deref().unwrap_or("");
            let os = p.os.as_deref().unwrap_or("");
            if os == "linux" && arch == "amd64" {
                return Some(m.clone());
            }
            if selected.is_none() {
                selected = Some(m.clone());
            }
        } else if selected.is_none() {
            selected = Some(m.clone());
        }
    }
    selected.or_else(|| manifests.first().cloned())
}

fn is_index_media_type(mt: &str) -> bool {
    matches!(
        mt,
        "application/vnd.oci.image.index.v1+json"
            | "application/vnd.docker.distribution.manifest.list.v2+json"
    )
}

fn is_manifest_media_type(mt: &str) -> bool {
    matches!(
        mt,
        "application/vnd.oci.image.manifest.v1+json"
            | "application/vnd.docker.distribution.manifest.v2+json"
    )
}

fn select_target(
    source_image: &str,
    target: Option<String>,
    registry_override: Option<String>,
) -> String {
    if let Some(t) = target {
        return t;
    }
    if let Some(reg) = registry_override {
        let trimmed = reg.trim_end_matches('/');
        return format!("{}/{}", trimmed, source_image);
    }
    source_image.to_string()
}

async fn resolve_credentials(
    target_ref: &Reference,
    username: Option<String>,
    password: Option<String>,
) -> Result<(String, String), PusherError> {
    if let (Some(u), Some(p)) = (username, password) {
        return Ok((u, p));
    }
    let registry = target_ref.registry_host();
    if let Ok(Some((u, p))) = state::load_credentials(registry).await {
        return Ok((u, p));
    }
    Err(PusherError::push_error(format!(
        "Credentials required for registry {}. Pass --username/--password or login first.",
        registry
    )))
}
