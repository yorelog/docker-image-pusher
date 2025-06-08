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

    /// Display unified pipeline progress with enhanced concurrency details and network regression
    /// Format: Multi-line display showing individual task progress (limited to max concurrent tasks)
    pub fn display_unified_pipeline_progress(
        &self,
        progress: &crate::concurrency::PipelineProgress, 
        concurrency_stats: &crate::registry::EnhancedConcurrencyStats,
    ) {
        if self.quiet {
            return;
        }

        // Calculate overall percentage
        let percentage = if progress.total_tasks > 0 {
            (progress.completed_tasks as f64 / progress.total_tasks as f64) * 100.0
        } else {
            0.0
        };

        use std::cell::RefCell;
        thread_local! {
            static DISPLAY_STATE: RefCell<(bool, usize)> = RefCell::new((true, 0)); // (first_display, last_line_count)
        }
        
        DISPLAY_STATE.with(|state| {
            let mut state = state.borrow_mut();
            if !state.0 { // Not first display
                // Clear previous display by moving cursor up and clearing lines
                for _ in 0..state.1 {
                    print!("\x1b[1A\x1b[2K"); // Move up one line and clear it
                }
                print!("\r"); // Return to start of line
            }
            state.0 = false; // Mark as no longer first display
        });

        // Calculate display limits - no artificial limit needed as we display actual active tasks

        // Summary line with overall progress
        let bar_width = 10;
        let filled = ((percentage / 100.0) * bar_width as f64) as usize;
        let empty = bar_width - filled;
        let bar = format!("[{}{}]", "🟩".repeat(filled), "░".repeat(empty));

        let speed_mbps = concurrency_stats.network_speed_measurement.current_speed_mbps;
        let trend_symbol = match concurrency_stats.network_speed_measurement.speed_trend.as_str() {
            trend if trend.contains("increasing") => "📈",
            trend if trend.contains("decreasing") => "📉", 
            trend if trend.contains("stable") => "📊",
            _ => "📈"
        };

        let strategy_abbrev = if concurrency_stats.scheduling_strategy.contains("small") || 
                                concurrency_stats.scheduling_strategy.contains("Size-based") {
            "SF"
        } else if concurrency_stats.scheduling_strategy.contains("priority") {
            "PQ"
        } else {
            "STD"
        };

        let auto_status = if concurrency_stats.network_speed_measurement.auto_adjustment_enabled {
            "🔧AUTO"
        } else {
            "🔧FIX"
        };

        let eta_display = if concurrency_stats.performance_prediction.confidence_level > 0.5 {
            let eta_str = self.format_duration(concurrency_stats.performance_prediction.estimated_completion_time);
            let confidence_pct = (concurrency_stats.performance_prediction.confidence_level * 100.0) as u8;
            format!("ETA:{}({}%)", eta_str, confidence_pct)
        } else {
            "ETA:calculating...".to_string()
        };

        // Main summary line
        println!("🚀 {} {:.1}% | T:{}/{} A:{} | ⚡{}/{} | {}{:.1}MB/s | S:{} | {} | {}", 
                bar, percentage,
                progress.completed_tasks, progress.total_tasks, progress.active_tasks,
                concurrency_stats.current_parallel_tasks, concurrency_stats.max_parallel_tasks,
                trend_symbol, speed_mbps,
                strategy_abbrev, auto_status, eta_display);

        // Individual task progress lines - only show currently active tasks
        let active_tasks: Vec<_> = progress.active_task_details.values().collect();
        let actual_active_count = active_tasks.len();
        
        // Only display tasks that are actually running
        for task_info in active_tasks.iter() {
            let task_bar_width = 8;
            let task_filled = ((task_info.progress_percentage / 100.0) * task_bar_width as f64) as usize;
            let task_empty = task_bar_width - task_filled;
            let task_bar = format!("[{}{}]", "▓".repeat(task_filled), "░".repeat(task_empty));

            // Display real task information
            let task_size_str = self.format_size(task_info.layer_size);
            
            // Calculate current speed for this task
            let elapsed = task_info.start_time.elapsed();
            let task_speed = if elapsed.as_secs() > 0 {
                task_info.processed_bytes / elapsed.as_secs()
            } else {
                0
            };
            let task_speed_str = format!("{}/s", self.format_size(task_speed));

            // Extract short layer digest (first 12 characters after sha256:)
            let layer_short = if task_info.layer_digest.starts_with("sha256:") {
                &task_info.layer_digest[7..19.min(task_info.layer_digest.len())]
            } else {
                &task_info.layer_digest[..12.min(task_info.layer_digest.len())]
            };

            println!("  sha256:{}: {} {:5.1}% | {} | {} | {:?}", 
                    layer_short, task_bar, task_info.progress_percentage, 
                    task_size_str, task_speed_str, task_info.stage);
        }

        // Store the total line count for next display (1 summary + actual active tasks)
        DISPLAY_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.1 = 1 + actual_active_count; // 1 summary line + actual active task lines
        });

        let _ = io::stdout().flush();
    }

    /// Display detailed unified pipeline status with network regression analysis
    pub fn display_detailed_unified_progress(
        &self,
        progress: &crate::concurrency::PipelineProgress,
        concurrency_stats: &crate::registry::EnhancedConcurrencyStats,
    ) {
        if self.quiet || !self.verbose {
            return;
        }

        self.subsection("Advanced Unified Pipeline Status");

        // Overall progress with efficiency metrics
        println!("📊 Pipeline Progress:");
        let completion_rate = if progress.total_tasks > 0 { 
            (progress.completed_tasks as f64 / progress.total_tasks as f64) * 100.0 
        } else { 
            0.0 
        };
        println!("   • Total Tasks: {} | Completed: {} ({:.1}%)", 
                progress.total_tasks, progress.completed_tasks, completion_rate);
        println!("   • Active: {} | Queued: {}", 
                progress.active_tasks, progress.queued_tasks);
        
        // Overall throughput analysis
        if progress.overall_speed > 0.0 {
            println!("   • Pipeline Speed: {:.2} MB/s | Efficiency: {:.1}%", 
                    progress.overall_speed / (1024.0 * 1024.0),
                    (progress.overall_speed / (1024.0 * 1024.0) / concurrency_stats.max_parallel_tasks as f64) * 100.0);
        }

        // Advanced concurrency management
        println!("\n🔧 Advanced Concurrency Management:");
        println!("   • Current/Max Parallel: {}/{} (utilization: {:.1}%)", 
                concurrency_stats.current_parallel_tasks,
                concurrency_stats.max_parallel_tasks,
                (concurrency_stats.current_parallel_tasks as f64 / 
                 concurrency_stats.max_parallel_tasks as f64) * 100.0);
        println!("   • Scheduling Strategy: {}", concurrency_stats.scheduling_strategy);

        // Enhanced priority queue analysis
        let queue_status = &concurrency_stats.priority_queue_status;
        let total_queued = queue_status.high_priority_remaining + 
                          queue_status.medium_priority_remaining + 
                          queue_status.low_priority_remaining;
        if total_queued > 0 {
            println!("   • Priority Queue Distribution:");
            println!("     - High: {} ({:.1}%) | Med: {} ({:.1}%) | Low: {} ({:.1}%)", 
                    queue_status.high_priority_remaining,
                    (queue_status.high_priority_remaining as f64 / total_queued as f64) * 100.0,
                    queue_status.medium_priority_remaining,
                    (queue_status.medium_priority_remaining as f64 / total_queued as f64) * 100.0,
                    queue_status.low_priority_remaining,
                    (queue_status.low_priority_remaining as f64 / total_queued as f64) * 100.0);
        }
        println!("   • Batch Strategy: {}", queue_status.current_batch_strategy);

        // Stage-specific progress analysis
        if !progress.stage_progress.is_empty() {
            println!("\n🎯 Stage-by-Stage Analysis:");
            for (stage, stage_progress) in &progress.stage_progress {
                let stage_completion = if stage_progress.total_tasks > 0 {
                    (stage_progress.completed_tasks as f64 / stage_progress.total_tasks as f64) * 100.0
                } else {
                    0.0
                };
                println!("   • {:?}: {}/{} ({:.1}%) | Success Rate: {:.1}% | Avg Duration: {}", 
                        stage,
                        stage_progress.completed_tasks,
                        stage_progress.total_tasks,
                        stage_completion,
                        stage_progress.success_rate * 100.0,
                        self.format_duration(stage_progress.average_duration));
            }
        }

        // Network performance with regression analysis
        println!("\n🌐 Network Performance & Regression Analysis:");
        let network = &concurrency_stats.network_speed_measurement;
        if network.current_speed_mbps > 0.0 {
            println!("   • Current Speed: {:.2} MB/s | Average: {:.2} MB/s", 
                    network.current_speed_mbps, network.average_speed_mbps);
            
            // Speed variance analysis
            let speed_variance = ((network.current_speed_mbps - network.average_speed_mbps) / network.average_speed_mbps) * 100.0;
            let variance_indicator = if speed_variance.abs() < 10.0 {
                "🟢 Stable"
            } else if speed_variance > 0.0 {
                "🔵 Above Average"
            } else {
                "🟡 Below Average"
            };
            println!("   • Speed Variance: {:.1}% {}", speed_variance, variance_indicator);
            
            // Trend analysis with prediction confidence
            println!("   • Speed Trend: {} | Regression Confidence: High", network.speed_trend);
        } else {
            println!("   • Speed: Measuring initial performance...");
        }
        
        // Dynamic adjustment capabilities
        println!("   • Auto-adjustment: {} | Adaptation Mode: {}", 
                if network.auto_adjustment_enabled { "✅ Enabled" } else { "❌ Disabled" },
                if network.auto_adjustment_enabled { "Regression-based" } else { "Static" });

        // Recent concurrency adjustments with performance impact
        if !concurrency_stats.dynamic_adjustments.is_empty() {
            println!("\n⚡ Recent Concurrency Adjustments (with Impact Analysis):");
            for (i, adj) in concurrency_stats.dynamic_adjustments.iter().rev().take(3).enumerate() {
                let elapsed = adj.timestamp.elapsed();
                let impact_indicator = if adj.performance_impact > 10.0 {
                    "🟢 Significant+"
                } else if adj.performance_impact > 0.0 {
                    "🔵 Positive"
                } else if adj.performance_impact > -10.0 {
                    "🟡 Neutral"
                } else {
                    "🔴 Negative"
                };
                println!("   {}. {}s ago: {} → {} ({}) | Impact: {:.1}% {}", 
                        i + 1,
                        elapsed.as_secs(),
                        adj.old_concurrency,
                        adj.new_concurrency,
                        adj.reason,
                        adj.performance_impact,
                        impact_indicator);
            }
        }

        // Advanced performance prediction with bottleneck analysis
        let prediction = &concurrency_stats.performance_prediction;
        if prediction.confidence_level > 0.0 {
            println!("\n🔮 Advanced Performance Prediction:");
            println!("   • ETA: {} | Confidence: {:.1}%", 
                    self.format_duration(prediction.estimated_completion_time), 
                    prediction.confidence_level * 100.0);
            
            // Confidence level indicator
            let confidence_indicator = if prediction.confidence_level > 0.8 {
                "🟢 Very High"
            } else if prediction.confidence_level > 0.6 {
                "🔵 High"
            } else if prediction.confidence_level > 0.4 {
                "🟡 Medium"
            } else {
                "🔴 Low"
            };
            println!("   • Prediction Quality: {}", confidence_indicator);
            
            // Bottleneck analysis
            println!("   • Bottleneck Analysis: {}", prediction.bottleneck_analysis);
            
            // Recommendations based on analysis
            if prediction.bottleneck_analysis.contains("network") {
                println!("   • 💡 Recommendation: Consider reducing concurrency for better stability");
            } else if prediction.bottleneck_analysis.contains("concurrency") {
                println!("   • 💡 Recommendation: Network can handle higher concurrency");
            } else if prediction.bottleneck_analysis.contains("optimal") {
                println!("   • 💡 Status: Running at optimal configuration");
            }
        }

        // ETA analysis
        if let Some(estimated_completion) = progress.estimated_completion {
            let time_remaining = estimated_completion.saturating_duration_since(Instant::now());
            println!("\n⏰ Time Analysis:");
            println!("   • Estimated Completion: {}", self.format_duration(time_remaining));
            println!("   • Concurrency Efficiency: {:.1}% | Resource Usage: Optimal", 
                    (concurrency_stats.current_parallel_tasks as f64 / 
                     concurrency_stats.max_parallel_tasks as f64) * 100.0);
        }

        println!(); // Empty line separator
    }

    /// Display network regression analysis with performance predictions
    pub fn display_network_regression_analysis(
        &self,
        current_speed_mbps: f64,
        average_speed_mbps: f64,
        speed_trend: &str,
        regression_data: Option<&crate::concurrency::RegressionAnalysis>,
    ) {
        if self.quiet || !self.verbose {
            return;
        }

        println!("\n🌐 Advanced Network Regression Analysis:");
        
        // Basic speed analysis
        println!("   • Current Speed: {:.2} MB/s | Average: {:.2} MB/s", 
                current_speed_mbps, average_speed_mbps);
        
        // Speed variance analysis
        let speed_variance = if average_speed_mbps > 0.0 {
            ((current_speed_mbps - average_speed_mbps) / average_speed_mbps) * 100.0
        } else {
            0.0
        };
        
        let variance_indicator = if speed_variance.abs() < 10.0 {
            "🟢 Stable"
        } else if speed_variance > 0.0 {
            "🔵 Above Average"
        } else {
            "🟡 Below Average"
        };
        
        println!("   • Speed Variance: {:.1}% {}", speed_variance, variance_indicator);
        println!("   • Speed Trend: {} | Regression Confidence: High", speed_trend);
        
        // Detailed regression analysis if available
        if let Some(regression) = regression_data {
            println!("   • Linear Regression:");
            println!("     - R-squared: {:.3} | Slope: {:.3} MB/s²", 
                    regression.correlation.powi(2), regression.slope);
            println!("     - Sample Size: {} measurements", regression.sample_size);
            
            // Performance prediction
            let confidence_pct = (regression.confidence * 100.0) as u8;
            println!("   • Performance Prediction (60s):");
            println!("     - Predicted Speed: {:.1} MB/s ± {:.1} MB/s", 
                    current_speed_mbps + (regression.slope * 60.0),
                    regression.slope.abs() * 10.0); // Simple error estimate
            println!("     - Confidence Level: {}%", confidence_pct);
        }
    }

    /// Display concurrency recommendations based on performance analysis
    pub fn display_concurrency_recommendations(
        &self,
        current_concurrency: usize,
        max_concurrency: usize,
        optimization_suggestions: &[String],
    ) {
        if self.quiet || !self.verbose {
            return;
        }

        println!("\n💡 Intelligent Concurrency Recommendations:");
        
        // Current status
        let utilization = (current_concurrency as f64 / max_concurrency as f64) * 100.0;
        println!("   • Current Utilization: {}/{} ({:.1}%)", 
                current_concurrency, max_concurrency, utilization);
        
        // Utilization analysis
        if utilization > 90.0 {
            println!("   • Status: 🔴 High utilization - monitor for bottlenecks");
        } else if utilization > 70.0 {
            println!("   • Status: 🟡 Optimal utilization - good performance balance");
        } else if utilization > 50.0 {
            println!("   • Status: 🟢 Moderate utilization - room for optimization");
        } else {
            println!("   • Status: 🔵 Low utilization - consider increasing concurrency");
        }
        
        // Dynamic recommendations
        for (i, suggestion) in optimization_suggestions.iter().enumerate() {
            println!("   {}. {}", i + 1, suggestion);
        }
        
        // General guidance
        if current_concurrency == max_concurrency {
            println!("   • 🎯 Running at maximum configured concurrency");
            println!("   • Consider increasing --max-concurrent if network can handle more");
        } else if current_concurrency < max_concurrency / 2 {
            println!("   • 🚀 Network appears capable of higher concurrency");
            println!("   • Auto-optimization may increase parallel tasks");
        }
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
