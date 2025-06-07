//! Enhanced logging and output control
//!
//! This module provides the [`Logger`] for controlling output verbosity, formatting logs,
//! and tracking operation timing. It supports quiet, verbose, and structured output.

use std::io::{self, Write};
use std::time::{Duration, Instant};

/// Logger responsible for all user-visible output
#[derive(Debug, Clone)]
pub struct Logger {
    pub verbose: bool,
    pub quiet: bool,
    pub start_time: Option<Instant>,
}

impl Logger {
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

    /// Main section heading
    pub fn section(&self, title: &str) {
        if !self.quiet {
            println!("\n=== {} ===", title);
        }
    }

    /// Sub-section heading
    pub fn subsection(&self, title: &str) {
        if !self.quiet {
            println!("\n--- {} ---", title);
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

    /// Information message
    pub fn info(&self, message: &str) {
        if !self.quiet {
            println!("ℹ️  {}", message);
        }
    }

    /// Success message
    pub fn success(&self, message: &str) {
        if !self.quiet {
            println!("✅ {}", message);
        }
    }

    /// Warning message
    pub fn warning(&self, message: &str) {
        if !self.quiet {
            println!("⚠️  WARNING: {}", message);
        }
    }

    /// Error message
    pub fn error(&self, message: &str) {
        eprintln!("❌ ERROR: {}", message);
    }

    /// Step information
    pub fn step(&self, message: &str) {
        if !self.quiet {
            println!("▶️  {}", message);
        }
    }

    /// Progress information
    pub fn progress(&self, message: &str) {
        if !self.quiet {
            print!("⏳ {}...", message);
            let _ = io::stdout().flush();
        }
    }

    /// Progress completion
    pub fn progress_done(&self) {
        if !self.quiet {
            println!(" Done");
        }
    }

    /// Detailed information (only shown in verbose mode)
    pub fn detail(&self, message: &str) {
        if self.verbose && !self.quiet {
            println!("   {}", message);
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

    /// Key-value pair summary display
    pub fn summary_kv(&self, title: &str, items: &[(&str, String)]) {
        if !self.quiet {
            self.subsection(title);
            for (key, value) in items {
                println!("  {}: {}", key, value);
            }
        }
    }

    // Structured list output
    pub fn list(&self, title: &str, items: &[String]) {
        if !self.quiet {
            self.subsection(title);
            for (i, item) in items.iter().enumerate() {
                println!("  {}. {}", i + 1, item);
            }

            if items.is_empty() {
                println!("  (No items to display)");
            }
        }
    }

    /// Format file size in human-readable units
    pub fn format_size(&self, bytes: u64) -> String {
        if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else if bytes < 1024 * 1024 * 1024 {
            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    /// Format duration in human-readable format
    pub fn format_duration(&self, duration: Duration) -> String {
        let secs = duration.as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m{}s", secs / 60, secs % 60)
        } else {
            format!("{}h{}m{}s", secs / 3600, (secs % 3600) / 60, secs % 60)
        }
    }

    /// Format transfer speed in human-readable format
    pub fn format_speed(&self, bytes_per_sec: u64) -> String {
        format!("{}/s", self.format_size(bytes_per_sec))
    }
}
