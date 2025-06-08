//! Enhanced registry client with better configuration and error handling

use crate::cli::config::AuthConfig;
use crate::error::{RegistryError, Result};
use crate::logging::Logger;
use crate::registry::auth::Auth;
use async_trait::async_trait;
use crate::registry::operations::{AuthOperations, BlobOperations, ManifestOperations, RepositoryOperations};
use crate::registry::oci_client::{OciClientAdapter, OciRegistryOperations};
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
    // Operations modules for better organization
    auth_operations: AuthOperations,
    blob_operations: BlobOperations,
    manifest_operations: ManifestOperations,
    repository_operations: RepositoryOperations,
    // OCI client for alternative implementation
    oci_client: Option<OciClientAdapter>,
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

        // Create operations modules
        let auth_operations = AuthOperations::new(client.clone(), auth.clone(), self.address.clone(), output.clone());
        let blob_operations = BlobOperations::new(client.clone(), self.address.clone(), output.clone(), None);
        let manifest_operations = ManifestOperations::new(client.clone(), self.address.clone(), output.clone());
        let repository_operations = RepositoryOperations::new(client.clone(), self.address.clone(), output.clone());

        // Create and enable OCI client by default
        let oci_client = if let Some(auth_config) = self.auth_config {
            output.verbose("Building OCI client with authentication...");
            Some(OciClientAdapter::with_auth(
                self.address.clone(),
                &auth_config,
                output.clone(),
            )?)
        } else {
            output.verbose("Building OCI client without authentication...");
            Some(OciClientAdapter::new(self.address.clone(), output.clone())?)
        };

        output.success("OCI client enabled by default for reliable operations");

        Ok(RegistryClient {
            client,
            auth,
            address: self.address,
            output,
            token_manager: None,
            auth_operations,
            blob_operations,
            manifest_operations,
            repository_operations,
            oci_client,
        })
    }
}

impl RegistryClient {
    /// Set token manager for all operations
    pub fn with_token_manager(mut self, token_manager: Option<TokenManager>) -> Self {
        self.token_manager = token_manager.clone();
        
        // Update all operations modules with the token manager
        self.auth_operations = self.auth_operations.with_token_manager(token_manager.clone());
        self.blob_operations = self.blob_operations.with_token_manager(token_manager.clone());
        self.manifest_operations = self.manifest_operations.with_token_manager(token_manager.clone());
        self.repository_operations = self.repository_operations.with_token_manager(token_manager.clone());
        
        // Update OCI client with token manager if available
        if let Some(ref mut oci_client) = self.oci_client {
            *oci_client = oci_client.clone().with_token_manager(token_manager);
        }
        
        self
    }

    pub async fn test_connectivity(&self) -> Result<()> {
        self.auth_operations.test_connectivity().await
    }

    pub async fn check_blob_exists(&self, digest: &str, repository: &str) -> Result<bool> {
        // Use OCI client by default if available
        if let Some(oci_client) = &self.oci_client {
            self.output.verbose("Using OCI client for blob existence check");
            oci_client.blob_exists(repository, digest).await
        } else {
            self.output.verbose("Falling back to legacy blob existence check");
            self.blob_operations.check_blob_exists(digest, repository).await
        }
    }

    pub async fn check_blob_exists_with_token(
        &self,
        digest: &str,
        repository: &str,
        token: &Option<String>,
    ) -> Result<bool> {
        // Use OCI client by default if available
        if let Some(oci_client) = &self.oci_client {
            self.output.verbose("Using OCI client for blob existence check");
            // If we have a token, create bearer auth for this operation
            if let Some(token_str) = token {
                let auth = oci_client::secrets::RegistryAuth::Bearer(token_str.clone());
                oci_client.blob_exists_with_auth(repository, digest, &auth).await
            } else {
                oci_client.blob_exists(repository, digest).await
            }
        } else {
            self.output.verbose("Falling back to legacy blob existence check");
            self.blob_operations.check_blob_exists_with_token(digest, repository, token).await
        }
    }

