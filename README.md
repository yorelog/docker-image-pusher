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

## 🆕 What's New in 0.5.4

- **Pipeline lives in `oci-core`** – The concurrent extraction/upload queue, blob-existence checks, rate limiting, and telemetry now ship inside the reusable `oci-core::blobs` module. Other projects can embed the exact same uploader without copy/paste.
- **Prefetch-aware chunk uploads** – Large layers read an initial chunk into memory before network I/O begins, giving registries a steady stream immediately and honoring any server-provided chunk size hints mid-flight.
- **Tar importer emits shared `LocalLayer` structs** – `tar_import.rs` now returns the exact structs consumed by `LayerUploadPool`, eliminating adapter code and reducing memory copies during extraction.
- **Cleaner push workflow** – `src/push.rs` delegates scheduling to `LayerUploadPool`, so the CLI only worries about plan setup, manifest publishing, and user prompts. The parallelism cap and chunk sizing still respect the same CLI flags as before.
- **Docs caught up** – This README now documents the pipeline-focused architecture, the new reusable uploader, and the 0.5.4 feature set.

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

### Quick Start (three commands)

1. **Login (once per registry)**
    ```bash
    docker-image-pusher login registry.example.com --username user --password pass
    ```
    Credentials are saved under `.docker-image-pusher/credentials.json` and reused automatically.

2. **Save a local image to a tarball**
    ```bash
    docker-image-pusher save nginx:latest
    ```
    - Detects Docker/nerdctl/Podman automatically (or pass `--runtime`).
    - Prompts for image selection if you omit arguments.
    - Produces a sanitized tar such as `./nginx_latest.tar`.

3. **Push the tar archive**
    ```bash
    docker-image-pusher push ./nginx_latest.tar
    ```
    - The RepoTag embedded in the tar is combined with the most recent registry you authenticated against (or `--target/--registry` overrides).
    - If the destination image was confirmed previously, we auto-continue after a short pause; otherwise we prompt before uploading.

### Command Reference

| Command | When to use | Key flags |
|---------|-------------|-----------|
| `save [IMAGE ...]` | Export one or more local images to tar archives | `--runtime`, `--output-dir`, `--force` |
| `push <tar>` | Upload a docker-save tar archive directly to a registry | `-t/--target`, `--registry`, `--username/--password`, `--blob-chunk` |
| `login <registry>` | Persist credentials for future pushes | `--username`, `--password` |

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

### State Directory

Credential material and push history are stored under `.docker-image-pusher/`:

```
.docker-image-pusher/
├── credentials.json   # registry → username/password pairs from `login`
└── push_history.json  # most recent destinations (used for inference/prompts)
```

Tar archives produced by `save` live wherever you choose to write them (current directory by default). They remain ordinary `docker save` outputs, so you can transfer them, scan them, or delete them independently of the CLI state.

### Processing Flow

#### Save Operation (runtime → tar):
1. **Runtime detection** – locate Docker, nerdctl, or Podman (or honor `--runtime`).
2. **Image selection** – parse JSON output from `images --format '{{json .}}'` and optionally prompt.
3. **Tar export** – call `<runtime> save image -o file.tar`, sanitizing filenames and warning before overwrites.

#### Push Operation (tar → registry):
1. **Authenticate** – load stored credentials or prompt for overrides.
2. **Tar analysis** – extract RepoTags + manifest to infer the final destination.
3. **Layer extraction** – stream each layer from the tar into temporary files while hashing and reporting progress.
4. **Layer/config upload** – reuse existing blobs when present, otherwise stream in fixed-size chunks with telemetry.
5. **Manifest publish** – rebuild the OCI manifest and push it once all blobs are present.

### Layer Processing Strategies

| Layer Size | Strategy | Memory Usage | Description |
|------------|----------|--------------|-------------|
| < 100MB | Direct Read | ~Layer Size | Read entire layer into memory |
| > 100MB | Chunked Read | ~50MB | Read in 50MB chunks with delays |
| Any Size | Streaming | ~Buffer Size | Direct stream to/from disk |
| Pipeline | Parallel Uploads | ~Buffer Size per worker | Extraction publishes layers into an async queue while up to 3 concurrent upload tasks push blobs |

Layer extraction now feeds an async channel as soon as each blob hits disk, so uploading overlaps with the remaining tar processing. The default concurrency spins up three upload tasks (tunable in code) to take advantage of multi-core hosts and higher latency links, while still honoring the sequential manifest ordering when publishing.

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

#### "No local images detected via <runtime>"
```bash
Error: No local images detected via docker
```
**Solution**: Ensure the image exists locally (e.g., `docker images`) or pass it explicitly to `save`.

#### "Failed to create state directory"
```bash
Error: Cache error: Failed to create state directory ...
```
**Solution**: Verify you have write access to the current working directory (or set `STATE_DIR` via environment variables, if you relocate it in code).

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
