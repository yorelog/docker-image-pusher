//! Performance Monitoring and Statistics for Concurrency Management
//!
//! This module provides comprehensive performance monitoring, statistical analysis,
//! and network speed regression capabilities for the concurrency management system.

use std::collections::VecDeque;
use std::time::{Duration, Instant};
use crate::concurrency::{SpeedTrend, PerformanceStatistics};
use crate::logging::Logger;

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

/// Data point representing a single transfer measurement
#[derive(Debug, Clone)]
pub struct SpeedDataPoint {
    /// Timestamp when measurement was taken
    pub timestamp: Instant,
    /// Number of bytes transferred
    pub bytes_transferred: u64,
    /// Time elapsed for this transfer
    pub duration: Duration,
    /// Calculated speed in bytes per second
    pub speed: u64,
}

impl SpeedDataPoint {
    /// Create a new speed data point
    pub fn new(bytes_transferred: u64, duration: Duration) -> Self {
        let speed = if duration.as_secs() > 0 {
            bytes_transferred / duration.as_secs()
        } else if duration.as_millis() > 0 {
            (bytes_transferred as u128 * 1000 / duration.as_millis()) as u64
        } else {
            bytes_transferred // Instantaneous transfer
        };

        Self {
            timestamp: Instant::now(),
            bytes_transferred,
            duration,
            speed,
        }
    }
}

/// Results of regression analysis on performance data
#[derive(Debug, Clone)]
pub struct RegressionAnalysis {
    /// Slope of the regression line (speed change over time)
    pub slope: f64,
    /// Y-intercept of the regression line
    pub intercept: f64,
    /// Correlation coefficient (strength of linear relationship)
    pub correlation: f64,
    /// Confidence level in the analysis (0.0-1.0)
    pub confidence: f64,
    /// Predicted speed for next measurement
    pub predicted_speed: u64,
    /// Number of data points used in analysis
    pub sample_size: usize,
}

/// Simple regression analysis result
#[derive(Debug, Clone)]
pub struct RegressionResult {
    pub slope: f64,
    pub confidence: f64,
}

/// Progress tracking for uploads and downloads with advanced reporting
#[derive(Debug, Clone)]
pub struct ProgressTracker {
    /// Total size of the operation
    pub total_size: u64,
    /// Currently processed bytes
    pub processed_bytes: u64,
    /// Start time of the operation
    pub start_time: Instant,
    /// Last update time
    pub last_update: Instant,
    /// Last processed bytes at last update
    pub last_processed: u64,
    /// Operation name for logging
    pub operation_name: String,
    /// Logger for output
    pub output: Logger,
    /// Update threshold (bytes)
    pub update_threshold: u64,
    /// Update interval (seconds)
    pub update_interval: Duration,
}

impl ProgressTracker {
    /// Create a new progress tracker
    pub fn new(total_size: u64, output: Logger, operation_name: String) -> Self {
        let update_threshold = std::cmp::min(10 * 1024 * 1024, total_size / 20); // 10MB or 5%
        
        Self {
            total_size,
            processed_bytes: 0,
            start_time: Instant::now(),
            last_update: Instant::now(),
            last_processed: 0,
            operation_name,
            output,
            update_threshold,
            update_interval: Duration::from_secs(5),
        }
    }

    /// Update progress with new processed bytes
    pub fn update(&mut self, processed_bytes: u64) {
        self.processed_bytes = processed_bytes;
        let now = Instant::now();
        let elapsed_since_last = now.duration_since(self.last_update);

        // Update progress based on thresholds
        if elapsed_since_last >= self.update_interval
            || processed_bytes - self.last_processed >= self.update_threshold
            || processed_bytes == self.total_size
        {
            let percent = if self.total_size > 0 {
                (processed_bytes as f64 / self.total_size as f64 * 100.0) as u8
            } else {
                0
            };

            let speed_mbps = if elapsed_since_last.as_secs() > 0 {
                let bytes_diff = processed_bytes - self.last_processed;
                bytes_diff / elapsed_since_last.as_secs() / 1024 / 1024
            } else {
                0
            };

            self.output.progress(&format!(
                "{}: {}% ({}/{}) - {} MB/s",
                self.operation_name,
                percent,
                self.output.format_size(processed_bytes),
                self.output.format_size(self.total_size),
                speed_mbps
            ));

            self.last_update = now;
            self.last_processed = processed_bytes;
        }
    }

