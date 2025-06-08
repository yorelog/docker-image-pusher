//! OCI client adapter for standardized container registry operations
//!
//! This module provides an adapter around the oci-client crate to integrate
//! OCI-compliant registry operations into our registry client.

use crate::cli::config::AuthConfig;
use crate::error::{RegistryError, Result};
use crate::logging::Logger;
use async_trait::async_trait;
use oci_client::{Client, Reference};
use oci_client::client::ClientConfig;
use oci_client::secrets::RegistryAuth;

#[derive(Clone)]
pub struct OciClientAdapter {
    client: Client,
    logger: Logger,
    registry_url: String,
    auth: Option<RegistryAuth>,
}

/// Builder pattern for creating OCI clients with configuration
#[derive(Debug)]
pub struct OciClientBuilder {
    registry_url: String,
    auth: Option<RegistryAuth>,
    logger: Logger,
}

impl OciClientBuilder {
    pub fn new(registry_url: String, logger: Logger) -> Self {
        Self {
            registry_url,
            auth: None,
            logger,
        }
    }

    pub fn with_auth(mut self, auth: RegistryAuth) -> Self {
        self.auth = Some(auth);
        self
    }

    pub fn build(self) -> Result<OciClientAdapter> {
        self.logger.verbose("Building OCI client...");
        
        // For now, use default configuration
        // The oci-client crate API might require different configuration
        let config = ClientConfig::default();
        let client = Client::new(config);
        
        Ok(OciClientAdapter {
            client,
            logger: self.logger,
            registry_url: self.registry_url,
            auth: self.auth,
        })
    }
}

impl OciClientAdapter {
    /// Create a new OCI client adapter without authentication
    pub fn new(registry_url: String, logger: Logger) -> Result<Self> {
        OciClientBuilder::new(registry_url, logger).build()
    }

    /// Create a new OCI client adapter with authentication
    pub fn with_auth(
        registry_url: String,
        auth_config: &AuthConfig,
        logger: Logger,
    ) -> Result<Self> {
        // Convert our auth config to OCI client auth
        let auth = if !auth_config.username.is_empty() && !auth_config.password.is_empty() {
            RegistryAuth::Basic(auth_config.username.clone(), auth_config.password.clone())
        } else {
            return Err(RegistryError::Auth("Username and password required for OCI client auth".to_string()));
        };

        OciClientBuilder::new(registry_url, logger)
            .with_auth(auth)
            .build()
    }

    /// Update the OCI client to use a bearer token for authentication
    pub fn update_with_bearer_token(&mut self, token: &str) -> Result<()> {
        self.logger.verbose("Updating OCI client with bearer token");
        self.auth = Some(RegistryAuth::Bearer(token.to_string()));
        Ok(())
    }

    /// Update the OCI client authentication
    pub fn update_auth(&mut self, auth: RegistryAuth) -> Result<()> {
        self.logger.verbose("Updating OCI client authentication");
        self.auth = Some(auth);
        Ok(())
    }

    fn create_reference(&self, repository: &str, reference: &str) -> Result<Reference> {
        let ref_str = if reference.starts_with("sha256:") {
            format!("{}/{}@{}", self.registry_url.trim_start_matches("https://").trim_start_matches("http://"), repository, reference)
        } else {
            format!("{}/{}:{}", self.registry_url.trim_start_matches("https://").trim_start_matches("http://"), repository, reference)
        };
        
        Reference::try_from(ref_str.as_str())
            .map_err(|e| RegistryError::Validation(format!("Invalid reference '{}': {}", ref_str, e)))
    }

