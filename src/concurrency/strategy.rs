//! Concurrency Strategy Management
//!
//! This module provides strategy selection algorithms and implementations
//! for different concurrency management approaches.

use super::{SpeedTrend, PerformanceStatistics, config::StrategyConfig};
use std::collections::HashMap;
use std::time::Instant;

/// Available concurrency strategies
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConcurrencyStrategy {
    /// Conservative strategy: prioritizes stability over performance
    Conservative,
    /// Aggressive strategy: prioritizes performance over stability
    Aggressive,
    /// Adaptive strategy: balances performance and stability
    Adaptive,
    /// Network-optimized strategy: optimizes for network characteristics
    NetworkOptimized,
    /// Resource-aware strategy: considers system resource constraints
    ResourceAware,
    /// ML-enhanced strategy: uses machine learning for optimization
    MLEnhanced,
}

impl std::fmt::Display for ConcurrencyStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConcurrencyStrategy::Conservative => write!(f, "conservative"),
            ConcurrencyStrategy::Aggressive => write!(f, "aggressive"),
            ConcurrencyStrategy::Adaptive => write!(f, "adaptive"),
            ConcurrencyStrategy::NetworkOptimized => write!(f, "network_optimized"),
            ConcurrencyStrategy::ResourceAware => write!(f, "resource_aware"),
            ConcurrencyStrategy::MLEnhanced => write!(f, "ml_enhanced"),
        }
    }
}

impl std::str::FromStr for ConcurrencyStrategy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "conservative" => Ok(ConcurrencyStrategy::Conservative),
            "aggressive" => Ok(ConcurrencyStrategy::Aggressive),
            "adaptive" => Ok(ConcurrencyStrategy::Adaptive),
            "network_optimized" | "network-optimized" => Ok(ConcurrencyStrategy::NetworkOptimized),
            "resource_aware" | "resource-aware" => Ok(ConcurrencyStrategy::ResourceAware),
            "ml_enhanced" | "ml-enhanced" => Ok(ConcurrencyStrategy::MLEnhanced),
            _ => Err(format!("Unknown strategy: {}", s)),
        }
    }
}

/// Strategy selector that chooses optimal strategies based on performance
#[derive(Debug)]
pub struct StrategySelector {
    /// Current active strategy
    current_strategy: ConcurrencyStrategy,
    /// Configuration for strategy selection
    config: StrategyConfig,
    /// Performance history for each strategy
    strategy_performance: HashMap<ConcurrencyStrategy, StrategyPerformance>,
    /// Strategy switch history
    switch_history: Vec<StrategySwitch>,
    /// Last strategy evaluation timestamp
    last_evaluation: Instant,
}

/// Performance metrics for a specific strategy
#[derive(Debug, Clone)]
pub struct StrategyPerformance {
    /// Number of times this strategy was used
    usage_count: u64,
    /// Total bytes transferred under this strategy
    total_bytes: u64,
    /// Total time spent using this strategy
    total_time: std::time::Duration,
    /// Average performance score
    avg_performance_score: f64,
    /// Success rate (0.0-1.0)
    success_rate: f64,
    /// Recent performance samples
    recent_samples: Vec<PerformanceSample>,
}

/// Individual performance sample
#[derive(Debug, Clone)]
pub struct PerformanceSample {
    timestamp: Instant,
    bytes_transferred: u64,
    duration: std::time::Duration,
    success: bool,
}

/// Record of strategy switches
#[derive(Debug, Clone)]
pub struct StrategySwitch {
    timestamp: Instant,
    from_strategy: ConcurrencyStrategy,
    to_strategy: ConcurrencyStrategy,
    reason: String,
    confidence: f64,
}

impl StrategySelector {
    /// Create a new strategy selector
    pub fn new(config: StrategyConfig) -> Self {
        let default_strategy = config.default_strategy.parse()
            .unwrap_or(ConcurrencyStrategy::Adaptive);

        Self {
            current_strategy: default_strategy,
            config,
            strategy_performance: HashMap::new(),
            switch_history: Vec::new(),
            last_evaluation: Instant::now(),
        }
    }

