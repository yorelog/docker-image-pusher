//! Command line argument parsing and validation
//!
//! This module defines the Args struct for parsing CLI arguments using clap,
//! and provides validation logic for user input.

use crate::error::{RegistryError, Result};
use clap::{ArgAction, Args as ClapArgs, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "docker-image-pusher",
    version = "0.2.0",
    about = "Docker 镜像操作工具 - 支持4种核心操作模式",
    long_about = "高性能的 Docker 镜像管理工具，支持从 registry 拉取镜像、从 tar 文件提取镜像、缓存镜像并推送到 registry。"
)]
pub struct Args {
    /// 子命令
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// 支持的子命令
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// 从 repository 拉取镜像并缓存
    Pull(PullArgs),

    /// 从 tar 文件提取镜像并缓存
    Extract(ExtractArgs),

    /// 推送镜像到 repository
    Push(PushArgs),

    /// 列出缓存中的镜像
    List(ListArgs),

    /// 清理缓存
    Clean(CleanArgs),
}

impl Args {
    pub fn parse() -> Self {
        <Self as Parser>::parse()
    }

    pub fn try_parse() -> Result<Self> {
        <Self as Parser>::try_parse()
            .map_err(|e| RegistryError::Validation(format!("Failed to parse arguments: {}", e)))
    }

    /// Validate command line arguments
    pub fn validate(&self) -> Result<()> {
        match &self.command {
            Some(cmd) => match cmd {
                Commands::Pull(args) => args.validate(),
                Commands::Extract(args) => args.validate(),
                Commands::Push(args) => args.validate(),
                Commands::List(args) => args.validate(),
                Commands::Clean(args) => args.validate(),
            },
            None => Err(RegistryError::Validation(
                "No command provided. Use --help for usage information.".into(),
            )),
        }
    }
}

/// 拉取镜像的参数
#[derive(ClapArgs, Debug, Clone)]
pub struct PullArgs {
    /// Registry 地址 (默认: https://registry-1.docker.io)
    #[arg(long, default_value = "https://registry-1.docker.io")]
    pub registry: String,

    /// Repository 名称 (例如: library/ubuntu)
    #[arg(short, long)]
    pub repository: String,

    /// 标签或摘要 (例如: latest 或 sha256:...)
    #[arg(short, long)]
    pub reference: String,

    /// Registry 用户名
    #[arg(short, long)]
    pub username: Option<String>,

    /// Registry 密码
    #[arg(short, long)]
    pub password: Option<String>,

    /// 跳过 TLS 证书验证
    #[arg(long, action = ArgAction::SetTrue)]
    pub skip_tls: bool,

    /// 缓存目录 (默认: .cache)
    #[arg(long, default_value = ".cache")]
    pub cache_dir: PathBuf,

    /// 启用详细输出
    #[arg(short, long, action = ArgAction::SetTrue)]
    pub verbose: bool,

    /// 超时时间（秒）
    #[arg(short = 't', long, default_value = "3600")]
    pub timeout: u64,
}

impl PullArgs {
    pub fn validate(&self) -> Result<()> {
        // Validate repository format
        if self.repository.is_empty() {
            return Err(RegistryError::Validation(
                "Repository name cannot be empty".to_string(),
            ));
        }

        // Validate reference format
        if self.reference.is_empty() {
            return Err(RegistryError::Validation(
                "Reference cannot be empty".to_string(),
            ));
        }

        // Validate registry URL format
        if !self.registry.starts_with("http://") && !self.registry.starts_with("https://") {
            return Err(RegistryError::Validation(format!(
                "Invalid registry URL: {}. Must start with http:// or https://",
                self.registry
            )));
        }

        // Validate authentication configuration consistency
        if (self.username.is_some() && self.password.is_none())
            || (self.username.is_none() && self.password.is_some())
        {
            return Err(RegistryError::Validation(
                "Username and password must be provided together".to_string(),
            ));
        }

        Ok(())
    }
}

