use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use oci_core::progress::{
    ChunkTransferEvent, LayerProgressComplete, LayerProgressInit, LayerProgressUpdate,
    ProgressReporter, ProgressReporterHandle,
};

const MB: f64 = 1024.0 * 1024.0;

pub fn docker_like_progress_reporter() -> ProgressReporterHandle {
    Arc::new(DockerLikeProgressReporter::default())
}

#[derive(Default)]
struct LayerStats {
    chunk_count: u64,
    total_bytes: u64,
    largest_chunk: usize,
}

#[derive(Default)]
pub struct DockerLikeProgressReporter {
    stats: Mutex<HashMap<String, LayerStats>>,
}

impl DockerLikeProgressReporter {
    fn digest_label(digest: &str) -> String {
        let short = digest.split(':').last().unwrap_or(digest);
        let trimmed: String = short.chars().take(12).collect();
        if trimmed.is_empty() {
            digest.to_string()
        } else {
            trimmed
        }
    }

    fn format_size(bytes: u64) -> (f64, &'static str) {
        let mb_value = bytes as f64 / MB;
        if mb_value > 1024.0 {
            (mb_value / 1024.0, "GB")
        } else {
            (mb_value, "MB")
        }
    }

    fn render_progress_bar(progress_ratio: f64, width: usize) -> String {
        let clamped = progress_ratio.clamp(0.0, 1.0);
        let filled = (clamped * width as f64).floor() as usize;
        let mut bar = String::with_capacity(width);
        for index in 0..width {
            if index < filled {
                bar.push('=');
            } else if index == filled {
                bar.push('>');
            } else {
                bar.push(' ');
            }
        }
        bar
    }

    fn format_eta(seconds: Option<f64>) -> String {
        match seconds {
            Some(value) if value.is_finite() => {
                let total_seconds = value.max(0.0).round() as u64;
                let minutes = total_seconds / 60;
                let secs = total_seconds % 60;
                if minutes == 0 {
                    format!("{}s", secs)
                } else {
                    format!("{}m{:02}s", minutes, secs)
                }
            }
            _ => "--".to_string(),
        }
    }

    fn with_stats<F>(&self, digest: &str, create: bool, mutator: F)
    where
        F: FnOnce(&mut LayerStats),
    {
        let mut guard = self.stats.lock().expect("progress stats poisoned");
        let entry = if create {
            guard.entry(digest.to_string()).or_default()
        } else if let Some(existing) = guard.get_mut(digest) {
            existing
        } else {
            return;
        };
        mutator(entry);
    }

    fn take_stats(&self, digest: &str) -> Option<LayerStats> {
        let mut guard = self.stats.lock().expect("progress stats poisoned");
        guard.remove(digest)
    }
}

impl ProgressReporter for DockerLikeProgressReporter {
    fn on_layer_start(&self, info: LayerProgressInit) {
        self.with_stats(&info.digest, true, |_| {});
        let label = Self::digest_label(&info.digest);
        let chunk_size_mb = info.chunk_size_bytes as f64 / MB;
        println!(
            "   🛰️  {} planning {} chunk(s) (~{:.1} MB each)",
            label, info.estimated_chunks, chunk_size_mb
        );
    }

    fn on_layer_progress(&self, update: LayerProgressUpdate) {
        let percent = if update.total_bytes > 0 {
            (update.sent_bytes as f64 / update.total_bytes as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        let (sent_display, sent_unit) = Self::format_size(update.sent_bytes);
        let (total_display, total_unit) = Self::format_size(update.total_bytes);
        let bar = Self::render_progress_bar(percent / 100.0, 28);
        let eta_display = Self::format_eta(update.eta_seconds);
        let label = Self::digest_label(&update.digest);

        println!(
            "   {}: [{}] {:.2} {} / {:.2} {} ({:.1}%) @ {:.1} MB/s ETA {}",
            label,
            bar,
            sent_display,
            sent_unit,
            total_display,
            total_unit,
            percent,
            update.speed_mbps,
            eta_display
        );
    }

    fn on_layer_complete(&self, summary: LayerProgressComplete) {
        let label = Self::digest_label(&summary.digest);
        if let Some(stats) = self.take_stats(&summary.digest) {
            let largest_mb = stats.largest_chunk as f64 / MB;
            println!(
                "   🧾 {}: completed {} chunk(s), largest {:.2} MB, total {:.2} GB in {:.1}s @ {:.1} MB/s",
                label,
                stats.chunk_count,
                largest_mb,
                summary.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                summary.elapsed.as_secs_f64(),
                summary.average_mbps
            );
        } else {
            println!(
                "   🧾 {}: completed transfer in {:.1}s @ {:.1} MB/s",
                label,
                summary.elapsed.as_secs_f64(),
                summary.average_mbps
            );
        }
    }

    fn on_chunk_transferred(&self, chunk: ChunkTransferEvent) {
        self.with_stats(&chunk.digest, false, |stats| {
            stats.chunk_count = stats.chunk_count.max(chunk.chunk_index);
            stats.total_bytes = chunk.total_transferred;
            stats.largest_chunk = stats.largest_chunk.max(chunk.chunk_bytes);
        });
    }
}
