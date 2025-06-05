//! Docker Image Pusher Library
//! 
//! A modular library for pushing Docker image tar packages to registries

pub mod cli;
pub mod config;
pub mod error;
pub mod image;
pub mod registry;

pub use cli::run;
pub use error::{Result, PusherError};