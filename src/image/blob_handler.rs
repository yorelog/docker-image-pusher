//! Blob handling operations
//!
//! This module contains blob-specific functionality extracted from image_manager
//! to improve modularity and reduce file size.

use crate::common::Validatable;
use crate::error::{RegistryError, Result};
use crate::image::{cache::BlobInfo, digest::DigestUtils, Cache};
use crate::logging::Logger;
use crate::registry::{RegistryClient, PipelineConfig};
use std::sync::Arc;

/// Blob upload task with embedded data to avoid cache cloning
#[derive(Debug)]
struct BlobUploadTaskWithData {
    digest: String,
    size: u64,
    is_config: bool,
    priority: u64,
    data: Vec<u8>,
}

/// Blob handler for processing Docker/OCI blobs
pub struct BlobHandler {
    logger: Logger,
    pipeline_config: PipelineConfig,
}

impl BlobHandler {
    pub fn new(logger: Logger) -> Self {
        Self { 
            logger,
            pipeline_config: PipelineConfig::default(),
        }
    }

    pub fn with_config(logger: Logger, pipeline_config: PipelineConfig) -> Self {
        Self {
            logger,
            pipeline_config,
        }
    }

    /// Push all blobs to registry with enhanced verification using UnifiedPipeline
    pub async fn push_blobs_to_registry(
        &self,
        client: &RegistryClient,
        repository: &str,
        blobs: &[BlobInfo],
        token: &Option<String>,
        cache: &Cache,
    ) -> Result<()> {
        if blobs.is_empty() {
            return Ok(());
        }

        // Use enhanced blob upload process with pre-loaded data
        self.logger.step(&format!("Pushing {} blobs using enhanced concurrent upload", blobs.len()));

        // Display initialization  
        self.logger.info(&format!(
            "🚀 Initializing enhanced upload pipeline for {} blob uploads with improved concurrency...",
            blobs.len()
        ));

        // Validate all blobs in cache and prepare upload tasks with blob data
        let mut upload_tasks = Vec::new();
        for blob in blobs {
            // 先验证blob在缓存中的完整性
            if !cache.has_blob_with_verification(&blob.digest, blob.is_config) {
                return Err(RegistryError::Cache {
                    message: format!("Blob {} failed integrity verification in cache", &blob.digest[..16]),
                    path: Some(blob.path.clone()),
                });
            }

            // 读取blob数据进行验证
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
                "✅ [Unified Pipeline] Blob {} ({}) verified for upload",
                &blob.digest[..16],
                self.format_size(blob.size)
            ));

            // Use BlobHandler's own priority calculation method instead of UnifiedPipeline
            let priority = self.calculate_blob_priority(blob.size, blob.is_config);

            // Create upload task with blob data pre-loaded to avoid cache cloning
            upload_tasks.push(BlobUploadTaskWithData {
                digest: blob.digest.clone(),
                size: blob.size,
                is_config: blob.is_config,
                priority,
                data: blob_data,
            });
        }

        // Execute concurrent blob uploads using semaphore-based concurrency control
        self.logger.info(&format!(
            "Executing {} blob uploads with {} concurrent workers",
            upload_tasks.len(),
            self.pipeline_config.max_concurrent
        ));

        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(self.pipeline_config.max_concurrent));
        let client_arc = std::sync::Arc::new(client.clone());
        let logger_arc = std::sync::Arc::new(self.logger.clone());
        let repository_arc = std::sync::Arc::new(repository.to_string());
        let token_arc = std::sync::Arc::new(token.clone());
        
        let start_time = std::time::Instant::now();
        let total_size: u64 = upload_tasks.iter().map(|t| t.size).sum();

        // Sort tasks by priority (small blobs first, config blobs highest priority)
        let mut sorted_tasks = upload_tasks;
        sorted_tasks.sort_by_key(|task| task.priority);

        // Create concurrent upload futures
        let upload_futures: Vec<_> = sorted_tasks
            .into_iter()
            .enumerate()
            .map(|(index, task)| {
                let semaphore = std::sync::Arc::clone(&semaphore);
                let client = std::sync::Arc::clone(&client_arc);
                let logger = std::sync::Arc::clone(&logger_arc);
                let repository = std::sync::Arc::clone(&repository_arc);
                let token = std::sync::Arc::clone(&token_arc);

                tokio::spawn(async move {
                    Self::execute_blob_upload_with_data(
                        task, index, semaphore, client, logger, repository, token,
                    )
                    .await
                })
            })
            .collect();

        // Wait for all uploads to complete
        let results = futures::future::try_join_all(upload_futures)
            .await
            .map_err(|e| RegistryError::Upload(format!("Blob upload task failed: {}", e)))?;

        // Check for upload failures
        for result in results {
            if let Err(e) = result {
                return Err(e);
            }
        }

        let elapsed = start_time.elapsed();
        let avg_speed = if elapsed.as_secs() > 0 {
            total_size / elapsed.as_secs()
        } else {
            total_size
        };

        self.logger.success(&format!(
            "✅ Unified Pipeline blob upload completed successfully in {} (avg speed: {}/s)",
            self.logger.format_duration(elapsed),
            self.logger.format_size(avg_speed)
        ));
        
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

    /// Calculate blob priority for upload scheduling
    fn calculate_blob_priority(&self, size: u64, is_config: bool) -> u64 {
        // Define small blob threshold locally since it's not in the simplified PipelineConfig
        let small_blob_threshold = 10 * 1024 * 1024; // 10MB

        if is_config {
            // Config blobs get highest priority (lowest number)
            0
        } else if size > self.pipeline_config.large_layer_threshold {
            // Large blobs get highest priority (lowest numbers) - big blobs first
            1
        } else if size > small_blob_threshold {
            // Medium blobs get medium priority - use size-based ordering
            // Larger medium blobs get lower priority numbers (higher priority)
            2 + (self.pipeline_config.large_layer_threshold - size) / 1024
        } else {
            // Small blobs get lowest priority (highest numbers)
            // Smaller blobs get higher priority numbers (lower priority)
            1000 + (small_blob_threshold - size) / 1024
        }
    }

    /// Execute a single blob upload with pre-loaded data (avoids cache cloning)
    async fn execute_blob_upload_with_data(
        task: BlobUploadTaskWithData,
        index: usize,
        semaphore: Arc<tokio::sync::Semaphore>,
        client: Arc<RegistryClient>,
        logger: Arc<Logger>,
        repository: Arc<String>,
        token: Arc<Option<String>>,
    ) -> Result<()> {
        // Acquire semaphore permit for concurrency control
        let _permit = semaphore.acquire().await
            .map_err(|e| RegistryError::Upload(format!("Failed to acquire semaphore: {}", e)))?;

        let start_time = std::time::Instant::now();
        
        logger.detail(&format!(
            "Upload task {}: Processing {} blob {} ({}) - priority {}",
            index + 1,
            if task.is_config { "config" } else { "layer" },
            &task.digest[..16],
            crate::common::FormatUtils::format_bytes(task.size),
            task.priority
        ));

        // Upload blob using the unified token-aware method with pre-loaded data
        client
            .upload_blob_with_token(&task.data, &task.digest, repository.as_ref(), token.as_ref())
            .await?;

        let elapsed = start_time.elapsed();
        let speed = if elapsed.as_secs() > 0 {
            task.size / elapsed.as_secs()
        } else {
            task.size
        };

        logger.success(&format!(
            "✅ Blob {} uploaded in {} ({}/s)",
            &task.digest[..16],
            logger.format_duration(elapsed),
            logger.format_size(speed)
        ));

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
