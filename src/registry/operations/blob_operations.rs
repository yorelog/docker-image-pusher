//! Blob operations for registry client
//!
//! Implements Docker Registry v2 blob operations:
//! - Blob existence checks (HEAD /v2/{name}/blobs/{digest})  
//! - Blob upload with staged upload pattern (/v2/{name}/blobs/uploads/)
//! - Blob download (GET /v2/{name}/blobs/{digest})
//! - OCI-compatible blob handling

use crate::error::{RegistryError, Result};
use crate::error::handlers::NetworkErrorHandler;
use crate::logging::Logger;
use crate::registry::token_manager::TokenManager;
use reqwest::Client;

#[derive(Clone)]
pub struct BlobOperations {
    client: Client,
    address: String,
    output: Logger,
    token_manager: Option<TokenManager>,
}

impl BlobOperations {
    pub fn new(
        client: Client, 
        address: String, 
        output: Logger,
        token_manager: Option<TokenManager>
    ) -> Self {
        Self {
            client,
            address,
            output,
            token_manager,
        }
    }

    pub fn with_token_manager(mut self, token_manager: Option<TokenManager>) -> Self {
        self.token_manager = token_manager;
        self
    }

    /// Check if blob exists using Docker Registry v2 HEAD request
    pub async fn check_blob_exists(&self, digest: &str, repository: &str) -> Result<bool> {
        self.check_blob_exists_with_token(digest, repository, &None).await
    }

    /// Check blob existence with authentication token
    pub async fn check_blob_exists_with_token(
        &self,
        digest: &str,
        repository: &str,
        token: &Option<String>,
    ) -> Result<bool> {
        // Ensure digest has proper sha256: prefix
        let normalized_digest = if digest.starts_with("sha256:") {
            digest.to_string()
        } else {
            format!("sha256:{}", digest)
        };

        let url = format!(
            "{}/v2/{}/blobs/{}",
            self.address, repository, normalized_digest
        );

        self.output.detail(&format!(
            "Checking blob existence in remote registry: {}",
            &normalized_digest[..23]
        ));

        // Use token manager for automatic retry if available
        if let Some(ref token_manager) = self.token_manager {
            let _repository_clone = repository.to_string();
            let url_clone = url.clone();
            let normalized_digest_clone = normalized_digest.clone();
            let output_clone = self.output.clone();
            let client_clone = self.client.clone();

            return token_manager.execute_with_retry(|token| {
                let url = url_clone.clone();
                let normalized_digest = normalized_digest_clone.clone();
                let output = output_clone.clone();
                let client = client_clone.clone();

                Box::pin(async move {
                    // Use HEAD request to check existence without downloading
                    let mut request = client.head(&url);

                    // Add authentication if token is provided
                    if let Some(token) = token {
                        request = request.bearer_auth(token);
                    }

                    let response = request.send().await.map_err(|e| {
                        output.warning(&format!("Failed to check blob existence: {}", e));
                        NetworkErrorHandler::handle_network_error(&e, "blob existence check")
                    })?;

                    let status = response.status();

                    match status.as_u16() {
                        200 => {
                            output.detail(&format!("✅ Blob {} exists in registry", &normalized_digest[..16]));
                            Ok(true)
                        }
                        404 => {
                            output.detail(&format!("❌ Blob {} does not exist in registry", &normalized_digest[..16]));
                            Ok(false)
                        }
                        401 => {
                            output.warning(&format!("🔐 Authentication failed for blob check: {} - token may have expired", &normalized_digest[..16]));
                            // Return 401 error for token manager to catch and retry
                            Err(RegistryError::Registry(format!(
                                "401 Unauthorized: Failed to check blob {} - token may have expired",
                                normalized_digest
                            )))
                        }
                        403 => {
                            output.warning(&format!("🚫 Permission denied for blob check: {}", &normalized_digest[..16]));
                            // Assume blob doesn't exist if we can't check permissions
                            Ok(false)
                        }
                        _ => {
                            output.warning(&format!(
                                "⚠️ Unexpected status {} when checking blob existence for {}",
                                status, &normalized_digest[..16]
                            ));
                            // On other errors, assume blob doesn't exist to be safe
                            Ok(false)
                        }
                    }
                })
            }).await;
        }

        // Fallback to direct request without token manager
        let mut request = self.client.head(&url);

        // Add authentication if token is provided
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            self.output
                .warning(&format!("Failed to check blob existence: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "blob existence check")
        })?;

        let status = response.status();

        match status.as_u16() {
            200 => {
                self.output
                    .detail(&format!("✅ Blob {} exists in registry", &normalized_digest[..16]));
                Ok(true)
            }
            404 => {
                self.output
                    .detail(&format!("❌ Blob {} does not exist in registry", &normalized_digest[..16]));
                Ok(false)
            }
            401 => {
                self.output
                    .warning(&format!("🔐 Authentication required for blob check: {}", &normalized_digest[..16]));
                // Return false if we still get 401 even with auth token
                Ok(false)
            }
            403 => {
                self.output.warning(&format!("🚫 Permission denied for blob check: {}", &normalized_digest[..16]));
                // Assume blob doesn't exist if we can't check permissions
                Ok(false)
            }
            _ => {
                self.output.warning(&format!(
                    "⚠️ Unexpected status {} when checking blob existence for {}",
                    status, &normalized_digest[..16]
                ));
                // On other errors, assume blob doesn't exist to be safe
                Ok(false)
            }
        }
    }

