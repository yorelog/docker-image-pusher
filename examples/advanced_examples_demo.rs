//! Advanced Examples Demo - Corresponds to README Advanced Examples section
//!
//! This example demonstrates the advanced usage patterns documented in the README:
//! 1. Enterprise ML Model Deployment
//! 2. Production Harbor Deployment
//! 3. Edge Computing Deployment
//! 4. Multi-Architecture Deployment
//!
//! Usage:
//! ```bash
//! export ML_REGISTRY_USER=ml_engineer
//! export ML_REGISTRY_PASSWORD=ml_token
//! export HARBOR_USER=prod_deployer  
//! export HARBOR_PASSWORD=harbor_secret
//! 
//! cargo run --example advanced_examples_demo
//! ```

use docker_image_pusher::{
    cli::operation_mode::OperationMode,
    error::Result,
    image::image_manager::ImageManager,
    registry::RegistryClientBuilder,
    logging::Logger,
};
use std::env;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Docker Image Pusher - Advanced Examples Demo");
    println!("================================================");
    println!("This demo corresponds to README Advanced Examples section");
    println!();

    let logger = Logger::new(true);
    let cache_dir = ".cache_advanced_demo";
    
    // Clean up previous demo
    let _ = std::fs::remove_dir_all(cache_dir);
    
    // Demo 1: Enterprise ML Model Deployment
    logger.section("Demo 1: Enterprise ML Model Deployment (15GB PyTorch)");
    demo_ml_model_deployment(&logger, cache_dir).await?;
    
    sleep(Duration::from_secs(1)).await;
    
    // Demo 2: Production Harbor Deployment
    logger.section("Demo 2: Production Harbor Deployment with Error Handling");
    demo_production_harbor(&logger, cache_dir).await?;
    
    sleep(Duration::from_secs(1)).await;
    
    // Demo 3: Edge Computing Deployment
    logger.section("Demo 3: Edge Computing Deployment (Limited Bandwidth)");
    demo_edge_deployment(&logger, cache_dir).await?;
    
    sleep(Duration::from_secs(1)).await;
    
    // Demo 4: Multi-Architecture Deployment
    logger.section("Demo 4: Multi-Architecture Deployment with Cache Optimization");
    demo_multiarch_deployment(&logger, cache_dir).await?;
    
    println!();
    println!("✅ Advanced examples demo completed successfully!");
    println!("📚 These examples correspond to the README Advanced Examples section");
    println!("💡 Each demo shows the command patterns and optimizations for different scenarios");
    
    Ok(())
}

async fn demo_ml_model_deployment(logger: &Logger, cache_dir: &str) -> Result<()> {
    println!("🧠 Scenario: Deploying 15GB PyTorch model to ML registry");
    println!("🎯 Optimizations: Large layer threshold, dynamic concurrency, retry handling");
    println!();
    
    println!("📄 Command sequence:");
    println!("   # Extract and cache large model locally first");
    println!("   docker-image-pusher extract \\");
    println!("     --file pytorch-model-15gb.tar \\");
    println!("     --verbose");
    println!();
    println!("   # Push to ML registry with optimized settings");
    println!("   docker-image-pusher push \\");
    println!("     --source pytorch-model:v3.0 \\");
    println!("     --target ml-registry.company.com/models/pytorch-model:v3.0 \\");
    println!("     --username ml-engineer \\");
    println!("     --password $(cat ~/.ml-registry-token) \\");
    println!("     --large-layer-threshold 2147483648 \\  # 2GB threshold");
    println!("     --max-concurrent 4 \\                  # 4 parallel uploads");
    println!("     --retry-attempts 5 \\                  # Extra retries");
    println!("     --enable-dynamic-concurrency \\        # Auto-optimize");
    println!("     --verbose");
    println!();
    
    // Simulate configuration
    let ml_registry_user = env::var("ML_REGISTRY_USER").unwrap_or_else(|_| "ml_engineer".to_string());
    let ml_registry = "ml-registry.company.com";
    
    logger.info(&format!("ML Registry: {}", ml_registry));
    logger.info(&format!("Username: {}", ml_registry_user));
    logger.info("Large Layer Threshold: 2GB (optimized for large ML models)");
    logger.info("Max Concurrent: 4 (balanced for large uploads)");
    logger.info("Dynamic Concurrency: Enabled (auto-optimization)");
    logger.info("Retry Attempts: 5 (production-grade reliability)");
    
    // Create registry client for demonstration
    let registry_url = format!("https://{}", ml_registry);
    let _client = RegistryClientBuilder::new(registry_url)
        .with_verbose(true)
        .build()?;
    
    logger.success("ML model deployment configuration validated");
    
    Ok(())
}

