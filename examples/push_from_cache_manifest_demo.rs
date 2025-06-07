//! Example: Push from Cache using Manifest - 从缓存推送镜像（使用manifest）
//!
//! 此示例展示如何从本地缓存推送Docker镜像到远程registry。
//! 这是4种核心操作模式中的第3种：PushFromCacheUsingManifest

use docker_image_pusher::{
    AuthConfig, cli::operation_mode::OperationMode, error::Result,
    image::image_manager::ImageManager, registry::RegistryClientBuilder,
};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Docker Image Pusher - Push from Cache (Manifest) Demo");
    println!("============================================================");

    // 配置参数 - 使用Aliyun registry，推送到已存在的repository
    let source_repository = "yoce/cblt"; // 从缓存中读取
    let source_reference = "yoce";
    let target_registry = "registry.cn-beijing.aliyuncs.com";
    let target_repository = "yoce/cblt"; // 推送回同一个repository
    let target_reference = "test-push"; // 使用新的tag
    let cache_dir = ".cache_demo";

    println!("📥 Configuration:");
    println!(
        "  Source (Cache): {}/{}",
        source_repository, source_reference
    );
    println!("  Target Registry: {}", target_registry);
    println!("  Target Repository: {}", target_repository);
    println!("  Target Reference: {}", target_reference);
    println!("  Cache Directory: {}", cache_dir);
    println!();

    // 1. 检查缓存是否存在
    check_cache_exists(cache_dir, source_repository, source_reference).await?;

    // 2. 创建 ImageManager
    println!("🔧 Creating ImageManager...");
    let mut image_manager = ImageManager::new(Some(cache_dir), true)?;
    println!("✅ ImageManager created successfully");

    // 3. 构建 Registry Client - 配置为Aliyun registry
    println!("🌐 Building Registry Client for Aliyun registry...");
    let client = RegistryClientBuilder::new(format!("https://{}", target_registry))
        .with_timeout(3600)
        .with_skip_tls(false) // Aliyun registry使用TLS
        .with_verbose(true)
        .build()?;
    println!("✅ Registry Client built successfully");

    // 4. 获取认证 - 使用Aliyun registry凭据
    println!("🔐 Authenticating with Aliyun registry...");
    let username = env::var("ALIYUN_USERNAME").unwrap_or_else(|_| "canny_best@163.com".to_string());
    let password = env::var("ALIYUN_PASSWORD").unwrap_or_else(|_| "ra201222".to_string());

    let auth_config = AuthConfig::new(username.clone(), password.clone());
    let auth_token = client
        .authenticate_for_repository(&auth_config, target_repository)
        .await?;
    println!("✅ Authentication successful with user: {}", username);
    println!("🔑 Token scope: repository:{}:pull,push", target_repository);

    // 5. 定义操作模式 - 使用 manifest 方式推送
    let mode = OperationMode::PushFromCacheUsingManifest {
        repository: target_repository.to_string(),
        reference: target_reference.to_string(),
    };

    println!("📋 Operation Mode: {}", mode.description());
    println!();

    // 6. 执行推送操作
    println!("🔄 Starting push from cache operation...");
    match image_manager
        .execute_operation(&mode, Some(&client), auth_token.as_deref())
        .await
    {
        Ok(()) => {
            println!("✅ Push from cache operation completed successfully!");
            println!();
            println!(
                "🎯 Image pushed to: {}/{}/{}",
                target_registry, target_repository, target_reference
            );
            println!("🔍 You can now verify the upload:");
            println!(
                "   curl -H \"Authorization: Bearer <token>\" https://{}/v2/{}/manifests/{}",
                target_registry, target_repository, target_reference
            );
            println!(
                "   curl -H \"Authorization: Bearer <token>\" https://{}/v2/{}/tags/list",
                target_registry, target_repository
            );
        }
        Err(e) => {
            eprintln!("❌ Push from cache operation failed: {}", e);
            eprintln!("💡 Possible solutions:");
            eprintln!(
                "   - Check if source image exists in cache: {}/{}",
                source_repository, source_reference
            );
            eprintln!("   - Verify Aliyun registry credentials");
            eprintln!("   - Check network connectivity to Aliyun registry");
            std::process::exit(1);
        }
    }

    // 7. 验证推送结果
    verify_push_result(&client, target_repository, target_reference, &auth_token).await;

    Ok(())
}

