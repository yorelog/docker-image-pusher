pub struct Manifest {
    pub schema_version: u32,
    pub media_type: String,
    pub layers: Vec<Layer>,
}

pub struct Layer {
    pub media_type: String,
    pub size: u64,
    pub digest: String,
}

impl Manifest {
    pub fn new(schema_version: u32, media_type: String, layers: Vec<Layer>) -> Self {
        Manifest {
            schema_version,
            media_type,
            layers,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 2 {
            return Err("Unsupported schema version".to_string());
        }
        if self.layers.is_empty() {
            return Err("Manifest must contain at least one layer".to_string());
        }
        Ok(())
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": self.schema_version,
            "mediaType": self.media_type,
            "layers": self.layers.iter().map(|layer| {
                serde_json::json!({
                    "mediaType": layer.media_type,
                    "size": layer.size,
                    "digest": layer.digest,
                })
            }).collect::<Vec<_>>(),
        })
    }
}