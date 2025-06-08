//! Blob handling operations
//!
//! This module contains blob-specific functionality extracted from image_manager
//! to improve modularity and reduce file size.

use crate::common::Validatable;
use crate::error::{RegistryError, Result};
use crate::image::{cache::BlobInfo, digest::DigestUtils, Cache};
use crate::logging::Logger;
use crate::registry::RegistryClient;

/// Blob handler for processing Docker/OCI blobs
pub struct BlobHandler {
    logger: Logger,
}

impl BlobHandler {
    pub fn new(logger: Logger) -> Self {
        Self { logger }
    }

    /// Push all blobs to registry with enhanced verification
    pub async fn push_blobs_to_registry(
        &self,
        client: &RegistryClient,
        repository: &str,
        blobs: &[BlobInfo],
        token: &Option<String>,
        cache: &Cache,
    ) -> Result<()> {
        self.logger.step(&format!("Pushing {} blobs with enhanced progress tracking", blobs.len()));

        // Display pipeline info for blob uploads
        self.logger.info(&format!(
            "Starting enhanced progress monitoring for {} blobs...",
            blobs.len()
        ));

        // Note: For cache-based pushes, we'll use basic blob upload with enhanced verification
        for blob in blobs {
            // 先验证blob在缓存中的完整性
            if !cache.has_blob_with_verification(&blob.digest, blob.is_config) {
                return Err(RegistryError::Cache {
                    message: format!("Blob {} failed integrity verification in cache", &blob.digest[..16]),
                    path: Some(blob.path.clone()),
                });
            }

            // 读取blob数据
            let blob_data = cache.get_blob(&blob.digest)?;
            
            // 对于config blob或小文件，进行额外的SHA256验证
            if blob.is_config || blob.size <= 10 * 1024 * 1024 {
                let actual_digest = format!(
                    "sha256:{}",
                    DigestUtils::compute_sha256(&blob_data)
                );
                if actual_digest != blob.digest {
                    return Err(RegistryError::Validation(format!(
                        "Blob {} digest mismatch before upload. Expected: {}, Got: {}",
                        &blob.digest[..16], blob.digest, actual_digest
                    )));
                }
            }

            self.logger.verbose(&format!(
                "Uploading blob {} ({}) - verified integrity",
                &blob.digest[..16],
                self.format_size(blob.size)
            ));

            let _ = client
                .upload_blob_with_token(&blob_data, &blob.digest, repository, token)
                .await?;
        }
        Ok(())
    }

    /// Validate cache completeness for an image
    pub fn validate_cache_completeness(
        &self,
        repository: &str,
        reference: &str,
        cache: &Cache,
    ) -> Result<()> {
        if !cache.is_image_complete(repository, reference)? {
            return Err(RegistryError::Cache {
                message: format!(
                    "Image {}/{} is not complete in cache",
                    repository, reference
                ),
                path: None,
            });
        }
        Ok(())
    }

    /// Pull and cache a single blob with verification
    pub async fn pull_and_cache_blob(
        &self,
        client: &RegistryClient,
        repository: &str,
        digest: &str,
        token: &Option<String>,
        is_config: bool,
        cache: &mut Cache,
    ) -> Result<()> {
        // 使用增强的缓存检查，避免大文件的昂贵SHA256验证
        let verify_integrity = is_config || cache.get_blob_size(digest).map_or(true, |size| size <= 10 * 1024 * 1024);
        
        if cache.has_blob_with_verification(digest, verify_integrity) {
            self.logger
                .detail(&format!("Blob {} already in cache (verified)", &digest[..16]));
            return Ok(());
        }

        self.logger
            .detail(&format!("Downloading blob {}", &digest[..16]));

        let blob_data = client.pull_blob(repository, digest, token).await?;

        // 使用增强的blob缓存方法，支持智能验证策略
        cache
            .add_blob_with_verification(digest, &blob_data, is_config, !is_config, false)
            .await?;
            
        self.logger
            .detail(&format!("Cached blob {} ({} bytes) with verification", &digest[..16], blob_data.len()));
            
        Ok(())
    }

    /// Associate blob with image in cache
    pub async fn associate_blob_with_image(
        &self,
        repository: &str,
        reference: &str,
        digest: &str,
        is_config: bool,
        cache: &mut Cache,
    ) -> Result<()> {
        let size = cache.get_blob_size(digest).unwrap_or(0);
        cache
            .associate_blob_with_image(repository, reference, digest, size, is_config, !is_config)
    }

    /// Pull and cache layers using legacy method (for compatibility)
    #[allow(dead_code)]
    pub async fn pull_and_cache_layers(
        &self,
        client: &RegistryClient,
        repository: &str,
        reference: &str,
        manifest: &serde_json::Value,
        token: &Option<String>,
        cache: &mut Cache,
    ) -> Result<()> {
        if let Some(layers) = manifest.get("layers").and_then(|l| l.as_array()) {
            self.logger
                .step(&format!("Pulling {} layer blobs", layers.len()));

            for (i, layer) in layers.iter().enumerate() {
                let layer_digest =
                    layer
                        .get("digest")
                        .and_then(|d| d.as_str())
                        .ok_or_else(|| {
                            RegistryError::Parse(format!("Missing digest for layer {}", i))
                        })?;

                self.logger.detail(&format!(
                    "Layer {}/{}: {}",
                    i + 1,
                    layers.len(),
                    &layer_digest[..16]
                ));

                self.pull_and_cache_blob(client, repository, layer_digest, token, false, cache)
                    .await?;
                self.associate_blob_with_image(repository, reference, layer_digest, false, cache)
                    .await?;
            }
        }
        Ok(())
    }

    // Helper method for formatting sizes
    fn format_size(&self, size: u64) -> String {
        crate::common::FormatUtils::format_bytes(size)
    }
}

impl Validatable for BlobHandler {
    type Error = RegistryError;
    
    fn validate(&self) -> std::result::Result<(), Self::Error> {
        // BlobHandler is always valid
        Ok(())
    }
}