    /// Force a progress update regardless of thresholds
    pub fn force_update(&mut self) {
        let percent = if self.total_size > 0 {
            (self.processed_bytes as f64 / self.total_size as f64 * 100.0) as u8
        } else {
            0
        };

        let total_elapsed = self.start_time.elapsed();
        let avg_speed_mbps = if total_elapsed.as_secs() > 0 {
            self.processed_bytes / total_elapsed.as_secs() / 1024 / 1024
        } else {
            0
        };

        self.output.progress(&format!(
            "{}: {}% ({}/{}) - Avg {} MB/s",
            self.operation_name,
            percent,
            self.output.format_size(self.processed_bytes),
            self.output.format_size(self.total_size),
            avg_speed_mbps
        ));
    }

    /// Complete the progress tracking with final statistics
    pub fn complete(&mut self) {
        self.processed_bytes = self.total_size;
        let total_elapsed = self.start_time.elapsed();
        let avg_speed_mbps = if total_elapsed.as_secs() > 0 {
            self.total_size / total_elapsed.as_secs() / 1024 / 1024
        } else {
            0
        };

        self.output.info(&format!(
            "{} completed: {} in {:.1}s (avg {} MB/s)",
            self.operation_name,
            self.output.format_size(self.total_size),
            total_elapsed.as_secs_f64(),
            avg_speed_mbps
        ));
    }
}

/// Layer-level upload/download statistics
#[derive(Debug, Clone)]
pub struct LayerStats {
    /// Layer digest
    pub digest: String,
    /// Layer size in bytes  
    pub size: u64,
    /// Processed bytes
    pub processed_bytes: u64,
    /// Start time
    pub start_time: Option<Instant>,
    /// Completion time
    pub completion_time: Option<Instant>,
    /// Status (pending, processing, completed, failed)
    pub status: LayerStatus,
    /// Error message if failed
    pub error_message: Option<String>,
}

/// Status of a layer operation
#[derive(Debug, Clone, PartialEq)]
pub enum LayerStatus {
    Pending,
    Processing,
    Completed,
    Skipped,
    Failed,
}

impl LayerStats {
    /// Create new layer statistics
    pub fn new(digest: String, size: u64) -> Self {
        Self {
            digest,
            size,
            processed_bytes: 0,
            start_time: None,
            completion_time: None,
            status: LayerStatus::Pending,
            error_message: None,
        }
    }

    /// Start processing the layer
    pub fn start_processing(&mut self) {
        self.status = LayerStatus::Processing;
        self.start_time = Some(Instant::now());
    }

    /// Update processing progress
    pub fn update_progress(&mut self, processed_bytes: u64) {
        self.processed_bytes = processed_bytes;
    }

    /// Mark layer as completed
    pub fn complete(&mut self) {
        self.status = LayerStatus::Completed;
        self.processed_bytes = self.size;
        self.completion_time = Some(Instant::now());
    }

    /// Mark layer as skipped
    pub fn skip(&mut self) {
        self.status = LayerStatus::Skipped;
        self.completion_time = Some(Instant::now());
    }

    /// Mark layer as failed
    pub fn fail(&mut self, error: String) {
        self.status = LayerStatus::Failed;
        self.error_message = Some(error);
        self.completion_time = Some(Instant::now());
    }

    /// Get processing duration
    pub fn duration(&self) -> Option<Duration> {
        match (self.start_time, self.completion_time) {
            (Some(start), Some(end)) => Some(end.duration_since(start)),
            _ => None,
        }
    }
}

/// Comprehensive operation statistics combining progress and layer stats
#[derive(Debug, Clone)]
pub struct OperationStats {
    /// Overall progress tracker
    pub progress: ProgressTracker,
    /// Per-layer statistics
    pub layers: Vec<LayerStats>,
    /// Total number of layers
    pub total_layers: usize,
    /// Successful layers count
    pub successful_layers: usize,
    /// Skipped layers count
    pub skipped_layers: usize,
    /// Failed layers count
    pub failed_layers: usize,
}

