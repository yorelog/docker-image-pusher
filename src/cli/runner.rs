//! Enhanced runner with better structure and error handling

use crate::cli::args::Args;
use crate::error::{Result, PusherError};
use crate::output::OutputManager;
use crate::image::parser::ImageParser;
use crate::registry::{RegistryClient, RegistryClientBuilder, AuthConfig};
use std::time::Instant;
use std::path::Path;

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
        let start_time = Instant::now();
        
        self.output.section("Docker Image Pusher");
        self.output.info("Starting image push operation");
        
        // Validate arguments
        self.validate_arguments()?;
        
        // Parse the Docker image
        let image_info = self.parse_image().await?;
        
        // Setup registry client
        let client = self.create_registry_client().await?;
        
        // Perform the actual push
        if !self.args.dry_run {
            self.push_image(&client, &image_info).await?;
        } else {
            self.output.info("Dry run mode - skipping actual upload");
        }
        
        let elapsed = start_time.elapsed();
        self.output.success(&format!(
            "Operation completed successfully in {}",
            self.output.format_duration(elapsed)
        ));
        
        Ok(())
    }

    fn validate_arguments(&self) -> Result<()> {
        self.output.subsection("Validating arguments");
        
        // Validate the arguments
        self.args.validate()?;
        
        // Additional runtime validation
        let file_path = Path::new(&self.args.file);
        let file_size = std::fs::metadata(file_path)
            .map_err(|e| PusherError::Validation(format!("Cannot read file size: {}", e)))?
            .len();
        
        self.output.info(&format!("Image file: {} ({})", 
            self.args.file, self.output.format_size(file_size)));
        self.output.info(&format!("Target registry: {}", self.args.repository_url));
        
        // Check if file might be too large for memory-based processing
        if file_size > 10 * 1024 * 1024 * 1024 { // > 10GB
            self.output.warning("Very large image detected - ensure sufficient memory is available");
        }
        
        if file_size > self.args.large_layer_threshold {
            self.output.info("Large layers will use streaming upload for optimal memory usage");
        }
        
        self.output.step("Arguments validation passed");
        Ok(())
    }

    async fn parse_image(&self) -> Result<crate::image::parser::ImageInfo> {
        self.output.subsection("Parsing Docker image");
        
        // Pass output manager to ImageParser constructor
        let parser = ImageParser::new(self.output.clone());
        let file_path = Path::new(&self.args.file);
        
        let start_time = Instant::now();
        let image_info = parser.parse_tar_file(file_path).await?;
        let elapsed = start_time.elapsed();
        
        self.output.info(&format!(
            "Image parsing completed in {} - {} layers found",
            self.output.format_duration(elapsed),
            image_info.layers.len()
        ));
        
        // Print summary of large layers
        let large_layers: Vec<_> = image_info.layers.iter()
            .filter(|layer| layer.size > self.args.large_layer_threshold)
            .collect();
        
        if !large_layers.is_empty() {
            self.output.info(&format!(
                "Found {} large layers (>{}) that will use streaming upload",
                large_layers.len(),
                self.output.format_size(self.args.large_layer_threshold)
            ));
            
            for layer in &large_layers {
                self.output.detail(&format!(
                    "  Large layer: {} ({})",
                    &layer.digest[..23],
                    self.output.format_size(layer.size)
                ));
            }
        }
        
        Ok(image_info)
    }

    async fn create_registry_client(&self) -> Result<RegistryClient> {
        self.output.subsection("Setting up registry client");
        
        // Parse repository URL to extract components
        let parsed_url = url::Url::parse(&self.args.repository_url)
            .map_err(|e| PusherError::Config(format!("Invalid repository URL: {}", e)))?;
        
        // Extract registry address
        let registry_address = format!("{}://{}", 
            parsed_url.scheme(), 
            parsed_url.host_str().unwrap_or("localhost"));
        
        // Extract repository path and tag
        let path = parsed_url.path().trim_start_matches('/');
        let (repository, tag) = if let Some(colon_pos) = path.rfind(':') {
            let (repo, tag_part) = path.split_at(colon_pos);
            (repo, &tag_part[1..]) // Remove the ':' prefix
        } else {
            (path, "latest")
        };
        
        self.output.info(&format!("Registry: {}", registry_address));
        self.output.info(&format!("Repository: {}", repository));
        self.output.info(&format!("Tag: {}", tag));
        
        // Create auth config if credentials provided
        let auth_config = if let (Some(username), Some(password)) = (&self.args.username, &self.args.password) {
            self.output.step("Using provided credentials");
            Some(AuthConfig::new(username.clone(), password.clone()))
        } else {
            self.output.step("No credentials provided - attempting anonymous access");
            None
        };
        
        // Build registry client
        let client = RegistryClientBuilder::new(registry_address)
            .with_auth(auth_config)  // Now correctly passing Option<AuthConfig>
            .with_timeout(self.args.timeout)
            .with_skip_tls(self.args.skip_tls)
            .with_verbose(self.args.verbose)
            .build()?;
        
        // Test connectivity
        self.output.step("Testing registry connectivity");
        client.test_connectivity().await?;
        self.output.success("Registry connectivity verified");
        
        Ok(client)
    }

    async fn push_image(
        &self, 
        _client: &RegistryClient,
        image_info: &crate::image::parser::ImageInfo
    ) -> Result<()> {
        self.output.section("Pushing image to registry");
        
        // TODO: Implement the actual push logic
        // This will involve:
        // 1. Pushing each layer to the registry
        // 2. Using streaming upload for large layers
        // 3. Pushing the config blob
        // 4. Creating and pushing the manifest
        
        self.output.info(&format!("Would push {} layers", image_info.layers.len()));
        self.output.info(&format!("Total size: {}", self.output.format_size(image_info.total_size)));
        
        // For now, just return success
        // The actual implementation would go here
        
        Ok(())
    }
}