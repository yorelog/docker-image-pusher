//! Concurrency Configuration Module
//!
//! This module provides comprehensive configuration structures and validation
//! for the advanced concurrency management system. It supports dynamic
//! adjustment, performance monitoring, and intelligent strategy selection.
//!
//! ## Key Features
//! - Hierarchical configuration structure
//! - Built-in validation and constraints
//! - Predefined optimized configurations
//! - Flexible strategy and monitoring settings

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Comprehensive concurrency configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyConfig {
    /// Basic concurrency limits
    pub limits: ConcurrencyLimits,
    /// Dynamic adjustment settings
    pub dynamic: DynamicSettings,
    /// Performance monitoring configuration
    pub monitoring: MonitoringConfig,
    /// Strategy selection parameters
    pub strategy: StrategyConfig,
}

/// Basic concurrency limits and thresholds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyLimits {
    /// Absolute minimum concurrency (safety limit)
    pub min_concurrent: usize,
    /// Absolute maximum concurrency (safety limit)
    pub max_concurrent: usize,
    /// Initial concurrency for small files
    pub small_file_concurrent: usize,
    /// Initial concurrency for medium files
    pub medium_file_concurrent: usize,
    /// Initial concurrency for large files
    pub large_file_concurrent: usize,
    /// File size thresholds for categorization
    pub small_blob_threshold: u64,
    pub medium_blob_threshold: u64,
    pub large_blob_threshold: u64,
}

/// Dynamic concurrency adjustment settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSettings {
    /// Enable dynamic concurrency adjustment
    pub enabled: bool,
    /// Minimum time between adjustments
    pub adjustment_interval: Duration,
    /// Speed threshold for triggering adjustments (bytes/sec)
    pub speed_threshold: u64,
    /// Adjustment aggressiveness factor (0.1 = conservative, 2.0 = aggressive)
    pub adjustment_factor: f64,
    /// Maximum adjustment per interval (absolute change)
    pub max_adjustment_step: usize,
    /// Minimum samples required before adjustment
    pub min_samples_for_adjustment: usize,
}

/// Performance monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Enable performance monitoring
    pub enabled: bool,
    /// Sampling interval for performance metrics
    pub sample_interval: Duration,
    /// Maximum number of samples to retain
    pub max_sample_history: usize,
    /// Minimum confidence level for trend analysis (0.0-1.0)
    pub min_confidence_threshold: f64,
    /// Enable regression analysis for trend prediction
    pub enable_regression_analysis: bool,
    /// Time window for speed calculations
    pub speed_calculation_window: Duration,
}

/// Strategy selection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Default strategy to use
    pub default_strategy: String,
    /// Enable automatic strategy switching
    pub auto_strategy_switching: bool,
    /// Confidence threshold for strategy changes (0.0-1.0)
    pub strategy_switch_confidence: f64,
    /// Minimum time between strategy changes
    pub strategy_switch_cooldown: Duration,
    /// Strategy-specific parameters
    pub strategy_parameters: std::collections::HashMap<String, serde_json::Value>,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            limits: ConcurrencyLimits::default(),
            dynamic: DynamicSettings::default(),
            monitoring: MonitoringConfig::default(),
            strategy: StrategyConfig::default(),
        }
    }
}

impl Default for ConcurrencyLimits {
    fn default() -> Self {
        Self {
            min_concurrent: 1,
            max_concurrent: 8,
            small_file_concurrent: 6,
            medium_file_concurrent: 4,
            large_file_concurrent: 2,
            small_blob_threshold: 10 * 1024 * 1024,   // 10MB
            medium_blob_threshold: 100 * 1024 * 1024, // 100MB
            large_blob_threshold: 500 * 1024 * 1024,  // 500MB
        }
    }
}

impl Default for DynamicSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            adjustment_interval: Duration::from_secs(5),
            speed_threshold: 10 * 1024 * 1024, // 10 MB/s
            adjustment_factor: 1.3,
            max_adjustment_step: 2,
            min_samples_for_adjustment: 3,
        }
    }
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sample_interval: Duration::from_millis(500),
            max_sample_history: 50,
            min_confidence_threshold: 0.6,
            enable_regression_analysis: true,
            speed_calculation_window: Duration::from_secs(10),
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            default_strategy: "adaptive".to_string(),
            auto_strategy_switching: true,
            strategy_switch_confidence: 0.75,
            strategy_switch_cooldown: Duration::from_secs(10),
            strategy_parameters: std::collections::HashMap::new(),
        }
    }
}

