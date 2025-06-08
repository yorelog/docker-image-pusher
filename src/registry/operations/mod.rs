//! Registry operations module - Internal modular organization
//!
//! This module provides internal organization for registry operations 
//! while preserving the existing public API in client.rs

pub mod auth_operations;
pub mod blob_operations;
pub mod manifest_operations;
pub mod repository_operations;

// Re-export for internal use
pub use auth_operations::AuthOperations;
pub use blob_operations::BlobOperations;
pub use manifest_operations::ManifestOperations;
pub use repository_operations::RepositoryOperations;
