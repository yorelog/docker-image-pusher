//! Repository operations for registry client
//!
//! Implements Docker Registry v2 repository-level operations:
//! - Tag listing (GET /v2/{name}/tags/list)
//! - Image existence checks (HEAD /v2/{name}/manifests/{reference})
//! - Repository-level metadata operations

use crate::error::{RegistryError, Result};
use crate::error::handlers::NetworkErrorHandler;
use crate::logging::Logger;
use crate::registry::token_manager::TokenManager;
use reqwest::Client;
use serde_json::Value;

#[derive(Clone)]
pub struct RepositoryOperations {
    client: Client,
    address: String,
    output: Logger,
    token_manager: Option<TokenManager>,
}

impl RepositoryOperations {
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

    /// List all tags in repository using Docker Registry v2 API
    pub async fn list_tags(&self, repository: &str, token: &Option<String>) -> Result<Vec<String>> {
        // Use token manager if available for automatic retry on 401 errors
        if let Some(ref token_manager) = self.token_manager {
            let repository_copy = repository.to_string();
            let address_copy = self.address.clone();
            let client_copy = self.client.clone();
            let output_copy = self.output.clone();

            return token_manager.execute_with_retry(move |current_token| {
                let repository_inner = repository_copy.clone();
                let address_inner = address_copy.clone();
                let client_inner = client_copy.clone();
                let output_inner = output_copy.clone();

                Box::pin(async move {
                    Self::list_tags_internal(
                        &client_inner,
                        &address_inner,
                        &output_inner,
                        &repository_inner,
                        &current_token,
                    ).await
                })
            }).await;
        }

        // Fallback to direct call without retry
        Self::list_tags_internal(
            &self.client,
            &self.address,
            &self.output,
            repository,
            token,
        ).await
    }

    /// Internal implementation for tag listing
    async fn list_tags_internal(
        client: &Client,
        address: &str,
        output: &Logger,
        repository: &str,
        token: &Option<String>,
    ) -> Result<Vec<String>> {
        output.verbose(&format!("Listing tags for repository: {}", repository));

        let url = format!("{}/v2/{}/tags/list", address, repository);

        let mut request = client.get(&url);

        // Add auth header if token is provided
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            output
                .error(&format!("Failed to list tags: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "tag listing")
        })?;

        if response.status().is_success() {
            let body = response.text().await.map_err(|e| {
                RegistryError::Network(format!("Failed to read tags response: {}", e))
            })?;

            let tags_response: Value = serde_json::from_str(&body).map_err(|e| {
                RegistryError::Registry(format!("Failed to parse tags response: {}", e))
            })?;

            let tags = tags_response
                .get("tags")
                .and_then(|t| t.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|tag| tag.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(Vec::new);

            output.success(&format!(
                "Found {} tags for repository {}",
                tags.len(),
                repository
            ));

            Ok(tags)
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());

            output.error(&format!(
                "Failed to list tags: HTTP {} - {}",
                status, error_text
            ));

            Err(RegistryError::Registry(format!(
                "Failed to list tags for {} (status {}): {}",
                repository, status, error_text
            )))
        }
    }

    /// Check if image exists in repository using Docker Registry v2 HEAD request
    pub async fn check_image_exists(
        &self,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<bool> {
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
                    Self::check_image_exists_internal(
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
        Self::check_image_exists_internal(
            &self.client,
            &self.address,
            &self.output,
            repository,
            reference,
            token,
        ).await
    }

    /// Internal implementation for image existence check
    async fn check_image_exists_internal(
        client: &Client,
        address: &str,
        output: &Logger,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<bool> {
        output.verbose(&format!(
            "Checking if image {}/{} exists",
            repository, reference
        ));

        // Try to get image manifest using HEAD request (more efficient)
        let url = format!("{}/v2/{}/manifests/{}", address, repository, reference);

        let mut request = client.head(&url).header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json",
        );

        // Add auth header if token is provided
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            output
                .error(&format!("Failed to check image existence: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "image existence check")
        })?;

        match response.status().as_u16() {
            200 => {
                output.success(&format!(
                    "Image {}/{} exists in registry",
                    repository, reference
                ));
                Ok(true)
            }
            404 => {
                output.info(&format!(
                    "Image {}/{} does not exist in registry",
                    repository, reference
                ));
                Ok(false)
            }
            401 => {
                output.warning(&format!(
                    "Authentication required to check {}/{}",
                    repository, reference
                ));
                // Return false if we can't authenticate
                Ok(false)
            }
            403 => {
                output.warning(&format!(
                    "Permission denied to check {}/{}",
                    repository, reference
                ));
                // Return false if we don't have permission
                Ok(false)
            }
            _ => {
                let status = response.status();
                output.warning(&format!(
                    "Unexpected status {} when checking image {}/{}",
                    status, repository, reference
                ));
                // On other errors, assume image doesn't exist to be safe
                Ok(false)
            }
        }
    }
}
