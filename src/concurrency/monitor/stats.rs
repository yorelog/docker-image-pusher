//! Statistics tracking for layer-level operations and progress monitoring

use std::time::{Duration, Instant};
use super::progress::ProgressTracker;
use crate::logging::Logger;

/// Layer-level upload/download statistics
#[derive(Debug, Clone)]
pub struct LayerStats {
    /// Layer digest
    pub digest: String,
    /// Layer size in bytes  
    pub size: u64,
    /// Processed bytes
    pub processed_bytes: u64,
    /// Start time
    pub start_time: Option<Instant>,
    /// Completion time
    pub completion_time: Option<Instant>,
    /// Status (pending, processing, completed, failed)
    pub status: LayerStatus,
    /// Error message if failed
    pub error_message: Option<String>,
}

/// Status of a layer operation
#[derive(Debug, Clone, PartialEq)]
pub enum LayerStatus {
    Pending,
    Processing,
    Completed,
    Skipped,
    Failed,
}

impl LayerStats {
    /// Create new layer statistics
    pub fn new(digest: String, size: u64) -> Self {
        Self {
            digest,
            size,
            processed_bytes: 0,
            start_time: None,
            completion_time: None,
            status: LayerStatus::Pending,
            error_message: None,
        }
    }

    /// Start processing the layer
    pub fn start_processing(&mut self) {
        self.status = LayerStatus::Processing;
        self.start_time = Some(Instant::now());
    }

    /// Update processing progress
    pub fn update_progress(&mut self, processed_bytes: u64) {
        self.processed_bytes = processed_bytes;
    }

    /// Mark layer as completed
    pub fn complete(&mut self) {
        self.status = LayerStatus::Completed;
        self.processed_bytes = self.size;
        self.completion_time = Some(Instant::now());
    }

    /// Mark layer as skipped
    pub fn skip(&mut self) {
        self.status = LayerStatus::Skipped;
        self.completion_time = Some(Instant::now());
    }

    /// Mark layer as failed
    pub fn fail(&mut self, error: String) {
        self.status = LayerStatus::Failed;
        self.error_message = Some(error);
        self.completion_time = Some(Instant::now());
    }

    /// Get processing duration
    pub fn duration(&self) -> Option<Duration> {
        match (self.start_time, self.completion_time) {
            (Some(start), Some(end)) => Some(end.duration_since(start)),
            _ => None,
        }
    }
}

/// Comprehensive operation statistics combining progress and layer stats
#[derive(Debug, Clone)]
pub struct OperationStats {
    /// Overall progress tracker
    pub progress: ProgressTracker,
    /// Per-layer statistics
    pub layers: Vec<LayerStats>,
    /// Total number of layers
    pub total_layers: usize,
    /// Successful layers count
    pub successful_layers: usize,
    /// Skipped layers count
    pub skipped_layers: usize,
    /// Failed layers count
    pub failed_layers: usize,
}

impl OperationStats {
    /// Create new operation statistics
    pub fn new(total_size: u64, total_layers: usize, output: Logger, operation_name: String) -> Self {
        Self {
            progress: ProgressTracker::new(total_size, output, operation_name),
            layers: Vec::with_capacity(total_layers),
            total_layers,
            successful_layers: 0,
            skipped_layers: 0,
            failed_layers: 0,
        }
    }

    /// Add a layer to track
    pub fn add_layer(&mut self, digest: String, size: u64) {
        self.layers.push(LayerStats::new(digest, size));
    }

    /// Start processing a layer by digest
    pub fn start_layer(&mut self, digest: &str) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.start_processing();
        }
    }

    /// Update layer progress
    pub fn update_layer(&mut self, digest: &str, processed_bytes: u64) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.update_progress(processed_bytes);
            
            // Update overall progress
            let total_processed: u64 = self.layers.iter().map(|l| l.processed_bytes).sum();
            self.progress.update(total_processed);
        }
    }

    /// Complete a layer
    pub fn complete_layer(&mut self, digest: &str) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.complete();
            self.successful_layers += 1;
            
            // Update overall progress
            let total_processed: u64 = self.layers.iter().map(|l| l.processed_bytes).sum();
            self.progress.update(total_processed);
        }
    }

    /// Skip a layer  
    pub fn skip_layer(&mut self, digest: &str) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.skip();
            self.skipped_layers += 1;
        }
    }

    /// Fail a layer
    pub fn fail_layer(&mut self, digest: &str, error: String) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.digest == digest) {
            layer.fail(error);
            self.failed_layers += 1;
        }
    }

    /// Generate final statistics report
    pub fn final_report(&mut self) {
        self.progress.complete();
        
        let total_elapsed = self.progress.start_time.elapsed();
        self.progress.output.info(&format!(
            "Operation Summary: {}/{} layers successful, {} skipped, {} failed in {:.1}s",
            self.successful_layers,
            self.total_layers,
            self.skipped_layers,
            self.failed_layers,
            total_elapsed.as_secs_f64()
        ));
    }
}
