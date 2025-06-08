//! Token management for automatic refresh during long-running operations
//!
//! This module provides automatic token refresh capabilities for long-running
//! registry operations, particularly useful for large image downloads/uploads
//! that may exceed token expiration times.

use crate::error::{RegistryError, Result};
use crate::logging::Logger;
use crate::registry::auth::{Auth, TokenInfo};
use std::sync::{Arc, RwLock};

/// Thread-safe token manager that handles automatic refresh
#[derive(Clone)]
pub struct TokenManager {
    auth: Auth,
    token_info: Arc<RwLock<Option<TokenInfo>>>,
    output: Logger,
}

impl TokenManager {
    pub fn new(auth: Auth, output: Logger) -> Self {
        Self {
            auth,
            token_info: Arc::new(RwLock::new(None)),
            output,
        }
    }

    /// Initialize with token info
    pub fn with_token_info(self, token_info: Option<TokenInfo>) -> Self {
        if let Ok(mut guard) = self.token_info.write() {
            *guard = token_info;
        }
        self
    }

    /// Get current token, refreshing if expired
    pub async fn get_valid_token(&self) -> Result<Option<String>> {
        // Check if we have a token and if it's still valid
        let should_refresh = {
            let guard = self.token_info.read().map_err(|_| {
                RegistryError::Registry("Failed to acquire token read lock".to_string())
            })?;
            
            match &*guard {
                Some(token_info) => token_info.is_expired(),
                None => false, // No token to refresh
            }
        };

        if should_refresh {
            self.refresh_token().await?;
        }

        // Return current token
        let guard = self.token_info.read().map_err(|_| {
            RegistryError::Registry("Failed to acquire token read lock".to_string())
        })?;
        
        Ok(guard.as_ref().map(|info| info.token.clone()))
    }

    /// Force refresh the token
    pub async fn refresh_token(&self) -> Result<()> {
        let token_info_clone = {
            let guard = self.token_info.read().map_err(|_| {
                RegistryError::Registry("Failed to acquire token read lock".to_string())
            })?;
            guard.clone()
        };

        if let Some(old_token_info) = token_info_clone {
            self.output.warning("Refreshing expired authentication token...");
            
            let new_token_info = self.auth.authenticate_with_token_info(
                &old_token_info.registry_url,
                &old_token_info.repository,
                old_token_info.username.as_deref(),
                old_token_info.password.as_deref(),
                &self.output,
            ).await?;

            // Update stored token info
            let mut guard = self.token_info.write().map_err(|_| {
                RegistryError::Registry("Failed to acquire token write lock".to_string())
            })?;
            *guard = new_token_info;

            self.output.success("Token refreshed successfully");
        }

        Ok(())
    }

    /// Handle 401 error by refreshing token and returning new token
    pub async fn handle_401_error(&self) -> Result<Option<String>> {
        self.output.warning("Received 401 Unauthorized - attempting token refresh...");
        self.refresh_token().await?;
        self.get_valid_token().await
    }

    /// Check if we have credentials for token refresh
    pub fn can_refresh(&self) -> bool {
        if let Ok(guard) = self.token_info.read() {
            guard.is_some()
        } else {
            false
        }
    }

    /// Execute operation with automatic token refresh on 401 errors
    pub async fn execute_with_retry<F, T>(&self, operation: F) -> Result<T>
    where
        F: Fn(Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
        T: Send,
    {
        let max_retries = 2;
        
        for attempt in 1..=max_retries {
            // Get current valid token
            let token = self.get_valid_token().await?;
            
            // Execute operation
            match operation(token).await {
                Ok(result) => return Ok(result),
                Err(RegistryError::Registry(msg)) if (msg.contains("401") || msg.contains("Unauthorized")) && attempt < max_retries => {
                    if !self.can_refresh() {
                        return Err(RegistryError::Registry(
                            "Cannot refresh token: no credentials available".to_string()
                        ));
                    }

                    self.output.warning(&format!("Attempt {} failed with 401, refreshing token...", attempt));
                    
                    // Try to refresh token for next attempt
                    match self.handle_401_error().await {
                        Ok(_) => continue, // Retry with new token
                        Err(refresh_err) => {
                            return Err(RegistryError::Registry(format!(
                                "Token refresh failed: {}. Original error: {}",
                                refresh_err, msg
                            )));
                        }
                    }
                }
                Err(error) => return Err(error), // Don't retry non-auth errors or final attempt
            }
        }

        Err(RegistryError::Registry(format!(
            "Operation failed after {} attempts with token refresh",
            max_retries
        )))
    }
}
