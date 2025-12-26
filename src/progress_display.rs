use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use oci_core::progress::{
    ChunkTransferEvent, LayerProgressComplete, LayerProgressInit, LayerProgressUpdate,
    ProgressReporter, ProgressReporterHandle,
};

const MB: f64 = 1024.0 * 1024.0;

pub fn docker_like_progress_reporter() -> ProgressReporterHandle {
    Arc::new(DockerLikeProgressReporter::new())
}

struct LayerStats {
    label: String,
    chunk_count: u64,
    total_bytes: u64,
    largest_chunk: usize,
    sent_bytes: u64,
    started_at: Option<Instant>,
    last_speed: f64,
    eta_seconds: Option<f64>,
    completed: bool,
    final_summary: Option<String>,
}

pub struct DockerLikeProgressReporter {
    stats: Mutex<HashMap<String, LayerStats>>,
    order: Mutex<Vec<String>>,
    rendered_lines: Mutex<usize>,
    interactive: bool
}

impl DockerLikeProgressReporter {
    pub fn new() -> Self {
        Self {
            stats: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
            rendered_lines: Mutex::new(0),
            interactive: io::stdout().is_terminal()
        }
    }

    fn ensure_layer(&self, digest: &str, total_bytes: u64) {
        let mut stats_guard = self.stats.lock().expect("progress stats poisoned");
        let entry = stats_guard
            .entry(digest.to_string())
            .or_insert_with(|| LayerStats {
                label: digest_label(digest),
                chunk_count: 0,
                total_bytes,
                largest_chunk: 0,
                sent_bytes: 0,
                started_at: Some(Instant::now()),
                last_speed: 0.0,
                eta_seconds: None,
                completed: false,
                final_summary: None,
            });
        entry.total_bytes = entry.total_bytes.max(total_bytes);
        entry.started_at.get_or_insert_with(Instant::now);
        drop(stats_guard);

        let mut order_guard = self.order.lock().expect("progress order poisoned");
        if !order_guard.iter().any(|value| value == digest) {
            order_guard.push(digest.to_string());
        }
    }

    fn with_stats_mut<F>(&self, digest: &str, mutator: F)
    where
        F: FnOnce(&mut LayerStats),
    {
        if let Some(entry) = self
            .stats
            .lock()
            .expect("progress stats poisoned")
            .get_mut(digest)
        {
            mutator(entry);
        }
    }

    fn snapshot_lines(&self) -> Vec<String> {
        let stats_guard = self.stats.lock().expect("progress stats poisoned");
        if stats_guard.is_empty() {
            return Vec::new();
        }

        let order_guard = self.order.lock().expect("progress order poisoned");
        let mut lines = Vec::with_capacity(order_guard.len() + 1);
        lines.extend(
            order_guard
                .iter()
                .filter_map(|digest| stats_guard.get(digest).map(LayerStats::render_line)),
        );
        lines
    }

    fn push_update(&self, _digest: &str) {
        if self.interactive {
            let lines = self.snapshot_lines();
            self.render_lines(&lines);
        } else {
            for line in self.snapshot_lines() {
                println!("{}", line);
            }
        }
    }

    fn render_lines(&self, lines: &[String]) {
        let mut rendered = self
            .rendered_lines
            .lock()
            .expect("progress render state poisoned");
        let mut stdout = io::stdout();
        if *rendered > 0 {
            write!(stdout, "\x1b[{}F", *rendered).ok();
        }
        for line in lines {
            write!(stdout, "\x1b[2K{}\n", line).ok();
        }
        stdout.flush().ok();
        *rendered = lines.len();
    }

    fn finalize_if_done(&self) {
        if !self.interactive {
            return;
        }
        let done = {
            let stats_guard = self.stats.lock().expect("progress stats poisoned");
            !stats_guard.is_empty() && stats_guard.values().all(|entry| entry.completed)
        };
        if done {
            let mut rendered = self
                .rendered_lines
                .lock()
                .expect("progress render state poisoned");
            if *rendered > 0 {
                let mut stdout = io::stdout();
                write!(stdout, "\x1b[{}E", *rendered).ok();
                stdout.flush().ok();
                *rendered = 0;
            }
        }
    }
}

impl ProgressReporter for DockerLikeProgressReporter {
    fn on_layer_start(&self, info: LayerProgressInit) {
        self.ensure_layer(&info.digest, info.total_bytes);
        self.with_stats_mut(&info.digest, |stats| {
            stats.chunk_count = 0;
            stats.largest_chunk = 0;
            stats.sent_bytes = 0;
            stats.completed = false;
            stats.final_summary = None;
            stats.started_at = Some(Instant::now());
        });
        self.push_update(&info.digest);
    }

