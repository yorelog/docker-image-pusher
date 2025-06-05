//! Authentication module for Docker registry access

use crate::error::{Result, PusherError};
use reqwest::{Client, header::{AUTHORIZATION, WWW_AUTHENTICATE}};
use serde::Deserialize;
use base64::{Engine as _, engine::general_purpose};

#[derive(Deserialize, Debug)]
struct AuthResponse {
    token: Option<String>,
    access_token: Option<String>,
}

#[derive(Debug)]
pub struct AuthChallenge {
    pub realm: String,
    pub service: String,
    pub scope: Option<String>,
}

impl AuthChallenge {
    pub fn parse(auth_header: &str) -> Result<Self> {
        let mut realm = String::new();
        let mut service = String::new();
        let mut scope = None;

        if auth_header.starts_with("Bearer ") {
            let params = &auth_header[7..]; // Remove "Bearer "
            for param in params.split(',') {
                let param = param.trim();
                if let Some((key, value)) = param.split_once('=') {
                    let value = value.trim_matches('"');
                    match key {
                        "realm" => realm = value.to_string(),
                        "service" => service = value.to_string(),
                        "scope" => scope = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }

        if realm.is_empty() || service.is_empty() {
            return Err(PusherError::Authentication("Invalid auth challenge format".to_string()));
        }

        Ok(AuthChallenge { realm, service, scope })
    }
}

pub struct Auth {
    client: Client,
    registry_url: String,
}

impl Auth {
    pub fn new(registry_url: &str, skip_tls: bool) -> Result<Self> {
        let client = if skip_tls {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
                .build()
                .map_err(PusherError::Network)?
        } else {
            Client::new()
        };

        Ok(Auth {
            client,
            registry_url: registry_url.to_string(),
        })
    }

    pub async fn get_auth_challenge(&self) -> Result<Option<AuthChallenge>> {
        let url = format!("{}/v2/", self.registry_url);
        println!("  Getting auth challenge from: {}", url);
        
        let response = self.client.get(&url).send().await?;
        println!("  Auth challenge response status: {}", response.status());
        
        if response.status() == 401 {
            if let Some(auth_header) = response.headers().get(WWW_AUTHENTICATE) {
                let auth_str = auth_header.to_str()
                    .map_err(|e| PusherError::Authentication(format!("Invalid auth header: {}", e)))?;
                println!("  WWW-Authenticate header: {}", auth_str);
                return Ok(Some(AuthChallenge::parse(auth_str)?));
            }
        }
        
        Ok(None)
    }

    pub async fn get_token_for_repository(
        &self,
        challenge: &AuthChallenge,
        username: &str,
        password: &str,
        repository: &str,
    ) -> Result<String> {
        let credentials = general_purpose::STANDARD.encode(format!("{}:{}", username, password));
        
        // For Harbor, we need the exact repository scope with push permission
        let scope = format!("repository:{}:pull,push", repository);
        let query_params = vec![
            ("service", challenge.service.as_str()),
            ("scope", scope.as_str()),
        ];
        
        println!("  Token request to: {}", challenge.realm);
        println!("  Service: {}", challenge.service);
        println!("  Scope: {}", scope);
        println!("  Username: {}", username);
        
        self.request_token(&challenge.realm, &credentials, &query_params).await
    }

    pub async fn get_token(
        &self,
        challenge: &AuthChallenge,
        username: &str,
        password: &str,
        repository: Option<&str>,
    ) -> Result<String> {
        let credentials = general_purpose::STANDARD.encode(format!("{}:{}", username, password));
        
        let mut query_params = vec![("service", challenge.service.as_str())];
        
        // Build scope for Harbor
        let scope_value = if let Some(repo) = repository {
            format!("repository:{}:pull,push", repo)
        } else {
            String::new()
        };
        
        if !scope_value.is_empty() {
            query_params.push(("scope", scope_value.as_str()));
            println!("  Token request to: {}", challenge.realm);
            println!("  Service: {}", challenge.service);
            println!("  Scope: {}", scope_value);
            println!("  Username: {}", username);
        }
        
        match self.request_token(&challenge.realm, &credentials, &query_params).await {
            Ok(token) => Ok(token),
            Err(e) => {
                println!("  Token request failed: {}", e);
                // Retry without scope if failed
                if !scope_value.is_empty() {
                    println!("  Retrying without scope...");
                    let basic_params = vec![("service", challenge.service.as_str())];
                    self.request_token(&challenge.realm, &credentials, &basic_params).await
                } else {
                    Err(PusherError::Authentication("Token request failed".to_string()))
                }
            }
        }
    }

    async fn request_token(
        &self,
        realm: &str,
        credentials: &str,
        query_params: &[(&str, &str)],
    ) -> Result<String> {
        let response = self.client
            .get(realm)
            .header(AUTHORIZATION, format!("Basic {}", credentials))
            .query(query_params)
            .send()
            .await?;

        println!("  Token response status: {}", response.status());

        if response.status().is_success() {
            let response_text = response.text().await?;
            println!("  Token response body: {}", response_text);
            
            let auth_response: AuthResponse = serde_json::from_str(&response_text)
                .map_err(|e| PusherError::Authentication(format!("Failed to parse token response: {}", e)))?;
            
            let token = auth_response.token
                .or(auth_response.access_token)
                .ok_or_else(|| PusherError::Authentication("No token in response".to_string()))?;
            
            println!("  Token received successfully");
            Ok(token)
        } else {
            let error_text = response.text().await?;
            println!("  Token request failed with body: {}", error_text);
            Err(PusherError::Authentication(format!("Token request failed: {}", error_text)))
        }
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<Option<String>> {
        if let Some(challenge) = self.get_auth_challenge().await? {
            println!("Auth challenge received from registry:");
            println!("  Realm: {}", challenge.realm);
            println!("  Service: {}", challenge.service);
            if let Some(scope) = &challenge.scope {
                println!("  Scope: {}", scope);
            }
            let token = self.get_token(&challenge, username, password, None).await?;
            Ok(Some(token))
        } else {
            println!("No auth challenge - registry may not require authentication");
            Ok(None)
        }
    }

    pub async fn login_with_repository(&self, username: &str, password: &str, repository: &str) -> Result<Option<String>> {
        if let Some(challenge) = self.get_auth_challenge().await? {
            println!("Getting repository-specific token for: {}", repository);
            let token = self.get_token_for_repository(&challenge, username, password, repository).await?;
            Ok(Some(token))
        } else {
            Ok(None)
        }
    }
}