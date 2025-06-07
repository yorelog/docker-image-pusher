//! Comprehensive Demo - 完整的4种操作模式演示
//!
//! 此示例展示Docker Image Pusher的完整工作流程，包括4种核心操作模式：
//! 1. PullAndCache - 从registry拉取并缓存
//! 2. ExtractAndCache - 从tar文件提取并缓存
//! 3. PushFromCacheUsingManifest - 从缓存推送（manifest方式）
//! 4. PushFromCacheUsingTar - 从缓存推送（tar引用方式）

use docker_image_pusher::{
    cli::operation_mode::OperationMode, error::Result, image::image_manager::ImageManager,
    registry::RegistryClientBuilder,
};
use std::env;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Docker Image Pusher - Comprehensive 4-Mode Demo");
    println!("===================================================");
    println!("This demo showcases all 4 core operation modes:");
    println!("1. Pull and Cache from registry");
    println!("2. Extract and Cache from tar file");
    println!("3. Push from Cache using Manifest");
    println!("4. Push from Cache using Tar reference");
    println!();

    let start_time = Instant::now();

    // 配置
    let cache_dir = ".cache_comprehensive_demo";
    let target_registry =
        env::var("TARGET_REGISTRY").unwrap_or_else(|_| "localhost:5000".to_string());

    // 清理之前的演示缓存
    let _ = std::fs::remove_dir_all(cache_dir);

    // 运行完整演示
    match run_comprehensive_demo(cache_dir, &target_registry).await {
        Ok(()) => {
            let duration = start_time.elapsed();
            println!("🎉 Comprehensive demo completed successfully!");
            println!("⏱️  Total time: {:.2}s", duration.as_secs_f64());

            // 显示最终统计
            show_final_statistics(cache_dir).await;
        }
        Err(e) => {
            eprintln!("❌ Demo failed: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn run_comprehensive_demo(cache_dir: &str, target_registry: &str) -> Result<()> {
    // =========================================================================
    // 模式1: PullAndCache - 从registry拉取并缓存
    // =========================================================================
    println!("📥 MODE 1: Pull and Cache from Registry");
    println!("========================================");

    let pull_start = Instant::now();

    // 创建ImageManager
    let mut image_manager = ImageManager::new(Some(cache_dir), true)?;

    // 构建registry client
    let source_client = RegistryClientBuilder::new("https://registry-1.docker.io".to_string())
        .with_timeout(3600)
        .with_verbose(true)
        .build()?;

    // 定义拉取操作
    let pull_mode = OperationMode::PullAndCache {
        repository: "library/hello-world".to_string(),
        reference: "latest".to_string(),
    };

    println!("🔄 Executing: {}", pull_mode.description());
    image_manager
        .execute_operation(&pull_mode, Some(&source_client), None)
        .await?;

    let pull_duration = pull_start.elapsed();
    println!("✅ Mode 1 completed in {:.2}s", pull_duration.as_secs_f64());
    println!();

    // =========================================================================
    // 模式2: ExtractAndCache - 从tar文件提取并缓存
    // =========================================================================
    println!("📦 MODE 2: Extract and Cache from Tar File");
    println!("===========================================");

    let extract_start = Instant::now();

    // 创建示例tar文件
    let tar_file = "nginx-demo.tar";
    create_demo_tar_file(tar_file, "nginx:alpine").await?;

    // 定义提取操作
    let extract_mode = OperationMode::ExtractAndCache {
        tar_file: tar_file.to_string(),
        repository: "local/nginx".to_string(),
        reference: "alpine".to_string(),
    };

    println!("🔄 Executing: {}", extract_mode.description());
    image_manager
        .execute_operation(&extract_mode, None, None)
        .await?;

    let extract_duration = extract_start.elapsed();
    println!(
        "✅ Mode 2 completed in {:.2}s",
        extract_duration.as_secs_f64()
    );
    println!();

    // =========================================================================
    // 准备目标registry客户端
    // =========================================================================
    println!("🌐 Setting up target registry...");
    ensure_target_registry_running(target_registry).await;

    let target_client = RegistryClientBuilder::new(format!("http://{}", target_registry))
        .with_timeout(3600)
        .with_skip_tls(true)
        .with_verbose(true)
        .build()?;

    // =========================================================================
    // 模式3: PushFromCacheUsingManifest - 从缓存推送（manifest方式）
    // =========================================================================
    println!("🚀 MODE 3: Push from Cache using Manifest");
    println!("==========================================");

    let push_manifest_start = Instant::now();

    // 定义推送操作（manifest方式）
    let push_manifest_mode = OperationMode::PushFromCacheUsingManifest {
        repository: "demo/hello-world-manifest".to_string(),
        reference: "v1.0".to_string(),
    };

    println!("🔄 Executing: {}", push_manifest_mode.description());
    image_manager
        .execute_operation(&push_manifest_mode, Some(&target_client), None)
        .await?;

    let push_manifest_duration = push_manifest_start.elapsed();
    println!(
        "✅ Mode 3 completed in {:.2}s",
        push_manifest_duration.as_secs_f64()
    );
    println!();

    // =========================================================================
    // 模式4: PushFromCacheUsingTar - 从缓存推送（tar引用方式）
    // =========================================================================
    println!("📦 MODE 4: Push from Cache using Tar Reference");
    println!("===============================================");

    let push_tar_start = Instant::now();

    // 定义推送操作（tar引用方式）
    let push_tar_mode = OperationMode::PushFromCacheUsingTar {
        repository: "demo/nginx-tar-ref".to_string(),
        reference: "alpine".to_string(),
    };

    println!("🔄 Executing: {}", push_tar_mode.description());
    image_manager
        .execute_operation(&push_tar_mode, Some(&target_client), None)
        .await?;

    let push_tar_duration = push_tar_start.elapsed();
    println!(
        "✅ Mode 4 completed in {:.2}s",
        push_tar_duration.as_secs_f64()
    );
    println!();

    // 验证所有推送结果
    verify_all_pushes(&target_client, target_registry).await;

    Ok(())
}

async fn create_demo_tar_file(tar_file: &str, image: &str) -> Result<()> {
    if std::path::Path::new(tar_file).exists() {
        println!("✅ Using existing tar file: {}", tar_file);
        return Ok(());
    }

    println!("🛠️  Creating demo tar file: {}", tar_file);

    // 尝试拉取镜像并保存为tar
    let pull_output = tokio::process::Command::new("docker")
        .args(&["pull", image])
        .output()
        .await;

    if let Ok(result) = pull_output {
        if result.status.success() {
            let save_output = tokio::process::Command::new("docker")
                .args(&["save", image, "-o", tar_file])
                .output()
                .await;

            if let Ok(save_result) = save_output {
                if save_result.status.success() {
                    println!("✅ Tar file created: {}", tar_file);
                    return Ok(());
                }
            }
        }
    }

    eprintln!("⚠️  Could not create tar file, using alternative approach");
    Ok(())
}

async fn ensure_target_registry_running(registry: &str) {
    println!("🔍 Checking if target registry is running...");

    // 尝试连接到registry
    let client = reqwest::Client::new();
    let url = format!("http://{}/v2/", registry);

    match client.get(&url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                println!("✅ Target registry is running: {}", registry);
            } else {
                println!("⚠️  Registry responded with status: {}", response.status());
            }
        }
        Err(_) => {
            println!("❌ Target registry not accessible: {}", registry);
            println!("💡 To start a local registry:");
            println!("   docker run -d -p 5000:5000 --name registry registry:2");
            println!("   Continuing with demo anyway...");
        }
    }
}

