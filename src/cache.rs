use crate::image;
use crate::PusherError;
use oci_client::{Client, Reference};

use std::path::Path;
use tokio::io::AsyncWriteExt;

/// Downloads and caches a Docker image using memory-efficient streaming with parallel processing
///
/// This function implements the core memory optimization strategy:
/// 1. Fetches only the image manifest first (small metadata)
/// 2. Downloads each layer in parallel using streaming APIs (controlled concurrency)
/// 3. Writes layers directly to disk without loading into memory
/// 4. Uses semaphore to limit concurrent downloads and prevent registry overload
///
/// # Cache Structure
///
/// Images are cached in `.cache/{sanitized_image_name}/` with:
/// - `manifest.json` - The OCI image manifest
/// - `config_{digest}.json` - The image configuration
/// - `{layer_digest}` - Individual layer files  
/// - `index.json` - Metadata for quick lookup
///
/// # Arguments
///
/// * `client` - OCI client for registry operations
/// * `source_image` - Image reference to pull (e.g., "nginx:latest")
///
/// # Returns
///
/// `Result<(), PusherError>` - Success or detailed error information
pub async fn cache_image(client: &Client, source_image: &str) -> Result<(), PusherError> {
    // Use anonymous authentication for public registries
    let auth = oci_client::secrets::RegistryAuth::Anonymous;

    // Parse the image reference to validate format and extract components
    let image_ref: Reference = source_image
        .parse()
        .map_err(|e| PusherError::PullError(format!("Invalid image reference: {}", e)))?;

    println!("📋 Pulling image: {}", source_image);
    println!("🔍 Parsed reference: {}", image_ref);

    // Step 1: Pull only the manifest (small metadata, ~1-5KB typically)
    // This gives us the list of layers and config without downloading everything
    println!("📄 Fetching manifest...");
    let (manifest, _digest) = client
        .pull_image_manifest(&image_ref, &auth)
        .await
        .map_err(|e| PusherError::PullError(format!("Failed to pull manifest: {}", e)))?;

    // Step 2: Set up local cache directory structure
    let cache_dir = Path::new(".cache");
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| PusherError::CacheError(format!("Failed to create cache directory: {}", e)))?;

    let image_cache_dir = cache_dir.join(image::sanitize_image_name(source_image));
    std::fs::create_dir_all(&image_cache_dir).map_err(|e| {
        PusherError::CacheError(format!("Failed to create image cache directory: {}", e))
    })?;
    let total_layers = manifest.layers.len();
    println!(
        "💾 Streaming {} layers to cache sequentially for memory efficiency...",
        total_layers
    );
    // Step 3: Process layers sequentially with memory-efficient streaming and cache checks
    let mut cached_layers = Vec::new();
    let mut skipped_layers = 0;

    for (i, layer_desc) in manifest.layers.iter().enumerate() {
        let layer_digest = layer_desc.digest.to_string();
        let layer_path = image_cache_dir.join(layer_digest.replace(":", "_"));
        let layer_size_mb = layer_desc.size as f64 / (1024.0 * 1024.0);
        // Check if layer is already cached and complete
        if is_layer_cached(&image_cache_dir, &layer_digest, layer_desc.size as u64).await? {
            println!(
                "📦 Layer {}/{}: {} ({:.1} MB) - ✅ Already cached, skipping download",
                i + 1,
                total_layers,
                layer_digest,
                layer_size_mb
            );
            cached_layers.push(layer_digest);
            skipped_layers += 1;
            continue;
        }

        println!(
            "📦 Streaming layer {}/{}: {} ({:.1} MB)",
            i + 1,
            total_layers,
            layer_digest,
            layer_size_mb
        );
        let download_start = std::time::Instant::now();

        let mut file = tokio::fs::File::create(&layer_path).await.map_err(|e| {
            PusherError::CacheError(format!(
                "Failed to create layer file {}: {}",
                layer_digest, e
            ))
        })?;

        client
            .pull_blob(&image_ref, layer_desc, &mut file)
            .await
            .map_err(|e| {
                PusherError::PullError(format!("Failed to stream layer {}: {}", layer_digest, e))
            })?;

        file.flush().await.map_err(|e| {
            PusherError::CacheError(format!(
                "Failed to flush layer file {}: {}",
                layer_digest, e
            ))
        })?;

        let download_duration = download_start.elapsed();
        let download_speed = if download_duration.as_secs() > 0 {
            layer_size_mb / download_duration.as_secs_f64()
        } else {
            0.0
        };

        println!(
            "   ✅ Downloaded layer: {} in {:.1}s @ {:.1} MB/s",
            layer_digest,
            download_duration.as_secs_f64(),
            download_speed
        );
        cached_layers.push(layer_digest);
    }
    println!(
        "🚀 Sequential download completed for {} layers",
        cached_layers.len()
    );
    if skipped_layers > 0 {
        println!(
            "💡 Skipped {} layers that were already cached",
            skipped_layers
        );
    }

    // Step 4: Cache the manifest for later reconstruction
    let manifest_path = image_cache_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    tokio::fs::write(&manifest_path, manifest_json)
        .await
        .map_err(|e| PusherError::CacheError(format!("Failed to cache manifest: {}", e)))?;

    // Step 5: Stream and cache the config blob (typically small, <10KB)
    let config_desc = &manifest.config;
    let config_digest = config_desc.digest.to_string();
    let config_path =
        image_cache_dir.join(format!("config_{}.json", config_digest.replace(":", "_")));

    let mut config_file = tokio::fs::File::create(&config_path)
        .await
        .map_err(|e| PusherError::CacheError(format!("Failed to create config file: {}", e)))?;

    client
        .pull_blob(&image_ref, config_desc, &mut config_file)
        .await
        .map_err(|e| PusherError::PullError(format!("Failed to stream config: {}", e)))?;

    config_file
        .flush()
        .await
        .map_err(|e| PusherError::CacheError(format!("Failed to flush config file: {}", e)))?;

    // Step 6: Create index file for quick cache lookup and metadata
    let index = serde_json::json!({
        "source_image": source_image,
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
        "✅ Successfully cached image with {} layers",
        cached_layers.len()
    );
    Ok(())
}

