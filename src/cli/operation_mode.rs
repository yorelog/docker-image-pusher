//! 操作模式定义 - 4种核心模式的统一抽象

use crate::error::{RegistryError, Result};

/// 4种核心操作模式
#[derive(Debug, Clone)]
pub enum OperationMode {
    /// 模式1: 从repository拉取并缓存
    PullAndCache {
        repository: String,
        reference: String,
    },

    /// 模式2: 从tar文件提取并缓存
    ExtractAndCache {
        tar_file: String,
        repository: String,
        reference: String,
    },

    /// 模式3: 从缓存推送（基于manifest）
    PushFromCacheUsingManifest {
        repository: String,
        reference: String,
    },

    /// 模式4: 从缓存推送（基于tar） - 实际与模式3相同
    PushFromCacheUsingTar {
        repository: String,
        reference: String,
    },
}

impl OperationMode {
    /// 获取模式描述
    pub fn description(&self) -> &'static str {
        match self {
            OperationMode::PullAndCache { .. } => "Pull from registry and cache locally",
            OperationMode::ExtractAndCache { .. } => "Extract from tar file and cache locally",
            OperationMode::PushFromCacheUsingManifest { .. } => "Push from cache using manifest",
            OperationMode::PushFromCacheUsingTar { .. } => "Push from cache using tar reference",
        }
    }

    /// 检查是否需要Registry客户端
    pub fn requires_registry_client(&self) -> bool {
        match self {
            OperationMode::PullAndCache { .. } => true,
            OperationMode::ExtractAndCache { .. } => false,
            OperationMode::PushFromCacheUsingManifest { .. } => true,
            OperationMode::PushFromCacheUsingTar { .. } => true,
        }
    }

    /// 获取目标repository和reference
    pub fn get_target(&self) -> (&str, &str) {
        match self {
            OperationMode::PullAndCache {
                repository,
                reference,
            } => (repository, reference),
            OperationMode::ExtractAndCache {
                repository,
                reference,
                ..
            } => (repository, reference),
            OperationMode::PushFromCacheUsingManifest {
                repository,
                reference,
            } => (repository, reference),
            OperationMode::PushFromCacheUsingTar {
                repository,
                reference,
            } => (repository, reference),
        }
    }

    /// 获取源信息（如果适用）
    pub fn get_source(&self) -> Option<&str> {
        match self {
            OperationMode::ExtractAndCache { tar_file, .. } => Some(tar_file),
            _ => None,
        }
    }

    /// 验证操作模式参数
    pub fn validate(&self) -> Result<()> {
        match self {
            OperationMode::PullAndCache {
                repository,
                reference,
            } => Self::validate_repository_reference(repository, reference),
            OperationMode::ExtractAndCache {
                tar_file,
                repository,
                reference,
            } => {
                Self::validate_tar_file(tar_file)?;
                Self::validate_repository_reference(repository, reference)
            }
            OperationMode::PushFromCacheUsingManifest {
                repository,
                reference,
            }
            | OperationMode::PushFromCacheUsingTar {
                repository,
                reference,
            } => Self::validate_repository_reference(repository, reference),
        }
    }

    fn validate_repository_reference(repository: &str, reference: &str) -> Result<()> {
        if repository.is_empty() {
            return Err(RegistryError::Validation(
                "Repository cannot be empty".to_string(),
            ));
        }
        if reference.is_empty() {
            return Err(RegistryError::Validation(
                "Reference cannot be empty".to_string(),
            ));
        }

        // 验证repository格式（基本验证）
        if repository.contains("..") || repository.starts_with('/') || repository.ends_with('/') {
            return Err(RegistryError::Validation(format!(
                "Invalid repository format: {}",
                repository
            )));
        }

        Ok(())
    }

    fn validate_tar_file(tar_file: &str) -> Result<()> {
        if tar_file.is_empty() {
            return Err(RegistryError::Validation(
                "Tar file path cannot be empty".to_string(),
            ));
        }

        let path = std::path::Path::new(tar_file);
        if !path.exists() {
            return Err(RegistryError::Validation(format!(
                "Tar file does not exist: {}",
                tar_file
            )));
        }

        if !path.is_file() {
            return Err(RegistryError::Validation(format!(
                "Path is not a file: {}",
                tar_file
            )));
        }

        Ok(())
    }

    /// 检查是否为推送操作
    pub fn is_push_operation(&self) -> bool {
        matches!(
            self,
            OperationMode::PushFromCacheUsingManifest { .. }
                | OperationMode::PushFromCacheUsingTar { .. }
        )
    }

    /// 检查是否为缓存操作
    pub fn is_cache_operation(&self) -> bool {
        matches!(
            self,
            OperationMode::PullAndCache { .. } | OperationMode::ExtractAndCache { .. }
        )
    }

    /// 检查是否需要访问tar文件
    pub fn requires_tar_file(&self) -> bool {
        matches!(
            self,
            OperationMode::ExtractAndCache { .. }
        )
    }
}

impl std::fmt::Display for OperationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationMode::PullAndCache {
                repository,
                reference,
            } => {
                write!(f, "Pull {}:{} from registry", repository, reference)
            }
            OperationMode::ExtractAndCache {
                tar_file,
                repository,
                reference,
            } => {
                write!(
                    f,
                    "Extract {} to cache as {}:{}",
                    tar_file, repository, reference
                )
            }
            OperationMode::PushFromCacheUsingManifest {
                repository,
                reference,
            } => {
                write!(f, "Push {}:{} from cache (manifest)", repository, reference)
            }
            OperationMode::PushFromCacheUsingTar {
                repository,
                reference,
            } => {
                write!(f, "Push {}:{} from cache (tar)", repository, reference)
            }
        }
    }
}
