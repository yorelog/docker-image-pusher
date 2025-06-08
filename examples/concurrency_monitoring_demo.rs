//! Demo showcasing the enhanced concurrency monitoring in blob uploads
//!
//! This example demonstrates how the Docker Image Pusher now shows real-time
//! information about active concurrent tasks during blob uploads.

use docker_image_pusher::image::BlobHandler;
use docker_image_pusher::logging::Logger;
use docker_image_pusher::registry::PipelineConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Docker Image Pusher - Concurrency Monitoring Demo");
    println!("====================================================");
    
    let logger = Logger::new(false); // false for non-verbose mode
    
    // Create blob handler with different concurrency configurations
    println!("\n📊 Testing different concurrency configurations:");
    
    // Test 1: Default configuration (should be 3 concurrent tasks now)
    let default_config = PipelineConfig::default();
    let _blob_handler_default = BlobHandler::with_config(logger.clone(), default_config.clone());
    
    println!("✅ Default Configuration:");
    println!("   - Max concurrent tasks: {}", default_config.max_concurrent);
    println!("   - Retry attempts: {}", default_config.retry_attempts);
    println!("   - Timeout: {}s", default_config.timeout_seconds);
    
    // Test 2: Conservative configuration (1 concurrent task)
    let conservative_config = PipelineConfig {
        max_concurrent: 1,
        ..PipelineConfig::default()
    };
    let _blob_handler_conservative = BlobHandler::with_config(logger.clone(), conservative_config.clone());
    
    println!("\n🔒 Conservative Configuration:");
    println!("   - Max concurrent tasks: {}", conservative_config.max_concurrent);
    println!("   - This prevents memory exhaustion for very large images");
    
    // Test 3: Aggressive configuration (5 concurrent tasks)
    let aggressive_config = PipelineConfig {
        max_concurrent: 5,
        ..PipelineConfig::default()
    };
    let _blob_handler_aggressive = BlobHandler::with_config(logger.clone(), aggressive_config.clone());
    
    println!("\n⚡ Aggressive Configuration:");
    println!("   - Max concurrent tasks: {}", aggressive_config.max_concurrent);
    println!("   - Use with caution - may cause SIGKILL for large blobs");
    
    println!("\n🎯 Key Features of Concurrency Monitoring:");
    println!("   • Real-time active task count in upload logs");
    println!("   • Adaptive concurrency based on blob sizes:");
    println!("     - Blobs > 1GB: Max 1 concurrent task");
    println!("     - Blobs > 500MB: Max 2 concurrent tasks");
    println!("     - Memory-based scaling for smaller blobs");
    println!("   • Peak concurrent tasks summary");
    println!("   • Periodic progress updates every 5 seconds");
    
    println!("\n📈 Sample Upload Logs with Concurrency Info:");
    println!("   Upload task 1: Processing layer blob abcd1234... (150.2 MB) [Active tasks: 1]");
    println!("   Upload task 2: Processing layer blob efgh5678... (75.8 MB) [Active tasks: 2]");
    println!("   ✅ Blob abcd1234 uploaded in 15.3s (9.8 MB/s) [Active tasks: 1]");
    println!("   📊 Upload progress: 2 active tasks, 1 remaining");
    println!("   ✅ Unified Pipeline completed successfully (avg speed: 12.5 MB/s) [Peak concurrent tasks: 3]");
    
    println!("\n🛡️  Memory Protection Benefits:");
    println!("   • Prevents SIGKILL termination from excessive memory usage");
    println!("   • 2GB memory limit with intelligent blob size detection");
    println!("   • Conservative handling of large total image sizes (>10GB)");
    println!("   • 10MB overhead estimation per concurrent blob");
    
    println!("\n✨ Enhanced Error Handling:");
    println!("   • Individual task failure tracking");
    println!("   • Active task count shown in error messages");
    println!("   • Graceful degradation under memory pressure");
    
    logger.success("Demo completed successfully! Your blob uploads now have full visibility into concurrent task execution.");
    
    Ok(())
}
