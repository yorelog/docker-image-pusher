//! 综合镜像管理器 - 统一处理4种操作模式
//!
//! 提供统一的接口来处理所有4种操作模式，最大化代码复用

use crate::cli::operation_mode::OperationMode;
use crate::error::{RegistryError, Result};
use crate::image::cache::Cache;
use crate::image::manifest::{ManifestType, ParsedManifest, parse_manifest_with_type};
use crate::image::parser::{ImageInfo, LayerInfo};
use crate::logging::Logger;
use crate::registry::RegistryClient;
use crate::registry::tar_utils::TarUtils;
use crate::registry::{PipelineConfig, UnifiedPipeline};
use std::path::Path;

/// 综合镜像管理器 - 4种操作模式的统一入口
pub struct ImageManager {
    cache: Cache,
    output: Logger,
    pipeline_config: PipelineConfig,
    use_optimized_upload: bool,
}

impl ImageManager {
    /// 创建新的镜像管理器
    pub fn new(cache_dir: Option<&str>, verbose: bool) -> Result<Self> {
        let cache = Cache::new(cache_dir)?;
        let output = Logger::new(verbose);
        let pipeline_config = PipelineConfig::default();

        Ok(Self {
            cache,
            output,
            pipeline_config,
            use_optimized_upload: true, // Default to optimized mode
        })
    }

    /// 创建镜像管理器，并允许配置优化选项
    pub fn with_config(
        cache_dir: Option<&str>,
        verbose: bool,
        use_optimized_upload: bool,
    ) -> Result<Self> {
        let cache = Cache::new(cache_dir)?;
        let output = Logger::new(verbose);
        let pipeline_config = PipelineConfig::default();

        Ok(Self {
            cache,
            output,
            pipeline_config,
            use_optimized_upload,
        })
    }

    /// 执行指定的操作模式 - 统一入口点
    pub async fn execute_operation(
        &mut self,
        mode: &OperationMode,
        client: Option<&RegistryClient>,
        auth_token: Option<&str>,
    ) -> Result<()> {
        self.output
            .section(&format!("Executing: {}", mode.description()));
        mode.validate()?;

        match mode {
            OperationMode::PullAndCache {
                repository,
                reference,
            } => {
                self.mode_1_pull_and_cache(client, repository, reference, auth_token)
                    .await
            }
            OperationMode::ExtractAndCache {
                tar_file,
                repository,
                reference,
            } => {
                self.mode_2_extract_and_cache(tar_file, repository, reference)
                    .await
            }
            OperationMode::PushFromCacheUsingManifest {
                repository,
                reference,
            }
            | OperationMode::PushFromCacheUsingTar {
                repository,
                reference,
            } => {
                // 模式3和4使用相同的逻辑，因为缓存格式统一
                self.mode_3_4_push_from_cache(client, repository, reference, auth_token)
                    .await
            }
            OperationMode::PushFromTar {
                tar_file,
                repository,
                reference,
            } => {
                if self.use_optimized_upload {
                    self.mode_5_push_from_tar_optimized(
                        client, tar_file, repository, reference, auth_token,
                    )
                    .await
                } else {
                    self.mode_5_push_from_tar(client, tar_file, repository, reference, auth_token)
                        .await
                }
            }
        }
    }

    // === 4种核心操作模式实现 ===

    /// 模式1: 从repository拉取并缓存
    async fn mode_1_pull_and_cache(
        &mut self,
        client: Option<&RegistryClient>,
        repository: &str,
        reference: &str,
        token: Option<&str>,
    ) -> Result<()> {
        // Validate client first, before any other operations
        let client = client.ok_or_else(|| {
            RegistryError::Validation("Registry client required for this operation".to_string())
        })?;
        let token = token.map(|s| s.to_string());

        self.output.info(&format!(
            "Pulling {}/{} from registry",
            repository, reference
        ));

        // 拉取并解析manifest
        let manifest_data = client.pull_manifest(repository, reference, &token).await?;
        let parsed_manifest = parse_manifest_with_type(&manifest_data)?;

        match parsed_manifest.manifest_type {
            ManifestType::OciIndex | ManifestType::DockerList => {
                // Handle multi-platform manifest
                self.handle_index_manifest(client, repository, reference, &parsed_manifest, &token)
                    .await?;
            }
            ManifestType::DockerV2 | ManifestType::OciManifest => {
                // Handle single-platform manifest
                self.handle_single_manifest(
                    client,
                    repository,
                    reference,
                    &parsed_manifest,
                    &token,
                )
                .await?;
            }
        }

        self.output
            .success(&format!("Successfully cached {}/{}", repository, reference));
        Ok(())
    }

