//! Registry client adapter for unified transport API
//! 
//! This module provides an adapter that integrates the existing RegistryClient
//! with the new unified transport API, maintaining backward compatibility while
//! providing access to the new standardized interface.

use crate::error::{RegistryError, Result};
use crate::registry::client::RegistryClient;
use crate::registry::transport::{
    RegistryTransport, ManifestRequest, ManifestResponse, ManifestPutRequest,
    BlobRequest, BlobResponse, BlobPutRequest, BlobMountRequest,
    TagListRequest, DeleteRequest, ApiVersion, Credentials
};
use async_trait::async_trait;

/// Adapter that makes RegistryClient compatible with the unified transport API
pub struct RegistryClientAdapter {
    client: RegistryClient,
}

impl RegistryClientAdapter {
    pub fn new(client: RegistryClient) -> Self {
        Self { client }
    }
    
    pub fn inner(&self) -> &RegistryClient {
        &self.client
    }
    
    pub fn into_inner(self) -> RegistryClient {
        self.client
    }
}

#[async_trait]
impl RegistryTransport for RegistryClientAdapter {
    async fn check_api_version(&self) -> Result<ApiVersion> {
        // Test connectivity to determine API capabilities
        self.client.test_connectivity().await?;
        
        // Return standard Docker Registry API v2 capabilities
        Ok(ApiVersion {
            version: "2.0".to_string(),
            supports_oci: true,
            supports_docker_v2: true,
            supports_chunked_upload: true,
            supports_cross_repo_mount: true,
            max_chunk_size: Some(100 * 1024 * 1024), // 100MB
        })
    }
    
    async fn authenticate(&self, credentials: Option<&Credentials>) -> Result<Option<String>> {
        if let Some(creds) = credentials {
            if let Some(token) = &creds.registry_token {
                // Use provided token
                Ok(Some(token.clone()))
            } else {
                // Use existing authentication method
                let auth_config = crate::cli::config::AuthConfig {
                    username: creds.username.clone(),
                    password: creds.password.clone(),
                };
                self.client.authenticate(&auth_config).await
            }
        } else {
            Ok(None)
        }
    }
    
    async fn get_manifest(&self, request: &ManifestRequest) -> Result<ManifestResponse> {
        let data = self.client.pull_manifest(
            &request.repository,
            &request.reference,
            &request.token,
        ).await?;
        
        // Detect content type from data
        let content_type = self.detect_manifest_content_type(&data)?;
        
        Ok(ManifestResponse {
            size: data.len() as u64,
            data,
            content_type,
            digest: None, // RegistryClient doesn't return digest
        })
    }
    
    async fn put_manifest(&self, request: &ManifestPutRequest) -> Result<()> {
        let manifest_str = String::from_utf8(request.data.clone())
            .map_err(|e| RegistryError::Parse(format!("Invalid UTF-8 in manifest: {}", e)))?;
            
        self.client.upload_manifest_with_token(
            &manifest_str,
            &request.repository,
            &request.reference,
            &request.token,
        ).await
    }
    
    async fn blob_exists(&self, request: &BlobRequest) -> Result<bool> {
        self.client.check_blob_exists_with_token(
            &request.digest,
            &request.repository,
            &request.token,
        ).await
    }
    
    async fn get_blob(&self, request: &BlobRequest) -> Result<BlobResponse> {
        let data = self.client.pull_blob(
            &request.repository,
            &request.digest,
            &request.token,
        ).await?;
        
        Ok(BlobResponse {
            size: data.len() as u64,
            data,
            content_type: "application/octet-stream".to_string(),
        })
    }
    
    async fn put_blob(&self, request: &BlobPutRequest) -> Result<String> {
        self.client.upload_blob_with_token(
            &request.data,
            &request.digest,
            &request.repository,
            &request.token,
        ).await
    }
    
    async fn mount_blob(&self, _request: &BlobMountRequest) -> Result<bool> {
        // RegistryClient doesn't support cross-repo blob mounting yet
        // Return false to indicate mount failed, requiring upload
        Ok(false)
    }
    
    async fn list_tags(&self, request: &TagListRequest) -> Result<Vec<String>> {
        self.client.list_tags(&request.repository, &request.token).await
    }
    
    async fn delete(&self, _request: &DeleteRequest) -> Result<()> {
        // RegistryClient doesn't support delete operations yet
        Err(RegistryError::Registry(
            "Delete operations not supported by RegistryClient".to_string()
        ))
    }
}

