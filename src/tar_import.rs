use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json;
use sha2::{Digest, Sha256};
use tar::Archive;
use tempfile::TempDir;
use tokio::sync::mpsc;

use crate::{
    GZIP_MAGIC_BYTES, PROGRESS_LAYER_THRESHOLD_BYTES, PROGRESS_UPDATE_INTERVAL_SECS, PusherError,
    STREAM_BUFFER_SIZE, state,
};
use oci_core::blobs::LocalLayer;

/// Metadata about the repository/tag fields embedded in a docker-save archive.
#[derive(Debug, Clone)]
pub struct TarRepoInfo {
    pub registry: Option<String>,
    pub repository: String,
    pub image_name: String,
    pub tag: String,
}

/// Result of extracting a docker-save archive into temporary artifacts ready for push.
#[derive(Debug)]
pub struct TarExtraction {
    pub config_digest: String,
    pub config_contents: Vec<u8>,
    pub layers: Vec<LocalLayer>,
    _temp_dir: TempDir,
}

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
    media_type: String,
}

/// Variant of [`extract_tar_archive`] that also emits each extracted layer over the provided
/// channel, allowing callers to start uploading while extraction continues.
pub fn extract_tar_archive_with_sender(
    tar_path: &str,
    layer_sender: Option<mpsc::Sender<LocalLayer>>,
) -> Result<TarExtraction, PusherError> {
    let manifest = parse_primary_manifest(tar_path)?;
    let temp_dir = TempDir::new().map_err(|e| {
        PusherError::tar_error(format!(
            "Failed to create temp dir for tar extraction: {}",
            e
        ))
    })?;
    let artifacts = extract_layers_and_config(tar_path, &manifest, temp_dir.path(), layer_sender)?;

    let mut layers = Vec::new();
    for digest in &artifacts.ordered_layers {
        let file = artifacts.layer_files.get(digest).ok_or_else(|| {
            PusherError::tar_error(format!("Missing extracted data for layer {}", digest))
        })?;
        layers.push(LocalLayer {
            digest: digest.clone(),
            media_type: file.media_type.clone(),
            size: file.size,
            path: file.path.clone(),
        });
    }

    Ok(TarExtraction {
        config_digest: artifacts.config_digest,
        config_contents: artifacts.config_contents,
        layers,
        _temp_dir: temp_dir,
    })
}

