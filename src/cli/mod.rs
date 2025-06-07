//! Command line interface module
//!
//! This module provides the entry point for parsing command-line arguments and running the main workflow.
//! It includes argument parsing, validation, and the main runner logic for pushing Docker images.

pub mod args;
pub mod config;
pub mod operation_mode;
pub mod runner;

pub use args::Args;
pub use config::AuthConfig;
pub use operation_mode::OperationMode;
pub use runner::Runner;
