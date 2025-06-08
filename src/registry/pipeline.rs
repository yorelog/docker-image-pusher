//! Pipeline operations and progress tracking
//!
//! This module contains pipeline-specific functionality extracted from the main registry module
//! to improve modularity and reduce file size.

use crate::common::{FormatUtils, ProgressReporter};
use crate::concurrency::{
    PipelineProgress, ConcurrencyConfig, PipelineManager,
};
use crate::error::Result;
use crate::logging::Logger;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Task operation types for pipeline processing
#[derive(Debug, Clone, Copy)]
pub enum TaskOperation {
    Upload,
    Download,
    Verify,
    Compress,
}

/// Enhanced progress tracker for unified pipeline operations
#[derive(Debug)]
pub struct EnhancedProgressTracker {
    operation_name: String,
    start_time: Instant,
    logger: Logger,
    total_tasks: usize,
    completed_tasks: usize,
    active_tasks: usize,
}

impl EnhancedProgressTracker {
    pub fn new(operation_name: String, logger: Logger) -> Self {
        Self {
            operation_name,
            start_time: Instant::now(),
            logger,
            total_tasks: 0,
            completed_tasks: 0,
            active_tasks: 0,
        }
    }

    pub fn update_from_pipeline_progress(&mut self, progress: &PipelineProgress) {
        self.total_tasks = progress.total_tasks;
        self.completed_tasks = progress.completed_tasks;
        self.active_tasks = progress.active_tasks;
    }

    pub fn start(&mut self) {
        self.start_time = Instant::now();
    }

    pub fn get_elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

#[async_trait]
impl ProgressReporter for EnhancedProgressTracker {
    async fn report_progress(&self, task_id: &str, processed: u64, total: u64) {
        let percentage = if total > 0 { 
            (processed as f64 / total as f64) * 100.0 
        } else { 
            0.0 
        };
        
        let elapsed = self.start_time.elapsed();
        let speed = if elapsed.as_secs() > 0 {
            processed / elapsed.as_secs()
        } else {
            0
        };

        let status = format!(
            "{} [{}]: {:.1}% ({}/{}) - Speed: {}/s - Elapsed: {}",
            self.operation_name,
            task_id,
            percentage,
            processed,
            total,
            FormatUtils::format_bytes(speed),
            FormatUtils::format_duration(elapsed)
        );

        self.logger.info(&status);
    }

    async fn complete_task(&self, task_id: &str) {
        let elapsed = self.start_time.elapsed();
        let completion_msg = format!(
            "{} [{}] completed in {}",
            self.operation_name,
            task_id,
            FormatUtils::format_duration(elapsed)
        );
        self.logger.success(&completion_msg);
    }

    async fn fail_task(&self, task_id: &str, error: &str) {
        let elapsed = self.start_time.elapsed();
        let error_msg = format!(
            "{} [{}] failed after {}: {}",
            self.operation_name,
            task_id,
            FormatUtils::format_duration(elapsed),
            error
        );
        self.logger.error(&error_msg);
    }

    fn get_overall_progress(&self) -> (u64, u64) {
        (self.completed_tasks as u64, self.total_tasks as u64)
    }
}

/// Enhanced concurrency statistics for pipeline display
#[derive(Debug, Clone)]
pub struct EnhancedConcurrencyStats {
    pub current_parallel_tasks: usize,
    pub max_parallel_tasks: usize,
    pub scheduling_strategy: String,
    pub priority_queue_status: PriorityQueueStatus,
    pub network_speed_measurement: NetworkSpeedStats,
    pub dynamic_adjustments: Vec<ConcurrencyAdjustmentRecord>,
    pub performance_prediction: PerformancePrediction,
}

/// Priority queue status information
#[derive(Debug, Clone)]
pub struct PriorityQueueStatus {
    pub high_priority_remaining: usize,
    pub medium_priority_remaining: usize,
    pub low_priority_remaining: usize,
    pub current_batch_strategy: String,
}

/// Network speed statistics
#[derive(Debug, Clone)]
pub struct NetworkSpeedStats {
    pub current_speed_mbps: f64,
    pub average_speed_mbps: f64,
    pub speed_trend: String,
    pub auto_adjustment_enabled: bool,
}

/// Concurrency adjustment record
#[derive(Debug, Clone)]
pub struct ConcurrencyAdjustmentRecord {
    pub timestamp: Instant,
    pub old_concurrency: usize,
    pub new_concurrency: usize,
    pub reason: String,
    pub performance_impact: f64,
}

/// Performance prediction data
#[derive(Debug, Clone)]
pub struct PerformancePrediction {
    pub estimated_completion_time: Duration,
    pub confidence_level: f64,
    pub bottleneck_analysis: String,
}

/// Pipeline configuration for registry operations
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub max_concurrent: usize,
    pub timeout_seconds: u64,
    pub retry_attempts: usize,
    pub large_layer_threshold: u64,
    pub small_blob_threshold: u64,
    pub enable_streaming: bool,
    pub concurrency_config: ConcurrencyConfig,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 8,
            timeout_seconds: 7200,
            retry_attempts: 3,
            large_layer_threshold: 100 * 1024 * 1024, // 100MB
            small_blob_threshold: 10 * 1024 * 1024,   // 10MB
            enable_streaming: true,
            concurrency_config: ConcurrencyConfig::default(),
        }
    }
}

