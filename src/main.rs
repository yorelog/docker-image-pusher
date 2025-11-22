use clap::{Parser, Subcommand};
use thiserror::Error;

mod progress_display;
mod push;
mod save;
mod state;
mod tar_import;

use oci_core::client::{Client, ClientConfig};

pub const STATE_DIR: &str = ".docker-image-pusher";
pub const STREAM_BUFFER_SIZE: usize = 8 * 1024 * 1024;
pub const PROGRESS_LAYER_THRESHOLD_BYTES: u64 = 500 * 1024 * 1024;
pub const PROGRESS_UPDATE_INTERVAL_SECS: u64 = 3;
pub const CHUNKED_LAYER_SIZE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_CHUNKED_LAYER_SIZE_BYTES: usize = 256 * 1024 * 1024;
pub const LARGE_LAYER_THRESHOLD_BYTES: u64 = 500 * 1024 * 1024;
pub const LARGE_LAYER_THRESHOLD_MB: f64 = 500.0;
pub const ESTIMATED_SPEED_MBPS: f64 = 10.0;
pub const LARGE_LAYER_PROGRESS_INTERVAL_SECS: u64 = 3;
pub const NORMAL_LAYER_PROGRESS_INTERVAL_SECS: u64 = 1;
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
    /// Push a docker-save tarball directly to a registry
    Push {
        /// Docker-save tar path
        #[arg(value_name = "TAR")]
        tar: String,
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
        /// Override the chunk size (in MiB) used for large layer uploads
        #[arg(long = "blob-chunk", value_name = "MB")]
        blob_chunk: Option<usize>,
    },
    /// Save local container images to tar archives
    Save(save::SaveArgs),
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
        Commands::Push {
            tar,
            target,
            username,
            password,
            registry,
            blob_chunk,
        } => {
            push::run_push(
                &client, &tar, target, username, password, registry, blob_chunk,
            )
            .await?;
        }
        Commands::Save(args) => {
            save::run_save(args).await?;
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
