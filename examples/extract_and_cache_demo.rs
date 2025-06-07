//! Example: Extract and Cache - 从 tar 文件提取镜像并缓存
//!
//! 此示例展示如何从Docker tar文件提取镜像并将其缓存到本地。
//! 这是4种核心操作模式中的第2种：ExtractAndCache

use docker_image_pusher::{
    cli::operation_mode::OperationMode, error::Result, image::image_manager::ImageManager,
};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    println!("📦 Docker Image Pusher - Extract and Cache Demo");
    println!("===================================================");

    // 配置参数
    let tar_file = get_tar_file_path();
    let cache_dir = ".cache_extract_demo";

    // 从tar文件名推导repository和reference
    let file_stem = Path::new(&tar_file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("extracted-image");
    let repository = format!("local/{}", file_stem);
    let reference = "latest";

    println!("📥 Configuration:");
    println!("  Tar File: {}", tar_file);
    println!("  Repository: {}", repository);
    println!("  Reference: {}", reference);
    println!("  Cache Directory: {}", cache_dir);
    println!();

    // 1. 验证tar文件存在
    if !Path::new(&tar_file).exists() {
        eprintln!("❌ Tar file not found: {}", tar_file);
        eprintln!("💡 Please ensure the dufs.tar file exists in the examples/ directory");
        return Err(docker_image_pusher::error::RegistryError::Validation(
            format!("Tar file does not exist: {}", tar_file),
        ));
    }

    // 2. 创建 ImageManager
    println!("🔧 Creating ImageManager...");
    let mut image_manager = ImageManager::new(Some(cache_dir), true)?;
    println!("✅ ImageManager created successfully");

    // 3. 定义操作模式
    let mode = OperationMode::ExtractAndCache {
        tar_file: tar_file.clone(),
        repository: repository.clone(),
        reference: reference.to_string(),
    };

    println!("📋 Operation Mode: {}", mode.description());
    println!();

    // 4. 执行提取和缓存操作
    println!("🔄 Starting extract and cache operation...");
    match image_manager.execute_operation(&mode, None, None).await {
        Ok(()) => {
            println!("✅ Extract and cache operation completed successfully!");
            println!();
            println!("📂 Image extracted and cached to: {}", cache_dir);
            println!("🔍 You can now inspect the cache contents:");
            println!(
                "   - Manifests: {}/manifests/{}/{}",
                cache_dir, repository, reference
            );
            println!("   - Blobs: {}/blobs/sha256/", cache_dir);
            println!("   - Index: {}/index.json", cache_dir);
        }
        Err(e) => {
            eprintln!("❌ Extract and cache operation failed: {}", e);
            std::process::exit(1);
        }
    }

    // 5. 显示提取统计
    show_extraction_stats(&tar_file, cache_dir).await;

    Ok(())
}

fn get_tar_file_path() -> String {
    // 检查命令行参数
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        return args[1].clone();
    }

    // 使用examples目录下的dufs.tar文件
    "examples/dufs.tar".to_string()
}

async fn show_extraction_stats(tar_file: &str, cache_dir: &str) {
    println!();
    println!("📊 Extraction Statistics:");

    // 显示原始tar文件信息
    if let Ok(metadata) = std::fs::metadata(tar_file) {
        println!("  📁 Original tar size: {} bytes", metadata.len());
    }

    // 检查提取的blobs
    if let Ok(entries) = std::fs::read_dir(format!("{}/blobs/sha256", cache_dir)) {
        let blob_count = entries.count();
        println!("  📦 Extracted blobs: {}", blob_count);
    }

    // 检查manifest
    if std::path::Path::new(&format!("{}/index.json", cache_dir)).exists() {
        println!("  📋 Index file created successfully");
    }

    // 显示缓存目录总大小
    if let Ok(entries) = std::fs::read_dir(cache_dir) {
        let mut total_size = 0u64;
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                total_size += metadata.len();
            }
        }
        println!("  💾 Total cache size: {} bytes", total_size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_extract_and_cache_creation() {
        // 测试 ImageManager 创建
        let result = ImageManager::new(Some(".test_extract_cache"), false);
        assert!(result.is_ok());

        // 清理测试缓存
        let _ = std::fs::remove_dir_all(".test_extract_cache");
    }

    #[test]
    fn test_tar_file_path_parsing() {
        let tar_file = "test-image.tar";
        let file_stem = Path::new(tar_file)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("extracted-image");
        assert_eq!(file_stem, "test-image");
    }

    #[test]
    fn test_operation_mode_description() {
        let mode = OperationMode::ExtractAndCache {
            tar_file: "test.tar".to_string(),
            repository: "local/test".to_string(),
            reference: "latest".to_string(),
        };
        assert_eq!(
            mode.description(),
            "Extract from tar file and cache locally"
        );
    }
}