fn parse_primary_manifest(tar_path: &str) -> Result<ManifestInfo, PusherError> {
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
            let mut contents = Vec::new();
            entry
                .read_to_end(&mut contents)
                .map_err(|e| PusherError::TarError(format!("Failed to read manifest: {}", e)))?;

            let docker_manifest: serde_json::Value =
                serde_json::from_slice(&contents).map_err(|e| {
                    PusherError::TarError(format!("Failed to parse manifest.json: {}", e))
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

            return Ok(ManifestInfo {
                config_file: config_file.to_string(),
                layers: layers
                    .iter()
                    .map(|layer| {
                        layer
                            .as_str()
                            .ok_or_else(|| PusherError::TarError("Invalid layer path".to_string()))
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

fn extract_layers_and_config(
    tar_path: &str,
    manifest: &ManifestInfo,
    output_dir: &Path,
    layer_sender: Option<mpsc::Sender<LocalLayer>>,
) -> Result<ExtractionArtifacts, PusherError> {
    let tar_file = File::open(tar_path)
        .map_err(|e| PusherError::TarError(format!("Failed to reopen tar file: {}", e)))?;
    let mut archive = Archive::new(tar_file);

    let mut config_data: Option<(String, Vec<u8>)> = None;
    let mut path_to_digest: HashMap<String, String> = HashMap::new();
    let mut layer_files: HashMap<String, LayerFile> = HashMap::new();
    let mut buffer = vec![0u8; STREAM_BUFFER_SIZE];
    let layer_whitelist: HashSet<&str> =
        manifest.layers.iter().map(|layer| layer.as_str()).collect();

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

        if !layer_whitelist.contains(path_str.as_str()) {
            continue;
        }

        let layer_size = entry.size();
        let layer_size_mb = layer_size as f64 / (1024.0 * 1024.0);
        println!(
            "📦 Extracting layer: {} ({:.1} MB)",
            path_str, layer_size_mb
        );
        let extract_start = Instant::now();

        let temp_layer_path = output_dir.join(format!(
            "layer_{}_{}",
            std::process::id(),
            path_str.replace('/', "_")
        ));
        let mut temp_file = std::fs::File::create(&temp_layer_path)
            .map_err(|e| PusherError::TarError(format!("Failed to create temp file: {}", e)))?;

        let mut hasher = Sha256::new();
        let mut total_read = 0u64;
        let mut last_progress_time = Instant::now();

        loop {
            let bytes_read = entry
                .read(&mut buffer)
                .map_err(|e| PusherError::TarError(format!("Failed to read layer chunk: {}", e)))?;

            if bytes_read == 0 {
                break;
            }

            temp_file.write_all(&buffer[..bytes_read]).map_err(|e| {
                PusherError::TarError(format!("Failed to write layer chunk: {}", e))
            })?;

            hasher.update(&buffer[..bytes_read]);
            total_read += bytes_read as u64;

            if layer_size > PROGRESS_LAYER_THRESHOLD_BYTES
                && last_progress_time.elapsed() > Duration::from_secs(PROGRESS_UPDATE_INTERVAL_SECS)
            {
                show_extraction_progress(total_read, layer_size, layer_size_mb, extract_start);
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

        let final_layer_path = output_dir.join(layer_digest.replace(":", "_"));
        std::fs::rename(&temp_layer_path, &final_layer_path)
            .map_err(|e| PusherError::TarError(format!("Failed to rename layer file: {}", e)))?;

        let media_type = detect_layer_media_type(&final_layer_path)?;
        path_to_digest.insert(path_str.to_string(), layer_digest.clone());
        layer_files.insert(
            layer_digest.clone(),
            LayerFile {
                path: final_layer_path.clone(),
                size: total_read,
                media_type: media_type.clone(),
            },
        );

        if let Some(sender) = &layer_sender {
            let layer = LocalLayer {
                digest: layer_digest.clone(),
                media_type: media_type.clone(),
                size: total_read,
                path: final_layer_path.clone(),
            };
            if let Err(err) = sender.blocking_send(layer) {
                println!(
                    "⚠️  Layer upload channel closed early ({}). Continuing extraction...",
                    err
                );
            }
        }
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

fn detect_layer_media_type(layer_path: &Path) -> Result<String, PusherError> {
    let mut file = std::fs::File::open(layer_path)
        .map_err(|e| PusherError::tar_error(format!("Failed to open layer file: {}", e)))?;

    let mut buffer = [0u8; 2];
    let bytes_read = file
        .read(&mut buffer)
        .map_err(|e| PusherError::tar_error(format!("Failed to read layer header: {}", e)))?;

    if bytes_read >= 2 && buffer == GZIP_MAGIC_BYTES {
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

/// Parses the first RepoTag entry inside the tar's `manifest.json`.
pub fn tar_repo_info_from_path(tar_path: &str) -> Result<TarRepoInfo, PusherError> {
    let repo_tags = extract_repo_tags_from_tar(tar_path)?;
    let repo_tag = repo_tags
        .first()
        .ok_or_else(|| PusherError::tar_error("No RepoTags entries found inside manifest.json"))?;
    println!("📝 Tar archive RepoTag detected: {}", repo_tag);
    parse_repo_tag(repo_tag)
}

/// Constructs `<registry>/<repository>:<tag>` strings using tar metadata and optional overrides.
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

/// Uses recent push history to infer a target that matches the tar's image name/tag.
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
