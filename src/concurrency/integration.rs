//! Integration bridge between concurrency monitoring and registry operations
//!
//! This module provides seamless integration between the advanced concurrency
//! monitoring capabilities and the registry's unified pipeline, ensuring that
//! network speed regression analysis and dynamic concurrency adjustments work
//! together efficiently.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::concurrency::{
    monitor::{PerformanceMonitor, RegressionAnalysis},
    PipelineProgress,
};
use crate::registry::{EnhancedConcurrencyStats, NetworkSpeedStats, PerformancePrediction};
use crate::logging::Logger;

/// Bridge for integrating performance monitoring with pipeline operations
pub struct PipelineMonitorBridge {
    performance_monitor: Arc<Mutex<PerformanceMonitor>>,
    logger: Logger,
    last_update: Instant,
    update_interval: Duration,
}

impl PipelineMonitorBridge {
    /// Create a new pipeline monitor bridge
    pub fn new(logger: Logger) -> Self {
        Self {
            performance_monitor: Arc::new(Mutex::new(PerformanceMonitor::new())),
            logger,
            last_update: Instant::now(),
            update_interval: Duration::from_millis(500),
        }
    }

    /// Update performance metrics with new transfer data
    pub fn update_performance(&mut self, bytes_transferred: u64, elapsed: Duration) {
        if let Ok(mut monitor) = self.performance_monitor.lock() {
            monitor.update_metrics(bytes_transferred, elapsed);
        }
    }

    /// Get current regression analysis
    pub fn get_regression_analysis(&self) -> Option<RegressionAnalysis> {
        if let Ok(monitor) = self.performance_monitor.lock() {
            monitor.get_regression_analysis()
        } else {
            None
        }
    }

    /// Get current network speed in MB/s
    pub fn get_current_speed_mbps(&self) -> f64 {
        if let Ok(monitor) = self.performance_monitor.lock() {
            monitor.get_speed_mbps()
        } else {
            0.0
        }
    }

    /// Get average network speed in MB/s
    pub fn get_average_speed_mbps(&self) -> f64 {
        if let Ok(monitor) = self.performance_monitor.lock() {
            monitor.get_average_speed_mbps()
        } else {
            0.0
        }
    }

    /// Create enhanced concurrency stats with regression data
    pub fn create_enhanced_stats_with_regression(
        &self,
        pipeline_progress: &PipelineProgress,
        max_concurrent: usize,
        small_blob_threshold: u64,
    ) -> EnhancedConcurrencyStats {
        let regression_analysis = self.get_regression_analysis();
        let current_speed = self.get_current_speed_mbps();
        let average_speed = self.get_average_speed_mbps();

        // Generate speed trend based on regression
        let speed_trend = if let Some(ref regression) = regression_analysis {
            if regression.slope > 0.5 {
                "📈 Rapidly increasing".to_string()
            } else if regression.slope > 0.1 {
                "📊 Gradually increasing".to_string()
            } else if regression.slope < -0.5 {
                "📉 Rapidly decreasing".to_string()
            } else if regression.slope < -0.1 {
                "📊 Gradually decreasing".to_string()
            } else {
                "➡️ Stable".to_string()
            }
        } else {
            "🔍 Analyzing...".to_string()
        };

        // Bottleneck analysis with regression insights
        let bottleneck_analysis = if let Some(ref regression) = regression_analysis {
            if regression.confidence > 0.7 {
                if regression.slope < -0.2 {
                    "Network performance declining - primary bottleneck identified".to_string()
                } else if regression.slope > 0.2 {
                    "Network performance improving - concurrency not limiting factor".to_string()
                } else if current_speed < 5.0 {
                    "Network bandwidth appears to be primary limitation".to_string()
                } else if pipeline_progress.active_tasks as f64 / max_concurrent as f64 > 0.9 {
                    "High concurrency utilization - may be reaching optimal point".to_string()
                } else {
                    "System running at optimal configuration".to_string()
                }
            } else {
                "Network performance unstable - analysis confidence low".to_string()
            }
        } else {
            "Insufficient data for bottleneck analysis".to_string()
        };

        // Confidence level based on regression quality
        let confidence_level = if let Some(ref regression) = regression_analysis {
            regression.confidence
        } else {
            0.3 // Low confidence without regression data
        };

        // Generate enhanced stats
        EnhancedConcurrencyStats {
            current_parallel_tasks: pipeline_progress.active_tasks,
            max_parallel_tasks: max_concurrent,
            scheduling_strategy: format!("Size-based priority (small <{})", 
                                       Logger::new(false).format_size(small_blob_threshold)),
            priority_queue_status: crate::registry::PriorityQueueStatus {
                high_priority_remaining: pipeline_progress.queued_tasks / 3,
                medium_priority_remaining: pipeline_progress.queued_tasks / 3,
                low_priority_remaining: pipeline_progress.queued_tasks - (2 * pipeline_progress.queued_tasks / 3),
                current_batch_strategy: "Regression-aware adaptive scheduling".to_string(),
            },
            network_speed_measurement: NetworkSpeedStats {
                current_speed_mbps: current_speed,
                average_speed_mbps: average_speed,
                speed_trend,
                auto_adjustment_enabled: true,
            },
            dynamic_adjustments: self.generate_adjustment_recommendations(
                pipeline_progress, max_concurrent, &regression_analysis
            ),
            performance_prediction: PerformancePrediction {
                estimated_completion_time: self.estimate_completion_time(pipeline_progress),
                confidence_level,
                bottleneck_analysis,
            },
        }
    }