    /// Get the current strategy
    pub fn current_strategy(&self) -> &ConcurrencyStrategy {
        &self.current_strategy
    }

    /// Record performance data for the current strategy
    pub fn record_performance(&mut self, bytes_transferred: u64, duration: std::time::Duration, success: bool) {
        let sample = PerformanceSample {
            timestamp: Instant::now(),
            bytes_transferred,
            duration,
            success,
        };

        let performance = self.strategy_performance
            .entry(self.current_strategy.clone())
            .or_insert_with(StrategyPerformance::new);

        performance.add_sample(sample);
    }

    /// Evaluate whether a strategy switch is recommended
    pub fn evaluate_strategy_switch(&mut self, current_performance: &PerformanceStatistics) -> Option<ConcurrencyStrategy> {
        if !self.config.auto_strategy_switching {
            return None;
        }

        // Check cooldown period
        if self.last_evaluation.elapsed() < self.config.strategy_switch_cooldown {
            return None;
        }

        self.last_evaluation = Instant::now();

        // Analyze current strategy performance
        let current_score = self.calculate_strategy_score(&self.current_strategy, current_performance);
        
        // Evaluate alternative strategies
        let mut best_alternative = None;
        let mut best_score = current_score;

        for strategy in self.get_alternative_strategies() {
            let score = self.calculate_strategy_score(&strategy, current_performance);
            if score > best_score + 0.1 { // Require significant improvement
                best_score = score;
                best_alternative = Some(strategy);
            }
        }

        // Switch strategy if a better alternative is found with sufficient confidence
        if let Some(new_strategy) = best_alternative {
            let confidence = (best_score - current_score) / current_score;
            if confidence >= self.config.strategy_switch_confidence {
                self.switch_strategy(new_strategy, confidence);
                return Some(self.current_strategy.clone());
            }
        }

        None
    }

    /// Calculate performance score for a strategy
    fn calculate_strategy_score(&self, strategy: &ConcurrencyStrategy, current_perf: &PerformanceStatistics) -> f64 {
        if let Some(perf) = self.strategy_performance.get(strategy) {
            // Use historical performance if available
            perf.calculate_score()
        } else {
            // Use current performance and strategy characteristics for new strategies
            self.estimate_strategy_score(strategy, current_perf)
        }
    }

    /// Estimate score for strategies without historical data
    fn estimate_strategy_score(&self, strategy: &ConcurrencyStrategy, current_perf: &PerformanceStatistics) -> f64 {
        let base_score = current_perf.current_speed as f64 / 1_000_000.0; // MB/s

        match strategy {
            ConcurrencyStrategy::Conservative => base_score * 0.8, // Stable but potentially slower
            ConcurrencyStrategy::Aggressive => base_score * 1.2,   // Potentially faster but less stable
            ConcurrencyStrategy::Adaptive => base_score * 1.0,     // Balanced approach
            ConcurrencyStrategy::NetworkOptimized => {
                // Bonus for network-intensive operations
                if current_perf.trend == SpeedTrend::Stable {
                    base_score * 1.1
                } else {
                    base_score * 0.9
                }
            },
            ConcurrencyStrategy::ResourceAware => base_score * 0.95, // Slightly conservative
            ConcurrencyStrategy::MLEnhanced => base_score * 1.3,     // Potentially best but needs learning
        }
    }

    /// Get list of alternative strategies to evaluate
    fn get_alternative_strategies(&self) -> Vec<ConcurrencyStrategy> {
        vec![
            ConcurrencyStrategy::Conservative,
            ConcurrencyStrategy::Aggressive,
            ConcurrencyStrategy::Adaptive,
            ConcurrencyStrategy::NetworkOptimized,
            ConcurrencyStrategy::ResourceAware,
            ConcurrencyStrategy::MLEnhanced,
        ].into_iter()
        .filter(|s| s != &self.current_strategy)
        .collect()
    }

