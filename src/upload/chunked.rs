//! Optimized upload implementation for large files

use crate::error::{Result, PusherError};
use crate::output::OutputManager;
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

    pub async fn upload_large_blob(
        &self,
        upload_url: &str,
        data: &[u8],
        digest: &str,
        token: &Option<String>,
    ) -> Result<()> {
        let data_size = data.len() as u64;
        
        self.output.step(&format!("Starting chunked upload for {} ({})", 
            &digest[..16], self.output.format_size(data_size)));
        
        for attempt in 1..=self.max_retries {
            self.output.detail(&format!("Chunked upload attempt {} of {}", attempt, self.max_retries));
            
            match self.try_upload(upload_url, data, digest, token).await {
                Ok(_) => {
                    self.output.success(&format!("Chunked upload completed successfully on attempt {}", attempt));
                    return Ok(());
                }
                Err(e) if attempt < self.max_retries => {
                    self.output.warning(&format!("Chunked attempt {} failed: {}", attempt, e));
                    self.output.info(&format!("Waiting {}s before retry...", self.retry_delay.as_secs()));
                    sleep(self.retry_delay).await;
                }
                Err(e) => {
                    self.output.error(&format!("All {} chunked attempts failed. Last error: {}", self.max_retries, e));
                    return Err(e);
                }
            }
        }
        
        Err(PusherError::Upload("All chunked upload attempts failed".to_string()))
    }

    async fn try_upload(
        &self,
        upload_url: &str,
        data: &[u8],
        digest: &str,
        token: &Option<String>,
    ) -> Result<()> {
        let data_size = data.len() as u64;
        
        let url = format!("{}digest={}", 
            if upload_url.contains('?') { format!("{}&", upload_url) } else { format!("{}?", upload_url) },
            digest
        );
        
        self.output.detail(&format!("Upload URL: {}", url));
        self.output.detail(&format!("Upload size: {}", self.output.format_size(data_size)));
        
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
                self.output.error(&format!("Network error during chunked upload: {}", e));
                
                if e.is_timeout() {
                    PusherError::Upload(format!("Chunked upload timeout after {}s", self.timeout.as_secs()))
                } else if e.is_connect() {
                    PusherError::Network(e.to_string())
                } else if e.to_string().contains("dns") {
                    PusherError::Network(e.to_string())
                } else if e.to_string().contains("certificate") {
                    PusherError::Network(e.to_string())
                } else {
                    PusherError::Network(e.to_string())
                }
            })?;
        
        let elapsed = start_time.elapsed();
        let speed = if elapsed.as_secs() > 0 {
            data_size / elapsed.as_secs()
        } else {
            data_size
        };
        
        self.output.progress_done();
        self.output.info(&format!("Upload completed in {} (avg speed: {})", 
                 self.output.format_duration(elapsed), self.output.format_size(speed)));
        
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            
            let error_msg = match status.as_u16() {
                400 => format!("Bad request: {}", error_text),
                401 => format!("Authentication failed: {}", error_text),
                403 => format!("Permission denied: {}", error_text),
                404 => format!("Repository not found: {}", error_text),
                413 => format!("File too large: {}", error_text),
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