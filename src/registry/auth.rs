//! Authentication module for Docker registry access

use crate::error::{Result, PusherError};
use crate::error::handlers::HttpErrorHandler;
use crate::output::OutputManager;
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AuthChallenge {
    realm: String,
    service: String,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: Option<String>,
    access_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Auth {
    client: Client,
    registry_address: String,
}

impl Auth {
    pub fn new(registry_address: &str, skip_tls: bool) -> Result<Self> {
        let client_builder = if skip_tls {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
        } else {
            Client::builder()
        };
        
        let client = client_builder
            .build()
            .map_err(|e| PusherError::Network(format!("Failed to build auth client: {}", e)))?;
        
        Ok(Self {
            client,
            registry_address: registry_address.to_string(),
        })
    }

    pub async fn login(&self, username: &str, password: &str, output: &OutputManager) -> Result<Option<String>> {
        output.verbose("Attempting to authenticate with registry...");
        
        // First, try to access the v2 API to get auth challenge
        let v2_url = format!("{}/v2/", self.registry_address);
        let response = self.client.get(&v2_url).send().await
            .map_err(|e| PusherError::Network(format!("Failed to access registry API: {}", e)))?;
        
        if response.status() == 401 {
            // Parse WWW-Authenticate header
            if let Some(auth_header) = response.headers().get("www-authenticate") {
                let auth_str = auth_header.to_str()
                    .map_err(|e| PusherError::Authentication(format!("Invalid auth header: {}", e)))?;
                
                if auth_str.starts_with("Bearer ") {
                    return self.handle_bearer_auth(auth_str, username, password, output).await;
                }
            }
        }
        
        // If no authentication required or unsupported auth method
        output.info("No authentication required or unsupported authentication method");
        Ok(None)
    }

    // Enhanced method to get repository-specific token
    pub async fn get_repository_token(
        &self,
        username: &str,
        password: &str,
        repository: &str,
        output: &OutputManager,
    ) -> Result<Option<String>> {
        output.verbose(&format!("Getting repository-specific token for: {}", repository));
        
        // Try to access a repository-specific endpoint to get proper scope
        let repo_url = format!("{}/v2/{}/blobs/uploads/", self.registry_address, repository);
        let response = self.client.post(&repo_url).send().await
            .map_err(|e| PusherError::Network(format!("Failed to access repository endpoint: {}", e)))?;
        
        if response.status() == 401 {
            if let Some(auth_header) = response.headers().get("www-authenticate") {
                let auth_str = auth_header.to_str()
                    .map_err(|e| PusherError::Authentication(format!("Invalid auth header: {}", e)))?;
                
                if auth_str.starts_with("Bearer ") {
                    return self.handle_bearer_auth_with_scope(auth_str, username, password, repository, output).await;
                }
            }
        }
        
        // Fallback to general token
        self.login(username, password, output).await
    }

    async fn handle_bearer_auth(
        &self,
        auth_header: &str,
        username: &str,
        password: &str,
        output: &OutputManager,
    ) -> Result<Option<String>> {
        let challenge = self.parse_auth_challenge(auth_header)?;
        
        output.verbose(&format!("Bearer auth challenge: realm={}, service={}", 
            challenge.realm, challenge.service));
        
        // Request token from auth service
        let mut token_url = format!("{}?service={}", challenge.realm, challenge.service);
        if let Some(scope) = &challenge.scope {
            token_url.push_str(&format!("&scope={}", scope));
        }
        
        self.request_token(&token_url, username, password, output).await
    }

    async fn handle_bearer_auth_with_scope(
        &self,
        auth_header: &str,
        username: &str,
        password: &str,
        repository: &str,
        output: &OutputManager,
    ) -> Result<Option<String>> {
        let challenge = self.parse_auth_challenge(auth_header)?;
        
        output.verbose(&format!("Bearer auth challenge for {}: realm={}, service={}", 
            repository, challenge.realm, challenge.service));
        
        // Build scope for push access to the specific repository
        let scope = format!("repository:{}:push,pull", repository);
        let token_url = format!("{}?service={}&scope={}", challenge.realm, challenge.service, scope);
        
        output.verbose(&format!("Requesting token with scope: {}", scope));
        
        self.request_token(&token_url, username, password, output).await
    }

    async fn request_token(
        &self,
        token_url: &str,
        username: &str,
        password: &str,
        output: &OutputManager,
    ) -> Result<Option<String>> {
        output.verbose(&format!("Token request URL: {}", token_url));
        
        let response = self.client
            .get(token_url)
            .basic_auth(username, Some(password))
            .send()
            .await
            .map_err(|e| PusherError::Network(format!("Failed to request auth token: {}", e)))?;
        
        output.verbose(&format!("Token response status: {}", response.status()));
        
        if response.status().is_success() {
            let token_response: TokenResponse = response.json().await
                .map_err(|e| PusherError::Authentication(format!("Failed to parse token response: {}", e)))?;
            
            let token = token_response.token.or(token_response.access_token)
                .ok_or_else(|| PusherError::Authentication("No token in auth response".to_string()))?;
            
            output.success("Authentication token obtained");
            output.verbose(&format!("Token prefix: {}...", &token[..10]));
            Ok(Some(token))
        } else {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            
            output.error(&format!("Token request failed (status {}): {}", status, error_text));
            
            Err(HttpErrorHandler::handle_auth_error(status, &error_text))
        }
    }

    fn parse_auth_challenge(&self, auth_header: &str) -> Result<AuthChallenge> {
        // Parse Bearer realm="...",service="...",scope="..."
        let mut realm = String::new();
        let mut service = String::new();
        let mut scope = None;
        
        // Remove "Bearer " prefix
        let params = auth_header.strip_prefix("Bearer ")
            .ok_or_else(|| PusherError::Authentication("Invalid Bearer auth header".to_string()))?;
        
        // Simple parsing of key=value pairs
        for param in params.split(',') {
            let param = param.trim();
            if let Some(eq_pos) = param.find('=') {
                let key = param[..eq_pos].trim();
                let value = param[eq_pos + 1..].trim().trim_matches('"');
                
                match key {
                    "realm" => realm = value.to_string(),
                    "service" => service = value.to_string(),
                    "scope" => scope = Some(value.to_string()),
                    _ => {} // Ignore unknown parameters
                }
            }
        }
        
        if realm.is_empty() || service.is_empty() {
            return Err(PusherError::Authentication("Invalid auth challenge format".to_string()));
        }
        
        Ok(AuthChallenge {
            realm,
            service,
            scope,
        })
    }
}