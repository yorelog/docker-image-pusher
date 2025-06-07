//! Configuration management module

use crate::error::{RegistryError, Result};
use serde::{Deserialize, Serialize};

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
}

impl AuthConfig {
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }

    pub fn validate(&self) -> Result<()> {
        if self.username.is_empty() {
            return Err(RegistryError::Validation(
                "Username cannot be empty".to_string(),
            ));
        }
        if self.password.is_empty() {
            return Err(RegistryError::Validation(
                "Password cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

/// Registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub address: String,
    pub skip_tls: bool,
    pub timeout: u64,
}

impl RegistryConfig {
    pub fn new(address: String) -> Self {
        Self {
            address,
            skip_tls: false,
            timeout: 7200,
        }
    }

    pub fn with_skip_tls(mut self, skip_tls: bool) -> Self {
        self.skip_tls = skip_tls;
        self
    }

    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.address.is_empty() {
            return Err(RegistryError::Validation(
                "Registry address cannot be empty".to_string(),
            ));
        }

        if !self.address.starts_with("http://") && !self.address.starts_with("https://") {
            return Err(RegistryError::Validation(format!(
                "Invalid registry address: {}. Must start with http:// or https://",
                self.address
            )));
        }

        if self.timeout == 0 {
            return Err(RegistryError::Validation(
                "Timeout must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub cache_dir: String,
    pub timeout: u64,
    pub max_concurrent: usize,
    pub retry_attempts: usize,
    pub large_layer_threshold: u64,
    pub skip_tls: bool,
    pub verbose: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            cache_dir: ".cache".to_string(),
            timeout: 7200,
            max_concurrent: 1,
            retry_attempts: 3,
            large_layer_threshold: 1024 * 1024 * 1024, // 1GB
            skip_tls: false,
            verbose: false,
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<()> {
        if self.max_concurrent == 0 {
            return Err(RegistryError::Validation(
                "max_concurrent must be greater than 0".to_string(),
            ));
        }
        if self.timeout == 0 {
            return Err(RegistryError::Validation(
                "timeout must be greater than 0".to_string(),
            ));
        }
        if self.large_layer_threshold == 0 {
            return Err(RegistryError::Validation(
                "large_layer_threshold must be greater than 0".to_string(),
            ));
        }
        if self.retry_attempts == 0 {
            return Err(RegistryError::Validation(
                "retry_attempts must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }

    /// Create config from environment variables and defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(val) = std::env::var("DOCKER_PUSHER_CACHE_DIR") {
            config.cache_dir = val;
        }
        if let Ok(val) = std::env::var("DOCKER_PUSHER_TIMEOUT") {
            if let Ok(timeout) = val.parse() {
                config.timeout = timeout;
            }
        }
        if let Ok(val) = std::env::var("DOCKER_PUSHER_MAX_CONCURRENT") {
            if let Ok(max_concurrent) = val.parse() {
                config.max_concurrent = max_concurrent;
            }
        }
        if let Ok(val) = std::env::var("DOCKER_PUSHER_VERBOSE") {
            config.verbose = val.to_lowercase() == "true" || val == "1";
        }

        config
    }

    /// Merge with another config, preferring non-default values
    pub fn merge(mut self, other: &AppConfig) -> Self {
        // Only override if the other value is not the default
        let default = AppConfig::default();

        if other.cache_dir != default.cache_dir {
            self.cache_dir = other.cache_dir.clone();
        }
        if other.timeout != default.timeout {
            self.timeout = other.timeout;
        }
        if other.max_concurrent != default.max_concurrent {
            self.max_concurrent = other.max_concurrent;
        }
        if other.retry_attempts != default.retry_attempts {
            self.retry_attempts = other.retry_attempts;
        }
        if other.large_layer_threshold != default.large_layer_threshold {
            self.large_layer_threshold = other.large_layer_threshold;
        }
        if other.skip_tls != default.skip_tls {
            self.skip_tls = other.skip_tls;
        }
        if other.verbose != default.verbose {
            self.verbose = other.verbose;
        }

        self
    }
}
