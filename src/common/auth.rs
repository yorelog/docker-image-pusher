/// Common authentication and credential handling for push operations.
/// Provides utilities for resolving and validating credentials across different push workflows.

use oci_core::reference::Reference;

use crate::PusherError;
use super::state;

/// Resolves credentials for a given registry host.
/// First checks for explicit username/password, then falls back to stored credentials.
pub async fn resolve_registry_credentials(
    registry_host: &str,
    username: Option<String>,
    password: Option<String>,
) -> Result<(String, String), PusherError> {
    // Use explicit credentials if provided
    if let (Some(u), Some(p)) = (username, password) {
        return Ok((u, p));
    }

    // Fall back to stored credentials
    if let Ok(Some((u, p))) = state::load_credentials(registry_host).await {
        return Ok((u, p));
    }

    // If no credentials found, return error
    Err(PusherError::push_error(format!(
        "Credentials required for registry {}. Pass --username/--password or login first.",
        registry_host
    )))
}

/// Validates that target reference has required components for pushing.
#[allow(dead_code)]
pub fn validate_target_reference(target_ref: &Reference) -> Result<(), PusherError> {
    if target_ref.repository.is_empty() {
        return Err(PusherError::push_error(
            "Target repository is required (e.g., 'registry.example.com/myapp')".to_string(),
        ));
    }

    Ok(())
}

