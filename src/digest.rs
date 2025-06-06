//! SHA256 digest utilities for Docker image processing
//! 
//! This module provides centralized functionality for computing, validating,
//! and formatting SHA256 digests used throughout the Docker image pusher.

use sha2::Digest;
use crate::error::{Result, PusherError};

/// Standard SHA256 digest for empty files/layers
pub const EMPTY_LAYER_DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Docker digest with sha256: prefix for empty layers
pub const EMPTY_LAYER_DIGEST_FULL: &str = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Utilities for working with SHA256 digests in Docker context
pub struct DigestUtils;

impl DigestUtils {
    /// Compute SHA256 digest from byte data
    pub fn compute_sha256(data: &[u8]) -> String {
        let mut hasher = sha2::Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    /// Compute SHA256 digest from string data
    pub fn compute_sha256_str(data: &str) -> String {
        Self::compute_sha256(data.as_bytes())
    }

    /// Compute full Docker digest (with sha256: prefix) from byte data
    pub fn compute_docker_digest(data: &[u8]) -> String {
        format!("sha256:{}", Self::compute_sha256(data))
    }

    /// Compute full Docker digest (with sha256: prefix) from string data
    pub fn compute_docker_digest_str(data: &str) -> String {
        format!("sha256:{}", Self::compute_sha256_str(data))
    }

    /// Validate SHA256 hex string (64 characters, all hex)
    pub fn is_valid_sha256_hex(digest: &str) -> bool {
        digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit())
    }

    /// Validate full Docker digest format (sha256:xxxxx)
    pub fn is_valid_docker_digest(digest: &str) -> bool {
        if let Some(hex_part) = digest.strip_prefix("sha256:") {
            Self::is_valid_sha256_hex(hex_part)
        } else {
            false
        }
    }

    /// Normalize digest to full Docker format (add sha256: prefix if missing)
    pub fn normalize_digest(digest: &str) -> Result<String> {
        if digest.starts_with("sha256:") {
            // Validate existing format
            if digest.len() != 71 {
                return Err(PusherError::Validation(format!(
                    "Invalid SHA256 digest length: expected 71 characters, got {}", digest.len()
                )));
            }
            let hex_part = &digest[7..];
            if !Self::is_valid_sha256_hex(hex_part) {
                return Err(PusherError::Validation(format!(
                    "Invalid SHA256 digest format: contains non-hex characters"
                )));
            }
            Ok(digest.to_string())
        } else {
            // Add prefix and validate
            if !Self::is_valid_sha256_hex(digest) {
                return Err(PusherError::Validation(format!(
                    "Invalid SHA256 digest: expected 64 hex characters, got '{}'", digest
                )));
            }
            Ok(format!("sha256:{}", digest))
        }
    }

    /// Extract SHA256 hex part from full Docker digest
    pub fn extract_hex_part(digest: &str) -> Result<&str> {
        if let Some(hex_part) = digest.strip_prefix("sha256:") {
            if Self::is_valid_sha256_hex(hex_part) {
                Ok(hex_part)
            } else {
                Err(PusherError::Validation(format!(
                    "Invalid SHA256 hex part in digest: {}", digest
                )))
            }
        } else {
            Err(PusherError::Validation(format!(
                "Digest missing sha256: prefix: {}", digest
            )))
        }
    }

    /// Check if a digest represents an empty layer
    pub fn is_empty_layer_digest(digest: &str) -> bool {
        digest == EMPTY_LAYER_DIGEST_FULL || digest == EMPTY_LAYER_DIGEST
    }

    /// Get the standard empty layer digest with full Docker format
    pub fn empty_layer_digest() -> String {
        EMPTY_LAYER_DIGEST_FULL.to_string()
    }

    /// Verify data matches expected digest
    pub fn verify_data_integrity(data: &[u8], expected_digest: &str) -> Result<()> {
        let computed = Self::compute_sha256(data);
        let expected_hex = Self::extract_hex_part(expected_digest)?;
        
        if computed != expected_hex {
            return Err(PusherError::Validation(format!(
                "Data integrity check failed: expected {}, computed sha256:{}",
                expected_digest, computed
            )));
        }
        
        Ok(())
    }

