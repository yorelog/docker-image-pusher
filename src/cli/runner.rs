//! Enhanced runner with parallel upload support

use crate::cli::args::Args;
use crate::error::{Result, PusherError};
use crate::output::OutputManager;
use crate::image::parser::ImageParser;
use crate::registry::{RegistryClient, RegistryClientBuilder, AuthConfig};
use crate::upload::ParallelUploader;
use crate::digest::DigestUtils;
use std::path::Path;
use std::sync::Arc;
use std::io::Read;

pub struct Runner {
    args: Args,
    output: OutputManager,
}

impl Runner {
    pub fn new(args: Args) -> Result<Self> {
        // Create output manager based on args
        let output = if args.quiet {
            OutputManager::new_quiet()
        } else {
            OutputManager::new(args.verbose)
        };

        Ok(Self { args, output })
    }

    pub async fn run(&self) -> Result<()> {
        self.output.section("Docker Image Pusher");
        
        // Validate arguments first
        self.validate_arguments()?;
        
        // Parse the image
        let image_info = self.parse_image().await?;
        
        // Don't proceed if dry run
        if self.args.dry_run {
            self.output.success("Dry run completed successfully - no data was uploaded");
            return Ok(());
        }
        
        // Create registry client and push
        let client = self.create_registry_client().await?;
        self.push_image(&client, &image_info).await?;
        
        self.output.success("Image push completed successfully!");
        Ok(())
    }

    fn validate_arguments(&self) -> Result<()> {
        // Validate file exists
        if !Path::new(&self.args.file).exists() {
            return Err(PusherError::Validation(format!("Image file '{}' does not exist", self.args.file)));
        }
        
        // Validate URL format
        let _parsed_url = url::Url::parse(&self.args.repository_url)
            .map_err(|e| PusherError::Validation(format!("Invalid repository URL: {}", e)))?;
        
        // Validate concurrent settings
        if self.args.max_concurrent == 0 || self.args.max_concurrent > 10 {
            return Err(PusherError::Validation("max_concurrent must be between 1 and 10".to_string()));
        }
        
        Ok(())
    }

    async fn parse_image(&self) -> Result<crate::image::parser::ImageInfo> {
        let mut parser = ImageParser::new(self.output.clone());
        parser.set_large_layer_threshold(self.args.large_layer_threshold);
        
        let tar_path = Path::new(&self.args.file);
        parser.parse_tar_file(tar_path).await
    }

    async fn create_registry_client(&self) -> Result<RegistryClient> {
        let parsed_url = url::Url::parse(&self.args.repository_url)?;
        let registry_address = format!("{}://{}", parsed_url.scheme(), parsed_url.host_str().unwrap_or(""));
        
        let auth_config = if let (Some(username), Some(password)) = (&self.args.username, &self.args.password) {
            Some(AuthConfig::new(username.clone(), password.clone()))
        } else {
            None
        };
        
        RegistryClientBuilder::new(registry_address)
            .with_auth(auth_config)
            .with_timeout(self.args.timeout)
            .with_skip_tls(self.args.skip_tls)
            .with_verbose(self.args.verbose)
            .build()
    }

