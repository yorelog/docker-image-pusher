use docker_image_pusher::concurrency::pipeline::{PipelineProgress, ActiveTaskInfo, TaskType};
use docker_image_pusher::logging::display_unified_pipeline_progress;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_progress_display_with_different_task_counts() {
    println!("🧪 Testing progress display with different numbers of active tasks...\n");

    // Test case 1: No active tasks
    test_case_no_active_tasks().await;
    
    // Test case 2: One active task
    test_case_one_active_task().await;
    
    // Test case 3: Three active tasks
    test_case_three_active_tasks().await;
    
    // Test case 4: Many active tasks (12)
    test_case_many_active_tasks().await;
    
    println!("✅ All test cases completed successfully!");
}

async fn test_case_no_active_tasks() {
    println!("📝 Test Case 1: No active tasks");
    
    let progress = PipelineProgress {
        active_task_details: HashMap::new(),
        completed_tasks: 5,
        failed_tasks: 0,
        queued_tasks: 0,
        total_tasks: 5,
        total_bytes_processed: 1024 * 1024 * 50, // 50MB
        current_speed_mbps: 0.0,
        start_time: Instant::now() - Duration::from_secs(30),
    };
    
    println!("Expected: No progress bars should be displayed");
    display_unified_pipeline_progress(&progress);
    println!("✅ Case 1 completed\n");
}

async fn test_case_one_active_task() {
    println!("📝 Test Case 2: One active task");
    
    let mut active_tasks = HashMap::new();
    active_tasks.insert("task_1".to_string(), ActiveTaskInfo {
        task_id: "task_1".to_string(),
        task_type: TaskType::LayerDownload,
        progress_percentage: 45.0,
        bytes_processed: 1024 * 1024 * 45, // 45MB
        total_bytes: 1024 * 1024 * 100,    // 100MB
        current_speed_mbps: 5.2,
        estimated_completion: Duration::from_secs(60),
        start_time: Instant::now() - Duration::from_secs(15),
    });
    
    let progress = PipelineProgress {
        active_task_details: active_tasks,
        completed_tasks: 3,
        failed_tasks: 0,
        queued_tasks: 2,
        total_tasks: 6,
        total_bytes_processed: 1024 * 1024 * 150, // 150MB
        current_speed_mbps: 5.2,
        start_time: Instant::now() - Duration::from_secs(45),
    };
    
    println!("Expected: Exactly 1 progress bar should be displayed");
    display_unified_pipeline_progress(&progress);
    println!("✅ Case 2 completed\n");
}

async fn test_case_three_active_tasks() {
    println!("📝 Test Case 3: Three active tasks");
    
    let mut active_tasks = HashMap::new();
    
    active_tasks.insert("download_1".to_string(), ActiveTaskInfo {
        task_id: "download_1".to_string(),
        task_type: TaskType::LayerDownload,
        progress_percentage: 67.0,
        bytes_processed: 1024 * 1024 * 67, // 67MB
        total_bytes: 1024 * 1024 * 100,    // 100MB
        current_speed_mbps: 8.1,
        estimated_completion: Duration::from_secs(25),
        start_time: Instant::now() - Duration::from_secs(30),
    });
    
    active_tasks.insert("upload_1".to_string(), ActiveTaskInfo {
        task_id: "upload_1".to_string(),
        task_type: TaskType::LayerUpload,
        progress_percentage: 23.0,
        bytes_processed: 1024 * 1024 * 23, // 23MB
        total_bytes: 1024 * 1024 * 100,    // 100MB
        current_speed_mbps: 3.5,
        estimated_completion: Duration::from_secs(120),
        start_time: Instant::now() - Duration::from_secs(12),
    });
    
    active_tasks.insert("download_2".to_string(), ActiveTaskInfo {
        task_id: "download_2".to_string(),
        task_type: TaskType::LayerDownload,
        progress_percentage: 89.0,
        bytes_processed: 1024 * 1024 * 89, // 89MB
        total_bytes: 1024 * 1024 * 100,    // 100MB
        current_speed_mbps: 12.3,
        estimated_completion: Duration::from_secs(8),
        start_time: Instant::now() - Duration::from_secs(45),
    });
    
    let progress = PipelineProgress {
        active_task_details: active_tasks,
        completed_tasks: 8,
        failed_tasks: 1,
        queued_tasks: 4,
        total_tasks: 16,
        total_bytes_processed: 1024 * 1024 * 800, // 800MB
        current_speed_mbps: 7.9,
        start_time: Instant::now() - Duration::from_secs(180),
    };
    
    println!("Expected: Exactly 3 progress bars should be displayed");
    display_unified_pipeline_progress(&progress);
    println!("✅ Case 3 completed\n");
}

async fn test_case_many_active_tasks() {
    println!("📝 Test Case 4: Many active tasks (12)");
    
    let mut active_tasks = HashMap::new();
    
    // Create 12 active tasks with varying progress
    for i in 1..=12 {
        let progress_pct = (i * 7 + 13) % 100; // Varying progress percentages
        let speed = 2.0 + (i as f64 * 0.8); // Varying speeds
        
        active_tasks.insert(format!("task_{}", i), ActiveTaskInfo {
            task_id: format!("task_{}", i),
            task_type: if i % 2 == 0 { TaskType::LayerUpload } else { TaskType::LayerDownload },
            progress_percentage: progress_pct as f64,
            bytes_processed: 1024 * 1024 * progress_pct as u64, 
            total_bytes: 1024 * 1024 * 100,    // 100MB each
            current_speed_mbps: speed,
            estimated_completion: Duration::from_secs(60 - (progress_pct as u64 / 2)),
            start_time: Instant::now() - Duration::from_secs(i as u64 * 5),
        });
    }
    
    let progress = PipelineProgress {
        active_task_details: active_tasks,
        completed_tasks: 25,
        failed_tasks: 2,
        queued_tasks: 8,
        total_tasks: 47,
        total_bytes_processed: 1024 * 1024 * 2500, // 2.5GB
        current_speed_mbps: 45.6,
        start_time: Instant::now() - Duration::from_secs(600),
    };
    
    println!("Expected: Exactly 12 progress bars should be displayed (no artificial limit)");
    display_unified_pipeline_progress(&progress);
    println!("✅ Case 4 completed\n");
}
