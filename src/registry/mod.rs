//! Registry client module
//!
//! This module provides authentication, client logic, and unified pipeline operations for
//! interacting with Docker Registry HTTP API v2. It supports login, token management, and
//! robust error handling for registry operations.
//!
//! ## Unified Pipeline Architecture
//!
//! The registry module uses a unified pipeline approach that handles both uploads and
//! downloads with priority-based scheduling, eliminating redundancy and simplifying the codebase.

// Core registry functionality
pub mod auth;
pub mod client;
pub mod tar;
pub mod tar_utils;

// Unified pipeline operations (consolidates all upload/download functionality)
pub mod progress;
pub mod stats;
pub mod unified_pipeline;

// Core registry exports
pub use auth::Auth;
pub use client::{RegistryClient, RegistryClientBuilder};
pub use tar_utils::TarUtils;

// Unified pipeline exports (primary interface)
pub use progress::ProgressTracker;
pub use stats::{LayerUploadStats, ProgressReporter, UploadStats};
pub use unified_pipeline::{PipelineConfig, PipelineTask, TaskOperation, UnifiedPipeline};

// No legacy exports needed - using unified pipeline only

use crate::error::Result;
use crate::logging::Logger;

/// Simplified upload configuration (consolidated)
#[derive(Debug, Clone)]
pub struct UploadConfig {
    pub max_concurrent: usize,
    pub timeout_seconds: u64,
    pub retry_attempts: usize,
    pub large_layer_threshold: u64,
    pub small_blob_threshold: u64,
    pub enable_streaming: bool,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            timeout_seconds: 7200,
            retry_attempts: 3,
            large_layer_threshold: 100 * 1024 * 1024, // 100MB
            small_blob_threshold: 10 * 1024 * 1024,   // 10MB
            enable_streaming: true,
        }
    }
}

/// Unified uploader interface (simplified)
pub struct Uploader {
    pipeline: UnifiedPipeline,
}

impl Uploader {
    /// Create new uploader with unified pipeline
    pub fn new(pipeline: UnifiedPipeline) -> Self {
        Self { pipeline }
    }

    /// Upload layers using unified pipeline
    pub async fn upload_layers(
        &self,
        layers: &[crate::image::parser::LayerInfo],
        repository: &str,
        tar_path: &std::path::Path,
        token: &Option<String>,
        client: std::sync::Arc<crate::registry::RegistryClient>,
    ) -> Result<()> {
        self.pipeline
            .process_uploads(layers, repository, tar_path, token, client)
            .await
    }
}

/// Registry coordinator (simplified and unified)
pub struct RegistryCoordinator {
    pipeline: UnifiedPipeline,
    config: PipelineConfig,
}

impl RegistryCoordinator {
    /// Create coordinator with unified pipeline
    pub fn new(output: Logger) -> Self {
        let config = PipelineConfig::default();
        let pipeline = UnifiedPipeline::new(output).with_config(config.clone());

        Self { pipeline, config }
    }

    /// Create coordinator with custom configuration
    pub fn with_config(output: Logger, config: PipelineConfig) -> Self {
        let pipeline = UnifiedPipeline::new(output).with_config(config.clone());

        Self { pipeline, config }
    }

    /// Create uploader using unified pipeline
    pub fn create_uploader(&self) -> Uploader {
        Uploader::new(self.pipeline.clone())
    }

    /// Get the current pipeline configuration
    pub fn get_config(&self) -> &PipelineConfig {
        &self.config
    }

    /// Upload layers using unified pipeline
    pub async fn upload_layers(
        &self,
        layers: &[crate::image::parser::LayerInfo],
        repository: &str,
        tar_path: &std::path::Path,
        token: &Option<String>,
        client: std::sync::Arc<crate::registry::RegistryClient>,
    ) -> Result<()> {
        self.pipeline
            .process_uploads(layers, repository, tar_path, token, client)
            .await
    }

    /// Download layers using unified pipeline
    pub async fn download_layers(
        &self,
        layers: &[crate::image::parser::LayerInfo],
        repository: &str,
        token: &Option<String>,
        client: std::sync::Arc<crate::registry::RegistryClient>,
        cache: &mut crate::image::cache::Cache,
    ) -> Result<()> {
        self.pipeline
            .process_downloads(layers, repository, token, client, cache)
            .await
    }
}
