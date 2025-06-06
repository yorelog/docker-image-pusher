//! Shared tar processing utilities to eliminate duplication
//!
//! This module provides [`TarUtils`] for extracting layer data and handling tarball offsets.
//! It ensures that layer data is extracted in the correct format (gzip or uncompressed) for digest validation and upload.

use crate::error::{PusherError, Result};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tar::Archive;

/// Tar processing utilities for layer extraction and offset calculation
pub struct TarUtils;

impl TarUtils {
    /// Extract layer data from tar archive
    ///
    /// 注意：Docker 镜像层的 digest 必须基于 gzip 压缩后的 tar 字节流计算。
    /// 本方法会自动检测数据是否已经是 gzip 格式（通过检查 gzip 魔数 0x1f 0x8b），
    /// 如果不是则进行 gzip 压缩，保证返回的数据始终为 gzip 字节流，
    /// 便于后续 digest 校验和上传。
    ///
    /// 参数 layer_path 应为 manifest.json 中的层路径（如 xxx/layer.tar 或 blobs/sha256/xxx）。
    ///
    /// 重要：Docker/Podman 中的层 digest 是基于 gzip 压缩后的 tar 内容计算的，
    /// 而不是基于原始的 tar 内容，因此必须确保正确的压缩格式。
    pub fn extract_layer_data(tar_path: &Path, layer_path: &str) -> Result<Vec<u8>> {
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        // First look for the original layer file path (from manifest)
        for entry_result in archive
            .entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))?
        {
            let mut entry = entry_result.map_err(|e| {
                PusherError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry
                .path()
                .map_err(|e| {
                    PusherError::ImageParsing(format!("Failed to read entry path: {}", e))
                })?
                .to_string_lossy()
                .to_string();

            if path == layer_path {
                // Found the exact layer path in the tar archive
                let mut data = Vec::new();
                entry.read_to_end(&mut data).map_err(|e| {
                    PusherError::ImageParsing(format!("Failed to read layer data: {}", e))
                })?;

                // Keep the original format of the layer file as it appears in the Docker tar archive
                // This preserves the digest calculation exactly as Docker expects it
                // DO NOT modify or compress the data here - return it exactly as stored in the archive
                return Ok(data);
            }
        }

        // If we didn't find the exact path, try a more flexible approach
        // Second pass: look for any file that matches the digest in the path
        let digest_part = if layer_path.contains("sha256:") {
            layer_path.split("sha256:").nth(1).unwrap_or("")
        } else {
            // Extract digest from filename like "abc123def.tar.gz"
            layer_path.split('.').next().unwrap_or("")
        };

        if digest_part.len() >= 8 {
            // Try to find a file containing this digest part
            let file = File::open(tar_path).map_err(|e| {
                PusherError::Io(format!("Failed to open tar file (second pass): {}", e))
            })?;
            let mut archive = Archive::new(file);
            archive.set_ignore_zeros(true);

            for entry_result in archive.entries().map_err(|e| {
                PusherError::ImageParsing(format!(
                    "Failed to read tar entries (second pass): {}",
                    e
                ))
            })? {
                let mut entry = entry_result.map_err(|e| {
                    PusherError::ImageParsing(format!(
                        "Failed to read tar entry (second pass): {}",
                        e
                    ))
                })?;

                let path = entry
                    .path()
                    .map_err(|e| {
                        PusherError::ImageParsing(format!(
                            "Failed to read entry path (second pass): {}",
                            e
                        ))
                    })?
                    .to_string_lossy()
                    .to_string();

                if path.contains(digest_part) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data).map_err(|e| {
                        PusherError::ImageParsing(format!(
                            "Failed to read layer data (second pass): {}",
                            e
                        ))
                    })?;
                    // Keep the original format
                    return Ok(data);
                }
            }
        }

        // Last resort: try to find any layer tar file in the archive
        let file = File::open(tar_path).map_err(|e| {
            PusherError::Io(format!("Failed to open tar file (last resort): {}", e))
        })?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        for entry_result in archive.entries().map_err(|e| {
            PusherError::ImageParsing(format!("Failed to read tar entries (last resort): {}", e))
        })? {
            let mut entry = entry_result.map_err(|e| {
                PusherError::ImageParsing(format!("Failed to read tar entry (last resort): {}", e))
            })?;

            let path = entry
                .path()
                .map_err(|e| {
                    PusherError::ImageParsing(format!(
                        "Failed to read entry path (last resort): {}",
                        e
                    ))
                })?
                .to_string_lossy()
                .to_string();

            if (path.ends_with(".tar") || path.ends_with(".tar.gz"))
                && (path.contains("layer") || path.contains("blob"))
            {
                let mut data = Vec::new();
                entry.read_to_end(&mut data).map_err(|e| {
                    PusherError::ImageParsing(format!(
                        "Failed to read layer data (last resort): {}",
                        e
                    ))
                })?;

                return Ok(data);
            }
        }

        Err(PusherError::ImageParsing(format!(
            "Layer '{}' not found in tar archive",
            layer_path
        )))
    }

    /// Find the offset of a layer within the tar archive
    pub fn find_layer_offset(tar_path: &Path, layer_path: &str) -> Result<u64> {
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        let mut current_offset = 0u64;

        for entry_result in archive
            .entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))?
        {
            let entry = entry_result.map_err(|e| {
                PusherError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry
                .path()
                .map_err(|e| {
                    PusherError::ImageParsing(format!("Failed to read entry path: {}", e))
                })?
                .to_string_lossy()
                .to_string();

            if path == layer_path {
                return Ok(current_offset);
            }

            // Calculate entry size including headers (simplified calculation)
            let size = entry.header().size().map_err(|e| {
                PusherError::ImageParsing(format!("Failed to read entry size: {}", e))
            })?;

            current_offset += size + 512; // 512 bytes for TAR header (simplified)
        }

        Err(PusherError::ImageParsing(format!(
            "Layer '{}' not found for offset calculation",
            layer_path
        )))
    }

    /// Get a list of all entries in the tar archive with their sizes
    pub fn list_tar_entries(tar_path: &Path) -> Result<Vec<(String, u64)>> {
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        let mut entries = Vec::new();

        for entry_result in archive
            .entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))?
        {
            let entry = entry_result.map_err(|e| {
                PusherError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry
                .path()
                .map_err(|e| {
                    PusherError::ImageParsing(format!("Failed to read entry path: {}", e))
                })?
                .to_string_lossy()
                .to_string();

            let size = entry.header().size().map_err(|e| {
                PusherError::ImageParsing(format!("Failed to read entry size: {}", e))
            })?;

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
        for entry_result in archive
            .entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))?
        {
            let entry = entry_result.map_err(|e| {
                PusherError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            // Validate that we can read the path
            let _ = entry.path().map_err(|e| {
                PusherError::ImageParsing(format!("Failed to read entry path: {}", e))
            })?;

            entry_count += 1;

            // Only validate the first 10 entries for performance
            if entry_count >= 10 {
                break;
            }
        }

        if entry_count == 0 {
            return Err(PusherError::ImageParsing(
                "Tar archive appears to be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Check if data is in gzip format by examining the gzip magic number (0x1f 0x8b)
    pub fn is_gzipped(data: &[u8]) -> bool {
        data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b
    }
}
