use thiserror::Error;

#[derive(Error, Debug)]
pub enum OciError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("Authentication error: {0}")]
    Auth(String),
    #[error("Reference error: {0}")]
    Reference(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("Upload session reset required: {0}")]
    UploadReset(String),
    #[error("Unexpected status {status}: {message}")]
    Status { status: u16, message: String },
}

impl From<reqwest::Error> for OciError {
    fn from(value: reqwest::Error) -> Self {
        OciError::Network(value.to_string())
    }
}

impl From<std::io::Error> for OciError {
    fn from(value: std::io::Error) -> Self {
        OciError::Io(value.to_string())
    }
}
