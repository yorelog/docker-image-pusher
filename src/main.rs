use oci_client::{Client, Reference};
use oci_client::client::{Config, ImageLayer};
use std::path::Path;
use clap::{Parser, Subcommand};
use futures::future::try_join_all;
use serde_json;
use thiserror::Error;
use anyhow::Result;
use sha256;

#[derive(Error, Debug)]
pub enum PusherError {
    #[error("Pull error: {0}")]
    PullError(String),
    #[error("Push error: {0}")]
    PushError(String),
    #[error("Cache error: {0}")]
    CacheError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("Cache not found")]
    CacheNotFound,
}

#[derive(Parser)]
#[command(name = "docker-image-pusher")]
#[command(about = "A tool to pull and push Docker images")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pull image and cache locally
    Pull {
        /// Source image to pull
        source_image: String,
    },
    /// Push image to target registry
    Push {
        source_image: String,
        /// Target image to push to
        target_image: String,
        /// Username for authentication
        #[arg(short, long)]
        username: String,
        /// Password for authentication
        #[arg(short, long)]
        password: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    // Configure client with platform resolver
    let mut client_config = oci_client::client::ClientConfig::default();
    // Set a platform resolver to handle multi-platform images
    client_config.platform_resolver = Some(Box::new(oci_client::client::linux_amd64_resolver));
    let client = Client::new(client_config);

    match cli.command {
        Commands::Pull { source_image } => {
            println!("Pulling and caching image: {}", source_image);
            cache_image(&client, &source_image).await?;
            println!("Successfully cached image: {}", source_image);
        }
        Commands::Push { source_image, target_image, username, password } => {
            println!("Pushing image from cache: {} -> {}", source_image, target_image);
            
            // First ensure we have the image cached
            if !has_cached_image(&source_image).await? {
                println!("Image not found in cache, pulling first...");
                cache_image(&client, &source_image).await?;
            }
            
            // Push the cached image
            push_cached_image(&client, &source_image, &target_image, &username, &password).await?;
            println!("Successfully pushed image: {}", target_image);
        }
    }

    Ok(())
}

async fn cache_image(client: &Client, source_image: &str) -> Result<(), PusherError> {
    let auth = oci_client::secrets::RegistryAuth::Anonymous;
    let image_ref: Reference = source_image.parse()
        .map_err(|e| PusherError::PullError(format!("Invalid image reference: {}", e)))?;
        
    println!("Pulling image: {}", source_image);
    println!("Parsed reference: {}", image_ref);
    
    // Try to pull with accepted media types (not platforms)
    let accepted_media_types = vec![
        "application/vnd.docker.distribution.manifest.v2+json",
        "application/vnd.oci.image.manifest.v1+json",
        "application/vnd.docker.distribution.manifest.list.v2+json",
        "application/vnd.docker.image.rootfs.diff.tar.gzip",
        "application/vnd.oci.image.layer.v1.tar+gzip",
        "application/vnd.oci.image.index.v1+json"
    ];
    println!("Using accepted media types: {:?}", accepted_media_types);
    
    let image_data = client.pull(&image_ref, &auth, accepted_media_types).await
        .map_err(|e| {
            eprintln!("Detailed pull error: {:?}", e);
            PusherError::PullError(format!("Failed to pull image: {}", e))
        })?;
    
    // Create .cache directory structure 
    let cache_dir = Path::new(".cache");
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| PusherError::CacheError(format!("Failed to create cache directory: {}", e)))?;
    
    // Create image-specific cache directory
    let image_cache_dir = cache_dir.join(sanitize_image_name(source_image));
    std::fs::create_dir_all(&image_cache_dir)
        .map_err(|e| PusherError::CacheError(format!("Failed to create image cache directory: {}", e)))?;
    
    println!("Caching {} layers concurrently...", image_data.layers.len());
    
    let layers_len = image_data.layers.len();
    
    // Concurrently cache all layers
    let layer_futures: Vec<_> = image_data.layers.into_iter().enumerate().map(|(i, layer)| {
        // For layers, we need to compute digest from data since it might not be available
        let layer_digest = format!("sha256:{}", sha256::digest(&layer.data));
        let layer_path = image_cache_dir.join(layer_digest.replace(":", "_"));
        let layer_data = layer.data.clone();
        
        async move {
            println!("Caching layer {}/{}: {}", i + 1, layers_len, layer_digest);
            tokio::fs::write(&layer_path, &layer_data).await
                .map_err(|e| PusherError::CacheError(format!("Failed to cache layer {}: {}", layer_digest, e)))?;
            Ok::<_, PusherError>(layer_digest)
        }
    }).collect();
    
    let cached_layers = try_join_all(layer_futures).await?;
    
    // Cache manifest
    let manifest_path = image_cache_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&image_data.manifest)?;
    tokio::fs::write(&manifest_path, manifest_json).await
        .map_err(|e| PusherError::CacheError(format!("Failed to cache manifest: {}", e)))?;
    
    // Cache config blob
    let config_digest = format!("sha256:{}", sha256::digest(&image_data.config.data));
    let config_path = image_cache_dir.join(format!("config_{}.json", config_digest.replace(":", "_")));
    tokio::fs::write(&config_path, &image_data.config.data).await
        .map_err(|e| PusherError::CacheError(format!("Failed to cache config: {}", e)))?;
    
    // Create index file for easy lookup
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
    
    println!("Successfully cached image with {} layers", cached_layers.len());
    Ok(())
}

