# Docker Image Pusher

[![Build Status](https://github.com/yorelog/docker-image-pusher/workflows/Build/badge.svg)](https://github.com/yorelog/docker-image-pusher/actions)
[![Crates.io](https://img.shields.io/crates/v/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![Downloads](https://img.shields.io/crates/d/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![License](https://img.shields.io/github/license/yorelog/docker-image-pusher)](https://github.com/yorelog/docker-image-pusher)

A memory-optimized Docker image transfer tool designed to handle large Docker images without excessive memory usage. This tool addresses the common problem of memory exhaustion when pulling or pushing multi-gigabyte Docker images.

## 🎯 Problem Statement

Traditional Docker image tools often load entire layers into memory, which can cause:
- **Memory exhaustion** with large images (>1GB)
- **System instability** when processing multiple large layers
- **Failed transfers** due to insufficient RAM
- **Poor performance** on resource-constrained systems

## 🚀 Solution

This tool implements **streaming-based layer processing** using the OCI client library:

- ✅ **Streaming Downloads**: Layers are streamed directly to disk without loading into memory
- ✅ **Sequential Processing**: Processes one layer at a time to minimize memory footprint  
- ✅ **Chunked Uploads**: Large layers (>100MB) are read in 50MB chunks during upload
- ✅ **Local Caching**: Efficient caching system for faster subsequent operations
- ✅ **Progress Monitoring**: Real-time feedback on transfer progress and layer sizes

## 🆕 What's New in 0.5.2

- **Push workflow orchestrator** – New `PushWorkflow` struct now coordinates input analysis, target inference, credential lookup, cache hydration, and the layer/config upload sequence. Each stage is its own method, making the CLI easier to extend while keeping the streaming guarantees the project is known for.
- **Smarter destination & credential inference** – History- and tar-metadata-based target suggestions now live inside the workflow. The confirmation prompt remembers previously accepted registries, and credential lookup cleanly falls back to stored logins before asking for overrides.
- **Large-layer telemetry** – Chunked uploads for 1GB+ layers emit richer progress, ETA, and throughput stats. We only keep a single chunk in memory and back off between medium-sized layers to stay friendly to registries with aggressive rate limits.
- **Tar importer refactor** – A dedicated `TarImporter` groups manifest parsing, layer extraction, digest calculation, and cache persistence. Extraction progress for oversized layers mirrors the push progress bars so you can see streaming speeds end-to-end.
- **Vendor cleanup** – Removed the old vendored OCI client copy and its tests; the workspace now relies solely on the published crates, which simplifies audits and shrinks the source tree.

## OCI Core Library

The OCI functionality now lives inside `crates/oci-core`, an MIT-licensed library crate that
can be embedded in other tools. It exposes:

- `reference` – a no-dependency reference parser with rich `OciError` signals
- `auth` – helpers for anonymous/basic auth negotiation
- `client` – an async `reqwest` uploader/downloader that understands chunked blobs,
  real-time telemetry, and registry-provided chunk hints

`docker-image-pusher` consumes `oci-core` through a normal Cargo path dependency, mirroring how
Rust itself treats the `core` crate. This keeps the CLI boundary clean while enabling other
projects to reuse the same stable OCI primitives without pulling in the rest of the binary.

## 📋 Prerequisites

- **Rust**: Version 1.70 or later
- **Network Access**: To source and target registries
- **Disk Space**: Sufficient space for caching large images

## 🛠️ Installation

### Download Pre-built Binaries (Recommended)

Download the latest compiled binaries from [GitHub Releases](https://github.com/yorelog/docker-image-pusher/releases):

**Available Platforms:**
- `docker-image-pusher-linux-x86_64` - Linux 64-bit
- `docker-image-pusher-macos-x86_64` - macOS Intel
- `docker-image-pusher-macos-aarch64` - macOS Apple Silicon (M1/M2)
- `docker-image-pusher-windows-x86_64.exe` - Windows 64-bit

**Installation Steps:**

1. Visit the [Releases page](https://github.com/yorelog/docker-image-pusher/releases)
2. Download the binary for your platform from the latest release
3. Make it executable and add to PATH:

```bash
# Linux/macOS
chmod +x docker-image-pusher-*
sudo mv docker-image-pusher-* /usr/local/bin/docker-image-pusher

# Windows
# Move docker-image-pusher-windows-x86_64.exe to a directory in your PATH
# Rename to docker-image-pusher.exe if desired
```

### Install from Crates.io

Install directly using Cargo from the official Rust package registry:

```bash
cargo install docker-image-pusher
```

This will compile and install the latest published version from [crates.io](https://crates.io/crates/docker-image-pusher).

### From Source

For development or customization:

```bash
git clone https://github.com/yorelog/docker-image-pusher
cd docker-image-pusher
cargo build --release
```

The compiled binary will be available at `target/release/docker-image-pusher` (or `.exe` on Windows)

## 📖 Usage

### Quick Start (two commands)

1. **Login (once per registry)**
    ```bash
    docker-image-pusher login registry.example.com --username user --password pass
    ```
    Credentials are saved under `.cache/credentials.json` for reuse.

2. **Push a docker save tar directly**
    ```bash
    docker-image-pusher push ./nginx.tar
    ```
    - Create the tar with `docker save nginx:latest -o nginx.tar` (or any image you like).
    - During `push`, the tool automatically combines the RepoTag inside the tar with the registry you just logged into (or the last five registries you pushed to) and prints something like `🎯 Target image resolved as: registry.example.com/tools/nginx:latest` before uploading. Pass `--registry other.example.com` if you need to override the destination host.

Need to cache an image first? Run `pull` or `import` (see the table below) and then call `push <image>`—the flow is identical once the image is in `.cache/`.

### Command Reference

| Command | When to use | Key flags |
|---------|-------------|-----------|
| `pull <image>` | Cache an image from any registry | – |
| `import <tar> <name>` | Convert `docker save` output into cache | – |
| `push <input>` | Upload cached image **or** tar; `<input>` can be `nginx:latest` or `./file.tar` | `-t` target override, `--registry` host override, `--username/--password` credential override, `--blob-chunk` chunk size (MiB) |
| `login <registry>` | Save credentials for future pushes | `--username`, `--password` |

The `push` command now handles most of the bookkeeping automatically:

- infers a sensible destination from tar metadata, the last five targets, or stored credentials
- prompts once when switching registries (or auto-confirms if you accepted it before)
- imports `docker save` archives on the fly before uploading
- reuses saved logins unless you pass explicit `--username/--password`

### Tips

- Need a different account temporarily? Pass `--username/--password` (or use env vars such as `DOCKER_USERNAME`) and they override stored credentials for that run only.
- Prefer scripting? Keep everything declarative: `login` once inside CI, then run `pull`, `push`, done.
- Unsure what target was used last time? Run `push` without `-t`; the history-based inference will suggest a sane default and print it before uploading.

## 🏗️ Architecture

### Memory Optimization Strategy

```
Traditional Approach (High Memory):
[Registry] → [Full Image in Memory] → [Local Storage]
     ↓
❌ Memory usage scales with image size
❌ Can exceed available RAM with large images

Optimized Approach (Low Memory):  
[Registry] → [Stream Layer by Layer] → [Local Storage]
     ↓
✅ Constant memory usage regardless of image size
✅ Handles multi-GB images efficiently
```

### Cache Structure

Images are cached in `.cache/` directory with the following structure:

```
.cache/
└── {sanitized_image_name}/
    ├── index.json              # Metadata and layer list
    ├── manifest.json           # OCI image manifest
    ├── config_{digest}.json    # Image configuration
    ├── {layer_digest_1}        # Layer file 1
    ├── {layer_digest_2}        # Layer file 2
    └── ...                     # Additional layers
```

### Processing Flow

#### Pull Operation:
1. **Fetch Manifest** - Download image metadata (~1-5KB)
2. **Create Cache Structure** - Set up local directories
3. **Stream Layers** - Download each layer directly to disk
4. **Cache Metadata** - Store manifest and configuration
5. **Create Index** - Generate lookup metadata

#### Push Operation:
1. **Authenticate** - Connect to target registry
2. **Read Cache** - Load cached image metadata
3. **Upload Layers** - Transfer layers with size-based optimization
4. **Upload Config** - Transfer image configuration
5. **Push Manifest** - Complete the image transfer

### Layer Processing Strategies

| Layer Size | Strategy | Memory Usage | Description |
|------------|----------|--------------|-------------|
| < 100MB | Direct Read | ~Layer Size | Read entire layer into memory |
| > 100MB | Chunked Read | ~50MB | Read in 50MB chunks with delays |
| Any Size | Streaming | ~Buffer Size | Direct stream to/from disk |

## 🔧 Configuration

### Client Configuration

The tool uses these default settings:

```rust
// Platform resolver for multi-arch images
platform_resolver = linux_amd64_resolver

// Authentication methods
- Anonymous (for public registries)
- Basic Auth (username/password)

// Chunk size for large layers
chunk_size = 50MB

// Rate limiting delays
large_layer_delay = 200ms
chunk_delay = 10ms
```

### Customization

You can modify these settings in `src/main.rs`:

```rust
// Adjust chunk size for very large layers
let chunk_size = 100 * 1024 * 1024; // 100MB chunks

// Modify size threshold for chunked processing  
if layer_size_mb > 50.0 { // Lower threshold
    // Use chunked approach
}

// Adjust rate limiting
tokio::time::sleep(tokio::time::Duration::from_millis(500)).await; // Longer delay
```

### Debugging OCI Traffic

Set the following environment variables to inspect the raw OCI flow without recompiling:

| Variable | Effect |
|----------|--------|
| `OCI_DEBUG=1` | Logs every HTTP request/response handled by the internal OCI client (method, URL, status, scope). |
| `OCI_DEBUG_UPLOAD=1` | Adds detailed tracing for blob uploads (upload session URLs, redirects, finalization). Inherits `OCI_DEBUG` when set. |

These logs run through `println!`, so they appear directly in the CLI output and can be piped to files for troubleshooting.

## 📊 Performance Comparison

### Memory Usage (Processing 5GB Image)

| Method | Peak Memory | Notes |
|--------|-------------|-------|
| Traditional Docker | ~5.2GB | Loads layers into memory |
| **This Tool** | ~50MB | Streams with chunked processing |

### Transfer Speed

- **Network bound**: Performance limited by network speed
- **Consistent memory**: No memory-related slowdowns
- **Parallel-safe**: Can run multiple instances without memory conflicts

## 🐛 Troubleshooting

### Common Issues

#### "Authentication failed"
```bash
Error: Push error: Authentication failed: ...
```
**Solution**: Verify username/password and registry permissions

#### "Cache not found"  
```bash
Error: Cache not found
```
**Solution**: Run `pull` command first to cache the image

#### "Failed to create cache directory"
```bash
Error: Cache error: Failed to create cache directory: ...
```
**Solution**: Check disk space and write permissions

#### Memory Issues (Still occurring)
If you're still experiencing memory issues:

1. **Check chunk size**: Reduce chunk size in code
2. **Monitor disk space**: Ensure sufficient space for caching
3. **Close other applications**: Free up system memory
4. **Use sequential processing**: Avoid concurrent operations

### Debug Mode

Add debug logging by setting environment variable:
```bash
RUST_LOG=debug docker-image-pusher pull nginx:latest
```

## 🤝 Contributing

### Development Setup

```bash
git clone <repository-url>
cd docker-image-pusher
cargo build
cargo test
```

### Code Structure

- `src/main.rs` - Lean CLI + shared constants (delegates to modules)
- `src/push.rs` - Push/import workflow, target inference, confirmation prompts
- `src/tar_import.rs` - Tar parsing, RepoTag helpers, import pipeline
- `src/cache.rs` - Pull and caching logic with streaming
- `src/state.rs` - Credential storage + push history tracking
- `PusherError` - Custom error type re-exported from `main.rs`

### Adding Features

1. **New authentication methods**: Extend `RegistryAuth` usage
2. **Progress bars**: Add progress indication for long transfers
3. **Compression**: Add layer compression/decompression support
4. **Parallel processing**: Implement safe concurrent layer transfers

## 📄 License

[Add your license information here]

## 🔗 Dependencies

- **oci-client**: OCI registry client with streaming support
- **tokio**: Async runtime for concurrent operations
- **clap**: Command-line argument parsing
- **serde_json**: JSON serialization for metadata
- **thiserror**: Structured error handling

## 📈 Future Enhancements

- [ ] Progress bars for long transfers
- [ ] Resume interrupted transfers
- [ ] Compression optimization
- [ ] Multi-registry synchronization
- [ ] Garbage collection for cache
- [ ] Configuration file support
- [ ] Integration with CI/CD pipelines

---

**Happy Docker image transferring! 🐳**
