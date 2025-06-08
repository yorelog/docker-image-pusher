//! 综合镜像管理器 - 统一处理4种操作模式
//!
//! 提供统一的接口来处理所有4种操作模式，最大化代码复用

use crate::cli::operation_mode::OperationMode;
use crate::error::{RegistryError, Result};
use crate::image::cache::Cache;
use crate::image::manifest::{ManifestType, ParsedManifest, parse_manifest_with_type};
use crate::image::parser::ImageInfo;
use crate::image::{BlobHandler, CacheManager, ManifestHandler, TarHandler};
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
    concurrency_config: Option<crate::concurrency::ConcurrencyConfig>,
    // Specialized handlers for modular operations
    manifest_handler: ManifestHandler,
    blob_handler: BlobHandler,
    tar_handler: TarHandler,
    cache_manager: CacheManager,
}

impl ImageManager {
    /// 创建新的镜像管理器
    pub fn new(cache_dir: Option<&str>, verbose: bool) -> Result<Self> {
        let cache = Cache::new(cache_dir)?;
        let cache2 = Cache::new(cache_dir)?; // Create a second cache instance for the manager
        let output = Logger::new(verbose);
        let pipeline_config = PipelineConfig::default();

        // Initialize specialized handlers with correct constructors
        let manifest_handler = ManifestHandler::new(output.clone(), pipeline_config.clone());
        let blob_handler = BlobHandler::new(output.clone());
        let tar_handler = TarHandler::new(output.clone(), pipeline_config.clone(), true);
        let cache_manager = CacheManager::new(cache2, output.clone());

        Ok(Self {
            cache,
            output,
            pipeline_config,
            use_optimized_upload: true, // Default to optimized mode
            concurrency_config: None,
            manifest_handler,
            blob_handler,
            tar_handler,
            cache_manager,
        })
    }

    /// 创建镜像管理器，并允许配置优化选项
    pub fn with_config(
        cache_dir: Option<&str>,
        verbose: bool,
        use_optimized_upload: bool,
    ) -> Result<Self> {
        let cache = Cache::new(cache_dir)?;
        let cache2 = Cache::new(cache_dir)?; // Create a second cache instance for the manager
        let output = Logger::new(verbose);
        let pipeline_config = PipelineConfig::default();

        // Initialize specialized handlers with correct constructors
        let manifest_handler = ManifestHandler::new(output.clone(), pipeline_config.clone());
        let blob_handler = BlobHandler::new(output.clone());
        let tar_handler = TarHandler::new(output.clone(), pipeline_config.clone(), use_optimized_upload);
        let cache_manager = CacheManager::new(cache2, output.clone());

        Ok(Self {
            cache,
            output,
            pipeline_config,
            use_optimized_upload,
            concurrency_config: None,
            manifest_handler,
            blob_handler,
            tar_handler,
            cache_manager,
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
                self.pull_and_cache_image(client, repository, reference, auth_token)
                    .await
            }
            OperationMode::ExtractAndCache {
                tar_file,
                repository,
                reference,
            } => {
                self.extract_and_cache_from_tar(tar_file, repository, reference)
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
                self.push_from_cache(client, repository, reference, auth_token)
                    .await
            }
            OperationMode::PushFromTar {
                tar_file,
                repository,
                reference,
            } => {
                if self.use_optimized_upload {
                    self.push_from_tar_optimized(
                        client, tar_file, repository, reference, auth_token,
                    )
                    .await
                } else {
                    self.push_from_tar(client, tar_file, repository, reference, auth_token)
                        .await
                }
            }
        }
    }

    /// 执行推送操作（支持源和目标坐标分离）
    pub async fn execute_push_from_cache_with_source(
        &mut self,
        source_repository: &str,
        source_reference: &str,
        target_repository: &str,
        target_reference: &str,
        client: Option<&RegistryClient>,
        auth_token: Option<&str>,
    ) -> Result<()> {
        self.push_from_cache_with_source(
            client, 
            source_repository, 
            source_reference, 
            target_repository, 
            target_reference, 
            auth_token
        ).await
    }

    // === 4种核心操作模式实现 ===

    /// Pull image from registry and cache locally
    async fn pull_and_cache_image(
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
            .success(&format!("Successfully cached {}:{}", repository, reference));
        Ok(())
    }

    /// Extract from tar file and cache locally
    async fn extract_and_cache_from_tar(
        &mut self,
        tar_file: &str,
        repository: &str,
        reference: &str,
    ) -> Result<()> {
        // Delegate to cache manager
        self.cache_manager.extract_and_cache_from_tar(
            tar_file, repository, reference
        ).await
    }

    /// Push from cache to registry
    async fn push_from_cache(
        &mut self,
        client: Option<&RegistryClient>,
        repository: &str,
        reference: &str,
        token: Option<&str>,
    ) -> Result<()> {
        self.push_from_cache_with_source(client, repository, reference, repository, reference, token).await
    }

    /// Push from cache to registry with separate source and target coordinates
    async fn push_from_cache_with_source(
        &mut self,
        client: Option<&RegistryClient>,
        source_repository: &str,
        source_reference: &str,
        target_repository: &str,
        target_reference: &str,
        token: Option<&str>,
    ) -> Result<()> {
        let client = self.require_client(client)?;
        let token = token.map(|s| s.to_string());

        self.output.info(&format!(
            "Pushing {}/{} from cache to registry as {}/{}",
            source_repository, source_reference, target_repository, target_reference
        ));

        // 验证缓存完整性 - 使用源镜像坐标
        self.validate_cache_completeness(source_repository, source_reference)?;

        // 推送所有blobs - 使用源镜像坐标获取blobs
        let blobs = self.cache.get_image_blobs(source_repository, source_reference)?;
        self.push_blobs_to_registry(client, target_repository, &blobs, &token)
            .await?;

        // 推送manifest - 使用源镜像坐标获取manifest，但推送到目标坐标
        self.push_manifest_to_registry_with_source(client, source_repository, source_reference, target_repository, target_reference, &token)
            .await?;

        self.output.success(&format!(
            "Successfully pushed {}/{} from cache to {}/{}",
            source_repository, source_reference, target_repository, target_reference
        ));
        Ok(())
    }

