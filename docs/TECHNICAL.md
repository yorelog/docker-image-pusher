# Technical Documentation

## 🏗️ Architecture Deep Dive

### Memory Optimization Techniques

This section explains the technical details of how the tool achieves memory efficiency.

#### 1. Streaming Layer Downloads

**Problem**: Traditional approaches load entire layers into memory:
```rust
// ❌ High memory approach
let layer_data = client.pull_layer(&layer_ref).await?; // Loads entire layer
fs::write("layer_file", layer_data).await?;
```

**Solution**: Direct streaming to file handles:
```rust
// ✅ Memory-efficient approach  
let mut file = tokio::fs::File::create("layer_file").await?;
client.pull_blob(&image_ref, layer_desc, &mut file).await?; // Streams directly
```

#### 2. Sequential vs Concurrent Processing

**Memory Impact**:
- **Concurrent**: N layers × Average layer size = High memory usage
- **Sequential**: 1 layer × Largest layer size = Predictable memory usage

**Implementation**:
```rust
// Process layers one by one instead of concurrently
for layer_desc in manifest.layers.iter() {
    // Download single layer
    stream_layer_to_cache(layer_desc).await?;
    // Memory is freed before next layer
}
```

#### 3. Chunked Reading for Large Layers

For layers > 100MB, we use chunked reading:

```rust
if layer_size_mb > 100.0 {
    let chunk_size = 50 * 1024 * 1024; // 50MB chunks
    let mut buffer = vec![0u8; chunk_size];
    
    loop {
        let bytes_read = file.read(&mut buffer).await?;
        if bytes_read == 0 { break; }
        
        // Process chunk
        all_data.extend_from_slice(&buffer[..bytes_read]);
        
        // Small delay to prevent memory pressure
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
```

### OCI Client Integration

#### Key APIs Used

1. **`pull_image_manifest()`**: Fetches only manifest metadata
2. **`pull_blob()`**: Streams individual blobs (layers/config) to file handles
3. **`push_blob()`**: Uploads blob data to registry
4. **`push_manifest()`**: Uploads final manifest to complete image

#### Authentication Handling

```rust
// Anonymous for public registries
let auth = oci_client::secrets::RegistryAuth::Anonymous;

// Basic auth for private registries  
let auth = oci_client::secrets::RegistryAuth::Basic(
    username.to_string(), 
    password.to_string()
);

// Authenticate before operations
client.auth(&target_ref, &auth, oci_client::RegistryOperation::Push).await?;
```

## 🗂️ Data Structures

### Cache Index Format

The `index.json` file contains metadata for quick cache lookups:

```json
{
  "source_image": "registry.example.com/app:v1.0",
  "manifest": "manifest.json",
  "config": "sha256:abc123...",
  "layers": [
    "sha256:layer1...",
    "sha256:layer2...",
    "sha256:layer3..."
  ],
  "cached_at": 1672531200
}
```

### OCI Manifest Structure

The tool works with OCI Image Manifests:

```rust
pub struct OciImageManifest {
    pub schema_version: u8,           // Always 2
    pub media_type: Option<String>,   // Content type
    pub config: OciDescriptor,        // Config blob descriptor
    pub layers: Vec<OciDescriptor>,   // Layer descriptors
    pub subject: Option<OciDescriptor>, // Optional subject linking
    pub artifact_type: Option<String>, // For artifacts
    pub annotations: Option<BTreeMap<String, String>>, // Metadata
}
```

### Layer Descriptor Format

Each layer is described by an OciDescriptor:

```rust
pub struct OciDescriptor {
    pub media_type: String,    // e.g., "application/vnd.oci.image.layer.v1.tar+gzip"
    pub digest: String,        // e.g., "sha256:abc123..."
    pub size: i64,            // Layer size in bytes
    pub urls: Option<Vec<String>>, // Optional download URLs
    pub annotations: Option<BTreeMap<String, String>>, // Layer metadata
}
```

## 🔧 Error Handling Strategy

### Custom Error Types

```rust
#[derive(Error, Debug)]
pub enum PusherError {
    #[error("Pull error: {0}")]
    PullError(String),        // Network/registry issues during pull
    
    #[error("Push error: {0}")]  
    PushError(String),        // Network/registry issues during push
    
    #[error("Cache error: {0}")]
    CacheError(String),       // Local file system issues
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error), // Standard I/O errors
    
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error), // JSON parsing errors
    
    #[error("Cache not found")]
    CacheNotFound,           // Missing cache entry
}
```

### Error Context Propagation

```rust
// Convert and add context to errors
.map_err(|e| PusherError::PullError(format!("Failed to stream layer {}: {}", digest, e)))?
```

## 🚀 Performance Optimizations

### Rate Limiting Strategy

```rust
// Prevent registry overload and memory pressure
if layer_size_mb > 50.0 {
    // Longer delay for large layers
    tokio::time::sleep(Duration::from_millis(200)).await;
}

// Small delays between chunks
tokio::time::sleep(Duration::from_millis(10)).await;
```

### Platform Resolution

