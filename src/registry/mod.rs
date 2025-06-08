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
//! All concurrency management and progress tracking is now handled by the dedicated
//! concurrency module for better feature richness and maintainability.

// Core registry functionality
pub mod adapter;
pub mod auth;
pub mod client;
pub mod oci_client;
pub mod operations;
pub mod pipeline;
pub mod stats;
pub mod tar;
pub mod tar_utils;
pub mod token_manager;
pub mod transport;

// Core registry exports
pub use auth::{Auth, TokenInfo};
pub use client::{RegistryClient, RegistryClientBuilder};
pub use oci_client::{OciClientAdapter, OciClientBuilder, OciRegistryOperations};
pub use pipeline::{
    EnhancedProgressTracker, TaskOperation, PipelineConfig, 
    EnhancedConcurrencyStats, PriorityQueueStatus, NetworkSpeedStats,
    ConcurrencyAdjustmentRecord, PerformancePrediction, PipelineStats,
    TaskMetadata, ProgressDisplayUtils, UnifiedPipeline, UploadConfig,
    Uploader, RegistryCoordinator
};
pub use stats::{UploadStats, LayerUploadStats, LayerStatus, ProgressReporter, SessionStats};
pub use tar_utils::TarUtils;
pub use token_manager::TokenManager;
pub use adapter::{RegistryClientAdapter, UnifiedRegistryClient};
