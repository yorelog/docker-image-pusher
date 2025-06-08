//! Network speed regression analysis for dynamic concurrency optimization

use std::collections::VecDeque;
use std::time::Instant;
use crate::concurrency::SpeedTrend;

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

    /// Expose speed history for external analysis
    pub fn speed_history(&self) -> &VecDeque<(Instant, f64)> {
        &self.speed_history
    }
}

/// Simple linear regression implementation
/// Returns (slope, intercept, r_squared) if successful
pub fn linear_regression(samples: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
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
