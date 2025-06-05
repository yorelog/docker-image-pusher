//! Enhanced output control module with structured logging

use std::io::{self, Write};
use std::time::{Instant, Duration};

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
        if self.verbose {
            self.print_with_timestamp("TRACE", message, "🔍");
        }
    }

    pub fn debug(&self, message: &str) {
        if self.verbose {
            self.print_with_timestamp("DEBUG", message, "🐛");
        }
    }

    pub fn verbose(&self, message: &str) {
        if self.verbose {
            self.print_with_timestamp("INFO", message, "ℹ️");
        }
    }

    pub fn info(&self, message: &str) {
        if !self.quiet {
            self.print_with_timestamp("INFO", message, "ℹ️");
        }
    }

    pub fn success(&self, message: &str) {
        if !self.quiet {
            self.print_with_timestamp("SUCCESS", message, "✅");
        }
    }

    pub fn warning(&self, message: &str) {
        self.print_with_timestamp("WARN", message, "⚠️");
    }

    pub fn error(&self, message: &str) {
        self.print_with_timestamp("ERROR", message, "❌");
    }

    // Progress indicators
    pub fn progress(&self, message: &str) {
        if self.quiet {
            return;
        }
        
        if self.verbose {
            println!("⏳ {}", message);
        } else {
            print!("⏳ {}...", message);
            io::stdout().flush().unwrap();
        }
    }

    pub fn progress_done(&self) {
        if !self.quiet && !self.verbose {
            println!(" ✓");
        }
    }

    // Section headers
    pub fn section(&self, title: &str) {
        if self.quiet {
            return;
        }
        
        if self.verbose {
            let separator = "━".repeat(60);
            println!("\n{}", separator);
            println!("📋 {}", title);
            println!("{}", separator);
        } else {
            println!("\n📋 {}", title);
        }
    }

    pub fn subsection(&self, title: &str) {
        if self.verbose {
            println!("  📂 {}", title);
        }
    }

    pub fn step(&self, step: &str) {
        if self.verbose {
            println!("    🔸 {}", step);
        }
    }

    pub fn detail(&self, detail: &str) {
        if self.verbose {
            println!("      📝 {}", detail);
        }
    }

    // Enhanced progress with metrics
    pub fn progress_with_metrics(&self, current: u64, total: u64, operation: &str) {
        if self.quiet {
            return;
        }

        let percentage = if total > 0 {
            (current as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        let speed = if let Some(start_time) = self.start_time {
            let elapsed = start_time.elapsed().as_secs();
            if elapsed > 0 {
                current / elapsed
            } else {
                0
            }
        } else {
            0
        };

        if self.verbose {
            println!("📊 {}: {:.1}% ({} / {}) | Speed: {}/s", 
                     operation,
                     percentage,
                     self.format_size(current),
                     self.format_size(total),
                     self.format_size(speed));
        } else {
            print!("\r⏳ {}: {:.1}% ({}/{}) {}    ", 
                   operation,
                   percentage,
                   self.format_size(current),
                   self.format_size(total),
                   if speed > 0 { format!("| {}/s", self.format_size(speed)) } else { String::new() });
            io::stdout().flush().unwrap();
        }
    }

    // Helper methods
    fn print_with_timestamp(&self, level: &str, message: &str, emoji: &str) {
        let timestamp = if let Some(start_time) = self.start_time {
            format!("[{:8.3}s]", start_time.elapsed().as_secs_f64())
        } else {
            String::new()
        };

        if self.verbose {
            println!("{} {} {} {}", timestamp, emoji, level, message);
        } else {
            println!("{} {}", emoji, message);
        }
    }

    pub fn format_size(&self, size: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = size as f64;
        let mut unit_index = 0;
        
        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }
        
        if unit_index == 0 {
            format!("{} {}", size as u64, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }

    pub fn format_duration(&self, duration: Duration) -> String {
        let secs = duration.as_secs();
        if secs < 60 {
            format!("{:.1}s", duration.as_secs_f64())
        } else if secs < 3600 {
            format!("{}m{:02}s", secs / 60, secs % 60)
        } else {
            format!("{}h{:02}m{:02}s", secs / 3600, (secs % 3600) / 60, secs % 60)
        }
    }

    // Summary and statistics
    pub fn summary(&self, title: &str, items: &[(&str, String)]) {
        if self.quiet {
            return;
        }

        println!("\n📊 {}", title);
        for (key, value) in items {
            println!("  • {}: {}", key, value);
        }
    }

    pub fn elapsed_time(&self) -> String {
        if let Some(start_time) = self.start_time {
            self.format_duration(start_time.elapsed())
        } else {
            "Unknown".to_string()
        }
    }
}