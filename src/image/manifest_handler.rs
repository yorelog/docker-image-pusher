//! Manifest handling operations
//!
//! This module contains manifest-specific functionality extracted from image_manager
//! to improve modularity and reduce file size.

use crate::error::{RegistryError, Result};
use crate::image::cache::Cache;
use crate::image::manifest::{ParsedManifest, parse_manifest_with_type};
use crate::image::parser::{ImageInfo, LayerInfo};
use crate::logging::Logger;
use crate::registry::{RegistryClient, PipelineConfig, UnifiedPipeline};

/// Manifest handler for processing different types of Docker/OCI manifests
pub struct ManifestHandler {
    logger: Logger,
    pipeline_config: PipelineConfig,
}

impl ManifestHandler {
    pub fn new(logger: Logger, pipeline_config: PipelineConfig) -> Self {
        Self {
            logger,
            pipeline_config,
        }
    }

    /// Handle OCI index or Docker manifest list
    pub async fn handle_index_manifest(
        &self,
        client: &RegistryClient,
        repository: &str,
        reference: &str,
        parsed_manifest: &ParsedManifest,
        token: &Option<String>,
        cache: &mut Cache,
    ) -> Result<()> {
        self.logger.info("Processing multi-platform manifest index");

        let platform_manifests = parsed_manifest.platform_manifests.as_ref().ok_or_else(|| {
            RegistryError::Parse("Missing platform manifests in index".to_string())
        })?;

        // For now, pick the first linux/amd64 manifest, or just the first one if no linux/amd64 found
        let target_manifest = platform_manifests
            .iter()
            .find(|m| {
                if let Some(platform) = &m.platform {
                    platform.os == "linux" && platform.architecture == "amd64"
                } else {
                    false
                }
            })
            .or_else(|| platform_manifests.first())
            .ok_or_else(|| {
                RegistryError::Parse("No suitable manifest found in index".to_string())
            })?;

        self.logger.detail(&format!(
            "Selected manifest: {} ({})",
            &target_manifest.digest[..16],
            target_manifest
                .platform
                .as_ref()
                .map(|p| format!("{}/{}", p.os, p.architecture))
                .unwrap_or_else(|| "unknown platform".to_string())
        ));

        // Pull the specific platform manifest using digest as reference
        let platform_manifest_data = client
            .pull_manifest(repository, &target_manifest.digest, token)
            .await?;
        let platform_parsed = parse_manifest_with_type(&platform_manifest_data)?;

        // Save the original index manifest
        if let Some(config_digest) = &platform_parsed.config_digest {
            cache.save_manifest(
                repository,
                reference,
                &parsed_manifest.raw_data,
                config_digest,
            )?;
        }

        // Process the platform-specific manifest
        self.handle_single_manifest(client, repository, reference, &platform_parsed, token, cache)
            .await?;

        Ok(())
    }

    /// Handle single-platform manifest (Docker V2 or OCI) - now using unified pipeline
    pub async fn handle_single_manifest(
        &self,
        client: &RegistryClient,
        repository: &str,
        reference: &str,
        parsed_manifest: &ParsedManifest,
        token: &Option<String>,
        cache: &mut Cache,
    ) -> Result<()> {
        let config_digest = parsed_manifest.config_digest.as_ref().ok_or_else(|| {
            RegistryError::Parse("Missing config digest in single manifest".to_string())
        })?;

        self.logger.detail(&format!(
            "Processing single-platform manifest with config {}",
            &config_digest[..16]
        ));

        // Save manifest to cache if not already done
        cache.save_manifest(
            repository,
            reference,
            &parsed_manifest.raw_data,
            config_digest,
        )?;

        // Pull and cache config blob directly (small, no need for pipeline)
        self.pull_and_cache_blob(client, repository, config_digest, token, true, cache)
            .await?;

        self.associate_blob_with_image(repository, reference, config_digest, true, cache)
            .await?;

        // Convert layer information to LayerInfo for unified pipeline
        let layers: Vec<LayerInfo> = parsed_manifest
            .layer_info
            .iter()
            .enumerate()
            .map(|(index, (digest, size))| {
                LayerInfo {
                    digest: digest.clone(),
                    size: *size,
                    tar_path: format!("layer_{}.tar", index), // Placeholder
                    // Default fields for download operations
                    media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                    compressed_size: Some(*size),
                    offset: None,
                }
            })
            .collect();

        if !layers.is_empty() {
            // Use unified pipeline for batch downloading layers
            let pipeline =
                UnifiedPipeline::new(self.logger.clone()).with_config(self.pipeline_config.clone());

            pipeline
                .process_downloads(
                    &layers,
                    repository,
                    token,
                    std::sync::Arc::new(client.clone()),
                    cache,
                )
                .await?;

            // Associate downloaded blobs with image
            for layer in &layers {
                self.associate_blob_with_image(repository, reference, &layer.digest, false, cache)
                    .await?;
            }
        }

        Ok(())
    }

    /// Create manifest from image info
    pub fn create_manifest_from_image_info(&self, image_info: &ImageInfo) -> Result<String> {
        let config = serde_json::json!({
            "mediaType": "application/vnd.docker.container.image.v1+json",
            "size": image_info.config_size,
            "digest": image_info.config_digest
        });

        let layers: Vec<serde_json::Value> = image_info
            .layers
            .iter()
            .map(|layer| {
                serde_json::json!({
                    "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
                    "size": layer.size,
                    "digest": layer.digest
                })
            })
            .collect();

        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": config,
            "layers": layers
        });

        serde_json::to_string_pretty(&manifest)
            .map_err(|e| RegistryError::Parse(format!("Failed to serialize manifest: {}", e)))
    }

    /// Push manifest to registry with separate source and target coordinates
    pub async fn push_manifest_to_registry_with_source(
        &self,
        client: &RegistryClient,
        source_repository: &str,
        source_reference: &str,
        target_repository: &str,
        target_reference: &str,
        token: &Option<String>,
        cache: &Cache,
    ) -> Result<()> {
        self.logger.step("Pushing manifest");
        // Get manifest from source coordinates in cache
        let manifest_data = cache.get_manifest(source_repository, source_reference)?;
        let manifest_str = String::from_utf8(manifest_data)?;
        // Push manifest to target coordinates in registry
        client
            .upload_manifest_with_token(&manifest_str, target_repository, target_reference, token)
            .await
    }

    // Helper methods
    async fn pull_and_cache_blob(
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

    async fn associate_blob_with_image(
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
}
