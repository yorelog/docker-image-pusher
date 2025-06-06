//! Command line argument parsing and validation
//!
//! This module defines the [`Args`] struct for parsing CLI arguments using `clap`,
//! and provides validation logic for user input.

use crate::error::handlers::ValidationErrorHandler;
use crate::error::{PusherError, Result};
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "docker-image-pusher",
    version = "0.1.3",
    about = "Push Docker images to registries with optimized large layer handling",
    long_about = None
)]
pub struct Args {
    /// Path to the Docker image tar file
    #[arg(short, long, value_name = "FILE")]
    pub file: String,

    /// Repository URL (e.g., https://registry.example.com/my-app:latest)
    #[arg(short, long, value_name = "URL")]
    pub repository_url: String,

    /// Registry username
    #[arg(short, long)]
    pub username: Option<String>,

    /// Registry password
    #[arg(short, long)]
    pub password: Option<String>,

    /// Timeout in seconds for uploads (default: 7200)
    #[arg(short = 't', long, default_value = "7200")]
    pub timeout: u64,

    /// Skip TLS certificate verification
    #[arg(long)]
    pub skip_tls: bool,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress all output except errors
    #[arg(short, long)]
    pub quiet: bool,

    /// Perform a dry run without actually uploading
    #[arg(long)]
    pub dry_run: bool,

    /// Threshold for large layer optimization in bytes (default: 1GB)
    #[arg(long, default_value = "1073741824")]
    pub large_layer_threshold: u64,

    /// Maximum concurrent uploads (default: 1)
    #[arg(long, default_value = "1")]
    pub max_concurrent: usize,

    /// Number of retry attempts for failed uploads (default: 3)
    #[arg(long, default_value = "3")]
    pub retry_attempts: usize,

    /// Enable exponential backoff for storage backend errors (default: true)
    #[arg(long, default_value = "true")]
    pub storage_error_backoff: bool,

    /// Skip uploading layers that already exist in the registry
    #[arg(long)]
    pub skip_existing: bool,

    /// Force upload even if layers already exist
    #[arg(long)]
    pub force_upload: bool,
}

impl Args {
    /// Parse command line arguments using clap
    /// This will exit the process if parsing fails (clap's default behavior)
    pub fn parse() -> Self {
        <Self as Parser>::parse()
    }

    /// Try to parse command line arguments, returning a Result
    /// Use this when you want to handle parsing errors yourself
    pub fn try_parse() -> Result<Self> {
        <Self as Parser>::try_parse()
            .map_err(|e| PusherError::Validation(format!("Failed to parse arguments: {}", e)))
    }
    pub fn validate(&self) -> Result<()> {
        // Use standardized file validation
        ValidationErrorHandler::validate_file_path(&self.file)?;

        // Use standardized URL validation
        ValidationErrorHandler::validate_repository_url(&self.repository_url)?;

        // Use standardized timeout validation
        ValidationErrorHandler::validate_timeout(self.timeout)?;

        // Use standardized credential validation
        ValidationErrorHandler::validate_credentials(&self.username, &self.password)?;

        // Validate large layer threshold
        if self.large_layer_threshold == 0 {
            return Err(PusherError::Validation(
                "Large layer threshold must be greater than 0".to_string(),
            ));
        }

        // Validate max concurrent
        if self.max_concurrent == 0 {
            return Err(PusherError::Validation(
                "Max concurrent uploads must be at least 1".to_string(),
            ));
        }

        if self.max_concurrent > 10 {
            return Err(PusherError::Validation(
                "Max concurrent uploads cannot exceed 10".to_string(),
            ));
        }

        // Validate retry attempts
        if self.retry_attempts > 10 {
            return Err(PusherError::Validation(
                "Retry attempts cannot exceed 10".to_string(),
            ));
        }

        // Validate credentials consistency
        match (&self.username, &self.password) {
            (Some(_), None) => {
                return Err(PusherError::Validation(
                    "Password is required when username is provided".to_string(),
                ));
            }
            (None, Some(_)) => {
                return Err(PusherError::Validation(
                    "Username is required when password is provided".to_string(),
                ));
            }
            _ => {} // Both provided or both None is fine
        }

        // Validate mutually exclusive flags
        if self.verbose && self.quiet {
            return Err(PusherError::Validation(
                "Cannot specify both --verbose and --quiet flags".to_string(),
            ));
        }

        if self.skip_existing && self.force_upload {
            return Err(PusherError::Validation(
                "Cannot specify both --skip-existing and --force-upload flags".to_string(),
            ));
        }

        Ok(())
    }

    pub fn get_file_size(&self) -> Result<u64> {
        let metadata = std::fs::metadata(&self.file)
            .map_err(|e| PusherError::Io(format!("Failed to read file metadata: {}", e)))?;
        Ok(metadata.len())
    }

    pub fn has_credentials(&self) -> bool {
        self.username.is_some() && self.password.is_some()
    }

    pub fn get_credentials(&self) -> Option<(String, String)> {
        match (&self.username, &self.password) {
            (Some(u), Some(p)) => Some((u.clone(), p.clone())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_missing_file() {
        let args = Args {
            file: "nonexistent.tar".to_string(),
            repository_url: "https://registry.example.com/test:latest".to_string(),
            username: None,
            password: None,
            timeout: 7200,
            skip_tls: false,
            verbose: false,
            quiet: false,
            dry_run: false,
            large_layer_threshold: 1073741824,
            max_concurrent: 1,
            retry_attempts: 3,
            skip_existing: false,
            force_upload: false,
            storage_error_backoff: true,
        };

        assert!(args.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_url() {
        let args = Args {
            file: "test.tar".to_string(),
            repository_url: "invalid-url".to_string(),
            username: None,
            password: None,
            timeout: 7200,
            skip_tls: false,
            verbose: false,
            quiet: false,
            dry_run: false,
            large_layer_threshold: 1073741824,
            max_concurrent: 1,
            retry_attempts: 3,
            skip_existing: false,
            force_upload: false,
            storage_error_backoff: true,
        };

        assert!(args.validate().is_err());
    }

    #[test]
    fn test_validation_credentials_mismatch() {
        let args = Args {
            file: "test.tar".to_string(),
            repository_url: "https://registry.example.com/test:latest".to_string(),
            username: Some("user".to_string()),
            password: None, // Missing password
            timeout: 7200,
            skip_tls: false,
            verbose: false,
            quiet: false,
            dry_run: false,
            large_layer_threshold: 1073741824,
            max_concurrent: 1,
            retry_attempts: 3,
            skip_existing: false,
            force_upload: false,
            storage_error_backoff: true,
        };

        assert!(args.validate().is_err());
    }
}
