//! Performance analysis and regression algorithms
//! 
//! This module provides statistical analysis capabilities for predicting performance trends
//! and making informed decisions about concurrency adjustments.

use std::time::Instant;
use serde::{Deserialize, Serialize};

/// A data point representing performance at a specific time
#[derive(Debug, Clone)]
pub struct SpeedDataPoint {
    /// When this measurement was taken
    pub timestamp: Instant,
    /// Number of bytes transferred in this measurement
    pub bytes_transferred: u64,
    /// Transfer speed in MB/s
    pub speed_mbps: f64,
    /// Number of concurrent operations at this time
    pub concurrent_count: usize,
}

impl SpeedDataPoint {
    /// Create a new speed data point
    pub fn new(bytes_transferred: u64, speed_mbps: f64, concurrent_count: usize) -> Self {
        Self {
            timestamp: Instant::now(),
            bytes_transferred,
            speed_mbps,
            concurrent_count,
        }
    }

    /// Get the age of this data point in seconds
    pub fn age_seconds(&self) -> f64 {
        Instant::now().duration_since(self.timestamp).as_secs_f64()
    }

    /// Calculate time-based weight (newer data has higher weight)
    pub fn time_weight(&self, decay_rate: f64) -> f64 {
        let age = self.age_seconds();
        1.0 / (1.0 + age / decay_rate)
    }
}

/// Result of regression analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionAnalysis {
    /// Predicted speed for next operation
    pub predicted_speed: f64,
    /// Confidence in the prediction (0.0 to 1.0)
    pub confidence: f64,
    /// Identified trend in the data
    pub trend: SpeedTrend,
}

/// Identified trend in performance data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeedTrend {
    /// Speed is generally increasing
    Increasing,
    /// Speed is generally decreasing  
    Decreasing,
    /// Speed is relatively stable
    Stable,
}

impl SpeedTrend {
    /// Get a human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            SpeedTrend::Increasing => "Speed is improving",
            SpeedTrend::Decreasing => "Speed is declining",
            SpeedTrend::Stable => "Speed is stable",
        }
    }
}

/// Performance analyzer that uses statistical methods to predict trends
#[derive(Debug)]
pub struct PerformanceAnalyzer {
    /// Maximum number of data points to keep
    max_history_size: usize,
    /// Time decay rate for weighting (seconds)
    time_decay_rate: f64,
    /// Minimum confidence threshold for trend detection
    min_confidence_threshold: f64,
    /// Slope threshold for trend classification
    trend_slope_threshold: f64,
}

impl PerformanceAnalyzer {
    /// Create a new performance analyzer with default settings
    pub fn new() -> Self {
        Self {
            max_history_size: 20,
            time_decay_rate: 60.0, // 1 minute decay
            min_confidence_threshold: 0.3,
            trend_slope_threshold: 0.5,
        }
    }

    /// Create a custom performance analyzer
    pub fn with_config(
        max_history_size: usize,
        time_decay_rate: f64,
        min_confidence_threshold: f64,
        trend_slope_threshold: f64,
    ) -> Self {
        Self {
            max_history_size,
            time_decay_rate,
            min_confidence_threshold,
            trend_slope_threshold,
        }
    }

    /// Perform weighted linear regression analysis on speed data
    pub fn analyze(&self, history: &[SpeedDataPoint], max_concurrent: usize) -> RegressionAnalysis {
        if history.len() < 3 {
            return RegressionAnalysis {
                predicted_speed: 0.0,
                confidence: 0.0,
                trend: SpeedTrend::Stable,
            };
        }

        // Create weighted data points
        let weighted_points: Vec<(f64, f64, f64)> = history.iter().enumerate()
            .map(|(i, point)| {
                let time_weight = point.time_weight(self.time_decay_rate);
                let concurrent_factor = point.concurrent_count as f64 / max_concurrent as f64;
                let combined_weight = time_weight * concurrent_factor;
                (i as f64, point.speed_mbps, combined_weight)
            })
            .collect();

        // Perform weighted linear regression
        let regression_result = self.weighted_linear_regression(&weighted_points);
        
        // Calculate data reliability factor
        let data_reliability = self.calculate_data_reliability(history);
        
        // Sample size factor
        let sample_size_factor = (history.len() as f64 / 10.0).min(1.0);
        
        // Combine confidence factors
        let final_confidence = regression_result.r_squared * data_reliability * sample_size_factor;
        
        // Determine trend
        let trend = self.classify_trend(regression_result.slope);
        
        // Predict next speed
        let next_x = history.len() as f64;
        let predicted_speed = (regression_result.slope * next_x + regression_result.intercept).max(0.0);

        RegressionAnalysis {
            predicted_speed,
            confidence: final_confidence.max(0.0).min(1.0),
            trend,
        }
    }

