//! Common utilities and helper functions
//!
//! This module provides reusable utility functions that can be used across the codebase
//! to reduce code duplication and improve maintainability.

use crate::error::{RegistryError, Result};
use crate::logging::Logger;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Timing utilities
pub struct Timer {
    start: Instant,
    description: String,
}

impl Timer {
    /// Start a new timer
    pub fn start(description: impl Into<String>) -> Self {
        Self {
            start: Instant::now(),
            description: description.into(),
        }
    }
    
    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    
    /// Stop timer and return elapsed time
    pub fn stop(self) -> Duration {
        self.elapsed()
    }
    
    /// Log elapsed time using provided logger
    pub fn log_elapsed(&self, logger: &Logger) {
        logger.info(&format!("{} completed in {:.2}s", 
                            self.description, 
                            self.elapsed().as_secs_f64()));
    }
}

/// File and path utilities
pub struct PathUtils;

impl PathUtils {
    /// Ensure directory exists, create if not
    pub fn ensure_dir_exists(path: &Path) -> Result<()> {
        if !path.exists() {
            std::fs::create_dir_all(path)
                .map_err(|e| RegistryError::Io(format!("Failed to create directory {}: {}", 
                                                       path.display(), e)))?;
        }
        Ok(())
    }
    
    /// Get file size safely
    pub fn get_file_size(path: &Path) -> Result<u64> {
        std::fs::metadata(path)
            .map(|m| m.len())
            .map_err(|e| RegistryError::Io(format!("Failed to get file size for {}: {}", 
                                                   path.display(), e)))
    }
    
    /// Check if file exists and is readable
    pub fn is_readable_file(path: &Path) -> bool {
        path.exists() && path.is_file() && std::fs::File::open(path).is_ok()
    }
    
    /// Get file extension safely
    pub fn get_extension(path: &Path) -> Option<&str> {
        path.extension()?.to_str()
    }
    
    /// Construct cache path for repository/reference
    pub fn cache_path(cache_dir: &Path, repository: &str, reference: &str) -> PathBuf {
        cache_dir.join("manifests").join(repository).join(reference)
    }
    
    /// Construct blob path for digest
    pub fn blob_path(cache_dir: &Path, digest: &str) -> PathBuf {
        if digest.starts_with("sha256:") {
            cache_dir.join("blobs").join("sha256").join(&digest[7..])
        } else {
            cache_dir.join("blobs").join("sha256").join(digest)
        }
    }
}

/// Format utilities
pub struct FormatUtils;

impl FormatUtils {
    /// Format bytes as human readable size
    pub fn format_bytes(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        
        if bytes == 0 {
            return "0 B".to_string();
        }
        
        let mut size = bytes as f64;
        let mut unit_index = 0;
        
        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }
        