    fn on_layer_progress(&self, update: LayerProgressUpdate) {
        self.with_stats_mut(&update.digest, |stats| {
            stats.sent_bytes = update.sent_bytes;
            stats.total_bytes = stats.total_bytes.max(update.total_bytes);
            stats.last_speed = update.speed_mbps;
            stats.eta_seconds = update.eta_seconds;
        });
        self.push_update(&update.digest);
    }

    fn on_layer_complete(&self, summary: LayerProgressComplete) {
        self.with_stats_mut(&summary.digest, |stats| {
            stats.sent_bytes = summary.total_bytes;
            stats.total_bytes = summary.total_bytes;
            stats.last_speed = summary.average_mbps;
            stats.eta_seconds = None;
            stats.completed = true;
            let (size_value, size_unit) = format_size(summary.total_bytes);
            let chunk_info = if stats.chunk_count > 0 {
                format!(
                    "{} chunk(s), largest {:.2} MB",
                    stats.chunk_count,
                    stats.largest_chunk as f64 / MB
                )
            } else {
                "single chunk".to_string()
            };
            stats.final_summary = Some(format!(
                "   {label:<12} ✅ {size_value:.2} {size_unit} in {duration:.1}s @ {average:.1} MB/s ({chunk_info})",
                label = stats.label,
                size_value = size_value,
                size_unit = size_unit,
                duration = summary.elapsed.as_secs_f64(),
                average = summary.average_mbps,
                chunk_info = chunk_info,
            ));
        });
        self.push_update(&summary.digest);
        self.finalize_if_done();
    }

    fn on_chunk_transferred(&self, chunk: ChunkTransferEvent) {
        self.with_stats_mut(&chunk.digest, |stats| {
            stats.chunk_count = stats.chunk_count.max(chunk.chunk_index);
            stats.largest_chunk = stats.largest_chunk.max(chunk.chunk_bytes);
            stats.sent_bytes = chunk.total_transferred;
            if let Some(total) = chunk.total_bytes {
                stats.total_bytes = stats.total_bytes.max(total);
            }
            stats.recalculate_speed();
        });
        self.push_update(&chunk.digest);
    }
}

impl LayerStats {
    fn recalculate_speed(&mut self) {
        if let Some(start) = self.started_at {
            let elapsed = start.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                let sent_mb = self.sent_bytes as f64 / MB;
                self.last_speed = sent_mb / elapsed;
                if self.total_bytes > 0
                    && self.sent_bytes < self.total_bytes
                    && self.last_speed > 0.0
                {
                    let remaining_mb = (self.total_bytes - self.sent_bytes) as f64 / MB;
                    self.eta_seconds = Some(remaining_mb / self.last_speed);
                } else {
                    self.eta_seconds = None;
                }
            }
        }
    }

    fn render_line(&self) -> String {
        if let Some(summary) = &self.final_summary {
            summary.clone()
        } else {
            let percent = if self.total_bytes > 0 {
                (self.sent_bytes as f64 / self.total_bytes as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            let (sent_value, sent_unit) = format_size(self.sent_bytes);
            let (total_value, total_unit) = format_size(self.total_bytes);
            let bar = render_progress_bar(percent / 100.0, 28);
            let speed_display = if self.last_speed > 0.0 {
                format!("{:.1} MB/s", self.last_speed)
            } else {
                "-- MB/s".to_string()
            };
            let eta_display = format_eta(self.eta_seconds);

            format!(
                "   {label:<12} [{bar}] {percent:>6.2}% {sent_value:>6.2} {sent_unit} / {total_value:>6.2} {total_unit} | {speed_display:<10} | ETA {eta_display}",
                label = self.label,
                bar = bar,
                percent = percent,
                sent_value = sent_value,
                sent_unit = sent_unit,
                total_value = total_value,
                total_unit = total_unit,
                speed_display = speed_display,
                eta_display = eta_display,
            )
        }
    }
}

impl Default for DockerLikeProgressReporter {
    fn default() -> Self {
        Self::new()
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

fn format_eta(value: Option<f64>) -> String {
    match value {
        Some(secs) if secs.is_finite() => {
            let total = secs.max(0.0).round() as u64;
            let minutes = total / 60;
            let seconds = total % 60;
            if minutes == 0 {
                format!("{}s", seconds)
            } else {
                format!("{}m{:02}s", minutes, seconds)
            }
        }
        _ => "--".to_string(),
    }
}

fn digest_label(digest: &str) -> String {
    let short = digest.split(':').last().unwrap_or(digest);
    let trimmed: String = short.chars().take(12).collect();
    if trimmed.is_empty() {
        digest.to_string()
    } else {
        trimmed
    }
}

