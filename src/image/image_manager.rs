//! 综合镜像管理器 - 统一处理4种操作模式
//!
//! 提供统一的接口来处理所有4种操作模式，最大化代码复用

use crate::cli::operation_mode::OperationMode;
use crate::error::{RegistryError, Result};
use crate::image::cache::Cache;
use crate::image::manifest::{ManifestType, ParsedManifest, parse_manifest_with_type};
use crate::image::{BlobHandler, CacheManager, ManifestHandler};
use crate::logging::Logger;
use crate::registry::RegistryClient;
use crate::registry::{PipelineConfig, UnifiedPipeline};

/// 综合镜像管理器 - 4种操作模式的统一入口
pub struct ImageManager {
    cache: Cache,
    output: Logger,
    pipeline_config: PipelineConfig,
    use_optimized_upload: bool,
    // Specialized handlers for modular operations
    manifest_handler: ManifestHandler,
    blob_handler: BlobHandler,
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
        let blob_handler = BlobHandler::with_config(output.clone(), pipeline_config.clone());
        let cache_manager = CacheManager::new(cache2, output.clone());

        Ok(Self {
            cache,
            output,
            pipeline_config,
            use_optimized_upload: true, // Default to optimized mode
            manifest_handler,
            blob_handler,
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
        let blob_handler = BlobHandler::with_config(output.clone(), pipeline_config.clone());
        let cache_manager = CacheManager::new(cache2, output.clone());

        Ok(Self {
            cache,
            output,
            pipeline_config,
            use_optimized_upload,
            manifest_handler,
            blob_handler,
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
        // Apply the same repository name normalization that was used during caching
        // to ensure we look up the image with the correct cache key
        let normalized_repository = self.normalize_repository_name(repository);
        
        self.push_from_cache_with_source(
            client, 
            &normalized_repository, 
            reference, 
            repository, 
            reference, 
            token
        ).await
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
        
        // 显示缓存详细信息用于调试
        if let Ok(cache_details) = self.cache.get_image_cache_details(source_repository, source_reference) {
            self.output.detail(&format!("📋 Cache details for {}/{}:\n{}", 
                source_repository, source_reference, cache_details));
        }

        // 推送所有blobs - 使用源镜像坐标获取blobs
        let blobs = self.cache.get_image_blobs(source_repository, source_reference)?;
        
        // 验证每个blob在本地缓存中的真实性和完整性
        self.output.info(&format!("🔍 Verifying {} blobs in local cache before upload...", blobs.len()));
        for (i, blob) in blobs.iter().enumerate() {
            self.output.detail(&format!("Verifying blob {}/{}: {}", i + 1, blobs.len(), &blob.digest[..16]));
            
            let (is_valid, report) = self.cache.verify_blob_exists_with_details(&blob.digest, Some(&self.output))?;
            if !is_valid {
                return Err(RegistryError::Cache {
                    message: format!(
                        "Blob {} failed local cache verification before upload. Report:\n{}",
                        &blob.digest[..16], report
                    ),
                    path: Some(blob.path.clone()),
                });
            }
            self.output.verbose(&format!("✅ Local cache verification passed for {}", &blob.digest[..16]));
        }
        self.output.success("✅ All blobs verified in local cache");
        
        self.push_blobs_to_registry(client, target_repository, &blobs, &token)
            .await?;

        // 验证所有blob都已成功上传到registry
        self.output.info("🔍 Verifying all blobs are present in registry before uploading manifest...");
        
        // Add a longer delay to account for registry consistency (阿里云Registry需要更多时间处理)
        tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;
        
        for blob in &blobs {
            // Enhanced retry mechanism with exponential backoff for blob existence check
            let mut retries = 8; // Increase retry count
            let mut exists = false;
            let base_delay = 1000; // Start with 1 second
            
            while retries > 0 && !exists {
                exists = client.check_blob_exists_with_token(&blob.digest, target_repository, &token).await?;
                if !exists {
                    retries -= 1;
                    if retries > 0 {
                        // Exponential backoff: 1s, 2s, 4s, 8s, 16s, etc.
                        let delay = base_delay * (2_u64.pow(8 - retries as u32 - 1));
                        self.output.verbose(&format!(
                            "Blob {} not yet available in registry, retrying in {}s... ({} attempts left)",
                            &blob.digest[..16], delay / 1000, retries
                        ));
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    }
                } else {
                    self.output.verbose(&format!(
                        "✅ Blob {} verified in registry after upload",
                        &blob.digest[..16]
                    ));
                    break;
                }
            }
            
            if !exists {
                // Try one final verification with a longer timeout
                self.output.warning(&format!(
                    "Final verification attempt for blob {} after extended wait...",
                    &blob.digest[..16]
                ));
                tokio::time::sleep(tokio::time::Duration::from_millis(10000)).await; // Wait 10 seconds
                exists = client.check_blob_exists_with_token(&blob.digest, target_repository, &token).await?;
                
                if !exists {
                    return Err(RegistryError::Upload(format!(
                        "Blob {} not found in remote registry after upload and extended verification (cached locally but registry verification failed) - this may indicate a registry consistency issue or network problem",
                        &blob.digest[..16]
                    )));
                } else {
                    self.output.success(&format!(
                        "✅ Blob {} verified in registry after extended wait",
                        &blob.digest[..16]
                    ));
                }
            }
        }
        self.output.success("✅ All blobs verified present in registry");

        // 推送manifest - 使用源镜像坐标获取manifest，但推送到目标坐标
        self.push_manifest_to_registry_with_source(client, source_repository, source_reference, target_repository, target_reference, &token)
            .await?;

        self.output.success(&format!(
            "Successfully pushed {}/{} from cache to {}/{}",
            source_repository, source_reference, target_repository, target_reference
        ));
        Ok(())
    }

    // === 共享的辅助方法 - 最大化代码复用 ===

    fn require_client<'a>(&self, client: Option<&'a RegistryClient>) -> Result<&'a RegistryClient> {
        client.ok_or_else(|| {
            RegistryError::Validation("Registry client required for this operation".to_string())
        })
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

        // Convert layer information to LayerInfo for unified pipeline
        let layers: Vec<crate::image::parser::LayerInfo> = parsed_manifest
            .layer_info
            .iter()
            .enumerate()
            .map(|(index, (digest, size))| {
                crate::image::parser::LayerInfo {
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

    /// Normalize repository name for cache lookup
    /// This applies the same logic used during pull operations to ensure
    /// that cache lookups use the correct normalized repository name
    fn normalize_repository_name(&self, repository: &str) -> String {
        // Check if this looks like a Docker Hub single-name repository
        // (no registry prefix, no namespace)
        let parts: Vec<&str> = repository.split('/').collect();
        
        match parts.len() {
            1 => {
                // Single name like "nginx" -> "library/nginx" for Docker Hub
                format!("library/{}", repository)
            }
            _ => {
                // Already has namespace or registry, use as-is
                repository.to_string()
            }
        }
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