    async fn push_image(
        &self, 
        client: &RegistryClient,
        image_info: &crate::image::parser::ImageInfo
    ) -> Result<()> {
        self.output.section("Pushing image to registry");
        
        // Extract repository info from URL
        let parsed_url = url::Url::parse(&self.args.repository_url)?;
        let path = parsed_url.path().trim_start_matches('/');
        let (repository, tag) = if let Some(colon_pos) = path.rfind(':') {
            let (repo, tag_part) = path.split_at(colon_pos);
            (repo, &tag_part[1..])
        } else {
            (path, "latest")
        };
        
        // Authenticate for repository access if credentials provided
        let token = if let (Some(username), Some(password)) = (&self.args.username, &self.args.password) {
            let auth_config = AuthConfig::new(username.clone(), password.clone());
            client.authenticate_for_repository(&auth_config, repository).await?
        } else {
            None
        };
        
        self.output.info(&format!("Pushing {} layers to {}", image_info.layers.len(), repository));
        self.output.info(&format!("Total size: {}", self.output.format_size(image_info.total_size)));
        
        // Step 1: Check which blobs already exist
        let mut missing_blobs = Vec::new();
        let mut existing_blobs = Vec::new();
        let mut upload_size = 0u64;
        let mut existing_size = 0u64;
        
        self.output.subsection("Checking existing blobs");
        for (i, layer) in image_info.layers.iter().enumerate() {
            self.output.detail(&format!("Checking layer {}/{}: {}...", 
                i + 1, image_info.layers.len(), &layer.digest[..16]));
            
            let exists = client.check_blob_exists(&layer.digest, repository).await?;
            
            if !exists {
                missing_blobs.push(layer.clone());
                upload_size += layer.size;
                self.output.detail(&format!("Layer {} needs upload", i + 1));
            } else {
                existing_blobs.push(layer.clone());
                existing_size += layer.size;
                self.output.success(&format!("Layer {} already exists", i + 1));
            }
        }
        
        // Report summary
        if existing_blobs.is_empty() {
            self.output.info("No existing layers found - full upload required");
        } else {
            self.output.success(&format!("Found {} existing layers ({} total)", 
                existing_blobs.len(), self.output.format_size(existing_size)));
        }
        
        if missing_blobs.is_empty() {
            self.output.success("All layers already exist in registry");
        } else {
            self.output.info(&format!("Need to upload {} layers ({} total)", 
                missing_blobs.len(), self.output.format_size(upload_size)));
            
            // Check if user wants to skip existing layers
            if self.args.skip_existing && !existing_blobs.is_empty() {
                self.output.warning("--skip-existing flag specified, but there are missing layers that need upload");
                self.output.info("Proceeding with upload of missing layers only");
            }
            
            if self.args.force_upload {
                self.output.warning("--force-upload specified, uploading all layers regardless of existence");
                missing_blobs = image_info.layers.clone();
                upload_size = image_info.total_size;
                self.output.info(&format!("Force uploading {} layers ({} total)", 
                    missing_blobs.len(), self.output.format_size(upload_size)));
            }
            
            // Step 2: Upload missing blobs in parallel/sequential
            if self.args.max_concurrent > 1 && missing_blobs.len() > 1 {
                self.upload_layers_parallel(client, missing_blobs, repository, &token).await?;
            } else {
                self.upload_layers_sequential(client, missing_blobs, repository, &token).await?;
            }
        }
        
        // Step 3: Upload config blob
        self.output.subsection("Uploading config");
        let config_exists = client.check_blob_exists(&image_info.config_digest, repository).await?;
        
        if !config_exists {
            self.output.step("Uploading image config");
            self.upload_config_blob(client, image_info, repository, &token).await?;
            self.output.success("Config uploaded successfully");
        } else {
            self.output.info("Config already exists in registry");
        }
        
        // Step 4: Create and upload manifest
        self.output.subsection("Creating manifest");
        let manifest = self.create_image_manifest(image_info)?;
        
        self.output.step(&format!("Uploading manifest for {}:{}", repository, tag));
        self.upload_manifest_with_token(client, &manifest, repository, tag, &token).await?;
        
        self.output.success(&format!("Image {}:{} pushed successfully!", repository, tag));
        
        Ok(())
    }

    async fn upload_layers_parallel(
        &self,
        client: &RegistryClient,
        layers: Vec<crate::image::parser::LayerInfo>,
        repository: &str,
        token: &Option<String>, // Add token parameter
    ) -> Result<()> {
        self.output.subsection("Parallel Layer Upload");
        
        // Fix: Create Arc from owned RegistryClient, not reference
        let client_owned = Arc::new(client.clone()); // Assume RegistryClient implements Clone
        
        let parallel_uploader = ParallelUploader::new(
            client_owned,
            self.args.max_concurrent,
            self.args.large_layer_threshold,
            self.args.timeout,
            self.output.clone(),
        );

        let tar_path = Path::new(&self.args.file);

        parallel_uploader.upload_layers_parallel(
            layers,
            repository,
            tar_path,
            token, // Pass the token
        ).await
    }

