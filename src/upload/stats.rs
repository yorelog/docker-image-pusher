//! Upload statistics and monitoring

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct UploadStats {
    pub total_layers: usize,
    pub completed_layers: usize,
    pub failed_layers: usize,
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
    pub start_time: Instant,
    pub concurrent_uploads: usize,
    pub avg_speed: u64,
    pub peak_speed: u64,
}

impl UploadStats {
    pub fn new(total_layers: usize, total_bytes: u64, concurrent_uploads: usize) -> Self {
        Self {
            total_layers,
            completed_layers: 0,
            failed_layers: 0,
            total_bytes,
            uploaded_bytes: 0,
            start_time: Instant::now(),
            concurrent_uploads,
            avg_speed: 0,
            peak_speed: 0,
        }
    }

    pub fn add_completed_layer(&mut self, layer_size: u64) {
        self.completed_layers += 1;
        self.uploaded_bytes += layer_size;
        self.update_speeds();
    }

    pub fn add_failed_layer(&mut self) {
        self.failed_layers += 1;
    }

    fn update_speeds(&mut self) {
        let elapsed = self.start_time.elapsed().as_secs();
        if elapsed > 0 {
            let current_speed = self.uploaded_bytes / elapsed;
            self.avg_speed = current_speed;
            if current_speed > self.peak_speed {
                self.peak_speed = current_speed;
            }
        }
    }

    pub fn completion_percentage(&self) -> f64 {
        if self.total_layers == 0 {
            100.0
        } else {
            (self.completed_layers as f64 / self.total_layers as f64) * 100.0
        }
    }

    pub fn estimated_time_remaining(&self) -> Option<Duration> {
        if self.avg_speed > 0 && self.uploaded_bytes < self.total_bytes {
            let remaining_bytes = self.total_bytes - self.uploaded_bytes;
            let remaining_seconds = remaining_bytes / self.avg_speed;
            Some(Duration::from_secs(remaining_seconds))
        } else {
            None
        }
    }
}

pub type SharedStats = Arc<Mutex<UploadStats>>;