/// Checks if an image is already cached locally
///
/// This is a quick check that looks for the presence of an index.json file
/// in the expected cache directory for the given image.
///
/// # Arguments
///
/// * `source_image` - Image name to check for in cache
///
/// # Returns
///
/// `Result<bool, PusherError>` - true if cached, false if not found
pub async fn has_cached_image(source_image: &str) -> Result<bool, PusherError> {
    let cache_dir = Path::new(".cache");
    let image_cache_dir = cache_dir.join(image::sanitize_image_name(source_image));
    let index_path = image_cache_dir.join("index.json");

    Ok(tokio::fs::metadata(&index_path).await.is_ok())
}

/// Checks if a specific layer is already cached locally
///
/// This function verifies that a layer file exists in the cache and has the expected size
/// to ensure it's a complete download.
///
/// # Arguments
///
/// * `cache_dir` - The cache directory path for the image
/// * `layer_digest` - The digest of the layer to check
/// * `expected_size` - Expected size of the layer in bytes
///
/// # Returns
///
/// `Result<bool, PusherError>` - true if layer exists and is complete, false otherwise
async fn is_layer_cached(
    cache_dir: &std::path::Path,
    layer_digest: &str,
    expected_size: u64,
) -> Result<bool, PusherError> {
    let layer_path = cache_dir.join(layer_digest.replace(":", "_"));

    match tokio::fs::metadata(&layer_path).await {
        Ok(metadata) => {
            // Check if file exists and has the expected size
            Ok(metadata.len() == expected_size)
        }
        Err(_) => Ok(false), // File doesn't exist
    }
}
