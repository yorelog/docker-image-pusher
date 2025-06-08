//! Enhanced registry client with better configuration and error handling

use crate::cli::config::AuthConfig;
use crate::error::handlers::NetworkErrorHandler;
use crate::error::{RegistryError, Result};
use crate::image::manifest::{ManifestType, parse_manifest};
use crate::logging::Logger;
use crate::registry::auth::Auth;
use crate::registry::token_manager::TokenManager;
use reqwest::Client;
use std::io::Read;
use std::time::Duration;

#[derive(Clone)] // Add Clone derive
pub struct RegistryClient {
    client: Client,
    pub auth: Auth,
    address: String,
    output: Logger,
    token_manager: Option<TokenManager>,
}

#[derive(Debug)]
pub struct RegistryClientBuilder {
    address: String,
    auth_config: Option<AuthConfig>,
    timeout: u64,
    skip_tls: bool,
    verbose: bool,
}

impl RegistryClientBuilder {
    pub fn new(address: String) -> Self {
        Self {
            address,
            auth_config: None,
            timeout: 7200, // 2 hours default
            skip_tls: false,
            verbose: false,
        }
    }

    pub fn with_auth(mut self, auth_config: Option<AuthConfig>) -> Self {
        self.auth_config = auth_config;
        self
    }

    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_skip_tls(mut self, skip_tls: bool) -> Self {
        self.skip_tls = skip_tls;
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn build(self) -> Result<RegistryClient> {
        let output = Logger::new(self.verbose);
        output.verbose("Building HTTP client...");

        let client_builder = if self.skip_tls {
            output.verbose("TLS verification disabled");
            Client::builder().danger_accept_invalid_certs(true)
            // This method is not available in the current reqwest version
            // .danger_accept_invalid_hostnames(true)
        } else {
            output.verbose("TLS verification enabled");
            Client::builder()
        };

        let client = client_builder
            .timeout(Duration::from_secs(self.timeout))
            .connect_timeout(Duration::from_secs(60))
            // This method is not available in the current reqwest version
            // .read_timeout(Duration::from_secs(3600))
            .pool_idle_timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(10)
            .user_agent("docker-image-pusher/1.0")
            .build()
            .map_err(|e| {
                output.error(&format!("Failed to build HTTP client: {}", e));
                RegistryError::Network(e.to_string())
            })?;

        output.verbose("HTTP client built successfully");

        let auth = Auth::new();

        Ok(RegistryClient {
            client,
            auth,
            address: self.address,
            output,
            token_manager: None,
        })
    }
}

impl RegistryClient {
    pub async fn test_connectivity(&self) -> Result<()> {
        self.output.verbose("Testing registry connectivity...");

        let url = format!("{}/v2/", self.address);
        let response =
            self.client.get(&url).send().await.map_err(|e| {
                RegistryError::Network(format!("Failed to connect to registry: {}", e))
            })?;

        self.output
            .verbose(&format!("Registry response status: {}", response.status()));

        if response.status().is_success() || response.status() == 401 {
            // 401 is expected for registries that require authentication
            self.output.verbose("Registry connectivity test passed");
            Ok(())
        } else {
            Err(RegistryError::Registry(format!(
                "Registry connectivity test failed with status: {}",
                response.status()
            )))
        }
    }

    pub async fn check_blob_exists(&self, digest: &str, repository: &str) -> Result<bool> {
        self.check_blob_exists_with_token(digest, repository, &None)
            .await
    }

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

        // Use HEAD request to check existence without downloading
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

    pub async fn authenticate(&self, auth_config: &AuthConfig) -> Result<Option<String>> {
        self.output.verbose("Authenticating with registry...");

        let token = self
            .auth
            .login(&auth_config.username, &auth_config.password, &self.output)
            .await?;

        if token.is_some() {
            self.output.success("Authentication successful");
        } else {
            self.output.info("No authentication required");
        }

        Ok(token)
    }

