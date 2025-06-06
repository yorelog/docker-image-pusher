//! Docker Image Pusher Library
//! 
//! This file serves as the library root for the docker-image-pusher crate,
//! organizing and exposing the various modules that make up the application.

pub mod error;
pub mod output;
pub mod config;
pub mod digest;  // 新增的digest工具模块
pub mod tar_utils; // Shared tar processing utilities
pub mod image;
pub mod registry;
pub mod upload;
pub mod cli;

pub use output::OutputManager;
pub use error::{Result, PusherError};
pub use config::AuthConfig;
pub use digest::DigestUtils;  // 导出digest工具