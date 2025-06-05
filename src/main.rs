//! Docker Image Pusher - Main entry point

use docker_image_pusher::cli;

fn main() {
    if let Err(e) = cli::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}