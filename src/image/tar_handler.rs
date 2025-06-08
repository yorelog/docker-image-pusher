//! Tar file operations
//!
//! This module contains tar-specific functionality extracted from image_manager
//! to improve modularity and reduce file size.

use crate::common::Validatable;
use crate::error::{RegistryError, Result};
use crate::image::parser::{ImageInfo, LayerInfo};
use crate::logging::Logger;
use crate::registry::{tar_utils::TarUtils, RegistryClient, PipelineConfig, UnifiedPipeline};
use std::path::Path;

/// Tar file handler for processing Docker/OCI tar archives
pub struct TarHandler {
    logger: Logger,
    pipeline_config: PipelineConfig,
    use_optimized_upload: bool,
}

impl TarHandler {
    pub fn new(logger: Logger, pipeline_config: PipelineConfig, use_optimized_upload: bool) -> Self {
        Self {
            logger,
            pipeline_config,
            use_optimized_upload,
        }
    }

    /// Push from tar file using optimized unified pipeline
    pub async fn push_from_tar_optimized(
        &self,
        client: &RegistryClient,
        tar_file: &str,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<()> {
        let tar_path = Path::new(tar_file);
        self.validate_tar_file(tar_path)?;
        
        self.logger.info(&format!(
            "Pushing {}/{} from tar file (unified pipeline)",
            repository, reference
        ));

        // Parse tar file to get layer information
        let image_info = TarUtils::parse_image_info(tar_path)?;

        self.logger.detail(&format!(
            "Found {} layers, total size: {}",
            image_info.layers.len(),
            self.format_size(image_info.total_size)
        ));

        // Create unified pipeline with enhanced progress display
        let pipeline =
            UnifiedPipeline::new(self.logger.clone()).with_config(self.pipeline_config.clone());

        // Upload config blob first (not included in layers)
        let config_data = TarUtils::extract_config_data(tar_path, &image_info.config_digest)?;
        client
            .upload_blob_with_token(&config_data, &image_info.config_digest, repository, token)
            .await?;

        // Process layer uploads using unified pipeline with enhanced progress tracking
        self.logger.info("Starting enhanced progress monitoring...");
        pipeline
            .process_uploads(
                &image_info.layers,
                repository,
                tar_path,
                token,
                std::sync::Arc::new(client.clone()),
            )
            .await?;

        // Create and push manifest
        let manifest_json = self.create_manifest_from_image_info(&image_info)?;
        client
            .upload_manifest_with_token(&manifest_json, repository, reference, token)
            .await?;

        self.logger.success(&format!(
            "Successfully pushed {}/{} from tar file (unified pipeline)",
            repository, reference
        ));
        Ok(())
    }

    /// Push directly from tar file (without caching)
    pub async fn push_from_tar(
        &self,
        client: &RegistryClient,
        tar_file: &str,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<()> {
        let tar_path = Path::new(tar_file);
        self.validate_tar_file(tar_path)?;
        
        self.logger.info(&format!(
            "Pushing {}/{} directly from tar file",
            repository, reference
        ));

        // 解析tar文件获取镜像信息
        let image_info = TarUtils::parse_image_info(tar_path)?;

        self.logger.detail(&format!(
            "Found {} layers, total size: {}",
            image_info.layers.len(),
            self.format_size(image_info.total_size)
        ));

        // 推送config blob
        self.push_config_from_tar(
            client,
            tar_path,
            &image_info.config_digest,
            repository,
            token,
        )
        .await?;

        // 推送所有layer blobs
        self.push_layers_from_tar(client, tar_path, &image_info.layers, repository, token)
            .await?;

        // 创建并推送manifest
        let manifest_json = self.create_manifest_from_image_info(&image_info)?;
        client
            .upload_manifest_with_token(&manifest_json, repository, reference, token)
            .await?;

        self.logger.success(&format!(
            "Successfully pushed {}/{} from tar file",
            repository, reference
        ));
        Ok(())
    }

    /// Push config blob from tar file
    pub async fn push_config_from_tar(
        &self,
        client: &RegistryClient,
        tar_path: &Path,
        config_digest: &str,
        repository: &str,
        token: &Option<String>,
    ) -> Result<()> {
        let config_data = TarUtils::extract_config_data(tar_path, config_digest)?;
        let _ = client
            .upload_blob_with_token(&config_data, config_digest, repository, token)
            .await?;
        Ok(())
    }

    /// Push layer blobs from tar file
    pub async fn push_layers_from_tar(
        &self,
        client: &RegistryClient,
        tar_path: &Path,
        layers: &[LayerInfo],
        repository: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.logger
            .step(&format!("Pushing {} layer blobs", layers.len()));

        for (i, layer) in layers.iter().enumerate() {
            self.logger.detail(&format!(
                "Layer {}/{}: {}",
                i + 1,
                layers.len(),
                &layer.digest[..16]
            ));

            // 检查blob是否已存在
            if client.check_blob_exists(&layer.digest, repository).await? {
                self.logger.detail("Layer already exists, skipping");
                continue;
            }

            // 从tar文件提取layer数据并上传
            let layer_data = TarUtils::extract_layer_data(tar_path, &layer.tar_path)?;
            let _ = client
                .upload_blob_with_token(&layer_data, &layer.digest, repository, token)
                .await?;
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

    /// Validate tar file
    pub fn validate_tar_file(&self, tar_path: &Path) -> Result<()> {
        if !tar_path.exists() {
            return Err(RegistryError::Validation(format!(
                "Tar file '{}' does not exist",
                tar_path.display()
            )));
        }
        TarUtils::validate_tar_archive(tar_path)
    }

    /// Parse manifest from bytes
    #[allow(dead_code)]
    pub fn parse_manifest(&self, manifest_data: &[u8]) -> Result<serde_json::Value> {
        serde_json::from_slice(manifest_data)
            .map_err(|e| RegistryError::Parse(format!("Failed to parse manifest: {}", e)))
    }

    /// Extract config digest from manifest
    #[allow(dead_code)]
    pub fn extract_config_digest(&self, manifest: &serde_json::Value) -> Result<String> {
        manifest
            .get("config")
            .and_then(|c| c.get("digest"))
            .and_then(|d| d.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| RegistryError::Parse("Missing config digest in manifest".to_string()))
    }

    // Helper method for formatting sizes
    fn format_size(&self, size: u64) -> String {
        crate::common::FormatUtils::format_bytes(size)
    }
}

impl Validatable for TarHandler {
    type Error = RegistryError;
    
    fn validate(&self) -> std::result::Result<(), Self::Error> {
        // TarHandler is always valid
        Ok(())
    }
}