    /// Weighted linear regression implementation
    fn weighted_linear_regression(&self, points: &[(f64, f64, f64)]) -> LinearRegressionResult {
        if points.is_empty() {
            return LinearRegressionResult::default();
        }

        let x_values: Vec<f64> = points.iter().map(|(x, _, _)| *x).collect();
        let y_values: Vec<f64> = points.iter().map(|(_, y, _)| *y).collect();
        let weights: Vec<f64> = points.iter().map(|(_, _, w)| *w).collect();

        // Calculate weighted means
        let weight_sum: f64 = weights.iter().sum();
        if weight_sum == 0.0 {
            return LinearRegressionResult::default();
        }

        let x_mean: f64 = x_values.iter().zip(weights.iter())
            .map(|(x, w)| x * w).sum::<f64>() / weight_sum;
        let y_mean: f64 = y_values.iter().zip(weights.iter())
            .map(|(y, w)| y * w).sum::<f64>() / weight_sum;

        // Calculate weighted regression coefficients
        let numerator: f64 = x_values.iter().zip(y_values.iter()).zip(weights.iter())
            .map(|((x, y), w)| w * (x - x_mean) * (y - y_mean))
            .sum();
        
        let denominator: f64 = x_values.iter().zip(weights.iter())
            .map(|(x, w)| w * (x - x_mean).powi(2))
            .sum();

        let slope = if denominator != 0.0 { numerator / denominator } else { 0.0 };
        let intercept = y_mean - slope * x_mean;

        // Calculate R-squared for confidence
        let ss_res: f64 = x_values.iter().zip(y_values.iter()).zip(weights.iter())
            .map(|((x, y), w)| {
                let predicted = slope * x + intercept;
                w * (y - predicted).powi(2)
            })
            .sum();

        let ss_tot: f64 = y_values.iter().zip(weights.iter())
            .map(|(y, w)| w * (y - y_mean).powi(2))
            .sum();

        let r_squared = if ss_tot != 0.0 { 1.0 - (ss_res / ss_tot) } else { 0.0 };

        LinearRegressionResult {
            slope,
            intercept,
            r_squared: r_squared.max(0.0).min(1.0),
        }
    }

    /// Calculate data reliability based on transfer volumes
    fn calculate_data_reliability(&self, history: &[SpeedDataPoint]) -> f64 {
        if history.is_empty() {
            return 0.0;
        }

        let total_bytes: u64 = history.iter().map(|p| p.bytes_transferred).sum();
        let avg_bytes_per_point = total_bytes as f64 / history.len() as f64;
        
        // Normalize to MB and cap at 1.0
        (avg_bytes_per_point / (1024.0 * 1024.0)).min(1.0)
    }

    /// Classify trend based on slope
    fn classify_trend(&self, slope: f64) -> SpeedTrend {
        if slope > self.trend_slope_threshold {
            SpeedTrend::Increasing
        } else if slope < -self.trend_slope_threshold {
            SpeedTrend::Decreasing
        } else {
            SpeedTrend::Stable
        }
    }

    /// Update analyzer configuration
    pub fn set_max_history_size(&mut self, size: usize) {
        self.max_history_size = size;
    }

    pub fn set_time_decay_rate(&mut self, rate: f64) {
        self.time_decay_rate = rate;
    }

