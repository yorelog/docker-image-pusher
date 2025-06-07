//! Enhanced Docker image parsing with better error handling and progress reporting

use crate::error::Result;
use crate::logging::Logger;
use crate::registry::tar_utils::TarUtils;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LayerInfo {
    pub digest: String,
    pub size: u64,
    pub media_type: String,
    pub tar_path: String,
    pub compressed_size: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImageConfig {
    pub architecture: Option<String>,
    pub os: Option<String>,
    pub config: Option<serde_json::Value>,
    pub rootfs: Option<serde_json::Value>,
    pub history: Option<Vec<serde_json::Value>>,
    pub created: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub config_digest: String,
    pub config_size: u64,
    pub layers: Vec<LayerInfo>,
    pub total_size: u64,
}

pub struct ImageParser {
    output: Logger,
    large_layer_threshold: u64,
}

impl ImageParser {
    pub fn new(output: Logger) -> Self {
        Self {
            output,
            large_layer_threshold: 100 * 1024 * 1024, // 100MB
        }
    }

    pub fn set_large_layer_threshold(&mut self, threshold: u64) {
        self.large_layer_threshold = threshold;
        self.output.detail(&format!(
            "Large layer threshold set to {}",
            self.output.format_size(threshold)
        ));
    }

    /// Parse Docker image from tar file using TarUtils
    pub async fn parse_tar_file(&mut self, tar_path: &Path) -> Result<ImageInfo> {
        let start_time = Instant::now();
        self.output.section("Parsing Docker Image");
        self.output.info(&format!("Source: {}", tar_path.display()));

        // Use TarUtils for parsing - single source of truth
        let image_info = TarUtils::parse_image_info(tar_path)?;

        let elapsed = start_time.elapsed();
        self.output.success(&format!(
            "Parsing completed in {} - {} layers, total size: {}",
            self.output.format_duration(elapsed),
            image_info.layers.len(),
            self.output.format_size(image_info.total_size)
        ));

        if self.output.verbose {
            self.print_image_summary(&image_info);
        }

        Ok(image_info)
    }

    fn print_image_summary(&self, image_info: &ImageInfo) {
        let empty_layers_count = image_info.layers.iter().filter(|l| l.size == 0).count();
        let large_layers_count = image_info
            .layers
            .iter()
            .filter(|l| l.size > self.large_layer_threshold)
            .count();

        let items = vec![
            ("Layers", image_info.layers.len().to_string()),
            ("Empty Layers", empty_layers_count.to_string()),
            (
                "Large Layers",
                format!(
                    "{} (>{})",
                    large_layers_count,
                    self.output.format_size(self.large_layer_threshold)
                ),
            ),
            ("Total Size", self.output.format_size(image_info.total_size)),
            (
                "Config Digest",
                format!("{}...", &image_info.config_digest[..23]),
            ),
        ];

        self.output.summary_kv("Image Information", &items);

        if self.output.verbose {
            self.output.subsection("Layer Details");
            for (i, layer) in image_info.layers.iter().enumerate() {
                let layer_type = if layer.size == 0 {
                    " (EMPTY)"
                } else if layer.size > self.large_layer_threshold {
                    " (LARGE)"
                } else {
                    ""
                };

                self.output.detail(&format!(
                    "Layer {}: {}... ({}){}",
                    i + 1,
                    &layer.digest[..23],
                    self.output.format_size(layer.size),
                    layer_type
                ));
            }
        }
    }
}