    /// Pull manifest using OCI client
    pub async fn pull_manifest(&self, repository: &str, reference: &str) -> Result<(Vec<u8>, String)> {
        self.logger.verbose(&format!("Pulling manifest {}:{} via OCI client", repository, reference));
        
        let image_ref = self.create_reference(repository, reference)?;
        let auth = self.auth.as_ref().unwrap_or(&RegistryAuth::Anonymous);
        
        match self.client.pull_manifest_raw(&image_ref, auth, &["application/vnd.docker.distribution.manifest.v2+json", "application/vnd.oci.image.manifest.v1+json"]).await {
            Ok((manifest_bytes, digest)) => {
                self.logger.verbose(&format!("Successfully pulled manifest with digest: {}", digest));
                Ok((manifest_bytes, digest))
            }
            Err(e) => {
                self.logger.error(&format!("Failed to pull manifest: {}", e));
                Err(RegistryError::Network(format!("OCI client manifest pull failed: {}", e)))
            }
        }
    }

    /// Pull blob using OCI client with authentication
    pub async fn pull_blob_with_auth(&self, repository: &str, digest: &str, auth: &RegistryAuth) -> Result<Vec<u8>> {
        self.logger.verbose(&format!("Pulling blob {} from {} via OCI client with explicit auth", &digest[..16], repository));
        
        let image_ref = self.create_reference(repository, "latest")?;
        
        // Create a new client instance with the provided authentication for this operation
        let config = oci_client::client::ClientConfig::default();
        let client = oci_client::Client::new(config);
        
        // Authenticate the client for this specific operation
        if let Err(e) = client.auth(&image_ref, auth, oci_client::RegistryOperation::Pull).await {
            self.logger.error(&format!("Failed to authenticate for pull operation: {}", e));
            return Err(RegistryError::Auth(format!("OCI client authentication failed: {}", e)));
        }
        
        let mut blob_data = Vec::new();
        match client.pull_blob(&image_ref, digest, &mut blob_data).await {
            Ok(_) => {
                self.logger.verbose(&format!("Successfully pulled blob {}", &digest[..16]));
                Ok(blob_data)
            }
            Err(e) => {
                self.logger.error(&format!("Failed to pull blob: {}", e));
                Err(RegistryError::Network(format!("OCI client blob pull failed: {}", e)))
            }
        }
    }

    /// Pull blob using OCI client
    pub async fn pull_blob(&self, repository: &str, digest: &str) -> Result<Vec<u8>> {
        let auth = self.auth.as_ref().unwrap_or(&RegistryAuth::Anonymous);
        self.pull_blob_with_auth(repository, digest, auth).await
    }

    /// Push blob using OCI client with authentication  
    pub async fn push_blob_with_auth(&self, repository: &str, data: &[u8], digest: &str, auth: &RegistryAuth) -> Result<String> {
        self.logger.verbose(&format!("Pushing blob {} to {} via OCI client with explicit auth", &digest[..16], repository));
        
        let image_ref = self.create_reference(repository, "latest")?;
        
        // Create a new client instance with the provided authentication for this operation
        let config = oci_client::client::ClientConfig::default();
        let client = oci_client::Client::new(config);
        
        // Authenticate the client for this specific operation
        if let Err(e) = client.auth(&image_ref, auth, oci_client::RegistryOperation::Push).await {
            self.logger.error(&format!("Failed to authenticate for push operation: {}", e));
            return Err(RegistryError::Auth(format!("OCI client authentication failed: {}", e)));
        }

        match client.push_blob(&image_ref, data, digest).await {
            Ok(url) => {
                self.logger.verbose(&format!("Successfully pushed blob to: {}", url));
                Ok(url)
            }
            Err(e) => {
                self.logger.error(&format!("Failed to push blob: {}", e));
                Err(RegistryError::Network(format!("OCI client blob push failed: {}", e)))
            }
        }
    }

    /// Push blob using OCI client
    pub async fn push_blob(&self, repository: &str, data: &[u8], digest: &str) -> Result<String> {
        let auth = self.auth.as_ref().unwrap_or(&RegistryAuth::Anonymous);
        self.push_blob_with_auth(repository, data, digest, auth).await
    }

