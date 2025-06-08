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
    pub layer_info: Vec<(String, u64)>, // (digest, size) pairs for layers
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
                layer_info: Vec::new(),    // Index doesn't have layer info directly
                raw_data: manifest_bytes.to_vec(),
                platform_manifests: Some(platform_manifests),
            })
        }
        ManifestType::DockerV2 | ManifestType::OciManifest => {
            // Parse single-platform manifest
            let config_digest = extract_config_digest(&manifest)?;
            let layer_digests = extract_layer_digests(&manifest)?;
            let layer_info = extract_layer_info(&manifest)?;
            Ok(ParsedManifest {
                manifest_type,
                config_digest: Some(config_digest),
                layer_digests,
                layer_info,
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

/// Extract layer information (digest and size) from single-platform manifest
pub fn extract_layer_info(manifest: &Value) -> Result<Vec<(String, u64)>> {
    let layers = manifest
        .get("layers")
        .and_then(|l| l.as_array())
        .ok_or_else(|| RegistryError::Parse("Missing layers in manifest".to_string()))?;

    let mut layer_info = Vec::new();
    for layer in layers {
        if let (Some(digest), Some(size)) = (
            layer.get("digest").and_then(|d| d.as_str()),
            layer.get("size").and_then(|s| s.as_u64()),
        ) {
            layer_info.push((digest.to_string(), size));
        }
    }

    if layer_info.is_empty() {
        return Err(RegistryError::Parse(
            "No layer information found in manifest".to_string(),
        ));
    }

    Ok(layer_info)
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

/// Convert OCI manifest to Docker V2 manifest format for registry compatibility
/// 
/// This function takes an OCI manifest and converts it to Docker V2 format
/// which is more widely supported by container registries including Aliyun.
pub fn convert_oci_to_docker_v2(manifest_bytes: &[u8]) -> Result<Vec<u8>> {
    let manifest: Value = parse_manifest(manifest_bytes)?;
    
    // Check if it's already a Docker V2 manifest
    let media_type = manifest
        .get("mediaType")
        .and_then(|m| m.as_str())
        .unwrap_or("application/vnd.docker.distribution.manifest.v2+json");
    
    if media_type == "application/vnd.docker.distribution.manifest.v2+json" {
        // Already Docker V2 format, return as-is
        return Ok(manifest_bytes.to_vec());
    }
    
    // Convert OCI manifest to Docker V2 format
    let mut docker_manifest = serde_json::Map::new();
    
    // Set schema version (always 2 for Docker V2)
    docker_manifest.insert("schemaVersion".to_string(), Value::Number(2.into()));
    
    // Set media type to Docker V2
    docker_manifest.insert(
        "mediaType".to_string(),
        Value::String("application/vnd.docker.distribution.manifest.v2+json".to_string())
    );
    
    // Convert config section
    if let Some(config) = manifest.get("config") {
        let mut docker_config = config.clone();
        
        // Convert config media type from OCI to Docker
        if let Some(config_obj) = docker_config.as_object_mut() {
            let config_media_type = config_obj
                .get("mediaType")
                .and_then(|m| m.as_str())
                .unwrap_or("application/vnd.oci.image.config.v1+json");
            
            if config_media_type == "application/vnd.oci.image.config.v1+json" {
                config_obj.insert(
                    "mediaType".to_string(),
                    Value::String("application/vnd.docker.container.image.v1+json".to_string())
                );
            }
        }
        
        docker_manifest.insert("config".to_string(), docker_config);
    }
    
    // Convert layers section
    if let Some(layers) = manifest.get("layers").and_then(|l| l.as_array()) {
        let mut docker_layers = Vec::new();
        
        for layer in layers {
            let mut docker_layer = layer.clone();
            
            // Convert layer media types from OCI to Docker
            if let Some(layer_obj) = docker_layer.as_object_mut() {
                let layer_media_type = layer_obj
                    .get("mediaType")
                    .and_then(|m| m.as_str())
                    .unwrap_or("application/vnd.oci.image.layer.v1.tar+gzip");
                
                let docker_media_type = match layer_media_type {
                    "application/vnd.oci.image.layer.v1.tar+gzip" => 
                        "application/vnd.docker.image.rootfs.diff.tar.gzip",
                    "application/vnd.oci.image.layer.v1.tar" => 
                        "application/vnd.docker.image.rootfs.diff.tar",
                    "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip" => 
                        "application/vnd.docker.image.rootfs.diff.tar.gzip",
                    "application/vnd.oci.image.layer.nondistributable.v1.tar" => 
                        "application/vnd.docker.image.rootfs.diff.tar",
                    // If it's already a Docker media type, keep it
                    _ if layer_media_type.starts_with("application/vnd.docker.") => layer_media_type,
                    // Default fallback
                    _ => "application/vnd.docker.image.rootfs.diff.tar.gzip"
                };
                
                layer_obj.insert(
                    "mediaType".to_string(),
                    Value::String(docker_media_type.to_string())
                );
            }
            
            docker_layers.push(docker_layer);
        }
        
        docker_manifest.insert("layers".to_string(), Value::Array(docker_layers));
    }
    
    // Convert the manifest to JSON bytes
    let docker_manifest_value = Value::Object(docker_manifest);
    let docker_manifest_bytes = serde_json::to_vec_pretty(&docker_manifest_value)
        .map_err(|e| RegistryError::Parse(format!("Failed to serialize Docker V2 manifest: {}", e)))?;
    
    Ok(docker_manifest_bytes)
}
