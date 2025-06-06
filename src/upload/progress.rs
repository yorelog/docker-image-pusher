//! Progress tracking for uploads

use crate::output::OutputManager;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct ProgressTracker {
    total_size: u64,
    start_time: Instant,
    last_update: Instant,
    last_uploaded: u64,
    output: OutputManager,
    operation_name: String,
}

impl ProgressTracker {
    pub fn new(total_size: u64, output: OutputManager, operation_name: String) -> Self {
        Self {
            total_size,
            start_time: Instant::now(),
            last_update: Instant::now(),
            last_uploaded: 0,
            output,
            operation_name,
        }
    }

    pub fn update(&mut self, uploaded: u64) {
        let now = Instant::now();
        let elapsed_since_last = now.duration_since(self.last_update);

        // Update progress every 5 seconds or 10MB or every 5% of total
        let size_threshold = std::cmp::min(10 * 1024 * 1024, self.total_size / 20); // 10MB or 5% of total

        if elapsed_since_last >= Duration::from_secs(5)
            || uploaded - self.last_uploaded >= size_threshold
            || uploaded == self.total_size
        {
            self.output
                .progress_with_metrics(uploaded, self.total_size, &self.operation_name);
            self.last_update = now;
            self.last_uploaded = uploaded;
        }
    }

    pub fn finish(&self) {
        self.output
            .progress_with_metrics(self.total_size, self.total_size, &self.operation_name);

        let total_elapsed = self.start_time.elapsed();
        let avg_speed = if total_elapsed.as_secs() > 0 {
            self.total_size / total_elapsed.as_secs()
        } else {
            self.total_size
        };

        self.output.success(&format!(
            "{} completed in {} (avg speed: {})",
            self.operation_name,
            self.output.format_duration(total_elapsed),
            self.output.format_size(avg_speed)
        ));
    }

    pub fn set_phase(&mut self, phase: &str) {
        self.operation_name = format!("{} - {}", self.operation_name, phase);
    }

    pub fn get_elapsed_time(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn get_estimated_remaining(&self, uploaded: u64) -> Option<Duration> {
        let elapsed = self.start_time.elapsed();
        if uploaded > 0 && elapsed.as_secs() > 0 {
            let speed = uploaded / elapsed.as_secs();
            if speed > 0 {
                let remaining_bytes = self.total_size.saturating_sub(uploaded);
                return Some(Duration::from_secs(remaining_bytes / speed));
            }
        }
        None
    }
}
