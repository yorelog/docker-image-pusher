//! Basic Usage Demo - Corresponds to README Quick Start examples
//!
//! This example demonstrates basic usage patterns documented in the README:
//! 1. Extract tar file and cache locally
//! 2. Push from cache to registry
//! 3. Pull from registry and cache
//! 4. Complete pull-to-push workflow
//!
//! Usage:
//! ```bash
//! # Set environment variables
//! export REGISTRY_USERNAME=your_username
//! export REGISTRY_PASSWORD=your_password
//! export TARGET_REGISTRY=registry.example.com
//! 
//! # Run the demo
//! cargo run --example basic_usage_demo
//! ```

use docker_image_pusher::{
    error::Result,
    image::image_manager::ImageManager,
    registry::RegistryClientBuilder,
    logging::Logger,
};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Docker Image Pusher - Basic Usage Demo");
    println!("==========================================");
    println!("This demo corresponds to README Quick Start examples");
    println!();

    // Initialize logger for verbose output
    let logger = Logger::new(true);
    
    // Configuration from environment
    let registry_username = env::var("REGISTRY_USERNAME").unwrap_or_else(|_| {
        println!("⚠️  REGISTRY_USERNAME not set, using demo credentials");
        "demo_user".to_string()
    });
    
    let registry_password = env::var("REGISTRY_PASSWORD").unwrap_or_else(|_| {
        println!("⚠️  REGISTRY_PASSWORD not set, using demo credentials");
        "demo_pass".to_string()
    });
    
    let target_registry = env::var("TARGET_REGISTRY").unwrap_or_else(|_| {
        "registry.example.com".to_string()
    });

    let cache_dir = ".cache_basic_demo";
    
    // Clean up previous demo
    let _ = std::fs::remove_dir_all(cache_dir);
    
    logger.section("Demo 1: Basic Extract and Push Workflow");
    println!("📝 Scenario: Extract tar file → Cache → Push to registry");
    println!("📄 Command equivalent:");
    println!("   docker-image-pusher extract --file image.tar --verbose");
    println!("   docker-image-pusher push --source image:tag --target {}/app:v1.0", target_registry);
    println!();
    
    // Demo the basic workflow
    demo_extract_and_push(&logger, cache_dir, &target_registry, &registry_username, &registry_password).await?;
    
    logger.section("Demo 2: Pull and Cache Workflow");
    println!("📝 Scenario: Pull from registry → Cache locally");
    println!("📄 Command equivalent:");
    println!("   docker-image-pusher pull --image nginx:latest --verbose");
    println!();
    
    demo_pull_and_cache(&logger, cache_dir, &registry_username, &registry_password).await?;
    
    logger.section("Demo 3: Complete Pull-to-Push Migration");
    println!("📝 Scenario: Pull from source → Cache → Push to target");
    println!("📄 Command equivalent:");
    println!("   docker-image-pusher pull --image alpine:latest");
    println!("   docker-image-pusher push --source alpine:latest --target {}/alpine:migrated", target_registry);
    println!();
    
    demo_complete_migration(&logger, cache_dir, &target_registry, &registry_username, &registry_password).await?;
    
    println!("✅ Basic usage demo completed successfully!");
    println!("📚 These examples correspond to the README Quick Start section");
    
    Ok(())
}

async fn demo_extract_and_push(
    logger: &Logger,
    _cache_dir: &str,
    target_registry: &str,
    username: &str,
    _password: &str,
) -> Result<()> {
    logger.info("Creating demo tar file (simulated)...");
    
    // In a real scenario, you would have an actual tar file
    // For demo purposes, we'll simulate the operation
    println!("📦 Would extract tar file and cache locally");
    println!("🚀 Would push cached image to: {}/project/app:v1.0", target_registry);
    
    // Create registry client for demonstration
    let registry_url = format!("https://{}", target_registry);
    let _client = RegistryClientBuilder::new(registry_url)
        .with_verbose(true)
        .build()?;
    
    logger.info(&format!("Registry client created for: {}", target_registry));
    logger.info(&format!("Would authenticate with username: {}", username));
    
    // Note: In production, you would:
    // 1. Use ExtractAndCache operation mode with actual tar file
    // 2. Use PushFromCacheUsingManifest operation mode
    
    Ok(())
}

async fn demo_pull_and_cache(
    logger: &Logger,
    cache_dir: &str,
    _username: &str,
    _password: &str,
) -> Result<()> {
    logger.info("Demonstrating pull and cache workflow...");
    
    // Create image manager
    let mut _image_manager = ImageManager::new(Some(cache_dir), true)?;
    
    println!("🔽 Would pull nginx:latest from Docker Hub");
    println!("💾 Would cache image locally in: {}", cache_dir);
    
    // In production, you would use:
    // let operation = OperationMode::PullAndCache {
    //     registry_url: "https://registry-1.docker.io".to_string(),
    //     repository: "library/nginx".to_string(),
    //     reference: "latest".to_string(),
    //     cache_dir: cache_dir.to_string(),
    //     auth_config: Some(auth_config),
    // };
    
    logger.info("Pull and cache operation configured (demo mode)");
    
    Ok(())
}

async fn demo_complete_migration(
    logger: &Logger,
    _cache_dir: &str,
    target_registry: &str,
    _username: &str,
    _password: &str,
) -> Result<()> {
    logger.info("Demonstrating complete migration workflow...");
    
    println!("🔄 Complete migration workflow:");
    println!("   1. Pull alpine:latest from Docker Hub");
    println!("   2. Cache locally");
    println!("   3. Push to target registry: {}/alpine:migrated", target_registry);
    
    // This would involve:
    // 1. PullAndCache operation
    // 2. PushFromCacheUsingManifest operation
    
    logger.info("Migration workflow completed (demo mode)");
    
    Ok(())
}