    pub async fn authenticate_for_repository(
        &self,
        auth_config: &AuthConfig,
        repository: &str,
    ) -> Result<Option<String>> {
        self.output.verbose(&format!(
            "Authenticating for repository access: {}",
            repository
        ));

        // Use the new Docker Registry API v2 compliant authentication
        let token = self
            .auth
            .authenticate_with_registry(
                &self.address,
                repository,
                Some(&auth_config.username),
                Some(&auth_config.password),
                &self.output,
            )
            .await?;

        if token.is_some() {
            self.output.success(&format!(
                "Repository authentication successful for: {}",
                repository
            ));
        } else {
            self.output
                .info("No repository-specific authentication required");
        }

        Ok(token)
    }

    /// 统一的blob上传方法（合并upload_blob和upload_blob_with_token）
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

        self.output.info(&format!(
            "Uploading blob {} ({}) to {}",
            &normalized_digest[..16],
            self.output.format_size(data.len() as u64),
            repository
        ));

        // 检查blob是否已存在
        if self
            .check_blob_exists_with_token(&normalized_digest, repository, token)
            .await?
        {
            self.output
                .info(&format!("Blob {} already exists, skipping", &normalized_digest[..16]));
            return Ok(normalized_digest);
        }

        // 使用分阶段上传（Staged Upload）- 更可靠的方式
        // 第一步：启动上传会话
        let upload_url = format!("{}/v2/{}/blobs/uploads/", self.address, repository);
        
        let mut request = self.client.post(&upload_url);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(RegistryError::Upload(format!(
                "Failed to start blob upload session (status {}): {}",
                status, error_text
            )));
        }

        // 获取上传位置URL
        let location = response
            .headers()
            .get("Location")
            .ok_or_else(|| RegistryError::Upload("Missing Location header in upload response".to_string()))?
            .to_str()
            .map_err(|e| RegistryError::Upload(format!("Invalid Location header: {}", e)))?;

        // 确保location是完整的URL
        let full_location = if location.starts_with("http") {
            location.to_string()
        } else {
            format!("{}{}", self.address, location)
        };

        // 第二步：上传blob数据并完成上传
        let final_url = format!("{}?digest={}", full_location, normalized_digest);
        
        let mut upload_request = self
            .client
            .put(&final_url)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", data.len().to_string())
            .body(data.to_vec());

        if let Some(token) = token {
            upload_request = upload_request.bearer_auth(token);
        }

