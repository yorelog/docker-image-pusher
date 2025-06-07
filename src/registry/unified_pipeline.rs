//! Unified registry pipeline for both upload and download operations
//!
//! This module consolidates all pipeline operations into a single, coherent system
//! that handles both uploads and downloads with priority-based scheduling.

use crate::error::{RegistryError, Result};
use crate::image::parser::LayerInfo;
use crate::logging::Logger;
use crate::registry::RegistryClient;
use futures::future;
use std::cmp::Ordering;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Unified task for both upload and download operations
#[derive(Debug, Clone)]
pub struct PipelineTask {
    pub layer: LayerInfo,
    pub index: usize,
    pub priority: u64,
    pub operation: TaskOperation,
}

#[derive(Debug, Clone)]
pub enum TaskOperation {
    Upload {
        upload_url: String,
        repository: String,
        tar_path: std::path::PathBuf,
    },
    Download {
        repository: String,
    },
}

impl PartialEq for PipelineTask {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for PipelineTask {}

impl Ord for PipelineTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap (smaller size first)
        other.priority.cmp(&self.priority)
    }
}

impl PartialOrd for PipelineTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Configuration for the unified pipeline
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub max_concurrent: usize,
    pub small_blob_threshold: u64,
    pub medium_blob_threshold: u64,
    pub large_blob_threshold: u64,
    pub timeout_seconds: u64,
    pub retry_attempts: usize,
    pub buffer_size: usize,
    pub memory_limit_mb: usize,
    pub enable_compression: bool,
    pub enable_streaming: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            small_blob_threshold: 10 * 1024 * 1024,   // 10MB
            medium_blob_threshold: 100 * 1024 * 1024, // 100MB
            large_blob_threshold: 500 * 1024 * 1024,  // 500MB
            timeout_seconds: 7200,
            retry_attempts: 3,
            buffer_size: 1024,
            memory_limit_mb: 512,
            enable_compression: true,
            enable_streaming: true,
        }
    }
}

/// Unified registry pipeline processor
#[derive(Clone)]
pub struct UnifiedPipeline {
    config: PipelineConfig,
    output: Logger,
}

impl UnifiedPipeline {
    pub fn new(output: Logger) -> Self {
        Self {
            config: PipelineConfig::default(),
            output,
        }
    }

    pub fn with_config(mut self, config: PipelineConfig) -> Self {
        self.config = config;
        self
    }

    /// Process upload operations with priority scheduling
    pub async fn process_uploads(
        &self,
        layers: &[LayerInfo],
        repository: &str,
        tar_path: &std::path::Path,
        token: &Option<String>,
        client: Arc<RegistryClient>,
    ) -> Result<()> {
        if layers.is_empty() {
            return Ok(());
        }

        self.output.section("Unified Pipeline Upload");
        self.output.info(&format!(
            "Processing {} layers with priority-based scheduling",
            layers.len()
        ));

        // Create upload tasks with priority
        let mut tasks = Vec::new();
        for (index, layer) in layers.iter().enumerate() {
            let priority = self.calculate_priority(layer.size);

            tasks.push(PipelineTask {
                layer: layer.clone(),
                index,
                priority,
                operation: TaskOperation::Upload {
                    upload_url: String::new(), // Will be set during execution
                    repository: repository.to_string(),
                    tar_path: tar_path.to_path_buf(),
                },
            });
        }

        // Sort by priority (small blobs first)
        tasks.sort();

        self.output.info(&format!(
            "Upload queue: {} small, {} medium, {} large blobs",
            tasks
                .iter()
                .filter(|t| t.layer.size <= self.config.small_blob_threshold)
                .count(),
            tasks
                .iter()
                .filter(|t| t.layer.size > self.config.small_blob_threshold
                    && t.layer.size <= self.config.medium_blob_threshold)
                .count(),
            tasks
                .iter()
                .filter(|t| t.layer.size > self.config.medium_blob_threshold)
                .count()
        ));

        // Execute with concurrency control - ignore download results for uploads
        self.execute_tasks(tasks, token, client).await.map(|_| ())
    }

    /// Process download operations with priority scheduling
    pub async fn process_downloads(
        &self,
        layers: &[LayerInfo],
        repository: &str,
        token: &Option<String>,
        client: Arc<RegistryClient>,
        cache: &mut crate::image::cache::Cache,
    ) -> Result<()> {
        if layers.is_empty() {
            return Ok(());
        }

        self.output.section("Unified Pipeline Download");
        self.output.info(&format!(
            "Processing {} layers with priority-based scheduling",
            layers.len()
        ));

        // Create download tasks with priority, filtering cached blobs
        let mut tasks = Vec::new();
        for (index, layer) in layers.iter().enumerate() {
            if !cache.has_blob(&layer.digest) {
                let priority = self.calculate_priority(layer.size);

                tasks.push(PipelineTask {
                    layer: layer.clone(),
                    index,
                    priority,
                    operation: TaskOperation::Download {
                        repository: repository.to_string(),
                    },
                });
            } else {
                self.output.detail(&format!(
                    "Skipping cached blob {} ({})",
                    &layer.digest[..16],
                    self.output.format_size(layer.size)
                ));
            }
        }

        if tasks.is_empty() {
            self.output.success("All layers already cached");
            return Ok(());
        }

        // Sort by priority (small blobs first)
        tasks.sort();

        self.output.info(&format!(
            "Download queue: {} new layers (skipped {} cached)",
            tasks.len(),
            layers.len() - tasks.len()
        ));

        // Execute downloads and cache results
        let results = self.execute_tasks(tasks, token, client).await?;

        // Cache downloaded blobs
        for (digest, data) in results {
            cache.add_blob(&digest, &data, false, true)?;
        }

        Ok(())
    }

