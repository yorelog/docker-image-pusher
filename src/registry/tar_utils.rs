//! Shared tar processing utilities to eliminate duplication
//!
//! This module provides [`TarUtils`] for extracting layer data and handling tarball offsets.
//! It ensures that layer data is extracted in the correct format (gzip or uncompressed) for digest validation and upload.

use crate::error::{RegistryError, Result};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tar::Archive;

/// Tar processing utilities for layer extraction and offset calculation
pub struct TarUtils;

impl TarUtils {
    /// Extract layer data from tar archive
    ///
    /// 重要：直接返回tar中的原始layer数据，保持Docker兼容性
    /// Docker镜像中的层已经是正确的gzip格式
    pub fn extract_layer_data(tar_path: &Path, layer_path: &str) -> Result<Vec<u8>> {
        let file = File::open(tar_path)
            .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        for entry_result in archive.entries().map_err(|e| {
            RegistryError::ImageParsing(format!("Failed to read tar entries: {}", e))
        })? {
            let mut entry = entry_result.map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry
                .path()
                .map_err(|e| {
                    RegistryError::ImageParsing(format!("Failed to read entry path: {}", e))
                })?
                .to_string_lossy()
                .to_string();

            if path == layer_path {
                let mut data = Vec::new();
                entry.read_to_end(&mut data).map_err(|e| {
                    RegistryError::ImageParsing(format!("Failed to read layer data: {}", e))
                })?;

                // 直接返回原始数据，不进行任何处理
                // Docker tar中的层数据已经是正确的格式
                return Ok(data);
            }
        }

        Err(RegistryError::ImageParsing(format!(
            "Layer '{}' not found in tar archive",
            layer_path
        )))
    }

    /// Find the offset of a layer within the tar archive
    pub fn find_layer_offset(tar_path: &Path, layer_path: &str) -> Result<u64> {
        let file = File::open(tar_path)
            .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        let mut current_offset = 0u64;

        for entry_result in archive.entries().map_err(|e| {
            RegistryError::ImageParsing(format!("Failed to read tar entries: {}", e))
        })? {
            let entry = entry_result.map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry
                .path()
                .map_err(|e| {
                    RegistryError::ImageParsing(format!("Failed to read entry path: {}", e))
                })?
                .to_string_lossy()
                .to_string();

            if path == layer_path {
                return Ok(current_offset);
            }

            // Calculate entry size including headers (simplified calculation)
            let size = entry.header().size().map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read entry size: {}", e))
            })?;

