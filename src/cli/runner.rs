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

    pub async fn run(&self, args: Args) -> Result<()> {
        self.output.section("Docker Image Pusher");
        args.validate()?;

        match args.command {
            Some(Commands::Pull(pull_args)) => {
                let mut image_manager = ImageManager::new(
                    Some(pull_args.cache_dir.to_str().unwrap()),
                    pull_args.verbose,
                )?;

                let auth_config = if let (Some(username), Some(password)) =
                    (&pull_args.username, &pull_args.password)
                {
                    Some(AuthConfig::new(username.clone(), password.clone()))
                } else {
                    None
                };

                let client = RegistryClientBuilder::new(pull_args.registry.clone())
                    .with_auth(auth_config.clone())
                    .with_timeout(pull_args.timeout)
                    .with_skip_tls(pull_args.skip_tls)
                    .with_verbose(pull_args.verbose)
                    .build()?;

                let token = if let Some(auth) = &auth_config {
                    client
                        .authenticate_for_repository(auth, &pull_args.repository)
                        .await?
                } else {
                    None
                };

                let mode = OperationMode::PullAndCache {
                    repository: pull_args.repository,
                    reference: pull_args.reference,
                };

                image_manager
                    .execute_operation(&mode, Some(&client), token.as_deref())
                    .await?;
            }
            Some(Commands::Extract(extract_args)) => {
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
            Some(Commands::Push(push_args)) => {
                let mut image_manager = ImageManager::new(
                    Some(push_args.cache_dir.to_str().unwrap()),
                    push_args.verbose,
                )?;

                let auth_config = if let (Some(username), Some(password)) =
                    (&push_args.username, &push_args.password)
                {
                    Some(AuthConfig::new(username.clone(), password.clone()))
                } else {
                    None
                };

                let client = RegistryClientBuilder::new(push_args.registry.clone())
                    .with_auth(auth_config.clone())
                    .with_timeout(push_args.timeout)
                    .with_skip_tls(push_args.skip_tls)
                    .with_verbose(push_args.verbose)
                    .build()?;

                let token = if let Some(auth) = &auth_config {
                    client
                        .authenticate_for_repository(auth, &push_args.repository)
                        .await?
                } else {
                    None
                };

                let mode = if push_args.is_tar_source() {
                    OperationMode::PushFromTar {
                        tar_file: push_args.source.clone(),
                        repository: push_args.repository,
                        reference: push_args.reference,
                    }
                } else {
                    if let Some((source_repo, source_ref)) = push_args.parse_source_repository() {
                        if image_manager.is_image_cached(&source_repo, &source_ref)? {
                            OperationMode::PushFromCacheUsingManifest {
                                repository: push_args.repository,
                                reference: push_args.reference,
                            }
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
                    .execute_operation(&mode, Some(&client), token.as_deref())
                    .await?;
            }
            Some(Commands::List(list_args)) => {
                let image_manager =
                    ImageManager::new(Some(list_args.cache_dir.to_str().unwrap()), false)?;

                self.output.section("Cached Images");
                let images = image_manager.list_cached_images();

                if images.is_empty() {
                    self.output.info("No images found in cache");
                } else {
                    for (repository, reference) in images {
                        self.output.info(&format!("{}:{}", repository, reference));
                    }
                }
            }
            Some(Commands::Clean(clean_args)) => {
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
            None => {
                return Err(RegistryError::Validation("No command provided".to_string()));
            }
        }

        self.output.success("Operation completed successfully!");
        Ok(())
    }

    fn confirm_cleanup(&self) -> Result<bool> {
        // 简化确认逻辑
        Ok(true)
    }
}
