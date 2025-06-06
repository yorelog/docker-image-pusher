//! Registry module for Docker registry interactions
//!
//! This module provides authentication and client logic for interacting with Docker Registry HTTP API v2.
//! It supports login, token management, and robust error handling for registry operations.

pub mod auth;
pub mod client;

pub use crate::config::AuthConfig;
pub use auth::Auth;
pub use client::{RegistryClient, RegistryClientBuilder}; // Make sure this is exported from registry module
