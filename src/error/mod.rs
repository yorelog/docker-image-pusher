//! Error types and handlers for registry operations

pub mod handlers;

use std::fmt;

pub type Result<T> = std::result::Result<T, RegistryError>;

#[derive(Debug, Clone)]
pub enum RegistryError {
    /// Network related errors
    Network(String),
    /// Registry related errors
    Registry(String),
    /// Authentication errors
    Auth(String),
    /// File IO errors
    Io(String),
    /// Parse errors
    Parse(String),
    /// Image parsing errors
    ImageParsing(String),
    /// Upload errors
    Upload(String),
    /// HTTP/Request errors
    Http(String),
    /// Validation errors
    Validation(String),
    /// Cache errors
    Cache {
        message: String,
        path: Option<std::path::PathBuf>,
    },
    /// Resource not found
    NotFound(String),
    /// Feature not implemented
    NotImplemented(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::Network(msg) => write!(f, "Network error: {}", msg),
            RegistryError::Registry(msg) => write!(f, "Registry error: {}", msg),
            RegistryError::Auth(msg) => write!(f, "Authentication error: {}", msg),
            RegistryError::Io(msg) => write!(f, "IO error: {}", msg),
            RegistryError::Parse(msg) => write!(f, "Parse error: {}", msg),
            RegistryError::ImageParsing(msg) => write!(f, "Image parsing error: {}", msg),
            RegistryError::Upload(msg) => write!(f, "Upload error: {}", msg),
            RegistryError::Http(msg) => write!(f, "HTTP error: {}", msg),
            RegistryError::Validation(msg) => write!(f, "Validation error: {}", msg),
            RegistryError::Cache { message, path } => {
                if let Some(path) = path {
                    write!(f, "Cache error at {}: {}", path.display(), message)
                } else {
                    write!(f, "Cache error: {}", message)
                }
            }
            RegistryError::NotFound(msg) => write!(f, "Not found: {}", msg),
            RegistryError::NotImplemented(msg) => write!(f, "Not implemented: {}", msg),
        }
    }
}

impl std::error::Error for RegistryError {}

impl From<std::io::Error> for RegistryError {
    fn from(err: std::io::Error) -> Self {
        RegistryError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for RegistryError {
    fn from(err: serde_json::Error) -> Self {
        RegistryError::Parse(err.to_string())
    }
}

impl From<reqwest::Error> for RegistryError {
    fn from(err: reqwest::Error) -> Self {
        RegistryError::Network(err.to_string())
    }
}

impl From<url::ParseError> for RegistryError {
    fn from(err: url::ParseError) -> Self {
        RegistryError::Validation(err.to_string())
    }
}

impl From<std::string::FromUtf8Error> for RegistryError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        RegistryError::Parse(format!("UTF-8 conversion error: {}", err))
    }
}

impl From<crate::concurrency::ConcurrencyError> for RegistryError {
    fn from(err: crate::concurrency::ConcurrencyError) -> Self {
        RegistryError::Registry(format!("Concurrency error: {}", err))
    }
}