    pub fn set_min_confidence_threshold(&mut self, threshold: f64) {
        self.min_confidence_threshold = threshold;
    }

    pub fn set_trend_slope_threshold(&mut self, threshold: f64) {
        self.trend_slope_threshold = threshold;
    }

    /// Get analyzer configuration
    pub fn config(&self) -> AnalyzerConfig {
        AnalyzerConfig {
            max_history_size: self.max_history_size,
            time_decay_rate: self.time_decay_rate,
            min_confidence_threshold: self.min_confidence_threshold,
            trend_slope_threshold: self.trend_slope_threshold,
        }
    }
}

impl Default for PerformanceAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of linear regression calculation
#[derive(Debug, Default)]
struct LinearRegressionResult {
    slope: f64,
    intercept: f64,
    r_squared: f64,
}

/// Configuration for the performance analyzer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerConfig {
    pub max_history_size: usize,
    pub time_decay_rate: f64,
    pub min_confidence_threshold: f64,
    pub trend_slope_threshold: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_speed_data_point() {
        let point = SpeedDataPoint::new(1024 * 1024, 5.0, 2);
        assert_eq!(point.bytes_transferred, 1024 * 1024);
        assert_eq!(point.speed_mbps, 5.0);
        assert_eq!(point.concurrent_count, 2);
        assert!(point.age_seconds() >= 0.0);
    }

    #[test]
    fn test_time_weight() {
        let point = SpeedDataPoint::new(1024, 5.0, 1);
        let weight1 = point.time_weight(60.0);
        
        thread::sleep(Duration::from_millis(100));
        let weight2 = point.time_weight(60.0);
        
        assert!(weight1 > weight2); // Newer should have higher weight
    }

    #[test]
    fn test_performance_analyzer() {
        let analyzer = PerformanceAnalyzer::new();
        
        // Create sample data with increasing trend
        let mut history = Vec::new();
        for i in 1..=10 {
            let mut point = SpeedDataPoint::new(1024 * 1024 * i as u64, i as f64, 2);
            // Simulate time progression
            point.timestamp = Instant::now() - Duration::from_secs((10 - i) as u64);
            history.push(point);
        }

        let analysis = analyzer.analyze(&history, 4);
        assert!(analysis.confidence > 0.0);
        assert_eq!(analysis.trend, SpeedTrend::Increasing);
        assert!(analysis.predicted_speed > 0.0);
    }

    #[test]
    fn test_insufficient_data() {
        let analyzer = PerformanceAnalyzer::new();
        let history = vec![
            SpeedDataPoint::new(1024, 1.0, 1),
            SpeedDataPoint::new(1024, 1.0, 1),
        ];

        let analysis = analyzer.analyze(&history, 4);
        assert_eq!(analysis.confidence, 0.0);
        assert_eq!(analysis.trend, SpeedTrend::Stable);
    }

    #[test]
    fn test_trend_classification() {
        let analyzer = PerformanceAnalyzer::new();
        
        assert_eq!(analyzer.classify_trend(1.0), SpeedTrend::Increasing);
        assert_eq!(analyzer.classify_trend(-1.0), SpeedTrend::Decreasing);
        assert_eq!(analyzer.classify_trend(0.1), SpeedTrend::Stable);
    }

    #[test]
    fn test_data_reliability() {
        let analyzer = PerformanceAnalyzer::new();
        
        // High volume data should have higher reliability
        let high_volume = vec![
            SpeedDataPoint::new(10 * 1024 * 1024, 5.0, 2), // 10MB
            SpeedDataPoint::new(10 * 1024 * 1024, 6.0, 2),
        ];
        
        let low_volume = vec![
            SpeedDataPoint::new(1024, 5.0, 2), // 1KB
            SpeedDataPoint::new(1024, 6.0, 2),
        ];
        
        let high_reliability = analyzer.calculate_data_reliability(&high_volume);
        let low_reliability = analyzer.calculate_data_reliability(&low_volume);
        
        assert!(high_reliability > low_reliability);
    }
}
