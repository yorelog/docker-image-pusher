//! Unified transport API for Docker Registry API v2 and OCI Distribution Specification
//! 
//! This module provides a unified interface for registry operations that works with:
//! - Docker Registry API v2
//! - OCI Distribution Specification v1.0+
//! - Harbor, GitLab Container Registry, Amazon ECR, Azure ACR, etc.

use crate::error::{RegistryError, Result};
use crate::logging::Logger;
use crate::registry::pipeline::UnifiedPipeline;
use async_trait::async_trait;
use std::sync::Arc;

/// OCI/Docker Registry transport operations
#[async_trait]
pub trait RegistryTransport: Send + Sync {
    /// Check registry API version and capabilities
    async fn check_api_version(&self) -> Result<ApiVersion>;
    
    /// Authenticate with registry and get token
    async fn authenticate(&self, credentials: Option<&Credentials>) -> Result<Option<String>>;
    
    /// Get manifest from registry
    async fn get_manifest(&self, request: &ManifestRequest) -> Result<ManifestResponse>;
    
    /// Put manifest to registry
    async fn put_manifest(&self, request: &ManifestPutRequest) -> Result<()>;
    
    /// Check if blob exists in registry
    async fn blob_exists(&self, request: &BlobRequest) -> Result<bool>;
    
    /// Get blob from registry
    async fn get_blob(&self, request: &BlobRequest) -> Result<BlobResponse>;
    
    /// Upload blob to registry (supports chunked upload)
    async fn put_blob(&self, request: &BlobPutRequest) -> Result<String>;
    
    /// Mount blob from another repository (cross-repo blob mount)
    async fn mount_blob(&self, request: &BlobMountRequest) -> Result<bool>;
    
    /// List repository tags
    async fn list_tags(&self, request: &TagListRequest) -> Result<Vec<String>>;
    
    /// Delete manifest or blob
    async fn delete(&self, request: &DeleteRequest) -> Result<()>;
}

/// Registry API version information
#[derive(Debug, Clone)]
pub struct ApiVersion {
    pub version: String,
    pub supports_oci: bool,
    pub supports_docker_v2: bool,
    pub supports_chunked_upload: bool,
    pub supports_cross_repo_mount: bool,
    pub max_chunk_size: Option<u64>,
}

/// Authentication credentials
#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    pub registry_token: Option<String>,
}

/// Manifest request parameters
#[derive(Debug, Clone)]
pub struct ManifestRequest {
    pub registry_url: String,
    pub repository: String,
    pub reference: String, // tag or digest
    pub accept_types: Vec<String>,
    pub token: Option<String>,
}

/// Manifest response data
#[derive(Debug)]
pub struct ManifestResponse {
    pub data: Vec<u8>,
    pub content_type: String,
    pub digest: Option<String>,
    pub size: u64,
}

/// Manifest upload request
#[derive(Debug)]
pub struct ManifestPutRequest {
    pub registry_url: String,
    pub repository: String,
    pub reference: String,
    pub data: Vec<u8>,
    pub content_type: String,
    pub token: Option<String>,
}

/// Blob request parameters
#[derive(Debug, Clone)]
pub struct BlobRequest {
    pub registry_url: String,
    pub repository: String,
    pub digest: String,
    pub token: Option<String>,
}

/// Blob response data
#[derive(Debug)]
pub struct BlobResponse {
    pub data: Vec<u8>,
    pub content_type: String,
    pub size: u64,
}

/// Blob upload request
#[derive(Debug)]
pub struct BlobPutRequest {
    pub registry_url: String,
    pub repository: String,
    pub digest: String,
    pub data: Vec<u8>,
    pub use_chunked_upload: bool,
    pub chunk_size: Option<u64>,
    pub token: Option<String>,
}

/// Cross-repository blob mount request
#[derive(Debug)]
pub struct BlobMountRequest {
    pub registry_url: String,
    pub source_repository: String,
    pub target_repository: String,
    pub digest: String,
    pub token: Option<String>,
}

/// Tag list request
#[derive(Debug)]
pub struct TagListRequest {
    pub registry_url: String,
    pub repository: String,
    pub limit: Option<u32>,
    pub last: Option<String>,
    pub token: Option<String>,
}

/// Delete request
#[derive(Debug)]
pub struct DeleteRequest {
    pub registry_url: String,
    pub repository: String,
    pub reference: String, // tag or digest
    pub token: Option<String>,
}

/// Standard registry transport implementation
pub struct StandardRegistryTransport {
    client: reqwest::Client,
    logger: Logger,
    pipeline: Option<Arc<UnifiedPipeline>>,
}

