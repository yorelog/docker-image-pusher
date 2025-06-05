// This file defines the module for handling Docker images, including parsing and processing image tar packages.

pub mod parser;

pub use parser::{ImageInfo, LayerInfo, ImageConfig, ImageParser};