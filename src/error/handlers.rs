//! Standardized error handling patterns to eliminate duplication

use crate::error::{PusherError, Result};
use reqwest::StatusCode;
use std::time::Duration;

/// Standard error handler for HTTP responses
pub struct HttpErrorHandler;

impl HttpErrorHandler {
    /// Handle upload-related HTTP errors with standardized messages
    pub fn handle_upload_error(status: StatusCode, error_text: &str, context: &str) -> PusherError {
        let error_msg = match status.as_u16() {
            400 => {
                if error_text.contains("exist blob require digest") {
                    format!(
                        "Digest validation failed - Registry reports blob exists but digest mismatch: {}",
                        error_text
                    )
                } else if error_text.contains("BAD_REQUEST") {
                    format!(
                        "Bad request - Check digest format and data integrity: {}",
                        error_text
                    )
                } else {
                    format!("Bad request during {}: {}", context, error_text)
                }
            }
            401 => format!("Authentication failed during {}: {}", context, error_text),
            403 => format!("Permission denied for {}: {}", context, error_text),
            404 => format!(
                "Repository not found or {} session expired: {}",
                context, error_text
            ),
            409 => format!(
                "Conflict - Blob already exists with different digest: {}",
                error_text
            ),
            413 => format!("File too large for {}: {}", context, error_text),
            422 => format!("Invalid digest or data for {}: {}", context, error_text),
            500 => format!("Registry server error during {}: {}", context, error_text),
            502 | 503 => format!("Registry unavailable during {}: {}", context, error_text),
            507 => format!("Registry out of storage during {}: {}", context, error_text),
            508 => format!("Streaming {} timeout: {}", context, error_text),
            _ => format!("{} failed (status {}): {}", context, status, error_text),
        };

        PusherError::Upload(error_msg)
    }

    /// Handle authentication-related HTTP errors
    pub fn handle_auth_error(status: StatusCode, error_text: &str) -> PusherError {
        let error_msg = match status.as_u16() {
            400 => "Invalid token request parameters".to_string(),
            401 => "Invalid credentials provided".to_string(),
            403 => "Access denied - insufficient permissions".to_string(),
            404 => "Authentication endpoint not found".to_string(),
            _ => format!("Authentication failed (status {}): {}", status, error_text),
        };

        PusherError::Authentication(error_msg)
    }

    /// Handle registry-related HTTP errors
    pub fn handle_registry_error(
        status: StatusCode,
        error_text: &str,
        operation: &str,
    ) -> PusherError {
        let error_msg = match status.as_u16() {
            401 => format!(
                "Unauthorized to perform {} operation: {}",
                operation, error_text
            ),
            403 => format!(
                "Forbidden: insufficient permissions for {}: {}",
                operation, error_text
            ),
            404 => format!("Resource not found for {}: {}", operation, error_text),
            429 => format!("Rate limited during {}: {}", operation, error_text),
            500 => format!("Registry server error during {}: {}", operation, error_text),
            502 | 503 => format!("Registry unavailable for {}: {}", operation, error_text),
            _ => format!("{} failed (status {}): {}", operation, status, error_text),
        };

        PusherError::Registry(error_msg)
    }

    /// Handle streaming upload specific errors
    pub fn handle_streaming_error(status: StatusCode, error_text: &str) -> PusherError {
        let error_msg = match status.as_u16() {
            400 => {
                if error_text.contains("DIGEST_INVALID") {
                    "Digest validation failed - Registry reports uploaded content doesn't match expected digest".to_string()
                } else {
                    format!("Bad request during streaming upload: {}", error_text)
                }
            }
            413 => "File too large for registry".to_string(),
            507 => "Insufficient storage space on registry".to_string(),
            401 => "Authentication failed during upload".to_string(),
            403 => "Permission denied for upload".to_string(),
            408 | 504 => "Streaming upload timeout".to_string(),
            500 => {
                if error_text.contains("s3aws") || error_text.contains("DriverName") {
                    format!(
                        "Registry storage backend error (S3): {}. This is typically a temporary issue with the registry's storage system",
                        error_text
                    )
                } else {
                    format!("Registry internal server error: {}", error_text)
                }
            }
            502 | 503 => format!("Registry temporarily unavailable: {}", error_text),
            _ => format!(
                "Streaming upload failed (status {}): {}",
                status, error_text
            ),
        };

        PusherError::Upload(error_msg)
    }

    /// Check if error indicates a storage backend issue that might be temporary
    pub fn is_storage_backend_error(error_text: &str) -> bool {
        error_text.contains("s3aws")
            || error_text.contains("DriverName")
            || error_text.contains("storage backend")
            || error_text.contains("500 Internal Server Error")
    }

