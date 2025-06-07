//! Standardized error handling patterns

use crate::error::{RegistryError, Result};
use reqwest::StatusCode;
use std::time::Duration;

/// HTTP error handler for registry operations
pub struct HttpErrorHandler;

impl HttpErrorHandler {
    /// Handle upload-related HTTP errors with standardized messages
    pub fn handle_upload_error(
        status: StatusCode,
        error_text: &str,
        context: &str,
    ) -> RegistryError {
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

        RegistryError::Upload(error_msg)
    }

    /// Handle authentication-related HTTP errors
    pub fn handle_auth_error(status: StatusCode, error_text: &str) -> RegistryError {
        let error_msg = match status.as_u16() {
            400 => "Invalid token request parameters".to_string(),
            401 => "Invalid credentials provided".to_string(),
            403 => "Access denied - insufficient permissions".to_string(),
            404 => "Authentication endpoint not found".to_string(),
            _ => format!("Authentication failed (status {}): {}", status, error_text),
        };

        RegistryError::Auth(error_msg)
    }

    /// Handle registry-related HTTP errors
    pub fn handle_registry_error(
        status: StatusCode,
        error_text: &str,
        operation: &str,
    ) -> RegistryError {
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

        RegistryError::Registry(error_msg)
    }

    /// Handle streaming upload specific errors
    pub fn handle_streaming_error(status: StatusCode, error_text: &str) -> RegistryError {
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

        RegistryError::Upload(error_msg)
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
    pub fn handle_network_error(error: &reqwest::Error, context: &str) -> RegistryError {
        if error.is_timeout() {
            RegistryError::Network(format!("Timeout during {}", context))
        } else if error.is_connect() {
            RegistryError::Network(format!("Connection failed during {}", context))
        } else {
            RegistryError::Network(format!("Network error during {}: {}", context, error))
        }
    }
}

/// Validation error utilities
pub struct ValidationErrorHandler;

impl ValidationErrorHandler {
    /// Validate required fields
    pub fn validate_required_field(field_name: &str, value: &Option<String>) -> Result<()> {
        if value.is_none() || value.as_ref().unwrap().is_empty() {
            Err(RegistryError::Validation(format!(
                "{} is required",
                field_name
            )))
        } else {
            Ok(())
        }
    }
}

/// Macro for standardizing error context
#[macro_export]
macro_rules! with_context {
    ($result:expr, $context:expr) => {
        $result.map_err(|e| match e {
            RegistryError::Auth(msg) => RegistryError::Auth(format!("{}: {}", $context, msg)),
            RegistryError::Network(msg) => RegistryError::Network(format!("{}: {}", $context, msg)),
            RegistryError::Upload(msg) => RegistryError::Upload(format!("{}: {}", $context, msg)),
            RegistryError::Io(msg) => RegistryError::Io(format!("{}: {}", $context, msg)),
            RegistryError::Parse(msg) => RegistryError::Parse(format!("{}: {}", $context, msg)),
            RegistryError::Registry(msg) => {
                RegistryError::Registry(format!("{}: {}", $context, msg))
            }
            RegistryError::ImageParsing(msg) => {
                RegistryError::ImageParsing(format!("{}: {}", $context, msg))
            }
            RegistryError::Validation(msg) => {
                RegistryError::Validation(format!("{}: {}", $context, msg))
            }
            RegistryError::Cache { message, path } => RegistryError::Cache {
                message: format!("{}: {}", $context, message),
                path,
            },
            RegistryError::NotFound(msg) => {
                RegistryError::NotFound(format!("{}: {}", $context, msg))
            }
        })
    };
}