async fn verify_all_pushes(client: &docker_image_pusher::registry::RegistryClient, registry: &str) {
    println!("🔍 Verifying all push operations...");
    println!("=====================================");

    let test_repositories = vec![
        ("demo/hello-world-manifest", "v1.0"),
        ("demo/nginx-tar-ref", "alpine"),
    ];

    for (repo, tag) in test_repositories {
        println!("📋 Checking {}/{}...", repo, tag);

        match client.pull_manifest(repo, tag, &None).await {
            Ok(manifest_data) => {
                println!("   ✅ Manifest found ({} bytes)", manifest_data.len());

                // 解析manifest获取层信息
                if let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&manifest_data) {
                    if let Some(layers) = manifest.get("layers").and_then(|v| v.as_array()) {
                        println!("   📦 Layers: {}", layers.len());
                    }
                }
            }
            Err(e) => {
                println!("   ❌ Manifest not found: {}", e);
            }
        }

        // 检查标签列表
        match client.list_tags(repo, &None).await {
            Ok(tags) => {
                println!("   🏷️  Tags: {:?}", tags);
            }
            Err(_) => {
                println!("   ⚠️  Could not list tags");
            }
        }

        println!();
    }

    println!("🌐 Registry endpoints to test manually:");
    println!("   curl http://{}/v2/_catalog", registry);
    println!(
        "   curl http://{}/v2/demo/hello-world-manifest/tags/list",
        registry
    );
    println!(
        "   curl http://{}/v2/demo/nginx-tar-ref/tags/list",
        registry
    );
}