impl RegistryClientAdapter {
    fn detect_manifest_content_type(&self, data: &[u8]) -> Result<String> {
        let manifest: serde_json::Value = serde_json::from_slice(data)
            .map_err(|e| RegistryError::Parse(format!("Invalid manifest JSON: {}", e)))?;
            
        let media_type = manifest
            .get("mediaType")
            .and_then(|m| m.as_str())
            .unwrap_or("application/vnd.docker.distribution.manifest.v2+json");
            
        Ok(media_type.to_string())
    }
}

/// Unified registry operations using both old and new APIs
pub struct UnifiedRegistryClient {
    transport: Box<dyn RegistryTransport>,
    legacy_client: Option<RegistryClient>,
}

impl UnifiedRegistryClient {
    pub fn from_client(client: RegistryClient) -> Self {
        let adapter = RegistryClientAdapter::new(client.clone());
        Self {
            transport: Box::new(adapter),
            legacy_client: Some(client),
        }
    }
    
    pub fn from_transport(transport: Box<dyn RegistryTransport>) -> Self {
        Self {
            transport,
            legacy_client: None,
        }
    }
    
    /// Get the transport interface
    pub fn transport(&self) -> &dyn RegistryTransport {
        self.transport.as_ref()
    }
    
    /// Get the legacy client if available (for backward compatibility)
    pub fn legacy_client(&self) -> Option<&RegistryClient> {
        self.legacy_client.as_ref()
    }
    
    /// Unified blob upload with automatic fallback
    pub async fn upload_blob_unified(
        &self,
        data: &[u8],
        digest: &str,
        repository: &str,
        registry_url: &str,
        token: Option<&str>,
    ) -> Result<String> {
        let request = BlobPutRequest {
            registry_url: registry_url.to_string(),
            repository: repository.to_string(),
            digest: digest.to_string(),
            data: data.to_vec(),
            use_chunked_upload: data.len() > 50 * 1024 * 1024, // Use chunked for >50MB
            chunk_size: Some(10 * 1024 * 1024), // 10MB chunks
            token: token.map(|s| s.to_string()),
        };
        
        self.transport.put_blob(&request).await
    }
    
    /// Unified blob download with automatic fallback
    pub async fn download_blob_unified(
        &self,
        digest: &str,
        repository: &str,
        registry_url: &str,
        token: Option<&str>,
    ) -> Result<Vec<u8>> {
        let request = BlobRequest {
            registry_url: registry_url.to_string(),
            repository: repository.to_string(),
            digest: digest.to_string(),
            token: token.map(|s| s.to_string()),
        };
        
        let response = self.transport.get_blob(&request).await?;
        Ok(response.data)
    }
    
    /// Unified manifest operations
    pub async fn upload_manifest_unified(
        &self,
        manifest_data: &[u8],
        repository: &str,
        reference: &str,
        registry_url: &str,
        token: Option<&str>,
    ) -> Result<()> {
        // Detect content type
        let content_type = self.detect_content_type(manifest_data)?;
        
        let request = ManifestPutRequest {
            registry_url: registry_url.to_string(),
            repository: repository.to_string(),
            reference: reference.to_string(),
            data: manifest_data.to_vec(),
            content_type,
            token: token.map(|s| s.to_string()),
        };
        
        self.transport.put_manifest(&request).await
    }
    
    pub async fn download_manifest_unified(
        &self,
        repository: &str,
        reference: &str,
        registry_url: &str,
        token: Option<&str>,
    ) -> Result<Vec<u8>> {
        let request = ManifestRequest {
            registry_url: registry_url.to_string(),
            repository: repository.to_string(),
            reference: reference.to_string(),
            accept_types: vec![
                "application/vnd.docker.distribution.manifest.v2+json".to_string(),
                "application/vnd.docker.distribution.manifest.list.v2+json".to_string(),
                "application/vnd.oci.image.manifest.v1+json".to_string(),
                "application/vnd.oci.image.index.v1+json".to_string(),
            ],
            token: token.map(|s| s.to_string()),
        };
        
        let response = self.transport.get_manifest(&request).await?;
        Ok(response.data)
    }
    
    fn detect_content_type(&self, data: &[u8]) -> Result<String> {
        let manifest: serde_json::Value = serde_json::from_slice(data)
            .map_err(|e| RegistryError::Parse(format!("Invalid manifest JSON: {}", e)))?;
            
        let media_type = manifest
            .get("mediaType")
            .and_then(|m| m.as_str())
            .unwrap_or("application/vnd.docker.distribution.manifest.v2+json");
            
        Ok(media_type.to_string())
    }
}