    /// Calculate task priority based on blob size
    fn calculate_priority(&self, size: u64) -> u64 {
        if size <= self.config.small_blob_threshold {
            // Small blobs get highest priority (lowest numbers)
            size
        } else if size <= self.config.medium_blob_threshold {
            // Medium blobs get medium priority
            self.config.small_blob_threshold + size
        } else {
            // Large blobs get lowest priority (highest numbers)
            self.config.medium_blob_threshold + size
        }
    }

    /// Execute tasks with concurrency control
    async fn execute_tasks(
        &self,
        tasks: Vec<PipelineTask>,
        token: &Option<String>,
        client: Arc<RegistryClient>,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent));
        let total_size: u64 = tasks.iter().map(|t| t.layer.size).sum();
        let start_time = std::time::Instant::now();

        self.output.info(&format!(
            "Executing {} tasks ({}) with {} concurrent workers",
            tasks.len(),
            self.output.format_size(total_size),
            self.config.max_concurrent
        ));

        // Create task futures
        let task_futures: Vec<_> = tasks
            .into_iter()
            .map(|task| {
                let semaphore = Arc::clone(&semaphore);
                let client = Arc::clone(&client);
                let token = token.clone();
                let output = self.output.clone();
                let config = self.config.clone();

                tokio::spawn(async move {
                    Self::execute_single_task(task, token, client, semaphore, output, config).await
                })
            })
            .collect();

        // Wait for all tasks to complete
        let results = future::try_join_all(task_futures)
            .await
            .map_err(|e| RegistryError::Upload(format!("Task execution failed: {}", e)))?;

        // Collect successful results
        let mut successful_results = Vec::new();
        for result in results {
            match result {
                Ok(Some((digest, data))) => {
                    successful_results.push((digest, data));
                }
                Ok(None) => {
                    // Upload task completed successfully (no data to return)
                }
                Err(e) => return Err(e),
            }
        }

        let elapsed = start_time.elapsed();
        let avg_speed = if elapsed.as_secs() > 0 {
            total_size / elapsed.as_secs()
        } else {
            total_size
        };

        self.output.success(&format!(
            "All tasks completed successfully in {} (avg speed: {}/s)",
            self.output.format_duration(elapsed),
            self.output.format_size(avg_speed)
        ));

        Ok(successful_results)
    }

    /// Execute a single task (upload or download)
    async fn execute_single_task(
        task: PipelineTask,
        token: Option<String>,
        client: Arc<RegistryClient>,
        semaphore: Arc<Semaphore>,
        output: Logger,
        _config: PipelineConfig,
    ) -> Result<Option<(String, Vec<u8>)>> {
        // Acquire semaphore permit for concurrency control
        let _permit = semaphore
            .acquire()
            .await
            .map_err(|e| RegistryError::Upload(format!("Failed to acquire permit: {}", e)))?;

        let start_time = std::time::Instant::now();

        match task.operation {
            TaskOperation::Upload {
                upload_url: _,
                repository,
                tar_path,
            } => {
                output.detail(&format!(
                    "Uploading layer {} ({}) - priority {}",
                    task.index + 1,
                    output.format_size(task.layer.size),
                    task.priority
                ));

                // Extract layer data and upload using the client's upload method
                let layer_data =
                    crate::registry::TarUtils::extract_layer_data(&tar_path, &task.layer.tar_path)?;
                client
                    .upload_blob_with_token(&layer_data, &task.layer.digest, &repository, &token)
                    .await?;

                let elapsed = start_time.elapsed();
                output.success(&format!(
                    "Layer {} uploaded in {}",
                    task.index + 1,
                    output.format_duration(elapsed)
                ));

                Ok(None) // Upload tasks don't return data
            }
            TaskOperation::Download { repository } => {
                output.detail(&format!(
                    "Downloading blob {} ({}) - priority {}",
                    &task.layer.digest[..16],
                    output.format_size(task.layer.size),
                    task.priority
                ));

                // Download blob from registry
                let data = client
                    .pull_blob(&repository, &task.layer.digest, &token)
                    .await?;

                let elapsed = start_time.elapsed();
                let speed = if elapsed.as_secs() > 0 {
                    task.layer.size / elapsed.as_secs()
                } else {
                    task.layer.size
                };

                output.success(&format!(
                    "Blob {} downloaded in {} ({}/s)",
                    &task.layer.digest[..16],
                    output.format_duration(elapsed),
                    output.format_size(speed)
                ));

                Ok(Some((task.layer.digest.clone(), data)))
            }
        }
    }
}
