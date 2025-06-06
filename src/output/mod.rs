//! Enhanced output control module with structured logging
//!
//! This module provides the [`OutputManager`] for controlling output verbosity, formatting logs,
//! and tracking operation timing. It supports quiet, verbose, and structured output for CI and debugging.

use std::io::{self, Write};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct OutputManager {
    pub verbose: bool,
    quiet: bool,
    start_time: Option<Instant>,
}

impl OutputManager {
    pub fn new(verbose: bool) -> Self {
        Self {
            verbose,
            quiet: false,
            start_time: Some(Instant::now()),
        }
    }

    pub fn new_quiet() -> Self {
        Self {
            verbose: false,
            quiet: true,
            start_time: Some(Instant::now()),
        }
    }

    // Structured logging levels
    pub fn trace(&self, message: &str) {
        if self.verbose && !self.quiet {
            println!("🔍 TRACE: {}", message);
        }
    }

    pub fn debug(&self, message: &str) {
        if self.verbose && !self.quiet {
            println!("🐛 DEBUG: {}", message);
        }
    }

    pub fn verbose(&self, message: &str) {
        if self.verbose && !self.quiet {
            println!("📝 {}", message);
        }
    }

    pub fn info(&self, message: &str) {
        if !self.quiet {
            println!("ℹ️  {}", message);
        }
    }

    pub fn success(&self, message: &str) {
        if !self.quiet {
            println!("✅ {}", message);
        }
    }

    pub fn warning(&self, message: &str) {
        if !self.quiet {
            println!("⚠️  WARNING: {}", message);
        }
    }

    pub fn error(&self, message: &str) {
        eprintln!("❌ ERROR: {}", message);
    }

    // Progress indicators
    pub fn progress(&self, message: &str) {
        if !self.quiet {
            print!("⏳ {}...", message);
            let _ = io::stdout().flush();
        }
    }

    pub fn progress_done(&self) {
        if !self.quiet {
            println!(" ✓");
        }
    }

    // Section headers
    pub fn section(&self, title: &str) {
        if !self.quiet {
            println!("\n🔧 {}", title);
            println!("{}", "=".repeat(title.len() + 3));
        }
    }

    pub fn subsection(&self, title: &str) {
        if !self.quiet {
            println!("\n📂 {}", title);
            println!("{}", "-".repeat(title.len() + 3));
        }
    }

    pub fn step(&self, step: &str) {
        if !self.quiet {
            println!("  🚀 {}", step);
        }
    }

    pub fn detail(&self, detail: &str) {
        if self.verbose && !self.quiet {
            println!("    📋 {}", detail);
        }
    }

    // Summary method for displaying structured information
    pub fn summary(&self, title: &str, items: &[String]) {
        if !self.quiet {
            println!("\n📋 {}", title);
            println!("{}", "─".repeat(title.len() + 3));

            for item in items {
                println!("  • {}", item);
            }

            if items.is_empty() {
                println!("  (No items to display)");
            }
        }
    }

    // Alternative summary method for key-value pairs
    pub fn summary_kv(&self, title: &str, items: &[(&str, String)]) {
        if !self.quiet {
            println!("\n📋 {}", title);
            println!("{}", "─".repeat(title.len() + 3));

            // Find the maximum key length for alignment
            let max_key_len = items.iter().map(|(key, _)| key.len()).max().unwrap_or(0);

            for (key, value) in items {
                println!("  {:width$}: {}", key, value, width = max_key_len);
            }

            if items.is_empty() {
                println!("  (No items to display)");
            }
        }
    }

    // Structured list output
    pub fn list(&self, title: &str, items: &[String]) {
        if !self.quiet {
            if !title.is_empty() {
                println!("\n📝 {}", title);
            }

            for (index, item) in items.iter().enumerate() {
                println!("  {}. {}", index + 1, item);
            }

            if items.is_empty() && !title.is_empty() {
                println!("  (No items in list)");
            }
        }
    }

    // Table-like output for structured data
    pub fn table(&self, headers: &[&str], rows: &[Vec<String>]) {
        if !self.quiet && !headers.is_empty() {
            // Calculate column widths
            let mut col_widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();

            for row in rows {
                for (i, cell) in row.iter().enumerate() {
                    if i < col_widths.len() {
                        col_widths[i] = col_widths[i].max(cell.len());
                    }
                }
            }

            // Print header
            print!("  ");
            for (i, header) in headers.iter().enumerate() {
                print!("{:width$}", header, width = col_widths[i]);
                if i < headers.len() - 1 {
                    print!(" │ ");
                }
            }
            println!();

            // Print separator
            print!("  ");
            for (i, &width) in col_widths.iter().enumerate() {
                print!("{}", "─".repeat(width));
                if i < col_widths.len() - 1 {
                    print!("─┼─");
                }
            }
            println!();

            // Print rows
            for row in rows {
                print!("  ");
                for (i, cell) in row.iter().enumerate() {
                    if i < col_widths.len() {
                        print!("{:width$}", cell, width = col_widths[i]);
                        if i < headers.len() - 1 {
                            print!(" │ ");
                        }
                    }
                }
                println!();
            }
        }
    }

    // Enhanced progress with metrics
    pub fn progress_with_metrics(&self, current: u64, total: u64, operation: &str) {
        if !self.quiet {
            let percentage = if total > 0 {
                (current as f64 / total as f64) * 100.0
            } else {
                100.0
            };

            println!(
                "📊 {}: {}/{} ({:.1}%)",
                operation,
                self.format_size(current),
                self.format_size(total),
                percentage
            );
        }
    }

    // Progress bar visualization
    pub fn progress_bar(&self, current: u64, total: u64, operation: &str, width: usize) {
        if !self.quiet {
            let percentage = if total > 0 {
                (current as f64 / total as f64) * 100.0
            } else {
                100.0
            };

            let filled = (width as f64 * (percentage / 100.0)) as usize;
            let empty = width - filled;

            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

            print!(
                "\r📊 {}: [{}] {:.1}% ({}/{})",
                operation,
                bar,
                percentage,
                self.format_size(current),
                self.format_size(total)
            );

            let _ = io::stdout().flush();

            if current >= total {
                println!(); // New line when complete
            }
        }
    }

    // Helper methods for formatting
    pub fn format_size(&self, bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} {}", bytes, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }

    pub fn format_duration(&self, duration: Duration) -> String {
        let total_seconds = duration.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }

    // Format speed (bytes per second)
    pub fn format_speed(&self, bytes_per_second: u64) -> String {
        format!("{}/s", self.format_size(bytes_per_second))
    }

    // Format percentage
    pub fn format_percentage(&self, current: u64, total: u64) -> String {
        if total == 0 {
            "100.0%".to_string()
        } else {
            format!("{:.1}%", (current as f64 / total as f64) * 100.0)
        }
    }

    // Get elapsed time since start
    pub fn elapsed_time(&self) -> Duration {
        self.start_time
            .map(|start| start.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0))
    }

    // Reset start time
    pub fn reset_timer(&mut self) {
        self.start_time = Some(Instant::now());
    }
}
