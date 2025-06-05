# Docker Image Pusher

[![Build Status](https://github.com/yorelog/docker-image-pusher/workflows/Build%20and%20Test/badge.svg)](https://github.com/yorelog/docker-image-pusher/actions)
[![Crates.io](https://img.shields.io/crates/v/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://img.shields.io/crates/d/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)

A high-performance command-line tool written in Rust for pushing Docker image tar packages directly to Docker registries. Designed for enterprise environments and offline deployments, it efficiently handles large images (>10GB) through intelligent chunked uploads and concurrent processing.

## [🇨🇳 中文文档](README_zh.md)

## ✨ Key Features

- **🚀 High Performance**: Multi-threaded chunked uploads with configurable concurrency
- **📦 Large Image Support**: Optimized for images larger than 10GB with resume capability
- **🔐 Enterprise Security**: Comprehensive authentication support including token management
- **🌐 Multi-Registry**: Compatible with Docker Hub, Harbor, AWS ECR, Google GCR, Azure ACR
- **📊 Progress Tracking**: Real-time upload progress with detailed feedback
- **🛡️ Robust Error Handling**: Automatic retry mechanisms and graceful failure recovery
- **⚙️ Flexible Configuration**: Environment variables, config files, and CLI arguments
- **🔄 Resume Support**: Resume interrupted uploads automatically
- **🎯 Dry Run Mode**: Validate configurations without actual uploads

## 🎯 Use Cases

### Enterprise & Production Environments
- **Air-Gapped Deployments**: Transfer images to internal registries without internet access
- **Security Compliance**: Meet data sovereignty and security audit requirements
- **Edge Computing**: Deploy to remote locations with limited connectivity
- **CI/CD Pipelines**: Automate image transfers between development and production environments
- **Disaster Recovery**: Backup and restore critical container images

## 📥 Installation

### Option 1: Download Pre-built Binary
Download from [GitHub Releases](https://github.com/yorelog/docker-image-pusher/releases):

```bash
# Linux x64
curl -L -o docker-image-pusher https://github.com/yorelog/docker-image-pusher/releases/latest/download/docker-image-pusher-x86_64-unknown-linux-gnu
chmod +x docker-image-pusher

# macOS Intel
curl -L -o docker-image-pusher https://github.com/yorelog/docker-image-pusher/releases/latest/download/docker-image-pusher-x86_64-apple-darwin
chmod +x docker-image-pusher

# macOS Apple Silicon  
curl -L -o docker-image-pusher https://github.com/yorelog/docker-image-pusher/releases/latest/download/docker-image-pusher-aarch64-apple-darwin
chmod +x docker-image-pusher

# Windows (PowerShell)
Invoke-WebRequest -Uri "https://github.com/yorelog/docker-image-pusher/releases/latest/download/docker-image-pusher-x86_64-pc-windows-msvc.exe" -OutFile "docker-image-pusher.exe"
```

### Option 2: Install via Cargo
```bash
cargo install docker-image-pusher
```

### Option 3: Build from Source
```bash
git clone https://github.com/yorelog/docker-image-pusher.git
cd docker-image-pusher
cargo build --release
# Binary will be at ./target/release/docker-image-pusher
```

## 🚀 Quick Start

### Basic Usage
```bash
# Simple push with authentication
docker-image-pusher \
  --repository-url https://registry.example.com/project/app:v1.0 \
  --file /path/to/image.tar \
  --username myuser \
  --password mypassword
```

### Common Workflow
```bash
# 1. Export image from Docker
docker save nginx:latest -o nginx.tar

# 2. Push to private registry
docker-image-pusher \
  -r https://harbor.company.com/library/nginx:latest \
  -f nginx.tar \
  -u admin \
  -p harbor_password \
  --verbose
```

## 📖 Command Reference

### Core Arguments

| Short | Long | Description | Required | Example |
|-------|------|-------------|----------|---------|
| `-f` | `--file` | Docker image tar file path | ✅ | `/path/to/image.tar` |
| `-r` | `--repository-url` | Full repository URL | ✅ | `https://registry.com/app:v1.0` |
| `-u` | `--username` | Registry username | ⚠️ | `admin` |
| `-p` | `--password` | Registry password | ⚠️ | `secret123` |

### Configuration Options

| Short | Long | Description | Default | Example |
|-------|------|-------------|---------|---------|
| `-t` | `--timeout` | Network timeout (seconds) | `7200` | `3600` |
|  | `--large-layer-threshold` | Large layer threshold (bytes) | `1GB` | `2147483648` |
|  | `--max-concurrent` | Maximum concurrent uploads | `1` | `4` |
|  | `--retry-attempts` | Number of retry attempts | `3` | `5` |

### Control Flags

| Long | Description | Usage |
|------|-------------|-------|
| `--skip-tls` | Skip TLS certificate verification | For self-signed certificates |
| `--verbose` | Enable detailed output | Debugging and monitoring |
| `--quiet` | Suppress all output except errors | Automated scripts |
| `--dry-run` | Validate without uploading | Configuration testing |

### Advanced Examples

#### Large Image Optimization
```bash
# Optimized for 15GB PyTorch model
docker-image-pusher \
  -r https://registry.example.com/ml/pytorch:latest \
  -f pytorch-15gb.tar \
  -u ml-user \
  -p $(cat ~/.registry-password) \
  --large-layer-threshold 2147483648 \    # 2GB threshold
  --max-concurrent 4 \                   # 4 parallel uploads  
  --timeout 3600 \                       # 1 hour timeout
  --retry-attempts 5 \                   # 5 retry attempts
  --verbose
```

#### Enterprise Harbor Registry
```bash
# Production deployment to Harbor
docker-image-pusher \
  -r https://harbor.company.com/production/webapp:v2.1.0 \
  -f webapp-v2.1.0.tar \
  -u prod-deployer \
  -p $HARBOR_PASSWORD \
  --skip-tls \               # For self-signed certificates
  --max-concurrent 2 \       # Conservative for production
  --verbose
```

#### Batch Processing Script
```bash
#!/bin/bash
# Process multiple images with error handling
REGISTRY_BASE="https://registry.internal.com/apps"
FAILED_IMAGES=()

for tar_file in *.tar; do
  image_name=$(basename "$tar_file" .tar)
  echo "Processing $image_name..."
  
  if docker-image-pusher \
    -r "${REGISTRY_BASE}/${image_name}:latest" \
    -f "$tar_file" \
    -u "$REGISTRY_USER" \
    -p "$REGISTRY_PASS" \
    --retry-attempts 3 \
    --quiet; then
    echo "✅ Successfully pushed $image_name"
  else
    echo "❌ Failed to push $image_name"
    FAILED_IMAGES+=("$image_name")
  fi
done

# Report results
if [ ${#FAILED_IMAGES[@]} -eq 0 ]; then
  echo "🎉 All images pushed successfully!"
else
  echo "⚠️  Failed images: ${FAILED_IMAGES[*]}"
  exit 1
fi
```

## 🔧 Configuration

### Environment Variables
Set credentials and defaults via environment variables:

```bash
# Authentication
export DOCKER_PUSHER_USERNAME=myuser
export DOCKER_PUSHER_PASSWORD=mypassword

# Configuration
export DOCKER_PUSHER_TIMEOUT=3600
export DOCKER_PUSHER_MAX_CONCURRENT=4
export DOCKER_PUSHER_SKIP_TLS=true
export DOCKER_PUSHER_VERBOSE=true

# Simplified command
docker-image-pusher -r https://registry.com/app:v1.0 -f app.tar
```

### Performance Tuning

#### Network-Optimized Settings
```bash
# For slow/unstable networks (< 10 Mbps)
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --max-concurrent 1 \       # Single connection
  --timeout 1800 \           # 30 minute timeout
  --retry-attempts 5         # More retries
```

#### High-Speed Network Settings
```bash
# For fast, stable networks (> 100 Mbps)
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --max-concurrent 4 \       # Multiple connections
  --timeout 600 \            # 10 minute timeout
  --retry-attempts 2         # Fewer retries needed
```

## 🏢 Enterprise Scenarios

### Financial Services - Air-Gapped Deployment
```bash
# Development environment
docker save trading-platform:v3.2.1 -o trading-platform-v3.2.1.tar

# Production environment (after secure transfer)
docker-image-pusher \
  -r https://prod-registry.bank.internal/trading/platform:v3.2.1 \
  -f trading-platform-v3.2.1.tar \
  -u prod-service \
  -p "$(vault kv get -field=password secret/registry)" \
  --skip-tls \
  --max-concurrent 2 \
  --timeout 3600 \
  --verbose
```

### Manufacturing - Edge Computing
```bash
# Deploy to factory edge nodes with limited bandwidth
docker-image-pusher \
  -r https://edge-registry.factory.com/iot/sensor-collector:v2.0 \
  -f sensor-collector.tar \
  -u edge-admin \
  -p $EDGE_PASSWORD \
  --max-concurrent 1 \       # Single connection for stability
  --timeout 3600 \           # Extended timeout
  --retry-attempts 10        # High retry count
```

### Healthcare - Compliance Environment
```bash
# HIPAA-compliant image deployment
docker-image-pusher \
  -r https://secure-registry.hospital.com/radiology/dicom-viewer:v1.2 \
  -f dicom-viewer.tar \
  -u $(cat /secure/credentials/username) \
  -p $(cat /secure/credentials/password) \
  --skip-tls \
  --verbose \
  --dry-run                  # Validate first
```

## 🔍 Troubleshooting

### Common Issues and Solutions

#### Authentication Failures
```bash
# Test credentials with dry run
docker-image-pusher \
  -r https://registry.com/test/hello:v1 \
  -f hello.tar \
  -u username \
  -p password \
  --dry-run \
  --verbose
```

**Common causes:**
- Expired credentials
- Insufficient registry permissions
- Registry-specific authentication requirements

#### Certificate Issues
```bash
# For self-signed certificates
docker-image-pusher \
  -r https://internal-registry.com/app:latest \
  -f app.tar \
  --skip-tls \
  --verbose
```

**Security note:** Only use `--skip-tls` in trusted networks.

#### Large File Upload Failures
```bash
# Optimized settings for large files
docker-image-pusher \
  -r https://registry.com/bigapp:latest \
  -f 20gb-image.tar \
  --large-layer-threshold 1073741824 \  # 1GB threshold
  --max-concurrent 2 \                  # Conservative concurrency
  --timeout 7200 \                      # 2 hour timeout
  --retry-attempts 5 \                  # High retry count
  --verbose
```

#### Network Timeout Issues
```bash
# For unstable networks
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --timeout 1800 \           # 30 minutes
  --retry-attempts 10 \      # More retries
  --max-concurrent 1         # Single connection
```

### Debug Information

Enable verbose logging to get detailed information:

```bash
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --verbose \
  2>&1 | tee upload.log
```

The verbose output includes:
- Layer extraction progress
- Upload attempt details
- Retry information
- Network timing
- Registry responses

## 📊 Performance Benchmarks

### Typical Performance Metrics

| Image Size | Network | Time | Concurrency | Settings |
|------------|---------|------|-------------|----------|
| 500MB | 100 Mbps | 45s | 2 | Default |
| 2GB | 100 Mbps | 3m 20s | 4 | Optimized |
| 10GB | 1 Gbps | 8m 15s | 4 | High-speed |
| 25GB | 100 Mbps | 45m 30s | 2 | Large image |

### Optimization Tips

1. **Concurrency**: Start with 2-4 concurrent uploads
2. **Timeouts**: Set based on your network stability
3. **Retries**: Higher for unstable networks
4. **Large Layer Threshold**: Adjust based on typical layer sizes

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

### Development Setup
```bash
git clone https://github.com/yorelog/docker-image-pusher.git
cd docker-image-pusher
cargo test
cargo run -- --help
```

### Running Tests
```bash
# Unit tests
cargo test

# Integration tests with Docker registry
cargo test --test integration -- --ignored

# Performance benchmarks
cargo test --release --test performance
```

### Code Quality
```bash
# Format code
cargo fmt

# Run linter
cargo clippy

# Security audit
cargo audit
```

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🆘 Support

- 📖 [Documentation](https://github.com/yorelog/docker-image-pusher/wiki)
- 🐛 [Report Issues](https://github.com/yorelog/docker-image-pusher/issues)
- 💬 [Discussions](https://github.com/yorelog/docker-image-pusher/discussions)
- 📧 Email: yorelog@gmail.com


## 🏆 Acknowledgments

- Docker Registry HTTP API V2 specification
- Rust community for excellent crates
- All contributors and users providing feedback

---

**⚠️ Security Notice**: Always use secure authentication methods in production. Consider using environment variables, credential files, or secure vaults instead of command-line arguments for sensitive information.

**📈 Performance Tip**: For optimal performance, test different concurrency settings with your specific network and registry setup.