impl OperationStats {
    /// Create new operation statistics
    pub fn new(total_size: u64, total_layers: usize, output: Logger, operation_name: String) -> Self {
        Self {
            progress: ProgressTracker::new(total_size, output, operation_name),
            layers: Vec::with_capacity(total_layers),
            total_layers,
            successful_layers: 0,
            skipped_layers: 0,
            failed_layers: 0,
        }
    }

    /// Add a layer to track
    pub fn add_layer(&mut self, digest: String, size: u64) {
        self.layers.push(LayerStats::new(digest, size));
    }

    /// Start processing a layer by digest
    pub fn start_layer(&mut self, digest: &str) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.start_processing();
        }
    }

    /// Update layer progress
    pub fn update_layer(&mut self, digest: &str, processed_bytes: u64) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.update_progress(processed_bytes);
            
            // Update overall progress
            let total_processed: u64 = self.layers.iter().map(|l| l.processed_bytes).sum();
            self.progress.update(total_processed);
        }
    }

    /// Complete a layer
    pub fn complete_layer(&mut self, digest: &str) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.complete();
            self.successful_layers += 1;
            
            // Update overall progress
            let total_processed: u64 = self.layers.iter().map(|l| l.processed_bytes).sum();
            self.progress.update(total_processed);
        }
    }

    /// Skip a layer  
    pub fn skip_layer(&mut self, digest: &str) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.skip();
            self.skipped_layers += 1;
        }
    }

    /// Fail a layer
    pub fn fail_layer(&mut self, digest: &str, error: String) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.fail(error);
            self.failed_layers += 1;
        }
    }

    /// Generate final statistics report
    pub fn final_report(&mut self) {
        self.progress.complete();
        
        let total_elapsed = self.progress.start_time.elapsed();
        self.progress.output.info(&format!(
            "Operation Summary: {}/{} layers successful, {} skipped, {} failed in {:.1}s",
            self.successful_layers,
            self.total_layers,
            self.skipped_layers,
            self.failed_layers,
            total_elapsed.as_secs_f64()
        ));
    }
}

/// Network speed regression analyzer for dynamic concurrency adjustment
#[derive(Debug, Clone)]
pub struct NetworkSpeedRegression {
    /// Historical speed measurements (timestamp, bytes/sec)
    speed_history: VecDeque<(Instant, f64)>,
    /// Maximum history length for regression analysis
    max_history_length: usize,
    /// Minimum samples needed for reliable regression
    min_samples: usize,
    /// Current regression coefficients (slope, intercept)
    regression_coefficients: Option<(f64, f64)>,
    /// Confidence level of current regression (0.0-1.0)
    confidence_level: f64,
    /// Last analysis timestamp
    last_analysis: Option<Instant>,
}

impl NetworkSpeedRegression {
    /// Create a new network speed regression analyzer
    pub fn new() -> Self {
        Self {
            speed_history: VecDeque::new(),
            max_history_length: 20, // Keep last 20 measurements
            min_samples: 5,
            regression_coefficients: None,
            confidence_level: 0.0,
            last_analysis: None,
        }
    }

    /// Add a new speed measurement
    pub fn add_measurement(&mut self, speed_bytes_per_sec: f64) {
        let now = Instant::now();
        
        // Add new measurement
        self.speed_history.push_back((now, speed_bytes_per_sec));
        
        // Limit history size
        while self.speed_history.len() > self.max_history_length {
            self.speed_history.pop_front();
        }
        
        // Perform regression analysis if we have enough samples
        if self.speed_history.len() >= self.min_samples {
            self.perform_regression_analysis();
        }
    }

    /// Get the current speed trend
    pub fn get_speed_trend(&self) -> SpeedTrend {
        if let Some((slope, _)) = self.regression_coefficients {
            if self.confidence_level > 0.7 {
                if slope > 0.1 {
                    SpeedTrend::Increasing
                } else if slope < -0.1 {
                    SpeedTrend::Decreasing
                } else {
                    SpeedTrend::Stable
                }
            } else {
                SpeedTrend::Unknown
            }
        } else {
            SpeedTrend::Unknown
        }
    }

    /// Get confidence level in current trend analysis
    pub fn get_confidence_level(&self) -> f64 {
        self.confidence_level
    }

