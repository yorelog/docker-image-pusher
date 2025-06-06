//! Parallel upload implementation with concurrency control

use crate::error::{Result, PusherError};
use crate::output::OutputManager;
use crate::upload::{ProgressTracker, UploadStrategyFactory};
use crate::image::parser::LayerInfo;
use crate::registry::RegistryClient;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use futures::future::try_join_all;

pub struct ParallelUploader {
    client: Arc<RegistryClient>,
    max_concurrent: usize,
    large_layer_threshold: u64,
    output: OutputManager,
    timeout: u64,
}

#[derive(Debug)]
pub struct UploadTask {
    pub layer: LayerInfo,
    pub index: usize,
    pub upload_url: String,
    pub repository: String,
}

impl ParallelUploader {
    pub fn new(
        client: Arc<RegistryClient>,
        max_concurrent: usize,
        large_layer_threshold: u64,
        timeout: u64,
        output: OutputManager,
    ) -> Self {
        Self {
            client,
            max_concurrent,
            large_layer_threshold,
            output,
            timeout,
        }
    }

    pub async fn upload_layers_parallel(
        &self,
        layers: Vec<LayerInfo>,
        repository: &str,
        tar_path: &std::path::Path,
        token: &Option<String>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let total_size: u64 = layers.iter().map(|l| l.size).sum();
        
        self.output.section("Parallel Layer Upload");
        self.output.info(&format!(
            "Uploading {} layers ({}) with {} concurrent connections",
            layers.len(),
            self.output.format_size(total_size),
            self.max_concurrent
        ));

        // Create semaphore for concurrency control
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        
        // Create progress tracker
        let progress_tracker = Arc::new(tokio::sync::Mutex::new(
            ProgressTracker::new(total_size, self.output.clone(), "Parallel Upload".to_string())
        ));
        
        // Prepare upload tasks
        let mut upload_tasks = Vec::new();
        for (index, layer) in layers.into_iter().enumerate() {
            // Start upload session for each layer
            let upload_url = self.client.start_upload_session(repository).await?;
            
            upload_tasks.push(UploadTask {
                layer,
                index,
                upload_url,
                repository: repository.to_string(),
            });
        }

        self.output.info(&format!("Created {} upload sessions", upload_tasks.len()));

        // Execute uploads in parallel
        let upload_futures = upload_tasks.into_iter().map(|task| {
            self.upload_single_layer(
                task,
                tar_path,
                token,
                Arc::clone(&semaphore),
                Arc::clone(&progress_tracker),
            )
        });

        // Wait for all uploads to complete
        let results = try_join_all(upload_futures).await?;
        
        let elapsed = start_time.elapsed();
        let avg_speed = if elapsed.as_secs() > 0 {
            total_size / elapsed.as_secs()
        } else {
            total_size
        };

        // Finalize progress
        {
            let tracker = progress_tracker.lock().await;
            tracker.finish();
        }

        self.output.success(&format!(
            "All {} layers uploaded successfully in {} (avg speed: {}/s)",
            results.len(),
            self.output.format_duration(elapsed),
            self.output.format_size(avg_speed)
        ));

        Ok(())
    }

    async fn upload_single_layer(
        &self,
        task: UploadTask,
        tar_path: &std::path::Path,
        token: &Option<String>,
        semaphore: Arc<Semaphore>,
        progress_tracker: Arc<tokio::sync::Mutex<ProgressTracker>>,
    ) -> Result<()> {
        // Acquire semaphore permit
        let _permit = semaphore.acquire().await
            .map_err(|e| PusherError::Upload(format!("Failed to acquire upload permit: {}", e)))?;

        let layer_start = Instant::now();
        
        // Replace unstable thread_id with a stable alternative
        let thread_info = format!("task-{}", task.index);
        
        self.output.detail(&format!(
            "Starting upload for layer {} ({}) - {}",
            task.index + 1,
            self.output.format_size(task.layer.size),
            thread_info
        ));        let result = {
            // Create strategy factory
            let factory = UploadStrategyFactory::new(
                self.large_layer_threshold,
                self.timeout,
                self.output.clone()
            );
            
            // Get appropriate strategy for this layer
            let strategy = factory.get_strategy(&task.layer);
            
            // Use the strategy to upload the layer
            strategy.upload_layer(
                &task.layer,
                &task.repository,
                tar_path,
                token,
                &task.upload_url,
            ).await
        };match result {
            Ok(_) => {
                let elapsed = layer_start.elapsed();
                let speed = if elapsed.as_secs() > 0 {
                    task.layer.size / elapsed.as_secs()
                } else {
                    task.layer.size
                };

                self.output.success(&format!(
                    "Layer {} completed in {} ({}/s)",
                    task.index + 1,
                    self.output.format_duration(elapsed),
                    self.output.format_size(speed)
                ));

                // Update overall progress
                {
                    let mut tracker = progress_tracker.lock().await;
                    tracker.update(task.layer.size);
                }

                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Layer {} failed: {}", task.index + 1, e);
                self.output.error(&error_msg);
                Err(e)
            }
        }
    }
}