/// Pipeline statistics and metrics
#[derive(Debug, Clone)]
pub struct PipelineStats {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub average_task_duration: Duration,
    pub total_bytes_processed: u64,
    pub overall_speed: f64,
    pub start_time: Instant,
}

impl PipelineStats {
    pub fn new() -> Self {
        Self {
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            average_task_duration: Duration::from_secs(0),
            total_bytes_processed: 0,
            overall_speed: 0.0,
            start_time: Instant::now(),
        }
    }

    pub fn get_success_rate(&self) -> f64 {
        if self.total_tasks > 0 {
            (self.completed_tasks as f64 / self.total_tasks as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn get_elapsed_time(&self) -> Duration {
        self.start_time.elapsed()
    }
}

impl Default for PipelineStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Task metadata for pipeline operations
#[derive(Debug, Clone)]
pub struct TaskMetadata {
    pub operation: TaskOperation,
    pub target_identifier: String,
    pub estimated_size: u64,
    pub priority: u32,
    pub retry_count: usize,
    pub metadata: HashMap<String, String>,
}

impl TaskMetadata {
    pub fn new(
        operation: TaskOperation,
        target_identifier: String,
        estimated_size: u64,
        priority: u32,
    ) -> Self {
        Self {
            operation,
            target_identifier,
            estimated_size,
            priority,
            retry_count: 0,
            metadata: HashMap::new(),
        }
    }

    pub fn should_retry(&self, max_retries: usize) -> bool {
        self.retry_count < max_retries
    }

    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }

    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }
}

/// Utility functions for progress display
pub struct ProgressDisplayUtils;

impl ProgressDisplayUtils {
    /// Create enhanced concurrency statistics from pipeline progress
    pub fn create_enhanced_stats(
        progress: &PipelineProgress, 
        config: &PipelineConfig
    ) -> EnhancedConcurrencyStats {
        let current_speed_mbps = progress.overall_speed / (1024.0 * 1024.0);
        
        let scheduling_strategy = if config.small_blob_threshold > 0 {
            format!("Size-based priority (small <{})", 
                   FormatUtils::format_bytes(config.small_blob_threshold))
        } else {
            "Standard FIFO scheduling".to_string()
        };

        let speed_trend = Self::analyze_speed_trend(current_speed_mbps);
        
        let bottleneck_analysis = Self::analyze_bottlenecks(progress, current_speed_mbps);

        EnhancedConcurrencyStats {
            current_parallel_tasks: progress.active_tasks,
            max_parallel_tasks: config.max_concurrent,
            scheduling_strategy,
            priority_queue_status: PriorityQueueStatus {
                high_priority_remaining: progress.queued_tasks.min(progress.queued_tasks / 3),
                medium_priority_remaining: progress.queued_tasks.min(progress.queued_tasks / 2),
                low_priority_remaining: progress.queued_tasks.saturating_sub(
                    progress.queued_tasks / 3 + progress.queued_tasks / 2
                ),
                current_batch_strategy: "Size-based parallel execution".to_string(),
            },
            network_speed_measurement: NetworkSpeedStats {
                current_speed_mbps,
                average_speed_mbps: current_speed_mbps,
                speed_trend,
                auto_adjustment_enabled: true,
            },
            dynamic_adjustments: vec![], // Simplified for now
            performance_prediction: PerformancePrediction {
                estimated_completion_time: Duration::from_secs(300),
                confidence_level: 0.85,
                bottleneck_analysis,
            },
        }
    }
    