    /// 模式2: 从tar文件提取并缓存
    async fn mode_2_extract_and_cache(
        &mut self,
        tar_file: &str,
        repository: &str,
        reference: &str,
    ) -> Result<()> {
        let tar_path = Path::new(tar_file);
        self.validate_tar_file(tar_path)?;

        self.output.info(&format!(
            "Extracting {} to cache as {}/{}",
            tar_file, repository, reference
        ));

        // 使用统一的tar解析和缓存逻辑
        self.cache.cache_from_tar(tar_path, repository, reference)?;

        self.output.success(&format!(
            "Successfully extracted and cached {}/{}",
            repository, reference
        ));
        Ok(())
    }

    /// 模式3和4: 统一的从缓存推送方法
    async fn mode_3_4_push_from_cache(
        &mut self,
        client: Option<&RegistryClient>,
        repository: &str,
        reference: &str,
        token: Option<&str>,
    ) -> Result<()> {
        let client = self.require_client(client)?;
        let token = token.map(|s| s.to_string());

        self.output.info(&format!(
            "Pushing {}/{} from cache to registry",
            repository, reference
        ));

        // 验证缓存完整性
        self.validate_cache_completeness(repository, reference)?;

        // 推送所有blobs
        let blobs = self.cache.get_image_blobs(repository, reference)?;
        self.push_blobs_to_registry(client, repository, &blobs, &token)
            .await?;

        // 推送manifest
        self.push_manifest_to_registry(client, repository, reference, &token)
            .await?;

        self.output.success(&format!(
            "Successfully pushed {}/{} from cache",
            repository, reference
        ));
        Ok(())
    }

    /// 模式5: 优化的直接从tar文件推送（使用统一管道）
    async fn mode_5_push_from_tar_optimized(
        &mut self,
        client: Option<&RegistryClient>,
        tar_file: &str,
        repository: &str,
        reference: &str,
        token: Option<&str>,
    ) -> Result<()> {
        let client = self.require_client(client)?;
        let token = token.map(|s| s.to_string());
        let tar_path = Path::new(tar_file);

        self.validate_tar_file(tar_path)?;
        self.output.info(&format!(
            "Pushing {}/{} from tar file (unified pipeline)",
            repository, reference
        ));

        // Parse tar file to get layer information
        let image_info = TarUtils::parse_image_info(tar_path)?;

        self.output.detail(&format!(
            "Found {} layers, total size: {}",
            image_info.layers.len(),
            self.output.format_size(image_info.total_size)
        ));

        // Create unified pipeline with configuration
        let pipeline =
            UnifiedPipeline::new(self.output.clone()).with_config(self.pipeline_config.clone());

        // Upload config blob first (not included in layers)
        let config_data = TarUtils::extract_config_data(tar_path, &image_info.config_digest)?;
        client
            .upload_blob_with_token(&config_data, &image_info.config_digest, repository, &token)
            .await?;

        // Process layer uploads using unified pipeline
        pipeline
            .process_uploads(
                &image_info.layers,
                repository,
                tar_path,
                &token,
                std::sync::Arc::new(client.clone()),
            )
            .await?;

        // Create and push manifest
        let manifest_json = self.create_manifest_from_image_info(&image_info)?;
        client
            .upload_manifest_with_token(&manifest_json, repository, reference, &token)
            .await?;

        self.output.success(&format!(
            "Successfully pushed {}/{} from tar file (unified pipeline)",
            repository, reference
        ));
        Ok(())
    }

    /// 模式5: 直接从tar文件推送（无需缓存）
    async fn mode_5_push_from_tar(
        &mut self,
        client: Option<&RegistryClient>,
        tar_file: &str,
        repository: &str,
        reference: &str,
        token: Option<&str>,
    ) -> Result<()> {
        let client = self.require_client(client)?;
        let token = token.map(|s| s.to_string());
        let tar_path = Path::new(tar_file);

        self.validate_tar_file(tar_path)?;
        self.output.info(&format!(
            "Pushing {}/{} directly from tar file",
            repository, reference
        ));

        // 解析tar文件获取镜像信息
        let image_info = TarUtils::parse_image_info(tar_path)?;

        self.output.detail(&format!(
            "Found {} layers, total size: {}",
            image_info.layers.len(),
            self.output.format_size(image_info.total_size)
        ));

        // 推送config blob
        self.push_config_from_tar(
            client,
            tar_path,
            &image_info.config_digest,
            repository,
            &token,
        )
        .await?;

        // 推送所有layer blobs
        self.push_layers_from_tar(client, tar_path, &image_info.layers, repository, &token)
            .await?;

        // 创建并推送manifest
        let manifest_json = self.create_manifest_from_image_info(&image_info)?;
        client
            .upload_manifest_with_token(&manifest_json, repository, reference, &token)
            .await?;

        self.output.success(&format!(
            "Successfully pushed {}/{} from tar file",
            repository, reference
        ));
        Ok(())
    }

    // === 共享的辅助方法 - 最大化代码复用 ===