/// 从 tar 文件提取镜像的参数
#[derive(ClapArgs, Debug, Clone)]
pub struct ExtractArgs {
    /// Docker 镜像 tar 文件路径
    #[arg(short, long, value_name = "FILE")]
    pub file: PathBuf,

    /// 缓存目录 (默认: .cache)
    #[arg(long, default_value = ".cache")]
    pub cache_dir: PathBuf,

    /// 启用详细输出
    #[arg(short, long, action = ArgAction::SetTrue)]
    pub verbose: bool,
}

impl ExtractArgs {
    pub fn validate(&self) -> Result<()> {
        // Validate file exists
        if !self.file.exists() {
            return Err(RegistryError::Validation(format!(
                "Tar file '{}' does not exist",
                self.file.display()
            )));
        }

        Ok(())
    }
}

/// 推送镜像的参数
#[derive(ClapArgs, Debug, Clone)]
pub struct PushArgs {
    /// 源镜像 (格式: repository:tag 或 tar 文件路径)
    #[arg(short, long)]
    pub source: String,

    /// 目标 Registry 地址 (默认: https://registry-1.docker.io)
    #[arg(long, default_value = "https://registry-1.docker.io")]
    pub registry: String,

    /// 目标 Repository 名称
    #[arg(short, long)]
    pub repository: String,

    /// 目标标签
    #[arg(short, long)]
    pub reference: String,

    /// Registry 用户名
    #[arg(short, long)]
    pub username: Option<String>,

    /// Registry 密码
    #[arg(short, long)]
    pub password: Option<String>,

    /// 跳过 TLS 证书验证
    #[arg(long, action = ArgAction::SetTrue)]
    pub skip_tls: bool,

    /// 缓存目录 (默认: .cache)
    #[arg(long, default_value = ".cache")]
    pub cache_dir: PathBuf,

    /// 启用详细输出
    #[arg(short, long, action = ArgAction::SetTrue)]
    pub verbose: bool,

    /// 超时时间（秒）
    #[arg(short = 't', long, default_value = "7200")]
    pub timeout: u64,

    /// 重试次数
    #[arg(long, default_value = "3")]
    pub retry_attempts: usize,

    /// 最大并发上传数
    #[arg(long, default_value = "1")]
    pub max_concurrent: usize,

    /// 大层阈值（字节）
    #[arg(long, default_value = "1073741824")]
    pub large_layer_threshold: u64,

    /// 跳过已存在的层
    #[arg(long, action = ArgAction::SetTrue)]
    pub skip_existing: bool,

    /// 强制上传即使层已存在
    #[arg(long, action = ArgAction::SetTrue)]
    pub force_upload: bool,

    /// 验证模式（不实际上传）
    #[arg(long, action = ArgAction::SetTrue)]
    pub dry_run: bool,

    /// 使用高级并发上传器（带动态调节和性能优化）
    #[arg(long, action = ArgAction::SetTrue)]
    pub use_concurrent_uploader: bool,
}

