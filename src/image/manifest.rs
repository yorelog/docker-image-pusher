use crate::error::{RegistryError, Result};
use serde_json::{self, Value};

/// Supported manifest types
#[derive(Debug, Clone, PartialEq)]
pub enum ManifestType {
    DockerV2,
    DockerList,
    OciManifest,
    OciIndex,
}

/// Parsed manifest information
#[derive(Debug, Clone)]
pub struct ParsedManifest {
    pub manifest_type: ManifestType,
    pub config_digest: Option<String>,
    pub layer_digests: Vec<String>,
    pub raw_data: Vec<u8>,
    pub platform_manifests: Option<Vec<PlatformManifest>>, // For index/list types
}

/// Platform-specific manifest reference (for OCI index/Docker manifest list)
#[derive(Debug, Clone)]
pub struct PlatformManifest {
    pub digest: String,
    pub media_type: String,
    pub platform: Option<Platform>,
}

/// Platform information
#[derive(Debug, Clone)]
pub struct Platform {
    pub architecture: String,
    pub os: String,
    pub variant: Option<String>,
}

impl ManifestType {
    pub fn from_media_type(media_type: &str) -> ManifestType {
        match media_type {
            "application/vnd.docker.distribution.manifest.v2+json" => ManifestType::DockerV2,
            "application/vnd.docker.distribution.manifest.list.v2+json" => ManifestType::DockerList,
            "application/vnd.oci.image.manifest.v1+json" => ManifestType::OciManifest,
            "application/vnd.oci.image.index.v1+json" => ManifestType::OciIndex,
            _ => ManifestType::DockerV2, // Default fallback
        }
    }

    pub fn is_index_type(&self) -> bool {
        matches!(self, ManifestType::DockerList | ManifestType::OciIndex)
    }

    pub fn to_content_type(&self) -> &'static str {
        match self {
            ManifestType::DockerV2 => "application/vnd.docker.distribution.manifest.v2+json",
            ManifestType::DockerList => "application/vnd.docker.distribution.manifest.list.v2+json",
            ManifestType::OciManifest => "application/vnd.oci.image.manifest.v1+json",
            ManifestType::OciIndex => "application/vnd.oci.image.index.v1+json",
        }
    }
}

pub fn parse_manifest(manifest_bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice(manifest_bytes).map_err(|e| RegistryError::Parse(e.to_string()))
}

/// Parse manifest and determine type and contents
pub fn parse_manifest_with_type(manifest_bytes: &[u8]) -> Result<ParsedManifest> {
    let manifest: Value = parse_manifest(manifest_bytes)?;

    // Determine manifest type from mediaType field
    let media_type = manifest
        .get("mediaType")
        .and_then(|m| m.as_str())
        .unwrap_or("application/vnd.docker.distribution.manifest.v2+json");

    let manifest_type = ManifestType::from_media_type(media_type);

    match manifest_type {
        ManifestType::OciIndex | ManifestType::DockerList => {
            // Parse index/list manifest
            let platform_manifests = parse_index_manifests(&manifest)?;
            Ok(ParsedManifest {
                manifest_type,
                config_digest: None,       // Index doesn't have config
                layer_digests: Vec::new(), // Index doesn't have layers directly
                raw_data: manifest_bytes.to_vec(),
                platform_manifests: Some(platform_manifests),
            })
        }
        ManifestType::DockerV2 | ManifestType::OciManifest => {
            // Parse single-platform manifest
            let config_digest = extract_config_digest(&manifest)?;
            let layer_digests = extract_layer_digests(&manifest)?;
            Ok(ParsedManifest {
                manifest_type,
                config_digest: Some(config_digest),
                layer_digests,
                raw_data: manifest_bytes.to_vec(),
                platform_manifests: None,
            })
        }
    }
}

/// Extract config digest from single-platform manifest
pub fn extract_config_digest(manifest: &Value) -> Result<String> {
    manifest
        .get("config")
        .and_then(|c| c.get("digest"))
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| RegistryError::Parse("Missing config digest in manifest".to_string()))
}

/// Extract layer digests from single-platform manifest
pub fn extract_layer_digests(manifest: &Value) -> Result<Vec<String>> {
    let layers = manifest
        .get("layers")
        .and_then(|l| l.as_array())
        .ok_or_else(|| RegistryError::Parse("Missing layers in manifest".to_string()))?;

    let mut digests = Vec::new();
    for layer in layers {
        if let Some(digest) = layer.get("digest").and_then(|d| d.as_str()) {
            digests.push(digest.to_string());
        }
    }

    if digests.is_empty() {
        return Err(RegistryError::Parse(
            "No layer digests found in manifest".to_string(),
        ));
    }

    Ok(digests)
}

/// Parse platform manifests from index/list
fn parse_index_manifests(manifest: &Value) -> Result<Vec<PlatformManifest>> {
    let manifests = manifest
        .get("manifests")
        .and_then(|m| m.as_array())
        .ok_or_else(|| RegistryError::Parse("Missing manifests array in index".to_string()))?;

    let mut platform_manifests = Vec::new();
    for m in manifests {
        let digest = m
            .get("digest")
            .and_then(|d| d.as_str())
            .ok_or_else(|| RegistryError::Parse("Missing digest in manifest entry".to_string()))?;

        let media_type = m
            .get("mediaType")
            .and_then(|mt| mt.as_str())
            .unwrap_or("application/vnd.docker.distribution.manifest.v2+json");

        let platform = if let Some(platform_obj) = m.get("platform") {
            Some(Platform {
                architecture: platform_obj
                    .get("architecture")
                    .and_then(|a| a.as_str())
                    .unwrap_or("amd64")
                    .to_string(),
                os: platform_obj
                    .get("os")
                    .and_then(|o| o.as_str())
                    .unwrap_or("linux")
                    .to_string(),
                variant: platform_obj
                    .get("variant")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })
        } else {
            None
        };

        platform_manifests.push(PlatformManifest {
            digest: digest.to_string(),
            media_type: media_type.to_string(),
            platform,
        });
    }

    Ok(platform_manifests)
}

// Get layer digests from manifest (legacy function for compatibility)
pub fn get_layers(manifest: &Value) -> Result<Vec<String>> {
    extract_layer_digests(manifest)
}

// Check if blob is already gzipped
pub fn is_gzipped(blob: &[u8]) -> bool {
    blob.len() >= 2 && blob[0] == 0x1f && blob[1] == 0x8b
}
