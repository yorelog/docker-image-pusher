//! Docker Image Pusher
//!
//! `docker-image-pusher` is a command-line tool and library for pushing Docker image tar packages directly to Docker registries (including Docker Hub, Harbor, and private registries) with advanced chunked upload support, robust digest validation, and detailed error reporting.
//!
//! ## Features
//! - **Direct push of Docker/Podman image tarballs**: No need to load images into a local daemon.
//! - **Chunked and parallel upload**: Efficiently uploads large layers with retry and progress tracking.
//! - **Digest validation**: Ensures layer and config digests match Docker/OCI standards.
//! - **Flexible authentication**: Supports username/password and token-based auth.
//! - **Comprehensive error handling**: Clear error messages for network, registry, and file issues.
//! - **Verbose and quiet modes**: Control output for CI or debugging.
//!
//! ## Main Modules
//! - [`cli`] - Command-line interface and argument parsing.
//! - [`config`] - Configuration and authentication structures.
//! - [`digest`] - Digest calculation and validation utilities.
//! - [`error`] - Error types and handlers.
//! - [`image`] - Image tarball parsing and metadata extraction.
//! - [`output`] - Structured output and logging.
//! - [`registry`] - Registry client and authentication.
//! - [`tar_utils`] - Tarball extraction and layer handling.
//! - [`upload`] - Upload strategies and progress tracking.
//!
//! ## Example Usage
//!
//! ```sh
//! docker save myimage:latest -o myimage.tar
//! docker-image-pusher --file myimage.tar --repository-url https://my-registry.com/myimage:latest --username user --password pass
//! ```
//!
//! ## Library Usage
//! This crate can also be used as a library for custom workflows. See the documentation for each module for details.

pub mod cli;
pub mod config;
pub mod digest; // 新增的digest工具模块
pub mod error;
pub mod image;
pub mod output;
pub mod registry;
pub mod tar_utils; // Shared tar processing utilities
pub mod upload;

pub use config::AuthConfig;
pub use digest::DigestUtils;
pub use error::{PusherError, Result};
pub use output::OutputManager; // 导出digest工具