        if unit_index == 0 {
            format!("{} {}", size as u64, UNITS[unit_index])
        } else {
            format!("{:.2} {}", size, UNITS[unit_index])
        }
    }
    
    /// Format speed in bytes/sec
    pub fn format_speed(bytes_per_sec: u64) -> String {
        format!("{}/s", Self::format_bytes(bytes_per_sec))
    }
    
    /// Format duration as human readable
    pub fn format_duration(duration: Duration) -> String {
        let total_secs = duration.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        
        if hours > 0 {
            format!("{}h{}m{}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m{}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }
    
    /// Format percentage
    pub fn format_percentage(value: f64) -> String {
        format!("{:.1}%", value)
    }
    
    /// Truncate digest for display
    pub fn truncate_digest(digest: &str, len: usize) -> String {
        if digest.starts_with("sha256:") {
            let hash_part = &digest[7..];
            if hash_part.len() > len {
                format!("sha256:{}...", &hash_part[..len])
            } else {
                digest.to_string()
            }
        } else if digest.len() > len {
            format!("{}...", &digest[..len])
        } else {
            digest.to_string()
        }
    }
}

/// Progress calculation utilities
pub struct ProgressUtils;

impl ProgressUtils {
    /// Calculate progress percentage
    pub fn calculate_percentage(processed: u64, total: u64) -> f64 {
        if total == 0 {
            0.0
        } else {
            (processed as f64 / total as f64) * 100.0
        }
    }
    
    /// Calculate speed (bytes per second)
    pub fn calculate_speed(bytes: u64, duration: Duration) -> u64 {
        let secs = duration.as_secs_f64();
        if secs > 0.0 {
            (bytes as f64 / secs) as u64
        } else {
            0
        }
    }
    
    /// Estimate remaining time
    pub fn estimate_remaining_time(processed: u64, total: u64, elapsed: Duration) -> Option<Duration> {
        if processed == 0 || processed >= total {
            return None;
        }
        
        let rate = processed as f64 / elapsed.as_secs_f64();
        let remaining_bytes = total - processed;
        let remaining_secs = remaining_bytes as f64 / rate;
        
        Some(Duration::from_secs_f64(remaining_secs))
    }
    
    /// Create progress bar string
    pub fn create_progress_bar(percentage: f64, width: usize) -> String {
        let filled = ((percentage / 100.0) * width as f64) as usize;
        let empty = width.saturating_sub(filled);
        
        format!("[{}{}]", 
                "█".repeat(filled), 
                "░".repeat(empty))
    }
}

/// Validation utilities
pub struct ValidationUtils;

impl ValidationUtils {
    /// Validate repository name
    pub fn validate_repository(repository: &str) -> Result<()> {
        if repository.is_empty() {
            return Err(RegistryError::Validation("Repository cannot be empty".to_string()));
        }
        
        // Basic repository name validation
        if repository.contains("//") || repository.starts_with('/') || repository.ends_with('/') {
            return Err(RegistryError::Validation(
                "Invalid repository format".to_string()
            ));
        }
        
        Ok(())
    }
    
    /// Validate reference (tag or digest)
    pub fn validate_reference(reference: &str) -> Result<()> {
        if reference.is_empty() {
            return Err(RegistryError::Validation("Reference cannot be empty".to_string()));
        }
        
        // Basic reference validation
        if reference.contains(' ') || reference.contains('\t') || reference.contains('\n') {
            return Err(RegistryError::Validation(
                "Reference cannot contain whitespace".to_string()
            ));
        }
        
        Ok(())
    }
    
    /// Validate digest format
    pub fn validate_digest(digest: &str) -> Result<()> {
        if !digest.starts_with("sha256:") {
            return Err(RegistryError::Validation(
                "Digest must start with 'sha256:'".to_string()
            ));
        }
        
        let hash_part = &digest[7..];
        if hash_part.len() != 64 {
            return Err(RegistryError::Validation(
                "SHA256 digest must be 64 characters".to_string()
            ));
        }
        
        if !hash_part.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(RegistryError::Validation(
                "Digest must contain only hexadecimal characters".to_string()
            ));
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_format_bytes() {
        assert_eq!(FormatUtils::format_bytes(0), "0 B");
        assert_eq!(FormatUtils::format_bytes(1024), "1.00 KB");
        assert_eq!(FormatUtils::format_bytes(1536), "1.50 KB");
        assert_eq!(FormatUtils::format_bytes(1048576), "1.00 MB");
    }

    #[test]
    fn test_progress_percentage() {
        assert_eq!(ProgressUtils::calculate_percentage(0, 100), 0.0);
        assert_eq!(ProgressUtils::calculate_percentage(50, 100), 50.0);
        assert_eq!(ProgressUtils::calculate_percentage(100, 100), 100.0);
        assert_eq!(ProgressUtils::calculate_percentage(0, 0), 0.0);
    }

    #[test]
    fn test_validate_repository() {
        assert!(ValidationUtils::validate_repository("valid/repo").is_ok());
        assert!(ValidationUtils::validate_repository("").is_err());
        assert!(ValidationUtils::validate_repository("//invalid").is_err());
        assert!(ValidationUtils::validate_repository("/invalid").is_err());
    }

    #[test]
    fn test_validate_digest() {
        let valid_digest = "sha256:abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        assert!(ValidationUtils::validate_digest(valid_digest).is_ok());
        assert!(ValidationUtils::validate_digest("invalid").is_err());
        assert!(ValidationUtils::validate_digest("sha256:invalid").is_err());
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start("test operation");
        std::thread::sleep(Duration::from_millis(10));
        assert!(timer.elapsed() >= Duration::from_millis(10));
    }
}
