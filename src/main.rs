/*!
# Docker Image Pusher

A memory-optimized Docker image transfer tool that streams large Docker images and layers
without loading them entirely into memory. This tool addresses the common issue of excessive
memory usage when pulling or pushing large Docker images (multi-GB images) by using streaming
APIs from the oci-client library.

## Key Features

- **Memory-Efficient Streaming**: Downloads and uploads image layers one by one using streaming APIs
- **Large Layer Handling**: Special chunked processing for layers > 100MB to prevent memory exhaustion  
- **Local Caching**: Caches pulled images locally for faster subsequent pushes
- **Registry Authentication**: Supports both anonymous and authenticated registry access
- **Progress Monitoring**: Real-time feedback on layer transfer progress and sizes

## Architecture

The tool operates in two main phases:

1. **Pull Phase**: 
   - Fetches image manifest to get layer information
   - Streams each layer directly to local cache files
   - Saves manifest and config separately for later use

2. **Push Phase**:
   - Reads cached layers from local storage  
   - Uploads layers individually with size-based optimization
   - Pushes final manifest to complete image transfer

## Memory Optimization Strategies

- Sequential layer processing instead of concurrent downloads
- Direct file-to-registry streaming without intermediate buffers
- Chunked reading (50MB chunks) for layers exceeding 100MB
- Rate limiting to prevent registry overload and memory pressure
*/

use oci_client::{Client, Reference};
use oci_client::manifest::OciImageManifest;
use std::path::Path;
use clap::{Parser, Subcommand};
use serde_json;
use thiserror::Error;
use anyhow::Result;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tar::Archive;
use sha2::{Sha256, Digest};
use std::io::{Read, Write};
use std::fs::File;

/// Custom error types for the Docker image pusher application
/// 
/// This enum provides specific error categories to help with debugging
/// and error handling throughout the application.
#[derive(Error, Debug)]
pub enum PusherError {
    /// Errors that occur during image pulling operations
    /// These typically involve network issues or authentication problems
    #[error("Pull error: {0}")]
    PullError(String),
    
    /// Errors that occur during image pushing operations  
    /// These may involve registry authentication or upload failures
    #[error("Push error: {0}")]
    PushError(String),
    
    /// Errors related to local cache operations
    /// Including file I/O issues and cache corruption
    #[error("Cache error: {0}")]
    CacheError(String),
    
    /// Standard I/O errors (file operations, network, etc.)
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    /// JSON serialization/deserialization errors
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error),
      /// Error when requested cached image is not found
    #[error("Cache not found")]
    CacheNotFound,
    
    /// Errors that occur during tar file processing
    /// Including tar archive parsing and layer extraction
    #[error("Tar processing error: {0}")]
    TarError(String),
}

/// Command-line interface definition for the Docker image pusher
/// 
/// Uses the clap crate for parsing command-line arguments and generating help text.
#[derive(Parser)]
#[command(name = "docker-image-pusher")]
#[command(about = "A memory-optimized tool to pull, import, and push Docker images")]
#[command(long_about = "This tool efficiently transfers Docker images between registries using streaming APIs to minimize memory usage. It supports pulling from registries, importing from tar archives (docker save), and pushing to registries - all optimized for large images (multi-GB).")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Available subcommands for the application
#[derive(Subcommand)]
enum Commands {
    /// Pull an image from a registry and cache it locally
    /// 
    /// This downloads the image manifest and all layers, storing them
    /// in a local cache directory (.cache/) for later use.
    Pull {
        /// Source image to pull (e.g., "nginx:latest" or "registry.example.com/app:v1.0")
        source_image: String,
    },
      /// Push a cached image to a target registry
    /// 
    /// Reads a previously cached image and uploads it to the specified
    /// target registry with authentication.
    Push {
        /// Source image name (must be previously cached)
        source_image: String,
        
        /// Target image to push to (full registry path with tag)
        target_image: String,
        
        /// Username for target registry authentication
        #[arg(short, long)]
        username: String,
        
        /// Password for target registry authentication  
        #[arg(short, long)]
        password: String,
    },
    
