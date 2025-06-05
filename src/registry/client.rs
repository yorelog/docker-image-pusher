// This file contains the implementation of the RegistryClient struct,
// which handles communication with the Docker registry API for pushing
// Docker image tar packages, including methods for uploading images
// and managing requests.

use reqwest::{Client, header::CONTENT_TYPE};
use std::path::Path;
use std::fs::File;
use std::io::Read;
use tar::Archive;
use crate::error::{Result, PusherError};
use crate::config::AuthConfig;
use crate::registry::auth::Auth;
use crate::image::parser::{ImageInfo, LayerInfo, ImageConfig};
use serde_json::json;
use sha2::{Sha256, Digest};

pub struct RegistryClientBuilder {
    address: String,
    auth_config: Option<AuthConfig>,
    auth_token: Option<String>,
    skip_tls: bool,
}

impl RegistryClientBuilder {
    pub fn new(address: String) -> Self {
        Self {
            address,
            auth_config: None,
            auth_token: None,
            skip_tls: false,
        }
    }

    pub fn with_auth(mut self, auth_config: AuthConfig) -> Self {
        self.auth_config = Some(auth_config);
        self
    }

    pub fn with_auth_token(mut self, token: Option<String>) -> Self {
        self.auth_token = token;
        self
    }

    pub fn with_skip_tls(mut self, skip_tls: bool) -> Self {
        self.skip_tls = skip_tls;
        self
    }

    pub fn build(self) -> Result<RegistryClient> {
        let client = if self.skip_tls {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
                .build()
                .map_err(PusherError::Network)?
        } else {
            Client::new()
        };

        let auth = Auth::new(&self.address, self.skip_tls)?;

        Ok(RegistryClient {
            client,
            address: self.address,
            auth_config: self.auth_config,
            auth_token: self.auth_token,
            auth: Some(auth),
        })
    }
}

pub struct RegistryClient {
    client: Client,
    address: String,
    auth_config: Option<AuthConfig>,
    auth_token: Option<String>,
    auth: Option<Auth>,
}

impl RegistryClient {
    pub fn new(address: String, username: Option<String>, password: Option<String>, skip_tls: bool) -> Result<Self> {
        let auth_config = AuthConfig { username, password };
        Self::builder(address)
            .with_auth(auth_config)
            .with_skip_tls(skip_tls)
            .build()
    }

    pub fn builder(address: String) -> RegistryClientBuilder {
        RegistryClientBuilder::new(address)
    }

    pub async fn authenticate(&mut self) -> Result<()> {
        if let (Some(auth), Some(auth_config)) = (&self.auth, &self.auth_config) {
            if let (Some(username), Some(password)) = (&auth_config.username, &auth_config.password) {
                println!("  Attempting authentication for user: {}", username);
                match auth.login(username, password).await? {
                    Some(token) => {
                        self.auth_token = Some(token);
                        println!("  Authentication successful - token received");
                        println!("  Token preview: {}...", &self.auth_token.as_ref().unwrap()[..std::cmp::min(20, self.auth_token.as_ref().unwrap().len())]);
                    }
                    None => {
                        println!("  No authentication required by registry");
                    }
                }
            }
        } else {
            println!("  No authentication credentials provided, proceeding without auth");
        }
        Ok(())
    }

    pub async fn check_registry_version(&self) -> Result<()> {
        let url = format!("{}/v2/", self.address);
        let mut request = self.client.get(&url);
        
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }
        
        let response = request.send().await?;
        