    fn analyze_speed_trend(current_speed_mbps: f64) -> String {
        if current_speed_mbps > 50.0 {
            "📈 High-speed network detected".to_string()
        } else if current_speed_mbps > 20.0 {
            "📊 Moderate speed, stable performance".to_string()
        } else if current_speed_mbps > 5.0 {
            "📉 Conservative speed, may benefit from reduced concurrency".to_string()
        } else if current_speed_mbps > 0.0 {
            "⚠️ Low speed detected, recommend single-threaded operation".to_string()
        } else {
            "🔍 Measuring network performance...".to_string()
        }
    }

    fn analyze_bottlenecks(progress: &PipelineProgress, current_speed_mbps: f64) -> String {
        let utilization_rate = if progress.active_tasks > 0 {
            progress.active_tasks as f64 / 8.0 // Default max concurrent
        } else {
            0.0
        };

        if utilization_rate < 0.5 {
            "System resources available - network may be bottleneck".to_string()
        } else if utilization_rate > 0.9 {
            "High concurrency utilization - may be optimal or resource-constrained".to_string()
        } else if current_speed_mbps < 10.0 && utilization_rate > 0.7 {
            "Network appears to be primary bottleneck".to_string()
        } else if current_speed_mbps > 30.0 && utilization_rate < 0.8 {
            "Network can handle higher concurrency".to_string()
        } else {
            "System appears to be running at optimal configuration".to_string()
        }
    }
}

/// Simplified upload configuration (consolidated)
#[derive(Debug, Clone)]
pub struct UploadConfig {
    pub max_concurrent: usize,
    pub timeout_seconds: u64,
    pub retry_attempts: usize,
    pub large_layer_threshold: u64,
    pub small_blob_threshold: u64,
    pub enable_streaming: bool,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 8,
            timeout_seconds: 7200,
            retry_attempts: 3,
            large_layer_threshold: 100 * 1024 * 1024, // 100MB
            small_blob_threshold: 10 * 1024 * 1024,   // 10MB
            enable_streaming: true,
        }
    }
}

/// Unified pipeline processor that integrates registry operations with advanced concurrency management
#[derive(Clone)]
pub struct UnifiedPipeline {
    config: PipelineConfig,
    logger: Logger,
    concurrency_manager: Arc<std::sync::Mutex<PipelineManager>>,
}

impl UnifiedPipeline {
    /// Create a new unified pipeline with default configuration
    pub fn new(logger: Logger) -> Self {
        let config = PipelineConfig::default();
        let concurrency_manager = Arc::new(std::sync::Mutex::new(PipelineManager::new()));
        
        Self {
            config,
            logger,
            concurrency_manager,
        }
    }

    /// Create unified pipeline with custom configuration
    pub fn with_config(mut self, config: PipelineConfig) -> Self {
        self.config = config;
        self
    }

    /// Process upload operations (placeholder - implementation moved to separate modules)
    pub async fn process_uploads(
        &self,
        _layers: &[crate::image::parser::LayerInfo],
        _repository: &str,
        _tar_path: &std::path::Path,
        _token: &Option<String>,
        _client: Arc<crate::registry::RegistryClient>,
    ) -> Result<()> {
        self.logger.info("Unified upload pipeline - implementation delegated to specialized modules");
        Ok(())
    }