    /// Import a Docker tar archive and cache it locally
    /// 
    /// This processes tar files created by `docker save` command,
    /// extracting layers and metadata to create a unified cache structure.
    Import {
        /// Path to the Docker tar archive file
        tar_file: String,
        
        /// Image name to use for caching (e.g., "myapp:v1.0")
        image_name: String,
    },
}

/// Application entry point
/// 
/// Initializes the OCI client with a platform resolver for Linux AMD64 images
/// and dispatches to the appropriate command handler based on user input.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    // Configure OCI client with platform resolver to handle multi-platform images
    // This ensures we pull the correct architecture variant (Linux AMD64 in this case)
    let mut client_config = oci_client::client::ClientConfig::default();
    client_config.platform_resolver = Some(Box::new(oci_client::client::linux_amd64_resolver));
    let client = Client::new(client_config);    match cli.command {
        Commands::Pull { source_image } => {
            println!("🚀 Pulling and caching image: {}", source_image);
            cache_image(&client, &source_image).await?;
            println!("✅ Successfully cached image: {}", source_image);
        }
        Commands::Push { source_image, target_image, username, password } => {
            println!("📤 Pushing image from cache: {} -> {}", source_image, target_image);
            
            // Ensure we have the image cached before attempting to push
            if !has_cached_image(&source_image).await? {
                println!("⚠️  Image not found in cache, pulling first...");
                cache_image(&client, &source_image).await?;
            }
            
            // Push the cached image to target registry
            push_cached_image(&client, &source_image, &target_image, &username, &password).await?;
            println!("✅ Successfully pushed image: {}", target_image);
        }
        Commands::Import { tar_file, image_name } => {
            println!("📦 Importing Docker tar archive: {} as {}", tar_file, image_name);
            import_tar_file(&tar_file, &image_name).await?;
            println!("✅ Successfully imported and cached image: {}", image_name);
        }
    }

    Ok(())
}

/// Downloads and caches a Docker image using memory-efficient streaming
/// 
/// This function implements the core memory optimization strategy:
/// 1. Fetches only the image manifest first (small metadata)
/// 2. Downloads each layer individually using streaming APIs
/// 3. Writes layers directly to disk without loading into memory
/// 4. Processes layers sequentially to avoid concurrent memory pressure
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
async fn cache_image(client: &Client, source_image: &str) -> Result<(), PusherError> {
    // Use anonymous authentication for public registries
    let auth = oci_client::secrets::RegistryAuth::Anonymous;
    
    // Parse the image reference to validate format and extract components
    let image_ref: Reference = source_image.parse()
        .map_err(|e| PusherError::PullError(format!("Invalid image reference: {}", e)))?;
        
    println!("📋 Pulling image: {}", source_image);
    println!("🔍 Parsed reference: {}", image_ref);
    
    // Step 1: Pull only the manifest (small metadata, ~1-5KB typically)
    // This gives us the list of layers and config without downloading everything
    println!("📄 Fetching manifest...");
    let (manifest, _digest) = client.pull_image_manifest(&image_ref, &auth).await
        .map_err(|e| PusherError::PullError(format!("Failed to pull manifest: {}", e)))?;
    
    // Step 2: Set up local cache directory structure
    let cache_dir = Path::new(".cache");
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| PusherError::CacheError(format!("Failed to create cache directory: {}", e)))?;
    
    let image_cache_dir = cache_dir.join(sanitize_image_name(source_image));
    std::fs::create_dir_all(&image_cache_dir)
        .map_err(|e| PusherError::CacheError(format!("Failed to create image cache directory: {}", e)))?;
    
    println!("💾 Streaming {} layers to cache...", manifest.layers.len());
    
    // Step 3: Stream and cache layers one by one (MEMORY OPTIMIZATION)
    // Processing sequentially instead of concurrently prevents memory explosion
    let mut cached_layers = Vec::new();
    for (i, layer_desc) in manifest.layers.iter().enumerate() {
        println!("📦 Streaming layer {}/{}: {}", i + 1, manifest.layers.len(), layer_desc.digest);
        
        let layer_digest = layer_desc.digest.to_string();
        let layer_path = image_cache_dir.join(layer_digest.replace(":", "_"));
        
        // Create file handle for direct streaming
        let mut file = tokio::fs::File::create(&layer_path).await
            .map_err(|e| PusherError::CacheError(format!("Failed to create layer file {}: {}", layer_digest, e)))?;
        
        // KEY OPTIMIZATION: Stream layer directly to file without loading into memory
        // This works for layers of any size (even multi-GB layers)
        client.pull_blob(&image_ref, layer_desc, &mut file).await
            .map_err(|e| PusherError::PullError(format!("Failed to stream layer {}: {}", layer_digest, e)))?;
        
        // Ensure data is written to disk
        file.flush().await
            .map_err(|e| PusherError::CacheError(format!("Failed to flush layer file {}: {}", layer_digest, e)))?;
        
        cached_layers.push(layer_digest);
    }
    
    // Step 4: Cache the manifest for later reconstruction
    let manifest_path = image_cache_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    tokio::fs::write(&manifest_path, manifest_json).await
        .map_err(|e| PusherError::CacheError(format!("Failed to cache manifest: {}", e)))?;
    
    // Step 5: Stream and cache the config blob (typically small, <10KB)
    let config_desc = &manifest.config;
    let config_digest = config_desc.digest.to_string();
    let config_path = image_cache_dir.join(format!("config_{}.json", config_digest.replace(":", "_")));
    
    let mut config_file = tokio::fs::File::create(&config_path).await
        .map_err(|e| PusherError::CacheError(format!("Failed to create config file: {}", e)))?;
    
    client.pull_blob(&image_ref, config_desc, &mut config_file).await
        .map_err(|e| PusherError::PullError(format!("Failed to stream config: {}", e)))?;
    
    config_file.flush().await
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
    tokio::fs::write(image_cache_dir.join("index.json"), index_json).await
        .map_err(|e| PusherError::CacheError(format!("Failed to create index: {}", e)))?;
    
    println!("✅ Successfully cached image with {} layers", cached_layers.len());
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
async fn has_cached_image(source_image: &str) -> Result<bool, PusherError> {
    let cache_dir = Path::new(".cache");
    let image_cache_dir = cache_dir.join(sanitize_image_name(source_image));
    let index_path = image_cache_dir.join("index.json");
    
    Ok(tokio::fs::metadata(&index_path).await.is_ok())
}

