//! Docker image handling module
//!
//! This module provides types and logic for parsing, validating, and extracting metadata from Docker image tar packages.
//! It exposes the main [`ImageParser`] for reading image manifests, configs, and layers.
//!
//! # Overview
//!
//! The module is primarily concerned with interpreting the contents of Docker image tarballs,
//! which include the image manifest, configuration data, and layer archives.
//! It provides structures and implementations for accessing and manipulating this data.
//!
//! # Usage
//!
//! To use this module, import it and call the [`ImageParser::parse`] method with a tarball reader.
//! This will return an [`ImageInfo`] structure containing metadata about the image,
//! as well as methods for accessing the individual layers and configuration.
//!
//! # Examples
//!
//! Basic usage involves creating an `ImageParser` and parsing a tar file:
//!
//! ```no_run
//! use std::path::Path;
//! use docker_image_pusher::image::ImageParser;
//! use docker_image_pusher::logging::Logger;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let output = Logger::new(false);
//! let mut parser = ImageParser::new(output);
//! let image_info = parser.parse_tar_file(Path::new("path/to/image.tar")).await?;
//! // Now you can access image metadata and layers
//! # Ok(())
//! # }
//! ```
//!
//! See the individual struct and enum documentation for more details on the available methods and fields.
//!
//! # Processing Docker Images
//!
//! This module also includes functionality for processing Docker images,
//! such as extracting and handling image manifests and layer data.

// This file defines the module for handling Docker images, including parsing and processing image tar packages.

pub mod cache;
pub mod digest;
pub mod image_manager;
pub mod manifest;
pub mod parser;

// Specific exports to avoid ambiguity
pub use cache::Cache;
pub use digest::DigestUtils;
pub use image_manager::ImageManager;
pub use manifest::{get_layers, is_gzipped, parse_manifest};
pub use parser::{ImageInfo, ImageParser, LayerInfo};

// Re-export ImageConfig only from parser to avoid ambiguity
pub use parser::ImageConfig;