    /// Get predicted speed based on regression
    pub fn predict_speed(&self, future_seconds: f64) -> Option<f64> {
        if let (Some((slope, intercept)), Some(base_time)) = 
            (self.regression_coefficients, self.speed_history.front().map(|(t, _)| *t)) {
            
            let elapsed_since_base = base_time.elapsed().as_secs_f64();
            let prediction_time = elapsed_since_base + future_seconds;
            Some(slope * prediction_time + intercept)
        } else {
            None
        }
    }

    /// Perform linear regression analysis on speed history
    fn perform_regression_analysis(&mut self) {
        if self.speed_history.len() < self.min_samples {
            return;
        }

        let base_time = self.speed_history.front().unwrap().0;
        let samples: Vec<(f64, f64)> = self.speed_history
            .iter()
            .map(|(timestamp, speed)| {
                let elapsed = timestamp.duration_since(base_time).as_secs_f64();
                (elapsed, *speed)
            })
            .collect();

        if let Some((slope, intercept, r_squared)) = linear_regression(&samples) {
            self.regression_coefficients = Some((slope, intercept));
            self.confidence_level = r_squared.abs(); // Use R² as confidence measure
            self.last_analysis = Some(Instant::now());
        }
    }

    /// Perform comprehensive regression analysis and return results
    pub fn analyze_performance(&self) -> Option<RegressionAnalysis> {
        if self.speed_history.len() < self.min_samples {
            return None;
        }

        if let Some((slope, intercept)) = self.regression_coefficients {
            // Calculate correlation coefficient from existing data
            let n = self.speed_history.len() as f64;
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut sum_xy = 0.0;
            let mut sum_x2 = 0.0;

            let start_time = self.speed_history.front().unwrap().0;
            for (timestamp, speed) in self.speed_history.iter() {
                let x = timestamp.duration_since(start_time).as_secs_f64();
                let y = *speed;
                sum_x += x;
                sum_y += y;
                sum_xy += x * y;
                sum_x2 += x * x;
            }

            let correlation = if n * sum_x2 - sum_x * sum_x != 0.0 {
                (n * sum_xy - sum_x * sum_y) / 
                ((n * sum_x2 - sum_x * sum_x) * (n * sum_y - sum_y * sum_y)).sqrt()
            } else {
                0.0
            };

            let predicted_speed = if let Some(predicted) = self.predict_speed(1.0) {
                predicted as u64
            } else {
                self.speed_history.back().unwrap().1 as u64
            };

            Some(RegressionAnalysis {
                slope,
                intercept,
                correlation,
                confidence: self.confidence_level,
                predicted_speed,
                sample_size: self.speed_history.len(),
            })
        } else {
            None
        }
    }

    /// Predict optimal concurrency based on performance trends
    pub fn predict_optimal_concurrency(&self, current_concurrency: usize) -> usize {
        if let Some((slope, _)) = self.regression_coefficients {
            if self.confidence_level > 0.6 {
                if slope < -0.2 {
                    // Performance declining - reduce concurrency
                    std::cmp::max(1, current_concurrency * 2 / 3)
                } else if slope > 0.2 {
                    // Performance improving - can increase concurrency
                    std::cmp::min(16, current_concurrency + 2)
                } else {
                    // Stable performance - maintain current level
                    current_concurrency
                }
            } else {
                // Low confidence - maintain current level
                current_concurrency
            }
        } else {
            current_concurrency
        }
    }
}

/// Enhanced performance monitor with network speed regression
#[derive(Debug)]
pub struct PerformanceMonitor {
    /// Speed regression analyzer
    speed_regression: NetworkSpeedRegression,
    /// Performance statistics
    statistics: PerformanceStatistics,
    /// Start time for monitoring session
    start_time: Instant,
    /// Last update timestamp
    last_update: Instant,
    /// Bytes transferred in current session
    total_bytes_transferred: u64,
    /// Current transfer speed (bytes/sec)
    current_speed: f64,
    /// Peak transfer speed seen
    peak_speed: f64,
    /// Speed measurements for moving average
    speed_window: VecDeque<f64>,
    /// Window size for speed averaging
    speed_window_size: usize,
}

