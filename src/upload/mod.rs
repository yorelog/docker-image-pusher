//! Upload module for handling chunked uploads and progress tracking
//!
//! This module provides multiple upload strategies (chunked, parallel, streaming) for efficient and reliable
//! transfer of Docker image layers to registries. It includes progress tracking and statistics reporting.

pub mod chunked;
pub mod parallel;
pub mod progress;
pub mod stats;
pub mod strategy;
pub mod streaming;

pub use chunked::ChunkedUploader;
pub use parallel::ParallelUploader;
pub use progress::ProgressTracker;
pub use strategy::{UploadStrategy, UploadStrategyFactory};
pub use streaming::StreamingUploader;
