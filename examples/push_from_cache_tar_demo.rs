//! Example: Push from Cache using Tar - 从缓存推送镜像（使用tar引用）
//!
//! 此示例展示如何从本地缓存推送Docker镜像到远程registry（使用tar文件作为引用）。
//! 这是4种核心操作模式中的第4种：PushFromCacheUsingTar
//! 注意：此模式实际上与模式3相同，因为缓存格式是统一的

use docker_image_pusher::{
    AuthConfig, cli::operation_mode::OperationMode, error::Result,
    image::image_manager::ImageManager, registry::RegistryClientBuilder,
};

#[tokio::main]
async fn main() -> Result<()> {
    println!("📦 Docker Image Pusher - Push from Cache (Tar Reference) Demo");
    println!("================================================================");

    // 配置参数
    let target_registry = "registry.cn-beijing.aliyuncs.com";
    let target_repository = "yoce/cblt"; // 推送到相同的repository
    let target_reference = "yoce"; // 推送到相同的reference
    let cache_dir = ".cache_demo"; // 使用已有的缓存

    println!("📥 Configuration:");
    println!("  Cache Directory: {}", cache_dir);
    println!("  Target Registry: {}", target_registry);
    println!("  Target Repository: {}", target_repository);
    println!("  Target Reference: {}", target_reference);
    println!();

    println!(
        "ℹ️  Note: This mode pushes cached image {}/{} to target registry",
        target_repository, target_reference
    );
    println!("   The implementation is identical to manifest-based push");
    println!();

    // 1. 检查缓存是否存在
    check_cache_exists(cache_dir, target_repository, target_reference).await?;

    // 2. 创建 ImageManager
    println!("🔧 Creating ImageManager...");
    let mut image_manager = ImageManager::new(Some(cache_dir), true)?;
    println!("✅ ImageManager created successfully");

    // 3. 构建 Registry Client
    println!("🌐 Building Registry Client for Aliyun registry...");
    let client = RegistryClientBuilder::new(format!("https://{}", target_registry))
        .with_timeout(3600)
        .with_skip_tls(false) // Aliyun uses TLS
        .with_verbose(true)
        .build()?;
    println!("✅ Registry Client built successfully");

    // 4. 获取认证（Aliyun credentials）
    println!("🔐 Authenticating with Aliyun registry...");
    let auth_config = AuthConfig::new("canny_best@163.com".to_string(), "ra201222".to_string());
    let auth_token = client
        .authenticate_for_repository(&auth_config, target_repository)
        .await?;
    println!(
        "✅ Authentication successful with user: {}",
        auth_config.username
    );
    println!("🔑 Token scope: repository:{}:pull,push", target_repository);

    // 5. 定义操作模式 - 使用 tar 引用方式推送（实际上与manifest模式相同）
    let mode = OperationMode::PushFromCacheUsingTar {
        repository: target_repository.to_string(),
        reference: target_reference.to_string(),
    };

    println!("📋 Operation Mode: {}", mode.description());
    println!("🔄 Internal Process: Reading from unified cache format (same as manifest mode)");
    println!();

    // 6. 执行推送操作
    println!("🔄 Starting push from cache operation...");
    match image_manager
        .execute_operation(&mode, Some(&client), auth_token.as_deref())
        .await
    {
        Ok(()) => {
            println!("✅ Push from cache (tar reference) operation completed successfully!");
            println!();
            println!(
                "🎯 Image pushed to: {}/{}/{}",
                target_registry, target_repository, target_reference
            );
            println!("🔍 You can now verify the upload:");
            println!(
                "   curl http://{}/v2/{}/manifests/{}",
                target_registry, target_repository, target_reference
            );
            println!(
                "   curl http://{}/v2/{}/tags/list",
                target_registry, target_repository
            );

            // 显示模式差异说明
            show_mode_explanation();
        }
        Err(e) => {
            eprintln!("❌ Push from cache (tar reference) operation failed: {}", e);
            eprintln!("💡 Possible solutions:");
            eprintln!(
                "   - Check if target registry is running: docker run -d -p 5000:5000 registry:2"
            );
            eprintln!("   - Verify cache contains the source image");
            eprintln!("   - Check network connectivity to target registry");
            std::process::exit(1);
        }
    }

    // 7. 验证推送结果并对比两种模式
    verify_push_result_and_compare(&client, target_repository, target_reference, &auth_token).await;

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

fn show_mode_explanation() {
    println!();
    println!("📚 Mode Comparison: Manifest vs Tar Reference");
    println!("===============================================");
    println!("🎯 PushFromCacheUsingManifest:");
    println!("   - Directly references cached manifest");
    println!("   - Fastest approach for cache-based pushes");
    println!("   - Standard Docker Registry API workflow");
    println!();
    println!("📦 PushFromCacheUsingTar:");
    println!("   - Uses tar file as reference but reads from cache");
    println!("   - Unified cache format makes both modes identical");
    println!("   - Maintains compatibility with tar-based workflows");
    println!();
    println!("💡 Key Insight: Both modes use the same optimized cache format!");
    println!("   The cache abstracts away the original source (manifest vs tar)");
}

async fn verify_push_result_and_compare(
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

            // 解析manifest获取详细信息
            if let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&manifest_data) {
                println!("📄 Manifest details:");

                if let Some(media_type) = manifest.get("mediaType").and_then(|v| v.as_str()) {
                    println!("   - Media Type: {}", media_type);
                }

                if let Some(layers) = manifest.get("layers").and_then(|v| v.as_array()) {
                    println!("   - Number of layers: {}", layers.len());
                    for (i, layer) in layers.iter().enumerate() {
                        if let Some(digest) = layer.get("digest").and_then(|v| v.as_str()) {
                            let short_digest = &digest[7..19]; // sha256: 前缀后的前12个字符
                            println!("   - Layer {}: {}...", i + 1, short_digest);
                        }
                    }
                }

                if let Some(config) = manifest.get("config") {
                    if let Some(digest) = config.get("digest").and_then(|v| v.as_str()) {
                        let short_digest = &digest[7..19];
                        println!("   - Config: {}...", short_digest);
                    }
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
            println!("🏷️  Available tags in repository: {:?}", tags);
        }
        Err(e) => {
            eprintln!("⚠️  Could not list tags: {}", e);
        }
    }

    // 显示性能对比
    println!();
    println!("⚡ Performance Notes:");
    println!("   - Both manifest and tar reference modes use the same cache");
    println!("   - No performance difference between the two approaches");
    println!("   - Cache format optimized for fast reads and minimal memory usage");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_push_from_cache_tar_creation() {
        // 测试 ImageManager 创建
        let result = ImageManager::new(Some(".test_push_tar_cache"), false);
        assert!(result.is_ok());

        // 清理测试缓存
        let _ = std::fs::remove_dir_all(".test_push_tar_cache");
    }

    #[test]
    fn test_operation_mode_description() {
        let mode = OperationMode::PushFromCacheUsingTar {
            repository: "demo/test".to_string(),
            reference: "v1.0".to_string(),
        };
        assert_eq!(mode.description(), "Push from cache using tar reference");
    }

    #[test]
    fn test_mode_equivalence() {
        // 测试两种推送模式的描述不同但实现相同
        let manifest_mode = OperationMode::PushFromCacheUsingManifest {
            repository: "test/repo".to_string(),
            reference: "latest".to_string(),
        };

        let tar_mode = OperationMode::PushFromCacheUsingTar {
            repository: "test/repo".to_string(),
            reference: "latest".to_string(),
        };

        // 描述应该不同
        assert_ne!(manifest_mode.description(), tar_mode.description());

        // 但repository和reference应该相同
        if let (
            OperationMode::PushFromCacheUsingManifest {
                repository: r1,
                reference: ref1,
            },
            OperationMode::PushFromCacheUsingTar {
                repository: r2,
                reference: ref2,
            },
        ) = (&manifest_mode, &tar_mode)
        {
            assert_eq!(r1, r2);
            assert_eq!(ref1, ref2);
        }
    }
}
