use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json;
use sha2::{Digest, Sha256};
use tar::Archive;

use crate::{
    CACHE_DIR, PROGRESS_LAYER_THRESHOLD_BYTES, PROGRESS_UPDATE_INTERVAL_SECS, PusherError,
    STREAM_BUFFER_SIZE, image, state,
};

#[derive(Debug, Clone)]
pub struct TarRepoInfo {
    pub registry: Option<String>,
    pub repository: String,
    pub image_name: String,
    pub tag: String,
}

/// Stream a docker-save archive into the cache without ever holding the full
/// contents in memory. The workflow mirrors `docker pull` so that pushes can
/// reuse exactly the same layout.
pub async fn import_tar_file(tar_path: &str, image_name: &str) -> Result<(), PusherError> {
    TarImporter::new().import(tar_path, image_name).await
}

/// Helper struct so tar import stages remain grouped and easier to reason about.
pub struct TarImporter;

#[derive(Debug)]
struct ManifestInfo {
    config_file: String,
    layers: Vec<String>,
}

#[derive(Debug)]
struct ExtractionArtifacts {
    config_digest: String,
    config_contents: Vec<u8>,
    ordered_layers: Vec<String>,
    layer_files: HashMap<String, LayerFile>,
}

#[derive(Debug)]
struct LayerFile {
    path: PathBuf,
    size: u64,
}

impl TarImporter {
    pub fn new() -> Self {
        Self
    }

    pub async fn import(&self, tar_path: &str, image_name: &str) -> Result<(), PusherError> {
        println!("📂 Opening tar archive: {}", tar_path);
        let image_cache_dir = self.prepare_image_cache_dir(image_name)?;
        let manifest = self.parse_primary_manifest(tar_path)?;

        println!("📋 Found image with {} layers", manifest.layers.len());
        println!("⚙️  Config file: {}", manifest.config_file);

        let artifacts = self.extract_layers_and_config(tar_path, &manifest, &image_cache_dir)?;
        self.persist_cache_entries(&image_cache_dir, image_name, tar_path, &artifacts)
            .await?;

        println!(
            "🎉 Successfully imported tar archive with {} layers",
            artifacts.ordered_layers.len()
        );
        println!("💡 Cache structure matches pulled images - can be pushed with 'push' command");

        Ok(())
    }

    /// Ensure the cache folder for the provided image exists and return its path.
    fn prepare_image_cache_dir(&self, image_name: &str) -> Result<PathBuf, PusherError> {
        let cache_dir = Path::new(CACHE_DIR);
        std::fs::create_dir_all(cache_dir).map_err(|e| {
            PusherError::CacheError(format!("Failed to create cache directory: {}", e))
        })?;

        let image_cache_dir = cache_dir.join(image::sanitize_image_name(image_name));
        std::fs::create_dir_all(&image_cache_dir).map_err(|e| {
            PusherError::CacheError(format!("Failed to create image cache directory: {}", e))
        })?;
        Ok(image_cache_dir)
    }

