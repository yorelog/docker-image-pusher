//! Application runner that orchestrates the image push process

use crate::{
    cli::Args,
    config::AppConfig,
    error::Result,
    image::parser::ImageParser,
    registry::{client::RegistryClient, auth::Auth},
};
use std::path::Path;

pub struct Runner {
    config: AppConfig,
}

impl Runner {
    pub fn new(args: Args) -> Result<Self> {
        let config = AppConfig::new(
            args.repository_url,
            args.file,
            args.username,
            args.password,
            args.chunk_size,
            args.concurrency,
            args.skip_tls,
            args.verbose,
        )?;

        Ok(Self { config })
    }

    pub async fn run(self) -> Result<()> {
        self.print_configuration();
        
        let auth_token = self.authenticate().await?;
        
        let client = self.create_registry_client(auth_token)?;
        client.check_registry_version().await?;
        
        let mut image_info = self.parse_image().await?;
        self.override_target_info(&mut image_info);
        
        self.print_image_info(&image_info);
        
        client.upload_image_with_info(
            Path::new(&self.config.tar_file_path), 
            &image_info
        ).await?;
        
        println!("\n=== Image Push Completed Successfully! ===");
        Ok(())
    }

    fn print_configuration(&self) {
        println!("=== Docker Image Pusher Starting ===");
        println!("Configuration:");
        println!("  Registry: {}", self.config.registry.url);
        println!("  Repository: {}", self.config.registry.repository);
        println!("  Tag: {}", self.config.registry.tag);
        println!("  File: {}", self.config.tar_file_path);
        println!("  Chunk size: {} bytes", self.config.upload.chunk_size);
        println!("  Concurrency: {}", self.config.upload.concurrency);
    }

    async fn authenticate(&self) -> Result<Option<String>> {
        if self.config.has_auth() {
            println!("  Authentication: enabled");
            println!("\n=== Authenticating ===");
            
            let auth = Auth::new(&self.config.registry.url, self.config.registry.skip_tls)?;
            
            if let (Some(username), Some(password)) = (&self.config.auth.username, &self.config.auth.password) {
                auth.login(username, password).await
            } else {
                Ok(None)
            }
        } else {
            println!("  Authentication: disabled");
            Ok(None)
        }
    }

    fn create_registry_client(&self, auth_token: Option<String>) -> Result<RegistryClient> {
        println!("\n=== Creating Registry Client ===");
        RegistryClient::builder(self.config.registry.url.clone())
            .with_auth(self.config.auth.clone())
            .with_auth_token(auth_token)
            .with_skip_tls(self.config.registry.skip_tls)
            .build()
    }

    async fn parse_image(&self) -> Result<crate::image::parser::ImageInfo> {
        println!("\n=== Parsing Docker Image Tar ===");
        let image_path = Path::new(&self.config.tar_file_path);
        let parser = ImageParser::new();
        parser.parse_tar_file(image_path).await
    }

    fn override_target_info(&self, image_info: &mut crate::image::parser::ImageInfo) {
        image_info.repository = self.config.registry.repository.clone();
        image_info.tag = self.config.registry.tag.clone();
    }

    fn print_image_info(&self, image_info: &crate::image::parser::ImageInfo) {
        println!("Image info:");
        println!("  Target Repository: {}", image_info.repository);
        println!("  Target Tag: {}", image_info.tag);
        println!("  Layers: {} found", image_info.layers.len());
        for (i, layer) in image_info.layers.iter().enumerate() {
            println!("    Layer {}: {} ({})", i + 1, layer.digest, layer.size);
        }
    }
}