async fn check_cache_exists(cache_dir: &str, repository: &str, reference: &str) -> Result<()> {
    println!("🔍 Checking cache for source image...");

    // 检查缓存目录
    if !std::path::Path::new(cache_dir).exists() {
        eprintln!("❌ Cache directory not found: {}", cache_dir);
        eprintln!("💡 Please run extract_and_cache_demo.rs or pull_and_cache_demo.rs first");
        std::process::exit(1);
    }

    // 检查索引文件
    let index_path = format!("{}/index.json", cache_dir);
    if !std::path::Path::new(&index_path).exists() {
        eprintln!("❌ Cache index not found: {}", index_path);
        eprintln!("💡 Cache appears to be empty or corrupted");
        std::process::exit(1);
    }

    // 检查特定镜像的manifest
    let manifest_path = format!("{}/manifests/{}/{}", cache_dir, repository, reference);
    if !std::path::Path::new(&manifest_path).exists() {
        eprintln!("❌ Manifest not found in cache: {}", manifest_path);
        eprintln!("💡 Available images in cache:");

        // 列出缓存中的镜像
        if let Ok(index_content) = std::fs::read_to_string(&index_path) {
            if let Ok(index) = serde_json::from_str::<serde_json::Value>(&index_content) {
                if let Some(obj) = index.as_object() {
                    for key in obj.keys() {
                        eprintln!("   - {}", key);
                    }
                }
            }
        }
        std::process::exit(1);
    }

    println!(
        "✅ Source image found in cache: {}/{}",
        repository, reference
    );
    Ok(())
}

async fn verify_push_result(
    client: &docker_image_pusher::registry::RegistryClient,
    repository: &str,
    reference: &str,
    auth_token: &Option<String>,
) {
    println!();
    println!("🔍 Verifying push result...");

    // 检查manifest是否存在
    match client
        .pull_manifest(repository, reference, auth_token)
        .await
    {
        Ok(manifest_data) => {
            println!("✅ Manifest successfully retrieved from target registry");
            println!("📊 Manifest size: {} bytes", manifest_data.len());

            // 解析manifest获取layer信息
            if let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&manifest_data) {
                if let Some(layers) = manifest.get("layers").and_then(|v| v.as_array()) {
                    println!("📦 Number of layers: {}", layers.len());
                }
            }
        }
        Err(e) => {
            eprintln!("⚠️  Could not verify manifest: {}", e);
        }
    }

    // 尝试检查标签列表
    match client.list_tags(repository, auth_token).await {
        Ok(tags) => {
            println!("🏷️  Available tags: {:?}", tags);
        }
        Err(e) => {
            eprintln!("⚠️  Could not list tags: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_push_from_cache_creation() {
        // 测试 ImageManager 创建
        let result = ImageManager::new(Some(".test_push_cache"), false);
        assert!(result.is_ok());

        // 清理测试缓存
        let _ = std::fs::remove_dir_all(".test_push_cache");
    }

    #[test]
    fn test_operation_mode_description() {
        let mode = OperationMode::PushFromCacheUsingManifest {
            repository: "demo/test".to_string(),
            reference: "v1.0".to_string(),
        };
        assert_eq!(mode.description(), "Push from cache using manifest");
    }

    #[test]
    fn test_cache_path_construction() {
        let cache_dir = ".test_cache";
        let repository = "local/hello-world";
        let reference = "latest";
        let manifest_path = format!("{}/manifests/{}/{}", cache_dir, repository, reference);
        assert_eq!(
            manifest_path,
            ".test_cache/manifests/local/hello-world/latest"
        );
    }
}