impl StandardRegistryTransport {
    pub fn new(logger: Logger) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");
            
        Self {
            client,
            logger,
            pipeline: None,
        }
    }
    
    pub fn with_pipeline(mut self, pipeline: Arc<UnifiedPipeline>) -> Self {
        self.pipeline = Some(pipeline);
        self
    }
    
    /// Build standard Accept header for manifest requests
    fn build_manifest_accept_header() -> String {
        vec![
            "application/vnd.docker.distribution.manifest.v2+json",
            "application/vnd.docker.distribution.manifest.list.v2+json", 
            "application/vnd.oci.image.manifest.v1+json",
            "application/vnd.oci.image.index.v1+json",
            "application/vnd.docker.distribution.manifest.v1+json", // Legacy support
        ].join(", ")
    }
    
    /// Normalize digest format
    fn normalize_digest(digest: &str) -> String {
        if digest.starts_with("sha256:") {
            digest.to_string()
        } else {
            format!("sha256:{}", digest)
        }
    }
}

#[async_trait]
impl RegistryTransport for StandardRegistryTransport {
    async fn check_api_version(&self) -> Result<ApiVersion> {
        // For now, assume standard capabilities
        // In a real implementation, we would probe the registry
        Ok(ApiVersion {
            version: "2.0".to_string(),
            supports_oci: true,
            supports_docker_v2: true,
            supports_chunked_upload: true,
            supports_cross_repo_mount: true,
            max_chunk_size: Some(100 * 1024 * 1024), // 100MB default
        })
    }
    
    async fn authenticate(&self, credentials: Option<&Credentials>) -> Result<Option<String>> {
        // Authentication is handled by the existing auth module
        // This is a placeholder for the unified interface
        Ok(credentials.and_then(|c| c.registry_token.clone()))
    }
    
    async fn get_manifest(&self, request: &ManifestRequest) -> Result<ManifestResponse> {
        let url = format!("{}/v2/{}/manifests/{}", 
            request.registry_url, request.repository, request.reference);
        
        let accept_header = if request.accept_types.is_empty() {
            Self::build_manifest_accept_header()
        } else {
            request.accept_types.join(", ")
        };
        
        let mut req = self.client.get(&url)
            .header("Accept", accept_header);
            
        if let Some(token) = &request.token {
            req = req.bearer_auth(token);
        }
        
        let response = req.send().await
            .map_err(|e| RegistryError::Network(format!("Failed to get manifest: {}", e)))?;
            
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(RegistryError::Registry(format!(
                "Failed to get manifest: HTTP {} - {}", status, error_text
            )));
        }
        
        let content_type = response.headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
            
        let digest = response.headers()
            .get("docker-content-digest")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
            
        let data = response.bytes().await
            .map_err(|e| RegistryError::Network(format!("Failed to read manifest data: {}", e)))?
            .to_vec();
            
