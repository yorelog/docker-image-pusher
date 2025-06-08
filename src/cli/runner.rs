//! 简化的运行器，使用操作模式
//!
//! 这个模块实现了主要的工作流程，根据操作模式执行相应的操作

use crate::cli::args::{Args, Commands};
use crate::cli::config::AuthConfig;
use crate::cli::operation_mode::OperationMode;
use crate::error::{RegistryError, Result};
use crate::image::image_manager::ImageManager;
use crate::logging::Logger;
use crate::registry::RegistryClientBuilder;

pub struct Runner {
    output: Logger,
}

impl Runner {
    pub fn new(verbose: bool) -> Self {
        Self {
            output: Logger::new(verbose),
        }
    }

    /// 统一的认证方法，用于 pull 和 push 操作
    async fn authenticate_with_registry(
        &self,
        client: &crate::registry::client::RegistryClient,
        auth_config: &Option<AuthConfig>,
        registry: &str,
        repository: &str,
    ) -> Result<(Option<String>, crate::registry::client::RegistryClient)> {
        if let Some(auth) = auth_config {
            // 使用提供的凭据进行认证，获取 TokenInfo 用于自动刷新
            self.output.verbose(&format!("Authenticating with provided credentials for registry: {}", registry));
            
            let token_info = client.auth.authenticate_with_token_info(
                registry,
                repository,
                Some(&auth.username),
                Some(&auth.password),
                &self.output,
            ).await?;

            let token = token_info.as_ref().map(|info| info.token.clone());
            
            // Create token manager for automatic refresh
            let token_manager = if token_info.is_some() {
                let token_manager = crate::registry::token_manager::TokenManager::new(
                    client.auth.clone(),
                    self.output.clone(),
                ).with_token_info(token_info);
                Some(token_manager)
            } else {
                None
            };
            
            // Create a client with token manager enabled
            let enhanced_client = client.clone().with_token_manager(token_manager);
            
            if token.is_some() {
                self.output.success(&format!(
                    "Repository authentication successful for: {} (token management enabled)",
                    repository
                ));
            } else {
                self.output
                    .info("No repository-specific authentication required");
            }

            Ok((token, enhanced_client))
        } else {
            // 尝试匿名认证用于公共仓库
            self.output.verbose("No credentials provided, attempting anonymous authentication...");
            match client.auth.authenticate_with_registry(
                registry,
                repository,
                None,
                None,
                &self.output,
            ).await {
                Ok(token) => {
                    if token.is_some() {
                        self.output.success("Anonymous authentication successful");
                    } else {
                        self.output.info("No authentication required for this registry");
                    }
                    Ok((token, client.clone()))
                }
                Err(e) => {
                    self.output.warning(&format!("Anonymous authentication failed: {}", e));
                    self.output.info("Proceeding without authentication token for public registry access");
                    Ok((None, client.clone()))
                }
            }
        }
    }