    /// Push manifest using OCI client with authentication
    pub async fn push_manifest_with_auth(&self, repository: &str, reference: &str, manifest: &[u8], auth: &RegistryAuth) -> Result<String> {
        self.logger.verbose(&format!("Pushing manifest {}:{} via OCI client with explicit auth", repository, reference));
        
        let image_ref = self.create_reference(repository, reference)?;
        
        // Create a new client instance with the provided authentication for this operation
        let config = oci_client::client::ClientConfig::default();
        let client = oci_client::Client::new(config);
        
        // Authenticate the client for this specific operation
        if let Err(e) = client.auth(&image_ref, auth, oci_client::RegistryOperation::Push).await {
            self.logger.error(&format!("Failed to authenticate for manifest push operation: {}", e));
            return Err(RegistryError::Auth(format!("OCI client authentication failed: {}", e)));
        }
        
        // Determine the manifest content type
        use reqwest::header::HeaderValue;
        
        let content_type = if manifest.starts_with(b"{") {
            // Try to determine the manifest type
            if let Ok(manifest_str) = std::str::from_utf8(manifest) {
                if manifest_str.contains("\"mediaType\":\"application/vnd.oci.image.manifest.v1+json\"") {
                    HeaderValue::from_static("application/vnd.oci.image.manifest.v1+json")
                } else {
                    HeaderValue::from_static("application/vnd.docker.distribution.manifest.v2+json")
                }
            } else {
                HeaderValue::from_static("application/vnd.docker.distribution.manifest.v2+json")
            }
        } else {
            HeaderValue::from_static("application/vnd.docker.distribution.manifest.v2+json")
        };
        
        match client.push_manifest_raw(&image_ref, manifest.to_vec(), content_type).await {
            Ok(url) => {
                self.logger.verbose(&format!("Successfully pushed manifest to: {}", url));
                Ok(url)
            }
            Err(e) => {
                self.logger.error(&format!("Failed to push manifest: {}", e));
                Err(RegistryError::Network(format!("OCI client manifest push failed: {}", e)))
            }
        }
    }

    /// Push manifest using OCI client
    pub async fn push_manifest(&self, repository: &str, reference: &str, manifest: &[u8]) -> Result<String> {
        let auth = self.auth.as_ref().unwrap_or(&RegistryAuth::Anonymous);
        self.push_manifest_with_auth(repository, reference, manifest, auth).await
    }

    /// Check if blob exists using OCI client
    pub async fn blob_exists(&self, repository: &str, digest: &str) -> Result<bool> {
        self.blob_exists_with_auth(repository, digest, self.auth.as_ref().unwrap_or(&RegistryAuth::Anonymous)).await
    }

    /// Check if blob exists using OCI client with authentication
    pub async fn blob_exists_with_auth(&self, repository: &str, digest: &str, auth: &RegistryAuth) -> Result<bool> {
        self.logger.verbose(&format!("Checking blob {} existence in {} via OCI client", &digest[..16], repository));
        
        let image_ref = self.create_reference(repository, "latest")?;
        
        // Create a new client instance with the provided authentication for this operation
        let config = oci_client::client::ClientConfig::default();
        let client = oci_client::Client::new(config);
        
        // Authenticate the client for this specific operation
        if let Err(e) = client.auth(&image_ref, auth, oci_client::RegistryOperation::Pull).await {
            self.logger.error(&format!("Failed to authenticate for blob existence check: {}", e));
            return Err(RegistryError::Auth(format!("OCI client authentication failed: {}", e)));
        }
        
        // Try to pull just the blob info (head request) to check existence
        let mut blob_data = Vec::new();
        match client.pull_blob(&image_ref, digest, &mut blob_data).await {
            Ok(_) => {
                self.logger.verbose(&format!("Blob {} exists", &digest[..16]));
                Ok(true)
            }
            Err(e) => {
                // Check if it's a "not found" error vs other errors
                let error_msg = format!("{}", e);
                if error_msg.contains("404") || error_msg.contains("Not Found") || error_msg.contains("not found") {
                    self.logger.verbose(&format!("Blob {} does not exist", &digest[..16]));
                    Ok(false)
                } else {
                    self.logger.error(&format!("Failed to check blob existence: {}", e));
                    Err(RegistryError::Network(format!("OCI client blob existence check failed: {}", e)))
                }
            }
        }
    }

