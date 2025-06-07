//! Authentication Test Demo - 测试 Docker Registry API v2 认证
//!
//! 此示例专门测试新的认证系统是否能正确与Docker Registry API v2工作

use docker_image_pusher::{error::Result, registry::RegistryClientBuilder};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🔐 Docker Registry API v2 Authentication Test");
    println!("==============================================");

    // 配置参数
    let registry = env::var("DOCKER_REGISTRY")
        .unwrap_or_else(|_| "https://registry.cn-beijing.aliyuncs.com".to_string());
    let repository = env::var("DOCKER_REPOSITORY").unwrap_or_else(|_| "yoce/cblt".to_string());

    println!("📋 Configuration:");
    println!("  Registry: {}", registry);
    println!("  Repository: {}", repository);
    println!();

    // 构建 Registry Client
    println!("🌐 Building Registry Client...");
    let client = RegistryClientBuilder::new(registry.clone())
        .with_timeout(300)
        .with_verbose(true)
        .build()?;
    println!("✅ Registry Client built successfully");

    // 测试无凭据的情况
    println!();
    println!("🔍 Test 1: Testing registry authentication challenge...");
    let auth = docker_image_pusher::registry::auth::Auth::new();
    let output = docker_image_pusher::logging::Logger::new(true);

    // 直接调用新的认证方法
    match auth
        .authenticate_with_registry(&registry, &repository, None, None, &output)
        .await
    {
        Ok(token) => {
            if let Some(token) = token {
                println!(
                    "✅ Received authentication token: {}...",
                    &token[..20.min(token.len())]
                );
            } else {
                println!("ℹ️  Registry does not require authentication");
            }
        }
        Err(e) => {
            println!("❌ Authentication test failed: {}", e);
            println!("   This is expected if the registry requires credentials");
        }
    }

    // 测试有凭据的情况（如果提供）
    if let (Ok(username), Ok(password)) = (env::var("DOCKER_USERNAME"), env::var("DOCKER_PASSWORD"))
    {
        println!();
        println!("🔍 Test 2: Testing with provided credentials...");
        println!("  Username: {}", username);

        match auth
            .authenticate_with_registry(
                &registry,
                &repository,
                Some(&username),
                Some(&password),
                &output,
            )
            .await
        {
            Ok(token) => {
                if let Some(token) = token {
                    println!("✅ Successfully authenticated with credentials");
                    println!("  Token: {}...", &token[..50.min(token.len())]);

                    // 测试token是否能用于访问manifest
                    println!();
                    println!("🔍 Test 3: Testing token with manifest access...");
                    match client
                        .pull_manifest(&repository, "yoce", &Some(token))
                        .await
                    {
                        Ok(manifest) => {
                            println!("✅ Successfully pulled manifest using token");
                            println!("  Manifest size: {} bytes", manifest.len());
                        }
                        Err(e) => {
                            println!("❌ Failed to pull manifest with token: {}", e);
                        }
                    }
                } else {
                    println!("ℹ️  Authentication successful but no token required");
                }
            }
            Err(e) => {
                println!("❌ Authentication with credentials failed: {}", e);
            }
        }
    } else {
        println!();
        println!("ℹ️  No credentials provided via DOCKER_USERNAME/DOCKER_PASSWORD");
        println!("   Set these environment variables to test credential-based authentication");
    }

    println!();
    println!("🏁 Authentication test completed");

    Ok(())
}
