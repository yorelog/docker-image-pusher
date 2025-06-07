// Example demonstrating the optimized upload integration

use docker_image_pusher::cli::operation_mode::OperationMode;
use docker_image_pusher::error::Result;
use docker_image_pusher::image::ImageManager;
use docker_image_pusher::registry::PipelineConfig;
use docker_image_pusher::registry::RegistryClientBuilder;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Docker Image Pusher - Optimized Upload Demo");

    // Create image manager with optimizations enabled (default)
    let mut manager = ImageManager::new(None, true)?;

    // Configure pipeline for demonstration
    let config = PipelineConfig {
        max_concurrent: 8,
        buffer_size: 1024,
        small_blob_threshold: 10 * 1024 * 1024,   // 10MB
        medium_blob_threshold: 100 * 1024 * 1024, // 100MB
        large_blob_threshold: 500 * 1024 * 1024,  // 500MB
        timeout_seconds: 7200,
        retry_attempts: 3,
        memory_limit_mb: 512,
        enable_compression: true,
        enable_streaming: true,
    };
    manager.configure_pipeline(config);

    // Verify configuration
    let (optimized, pipeline_config) = manager.get_config();
    println!("Optimized mode: {}", optimized);
    println!("Pipeline config: {:?}", pipeline_config);

    // Example registry client (would need real registry URL)
    let client = RegistryClientBuilder::new("https://registry.example.com".to_string())
        .with_verbose(true)
        .build()?;

    // Test connectivity (this would fail with example URL)
    println!("Testing registry connectivity...");
    match client.test_connectivity().await {
        Ok(_) => println!("✓ Registry connectivity successful"),
        Err(e) => println!(
            "✗ Registry connectivity failed: {} (expected with example URL)",
            e
        ),
    }

    // Example operation mode for pushing from tar
    let mode = OperationMode::PushFromTar {
        tar_file: "example-image.tar".to_string(),
        repository: "myapp".to_string(),
        reference: "latest".to_string(),
    };

    println!("Operation mode: {}", mode.description());

    // In a real scenario, you would call:
    // manager.execute_operation(&mode, Some(&client), None).await?;

    println!("Demo completed successfully!");
    println!("\nKey benefits of optimized mode:");
    println!("• Priority-based upload scheduling (small blobs first)");
    println!("• Streaming TAR processing with parallel uploads");
    println!("• Memory-efficient processing of large files");
    println!("• Configurable pipeline parameters");

    Ok(())
}