    /// Upload blob using Docker Registry v2 staged upload pattern
    pub async fn upload_blob_with_token(
        &self,
        data: &[u8],
        digest: &str,
        repository: &str,
        token: &Option<String>,
    ) -> Result<String> {
        // Ensure digest has proper sha256: prefix
        let normalized_digest = if digest.starts_with("sha256:") {
            digest.to_string()
        } else {
            format!("sha256:{}", digest)
        };

        // Keep a copy for later use
        let normalized_digest_final = normalized_digest.clone();

        self.output.info(&format!(
            "Uploading blob {} ({}) to {}",
            &normalized_digest[..16],
            self.output.format_size(data.len() as u64),
            repository
        ));

        // Check if blob already exists
        if self
            .check_blob_exists_with_token(&normalized_digest, repository, token)
            .await?
        {
            self.output
                .info(&format!("Blob {} already exists, skipping", &normalized_digest[..16]));
            return Ok(normalized_digest);
        }

        // Use staged upload (Docker Registry v2 pattern) - more reliable
        // Step 1: Start upload session with token retry capability
        let upload_url = format!("{}/v2/{}/blobs/uploads/", self.address, repository);
        
        self.output.detail(&format!("Starting blob upload session at: {}", upload_url));
        
        let (response, used_token) = if let Some(ref token_manager) = self.token_manager {
            let upload_url_clone = upload_url.clone();
            let output_clone = self.output.clone();
            let client_clone = self.client.clone();

            let result = token_manager.execute_with_retry(|token| {
                let upload_url = upload_url_clone.clone();
                let output = output_clone.clone();
                let client = client_clone.clone();

                Box::pin(async move {
                    let token_for_request = token.clone();
                    let mut request = client.post(&upload_url);
                    if let Some(token_str) = token_for_request {
                        request = request.bearer_auth(token_str);
                    }

                    let response = request.send().await.map_err(|e| {
                        output.error(&format!("Failed to start upload session: {}", e));
                        RegistryError::Network(e.to_string())
                    })?;
                    
                    let status = response.status();
                    if status == 401 {
                        // Return 401 error for token manager to catch and retry
                        return Err(RegistryError::Registry(format!(
                            "401 Unauthorized: Failed to start upload session - token may have expired"
                        )));
                    }

                    if !status.is_success() {
                        let error_text = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Failed to read error response".to_string());
                        output.error(&format!("Upload session failed with status {}: {}", status, error_text));
                        return Err(RegistryError::Upload(format!(
                            "Failed to start blob upload session (status {}): {}",
                            status, error_text
                        )));
                    }

                    Ok((response, token))
                })
            }).await?;
            result
        } else {
            let mut request = self.client.post(&upload_url);
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }

            let response = request.send().await.map_err(|e| {
                self.output.error(&format!("Failed to start upload session: {}", e));
                RegistryError::Network(e.to_string())
            })?;
            
            if !response.status().is_success() {
                let status = response.status();
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to read error response".to_string());
                self.output.error(&format!("Upload session failed with status {}: {}", status, error_text));
                return Err(RegistryError::Upload(format!(
                    "Failed to start blob upload session (status {}): {}",
                    status, error_text
                )));
            }

            (response, token.clone())
        };

