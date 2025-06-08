//! Performance monitoring and analysis for transfer operations

use std::collections::VecDeque;
use std::time::{Duration, Instant};
use crate::concurrency::{SpeedTrend, PerformanceStatistics};
use super::regression::{NetworkSpeedRegression, RegressionAnalysis};

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
                              std::cmp::max(1, self.speed_regression.speed_history().len()) as f64;
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