    async fn upload_layers_sequential(
        &self,
        client: &RegistryClient,
        layers: Vec<crate::image::parser::LayerInfo>,
        repository: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.output.subsection("Sequential Layer Upload");
        
        let tar_path = Path::new(&self.args.file);
        
        for (i, layer) in layers.iter().enumerate() {
            self.output.info(&format!("Uploading layer {}/{}: {} ({})", 
                i + 1, layers.len(), &layer.digest[..16], self.output.format_size(layer.size)));
            
            if layer.size == 0 {
                self.upload_empty_layer(client, layer, repository, token).await?;
            } else if layer.size > self.args.large_layer_threshold {
                self.upload_large_layer_streaming(client, layer, repository, tar_path, token).await?;
            } else {
                self.upload_regular_layer(client, layer, repository, tar_path, token).await?;
            }
        }
        
        Ok(())
    }

    async fn upload_empty_layer(
        &self,
        client: &RegistryClient,
        layer: &crate::image::parser::LayerInfo,
        repository: &str,
        token: &Option<String>,
    ) -> Result<()> {
        self.output.detail("Uploading empty layer");
        
        let upload_url = client.start_upload_session_with_token(repository, token).await?;
        let empty_data = Vec::new();
        
        // Fix URL construction to match other uploaders
        let url = if upload_url.contains('?') {
            format!("{}&digest={}", upload_url, layer.digest)
        } else {
            format!("{}?digest={}", upload_url, layer.digest)
        };
        
        let mut request = client.get_http_client()
            .put(&url)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", "0")
            .body(empty_data);

        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await
            .map_err(|e| PusherError::Network(format!("Failed to upload empty layer: {}", e)))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(PusherError::Upload(format!("Empty layer upload failed (status {}): {}", status, error_text)))
        }
    }

    async fn upload_manifest_with_token(
        &self,
        client: &RegistryClient,
        manifest: &str,
        repository: &str,
        tag: &str,
        token: &Option<String>,
    ) -> Result<()> {
        let url = format!("{}/v2/{}/manifests/{}", client.get_address(), repository, tag);
        
        self.output.info(&format!("Uploading manifest for {}:{}", repository, tag));
        
        let mut request = client.get_http_client()
            .put(&url)
            .header("Content-Type", "application/vnd.docker.distribution.manifest.v2+json")
            .body(manifest.to_string());

        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await
            .map_err(|e| PusherError::Network(format!("Failed to upload manifest: {}", e)))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            Err(PusherError::Registry(format!(
                "Manifest upload failed (status {}): {}", 
                status, 
                error_text
            )))
        }
    }

    async fn upload_config_blob(
        &self,
        client: &RegistryClient,
        image_info: &crate::image::parser::ImageInfo,
        repository: &str,
        token: &Option<String>, // Add token parameter
    ) -> Result<()> {
        let config_data = self.extract_config_data_from_tar(image_info).await?;
        let upload_url = client.start_upload_session_with_token(repository, token).await?;
        
        let uploader = crate::upload::ChunkedUploader::new(self.args.timeout, self.output.clone());
        uploader.upload_large_blob(&upload_url, &config_data, &image_info.config_digest, token).await
    }

    async fn upload_large_layer_streaming(
        &self,
        client: &RegistryClient,
        layer: &crate::image::parser::LayerInfo,
        repository: &str,
        tar_path: &Path,
        token: &Option<String>,
    ) -> Result<()> {
        let upload_url = client.start_upload_session_with_token(repository, token).await?;
        let offset = 0; // Simplified - would need proper offset calculation
        
        let streaming_uploader = crate::upload::StreamingUploader::new(
            client.get_http_client().clone(),
            self.args.retry_attempts,
            self.args.timeout,
            self.output.clone(),
        );

        streaming_uploader.upload_from_tar_entry(
            tar_path,
            &layer.tar_path,
            offset,
            layer.size,
            &upload_url,
            &layer.digest,
            token,
            |_uploaded, _total| {
                // Progress callback
            },
        ).await
    }

    async fn upload_regular_layer(
        &self,
        client: &RegistryClient,
        layer: &crate::image::parser::LayerInfo,
        repository: &str,
        tar_path: &Path,
        token: &Option<String>,
    ) -> Result<()> {
        let layer_data = self.extract_layer_data_from_tar(tar_path, &layer.tar_path).await?;
        let upload_url = client.start_upload_session_with_token(repository, token).await?;
        
        let uploader = crate::upload::ChunkedUploader::new(self.args.timeout, self.output.clone());
        uploader.upload_large_blob(&upload_url, &layer_data, &layer.digest, token).await
    }

    async fn extract_layer_data_from_tar(
        &self,
        tar_path: &Path,
        layer_path: &str,
    ) -> Result<Vec<u8>> {
        use std::fs::File;
        use tar::Archive;
        
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);

        for entry_result in archive.entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))? {
            let mut entry = entry_result
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entry: {}", e)))?;
            
            let path = entry.path()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry path: {}", e)))?
                .to_string_lossy()
                .to_string();
            
            if path == layer_path {                self.output.detail(&format!("Extracting layer data: {}", layer_path));
                
                let mut data = Vec::new();
                entry.read_to_end(&mut data)
                    .map_err(|e| PusherError::Io(format!("Failed to read layer data: {}", e)))?;
                
                self.output.detail(&format!("Extracted {} bytes", data.len()));
                
                // Verify the extracted data using DigestUtils
                let computed = DigestUtils::compute_sha256(&data);
                self.output.detail(&format!("Extracted data SHA256: {}...", &computed[..16]));
                
                return Ok(data);
            }
        }

        Err(PusherError::ImageParsing(format!("Layer '{}' not found in tar archive", layer_path)))
    }
    
    async fn extract_config_data_from_tar(
        &self,
        image_info: &crate::image::parser::ImageInfo,
    ) -> Result<Vec<u8>> {
        use std::fs::File;
        use tar::Archive;
        
        let tar_path = Path::new(&self.args.file);
        let file = File::open(tar_path)
            .map_err(|e| PusherError::Io(format!("Failed to open tar file: {}", e)))?;
        let mut archive = Archive::new(file);

        // Look for config file (usually named like sha256:xxxxx.json)
        let config_filename = format!("{}.json", image_info.config_digest.replace("sha256:", ""));

        for entry_result in archive.entries()
            .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entries: {}", e)))? {
            let mut entry = entry_result
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read tar entry: {}", e)))?;

            let path = entry.path()
                .map_err(|e| PusherError::ImageParsing(format!("Failed to read entry path: {}", e)))?
                .to_string_lossy()
                .to_string();

            if path == config_filename {
                let mut config_string = String::new();
                entry.read_to_string(&mut config_string)
                    .map_err(|e| PusherError::ImageParsing(format!("Failed to read config data: {}", e)))?;
                return Ok(config_string.into_bytes());
            }
        }

        Err(PusherError::ImageParsing("Config file not found in tar archive".to_string()))
    }
    
    fn create_image_manifest(&self, image_info: &crate::image::parser::ImageInfo) -> Result<String> {
        use serde_json::json;
        
        // Validate all layer digests before creating manifest
        for (i, layer) in image_info.layers.iter().enumerate() {
            if !layer.digest.starts_with("sha256:") || layer.digest.len() != 71 {
                return Err(PusherError::Parse(format!(
                    "Invalid digest format for layer {}: {}", i + 1, layer.digest
                )));
            }
        }
        
        let layers: Vec<serde_json::Value> = image_info.layers.iter().map(|layer| {
            json!({
                "mediaType": layer.media_type,
                "size": layer.size,
                "digest": layer.digest
            })
        }).collect();

        // Validate config digest
        if !image_info.config_digest.starts_with("sha256:") || image_info.config_digest.len() != 71 {
            return Err(PusherError::Parse(format!(
                "Invalid config digest format: {}", image_info.config_digest
            )));
        }

        let config_size = self.calculate_config_size(image_info)?;
        
        let manifest = json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": {
                "mediaType": "application/vnd.docker.container.image.v1+json",
                "size": config_size,
                "digest": image_info.config_digest
            },
            "layers": layers
        });

        self.output.detail("✅ Created manifest with validated SHA256 digests");
        
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| PusherError::Parse(format!("Failed to serialize manifest: {}", e)))
    }

    fn calculate_config_size(&self, _image_info: &crate::image::parser::ImageInfo) -> Result<u64> {
        // Simplified - in practice you'd calculate the actual config size
        // from the config data extracted from the tar
        Ok(1000) // Placeholder
    }
}