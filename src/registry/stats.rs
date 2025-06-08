//! Statistics tracking and reporting
//!
//! This module provides reusable statistics functionality for tracking
//! upload/download progress and performance metrics.

use crate::common::{FormatUtils, ProgressUtils};
use crate::logging::Logger;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Upload statistics tracker
#[derive(Debug, Clone)]
pub struct UploadStats {
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
    pub total_layers: usize,
    pub completed_layers: usize,
    pub failed_layers: usize,
    pub skipped_layers: usize,
    pub start_time: Option<Instant>,
    pub layer_stats: HashMap<String, LayerUploadStats>,
}

impl UploadStats {
    pub fn new(total_bytes: u64, total_layers: usize) -> Self {
        Self {
            total_bytes,
            uploaded_bytes: 0,
            total_layers,
            completed_layers: 0,
            failed_layers: 0,
            skipped_layers: 0,
            start_time: None,
            layer_stats: HashMap::new(),
        }
    }

    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
    }

    pub fn set_total_layers(&mut self, count: usize) {
        self.total_layers = count;
    }

    pub fn begin_layer_upload(&mut self, digest: &str, size: u64) {
        let layer_stats = LayerUploadStats::new(digest.to_string(), size);
        self.layer_stats.insert(digest.to_string(), layer_stats);
    }

    pub fn mark_layer_completed(&mut self, digest: &str) {
        if let Some(layer_stats) = self.layer_stats.get_mut(digest) {
            layer_stats.complete();
            self.completed_layers += 1;
            self.uploaded_bytes += layer_stats.size;
        }
    }

    pub fn mark_layer_skipped(&mut self, digest: &str) {
        if let Some(layer_stats) = self.layer_stats.get_mut(digest) {
            layer_stats.skip();
            self.skipped_layers += 1;
        }
    }

    pub fn mark_layer_failed(&mut self, digest: &str, error: String) {
        if let Some(layer_stats) = self.layer_stats.get_mut(digest) {
            layer_stats.fail(error);
            self.failed_layers += 1;
        }
    }

    pub fn update_layer_progress(&mut self, digest: &str, uploaded: u64) {
        if let Some(layer_stats) = self.layer_stats.get_mut(digest) {
            layer_stats.update_progress(uploaded);
        }
    }

    pub fn get_overall_progress(&self) -> f64 {
        ProgressUtils::calculate_percentage(self.uploaded_bytes, self.total_bytes)
    }

    pub fn get_layer_progress(&self) -> f64 {
        ProgressUtils::calculate_percentage(
            self.completed_layers as u64 + self.skipped_layers as u64,
            self.total_layers as u64,
        )
    }

    pub fn get_speed(&self) -> u64 {
        if let Some(start_time) = self.start_time {
            ProgressUtils::calculate_speed(self.uploaded_bytes, start_time.elapsed())
        } else {
            0
        }
    }

    pub fn get_eta(&self) -> Option<Duration> {
        if let Some(start_time) = self.start_time {
            ProgressUtils::estimate_remaining_time(
                self.uploaded_bytes,
                self.total_bytes,
                start_time.elapsed(),
            )
        } else {
            None
        }
    }

    pub fn is_complete(&self) -> bool {
        self.completed_layers + self.skipped_layers >= self.total_layers
    }

    pub fn has_failures(&self) -> bool {
        self.failed_layers > 0
    }
}

/// Individual layer upload statistics
#[derive(Debug, Clone)]
pub struct LayerUploadStats {
    pub digest: String,
    pub size: u64,
    pub uploaded_bytes: u64,
    pub status: LayerStatus,
    pub start_time: Option<Instant>,
    pub end_time: Option<Instant>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayerStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
    Failed,
}

impl LayerUploadStats {
    pub fn new(digest: String, size: u64) -> Self {
        Self {
            digest,
            size,
            uploaded_bytes: 0,
            status: LayerStatus::Pending,
            start_time: None,
            end_time: None,
            error_message: None,
        }
    }

    pub fn start(&mut self) {
        self.status = LayerStatus::InProgress;
        self.start_time = Some(Instant::now());
    }

    pub fn update_progress(&mut self, uploaded: u64) {
        self.uploaded_bytes = uploaded;
        if self.status == LayerStatus::Pending {
            self.start();
        }
    }

    pub fn complete(&mut self) {
        self.status = LayerStatus::Completed;
        self.end_time = Some(Instant::now());
        self.uploaded_bytes = self.size;
    }

    pub fn skip(&mut self) {
        self.status = LayerStatus::Skipped;
        self.end_time = Some(Instant::now());
    }

    pub fn fail(&mut self, error: String) {
        self.status = LayerStatus::Failed;
        self.end_time = Some(Instant::now());
        self.error_message = Some(error);
    }

    pub fn get_progress_percentage(&self) -> f64 {
        ProgressUtils::calculate_percentage(self.uploaded_bytes, self.size)
    }

    pub fn get_speed(&self) -> u64 {
        if let Some(start_time) = self.start_time {
            ProgressUtils::calculate_speed(self.uploaded_bytes, start_time.elapsed())
        } else {
            0
        }
    }

    pub fn get_duration(&self) -> Option<Duration> {
        if let (Some(start), Some(end)) = (self.start_time, self.end_time) {
            Some(end.duration_since(start))
        } else if let Some(start) = self.start_time {
            Some(start.elapsed())
        } else {
            None
        }
    }
}

/// Progress reporter for upload operations
pub struct ProgressReporter {
    output: Logger,
    stats: UploadStats,
    last_report_time: Instant,
    report_interval: Duration,
}

