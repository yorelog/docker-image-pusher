// This file contains functions for tracking and displaying upload progress to the user.

use std::io::{self, Write};
use std::time::Instant;

pub struct ProgressTracker {
    total_size: u64,
    uploaded_size: u64,
    start_time: Instant,
}

impl ProgressTracker {
    pub fn new(total_size: u64) -> Self {
        ProgressTracker {
            total_size,
            uploaded_size: 0,
            start_time: Instant::now(),
        }
    }

    pub fn update(&mut self, bytes_uploaded: u64) {
        self.uploaded_size += bytes_uploaded;
        self.display_progress();
    }

    fn display_progress(&self) {
        let percentage = (self.uploaded_size as f64 / self.total_size as f64) * 100.0;
        let elapsed_time = self.start_time.elapsed();
        let speed = if elapsed_time.as_secs() > 0 {
            self.uploaded_size as f64 / elapsed_time.as_secs_f64()
        } else {
            0.0
        };

        print!("\rUploading: {:.2}%, Speed: {:.2} bytes/sec", percentage, speed);
        io::stdout().flush().unwrap();
    }

    pub fn finish(&self) {
        println!("\nUpload completed successfully!");
    }
}