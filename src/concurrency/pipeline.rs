//! Pipeline Management for Concurrency Control
//!
//! This module provides pipeline-aware concurrency management,
//! integrating task scheduling with performance optimization.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use crate::concurrency::{ConcurrencyError, ConcurrencyResult};

/// Pipeline execution stages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelineStage {
    Download,
    Upload,
    Verification,
    Compression,
}

/// Pipeline task definition
#[derive(Debug, Clone)]
pub struct PipelineTask {
    pub id: String,
    pub stage: PipelineStage,
    pub priority: u32,
    pub estimated_size: u64,
    pub estimated_duration: Duration,
    pub dependencies: Vec<String>,
    pub metadata: HashMap<String, String>,
}

/// Active task information for progress display
#[derive(Debug, Clone)]
pub struct ActiveTaskInfo {
    pub task_id: String,
    pub layer_digest: String,
    pub layer_size: u64,
    pub processed_bytes: u64,
    pub start_time: Instant,
    pub stage: PipelineStage,
    pub progress_percentage: f64,
}

/// Pipeline progress tracking
#[derive(Debug, Clone)]
pub struct PipelineProgress {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub active_tasks: usize,
    pub queued_tasks: usize,
    pub stage_progress: HashMap<PipelineStage, StageProgress>,
    pub estimated_completion: Option<Instant>,
    pub overall_speed: f64,
    pub active_task_details: HashMap<String, ActiveTaskInfo>,
}

/// Stage-specific progress information
#[derive(Debug, Clone)]
pub struct StageProgress {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub active_tasks: usize,
    pub average_duration: Duration,
    pub success_rate: f64,
}

/// Pipeline manager for coordinating tasks across stages
#[derive(Debug)]
pub struct PipelineManager {
    tasks: HashMap<String, PipelineTask>,
    active_tasks: HashMap<String, TaskExecution>,
    completed_tasks: Vec<String>,
    task_queue: VecDeque<String>,
    stage_queues: HashMap<PipelineStage, VecDeque<String>>,
    dependency_graph: HashMap<String, Vec<String>>,
    pipeline_start_time: Option<Instant>,
}

/// Active task execution tracking
#[derive(Debug)]
struct TaskExecution {
    task_id: String,
    start_time: Instant,
    stage: PipelineStage,
    bytes_processed: u64,
    estimated_completion: Option<Instant>,
}

