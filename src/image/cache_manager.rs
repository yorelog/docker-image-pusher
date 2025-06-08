//! Cache management operations
//!
//! This module contains cache-specific functionality extracted from image_manager
//! to improve modularity and reduce file size.

use crate::common::{Configurable, Cacheable};
use crate::error::{RegistryError, Result};
use crate::image::cache::{Cache, CacheStats, BlobInfo};
use crate::logging::Logger;
use crate::registry::RegistryClient;
use async_trait::async_trait;
use std::path::Path;

/// Cache manager configuration
#[derive(Debug, Clone)]
pub struct CacheManagerConfig {
    pub cache_dir: Option<String>,
    pub verify_integrity: bool,
    pub enable_compression: bool,
    pub max_cache_size: Option<u64>,
}

impl Default for CacheManagerConfig {
    fn default() -> Self {
        Self {
            cache_dir: None,
            verify_integrity: true,
            enable_compression: false,
            max_cache_size: None,
        }
    }
}

/// Cache manager for handling Docker/OCI image caching operations
pub struct CacheManager {
    cache: Cache,
    logger: Logger,
    config: CacheManagerConfig,
}

impl CacheManager {
    pub fn new(cache: Cache, logger: Logger) -> Self {
        Self {
            cache,
            logger,
            config: CacheManagerConfig::default(),
        }
    }

    pub fn with_config(cache: Cache, logger: Logger, config: CacheManagerConfig) -> Self {
        Self {
            cache,
            logger,
            config,
        }
    }

    /// Extract from tar file and cache locally
    pub async fn extract_and_cache_from_tar(
        &mut self,
        tar_file: &str,
        repository: &str,
        reference: &str,
    ) -> Result<()> {
        let tar_path = Path::new(tar_file);
        self.validate_tar_file(tar_path)?;

        self.logger.info(&format!(
            "Extracting {} to cache as {}/{}",
            tar_file, repository, reference
        ));

        // 使用统一的tar解析和缓存逻辑
        self.cache.cache_from_tar(tar_path, repository, reference)?;

        self.logger.success(&format!(
            "Successfully extracted and cached {}/{}",
            repository, reference
        ));
        Ok(())
    }

    /// Push from cache to registry with separate source and target coordinates
    pub async fn push_from_cache_with_source(
        &mut self,
        client: &RegistryClient,
        source_repository: &str,
        source_reference: &str,
        target_repository: &str,
        target_reference: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.logger.info(&format!(
            "Pushing {}/{} from cache to registry as {}/{}",
            source_repository, source_reference, target_repository, target_reference
        ));

        // 验证缓存完整性 - 使用源镜像坐标
        self.validate_cache_completeness(source_repository, source_reference)?;

        // 推送所有blobs - 使用源镜像坐标获取blobs
        let blobs = self.cache.get_image_blobs(source_repository, source_reference)?;
        self.push_blobs_to_registry(client, target_repository, &blobs, token).await?;

        // 推送manifest - 使用源镜像坐标获取manifest，但推送到目标坐标
        self.push_manifest_to_registry_with_source(
            client, 
            source_repository, 
            source_reference, 
            target_repository, 
            target_reference, 
            token
        ).await?;

        self.logger.success(&format!(
            "Successfully pushed {}/{} from cache to {}/{}",
            source_repository, source_reference, target_repository, target_reference
        ));
        Ok(())
    }