    /// Check if manifest exists using OCI client
    pub async fn manifest_exists(&self, repository: &str, reference: &str) -> Result<bool> {
        self.logger.verbose(&format!("Checking manifest {}:{} existence via OCI client", repository, reference));
        
        let image_ref = self.create_reference(repository, reference)?;
        let auth = self.auth.as_ref().unwrap_or(&RegistryAuth::Anonymous);
        
        // Try to fetch just the manifest digest to check existence
        match self.client.fetch_manifest_digest(&image_ref, auth).await {
            Ok(digest) => {
                self.logger.verbose(&format!("Manifest {}:{} exists with digest: {}", repository, reference, digest));
                Ok(true)
            }
            Err(e) => {
                // Check if it's a "not found" error vs other errors
                let error_msg = format!("{}", e);
                if error_msg.contains("404") || error_msg.contains("Not Found") || error_msg.contains("not found") {
                    self.logger.verbose(&format!("Manifest {}:{} does not exist", repository, reference));
                    Ok(false)
                } else {
                    self.logger.error(&format!("Failed to check manifest existence: {}", e));
                    Err(RegistryError::Network(format!("OCI client manifest existence check failed: {}", e)))
                }
            }
        }
    }

    /// List tags using OCI client
    pub async fn list_tags(&self, repository: &str) -> Result<Vec<String>> {
        self.logger.verbose(&format!("Listing tags for {} via OCI client", repository));
        
        let image_ref = self.create_reference(repository, "latest")?;
        let auth = self.auth.as_ref().unwrap_or(&RegistryAuth::Anonymous);
        
        match self.client.list_tags(&image_ref, auth, None, None).await {
            Ok(tag_response) => {
                self.logger.verbose(&format!("Successfully listed {} tags", tag_response.tags.len()));
                Ok(tag_response.tags)
            }
            Err(e) => {
                self.logger.error(&format!("Failed to list tags: {}", e));
                Err(RegistryError::Network(format!("OCI client tag listing failed: {}", e)))
            }
        }
    }
}

/// Trait defining OCI registry operations interface
#[async_trait]
pub trait OciRegistryOperations {
    async fn oci_pull_manifest(&self, repository: &str, reference: &str) -> Result<(Vec<u8>, String)>;
    async fn oci_pull_blob(&self, repository: &str, digest: &str) -> Result<Vec<u8>>;
    async fn oci_push_blob(&self, repository: &str, data: &[u8], digest: &str) -> Result<String>;
    async fn oci_push_manifest(&self, repository: &str, reference: &str, manifest: &[u8]) -> Result<String>;
    async fn oci_blob_exists(&self, repository: &str, digest: &str) -> Result<bool>;
    async fn oci_manifest_exists(&self, repository: &str, reference: &str) -> Result<bool>;
    async fn oci_list_tags(&self, repository: &str) -> Result<Vec<String>>;
}

/// Implement OCI operations directly on the adapter
#[async_trait]
impl OciRegistryOperations for OciClientAdapter {
    async fn oci_pull_manifest(&self, repository: &str, reference: &str) -> Result<(Vec<u8>, String)> {
        self.pull_manifest(repository, reference).await
    }

    async fn oci_pull_blob(&self, repository: &str, digest: &str) -> Result<Vec<u8>> {
        self.pull_blob(repository, digest).await
    }

    async fn oci_push_blob(&self, repository: &str, data: &[u8], digest: &str) -> Result<String> {
        self.push_blob(repository, data, digest).await
    }

    async fn oci_push_manifest(&self, repository: &str, reference: &str, manifest: &[u8]) -> Result<String> {
        self.push_manifest(repository, reference, manifest).await
    }

    async fn oci_blob_exists(&self, repository: &str, digest: &str) -> Result<bool> {
        self.blob_exists(repository, digest).await
    }

    async fn oci_manifest_exists(&self, repository: &str, reference: &str) -> Result<bool> {
        self.manifest_exists(repository, reference).await
    }

    async fn oci_list_tags(&self, repository: &str) -> Result<Vec<String>> {
        self.list_tags(repository).await
    }
}