        let upload_response = upload_request.send().await?;

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
            Err(RegistryError::Upload(format!(
                "Blob upload failed (status {}): {}",
                status, error_text
            )))
        }
    }

    /// 统一的manifest上传方法
    pub async fn upload_manifest_with_token(
        &self,
        manifest: &str,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<()> {
        let url = format!("{}/v2/{}/manifests/{}", self.address, repository, reference);

        // Parse manifest to detect content type
        let content_type = match parse_manifest(manifest.as_bytes()) {
            Ok(manifest_json) => {
                let media_type = manifest_json
                    .get("mediaType")
                    .and_then(|m| m.as_str())
                    .unwrap_or("application/vnd.docker.distribution.manifest.v2+json");

                let manifest_type = ManifestType::from_media_type(media_type);
                manifest_type.to_content_type()
            }
            Err(_) => {
                // Fallback to Docker v2 if parsing fails
                "application/vnd.docker.distribution.manifest.v2+json"
            }
        };

        self.output.verbose(&format!(
            "Uploading manifest with content-type: {}",
            content_type
        ));

        let mut request = self
            .client
            .put(&url)
            .header("Content-Type", content_type)
            .body(manifest.to_string());

        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if response.status().is_success() {
            self.output.success(&format!(
                "Manifest uploaded successfully for {}:{}",
                repository, reference
            ));
            Ok(())
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(RegistryError::Registry(format!(
                "Failed to upload manifest: HTTP {} - {}",
                status, error_text
            )))
        }
    }

    pub async fn pull_manifest(
        &self,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<Vec<u8>> {
        self.output.verbose(&format!(
            "Pulling manifest for {}/{}",
            repository, reference
        ));

        let url = format!("{}/v2/{}/manifests/{}", self.address, repository, reference);

        let mut request = self.client.get(&url).header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json, \
                 application/vnd.docker.distribution.manifest.list.v2+json, \
                 application/vnd.oci.image.manifest.v1+json, \
                 application/vnd.oci.image.index.v1+json",
        );

        // 添加授权头（如果提供了 token）
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            self.output
                .error(&format!("Failed to pull manifest: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "manifest pull")
        })?;

        if response.status().is_success() {
            self.output.success(&format!(
                "Successfully pulled manifest for {}/{}",
                repository, reference
            ));

            let content_type = response
                .headers()
                .get("Content-Type")
                .map(|h| h.to_str().unwrap_or("unknown"))
                .unwrap_or("unknown");

            self.output
                .detail(&format!("Manifest type: {}", content_type));

            let data = response.bytes().await.map_err(|e| {
                RegistryError::Network(format!("Failed to read manifest response: {}", e))
            })?;

            Ok(data.to_vec())
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());

            self.output.error(&format!(
                "Failed to pull manifest: HTTP {} - {}",
                status, error_text
            ));

            Err(RegistryError::Registry(format!(
                "Failed to pull manifest for {}/{} (status {}): {}",
                repository, reference, status, error_text
            )))
        }
    }

    /// 从 repository 拉取 blob
    ///
    /// 通过 registry API 获取指定的 blob 数据
    pub async fn pull_blob(
        &self,
        repository: &str,
        digest: &str,
        token: &Option<String>,
    ) -> Result<Vec<u8>> {
        // 确保摘要格式正确
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

                    // 添加授权头（如果提供了 token）
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

        // 添加授权头（如果提供了 token）
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

    /// Pull blob silently without printing individual success messages (for enhanced progress display)
    pub async fn pull_blob_silent(
        &self,
        repository: &str,
        digest: &str,
        token: &Option<String>,
    ) -> Result<Vec<u8>> {
        // 确保摘要格式正确
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

        // 如果有认证token，添加到请求头
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

    /// 获取仓库中的所有标签列表
    pub async fn list_tags(&self, repository: &str, token: &Option<String>) -> Result<Vec<String>> {
        self.output
            .verbose(&format!("Listing tags for repository: {}", repository));

        let url = format!("{}/v2/{}/tags/list", self.address, repository);

        let mut request = self.client.get(&url);

        // 添加授权头（如果提供了 token）
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            self.output.error(&format!("Failed to list tags: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "list tags")
        })?;

        if response.status().is_success() {
            let data: serde_json::Value = response.json().await.map_err(|e| {
                RegistryError::Parse(format!("Failed to parse tag list response: {}", e))
            })?;

            if let Some(tags) = data.get("tags").and_then(|t| t.as_array()) {
                let tag_list: Vec<String> = tags
                    .iter()
                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                    .collect();

                self.output.success(&format!(
                    "Successfully listed {} tags for {}",
                    tag_list.len(),
                    repository
                ));

                Ok(tag_list)
            } else {
                self.output
                    .warning(&format!("Repository {} has no tags", repository));
                Ok(Vec::new())
            }
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());

            // 如果返回 404，表示仓库可能不存在或没有标签
            if status.as_u16() == 404 {
                self.output.warning(&format!(
                    "Repository {} not found or has no tags",
                    repository
                ));
                return Ok(Vec::new());
            }

            self.output.error(&format!(
                "Failed to list tags: HTTP {} - {}",
                status, error_text
            ));

            Err(RegistryError::Registry(format!(
                "Failed to list tags for {} (status {}): {}",
                repository, status, error_text
            )))
        }
    }

    /// 检查镜像是否存在于仓库中
    pub async fn check_image_exists(
        &self,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<bool> {
        self.output.verbose(&format!(
            "Checking if image {}/{} exists",
            repository, reference
        ));

        // 尝试获取镜像清单，只获取头信息
        let url = format!("{}/v2/{}/manifests/{}", self.address, repository, reference);

        let mut request = self.client.head(&url).header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json",
        );

        // 添加授权头（如果提供了 token）
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            self.output
                .error(&format!("Failed to check image existence: {}", e));
            NetworkErrorHandler::handle_network_error(&e, "image existence check")
        })?;

        let exists = response.status().is_success();

        if exists {
            self.output.detail(&format!(
                "Image {}/{} exists in registry",
                repository, reference
            ));
        } else {
            self.output.detail(&format!(
                "Image {}/{} does not exist in registry",
                repository, reference
            ));
        }

        Ok(exists)
    }

    /// 从 tar 文件中提取并推送 blob 到 registry
    pub async fn push_blob_from_tar(
        &self,
        tar_path: &std::path::Path,
        layer_path: &str,
        digest: &str,
        repository: &str,
        _token: &Option<String>,
    ) -> Result<()> {
        use std::fs::File;
        use tar::Archive;

        self.output.verbose(&format!(
            "Extracting and pushing blob {} from tar file",
            &digest[..16]
        ));

        // 首先检查 blob 是否已存在
        if self.check_blob_exists(digest, repository).await? {
            self.output.info(&format!(
                "Blob {} already exists in registry",
                &digest[..16]
            ));
            return Ok(());
        }

        // 打开 tar 文件并提取 layer
        let file = File::open(tar_path)
            .map_err(|e| RegistryError::Io(format!("Failed to open tar file: {}", e)))?;

        let mut archive = Archive::new(file);

        // 查找并提取指定的 layer
        for entry_result in archive
            .entries()
            .map_err(|e| RegistryError::Io(format!("Failed to read tar entries: {}", e)))?
        {
            let mut entry = entry_result
                .map_err(|e| RegistryError::Io(format!("Failed to read tar entry: {}", e)))?;

            let path = entry
                .path()
                .map_err(|e| RegistryError::Io(format!("Failed to get entry path: {}", e)))?;

            if path.to_string_lossy() == layer_path {
                self.output.info(&format!("Found layer: {}", layer_path));

                // 读取 layer 内容
                let mut data = Vec::new();
                entry
                    .read_to_end(&mut data)
                    .map_err(|e| RegistryError::Io(format!("Failed to read layer data: {}", e)))?;

                // 上传 blob
                self.upload_blob(&data, digest, repository).await?;

                return Ok(());
            }
        }

        Err(RegistryError::ImageParsing(format!(
            "Layer {} not found in tar file",
            layer_path
        )))
    }

    /// 启动上传会话（内部方法）
    #[allow(dead_code)]
    async fn start_upload_session_with_token(
        &self,
        repository: &str,
        token: &Option<String>,
    ) -> Result<String> {
        let url = format!("{}/v2/{}/blobs/uploads/", self.address, repository);

        let mut request = self.client.post(&url);

        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if response.status().is_success() {
            // 从Location头获取上传URL
            if let Some(location) = response.headers().get("Location") {
                let upload_url = location.to_str().map_err(|_| {
                    RegistryError::Registry("Invalid upload URL in response".to_string())
                })?;

                // 如果是相对URL，需要拼接完整URL
                if upload_url.starts_with("/") {
                    Ok(format!("{}{}", self.address, upload_url))
                } else {
                    Ok(upload_url.to_string())
                }
            } else {
                Err(RegistryError::Registry(
                    "No upload URL provided in response".to_string(),
                ))
            }
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(RegistryError::Registry(format!(
                "Failed to start upload session (status {}): {}",
                status, error_text
            )))
        }
    }

    /// 简化的blob上传方法（用于内部调用）
    async fn upload_blob(&self, data: &[u8], digest: &str, repository: &str) -> Result<String> {
        self.upload_blob_with_token(data, digest, repository, &None)
            .await
    }

    /// Enable automatic token refresh for long-running operations
    pub fn with_token_manager(mut self, token_info: Option<crate::registry::auth::TokenInfo>) -> Self {
        if let Some(info) = token_info {
            self.token_manager = Some(TokenManager::new(self.auth.clone(), self.output.clone()).with_token_info(Some(info)));
        }
        self
    }
}