    pub async fn authenticate(&self, auth_config: &AuthConfig) -> Result<Option<String>> {
        self.auth_operations.authenticate(auth_config).await
    }

    pub async fn authenticate_for_repository(
        &self,
        auth_config: &AuthConfig,
        repository: &str,
    ) -> Result<Option<String>> {
        self.auth_operations.authenticate_for_repository(auth_config, repository).await
    }

    /// 统一的blob上传方法（合并upload_blob和upload_blob_with_token）
    pub async fn upload_blob_with_token(
        &self,
        data: &[u8],
        digest: &str,
        repository: &str,
        token: &Option<String>,
    ) -> Result<String> {
        // Use OCI client by default if available
        if let Some(oci_client) = &self.oci_client {
            self.output.verbose("Using OCI client for blob push");
            // If we have a token, create bearer auth for this operation
            if let Some(token_str) = token {
                let auth = oci_client::secrets::RegistryAuth::Bearer(token_str.clone());
                oci_client.push_blob_with_auth(repository, data, digest, &auth).await
            } else {
                oci_client.push_blob(repository, data, digest).await
            }
        } else {
            self.output.verbose("Falling back to legacy blob push");
            self.blob_operations.upload_blob_with_token(data, digest, repository, token).await
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
        use crate::image::manifest::convert_oci_to_docker_v2;
        
        // Use OCI client by default if available
        if let Some(oci_client) = &self.oci_client {
            self.output.verbose("Using OCI client for manifest push");
            
            // Convert OCI manifest to Docker V2 format for better registry compatibility
            let docker_manifest_bytes = convert_oci_to_docker_v2(manifest.as_bytes())?;
            self.output.verbose("Converted manifest to Docker V2 format for registry compatibility");
            
            // If we have a token, create bearer auth for this operation
            if let Some(token_str) = token {
                let auth = oci_client::secrets::RegistryAuth::Bearer(token_str.clone());
                let _ = oci_client.push_manifest_with_auth(repository, reference, &docker_manifest_bytes, &auth).await?;
            } else {
                let _ = oci_client.push_manifest(repository, reference, &docker_manifest_bytes).await?;
            }
            Ok(())
        } else {
            self.output.verbose("Falling back to legacy manifest push");
            self.manifest_operations.upload_manifest_with_token(manifest, repository, reference, token).await
        }
    }

    pub async fn pull_manifest(
        &self,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<Vec<u8>> {
        // Use OCI client by default if available
        if let Some(oci_client) = &self.oci_client {
            self.output.verbose("Using OCI client for manifest pull");
            let (manifest_data, _digest) = oci_client.pull_manifest(repository, reference).await?;
            Ok(manifest_data)
        } else {
            self.output.verbose("Falling back to legacy manifest pull");
            self.manifest_operations.pull_manifest(repository, reference, token).await
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
        // Use OCI client by default if available
        if let Some(oci_client) = &self.oci_client {
            self.output.verbose("Using OCI client for blob pull");
            oci_client.pull_blob(repository, digest).await
        } else {
            self.output.verbose("Falling back to legacy blob pull");
            self.blob_operations.pull_blob(repository, digest, token).await
        }
    }

    /// Pull blob silently without printing individual success messages (for enhanced progress display)
    pub async fn pull_blob_silent(
        &self,
        repository: &str,
        digest: &str,
        token: &Option<String>,
    ) -> Result<Vec<u8>> {
        // Use OCI client by default if available
        if let Some(oci_client) = &self.oci_client {
            // OCI client is already silent by default
            oci_client.pull_blob(repository, digest).await
        } else {
            self.blob_operations.pull_blob_silent(repository, digest, token).await
        }
    }

    /// 获取仓库中的所有标签列表
    pub async fn list_tags(&self, repository: &str, token: &Option<String>) -> Result<Vec<String>> {
        // Use OCI client by default if available
        if let Some(oci_client) = &self.oci_client {
            self.output.verbose("Using OCI client for tag listing");
            oci_client.list_tags(repository).await
        } else {
            self.output.verbose("Falling back to legacy tag listing");
            self.repository_operations.list_tags(repository, token).await
        }
    }

    /// 检查镜像是否存在于仓库中
    pub async fn check_image_exists(
        &self,
        repository: &str,
        reference: &str,
        token: &Option<String>,
    ) -> Result<bool> {
        self.repository_operations.check_image_exists(repository, reference, token).await
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

    /// Enable OCI client functionality
    pub fn enable_oci_client(&mut self) -> Result<()> {
        let oci_client = OciClientAdapter::new(self.address.clone(), self.output.clone())?;
        self.oci_client = Some(oci_client);
        Ok(())
    }

    /// Enable OCI client with authentication
    pub fn enable_oci_client_with_auth(&mut self, auth_config: &AuthConfig) -> Result<()> {
        let oci_client = OciClientAdapter::with_auth(
            self.address.clone(),
            auth_config,
            self.output.clone(),
        )?;
        self.oci_client = Some(oci_client);
        Ok(())
    }

    /// Check if OCI client is enabled
    pub fn has_oci_client(&self) -> bool {
        self.oci_client.is_some()
    }

    /// Get OCI client reference if available
    pub fn oci_client(&self) -> Option<&OciClientAdapter> {
        self.oci_client.as_ref()
    }
}

// Implement OCI Registry Operations trait for RegistryClient
#[async_trait]
impl OciRegistryOperations for RegistryClient {
    async fn oci_pull_manifest(&self, repository: &str, reference: &str) -> Result<(Vec<u8>, String)> {
        if let Some(oci_client) = &self.oci_client {
            oci_client.pull_manifest(repository, reference).await
        } else {
            Err(RegistryError::Validation(
                "OCI client not enabled. Call enable_oci_client() first.".to_string()
            ))
        }
    }

    async fn oci_pull_blob(&self, repository: &str, digest: &str) -> Result<Vec<u8>> {
        if let Some(oci_client) = &self.oci_client {
            oci_client.pull_blob(repository, digest).await
        } else {
            Err(RegistryError::Validation(
                "OCI client not enabled. Call enable_oci_client() first.".to_string()
            ))
        }
    }

    async fn oci_push_blob(&self, repository: &str, data: &[u8], digest: &str) -> Result<String> {
        if let Some(oci_client) = &self.oci_client {
            oci_client.push_blob(repository, data, digest).await
        } else {
            Err(RegistryError::Validation(
                "OCI client not enabled. Call enable_oci_client() first.".to_string()
            ))
        }
    }

    async fn oci_push_manifest(&self, repository: &str, reference: &str, manifest: &[u8]) -> Result<String> {
        if let Some(oci_client) = &self.oci_client {
            oci_client.push_manifest(repository, reference, manifest).await
        } else {
            Err(RegistryError::Validation(
                "OCI client not enabled. Call enable_oci_client() first.".to_string()
            ))
        }
    }

    async fn oci_blob_exists(&self, repository: &str, digest: &str) -> Result<bool> {
        if let Some(oci_client) = &self.oci_client {
            oci_client.blob_exists(repository, digest).await
        } else {
            Err(RegistryError::Validation(
                "OCI client not enabled. Call enable_oci_client() first.".to_string()
            ))
        }
    }

    async fn oci_manifest_exists(&self, repository: &str, reference: &str) -> Result<bool> {
        if let Some(oci_client) = &self.oci_client {
            oci_client.manifest_exists(repository, reference).await
        } else {
            Err(RegistryError::Validation(
                "OCI client not enabled. Call enable_oci_client() first.".to_string()
            ))
        }
    }

    async fn oci_list_tags(&self, repository: &str) -> Result<Vec<String>> {
        if let Some(oci_client) = &self.oci_client {
            oci_client.list_tags(repository).await
        } else {
            Err(RegistryError::Validation(
                "OCI client not enabled. Call enable_oci_client() first.".to_string()
            ))
        }
    }
}