impl PipelineManager {
    /// Create a new pipeline manager
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            active_tasks: HashMap::new(),
            completed_tasks: Vec::new(),
            task_queue: VecDeque::new(),
            stage_queues: HashMap::new(),
            dependency_graph: HashMap::new(),
            pipeline_start_time: None,
        }
    }

    /// Register a new task in the pipeline
    pub fn register_task(&mut self, task: PipelineTask) -> ConcurrencyResult<()> {
        let task_id = task.id.clone();
        
        // Build dependency graph
        for dep in &task.dependencies {
            self.dependency_graph
                .entry(dep.clone())
                .or_insert_with(Vec::new)
                .push(task_id.clone());
        }

        // Add to appropriate stage queue
        self.stage_queues
            .entry(task.stage)
            .or_insert_with(VecDeque::new)
            .push_back(task_id.clone());

        // Store task
        self.tasks.insert(task_id.clone(), task);
        
        // Add to main queue if no dependencies
        if self.tasks[&task_id].dependencies.is_empty() {
            self.task_queue.push_back(task_id);
        }

        Ok(())
    }

    /// Get the next available task for execution
    pub fn get_next_task(&mut self, stage: Option<PipelineStage>) -> Option<PipelineTask> {
        if let Some(stage) = stage {
            // Get task from specific stage
            if let Some(queue) = self.stage_queues.get_mut(&stage) {
                if let Some(task_id) = queue.pop_front() {
                    return self.tasks.get(&task_id).cloned();
                }
            }
        } else {
            // Get any available task
            while let Some(task_id) = self.task_queue.pop_front() {
                if self.are_dependencies_satisfied(&task_id) {
                    return self.tasks.get(&task_id).cloned();
                }
                // Re-queue if dependencies not satisfied
                self.task_queue.push_back(task_id);
            }
        }
        None
    }

    /// Mark task as started
    pub fn start_task(&mut self, task_id: &str) -> ConcurrencyResult<()> {
        if let Some(task) = self.tasks.get(task_id) {
            // Set pipeline start time on first task start
            if self.pipeline_start_time.is_none() {
                self.pipeline_start_time = Some(Instant::now());
            }
            
            let execution = TaskExecution {
                task_id: task_id.to_string(),
                start_time: Instant::now(),
                stage: task.stage,
                bytes_processed: 0,
                estimated_completion: Some(Instant::now() + task.estimated_duration),
            };
            
            self.active_tasks.insert(task_id.to_string(), execution);
            Ok(())
        } else {
            Err(ConcurrencyError::TaskNotFound(task_id.to_string()))
        }
    }

    /// Update task progress
    pub fn update_task_progress(&mut self, task_id: &str, bytes_processed: u64) -> ConcurrencyResult<()> {
        if let Some(execution) = self.active_tasks.get_mut(task_id) {
            execution.bytes_processed = bytes_processed;
            Ok(())
        } else {
            Err(ConcurrencyError::TaskNotFound(task_id.to_string()))
        }
    }

    /// Mark task as completed
    pub fn complete_task(&mut self, task_id: &str) -> ConcurrencyResult<()> {
        if self.active_tasks.remove(task_id).is_some() {
            self.completed_tasks.push(task_id.to_string());
            
            // Check for newly available tasks
            self.update_available_tasks(task_id);
            
            Ok(())
        } else {
            Err(ConcurrencyError::TaskNotFound(task_id.to_string()))
        }
    }

    /// Get current pipeline progress
    pub fn get_progress(&self) -> PipelineProgress {
        let total_tasks = self.tasks.len();
        let completed_tasks = self.completed_tasks.len();
        let active_tasks = self.active_tasks.len();
        let queued_tasks = total_tasks - completed_tasks - active_tasks;

        // Calculate stage progress
        let mut stage_progress = HashMap::new();
        for stage in [PipelineStage::Download, PipelineStage::Upload, 
                     PipelineStage::Verification, PipelineStage::Compression] {
            stage_progress.insert(stage, self.calculate_stage_progress(stage));
        }

        // Estimate completion time
        let estimated_completion = self.estimate_completion_time();
        
        // Calculate overall speed
        let overall_speed = self.calculate_overall_speed();

        // Collect active task details
        let mut active_task_details = HashMap::new();
        for (task_id, execution) in &self.active_tasks {
            if let Some(task) = self.tasks.get(task_id) {
                let progress_percentage = if task.estimated_size > 0 {
                    (execution.bytes_processed as f64 / task.estimated_size as f64) * 100.0
                } else {
                    0.0
                };

                let active_info = ActiveTaskInfo {
                    task_id: task_id.clone(),
                    layer_digest: task.metadata.get("layer_digest").cloned().unwrap_or_else(|| task_id.clone()),
                    layer_size: task.estimated_size,
                    processed_bytes: execution.bytes_processed,
                    start_time: execution.start_time,
                    stage: execution.stage,
                    progress_percentage,
                };
                
                active_task_details.insert(task_id.clone(), active_info);
            }
        }

        PipelineProgress {
            total_tasks,
            completed_tasks,
            active_tasks,
            queued_tasks,
            stage_progress,
            estimated_completion,
            overall_speed,
            active_task_details,
        }
    }

    /// Check if all dependencies for a task are satisfied
    fn are_dependencies_satisfied(&self, task_id: &str) -> bool {
        if let Some(task) = self.tasks.get(task_id) {
            task.dependencies.iter()
                .all(|dep| self.completed_tasks.contains(dep))
        } else {
            false
        }
    }

    /// Update available tasks after completing a task
    fn update_available_tasks(&mut self, completed_task_id: &str) {
        if let Some(dependent_tasks) = self.dependency_graph.get(completed_task_id) {
            for task_id in dependent_tasks {
                if self.are_dependencies_satisfied(task_id) {
                    self.task_queue.push_back(task_id.clone());
                }
            }
        }
    }

    /// Calculate progress for a specific stage
    fn calculate_stage_progress(&self, stage: PipelineStage) -> StageProgress {
        let stage_tasks: Vec<_> = self.tasks.values()
            .filter(|task| task.stage == stage)
            .collect();

        let total_tasks = stage_tasks.len();
        let completed_tasks = stage_tasks.iter()
            .filter(|task| self.completed_tasks.contains(&task.id))
            .count();
        let active_tasks = stage_tasks.iter()
            .filter(|task| self.active_tasks.contains_key(&task.id))
            .count();

        // Calculate average duration from completed tasks
        let avg_duration = if completed_tasks > 0 {
            stage_tasks.iter()
                .filter(|task| self.completed_tasks.contains(&task.id))
                .map(|task| task.estimated_duration)
                .sum::<Duration>() / completed_tasks as u32
        } else {
            Duration::from_secs(0)
        };

        let success_rate = if total_tasks > 0 {
            completed_tasks as f64 / total_tasks as f64
        } else {
            0.0
        };

        StageProgress {
            total_tasks,
            completed_tasks,
            active_tasks,
            average_duration: avg_duration,
            success_rate,
        }
    }

    /// Estimate overall completion time
    fn estimate_completion_time(&self) -> Option<Instant> {
        if self.active_tasks.is_empty() {
            return None;
        }

        // Use the latest estimated completion from active tasks
        self.active_tasks.values()
            .filter_map(|exec| exec.estimated_completion)
            .max()
    }

    /// Calculate overall processing speed
    fn calculate_overall_speed(&self) -> f64 {
        // If pipeline hasn't started yet, return 0
        let pipeline_start_time = match self.pipeline_start_time {
            Some(start_time) => start_time,
            None => return 0.0,
        };
        
        // Calculate total bytes processed by all tasks (active + completed)
        let active_bytes: u64 = self.active_tasks.values()
            .map(|exec| exec.bytes_processed)
            .sum();
            
        let completed_bytes: u64 = self.completed_tasks.iter()
            .filter_map(|task_id| self.tasks.get(task_id))
            .map(|task| task.estimated_size)
            .sum();
            
        let total_bytes = active_bytes + completed_bytes;
        
        // Calculate pipeline elapsed time since first task started
        let pipeline_elapsed = pipeline_start_time.elapsed();
        
        if pipeline_elapsed.as_secs_f64() > 0.0 && total_bytes > 0 {
            total_bytes as f64 / pipeline_elapsed.as_secs_f64()
        } else {
            0.0
        }
    }
}

impl Default for PipelineManager {
    fn default() -> Self {
        Self::new()
    }
}
