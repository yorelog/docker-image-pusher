//! Concurrency Management Module - Flagship Feature
//!
//! This module provides a sophisticated, unified concurrency control system for Docker image
//! operations. It consolidates all concurrency-related functionality that was previously
//! scattered across multiple files, offering advanced features like dynamic adjustment,
//! performance monitoring, intelligent task scheduling, and comprehensive progress tracking.
//!
//! ## Key Features
//!
//! ### 🚀 **Advanced Concurrency Management**
//! - **Adaptive Concurrency**: Machine learning-based optimization with intelligent strategy selection
//! - **Dynamic Adjustment**: Real-time adjustment based on performance metrics
//! - **Performance Monitoring**: Comprehensive tracking with regression analysis
//! - **Intelligent Strategy Selection**: Automatic strategy switching based on conditions
//!
//! ### 📊 **Comprehensive Monitoring & Statistics**
//! - Real-time performance tracking with regression analysis
//! - Layer-level progress tracking and statistics
//! - Operation-level statistics and reporting
//! - Statistical trend detection and prediction
//!
//! ### 🔧 **Flexible Configuration**
//! - Hierarchical configuration structure with validation
//! - Predefined optimized configurations for common scenarios
//! - Runtime adjustment capabilities
//! - Strategy-specific parameter tuning
//!
//! ### 🎯 **Unified Interface**
//! - Single point of integration for all concurrency needs
//! - Consistent API across different concurrency strategies
//! - Seamless integration with registry operations
//! - Comprehensive error handling and recovery
//!
//! ## Usage Example
//!
//! ```no_run
//! use docker_image_pusher::concurrency::{ConcurrencyConfig, AdaptiveConcurrencyManager};
//! use docker_image_pusher::logging::Logger;
//! 
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create adaptive concurrency configuration
//! let config = ConcurrencyConfig::default()
//!     .with_max_concurrent(16)
//!     .enable_dynamic_concurrency(true)
//!     .with_speed_threshold(50.0);
//!     
//! // Initialize adaptive concurrency manager
//! let manager = AdaptiveConcurrencyManager::new(config);
//! 
//! // Acquire permits for concurrent operations
//! let permits = manager.acquire_permits(4).await?;
//! // ... perform operations with permits
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! This module replaces and consolidates the following previously scattered functionality:
//! - Registry progress tracking (`src/registry/progress.rs` - REMOVED)
//! - Registry statistics (`src/registry/stats.rs` - REMOVED) 
//! - Registry streaming (`src/registry/streaming.rs` - REMOVED)
//! - Registry strategy (`src/registry/strategy.rs` - REMOVED)
//! - Various pipeline-specific concurrency handling
//!
//! All concurrency logic is now centralized here for better maintainability,
//! feature richness, and performance optimization.

pub mod manager;
pub mod monitor;
pub mod strategy;
pub mod config;
pub mod pipeline;
pub mod integration;

pub use manager::AdaptiveConcurrencyManager;
pub use strategy::{ConcurrencyStrategy, StrategySelector};
pub use monitor::{
    PerformanceMonitor, ProgressTracker, LayerStats, LayerStatus, OperationStats,
    RegressionAnalysis, SpeedDataPoint, NetworkSpeedRegression
};
pub use config::{ConcurrencyConfig, ConcurrencyLimits};
pub use pipeline::{
    PipelineStage, PipelineTask, PipelineProgress, PipelineManager,
    StageProgress,
};
pub use integration::PipelineMonitorBridge;

use std::time::Instant;

/// Core concurrency management interface
/// 
/// This trait defines the contract for concurrency controllers used
/// throughout the application.
#[async_trait::async_trait]
pub trait ConcurrencyController: Send + Sync {
    /// Get the current concurrency limit
    fn current_concurrency(&self) -> usize;
    
    /// Update performance metrics and potentially adjust concurrency
    fn update_metrics(&self, bytes_transferred: u64, elapsed: std::time::Duration);
    
    /// Request permits for task execution
    async fn acquire_permits(&self, count: usize) -> Result<Vec<ConcurrencyPermit>, ConcurrencyError>;
    
    /// Check if concurrency adjustment is recommended
    fn should_adjust_concurrency(&self) -> bool;
    
    /// Get performance statistics
    fn get_statistics(&self) -> ConcurrencyStatistics;
}

/// Represents a concurrency permit for task execution
pub struct ConcurrencyPermit {
    /// Unique permit identifier
    pub id: String,
    /// Timestamp when permit was acquired
    pub acquired_at: Instant,
    /// Release callback - now stored as an optional closure
    release_fn: Option<Box<dyn FnOnce() + Send>>,
}