        // Get upload location URL from Location header
        let location = response
            .headers()
            .get("Location")
            .ok_or_else(|| {
                self.output.error("Missing Location header in upload session response");
                RegistryError::Upload("Missing Location header in upload response".to_string())
            })?
            .to_str()
            .map_err(|e| {
                self.output.error(&format!("Invalid Location header: {}", e));
                RegistryError::Upload(format!("Invalid Location header: {}", e))
            })?;

        self.output.detail(&format!("Upload session started, location: {}", location));

        // Ensure location is complete URL
        let full_location = if location.starts_with("http") {
            location.to_string()
        } else {
            // Handle both absolute and relative paths
            if location.starts_with('/') {
                format!("{}{}", self.address, location)
            } else {
                format!("{}/v2/{}/blobs/uploads/{}", self.address, repository, location)
            }
        };

        self.output.detail(&format!("Full upload location: {}", full_location));

        // Step 2: Upload blob data using monolithic upload
        // According to Docker Registry v2 spec, we can do a single PUT with data and digest
        let final_url = format!("{}?digest={}", full_location, normalized_digest);
        
        self.output.detail(&format!("Uploading {} bytes to: {}", data.len(), final_url));
        
        // Use token manager for upload operation as well
        let upload_result = if let Some(ref token_manager) = self.token_manager {
            let final_url_clone = final_url.clone();
            let data_clone = data.to_vec();
            let normalized_digest_clone = normalized_digest.clone();
            let output_clone = self.output.clone();
            let client_clone = self.client.clone();

            token_manager.execute_with_retry(|token| {
                let final_url = final_url_clone.clone();
                let data = data_clone.clone();
                let normalized_digest = normalized_digest_clone.clone();
                let output = output_clone.clone();
                let client = client_clone.clone();

                Box::pin(async move {
                    let mut upload_request = client
                        .put(&final_url)
                        .header("Content-Type", "application/octet-stream")
                        .header("Content-Length", data.len().to_string())
                        .body(data);

                    if let Some(token) = token {
                        upload_request = upload_request.bearer_auth(token);
                    }

                    let upload_response = upload_request.send().await.map_err(|e| {
                        output.error(&format!("Failed to upload blob data: {}", e));
                        RegistryError::Network(e.to_string())
                    })?;

                    let status = upload_response.status();
                    if status == 401 {
                        // Return 401 error for token manager to catch and retry
                        return Err(RegistryError::Registry(format!(
                            "401 Unauthorized: Failed to upload blob - token may have expired"
                        )));
                    }

                    if status.is_success() {
                        output.success(&format!("Blob {} uploaded successfully", &normalized_digest[..16]));
                        // Add a small delay to help with registry consistency
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        Ok(normalized_digest)
                    } else {
                        let error_text = upload_response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Failed to read error response".to_string());
                        
                        output.warning(&format!(
                            "Monolithic upload failed (status {}): {}",
                            status, error_text
                        ));
                        
                        // Check if this is a specific error that should trigger chunked upload
                        if status == 404 && error_text.contains("BLOB_UPLOAD_INVALID") {
                            // Return special error to trigger chunked upload fallback
                            Err(RegistryError::Upload("CHUNKED_FALLBACK".to_string()))
                        } else {
                            Err(RegistryError::Upload(format!(
                                "Blob upload failed (status {}): {}",
                                status, error_text
                            )))
                        }
                    }
                })
            }).await
        } else {
            let mut upload_request = self
                .client
                .put(&final_url)
                .header("Content-Type", "application/octet-stream")
                .header("Content-Length", data.len().to_string())
                .body(data.to_vec());

            if let Some(token_str) = &used_token {
                upload_request = upload_request.bearer_auth(token_str);
            }

            let upload_response = upload_request.send().await.map_err(|e| {
                self.output.error(&format!("Failed to upload blob data: {}", e));
                RegistryError::Network(e.to_string())
            })?;

            if upload_response.status().is_success() {
                self.output
                    .success(&format!("Blob {} uploaded successfully", &normalized_digest[..16]));
                
                // Add a small delay to help with registry consistency
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                
                Ok(normalized_digest)
            } else {
                let status = upload_response.status();
                let error_text = upload_response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to read error response".to_string());
                
                self.output.warning(&format!(
                    "Monolithic upload failed (status {}): {}", 
                    status, error_text
                ));
                
                // If monolithic upload fails, check if we should try chunked upload approach
                if status == 404 && error_text.contains("BLOB_UPLOAD_INVALID") {
                    Err(RegistryError::Upload("CHUNKED_FALLBACK".to_string()))
                } else {
                    Err(RegistryError::Upload(format!(
                        "Blob upload failed (status {}): {}",
                        status, error_text
                    )))
                }
            }
        };

