//! Common traits and interfaces for improved code reuse
//!
//! This module defines common traits that can be implemented across different components
//! to maximize code reuse and reduce specialized methods.

use crate::error::Result;
use async_trait::async_trait;
use std::time::Duration;

/// Common progress reporting interface
#[async_trait]
pub trait ProgressReporter: Send + Sync {
    /// Report progress for a task
    async fn report_progress(&self, task_id: &str, processed: u64, total: u64);
    
    /// Mark task as completed
    async fn complete_task(&self, task_id: &str);
    
    /// Mark task as failed
    async fn fail_task(&self, task_id: &str, error: &str);
    
    /// Get overall progress
    fn get_overall_progress(&self) -> (u64, u64); // (processed, total)
}

/// Configurable component interface
pub trait Configurable<T> {
    /// Apply configuration
    fn configure(&mut self, config: T) -> Result<()>;
    
    /// Get current configuration
    fn get_config(&self) -> &T;
    
    /// Validate configuration
    fn validate_config(config: &T) -> Result<()>;
}

/// Cacheable resource interface
#[async_trait]
pub trait Cacheable: Send + Sync {
    type Key;
    type Value;
    
    /// Get from cache
    async fn get(&self, key: &Self::Key) -> Result<Option<Self::Value>>;
    
    /// Put into cache
    async fn put(&self, key: Self::Key, value: Self::Value) -> Result<()>;
    
    /// Check if exists in cache
    async fn exists(&self, key: &Self::Key) -> bool;
    
    /// Remove from cache
    async fn remove(&self, key: &Self::Key) -> Result<()>;
    
    /// Clear all cache
    async fn clear(&self) -> Result<()>;
}

/// Metrics collection interface
pub trait MetricsCollector {
    /// Record a timing metric
    fn record_duration(&mut self, metric_name: &str, duration: Duration);
    
    /// Record a counter metric
    fn record_count(&mut self, metric_name: &str, count: u64);
    
    /// Record a gauge metric
    fn record_gauge(&mut self, metric_name: &str, value: f64);
    
    /// Get all metrics
    fn get_metrics(&self) -> std::collections::HashMap<String, MetricValue>;
}

/// Metric value types
#[derive(Debug, Clone)]
pub enum MetricValue {
    Duration(Duration),
    Count(u64),
    Gauge(f64),
}

/// Retry strategy interface
#[async_trait]
pub trait RetryStrategy: Send + Sync {
    /// Execute operation with retry
    async fn execute_with_retry<F, T, E>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Result<T> + Send + Sync,
        T: Send,
        E: std::error::Error + Send + Sync + 'static;
        
    /// Get retry configuration
    fn get_retry_config(&self) -> RetryConfig;
}

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub exponential_backoff: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(60),
            exponential_backoff: true,
        }
    }
}

/// Resource management interface
#[async_trait]
pub trait ResourceManager: Send + Sync {
    type Resource;
    type Config;
    
    /// Acquire a resource
    async fn acquire(&self) -> Result<Self::Resource>;
    
    /// Release a resource
    async fn release(&self, resource: Self::Resource);
    
    /// Configure the manager
    fn configure(&mut self, config: Self::Config);
    
    /// Get resource statistics
    fn get_stats(&self) -> ResourceStats;
}

/// Resource usage statistics
#[derive(Debug, Clone)]
pub struct ResourceStats {
    pub active_count: usize,
    pub total_acquired: u64,
    pub total_released: u64,
    pub peak_usage: usize,
}

/// Validatable interface for consistent validation
pub trait Validatable {
    type Error;
    
    /// Validate the object
    fn validate(&self) -> std::result::Result<(), Self::Error>;
}
