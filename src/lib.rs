//! Docker Image Pusher Library
//! 
//! This file serves as the library root for the docker-image-pusher crate,
//! organizing and exposing the various modules that make up the application.

pub mod error;
pub mod output;
pub mod config;
pub mod image;
pub mod registry;
pub mod upload;
pub mod cli;

pub use output::OutputManager;
pub use error::{Result, PusherError};
pub use config::AuthConfig;