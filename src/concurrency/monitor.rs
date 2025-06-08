//! Performance Monitoring and Statistics for Concurrency Management
//!
//! This module provides comprehensive performance monitoring, statistical analysis,
//! and network speed regression capabilities for the concurrency management system.

pub mod progress;
pub mod regression;
pub mod stats; 
pub mod performance;
pub mod display;

// Re-export key types for backward compatibility
pub use progress::{ProgressTracker, ActiveTaskInfo, SpeedDataPoint};
pub use regression::{NetworkSpeedRegression, RegressionAnalysis, RegressionResult};
pub use stats::{LayerStats, LayerStatus, OperationStats};
pub use performance::{PerformanceMonitor, PerformanceAnalysis};
pub use display::{
    EnhancedProgressDisplay, PriorityStatistics, ConcurrencyAdjustment,
    SchedulingStrategy
};
