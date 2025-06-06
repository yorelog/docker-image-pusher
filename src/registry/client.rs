//! Enhanced registry client with better configuration and error handling

use crate::config::AuthConfig;
use crate::error::handlers::NetworkErrorHandler;
use crate::error::{PusherError, Result};
use crate::output::OutputManager;
use crate::registry::auth::Auth;
use reqwest::Client;
use std::time::Duration;

#[derive(Clone)] // Add Clone derive
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
        let response =
            self.client.get(&url).send().await.map_err(|e| {
                PusherError::Network(format!("Failed to connect to registry: {}", e))
            })?;

        self.output
            .verbose(&format!("Registry response status: {}", response.status()));

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

    pub async fn check_blob_exists(&self, digest: &str, repository: &str) -> Result<bool> {
        // Ensure digest has proper sha256: prefix
        let normalized_digest = if digest.starts_with("sha256:") {
            digest.to_string()
        } else {
            format!("sha256:{}", digest)
        };

        let url = format!(
            "{}/v2/{}/blobs/{}",
            self.address, repository, normalized_digest
        );

        self.output.detail(&format!(
            "Checking blob existence: {}",
            &normalized_digest[..23]
        ));

        // Use HEAD request to check existence without downloading
        let request = self.client.head(&url);

        let response = request.send().await.map_err(|e| {
            self.output
                .warning(&format!("Failed to check blob existence: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "blob existence check")
        })?;

        let status = response.status();

        match status.as_u16() {
            200 => {
                self.output
                    .detail(&format!("Blob {} exists", &normalized_digest[..16]));
                Ok(true)
            }
            404 => {
                self.output
                    .detail(&format!("Blob {} does not exist", &normalized_digest[..16]));
                Ok(false)
            }
            401 => {
                self.output
                    .warning("Authentication required for blob check");
                // Assume blob doesn't exist if we can't authenticate to check
                Ok(false)
            }
            403 => {
                self.output.warning("Permission denied for blob check");
                // Assume blob doesn't exist if we can't check permissions
                Ok(false)
            }
            _ => {
                self.output.warning(&format!(
                    "Unexpected status {} when checking blob existence",
                    status
                ));
                // On other errors, assume blob doesn't exist to be safe
                Ok(false)
            }
        }
    }

    pub async fn authenticate(&self, auth_config: &AuthConfig) -> Result<Option<String>> {
        self.output.verbose("Authenticating with registry...");

        let token = self
            .auth
            .login(&auth_config.username, &auth_config.password, &self.output)
            .await?;

        if token.is_some() {
            self.output.success("Authentication successful");
        } else {
            self.output.info("No authentication required");
        }

        Ok(token)
    }

    pub async fn authenticate_for_repository(
        &self,
        auth_config: &AuthConfig,
        repository: &str,
    ) -> Result<Option<String>> {
        self.output.verbose(&format!(
            "Authenticating for repository access: {}",
            repository
        ));

        let token = self
            .auth
            .get_repository_token(
                &auth_config.username,
                &auth_config.password,
                repository,
                &self.output,
            )
            .await?;

        if token.is_some() {
            self.output.success(&format!(
                "Repository authentication successful for: {}",
                repository
            ));
        } else {
            self.output
                .info("No repository-specific authentication required");
        }

        Ok(token)
    }

    pub async fn upload_blob(&self, data: &[u8], digest: &str, repository: &str) -> Result<String> {
        self.output.info(&format!(
            "Uploading blob {} ({}) to {}",
            &digest[..16],
            self.output.format_size(data.len() as u64),
            repository
        ));

        // Step 1: Start upload session
        let upload_url = self.start_upload_session(repository).await?;

        // Step 2: Upload data
        let upload_response = self
            .client
            .put(&format!("{}?digest={}", upload_url, digest))
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", data.len().to_string())
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| PusherError::Network(format!("Failed to upload blob: {}", e)))?;

        if upload_response.status().is_success() {
            self.output
                .success(&format!("Blob {} uploaded successfully", &digest[..16]));
            Ok(digest.to_string())
        } else {
            // Store status before consuming response
            let status = upload_response.status();
            let error_text = upload_response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(PusherError::Upload(format!(
                "Blob upload failed (status {}): {}",
                status, error_text
            )))
        }
    }

    pub async fn start_upload_session(&self, repository: &str) -> Result<String> {
        self.start_upload_session_with_token(repository, &None)
            .await
    }

    pub async fn start_upload_session_with_token(
        &self,
        repository: &str,
        token: &Option<String>,
    ) -> Result<String> {
        let url = format!("{}/v2/{}/blobs/uploads/", self.address, repository);

        self.output
            .detail(&format!("Starting upload session for {}", repository));

        let mut request = self.client.post(&url);

        if let Some(token) = token {
            request = request.bearer_auth(token);
            self.output
                .detail("Using authentication token for upload session");
        }

        let response = request
            .send()
            .await
            .map_err(|e| PusherError::Network(format!("Failed to start upload session: {}", e)))?;

        if response.status() == 202 {
            // Extract upload URL from Location header
            let location = response
                .headers()
                .get("Location")
                .and_then(|h| h.to_str().ok())
                .ok_or_else(|| {
                    PusherError::Registry(
                        "No Location header in upload session response".to_string(),
                    )
                })?;

            // Convert relative URL to absolute if needed
            let upload_url = if location.starts_with("http") {
                location.to_string()
            } else {
                format!("{}{}", self.address, location)
            };

            self.output
                .detail(&format!("Upload session started: {}", &upload_url[..50]));
            Ok(upload_url)
        } else {
            // Store status before consuming response
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());

            let error_msg = match status.as_u16() {
                401 => format!(
                    "Unauthorized to access repository: {} - {}",
                    repository, error_text
                ),
                403 => format!(
                    "Forbidden: insufficient permissions for repository: {} - {}",
                    repository, error_text
                ),
                404 => format!("Repository not found: {} - {}", repository, error_text),
                _ => format!(
                    "Failed to start upload session (status {}): {}",
                    status, error_text
                ),
            };

            Err(PusherError::Registry(error_msg))
        }
    }

    pub async fn upload_manifest(&self, manifest: &str, repository: &str, tag: &str) -> Result<()> {
        let url = format!("{}/v2/{}/manifests/{}", self.address, repository, tag);

        self.output
            .info(&format!("Uploading manifest for {}:{}", repository, tag));
        self.output.detail(&format!(
            "Manifest size: {}",
            self.output.format_size(manifest.len() as u64)
        ));

        let response = self
            .client
            .put(&url)
            .header(
                "Content-Type",
                "application/vnd.docker.distribution.manifest.v2+json",
            )
            .body(manifest.to_string())
            .send()
            .await
            .map_err(|e| PusherError::Network(format!("Failed to upload manifest: {}", e)))?;

        if response.status().is_success() {
            self.output.success(&format!(
                "Manifest uploaded successfully for {}:{}",
                repository, tag
            ));
            Ok(())
        } else {
            // Store status before consuming response
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(PusherError::Registry(format!(
                "Manifest upload failed (status {}): {}",
                status, error_text
            )))
        }
    }

    // Add getter for address
    pub fn get_address(&self) -> &str {
        &self.address
    }

    // Add getter for HTTP client
    pub fn get_http_client(&self) -> &Client {
        &self.client
    }

    pub fn get_output_manager(&self) -> &OutputManager {
        &self.output
    }
}
