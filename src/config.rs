//! Configuration structures and utilities

use serde::{Deserialize, Serialize};
use crate::error::{Result, PusherError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
}

impl AuthConfig {
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }

    pub fn from_optional(username: Option<String>, password: Option<String>) -> Result<Self> {
        match (username, password) {
            (Some(u), Some(p)) => Ok(Self::new(u, p)),
            (None, _) => Err(PusherError::Config("Username is required for authentication".to_string())),
            (_, None) => Err(PusherError::Config("Password is required for authentication".to_string())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub address: String,
    pub repository: String,
    pub tag: String,
    pub auth: Option<AuthConfig>,
    pub skip_tls: bool,
    pub timeout: u64,
}

impl RegistryConfig {
    pub fn new(address: String, repository: String, tag: String) -> Self {
        Self {
            address,
            repository,
            tag,
            auth: None,
            skip_tls: false,
            timeout: 7200, // 2 hours default
        }
    }

    pub fn with_auth(mut self, auth: AuthConfig) -> Self {
        self.auth = Some(auth);
        self
    }

    pub fn with_skip_tls(mut self, skip_tls: bool) -> Self {
        self.skip_tls = skip_tls;
        self
    }

    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub registry: RegistryConfig,
    pub file_path: String,
    pub large_layer_threshold: u64,
    pub max_concurrent: usize,
    pub retry_attempts: usize,
    pub verbose: bool,
    pub dry_run: bool,
}

impl AppConfig {
    pub fn new(
        registry_url: &str,
        repository: &str,
        tag: &str,
        file_path: String,
    ) -> Result<Self> {
        let registry_config = RegistryConfig::new(
            registry_url.to_string(),
            repository.to_string(),
            tag.to_string(),
        );

        Ok(Self {
            registry: registry_config,
            file_path,
            large_layer_threshold: 1024 * 1024 * 1024, // 1GB default
            max_concurrent: 1,
            retry_attempts: 3,
            verbose: false,
            dry_run: false,
        })
    }

    pub fn with_auth(mut self, username: String, password: String) -> Self {
        let auth = AuthConfig::new(username, password);
        self.registry = self.registry.with_auth(auth);
        self
    }

    pub fn with_optional_auth(mut self, username: Option<String>, password: Option<String>) -> Result<Self> {
        if let (Some(u), Some(p)) = (username, password) {
            self = self.with_auth(u, p);
        }
        Ok(self)
    }

    pub fn has_auth(&self) -> bool {
        self.registry.auth.is_some()
    }

    pub fn with_large_layer_threshold(mut self, threshold: u64) -> Self {
        self.large_layer_threshold = threshold;
        self
    }

    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.registry = self.registry.with_timeout(timeout);
        self
    }

    pub fn with_skip_tls(mut self, skip_tls: bool) -> Self {
        self.registry = self.registry.with_skip_tls(skip_tls);
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn with_max_concurrent(mut self, max_concurrent: usize) -> Self {
        self.max_concurrent = max_concurrent;
        self
    }

    pub fn with_retry_attempts(mut self, retry_attempts: usize) -> Self {
        self.retry_attempts = retry_attempts;
        self
    }

    pub fn parse_repository_url(url: &str) -> Result<(String, String, String)> {
        let parsed_url = url::Url::parse(url)
            .map_err(|e| PusherError::Config(format!("Invalid repository URL: {}", e)))?;

        let registry_address = format!("{}://{}", 
            parsed_url.scheme(), 
            parsed_url.host_str().unwrap_or("localhost"));

        let path = parsed_url.path().trim_start_matches('/');
        let (repository, tag) = if let Some(colon_pos) = path.rfind(':') {
            let (repo, tag_part) = path.split_at(colon_pos);
            (repo, &tag_part[1..]) // Remove the ':' prefix
        } else {
            (path, "latest")
        };

        Ok((registry_address, repository.to_string(), tag.to_string()))
    }
}