    /// Switch to a new strategy
    fn switch_strategy(&mut self, new_strategy: ConcurrencyStrategy, confidence: f64) {
        let switch = StrategySwitch {
            timestamp: Instant::now(),
            from_strategy: self.current_strategy.clone(),
            to_strategy: new_strategy.clone(),
            reason: format!("Performance improvement expected: {:.1}%", confidence * 100.0),
            confidence,
        };

        self.switch_history.push(switch);
        self.current_strategy = new_strategy;

        // Keep switch history manageable
        if self.switch_history.len() > 50 {
            self.switch_history.remove(0);
        }
    }

    /// Get strategy switch history
    pub fn get_switch_history(&self) -> &[StrategySwitch] {
        &self.switch_history
    }

    /// Get performance data for all strategies
    pub fn get_strategy_performance(&self) -> &HashMap<ConcurrencyStrategy, StrategyPerformance> {
        &self.strategy_performance
    }
}

impl StrategyPerformance {
    /// Create new strategy performance tracker
    fn new() -> Self {
        Self {
            usage_count: 0,
            total_bytes: 0,
            total_time: std::time::Duration::from_secs(0),
            avg_performance_score: 0.0,
            success_rate: 1.0,
            recent_samples: Vec::new(),
        }
    }

    /// Add a new performance sample
    fn add_sample(&mut self, sample: PerformanceSample) {
        self.usage_count += 1;
        self.total_bytes += sample.bytes_transferred;
        self.total_time += sample.duration;
        
        // Update success rate
        let success_count = if sample.success { 1 } else { 0 };
        self.success_rate = (self.success_rate * (self.usage_count - 1) as f64 + success_count as f64) / self.usage_count as f64;

        // Add to recent samples
        self.recent_samples.push(sample);
        
        // Keep only recent samples (last 20)
        if self.recent_samples.len() > 20 {
            self.recent_samples.remove(0);
        }

        // Recalculate average performance score
        self.recalculate_avg_score();
    }

    /// Calculate overall performance score for this strategy
    fn calculate_score(&self) -> f64 {
        if self.usage_count == 0 {
            return 0.0;
        }

        let avg_speed = if self.total_time.as_secs() > 0 {
            self.total_bytes as f64 / self.total_time.as_secs_f64()
        } else {
            0.0
        };

        // Combine speed and success rate with weighted importance
        let speed_score = avg_speed / 1_000_000.0; // Convert to MB/s
        let reliability_score = self.success_rate * 10.0; // Scale success rate
        
        // Weighted combination: 70% speed, 30% reliability
        speed_score * 0.7 + reliability_score * 0.3
    }

    /// Recalculate average performance score based on recent samples
    fn recalculate_avg_score(&mut self) {
        if self.recent_samples.is_empty() {
            self.avg_performance_score = 0.0;
            return;
        }

        let total_score: f64 = self.recent_samples.iter()
            .map(|sample| {
                let speed = sample.bytes_transferred as f64 / sample.duration.as_secs_f64().max(0.001);
                let success_bonus = if sample.success { 1.0 } else { 0.5 };
                speed * success_bonus
            })
            .sum();

        self.avg_performance_score = total_score / self.recent_samples.len() as f64;
    }

    /// Get average speed for this strategy
    pub fn avg_speed(&self) -> f64 {
        if self.total_time.as_secs() > 0 {
            self.total_bytes as f64 / self.total_time.as_secs_f64()
        } else {
            0.0
        }
    }

    /// Get success rate for this strategy
    pub fn success_rate(&self) -> f64 {
        self.success_rate
    }

    /// Get usage count for this strategy
    pub fn usage_count(&self) -> u64 {
        self.usage_count
    }
}

/// Strategy-specific configuration and behavior implementations
pub struct StrategyImplementation;

