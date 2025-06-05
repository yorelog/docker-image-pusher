//! Registry module for Docker registry interactions

pub mod client;
pub mod auth;

pub use auth::Auth;
pub use client::{RegistryClient, RegistryClientBuilder};
pub use crate::config::AuthConfig;  // Make sure this is exported from registry module