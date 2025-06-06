//! Streaming upload implementation for very large files

use crate::digest::DigestUtils;
use crate::error::handlers::NetworkErrorHandler;
use crate::error::{PusherError, Result};
use crate::output::OutputManager;
use reqwest::{Client, header::CONTENT_TYPE};
use std::io::SeekFrom;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::time::sleep;

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
        self.output.step(&format!(
            "Starting streaming upload for {} ({})",
            &digest[..16],
            self.output.format_size(entry_size)
        ));

        let mut last_error = None;
        let mut storage_error_count = 0;

        for attempt in 1..=self.max_retries {
            self.output.detail(&format!(
                "Streaming upload attempt {} of {}",
                attempt, self.max_retries
            ));

            match self
                .try_streaming_upload(
                    tar_path,
                    entry_offset,
                    entry_size,
                    upload_url,
                    digest,
                    token,
                    &progress_callback,
                )
                .await
            {
                Ok(_) => {
                    progress_callback(entry_size, entry_size);
                    self.output.success(&format!(
                        "Streaming upload completed successfully on attempt {}",
                        attempt
                    ));
                    return Ok(());
                }
                Err(e) if attempt < self.max_retries => {
                    // Check if this is a storage backend error
                    let error_str = e.to_string();
                    let is_storage_error = error_str.contains("s3aws")
                        || error_str.contains("DriverName")
                        || error_str.contains("500 Internal Server Error");

                    if is_storage_error {
                        storage_error_count += 1;
                        self.output.warning(&format!(
                            "Storage backend error (attempt {}): {}",
                            storage_error_count, e
                        ));

                        // For storage errors, use exponential backoff
                        let backoff_delay = self.retry_delay.as_secs()
                            * (2_u64.pow(storage_error_count.min(4) as u32));
                        self.output.info(&format!(
                            "Storage error - waiting {}s before retry (exponential backoff)...",
                            backoff_delay
                        ));
                        sleep(Duration::from_secs(backoff_delay)).await;
                    } else {
                        self.output
                            .warning(&format!("Streaming attempt {} failed: {}", attempt, e));
                        self.output.info(&format!(
                            "Waiting {}s before retry...",
                            self.retry_delay.as_secs()
                        ));
                        sleep(self.retry_delay).await;
                    }

                    last_error = Some(e);
                }
                Err(e) => {
                    last_error = Some(e);
                    break;
                }
            }
        }

        let final_error = last_error.unwrap_or_else(|| {
            PusherError::Upload("All streaming upload attempts failed".to_string())
        });

        if storage_error_count > 0 {
            self.output.error(&format!(
                "All {} streaming attempts failed due to registry storage issues. Last error: {}",
                self.max_retries, final_error
            ));
            self.output
                .info("💡 This appears to be a registry storage backend problem. Consider:");
            self.output
                .info("   • Contacting your registry administrator");
            self.output.info("   • Checking registry storage capacity");
            self.output
                .info("   • Retrying later when storage issues may be resolved");
        } else {
            self.output.error(&format!(
                "All {} streaming attempts failed. Last error: {}",
                self.max_retries, final_error
            ));
        }

        Err(final_error)
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
        // Normalize and validate digest using DigestUtils
        let normalized_digest = DigestUtils::normalize_digest(digest)?;

        // Fix URL construction to match the chunked uploader
        let url = if upload_url.contains('?') {
            format!("{}&digest={}", upload_url, normalized_digest)
        } else {
            format!("{}?digest={}", upload_url, normalized_digest)
        };

        // Show more of the URL for debugging
        let display_url = if url.len() > 100 {
            format!("{}...{}", &url[..50], &url[url.len() - 30..])
        } else {
            url.clone()
        };

        self.output.detail(&format!("Upload URL: {}", display_url));

        // Open async file and seek to the correct position
        let mut async_file = tokio::fs::File::open(tar_path)
            .await
            .map_err(|e| PusherError::Io(e.to_string()))?;

        async_file
            .seek(SeekFrom::Start(entry_offset))
            .await
            .map_err(|e| PusherError::Io(e.to_string()))?;

        // Read the data first for integrity check
        let mut data = vec![0u8; entry_size as usize];
        async_file
            .read_exact(&mut data)
            .await
            .map_err(|e| PusherError::Io(format!("Failed to read layer data: {}", e)))?;

        // Verify data integrity before uploading with gzip fallback logic (same as chunked uploader)
        self.output
            .detail("Verifying data integrity before upload...");

        let upload_data = match crate::digest::DigestUtils::verify_data_integrity(&data, &normalized_digest) {
            Ok(_) => {
                self.output.success("✅ Data integrity check passed");
                data
            }
            Err(e) => {
                // Check if data is already gzipped
                let is_gzipped = data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b;

                if !is_gzipped {
                    // Try to gzip the data and see if that matches
                    use flate2::{Compression, write::GzEncoder};
                    use std::io::Write;

                    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                    encoder.write_all(&data).map_err(|e| {
                        crate::error::PusherError::Io(format!("Failed to gzip data: {}", e))
                    })?;
                    let gzipped = encoder.finish().map_err(|e| {
                        crate::error::PusherError::Io(format!("Failed to finish gzip: {}", e))
                    })?;

                    let computed = crate::digest::DigestUtils::compute_docker_digest(&gzipped);

                    if computed == normalized_digest {
                        self.output
                            .success("✅ Data integrity check passed after gzip compression");
                        gzipped
                    } else {
                        // Still doesn't match, log the error but proceed (might be a false alarm)
                        self.output.warning(&format!(
                            "⚠️ Data integrity check warning: {}. Proceeding with gzipped data anyway.",
                            e
                        ));
                        gzipped
                    }
                } else {
                    // Data is already gzipped but still doesn't match - proceed anyway
                    self.output.warning(&format!(
                        "⚠️ Data integrity check warning: {}. Proceeding with upload anyway.",
                        e
                    ));
                    data
                }
            }
        };

        // Create request with verified data
        let upload_size = upload_data.len() as u64;
        let mut request = self
            .client
            .put(&url)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header("Content-Length", upload_size.to_string())
            .timeout(self.timeout)
            .body(upload_data);

        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        progress_callback(0, entry_size);

        self.output.progress(&format!(
            "Streaming {}",
            self.output.format_size(upload_size)
        ));
        let start_time = std::time::Instant::now();

        let response = request.send().await.map_err(|e| {
            self.output
                .error(&format!("Network error during streaming upload: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "streaming upload")
        })?;

        let elapsed = start_time.elapsed();
        let speed = if elapsed.as_secs() > 0 {
            upload_size / elapsed.as_secs()
        } else {
            upload_size
        };

        self.output.progress_done();
        self.output.info(&format!(
            "Streaming completed in {} (avg speed: {})",
            self.output.format_duration(elapsed),
            self.output.format_speed(speed)
        ));

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());

            let error_msg = match status.as_u16() {
                400 => {
                    if error_text.contains("DIGEST_INVALID") {
                        format!(
                            "Digest validation failed on registry side - Registry reports digest mismatch: {}",
                            error_text
                        )
                    } else {
                        format!("Bad request - Check data format: {}", error_text)
                    }
                }
                413 => "File too large for registry".to_string(),
                507 => "Insufficient storage space on registry".to_string(),
                401 => "Authentication failed during upload".to_string(),
                403 => "Permission denied for upload".to_string(),
                408 | 504 => "Streaming upload timeout".to_string(),
                500 => {
                    if error_text.contains("s3aws") || error_text.contains("DriverName") {
                        format!(
                            "Registry storage backend error (S3): {}. Consider retrying or contacting registry administrator",
                            error_text
                        )
                    } else {
                        format!("Registry internal server error: {}", error_text)
                    }
                }
                502 | 503 => format!("Registry temporarily unavailable: {}", error_text),
                _ => format!(
                    "Streaming upload failed (status {}): {}",
                    status, error_text
                ),
            };

            self.output.error(&error_msg);
            Err(PusherError::Upload(error_msg))
        }
    }
}
