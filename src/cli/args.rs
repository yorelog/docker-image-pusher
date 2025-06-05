//! Command-line argument parsing

use clap::Parser;

#[derive(Parser)]
#[command(name = "docker-image-pusher")]
#[command(about = "A tool to push Docker image tar packages to registries")]
#[command(version, author)]
pub struct Args {
    /// Repository URL
    #[arg(
        long = "repository-url",
        short = 'r',
        help = "Full repository URL including registry, project, and tag"
    )]
    pub repository_url: String,

    /// Path to the Docker image tar file
    #[arg(
        long = "file",
        short = 'f',
        help = "Path to the Docker image tar file"
    )]
    pub file: String,

    /// Registry username
    #[arg(
        long = "username",
        short = 'u',
        help = "Username for registry authentication"
    )]
    pub username: Option<String>,

    /// Registry password
    #[arg(
        long = "password",
        short = 'p',
        help = "Password for registry authentication"
    )]
    pub password: Option<String>,

    /// Chunk size for upload (default: 10MB)
    #[arg(
        long = "chunk-size",
        short = 'c',
        default_value = "10485760",
        help = "Chunk size for upload in bytes"
    )]
    pub chunk_size: usize,

    /// Number of concurrent uploads
    #[arg(
        long = "concurrency",
        short = 'j',
        default_value = "4",
        help = "Number of concurrent upload workers"
    )]
    pub concurrency: usize,

    /// Skip TLS verification
    #[arg(
        long = "skip-tls",
        short = 'k',
        default_value = "false",
        help = "Skip TLS certificate verification"
    )]
    pub skip_tls: bool,

    /// Verbose output
    #[arg(
        long = "verbose",
        short = 'v',
        help = "Enable verbose output"
    )]
    pub verbose: bool,

    /// Timeout in seconds for network operations
    #[arg(
        long = "timeout",
        short = 't',
        default_value = "300",
        help = "Timeout for network operations in seconds"
    )]
    pub timeout: u64,

    /// Retry attempts for failed operations
    #[arg(
        long = "retry",
        default_value = "3",
        help = "Number of retry attempts for failed operations"
    )]
    pub retry: usize,

    /// Registry type (docker, harbor, etc.)
    #[arg(
        long = "registry-type",
        default_value = "auto",
        help = "Registry type: auto, docker, harbor, aws, gcp, azure"
    )]
    pub registry_type: String,

    /// Force overwrite existing image
    #[arg(
        long = "force",
        help = "Force overwrite existing image"
    )]
    pub force: bool,

    /// Dry run mode (validate without uploading)
    #[arg(
        long = "dry-run",
        short = 'n',
        help = "Perform a dry run without actually uploading"
    )]
    pub dry_run: bool,

    /// Output format for results
    #[arg(
        long = "output",
        short = 'o',
        default_value = "text",
        help = "Output format: text, json, yaml"
    )]
    pub output: String,

    /// Configuration file path
    #[arg(
        long = "config",
        help = "Path to configuration file"
    )]
    pub config: Option<String>,
}

impl Args {
    pub fn parse_args() -> Self {
        Args::parse()
    }

    /// Validate arguments
    pub fn validate(&self) -> Result<(), String> {
        // Validate file exists
        if !std::path::Path::new(&self.file).exists() {
            return Err(format!("File does not exist: {}", self.file));
        }

        // Validate repository URL format
        if !self.repository_url.starts_with("http://") && !self.repository_url.starts_with("https://") {
            return Err("Repository URL must start with http:// or https://".to_string());
        }

        // Validate chunk size
        if self.chunk_size == 0 {
            return Err("Chunk size must be greater than 0".to_string());
        }

        // Validate concurrency
        if self.concurrency == 0 {
            return Err("Concurrency must be greater than 0".to_string());
        }

        // Validate timeout
        if self.timeout == 0 {
            return Err("Timeout must be greater than 0".to_string());
        }

        // Validate output format
        match self.output.as_str() {
            "text" | "json" | "yaml" => {}
            _ => return Err("Output format must be one of: text, json, yaml".to_string()),
        }

        // Validate registry type
        match self.registry_type.as_str() {
            "auto" | "docker" | "harbor" | "aws" | "gcp" | "azure" => {}
            _ => return Err("Registry type must be one of: auto, docker, harbor, aws, gcp, azure".to_string()),
        }

        Ok(())
    }

    /// Print usage examples
    pub fn print_examples() {
        println!("Examples:");
        println!("  # Basic usage with short options");
        println!("  docker-image-pusher -r https://registry.example.com/myproject/myimage:v1.0 -f image.tar");
        println!();
        println!("  # With authentication using short options");
        println!("  docker-image-pusher -r https://harbor.example.com/project/app:latest \\");
        println!("                      -f app.tar -u myuser -p mypassword");
        println!();
        println!("  # With custom settings using long options");
        println!("  docker-image-pusher --repository-url https://registry.example.com/project/app:v2.0 \\");
        println!("                      --file app.tar --username admin --password secret \\");
        println!("                      --chunk-size 5242880 --concurrency 8 --verbose");
        println!();
        println!("  # Dry run to validate without uploading");
        println!("  docker-image-pusher -r https://registry.example.com/test/app:latest \\");
        println!("                      -f test.tar --dry-run --verbose");
        println!();
        println!("  # Skip TLS verification for self-signed certificates");
        println!("  docker-image-pusher -r https://internal-registry.com/app:latest \\");
        println!("                      -f app.tar -u user -p pass --skip-tls");
        println!();
        println!("  # Using environment variables for sensitive data");
        println!("  export DOCKER_PUSHER_USERNAME=myuser");
        println!("  export DOCKER_PUSHER_PASSWORD=mypassword");
        println!("  docker-image-pusher -r https://registry.example.com/app:latest -f app.tar");
    }

    /// Load configuration from environment variables
    pub fn from_env(mut self) -> Self {
        if self.username.is_none() {
            self.username = std::env::var("DOCKER_PUSHER_USERNAME").ok();
        }
        
        if self.password.is_none() {
            self.password = std::env::var("DOCKER_PUSHER_PASSWORD").ok();
        }

        // Override with environment variables if present
        if let Ok(timeout) = std::env::var("DOCKER_PUSHER_TIMEOUT") {
            if let Ok(t) = timeout.parse() {
                self.timeout = t;
            }
        }

        if let Ok(chunk_size) = std::env::var("DOCKER_PUSHER_CHUNK_SIZE") {
            if let Ok(c) = chunk_size.parse() {
                self.chunk_size = c;
            }
        }

        if let Ok(concurrency) = std::env::var("DOCKER_PUSHER_CONCURRENCY") {
            if let Ok(c) = concurrency.parse() {
                self.concurrency = c;
            }
        }

        if std::env::var("DOCKER_PUSHER_VERBOSE").is_ok() {
            self.verbose = true;
        }

        if std::env::var("DOCKER_PUSHER_SKIP_TLS").is_ok() {
            self.skip_tls = true;
        }

        self
    }
}