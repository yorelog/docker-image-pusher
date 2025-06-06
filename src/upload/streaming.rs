//! Streaming upload implementation for very large files

use crate::error::{Result, PusherError};
use crate::error::handlers::NetworkErrorHandler;
use crate::output::OutputManager;
use reqwest::{Client, header::CONTENT_TYPE, Body};
use std::path::Path;
use std::io::SeekFrom;
use std::time::Duration;
use tokio::time::sleep;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

pub struct StreamingUploader {
    client: Client,
    max_retries: usize,
    retry_delay: Duration,
    timeout: Duration,
    output: OutputManager,
}

impl StreamingUploader {
    pub fn new(
        client: Client,
        max_retries: usize,
        timeout_seconds: u64,
        output: OutputManager,
    ) -> Self {
        Self {
            client,
            max_retries,
            retry_delay: Duration::from_secs(10),
            timeout: Duration::from_secs(timeout_seconds),
            output,
        }
    }

    pub async fn upload_from_tar_entry<P>(
        &self,
        tar_path: &Path,
        _entry_path: &str,
        entry_offset: u64,
        entry_size: u64,
        upload_url: &str,
        digest: &str,
        token: &Option<String>,
        progress_callback: P,
    ) -> Result<()>
    where
        P: Fn(u64, u64) + Send + Sync + 'static,
    {
        self.output.step(&format!("Starting streaming upload for {} ({})", 
                 &digest[..16], self.output.format_size(entry_size)));

        for attempt in 1..=self.max_retries {
            self.output.detail(&format!("Streaming upload attempt {} of {}", attempt, self.max_retries));
            
            match self.try_streaming_upload(
                tar_path, 
                entry_offset, 
                entry_size, 
                upload_url, 
                digest, 
                token, 
                &progress_callback
            ).await {
                Ok(_) => {
                    progress_callback(entry_size, entry_size);
                    self.output.success(&format!("Streaming upload completed successfully on attempt {}", attempt));
                    return Ok(());
                }
                Err(e) if attempt < self.max_retries => {
                    self.output.warning(&format!("Streaming attempt {} failed: {}", attempt, e));
                    self.output.info(&format!("Waiting {}s before retry...", self.retry_delay.as_secs()));
                    sleep(self.retry_delay).await;
                }
                Err(e) => {
                    self.output.error(&format!("All {} streaming attempts failed. Last error: {}", self.max_retries, e));
                    return Err(e);
                }
            }
        }
        
        Err(PusherError::Upload("All streaming upload attempts failed".to_string()))
    }

    async fn try_streaming_upload<P>(
        &self,
        tar_path: &Path,
        entry_offset: u64,
        entry_size: u64,
        upload_url: &str,
        digest: &str,
        token: &Option<String>,
        progress_callback: &P,
    ) -> Result<()>
    where
        P: Fn(u64, u64) + Send + Sync + 'static,
    {
        // Fix URL construction to match the chunked uploader
        let url = if upload_url.contains('?') {
            format!("{}&digest={}", upload_url, digest)
        } else {
            format!("{}?digest={}", upload_url, digest)
        };

        // Open async file and seek to the correct position
        let mut async_file = tokio::fs::File::open(tar_path).await
            .map_err(|e| PusherError::Io(e.to_string()))?;
        
        async_file.seek(SeekFrom::Start(entry_offset)).await
            .map_err(|e| PusherError::Io(e.to_string()))?;
        
        // Create a limited async reader
        let limited_async_reader = async_file.take(entry_size);
        let stream = ReaderStream::new(limited_async_reader);
        let body = Body::wrap_stream(stream);

        let mut request = self.client
            .put(&url)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header("Content-Length", entry_size.to_string())
            .timeout(self.timeout)
            .body(body);

        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        progress_callback(0, entry_size);
        
        self.output.progress(&format!("Streaming {}", self.output.format_size(entry_size)));
        let start_time = std::time::Instant::now();

        let response = request.send().await            .map_err(|e| {
                self.output.error(&format!("Network error during streaming upload: {}", e));
                NetworkErrorHandler::handle_network_error(&e, "streaming upload")
            })?;

        let elapsed = start_time.elapsed();
        let speed = if elapsed.as_secs() > 0 {
            entry_size / elapsed.as_secs()
        } else {
            entry_size
        };

        self.output.progress_done();
        self.output.info(&format!("Streaming completed in {} (avg speed: {})", 
                 self.output.format_duration(elapsed), self.output.format_size(speed)));

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            
            let error_msg = match status.as_u16() {
                413 => "File too large for registry".to_string(),
                507 => "Insufficient storage space on registry".to_string(),
                401 => "Authentication failed during upload".to_string(),
                403 => "Permission denied for upload".to_string(),
                408 | 504 => "Streaming upload timeout".to_string(),
                _ => format!("Streaming upload failed (status {}): {}", status, error_text)
            };
            
            self.output.error(&error_msg);
            Err(PusherError::Upload(error_msg))
        }
    }
}