//! Unified Registry Operations
//! 
//! This module consolidates upload and download operations into a single API
//! that uses the existing proven logic from RegistryClient and ImageManager,
//! eliminating code duplication and providing consistent concurrency support.

use crate::error::{RegistryError, Result};
use crate::logging::Logger;
use crate::registry::client::RegistryClient;
use crate::registry::pipeline::UnifiedPipeline;
use std::sync::Arc;

/// Unified registry operations that support both upload and download
/// with consistent concurrency, progress tracking, and error handling
pub struct UnifiedRegistryOperations {
    client: RegistryClient,
    pipeline: Arc<UnifiedPipeline>,
    logger: Logger,
}

/// Transfer operation metadata
#[derive(Debug, Clone)]
pub struct TransferOperation {
    pub operation_id: String,
    pub operation_type: TransferType,
    pub repository: String,
    pub digest_or_reference: String,
    pub size_hint: Option<u64>,
    pub priority: u32,
}

/// Type of transfer operation
#[derive(Debug, Clone, PartialEq)]
pub enum TransferType {
    BlobUpload,
    BlobDownload,
    ManifestUpload,
    ManifestDownload,
}

/// Transfer result with metadata
#[derive(Debug)]
pub struct TransferResult {
    pub operation_id: String,
    pub success: bool,
    pub bytes_transferred: u64,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// Batch transfer request
#[derive(Debug)]
pub struct BatchTransferRequest {
    pub operations: Vec<TransferOperation>,
    pub registry_url: String,
    pub token: Option<String>,
    pub max_concurrent: Option<usize>,
}

impl UnifiedRegistryOperations {
    pub fn new(
        client: RegistryClient,
        pipeline: Arc<UnifiedPipeline>,
        logger: Logger,
    ) -> Self {
        Self {
            client,
            pipeline,
            logger,
            concurrency: None,
        }
    }
    
    pub fn with_concurrency(mut self, concurrency: Arc<dyn ConcurrencyController>) -> Self {
        self.concurrency = Some(concurrency);
        self
    }
    
    /// Upload a single blob using existing proven logic
    pub async fn upload_blob(
        &self,
        data: &[u8],
        digest: &str,
        repository: &str,
        token: Option<&str>,
    ) -> Result<String> {
        let start_time = std::time::Instant::now();
        
        self.logger.info(&format!(
            "🚀 Starting blob upload: {} ({}) to {}",
            &digest[..16],
            self.logger.format_size(data.len() as u64),
            repository
        ));
        
        // Use existing proven upload logic from RegistryClient
        let result = self.client.upload_blob_with_token(
            data,
            digest,
            repository,
            &token.map(|s| s.to_string()),
        ).await;
        
        let duration = start_time.elapsed();
        
        match &result {
            Ok(digest) => {
                self.logger.success(&format!(
                    "✅ Blob upload completed: {} in {:.2}s ({}/s)",
                    &digest[..16],
                    duration.as_secs_f64(),
                    self.logger.format_size((data.len() as f64 / duration.as_secs_f64()) as u64)
                ));
            }
            Err(e) => {
                self.logger.error(&format!(
                    "❌ Blob upload failed: {} - {}",
                    &digest[..16],
                    e
                ));
            }
        }
        
        result
    }
    
    /// Download a single blob using existing proven logic
    pub async fn download_blob(
        &self,
        digest: &str,
        repository: &str,
        token: Option<&str>,
    ) -> Result<Vec<u8>> {
        let start_time = std::time::Instant::now();
        
        self.logger.info(&format!(
            "⬇️ Starting blob download: {} from {}",
            &digest[..16],
            repository
        ));
        
        // Use existing proven download logic from RegistryClient
        let result = self.client.pull_blob(
            repository,
            digest,
            &token.map(|s| s.to_string()),
        ).await;
        
        let duration = start_time.elapsed();
        
        match &result {
            Ok(data) => {
                self.logger.success(&format!(
                    "✅ Blob download completed: {} ({}) in {:.2}s ({}/s)",
                    &digest[..16],
                    self.logger.format_size(data.len() as u64),
                    duration.as_secs_f64(),
                    self.logger.format_size((data.len() as f64 / duration.as_secs_f64()) as u64)
                ));
            }
            Err(e) => {
                self.logger.error(&format!(
                    "❌ Blob download failed: {} - {}",
                    &digest[..16],
                    e
                ));
            }
        }
        
        result
    }
    
