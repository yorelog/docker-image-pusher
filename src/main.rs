//! Docker Image Pusher - Main entry point
//! 
//! This tool allows you to push Docker images to registries with optimized
//! handling for large layers and improved error reporting.

use docker_image_pusher::cli::{args::Args, runner::Runner};
use docker_image_pusher::error::PusherError;

#[tokio::main]
async fn main() {
    // Parse command line arguments (clap will handle errors and exit on failure)
    let args = Args::parse();
    
    // Validate arguments
    if let Err(e) = args.validate() {
        eprintln!("❌ Validation error: {}", e);
        
        // Add helpful hints for new flags
        if e.to_string().contains("skip-existing") || e.to_string().contains("force-upload") {
            eprintln!("💡 Use --skip-existing to avoid uploading layers that already exist");
            eprintln!("   Use --force-upload to upload all layers regardless of existence");
            eprintln!("   These flags are mutually exclusive");
        }
        
        std::process::exit(1);
    }

    // Create and run the runner
    match Runner::new(args) {
        Ok(runner) => {
            if let Err(e) = runner.run().await {
                eprintln!("❌ Error: {}", e);
                
                // Enhanced error reporting based on error type
                match e {
                    PusherError::Validation(msg) => {
                        eprintln!("💡 Please check your command line arguments");
                        eprintln!("   {}", msg);
                    },
                    PusherError::Io(msg) => {
                        eprintln!("💡 File system error occurred");
                        eprintln!("   {}", msg);
                        eprintln!("   Please check file permissions and available disk space");
                    },
                    PusherError::Network(msg) => {
                        eprintln!("💡 Network connectivity issue");
                        eprintln!("   {}", msg);
                        eprintln!("   Please check your internet connection and registry URL");
                    },
                    PusherError::Authentication(msg) => {
                        eprintln!("💡 Authentication failed");
                        eprintln!("   {}", msg);
                        eprintln!("   Please check your credentials and registry permissions");
                    },
                    PusherError::Registry(msg) => {
                        eprintln!("💡 Registry error");
                        eprintln!("   {}", msg);
                        eprintln!("   Please check registry availability and configuration");
                    },
                    PusherError::Upload(msg) => {
                        eprintln!("💡 Upload failed");
                        eprintln!("   {}", msg);
                        eprintln!("   Consider retrying or checking registry storage limits");
                    },
                    PusherError::ImageParsing(msg) => {
                        eprintln!("💡 Image parsing error");
                        eprintln!("   {}", msg);
                        eprintln!("   Please verify the Docker image file is valid");
                    },
                    PusherError::Parse(msg) => {
                        eprintln!("💡 Data parsing error");
                        eprintln!("   {}", msg);
                    },
                    PusherError::Config(msg) => {
                        eprintln!("💡 Configuration error");
                        eprintln!("   {}", msg);
                    },
                    PusherError::Timeout(msg) => {
                        eprintln!("💡 Operation timed out");
                        eprintln!("   {}", msg);
                        eprintln!("   Consider increasing the timeout value with --timeout option");
                        eprintln!("   or check network stability for large file uploads");
                    },
                }
                
                std::process::exit(1);
            }
            
            // Success - no need to print anything here as Runner will handle success output
        },
        Err(e) => {
            eprintln!("❌ Failed to initialize: {}", e);
            std::process::exit(1);
        }
    }
}