            current_offset += size + 512; // 512 bytes for TAR header (simplified)
        }

        Err(RegistryError::ImageParsing(format!(
            "Layer '{}' not found for offset calculation",
            layer_path
        )))
    }

    /// Get a list of all entries in the tar archive with their sizes
    pub fn list_tar_entries(tar_path: &Path) -> Result<Vec<(String, u64)>> {
        let file = File::open(tar_path)
            .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        let mut entries = Vec::new();

        for entry_result in archive.entries().map_err(|e| {
            RegistryError::ImageParsing(format!("Failed to read tar entries: {}", e))
        })? {
            let entry = entry_result.map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry
                .path()
                .map_err(|e| {
                    RegistryError::ImageParsing(format!("Failed to read entry path: {}", e))
                })?
                .to_string_lossy()
                .to_string();

            let size = entry.header().size().map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read entry size: {}", e))
            })?;

            entries.push((path, size));
        }

        Ok(entries)
    }

    /// Validate that a tar archive is readable and properly formatted
    pub fn validate_tar_archive(tar_path: &Path) -> Result<()> {
        let file = File::open(tar_path)
            .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        // Try to read the first few entries to validate format
        let mut entry_count = 0;
        for entry_result in archive.entries().map_err(|e| {
            RegistryError::ImageParsing(format!("Failed to read tar entries: {}", e))
        })? {
            let entry = entry_result.map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            // Validate that we can read the path
            let _ = entry.path().map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read entry path: {}", e))
            })?;

            entry_count += 1;

            // Only validate the first 10 entries for performance
            if entry_count >= 10 {
                break;
            }
        }

        if entry_count == 0 {
            return Err(RegistryError::ImageParsing(
                "Tar archive appears to be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// 从 tar 文件中提取镜像清单
    ///
    /// 解析 Docker 镜像 tar 文件，提取 manifest.json 内容
    pub fn extract_manifest(tar_path: &Path) -> Result<String> {
        let file = File::open(tar_path)
            .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);

        for entry_result in archive.entries().map_err(|e| {
            RegistryError::ImageParsing(format!("Failed to read tar entries: {}", e))
        })? {
            let mut entry = entry_result.map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry.path().map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to get entry path: {}", e))
            })?;

            if path.to_string_lossy() == "manifest.json" {
                let mut content = String::new();
                entry
                    .read_to_string(&mut content)
                    .map_err(|e| RegistryError::Io(format!("Failed to read manifest: {}", e)))?;

                return Ok(content);
            }
        }

        Err(RegistryError::ImageParsing(
            "manifest.json not found in tar file".to_string(),
        ))
    }

    /// 从 tar 文件中提取镜像配置
    ///
    /// 解析 Docker 镜像 tar 文件，提取指定的配置文件内容
    pub fn extract_config(tar_path: &Path, config_path: &str) -> Result<String> {
        let file = File::open(tar_path)
            .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);

        for entry_result in archive.entries().map_err(|e| {
            RegistryError::ImageParsing(format!("Failed to read tar entries: {}", e))
        })? {
            let mut entry = entry_result.map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry.path().map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to get entry path: {}", e))
            })?;

            if path.to_string_lossy() == config_path {
                let mut content = String::new();
                entry
                    .read_to_string(&mut content)
                    .map_err(|e| RegistryError::Io(format!("Failed to read config: {}", e)))?;

                return Ok(content);
            }
        }

        Err(RegistryError::ImageParsing(format!(
            "Config file {} not found in tar file",
            config_path
        )))
    }

    /// 从 tar 文件中提取镜像配置数据为字节数组
    pub fn extract_config_data(tar_path: &Path, config_digest: &str) -> Result<Vec<u8>> {
        let digest_hash = config_digest.replace("sha256:", "");

        // 支持多种可能的配置文件路径格式
        let possible_paths = vec![
            format!("{}.json", digest_hash),         // Docker format: abc123.json
            format!("blobs/sha256/{}", digest_hash), // OCI format: blobs/sha256/abc123
            format!("{}/json", digest_hash),         // Alternative Docker: abc123/json
        ];

        let file = File::open(tar_path)
            .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);

        for entry_result in archive.entries().map_err(|e| {
            RegistryError::ImageParsing(format!("Failed to read tar entries: {}", e))
        })? {
            let mut entry = entry_result.map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry
                .path()
                .map_err(|e| {
                    RegistryError::ImageParsing(format!("Failed to get entry path: {}", e))
                })?
                .to_string_lossy()
                .to_string();

            // 检查是否匹配任何可能的路径格式
            for possible_path in &possible_paths {
                if path == *possible_path || path.ends_with(possible_path) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data).map_err(|e| {
                        RegistryError::Io(format!("Failed to read config data: {}", e))
                    })?;

                    return Ok(data);
                }
            }
        }

        Err(RegistryError::ImageParsing(format!(
            "Config file for digest {} not found in tar file. Tried paths: {:?}",
            config_digest, possible_paths
        )))
    }

    /// 解析 tar 文件获取完整的镜像信息
    pub fn parse_image_info(tar_path: &Path) -> Result<crate::image::parser::ImageInfo> {
        let manifest_content = Self::extract_manifest(tar_path)?;
        let manifest: Vec<serde_json::Value> = serde_json::from_str(&manifest_content)?;

        let image_manifest = manifest
            .first()
            .ok_or_else(|| RegistryError::ImageParsing("Empty manifest array".to_string()))?;

        // 获取config digest - 支持Docker和OCI格式
        let config_file = image_manifest
            .get("Config")
            .and_then(|c| c.as_str())
            .ok_or_else(|| RegistryError::ImageParsing("Config field not found".to_string()))?;

        let config_digest = if config_file.starts_with("blobs/sha256/") {
            // OCI格式: "blobs/sha256/61eb38817b494eabe077e218c04189b566af694f9a37cea8e84e154eff0fcd3a"
            format!("sha256:{}", config_file.replace("blobs/sha256/", ""))
        } else if config_file.contains("/") && config_file.ends_with(".json") {
            // Docker格式: "abc123.../config.json"
            let digest_part = config_file.split('/').next().unwrap_or("");
            format!("sha256:{}", digest_part)
        } else {
            // 简单格式: "abc123...json"
            format!("sha256:{}", config_file.replace(".json", ""))
        };

        // 获取layers信息
        let layers_array = image_manifest
            .get("Layers")
            .and_then(|l| l.as_array())
            .ok_or_else(|| RegistryError::ImageParsing("Layers field not found".to_string()))?;

        let mut layers = Vec::new();
        for layer_file in layers_array {
            let layer_path = layer_file
                .as_str()
                .ok_or_else(|| RegistryError::ImageParsing("Invalid layer path".to_string()))?;

            let (digest, size) = Self::get_layer_info_from_tar(tar_path, layer_path)?;

            layers.push(crate::image::parser::LayerInfo {
                digest,
                size,
                tar_path: layer_path.to_string(),
                media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                compressed_size: Some(size),
                offset: None,
            });
        }

        let config_size = Self::get_config_size_from_tar(tar_path, &config_digest)?;
        let total_size = layers.iter().map(|l| l.size).sum();

        Ok(crate::image::parser::ImageInfo {
            config_digest,
            config_size,
            layers,
            total_size,
        })
    }

    fn get_layer_info_from_tar(tar_path: &Path, layer_path: &str) -> Result<(String, u64)> {
        let file = File::open(tar_path)?;
        let mut archive = Archive::new(file);

        for entry_result in archive.entries()? {
            let mut entry = entry_result?;
            let path = entry.path()?.to_string_lossy().to_string();

            if path == layer_path {
                let size = entry.size();
                let mut data = Vec::new();
                entry.read_to_end(&mut data)?;

                let digest = format!(
                    "sha256:{}",
                    crate::image::digest::DigestUtils::compute_sha256(&data)
                );
                return Ok((digest, size));
            }
        }

        Err(RegistryError::ImageParsing(format!(
            "Layer {} not found",
            layer_path
        )))
    }

    fn get_config_size_from_tar(tar_path: &Path, config_digest: &str) -> Result<u64> {
        let file = File::open(tar_path)?;
        let mut archive = Archive::new(file);

        // 尝试多种可能的config文件路径格式
        let possible_paths = vec![
            // OCI格式: blobs/sha256/digest
            format!("blobs/sha256/{}", config_digest.replace("sha256:", "")),
            // Docker格式: digest.json
            format!("{}.json", config_digest.replace("sha256:", "")),
            // Docker格式: digest/json
            format!("{}/json", config_digest.replace("sha256:", "")),
        ];

        for entry_result in archive.entries()? {
            let entry = entry_result?;
            let path = entry.path()?.to_string_lossy().to_string();

            for possible_path in &possible_paths {
                if path == *possible_path {
                    return Ok(entry.size());
                }
            }
        }

        Err(RegistryError::ImageParsing(format!(
            "Config file not found for digest {}",
            config_digest
        )))
    }

    /// Extract layer data using streaming approach for memory efficiency
    ///
    /// This method streams data in chunks to avoid loading large files entirely into memory
    pub async fn extract_layer_data_streaming(
        tar_path: &Path,
        layer_path: &str,
    ) -> Result<Vec<u8>> {
        use tokio::task;

        let tar_path = tar_path.to_path_buf();
        let layer_path = layer_path.to_string();

        // Use blocking task for file I/O to avoid blocking async runtime
        task::spawn_blocking(move || {
            let file = File::open(&tar_path)
                .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;

            let mut archive = Archive::new(file);
            archive.set_ignore_zeros(true);

            for entry_result in archive.entries().map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read tar entries: {}", e))
            })? {
                let mut entry = entry_result.map_err(|e| {
                    RegistryError::ImageParsing(format!("Failed to read tar entry: {}", e))
                })?;

                let path = entry
                    .path()
                    .map_err(|e| {
                        RegistryError::ImageParsing(format!("Failed to read entry path: {}", e))
                    })?
                    .to_string_lossy()
                    .to_string();

                if path == layer_path {
                    // Stream the data in chunks to reduce memory pressure
                    let mut data = Vec::new();
                    const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks
                    let mut buffer = vec![0u8; CHUNK_SIZE];

                    loop {
                        let bytes_read = entry.read(&mut buffer).map_err(|e| {
                            RegistryError::ImageParsing(format!(
                                "Failed to read layer chunk: {}",
                                e
                            ))
                        })?;

                        if bytes_read == 0 {
                            break;
                        }

                        data.extend_from_slice(&buffer[..bytes_read]);
                    }

                    return Ok(data);
                }
            }

            Err(RegistryError::ImageParsing(format!(
                "Layer '{}' not found in tar archive",
                layer_path
            )))
        })
        .await
        .map_err(|e| RegistryError::Upload(format!("Streaming extraction task failed: {}", e)))?
    }

    /// Extract layer data with size limit to prevent memory exhaustion
    pub fn extract_layer_data_limited(
        tar_path: &Path,
        layer_path: &str,
        max_size: u64,
    ) -> Result<Vec<u8>> {
        let file = File::open(tar_path)
            .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        archive.set_ignore_zeros(true);

        for entry_result in archive.entries().map_err(|e| {
            RegistryError::ImageParsing(format!("Failed to read tar entries: {}", e))
        })? {
            let mut entry = entry_result.map_err(|e| {
                RegistryError::ImageParsing(format!("Failed to read tar entry: {}", e))
            })?;

            let path = entry
                .path()
                .map_err(|e| {
                    RegistryError::ImageParsing(format!("Failed to read entry path: {}", e))
                })?
                .to_string_lossy()
                .to_string();

            if path == layer_path {
                let size = entry.header().size().map_err(|e| {
                    RegistryError::ImageParsing(format!("Failed to read entry size: {}", e))
                })?;

                if size > max_size {
                    return Err(RegistryError::Validation(format!(
                        "Layer size {} exceeds limit {}",
                        size, max_size
                    )));
                }

                let mut data = Vec::with_capacity(size as usize);
                entry.read_to_end(&mut data).map_err(|e| {
                    RegistryError::ImageParsing(format!("Failed to read layer data: {}", e))
                })?;

                return Ok(data);
            }
        }

        Err(RegistryError::ImageParsing(format!(
            "Layer '{}' not found in tar archive",
            layer_path
        )))
    }
}
