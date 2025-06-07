//! Docker Image Pusher - Main entry point
//!
//! This tool allows you to push Docker images to registries with optimized
//! handling for large layers and improved error reporting.

use docker_image_pusher::cli::args::Args;
use docker_image_pusher::cli::runner::Runner;
use docker_image_pusher::error::{RegistryError, Result};
use docker_image_pusher::logging::Logger;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse arguments early to handle help/version
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("❌ Error parsing arguments: {}", e);
            std::process::exit(1);
        }
    };

    // Get verbose setting from command
    let verbose = extract_verbose_flag(&args);
    let logger = Logger::new(verbose);

    // Run the application
    match run_app(args, logger).await {
        Ok(()) => {
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("❌ Application error: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run_app(args: Args, logger: Logger) -> Result<()> {
    // Validate arguments
    args.validate()?;

    // Create and run the application
    let runner = Runner::new(logger.verbose);
    runner.run(args).await.map_err(|e| {
        // Add context to any errors that occur during execution
        match e {
            RegistryError::Network(msg) => {
                RegistryError::Network(format!("Network operation failed: {}", msg))
            }
            RegistryError::Auth(msg) => {
                RegistryError::Auth(format!("Authentication failed: {}", msg))
            }
            other => other,
        }
    })
}

fn extract_verbose_flag(args: &Args) -> bool {
    match &args.command {
        Some(command) => match command {
            docker_image_pusher::cli::args::Commands::Pull(pull_args) => pull_args.verbose,
            docker_image_pusher::cli::args::Commands::Extract(extract_args) => extract_args.verbose,
            docker_image_pusher::cli::args::Commands::Push(push_args) => push_args.verbose,
            docker_image_pusher::cli::args::Commands::List(_) => false,
            docker_image_pusher::cli::args::Commands::Clean(_) => false,
        },
        None => false,
    }
}