impl ProgressReporter {
    pub fn new(output: Logger, total_bytes: u64, total_layers: usize) -> Self {
        Self {
            output,
            stats: UploadStats::new(total_bytes, total_layers),
            last_report_time: Instant::now(),
            report_interval: Duration::from_secs(1),
        }
    }

    pub fn add_layer(&mut self, digest: String, size: u64) {
        self.stats.begin_layer_upload(&digest, size);
    }

    pub fn start_layer(&mut self, digest: &str) {
        if let Some(layer_stats) = self.stats.layer_stats.get_mut(digest) {
            layer_stats.start();
        }
    }

    pub fn update_layer_progress(&mut self, digest: &str, uploaded: u64) {
        self.stats.update_layer_progress(digest, uploaded);
        self.report_if_needed();
    }

    pub fn finish_layer(&mut self, digest: &str, _size: u64) {
        self.stats.mark_layer_completed(digest);
        self.output.verbose(&format!("Layer {} completed", digest));
    }

    pub fn skip_layer(&mut self, digest: &str) {
        self.stats.mark_layer_skipped(digest);
        self.output.verbose(&format!("Layer {} skipped (already exists)", digest));
    }

    pub fn fail_layer(&mut self, digest: &str, error: String) {
        self.stats.mark_layer_failed(digest, error.clone());
        self.output.error(&format!("Layer {} failed: {}", digest, error));
    }

    pub fn report_progress(&self) {
        let progress = self.stats.get_overall_progress();
        let layer_progress = self.stats.get_layer_progress();
        let speed = self.stats.get_speed();
        
        let progress_bar = ProgressUtils::create_progress_bar(progress, 20);
        
        if let Some(eta) = self.stats.get_eta() {
            self.output.info(&format!(
                "{} {:.1}% | Layers: {:.1}% | Speed: {} | ETA: {}",
                progress_bar,
                progress,
                layer_progress,
                FormatUtils::format_speed(speed),
                FormatUtils::format_duration(eta)
            ));
        } else {
            self.output.info(&format!(
                "{} {:.1}% | Layers: {:.1}% | Speed: {}",
                progress_bar,
                progress,
                layer_progress,
                FormatUtils::format_speed(speed)
            ));
        }
    }

    pub fn report_final_stats(&self) {
        let total_time = if let Some(start_time) = self.stats.start_time {
            start_time.elapsed()
        } else {
            Duration::from_secs(0)
        };

        let avg_speed = if total_time.as_secs() > 0 {
            self.stats.uploaded_bytes / total_time.as_secs()
        } else {
            0
        };

        self.output.success("Upload completed!");
        self.output.info(&format!("Total time: {}", FormatUtils::format_duration(total_time)));
        self.output.info(&format!("Average speed: {}", FormatUtils::format_speed(avg_speed)));
        self.output.info(&format!(
            "Layers: {} completed, {} skipped, {} failed",
            self.stats.completed_layers,
            self.stats.skipped_layers,
            self.stats.failed_layers
        ));
    }

    fn report_if_needed(&self) {
        if self.last_report_time.elapsed() >= self.report_interval {
            self.report_progress();
        }
    }

    pub fn get_stats(&self) -> &UploadStats {
        &self.stats
    }
}

/// Session statistics for tracking performance
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub start_time: Instant,
    pub operations_completed: u64,
    pub bytes_transferred: u64,
    pub errors_encountered: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

impl Default for SessionStats {
    fn default() -> Self {
        Self {
            start_time: Instant::now(),
            operations_completed: 0,
            bytes_transferred: 0,
            errors_encountered: 0,
            cache_hits: 0,
            cache_misses: 0,
        }
    }
}

impl SessionStats {
    pub fn record_operation(&mut self, bytes: u64) {
        self.operations_completed += 1;
        self.bytes_transferred += bytes;
    }

    pub fn record_error(&mut self) {
        self.errors_encountered += 1;
    }

    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    pub fn record_cache_miss(&mut self) {
        self.cache_misses += 1;
    }

    pub fn get_average_speed(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.bytes_transferred as f64 / elapsed
        } else {
            0.0
        }
    }

    pub fn get_cache_hit_rate(&self) -> f64 {
        let total_cache_operations = self.cache_hits + self.cache_misses;
        if total_cache_operations > 0 {
            (self.cache_hits as f64 / total_cache_operations as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn get_error_rate(&self) -> f64 {
        if self.operations_completed > 0 {
            (self.errors_encountered as f64 / self.operations_completed as f64) * 100.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upload_stats() {
        let mut stats = UploadStats::new(1000, 5);
        assert_eq!(stats.get_overall_progress(), 0.0);
        assert!(!stats.is_complete());

        stats.begin_layer_upload("layer1", 200);
        stats.mark_layer_completed("layer1");
        
        assert_eq!(stats.completed_layers, 1);
        assert_eq!(stats.uploaded_bytes, 200);
        assert_eq!(stats.get_overall_progress(), 20.0);
    }

    #[test]
    fn test_layer_upload_stats() {
        let mut layer = LayerUploadStats::new("sha256:abc123".to_string(), 1000);
        assert_eq!(layer.status, LayerStatus::Pending);

        layer.update_progress(500);
        assert_eq!(layer.status, LayerStatus::InProgress);
        assert_eq!(layer.get_progress_percentage(), 50.0);

        layer.complete();
        assert_eq!(layer.status, LayerStatus::Completed);
        assert_eq!(layer.uploaded_bytes, 1000);
    }

    #[test]
    fn test_session_stats() {
        let mut stats = SessionStats::default();
        
        stats.record_operation(100);
        stats.record_cache_hit();
        stats.record_cache_miss();
        
        assert_eq!(stats.operations_completed, 1);
        assert_eq!(stats.bytes_transferred, 100);
        assert_eq!(stats.get_cache_hit_rate(), 50.0);
    }
}