impl ConcurrencyConfig {
    /// Validate configuration parameters
    pub fn validate(&self) -> Result<(), String> {
        // Validate limits
        if self.limits.min_concurrent == 0 {
            return Err("min_concurrent must be greater than 0".to_string());
        }
        
        if self.limits.max_concurrent < self.limits.min_concurrent {
            return Err("max_concurrent must be >= min_concurrent".to_string());
        }
        
        if self.limits.small_file_concurrent > self.limits.max_concurrent {
            return Err("small_file_concurrent must be <= max_concurrent".to_string());
        }
        
        // Validate thresholds
        if self.limits.small_blob_threshold >= self.limits.medium_blob_threshold {
            return Err("small_blob_threshold must be < medium_blob_threshold".to_string());
        }
        
        if self.limits.medium_blob_threshold >= self.limits.large_blob_threshold {
            return Err("medium_blob_threshold must be < large_blob_threshold".to_string());
        }
        
        // Validate dynamic settings
        if self.dynamic.adjustment_factor <= 0.0 {
            return Err("adjustment_factor must be positive".to_string());
        }
        
        if self.dynamic.max_adjustment_step == 0 {
            return Err("max_adjustment_step must be greater than 0".to_string());
        }
        
        // Validate monitoring
        if self.monitoring.min_confidence_threshold < 0.0 || self.monitoring.min_confidence_threshold > 1.0 {
            return Err("min_confidence_threshold must be between 0.0 and 1.0".to_string());
        }
        
        // Validate strategy
        if self.strategy.strategy_switch_confidence < 0.0 || self.strategy.strategy_switch_confidence > 1.0 {
            return Err("strategy_switch_confidence must be between 0.0 and 1.0".to_string());
        }
        
        Ok(())
    }
    
    /// Create a configuration optimized for small files
    pub fn for_small_files() -> Self {
        let mut config = Self::default();
        config.limits.small_file_concurrent = 8;
        config.limits.medium_file_concurrent = 6;
        config.limits.large_file_concurrent = 4;
        config.dynamic.adjustment_factor = 1.5;
        config
    }
    
    /// Create a configuration optimized for large files
    pub fn for_large_files() -> Self {
        let mut config = Self::default();
        config.limits.small_file_concurrent = 4;
        config.limits.medium_file_concurrent = 3;
        config.limits.large_file_concurrent = 2;
        config.dynamic.adjustment_factor = 1.2;
        config
    }
    
    /// Create a conservative configuration
    pub fn conservative() -> Self {
        let mut config = Self::default();
        config.limits.max_concurrent = 4;
        config.limits.small_file_concurrent = 3;
        config.limits.medium_file_concurrent = 2;
        config.limits.large_file_concurrent = 1;
        config.dynamic.adjustment_factor = 1.1;
        config.dynamic.max_adjustment_step = 1;
        config
    }
    
    /// Create an aggressive configuration
    pub fn aggressive() -> Self {
        let mut config = Self::default();
        config.limits.max_concurrent = 16;
        config.dynamic.adjustment_factor = 2.0;
        config.dynamic.max_adjustment_step = 4;
        config
    }

    /// Builder method to set maximum concurrency
    pub fn with_max_concurrent(mut self, max_concurrent: usize) -> Self {
        self.limits.max_concurrent = max_concurrent;
        self
    }

    /// Builder method to set minimum concurrency
    pub fn with_min_concurrent(mut self, min_concurrent: usize) -> Self {
        self.limits.min_concurrent = min_concurrent;
        self
    }

    /// Builder method to set small file concurrency
    pub fn with_small_file_concurrent(mut self, small_file_concurrent: usize) -> Self {
        self.limits.small_file_concurrent = small_file_concurrent;
        self
    }

    /// Builder method to set large file concurrency
    pub fn with_large_file_concurrent(mut self, large_file_concurrent: usize) -> Self {
        self.limits.large_file_concurrent = large_file_concurrent;
        self
    }

    /// Builder method to set speed threshold
    pub fn with_speed_threshold(mut self, speed_threshold: f64) -> Self {
        self.dynamic.speed_threshold = speed_threshold as u64;
        self
    }

    /// Builder method to set adjustment factor
    pub fn with_adjustment_factor(mut self, adjustment_factor: f64) -> Self {
        self.dynamic.adjustment_factor = adjustment_factor;
        self
    }

    /// Builder method to set check interval
    pub fn with_check_interval(mut self, check_interval_secs: u64) -> Self {
        self.dynamic.adjustment_interval = Duration::from_secs(check_interval_secs);
        self
    }

    /// Builder method to enable/disable dynamic concurrency
    pub fn enable_dynamic_concurrency(mut self, enabled: bool) -> Self {
        self.dynamic.enabled = enabled;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config_validation() {
        let config = ConcurrencyConfig::default();
        assert!(config.validate().is_ok());
    }
    
    #[test]
    fn test_invalid_config() {
        let mut config = ConcurrencyConfig::default();
        config.limits.min_concurrent = 0;
        assert!(config.validate().is_err());
    }
    
    #[test]
    fn test_specialized_configs() {
        assert!(ConcurrencyConfig::for_small_files().validate().is_ok());
        assert!(ConcurrencyConfig::for_large_files().validate().is_ok());
        assert!(ConcurrencyConfig::conservative().validate().is_ok());
        assert!(ConcurrencyConfig::aggressive().validate().is_ok());
    }
}