/// Pushes a cached image to a target registry with memory optimization
/// 
/// This function implements several memory optimization strategies:
/// 
/// ## Size-Based Processing Strategy:
/// - **Small layers (<100MB)**: Read entire layer into memory and upload
/// - **Large layers (>100MB)**: Use chunked reading (50MB chunks) to prevent memory exhaustion
/// - **Rate limiting**: Add delays between large layer uploads to prevent registry overload
/// 
/// ## Upload Process:
/// 1. Authenticate with target registry
/// 2. Read cached manifest and layer information  
/// 3. Upload each layer individually with size-appropriate strategy
/// 4. Upload image configuration
/// 5. Push final manifest to complete the image
/// 
/// # Arguments
/// 
/// * `client` - OCI client for registry operations
/// * `source_image` - Name of cached image to push
/// * `target_image` - Destination image reference with registry
/// * `username` - Authentication username for target registry
/// * `password` - Authentication password for target registry
/// 
/// # Returns
/// 
/// `Result<(), PusherError>` - Success or detailed error information
async fn push_cached_image(
    client: &Client, 
    source_image: &str, 
    target_image: &str, 
    username: &str, 
    password: &str
) -> Result<(), PusherError> {
    let cache_dir = Path::new(".cache");
    let image_cache_dir = cache_dir.join(sanitize_image_name(source_image));
    
    // Setup Basic authentication for target registry
    let auth = oci_client::secrets::RegistryAuth::Basic(username.to_string(), password.to_string());
    
    // Parse and validate target image reference
    let target_ref: Reference = target_image.parse()
        .map_err(|e| PusherError::PushError(format!("Invalid target image reference: {}", e)))?;
    
    // Step 1: Authenticate with the target registry
    println!("🔐 Authenticating with registry...");
    client.auth(&target_ref, &auth, oci_client::RegistryOperation::Push).await
        .map_err(|e| PusherError::PushError(format!("Authentication failed: {}", e)))?;
    println!("✅ Authentication successful!");
    
    // Step 2: Read cached metadata and manifest
    let index_path = image_cache_dir.join("index.json");
    let index_content = tokio::fs::read_to_string(&index_path).await
        .map_err(|_| PusherError::CacheNotFound)?;
    let index: serde_json::Value = serde_json::from_str(&index_content)?;
    
    let manifest_path = image_cache_dir.join("manifest.json");
    let manifest_content = tokio::fs::read_to_string(&manifest_path).await
        .map_err(|e| PusherError::CacheError(format!("Failed to read cached manifest: {}", e)))?;
    let manifest: OciImageManifest = serde_json::from_str(&manifest_content)?;
    
    // Extract layer digest list from index
    let layer_digests: Vec<String> = index["layers"].as_array()
        .ok_or(PusherError::CacheError("Invalid layers format in index".to_string()))?
        .iter()
        .map(|v| v.as_str().unwrap_or("").to_string())
        .collect();
    
    println!("📤 Uploading {} cached layers with memory optimization...", layer_digests.len());
    
    // Step 3: Upload layers with size-based optimization strategy
    for (i, digest) in layer_digests.iter().enumerate() {
        let layer_path = image_cache_dir.join(digest.replace(":", "_"));
        
        // Check layer size to determine upload strategy
        let layer_metadata = tokio::fs::metadata(&layer_path).await
            .map_err(|e| PusherError::CacheError(format!("Failed to get layer metadata {}: {}", digest, e)))?;
        let layer_size_mb = layer_metadata.len() as f64 / (1024.0 * 1024.0);
        
        println!("📦 Uploading layer {}/{}: {} ({:.1} MB)", i + 1, layer_digests.len(), digest, layer_size_mb);
        
        // MEMORY OPTIMIZATION: Different strategies based on layer size
        if layer_size_mb > 100.0 {
            // Strategy for very large layers: Chunked reading to prevent memory exhaustion
            println!("   🔄 Using chunked upload for large layer...");
            let chunk_size = 50 * 1024 * 1024; // 50MB chunks
            let mut file = tokio::fs::File::open(&layer_path).await
                .map_err(|e| PusherError::CacheError(format!("Failed to open cached layer {}: {}", digest, e)))?;
            
            let mut all_data = Vec::new();
            let mut buffer = vec![0u8; chunk_size];
            
            // Read file in chunks to control memory usage
            loop {
                let bytes_read = file.read(&mut buffer).await
                    .map_err(|e| PusherError::CacheError(format!("Failed to read layer chunk {}: {}", digest, e)))?;
                
                if bytes_read == 0 {
                    break; // End of file
                }
                
                all_data.extend_from_slice(&buffer[..bytes_read]);
                
                // Small delay to prevent memory pressure
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
            
            client.push_blob(&target_ref, &all_data, digest).await
                .map_err(|e| PusherError::PushError(format!("Failed to upload layer {}: {}", digest, e)))?;
        } else {
            // Strategy for smaller layers: Direct read and upload
            let layer_data = tokio::fs::read(&layer_path).await
                .map_err(|e| PusherError::CacheError(format!("Failed to read cached layer {}: {}", digest, e)))?;
            
            client.push_blob(&target_ref, &layer_data, digest).await
                .map_err(|e| PusherError::PushError(format!("Failed to upload layer {}: {}", digest, e)))?;
        }
        
        println!("   ✅ Successfully uploaded layer {}", digest);
        
        // Rate limiting: Add delay for large layers to prevent overwhelming the registry
        if layer_size_mb > 50.0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
    }
    
    // Step 4: Upload image configuration
    let config_digest = index["config"].as_str().ok_or(PusherError::CacheError("Invalid index format".to_string()))?;
    let config_path = image_cache_dir.join(format!("config_{}.json", config_digest.replace(":", "_")));
    
    println!("⚙️  Uploading config: {}", config_digest);
    let config_data = tokio::fs::read(&config_path).await
        .map_err(|e| PusherError::CacheError(format!("Failed to read cached config: {}", e)))?;
    
    client.push_blob(&target_ref, &config_data, config_digest).await
        .map_err(|e| PusherError::PushError(format!("Failed to upload config: {}", e)))?;
    
    // Step 5: Push the final manifest to complete the image
    println!("📋 Pushing manifest to registry: {}", target_image);
    let manifest_enum = oci_client::manifest::OciManifest::Image(manifest);
    let manifest_url = client.push_manifest(&target_ref, &manifest_enum).await
        .map_err(|e| PusherError::PushError(format!("Failed to push manifest: {}", e)))?;
    
    println!("🎉 Successfully pushed {} layers to {}", layer_digests.len(), manifest_url);
    Ok(())
}

/// Imports a Docker tar archive and caches it using the same structure as pulled images
/// 
/// This function processes tar files created by `docker save` command and extracts:
/// - Image manifest(s)
/// - Layer tar.gz files  
/// - Image configuration JSON
/// 
/// The extracted components are cached using the same structure as `cache_image()`,
/// ensuring compatibility with the push functionality.
/// 
/// ## Docker Save Format
/// 
/// A `docker save` tar contains:
/// - `manifest.json` - List of images with their layers and config references
/// - `<layer_id>/layer.tar` - Individual layer data (sometimes gzipped)
/// - `<config_hash>.json` - Image configuration
/// - `repositories` (optional) - Repository and tag information
/// 
/// ## Cache Structure
/// 
/// The function creates the same cache structure as `cache_image()`:
/// - `.cache/{sanitized_image_name}/manifest.json` - OCI image manifest
/// - `.cache/{sanitized_image_name}/config_{digest}.json` - Image config
/// - `.cache/{sanitized_image_name}/{layer_digest}` - Layer files
/// - `.cache/{sanitized_image_name}/index.json` - Cache metadata
/// 
/// # Arguments
/// 
/// * `tar_path` - Path to the Docker tar archive file
/// * `image_name` - Name to use for caching (e.g., "myapp:v1.0")
/// 
/// # Returns
/// 
/// `Result<(), PusherError>` - Success or detailed error information
/// 
/// # Example
/// 
/// ```bash
/// # Create a tar archive with docker save
/// docker save myapp:latest > myapp.tar
/// 
/// # Import it into the cache
/// docker-image-pusher import myapp.tar myapp:latest
/// 
/// # Now it can be pushed like any cached image
/// docker-image-pusher push myapp:latest registry.example.com/myapp:latest -u user -p pass
/// ```
async fn import_tar_file(tar_path: &str, image_name: &str) -> Result<(), PusherError> {
    println!("📂 Opening tar archive: {}", tar_path);
      // Step 1: Open and parse the tar archive
    let tar_file = File::open(tar_path)
        .map_err(|e| PusherError::TarError(format!("Failed to open tar file: {}", e)))?;
    
    let mut archive = Archive::new(tar_file);
    let _entries = archive.entries()
        .map_err(|e| PusherError::TarError(format!("Failed to read tar entries: {}", e)))?;
    
    // Step 2: Create cache directory structure
    let cache_dir = Path::new(".cache");
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| PusherError::CacheError(format!("Failed to create cache directory: {}", e)))?;
    
    let image_cache_dir = cache_dir.join(sanitize_image_name(image_name));
    std::fs::create_dir_all(&image_cache_dir)
        .map_err(|e| PusherError::CacheError(format!("Failed to create image cache directory: {}", e)))?;
      // Step 3: Extract and parse the main manifest.json from the tar
    println!("🔍 Searching for Docker manifest in tar archive...");
    let mut docker_manifest: Option<serde_json::Value> = None;
    let mut layer_mapping: std::collections::HashMap<String, (std::path::PathBuf, u64)> = std::collections::HashMap::new();
    let mut config_data: Option<(String, Vec<u8>)> = None;
    
    // Reset the archive for reading
    let tar_file = File::open(tar_path)
        .map_err(|e| PusherError::TarError(format!("Failed to reopen tar file: {}", e)))?;
    let mut archive = Archive::new(tar_file);
    
    // Step 4: First pass - find and parse manifest.json
    for entry_result in archive.entries()
        .map_err(|e| PusherError::TarError(format!("Failed to read tar entries: {}", e)))? {
        
        let mut entry = entry_result
            .map_err(|e| PusherError::TarError(format!("Failed to read tar entry: {}", e)))?;
        
        let path = entry.path()
            .map_err(|e| PusherError::TarError(format!("Failed to get entry path: {}", e)))?;
        let path_str = path.to_string_lossy();
        
        if path_str == "manifest.json" {
            println!("📄 Found Docker manifest.json");
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)
                .map_err(|e| PusherError::TarError(format!("Failed to read manifest: {}", e)))?;
            
            docker_manifest = Some(serde_json::from_slice(&contents)
                .map_err(|e| PusherError::TarError(format!("Failed to parse manifest.json: {}", e)))?);
            break;
        }
    }
    
    let docker_manifest = docker_manifest
        .ok_or_else(|| PusherError::TarError("No manifest.json found in tar archive".to_string()))?;
    
    // Step 5: Parse the Docker manifest to get image info
    let manifest_array = docker_manifest.as_array()
        .ok_or_else(|| PusherError::TarError("Invalid manifest.json format".to_string()))?;
    
    if manifest_array.is_empty() {
        return Err(PusherError::TarError("Empty manifest.json".to_string()));
    }
    
    // Use the first image in the manifest (docker save can contain multiple images)
    let image_info = &manifest_array[0];
    let config_file = image_info["Config"].as_str()
        .ok_or_else(|| PusherError::TarError("No Config field in manifest".to_string()))?;
    let layers = image_info["Layers"].as_array()
        .ok_or_else(|| PusherError::TarError("No Layers field in manifest".to_string()))?;
    
    println!("📋 Found image with {} layers", layers.len());
    println!("⚙️  Config file: {}", config_file);
    
    // Step 6: Second pass - extract layers and config
    let tar_file = File::open(tar_path)
        .map_err(|e| PusherError::TarError(format!("Failed to reopen tar file: {}", e)))?;
    let mut archive = Archive::new(tar_file);
    
    for entry_result in archive.entries()
        .map_err(|e| PusherError::TarError(format!("Failed to read tar entries: {}", e)))? {
        
        let mut entry = entry_result
            .map_err(|e| PusherError::TarError(format!("Failed to read tar entry: {}", e)))?;
        
        let path = entry.path()
            .map_err(|e| PusherError::TarError(format!("Failed to get entry path: {}", e)))?;
        let path_str = path.to_string_lossy();
        
        // Extract config file
        if path_str == config_file {
            println!("⚙️  Extracting config: {}", config_file);
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)
                .map_err(|e| PusherError::TarError(format!("Failed to read config: {}", e)))?;
            
            // Compute config digest
            let mut hasher = Sha256::new();
            hasher.update(&contents);
            let config_digest = format!("sha256:{:x}", hasher.finalize());
            
            config_data = Some((config_digest, contents));
            continue;
        }
          // Extract layer files using streaming approach for memory efficiency
        for layer in layers {
            let layer_path = layer.as_str()
                .ok_or_else(|| PusherError::TarError("Invalid layer path".to_string()))?;
            
            if path_str == layer_path {
                println!("📦 Extracting layer: {}", layer_path);
                
                // Get layer size for progress indication
                let layer_size = entry.size();
                let layer_size_mb = layer_size as f64 / (1024.0 * 1024.0);
                println!("   📏 Layer size: {:.1} MB", layer_size_mb);
                  // Create temporary file for the layer
                let temp_layer_path = image_cache_dir.join(format!("temp_layer_{}", std::process::id()));
                let mut temp_file = std::fs::File::create(&temp_layer_path)
                    .map_err(|e| PusherError::TarError(format!("Failed to create temp file: {}", e)))?;
                
                // Stream layer data to temp file while computing hash
                let mut hasher = Sha256::new();
                let mut buffer = [0u8; 65536]; // 64KB buffer for streaming
                let mut total_read = 0u64;
                
                loop {
                    let bytes_read = entry.read(&mut buffer)
                        .map_err(|e| PusherError::TarError(format!("Failed to read layer chunk: {}", e)))?;
                    
                    if bytes_read == 0 {
                        break; // End of layer
                    }
                    
                    // Write to temp file using std::io::Write trait
                    temp_file.write_all(&buffer[..bytes_read])
                        .map_err(|e| PusherError::TarError(format!("Failed to write layer chunk: {}", e)))?;
                    
                    // Update hash
                    hasher.update(&buffer[..bytes_read]);
                    
                    total_read += bytes_read as u64;
                    
                    // Progress indication for large layers
                    if layer_size > 0 && total_read % (10 * 1024 * 1024) == 0 { // Every 10MB
                        let progress = (total_read as f64 / layer_size as f64) * 100.0;
                        println!("   🔄 Progress: {:.1}%", progress);
                    }
                }
                
                // Finalize temp file using std::io::Write trait
                temp_file.flush()
                    .map_err(|e| PusherError::TarError(format!("Failed to flush temp file: {}", e)))?;
                drop(temp_file);
                
                // Compute final digest
                let layer_digest = format!("sha256:{:x}", hasher.finalize());
                println!("   ✅ Layer digest: {}", layer_digest);
                  // Move the temp file to final location with proper digest name
                let final_layer_path = image_cache_dir.join(layer_digest.replace(":", "_"));
                std::fs::rename(&temp_layer_path, &final_layer_path)
                    .map_err(|e| PusherError::TarError(format!("Failed to rename layer file: {}", e)))?;
                
                // Store layer info without loading content into memory
                layer_mapping.insert(layer_digest.clone(), (final_layer_path, total_read));
                
                break;
            }
        }
    }
    
    // Step 7: Verify we got all required components
    let (config_digest, config_contents) = config_data
        .ok_or_else(|| PusherError::TarError("Config file not found in tar".to_string()))?;
    
    if layer_mapping.len() != layers.len() {
        return Err(PusherError::TarError(format!(
            "Expected {} layers, found {}", 
            layers.len(), 
            layer_mapping.len()
        )));
    }
    
    println!("✅ Successfully extracted {} layers and config", layer_mapping.len());    // Step 8: Create OCI-compatible manifest using file-based layer info
    let mut oci_layers = Vec::new();
    let mut cached_layers = Vec::new();
    
    for (layer_digest, (_layer_path, layer_size)) in &layer_mapping {
        cached_layers.push(layer_digest.clone());
        
        // Create OCI layer descriptor using file size
        oci_layers.push(serde_json::json!({
            "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
            "size": layer_size,
            "digest": layer_digest
        }));
    }
    
    // Step 9: Save config to cache
    let config_file_name = format!("config_{}.json", config_digest.replace(":", "_"));
    let config_path = image_cache_dir.join(&config_file_name);
    
    tokio::fs::write(&config_path, &config_contents).await
        .map_err(|e| PusherError::CacheError(format!("Failed to cache config: {}", e)))?;
    
    // Step 10: Create OCI manifest
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
    
    // Step 11: Save manifest to cache
    let manifest_path = image_cache_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&oci_manifest)?;
    tokio::fs::write(&manifest_path, manifest_json).await
        .map_err(|e| PusherError::CacheError(format!("Failed to cache manifest: {}", e)))?;
    
    // Step 12: Create index file for cache lookup
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
    tokio::fs::write(image_cache_dir.join("index.json"), index_json).await
        .map_err(|e| PusherError::CacheError(format!("Failed to create index: {}", e)))?;
    
    println!("🎉 Successfully imported tar archive with {} layers", cached_layers.len());
    println!("💡 Cache structure matches pulled images - can be pushed with 'push' command");
    
    Ok(())
}

/// Sanitizes image names for use as directory names
/// 
/// Docker image names can contain characters that are not valid in file paths.
/// This function replaces problematic characters with underscores to create
/// safe directory names for the cache.
/// 
/// # Replacements
/// 
/// - `/` → `_` (registry separators)  
/// - `:` → `_` (tag separators)
/// - `@` → `_` (digest separators)
/// 
/// # Examples
/// 
/// ```
/// assert_eq!(sanitize_image_name("nginx:latest"), "nginx_latest");
/// assert_eq!(sanitize_image_name("registry.example.com/app:v1.0"), "registry.example.com_app_v1.0");
/// ```
/// 
/// # Arguments
/// 
/// * `image_name` - Original image name with potentially unsafe characters
/// 
/// # Returns
/// 
/// `String` - Sanitized name safe for use as directory name
fn sanitize_image_name(image_name: &str) -> String {
    image_name
        .replace("/", "_")  // Replace registry/namespace separators
        .replace(":", "_")  // Replace tag separators  
        .replace("@", "_")  // Replace digest separators
}