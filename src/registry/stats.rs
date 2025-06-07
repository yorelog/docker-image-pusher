//! Upload statistics and progress reporting

use crate::logging::Logger;
use std::time::{Duration, Instant};

/// Upload statistics for tracking progress and performance
#[derive(Debug, Clone)]
pub struct UploadStats {
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
    pub total_layers: usize,
    pub uploaded_layers: usize,
    pub successful_layers: usize,
    pub skipped_layers: usize,
    pub failed_layers: usize,
    pub start_time: Instant,
    pub current_layer_start: Option<Instant>,
}

impl UploadStats {
    pub fn new(total_bytes: u64, total_layers: usize) -> Self {
        Self {
            total_bytes,
            uploaded_bytes: 0,
            total_layers,
            uploaded_layers: 0,
            successful_layers: 0,
            skipped_layers: 0,
            failed_layers: 0,
            start_time: Instant::now(),
            current_layer_start: None,
        }
    }

    /// Start tracking upload statistics
    pub fn start(&mut self) {
        self.start_time = Instant::now();
    }

    /// Set total number of layers
    pub fn set_total_layers(&mut self, count: usize) {
        self.total_layers = count;
    }

    /// Legacy method for compatibility with ProgressReporter
    pub fn start_layer(&mut self) {
        self.current_layer_start = Some(Instant::now());
    }

    /// Begin tracking a specific layer by digest and size
    pub fn begin_layer_upload(&mut self, _digest: &str, _size: u64) {
        self.current_layer_start = Some(Instant::now());
    }

    /// Mark a layer as completed successfully
    pub fn mark_layer_completed(&mut self, _digest: &str) {
        self.successful_layers += 1;
        self.uploaded_layers += 1;
        self.current_layer_start = None;
    }

    /// Mark a layer as skipped (already exists)
    pub fn mark_layer_skipped(&mut self, _digest: &str) {
        self.skipped_layers += 1;
        self.uploaded_layers += 1;
    }

    /// Mark a layer as failed
    pub fn mark_layer_failed(&mut self, _digest: &str, _error: String) {
        self.failed_layers += 1;
    }

    /// Get total duration of upload
    pub fn total_duration(&self) -> Option<Duration> {
        Some(self.start_time.elapsed())
    }

    pub fn finish_layer(&mut self, layer_bytes: u64) {
        self.uploaded_bytes += layer_bytes;
        self.uploaded_layers += 1;
        self.current_layer_start = None;
    }

    pub fn get_progress_percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            100.0
        } else {
            (self.uploaded_bytes as f64 / self.total_bytes as f64) * 100.0
        }
    }

    pub fn get_average_speed(&self) -> u64 {
        let elapsed = self.start_time.elapsed().as_secs();
        if elapsed > 0 {
            self.uploaded_bytes / elapsed
        } else {
            0
        }
    }

    pub fn get_eta(&self) -> Option<Duration> {
        if self.uploaded_bytes == 0 {
            return None;
        }

        let _elapsed = self.start_time.elapsed();
        let remaining_bytes = self.total_bytes.saturating_sub(self.uploaded_bytes);
        let speed = self.get_average_speed();

        if speed > 0 {
            Some(Duration::from_secs(remaining_bytes / speed))
        } else {
            None
        }
    }
}

/// Layer-specific upload statistics
#[derive(Debug, Clone)]
pub struct LayerUploadStats {
    pub digest: String,
    pub size: u64,
    pub uploaded: u64,
    pub start_time: Instant,
    pub status: LayerUploadStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayerUploadStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
    Failed(String),
}

impl LayerUploadStats {
    pub fn new(digest: String, size: u64) -> Self {
        Self {
            digest,
            size,
            uploaded: 0,
            start_time: Instant::now(),
            status: LayerUploadStatus::Pending,
        }
    }

    pub fn start(&mut self) {
        self.status = LayerUploadStatus::InProgress;
        self.start_time = Instant::now();
    }

