//! Enhanced Docker image parsing with better error handling and progress reporting

use std::fs::File;
use std::io::Read;
use std::path::Path;
use tar::Archive;
use crate::error::{Result, PusherError};
use crate::output::OutputManager;
use crate::digest::DigestUtils;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LayerInfo {
    pub digest: String,
    pub size: u64,
    pub media_type: String,
    pub tar_path: String,
    pub compressed_size: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImageConfig {
    pub architecture: Option<String>,
    pub os: Option<String>,
    pub config: Option<serde_json::Value>,
    pub rootfs: Option<serde_json::Value>,
    pub history: Option<Vec<serde_json::Value>>,
    pub created: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImageInfo {
    pub repository: String,
    pub tag: String,
    pub layers: Vec<LayerInfo>,
    pub config: ImageConfig,
    pub config_digest: String,
    pub total_size: u64,
    pub layer_count: usize,
    pub large_layers_count: usize,
}

pub struct ImageParser {
    output: OutputManager,
    large_layer_threshold: u64,
}

impl ImageParser {
    pub fn new(output: OutputManager) -> Self {
        Self {
            output,
            large_layer_threshold: 100 * 1024 * 1024, // 100MB
        }
    }

    pub fn set_large_layer_threshold(&mut self, threshold: u64) {
        self.large_layer_threshold = threshold;
        self.output.detail(&format!("Large layer threshold set to {}", 
            self.output.format_size(threshold)));
    }

    pub async fn parse_tar_file(&mut self, tar_path: &Path) -> Result<ImageInfo> {
        let start_time = Instant::now();
        self.output.section("Parsing Docker Image");
        self.output.info(&format!("Source: {}", tar_path.display()));
        
        let file_size = std::fs::metadata(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to read file metadata: {}", e)))?
            .len();
        
        self.output.info(&format!("Archive size: {}", self.output.format_size(file_size)));

        let parse_result = self.parse_tar_contents(tar_path).await;
        
        match parse_result {
            Ok(mut image_info) => {
                let elapsed = start_time.elapsed();
                image_info.total_size = image_info.layers.iter().map(|l| l.size).sum();
                image_info.layer_count = image_info.layers.len();
                image_info.large_layers_count = image_info.layers.iter()
                    .filter(|l| l.size > self.large_layer_threshold)
                    .count();
                
                self.output.success(&format!(
                    "Parsing completed in {} - {} layers, total size: {}",
                    self.output.format_duration(elapsed),
                    image_info.layer_count,
                    self.output.format_size(image_info.total_size)
                ));
                
                self.print_image_summary(&image_info);
                Ok(image_info)
            }
            Err(e) => {
                self.output.error(&format!("Parsing failed after {}: {}", 
                    self.output.format_duration(start_time.elapsed()), e));
                Err(e)
            }
        }
    }

    // Remove the unused compute_layer_digest method
    // async fn compute_layer_digest(&self, tar_path: &Path, layer_path: &str) -> Result<String> {
    //     // Method removed as we now use manifest-based digest extraction
    // }

    // 添加缺少的 detect_media_type 方法
    fn detect_media_type(&self, layer_path: &str) -> String {
        if layer_path.ends_with(".tar.gz") || layer_path.contains("gzip") {
            "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string()
        } else if layer_path.ends_with(".tar") {
            "application/vnd.docker.image.rootfs.diff.tar".to_string()
        } else {
            // 默认使用未压缩的 tar 格式
            "application/vnd.docker.image.rootfs.diff.tar".to_string()
        }
    }

    fn print_image_summary(&self, image_info: &ImageInfo) {
        let empty_layers_count = image_info.layers.iter()
            .filter(|l| l.size == 0)
            .count();
        
        let items = vec![
            ("Layers", image_info.layer_count.to_string()),
            ("Empty Layers", empty_layers_count.to_string()),
            ("Large Layers", format!("{} (>{})", 
                image_info.large_layers_count, 
                self.output.format_size(self.large_layer_threshold))),
            ("Total Size", self.output.format_size(image_info.total_size)),
            ("Architecture", image_info.config.architecture.clone().unwrap_or_else(|| "unknown".to_string())),
            ("OS", image_info.config.os.clone().unwrap_or_else(|| "unknown".to_string())),
            ("Config Digest", format!("{}...", &image_info.config_digest[..23])),
        ];
        
        // Change from summary to summary_kv for key-value pairs
        self.output.summary_kv("Image Information", &items);
        
        if self.output.verbose {
            self.output.subsection("Layer Details");
            for (i, layer) in image_info.layers.iter().enumerate() {
                let layer_type = if layer.size == 0 {
                    " (EMPTY)"
                } else if layer.size > self.large_layer_threshold { 
                    " (LARGE)" 
                } else { 
                    "" 
                };
                
                self.output.detail(&format!("Layer {}: {}... ({}){}", 
                    i + 1, 
                    &layer.digest[..23],
                    self.output.format_size(layer.size),
                    layer_type));
            }
        }
    }

    async fn parse_tar_contents(&mut self, tar_path: &Path) -> Result<ImageInfo> {
        let mut manifest_data = None;
        let mut config_data = None;
        let mut layers = Vec::new();
        
        self.output.subsection("Scanning archive entries");
        
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        
        archive.set_ignore_zeros(true);
        
        let entries = archive.entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))?;

        let mut entry_count = 0;
        let mut layer_count = 0;
        
        for entry_result in entries {
            let mut entry = entry_result
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entry: {}", e)))?;
            
            let path = entry.path()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry path: {}", e)))?
                .to_string_lossy()
                .to_string();
            
            let size = entry.header().size()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry size: {}", e)))?;
            
            entry_count += 1;
            
            if path.ends_with(".tar") || path.ends_with(".tar.gz") || path.ends_with(".json") || path == "manifest.json" {
                if size == 0 {
                    self.output.detail(&format!("Entry {}: {} (EMPTY)", entry_count, path));
                } else {
                    self.output.detail(&format!("Entry {}: {} ({})", entry_count, path, self.output.format_size(size)));
                }
            }
            
            match self.process_tar_entry(&mut entry, &path, size, tar_path).await? {
                EntryType::Manifest(data) => manifest_data = Some(data),
                EntryType::Config(data) => config_data = Some(data),
                EntryType::Layer(layer_info) => {
                    layers.push(layer_info);
                    layer_count += 1;
                },
                EntryType::Other => {}
            }
        }

        self.output.info(&format!("Processed {} entries total", entry_count));
        self.output.info(&format!("Found {} layer entries", layer_count));
        
        // Build image info using manifest-provided digests
        let image_info = self.build_image_info_with_manifest_digests(manifest_data, config_data, layers).await?;
        Ok(image_info)
    }

    // 完整的 extract_digest_from_layer_path 方法
    fn extract_digest_from_layer_path(&self, layer_path: &str) -> Option<String> {
        self.output.detail(&format!("Extracting digest from layer path: {}", layer_path));
        
        if let Some(digest) = DigestUtils::extract_digest_from_layer_path(layer_path) {
            self.output.detail(&format!("  ✅ Found digest: {}...", &digest[..16]));
            Some(digest)
        } else {
            self.output.detail("  ❌ No valid digest found in layer path");
            None
        }
    }

    // 使用DigestUtils进行SHA256验证
    fn is_valid_sha256_hex(&self, s: &str) -> bool {
        DigestUtils::is_valid_sha256_hex(s)
    }

    // 修复 process_layer 方法中的空层处理
    async fn process_layer(&mut self, _tar_path: &Path, layer_path: &str, size: u64) -> Result<LayerInfo> {        // Handle empty layers specially - 使用标准的空文件SHA256
        if size == 0 {
            self.output.detail("Processing empty layer (0 bytes)");
            let empty_digest = DigestUtils::empty_layer_digest();
            
            return Ok(LayerInfo {
                digest: empty_digest,
                size: 0,
                media_type: self.detect_media_type(layer_path),
                tar_path: layer_path.to_string(),
                compressed_size: Some(0),
                offset: None,
            });
        }
        
        // For non-empty layers, extract digest from path or compute placeholder
        let digest = if let Some(extracted_digest) = self.extract_digest_from_layer_path(layer_path) {
            format!("sha256:{}", extracted_digest)        } else {
            // 如果无法从路径提取，使用路径的hash作为临时标识符
            let digest = DigestUtils::generate_path_based_digest(layer_path);
            self.output.warning(&format!("Cannot extract digest from path '{}', using path hash: {}...", 
                layer_path, &digest[..23]));
            digest
        };
        
        self.output.detail(&format!("Processing layer: {} ({}) -> {}", 
            layer_path, self.output.format_size(size), &digest[..23]));
        
        Ok(LayerInfo {
            digest,
            size,
            media_type: self.detect_media_type(layer_path),
            tar_path: layer_path.to_string(),
            compressed_size: Some(size),
            offset: None,
        })
    }

    async fn build_image_info_with_manifest_digests(
        &self,
        manifest_data: Option<String>,
        config_data: Option<(String, String)>,
        mut layers: Vec<LayerInfo>,
    ) -> Result<ImageInfo> {
        self.output.subsection("Building image metadata");
        
        let manifest_str = manifest_data
            .ok_or_else(|| PusherError::ImageParsing("No manifest.json found in archive".to_string()))?;
        
        // 打印完整的manifest内容用于调试
        self.output.detail("=== MANIFEST.JSON CONTENT ===");
        self.output.detail(&manifest_str);
        self.output.detail("=== END MANIFEST.JSON ===");
        
        let manifest: Vec<serde_json::Value> = serde_json::from_str(&manifest_str)
            .map_err(|e| PusherError::Parse(format!("Failed to parse manifest.json: {}", e)))?;
        
        let image_manifest = manifest.first()
            .ok_or_else(|| PusherError::ImageParsing("Empty manifest array".to_string()))?;
        
        self.output.detail("Available manifest keys:");
        if let Some(obj) = image_manifest.as_object() {
            for (key, value) in obj.iter() {
                let value_preview = if value.to_string().len() > 100 {
                    format!("{}...", &value.to_string()[..100])
                } else {
                    value.to_string()
                };
                self.output.detail(&format!("  - {}: {}", key, value_preview));
            }
        }
        
        // 尝试多种可能的层信息位置
        let mut found_layer_digests = false;
        let mut ordered_layers = Vec::new();
        
        // 方法1: 查找 "Layers" 字段
        if let Some(layer_digests) = image_manifest.get("Layers").and_then(|l| l.as_array()) {
            self.output.info(&format!("✅ Found {} layer paths in 'Layers' field", layer_digests.len()));
            found_layer_digests = true;
            
            // Process layers in manifest order
            for (manifest_index, layer_digest_value) in layer_digests.iter().enumerate() {
                if let Some(layer_file) = layer_digest_value.as_str() {
                    self.output.detail(&format!("Manifest Layer {}: {}", manifest_index + 1, layer_file));
                    
                    // 从路径中提取digest
                    let extracted_digest = self.extract_digest_from_layer_path(layer_file);
                    
                    if let Some(digest) = extracted_digest {
                        let full_digest = format!("sha256:{}", digest);
                        
                        // Find matching layer in our parsed layers
                        let mut matched_layer = None;
                        for (i, layer) in layers.iter().enumerate() {
                            // Match by tar path, digest content, or extracted digest
                            if layer.tar_path == layer_file || 
                               layer.digest.ends_with(&digest) ||
                               layer.tar_path.contains(&digest) {
                                matched_layer = Some(layers.remove(i));
                                break;
                            }
                        }
                        
                        if let Some(mut layer) = matched_layer {
                            // 更新为manifest中的正确digest
                            layer.digest = full_digest.clone();
                            self.output.success(&format!("✅ Matched layer {}: {} -> {}...", 
                                manifest_index + 1, layer.tar_path, &full_digest[..23]));
                            ordered_layers.push(layer);
                        } else {
                            // 创建占位层 - 检查是否为空层
                            let is_empty = digest == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
                            
                            self.output.warning(&format!("⚠️  Creating placeholder for layer {}: {} ({})", 
                                manifest_index + 1, layer_file, if is_empty { "EMPTY" } else { "UNKNOWN SIZE" }));
                            
                            ordered_layers.push(LayerInfo {
                                digest: full_digest,
                                size: if is_empty { 0 } else { 
                                    // 尝试从已解析的层中找到大小信息
                                    layers.iter()
                                        .find(|l| l.tar_path.contains(&digest))
                                        .map(|l| l.size)
                                        .unwrap_or(0)
                                },
                                media_type: self.detect_media_type(layer_file),
                                tar_path: layer_file.to_string(),
                                compressed_size: Some(0),
                                offset: None,
                            });
                        }
                    } else {
                        return Err(PusherError::ImageParsing(format!(
                            "Could not extract valid SHA256 digest from layer path: {}", layer_file
                        )));
                    }
                }
            }
            
            // Use the ordered layers
            layers = ordered_layers;
        }
        
        // 如果manifest中没有找到digest，使用文件名作为备选方案
        if !found_layer_digests {
            self.output.warning("No 'Layers' field found in manifest, using filenames as fallback");
            for (i, layer) in layers.iter_mut().enumerate() {
                if let Some(extracted_digest) = self.extract_digest_from_layer_path(&layer.tar_path) {
                    layer.digest = format!("sha256:{}", extracted_digest);
                    self.output.detail(&format!("Layer {}: Extracted digest from filename: {}...", 
                        i + 1, &layer.digest[..23]));
                } else {
                    self.output.warning(&format!("Layer {}: Could not extract digest from path: {}", 
                        i + 1, layer.tar_path));
                }
            }
        }
        
        // 验证所有层都有有效的SHA256 digest
        for (i, layer) in layers.iter().enumerate() {
            if !layer.digest.starts_with("sha256:") || layer.digest.len() != 71 {
                return Err(PusherError::ImageParsing(format!(
                    "Layer {} has invalid SHA256 digest format: {}", i + 1, layer.digest
                )));
            }
            
            // 验证digest的十六进制部分
            let hex_part = &layer.digest[7..]; // 跳过 "sha256:" 前缀
            if !self.is_valid_sha256_hex(hex_part) {
                return Err(PusherError::ImageParsing(format!(
                    "Layer {} has invalid SHA256 hex digest: {}", i + 1, layer.digest
                )));
            }
        }
        
        let (_, config_str) = config_data
            .ok_or_else(|| PusherError::ImageParsing("No config file found in archive".to_string()))?;
        
        let config: ImageConfig = serde_json::from_str(&config_str)
            .map_err(|e| PusherError::Parse(format!("Failed to parse image config: {}", e)))?;
          // 计算config digest
        let config_digest = DigestUtils::compute_docker_digest_str(&config_str);
        
        self.output.step(&format!("Found {} layers", layers.len()));
        self.output.step(&format!("Config digest: {}...", &config_digest[..23]));
        
        // 显示所有层的digest总结
        self.output.subsection("Layer Digest Summary");
        for (i, layer) in layers.iter().enumerate() {
            let source = if found_layer_digests { "manifest" } else { "filename" };
            let size_info = if layer.size > 0 { 
                format!(" ({})", self.output.format_size(layer.size)) 
            } else { 
                " (EMPTY)".to_string() 
            };
            self.output.detail(&format!("Layer {}: {}{} (from {})", 
                i + 1, &layer.digest[..23], size_info, source));
        }
        
        if found_layer_digests {
            self.output.success("✅ Using real digests from Docker manifest");
        } else {
            self.output.warning("⚠️  Using filename-based digests (may cause upload issues)");
        }
        
        self.output.success("✅ All layer digests validated as proper SHA256 format");
        
        Ok(ImageInfo {
            repository: "unknown".to_string(),
            tag: "latest".to_string(),
            layers,
            config,
            config_digest,
            total_size: 0,
            layer_count: 0,
            large_layers_count: 0,
        })    }
    
    // 添加新的方法来处理tar条目
    async fn process_tar_entry(
        &mut self,
        entry: &mut tar::Entry<'_, std::fs::File>,
        path: &str,
        size: u64,
        tar_path: &Path,
    ) -> Result<EntryType> {
        if path == "manifest.json" {
            let mut content = String::new();
            entry.read_to_string(&mut content)
                .map_err(|e| PusherError::Io(format!("Failed to read manifest: {}", e)))?;
            return Ok(EntryType::Manifest(content));
        }
        
        if path.ends_with(".json") && !path.contains("/") {
            // 这可能是配置文件
            let mut content = String::new();
            entry.read_to_string(&mut content)
                .map_err(|e| PusherError::Io(format!("Failed to read config: {}", e)))?;
            return Ok(EntryType::Config((path.to_string(), content)));
        }
        
        if path.ends_with(".tar") || path.ends_with("layer.tar") || path.contains("/layer") {
            // 这是一个层文件
            let layer_info = self.process_layer(tar_path, path, size).await?;
            return Ok(EntryType::Layer(layer_info));
        }
        
        Ok(EntryType::Other)
    }
}

// 确保 EntryType 枚举在正确的位置
enum EntryType {
    Manifest(String),
    Config((String, String)),
    Layer(LayerInfo),
    Other,
}