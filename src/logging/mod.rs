//! Enhanced logging and output control
//!
//! This module provides the [`Logger`] for controlling output verbosity, formatting logs,
//! and tracking operation timing. It supports quiet, verbose, and structured output.

use std::io::{self, Write};
use std::time::{Duration, Instant};
use std::collections::HashMap;

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

    /// 显示实时进度状态
    pub fn display_live_progress(&self, progress: &ProgressState) {
        if self.quiet {
            return;
        }

        // 清除当前行并重新定位光标
        print!("\r\x1b[K");

        let percentage = progress.get_progress_percentage();
        let current_speed = progress.get_current_speed();
        
        // 创建进度条
        let bar_width = 30;
        let filled = ((percentage / 100.0) * bar_width as f64) as usize;
        let empty = bar_width - filled;
        let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));

        // 基础进度信息
        print!("⏳ {} {:.1}% | ", bar, percentage);
        print!("{}/{} tasks | ", progress.completed_tasks, progress.total_tasks);
        print!("{} active | ", progress.active_tasks);
        print!("{} | ", self.format_speed(current_speed));

        // 并发信息
        if progress.current_concurrent != progress.max_concurrent {
            print!("🔧 {}/{} concurrent | ", progress.current_concurrent, progress.max_concurrent);
        } else {
            print!("{} concurrent | ", progress.current_concurrent);
        }

        // 剩余时间估计
        if let Some(eta) = progress.get_estimated_time_remaining() {
            print!("ETA: {}", self.format_duration(eta));
        } else {
            print!("ETA: calculating...");
        }

        let _ = io::stdout().flush();
    }

    /// Display detailed parallel task status
    pub fn display_detailed_progress(&self, progress: &ProgressState) {
        if self.quiet || !self.verbose {
            return;
        }

        self.subsection("Detailed Progress Status");
        
        // Overall statistics
        println!("📊 Overall Progress:");
        println!("   • Total Tasks: {}", progress.total_tasks);
        println!("   • Completed: {} ({:.1}%)", progress.completed_tasks, 
                (progress.completed_tasks as f64 / progress.total_tasks as f64) * 100.0);
        println!("   • Active: {}", progress.active_tasks);
        println!("   • Data: {} / {} ({:.1}%)", 
                self.format_size(progress.processed_bytes),
                self.format_size(progress.total_bytes),
                progress.get_progress_percentage());

        // Concurrency status
        println!("\n🔧 Concurrency Status:");
        println!("   • Current: {}/{}", progress.current_concurrent, progress.max_concurrent);
        
        // Concurrency adjustment history
        if !progress.concurrency_adjustments.is_empty() {
            println!("   • Recent Adjustments:");
            for adjustment in progress.concurrency_adjustments.iter().rev().take(3) {
                let elapsed = adjustment.timestamp.elapsed();
                println!("     - {}s ago: {} → {} ({})", 
                        elapsed.as_secs(),
                        adjustment.old_value,
                        adjustment.new_value,
                        adjustment.reason);
            }
        }

        // Active task details
        if !progress.active_task_details.is_empty() {
            println!("\n🔄 Active Tasks:");
            let mut tasks: Vec<_> = progress.active_task_details.values().collect();
            tasks.sort_by_key(|t| t.layer_index);
            
            for task in tasks.iter().take(5) { // Show only first 5 tasks
                let task_progress = if task.layer_size > 0 {
                    (task.processed_bytes as f64 / task.layer_size as f64) * 100.0
                } else {
                    0.0
                };
                let elapsed = task.start_time.elapsed();
                let speed = if elapsed.as_secs() > 0 {
                    task.processed_bytes / elapsed.as_secs()
                } else {
                    0
                };

                println!("   • Layer {}: {} {:.1}% ({}) - {} - Priority: {}", 
                        task.layer_index + 1,
                        task.task_type,
                        task_progress,
                        self.format_size(task.layer_size),
                        self.format_speed(speed),
                        task.priority);
            }
            
            if progress.active_task_details.len() > 5 {
                println!("   • ... and {} more tasks", progress.active_task_details.len() - 5);
            }
        }

        println!(); // Empty line separator
    }

    /// Display concurrency adjustment notification
    pub fn notify_concurrency_adjustment(&self, old_value: usize, new_value: usize, reason: &str) {
        if !self.quiet {
            if new_value > old_value {
                println!("🔼 Concurrency increased: {} → {} ({})", old_value, new_value, reason);
            } else if new_value < old_value {
                println!("🔽 Concurrency decreased: {} → {} ({})", old_value, new_value, reason);
            }
        }
    }

    /// Display task start notification
    pub fn notify_task_start(&self, task_type: &str, layer_index: usize, size: u64, priority: u64) {
        if self.verbose && !self.quiet {
            println!("🚀 Starting {} task: Layer {} ({}) - Priority: {}", 
                    task_type, layer_index + 1, self.format_size(size), priority);
        }
    }

    /// Display task completion notification
    pub fn notify_task_complete(&self, task_type: &str, layer_index: usize, duration: Duration, size: u64) {
        if self.verbose && !self.quiet {
            let speed = if duration.as_secs() > 0 {
                size / duration.as_secs()
            } else {
                size
            };
            println!("✅ Completed {} task: Layer {} in {} ({})", 
                    task_type, layer_index + 1, self.format_duration(duration), self.format_speed(speed));
        }
    }

    /// Display simple progress information
    pub fn display_simple_progress(&self, completed: usize, total: usize, message: &str) {
        if self.quiet {
            return;
        }

        // Calculate overall percentage
        let percentage = if total > 0 {
            (completed as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        // Create simple progress bar
        let bar_width = 20;
        let filled = ((percentage / 100.0) * bar_width as f64) as usize;
        let empty = bar_width - filled;
        let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));

        // Display progress with message
        println!("⏳ {} {:.1}% | {}/{} {} | {}", 
                bar, percentage, completed, total, 
                if total > 1 { "tasks" } else { "task" }, message);

        let _ = io::stdout().flush();
    }

    // ...existing code...
}