impl StrategyImplementation {
    /// Get concurrency adjustment recommendation for a strategy
    pub fn get_adjustment_recommendation(
        strategy: &ConcurrencyStrategy,
        current_concurrency: usize,
        performance: &PerformanceStatistics,
        limits: &super::config::ConcurrencyLimits,
    ) -> Option<usize> {
        match strategy {
            ConcurrencyStrategy::Conservative => {
                Self::conservative_adjustment(current_concurrency, performance, limits)
            },
            ConcurrencyStrategy::Aggressive => {
                Self::aggressive_adjustment(current_concurrency, performance, limits)
            },
            ConcurrencyStrategy::Adaptive => {
                Self::adaptive_adjustment(current_concurrency, performance, limits)
            },
            ConcurrencyStrategy::NetworkOptimized => {
                Self::network_optimized_adjustment(current_concurrency, performance, limits)
            },
            ConcurrencyStrategy::ResourceAware => {
                Self::resource_aware_adjustment(current_concurrency, performance, limits)
            },
            ConcurrencyStrategy::MLEnhanced => {
                Self::ml_enhanced_adjustment(current_concurrency, performance, limits)
            },
        }
    }

    /// Conservative strategy: small, safe adjustments
    fn conservative_adjustment(
        current: usize,
        performance: &PerformanceStatistics,
        limits: &super::config::ConcurrencyLimits,
    ) -> Option<usize> {
        match performance.trend {
            SpeedTrend::Decreasing => {
                if current > limits.min_concurrent {
                    Some(current - 1)
                } else {
                    None
                }
            },
            SpeedTrend::Increasing => {
                if current < limits.max_concurrent && performance.trend_confidence > 0.8 {
                    Some(current + 1)
                } else {
                    None
                }
            },
            _ => None,
        }
    }

    /// Aggressive strategy: larger adjustments for better performance
    fn aggressive_adjustment(
        current: usize,
        performance: &PerformanceStatistics,
        limits: &super::config::ConcurrencyLimits,
    ) -> Option<usize> {
        let adjustment_size = (current as f64 * 0.5).ceil() as usize;
        
        match performance.trend {
            SpeedTrend::Decreasing => {
                let new_concurrency = current.saturating_sub(adjustment_size);
                if new_concurrency >= limits.min_concurrent {
                    Some(new_concurrency)
                } else {
                    Some(limits.min_concurrent)
                }
            },
            SpeedTrend::Increasing => {
                let new_concurrency = current + adjustment_size;
                if new_concurrency <= limits.max_concurrent {
                    Some(new_concurrency)
                } else {
                    Some(limits.max_concurrent)
                }
            },
            _ => None,
        }
    }

    /// Adaptive strategy: balanced approach based on confidence
    fn adaptive_adjustment(
        current: usize,
        performance: &PerformanceStatistics,
        limits: &super::config::ConcurrencyLimits,
    ) -> Option<usize> {
        let confidence = performance.trend_confidence;
        let adjustment_size = if confidence > 0.8 {
            2
        } else if confidence > 0.6 {
            1
        } else {
            return None; // Not confident enough to adjust
        };

        match performance.trend {
            SpeedTrend::Decreasing => {
                let new_concurrency = current.saturating_sub(adjustment_size);
                if new_concurrency >= limits.min_concurrent {
                    Some(new_concurrency)
                } else {
                    None
                }
            },
            SpeedTrend::Increasing => {
                let new_concurrency = current + adjustment_size;
                if new_concurrency <= limits.max_concurrent {
                    Some(new_concurrency)
                } else {
                    None
                }
            },
            _ => None,
        }
    }

