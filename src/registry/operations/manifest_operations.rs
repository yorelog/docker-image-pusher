//! Manifest operations for registry client
//!
//! Implements Docker Registry v2 and OCI manifest operations:
//! - Manifest upload (PUT /v2/{name}/manifests/{reference})
//! - Manifest download (GET /v2/{name}/manifests/{reference})
//! - Content-Type detection for Docker v2, OCI Image, and OCI Index manifests
//! - Proper Accept headers for multi-format support

use crate::error::{RegistryError, Result};
use crate::error::handlers::NetworkErrorHandler;
use crate::logging::Logger;
use crate::image::manifest::{parse_manifest, ManifestType};
use crate::registry::token_manager::TokenManager;
use reqwest::Client;

#[derive(Clone)]
pub struct ManifestOperations {
    client: Client,
    address: String,
    output: Logger,
    token_manager: Option<TokenManager>,
}

impl ManifestOperations {
    pub fn new(client: Client, address: String, output: Logger) -> Self {
        Self {
            client,
            address,
            output,
            token_manager: None,
        }
    }

    pub fn with_token_manager(mut self, token_manager: Option<TokenManager>) -> Self {
        self.token_manager = token_manager;
        self
    }

    /// Upload manifest using Docker Registry v2/OCI API with proper content-type detection
    pub async fn upload_manifest_with_token(
        &self,
        manifest: &str,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<()> {
        // Use token manager if available for automatic retry on 401 errors
        if let Some(ref token_manager) = self.token_manager {
            let manifest_copy = manifest.to_string();
            let repository_copy = repository.to_string();
            let reference_copy = reference.to_string();
            let address_copy = self.address.clone();
            let client_copy = self.client.clone();
            let output_copy = self.output.clone();

            return token_manager.execute_with_retry(move |current_token| {
                let manifest_inner = manifest_copy.clone();
                let repository_inner = repository_copy.clone();
                let reference_inner = reference_copy.clone();
                let address_inner = address_copy.clone();
                let client_inner = client_copy.clone();
                let output_inner = output_copy.clone();

                Box::pin(async move {
                    Self::upload_manifest_internal(
                        &client_inner,
                        &address_inner,
                        &output_inner,
                        &manifest_inner,
                        &repository_inner,
                        &reference_inner,
                        &current_token,
                    ).await
                })
            }).await;
        }

        // Fallback to direct call without retry
        Self::upload_manifest_internal(
            &self.client,
            &self.address,
            &self.output,
            manifest,
            repository,
            reference,
            token,
        ).await
    }

    /// Internal implementation for manifest upload
    async fn upload_manifest_internal(
        client: &Client,
        address: &str,
        output: &Logger,
        manifest: &str,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<()> {
        let url = format!("{}/v2/{}/manifests/{}", address, repository, reference);

        // Parse manifest to detect content type following OCI/Docker standards
        let content_type = match parse_manifest(manifest.as_bytes()) {
            Ok(manifest_json) => {
                let media_type = manifest_json
                    .get("mediaType")
                    .and_then(|m| m.as_str())
                    .unwrap_or("application/vnd.docker.distribution.manifest.v2+json");

                let manifest_type = ManifestType::from_media_type(media_type);
                manifest_type.to_content_type()
            }
            Err(_) => {
                // Fallback to Docker v2 if parsing fails
                "application/vnd.docker.distribution.manifest.v2+json"
            }
        };

        output.verbose(&format!(
            "Uploading manifest with content-type: {}",
            content_type
        ));

        let mut request = client
            .put(&url)
            .header("Content-Type", content_type)
            .body(manifest.to_string());

        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if response.status().is_success() {
            output.success(&format!(
                "Manifest uploaded successfully for {}:{}",
                repository, reference
            ));
            Ok(())
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(RegistryError::Registry(format!(
                "Failed to upload manifest: HTTP {} - {}",
                status, error_text
            )))
        }
    }

    /// Download manifest using Docker Registry v2/OCI API with multi-format Accept headers
    pub async fn pull_manifest(
        &self,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<Vec<u8>> {
        // Use token manager if available for automatic retry on 401 errors
        if let Some(ref token_manager) = self.token_manager {
            let repository_copy = repository.to_string();
            let reference_copy = reference.to_string();
            let address_copy = self.address.clone();
            let client_copy = self.client.clone();
            let output_copy = self.output.clone();

            return token_manager.execute_with_retry(move |current_token| {
                let repository_inner = repository_copy.clone();
                let reference_inner = reference_copy.clone();
                let address_inner = address_copy.clone();
                let client_inner = client_copy.clone();
                let output_inner = output_copy.clone();

                Box::pin(async move {
                    Self::pull_manifest_internal(
                        &client_inner,
                        &address_inner,
                        &output_inner,
                        &repository_inner,
                        &reference_inner,
                        &current_token,
                    ).await
                })
            }).await;
        }

        // Fallback to direct call without retry
        Self::pull_manifest_internal(
            &self.client,
            &self.address,
            &self.output,
            repository,
            reference,
            token,
        ).await
    }

    /// Internal implementation for manifest download
    async fn pull_manifest_internal(
        client: &Client,
        address: &str,
        output: &Logger,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<Vec<u8>> {
        output.verbose(&format!(
            "Pulling manifest for {}/{}",
            repository, reference
        ));

        let url = format!("{}/v2/{}/manifests/{}", address, repository, reference);

        // Accept all standard manifest types per OCI/Docker Registry v2 specs
        let mut request = client.get(&url).header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json, \
                 application/vnd.docker.distribution.manifest.list.v2+json, \
                 application/vnd.oci.image.manifest.v1+json, \
                 application/vnd.oci.image.index.v1+json",
        );

        // Add auth header if token is provided
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            output
                .error(&format!("Failed to pull manifest: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "manifest pull")
        })?;

        if response.status().is_success() {
            output.success(&format!(
                "Successfully pulled manifest for {}/{}",
                repository, reference
            ));

            let content_type = response
                .headers()
                .get("Content-Type")
                .map(|h| h.to_str().unwrap_or("unknown"))
                .unwrap_or("unknown");

            output
                .detail(&format!("Manifest type: {}", content_type));

            let data = response.bytes().await.map_err(|e| {
                RegistryError::Network(format!("Failed to read manifest response: {}", e))
            })?;

            Ok(data.to_vec())
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());

            output.error(&format!(
                "Failed to pull manifest: HTTP {} - {}",
                status, error_text
            ));

            Err(RegistryError::Registry(format!(
                "Failed to pull manifest for {}/{} (status {}): {}",
                repository, reference, status, error_text
            )))
        }
    }
}
