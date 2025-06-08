use crate::error::{RegistryError, Result};
use crate::registry::tar_utils::TarUtils;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

// 缓存目录结构常量
pub const CACHE_DIR: &str = ".cache";
pub const MANIFESTS_DIR: &str = "manifests";
pub const BLOBS_DIR: &str = "blobs";
pub const SHA256_PREFIX: &str = "sha256";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlobInfo {
    pub digest: String,
    pub size: u64,
    pub path: PathBuf,
    pub is_config: bool,
    pub compressed: bool,
    pub media_type: String, // Add media_type field
}

/// Docker 镜像缓存管理
///
/// 提供本地缓存功能，结构与 Docker Registry API 兼容，支持从 repository 或 tar 文件
/// 获取 manifest 和 blob，并支持从缓存中推送。
///
/// 缓存结构:
/// ```text
/// .cache/
///   manifests/{repository}/{reference}
///   blobs/sha256/{digest}
///   index.json  // 缓存索引
/// ```
pub struct Cache {
    cache_dir: PathBuf,
    index: HashMap<String, CacheEntry>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    repository: String,
    reference: String,
    manifest_path: PathBuf,
    config_digest: String,
    blobs: HashMap<String, BlobInfo>,
}

impl Cache {
    /// 创建新的缓存管理器
    pub fn new<P: AsRef<Path>>(cache_dir: Option<P>) -> Result<Self> {
        let cache_dir = match cache_dir {
            Some(dir) => PathBuf::from(dir.as_ref()),
            None => PathBuf::from(CACHE_DIR),
        };

        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir)?;
            // 创建 manifests 和 blobs 目录
            fs::create_dir_all(cache_dir.join(MANIFESTS_DIR))?;
            fs::create_dir_all(cache_dir.join(BLOBS_DIR).join(SHA256_PREFIX))?;
        }

        let index_path = cache_dir.join("index.json");
        let index = if index_path.exists() {
            let mut file = File::open(&index_path)?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            serde_json::from_str(&contents)
                .map_err(|e| RegistryError::Parse(format!("Failed to parse cache index: {}", e)))?
        } else {
            HashMap::new()
        };

        Ok(Cache { cache_dir, index })
    }

    /// 保存清单（manifest）到缓存
    pub fn save_manifest(
        &mut self,
        repository: &str,
        reference: &str,
        manifest: &[u8],
        config_digest: &str,
    ) -> Result<PathBuf> {
        // 确保目录结构存在
        let manifest_dir = self.cache_dir.join(MANIFESTS_DIR).join(repository);
        fs::create_dir_all(&manifest_dir)?;

        // 保存 manifest 文件
        let manifest_path = manifest_dir.join(reference);
        let mut file = File::create(&manifest_path)?;
        file.write_all(manifest)?;

        // 更新或创建缓存条目
        let cache_key = format!("{}/{}", repository, reference);
        let entry = self
            .index
            .entry(cache_key.clone())
            .or_insert_with(|| CacheEntry {
                repository: repository.to_string(),
                reference: reference.to_string(),
                manifest_path: manifest_path.clone(),
                config_digest: config_digest.to_string(),
                blobs: HashMap::new(),
            });

        entry.manifest_path = manifest_path.clone();
        entry.config_digest = config_digest.to_string();

        self.save_index()?;

        Ok(manifest_path)
    }

    /// 保存 blob 到缓存
    pub fn save_blob(
        &mut self,
        digest: &str,
        data: &[u8],
        _is_config: bool,
        _compressed: bool,
    ) -> Result<PathBuf> {
        // 标准化摘要格式 (确保有 sha256: 前缀)
        let normalized_digest = self.normalize_digest(digest);
        let digest_value = normalized_digest
            .split(':')
            .nth(1)
            .unwrap_or(&normalized_digest);

        // 创建 blob 目录
        let blob_dir = self.cache_dir.join(BLOBS_DIR).join(SHA256_PREFIX);
        fs::create_dir_all(&blob_dir)?;

        // 保存 blob 文件
        let blob_path = blob_dir.join(digest_value);

        if blob_path.exists() {
            // 如果 blob 已存在，检查文件大小是否匹配 (简单验证)
            let metadata = fs::metadata(&blob_path)?;
            if metadata.len() == data.len() as u64 {
                return Ok(blob_path);
            }
        }

        let mut file = File::create(&blob_path)?;
        file.write_all(data)?;

        // 记录 blob 信息，但不与特定镜像关联（通过 manifest 关联）

        Ok(blob_path)
    }

    /// 将 blob 关联到指定的镜像
    pub fn associate_blob_with_image(
        &mut self,
        repository: &str,
        reference: &str,
        digest: &str,
        size: u64,
        is_config: bool,
        compressed: bool,
    ) -> Result<()> {
        let normalized_digest = self.normalize_digest(digest);
        let cache_key = format!("{}/{}", repository, reference);

        if let Some(entry) = self.index.get_mut(&cache_key) {
            // 获取 blob 文件路径
            let digest_value = normalized_digest
                .split(':')
                .nth(1)
                .unwrap_or(&normalized_digest);
            let blob_path = self
                .cache_dir
                .join(BLOBS_DIR)
                .join(SHA256_PREFIX)
                .join(digest_value);

            // 检查文件是否存在
            if !blob_path.exists() {
                return Err(RegistryError::Cache {
                    message: format!("Blob {} not found in cache", normalized_digest),
                    path: Some(blob_path),
                });
            }

            entry.blobs.insert(
                normalized_digest.clone(),
                BlobInfo {
                    digest: normalized_digest,
                    size,
                    path: blob_path,
                    is_config,
                    compressed,
                    media_type: String::new(), // Default to empty media_type
                },
            );

            self.save_index()?;
            Ok(())
        } else {
            Err(RegistryError::Cache {
                message: format!("Image {}/{} not found in cache", repository, reference),
                path: None,
            })
        }
    }

    /// Add a blob to the cache
    pub fn add_blob(
        &mut self,
        digest: &str,
        data: &[u8],
        _is_config: bool,
        _compressed: bool,
    ) -> Result<PathBuf> {
        if !digest.starts_with("sha256:") {
            return Err(RegistryError::Validation(
                "Blob digest must start with sha256:".into(),
            ));
        }

        // Verify the blob digest
        let actual_digest = format!(
            "sha256:{}",
            hex::encode(crate::image::digest::DigestUtils::compute_sha256(data))
        );
        if actual_digest != digest {
            return Err(RegistryError::Validation(format!(
                "Blob digest mismatch. Expected: {}, Got: {}",
                digest, actual_digest
            )));
        }

        let blob_path = self.get_blob_path(digest);
        if !blob_path.exists() {
            // Ensure parent directories exist
            if let Some(parent) = blob_path.parent() {
                fs::create_dir_all(parent)?;
            }

            // Write blob data
            let mut file = File::create(&blob_path)?;
            file.write_all(data)?;
        }

        Ok(blob_path)
    }

    /// Get blob path from digest
    pub fn get_blob_path(&self, digest: &str) -> PathBuf {
        let digest = digest.trim_start_matches("sha256:");
        self.cache_dir
            .join(BLOBS_DIR)
            .join(SHA256_PREFIX)
            .join(digest)
    }

    /// Check if a blob exists in cache
    pub fn has_blob(&self, digest: &str) -> bool {
        self.get_blob_path(digest).exists()
    }

    /// Check if a blob exists in cache with enhanced integrity verification
    pub fn has_blob_with_verification(&self, digest: &str, verify_integrity: bool) -> bool {
        let blob_path = self.get_blob_path(digest);
        
        if !blob_path.exists() {
            return false;
        }

        // 获取文件元数据
        let metadata = match fs::metadata(&blob_path) {
            Ok(meta) => meta,
            Err(_) => return false,
        };

        let file_size = metadata.len();

        // 对于小文件或强制验证，进行完整SHA256验证
        if verify_integrity || file_size <= 10 * 1024 * 1024 {
            match self.verify_blob_integrity(&blob_path, digest) {
                Ok(valid) => valid,
                Err(_) => false,
            }
        } else {
            // 对于大文件，使用轻量级完整性检查
            self.verify_large_file_integrity(&blob_path, digest, file_size)
        }
    }

    /// Verify blob integrity using full SHA256 calculation
    fn verify_blob_integrity(&self, blob_path: &Path, expected_digest: &str) -> Result<bool> {
        let data = fs::read(blob_path)?;
        let actual_digest = format!(
            "sha256:{}",
            crate::image::digest::DigestUtils::compute_sha256(&data)
        );
        Ok(actual_digest == expected_digest)
    }

    /// Verify large file integrity using lightweight checks
    fn verify_large_file_integrity(&self, blob_path: &Path, _digest: &str, file_size: u64) -> bool {
        // 基础检查：文件大小是否合理
        if file_size == 0 {
            return false;
        }

        // 检查文件头部是否为有效的gzip格式（大多数Docker层都是gzip压缩的）
        if let Ok(mut file) = File::open(blob_path) {
            let mut header = [0u8; 10];
            if let Ok(n) = file.read(&mut header) {
                if n >= 2 {
                    // 检查gzip魔数 (0x1f, 0x8b)
                    if header[0] == 0x1f && header[1] == 0x8b {
                        // 对于超大文件，进行抽样验证
                        if file_size > 100 * 1024 * 1024 {
                            return self.verify_large_file_sampling(blob_path, file_size);
                        }
                        return true;
                    }
                }
            }
        }

        // 如果不是gzip格式，可能是其他有效格式，进行基础验证
        self.verify_file_basic_integrity(blob_path, file_size)
    }

    /// Perform sampling verification for very large files
    fn verify_large_file_sampling(&self, blob_path: &Path, file_size: u64) -> bool {
        use std::io::Seek;
        
        if let Ok(mut file) = File::open(blob_path) {
            let sample_size = 4096;
            let mut buffer = vec![0u8; sample_size];

            // 检查文件开头
            if file.read(&mut buffer).is_err() {
                return false;
            }

            // 检查是否全为零字节（损坏的文件模式）
            if buffer.iter().all(|&b| b == 0) {
                return false;
            }

            // 检查文件中间部分
            let middle_pos = file_size / 2;
            if let Ok(_) = file.seek(std::io::SeekFrom::Start(middle_pos)) {
                if file.read(&mut buffer).is_err() {
                    return false;
                }
                // 同样检查零字节模式
                if buffer.iter().all(|&b| b == 0) {
                    return false;
                }
            }

            // 检查文件末尾
            let end_pos = file_size.saturating_sub(sample_size as u64);
            if let Ok(_) = file.seek(std::io::SeekFrom::Start(end_pos)) {
                if file.read(&mut buffer).is_err() {
                    return false;
                }
                // 检查零字节模式
                if buffer.iter().all(|&b| b == 0) {
                    return false;
                }
            }

            true
        } else {
            false
        }
    }

    /// Basic file integrity verification
    fn verify_file_basic_integrity(&self, blob_path: &Path, file_size: u64) -> bool {
        // 基础检查：文件大小合理性
        if file_size == 0 || file_size > 10 * 1024 * 1024 * 1024 {
            // 拒绝空文件或超过10GB的文件
            return false;
        }

        // 简单的读取测试
        if let Ok(mut file) = File::open(blob_path) {
            let mut buffer = [0u8; 1024];
            // 尝试读取文件开头以确保文件可读
            file.read(&mut buffer).is_ok()
        } else {
            false
        }
    }

    /// 从缓存中获取 manifest
    pub fn get_manifest(&self, repository: &str, reference: &str) -> Result<Vec<u8>> {
        let cache_key = format!("{}/{}", repository, reference);
        if let Some(entry) = self.index.get(&cache_key) {
            if entry.manifest_path.exists() {
                return Ok(fs::read(&entry.manifest_path)?);
            }
        }
        Err(RegistryError::NotFound(format!(
            "Manifest not found for {}/{}",
            repository, reference
        )))
    }

    /// 从缓存中获取 blob
    pub fn get_blob(&self, digest: &str) -> Result<Vec<u8>> {
        let blob_path = self.get_blob_path(digest);
        if blob_path.exists() {
            Ok(fs::read(blob_path)?)
        } else {
            Err(RegistryError::NotFound(format!(
                "Blob not found: {}",
                digest
            )))
        }
    }

    /// 删除清单及其未被其他镜像引用的 blob
    pub fn remove_manifest(&mut self, repository: &str, reference: &str) -> Result<()> {
        let cache_key = format!("{}/{}", repository, reference);
        if let Some(entry) = self.index.remove(&cache_key) {
            // 删除 manifest 文件
            if entry.manifest_path.exists() {
                fs::remove_file(&entry.manifest_path)?;
            }

            // 清理未被引用的 blob
            self.cleanup_unreferenced_blobs()?;
        }
        self.save_index()
    }

    /// 清理未被引用的 blob
    fn cleanup_unreferenced_blobs(&self) -> Result<()> {
        let mut referenced_blobs: HashMap<String, bool> = HashMap::new();

        // 收集所有引用的 blob
        for entry in self.index.values() {
            for blob_info in entry.blobs.values() {
                referenced_blobs.insert(blob_info.digest.clone(), true);
            }
        }

        // 检查并删除未被引用的 blob
        let blobs_dir = self.cache_dir.join(BLOBS_DIR).join(SHA256_PREFIX);
        if blobs_dir.exists() {
            for entry in fs::read_dir(blobs_dir)? {
                let entry = entry?;
                let digest = format!("sha256:{}", entry.file_name().to_string_lossy());
                if !referenced_blobs.contains_key(&digest) {
                    fs::remove_file(entry.path())?;
                }
            }
        }

        Ok(())
    }

    /// 列出缓存中的所有清单
    pub fn list_manifests(&self) -> Vec<(String, String)> {
        self.index
            .iter()
            .map(|(_, entry)| (entry.repository.clone(), entry.reference.clone()))
            .collect()
    }

    /// 获取缓存统计信息
    pub fn get_stats(&self) -> Result<CacheStats> {
        let mut stats = CacheStats {
            manifest_count: self.index.len(),
            blob_count: 0,
            total_size: 0,
        };

        // 计算 blob 统计信息
        let blobs_dir = self.cache_dir.join(BLOBS_DIR).join(SHA256_PREFIX);
        if blobs_dir.exists() {
            for entry in fs::read_dir(blobs_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    stats.blob_count += 1;
                    stats.total_size += entry.metadata()?.len();
                }
            }
        }

        Ok(stats)
    }

    /// 保存索引文件
    fn save_index(&self) -> Result<()> {
        let index_path = self.cache_dir.join("index.json");
        let json_data = serde_json::to_string_pretty(&self.index)
            .map_err(|e| RegistryError::Parse(format!("Failed to serialize cache index: {}", e)))?;

        let mut file = File::create(&index_path)?;
        file.write_all(json_data.as_bytes())?;
        Ok(())
    }

    /// 标准化摘要格式
    fn normalize_digest(&self, digest: &str) -> String {
        if digest.starts_with("sha256:") {
            digest.to_string()
        } else {
            format!("sha256:{}", digest)
        }
    }

    /// 从缓存中获取blob大小
    pub fn get_blob_size(&self, digest: &str) -> Option<u64> {
        let blob_path = self.get_blob_path(digest);
        if blob_path.exists() {
            if let Ok(metadata) = fs::metadata(&blob_path) {
                return Some(metadata.len());
            }
        }
        None
    }

    /// 获取镜像的所有blob信息
    pub fn get_image_blobs(&self, repository: &str, reference: &str) -> Result<Vec<BlobInfo>> {
        let cache_key = format!("{}/{}", repository, reference);
        if let Some(entry) = self.index.get(&cache_key) {
            Ok(entry.blobs.values().cloned().collect())
        } else {
            Err(RegistryError::NotFound(format!(
                "Image {}/{} not found in cache",
                repository, reference
            )))
        }
    }

    /// 检查镜像是否完整（manifest和所有blob都存在）
    pub fn is_image_complete(&self, repository: &str, reference: &str) -> Result<bool> {
        let cache_key = format!("{}/{}", repository, reference);
        if let Some(entry) = self.index.get(&cache_key) {
            // 检查manifest文件存在
            if !entry.manifest_path.exists() {
                return Ok(false);
            }

            // 检查所有blob存在
            for blob_info in entry.blobs.values() {
                if !blob_info.path.exists() {
                    return Ok(false);
                }
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }
    /// 从tar文件中提取blob信息并缓存
    pub fn cache_from_tar(
        &mut self,
        tar_path: &Path,
        repository: &str,
        reference: &str,
    ) -> Result<()> {
        // 直接使用TarUtils解析tar文件
        let image_info = TarUtils::parse_image_info(tar_path)?;

        // 创建简化的manifest结构并保存
        let manifest_json = self.create_manifest_from_image_info(&image_info)?;
        self.save_manifest(
            repository,
            reference,
            manifest_json.as_bytes(),
            &image_info.config_digest,
        )?;

        // 缓存config blob
        let config_data = TarUtils::extract_config_data(tar_path, &image_info.config_digest)?;
        self.save_blob(&image_info.config_digest, &config_data, true, false)?;
        self.associate_blob_with_image(
            repository,
            reference,
            &image_info.config_digest,
            config_data.len() as u64,
            true,
            false,
        )?;

        // Cache all layer blobs
        for layer in &image_info.layers {
            let layer_data = TarUtils::extract_layer_data(tar_path, &layer.tar_path)?;
            // Layer data is already in correct gzip format, save directly
            self.save_blob(&layer.digest, &layer_data, false, true)?;
            self.associate_blob_with_image(
                repository,
                reference,
                &layer.digest,
                layer_data.len() as u64,
                false,
                true,
            )?;
        }

        Ok(())
    }

    /// 从ImageInfo创建Docker v2 manifest
    fn create_manifest_from_image_info(
        &self,
        image_info: &crate::image::parser::ImageInfo,
    ) -> Result<String> {
        let config = serde_json::json!({
            "mediaType": "application/vnd.docker.container.image.v1+json",
            "size": image_info.config_size,
            "digest": image_info.config_digest
        });

        let layers: Vec<serde_json::Value> = image_info
            .layers
            .iter()
            .map(|layer| {
                serde_json::json!({
                    "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
                    "size": layer.size,
                    "digest": layer.digest
                })
            })
            .collect();

        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": config,
            "layers": layers
        });

        Ok(serde_json::to_string_pretty(&manifest)?)
    }

    /// Add a blob to the cache with enhanced integrity verification
    pub async fn add_blob_with_verification(
        &mut self,
        digest: &str,
        data: &[u8],
        is_config: bool,
        _compressed: bool, // 标记为未使用但保留接口兼容性
        force_verify: bool,
    ) -> Result<PathBuf> {
        if !digest.starts_with("sha256:") {
            return Err(RegistryError::Validation(
                "Blob digest must start with sha256:".into(),
            ));
        }

        let blob_path = self.get_blob_path(digest);
        
        // 检查是否已存在 (增强完整性检查)
        if blob_path.exists() {
            if let Ok(existing_metadata) = fs::metadata(&blob_path) {
                if existing_metadata.len() == data.len() as u64 && existing_metadata.len() > 0 {
                    // 根据文件大小和强制验证标志决定验证策略
                    let should_verify = force_verify || is_config || data.len() <= 10 * 1024 * 1024;
                    
                    if should_verify {
                        // 小文件或强制验证 - 完整SHA256验证
                        if let Ok(existing_data) = fs::read(&blob_path) {
                            let expected_digest_value = digest.trim_start_matches("sha256:");
                            let existing_digest = crate::image::digest::DigestUtils::compute_sha256(&existing_data);
                            if existing_digest == expected_digest_value {
                                return Ok(blob_path);
                            }
                        }
                    } else {
                        // 大文件 - 使用增强的完整性检查
                        if self.verify_large_file_integrity(&blob_path, digest, existing_metadata.len()) {
                            return Ok(blob_path);
                        }
                    }
                }
            }
            // 如果验证失败，删除损坏的文件
            let _ = fs::remove_file(&blob_path);
        }

        // 对于新blob，进行适当的完整性验证
        if force_verify || is_config || data.len() <= 10 * 1024 * 1024 {
            // 小文件或强制验证 - 完整SHA256验证
            let actual_digest = format!(
                "sha256:{}",
                crate::image::digest::DigestUtils::compute_sha256(data)
            );
            if actual_digest != digest {
                return Err(RegistryError::Validation(format!(
                    "Blob digest mismatch. Expected: {}, Got: {}",
                    digest, actual_digest
                )));
            }
        } else {
            // 大文件 - 基础验证 (检查数据完整性但不计算SHA256)
            if data.is_empty() {
                return Err(RegistryError::Validation(
                    "Cannot cache empty blob data".to_string(),
                ));
            }
        }

        // Ensure parent directories exist
        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // 使用临时文件确保原子写入
        let temp_path = blob_path.with_extension("tmp");
        let mut file = File::create(&temp_path)?;
        file.write_all(data)?;
        file.sync_all()?;

        // 原子性地重命名临时文件
        fs::rename(&temp_path, &blob_path)?;

        Ok(blob_path)
    }

    /// 流式保存blob到缓存，支持边下载边验证
    pub async fn add_blob_stream_with_verification<R>(
        &mut self,
        digest: &str,
        mut reader: R,
        expected_size: Option<u64>,
        _is_config: bool, // 保留接口兼容性但标记为未使用
        verify_integrity: bool,
    ) -> Result<PathBuf>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        use tokio::io::AsyncReadExt;
        use sha2::{Sha256, Digest};
        
        if !digest.starts_with("sha256:") {
            return Err(RegistryError::Validation(
                "Blob digest must start with sha256:".into(),
            ));
        }

        let blob_path = self.get_blob_path(digest);
        
        // 检查是否已存在
        if blob_path.exists() {
            if let Ok(metadata) = fs::metadata(&blob_path) {
                if let Some(expected) = expected_size {
                    if metadata.len() == expected && metadata.len() > 0 {
                        // 使用增强的完整性检查
                        if self.has_blob_with_verification(digest, verify_integrity) {
                            return Ok(blob_path);
                        }
                    }
                }
            }
            // 如果检查失败，删除现有文件
            let _ = fs::remove_file(&blob_path);
        }

        // Ensure parent directories exist
        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // 使用临时文件进行流式写入
        let temp_path = blob_path.with_extension("tmp");
        let mut temp_file = tokio::fs::File::create(&temp_path).await
            .map_err(|e| RegistryError::Io(format!("Failed to create temp file: {}", e)))?;

        let mut total_written = 0u64;
        let mut hasher: Option<Sha256> = if verify_integrity {
            Some(Sha256::new())
        } else {
            None
        };

        // 流式读取和写入
        let mut chunk = [0u8; 8192];
        loop {
            let n = reader.read(&mut chunk).await
                .map_err(|e| RegistryError::Io(format!("Failed to read stream: {}", e)))?;
            
            if n == 0 {
                break;
            }

            let chunk_data = &chunk[..n];
            
            // 写入临时文件
            tokio::io::AsyncWriteExt::write_all(&mut temp_file, chunk_data).await
                .map_err(|e| RegistryError::Io(format!("Failed to write to temp file: {}", e)))?;
            
            // 更新哈希计算 (如果需要验证)
            if let Some(ref mut h) = hasher {
                h.update(chunk_data);
            }
            
            total_written += n as u64;
            
            // 检查文件大小是否超出预期
            if let Some(expected) = expected_size {
                if total_written > expected {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(RegistryError::Validation(format!(
                        "Blob size exceeded expected: {} > {}",
                        total_written, expected
                    )));
                }
            }
        }

        // 确保数据写入磁盘
        temp_file.sync_all().await
            .map_err(|e| RegistryError::Io(format!("Failed to sync temp file: {}", e)))?;

        // 验证文件大小
        if let Some(expected) = expected_size {
            if total_written != expected {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(RegistryError::Validation(format!(
                    "Blob size mismatch: expected {}, got {}",
                    expected, total_written
                )));
            }
        }

        // 验证SHA256 (如果需要)
        if let Some(h) = hasher {
            let computed_digest = format!("sha256:{}", hex::encode(h.finalize()));
            if computed_digest != digest {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(RegistryError::Validation(format!(
                    "Stream digest mismatch. Expected: {}, Got: {}",
                    digest, computed_digest
                )));
            }
        }

        // 原子性地重命名临时文件
        tokio::fs::rename(&temp_path, &blob_path).await
            .map_err(|e| RegistryError::Io(format!("Failed to rename temp file: {}", e)))?;

        Ok(blob_path)
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub manifest_count: usize,
    pub blob_count: usize,
    pub total_size: u64,
}
