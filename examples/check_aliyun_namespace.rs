//! Check Aliyun Registry Namespace - 检查阿里云容器镜像服务命名空间
//!
//! This script helps verify Aliyun registry access and namespace existence

use docker_image_pusher::{AuthConfig, error::Result, registry::RegistryClientBuilder};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🔍 Aliyun Registry Namespace Checker");
    println!("=====================================");

    // 阿里云配置
    let aliyun_registry = "registry.cn-beijing.aliyuncs.com";
    let aliyun_username = "canny_best@163.com";
    let aliyun_password = "ra201222";

    // 要检查的namespace/repository
    let namespace = "canny_best";
    let repository = "canny_best/test-repo";

    println!("📊 Configuration:");
    println!("  Registry: {}", aliyun_registry);
    println!("  Username: {}", aliyun_username);
    println!("  Namespace: {}", namespace);
    println!("  Repository: {}", repository);
    println!();

    // 构建客户端
    println!("🌐 Building Registry Client...");
    let client = RegistryClientBuilder::new(format!("https://{}", aliyun_registry))
        .with_timeout(3600)
        .with_skip_tls(false)
        .with_verbose(true)
        .build()?;

    // 测试连接性
    println!("🔗 Testing registry connectivity...");
    match client.test_connectivity().await {
        Ok(_) => println!("✅ Registry is accessible"),
        Err(e) => {
            println!("⚠️  Registry connectivity test failed: {}", e);
            println!("   This may be normal for some registries");
        }
    }

    // 认证
    println!("🔐 Authenticating...");
    let auth_config = AuthConfig::new(aliyun_username.to_string(), aliyun_password.to_string());

    match client.authenticate(&auth_config).await {
        Ok(Some(token)) => {
            println!("✅ Authentication successful (token received)");

            // 尝试访问repository
            println!("📦 Checking repository access...");
            match client.list_tags(repository, &Some(token.clone())).await {
                Ok(tags) => {
                    println!("✅ Repository {} is accessible", repository);
                    println!("🏷️  Available tags: {:?}", tags);
                }
                Err(e) => {
                    println!("❌ Repository {} is not accessible: {}", repository, e);

                    println!("\n💡 To fix this issue:");
                    println!("   1. Login to Aliyun Console: https://cr.console.aliyun.com/");
                    println!("   2. Create namespace '{}' if it doesn't exist", namespace);
                    println!(
                        "   3. Create repository '{}' in the namespace",
                        repository.split('/').nth(1).unwrap_or("unknown")
                    );
                    println!("   4. Ensure your account has push/pull permissions");

                    return Ok(());
                }
            }

            // 尝试检查认证的repository访问
            println!("🔐 Testing repository-specific authentication...");
            match client
                .authenticate_for_repository(&auth_config, repository)
                .await
            {
                Ok(Some(repo_token)) => {
                    println!("✅ Repository-specific authentication successful");

                    // 检查一个不存在的镜像
                    println!("🔍 Testing image existence check...");
                    match client
                        .check_image_exists(repository, "non-existent-tag", &Some(repo_token))
                        .await
                    {
                        Ok(exists) => {
                            if exists {
                                println!("⚠️  Image unexpectedly exists");
                            } else {
                                println!(
                                    "✅ Image existence check works (image doesn't exist as expected)"
                                );
                            }
                        }
                        Err(e) => {
                            println!("⚠️  Image existence check failed: {}", e);
                        }
                    }
                }
                Ok(None) => {
                    println!("✅ Repository-specific authentication successful (no token)");
                }
                Err(e) => {
                    println!("❌ Repository-specific authentication failed: {}", e);
                }
            }
        }
        Ok(None) => {
            println!("✅ Authentication successful (no token required)");
        }
        Err(e) => {
            println!("❌ Authentication failed: {}", e);
            println!("\n💡 Please check:");
            println!("   - Username: {}", aliyun_username);
            println!("   - Password is correct");
            println!("   - Account has access to Aliyun Container Registry");
            return Ok(());
        }
    }

    println!("\n🎉 All checks completed!");
    println!(
        "   Repository {} appears to be ready for push operations",
        repository
    );

    Ok(())
}