impl ConcurrencyPermit {
    /// Create a new concurrency permit
    pub fn new(id: String, release_fn: Box<dyn FnOnce() + Send>) -> Self {
        Self {
            id,
            acquired_at: Instant::now(),
            release_fn: Some(release_fn),
        }
    }
    
    /// Release the permit manually
    pub fn release(mut self) {
        if let Some(release_fn) = self.release_fn.take() {
            release_fn();
        }
    }
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        if let Some(release_fn) = self.release_fn.take() {
            release_fn();
        }
    }
}

/// Extended concurrency error types
#[derive(Debug, thiserror::Error)]
pub enum ConcurrencyError {
    #[error("Failed to acquire permit: {0}")]
    PermitAcquisitionFailed(String),
    
    #[error("Concurrency limit exceeded: requested {requested}, limit {limit}")]
    LimitExceeded { requested: usize, limit: usize },
    
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
    
    #[error("Performance monitoring error: {0}")]
    MonitoringError(String),
    
    #[error("Task not found: {0}")]
    TaskNotFound(String),
    
    #[error("Pipeline error: {0}")]
    PipelineError(String),
    
    #[error("Dependency resolution failed for task: {0}")]
    DependencyResolutionFailed(String),
}

/// Comprehensive concurrency statistics
#[derive(Debug, Clone)]
pub struct ConcurrencyStatistics {
    /// Current active permits
    pub active_permits: usize,
    /// Maximum concurrency limit
    pub max_concurrency: usize,
    /// Current concurrency strategy
    pub current_strategy: String,
    /// Total permits issued
    pub total_permits_issued: u64,
    /// Total permits released
    pub total_permits_released: u64,
    /// Average permit hold time
    pub avg_permit_hold_time: std::time::Duration,
    /// Performance metrics
    pub performance: PerformanceStatistics,
    /// Strategy adjustment history
    pub strategy_history: Vec<StrategyAdjustment>,
}

/// Performance statistics for monitoring
#[derive(Debug, Clone)]
pub struct PerformanceStatistics {
    /// Current transfer speed (bytes/sec)
    pub current_speed: u64,
    /// Average transfer speed (bytes/sec)
    pub average_speed: u64,
    /// Peak transfer speed (bytes/sec)
    pub peak_speed: u64,
    /// Total bytes transferred
    pub total_bytes: u64,
    /// Total transfer time
    pub total_time: std::time::Duration,
    /// Speed trend analysis
    pub trend: SpeedTrend,
    /// Confidence in trend analysis (0.0-1.0)
    pub trend_confidence: f64,
}

/// Speed trend analysis results
#[derive(Debug, Clone, PartialEq)]
pub enum SpeedTrend {
    /// Speed is increasing consistently
    Increasing,
    /// Speed is decreasing consistently  
    Decreasing,
    /// Speed is relatively stable
    Stable,
    /// Insufficient data for analysis
    Unknown,
}

/// Strategy adjustment record
#[derive(Debug, Clone)]
pub struct StrategyAdjustment {
    /// When the adjustment occurred
    pub timestamp: Instant,
    /// Previous strategy
    pub from_strategy: String,
    /// New strategy
    pub to_strategy: String,
    /// Previous concurrency limit
    pub from_concurrency: usize,
    /// New concurrency limit
    pub to_concurrency: usize,
    /// Reason for adjustment
    pub reason: String,
    /// Performance metrics at time of adjustment
    pub performance_snapshot: PerformanceStatistics,
}

/// Result type for concurrency operations
pub type ConcurrencyResult<T> = Result<T, ConcurrencyError>;

/// Factory for creating concurrency managers
pub struct ConcurrencyFactory;

impl ConcurrencyFactory {
    /// Create an adaptive concurrency manager with ML capabilities
    pub fn create_adaptive_manager(config: ConcurrencyConfig) -> Box<dyn ConcurrencyController> {
        Box::new(manager::AdaptiveConcurrencyManager::new(config))
    }
    
    /// Create a concurrency manager (defaults to adaptive)
    pub fn create_manager(config: ConcurrencyConfig) -> Box<dyn ConcurrencyController> {
        Self::create_adaptive_manager(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_concurrency_factory() {
        let config = ConcurrencyConfig::default();
        let manager = ConcurrencyFactory::create_adaptive_manager(config);
        assert!(manager.current_concurrency() > 0);
    }
    
    #[tokio::test]
    async fn test_permit_acquisition() {
        let config = ConcurrencyConfig::default();
        let manager = ConcurrencyFactory::create_manager(config);
        
        let permits = manager.acquire_permits(2).await.unwrap();
        assert_eq!(permits.len(), 2);
        
        // Release permits
        for permit in permits {
            permit.release();
        }
    }
}
