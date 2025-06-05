//! Authentication module for Docker registry access

use crate::error::{Result, PusherError};
use crate::output::OutputManager;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    // Note: expires_in is part of the API response but not currently used
    #[allow(dead_code)]
    expires_in: Option<u64>,
}

// Note: LoginRequest might be used for future password-based auth
#[allow(dead_code)]
#[derive(Debug, Serialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug)]
pub struct Auth {
    client: Client,
    registry_address: String,
    // Note: auth_endpoint might be used for future enhanced auth flows
    #[allow(dead_code)]
    auth_endpoint: Option<String>,
}

impl Auth {
    pub fn new(registry_address: &str, skip_tls: bool) -> Result<Self> {
        let client = if skip_tls {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
                .build()
        } else {
            Client::builder().build()
        }
        .map_err(|e| PusherError::Network(format!("Failed to create auth client: {}", e)))?;

        Ok(Self {
            client,
            registry_address: registry_address.to_string(),
            auth_endpoint: None,
        })
    }

    pub async fn login(&self, username: &str, password: &str, output: &OutputManager) -> Result<Option<String>> {
        output.verbose(&format!("Attempting authentication for user: {}", username));

        // First, try to get auth challenge from registry
        let auth_challenge = self.get_auth_challenge(output).await?;
        
        if let Some(challenge) = auth_challenge {
            output.verbose(&format!("Auth challenge received: realm={}, service={}", 
                         challenge.realm, challenge.service));

            // Get token from auth server
            let token = self.get_token(&challenge, username, password, output).await?;
            
            if token.is_some() {
                output.success("Authentication token obtained successfully");
            } else {
                output.info("No token required for this registry");
            }
            
            Ok(token)
        } else {
            output.info("No authentication challenge - registry may not require auth");
            Ok(None)
        }
    }

    async fn get_auth_challenge(&self, output: &OutputManager) -> Result<Option<AuthChallenge>> {
        output.detail("Sending auth challenge request to registry");
        
        let url = format!("{}/v2/", self.registry_address);
        let response = self.client.get(&url).send().await
            .map_err(|e| PusherError::Network(format!("Failed to get auth challenge: {}", e)))?;

        output.detail(&format!("Auth challenge response status: {}", response.status()));

        if response.status() == 401 {
            if let Some(auth_header) = response.headers().get("www-authenticate") {
                let auth_str = auth_header.to_str()
                    .map_err(|e| PusherError::Parse(format!("Invalid auth header: {}", e)))?;
                
                output.detail(&format!("Parsing auth header: {}", auth_str));
                return self.parse_auth_challenge(auth_str, output);
            }
        }

        Ok(None)
    }

    fn parse_auth_challenge(&self, auth_header: &str, output: &OutputManager) -> Result<Option<AuthChallenge>> {
        // Parse Bearer challenge: Bearer realm="...",service="...",scope="..."
        if !auth_header.starts_with("Bearer ") {
            output.debug("Auth header is not Bearer type");
            return Ok(None);
        }

        let params_str = &auth_header[7..]; // Remove "Bearer "
        let mut params = HashMap::new();

        for param in params_str.split(',') {
            let param = param.trim();
            if let Some(eq_pos) = param.find('=') {
                let key = param[..eq_pos].trim();
                let value = param[eq_pos + 1..].trim().trim_matches('"');
                params.insert(key, value);
            }
        }

        if let Some(realm) = params.get("realm") {
            let service = params.get("service").unwrap_or(&"").to_string();
            let scope = params.get("scope").map(|s| s.to_string());

            output.detail(&format!("Parsed auth challenge - realm: {}, service: {}, scope: {:?}", 
                realm, service, scope));

            Ok(Some(AuthChallenge {
                realm: realm.to_string(),
                service,
                scope,
            }))
        } else {
            output.warning("Auth header missing realm parameter");
            Ok(None)
        }
    }

    async fn get_token(&self, challenge: &AuthChallenge, username: &str, password: &str, output: &OutputManager) -> Result<Option<String>> {
        let mut url = format!("{}?service={}", challenge.realm, challenge.service);
        
        if let Some(scope) = &challenge.scope {
            url.push_str(&format!("&scope={}", scope));
        }

        output.detail(&format!("Requesting token from: {}", url));

        let response = self.client
            .get(&url)
            .basic_auth(username, Some(password))
            .send()
            .await
            .map_err(|e| PusherError::Network(format!("Failed to get auth token: {}", e)))?;

        if response.status().is_success() {
            output.detail("Token request successful, parsing response");
            
            let token_response: TokenResponse = response.json().await
                .map_err(|e| PusherError::Parse(format!("Failed to parse token response: {}", e)))?;

            let token = token_response.token.or(token_response.access_token);
            
            if let Some(ref token) = token {
                output.detail(&format!("Token obtained (length: {} chars)", token.len()));
                
                // Log expiration info if available
                if let Some(expires_in) = token_response.expires_in {
                    output.detail(&format!("Token expires in {} seconds", expires_in));
                }
            }

            Ok(token)
        } else {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            
            output.error(&format!("Token request failed with status {}: {}", status, error_text));
            
            Err(PusherError::Authentication(format!(
                "Authentication failed with status: {}", 
                status
            )))
        }
    }

    // Future method for enhanced authentication flows
    #[allow(dead_code)]
    pub fn set_auth_endpoint(&mut self, endpoint: String) {
        self.auth_endpoint = Some(endpoint);
    }
}