```rust
// Ensure we get the correct architecture
let mut client_config = oci_client::client::ClientConfig::default();
client_config.platform_resolver = Some(Box::new(oci_client::client::linux_amd64_resolver));
```

### Memory Usage Patterns

| Operation | Peak Memory Usage | Notes |
|-----------|------------------|-------|
| Manifest fetch | ~1-5KB | Small JSON document |
| Layer streaming | ~Buffer size (8KB default) | Constant regardless of layer size |
| Chunked reading | ~50MB | Configured chunk size |
| Config upload | ~Config size (<10KB typical) | Small JSON config |

## 🧪 Testing Strategies

### Unit Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sanitize_image_name() {
        assert_eq!(
            sanitize_image_name("registry.example.com/app:v1.0"),
            "registry.example.com_app_v1.0"
        );
    }
    
    #[tokio::test]
    async fn test_cache_detection() {
        // Test cache existence detection
        assert!(!has_cached_image("nonexistent:image").await.unwrap());
    }
}
```

### Integration Testing

```bash
# Test with small public image
cargo run -- pull alpine:latest
cargo run -- push alpine:latest localhost:5000/alpine:test --username test --password test

# Test with larger image  
cargo run -- pull nginx:latest
```

### Memory Testing

```bash
# Monitor memory usage during operation
cargo run -- pull large-image:latest &
PID=$!
while kill -0 $PID 2>/dev/null; do
    ps -p $PID -o rss= | awk '{print $1/1024 " MB"}'
    sleep 5
done
```

## 🔍 Debugging

### Enable Debug Logging

```bash
RUST_LOG=debug cargo run -- pull nginx:latest
```

### Common Debug Scenarios

#### 1. Authentication Issues
```rust
// Add debug output for auth
println!("🔐 Using auth: {:?}", auth);
```

#### 2. Layer Processing Issues  
```rust
// Add layer size debugging
println!("📦 Layer {}: {} bytes", digest, layer_size);
```

#### 3. Network Issues
```rust
// Add retry logic with exponential backoff
for attempt in 1..=3 {
    match client.pull_blob(&image_ref, layer_desc, &mut file).await {
        Ok(_) => break,
        Err(e) if attempt < 3 => {
            println!("⚠️  Attempt {} failed: {}, retrying...", attempt, e);
            tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await;
        }
        Err(e) => return Err(e.into()),
    }
}
```

## 🏷️ Tagging and Versioning

### Image Reference Parsing

The tool supports various image reference formats:

```bash
# Short names (Docker Hub)
nginx:latest                    → docker.io/library/nginx:latest

# Fully qualified names  
registry.example.com/app:v1.0   → registry.example.com/app:v1.0

# With ports
localhost:5000/test:latest      → localhost:5000/test:latest

# With digests
nginx@sha256:abc123...          → docker.io/library/nginx@sha256:abc123...
```

### Sanitization for Cache Paths

```rust
fn sanitize_image_name(image_name: &str) -> String {
    image_name
        .replace("/", "_")   // registry.com/app → registry.com_app
        .replace(":", "_")   // app:v1.0 → app_v1.0  
        .replace("@", "_")   // app@sha256:... → app_sha256:...
}
```

## 📊 Monitoring and Metrics

### Built-in Progress Reporting

```rust
// Layer progress
println!("📦 Streaming layer {}/{}: {}", i + 1, total_layers, digest);

// Size information
println!("📦 Uploading layer {}/{}: {} ({:.1} MB)", i + 1, total, digest, size_mb);

// Completion status
println!("✅ Successfully cached image with {} layers", layer_count);
```

### Custom Metrics (Future Enhancement)

```rust
// Example metrics structure
pub struct TransferMetrics {
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub start_time: Instant,
    pub layer_count: usize,
    pub current_layer: usize,
}

impl TransferMetrics {
    pub fn progress_percent(&self) -> f64 {
        (self.transferred_bytes as f64 / self.total_bytes as f64) * 100.0
    }
    
    pub fn transfer_rate(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        self.transferred_bytes as f64 / elapsed
    }
}
```

## 🔒 Security Considerations

### Credential Handling

```bash
# ❌ Avoid logging credentials
docker-image-pusher push app:v1.0 target:v1.0 --username user --password secret123

# ✅ Use environment variables
export DOCKER_PASSWORD=secret123
docker-image-pusher push app:v1.0 target:v1.0 --username user --password $DOCKER_PASSWORD
```

### Registry Validation

```rust
// Validate registry URLs to prevent injection
fn validate_registry_url(url: &str) -> Result<(), PusherError> {
    let parsed = url.parse::<Uri>()
        .map_err(|_| PusherError::PushError("Invalid registry URL".to_string()))?;
    
    if !["http", "https"].contains(&parsed.scheme_str().unwrap_or("")) {
        return Err(PusherError::PushError("Invalid URL scheme".to_string()));
    }
    
    Ok(())
}
```

### Cache Directory Security

```bash
# Set appropriate permissions on cache directory
chmod 700 .cache/
```

---

This technical documentation provides the implementation details needed to understand, modify, and extend the Docker Image Pusher tool.
