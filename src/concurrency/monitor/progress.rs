//! Progress tracking functionality for upload and download operations

use std::time::{Duration, Instant};
use crate::logging::Logger;

/// Data point representing a single transfer measurement
#[derive(Debug, Clone)]
pub struct SpeedDataPoint {
    /// Timestamp when measurement was taken
    pub timestamp: Instant,
    /// Number of bytes transferred
    pub bytes_transferred: u64,
    /// Time elapsed for this transfer
    pub duration: Duration,
    /// Calculated speed in bytes per second
    pub speed: u64,
}

impl SpeedDataPoint {
    /// Create a new speed data point
    pub fn new(bytes_transferred: u64, duration: Duration) -> Self {
        let speed = if duration.as_secs() > 0 {
            bytes_transferred / duration.as_secs()
        } else if duration.as_millis() > 0 {
            (bytes_transferred as u128 * 1000 / duration.as_millis()) as u64
        } else {
            bytes_transferred // Instantaneous transfer
        };

        Self {
            timestamp: Instant::now(),
            bytes_transferred,
            duration,
            speed,
        }
    }
}

/// Progress tracking for uploads and downloads with advanced reporting
#[derive(Debug, Clone)]
pub struct ProgressTracker {
    /// Total size of the operation
    pub total_size: u64,
    /// Currently processed bytes
    pub processed_bytes: u64,
    /// Start time of the operation
    pub start_time: Instant,
    /// Last update time
    pub last_update: Instant,
    /// Last processed bytes at last update
    pub last_processed: u64,
    /// Operation name for logging
    pub operation_name: String,
    /// Logger for output
    pub output: Logger,
    /// Update threshold (bytes)
    pub update_threshold: u64,
    /// Update interval (seconds)
    pub update_interval: Duration,
}

impl ProgressTracker {
    /// Create a new progress tracker
    pub fn new(total_size: u64, output: Logger, operation_name: String) -> Self {
        let update_threshold = std::cmp::min(10 * 1024 * 1024, total_size / 20); // 10MB or 5%
        
        Self {
            total_size,
            processed_bytes: 0,
            start_time: Instant::now(),
            last_update: Instant::now(),
            last_processed: 0,
            operation_name,
            output,
            update_threshold,
            update_interval: Duration::from_secs(5),
        }
    }

    /// Update progress with new processed bytes
    pub fn update(&mut self, processed_bytes: u64) {
        self.processed_bytes = processed_bytes;
        let now = Instant::now();
        let elapsed_since_last = now.duration_since(self.last_update);

        // Update progress based on thresholds
        if elapsed_since_last >= self.update_interval
            || processed_bytes - self.last_processed >= self.update_threshold
            || processed_bytes == self.total_size
        {
            let percent = if self.total_size > 0 {
                (processed_bytes as f64 / self.total_size as f64 * 100.0) as u8
            } else {
                0
            };

            let speed_mbps = if elapsed_since_last.as_secs() > 0 {
                let bytes_diff = processed_bytes - self.last_processed;
                bytes_diff / elapsed_since_last.as_secs() / 1024 / 1024
            } else {
                0
            };

            self.output.progress(&format!(
                "{}: {}% ({}/{}) - {} MB/s",
                self.operation_name,
                percent,
                self.output.format_size(processed_bytes),
                self.output.format_size(self.total_size),
                speed_mbps
            ));

            self.last_update = now;
            self.last_processed = processed_bytes;
        }
    }

    /// Force a progress update regardless of thresholds
    pub fn force_update(&mut self) {
        let percent = if self.total_size > 0 {
            (self.processed_bytes as f64 / self.total_size as f64 * 100.0) as u8
        } else {
            0
        };

        let total_elapsed = self.start_time.elapsed();
        let avg_speed_mbps = if total_elapsed.as_secs() > 0 {
            self.processed_bytes / total_elapsed.as_secs() / 1024 / 1024
        } else {
            0
        };

        self.output.progress(&format!(
            "{}: {}% ({}/{}) - Avg {} MB/s",
            self.operation_name,
            percent,
            self.output.format_size(self.processed_bytes),
            self.output.format_size(self.total_size),
            avg_speed_mbps
        ));
    }

    /// Complete the progress tracking with final statistics
    pub fn complete(&mut self) {
        self.processed_bytes = self.total_size;
        let total_elapsed = self.start_time.elapsed();
        let avg_speed_mbps = if total_elapsed.as_secs() > 0 {
            self.total_size / total_elapsed.as_secs() / 1024 / 1024
        } else {
            0
        };

        self.output.info(&format!(
            "{} completed: {} in {:.1}s (avg {} MB/s)",
            self.operation_name,
            self.output.format_size(self.total_size),
            total_elapsed.as_secs_f64(),
            avg_speed_mbps
        ));
    }
}

/// Active task information for progress display
#[derive(Debug, Clone)]
pub struct ActiveTaskInfo {
    /// Task identifier
    pub task_id: String,
    /// Task type (upload, download, etc.)
    pub task_type: String,
    /// Layer index in the operation
    pub layer_index: usize,
    /// Total size of the layer
    pub layer_size: u64,
    /// Currently processed bytes
    pub processed_bytes: u64,
    /// Currently processed bytes (alias for compatibility)
    pub bytes_processed: u64,
    /// Task start time
    pub start_time: Instant,
    /// Task priority level
    pub priority: u64,
    /// Estimated completion time
    pub estimated_completion: Option<Instant>,
}
