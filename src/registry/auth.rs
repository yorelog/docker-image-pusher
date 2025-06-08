use crate::error::{RegistryError, Result};
use crate::logging::Logger;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub expires_in: Option<u64>,
    pub issued_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub token: String,
    pub expires_at: Option<std::time::Instant>,
    pub repository: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub registry_url: String,
}

impl TokenInfo {
    pub fn new(
        token: String,
        expires_in: Option<u64>,
        repository: String,
        username: Option<String>,
        password: Option<String>,
        registry_url: String,
    ) -> Self {
        let expires_at = expires_in.map(|seconds| {
            std::time::Instant::now() + std::time::Duration::from_secs(seconds.saturating_sub(60)) // 1 minute buffer
        });
        
        Self {
            token,
            expires_at,
            repository,
            username,
            password,
            registry_url,
        }
    }
    
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            std::time::Instant::now() > expires_at
        } else {
            false // If no expiry info, assume token is still valid
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthChallenge {
    pub realm: String,
    pub service: Option<String>,
    pub scope: Option<String>,
}

#[derive(Clone)]
pub struct Auth {
    client: Client,
}

impl Auth {
    pub fn new() -> Self {
        Auth {
            client: Client::new(),
        }
    }

    /// Parse WWW-Authenticate header according to Docker Registry API v2 spec
    fn parse_www_authenticate(header_value: &str) -> Result<AuthChallenge> {
        if !header_value.starts_with("Bearer ") {
            return Err(RegistryError::Registry(
                "Only Bearer authentication is supported".to_string(),
            ));
        }

        let params_str = &header_value[7..]; // Remove "Bearer " prefix
        let mut params = HashMap::new();

        // Parse key=value pairs
        for param in params_str.split(',') {
            let param = param.trim();
            if let Some(eq_pos) = param.find('=') {
                let key = param[..eq_pos].trim();
                let value = param[eq_pos + 1..].trim();
                // Remove quotes if present
                let value = if value.starts_with('"') && value.ends_with('"') {
                    &value[1..value.len() - 1]
                } else {
                    value
                };
                params.insert(key, value);
            }
        }

        let realm = params
            .get("realm")
            .ok_or_else(|| {
                RegistryError::Registry("Missing realm in WWW-Authenticate header".to_string())
            })?
            .to_string();

        Ok(AuthChallenge {
            realm,
            service: params.get("service").map(|s| s.to_string()),
            scope: params.get("scope").map(|s| s.to_string()),
        })
    }

    /// Perform Docker Registry API v2 authentication
    pub async fn authenticate_with_registry(
        &self,
        registry_url: &str,
        repository: &str,
        username: Option<&str>,
        password: Option<&str>,
        output: &Logger,
    ) -> Result<Option<String>> {
        output.verbose("Starting Docker Registry API v2 authentication...");

        // Step 1: Try to access the registry to get auth challenge
        let ping_url = format!("{}/v2/", registry_url);
        let response = self
            .client
            .get(&ping_url)
            .send()
            .await
            .map_err(|e| RegistryError::Network(format!("Failed to ping registry: {}", e)))?;

        match response.status().as_u16() {
            200 => {
                output.verbose("Registry does not require authentication");
                return Ok(None);
            }
            401 => {
                output.verbose("Registry requires authentication, processing challenge...");

                // Step 2: Parse WWW-Authenticate header
                let www_auth = response
                    .headers()
                    .get("www-authenticate")
                    .and_then(|h| h.to_str().ok())
                    .ok_or_else(|| {
                        RegistryError::Registry(
                            "Missing WWW-Authenticate header in 401 response".to_string(),
                        )
                    })?;

                let challenge = Self::parse_www_authenticate(www_auth)?;
                output.verbose(&format!(
                    "Auth challenge: realm={}, service={:?}, scope={:?}",
                    challenge.realm, challenge.service, challenge.scope
                ));

                // Step 3: Request token from auth service
                return self
                    .request_token(challenge, repository, username, password, output)
                    .await;
            }
            _ => {
                return Err(RegistryError::Registry(format!(
                    "Unexpected status {} when checking registry authentication",
                    response.status()
                )));
            }
        }
    }

    async fn request_token(
        &self,
        challenge: AuthChallenge,
        repository: &str,
        username: Option<&str>,
        password: Option<&str>,
        output: &Logger,
    ) -> Result<Option<String>> {
        let mut url = reqwest::Url::parse(&challenge.realm)
            .map_err(|e| RegistryError::Registry(format!("Invalid auth realm URL: {}", e)))?;

        // Add query parameters
        if let Some(service) = &challenge.service {
            url.query_pairs_mut().append_pair("service", service);
        }

        // Build scope for repository access
        let scope = format!("repository:{}:pull,push", repository);
        url.query_pairs_mut().append_pair("scope", &scope);

        output.verbose(&format!("Requesting token from: {}", url));

        // Build request with optional basic auth
        let mut request = self.client.get(url);

        if let (Some(user), Some(pass)) = (username, password) {
            output.verbose(&format!("Using basic auth for user: {}", user));
            request = request.basic_auth(user, Some(pass));
        }

        let response = request
            .send()
            .await
            .map_err(|e| RegistryError::Network(format!("Failed to request auth token: {}", e)))?;

        if response.status().is_success() {
            let auth_response: AuthResponse = response.json().await.map_err(|e| {
                RegistryError::Registry(format!("Failed to parse auth response: {}", e))
            })?;

            output.success("Successfully obtained authentication token");
            output.verbose(&format!(
                "Token expires in: {:?} seconds",
                auth_response.expires_in
            ));

            Ok(Some(auth_response.token))
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(RegistryError::Registry(format!(
                "Authentication failed (status {}): {}",
                status, error_text
            )))
        }
    }

    /// Refresh an expired token using stored credentials
    pub async fn refresh_token(
        &self,
        token_info: &TokenInfo,
        output: &Logger,
    ) -> Result<Option<String>> {
        output.warning("Token expired, refreshing authentication...");
        
        self.authenticate_with_registry(
            &token_info.registry_url,
            &token_info.repository,
            token_info.username.as_deref(),
            token_info.password.as_deref(),
            output,
        )
        .await
    }

    /// Create a new TokenInfo with authentication
    pub async fn authenticate_with_token_info(
        &self,
        registry_url: &str,
        repository: &str,
        username: Option<&str>,
        password: Option<&str>,
        output: &Logger,
    ) -> Result<Option<TokenInfo>> {
        output.verbose("Starting Docker Registry API v2 authentication with token tracking...");

        // Step 1: Try to access the registry to get auth challenge
        let ping_url = format!("{}/v2/", registry_url);
        let response = self
            .client
            .get(&ping_url)
            .send()
            .await
            .map_err(|e| RegistryError::Network(format!("Failed to ping registry: {}", e)))?;

        match response.status().as_u16() {
            200 => {
                output.verbose("Registry does not require authentication");
                return Ok(None);
            }
            401 => {
                output.verbose("Registry requires authentication, processing challenge...");

                // Step 2: Parse WWW-Authenticate header
                let www_auth = response
                    .headers()
                    .get("www-authenticate")
                    .and_then(|h| h.to_str().ok())
                    .ok_or_else(|| {
                        RegistryError::Registry(
                            "Missing WWW-Authenticate header in 401 response".to_string(),
                        )
                    })?;

                let challenge = Self::parse_www_authenticate(www_auth)?;
                output.verbose(&format!(
                    "Auth challenge: realm={}, service={:?}, scope={:?}",
                    challenge.realm, challenge.service, challenge.scope
                ));

                // Step 3: Request token from auth service and get TokenInfo
                let token_result = self.request_token_with_info(
                    challenge, 
                    repository, 
                    username, 
                    password, 
                    registry_url,
                    output
                ).await?;
                
                return Ok(token_result);
            }
            _ => {
                return Err(RegistryError::Registry(format!(
                    "Unexpected status {} when checking registry authentication",
                    response.status()
                )));
            }
        }
    }

    /// Request token and return TokenInfo with expiration tracking
    async fn request_token_with_info(
        &self,
        challenge: AuthChallenge,
        repository: &str,
        username: Option<&str>,
        password: Option<&str>,
        registry_url: &str,
        output: &Logger,
    ) -> Result<Option<TokenInfo>> {
        let mut url = reqwest::Url::parse(&challenge.realm)
            .map_err(|e| RegistryError::Registry(format!("Invalid auth realm URL: {}", e)))?;

        // Add query parameters
        if let Some(service) = &challenge.service {
            url.query_pairs_mut().append_pair("service", service);
        }

        // Build scope for repository access
        let scope = format!("repository:{}:pull,push", repository);
        url.query_pairs_mut().append_pair("scope", &scope);

        output.verbose(&format!("Requesting token from: {}", url));

        // Build request with optional basic auth
        let mut request = self.client.get(url);

        if let (Some(user), Some(pass)) = (username, password) {
            output.verbose(&format!("Using basic auth for user: {}", user));
            request = request.basic_auth(user, Some(pass));
        }

        let response = request
            .send()
            .await
            .map_err(|e| RegistryError::Network(format!("Failed to request auth token: {}", e)))?;

        if response.status().is_success() {
            let auth_response: AuthResponse = response.json().await.map_err(|e| {
                RegistryError::Registry(format!("Failed to parse auth response: {}", e))
            })?;

            output.success("Successfully obtained authentication token");
            output.verbose(&format!(
                "Token expires in: {:?} seconds",
                auth_response.expires_in
            ));

            let token_info = TokenInfo::new(
                auth_response.token,
                auth_response.expires_in,
                repository.to_string(),
                username.map(|s| s.to_string()),
                password.map(|s| s.to_string()),
                registry_url.to_string(),
            );

            Ok(Some(token_info))
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(RegistryError::Registry(format!(
                "Authentication failed (status {}): {}",
                status, error_text
            )))
        }
    }

    pub async fn login(
        &self,
        username: &str,
        _password: &str,
        output: &Logger,
    ) -> Result<Option<String>> {
        output.info(&format!("Authenticating user: {}", username));

        // This is a placeholder - in real usage, registry URL should be provided
        output.warning("login() method is deprecated, use authenticate_with_registry() instead");
        Ok(Some("dummy_token".to_string()))
    }

    pub async fn get_repository_token(
        &self,
        _username: &str,
        _password: &str,
        repository: &str,
        output: &Logger,
    ) -> Result<Option<String>> {
        output.info(&format!("Getting repository token for: {}", repository));

        // This is a placeholder - in real usage, registry URL should be provided
        output.warning(
            "get_repository_token() method is deprecated, use authenticate_with_registry() instead",
        );
        Ok(Some("dummy_repo_token".to_string()))
    }

    pub async fn get_token(
        &self,
        _registry: &str,
        _repo: &str,
        _username: Option<&str>,
        _password: Option<&str>,
    ) -> Result<String> {
        // Placeholder for backward compatibility
        Ok("token".to_string())
    }
}
