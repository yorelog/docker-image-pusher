//! Configuration module for managing application settings and URL parsing

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub url: String,
    pub repository: String,
    pub tag: String,
    pub skip_tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadConfig {
    pub chunk_size: usize,
    pub concurrency: usize,
    pub verbose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub registry: RegistryConfig,
    pub auth: AuthConfig,
    pub upload: UploadConfig,
    pub tar_file_path: String,
}

impl AppConfig {
    pub fn new(
        repository_url: String,
        tar_file_path: String,
        username: Option<String>,
        password: Option<String>,
        chunk_size: usize,
        concurrency: usize,
        skip_tls: bool,
        verbose: bool,
    ) -> Result<Self> {
        let registry = RegistryConfig::parse_url(&repository_url, skip_tls)?;
        let auth = AuthConfig { username, password };
        let upload = UploadConfig { chunk_size, concurrency, verbose };

        Ok(AppConfig {
            registry,
            auth,
            upload,
            tar_file_path,
        })
    }

    pub fn has_auth(&self) -> bool {
        self.auth.username.is_some() && self.auth.password.is_some()
    }
}

impl RegistryConfig {
    pub fn parse_url(url: &str, skip_tls: bool) -> Result<Self> {
        // Parse URL
        let (protocol, remaining) = if let Some(pos) = url.find("://") {
            (&url[..pos + 3], &url[pos + 3..])
        } else {
            ("https://", url)
        };

        let (host, path) = if let Some(pos) = remaining.find('/') {
            (&remaining[..pos], &remaining[pos + 1..])
        } else {
            return Err(anyhow!("Invalid repository URL format. Expected: https://registry/project/repo:tag"));
        };

        let registry_url = format!("{}{}", protocol, host);

        let (repository, tag) = if let Some(colon_pos) = path.rfind(':') {
            (&path[..colon_pos], &path[colon_pos + 1..])
        } else {
            (path, "latest")
        };

        if repository.is_empty() {
            return Err(anyhow!("Repository name cannot be empty"));
        }

        Ok(RegistryConfig {
            url: registry_url,
            repository: repository.to_string(),
            tag: tag.to_string(),
            skip_tls,
        })
    }
}

#[derive(Debug)]
pub struct Config {
    pub registry_address: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub project: Option<String>,
    pub skip_tls: bool,
    pub chunk_size: usize,
}

impl Config {
    pub fn new() -> Result<Self, &'static str> {
        let registry_address = env::var("REGISTRY_ADDRESS").map_err(|_| "REGISTRY_ADDRESS not set")?;
        let username = env::var("REGISTRY_USERNAME").ok();
        let password = env::var("REGISTRY_PASSWORD").ok();
        let project = env::var("REGISTRY_PROJECT").ok();
        let skip_tls = env::var("SKIP_TLS").map_or(false, |v| v == "true");
        let chunk_size = env::var("CHUNK_SIZE")
            .map(|v| v.parse::<usize>().unwrap_or(1048576)) // Default to 1MB
            .unwrap_or(1048576); // Default to 1MB

        Ok(Config {
            registry_address,
            username,
            password,
            project,
            skip_tls,
            chunk_size,
        })
    }
}