    /// Upload manifest using existing proven logic
    pub async fn upload_manifest(
        &self,
        manifest_data: &str,
        repository: &str,
        reference: &str,
        token: Option<&str>,
    ) -> Result<()> {
        let start_time = std::time::Instant::now();
        
        self.logger.info(&format!(
            "📄 Starting manifest upload: {}:{} ({})",
            repository,
            reference,
            self.logger.format_size(manifest_data.len() as u64)
        ));
        
        // Use existing proven upload logic from RegistryClient
        let result = self.client.upload_manifest_with_token(
            manifest_data,
            repository,
            reference,
            &token.map(|s| s.to_string()),
        ).await;
        
        let duration = start_time.elapsed();
        
        match &result {
            Ok(_) => {
                self.logger.success(&format!(
                    "✅ Manifest upload completed: {}:{} in {:.2}s",
                    repository,
                    reference,
                    duration.as_secs_f64()
                ));
            }
            Err(e) => {
                self.logger.error(&format!(
                    "❌ Manifest upload failed: {}:{} - {}",
                    repository,
                    reference,
                    e
                ));
            }
        }
        
        result
    }
    
    /// Download manifest using existing proven logic
    pub async fn download_manifest(
        &self,
        repository: &str,
        reference: &str,
        token: Option<&str>,
    ) -> Result<Vec<u8>> {
        let start_time = std::time::Instant::now();
        
        self.logger.info(&format!(
            "📄 Starting manifest download: {}:{}",
            repository,
            reference
        ));
        
        // Use existing proven download logic from RegistryClient
        let result = self.client.pull_manifest(
            repository,
            reference,
            &token.map(|s| s.to_string()),
        ).await;
        
        let duration = start_time.elapsed();
        
        match &result {
            Ok(data) => {
                self.logger.success(&format!(
                    "✅ Manifest download completed: {}:{} ({}) in {:.2}s",
                    repository,
                    reference,
                    self.logger.format_size(data.len() as u64),
                    duration.as_secs_f64()
                ));
            }
            Err(e) => {
                self.logger.error(&format!(
                    "❌ Manifest download failed: {}:{} - {}",
                    repository,
                    reference,
                    e
                ));
            }
        }
        
        result
    }
    
    /// Check if blob exists using existing proven logic
    pub async fn blob_exists(
        &self,
        digest: &str,
        repository: &str,
        token: Option<&str>,
    ) -> Result<bool> {
        self.client.check_blob_exists_with_token(
            digest,
            repository,
            &token.map(|s| s.to_string()),
        ).await
    }
    
    /// Batch upload blobs with concurrency and progress tracking
    pub async fn batch_upload_blobs(
        &self,
        blobs: Vec<(Vec<u8>, String)>, // (data, digest)
        repository: &str,
        token: Option<&str>,
        max_concurrent: Option<usize>,
    ) -> Result<Vec<TransferResult>> {
        let total_count = blobs.len();
        let total_size: u64 = blobs.iter().map(|(data, _)| data.len() as u64).sum();
        
        self.logger.section(&format!(
            "🚀 Starting batch blob upload: {} blobs ({})",
            total_count,
            self.logger.format_size(total_size)
        ));
        
        let start_time = std::time::Instant::now();
        let mut results = Vec::new();
        
        // For now, use sequential processing with existing logic
        // TODO: Integrate with UnifiedPipeline for true parallelization
        for (i, (data, digest)) in blobs.into_iter().enumerate() {
            let operation_start = std::time::Instant::now();
            let operation_id = format!("blob_upload_{}", i);
            
            self.logger.info(&format!(
                "📤 [{}/{}] Uploading blob: {}",
                i + 1, total_count, &digest[..16]
            ));
            
            match self.upload_blob(&data, &digest, repository, token).await {
                Ok(_) => {
                    let duration = operation_start.elapsed();
                    results.push(TransferResult {
                        operation_id,
                        success: true,
                        bytes_transferred: data.len() as u64,
                        duration_ms: duration.as_millis() as u64,
                        error: None,
                    });
                }
                Err(e) => {
                    let duration = operation_start.elapsed();
                    results.push(TransferResult {
                        operation_id,
                        success: false,
                        bytes_transferred: 0,
                        duration_ms: duration.as_millis() as u64,
                        error: Some(e.to_string()),
                    });
                }
            }
        }
        
        let total_duration = start_time.elapsed();
        let successful_count = results.iter().filter(|r| r.success).count();
        let total_transferred: u64 = results.iter().map(|r| r.bytes_transferred).sum();
        
        if successful_count == total_count {
            self.logger.success(&format!(
                "✅ Batch upload completed: {}/{} blobs ({}) in {:.2}s ({}/s)",
                successful_count,
                total_count,
                self.logger.format_size(total_transferred),
                total_duration.as_secs_f64(),
                self.logger.format_size((total_transferred as f64 / total_duration.as_secs_f64()) as u64)
            ));
        } else {
            self.logger.warning(&format!(
                "⚠️ Batch upload partially completed: {}/{} blobs successful",
                successful_count,
                total_count
            ));
        }
        
        Ok(results)
    }
    