        Ok(ManifestResponse {
            size: data.len() as u64,
            data,
            content_type,
            digest,
        })
    }
    
    async fn put_manifest(&self, request: &ManifestPutRequest) -> Result<()> {
        let url = format!("{}/v2/{}/manifests/{}", 
            request.registry_url, request.repository, request.reference);
            
        let mut req = self.client.put(&url)
            .header("Content-Type", &request.content_type)
            .body(request.data.clone());
            
        if let Some(token) = &request.token {
            req = req.bearer_auth(token);
        }
        
        let response = req.send().await
            .map_err(|e| RegistryError::Network(format!("Failed to put manifest: {}", e)))?;
            
        if response.status().is_success() {
            self.logger.success(&format!(
                "Manifest uploaded successfully for {}:{}", 
                request.repository, request.reference
            ));
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(RegistryError::Registry(format!(
                "Failed to put manifest: HTTP {} - {}", status, error_text
            )))
        }
    }
    
    async fn blob_exists(&self, request: &BlobRequest) -> Result<bool> {
        let digest = Self::normalize_digest(&request.digest);
        let url = format!("{}/v2/{}/blobs/{}", 
            request.registry_url, request.repository, digest);
            
        let mut req = self.client.head(&url);
        
        if let Some(token) = &request.token {
            req = req.bearer_auth(token);
        }
        
        let response = req.send().await
            .map_err(|e| RegistryError::Network(format!("Failed to check blob existence: {}", e)))?;
            
        Ok(response.status().is_success())
    }
    
    async fn get_blob(&self, request: &BlobRequest) -> Result<BlobResponse> {
        let digest = Self::normalize_digest(&request.digest);
        let url = format!("{}/v2/{}/blobs/{}", 
            request.registry_url, request.repository, digest);
            
        let mut req = self.client.get(&url);
        
        if let Some(token) = &request.token {
            req = req.bearer_auth(token);
        }
        
        let response = req.send().await
            .map_err(|e| RegistryError::Network(format!("Failed to get blob: {}", e)))?;
            
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(RegistryError::Registry(format!(
                "Failed to get blob: HTTP {} - {}", status, error_text
            )));
        }
        
        let content_type = response.headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
            
        let data = response.bytes().await
            .map_err(|e| RegistryError::Network(format!("Failed to read blob data: {}", e)))?
            .to_vec();
            
        Ok(BlobResponse {
            size: data.len() as u64,
            data,
            content_type,
        })
    }
    
    async fn put_blob(&self, request: &BlobPutRequest) -> Result<String> {
        let digest = Self::normalize_digest(&request.digest);
        
        // Check if blob already exists
        let exists_request = BlobRequest {
            registry_url: request.registry_url.clone(),
            repository: request.repository.clone(),
            digest: digest.clone(),
            token: request.token.clone(),
        };
        
        if self.blob_exists(&exists_request).await? {
            self.logger.info(&format!("Blob {} already exists, skipping", &digest[..16]));
            return Ok(digest);
        }
        
        if request.use_chunked_upload && request.data.len() > 50 * 1024 * 1024 {
            // Use chunked upload for large blobs
            self.put_blob_chunked(request).await
        } else {
            // Use monolithic upload for smaller blobs
            self.put_blob_monolithic(request).await
        }
    }
    
    async fn mount_blob(&self, request: &BlobMountRequest) -> Result<bool> {
        let digest = Self::normalize_digest(&request.digest);
        let url = format!("{}/v2/{}/blobs/uploads/?mount={}&from={}", 
            request.registry_url, request.target_repository, digest, request.source_repository);
            
        let mut req = self.client.post(&url);
        
        if let Some(token) = &request.token {
            req = req.bearer_auth(token);
        }
        
        let response = req.send().await
            .map_err(|e| RegistryError::Network(format!("Failed to mount blob: {}", e)))?;
            
        // 201 Created means mount succeeded
        // 202 Accepted means mount failed, need to upload
        match response.status().as_u16() {
            201 => {
                self.logger.success(&format!("Blob {} mounted successfully", &digest[..16]));
                Ok(true)
            },
            202 => {
                self.logger.verbose(&format!("Blob {} mount failed, upload required", &digest[..16]));
                Ok(false)
            },
            _ => {
                let status = response.status();
                let error_text = response.text().await
                    .unwrap_or_else(|_| "Failed to read error response".to_string());
                Err(RegistryError::Registry(format!(
                    "Failed to mount blob: HTTP {} - {}", status, error_text
                )))
            }
        }
    }
    
    async fn list_tags(&self, request: &TagListRequest) -> Result<Vec<String>> {
        let mut url = format!("{}/v2/{}/tags/list", request.registry_url, request.repository);
        
        let mut params = Vec::new();
        if let Some(n) = request.limit {
            params.push(format!("n={}", n));
        }
        if let Some(last) = &request.last {
            params.push(format!("last={}", last));
        }
        
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        
        let mut req = self.client.get(&url);
        
        if let Some(token) = &request.token {
            req = req.bearer_auth(token);
        }
        
        let response = req.send().await
            .map_err(|e| RegistryError::Network(format!("Failed to list tags: {}", e)))?;
            
        if !response.status().is_success() {
            let status = response.status();
            if status.as_u16() == 404 {
                return Ok(Vec::new());
            }
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(RegistryError::Registry(format!(
                "Failed to list tags: HTTP {} - {}", status, error_text
            )));
        }
        
        let data: serde_json::Value = response.json().await
            .map_err(|e| RegistryError::Parse(format!("Failed to parse tag list: {}", e)))?;
            
        let tags = data.get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                   .filter_map(|v| v.as_str().map(|s| s.to_string()))
                   .collect()
            })
            .unwrap_or_else(Vec::new);
            
        Ok(tags)
    }
    
    async fn delete(&self, request: &DeleteRequest) -> Result<()> {
        let url = format!("{}/v2/{}/manifests/{}", 
            request.registry_url, request.repository, request.reference);
            
        let mut req = self.client.delete(&url);
        
        if let Some(token) = &request.token {
            req = req.bearer_auth(token);
        }
        
        let response = req.send().await
            .map_err(|e| RegistryError::Network(format!("Failed to delete: {}", e)))?;
            
        if response.status().is_success() {
            self.logger.success(&format!(
                "Successfully deleted {}/{}", request.repository, request.reference
            ));
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(RegistryError::Registry(format!(
                "Failed to delete: HTTP {} - {}", status, error_text
            )))
        }
    }
}