    /// Get suggested retry delay for storage backend errors (exponential backoff)
    pub fn get_storage_error_retry_delay(attempt: u32, base_delay_secs: u64) -> Duration {
        let backoff_multiplier = 2_u64.pow(attempt.min(4));
        Duration::from_secs(base_delay_secs * backoff_multiplier)
    }
}

/// Network error categorization and handling
pub struct NetworkErrorHandler;

impl NetworkErrorHandler {
    /// Categorize and format network errors with helpful context
    pub fn handle_network_error(error: &reqwest::Error, context: &str) -> PusherError {
        if error.is_timeout() {
            PusherError::Timeout(format!("{} timeout: {}", context, error))
        } else if error.is_connect() {
            PusherError::Network(format!("Connection error during {}: {}", context, error))
        } else if error.to_string().contains("dns") {
            PusherError::Network(format!("DNS resolution error for {}: {}", context, error))
        } else if error.to_string().contains("certificate") {
            PusherError::Network(format!(
                "TLS certificate error during {}: {}",
                context, error
            ))
        } else {
            PusherError::Network(format!("{} network error: {}", context, error))
        }
    }
}

/// Validation error utilities
pub struct ValidationErrorHandler;

impl ValidationErrorHandler {
    /// Standard file validation error messages
    pub fn validate_file_path(file_path: &str) -> Result<()> {
        use std::path::Path;

        let path = Path::new(file_path);

        if !path.exists() {
            return Err(PusherError::Validation(format!(
                "Input file does not exist: {}",
                file_path
            )));
        }

        if !path.is_file() {
            return Err(PusherError::Validation(format!(
                "Input path is not a file: {}",
                file_path
            )));
        }

        // Check file extension
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

        if !matches!(extension.to_lowercase().as_str(), "tar" | "tar.gz" | "tgz") {
            return Err(PusherError::Validation(format!(
                "Input file must be a tar archive (.tar, .tar.gz, or .tgz): {}",
                file_path
            )));
        }

        Ok(())
    }

    /// Standard URL validation
    pub fn validate_repository_url(url: &str) -> Result<()> {
        if url.is_empty() {
            return Err(PusherError::Validation(
                "Repository URL cannot be empty".to_string(),
            ));
        }

        if !url.contains("://") {
            return Err(PusherError::Validation(
                "Repository URL must include protocol (http:// or https://)".to_string(),
            ));
        }

        Ok(())
    }

    /// Standard credential validation
    pub fn validate_credentials(
        username: &Option<String>,
        password: &Option<String>,
    ) -> Result<()> {
        match (username, password) {
            (Some(_), None) => Err(PusherError::Validation(
                "Password is required when username is provided".to_string(),
            )),
            (None, Some(_)) => Err(PusherError::Validation(
                "Username is required when password is provided".to_string(),
            )),
            _ => Ok(()), // Both provided or both None is fine
        }
    }

    /// Standard numeric range validation
    pub fn validate_timeout(timeout: u64) -> Result<()> {
        if timeout == 0 {
            return Err(PusherError::Validation(
                "Timeout must be greater than 0".to_string(),
            ));
        }

        if timeout > 86400 {
            // 24 hours
            return Err(PusherError::Validation(
                "Timeout cannot exceed 24 hours (86400 seconds)".to_string(),
            ));
        }

        Ok(())
    }
}

/// Macro for standardizing error context
#[macro_export]
macro_rules! with_context {
    ($result:expr, $context:expr) => {
        $result.map_err(|e| match e {
            PusherError::Config(msg) => PusherError::Config(format!("{}: {}", $context, msg)),
            PusherError::Authentication(msg) => {
                PusherError::Authentication(format!("{}: {}", $context, msg))
            }
            PusherError::Network(msg) => PusherError::Network(format!("{}: {}", $context, msg)),
            PusherError::Upload(msg) => PusherError::Upload(format!("{}: {}", $context, msg)),
            PusherError::Io(msg) => PusherError::Io(format!("{}: {}", $context, msg)),
            PusherError::Parse(msg) => PusherError::Parse(format!("{}: {}", $context, msg)),
            PusherError::Registry(msg) => PusherError::Registry(format!("{}: {}", $context, msg)),
            PusherError::ImageParsing(msg) => {
                PusherError::ImageParsing(format!("{}: {}", $context, msg))
            }
            PusherError::Validation(msg) => {
                PusherError::Validation(format!("{}: {}", $context, msg))
            }
            PusherError::Timeout(msg) => PusherError::Timeout(format!("{}: {}", $context, msg)),
        })
    };
}
