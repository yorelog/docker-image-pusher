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
    version = "0.2.2",
    about = "Docker 镜像操作工具 - 支持4种核心操作模式",
    long_about = "高性能的 Docker 镜像管理工具，支持从 registry 拉取镜像、从 tar 文件提取镜像、缓存镜像并推送到 registry。"
)]
pub struct Args {
    /// Docker 镜像 tar 文件路径 (当未指定子命令时，默认执行 extract)
    #[arg(value_name = "FILE")]
    pub file: Option<PathBuf>,

    /// 子命令
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// 支持的子命令
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// 从 repository 拉取镜像并缓存
    #[command(alias = "p")]
    Pull(PullArgs),

    /// 从 tar 文件提取镜像并缓存
    #[command(alias = "e")]
    Extract(ExtractArgs),

    /// 推送镜像到 repository
    #[command(alias = "ps")]
    Push(PushArgs),

    /// 列出缓存中的镜像
    #[command(aliases = ["l", "ls"])]
    List(ListArgs),

    /// 清理缓存
    #[command(alias = "c")]
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
            None => {
                // 当没有提供子命令时，默认列出缓存内容且显示帮助
                Ok(())
            }
        }
    }

    /// 获取有效的命令，如果没有提供子命令则返回默认的 list 命令
    pub fn get_effective_command(&self) -> Commands {
        match &self.command {
            Some(cmd) => cmd.clone(),
            None => {
                // 默认列出缓存内容
                Commands::List(ListArgs {
                    cache_dir: PathBuf::from(".cache"),
                })
            }
        }
    }

    /// 检查是否需要显示帮助信息
    pub fn should_show_help(&self) -> bool {
        self.command.is_none()
    }
}

/// 解析后的镜像信息
#[derive(Debug, Clone)]
pub struct ParsedImage {
    pub registry: String,
    pub repository: String,
    pub tag: String,
}

/// 拉取镜像的参数
#[derive(ClapArgs, Debug, Clone)]
pub struct PullArgs {
    /// 完整镜像引用 (支持多种格式):
    /// - docker.io/library/ubuntu:latest  
    /// - ubuntu:latest (默认使用 docker.io/library)
    /// - ubuntu (默认标签 latest)
    #[arg(short, long, value_name = "IMAGE")]
    pub image: String,

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

    /// 最大并发下载数 (可以使用 --concurrent 作为别名)
    #[arg(long, alias = "concurrent", default_value = "8")]
    pub max_concurrent: usize,
}

impl PullArgs {
    /// 解析镜像引用，支持多种格式
    pub fn parse_image(&self) -> Result<ParsedImage> {
        parse_image_reference(&self.image)
    }

