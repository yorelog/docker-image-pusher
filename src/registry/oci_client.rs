//! OCI client adapter for standardized container registry operations
//!
//! This module provides an adapter around the oci-client crate to integrate
//! OCI-compliant registry operations into our registry client.

use crate::cli::config::AuthConfig;
use crate::error::{RegistryError, Result};
use crate::logging::Logger;
use crate::registry::token_manager::TokenManager;
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
    token_manager: Option<TokenManager>,
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
            token_manager: None,
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

    /// Set the token manager for automatic token refresh
    pub fn with_token_manager(mut self, token_manager: Option<TokenManager>) -> Self {
        self.token_manager = token_manager;
        self
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

    /// Static version of create_reference for use in static methods
    fn create_reference_static(repository: &str, reference: &str, registry_url: &str) -> Result<Reference> {
        let ref_str = if reference.starts_with("sha256:") {
            format!("{}/{}@{}", registry_url.trim_start_matches("https://").trim_start_matches("http://"), repository, reference)
        } else {
            format!("{}/{}:{}", registry_url.trim_start_matches("https://").trim_start_matches("http://"), repository, reference)
        };
        
        Reference::try_from(ref_str.as_str())
            .map_err(|e| RegistryError::Validation(format!("Invalid reference '{}': {}", ref_str, e)))
    }

    /// Pull manifest using OCI client
    pub async fn pull_manifest(&self, repository: &str, reference: &str) -> Result<(Vec<u8>, String)> {
        self.logger.verbose(&format!("Pulling manifest {}:{} via OCI client", repository, reference));
        
        // If we have a token manager, use retry logic for automatic token refresh
        if let Some(ref token_manager) = self.token_manager {
            let repository_clone = repository.to_string();
            let reference_clone = reference.to_string();
            let logger_clone = self.logger.clone();
            let registry_url_clone = self.registry_url.clone();
            
            return token_manager.execute_with_retry(|token| {
                let repository = repository_clone.clone();
                let reference = reference_clone.clone();
                let logger = logger_clone.clone();
                let registry_url = registry_url_clone.clone();
                
                Box::pin(async move {
                    // Use the refreshed token if available, otherwise use anonymous auth
                    let current_auth = if let Some(token_str) = token {
                        RegistryAuth::Bearer(token_str)
                    } else {
                        RegistryAuth::Anonymous
                    };
                    
                    Self::execute_manifest_pull(&repository, &reference, &current_auth, &logger, &registry_url).await
                })
            }).await;
        }
        
        // Fallback to direct pull without retry if no token manager
        let auth = self.auth.as_ref().unwrap_or(&RegistryAuth::Anonymous);
        Self::execute_manifest_pull(repository, reference, auth, &self.logger, &self.registry_url).await
    }
    
    /// Execute the actual manifest pull operation
    async fn execute_manifest_pull(
        repository: &str,
        reference: &str,
        auth: &RegistryAuth,
        logger: &Logger,
        registry_url: &str,
    ) -> Result<(Vec<u8>, String)> {
        let image_ref = Self::create_reference_static(repository, reference, registry_url)?;
        
        // Create a new client instance for consistent authentication pattern
        let config = oci_client::client::ClientConfig::default();
        let client = oci_client::Client::new(config);
        
        match client.pull_manifest_raw(&image_ref, auth, &["application/vnd.docker.distribution.manifest.v2+json", "application/vnd.oci.image.manifest.v1+json"]).await {
            Ok((manifest_bytes, digest)) => {
                logger.verbose(&format!("Successfully pulled manifest with digest: {}", digest));
                Ok((manifest_bytes, digest))
            }
            Err(e) => {
                let error_msg = format!("{}", e);
                // Check for 401 errors to enable token refresh retry
                if error_msg.contains("401") || error_msg.contains("Unauthorized") {
                    logger.warning(&format!("Received 401 error during manifest pull: {}", error_msg));
                    Err(RegistryError::Registry(format!("401 Unauthorized: {}", error_msg)))
                } else {
                    logger.error(&format!("Failed to pull manifest: {}", e));
                    Err(RegistryError::Network(format!("OCI client manifest pull failed: {}", e)))
                }
            }
        }
    }

    /// Pull blob using OCI client with authentication
    pub async fn pull_blob_with_auth(&self, repository: &str, digest: &str, auth: &RegistryAuth) -> Result<Vec<u8>> {
        self.logger.verbose(&format!("Pulling blob {} from {} via OCI client with explicit auth", &digest[..16], repository));
        
        // If we have a token manager, use retry logic for automatic token refresh
        if let Some(ref token_manager) = self.token_manager {
            let repository_clone = repository.to_string();
            let digest_clone = digest.to_string();
            let logger_clone = self.logger.clone();
            let registry_url_clone = self.registry_url.clone();
            
            return token_manager.execute_with_retry(|token| {
                let repository = repository_clone.clone();
                let digest = digest_clone.clone();
                let logger = logger_clone.clone();
                let registry_url = registry_url_clone.clone();
                
                Box::pin(async move {
                    // Use the refreshed token if available, otherwise use anonymous auth
                    let current_auth = if let Some(token_str) = token {
                        RegistryAuth::Bearer(token_str)
                    } else {
                        RegistryAuth::Anonymous
                    };
                    
                    Self::execute_blob_pull(&repository, &digest, &current_auth, &logger, &registry_url).await
                })
            }).await;
        }
        
        // Fallback to direct pull without retry if no token manager
        Self::execute_blob_pull(repository, digest, auth, &self.logger, &self.registry_url).await
    }
    
    /// Execute the actual blob pull operation
    async fn execute_blob_pull(
        repository: &str,
        digest: &str,
        auth: &RegistryAuth,
        logger: &Logger,
        registry_url: &str,
    ) -> Result<Vec<u8>> {
        let start_time = std::time::Instant::now();
        logger.info(&format!("⬇️  Downloading blob {} from {}", &digest[..16], repository));
        
        let image_ref = Self::create_reference_static(repository, "latest", registry_url)?;
        
        // Create a new client instance with the provided authentication for this operation
        let config = oci_client::client::ClientConfig::default();
        let client = oci_client::Client::new(config);
        
        // Authenticate the client for this specific operation
        if let Err(e) = client.auth(&image_ref, auth, oci_client::RegistryOperation::Pull).await {
            logger.error(&format!("Failed to authenticate for pull operation: {}", e));
            return Err(RegistryError::Auth(format!("OCI client authentication failed: {}", e)));
        }
        
        let mut blob_data = Vec::new();
        match client.pull_blob(&image_ref, digest, &mut blob_data).await {
            Ok(_) => {
                let elapsed = start_time.elapsed();
                let size = blob_data.len();
                let speed = if elapsed.as_secs() > 0 {
                    size as f64 / elapsed.as_secs_f64()
                } else {
                    size as f64
                };
                
                logger.success(&format!(
                    "✅ Downloaded blob {} ({}) in {} at {}/s",
                    &digest[..16],
                    Self::format_size_static(size),
                    Self::format_duration_static(elapsed),
                    Self::format_size_static(speed as usize)
                ));
                Ok(blob_data)
            }
            Err(e) => {
                let elapsed = start_time.elapsed();
                let error_msg = format!("{}", e);
                // Check for 401 errors to enable token refresh retry
                if error_msg.contains("401") || error_msg.contains("Unauthorized") {
                    logger.warning(&format!("Received 401 error during blob pull after {}: {}", 
                        Self::format_duration_static(elapsed), error_msg));
                    Err(RegistryError::Registry(format!("401 Unauthorized: {}", error_msg)))
                } else {
                    logger.error(&format!("❌ Failed to download blob {} after {}: {}", 
                        &digest[..16], Self::format_duration_static(elapsed), e));
                    Err(RegistryError::Network(format!("OCI client blob pull failed: {}", e)))
                }
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
        
        // If we have a token manager, use retry logic for automatic token refresh
        if let Some(ref token_manager) = self.token_manager {
            let repository_clone = repository.to_string();
            let data_clone = data.to_vec();
            let digest_clone = digest.to_string();
            let logger_clone = self.logger.clone();
            let registry_url_clone = self.registry_url.clone();
            
            return token_manager.execute_with_retry(|token| {
                let repository = repository_clone.clone();
                let data = data_clone.clone();
                let digest = digest_clone.clone();
                let logger = logger_clone.clone();
                let registry_url = registry_url_clone.clone();
                
                Box::pin(async move {
                    // Use the refreshed token if available, otherwise use anonymous auth
                    let current_auth = if let Some(token_str) = token {
                        RegistryAuth::Bearer(token_str)
                    } else {
                        RegistryAuth::Anonymous
                    };
                    
                    Self::execute_blob_push(&repository, &data, &digest, &current_auth, &logger, &registry_url).await
                })
            }).await;
        }
        
        // Fallback to direct push without retry if no token manager
        Self::execute_blob_push(repository, data, digest, auth, &self.logger, &self.registry_url).await
    }
    
    /// Execute the actual blob push operation
    async fn execute_blob_push(
        repository: &str,
        data: &[u8], 
        digest: &str,
        auth: &RegistryAuth,
        logger: &Logger,
        registry_url: &str,
    ) -> Result<String> {
        let start_time = std::time::Instant::now();
        let size = data.len();
        logger.info(&format!("⬆️  Uploading blob {} ({}) to {}", 
            &digest[..16], Self::format_size_static(size), repository));
        
        let image_ref = Self::create_reference_static(repository, "latest", registry_url)?;
        
        // Create a new client instance with the provided authentication for this operation
        let config = oci_client::client::ClientConfig::default();
        let client = oci_client::Client::new(config);
        
        // Authenticate the client for this specific operation
        if let Err(e) = client.auth(&image_ref, auth, oci_client::RegistryOperation::Push).await {
            logger.error(&format!("Failed to authenticate for push operation: {}", e));
            return Err(RegistryError::Auth(format!("OCI client authentication failed: {}", e)));
        }

        match client.push_blob(&image_ref, data, digest).await {
            Ok(url) => {
                let elapsed = start_time.elapsed();
                let speed = if elapsed.as_secs() > 0 {
                    size as f64 / elapsed.as_secs_f64()
                } else {
                    size as f64
                };
                
                logger.success(&format!(
                    "✅ Uploaded blob {} ({}) in {} at {}/s",
                    &digest[..16],
                    Self::format_size_static(size),
                    Self::format_duration_static(elapsed),
                    Self::format_size_static(speed as usize)
                ));
                Ok(url)
            }
            Err(e) => {
                let elapsed = start_time.elapsed();
                let error_msg = format!("{}", e);
                // Check for 401 errors to enable token refresh retry
                if error_msg.contains("401") || error_msg.contains("Unauthorized") {
                    logger.warning(&format!("Received 401 error during blob push after {}: {}", 
                        Self::format_duration_static(elapsed), error_msg));
                    Err(RegistryError::Registry(format!("401 Unauthorized: {}", error_msg)))
                } else {
                    logger.error(&format!("❌ Failed to upload blob {} ({}) after {}: {}", 
                        &digest[..16], Self::format_size_static(size), 
                        Self::format_duration_static(elapsed), e));
                    Err(RegistryError::Network(format!("OCI client blob push failed: {}", e)))
                }
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
        
        // If we have a token manager, use retry logic for automatic token refresh
        if let Some(ref token_manager) = self.token_manager {
            let repository_clone = repository.to_string();
            let reference_clone = reference.to_string();
            let manifest_clone = manifest.to_vec();
            let logger_clone = self.logger.clone();
            let registry_url_clone = self.registry_url.clone();
            
            return token_manager.execute_with_retry(|token| {
                let repository = repository_clone.clone();
                let reference = reference_clone.clone();
                let manifest = manifest_clone.clone();
                let logger = logger_clone.clone();
                let registry_url = registry_url_clone.clone();
                
                Box::pin(async move {
                    // Use the refreshed token if available, otherwise use anonymous auth
                    let current_auth = if let Some(token_str) = token {
                        RegistryAuth::Bearer(token_str)
                    } else {
                        RegistryAuth::Anonymous
                    };
                    
                    Self::execute_manifest_push(&repository, &reference, &manifest, &current_auth, &logger, &registry_url).await
                })
            }).await;
        }
        
        // Fallback to direct push without retry if no token manager
        Self::execute_manifest_push(repository, reference, manifest, auth, &self.logger, &self.registry_url).await
    }
    
    /// Execute the actual manifest push operation
    async fn execute_manifest_push(
        repository: &str,
        reference: &str,
        manifest: &[u8],
        auth: &RegistryAuth,
        logger: &Logger,
        registry_url: &str,
    ) -> Result<String> {
        let image_ref = Self::create_reference_static(repository, reference, registry_url)?;
        
        // Create a new client instance with the provided authentication for this operation
        let config = oci_client::client::ClientConfig::default();
        let client = oci_client::Client::new(config);
        
        // Authenticate the client for this specific operation
        if let Err(e) = client.auth(&image_ref, auth, oci_client::RegistryOperation::Push).await {
            logger.error(&format!("Failed to authenticate for manifest push operation: {}", e));
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
                logger.verbose(&format!("Successfully pushed manifest to: {}", url));
                Ok(url)
            }
            Err(e) => {
                let error_msg = format!("{}", e);
                // Check for 401 errors to enable token refresh retry
                if error_msg.contains("401") || error_msg.contains("Unauthorized") {
                    logger.warning(&format!("Received 401 error during manifest push: {}", error_msg));
                    Err(RegistryError::Registry(format!("401 Unauthorized: {}", error_msg)))
                } else {
                    logger.error(&format!("Failed to push manifest: {}", e));
                    Err(RegistryError::Network(format!("OCI client manifest push failed: {}", e)))
                }
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
        
        // If we have a token manager, use retry logic for automatic token refresh
        if let Some(ref token_manager) = self.token_manager {
            let repository_clone = repository.to_string();
            let digest_clone = digest.to_string();
            let logger_clone = self.logger.clone();
            let registry_url_clone = self.registry_url.clone();
            
            return token_manager.execute_with_retry(|token| {
                let repository = repository_clone.clone();
                let digest = digest_clone.clone();
                let logger = logger_clone.clone();
                let registry_url = registry_url_clone.clone();
                
                Box::pin(async move {
                    // Use the refreshed token if available, otherwise use anonymous auth
                    let current_auth = if let Some(token_str) = token {
                        RegistryAuth::Bearer(token_str)
                    } else {
                        RegistryAuth::Anonymous
                    };
                    
                    Self::execute_blob_exists_check(&repository, &digest, &current_auth, &logger, &registry_url).await
                })
            }).await;
        }
        
        // Fallback to direct check without retry if no token manager
        Self::execute_blob_exists_check(repository, digest, auth, &self.logger, &self.registry_url).await
    }
    
    /// Execute the actual blob existence check
    async fn execute_blob_exists_check(
        repository: &str,
        digest: &str,
        auth: &RegistryAuth,
        logger: &Logger,
        registry_url: &str,
    ) -> Result<bool> {
        let image_ref = Self::create_reference_static(repository, "latest", registry_url)?;
        
        // Create a new client instance with the provided authentication for this operation
        let config = oci_client::client::ClientConfig::default();
        let client = oci_client::Client::new(config);
        
        // Authenticate the client for this specific operation
        if let Err(e) = client.auth(&image_ref, auth, oci_client::RegistryOperation::Pull).await {
            logger.error(&format!("Failed to authenticate for blob existence check: {}", e));
            return Err(RegistryError::Auth(format!("OCI client authentication failed: {}", e)));
        }
        
        // Try to pull just the blob info (head request) to check existence
        let mut blob_data = Vec::new();
        match client.pull_blob(&image_ref, digest, &mut blob_data).await {
            Ok(_) => {
                logger.verbose(&format!("Blob {} exists", &digest[..16]));
                Ok(true)
            }
            Err(e) => {
                // Check if it's a "not found" error vs other errors
                let error_msg = format!("{}", e);
                if error_msg.contains("404") || error_msg.contains("Not Found") || error_msg.contains("not found") {
                    logger.verbose(&format!("Blob {} does not exist", &digest[..16]));
                    Ok(false)
                } else if error_msg.contains("401") || error_msg.contains("Unauthorized") {
                    logger.warning(&format!("Received 401 error during blob existence check: {}", error_msg));
                    Err(RegistryError::Registry(format!("401 Unauthorized: {}", error_msg)))
                } else {
                    logger.error(&format!("Failed to check blob existence: {}", e));
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



    /// Static version of format_size for use in static methods
    fn format_size_static(bytes: usize) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        const THRESHOLD: f64 = 1024.0;
        
        let mut size = bytes as f64;
        let mut unit_index = 0;
        
        while size >= THRESHOLD && unit_index < UNITS.len() - 1 {
            size /= THRESHOLD;
            unit_index += 1;
        }
        
        if unit_index == 0 {
            format!("{} {}", bytes, UNITS[unit_index])
        } else {
            format!("{:.2} {}", size, UNITS[unit_index])
        }
    }



    /// Static version of format_duration for use in static methods
    fn format_duration_static(duration: std::time::Duration) -> String {
        let total_seconds = duration.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        let millis = duration.subsec_millis();

        if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else if seconds > 0 {
            format!("{}.{:03}s", seconds, millis)
        } else {
            format!("{}ms", millis)
        }
    }

    /// Batch pull multiple blobs with progress tracking and remaining count display
    pub async fn batch_pull_blobs(
        &self,
        digests: &[String],
        repository: &str,
    ) -> Result<Vec<(String, Result<Vec<u8>>)>> {
        let total_count = digests.len();
        let mut results = Vec::with_capacity(total_count);
        
        self.logger.info(&format!(
            "⬇️  Starting batch blob download: {} blobs from {}",
            total_count, repository
        ));
        
        let start_time = std::time::Instant::now();
        let mut total_downloaded: u64 = 0;
        
        for (index, digest) in digests.iter().enumerate() {
            let remaining = total_count - index;
            let blob_start_time = std::time::Instant::now();
            
            self.logger.info(&format!(
                "📥 [{}/{}] Downloading blob {} (remaining: {})",
                index + 1, total_count, &digest[..16], remaining - 1
            ));
            
            match self.pull_blob(repository, digest).await {
                Ok(data) => {
                    let blob_elapsed = blob_start_time.elapsed();
                    let blob_size = data.len() as u64;
                    total_downloaded += blob_size;
                    
                    let speed = if blob_elapsed.as_secs_f64() > 0.0 {
                        blob_size as f64 / blob_elapsed.as_secs_f64()
                    } else {
                        blob_size as f64
                    };
                    
                    self.logger.success(&format!(
                        "✅ Downloaded blob {} ({}) in {} at {}/s",
                        &digest[..16],
                        Self::format_size_static(blob_size as usize),
                        Self::format_duration_static(blob_elapsed),
                        Self::format_size_static(speed as usize)
                    ));
                    
                    results.push((digest.clone(), Ok(data)));
                }
                Err(e) => {
                    let blob_elapsed = blob_start_time.elapsed();
                    self.logger.error(&format!(
                        "❌ Failed to download blob {} after {}: {}",
                        &digest[..16],
                        Self::format_duration_static(blob_elapsed),
                        e
                    ));
                    results.push((digest.clone(), Err(e)));
                }
            }
        }
        
        let total_elapsed = start_time.elapsed();
        let successful_count = results.iter().filter(|(_, r)| r.is_ok()).count();
        let avg_speed = if total_elapsed.as_secs_f64() > 0.0 {
            total_downloaded as f64 / total_elapsed.as_secs_f64()
        } else {
            total_downloaded as f64
        };
        
        if successful_count == total_count {
            self.logger.success(&format!(
                "🎉 Batch download completed: {}/{} blobs ({}) in {} at {}/s",
                successful_count, total_count,
                Self::format_size_static(total_downloaded as usize),
                Self::format_duration_static(total_elapsed),
                Self::format_size_static(avg_speed as usize)
            ));
        } else {
            self.logger.warning(&format!(
                "⚠️  Batch download partially completed: {}/{} blobs successful",
                successful_count, total_count
            ));
        }
        
        Ok(results)
    }
    
    /// Batch push multiple blobs with progress tracking and remaining count display
    pub async fn batch_push_blobs(
        &self,
        blobs: &[(Vec<u8>, String)], // (data, digest)
        repository: &str,
    ) -> Result<Vec<(String, Result<String>)>> {
        let total_count = blobs.len();
        let mut results = Vec::with_capacity(total_count);
        
        let total_size: u64 = blobs.iter().map(|(data, _)| data.len() as u64).sum();
        
        self.logger.info(&format!(
            "⬆️  Starting batch blob upload: {} blobs ({}) to {}",
            total_count, Self::format_size_static(total_size as usize), repository
        ));
        
        let start_time = std::time::Instant::now();
        let mut total_uploaded: u64 = 0;
        
        for (index, (data, digest)) in blobs.iter().enumerate() {
            let remaining = total_count - index;
            let blob_start_time = std::time::Instant::now();
            let blob_size = data.len() as u64;
            
            self.logger.info(&format!(
                "📤 [{}/{}] Uploading blob {} ({}) (remaining: {})",
                index + 1, total_count, &digest[..16], 
                Self::format_size_static(blob_size as usize),
                remaining - 1
            ));
            
            match self.push_blob(repository, data, digest).await {
                Ok(url) => {
                    let blob_elapsed = blob_start_time.elapsed();
                    total_uploaded += blob_size;
                    
                    let speed = if blob_elapsed.as_secs_f64() > 0.0 {
                        blob_size as f64 / blob_elapsed.as_secs_f64()
                    } else {
                        blob_size as f64
                    };
                    
                    self.logger.success(&format!(
                        "✅ Uploaded blob {} ({}) in {} at {}/s",
                        &digest[..16],
                        Self::format_size_static(blob_size as usize),
                        Self::format_duration_static(blob_elapsed),
                        Self::format_size_static(speed as usize)
                    ));
                    
                    results.push((digest.clone(), Ok(url)));
                }
                Err(e) => {
                    let blob_elapsed = blob_start_time.elapsed();
                    self.logger.error(&format!(
                        "❌ Failed to upload blob {} ({}) after {}: {}",
                        &digest[..16],
                        Self::format_size_static(blob_size as usize),
                        Self::format_duration_static(blob_elapsed),
                        e
                    ));
                    results.push((digest.clone(), Err(e)));
                }
            }
        }
        
        let total_elapsed = start_time.elapsed();
        let successful_count = results.iter().filter(|(_, r)| r.is_ok()).count();
        let avg_speed = if total_elapsed.as_secs_f64() > 0.0 {
            total_uploaded as f64 / total_elapsed.as_secs_f64()
        } else {
            total_uploaded as f64
        };
        
        if successful_count == total_count {
            self.logger.success(&format!(
                "🎉 Batch upload completed: {}/{} blobs ({}) in {} at {}/s",
                successful_count, total_count,
                Self::format_size_static(total_uploaded as usize),
                Self::format_duration_static(total_elapsed),
                Self::format_size_static(avg_speed as usize)
            ));
        } else {
            self.logger.warning(&format!(
                "⚠️  Batch upload partially completed: {}/{} blobs successful",
                successful_count, total_count
            ));
        }
        
        Ok(results)
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