impl PerformanceMonitor {
    /// Create a new performance monitor
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            speed_regression: NetworkSpeedRegression::new(),
            statistics: PerformanceStatistics {
                current_speed: 0,
                average_speed: 0,
                peak_speed: 0,
                total_bytes: 0,
                total_time: Duration::from_secs(0),
                trend: SpeedTrend::Unknown,
                trend_confidence: 0.0,
            },
            start_time: now,
            last_update: now,
            total_bytes_transferred: 0,
            current_speed: 0.0,
            peak_speed: 0.0,
            speed_window: VecDeque::new(),
            speed_window_size: 10,
        }
    }

    /// Update performance metrics with new transfer data
    pub fn update_metrics(&mut self, bytes_transferred: u64, _elapsed: Duration) {
        let now = Instant::now();
        let delta_time = now.duration_since(self.last_update).as_secs_f64();
        
        if delta_time > 0.0 {
            // Calculate current speed
            self.current_speed = bytes_transferred as f64 / delta_time;
            
            // Update peak speed
            if self.current_speed > self.peak_speed {
                self.peak_speed = self.current_speed;
            }
            
            // Add to speed window for moving average
            self.speed_window.push_back(self.current_speed);
            while self.speed_window.len() > self.speed_window_size {
                self.speed_window.pop_front();
            }
            
            // Update regression analyzer
            self.speed_regression.add_measurement(self.current_speed);
            
            // Update total bytes
            self.total_bytes_transferred += bytes_transferred;
            
            // Calculate average speed
            let total_elapsed = now.duration_since(self.start_time).as_secs_f64();
            let average_speed = if total_elapsed > 0.0 {
                self.total_bytes_transferred as f64 / total_elapsed
            } else {
                0.0
            };
            
            // Update statistics
            self.statistics = PerformanceStatistics {
                current_speed: self.current_speed as u64,
                average_speed: average_speed as u64,
                peak_speed: self.peak_speed as u64,
                total_bytes: self.total_bytes_transferred,
                total_time: now.duration_since(self.start_time),
                trend: self.speed_regression.get_speed_trend(),
                trend_confidence: self.speed_regression.get_confidence_level(),
            };
            
            self.last_update = now;
        }
    }

    /// Get current performance statistics
    pub fn get_statistics(&self) -> &PerformanceStatistics {
        &self.statistics
    }

    /// Get network speed in MB/s
    pub fn get_speed_mbps(&self) -> f64 {
        self.current_speed / (1024.0 * 1024.0)
    }

    /// Get average speed in MB/s
    pub fn get_average_speed_mbps(&self) -> f64 {
        self.statistics.average_speed as f64 / (1024.0 * 1024.0)
    }

    /// Get current speed in bytes per second
    pub fn get_current_speed(&self) -> u64 {
        self.current_speed as u64
    }

    /// Get average speed in bytes per second
    pub fn get_average_speed(&self) -> u64 {
        self.statistics.average_speed
    }

    /// Get regression analysis results
    pub fn get_regression_analysis(&self) -> Option<RegressionAnalysis> {
        self.speed_regression.analyze_performance()
    }

    /// Get optimal concurrency prediction based on performance analysis
    pub fn get_optimal_concurrency(&self, current_concurrency: usize) -> usize {
        self.speed_regression.predict_optimal_concurrency(current_concurrency)
    }

    /// Record a transfer operation for performance tracking
    pub fn record_transfer(&mut self, bytes_transferred: u64, elapsed: Duration) {
        self.update_metrics(bytes_transferred, elapsed);
    }

    /// Get performance statistics (alias for get_statistics)
    pub fn get_performance_statistics(&self) -> &PerformanceStatistics {
        self.get_statistics()
    }

    /// Analyze current performance and provide detailed analysis
    pub fn analyze_performance(&self) -> PerformanceAnalysis {
        let regression = self.get_regression_analysis();
        let confidence = self.statistics.trend_confidence;
        let predicted_speed = if let Some(ref reg) = regression {
            reg.predicted_speed as f64
        } else {
            self.current_speed
        };

        PerformanceAnalysis {
            current_speed: self.current_speed,
            average_speed: self.statistics.average_speed as f64,
            peak_speed: self.peak_speed,
            trend: self.statistics.trend.clone(),
            trend_confidence: self.statistics.trend_confidence,
            monitoring_duration: self.start_time.elapsed(),
            total_bytes_transferred: self.total_bytes_transferred,
            active_transfers: 1, // TODO: Track actual active transfers
            regression_analysis: regression,
            confidence,
            predicted_speed,
            performance_snapshot: self.statistics.clone(),
        }
    }

    /// Check if performance is stable enough for concurrency adjustments
    pub fn is_performance_stable(&self) -> bool {
        self.statistics.trend_confidence > 0.6
    }

    /// Get performance prediction with confidence level
    pub fn predict_performance(&self, target_concurrency: usize) -> (f64, f64) {
        // Returns (predicted_speed, confidence)
        let analysis = self.speed_regression.analyze_performance();
        if let Some(analysis) = analysis {
            let speed_factor = target_concurrency as f64 / 
                              std::cmp::max(1, self.speed_regression.speed_history.len()) as f64;
            let predicted_speed = analysis.predicted_speed as f64 * speed_factor.sqrt();
            (predicted_speed, analysis.confidence)
        } else {
            (self.current_speed, 0.3)
        }
    }

    /// Reset monitoring session
    pub fn reset(&mut self) {
        let now = Instant::now();
        self.start_time = now;
        self.last_update = now;
        self.total_bytes_transferred = 0;
        self.current_speed = 0.0;
        self.peak_speed = 0.0;
        self.speed_window.clear();
        self.speed_regression = NetworkSpeedRegression::new();
    }
}