/// 进度跟踪状态
#[derive(Debug, Clone)]
pub struct ProgressState {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub active_tasks: usize,
    pub max_concurrent: usize,
    pub current_concurrent: usize,
    pub total_bytes: u64,
    pub processed_bytes: u64,
    pub start_time: Instant,
    pub active_task_details: HashMap<String, TaskProgress>,
    pub concurrency_adjustments: Vec<ConcurrencyAdjustment>,
}

/// 单个任务的进度信息
#[derive(Debug, Clone)]
pub struct TaskProgress {
    pub task_id: String,
    pub task_type: String, // "upload" or "download"
    pub layer_index: usize,
    pub layer_size: u64,
    pub processed_bytes: u64,
    pub start_time: Instant,
    pub priority: u64,
}

/// 并发调整记录
#[derive(Debug, Clone)]
pub struct ConcurrencyAdjustment {
    pub timestamp: Instant,
    pub old_value: usize,
    pub new_value: usize,
    pub reason: String,
}

impl ProgressState {
    pub fn new(total_tasks: usize, max_concurrent: usize, total_bytes: u64) -> Self {
        Self {
            total_tasks,
            completed_tasks: 0,
            active_tasks: 0,
            max_concurrent,
            current_concurrent: max_concurrent,
            total_bytes,
            processed_bytes: 0,
            start_time: Instant::now(),
            active_task_details: HashMap::new(),
            concurrency_adjustments: Vec::new(),
        }
    }

    pub fn add_task(&mut self, task: TaskProgress) {
        self.active_task_details.insert(task.task_id.clone(), task);
        self.active_tasks = self.active_task_details.len();
    }

    pub fn update_task_progress(&mut self, task_id: &str, processed_bytes: u64) {
        if let Some(task) = self.active_task_details.get_mut(task_id) {
            let old_processed = task.processed_bytes;
            task.processed_bytes = processed_bytes;
            self.processed_bytes += processed_bytes - old_processed;
        }
    }

    pub fn complete_task(&mut self, task_id: &str) {
        if let Some(task) = self.active_task_details.remove(task_id) {
            self.completed_tasks += 1;
            self.active_tasks = self.active_task_details.len();
            // 确保已处理字节数包含完成的任务
            self.processed_bytes += task.layer_size - task.processed_bytes;
        }
    }

    pub fn adjust_concurrency(&mut self, new_concurrent: usize, reason: String) {
        let adjustment = ConcurrencyAdjustment {
            timestamp: Instant::now(),
            old_value: self.current_concurrent,
            new_value: new_concurrent,
            reason,
        };
        self.concurrency_adjustments.push(adjustment);
        self.current_concurrent = new_concurrent;
    }

    pub fn get_progress_percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            (self.completed_tasks as f64 / self.total_tasks as f64) * 100.0
        } else {
            (self.processed_bytes as f64 / self.total_bytes as f64) * 100.0
        }
    }

    pub fn get_estimated_time_remaining(&self) -> Option<Duration> {
        let elapsed = self.start_time.elapsed();
        if self.processed_bytes == 0 || elapsed.as_secs() == 0 {
            return None;
        }

        let rate = self.processed_bytes as f64 / elapsed.as_secs_f64();
        let remaining_bytes = self.total_bytes - self.processed_bytes;
        let estimated_seconds = remaining_bytes as f64 / rate;
        
        Some(Duration::from_secs_f64(estimated_seconds))
    }

    pub fn get_current_speed(&self) -> u64 {
        let elapsed = self.start_time.elapsed();
        if elapsed.as_secs() == 0 {
            0
        } else {
            self.processed_bytes / elapsed.as_secs()
        }
    }
}