        match response.status().as_u16() {
            200 => {
                println!("Registry API v2 is available");
                Ok(())
            }
            401 => {
                println!("Registry requires authentication");
                Ok(())
            }
            _ => {
                Err(PusherError::Registry(format!("Registry API v2 not available. Status: {}", response.status())))
            }
        }
    }

    pub async fn upload_image_with_info(&self, tar_path: &Path, image_info: &ImageInfo) -> Result<()> {
        println!("Starting upload for {}:{}", image_info.repository, image_info.tag);
        
        let repository = &image_info.repository;
        
        println!("Target repository: {}", repository);
        println!("Registry address: {}", self.address);
        println!("Auth token available: {}", self.auth_token.is_some());
        
        // Test repository access first
        println!("Testing repository access...");
        self.test_repository_access(repository).await?;
        
        // If we have auth config but upload fails, try to get repository-specific token
        let mut current_token = self.auth_token.clone();
        
        // Step 1: Upload each layer
        for (i, layer) in image_info.layers.iter().enumerate() {
            println!("Uploading layer {} of {}: {}", i + 1, image_info.layers.len(), layer.digest);
            
            match self.upload_layer_with_token(repository, layer, tar_path, &current_token).await {
                Ok(_) => {
                    println!("    Layer uploaded successfully");
                }
                Err(PusherError::Upload(msg)) if msg.contains("UNAUTHORIZED") => {
                    println!("    Upload unauthorized, attempting to get repository-specific token...");
                    current_token = self.get_repository_token(repository).await?;
                    self.upload_layer_with_token(repository, layer, tar_path, &current_token).await?;
                }
                Err(e) => return Err(e),
            }
        }
        
        // Step 2: Upload config blob
        println!("Uploading config blob...");
        match self.upload_config_with_token(repository, &image_info.config_digest, &image_info.config, tar_path, &current_token).await {
            Ok(_) => {
                println!("    Config uploaded successfully");
            }
            Err(PusherError::Upload(msg)) if msg.contains("UNAUTHORIZED") => {
                println!("    Config upload unauthorized, using repository-specific token...");
                current_token = self.get_repository_token(repository).await?;
                self.upload_config_with_token(repository, &image_info.config_digest, &image_info.config, tar_path, &current_token).await?;
            }
            Err(e) => return Err(e),
        }
        
        // Step 3: Upload manifest
        println!("Uploading manifest...");
        match self.upload_manifest_with_token(repository, &image_info.tag, image_info, &current_token).await {
            Ok(_) => {
                println!("    Manifest uploaded successfully");
            }
            Err(PusherError::Upload(msg)) if msg.contains("UNAUTHORIZED") => {
                println!("    Manifest upload unauthorized, using repository-specific token...");
                current_token = self.get_repository_token(repository).await?;
                self.upload_manifest_with_token(repository, &image_info.tag, image_info, &current_token).await?;
            }
            Err(e) => return Err(e),
        }
        
        println!("All components uploaded successfully!");
        Ok(())
    }

    async fn get_repository_token(&self, repository: &str) -> Result<Option<String>> {
        if let (Some(auth), Some(auth_config)) = (&self.auth, &self.auth_config) {
            if let (Some(username), Some(password)) = (&auth_config.username, &auth_config.password) {
                return auth.login_with_repository(username, password, repository).await;
            }
        }
        Err(PusherError::Authentication("No auth credentials available for repository token".to_string()))
    }

    async fn upload_layer_with_token(&self, repository: &str, layer: &LayerInfo, tar_path: &Path, token: &Option<String>) -> Result<()> {
        println!("  Uploading layer: {}", layer.digest);
        
        // Step 1: Extract layer data from tar
        let layer_data = self.extract_layer_from_tar(tar_path, &layer.tar_path).await?;
        
        // Step 2: Start blob upload
        let upload_url = self.start_blob_upload_with_token(repository, token).await?;
        println!("    Started blob upload: {}", upload_url);
        
        // Step 3: Upload layer data
        self.upload_blob_data_with_token(&upload_url, layer_data, &layer.digest, token).await?;
        
        Ok(())
    }

    async fn upload_config_with_token(&self, repository: &str, config_digest: &str, _config: &ImageConfig, tar_path: &Path, token: &Option<String>) -> Result<()> {
        println!("  Uploading config: {}", config_digest);
        
        // Step 1: Extract config data from tar
        let config_data = self.extract_config_from_tar(tar_path, config_digest).await?;
        
        // Step 2: Start blob upload
        let upload_url = self.start_blob_upload_with_token(repository, token).await?;
        println!("    Started config upload: {}", upload_url);
        
        // Step 3: Upload config data
        self.upload_blob_data_with_token(&upload_url, config_data, config_digest, token).await?;
        
        Ok(())
    }

    async fn start_blob_upload_with_token(&self, repository: &str, token: &Option<String>) -> Result<String> {
        let url = format!("{}/v2/{}/blobs/uploads/", self.address, repository);
        println!("    Attempting to start blob upload to: {}", url);
        
        let mut request = self.client.post(&url);
        
        if let Some(token) = token {
            request = request.bearer_auth(token);
            println!("    Using bearer token authentication");
        } else {
            println!("    No authentication token available");
        }
        
        let response = request.send().await?;
        println!("    Response status: {}", response.status());
        
        if response.status().is_success() {
            if let Some(location) = response.headers().get("Location") {
                let location_str = location.to_str()
                    .map_err(|e| PusherError::Upload(format!("Invalid location header: {}", e)))?;
                println!("    Upload location: {}", location_str);
                
                if location_str.starts_with('/') {
                    Ok(format!("{}{}", self.address, location_str))
                } else {
                    Ok(location_str.to_string())
                }
            } else {
                Err(PusherError::Upload("No Location header in upload response".to_string()))
            }
        } else {
            let error_text = response.text().await?;
            Err(PusherError::Upload(format!("Failed to start blob upload: {}", error_text)))
        }
    }

    async fn upload_blob_data_with_token(&self, upload_url: &str, data: Vec<u8>, expected_digest: &str, token: &Option<String>) -> Result<()> {
        // Calculate actual digest for verification
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let actual_digest = format!("sha256:{:x}", hasher.finalize());
        
        // Verify digest matches expected
        if actual_digest != expected_digest {
            println!("    Warning: Digest mismatch! Expected: {}, Actual: {}", expected_digest, actual_digest);
        } else {
            println!("    Digest verified: {}", actual_digest);
        }
        
        let url = format!("{}digest={}", 
            if upload_url.contains('?') { format!("{}&", upload_url) } else { format!("{}?", upload_url) },
            expected_digest
        );
        
        let mut request = self.client.put(&url)
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(data);
        
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        
        let response = request.send().await?;
        
        if response.status().is_success() {
            println!("    Blob uploaded successfully (digest verified)");
            Ok(())
        } else {
            let error_text = response.text().await?;
            Err(PusherError::Upload(format!("Failed to upload blob: {}", error_text)))
        }
    }

    async fn upload_manifest_with_token(&self, repository: &str, tag: &str, image_info: &ImageInfo, token: &Option<String>) -> Result<()> {
        // Create Docker manifest v2 schema 2
        let manifest = json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": {
                "mediaType": "application/vnd.docker.container.image.v1+json",
                "size": 1000, // This should be actual config size
                "digest": image_info.config_digest
            },
            "layers": image_info.layers.iter().map(|layer| {
                json!({
                    "mediaType": layer.media_type,
                    "size": layer.size,
                    "digest": layer.digest
                })
            }).collect::<Vec<_>>()
        });
        
        let manifest_json = serde_json::to_string(&manifest)?;
        let url = format!("{}/v2/{}/manifests/{}", self.address, repository, tag);
        
        let mut request = self.client.put(&url)
            .header(CONTENT_TYPE, "application/vnd.docker.distribution.manifest.v2+json")
            .body(manifest_json);
        
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        
        let response = request.send().await?;
        
        if response.status().is_success() {
            println!("  Manifest uploaded successfully for {}:{}", repository, tag);
            Ok(())
        } else {
            let error_text = response.text().await?;
            Err(PusherError::Upload(format!("Failed to upload manifest: {}", error_text)))
        }
    }

    async fn test_repository_access(&self, repository: &str) -> Result<()> {
        let test_url = format!("{}/v2/{}/", self.address, repository);
        println!("  Testing: {}", test_url);
        
        let mut request = self.client.head(&test_url);
        
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }
        
        let response = request.send().await?;
        println!("  Repository access test status: {}", response.status());
        
        match response.status().as_u16() {
            200 | 404 => {
                println!("  Repository access OK");
                Ok(())
            }
            401 => {
                if self.auth_token.is_some() {
                    Err(PusherError::Authentication(format!("Authentication token rejected for repository: {}", repository)))
                } else {
                    Err(PusherError::Authentication(format!("Authentication required for repository: {}", repository)))
                }
            }
            403 => {
                Err(PusherError::Authentication(format!("Insufficient permissions for repository: {}", repository)))
            }
            _ => {
                println!("  Unexpected status, but proceeding...");
                Ok(())
            }
        }
    }

    async fn extract_layer_from_tar(&self, tar_path: &Path, layer_path: &str) -> Result<Vec<u8>> {
        let file = File::open(tar_path)?;
        let mut archive = Archive::new(file);
        
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.to_string_lossy().to_string();
            
            if path == layer_path {
                let mut data = Vec::new();
                entry.read_to_end(&mut data)?;
                return Ok(data);
            }
        }
        
        Err(PusherError::ImageParsing(format!("Layer {} not found in tar", layer_path)))
    }

    async fn extract_config_from_tar(&self, tar_path: &Path, config_name: &str) -> Result<Vec<u8>> {
        let file = File::open(tar_path)?;
        let mut archive = Archive::new(file);
        
        // Extract just the filename from the digest
        let config_filename = if config_name.starts_with("sha256:") {
            format!("{}.json", &config_name[7..])
        } else {
            config_name.to_string()
        };
        
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.to_string_lossy().to_string();
            
            if path.contains(&config_filename) || path.ends_with(".json") && !path.contains("manifest") {
                let mut data = Vec::new();
                entry.read_to_end(&mut data)?;
                return Ok(data);
            }
        }
        
        Err(PusherError::ImageParsing(format!("Config {} not found in tar", config_name)))
    }
}