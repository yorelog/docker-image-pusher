//! Enhanced Docker image parsing with better error handling and progress reporting

use std::fs::File;
use std::io::{Read, BufReader, Seek, SeekFrom};
use std::path::Path;
use tar::Archive;
use crate::error::{Result, PusherError};
use crate::output::OutputManager;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::time::Instant;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LayerInfo {
    pub digest: String,
    pub size: u64,
    pub media_type: String,
    pub tar_path: String,
    pub compressed_size: Option<u64>,
    pub offset: Option<u64>, // Add offset for streaming access
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

    pub async fn parse_tar_file(&self, tar_path: &Path) -> Result<ImageInfo> {
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

    async fn calculate_digest_streaming(&self, tar_path: &Path, layer_path: &str, size: u64) -> Result<String> {
        // Handle empty layers
        if size == 0 {
            self.output.detail("Empty layer detected, returning empty digest");
            // Empty layer has a known SHA256 hash
            return Ok("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string());
        }

        let (offset, actual_size) = self.find_tar_entry_offset(tar_path, layer_path)?;
        
        if actual_size != size {
            self.output.warning(&format!("Size mismatch: expected {}, found {}", 
                self.output.format_size(size), self.output.format_size(actual_size)));
        }

        let mut file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to reopen tar file: {}", e)))?;
        
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| PusherError::Io(format!("Failed to seek to layer data: {}", e)))?;
        
        let mut reader = BufReader::with_capacity(1024 * 1024, file); // 1MB buffer
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024]; // 64KB buffer
        let mut remaining = actual_size;
        let mut processed = 0u64;
        
        while remaining > 0 {
            let to_read = std::cmp::min(buffer.len() as u64, remaining) as usize;
            let bytes_read = reader.read(&mut buffer[..to_read])
                .map_err(|e| PusherError::Io(format!("Failed to read layer data: {}", e)))?;
            
            if bytes_read == 0 {
                break;
            }
            
            hasher.update(&buffer[..bytes_read]);
            remaining -= bytes_read as u64;
            processed += bytes_read as u64;
            
            // Progress reporting for large files
            if actual_size > 1024 * 1024 * 1024 { // > 1GB
                if processed % (100 * 1024 * 1024) == 0 || remaining == 0 { // Every 100MB or completion
                    self.output.progress_with_metrics(processed, actual_size, "Digest");
                }
            }
        }
        
        let digest = format!("{:x}", hasher.finalize());
        
        // Clear progress line for large files
        if actual_size > 1024 * 1024 * 1024 {
            self.output.progress_done();
        }
        
        Ok(digest)
    }

    async fn process_layer(&self, tar_path: &Path, layer_path: &str, size: u64) -> Result<LayerInfo> {
        let start_time = Instant::now();
        
        // Handle empty layers specially
        if size == 0 {
            self.output.detail("Processing empty layer (0 bytes)");
            let digest = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"; // SHA256 of empty string
            
            return Ok(LayerInfo {
                digest: format!("sha256:{}", digest),
                size: 0,
                media_type: self.detect_media_type(layer_path),
                tar_path: layer_path.to_string(),
                compressed_size: Some(0),
                offset: None, // No offset needed for empty layers
            });
        }
        
        let (digest, offset) = if size > self.large_layer_threshold {
            self.output.detail(&format!("Large layer detected ({}), using streaming digest calculation", 
                self.output.format_size(size)));
            let (offset, _) = self.find_tar_entry_offset(tar_path, layer_path)?;
            let digest = self.calculate_digest_streaming(tar_path, layer_path, size).await?;
            (digest, Some(offset))
        } else {
            self.output.detail(&format!("Small layer ({}), calculating digest in memory", 
                self.output.format_size(size)));
            let digest = self.calculate_digest_from_tar(tar_path, layer_path).await?;
            (digest, None)
        };
        
        let elapsed = start_time.elapsed();
        self.output.detail(&format!("Digest calculation completed in {} - sha256:{}", 
            self.output.format_duration(elapsed), &digest[..16]));
        
        Ok(LayerInfo {
            digest: format!("sha256:{}", digest),
            size,
            media_type: self.detect_media_type(layer_path),
            tar_path: layer_path.to_string(),
            compressed_size: Some(size), // In tar, this is the compressed size
            offset,
        })
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
        
        self.output.summary("Image Information", &items);
        
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
                
                self.output.detail(&format!("Layer {}: {} ({}){}", 
                    i + 1, 
                    &layer.digest[..23],
                    self.output.format_size(layer.size),
                    layer_type));
            }
        }
    }

    async fn parse_tar_contents(&self, tar_path: &Path) -> Result<ImageInfo> {
        let mut manifest_data = None;
        let mut config_data = None;
        let mut layers = Vec::new();
        
        self.output.subsection("Scanning archive entries");
        
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        
        let entries = archive.entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))?;

        let mut entry_count = 0;
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
            
            if size == 0 {
                self.output.detail(&format!("Entry {}: {} (EMPTY)", entry_count, path));
            } else {
                self.output.detail(&format!("Entry {}: {} ({})", entry_count, path, self.output.format_size(size)));
            }
            
            match self.process_tar_entry(&mut entry, &path, size, tar_path).await? {
                EntryType::Manifest(data) => manifest_data = Some(data),
                EntryType::Config(data) => config_data = Some(data),
                EntryType::Layer(layer_info) => layers.push(layer_info),
                EntryType::Other => {} // Skip other files
            }
        }

        self.output.info(&format!("Processed {} entries total", entry_count));
        
        // Process manifest and config
        let image_info = self.build_image_info(manifest_data, config_data, layers).await?;
        Ok(image_info)
    }

    async fn process_tar_entry(
        &self,
        entry: &mut tar::Entry<'_, File>,
        path: &str,
        size: u64,
        tar_path: &Path,
    ) -> Result<EntryType> {
        if path == "manifest.json" {
            self.output.step("Processing manifest");
            let mut contents = String::new();
            entry.read_to_string(&mut contents)
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read manifest: {}", e)))?;
            Ok(EntryType::Manifest(contents))
        } else if path.ends_with(".json") && !path.contains("manifest") {
            self.output.step(&format!("Processing config: {}", path));
            let mut contents = String::new();
            entry.read_to_string(&mut contents)
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read config: {}", e)))?;
            Ok(EntryType::Config((path.to_string(), contents)))
        } else if path.ends_with(".tar.gz") || path.ends_with(".tar") {
            if size == 0 {
                self.output.step(&format!("Processing empty layer: {}", path));
            } else {
                self.output.step(&format!("Processing layer: {} ({})", path, self.output.format_size(size)));
            }
            let layer_info = self.process_layer(tar_path, path, size).await?;
            Ok(EntryType::Layer(layer_info))
        } else {
            self.output.debug(&format!("Skipping entry: {}", path));
            Ok(EntryType::Other)
        }
    }

    fn detect_media_type(&self, path: &str) -> String {
        if path.ends_with(".tar.gz") {
            "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string()
        } else if path.ends_with(".tar") {
            "application/vnd.docker.image.rootfs.diff.tar".to_string()
        } else {
            "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string() // default
        }
    }

    async fn calculate_digest_from_tar(&self, tar_path: &Path, layer_path: &str) -> Result<String> {
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);
        
        for entry_result in archive.entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))? {
            let mut entry = entry_result
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entry: {}", e)))?;
            let path = entry.path()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry path: {}", e)))?
                .to_string_lossy()
                .to_string();
            
            if path == layer_path {
                let size = entry.header().size()
                    .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry size: {}", e)))?;
                
                // Handle empty files
                if size == 0 {
                    return Ok("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string());
                }
                
                // Use buffered reading to avoid memory issues with large files
                let mut hasher = Sha256::new();
                let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer
                let mut total_read = 0u64;
                
                loop {
                    let bytes_read = entry.read(&mut buffer)
                        .map_err(|e| PusherError::Io(format!("Failed to read layer data: {}", e)))?;
                    
                    if bytes_read == 0 {
                        break;
                    }
                    
                    hasher.update(&buffer[..bytes_read]);
                    total_read += bytes_read as u64;
                    
                    // Safety check to prevent infinite loops
                    if total_read > size * 2 {
                        return Err(PusherError::ImageParsing(format!(
                            "Read more data than expected for layer '{}': {} > {}", 
                            layer_path, total_read, size
                        )));
                    }
                }
                
                return Ok(format!("{:x}", hasher.finalize()));
            }
        }
        
        Err(PusherError::ImageParsing(format!("Layer '{}' not found in archive", layer_path)))
    }

    fn find_tar_entry_offset(&self, tar_path: &Path, entry_path: &str) -> Result<(u64, u64)> {
        let mut file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut pos = 0u64;
        
        loop {
            file.seek(SeekFrom::Start(pos))
                .map_err(|e| PusherError::Io(format!("Failed to seek in tar file: {}", e)))?;
            
            let mut header_buf = [0u8; 512];
            match file.read_exact(&mut header_buf) {
                Ok(_) => {},
                Err(_) => break, // End of file
            }
            
            // Check for end of archive (two consecutive zero blocks)
            if header_buf.iter().all(|&b| b == 0) {
                break;
            }
            
            let name_end = header_buf.iter().position(|&b| b == 0).unwrap_or(100);
            let name = String::from_utf8_lossy(&header_buf[..name_end]);
            
            let size_bytes = &header_buf[124..136];
            let size_string = String::from_utf8_lossy(size_bytes).trim_end_matches('\0').to_string();
            let size = u64::from_str_radix(size_string.trim(), 8).unwrap_or(0);
            
            if name == entry_path {
                return Ok((pos + 512, size));
            }
            
            // Move to next entry (round up to 512-byte boundary)
            pos += 512 + ((size + 511) & !511);
        }
        
        Err(PusherError::ImageParsing(format!("Entry '{}' not found in tar archive", entry_path)))
    }

    async fn build_image_info(
        &self,
        manifest_data: Option<String>,
        config_data: Option<(String, String)>,
        layers: Vec<LayerInfo>,
    ) -> Result<ImageInfo> {
        self.output.subsection("Building image metadata");
        
        let manifest_str = manifest_data
            .ok_or_else(|| PusherError::ImageParsing("No manifest.json found in archive".to_string()))?;
        
        let manifest: Vec<serde_json::Value> = serde_json::from_str(&manifest_str)
            .map_err(|e| PusherError::Parse(format!("Failed to parse manifest.json: {}", e)))?;
        
        let image_manifest = manifest.first()
            .ok_or_else(|| PusherError::ImageParsing("Empty manifest array".to_string()))?;
        
        let _config_path = image_manifest.get("Config")
            .and_then(|c| c.as_str())
            .ok_or_else(|| PusherError::ImageParsing("No Config field in manifest".to_string()))?;
        
        let (_, config_str) = config_data
            .ok_or_else(|| PusherError::ImageParsing("No config file found in archive".to_string()))?;
        
        let config: ImageConfig = serde_json::from_str(&config_str)
            .map_err(|e| PusherError::Parse(format!("Failed to parse image config: {}", e)))?;
        
        let mut hasher = Sha256::new();
        hasher.update(config_str.as_bytes());
        let config_digest = format!("sha256:{:x}", hasher.finalize());
        
        self.output.step(&format!("Found {} layers", layers.len()));
        self.output.step(&format!("Config digest: {}", &config_digest[..23]));
        
        // Count empty and large layers
        let empty_layers = layers.iter().filter(|l| l.size == 0).count();
        if empty_layers > 0 {
            self.output.info(&format!("Found {} empty layers", empty_layers));
        }
        
        Ok(ImageInfo {
            repository: "unknown".to_string(),
            tag: "latest".to_string(),
            layers,
            config,
            config_digest,
            total_size: 0, // Will be calculated by caller
            layer_count: 0, // Will be calculated by caller
            large_layers_count: 0, // Will be calculated by caller
        })
    }
}

enum EntryType {
    Manifest(String),
    Config((String, String)),
    Layer(LayerInfo),
    Other,
}