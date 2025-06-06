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
//! Basic usage involves creating an `ImageParser`, passing it a tar reader, and calling `parse`:
//!
//! ```no_run
//! use std::fs::File;
//! use std::io::BufReader;
//! use your_crate::image::{ImageParser, ImageInfo};
//!
//! let file = File::open("path/to/image.tar")?;
//! let reader = BufReader::new(file);
//! let parser = ImageParser::new(reader);
//! let image_info: ImageInfo = parser.parse()?;
//! // Now you can access image metadata and layers
//! ```
//!
//! See the individual struct and enum documentation for more details on the available methods and fields.


// This file defines the module for handling Docker images, including parsing and processing image tar packages.

pub mod parser;

pub use parser::{ImageConfig, ImageInfo, ImageParser, LayerInfo};