        match upload_result {
            Ok(digest) => Ok(digest),
            Err(RegistryError::Upload(ref msg)) if msg == "CHUNKED_FALLBACK" => {
                self.output.info("Trying chunked upload approach...");
                self.upload_blob_chunked(data, &normalized_digest_final, repository, &used_token).await
            },
            Err(e) => Err(e),
        }
    }

    /// Alternative chunked upload method for registries that don't support monolithic uploads
    async fn upload_blob_chunked(
        &self,
        data: &[u8],
        digest: &str,
        repository: &str,
        token: &Option<String>,
    ) -> Result<String> {
        self.output.detail("Starting chunked upload session...");
        
        // Use token manager for chunked upload if available
        if let Some(ref token_manager) = self.token_manager {
            let data_clone = data.to_vec();
            let digest_clone = digest.to_string();
            let repository_clone = repository.to_string();
            let output_clone = self.output.clone();
            let client_clone = self.client.clone();
            let address_clone = self.address.clone();

            return token_manager.execute_with_retry(|token| {
                let data = data_clone.clone();
                let digest = digest_clone.clone();
                let repository = repository_clone.clone();
                let output = output_clone.clone();
                let client = client_clone.clone();
                let address = address_clone.clone();

                Box::pin(async move {
                    Self::perform_chunked_upload(
                        &client,
                        &address,
                        &output,
                        &data,
                        &digest,
                        &repository,
                        &token,
                    ).await
                })
            }).await;
        }

        // Fallback to direct chunked upload without token manager
        Self::perform_chunked_upload(
            &self.client,
            &self.address,
            &self.output,
            data,
            digest,
            repository,
            token,
        ).await
    }

    /// Internal implementation of chunked upload that can be retried with token refresh
    async fn perform_chunked_upload(
        client: &reqwest::Client,
        address: &str,
        output: &Logger,
        data: &[u8],
        digest: &str,
        repository: &str,
        token: &Option<String>,
    ) -> Result<String> {
        // Step 1: Start upload session
        let upload_url = format!("{}/v2/{}/blobs/uploads/", address, repository);
        
        let mut request = client.post(&upload_url);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            output.error(&format!("Failed to start chunked upload session: {}", e));
            RegistryError::Network(e.to_string())
        })?;
        
        let status = response.status();
        if status == 401 {
            // Return 401 error for token manager to catch and retry
            return Err(RegistryError::Registry(format!(
                "401 Unauthorized: Failed to start chunked upload session - token may have expired"
            )));
        }
        
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(RegistryError::Upload(format!(
                "Failed to start chunked upload session (status {}): {}",
                status, error_text
            )));
        }

        // Get upload location URL
        let location = response
            .headers()
            .get("Location")
            .ok_or_else(|| RegistryError::Upload("Missing Location header in chunked upload response".to_string()))?
            .to_str()
            .map_err(|e| RegistryError::Upload(format!("Invalid Location header: {}", e)))?;

        let full_location = if location.starts_with("http") {
            location.to_string()
        } else {
            if location.starts_with('/') {
                format!("{}{}", address, location)
            } else {
                format!("{}/v2/{}/blobs/uploads/{}", address, repository, location)
            }
        };

        output.detail(&format!("Chunked upload location: {}", full_location));

        // Step 2: For small blobs, try single chunk upload (faster and more reliable)
        if data.len() <= 1024 * 1024 { // 1MB or less, use single chunk
            output.detail("Using single-chunk upload for small blob");
            
            let final_url = format!("{}?digest={}", full_location, digest);
            
            let mut upload_request = client
                .put(&final_url)
                .header("Content-Type", "application/octet-stream")
                .header("Content-Length", data.len().to_string())
                .body(data.to_vec());

            if let Some(token) = token {
                upload_request = upload_request.bearer_auth(token);
            }

            let upload_response = upload_request.send().await.map_err(|e| {
                output.error(&format!("Failed to upload single chunk: {}", e));
                RegistryError::Network(e.to_string())
            })?;

            let status = upload_response.status();
            if status == 401 {
                // Return 401 error for token manager to catch and retry
                return Err(RegistryError::Registry(format!(
                    "401 Unauthorized: Failed to upload single chunk - token may have expired"
                )));
            }

            if status.is_success() {
                output.success(&format!("Blob {} uploaded successfully via single chunk", &digest[..16]));
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                return Ok(digest.to_string());
            } else {
                let error_text = upload_response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to read error response".to_string());
                output.warning(&format!(
                    "Single chunk upload failed (status {}): {}, trying multi-chunk approach", 
                    status, error_text
                ));
            }
        }

        // Step 3: Multi-chunk upload for larger blobs
        output.detail("Using multi-chunk upload for large blob");
        let chunk_size = 1024 * 1024; // 1MB chunks
        let mut uploaded = 0;
        let mut current_location = full_location;
        let total_size = data.len();

        // Upload all chunks using PATCH, then send final PUT with digest only
        while uploaded < data.len() {
            let end = std::cmp::min(uploaded + chunk_size, data.len());
            let chunk = &data[uploaded..end];
            let is_final_chunk = end == data.len();
            
            output.detail(&format!(
                "Uploading chunk: bytes {}-{}/{} (final: {})", 
                uploaded, end - 1, total_size, is_final_chunk
            ));

            // Use PATCH for all chunks, including the final one
            let mut patch_request = client
                .patch(&current_location)
                .header("Content-Type", "application/octet-stream")
                .header("Content-Length", chunk.len().to_string());

            // Add Content-Range header with total size
            patch_request = patch_request.header(
                "Content-Range", 
                format!("{}-{}/{}", uploaded, end - 1, total_size)
            );

            patch_request = patch_request.body(chunk.to_vec());

            if let Some(token) = token {
                patch_request = patch_request.bearer_auth(token);
            }

            let patch_response = patch_request.send().await.map_err(|e| {
                output.error(&format!("Failed to upload chunk: {}", e));
                RegistryError::Network(e.to_string())
            })?;

            let status = patch_response.status();
            if status == 401 {
                // Return 401 error for token manager to catch and retry
                return Err(RegistryError::Registry(format!(
                    "401 Unauthorized: Failed to upload chunk - token may have expired"
                )));
            }

            if !status.is_success() {
                let error_text = patch_response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to read error response".to_string());
                return Err(RegistryError::Upload(format!(
                    "Chunk upload failed (status {}): {}",
                    status, error_text
                )));
            }

            // Update location for next chunk if provided
            if let Some(new_location) = patch_response.headers().get("Location") {
                if let Ok(new_location_str) = new_location.to_str() {
                    current_location = if new_location_str.starts_with("http") {
                        new_location_str.to_string()
                    } else if new_location_str.starts_with('/') {
                        format!("{}{}", address, new_location_str)
                    } else {
                        format!("{}/v2/{}/blobs/uploads/{}", address, repository, new_location_str)
                    };
                    output.detail(&format!("Updated location for next chunk: {}", current_location));
                }
            }

            uploaded = end;
        }

        // After all chunks are uploaded with PATCH, send final PUT with digest only (no body)
        output.detail("Finalizing chunked upload with digest...");
        let final_url = format!("{}?digest={}", current_location, digest);
        
        let mut put_request = client
            .put(&final_url)
            .header("Content-Length", "0"); // No body for final PUT

        if let Some(token) = token {
            put_request = put_request.bearer_auth(token);
        }

        let final_response = put_request.send().await.map_err(|e| {
            output.error(&format!("Failed to finalize chunked upload: {}", e));
            RegistryError::Network(e.to_string())
        })?;

        let status = final_response.status();
        if status == 401 {
            // Return 401 error for token manager to catch and retry
            return Err(RegistryError::Registry(format!(
                "401 Unauthorized: Failed to finalize chunked upload - token may have expired"
            )));
        }

        if status.is_success() {
            output.success(&format!(
                "Blob {} uploaded successfully via multi-chunk upload", 
                &digest[..16]
            ));
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            return Ok(digest.to_string());
        } else {
            let error_text = final_response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(RegistryError::Upload(format!(
                "Chunked upload finalization failed (status {}): {}",
                status, error_text
            )));
        }
    }

    /// Download blob from registry using Docker Registry v2 API
    pub async fn pull_blob(
        &self,
        repository: &str,
        digest: &str,
        token: &Option<String>,
    ) -> Result<Vec<u8>> {
        // Ensure digest format is correct
        let normalized_digest = if digest.starts_with("sha256:") {
            digest.to_string()
        } else {
            format!("sha256:{}", digest)
        };

        self.output.verbose(&format!(
            "Pulling blob {} from {}",
            &normalized_digest[..16],
            repository
        ));

        let url = format!(
            "{}/v2/{}/blobs/{}",
            self.address, repository, normalized_digest
        );

        // Use token manager for automatic refresh if available
        if let Some(ref token_manager) = self.token_manager {
            let repository_clone = repository.to_string();
            let url_clone = url.clone();
            let normalized_digest_clone = normalized_digest.clone();
            let output_clone = self.output.clone();
            let client_clone = self.client.clone();

            return token_manager.execute_with_retry(|token| {
                let url = url_clone.clone();
                let normalized_digest = normalized_digest_clone.clone();
                let output = output_clone.clone();
                let client = client_clone.clone();
                let repository = repository_clone.clone();

                Box::pin(async move {
                    let mut request = client.get(&url);

                    // Add auth header if token is provided
                    if let Some(token) = token {
                        request = request.bearer_auth(token);
                    }

                    let response = request.send().await.map_err(|e| {
                        output.error(&format!("Failed to pull blob: {}", e));
                        NetworkErrorHandler::handle_network_error(&e, "blob pull")
                    })?;

                    if response.status().is_success() {
                        let content_length = response.content_length().unwrap_or(0);

                        output.success(&format!(
                            "Successfully pulled blob {} ({}) from {}",
                            &normalized_digest[..16],
                            output.format_size(content_length),
                            repository
                        ));

                        let data = response.bytes().await.map_err(|e| {
                            RegistryError::Network(format!("Failed to read blob response: {}", e))
                        })?;

                        Ok(data.to_vec())
                    } else {
                        let status = response.status();
                        let error_text = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Failed to read error response".to_string());

                        // For 401 errors, return a specific error that the token manager can catch
                        if status == reqwest::StatusCode::UNAUTHORIZED {
                            return Err(RegistryError::Registry(format!(
                                "401 Unauthorized: Failed to pull blob {} from {} - token may have expired",
                                normalized_digest, repository
                            )));
                        }

                        output.error(&format!(
                            "Failed to pull blob: HTTP {} - {}",
                            status, error_text
                        ));

                        Err(RegistryError::Registry(format!(
                            "Failed to pull blob {} from {} (status {}): {}",
                            normalized_digest, repository, status, error_text
                        )))
                    }
                })
            }).await;
        }

        // Fallback to direct request without token manager
        let mut request = self.client.get(&url);

        // Add auth header if token is provided
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            self.output.error(&format!("Failed to pull blob: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "blob pull")
        })?;

        if response.status().is_success() {
            let content_length = response.content_length().unwrap_or(0);

            self.output.success(&format!(
                "Successfully pulled blob {} ({}) from {}",
                &normalized_digest[..16],
                self.output.format_size(content_length),
                repository
            ));

            let data = response.bytes().await.map_err(|e| {
                RegistryError::Network(format!("Failed to read blob response: {}", e))
            })?;

            Ok(data.to_vec())
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());

            self.output.error(&format!(
                "Failed to pull blob: HTTP {} - {}",
                status, error_text
            ));

            Err(RegistryError::Registry(format!(
                "Failed to pull blob {} from {} (status {}): {}",
                normalized_digest, repository, status, error_text
            )))
        }
    }

    /// Pull blob silently without individual success messages (for enhanced progress display)
    pub async fn pull_blob_silent(
        &self,
        repository: &str,
        digest: &str,
        token: &Option<String>,
    ) -> Result<Vec<u8>> {
        // Ensure digest format is correct
        let normalized_digest = if digest.starts_with("sha256:") {
            digest.to_string()
        } else {
            format!("sha256:{}", digest)
        };

        self.output.verbose(&format!(
            "Pulling blob {} from {}",
            &normalized_digest[..16], repository
        ));

        let url = format!(
            "{}/v2/{}/blobs/{}",
            self.address, repository, normalized_digest
        );

        let mut request = self.client.get(&url);

        // Add auth token if provided
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            self.output.error(&format!("Failed to pull blob: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "blob pull")
        })?;

        if response.status().is_success() {
            let data = response.bytes().await.map_err(|e| {
                RegistryError::Network(format!("Failed to read blob response: {}", e))
            })?;

            Ok(data.to_vec())
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());

            self.output.error(&format!(
                "Failed to pull blob: HTTP {} - {}",
                status, error_text
            ));

            Err(RegistryError::Registry(format!(
                "Failed to pull blob {} from {} (status {}): {}",
                normalized_digest, repository, status, error_text
            )))
        }
    }
}
