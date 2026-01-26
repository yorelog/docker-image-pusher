use clap::{Parser, Subcommand};
use thiserror::Error;

mod common;
mod import;
mod push;

use oci_core::client::{Client, ClientConfig};

pub const STATE_DIR: &str = ".docker-image-pusher";
pub const STREAM_BUFFER_SIZE: usize = 8 * 1024 * 1024;
pub const PROGRESS_LAYER_THRESHOLD_BYTES: u64 = 500 * 1024 * 1024;
pub const PROGRESS_UPDATE_INTERVAL_SECS: u64 = 3;
pub const CHUNKED_LAYER_SIZE_BYTES: usize = 10 * 1024 * 1024;
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
    /// Skip TLS certificate verification (for self-signed certificates)
    #[arg(long, global = true)]
    insecure: bool,
    
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Push from a docker-save tarball or directly from a containerd store
    Push {
        /// Docker-save tar path (use this to push from a tarball)
        #[arg(long, value_name = "TAR")]
        tar: Option<String>,
        /// Containerd root directory (defaults to ~/.local/share/containerd)
        #[arg(long, value_name = "DIR")]
        root: Option<String>,
        /// Namespace in containerd (e.g., "default")
        #[arg(long, value_name = "NS", default_value = "default")]
        namespace: String,
        /// Image reference when pushing from containerd (e.g., "busybox:latest")
        #[arg(long, value_name = "IMAGE")]
        image: Option<String>,
        /// Override the destination reference (defaults to the source image)
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
    /// Save images from a containerd store into a portable folder or tarball
    Save {
        /// Containerd root directory (defaults to ~/.local/share/containerd)
        #[arg(long, value_name = "DIR")]
        root: Option<String>,
        /// Namespace in containerd (e.g., "default")
        #[arg(long, value_name = "NS", default_value = "default")]
        namespace: String,
        /// Image reference(s) (e.g., "busybox:latest")
        #[arg(value_name = "IMAGE", num_args = 1..)]
        images: Vec<String>,
        /// Output directory (or tar path if you tar it yourself afterwards)
        #[arg(long, value_name = "OUT")]
        out: String,
        /// Optional manifest digest to bypass metadata lookup (e.g., sha256:abcd...)
        #[arg(long, value_name = "DIGEST")]
        digest: Option<String>,
    },
    /// List images recorded in containerd metadata.db
    ListContainerd {
        /// Containerd root directory (defaults to ~/.local/share containerd)
        #[arg(long, value_name = "DIR")]
        root: Option<String>,
        /// Namespace in containerd (e.g., "default")
        #[arg(long, value_name = "NS", default_value = "default")]
        namespace: String,
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
    let client = Client::new(ClientConfig {
        user_agent: None,
        insecure_skip_tls_verify: cli.insecure,
    });

    match cli.command {
        Commands::Push {
            tar,
            root,
            namespace,
            image,
            target,
            registry,
            username,
            password,
            blob_chunk,
        } => {
            if let Some(tar_path) = tar {
                push::run_push(
                    &client, &tar_path, target, username, password, registry, blob_chunk,
                )
                .await?;
            } else {
                let image = image.ok_or_else(|| {
                    PusherError::push_error("--image is required when pushing from containerd")
                })?;
                import::containerd::run_push_containerd(
                    &client,
                    root.as_deref(),
                    &namespace,
                    &image,
                    target,
                    username,
                    password,
                    registry,
                    blob_chunk,
                )
                .await?;
            }
        }
        Commands::Save {
            root,
            namespace,
            images,
            out,
            digest,
        } => {
            import::containerd::export_images(
                root.as_deref(),
                &namespace,
                &images,
                &out,
                digest.as_deref(),
            )
            .await?;
        }
        Commands::ListContainerd { root, namespace } => {
            import::containerd::list_images(root.as_deref(), &namespace).await?;
        }
        Commands::Login {
            registry,
            username,
            password,
        } => {
            common::state::store_credentials(&registry, &username, &password).await?;
            println!(" Stored credentials for {}", registry);
        }
    }

    Ok(())
}