    pub fn validate(&self) -> Result<()> {
        // 验证镜像引用格式
        if self.image.is_empty() {
            return Err(RegistryError::Validation(
                "Image reference cannot be empty".to_string(),
            ));
        }

        // 尝试解析镜像引用
        self.parse_image()?;

        // 验证认证配置一致性
        if (self.username.is_some() && self.password.is_none())
            || (self.username.is_none() && self.password.is_some())
        {
            return Err(RegistryError::Validation(
                "Username and password must be provided together".to_string(),
            ));
        }

        // 验证并发参数
        if self.max_concurrent == 0 {
            return Err(RegistryError::Validation(
                "max_concurrent must be greater than 0".to_string(),
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

    /// 目标镜像引用 (支持多种格式):
    /// - docker.io/library/ubuntu:latest
    /// - ubuntu:latest (默认使用 docker.io/library)
    #[arg(short, long, value_name = "TARGET")]
    pub target: String,

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
    #[arg(long, default_value = "7200")]
    pub timeout: u64,

    /// 最大并发上传数 (基础设置，AdaptiveConcurrencyManager 会动态调整)
    #[arg(long, alias = "concurrent", default_value = "8")]
    pub max_concurrent: usize,

    /// 跳过已存在的层
    #[arg(long, action = ArgAction::SetTrue)]
    pub skip_existing: bool,

    /// 强制上传即使层已存在
    #[arg(long, action = ArgAction::SetTrue)]
    pub force_upload: bool,

    /// 验证模式（不实际上传）
    #[arg(long, action = ArgAction::SetTrue)]
    pub dry_run: bool,
}

impl PushArgs {
    /// 解析目标镜像引用
    pub fn parse_target(&self) -> Result<ParsedImage> {
        parse_image_reference(&self.target)
    }

    pub fn validate(&self) -> Result<()> {
        // Validate source format
        if self.source.is_empty() {
            return Err(RegistryError::Validation(
                "Source cannot be empty".to_string(),
            ));
        }

        // 验证目标镜像引用格式
        if self.target.is_empty() {
            return Err(RegistryError::Validation(
                "Target image reference cannot be empty".to_string(),
            ));
        }

        // 尝试解析目标镜像引用
        self.parse_target()?;

        // 验证认证配置一致性
        if (self.username.is_some() && self.password.is_none())
            || (self.username.is_none() && self.password.is_some())
        {
            return Err(RegistryError::Validation(
                "Username and password must be provided together".to_string(),
            ));
        }

        // 验证并发参数
        if self.max_concurrent == 0 {
            return Err(RegistryError::Validation(
                "max_concurrent must be greater than 0".to_string(),
            ));
        }

        // 验证冲突的参数
        if self.skip_existing && self.force_upload {
            return Err(RegistryError::Validation(
                "Cannot specify both --skip-existing and --force-upload".to_string(),
            ));
        }

        // 如果source不是tar文件，验证它是否存在于缓存中
        if !self.is_tar_source() {
            // 这里应该验证缓存中是否存在该镜像，但需要访问缓存
            // 暂时只检查格式
        } else {
            // 验证tar文件存在
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

/// 解析镜像引用，支持多种格式
/// 
/// 支持的格式:
/// - docker.io/library/ubuntu:latest
/// - ubuntu:latest (默认使用 docker.io/library)
/// - ubuntu (默认标签 latest)
pub fn parse_image_reference(image_ref: &str) -> Result<ParsedImage> {
    let image_ref = image_ref.trim();
    
    if image_ref.is_empty() {
        return Err(RegistryError::Validation(
            "Image reference cannot be empty".to_string(),
        ));
    }

    // 首先分离标签部分
    let (image_part, tag) = if let Some(colon_pos) = image_ref.rfind(':') {
        // 检查冒号后面是否是端口号（如果包含 '/' 则不是标签）
        let after_colon = &image_ref[colon_pos + 1..];
        if after_colon.contains('/') {
            // 这是端口号，没有标签
            (image_ref, "latest")
        } else {
            // 这是标签
            (&image_ref[..colon_pos], after_colon)
        }
    } else {
        // 没有冒号，使用默认标签
        (image_ref, "latest")
    };

    // 解析注册表和仓库部分
    let parts: Vec<&str> = image_part.split('/').collect();
    
    let (registry, repository) = match parts.len() {
        1 => {
            // 格式: ubuntu
            ("https://registry-1.docker.io".to_string(), format!("library/{}", parts[0]))
        }
        2 => {
            // 可能是: library/ubuntu 或 registry.com/repo
            if parts[0].contains('.') || parts[0].contains(':') {
                // 包含域名或端口，这是自定义注册表
                let registry_url = if parts[0].starts_with("http://") || parts[0].starts_with("https://") {
                    parts[0].to_string()
                } else {
                    format!("https://{}", parts[0])
                };
                (registry_url, parts[1].to_string())
            } else {
                // 这是 Docker Hub 的 namespace/repository 格式
                ("https://registry-1.docker.io".to_string(), format!("{}/{}", parts[0], parts[1]))
            }
        }
        3 => {
            // 格式: registry.com/namespace/repo
            let registry_url = if parts[0].starts_with("http://") || parts[0].starts_with("https://") {
                parts[0].to_string()
            } else {
                format!("https://{}", parts[0])
            };
            (registry_url, format!("{}/{}", parts[1], parts[2]))
        }
        _ => {
            // 更复杂的路径，将第一部分作为注册表，其余作为仓库路径
            let registry_url = if parts[0].starts_with("http://") || parts[0].starts_with("https://") {
                parts[0].to_string()
            } else {
                format!("https://{}", parts[0])
            };
            let repository_path = parts[1..].join("/");
            (registry_url, repository_path)
        }
    };

    Ok(ParsedImage {
        registry,
        repository,
        tag: tag.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_no_command() {
        let args = Args { 
            file: None,
            command: None 
        };
        assert!(args.validate().is_ok()); 
    }

    #[test]
    fn test_validation_credentials_mismatch() {
        let args = PullArgs {
            image: "registry.example.com/test:latest".to_string(),
            username: Some("user".to_string()),
            password: None, // Missing password
            skip_tls: false,
            cache_dir: PathBuf::from(".cache"),
            verbose: false,
            timeout: 3600,
            max_concurrent: 8,
        };

        assert!(args.validate().is_err());
    }

    #[test]
    fn test_parse_image_simple() {
        let parsed = parse_image_reference("ubuntu").unwrap();
        assert_eq!(parsed.registry, "https://registry-1.docker.io");
        assert_eq!(parsed.repository, "library/ubuntu");
        assert_eq!(parsed.tag, "latest");
    }

    #[test]
    fn test_parse_image_with_tag() {
        let parsed = parse_image_reference("ubuntu:22.04").unwrap();
        assert_eq!(parsed.registry, "https://registry-1.docker.io");
        assert_eq!(parsed.repository, "library/ubuntu");
        assert_eq!(parsed.tag, "22.04");
    }

    #[test]
    fn test_parse_image_with_namespace() {
        let parsed = parse_image_reference("library/ubuntu:latest").unwrap();
        assert_eq!(parsed.registry, "https://registry-1.docker.io");
        assert_eq!(parsed.repository, "library/ubuntu");
        assert_eq!(parsed.tag, "latest");
    }


    #[test]
    fn test_parse_image_custom_registry_with_port() {
        let parsed = parse_image_reference("localhost:5000/myapp:latest").unwrap();
        assert_eq!(parsed.registry, "https://localhost:5000");
        assert_eq!(parsed.repository, "myapp");
        assert_eq!(parsed.tag, "latest");
    }

    #[test]
    fn test_parse_image_empty() {
        assert!(parse_image_reference("").is_err());
    }

    #[test]
    fn test_validation_max_concurrent() {
        let args = PushArgs {
            source: "test:latest".to_string(),
            target: "registry.example.com/test:latest".to_string(),
            username: None,
            password: None,
            skip_tls: false,
            cache_dir: PathBuf::from(".cache"),
            verbose: false,
            timeout: 7200,
            max_concurrent: 0, // Invalid value
            skip_existing: false,
            force_upload: false,
            dry_run: false
        };

        assert!(args.validate().is_err());
    }
}
