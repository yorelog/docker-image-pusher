//! Shared tar processing utilities to eliminate duplication

use crate::error::{Result, PusherError};
use std::path::Path;
use std::io::Read;
use std::fs::File;
use tar::Archive;

/// Tar processing utilities for layer extraction and offset calculation
pub struct TarUtils;

impl TarUtils {
    /// Extract layer data from tar archive
    pub fn extract_layer_data(tar_path: &Path, layer_path: &str) -> Result<Vec<u8>> {
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        for entry_result in archive.entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))? {
            let mut entry = entry_result
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entry: {}", e)))?;

            let path = entry.path()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry path: {}", e)))?
                .to_string_lossy()
                .to_string();

            if path == layer_path {
                let mut data = Vec::new();
                entry.read_to_end(&mut data)
                    .map_err(|e| PusherError::ImageParsing(format!("Failed to read layer data: {}", e)))?;
                return Ok(data);
            }
        }

        Err(PusherError::ImageParsing(format!("Layer '{}' not found in tar archive", layer_path)))
    }

    /// Find the offset of a layer within the tar archive
    pub fn find_layer_offset(tar_path: &Path, layer_path: &str) -> Result<u64> {
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        let mut current_offset = 0u64;

        for entry_result in archive.entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))? {
            let entry = entry_result
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entry: {}", e)))?;

            let path = entry.path()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry path: {}", e)))?
                .to_string_lossy()
                .to_string();

            if path == layer_path {
                return Ok(current_offset);
            }

            // Calculate entry size including headers (simplified calculation)
            let size = entry.header().size()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry size: {}", e)))?;
            
            current_offset += size + 512; // 512 bytes for TAR header (simplified)
        }

        Err(PusherError::ImageParsing(format!("Layer '{}' not found for offset calculation", layer_path)))
    }

    /// Get a list of all entries in the tar archive with their sizes
    pub fn list_tar_entries(tar_path: &Path) -> Result<Vec<(String, u64)>> {
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        let mut entries = Vec::new();

        for entry_result in archive.entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))? {
            let entry = entry_result
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entry: {}", e)))?;

            let path = entry.path()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry path: {}", e)))?
                .to_string_lossy()
                .to_string();

            let size = entry.header().size()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry size: {}", e)))?;

            entries.push((path, size));
        }

        Ok(entries)
    }

    /// Validate that a tar archive is readable and properly formatted
    pub fn validate_tar_archive(tar_path: &Path) -> Result<()> {
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        // Try to read the first few entries to validate format
        let mut entry_count = 0;
        for entry_result in archive.entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))? {
            let entry = entry_result
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entry: {}", e)))?;

            // Validate that we can read the path
            let _ = entry.path()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry path: {}", e)))?;

            entry_count += 1;
            
            // Only validate the first 10 entries for performance
            if entry_count >= 10 {
                break;
            }
        }

        if entry_count == 0 {
            return Err(PusherError::ImageParsing("Tar archive appears to be empty".to_string()));
        }

        Ok(())
    }
}