    /// Push from tar file using optimized unified pipeline
    async fn push_from_tar_optimized(
        &mut self,
        client: Option<&RegistryClient>,
        tar_file: &str,
        repository: &str,
        reference: &str,
        token: Option<&str>,
    ) -> Result<()> {
        let client = self.require_client(client)?;
        let token = token.map(|s| s.to_string());

        // Delegate to tar handler
        self.tar_handler.push_from_tar_optimized(
            client, tar_file, repository, reference, &token
        ).await
    }

    /// Push directly from tar file (without caching)
    async fn push_from_tar(
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
        // 使用增强的缓存检查，避免大文件的昂贵SHA256验证
        let verify_integrity = is_config || self.cache.get_blob_size(digest).map_or(true, |size| size <= 10 * 1024 * 1024);
        
        if self.cache.has_blob_with_verification(digest, verify_integrity) {
            self.output
                .detail(&format!("Blob {} already in cache (verified)", &digest[..16]));
            return Ok(());
        }

        self.output
            .detail(&format!("Downloading blob {}", &digest[..16]));

        let blob_data = client.pull_blob(repository, digest, token).await?;
        
        // 使用增强的blob缓存方法，支持智能验证策略
        self.cache
            .add_blob_with_verification(digest, &blob_data, is_config, !is_config, false)
            .await?;
            
        self.output
            .detail(&format!("Cached blob {} ({} bytes) with verification", &digest[..16], blob_data.len()));
            
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
        // Delegate to blob handler
        self.blob_handler.push_blobs_to_registry(
            client, repository, blobs, token, &self.cache
        ).await
    }


    async fn push_manifest_to_registry_with_source(
        &self,
        client: &RegistryClient,
        source_repository: &str,
        source_reference: &str,
        target_repository: &str,
        target_reference: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.output.step("Pushing manifest");
        // Get manifest from source coordinates in cache
        let manifest_data = self.cache.get_manifest(source_repository, source_reference)?;
        let manifest_str = String::from_utf8(manifest_data)?;
        // Push manifest to target coordinates in registry
        client
            .upload_manifest_with_token(&manifest_str, target_repository, target_reference, token)
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

    /// Configure concurrency management through the new concurrency module
    /// 
    /// This method integrates with the unified concurrency management system,
    /// replacing the old pipeline-specific concurrency configuration.
    pub fn configure_concurrency(&mut self, config: crate::concurrency::ConcurrencyConfig) {
        // Store the concurrency config for use with the concurrency manager
        // The actual concurrency control is now handled by the dedicated concurrency module
        // which provides advanced features like dynamic adjustment, performance monitoring,
        // and intelligent strategy selection.
        self.concurrency_config = Some(config);
        
        self.output.detail("Concurrency management configured using unified concurrency module");
    }

    /// Create appropriate concurrency manager based on configuration
    /// 
    /// Returns a concurrency manager instance based on the configured strategy.
    /// This provides a factory method for creating the right type of concurrency
    /// manager for the current configuration.
    pub fn create_concurrency_manager(&self) -> Result<Box<dyn crate::concurrency::ConcurrencyController>> {
        let config = self.concurrency_config.as_ref()
            .ok_or_else(|| crate::error::RegistryError::Validation(
                "No concurrency configuration available. Call configure_concurrency() first.".to_string()
            ))?;

        // Always use adaptive concurrency manager
        self.output.detail("Creating adaptive concurrency manager");
        Ok(Box::new(crate::concurrency::AdaptiveConcurrencyManager::new(config.clone())))
    }

    /// Get configured concurrency limits for pipeline operations
    /// 
    /// Returns the concurrency limits from the stored configuration,
    /// providing backward compatibility for pipeline operations that
    /// need basic concurrency information.
    pub fn get_concurrency_limits(&self) -> (usize, usize, usize) {
        if let Some(config) = &self.concurrency_config {
            (
                config.limits.max_concurrent,
                config.limits.small_file_concurrent,
                config.limits.large_file_concurrent,
            )
        } else {
            // Default values for backward compatibility
            (8, 12, 4)
        }
    }

    /// Configure simple concurrency (convenience method)
    /// 
    /// This is a convenience method for simple use cases that only need
    /// to set maximum concurrency without advanced features.
    pub fn configure_simple_concurrency(&mut self, max_concurrent: usize) {
        let config = crate::concurrency::ConcurrencyConfig::default()
            .with_max_concurrent(max_concurrent);
        self.configure_concurrency(config);
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
        // Delegate to manifest handler
        self.manifest_handler.handle_index_manifest(
            client, repository, reference, parsed_manifest, token, &mut self.cache
        ).await
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
        let layers: Vec<crate::image::parser::LayerInfo> = parsed_manifest
            .layer_digests
            .iter()
            .enumerate()
            .map(|(index, digest)| {
                crate::image::parser::LayerInfo {
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

    /// 获取Logger引用
    pub fn get_logger(&self) -> &Logger {
        &self.output
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
