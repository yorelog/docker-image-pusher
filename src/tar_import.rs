use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
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

pub async fn import_tar_file(tar_path: &str, image_name: &str) -> Result<(), PusherError> {
    println!("📂 Opening tar archive: {}", tar_path);
    let tar_file = File::open(tar_path)
        .map_err(|e| PusherError::TarError(format!("Failed to open tar file: {}", e)))?;

    let mut archive = Archive::new(tar_file);
    archive
        .entries()
        .map_err(|e| PusherError::TarError(format!("Failed to read tar entries: {}", e)))?;

    let cache_dir = Path::new(CACHE_DIR);
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| PusherError::CacheError(format!("Failed to create cache directory: {}", e)))?;

    let image_cache_dir = cache_dir.join(image::sanitize_image_name(image_name));
    std::fs::create_dir_all(&image_cache_dir).map_err(|e| {
        PusherError::CacheError(format!("Failed to create image cache directory: {}", e))
    })?;

    println!("🔍 Searching for Docker manifest in tar archive...");
    let mut docker_manifest: Option<serde_json::Value> = None;

    let tar_file = File::open(tar_path)
        .map_err(|e| PusherError::TarError(format!("Failed to reopen tar file: {}", e)))?;
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
            entry
                .read_to_end(&mut contents)
                .map_err(|e| PusherError::TarError(format!("Failed to read manifest: {}", e)))?;

            docker_manifest = Some(serde_json::from_slice(&contents).map_err(|e| {
                PusherError::TarError(format!("Failed to parse manifest.json: {}", e))
            })?);
            break;
        }
    }

    let docker_manifest = docker_manifest.ok_or_else(|| {
        PusherError::TarError("No manifest.json found in tar archive".to_string())
    })?;

    let manifest_array = docker_manifest
        .as_array()
        .ok_or_else(|| PusherError::TarError("Invalid manifest.json format".to_string()))?;
    if manifest_array.is_empty() {
        return Err(PusherError::TarError("Empty manifest.json".to_string()));
    }

    let image_info = &manifest_array[0];
    let config_file = image_info["Config"]
        .as_str()
        .ok_or_else(|| PusherError::TarError("No Config field in manifest".to_string()))?;
    let layers = image_info["Layers"]
        .as_array()
        .ok_or_else(|| PusherError::TarError("No Layers field in manifest".to_string()))?;

    println!("📋 Found image with {} layers", layers.len());
    println!("⚙️  Config file: {}", config_file);

    let tar_file = File::open(tar_path)
        .map_err(|e| PusherError::TarError(format!("Failed to reopen tar file: {}", e)))?;
    let mut archive = Archive::new(tar_file);

    let mut digest_metadata: HashMap<String, (std::path::PathBuf, u64)> = HashMap::new();
    let mut path_to_digest: HashMap<String, String> = HashMap::new();
    let mut config_data: Option<(String, Vec<u8>)> = None;

    for entry_result in archive
        .entries()
        .map_err(|e| PusherError::TarError(format!("Failed to read tar entries: {}", e)))?
    {
        let mut entry = entry_result
            .map_err(|e| PusherError::TarError(format!("Failed to read tar entry: {}", e)))?;

        let path = entry
            .path()
            .map_err(|e| PusherError::TarError(format!("Failed to get entry path: {}", e)))?;
        let path_str = path.to_string_lossy();

        if path_str == config_file {
            println!("⚙️  Extracting config: {}", config_file);
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

        for layer in layers {
            let layer_path = layer
                .as_str()
                .ok_or_else(|| PusherError::TarError("Invalid layer path".to_string()))?;

            if path_str == layer_path {
                let layer_size = entry.size();
                let layer_size_mb = layer_size as f64 / (1024.0 * 1024.0);
                println!(
                    "📦 Extracting layer: {} ({:.1} MB)",
                    layer_path, layer_size_mb
                );
                let extract_start = Instant::now();

                let temp_layer_path =
                    image_cache_dir.join(format!("temp_layer_{}", std::process::id()));
                let mut temp_file = std::fs::File::create(&temp_layer_path).map_err(|e| {
                    PusherError::TarError(format!("Failed to create temp file: {}", e))
                })?;

                let mut hasher = Sha256::new();
                let mut buffer = [0u8; STREAM_BUFFER_SIZE];
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
                        show_extraction_progress(
                            total_read,
                            layer_size,
                            layer_size_mb,
                            extract_start,
                        );
                        last_progress_time = Instant::now();
                    }
                }

                temp_file.flush().map_err(|e| {
                    PusherError::TarError(format!("Failed to flush temp file: {}", e))
                })?;
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

                digest_metadata.insert(layer_digest.clone(), (final_layer_path, total_read));
                path_to_digest.insert(layer_path.to_string(), layer_digest.clone());

                break;
            }
        }
    }

    let (config_digest, config_contents) = config_data
        .ok_or_else(|| PusherError::TarError("Config file not found in tar".to_string()))?;

    let mut layer_sequence = Vec::new();
    for layer_value in layers {
        let layer_path = layer_value
            .as_str()
            .ok_or_else(|| PusherError::TarError("Invalid layer path".to_string()))?;
        let digest = path_to_digest.get(layer_path).ok_or_else(|| {
            PusherError::TarError(format!(
                "Layer path {} referenced in manifest but missing in tar",
                layer_path
            ))
        })?;
        layer_sequence.push(digest.clone());
    }

    println!(
        "✅ Successfully extracted {} layers and config",
        layer_sequence.len()
    );

    let mut oci_layers = Vec::new();
    let mut cached_layers = Vec::new();

    for layer_digest in &layer_sequence {
        let (layer_path, layer_size) = digest_metadata.get(layer_digest).ok_or_else(|| {
            PusherError::TarError(format!("Missing extracted data for layer {}", layer_digest))
        })?;
        cached_layers.push(layer_digest.clone());

        let media_type = detect_layer_media_type(layer_path)?;

        oci_layers.push(serde_json::json!({
            "mediaType": media_type,
            "size": *layer_size,
            "digest": layer_digest
        }));
    }

    let config_file_name = format!("config_{}.json", config_digest.replace(":", "_"));
    let config_path = image_cache_dir.join(&config_file_name);

    tokio::fs::write(&config_path, &config_contents)
        .await
        .map_err(|e| PusherError::CacheError(format!("Failed to cache config: {}", e)))?;

    let oci_manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "config": {
            "mediaType": "application/vnd.docker.container.image.v1+json",
            "size": config_contents.len(),
            "digest": config_digest
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
        "config": config_digest,
        "layers": cached_layers,
        "cached_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    });

    let index_json = serde_json::to_string_pretty(&index)?;
    tokio::fs::write(image_cache_dir.join("index.json"), index_json)
        .await
        .map_err(|e| PusherError::CacheError(format!("Failed to create index: {}", e)))?;

    println!(
        "🎉 Successfully imported tar archive with {} layers",
        cached_layers.len()
    );
    println!("💡 Cache structure matches pulled images - can be pushed with 'push' command");

    Ok(())
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

fn detect_layer_media_type(layer_path: &Path) -> Result<String, PusherError> {
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
