//! Docker Image Pusher Library
//!
//! A library for pushing Docker images to registries

pub mod cli;
pub mod error;
pub mod image;
pub mod logging;
pub mod registry;

// 核心类型导出
pub use cli::config::AuthConfig;
pub use error::{RegistryError, Result};
pub use logging::Logger;
pub use registry::{RegistryClient, RegistryClientBuilder};

/// Create upload configuration from CLI arguments
pub fn create_upload_config_from_args(
    max_concurrent: usize,
    timeout: u64,
    retry_attempts: usize,
    large_threshold: u64,
) -> registry::UploadConfig {
    registry::UploadConfig {
        max_concurrent,
        timeout_seconds: timeout,
        retry_attempts,
        large_layer_threshold: large_threshold,
        small_blob_threshold: 1024 * 1024, // 1MB default
        enable_streaming: true,
    }
}
