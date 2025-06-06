//! Optimized upload implementation for large files

use crate::error::{Result, PusherError};
use crate::error::handlers::NetworkErrorHandler;
use crate::output::OutputManager;
use crate::digest::DigestUtils;
use reqwest::{Client, header::CONTENT_TYPE};
use std::time::Duration;
use tokio::time::sleep;

pub struct ChunkedUploader {
    client: Client,
    max_retries: usize,
    retry_delay: Duration,
    timeout: Duration,
    output: OutputManager,
}

impl ChunkedUploader {
    pub fn new(timeout_seconds: u64, output: OutputManager) -> Self {
        // Build HTTP client with optimized settings for large uploads
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .connect_timeout(Duration::from_secs(60))
            .read_timeout(Duration::from_secs(3600))
            .pool_idle_timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(10)
            .user_agent("docker-image-pusher/1.0")
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            max_retries: 3,
            retry_delay: Duration::from_secs(10),
            timeout: Duration::from_secs(timeout_seconds),
            output,
        }
    }

    /// Upload a blob using direct upload (recommended for Docker registries)
    pub async fn upload_large_blob(
        &self,
        upload_url: &str,
        data: &[u8],
        expected_digest: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.output.detail(&format!("Starting upload for {} bytes", data.len()));
        
        // Use direct upload - most reliable method for Docker registries
        self.upload_direct(upload_url, data, expected_digest, token).await
    }

    async fn upload_direct(
        &self,
        upload_url: &str,
        data: &[u8],
        expected_digest: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.output.detail(&format!("Using direct upload for {} bytes", data.len()));
        
        for attempt in 1..=self.max_retries {
            match self.try_upload(upload_url, data, expected_digest, token).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if attempt < self.max_retries {
                        self.output.warning(&format!(
                            "Upload attempt {}/{} failed: {}. Retrying in {}s...",
                            attempt,
                            self.max_retries,
                            e,
                            self.retry_delay.as_secs()
                        ));
                        sleep(self.retry_delay).await;
                    } else {
                        self.output.error(&format!("All {} upload attempts failed", self.max_retries));
                        return Err(e);
                    }
                }
            }
        }
        
        unreachable!()
    }

    async fn try_upload(
        &self,
        upload_url: &str,
        data: &[u8],
        digest: &str,
        token: &Option<String>,
    ) -> Result<()> {
        let data_size = data.len() as u64;
        
        // Normalize and validate digest using DigestUtils
        let normalized_digest = DigestUtils::normalize_digest(digest)?;
        
        // Fix URL construction - ensure proper format for Harbor registry
        let url = if upload_url.contains('?') {
            format!("{}&digest={}", upload_url, normalized_digest)
        } else {
            format!("{}?digest={}", upload_url, normalized_digest)
        };
        
        // Show more of the URL for debugging
        let display_url = if url.len() > 100 {
            format!("{}...{}", &url[..50], &url[url.len()-30..])
        } else {
            url.clone()
        };
        
        self.output.detail(&format!("Upload URL: {}", display_url));
        self.output.detail(&format!("Upload size: {}", self.output.format_size(data_size)));
        self.output.detail(&format!("Expected digest: {}", normalized_digest));
        
        // Verify data integrity before upload using DigestUtils
        DigestUtils::verify_data_integrity(data, &normalized_digest)?;
        self.output.detail(&format!("✅ Data integrity verified: SHA256 digest matches"));
        
        let mut request = self.client
            .put(&url)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header("Content-Length", data_size.to_string())
            .timeout(self.timeout)
            .body(data.to_vec());
        
        if let Some(token) = token {
            request = request.bearer_auth(token);
            self.output.detail("Using authentication token");
        } else {
            self.output.detail("No authentication token");
        }
        
        let start_time = std::time::Instant::now();
        self.output.progress(&format!("Uploading {}", self.output.format_size(data_size)));
        
        let response = request.send().await
            .map_err(|e| {
                self.output.error(&format!("Network error during upload: {}", e));
                NetworkErrorHandler::handle_network_error(&e, "upload")
            })?;
        
        let elapsed = start_time.elapsed();
        let speed = if elapsed.as_secs() > 0 {
            data_size / elapsed.as_secs()
        } else {
            data_size
        };
        
        self.output.progress_done();
        self.output.info(&format!("Upload completed in {} (avg speed: {})", 
                 self.output.format_duration(elapsed), self.output.format_speed(speed)));
        
        if response.status().is_success() || response.status().as_u16() == 201 {
            self.output.success("Upload successful");
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            
            let error_msg = match status.as_u16() {
                400 => {
                    if error_text.contains("exist blob require digest") {
                        format!("Digest validation failed - Registry reports blob exists but digest mismatch: {}", error_text)
                    } else if error_text.contains("BAD_REQUEST") {
                        format!("Bad request - Check digest format and data integrity: {}", error_text)
                    } else {
                        format!("Bad request: {}", error_text)
                    }
                },
                401 => format!("Authentication failed: {}", error_text),
                403 => format!("Permission denied: {}", error_text),
                404 => format!("Repository not found or upload session expired: {}", error_text),
                409 => format!("Conflict - Blob already exists with different digest: {}", error_text),
                413 => format!("File too large: {}", error_text),
                422 => format!("Invalid digest or data: {}", error_text),
                500 => format!("Registry server error: {}", error_text),
                502 | 503 => format!("Registry unavailable: {}", error_text),
                507 => format!("Registry out of storage: {}", error_text),
                _ => format!("Upload failed (status {}): {}", status, error_text)
            };
            
            self.output.error(&error_msg);
            Err(PusherError::Upload(error_msg))
        }
    }

}