//! Authentication operations for registry client
//! 
//! Handles Docker Registry v2 authentication flows, including:
//! - Basic connectivity testing
//! - Bearer token authentication  
//! - Repository-specific authentication
//! - OCI-compatible auth patterns

use crate::cli::config::AuthConfig;
use crate::error::{RegistryError, Result};
use crate::logging::Logger;
use crate::registry::auth::Auth;
use crate::registry::token_manager::TokenManager;
use reqwest::Client;

#[derive(Clone)]
pub struct AuthOperations {
    client: Client,
    auth: Auth,
    address: String,
    output: Logger,
    token_manager: Option<TokenManager>,
}

impl AuthOperations {
    pub fn new(client: Client, auth: Auth, address: String, output: Logger) -> Self {
        Self {
            client,
            auth,
            address,
            output,
            token_manager: None,
        }
    }

    pub fn with_token_manager(mut self, token_manager: Option<TokenManager>) -> Self {
        self.token_manager = token_manager;
        self
    }

    /// Test registry connectivity following Docker Registry v2 API
    pub async fn test_connectivity(&self) -> Result<()> {
        self.output.verbose("Testing registry connectivity...");

        let url = format!("{}/v2/", self.address);
        let response =
            self.client.get(&url).send().await.map_err(|e| {
                RegistryError::Network(format!("Failed to connect to registry: {}", e))
            })?;

        self.output
            .verbose(&format!("Registry response status: {}", response.status()));

        if response.status().is_success() || response.status() == 401 {
            // 401 is expected for registries that require authentication
            self.output.verbose("Registry connectivity test passed");
            Ok(())
        } else {
            Err(RegistryError::Registry(format!(
                "Registry connectivity test failed with status: {}",
                response.status()
            )))
        }
    }

    /// Authenticate with registry using Docker Registry v2/OCI standards
    pub async fn authenticate(&self, auth_config: &AuthConfig) -> Result<Option<String>> {
        self.output.verbose("Authenticating with registry...");

        let token = self
            .auth
            .authenticate_with_registry(
                &self.address,
                "", // General authentication, not repository-specific
                Some(&auth_config.username),
                Some(&auth_config.password),
                &self.output,
            )
            .await?;

        if token.is_some() {
            self.output.success("Authentication successful");
        } else {
            self.output.info("No authentication required");
        }

        Ok(token)
    }

    /// Authenticate for specific repository access (Docker Registry v2 scope pattern)
    pub async fn authenticate_for_repository(
        &self,
        auth_config: &AuthConfig,
        repository: &str,
    ) -> Result<Option<String>> {
        self.output.verbose(&format!(
            "Authenticating for repository access: {}",
            repository
        ));

        // Use the Docker Registry API v2 compliant authentication
        let token = self
            .auth
            .authenticate_with_registry(
                &self.address,
                repository,
                Some(&auth_config.username),
                Some(&auth_config.password),
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
}