async fn show_final_statistics(cache_dir: &str) {
    println!();
    println!("📊 Final Demo Statistics");
    println!("========================");

    // 缓存统计
    if let Ok(entries) = std::fs::read_dir(format!("{}/blobs/sha256", cache_dir)) {
        let blob_count = entries.count();
        println!("📦 Total cached blobs: {}", blob_count);
    }

    // 缓存的镜像数量
    if let Ok(index_content) = std::fs::read_to_string(format!("{}/index.json", cache_dir)) {
        if let Ok(index) = serde_json::from_str::<serde_json::Value>(&index_content) {
            if let Some(obj) = index.as_object() {
                println!("🖼️  Cached images: {}", obj.len());
                for key in obj.keys() {
                    println!("   - {}", key);
                }
            }
        }
    }

    // 计算缓存目录大小
    if let Ok(metadata) = std::fs::metadata(cache_dir) {
        if metadata.is_dir() {
            if let Ok(_entries) = std::fs::read_dir(cache_dir) {
                let mut total_size = 0u64;
                count_dir_size(&format!("{}/blobs", cache_dir), &mut total_size);
                println!(
                    "💾 Cache size: {} bytes ({:.2} MB)",
                    total_size,
                    total_size as f64 / 1024.0 / 1024.0
                );
            }
        }
    }

    println!();
    println!("🎯 Demo Results Summary:");
    println!("   ✅ Mode 1 (Pull and Cache): SUCCESS");
    println!("   ✅ Mode 2 (Extract and Cache): SUCCESS");
    println!("   ✅ Mode 3 (Push from Cache - Manifest): SUCCESS");
    println!("   ✅ Mode 4 (Push from Cache - Tar Ref): SUCCESS");
    println!();
    println!(
        "🧹 Cleanup: Run 'rm -rf {}' to remove demo cache",
        cache_dir
    );
}

fn count_dir_size(dir: &str, total: &mut u64) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    *total += metadata.len();
                } else if metadata.is_dir() {
                    count_dir_size(&entry.path().to_string_lossy(), total);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_comprehensive_demo_setup() {
        // 测试基本组件创建
        let result = ImageManager::new(Some(".test_comprehensive"), false);
        assert!(result.is_ok());

        let client_result =
            RegistryClientBuilder::new("https://registry-1.docker.io".to_string()).build();
        assert!(client_result.is_ok());

        // 清理
        let _ = std::fs::remove_dir_all(".test_comprehensive");
    }

    #[test]
    fn test_all_operation_modes() {
        let modes = vec![
            OperationMode::PullAndCache {
                repository: "test/repo".to_string(),
                reference: "latest".to_string(),
            },
            OperationMode::ExtractAndCache {
                tar_file: "test.tar".to_string(),
                repository: "local/test".to_string(),
                reference: "latest".to_string(),
            },
            OperationMode::PushFromCacheUsingManifest {
                repository: "demo/test".to_string(),
                reference: "v1.0".to_string(),
            },
            OperationMode::PushFromCacheUsingTar {
                repository: "demo/test-tar".to_string(),
                reference: "v1.0".to_string(),
            },
        ];

        // 验证所有模式都有描述
        for mode in modes {
            assert!(!mode.description().is_empty());
        }
    }
}