/// Active task information for progress display
#[derive(Debug, Clone)]
pub struct ActiveTaskInfo {
    /// Task identifier
    pub task_id: String,
    /// Task type (upload, download, etc.)
    pub task_type: String,
    /// Layer index in the operation
    pub layer_index: usize,
    /// Total size of the layer
    pub layer_size: u64,
    /// Currently processed bytes
    pub processed_bytes: u64,
    /// Currently processed bytes (alias for compatibility)
    pub bytes_processed: u64,
    /// Task start time
    pub start_time: Instant,
    /// Task priority level
    pub priority: u64,
    /// Estimated completion time
    pub estimated_completion: Option<Instant>,
}

/// Performance analysis result from monitor
#[derive(Debug, Clone)]
pub struct PerformanceAnalysis {
    /// Current network speed in bytes per second
    pub current_speed: f64,
    /// Average speed over the monitoring period
    pub average_speed: f64,
    /// Peak speed achieved
    pub peak_speed: f64,
    /// Speed trend direction
    pub trend: SpeedTrend,
    /// Confidence in trend analysis (0.0-1.0)
    pub trend_confidence: f64,
    /// Total monitoring duration
    pub monitoring_duration: Duration,
    /// Total bytes transferred
    pub total_bytes_transferred: u64,
    /// Number of active transfers
    pub active_transfers: usize,
    /// Regression analysis result if available
    pub regression_analysis: Option<RegressionAnalysis>,
    /// Confidence level in the analysis (0.0-1.0)
    pub confidence: f64,
    /// Predicted speed for future operations
    pub predicted_speed: f64,
    /// Performance snapshot at analysis time
    pub performance_snapshot: PerformanceStatistics,
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

/// Simple linear regression implementation
/// Returns (slope, intercept, r_squared) if successful
fn linear_regression(samples: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = samples.len() as f64;
    if n < 2.0 {
        return None;
    }

    let sum_x: f64 = samples.iter().map(|(x, _)| x).sum();
    let sum_y: f64 = samples.iter().map(|(_, y)| y).sum();
    let sum_xy: f64 = samples.iter().map(|(x, y)| x * y).sum();
    let sum_x2: f64 = samples.iter().map(|(x, _)| x * x).sum();

    let denominator = n * sum_x2 - sum_x * sum_x;
    if denominator.abs() < f64::EPSILON {
        return None;
    }

    let slope = (n * sum_xy - sum_x * sum_y) / denominator;
    let intercept = (sum_y - slope * sum_x) / n;

    // Calculate R-squared
    let mean_y = sum_y / n;
    let ss_tot: f64 = samples.iter().map(|(_, y)| (y - mean_y).powi(2)).sum();
    let ss_res: f64 = samples.iter().map(|(x, y)| (y - (slope * x + intercept)).powi(2)).sum();
    
    let r_squared = if ss_tot.abs() < f64::EPSILON {
        0.0
    } else {
        1.0 - (ss_res / ss_tot)
    };

    Some((slope, intercept, r_squared))
}