    fn require_client<'a>(&self, client: Option<&'a RegistryClient>) -> Result<&'a RegistryClient> {
        client.ok_or_else(|| {
            RegistryError::Validation("Registry client required for this operation".to_string())
        })
    }

    fn validate_tar_file(&self, tar_path: &Path) -> Result<()> {
        if !tar_path.exists() {
            return Err(RegistryError::Validation(format!(
                "Tar file '{}' does not exist",
                tar_path.display()
            )));
        }
        TarUtils::validate_tar_archive(tar_path)
    }

    #[allow(dead_code)]
    fn parse_manifest(&self, manifest_data: &[u8]) -> Result<serde_json::Value> {
        serde_json::from_slice(manifest_data)
            .map_err(|e| RegistryError::Parse(format!("Failed to parse manifest: {}", e)))
    }

    #[allow(dead_code)]
    fn extract_config_digest(&self, manifest: &serde_json::Value) -> Result<String> {
        manifest
            .get("config")
            .and_then(|c| c.get("digest"))
            .and_then(|d| d.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| RegistryError::Parse("Missing config digest in manifest".to_string()))
    }

    async fn pull_and_cache_blob(
        &mut self,
        client: &RegistryClient,
        repository: &str,
        digest: &str,
        token: &Option<String>,
        is_config: bool,
    ) -> Result<()> {
        if self.cache.has_blob(digest) {
            self.output
                .detail(&format!("Blob {} already in cache", &digest[..16]));
            return Ok(());
        }

        let blob_data = client.pull_blob(repository, digest, token).await?;
        self.cache
            .save_blob(digest, &blob_data, is_config, !is_config)?;
        Ok(())
    }

    async fn associate_blob_with_image(
        &mut self,
        repository: &str,
        reference: &str,
        digest: &str,
        is_config: bool,
    ) -> Result<()> {
        let size = self.cache.get_blob_size(digest).unwrap_or(0);
        self.cache
            .associate_blob_with_image(repository, reference, digest, size, is_config, !is_config)
    }

    #[allow(dead_code)]
    async fn pull_and_cache_layers(
        &mut self,
        client: &RegistryClient,
        repository: &str,
        reference: &str,
        manifest: &serde_json::Value,
        token: &Option<String>,
    ) -> Result<()> {
        if let Some(layers) = manifest.get("layers").and_then(|l| l.as_array()) {
            self.output
                .step(&format!("Pulling {} layer blobs", layers.len()));

            for (i, layer) in layers.iter().enumerate() {
                let layer_digest =
                    layer
                        .get("digest")
                        .and_then(|d| d.as_str())
                        .ok_or_else(|| {
                            RegistryError::Parse(format!("Missing digest for layer {}", i))
                        })?;

                self.output.detail(&format!(
                    "Layer {}/{}: {}",
                    i + 1,
                    layers.len(),
                    &layer_digest[..16]
                ));

                self.pull_and_cache_blob(client, repository, layer_digest, token, false)
                    .await?;
                self.associate_blob_with_image(repository, reference, layer_digest, false)
                    .await?;
            }
        }
        Ok(())
    }

    fn validate_cache_completeness(&self, repository: &str, reference: &str) -> Result<()> {
        if !self.cache.is_image_complete(repository, reference)? {
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

    async fn push_blobs_to_registry(
        &self,
        client: &RegistryClient,
        repository: &str,
        blobs: &[crate::image::cache::BlobInfo],
        token: &Option<String>,
    ) -> Result<()> {
        self.output.step(&format!("Pushing {} blobs", blobs.len()));

        for blob in blobs {
            let blob_data = self.cache.get_blob(&blob.digest)?;
            let _ = client
                .upload_blob_with_token(&blob_data, &blob.digest, repository, token)
                .await?;
        }
        Ok(())
    }

    async fn push_manifest_to_registry(
        &self,
        client: &RegistryClient,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.output.step("Pushing manifest");
        let manifest_data = self.cache.get_manifest(repository, reference)?;
        let manifest_str = String::from_utf8(manifest_data)?;
        client
            .upload_manifest_with_token(&manifest_str, repository, reference, token)
            .await
    }

    async fn push_config_from_tar(
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

    async fn push_layers_from_tar(
        &self,
        client: &RegistryClient,
        tar_path: &Path,
        layers: &[crate::image::parser::LayerInfo],
        repository: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.output
            .step(&format!("Pushing {} layer blobs", layers.len()));

        for (i, layer) in layers.iter().enumerate() {
            self.output.detail(&format!(
                "Layer {}/{}: {}",
                i + 1,
                layers.len(),
                &layer.digest[..16]
            ));

            // 检查blob是否已存在
            if client.check_blob_exists(&layer.digest, repository).await? {
                self.output.detail("Layer already exists, skipping");
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

    /// 统一的manifest创建方法
    fn create_manifest_from_image_info(&self, image_info: &ImageInfo) -> Result<String> {
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

    // === 公共查询方法 ===

    /// 获取缓存统计信息
    pub fn get_cache_stats(&self) -> Result<crate::image::cache::CacheStats> {
        self.cache.get_stats()
    }

    /// 列出缓存中的所有镜像
    pub fn list_cached_images(&self) -> Vec<(String, String)> {
        self.cache.list_manifests()
    }

    /// 检查镜像是否在缓存中
    pub fn is_image_cached(&self, repository: &str, reference: &str) -> Result<bool> {
        self.cache.is_image_complete(repository, reference)
    }

    /// 配置流式处理管道参数
    pub fn configure_pipeline(&mut self, config: PipelineConfig) {
        self.pipeline_config = config;
    }

    /// 设置是否使用优化的上传模式
    pub fn set_optimized_upload(&mut self, enabled: bool) {
        self.use_optimized_upload = enabled;
    }

    /// 获取当前配置状态
    pub fn get_config(&self) -> (bool, &PipelineConfig) {
        (self.use_optimized_upload, &self.pipeline_config)
    }

    /// Handle OCI index or Docker manifest list
    async fn handle_index_manifest(
        &mut self,
        client: &RegistryClient,
        repository: &str,
        reference: &str,
        parsed_manifest: &ParsedManifest,
        token: &Option<String>,
    ) -> Result<()> {
        self.output.info("Processing multi-platform manifest index");

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

        self.output.detail(&format!(
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
            self.cache.save_manifest(
                repository,
                reference,
                &parsed_manifest.raw_data,
                config_digest,
            )?;
        }

        // Process the platform-specific manifest
        self.handle_single_manifest(client, repository, reference, &platform_parsed, token)
            .await?;

        Ok(())
    }

    /// Handle single-platform manifest (Docker V2 or OCI) - now using unified pipeline
    async fn handle_single_manifest(
        &mut self,
        client: &RegistryClient,
        repository: &str,
        reference: &str,
        parsed_manifest: &ParsedManifest,
        token: &Option<String>,
    ) -> Result<()> {
        let config_digest = parsed_manifest.config_digest.as_ref().ok_or_else(|| {
            RegistryError::Parse("Missing config digest in single manifest".to_string())
        })?;

        self.output.detail(&format!(
            "Processing single-platform manifest with config {}",
            &config_digest[..16]
        ));

        // Save manifest to cache if not already done
        self.cache.save_manifest(
            repository,
            reference,
            &parsed_manifest.raw_data,
            config_digest,
        )?;

        // Pull and cache config blob directly (small, no need for pipeline)
        self.pull_and_cache_blob(client, repository, config_digest, token, true)
            .await?;
        self.associate_blob_with_image(repository, reference, config_digest, true)
            .await?;

        // Convert layer digests to LayerInfo for unified pipeline
        let layers: Vec<LayerInfo> = parsed_manifest
            .layer_digests
            .iter()
            .enumerate()
            .map(|(index, digest)| {
                LayerInfo {
                    digest: digest.clone(),
                    size: 0, // Size will be determined during download or estimated
                    tar_path: format!("layer_{}.tar", index), // Placeholder
                    // Default fields for download operations
                    media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                    compressed_size: Some(0),
                    offset: None,
                }
            })
            .collect();

        if !layers.is_empty() {
            // Use unified pipeline for batch downloading layers
            let pipeline =
                UnifiedPipeline::new(self.output.clone()).with_config(self.pipeline_config.clone());

            pipeline
                .process_downloads(
                    &layers,
                    repository,
                    token,
                    std::sync::Arc::new(client.clone()),
                    &mut self.cache,
                )
                .await?;

            // Associate downloaded blobs with image
            for layer in &layers {
                self.associate_blob_with_image(repository, reference, &layer.digest, false)
                    .await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_manager_creation() {
        let manager = ImageManager::new(None, false).unwrap();
        let (optimized, _config) = manager.get_config();
        assert!(optimized, "Should default to optimized mode");
    }

    #[test]
    fn test_image_manager_with_config() {
        let manager = ImageManager::with_config(None, false, false).unwrap();
        let (optimized, _config) = manager.get_config();
        assert!(!optimized, "Should respect provided optimization setting");
    }

    #[test]
    fn test_optimization_toggle() {
        let mut manager = ImageManager::new(None, false).unwrap();

        // Default is optimized
        let (optimized, _) = manager.get_config();
        assert!(optimized);

        // Disable optimization
        manager.set_optimized_upload(false);
        let (optimized, _) = manager.get_config();
        assert!(!optimized);

        // Re-enable optimization
        manager.set_optimized_upload(true);
        let (optimized, _) = manager.get_config();
        assert!(optimized);
    }
}