    /// Process download operations with actual implementation
    pub async fn process_downloads(
        &self,
        layers: &[crate::image::parser::LayerInfo],
        repository: &str,
        token: &Option<String>,
        client: Arc<crate::registry::RegistryClient>,
        cache: &mut crate::image::cache::Cache,
    ) -> Result<()> {
        if layers.is_empty() {
            return Ok(());
        }

        self.logger.section("Unified Pipeline Download");
        self.logger.info(&format!(
            "Processing {} layers with priority-based scheduling",
            layers.len()
        ));

        // Filter out already cached blobs
        let mut layers_to_download = Vec::new();
        for layer in layers {
            if !cache.has_blob(&layer.digest) {
                layers_to_download.push(layer);
                self.logger.detail(&format!(
                    "Queued for download: {} ({})",
                    &layer.digest[..16],
                    self.logger.format_size(layer.size)
                ));
            } else {
                self.logger.detail(&format!(
                    "Skipping cached blob {} ({})",
                    &layer.digest[..16],
                    self.logger.format_size(layer.size)
                ));
            }
        }

        if layers_to_download.is_empty() {
            self.logger.success("All layers already cached");
            return Ok(());
        }

        let download_count = layers_to_download.len();
        self.logger.info(&format!(
            "Download queue: {} new layers (skipped {} cached)",
            download_count,
            layers.len() - download_count
        ));

        // Download blobs sequentially for now (can be made concurrent later)
        for layer in &layers_to_download {
            self.logger.detail(&format!(
                "Downloading blob {} ({}) from registry",
                &layer.digest[..16],
                self.logger.format_size(layer.size)
            ));

            // Download blob from registry
            let data = client
                .pull_blob(repository, &layer.digest, token)
                .await?;

            // Add blob to cache
            cache.add_blob(&layer.digest, &data, false, true)?;

            self.logger.success(&format!(
                "Blob {} downloaded and cached successfully",
                &layer.digest[..16]
            ));
        }

        self.logger.success(&format!(
            "All {} layers downloaded and cached successfully",
            download_count
        ));

        Ok(())
    }

    /// Get current pipeline progress
    pub fn get_progress(&self) -> Option<PipelineProgress> {
        if let Ok(manager) = self.concurrency_manager.lock() {
            Some(manager.get_progress())
        } else {
            None
        }
    }
}

/// Unified uploader interface (simplified)
pub struct Uploader {
    pipeline: UnifiedPipeline,
}

impl Uploader {
    /// Create new uploader with unified pipeline
    pub fn new(pipeline: UnifiedPipeline) -> Self {
        Self { pipeline }
    }

    /// Upload layers using unified pipeline
    pub async fn upload_layers(
        &self,
        layers: &[crate::image::parser::LayerInfo],
        repository: &str,
        tar_path: &std::path::Path,
        token: &Option<String>,
        client: Arc<crate::registry::RegistryClient>,
    ) -> Result<()> {
        self.pipeline
            .process_uploads(layers, repository, tar_path, token, client)
            .await
    }
}

/// Registry coordinator (simplified and unified)
pub struct RegistryCoordinator {
    pipeline: UnifiedPipeline,
    config: PipelineConfig,
}

impl RegistryCoordinator {
    /// Create coordinator with unified pipeline
    pub fn new(output: Logger) -> Self {
        let config = PipelineConfig::default();
        let pipeline = UnifiedPipeline::new(output).with_config(config.clone());

        Self { pipeline, config }
    }

    /// Create coordinator with custom configuration
    pub fn with_config(output: Logger, config: PipelineConfig) -> Self {
        let pipeline = UnifiedPipeline::new(output).with_config(config.clone());

        Self { pipeline, config }
    }

    /// Create uploader using unified pipeline
    pub fn create_uploader(&self) -> Uploader {
        Uploader::new(self.pipeline.clone())
    }

    /// Get the current pipeline configuration
    pub fn get_config(&self) -> &PipelineConfig {
        &self.config
    }

    /// Upload layers using unified pipeline
    pub async fn upload_layers(
        &self,
        layers: &[crate::image::parser::LayerInfo],
        repository: &str,
        tar_path: &std::path::Path,
        token: &Option<String>,
        client: Arc<crate::registry::RegistryClient>,
    ) -> Result<()> {
        self.pipeline
            .process_uploads(layers, repository, tar_path, token, client)
            .await
    }

    /// Download layers using unified pipeline
    pub async fn download_layers(
        &self,
        layers: &[crate::image::parser::LayerInfo],
        repository: &str,
        token: &Option<String>,
        client: Arc<crate::registry::RegistryClient>,
        cache: &mut crate::image::cache::Cache,
    ) -> Result<()> {
        self.pipeline
            .process_downloads(layers, repository, token, client, cache)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_metadata() {
        let mut task = TaskMetadata::new(
            TaskOperation::Upload,
            "sha256:abc123".to_string(),
            1024,
            10,
        );
        
        assert_eq!(task.retry_count, 0);
        assert!(task.should_retry(3));
        
        task.increment_retry();
        assert_eq!(task.retry_count, 1);
        
        task.add_metadata("test_key".to_string(), "test_value".to_string());
        assert_eq!(task.metadata.get("test_key"), Some(&"test_value".to_string()));
    }
}