    /// Extract digest from Docker layer path (various formats)
    pub fn extract_digest_from_layer_path(layer_path: &str) -> Option<String> {
        // Docker tar文件中的层路径通常是这样的格式：
        // "abc123def456.../layer.tar"
        // "blobs/sha256/abc123def456..."
        // "abc123def456.tar"
        
        // 首先尝试目录名格式 (最常见的格式)
        if let Some(slash_pos) = layer_path.find('/') {
            let digest_part = &layer_path[..slash_pos];
            if Self::is_valid_sha256_hex(digest_part) {
                return Some(digest_part.to_string());
            }
        }
          // 尝试blobs格式
        if layer_path.contains("blobs/sha256/") {
            if let Some(start) = layer_path.find("blobs/sha256/") {
                let after_prefix = &layer_path[start + 13..];
                let end = after_prefix.find('/').unwrap_or(after_prefix.len());
                let digest_part = &after_prefix[..end];
                if Self::is_valid_sha256_hex(digest_part) {
                    return Some(digest_part.to_string());
                }
            }
        }
        
        // 尝试文件名格式
        if let Some(dot_pos) = layer_path.rfind('.') {
            let digest_part = &layer_path[..dot_pos];
            if Self::is_valid_sha256_hex(digest_part) {
                return Some(digest_part.to_string());
            }
        }
        
        // 尝试完整路径作为digest (某些特殊情况)
        if Self::is_valid_sha256_hex(layer_path) {
            return Some(layer_path.to_string());
        }
        
        None
    }

    /// Generate a fallback digest from path when real digest cannot be extracted
    pub fn generate_path_based_digest(layer_path: &str) -> String {
        let mut hasher = sha2::Sha256::new();
        hasher.update(layer_path.as_bytes());
        format!("sha256:{:x}", hasher.finalize())
    }

    /// Format digest for display (truncated for readability)
    pub fn format_digest_short(digest: &str) -> String {
        if digest.len() > 23 {
            format!("{}...", &digest[..23])
        } else {
            digest.to_string()
        }
    }

    /// Batch validate multiple digests
    pub fn validate_digests(digests: &[&str]) -> Result<()> {
        for (i, digest) in digests.iter().enumerate() {
            if !Self::is_valid_docker_digest(digest) {
                return Err(PusherError::Validation(format!(
                    "Invalid digest format at index {}: {}", i, digest
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_sha256() {
        let data = b"hello world";
        let digest = DigestUtils::compute_sha256(data);
        assert_eq!(digest, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn test_compute_docker_digest() {
        let data = b"hello world";
        let digest = DigestUtils::compute_docker_digest(data);
        assert_eq!(digest, "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn test_empty_layer_digest() {
        let empty_data = b"";
        let computed = DigestUtils::compute_sha256(empty_data);
        assert_eq!(computed, EMPTY_LAYER_DIGEST);
    }

    #[test]
    fn test_validate_digest() {
        assert!(DigestUtils::is_valid_docker_digest("sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"));
        assert!(!DigestUtils::is_valid_docker_digest("sha256:invalid"));
        assert!(!DigestUtils::is_valid_docker_digest("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"));
    }

    #[test]
    fn test_normalize_digest() {
        let hex_only = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        let normalized = DigestUtils::normalize_digest(hex_only).unwrap();
        assert_eq!(normalized, "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn test_extract_digest_from_layer_path() {
        let paths = vec![
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9/layer.tar",
            "blobs/sha256/b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9.tar",
        ];

        for path in paths {
            let digest = DigestUtils::extract_digest_from_layer_path(path);
            assert_eq!(digest, Some("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_string()));
        }
    }

    #[test]
    fn test_verify_data_integrity() {
        let data = b"hello world";
        let digest = "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(DigestUtils::verify_data_integrity(data, digest).is_ok());

        let wrong_digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
        assert!(DigestUtils::verify_data_integrity(data, wrong_digest).is_err());
    }
}