    /// Generate concurrency adjustment recommendations based on regression
    fn generate_adjustment_recommendations(
        &self,
        pipeline_progress: &PipelineProgress,
        max_concurrent: usize,
        regression_analysis: &Option<RegressionAnalysis>,
    ) -> Vec<crate::registry::ConcurrencyAdjustmentRecord> {
        let mut adjustments = Vec::new();

        if let Some(regression) = regression_analysis {
            if regression.confidence > 0.6 {
                let current_speed = self.get_current_speed_mbps();
                
                // Recommend reduction if speed is declining
                if regression.slope < -0.3 && current_speed < 10.0 {
                    let recommended = std::cmp::max(1, max_concurrent / 2);
                    adjustments.push(crate::registry::ConcurrencyAdjustmentRecord {
                        timestamp: Instant::now(),
                        old_concurrency: pipeline_progress.active_tasks,
                        new_concurrency: recommended,
                        reason: format!("Speed declining (slope: {:.3})", regression.slope),
                        performance_impact: 20.0,
                    });
                }
                
                // Recommend increase if speed is improving and utilization is low
                else if regression.slope > 0.2 && current_speed > 20.0 {
                    let utilization = pipeline_progress.active_tasks as f64 / max_concurrent as f64;
                    if utilization < 0.8 {
                        let recommended = std::cmp::min(max_concurrent, pipeline_progress.active_tasks + 2);
                        adjustments.push(crate::registry::ConcurrencyAdjustmentRecord {
                            timestamp: Instant::now(),
                            old_concurrency: pipeline_progress.active_tasks,
                            new_concurrency: recommended,
                            reason: format!("Speed improving (slope: {:.3})", regression.slope),
                            performance_impact: 15.0,
                        });
                    }
                }
            }
        }

        adjustments
    }

    /// Estimate completion time based on current progress and speed trends
    fn estimate_completion_time(&self, pipeline_progress: &PipelineProgress) -> Duration {
        if pipeline_progress.completed_tasks == 0 {
            return Duration::from_secs(300); // Default 5 minutes
        }

        let remaining_tasks = pipeline_progress.total_tasks - pipeline_progress.completed_tasks;
        if remaining_tasks == 0 {
            return Duration::from_secs(0);
        }

        // Use regression analysis to improve estimation if available
        if let Some(regression) = self.get_regression_analysis() {
            if regression.confidence > 0.5 {
                let predicted_speed_mbps = regression.predicted_speed as f64 / (1024.0 * 1024.0);
                if predicted_speed_mbps > 0.0 {
                    // Rough estimation based on predicted speed
                    let estimated_seconds = (remaining_tasks as f64 * 10.0) / predicted_speed_mbps;
                    return Duration::from_secs_f64(estimated_seconds.max(1.0));
                }
            }
        }

        // Fallback to simple linear projection
        let current_speed = self.get_current_speed_mbps();
        if current_speed > 0.0 {
            let estimated_seconds = (remaining_tasks as f64 * 8.0) / current_speed;
            Duration::from_secs_f64(estimated_seconds.max(1.0))
        } else {
            Duration::from_secs(300)
        }
    }

    /// Display enhanced progress with regression integration
    pub fn display_enhanced_progress(&self, pipeline_progress: &PipelineProgress, config: &crate::registry::PipelineConfig) {
        if self.last_update.elapsed() < self.update_interval {
            return; // Rate limiting
        }

        let enhanced_stats = self.create_enhanced_stats_with_regression(
            pipeline_progress,
            config.max_concurrent,
            config.small_blob_threshold,
        );

        // Display live progress
        self.logger.display_unified_pipeline_progress(pipeline_progress, &enhanced_stats);

        // Display detailed analysis in verbose mode
        if self.logger.verbose && pipeline_progress.completed_tasks > 3 {
            self.logger.display_detailed_unified_progress(pipeline_progress, &enhanced_stats);
            
            // Show regression analysis if we have sufficient data
            if let Some(regression) = self.get_regression_analysis() {
                self.logger.display_network_regression_analysis(
                    enhanced_stats.network_speed_measurement.current_speed_mbps,
                    enhanced_stats.network_speed_measurement.average_speed_mbps,
                    &enhanced_stats.network_speed_measurement.speed_trend,
                    Some(&regression),
                );
            }
        }
    }

    /// Update the last update timestamp (for rate limiting)
    pub fn update_timestamp(&mut self) {
        self.last_update = Instant::now();
    }
}
