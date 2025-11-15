use clap::{Parser, Subcommand};
use thiserror::Error;

mod cache;
mod image;
mod oci;
mod push;
mod state;
mod tar_import;

use crate::oci::client::{Client, ClientConfig};

pub const CACHE_DIR: &str = ".cache";
pub const STREAM_BUFFER_SIZE: usize = 8 * 1024 * 1024;
pub const PROGRESS_LAYER_THRESHOLD_BYTES: u64 = 100 * 1024 * 1024;
pub const PROGRESS_UPDATE_INTERVAL_SECS: u64 = 3;
pub const CHUNKED_LAYER_SIZE_BYTES: usize = 50 * 1024 * 1024;
pub const LARGE_LAYER_THRESHOLD_BYTES: u64 = 100 * 1024 * 1024;
pub const LARGE_LAYER_THRESHOLD_MB: f64 = 100.0;
pub const MEDIUM_LAYER_THRESHOLD_MB: f64 = 250.0;
pub const ESTIMATED_SPEED_MBPS: f64 = 180.0;
pub const LARGE_LAYER_PROGRESS_INTERVAL_SECS: u64 = 30;
pub const NORMAL_LAYER_PROGRESS_INTERVAL_SECS: u64 = 10;
pub const RATE_LIMIT_DELAY_MS: u64 = 250;
pub const GZIP_MAGIC_BYTES: [u8; 2] = [0x1F, 0x8B];

#[derive(Parser, Debug)]
#[command(
    name = "docker-image-pusher",
    version,
    about = "Stream large Docker/OCI images through a tiny local cache"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Stream an image from a registry into the on-disk cache
    Pull {
        /// Image reference such as `nginx:latest`
        #[arg(value_name = "IMAGE")]
        image: String,
    },
    /// Push either a cached image or a docker-save tarball to a registry
    Push {
        /// Cached image name or docker-save tar path
        #[arg(value_name = "INPUT")]
        input: String,
        /// Override the destination reference (defaults to inference)
        #[arg(short, long)]
        target: Option<String>,
        /// Force a specific registry hostname
        #[arg(long)]
        registry: Option<String>,
        /// Username override for one-off pushes
        #[arg(long)]
        username: Option<String>,
        /// Password override for one-off pushes
        #[arg(long)]
        password: Option<String>,
    },
    /// Import a docker-save tarball under a friendly cache key
    Import {
        /// Path to the docker-save tarball
        #[arg(value_name = "TAR")]
        tar: String,
        /// Cache key to write under (e.g. myapp:latest)
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Persist credentials for a registry so pushes can reuse them
    Login {
        /// Registry hostname such as registry.example.com
        #[arg(value_name = "REGISTRY")]
        registry: String,
        /// Username to store
        #[arg(long)]
        username: String,
        /// Password to store
        #[arg(long)]
        password: String,
    },
}

#[derive(Error, Debug)]
pub enum PusherError {
    #[error("Pull error: {0}")]
    PullError(String),
    #[error("Push error: {0}")]
    PushError(String),
    #[error("Cache error: {0}")]
    CacheError(String),
    #[error("Cache entry not found")]
    CacheNotFound,
    #[error("Tar error: {0}")]
    TarError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl From<serde_json::Error> for PusherError {
    fn from(value: serde_json::Error) -> Self {
        PusherError::CacheError(format!("JSON error: {}", value))
    }
}

impl PusherError {
    pub fn push_error(message: impl Into<String>) -> Self {
        PusherError::PushError(message.into())
    }

    pub fn tar_error(message: impl Into<String>) -> Self {
        PusherError::TarError(message.into())
    }
}

#[tokio::main]
async fn main() -> Result<(), PusherError> {
    let cli = Cli::parse();
    let client = Client::new(ClientConfig::default());

    match cli.command {
        Commands::Pull { image } => {
            cache::cache_image(&client, &image).await?;
            println!(" Cached image: {}", image);
        }
        Commands::Push {
            input,
            target,
            username,
            password,
            registry,
        } => {
            push::run_push(&client, &input, target, username, password, registry).await?;
        }
        Commands::Import { tar, name } => {
            tar_import::import_tar_file(&tar, &name).await?;
            println!(" Imported {} into cache entry {}", tar, name);
        }
        Commands::Login {
            registry,
            username,
            password,
        } => {
            state::store_credentials(&registry, &username, &password).await?;
            println!(" Stored credentials for {}", registry);
        }
    }

    Ok(())
}