    pub async fn run(&self, args: Args) -> Result<()> {
        self.output.section("Docker Image Pusher");
        args.validate()?;

        // 如果没有提供子命令，先显示帮助信息并执行 list 命令
        if args.should_show_help() {
            self.show_help_and_cache();
            // 执行 list 命令显示缓存内容
            let list_args = crate::cli::args::ListArgs {
                cache_dir: std::path::PathBuf::from(".cache"),
            };
            self.execute_list_command(&list_args).await?;
            return Ok(());
        }

        match args.get_effective_command() {
            Commands::Pull(pull_args) => {
                let mut image_manager = ImageManager::new(
                    Some(pull_args.cache_dir.to_str().unwrap()),
                    pull_args.verbose,
                )?;

                // Configure concurrency based on CLI args
                let concurrency_config = crate::concurrency::ConcurrencyConfig::default()
                    .with_max_concurrent(pull_args.max_concurrent);
                image_manager.configure_concurrency(concurrency_config);
                
                // Parse the image reference to get registry, repository, and tag
                let parsed_image = pull_args.parse_image()?;

                let auth_config = if let (Some(username), Some(password)) =
                    (&pull_args.username, &pull_args.password)
                {
                    Some(AuthConfig::new(username.clone(), password.clone()))
                } else {
                    None
                };

                let client = RegistryClientBuilder::new(parsed_image.registry.clone())
                    .with_auth(auth_config.clone())
                    .with_timeout(pull_args.timeout)
                    .with_skip_tls(pull_args.skip_tls)
                    .with_verbose(pull_args.verbose)
                    .build()?;

                let (token, enhanced_client) = self
                    .authenticate_with_registry(&client, &auth_config, &parsed_image.registry, &parsed_image.repository)
                    .await?;

                let mode = OperationMode::PullAndCache {
                    repository: parsed_image.repository,
                    reference: parsed_image.tag,
                };

                image_manager
                    .execute_operation(&mode, Some(&enhanced_client), token.as_deref())
                    .await?;
            }
            Commands::Extract(extract_args) => {
                let mut image_manager = ImageManager::new(
                    Some(extract_args.cache_dir.to_str().unwrap()),
                    extract_args.verbose,
                )?;

                let file_stem = extract_args
                    .file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("extracted-image");
                let repository = format!("local/{}", file_stem);
                let reference = "latest".to_string();

                let mode = OperationMode::ExtractAndCache {
                    tar_file: extract_args.file.to_string_lossy().to_string(),
                    repository,
                    reference,
                };

                image_manager.execute_operation(&mode, None, None).await?;
            }
            Commands::Push(push_args) => {
                let mut image_manager = ImageManager::new(
                    Some(push_args.cache_dir.to_str().unwrap()),
                    push_args.verbose,
                )?;

                // Configure adaptive concurrency with simplified arguments
                let concurrency_config = crate::create_concurrency_config_from_args(
                    push_args.max_concurrent,
                );
                image_manager.configure_concurrency(concurrency_config);

                // Parse the target image reference to get registry, repository, and tag
                let parsed_target = push_args.parse_target()?;

                let auth_config = if let (Some(username), Some(password)) =
                    (&push_args.username, &push_args.password)
                {
                    Some(AuthConfig::new(username.clone(), password.clone()))
                } else {
                    None
                };

                let client = RegistryClientBuilder::new(parsed_target.registry.clone())
                    .with_auth(auth_config.clone())
                    .with_timeout(push_args.timeout)
                    .with_skip_tls(push_args.skip_tls)
                    .with_verbose(push_args.verbose)
                    .build()?;

                let (token, enhanced_client) = self
                    .authenticate_with_registry(&client, &auth_config, &parsed_target.registry, &parsed_target.repository)
                    .await?;

                let mode = if push_args.is_tar_source() {
                    OperationMode::PushFromTar {
                        tar_file: push_args.source.clone(),
                        repository: parsed_target.repository,
                        reference: parsed_target.tag,
                    }
                } else {
                    if let Some((source_repo, source_ref)) = push_args.parse_source_repository() {
                        // Apply the same repository name normalization that's used during caching
                        // to ensure we look up the image with the correct cache key
                        let normalized_source_repo = if source_repo.contains('/') {
                            source_repo.clone()
                        } else {
                            format!("library/{}", source_repo)
                        };
                        
                        if image_manager.is_image_cached(&normalized_source_repo, &source_ref)? {
                            // Use the new method that supports separate source and target coordinates
                            image_manager
                                .execute_push_from_cache_with_source(
                                    &normalized_source_repo,
                                    &source_ref,
                                    &parsed_target.repository,
                                    &parsed_target.tag,
                                    Some(&enhanced_client),
                                    token.as_deref(),
                                )
                                .await?;
                            return Ok(());
                        } else {
                            return Err(RegistryError::Validation(format!(
                                "Source image {}:{} not found in cache. Please pull it first.",
                                source_repo, source_ref
                            )));
                        }
                    } else {
                        return Err(RegistryError::Validation(format!(
                            "Invalid source format: {}. Expected repository:tag or tar file path.",
                            push_args.source
                        )));
                    }
                };

                image_manager
                    .execute_operation(&mode, Some(&enhanced_client), token.as_deref())
                    .await?;
            }
            Commands::List(list_args) => {
                self.execute_list_command(&list_args).await?;
            }
            Commands::Clean(clean_args) => {
                self.output.section("Cleaning Cache");

                if clean_args.force || self.confirm_cleanup()? {
                    if clean_args.cache_dir.exists() {
                        std::fs::remove_dir_all(&clean_args.cache_dir)?;
                        self.output.success("Cache cleaned successfully");
                    } else {
                        self.output.info("Cache directory does not exist");
                    }
                } else {
                    self.output.info("Cache cleanup cancelled");
                }
            }
        }

        self.output.success("Operation completed successfully!");
        Ok(())
    }

    fn confirm_cleanup(&self) -> Result<bool> {
        // 简化确认逻辑
        Ok(true)
    }

    /// 显示帮助信息和当前缓存状态
    fn show_help_and_cache(&self) {
        self.output.info("没有指定子命令，显示当前缓存状态。");
        self.output.info("");
        self.output.info("可用的命令:");
        self.output.info("  pull    - 从 registry 拉取镜像并缓存");
        self.output.info("  extract - 从 tar 文件提取镜像并缓存");
        self.output.info("  push    - 推送镜像到 registry");
        self.output.info("  list    - 列出缓存中的镜像");
        self.output.info("  clean   - 清理缓存");
        self.output.info("");
        self.output.info("使用 --help 获取详细帮助信息。");
        self.output.info("");
    }

    /// 执行 list 命令
    async fn execute_list_command(&self, list_args: &crate::cli::args::ListArgs) -> Result<()> {
        let image_manager =
            ImageManager::new(Some(list_args.cache_dir.to_str().unwrap()), false)?;

        self.output.section("当前缓存内容");
        let images = image_manager.list_cached_images();

        if images.is_empty() {
            self.output.info("缓存中没有找到镜像");
        } else {
            for (repository, reference) in images {
                self.output.info(&format!("{}:{}", repository, reference));
            }
        }
        Ok(())
    }
}
