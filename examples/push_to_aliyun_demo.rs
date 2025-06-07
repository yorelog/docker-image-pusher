//! Example: Push to Aliyun Registry - 推送镜像到阿里云容器镜像服务
//!
//! 此示例展示如何将缓存中的镜像推送到阿里云容器镜像服务

use docker_image_pusher::{
    AuthConfig, cli::operation_mode::OperationMode, error::Result,
    image::image_manager::ImageManager, registry::RegistryClientBuilder,
};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Docker Image Pusher - Push to Aliyun Registry Demo");
    println!("=======================================================");

    // 阿里云配置
    let aliyun_registry = "registry.cn-beijing.aliyuncs.com";
    let aliyun_username = "canny_best@163.com";
    let aliyun_password = "ra201222";

    // 源镜像（从缓存）
    let source_repository = "yoce/cblt";
    let source_reference = "yoce";

    // 目标镜像（推送到阿里云）- 推送回同一个仓库，不同tag
    let target_repository = "yoce/cblt";
    let target_reference = "push-test";
    let cache_dir = ".cache_demo";

    println!("📥 Configuration:");
    println!(
        "  Source (Cache): {}/{}",
        source_repository, source_reference
    );
    println!("  Target Registry: {}", aliyun_registry);
    println!("  Target Repository: {}", target_repository);
    println!("  Target Reference: {}", target_reference);
    println!("  Cache Directory: {}", cache_dir);
    println!("  Username: {}", aliyun_username);
    println!();

    // 1. 检查缓存是否存在
    check_cache_exists(cache_dir, source_repository, source_reference).await?;

    // 2. 创建 ImageManager
    println!("🔧 Creating ImageManager...");
    let mut image_manager = ImageManager::new(Some(cache_dir), true)?;
    println!("✅ ImageManager created successfully");

    // 3. 复制缓存中的镜像到目标仓库名称
    println!("📋 Copying cached image to target repository name...");
    copy_image_in_cache(
        cache_dir,
        source_repository,
        source_reference,
        target_repository,
        target_reference,
    )
    .await?;
    println!("✅ Image copied in cache");

    // 4. 构建 Registry Client for 阿里云
    println!("🌐 Building Registry Client for Aliyun...");
    let client = RegistryClientBuilder::new(format!("https://{}", aliyun_registry))
        .with_timeout(3600)
        .with_skip_tls(false)
        .with_verbose(true)
        .build()?;
    println!("✅ Registry Client built successfully");

    // 4. 认证到阿里云
    println!("🔐 Authenticating with Aliyun registry...");
    let auth_config = AuthConfig::new(aliyun_username.to_string(), aliyun_password.to_string());
    let auth_token = client
        .authenticate_for_repository(&auth_config, target_repository)
        .await?;
    println!("✅ Authentication successful");

    // 5. 定义操作模式 - 推送缓存中的镜像到阿里云
    let mode = OperationMode::PushFromCacheUsingManifest {
        repository: target_repository.to_string(),
        reference: target_reference.to_string(),
    };

    println!("📋 Operation Mode: {}", mode.description());
    println!();

    // 6. 执行推送操作
    println!("🔄 Starting push to Aliyun operation...");
    println!(
        "🎯 Target: {}/{}/{}",
        aliyun_registry, target_repository, target_reference
    );

    match image_manager
        .execute_operation(&mode, Some(&client), auth_token.as_deref())
        .await
    {
        Ok(()) => {
            println!("✅ Push to Aliyun operation completed successfully!");
            println!(
                "🎯 Image pushed to: {}/{}/{}",
                aliyun_registry, target_repository, target_reference
            );
            println!("🔍 You can verify the upload in Aliyun Console:");
            println!("   https://cr.console.aliyun.com");

            // 验证推送结果
            verify_push_result(&client, target_repository, target_reference, &auth_token).await;
        }
        Err(e) => {
            eprintln!("❌ Push to Aliyun operation failed: {}", e);
            eprintln!("💡 Possible solutions:");
            eprintln!("   - Check Aliyun credentials and permissions");
            eprintln!("   - Verify repository name format (namespace/repo)");
            eprintln!("   - Check network connectivity to Aliyun registry");
            eprintln!("   - Ensure the repository exists in Aliyun console");
            eprintln!("   - Check if the namespace 'yoce' exists");
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn check_cache_exists(cache_dir: &str, repository: &str, reference: &str) -> Result<()> {
    println!("🔍 Checking cache for source image...");

    if !std::path::Path::new(cache_dir).exists() {
        eprintln!("❌ Cache directory not found: {}", cache_dir);
        std::process::exit(1);
    }

    let index_path = format!("{}/index.json", cache_dir);
    if !std::path::Path::new(&index_path).exists() {
        eprintln!("❌ Cache index not found: {}", index_path);
        std::process::exit(1);
    }

    let manifest_path = format!("{}/manifests/{}/{}", cache_dir, repository, reference);
    if !std::path::Path::new(&manifest_path).exists() {
        eprintln!("❌ Manifest not found in cache: {}", manifest_path);
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
            println!("✅ Manifest successfully retrieved from Aliyun registry");
            println!("📊 Manifest size: {} bytes", manifest_data.len());

            // 解析manifest获取layer信息
            if let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&manifest_data) {
                if let Some(layers) = manifest.get("layers").and_then(|v| v.as_array()) {
                    println!("📦 Number of layers: {}", layers.len());
                }
                if let Some(media_type) = manifest.get("mediaType").and_then(|v| v.as_str()) {
                    println!("📋 Media type: {}", media_type);
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

async fn copy_image_in_cache(
    cache_dir: &str,
    src_repo: &str,
    src_ref: &str,
    dst_repo: &str,
    dst_ref: &str,
) -> Result<()> {
    use std::fs;
    use std::path::Path;

    // 读取index.json
    let index_path = format!("{}/index.json", cache_dir);
    let index_content = fs::read_to_string(&index_path)?;
    let mut index: serde_json::Value = serde_json::from_str(&index_content)?;

    let src_key = format!("{}/{}", src_repo, src_ref);
    let dst_key = format!("{}/{}", dst_repo, dst_ref);

    // 复制源镜像条目到目标
    if let Some(src_entry) = index.get(&src_key).cloned() {
        let mut dst_entry = src_entry;

        // 更新路径
        if let Some(obj) = dst_entry.as_object_mut() {
            obj.insert(
                "repository".to_string(),
                serde_json::Value::String(dst_repo.to_string()),
            );
            obj.insert(
                "reference".to_string(),
                serde_json::Value::String(dst_ref.to_string()),
            );

            let new_manifest_path = format!("{}/manifests/{}/{}", cache_dir, dst_repo, dst_ref);
            obj.insert(
                "manifest_path".to_string(),
                serde_json::Value::String(new_manifest_path.clone()),
            );

            // 创建目标manifest目录
            if let Some(parent) = Path::new(&new_manifest_path).parent() {
                fs::create_dir_all(parent)?;
            }

            // 复制manifest文件
            let src_manifest_path = format!("{}/manifests/{}/{}", cache_dir, src_repo, src_ref);
            fs::copy(&src_manifest_path, &new_manifest_path)?;
        }

        // 添加到index
        index.as_object_mut().unwrap().insert(dst_key, dst_entry);

        // 保存index.json
        let updated_index = serde_json::to_string_pretty(&index)?;
        fs::write(&index_path, updated_index)?;

        println!(
            "✅ Copied {}/{} -> {}/{} in cache",
            src_repo, src_ref, dst_repo, dst_ref
        );
        Ok(())
    } else {
        Err(docker_image_pusher::error::RegistryError::Validation(
            format!("Source image {}/{} not found in cache", src_repo, src_ref),
        ))
    }
}
