use docker_image_pusher::registry::oci_client::OciClientAdapter;
use docker_image_pusher::logging::Logger;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger
    let logger = Logger::new(true); // Set verbose to true for detailed output
    logger.info("🚀 Starting OCI Client Batch Progress Demo");

    // Create OCI client with enhanced progress tracking
    let registry_url = "https://registry-1.docker.io".to_string();
    let client = OciClientAdapter::new(registry_url, logger.clone())?;

    // Demo repository
    let repository = "library/hello-world";
    
    // Simulate batch blob operations with progress tracking
    logger.section("📦 Batch Blob Operations Demo");
    
    // Example 1: Batch download simulation
    logger.info("Demonstrating batch download with progress tracking...");
    let sample_digests = vec![
        "sha256:719385e32844401d57ecfd3eacab360bf551a1491c05b85806ed8de58c9f5267".to_string(),  // Sample digest
        "sha256:b49b9a47bc3c6ac2c9b43e3a0a3c9b3e7a9c8d6e9f0e8d7c6b5a4938271c5e0f".to_string(),  // Sample digest
    ];
    
    // Note: This is just a demo - these digests may not exist
    logger.warning("⚠️  Note: Running demo with sample digests (may fail with real registry)");
    
    match client.batch_pull_blobs(&sample_digests, repository).await {
        Ok(results) => {
            logger.success(&format!("Batch download demo completed with {} results", results.len()));
            for (digest, result) in results {
                match result {
                    Ok(data) => logger.success(&format!("✅ Downloaded {}: {} bytes", &digest[..16], data.len())),
                    Err(e) => logger.error(&format!("❌ Failed {}: {}", &digest[..16], e)),
                }
            }
        }
        Err(e) => {
            logger.warning(&format!("Batch download demo failed (expected): {}", e));
        }
    }
    
    // Example 2: Batch upload simulation
    logger.info("Demonstrating batch upload progress tracking...");
    let sample_blobs = vec![
        (b"Hello World Content 1".to_vec(), "sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string()),
        (b"Hello World Content 2 with more data".to_vec(), "sha256:abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string()),
        (b"Hello World Content 3 with even more data for demonstration".to_vec(), "sha256:fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321".to_string()),
    ];
    
    logger.warning("⚠️  Note: Running upload demo with test data (may fail with real registry)");
    
    match client.batch_push_blobs(&sample_blobs, repository).await {
        Ok(results) => {
            logger.success(&format!("Batch upload demo completed with {} results", results.len()));
            for (digest, result) in results {
                match result {
                    Ok(url) => logger.success(&format!("✅ Uploaded {}: {}", &digest[..16], url)),
                    Err(e) => logger.error(&format!("❌ Failed {}: {}", &digest[..16], e)),
                }
            }
        }
        Err(e) => {
            logger.warning(&format!("Batch upload demo failed (expected): {}", e));
        }
    }
    
    // Example 3: Show individual blob operations with enhanced progress
    logger.info("Demonstrating individual blob operations with enhanced progress...");
    
    logger.info("Individual blob download with detailed progress...");
    match client.pull_blob(repository, &sample_digests[0]).await {
        Ok(data) => {
            logger.success(&format!("Individual download successful: {} bytes", data.len()));
        }
        Err(e) => {
            logger.warning(&format!("Individual download failed (expected): {}", e));
        }
    }
    
    logger.info("Individual blob upload with detailed progress...");
    let test_data = b"Test blob data for individual upload demonstration";
    match client.push_blob(repository, test_data, "sha256:individual_test").await {
        Ok(url) => {
            logger.success(&format!("Individual upload successful: {}", url));
        }
        Err(e) => {
            logger.warning(&format!("Individual upload failed (expected): {}", e));
        }
    }

    logger.section("✨ Enhanced Progress Features Demonstrated");
    logger.info("✅ Blob size display (KB, MB, GB formatting)");
    logger.info("✅ Duration tracking (ms, s, m, h formatting)");
    logger.info("✅ Transfer speed calculation");
    logger.info("✅ Remaining blob count in batch operations");
    logger.info("✅ Progress indicators with emojis");
    logger.info("✅ Detailed error reporting with timing");
    logger.info("✅ Batch operation summary statistics");

    logger.success("🎉 OCI Client Batch Progress Demo completed!");
    
    Ok(())
}
