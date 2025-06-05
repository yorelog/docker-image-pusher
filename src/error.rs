//! Error handling module for the Docker image pusher

use std::fmt;

#[derive(Debug)]
pub enum PusherError {
    Authentication(String),
    Registry(String),
    ImageParsing(String),
    Upload(String),
    Configuration(String),
    Io(std::io::Error),
    Network(reqwest::Error),
    Serialization(serde_json::Error),
}

impl fmt::Display for PusherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PusherError::Authentication(msg) => write!(f, "Authentication error: {}", msg),
            PusherError::Registry(msg) => write!(f, "Registry error: {}", msg),
            PusherError::ImageParsing(msg) => write!(f, "Image parsing error: {}", msg),
            PusherError::Upload(msg) => write!(f, "Upload error: {}", msg),
            PusherError::Configuration(msg) => write!(f, "Configuration error: {}", msg),
            PusherError::Io(err) => write!(f, "IO error: {}", err),
            PusherError::Network(err) => write!(f, "Network error: {}", err),
            PusherError::Serialization(err) => write!(f, "Serialization error: {}", err),
        }
    }
}

impl std::error::Error for PusherError {}

impl From<std::io::Error> for PusherError {
    fn from(err: std::io::Error) -> Self {
        PusherError::Io(err)
    }
}

impl From<reqwest::Error> for PusherError {
    fn from(err: reqwest::Error) -> Self {
        PusherError::Network(err)
    }
}

impl From<serde_json::Error> for PusherError {
    fn from(err: serde_json::Error) -> Self {
        PusherError::Serialization(err)
    }
}

impl From<anyhow::Error> for PusherError {
    fn from(err: anyhow::Error) -> Self {
        PusherError::Configuration(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PusherError>;