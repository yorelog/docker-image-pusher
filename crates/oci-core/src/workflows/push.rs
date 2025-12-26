/// Common utilities for OCI image push workflows.
/// Provides high-level abstractions for pushing layers, configs, and manifests to registries.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::core::auth::RegistryAuth;
use crate::core::blobs::{LayerUploadOptions, LayerUploadPool, LocalLayer, UploadSummary};
use crate::core::client::Client;
use crate::core::errors::OciError;
use crate::core::manifest::OciImageManifest;
use crate::core::reference::Reference;

/// Encapsulates metadata needed to push a complete OCI image.
#[derive(Debug, Clone)]
pub struct PushMetadata {
    /// Target registry reference
    pub target_ref: Reference,
    /// All layers to be pushed
    pub layers: Vec<LocalLayer>,
    /// Configuration descriptor
    pub config: ConfigBlob,
    /// Image manifest
    pub manifest: OciImageManifest,
    /// Optional index for multi-platform images
    pub index: Option<IndexBlob>,
}

/// Configuration blob metadata
#[derive(Debug, Clone)]
pub struct ConfigBlob {
    pub digest: String,
    pub data: Vec<u8>,
}

/// Index manifest (for multi-architecture support)
#[derive(Debug, Clone)]
pub struct IndexBlob {
    pub media_type: String,
    pub data: Vec<u8>,
}

/// Options controlling how a push operation behaves.
#[derive(Debug, Clone)]
pub struct PushOptions {
    /// Layer upload options (chunk size, concurrency, etc.)
    pub layer_upload_opts: Option<LayerUploadOptions>,
    /// Whether to skip pre-checking blobs for existence
    pub skip_blob_check: bool,
}

impl Default for PushOptions {
    fn default() -> Self {
        Self {
            layer_upload_opts: None,
            skip_blob_check: false,
        }
    }
}

/// Result of a push operation.
#[derive(Debug)]
pub struct PushResult {
    /// Number of layers uploaded
    pub layers_uploaded: usize,
    /// Number of layers skipped (already existed)
    pub layers_skipped: usize,
    /// Whether config was uploaded
    pub config_uploaded: bool,
    /// Whether index was pushed (if applicable)
    pub index_pushed: bool,
}

/// Unified interface for pushing OCI images.
/// This trait allows implementations to provide their own orchestration
/// while using common building blocks from the OCI library.
#[allow(async_fn_in_trait)]
pub trait OciImagePusher {
    /// Push the complete image (layers + config + manifest + index)
    async fn push(&self, metadata: PushMetadata, opts: PushOptions) -> Result<PushResult, OciError>;
}

/// Common validation for push operations.
pub fn validate_push_metadata(metadata: &PushMetadata) -> Result<(), OciError> {
    if metadata.layers.is_empty() {
        return Err(OciError::PushError("Image must have at least one layer".to_string()));
    }

    if metadata.config.digest.is_empty() {
        return Err(OciError::PushError("Config digest is required".to_string()));
    }

    if metadata.target_ref.repository.is_empty() {
        return Err(OciError::PushError("Target reference repository is required".to_string()));
    }

    Ok(())
}

/// Result of checking and filtering blobs for upload.
#[derive(Debug)]
pub struct BlobCheckResult {
    pub layers_to_upload: Vec<LocalLayer>,
    pub skipped_count: usize,
}

/// Check remote registry for blob existence and filter layers.
/// Returns layers that need to be uploaded.
pub async fn check_and_filter_layers(
    client: &Client,
    target_ref: &Reference,
    auth: &RegistryAuth,
    layers: Vec<LocalLayer>,
) -> Result<BlobCheckResult, OciError> {
    let mut layers_to_upload = Vec::new();
    let mut skipped_count = 0;

    for layer in layers {
        match client.blob_exists(target_ref, &layer.digest, auth).await {
            Ok(true) => {
                skipped_count += 1;
                continue;
            }
            Ok(false) => {
                // Blob doesn't exist, proceed with upload
            }
            Err(_err) => {
                // Unable to check, attempt upload anyway
            }
        }
        layers_to_upload.push(layer);
    }

    Ok(BlobCheckResult {
        layers_to_upload,
        skipped_count,
    })
}

/// Check if config blob exists in remote registry.
pub async fn check_config_exists(
    client: &Client,
    target_ref: &Reference,
    auth: &RegistryAuth,
    config_digest: &str,
) -> Result<bool, OciError> {
    match client.blob_exists(target_ref, config_digest, auth).await {
        Ok(exists) => Ok(exists),
        Err(_) => {
            // If we can't check, assume it doesn't exist and attempt upload
            Ok(false)
        }
    }
}

/// Upload config blob if it doesn't exist.
pub async fn upload_config_if_needed(
    client: &Client,
    target_ref: &Reference,
    auth: &RegistryAuth,
    config_digest: &str,
    config_bytes: &[u8],
    config_exists: bool,
) -> Result<(), OciError> {
    if !config_exists {
        client
            .push_blob(target_ref, auth, config_bytes, config_digest)
            .await?;
    }
    Ok(())
}

/// Stream layers through the upload pool with concurrent workers.
pub async fn upload_layers_concurrent(
    client: &Client,
    target_ref: &Reference,
    auth: Arc<RegistryAuth>,
    layers: Vec<LocalLayer>,
    options: LayerUploadOptions,
) -> Result<UploadSummary, OciError> {
    if layers.is_empty() {
        return Ok(UploadSummary {
            uploaded: vec![],
            skipped: 0,
        });
    }

    let (tx, rx) = mpsc::channel::<LocalLayer>(options.concurrency * 2);

    // Spawn upload task
    let client_clone = client.clone();
    let target_ref_clone = target_ref.clone();
    let auth_clone = Arc::clone(&auth);
    let options_clone = options.clone();

    let upload_task = tokio::task::spawn(async move {
        let uploader = LayerUploadPool::new(&client_clone, &target_ref_clone, auth_clone, options_clone);
        uploader.upload_stream(rx).await
    });

    // Send layers
    for layer in layers {
        tx.send(layer).await.map_err(|e| {
            OciError::Network(format!("Failed to enqueue layer: {}", e))
        })?;
    }
    drop(tx);

    // Wait for upload to complete
    upload_task
        .await
        .map_err(|err| OciError::Network(format!("Upload task failed: {}", err)))?
}

/// Push manifest bytes to remote registry.
pub async fn push_manifest_bytes(
    client: &Client,
    target_ref: &Reference,
    auth: &RegistryAuth,
    manifest_media_type: &str,
    manifest_bytes: &[u8],
) -> Result<String, OciError> {
    client
        .push_manifest_bytes(target_ref, manifest_media_type, manifest_bytes, auth)
        .await
}

/// Optionally push index bytes (for multi-architecture support).
pub async fn push_index_bytes_if_present(
    client: &Client,
    target_ref: &Reference,
    auth: &RegistryAuth,
    index_media_type: Option<&str>,
    index_bytes: Option<&[u8]>,
) -> Result<(), OciError> {
    if let (Some(mt), Some(bytes)) = (index_media_type, index_bytes) {
        client
            .push_manifest_bytes(target_ref, mt, bytes, auth)
            .await?;
    }
    Ok(())
}
