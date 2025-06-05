//! Error handling for the docker image pusher

use std::fmt;

#[derive(Debug)]
pub enum PusherError {
    Config(String),
    Authentication(String),
    Network(String),
    Upload(String),
    Io(String),
    Parse(String),
    Registry(String),
    ImageParsing(String),
    Validation(String),
    Timeout(String),
}

impl Clone for PusherError {
    fn clone(&self) -> Self {
        match self {
            PusherError::Config(msg) => PusherError::Config(msg.clone()),
            PusherError::Authentication(msg) => PusherError::Authentication(msg.clone()),
            PusherError::Network(msg) => PusherError::Network(msg.clone()),
            PusherError::Upload(msg) => PusherError::Upload(msg.clone()),
            PusherError::Io(msg) => PusherError::Io(msg.clone()),
            PusherError::Parse(msg) => PusherError::Parse(msg.clone()),
            PusherError::Registry(msg) => PusherError::Registry(msg.clone()),
            PusherError::ImageParsing(msg) => PusherError::ImageParsing(msg.clone()),
            PusherError::Validation(msg) => PusherError::Validation(msg.clone()),
            PusherError::Timeout(msg) => PusherError::Timeout(msg.clone()),
        }
    }
}

impl fmt::Display for PusherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PusherError::Config(msg) => write!(f, "Configuration error: {}", msg),
            PusherError::Authentication(msg) => write!(f, "Authentication failed: {}", msg),
            PusherError::Network(msg) => write!(f, "Network error: {}", msg),
            PusherError::Upload(msg) => write!(f, "Upload failed: {}", msg),
            PusherError::Io(msg) => write!(f, "I/O error: {}", msg),
            PusherError::Parse(msg) => write!(f, "Parse error: {}", msg),
            PusherError::Registry(msg) => write!(f, "Registry error: {}", msg),
            PusherError::ImageParsing(msg) => write!(f, "Image parsing failed: {}", msg),
            PusherError::Validation(msg) => write!(f, "Validation error: {}", msg),
            PusherError::Timeout(msg) => write!(f, "Operation timed out: {}", msg),
        }
    }
}

impl std::error::Error for PusherError {}

// Enhanced From implementations with context
impl From<std::io::Error> for PusherError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => PusherError::Io(format!("File not found: {}", err)),
            std::io::ErrorKind::PermissionDenied => PusherError::Io(format!("Permission denied: {}", err)),
            std::io::ErrorKind::TimedOut => PusherError::Timeout(format!("I/O operation timed out: {}", err)),
            _ => PusherError::Io(err.to_string()),
        }
    }
}

impl From<reqwest::Error> for PusherError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            PusherError::Timeout(format!("Network request timed out: {}", err))
        } else if err.is_connect() {
            PusherError::Network(format!("Connection failed: {}", err))
        } else if err.is_decode() {
            PusherError::Parse(format!("Response decode error: {}", err))
        } else {
            PusherError::Network(err.to_string())
        }
    }
}

impl From<serde_json::Error> for PusherError {
    fn from(err: serde_json::Error) -> Self {
        PusherError::Parse(format!("JSON parsing failed: {}", err))
    }
}

impl From<url::ParseError> for PusherError {
    fn from(err: url::ParseError) -> Self {
        PusherError::Config(format!("Invalid URL format: {}", err))
    }
}

pub type Result<T> = std::result::Result<T, PusherError>;

// Utility function for creating contextual errors
pub fn context_error<T>(result: std::result::Result<T, PusherError>, context: &str) -> Result<T> {
    result.map_err(|e| match e {
        PusherError::Config(msg) => PusherError::Config(format!("{}: {}", context, msg)),
        PusherError::Authentication(msg) => PusherError::Authentication(format!("{}: {}", context, msg)),
        PusherError::Network(msg) => PusherError::Network(format!("{}: {}", context, msg)),
        PusherError::Upload(msg) => PusherError::Upload(format!("{}: {}", context, msg)),
        PusherError::Io(msg) => PusherError::Io(format!("{}: {}", context, msg)),
        PusherError::Parse(msg) => PusherError::Parse(format!("{}: {}", context, msg)),
        PusherError::Registry(msg) => PusherError::Registry(format!("{}: {}", context, msg)),
        PusherError::ImageParsing(msg) => PusherError::ImageParsing(format!("{}: {}", context, msg)),
        PusherError::Validation(msg) => PusherError::Validation(format!("{}: {}", context, msg)),
        PusherError::Timeout(msg) => PusherError::Timeout(format!("{}: {}", context, msg)),
    })
}