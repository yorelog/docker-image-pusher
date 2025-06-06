//! Unified upload strategy interface to eliminate duplication

use crate::error::Result;
use crate::image::parser::LayerInfo;
use crate::output::OutputManager;
use crate::tar_utils::TarUtils;
use async_trait::async_trait;
use std::path::Path;

/// Common upload strategy trait
#[async_trait]
pub trait UploadStrategy {
    /// Upload a single layer
    async fn upload_layer(
        &self,
        layer: &LayerInfo,
        repository: &str,
        tar_path: &Path,
        token: &Option<String>,
        upload_url: &str,
    ) -> Result<()>;

    /// Check if this strategy supports the given layer
    fn supports_layer(&self, layer: &LayerInfo) -> bool;

    /// Get strategy name for logging
    fn name(&self) -> &'static str;
}

/// Upload strategy for empty layers (0 bytes)
pub struct EmptyLayerStrategy {
    pub output: OutputManager,
}

/// Upload strategy for small/regular layers
pub struct RegularLayerStrategy {
    pub output: OutputManager,
    pub timeout: u64,
    pub large_threshold: u64,
}

/// Upload strategy for large layers using streaming
pub struct StreamingLayerStrategy {
    pub output: OutputManager,
    pub timeout: u64,
    pub large_threshold: u64,
}

#[async_trait]
impl UploadStrategy for EmptyLayerStrategy {
    async fn upload_layer(
        &self,
        layer: &LayerInfo,
        _repository: &str,
        _tar_path: &Path,
        token: &Option<String>,
        upload_url: &str,
    ) -> Result<()> {
        self.output
            .detail(&format!("Uploading empty layer (0 bytes)"));

        // For empty layers, we can use the chunked uploader with empty data
        let uploader = crate::upload::ChunkedUploader::new(3600, self.output.clone());
        let empty_data = Vec::new();

        uploader
            .upload_large_blob(upload_url, &empty_data, &layer.digest, token)
            .await
    }

    fn supports_layer(&self, layer: &LayerInfo) -> bool {
        layer.size == 0
    }

    fn name(&self) -> &'static str {
        "EmptyLayer"
    }
}

#[async_trait]
impl UploadStrategy for RegularLayerStrategy {
    async fn upload_layer(
        &self,
        layer: &LayerInfo,
        _repository: &str,
        tar_path: &Path,
        token: &Option<String>,
        upload_url: &str,
    ) -> Result<()> {
        self.output.detail(&format!(
            "Uploading regular layer: {} ({})",
            &layer.digest[..16],
            self.output.format_size(layer.size)
        ));

        // Extract layer data from tar
        let layer_data = self.extract_layer_data(tar_path, &layer.tar_path).await?;

        // Use chunked uploader
        let uploader = crate::upload::ChunkedUploader::new(self.timeout, self.output.clone());

        uploader
            .upload_large_blob(upload_url, &layer_data, &layer.digest, token)
            .await
    }

    fn supports_layer(&self, layer: &LayerInfo) -> bool {
        layer.size > 0 && layer.size <= self.large_threshold
    }

    fn name(&self) -> &'static str {
        "RegularLayer"
    }
}

impl RegularLayerStrategy {
    async fn extract_layer_data(&self, tar_path: &Path, layer_path: &str) -> Result<Vec<u8>> {
        TarUtils::extract_layer_data(tar_path, layer_path)
    }
}

#[async_trait]
impl UploadStrategy for StreamingLayerStrategy {
    async fn upload_layer(
        &self,
        layer: &LayerInfo,
        _repository: &str,
        tar_path: &Path,
        token: &Option<String>,
        upload_url: &str,
    ) -> Result<()> {
        self.output.detail(&format!(
            "Uploading large layer via streaming: {} ({})",
            &layer.digest[..16],
            self.output.format_size(layer.size)
        ));

        // Find layer offset (simplified - in real usage you'd cache this)
        let offset = self.find_layer_offset(tar_path, &layer.tar_path).await?;

        // Use streaming uploader
        let streaming_uploader = crate::upload::StreamingUploader::new(
            reqwest::Client::new(),
            3, // max retries
            self.timeout,
            self.output.clone(),
        );

        streaming_uploader
            .upload_from_tar_entry(
                tar_path,
                &layer.tar_path,
                offset,
                layer.size,
                upload_url,
                &layer.digest,
                token,
                |_uploaded, _total| {
                    // Progress callback - could be enhanced
                },
            )
            .await
    }

    fn supports_layer(&self, layer: &LayerInfo) -> bool {
        layer.size > self.large_threshold
    }

    fn name(&self) -> &'static str {
        "StreamingLayer"
    }
}

impl StreamingLayerStrategy {
    async fn find_layer_offset(&self, tar_path: &Path, layer_path: &str) -> Result<u64> {
        TarUtils::find_layer_offset(tar_path, layer_path)
    }
}

/// Factory for creating appropriate upload strategies
pub struct UploadStrategyFactory {
    pub large_threshold: u64,
    pub timeout: u64,
    pub output: OutputManager,
}

impl UploadStrategyFactory {
    pub fn new(large_threshold: u64, timeout: u64, output: OutputManager) -> Self {
        Self {
            large_threshold,
            timeout,
            output,
        }
    }

    /// Get the appropriate strategy for a layer
    pub fn get_strategy(&self, layer: &LayerInfo) -> Box<dyn UploadStrategy + Send + Sync> {
        if layer.size == 0 {
            Box::new(EmptyLayerStrategy {
                output: self.output.clone(),
            })
        } else if layer.size > self.large_threshold {
            Box::new(StreamingLayerStrategy {
                output: self.output.clone(),
                timeout: self.timeout,
                large_threshold: self.large_threshold,
            })
        } else {
            Box::new(RegularLayerStrategy {
                output: self.output.clone(),
                timeout: self.timeout,
                large_threshold: self.large_threshold,
            })
        }
    }
}
