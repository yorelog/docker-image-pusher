//! Example: Pull and Cache - 从 registry 拉取镜像并缓存
//!
//! 此示例展示如何从远程registry拉取Docker镜像并将其缓存到本地。
//! 这是4种核心操作模式中的第1种：PullAndCache

use docker_image_pusher::{
    AuthConfig, cli::operation_mode::OperationMode, error::Result,
    image::image_manager::ImageManager, registry::RegistryClientBuilder,
};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Docker Image Pusher - Pull and Cache Demo");
    println!("==================================================");

    // 配置参数 - 支持环境变量
    let registry = env::var("DOCKER_REGISTRY")
        .unwrap_or_else(|_| "https://registry.cn-beijing.aliyuncs.com".to_string());
    let repository = env::var("DOCKER_REPOSITORY").unwrap_or_else(|_| "yoce/cblt".to_string());
    let reference = env::var("DOCKER_REFERENCE").unwrap_or_else(|_| "yoce".to_string());
    let cache_dir = ".cache_demo";

    println!("📥 Configuration:");
    println!("  Registry: {}", registry);
    println!("  Repository: {}", repository);
    println!("  Reference: {}", reference);
    println!("  Cache Directory: {}", cache_dir);
    println!();

    // 1. 创建 ImageManager
    println!("🔧 Creating ImageManager...");
    let mut image_manager = ImageManager::new(Some(cache_dir), true)?;
    println!("✅ ImageManager created successfully");

    // 2. 构建 Registry Client
    println!("🌐 Building Registry Client...");
    let client = RegistryClientBuilder::new(registry.to_string())
        .with_timeout(3600)
        .with_verbose(true)
        .build()?;
    println!("✅ Registry Client built successfully");

    // 3. 获取认证（总是尝试，支持匿名token）
    println!("🔐 Attempting authentication...");
    let auth_token = if let (Ok(username), Ok(password)) =
        (env::var("DOCKER_USERNAME"), env::var("DOCKER_PASSWORD"))
    {
        println!("  Using provided credentials for user: {}", username);
        let auth_config = AuthConfig::new(username, password);
        client
            .authenticate_for_repository(&auth_config, &repository)
            .await?
    } else {
        println!("  No credentials provided, trying anonymous authentication...");
        // 使用直接认证方法尝试获取匿名token
        let auth = docker_image_pusher::registry::auth::Auth::new();
        let output = docker_image_pusher::logging::Logger::new(true);
        auth.authenticate_with_registry(&registry, &repository, None, None, &output)
            .await?
    };

    if auth_token.is_some() {
        println!("✅ Authentication successful");
    } else {
        println!("ℹ️  No authentication required");
    }

    // 4. 定义操作模式
    let mode = OperationMode::PullAndCache {
        repository: repository.to_string(),
        reference: reference.to_string(),
    };

    println!("📋 Operation Mode: {}", mode.description());
    println!();

    // 5. 执行拉取和缓存操作
    println!("🔄 Starting pull and cache operation...");
    match image_manager
        .execute_operation(&mode, Some(&client), auth_token.as_deref())
        .await
    {
        Ok(()) => {
            println!("✅ Pull and cache operation completed successfully!");
            println!();
            println!("📂 Image cached to: {}", cache_dir);
            println!("🔍 You can now inspect the cache contents:");
            println!(
                "   - Manifests: {}/manifests/{}/{}",
                cache_dir, repository, reference
            );
            println!("   - Blobs: {}/blobs/sha256/", cache_dir);
            println!("   - Index: {}/index.json", cache_dir);
        }
        Err(e) => {
            eprintln!("❌ Pull and cache operation failed: {}", e);
            std::process::exit(1);
        }
    }

    // 6. 显示缓存统计
    show_cache_stats(cache_dir).await;

    Ok(())
}

async fn show_cache_stats(cache_dir: &str) {
    println!();
    println!("📊 Cache Statistics:");

    // 检查缓存目录
    if let Ok(entries) = std::fs::read_dir(format!("{}/blobs/sha256", cache_dir)) {
        let blob_count = entries.count();
        println!("  📦 Cached blobs: {}", blob_count);
    }

    // 检查索引文件
    if let Ok(index_content) = std::fs::read_to_string(format!("{}/index.json", cache_dir)) {
        if let Ok(index) = serde_json::from_str::<serde_json::Value>(&index_content) {
            if let Some(obj) = index.as_object() {
                println!("  📋 Cached images: {}", obj.len());
            }
        }
    }

    // 显示缓存目录大小
    if let Ok(metadata) = std::fs::metadata(cache_dir) {
        if metadata.is_dir() {
            println!("  💾 Cache directory exists and is ready");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pull_and_cache_creation() {
        // 测试 ImageManager 创建
        let result = ImageManager::new(Some(".test_cache"), false);
        assert!(result.is_ok());

        // 清理测试缓存
        let _ = std::fs::remove_dir_all(".test_cache");
    }

    #[tokio::test]
    async fn test_registry_client_creation() {
        // 测试 RegistryClient 创建
        let result = RegistryClientBuilder::new("https://registry-1.docker.io".to_string())
            .with_timeout(60)
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_operation_mode_description() {
        let mode = OperationMode::PullAndCache {
            repository: "test/repo".to_string(),
            reference: "latest".to_string(),
        };
        assert_eq!(mode.description(), "Pull from registry and cache locally");
    }
}
