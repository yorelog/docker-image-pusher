// This file contains the ImageParser struct, which is responsible for extracting image layers and metadata from the tar package.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use tar::Archive;
use crate::error::{Result, PusherError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sha2::{Sha256, Digest};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LayerInfo {
    pub digest: String,
    pub size: u64,
    pub media_type: String,
    pub tar_path: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImageConfig {
    pub architecture: Option<String>,
    pub os: Option<String>,
    pub config: Option<serde_json::Value>,
    pub rootfs: Option<serde_json::Value>,
    pub history: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImageInfo {
    pub repository: String,
    pub tag: String,
    pub layers: Vec<LayerInfo>,
    pub config_digest: String,
    pub config: ImageConfig,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    #[serde(rename = "Config")]
    config: String,
    #[serde(rename = "RepoTags")]
    repo_tags: Vec<String>,
    #[serde(rename = "Layers")]
    layers: Vec<String>,
}

pub struct ImageParser;

impl ImageParser {
    pub fn new() -> Self {
        Self
    }

    pub async fn parse_tar_file(&self, tar_path: &Path) -> Result<ImageInfo> {
        println!("Opening tar file: {}", tar_path.display());
        
        if !tar_path.exists() {
            return Err(PusherError::ImageParsing(format!("Tar file does not exist: {}", tar_path.display())));
        }

        // Scan tar file to collect information
        let (manifest_entry, config_data, layer_files) = self.scan_tar_file(tar_path)?;
        
        // Parse config
        let config: ImageConfig = serde_json::from_str(&config_data)
            .map_err(|e| PusherError::ImageParsing(format!("Failed to parse config: {}", e)))?;
        
        // Build layer info
        let layers = self.build_layer_info(&manifest_entry, &layer_files)?;
        
        // Parse repository and tag
        let (repository, tag) = self.parse_repo_tag(&manifest_entry.repo_tags)?;
        
        // Calculate config digest
        let config_digest = self.calculate_config_digest(&config_data)?;
        
        println!("Image info:");
        println!("  Repository: {}", repository);
        println!("  Tag: {}", tag);
        println!("  Layers: {} found", layers.len());
        for (i, layer) in layers.iter().enumerate() {
            println!("    Layer {}: {} ({})", i + 1, layer.digest, layer.size);
        }
        
        Ok(ImageInfo {
            repository,
            tag,
            layers,
            config_digest,
            config,
        })
    }

    fn scan_tar_file(&self, tar_path: &Path) -> Result<(ManifestEntry, String, HashMap<String, u64>)> {
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(e))?;
        let mut archive = Archive::new(file);
        
        let mut manifest_data = None;
        let mut config_data = None;
        let mut layer_files = HashMap::new();
        
        println!("Scanning tar entries...");
        
        for entry in archive.entries().map_err(PusherError::Io)? {
            let mut entry = entry.map_err(PusherError::Io)?;
            let path = entry.path().map_err(PusherError::Io)?.to_string_lossy().to_string();
            let size = entry.header().size().map_err(PusherError::Io)?;
            
            println!("  Found: {}", path);
            
            if path == "manifest.json" {
                let mut contents = String::new();
                entry.read_to_string(&mut contents).map_err(PusherError::Io)?;
                manifest_data = Some(contents);
                println!("    -> Manifest file found");
            } else if path.ends_with(".json") && !path.contains("manifest") {
                let mut contents = String::new();
                entry.read_to_string(&mut contents).map_err(PusherError::Io)?;
                config_data = Some(contents);
                println!("    -> Config file found: {}", path);
            } else if path.ends_with(".tar") || path.contains("layer") {
                layer_files.insert(path.clone(), size);
                println!("    -> Layer file found: {} ({} bytes)", path, size);
            }
        }
        
        let manifest_str = manifest_data
            .ok_or_else(|| PusherError::ImageParsing("No manifest.json found in tar".to_string()))?;
        
        let manifest_array: Vec<ManifestEntry> = serde_json::from_str(&manifest_str)
            .map_err(|e| PusherError::ImageParsing(format!("Failed to parse manifest: {}", e)))?;
        
        let manifest_entry = manifest_array.into_iter().next()
            .ok_or_else(|| PusherError::ImageParsing("Empty manifest".to_string()))?;
        
        let config = config_data
            .ok_or_else(|| PusherError::ImageParsing("No config file found in tar".to_string()))?;
        
        println!("Parsed manifest:");
        println!("  Config: {}", manifest_entry.config);
        println!("  RepoTags: {:?}", manifest_entry.repo_tags);
        println!("  Layers: {} entries", manifest_entry.layers.len());
        
        Ok((manifest_entry, config, layer_files))
    }

    fn build_layer_info(&self, manifest_entry: &ManifestEntry, layer_files: &HashMap<String, u64>) -> Result<Vec<LayerInfo>> {
        let mut layers = Vec::new();
        
        for layer_path in &manifest_entry.layers {
            let size = layer_files.get(layer_path)
                .copied()
                .unwrap_or(0);
            
            // Extract digest from layer path or calculate it
            let digest = self.extract_digest_from_path(layer_path)?;
            
            layers.push(LayerInfo {
                digest,
                size,
                media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                tar_path: layer_path.clone(),
            });
        }
        
        Ok(layers)
    }

    fn extract_digest_from_path(&self, layer_path: &str) -> Result<String> {
        // Try to extract digest from path like "sha256:abc123.tar" or "abc123/layer.tar"
        if layer_path.contains("sha256:") {
            if let Some(start) = layer_path.find("sha256:") {
                let digest_part = &layer_path[start..];
                if let Some(end) = digest_part.find('.') {
                    return Ok(digest_part[..end].to_string());
                } else {
                    return Ok(digest_part.to_string());
                }
            }
        }
        
        // For paths like "abc123.tar", assume abc123 is the short digest
        if let Some(file_name) = layer_path.split('/').last() {
            if let Some(name_without_ext) = file_name.strip_suffix(".tar") {
                if name_without_ext.len() == 64 {
                    return Ok(format!("sha256:{}", name_without_ext));
                }
            }
        }
        
        // For paths like "abc123/layer.tar", use the directory name
        if let Some(dir_name) = layer_path.split('/').next() {
            if dir_name.len() == 64 {
                return Ok(format!("sha256:{}", dir_name));
            }
        }
        
        Err(PusherError::ImageParsing(format!("Could not extract digest from layer path: {}", layer_path)))
    }

    fn parse_repo_tag(&self, repo_tags: &[String]) -> Result<(String, String)> {
        let repo_tag = repo_tags.first()
            .ok_or_else(|| PusherError::ImageParsing("No repository tags found".to_string()))?;
            
        if let Some((repo, tag)) = repo_tag.rsplit_once(':') {
            Ok((repo.to_string(), tag.to_string()))
        } else {
            Ok((repo_tag.clone(), "latest".to_string()))
        }
    }

    fn calculate_config_digest(&self, config_data: &str) -> Result<String> {
        let mut hasher = Sha256::new();
        hasher.update(config_data.as_bytes());
        Ok(format!("sha256:{:x}", hasher.finalize()))
    }
}