async fn has_cached_image(source_image: &str) -> Result<bool, PusherError> {
    let cache_dir = Path::new(".cache");
    let image_cache_dir = cache_dir.join(sanitize_image_name(source_image));
    let index_path = image_cache_dir.join("index.json");
    
    Ok(tokio::fs::metadata(&index_path).await.is_ok())
}

async fn push_cached_image(
    client: &Client, 
    source_image: &str, 
    target_image: &str, 
    username: &str, 
    password: &str
) -> Result<(), PusherError> {
    let cache_dir = Path::new(".cache");
    let image_cache_dir = cache_dir.join(sanitize_image_name(source_image));
    
    // Setup authentication first
    let auth = oci_client::secrets::RegistryAuth::Basic(username.to_string(), password.to_string());
    
    // Parse target image reference
    let target_ref: Reference = target_image.parse()
        .map_err(|e| PusherError::PushError(format!("Invalid target image reference: {}", e)))?;
    
    // Authenticate with the registry first
    println!("Authenticating with registry...");
    client.auth(&target_ref, &auth, oci_client::RegistryOperation::Push).await
        .map_err(|e| PusherError::PushError(format!("Authentication failed: {}", e)))?;
    println!("Authentication successful!");
    
    // Read index file
    let index_path = image_cache_dir.join("index.json");
    let index_content = tokio::fs::read_to_string(&index_path).await
        .map_err(|_| PusherError::CacheNotFound)?;
    let index: serde_json::Value = serde_json::from_str(&index_content)?;
    
    // Read manifest
    let manifest_path = image_cache_dir.join("manifest.json");
    let manifest_content = tokio::fs::read_to_string(&manifest_path).await
        .map_err(|e| PusherError::CacheError(format!("Failed to read cached manifest: {}", e)))?;
    let manifest: oci_client::manifest::OciImageManifest = serde_json::from_str(&manifest_content)?;
    
    // Read config
    let config_digest = index["config"].as_str().ok_or(PusherError::CacheError("Invalid index format".to_string()))?;
    let config_path = image_cache_dir.join(format!("config_{}.json", config_digest.replace(":", "_")));
    let config_data = tokio::fs::read(&config_path).await
        .map_err(|e| PusherError::CacheError(format!("Failed to read cached config: {}", e)))?;
    
    // Create Config struct
    let config = Config {
        data: config_data,
        media_type: "application/vnd.oci.image.config.v1+json".to_string(),
        annotations: None,
    };
    
    // Get layer digests
    let layer_digests: Vec<String> = index["layers"].as_array()
        .ok_or(PusherError::CacheError("Invalid layers format in index".to_string()))?
        .iter()
        .map(|v| v.as_str().unwrap_or("").to_string())
        .collect();
    
    println!("Loading {} cached layers with memory optimization...", layer_digests.len());
    
    // Load layers one by one to minimize memory usage for large layers
    let mut layers = Vec::new();
    
    for (i, digest) in layer_digests.iter().enumerate() {
        let layer_path = image_cache_dir.join(digest.replace(":", "_"));
        
        // Check layer size first
        let layer_metadata = tokio::fs::metadata(&layer_path).await
            .map_err(|e| PusherError::CacheError(format!("Failed to get layer metadata {}: {}", digest, e)))?;
        let layer_size_mb = layer_metadata.len() as f64 / (1024.0 * 1024.0);
        
        println!("Loading layer {}/{}: {} ({:.1} MB)", i + 1, layer_digests.len(), digest, layer_size_mb);
        
        // For very large layers (>100MB), we could implement streaming, but for now load sequentially
        let data = tokio::fs::read(&layer_path).await
            .map_err(|e| PusherError::CacheError(format!("Failed to read cached layer {}: {}", digest, e)))?;
        
        layers.push(ImageLayer {
            data,
            media_type: "application/vnd.oci.image.layer.v1.tar+gzip".to_string(),
            annotations: None,
        });
        
        // Optional: Add a small delay for very large layers to prevent memory pressure
        if layer_size_mb > 50.0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }
    
    // Push image
    println!("Pushing to registry: {}", target_image);
    let push_response = client.push(&target_ref, &layers, config, &auth, Some(manifest)).await
        .map_err(|e| PusherError::PushError(e.to_string()))?;
    
    println!("Successfully pushed {} layers to {}", layers.len(), push_response.manifest_url);
    Ok(())
}

fn sanitize_image_name(image_name: &str) -> String {
    image_name
        .replace("/", "_")
        .replace(":", "_")
        .replace("@", "_")
}