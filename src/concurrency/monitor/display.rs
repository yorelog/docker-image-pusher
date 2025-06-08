//! Display structures and reporting functionality for progress visualization

use std::time::Instant;
use crate::concurrency::SpeedTrend;
use super::progress::ActiveTaskInfo;
use super::performance::PerformanceAnalysis;

/// Scheduling strategy for task prioritization
#[derive(Debug, Clone, PartialEq)]
pub enum SchedulingStrategy {
    /// Large files first (for reliability)
    LargeFilesFirst,
    /// Small files first (for faster completion)
    SmallFilesFirst,
    /// Priority-based scheduling with adaptive weights
    PriorityBased,
    /// Speed-optimized scheduling based on network performance
    SpeedOptimized,
    /// Round-robin scheduling for fairness
    RoundRobin,
}

/// Enhanced progress display information
#[derive(Debug, Clone)]
pub struct EnhancedProgressDisplay {
    /// Current performance analysis
    pub analysis: PerformanceAnalysis,
    /// List of active tasks
    pub active_tasks: Vec<ActiveTaskInfo>,
    /// Priority distribution statistics
    pub priority_stats: PriorityStatistics,
    /// Recent concurrency adjustments
    pub recent_adjustments: Vec<ConcurrencyAdjustment>,
    /// Scheduling strategy being used
    pub scheduling_strategy: SchedulingStrategy,
    /// Progress percentage (0.0-100.0)
    pub progress_percentage: f64,
    /// Bytes percentage (0.0-100.0)
    pub bytes_percentage: f64,
    /// Estimated time to completion in seconds
    pub eta_seconds: Option<u64>,
    /// Number of active tasks
    pub active_task_count: usize,
    /// Active task information
    pub active_task_info: Vec<ActiveTaskInfo>,
    /// Current concurrency level
    pub current_concurrency: usize,
    /// Maximum concurrency allowed
    pub max_concurrency: usize,
    /// Current transfer speed
    pub current_speed: f64,
    /// Network performance trend
    pub network_trend: SpeedTrend,
    /// Speed regression slope indicator
    pub speed_regression_slope: f64,
    /// Confidence in speed regression analysis
    pub speed_regression_confidence: f64,
    /// Overall performance score
    pub performance_score: f64,
}

/// Priority queue statistics
#[derive(Debug, Clone)]
pub struct PriorityStatistics {
    /// Number of high-priority tasks
    pub high_priority_count: usize,
    /// Number of medium-priority tasks
    pub medium_priority_count: usize,
    /// Number of low-priority tasks
    pub low_priority_count: usize,
    /// Average priority level
    pub average_priority: f64,
    /// Priority distribution efficiency score
    pub distribution_efficiency: f64,
    /// Total completed tasks
    pub total_completed: usize,
    /// Small files completed
    pub small_files_completed: usize,
    /// Medium files completed
    pub medium_files_completed: usize,
    /// Large files completed
    pub large_files_completed: usize,
    /// Average speed for small files
    pub avg_small_speed: f64,
    /// Average speed for medium files
    pub avg_medium_speed: f64,
    /// Average speed for large files
    pub avg_large_speed: f64,
}

/// Concurrency adjustment record for display
#[derive(Debug, Clone)]
pub struct ConcurrencyAdjustment {
    /// Timestamp of the adjustment
    pub timestamp: Instant,
    /// Previous concurrency level
    pub old_concurrency: usize,
    /// New concurrency level
    pub new_concurrency: usize,
    /// Reason for the adjustment
    pub reason: String,
    /// Performance impact of the change
    pub performance_impact: f64,
    /// Performance before the adjustment
    pub performance_before: f64,
    /// Performance after the adjustment
    pub performance_after: Option<f64>,
}