impl StandardRegistryTransport {
    /// Monolithic blob upload
    async fn put_blob_monolithic(&self, request: &BlobPutRequest) -> Result<String> {
        let digest = Self::normalize_digest(&request.digest);
        
        // Step 1: Start upload session
        let upload_url = format!("{}/v2/{}/blobs/uploads/", 
            request.registry_url, request.repository);
            
        let mut req = self.client.post(&upload_url);
        if let Some(token) = &request.token {
            req = req.bearer_auth(token);
        }
        
        let response = req.send().await
            .map_err(|e| RegistryError::Network(format!("Failed to start upload: {}", e)))?;
            
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(RegistryError::Upload(format!(
                "Failed to start upload session: HTTP {} - {}", status, error_text
            )));
        }
        
        // Get upload location
        let location = response.headers()
            .get("Location")
            .ok_or_else(|| RegistryError::Upload("Missing Location header".to_string()))?
            .to_str()
            .map_err(|e| RegistryError::Upload(format!("Invalid Location header: {}", e)))?;
            
        let full_location = if location.starts_with("http") {
            location.to_string()
        } else {
            format!("{}{}", request.registry_url, location)
        };
        
        // Step 2: Upload blob data
        let final_url = format!("{}?digest={}", full_location, digest);
        
        let mut upload_req = self.client.put(&final_url)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", request.data.len().to_string())
            .body(request.data.clone());
            
        if let Some(token) = &request.token {
            upload_req = upload_req.bearer_auth(token);
        }
        
        let upload_response = upload_req.send().await
            .map_err(|e| RegistryError::Network(format!("Failed to upload blob: {}", e)))?;
            
        if upload_response.status().is_success() {
            self.logger.success(&format!("Blob {} uploaded successfully", &digest[..16]));
            Ok(digest)
        } else {
            let status = upload_response.status();
            let error_text = upload_response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(RegistryError::Upload(format!(
                "Blob upload failed: HTTP {} - {}", status, error_text
            )))
        }
    }
    
    /// Chunked blob upload for large blobs
    async fn put_blob_chunked(&self, request: &BlobPutRequest) -> Result<String> {
        // TODO: Implement chunked upload
        // For now, fall back to monolithic upload
        self.put_blob_monolithic(request).await
    }
}

/// Parallel transport operations using UnifiedPipeline
pub struct ParallelRegistryTransport {
    base_transport: StandardRegistryTransport,
    #[allow(dead_code)]
    pipeline: Arc<UnifiedPipeline>,
}

impl ParallelRegistryTransport {
    pub fn new(
        base_transport: StandardRegistryTransport,
        pipeline: Arc<UnifiedPipeline>,
    ) -> Self {
        Self {
            base_transport,
            pipeline,
        }
    }
    
    /// Parallel blob upload operations
    pub async fn put_blobs_parallel(
        &self,
        requests: Vec<BlobPutRequest>,
    ) -> Result<Vec<String>> {
        // TODO: Implement parallel upload using UnifiedPipeline
        // This will use the existing concurrency and pipeline infrastructure
        let mut results = Vec::new();
        
        for request in requests {
            let result = self.base_transport.put_blob(&request).await?;
            results.push(result);
        }
        
        Ok(results)
    }
    
    /// Parallel blob download operations
    pub async fn get_blobs_parallel(
        &self,
        requests: Vec<BlobRequest>,
    ) -> Result<Vec<BlobResponse>> {
        // TODO: Implement parallel download using UnifiedPipeline
        let mut results = Vec::new();
        
        for request in requests {
            let result = self.base_transport.get_blob(&request).await?;
            results.push(result);
        }
        
        Ok(results)
    }
}

/// Transport factory for creating appropriate transport implementations
pub struct TransportFactory;

impl TransportFactory {
    pub fn create_standard(logger: Logger) -> StandardRegistryTransport {
        StandardRegistryTransport::new(logger)
    }
    
    pub fn create_parallel(
        logger: Logger,
        pipeline: Arc<UnifiedPipeline>,
    ) -> ParallelRegistryTransport {
        let base = StandardRegistryTransport::new(logger)
            .with_pipeline(pipeline.clone());
            
        ParallelRegistryTransport::new(base, pipeline)
    }
}