    pub fn update_progress(&mut self, uploaded: u64) {
        self.uploaded = uploaded;
    }

    pub fn complete(&mut self) {
        self.status = LayerUploadStatus::Completed;
        self.uploaded = self.size;
    }

    pub fn skip(&mut self) {
        self.status = LayerUploadStatus::Skipped;
    }

    pub fn fail(&mut self, error: String) {
        self.status = LayerUploadStatus::Failed(error);
    }

    pub fn get_progress_percentage(&self) -> f64 {
        if self.size == 0 {
            100.0
        } else {
            (self.uploaded as f64 / self.size as f64) * 100.0
        }
    }

    pub fn get_speed(&self) -> u64 {
        let elapsed = self.start_time.elapsed().as_secs();
        if elapsed > 0 && self.uploaded > 0 {
            self.uploaded / elapsed
        } else {
            0
        }
    }
}

/// Progress reporter for upload operations
pub struct ProgressReporter {
    output: Logger,
    stats: UploadStats,
    layer_stats: Vec<LayerUploadStats>,
}

impl ProgressReporter {
    pub fn new(output: Logger, total_bytes: u64, total_layers: usize) -> Self {
        Self {
            output,
            stats: UploadStats::new(total_bytes, total_layers),
            layer_stats: Vec::new(),
        }
    }

    pub fn add_layer(&mut self, digest: String, size: u64) {
        self.layer_stats.push(LayerUploadStats::new(digest, size));
    }

    pub fn start_layer(&mut self, digest: &str) {
        self.stats.start_layer();
        if let Some(layer_stat) = self.layer_stats.iter_mut().find(|l| l.digest == digest) {
            layer_stat.start();
        }
    }

    pub fn update_layer_progress(&mut self, digest: &str, uploaded: u64) {
        if let Some(layer_stat) = self.layer_stats.iter_mut().find(|l| l.digest == digest) {
            layer_stat.update_progress(uploaded);
        }
    }

    pub fn finish_layer(&mut self, digest: &str, size: u64) {
        self.stats.finish_layer(size);
        if let Some(layer_stat) = self.layer_stats.iter_mut().find(|l| l.digest == digest) {
            layer_stat.complete();
        }
    }

    pub fn skip_layer(&mut self, digest: &str) {
        if let Some(layer_stat) = self.layer_stats.iter_mut().find(|l| l.digest == digest) {
            layer_stat.skip();
        }
    }

    pub fn fail_layer(&mut self, digest: &str, error: String) {
        if let Some(layer_stat) = self.layer_stats.iter_mut().find(|l| l.digest == digest) {
            layer_stat.fail(error);
        }
    }

    pub fn report_progress(&self) {
        let progress = self.stats.get_progress_percentage();
        let speed = self.stats.get_average_speed();

        self.output.info(&format!(
            "Upload progress: {:.1}% ({}/{} layers, avg speed: {})",
            progress,
            self.stats.uploaded_layers,
            self.stats.total_layers,
            self.output.format_speed(speed)
        ));

        if let Some(eta) = self.stats.get_eta() {
            self.output.detail(&format!(
                "Estimated time remaining: {}",
                self.output.format_duration(eta)
            ));
        }
    }

    pub fn report_final_stats(&self) {
        let total_time = self.stats.start_time.elapsed();
        let avg_speed = self.stats.get_average_speed();

        let completed = self
            .layer_stats
            .iter()
            .filter(|l| l.status == LayerUploadStatus::Completed)
            .count();
        let skipped = self
            .layer_stats
            .iter()
            .filter(|l| l.status == LayerUploadStatus::Skipped)
            .count();
        let failed = self
            .layer_stats
            .iter()
            .filter(|l| matches!(l.status, LayerUploadStatus::Failed(_)))
            .count();

        self.output.success(&format!(
            "Upload completed in {} - {} uploaded, {} skipped, {} failed (avg speed: {})",
            self.output.format_duration(total_time),
            completed,
            skipped,
            failed,
            self.output.format_speed(avg_speed)
        ));
    }
}