impl PushArgs {
    pub fn validate(&self) -> Result<()> {
        // Validate source format
        if self.source.is_empty() {
            return Err(RegistryError::Validation(
                "Source cannot be empty".to_string(),
            ));
        }

        // Validate repository format
        if self.repository.is_empty() {
            return Err(RegistryError::Validation(
                "Repository name cannot be empty".to_string(),
            ));
        }

        // Validate reference format
        if self.reference.is_empty() {
            return Err(RegistryError::Validation(
                "Reference cannot be empty".to_string(),
            ));
        }

        // 验证registry URL格式
        if !self.registry.starts_with("http://") && !self.registry.starts_with("https://") {
            return Err(RegistryError::Validation(format!(
                "Invalid registry URL: {}. Must start with http:// or https://",
                self.registry
            )));
        }

        // 验证认证配置的一致性
        if (self.username.is_some() && self.password.is_none())
            || (self.username.is_none() && self.password.is_some())
        {
            return Err(RegistryError::Validation(
                "Username and password must be provided together".to_string(),
            ));
        }

        // 验证并发数量
        if self.max_concurrent == 0 {
            return Err(RegistryError::Validation(
                "max_concurrent must be greater than 0".to_string(),
            ));
        }

        // 验证冲突选项
        if self.skip_existing && self.force_upload {
            return Err(RegistryError::Validation(
                "Cannot specify both --skip-existing and --force-upload".to_string(),
            ));
        }

        // 验证source是tar文件时的路径存在性
        if self.source.ends_with(".tar") || self.source.ends_with(".tar.gz") {
            let source_path = std::path::Path::new(&self.source);
            if !source_path.exists() {
                return Err(RegistryError::Validation(format!(
                    "Source tar file '{}' does not exist",
                    self.source
                )));
            }
        }

        Ok(())
    }

    /// 判断source是否为tar文件
    pub fn is_tar_source(&self) -> bool {
        self.source.ends_with(".tar")
            || self.source.ends_with(".tar.gz")
            || self.source.ends_with(".tgz")
    }

    /// 解析source为repository:reference格式
    pub fn parse_source_repository(&self) -> Option<(String, String)> {
        if self.is_tar_source() {
            return None;
        }

        if let Some(colon_pos) = self.source.rfind(':') {
            let repository = self.source[..colon_pos].to_string();
            let reference = self.source[colon_pos + 1..].to_string();
            Some((repository, reference))
        } else {
            Some((self.source.clone(), "latest".to_string()))
        }
    }
}

/// 列出缓存中的镜像的参数
#[derive(ClapArgs, Debug, Clone)]
pub struct ListArgs {
    /// 缓存目录 (默认: .cache)
    #[arg(long, default_value = ".cache")]
    pub cache_dir: PathBuf,
}

impl ListArgs {
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

/// 清理缓存的参数
#[derive(ClapArgs, Debug, Clone)]
pub struct CleanArgs {
    /// 缓存目录 (默认: .cache)
    #[arg(long, default_value = ".cache")]
    pub cache_dir: PathBuf,

    /// 强制删除所有缓存
    #[arg(long, action = ArgAction::SetTrue)]
    pub force: bool,
}

impl CleanArgs {
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_no_command() {
        let args = Args { command: None };
        assert!(args.validate().is_err());
    }

    #[test]
    fn test_validation_credentials_mismatch() {
        let args = PullArgs {
            registry: "https://registry.example.com".to_string(),
            repository: "test".to_string(),
            reference: "latest".to_string(),
            username: Some("user".to_string()),
            password: None, // Missing password
            skip_tls: false,
            cache_dir: PathBuf::from(".cache"),
            verbose: false,
            timeout: 3600,
        };

        assert!(args.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_registry_url() {
        let args = PullArgs {
            registry: "invalid-url".to_string(),
            repository: "test".to_string(),
            reference: "latest".to_string(),
            username: None,
            password: None,
            skip_tls: false,
            cache_dir: PathBuf::from(".cache"),
            verbose: false,
            timeout: 3600,
        };

        assert!(args.validate().is_err());
    }

    #[test]
    fn test_validation_max_concurrent() {
        let args = PushArgs {
            source: "test:latest".to_string(),
            registry: "https://registry.example.com".to_string(),
            repository: "test".to_string(),
            reference: "latest".to_string(),
            username: None,
            password: None,
            skip_tls: false,
            cache_dir: PathBuf::from(".cache"),
            verbose: false,
            timeout: 7200,
            retry_attempts: 3,
            max_concurrent: 0, // Invalid value
            large_layer_threshold: 1073741824,
            skip_existing: false,
            force_upload: false,
            dry_run: false,
            use_concurrent_uploader: false,
        };

        assert!(args.validate().is_err());
    }
}
