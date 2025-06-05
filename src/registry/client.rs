//! Enhanced registry client with better configuration and error handling

use crate::error::{Result, PusherError};
use crate::output::OutputManager;
use crate::config::AuthConfig;
use crate::registry::auth::Auth;
use reqwest::Client;
use std::time::Duration;

pub struct RegistryClient {
    client: Client,
    auth: Auth,
    address: String,
    output: OutputManager,
}

#[derive(Debug)]
pub struct RegistryClientBuilder {
    address: String,
    auth_config: Option<AuthConfig>,
    timeout: u64,
    skip_tls: bool,
    verbose: bool,
}

impl RegistryClientBuilder {
    pub fn new(address: String) -> Self {
        Self {
            address,
            auth_config: None,
            timeout: 7200, // 2 hours default
            skip_tls: false,
            verbose: false,
        }
    }

    pub fn with_auth(mut self, auth_config: Option<AuthConfig>) -> Self {
        self.auth_config = auth_config;
        self
    }

    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_skip_tls(mut self, skip_tls: bool) -> Self {
        self.skip_tls = skip_tls;
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn build(self) -> Result<RegistryClient> {
        let output = OutputManager::new(self.verbose);
        output.verbose("Building HTTP client...");
        
        let client_builder = if self.skip_tls {
            output.verbose("TLS verification disabled");
            Client::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
        } else {
            output.verbose("TLS verification enabled");
            Client::builder()
        };
        
        let client = client_builder
            .timeout(Duration::from_secs(self.timeout))
            .connect_timeout(Duration::from_secs(60))
            .read_timeout(Duration::from_secs(3600))
            .pool_idle_timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(10)
            .user_agent("docker-image-pusher/1.0")
            .build()
            .map_err(|e| {
                output.error(&format!("Failed to build HTTP client: {}", e));
                PusherError::Network(e.to_string())
            })?;
        
        output.verbose("HTTP client built successfully");
        
        let auth = Auth::new(&self.address, self.skip_tls)?;
        
        Ok(RegistryClient {
            client,
            auth,
            address: self.address,
            output,
        })
    }
}

impl RegistryClient {
    pub async fn test_connectivity(&self) -> Result<()> {
        self.output.verbose("Testing registry connectivity...");
        
        let url = format!("{}/v2/", self.address);
        let response = self.client.get(&url).send().await
            .map_err(|e| PusherError::Network(format!("Failed to connect to registry: {}", e)))?;
        
        self.output.verbose(&format!("Registry response status: {}", response.status()));
        
        if response.status().is_success() || response.status() == 401 {
            // 401 is expected for registries that require authentication
            self.output.verbose("Registry connectivity test passed");
            Ok(())
        } else {
            Err(PusherError::Registry(format!(
                "Registry connectivity test failed with status: {}", 
                response.status()
            )))
        }
    }

    pub async fn authenticate(&self, auth_config: &AuthConfig) -> Result<Option<String>> {
        self.output.verbose("Authenticating with registry...");
        
        let token = self.auth.login(&auth_config.username, &auth_config.password, &self.output).await?;
        
        if token.is_some() {
            self.output.success("Authentication successful");
        } else {
            self.output.info("No authentication required");
        }
        
        Ok(token)
    }

    pub async fn check_blob_exists(&self, digest: &str, repository: &str, token: Option<&str>) -> Result<bool> {
        let url = format!("{}/v2/{}/blobs/{}", self.address, repository, digest);
        
        self.output.detail(&format!("Checking blob existence: {}", &digest[..16]));
        
        let mut request = self.client.head(&url);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }
        
        let response = request.send().await
            .map_err(|e| PusherError::Network(format!("Failed to check blob existence: {}", e)))?;
        
        let exists = response.status().is_success();
        self.output.detail(&format!("Blob {} exists: {}", &digest[..16], exists));
        
        Ok(exists)
    }

    pub async fn upload_blob(&self, data: Vec<u8>, repository: &str, _token: Option<&str>) -> Result<String> {
        // TODO: Implement blob upload
        self.output.info(&format!("Would upload blob of {} to {}", 
            self.output.format_size(data.len() as u64), repository));
        
        // Placeholder implementation
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let digest = format!("sha256:{:x}", hasher.finalize());
        
        Ok(digest)
    }

    pub async fn upload_manifest(&self, manifest: &str, repository: &str, tag: &str, _token: Option<&str>) -> Result<()> {
        // TODO: Implement manifest upload
        self.output.info(&format!("Would upload manifest for {}:{}", repository, tag));
        self.output.verbose(&format!("Manifest content ({}): {}", 
            self.output.format_size(manifest.len() as u64), 
            if manifest.len() > 200 { 
                format!("{}...", &manifest[..200]) 
            } else { 
                manifest.to_string() 
            }));
        
        Ok(())
    }

    pub fn get_output_manager(&self) -> &OutputManager {
        &self.output
    }
}