    /// Network-optimized strategy: considers network characteristics
    fn network_optimized_adjustment(
        current: usize,
        performance: &PerformanceStatistics,
        limits: &super::config::ConcurrencyLimits,
    ) -> Option<usize> {
        // Consider network latency and bandwidth utilization
        let current_speed_mbps = performance.current_speed as f64 / 1_000_000.0;
        
        if current_speed_mbps < 1.0 { // Low speed, reduce concurrency
            if current > limits.min_concurrent {
                Some(current - 1)
            } else {
                None
            }
        } else if current_speed_mbps > 50.0 && current < limits.max_concurrent {
            // High speed, can increase concurrency
            Some(current + 1)
        } else {
            None
        }
    }

    /// Resource-aware strategy: considers system resource constraints
    fn resource_aware_adjustment(
        current: usize,
        performance: &PerformanceStatistics,
        limits: &super::config::ConcurrencyLimits,
    ) -> Option<usize> {
        // This would typically check CPU usage, memory usage, etc.
        // For now, use a conservative approach
        match performance.trend {
            SpeedTrend::Decreasing => {
                if current > limits.min_concurrent {
                    Some(current - 1)
                } else {
                    None
                }
            },
            SpeedTrend::Increasing if performance.trend_confidence > 0.9 => {
                if current < limits.max_concurrent {
                    Some(current + 1)
                } else {
                    None
                }
            },
            _ => None,
        }
    }

    /// ML-enhanced strategy: uses machine learning predictions
    fn ml_enhanced_adjustment(
        current: usize,
        performance: &PerformanceStatistics,
        limits: &super::config::ConcurrencyLimits,
    ) -> Option<usize> {
        // This would use ML predictions
        // For now, use an adaptive approach with higher confidence requirements
        if performance.trend_confidence > 0.9 {
            Self::adaptive_adjustment(current, performance, limits)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_strategy_parsing() {
        assert_eq!("conservative".parse::<ConcurrencyStrategy>().unwrap(), 
                  ConcurrencyStrategy::Conservative);
        assert_eq!("aggressive".parse::<ConcurrencyStrategy>().unwrap(), 
                  ConcurrencyStrategy::Aggressive);
        assert!("invalid".parse::<ConcurrencyStrategy>().is_err());
    }

    #[test]
    fn test_strategy_selector_creation() {
        let config = StrategyConfig::default();
        let selector = StrategySelector::new(config);
        assert_eq!(selector.current_strategy(), &ConcurrencyStrategy::Adaptive);
    }

    #[test]
    fn test_strategy_performance_tracking() {
        let mut perf = StrategyPerformance::new();
        assert_eq!(perf.usage_count(), 0);
        assert_eq!(perf.success_rate(), 1.0);

        let sample = PerformanceSample {
            timestamp: Instant::now(),
            bytes_transferred: 1024,
            duration: Duration::from_secs(1),
            success: true,
        };

        perf.add_sample(sample);
        assert_eq!(perf.usage_count(), 1);
        assert!(perf.avg_speed() > 0.0);
    }

    #[test]
    fn test_conservative_adjustment() {
        let limits = super::super::config::ConcurrencyLimits::default();
        let performance = PerformanceStatistics {
            current_speed: 1000000,
            average_speed: 1000000,
            peak_speed: 1000000,
            total_bytes: 1000000,
            total_time: Duration::from_secs(1),
            trend: SpeedTrend::Decreasing,
            trend_confidence: 0.9,
        };

        let adjustment = StrategyImplementation::conservative_adjustment(4, &performance, &limits);
        assert_eq!(adjustment, Some(3)); // Should decrease by 1
    }

    #[test]
    fn test_aggressive_adjustment() {
        let limits = super::super::config::ConcurrencyLimits::default();
        let performance = PerformanceStatistics {
            current_speed: 1000000,
            average_speed: 1000000,
            peak_speed: 1000000,
            total_bytes: 1000000,
            total_time: Duration::from_secs(1),
            trend: SpeedTrend::Increasing,
            trend_confidence: 0.9,
        };

        let adjustment = StrategyImplementation::aggressive_adjustment(4, &performance, &limits);
        assert!(adjustment.unwrap() > 4); // Should increase significantly
    }
}