    /// Batch download blobs with concurrency and progress tracking
    pub async fn batch_download_blobs(
        &self,
        digests: Vec<String>,
        repository: &str,
        token: Option<&str>,
        max_concurrent: Option<usize>,
    ) -> Result<Vec<(String, Result<Vec<u8>>)>> {
        let total_count = digests.len();
        
        self.logger.section(&format!(
            "⬇️ Starting batch blob download: {} blobs from {}",
            total_count,
            repository
        ));
        
        let start_time = std::time::Instant::now();
        let mut results = Vec::new();
        
        // For now, use sequential processing with existing logic
        // TODO: Integrate with UnifiedPipeline for true parallelization
        for (i, digest) in digests.into_iter().enumerate() {
            self.logger.info(&format!(
                "📥 [{}/{}] Downloading blob: {}",
                i + 1, total_count, &digest[..16]
            ));
            
            let result = self.download_blob(&digest, repository, token).await;
            results.push((digest, result));
        }
        
        let total_duration = start_time.elapsed();
        let successful_count = results.iter().filter(|(_, r)| r.is_ok()).count();
        let total_downloaded: u64 = results.iter()
            .filter_map(|(_, r)| r.as_ref().ok())
            .map(|data| data.len() as u64)
            .sum();
        
        if successful_count == total_count {
            self.logger.success(&format!(
                "✅ Batch download completed: {}/{} blobs ({}) in {:.2}s ({}/s)",
                successful_count,
                total_count,
                self.logger.format_size(total_downloaded),
                total_duration.as_secs_f64(),
                self.logger.format_size((total_downloaded as f64 / total_duration.as_secs_f64()) as u64)
            ));
        } else {
            self.logger.warning(&format!(
                "⚠️ Batch download partially completed: {}/{} blobs successful",
                successful_count,
                total_count
            ));
        }
        
        Ok(results)
    }
    
    /// Execute a general transfer operation (can be used by UnifiedPipeline)
    pub async fn execute_transfer_operation(
        &self,
        operation: &TransferOperation,
        data: Option<&[u8]>,
        token: Option<&str>,
    ) -> Result<TransferResult> {
        let start_time = std::time::Instant::now();
        
        let result = match operation.operation_type {
            TransferType::BlobUpload => {
                let data = data.ok_or_else(|| {
                    RegistryError::Validation("Data required for blob upload".to_string())
                })?;
                
                match self.upload_blob(data, &operation.digest_or_reference, &operation.repository, token).await {
                    Ok(_) => (true, data.len() as u64, None),
                    Err(e) => (false, 0, Some(e.to_string())),
                }
            }
            
            TransferType::BlobDownload => {
                match self.download_blob(&operation.digest_or_reference, &operation.repository, token).await {
                    Ok(data) => (true, data.len() as u64, None),
                    Err(e) => (false, 0, Some(e.to_string())),
                }
            }
            
            TransferType::ManifestUpload => {
                let data = data.ok_or_else(|| {
                    RegistryError::Validation("Data required for manifest upload".to_string())
                })?;
                
                let manifest_str = String::from_utf8(data.to_vec()).map_err(|e| {
                    RegistryError::Parse(format!("Invalid UTF-8 in manifest: {}", e))
                })?;
                
                match self.upload_manifest(&manifest_str, &operation.repository, &operation.digest_or_reference, token).await {
                    Ok(_) => (true, data.len() as u64, None),
                    Err(e) => (false, 0, Some(e.to_string())),
                }
            }
            
            TransferType::ManifestDownload => {
                match self.download_manifest(&operation.repository, &operation.digest_or_reference, token).await {
                    Ok(data) => (true, data.len() as u64, None),
                    Err(e) => (false, 0, Some(e.to_string())),
                }
            }
        };
        
        let duration = start_time.elapsed();
        
        Ok(TransferResult {
            operation_id: operation.operation_id.clone(),
            success: result.0,
            bytes_transferred: result.1,
            duration_ms: duration.as_millis() as u64,
            error: result.2,
        })
    }
}

/// Factory for creating unified registry operations
pub struct UnifiedOperationsFactory;

impl UnifiedOperationsFactory {
    /// Create unified operations from existing RegistryClient
    pub fn from_client(
        client: RegistryClient,
        pipeline: Arc<UnifiedPipeline>,
        logger: Logger,
    ) -> UnifiedRegistryOperations {
        UnifiedRegistryOperations::new(client, pipeline, logger)
    }
    
    /// Create unified operations with concurrency support
    pub fn with_concurrency(
        client: RegistryClient,
        pipeline: Arc<UnifiedPipeline>,
        logger: Logger,
        concurrency: Arc<dyn ConcurrencyController>,
    ) -> UnifiedRegistryOperations {
        UnifiedRegistryOperations::new(client, pipeline, logger)
            .with_concurrency(concurrency)
    }
}