    /// Push from cache to registry (same coordinates)
    pub async fn push_from_cache(
        &mut self,
        client: &RegistryClient,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.push_from_cache_with_source(
            client, 
            repository, 
            reference, 
            repository, 
            reference, 
            token
        ).await
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> Result<CacheStats> {
        self.cache.get_stats()
    }

    /// List all cached images
    pub fn list_cached_images(&self) -> Vec<(String, String)> {
        self.cache.list_manifests()
    }

    /// Check if image is cached
    pub fn is_image_cached(&self, repository: &str, reference: &str) -> Result<bool> {
        self.cache.is_image_complete(repository, reference)
    }

    /// Validate cache completeness for an image
    pub fn validate_cache_completeness(&self, repository: &str, reference: &str) -> Result<()> {
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

    /// Get image blobs from cache
    pub fn get_image_blobs(&self, repository: &str, reference: &str) -> Result<Vec<BlobInfo>> {
        self.cache.get_image_blobs(repository, reference)
    }

    /// Get cache reference (for external operations)
    pub fn get_cache(&self) -> &Cache {
        &self.cache
    }

    /// Get mutable cache reference (for external operations)
    pub fn get_cache_mut(&mut self) -> &mut Cache {
        &mut self.cache
    }

    // Helper methods
    fn validate_tar_file(&self, tar_path: &Path) -> Result<()> {
        if !tar_path.exists() {
            return Err(RegistryError::Validation(format!(
                "Tar file '{}' does not exist",
                tar_path.display()
            )));
        }
        crate::registry::tar_utils::TarUtils::validate_tar_archive(tar_path)
    }

    async fn push_blobs_to_registry(
        &self,
        client: &RegistryClient,
        repository: &str,
        blobs: &[BlobInfo],
        token: &Option<String>,
    ) -> Result<()> {
        self.logger.step(&format!("Pushing {} blobs with enhanced progress tracking", blobs.len()));

        for blob in blobs {
            // 先验证blob在缓存中的完整性
            if !self.cache.has_blob_with_verification(&blob.digest, blob.is_config) {
                return Err(RegistryError::Cache {
                    message: format!("Blob {} failed integrity verification in cache", &blob.digest[..16]),
                    path: Some(blob.path.clone()),
                });
            }

            // 读取blob数据
            let blob_data = self.cache.get_blob(&blob.digest)?;

            self.logger.verbose(&format!(
                "Uploading blob {} ({})",
                &blob.digest[..16],
                crate::common::FormatUtils::format_bytes(blob.size)
            ));

            let _ = client
                .upload_blob_with_token(&blob_data, &blob.digest, repository, token)
                .await?;
        }
        Ok(())
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
        self.logger.step("Pushing manifest");
        // Get manifest from source coordinates in cache
        let manifest_data = self.cache.get_manifest(source_repository, source_reference)?;
        let manifest_str = String::from_utf8(manifest_data)?;
        // Push manifest to target coordinates in registry
        client
            .upload_manifest_with_token(&manifest_str, target_repository, target_reference, token)
            .await
    }
}

#[async_trait]
impl Configurable<CacheManagerConfig> for CacheManager {
    fn configure(&mut self, config: CacheManagerConfig) -> Result<()> {
        self.config = config;
        self.logger.detail("Cache manager configuration updated");
        Ok(())
    }

    fn get_config(&self) -> &CacheManagerConfig {
        &self.config
    }

    fn validate_config(config: &CacheManagerConfig) -> Result<()> {
        if let Some(cache_dir) = &config.cache_dir {
            let path = Path::new(cache_dir);
            if !path.exists() {
                return Err(RegistryError::Validation(format!(
                    "Cache directory does not exist: {}",
                    cache_dir
                )));
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Cacheable for CacheManager {
    type Key = (String, String); // (repository, reference)
    type Value = bool; // cached status

    async fn get(&self, key: &Self::Key) -> Result<Option<Self::Value>> {
        let (repository, reference) = key;
        match self.is_image_cached(repository, reference) {
            Ok(cached) => Ok(Some(cached)),
            Err(_) => Ok(None),
        }
    }

    async fn put(&self, _key: Self::Key, _value: Self::Value) -> Result<()> {
        // Cache operations are handled through other methods
        Ok(())
    }

    async fn exists(&self, key: &Self::Key) -> bool {
        let (repository, reference) = key;
        self.is_image_cached(repository, reference).unwrap_or(false)
    }

    async fn remove(&self, _key: &Self::Key) -> Result<()> {
        // Not implemented for now
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        // Not implemented for now
        Ok(())
    }
}