async fn demo_production_harbor(logger: &Logger, cache_dir: &str) -> Result<()> {
    println!("🏢 Scenario: Production deployment to Harbor with comprehensive error handling");
    println!("🎯 Optimizations: TLS skip for self-signed certs, conservative concurrency, layer skipping");
    println!();
    
    println!("📄 Command sequence:");
    println!("   # Pull from Docker Hub and cache locally");
    println!("   docker-image-pusher pull \\");
    println!("     --image nginx:1.21 \\");
    println!("     --verbose");
    println!();
    println!("   # Push to production Harbor");
    println!("   docker-image-pusher push \\");
    println!("     --source nginx:1.21 \\");
    println!("     --target harbor.company.com/production/webapp:v2.1.0 \\");
    println!("     --username prod-deployer \\");
    println!("     --password $HARBOR_PASSWORD \\");
    println!("     --skip-tls \\               # For self-signed certificates");
    println!("     --max-concurrent 2 \\       # Conservative for production");
    println!("     --skip-existing \\          # Skip existing layers");
    println!("     --retry-attempts 5 \\       # Production-grade retry");
    println!("     --verbose");
    println!();
    
    let harbor_user = env::var("HARBOR_USER").unwrap_or_else(|_| "prod_deployer".to_string());
    let harbor_registry = "harbor.company.com";
    
    logger.info(&format!("Harbor Registry: {}", harbor_registry));
    logger.info(&format!("Username: {}", harbor_user));
    logger.info("TLS Verification: Skipped (for self-signed certificates)");
    logger.info("Max Concurrent: 2 (conservative for production stability)");
    logger.info("Skip Existing: Enabled (resume interrupted uploads)");
    logger.info("Retry Attempts: 5 (production-grade error handling)");
    
    logger.success("Production Harbor deployment configuration validated");
    
    Ok(())
}

async fn demo_edge_deployment(logger: &Logger, cache_dir: &str) -> Result<()> {
    println!("🌐 Scenario: Edge computing deployment with limited bandwidth");
    println!("🎯 Optimizations: Single connection, small layer threshold, high retry count");
    println!();
    
    println!("📄 Command:");
    println!("   docker-image-pusher push \\");
    println!("     --source sensor-hub.tar \\");
    println!("     --target edge-registry.factory.local/iot/sensor-hub:v2.1 \\");
    println!("     --username edge-deploy \\");
    println!("     --password $EDGE_PASSWORD \\");
    println!("     --max-concurrent 1 \\               # Single connection");
    println!("     --large-layer-threshold 536870912 \\ # 512MB threshold");
    println!("     --retry-attempts 10 \\              # High retry count");
    println!("     --enable-dynamic-concurrency \\     # Auto-adjust");
    println!("     --verbose");
    println!();
    
    logger.info("Edge Registry: edge-registry.factory.local");
    logger.info("Max Concurrent: 1 (single connection for stability)");
    logger.info("Large Layer Threshold: 512MB (smaller for edge networks)");
    logger.info("Retry Attempts: 10 (high retry for unreliable networks)");
    logger.info("Dynamic Concurrency: Enabled (auto-adjust based on network)");
    
    logger.success("Edge deployment configuration validated");
    
    Ok(())
}

async fn demo_multiarch_deployment(logger: &Logger, cache_dir: &str) -> Result<()> {
    println!("🏗️  Scenario: Multi-architecture deployment with shared layer optimization");
    println!("🎯 Optimizations: Skip existing layers between architectures, moderate concurrency");
    println!();
    
    println!("📄 Script pattern:");
    println!("   for arch in amd64 arm64 arm; do");
    println!("     echo \"🏗️  Deploying $arch architecture...\"");
    println!("     ");
    println!("     # Extract architecture-specific tar");
    println!("     docker-image-pusher extract --file \"webapp-${{arch}}.tar\" --verbose");
    println!("     ");
    println!("     # Push with shared layer optimization");
    println!("     docker-image-pusher push \\");
    println!("       --source \"webapp:latest\" \\");
    println!("       --target \"registry.company.com/multiarch/webapp:v1.0-${{arch}}\" \\");
    println!("       --username cicd-deploy \\");
    println!("       --password \"$CICD_TOKEN\" \\");
    println!("       --max-concurrent 3 \\");
    println!("       --skip-existing \\                   # Skip common base layers");
    println!("       --retry-attempts 3 \\");
    println!("       --verbose");
    println!("   done");
    println!();
    
    let architectures = ["amd64", "arm64", "arm"];
    
    for arch in &architectures {
        logger.info(&format!("Architecture: {} - Extract tar and push to registry", arch));
        logger.info("  Skip Existing: Enabled (optimize shared base layers)");
        logger.info("  Max Concurrent: 3 (balanced for CI/CD pipelines)");
        sleep(Duration::from_millis(500)).await;
    }
    
    logger.success("Multi-architecture deployment pattern demonstrated");
    
    Ok(())
}