    /// Read the `manifest.json` entry from the docker-save archive and keep the
    /// pieces we care about for downstream processing.
    fn parse_primary_manifest(&self, tar_path: &str) -> Result<ManifestInfo, PusherError> {
        println!("🔍 Searching for Docker manifest in tar archive...");
        let tar_file = File::open(tar_path)
            .map_err(|e| PusherError::TarError(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(tar_file);

        for entry_result in archive
            .entries()
            .map_err(|e| PusherError::TarError(format!("Failed to read tar entries: {}", e)))?
        {
            let mut entry = entry_result
                .map_err(|e| PusherError::TarError(format!("Failed to read tar entry: {}", e)))?;

            let path = entry
                .path()
                .map_err(|e| PusherError::TarError(format!("Failed to get entry path: {}", e)))?;
            if path.to_string_lossy() == "manifest.json" {
                println!("📄 Found Docker manifest.json");
                let mut contents = Vec::new();
                entry.read_to_end(&mut contents).map_err(|e| {
                    PusherError::TarError(format!("Failed to read manifest: {}", e))
                })?;

                let docker_manifest: serde_json::Value = serde_json::from_slice(&contents)
                    .map_err(|e| {
                        PusherError::TarError(format!("Failed to parse manifest.json: {}", e))
                    })?;
                let manifest_array = docker_manifest.as_array().ok_or_else(|| {
                    PusherError::TarError("Invalid manifest.json format".to_string())
                })?;
                if manifest_array.is_empty() {
                    return Err(PusherError::TarError("Empty manifest.json".to_string()));
                }

                let image_info = &manifest_array[0];
                let config_file = image_info["Config"].as_str().ok_or_else(|| {
                    PusherError::TarError("No Config field in manifest".to_string())
                })?;
                let layers = image_info["Layers"].as_array().ok_or_else(|| {
                    PusherError::TarError("No Layers field in manifest".to_string())
                })?;

                return Ok(ManifestInfo {
                    config_file: config_file.to_string(),
                    layers: layers
                        .iter()
                        .map(|layer| {
                            layer
                                .as_str()
                                .ok_or_else(|| {
                                    PusherError::TarError("Invalid layer path".to_string())
                                })
                                .map(|value| value.to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                });
            }
        }

        Err(PusherError::TarError(
            "manifest.json not found in provided tar archive".to_string(),
        ))
    }

    /// Extract the config JSON and each referenced layer to the cache directory,
    /// computing digests along the way so we can reuse them during push.
    fn extract_layers_and_config(
        &self,
        tar_path: &str,
        manifest: &ManifestInfo,
        image_cache_dir: &Path,
    ) -> Result<ExtractionArtifacts, PusherError> {
        let tar_file = File::open(tar_path)
            .map_err(|e| PusherError::TarError(format!("Failed to reopen tar file: {}", e)))?;
        let mut archive = Archive::new(tar_file);

        let mut config_data: Option<(String, Vec<u8>)> = None;
        let mut path_to_digest: HashMap<String, String> = HashMap::new();
        let mut layer_files: HashMap<String, LayerFile> = HashMap::new();
        let mut buffer = vec![0u8; STREAM_BUFFER_SIZE];

        for entry_result in archive
            .entries()
            .map_err(|e| PusherError::TarError(format!("Failed to read tar entries: {}", e)))?
        {
            let mut entry = entry_result
                .map_err(|e| PusherError::TarError(format!("Failed to read tar entry: {}", e)))?;

            let path = entry
                .path()
                .map_err(|e| PusherError::TarError(format!("Failed to get entry path: {}", e)))?;
            let path_str = path.to_string_lossy().to_string();

            if path_str == manifest.config_file {
                println!("⚙️  Extracting config: {}", manifest.config_file);
                let mut contents = Vec::new();
                entry
                    .read_to_end(&mut contents)
                    .map_err(|e| PusherError::TarError(format!("Failed to read config: {}", e)))?;

                let mut hasher = Sha256::new();
                hasher.update(&contents);
                let config_digest = format!("sha256:{:x}", hasher.finalize());
                config_data = Some((config_digest, contents));
                continue;
            }

            if !manifest.layers.iter().any(|layer| layer == &path_str) {
                continue;
            }

            let layer_size = entry.size();
            let layer_size_mb = layer_size as f64 / (1024.0 * 1024.0);
            println!(
                "📦 Extracting layer: {} ({:.1} MB)",
                path_str, layer_size_mb
            );
            let extract_start = Instant::now();

            let temp_layer_path =
                image_cache_dir.join(format!("temp_layer_{}", std::process::id()));
            let mut temp_file = std::fs::File::create(&temp_layer_path)
                .map_err(|e| PusherError::TarError(format!("Failed to create temp file: {}", e)))?;

            let mut hasher = Sha256::new();
            let mut total_read = 0u64;
            let mut last_progress_time = Instant::now();

            loop {
                let bytes_read = entry.read(&mut buffer).map_err(|e| {
                    PusherError::TarError(format!("Failed to read layer chunk: {}", e))
                })?;

                if bytes_read == 0 {
                    break;
                }

                temp_file.write_all(&buffer[..bytes_read]).map_err(|e| {
                    PusherError::TarError(format!("Failed to write layer chunk: {}", e))
                })?;

                hasher.update(&buffer[..bytes_read]);
                total_read += bytes_read as u64;

                if layer_size > PROGRESS_LAYER_THRESHOLD_BYTES
                    && last_progress_time.elapsed()
                        > Duration::from_secs(PROGRESS_UPDATE_INTERVAL_SECS)
                {
                    self.show_extraction_progress(
                        total_read,
                        layer_size,
                        layer_size_mb,
                        extract_start,
                    );
                    last_progress_time = Instant::now();
                }
            }

            temp_file
                .flush()
                .map_err(|e| PusherError::TarError(format!("Failed to flush temp file: {}", e)))?;
            drop(temp_file);

            let layer_digest = format!("sha256:{:x}", hasher.finalize());
            let extract_duration = extract_start.elapsed();
            let extract_speed = if extract_duration.as_secs() > 0 {
                layer_size_mb / extract_duration.as_secs_f64()
            } else {
                0.0
            };

            println!(
                "   ✅ Layer extracted: {} in {:.1}s @ {:.1} MB/s",
                layer_digest,
                extract_duration.as_secs_f64(),
                extract_speed
            );

            let final_layer_path = image_cache_dir.join(layer_digest.replace(":", "_"));
            std::fs::rename(&temp_layer_path, &final_layer_path).map_err(|e| {
                PusherError::TarError(format!("Failed to rename layer file: {}", e))
            })?;

            path_to_digest.insert(path_str.to_string(), layer_digest.clone());
            layer_files.insert(
                layer_digest,
                LayerFile {
                    path: final_layer_path,
                    size: total_read,
                },
            );
        }

        let (config_digest, config_contents) = config_data
            .ok_or_else(|| PusherError::TarError("Config file not found in tar".to_string()))?;

        let mut ordered_layers = Vec::new();
        for layer_path in &manifest.layers {
            let digest = path_to_digest.get(layer_path).ok_or_else(|| {
                PusherError::TarError(format!(
                    "Layer path {} referenced in manifest but missing in tar",
                    layer_path
                ))
            })?;
            ordered_layers.push(digest.clone());
        }

        println!(
            "✅ Successfully extracted {} layers and config",
            ordered_layers.len()
        );

        Ok(ExtractionArtifacts {
            config_digest,
            config_contents,
            ordered_layers,
            layer_files,
        })
    }

    /// Persist the extracted components (config, manifest, index) so the cache
    /// looks identical to the one created by the pull command.
    async fn persist_cache_entries(
        &self,
        image_cache_dir: &Path,
        image_name: &str,
        tar_path: &str,
        artifacts: &ExtractionArtifacts,
    ) -> Result<(), PusherError> {
        let config_file_name = format!("config_{}.json", artifacts.config_digest.replace(":", "_"));
        let config_path = image_cache_dir.join(&config_file_name);

        tokio::fs::write(&config_path, &artifacts.config_contents)
            .await
            .map_err(|e| PusherError::CacheError(format!("Failed to cache config: {}", e)))?;

        let mut oci_layers = Vec::new();
        for digest in &artifacts.ordered_layers {
            let file = artifacts.layer_files.get(digest).ok_or_else(|| {
                PusherError::TarError(format!("Missing extracted data for layer {}", digest))
            })?;
            let media_type = self.detect_layer_media_type(&file.path)?;
            oci_layers.push(serde_json::json!({
                "mediaType": media_type,
                "size": file.size,
                "digest": digest
            }));
        }

        let oci_manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": {
                "mediaType": "application/vnd.docker.container.image.v1+json",
                "size": artifacts.config_contents.len(),
                "digest": artifacts.config_digest
            },
            "layers": oci_layers
        });

        let manifest_path = image_cache_dir.join("manifest.json");
        let manifest_json = serde_json::to_string_pretty(&oci_manifest)?;
        tokio::fs::write(&manifest_path, manifest_json)
            .await
            .map_err(|e| PusherError::CacheError(format!("Failed to cache manifest: {}", e)))?;

        let index = serde_json::json!({
            "source_image": image_name,
            "source_type": "tar_import",
            "source_file": tar_path,
            "manifest": "manifest.json",
            "config": artifacts.config_digest,
            "layers": artifacts.ordered_layers,
            "cached_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        let index_json = serde_json::to_string_pretty(&index)?;
        tokio::fs::write(image_cache_dir.join("index.json"), index_json)
            .await
            .map_err(|e| PusherError::CacheError(format!("Failed to create index: {}", e)))?;

        Ok(())
    }

    fn detect_layer_media_type(&self, layer_path: &Path) -> Result<String, PusherError> {
        let mut file = std::fs::File::open(layer_path)
            .map_err(|e| PusherError::tar_error(format!("Failed to open layer file: {}", e)))?;

        let mut buffer = [0u8; 2];
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|e| PusherError::tar_error(format!("Failed to read layer header: {}", e)))?;

        if bytes_read >= 2 && buffer == crate::GZIP_MAGIC_BYTES {
            Ok("application/vnd.docker.image.rootfs.diff.tar.gzip".to_string())
        } else if bytes_read >= 2 {
            Ok("application/vnd.docker.image.rootfs.diff.tar".to_string())
        } else {
            Ok("application/vnd.docker.image.rootfs.diff.tar.gzip".to_string())
        }
    }

    fn show_extraction_progress(
        &self,
        total_read: u64,
        layer_size: u64,
        layer_size_mb: f64,
        extract_start: Instant,
    ) {
        let progress = (total_read as f64 / layer_size as f64) * 100.0;
        let elapsed = extract_start.elapsed();
        let mb_per_sec = if elapsed.as_secs() > 0 {
            (total_read as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64()
        } else {
            0.0
        };

        println!(
            "   📊 Progress: {:.1}% ({:.1}/{:.1} MB) @ {:.1} MB/s",
            progress,
            total_read as f64 / (1024.0 * 1024.0),
            layer_size_mb,
            mb_per_sec
        );
    }
}

pub fn tar_repo_info_from_path(tar_path: &str) -> Result<TarRepoInfo, PusherError> {
    let repo_tags = extract_repo_tags_from_tar(tar_path)?;
    let repo_tag = repo_tags
        .first()
        .ok_or_else(|| PusherError::tar_error("No RepoTags entries found inside manifest.json"))?;
    println!("📝 Tar archive RepoTag detected: {}", repo_tag);
    parse_repo_tag(repo_tag)
}

pub fn build_target_from_tar(info: &TarRepoInfo, registry_override: Option<&str>) -> String {
    let registry_part = registry_override
        .map(|value| value.trim_end_matches('/').to_string())
        .or_else(|| info.registry.clone());
    let repository = if let Some(registry) = registry_part {
        format!("{}/{}", registry, info.repository)
    } else {
        info.repository.clone()
    };
    format!("{}:{}", repository, info.tag)
}

pub async fn infer_target_from_history(info: &TarRepoInfo) -> Result<Option<String>, PusherError> {
    match state::recent_targets().await {
        Ok(history) => {
            for entry in history {
                if let Some(candidate) = adjust_target_from_history(&entry, info) {
                    println!(
                        "💡 Using recent push history to derive target: {} (based on {})",
                        candidate, entry
                    );
                    return Ok(Some(candidate));
                }
            }
            Ok(None)
        }
        Err(err) => {
            println!("⚠️  Unable to read push history: {}", err);
            Ok(None)
        }
    }
}

fn adjust_target_from_history(previous: &str, info: &TarRepoInfo) -> Option<String> {
    let (repo_without_tag, _) = previous.rsplit_once(':')?;
    let new_repository = if let Some(last_slash) = repo_without_tag.rfind('/') {
        let prefix = &repo_without_tag[..last_slash];
        format!("{}/{}", prefix, info.image_name)
    } else {
        info.image_name.clone()
    };
    Some(format!("{}:{}", new_repository, info.tag))
}

fn extract_repo_tags_from_tar(tar_path: &str) -> Result<Vec<String>, PusherError> {
    let tar_file = File::open(tar_path).map_err(|e| {
        PusherError::tar_error(format!("Failed to open tar file {}: {}", tar_path, e))
    })?;
    let mut archive = Archive::new(tar_file);
    for entry_result in archive
        .entries()
        .map_err(|e| PusherError::tar_error(format!("Failed to iterate tar entries: {}", e)))?
    {
        let mut entry = entry_result
            .map_err(|e| PusherError::tar_error(format!("Failed to read tar entry: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| PusherError::tar_error(format!("Failed to read tar entry path: {}", e)))?;
        if path.to_string_lossy() == "manifest.json" {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents).map_err(|e| {
                PusherError::tar_error(format!("Failed to read manifest.json: {}", e))
            })?;
            let manifest: serde_json::Value = serde_json::from_slice(&contents).map_err(|e| {
                PusherError::tar_error(format!("Failed to parse manifest.json: {}", e))
            })?;
            let images = manifest.as_array().ok_or_else(|| {
                PusherError::tar_error("manifest.json is not an array of image entries")
            })?;
            let mut tags = Vec::new();
            for image in images {
                if let Some(repo_tags) = image["RepoTags"].as_array() {
                    for tag in repo_tags {
                        if let Some(tag_str) = tag.as_str() {
                            tags.push(tag_str.to_string());
                        }
                    }
                }
            }
            if tags.is_empty() {
                return Err(PusherError::tar_error(
                    "manifest.json contains no RepoTags entries",
                ));
            }
            return Ok(tags);
        }
    }

    Err(PusherError::tar_error(
        "manifest.json not found in provided tar archive",
    ))
}

fn parse_repo_tag(repo_tag: &str) -> Result<TarRepoInfo, PusherError> {
    let (repository_part, tag) = repo_tag.rsplit_once(':').ok_or_else(|| {
        PusherError::tar_error(format!("RepoTag '{}' is missing a tag suffix", repo_tag))
    })?;
    if tag.is_empty() {
        return Err(PusherError::tar_error(format!(
            "RepoTag '{}' has an empty tag",
            repo_tag
        )));
    }

    let (registry, repository) = split_registry(repository_part);
    let image_name = repository
        .split('/')
        .last()
        .unwrap_or(repository.as_str())
        .to_string();

    Ok(TarRepoInfo {
        registry,
        repository,
        image_name,
        tag: tag.to_string(),
    })
}

fn split_registry(repo: &str) -> (Option<String>, String) {
    if let Some(slash_index) = repo.find('/') {
        let candidate = &repo[..slash_index];
        if candidate.contains('.') || candidate.contains(':') || candidate == "localhost" {
            let remainder = repo[slash_index + 1..].to_string();
            return (Some(candidate.to_string()), remainder);
        }
    }
    (None, repo.to_string())
}
