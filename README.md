# Docker Image Pusher

[![Build Status](https://github.com/yorelog/docker-image-pusher/workflows/Build%20and%20Test/badge.svg)](https://github.com/yorelog/docker-image-pusher/actions)
[![Crates.io](https://img.shields.io/crates/v/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Docker Image Pusher is a high-performance command-line tool written in Rust that enables direct upload of Docker image tar packages to Docker registries. Designed for enterprise environments and offline deployments, it efficiently handles large images (>10GB) through intelligent chunked uploads and concurrent processing.

## [🇨🇳 中文文档](README_zh.md)

## ✨ Key Features

- **🚀 High Performance**: Multi-threaded chunked uploads with configurable concurrency
- **📦 Large Image Support**: Optimized for images larger than 10GB with resume capability
- **🔐 Enterprise Security**: Comprehensive authentication support including token management
- **🌐 Multi-Registry**: Compatible with Docker Hub, Harbor, AWS ECR, Google GCR, Azure ACR
- **📊 Progress Tracking**: Real-time upload progress with detailed feedback
- **🛡️ Robust Error Handling**: Automatic retry mechanisms and graceful failure recovery
- **⚙️ Flexible Configuration**: Environment variables, config files, and CLI arguments

## 🎯 Use Cases

### Offline & Air-Gapped Deployments
- **Enterprise Networks**: Transfer images to internal registries without internet access
- **Security Compliance**: Meet data sovereignty and security audit requirements
- **Edge Computing**: Deploy to remote locations with limited connectivity
- **CI/CD Pipelines**: Automate image transfers between development and production environments

## 📥 Installation

### Option 1: Download Pre-built Binary
Download from [GitHub Releases](https://github.com/yorelog/docker-image-pusher/releases):

```bash
# Linux x64
wget https://github.com/yorelog/docker-image-pusher/releases/latest/download/docker-image-pusher-x86_64-unknown-linux-gnu
chmod +x docker-image-pusher-x86_64-unknown-linux-gnu

# macOS Intel
wget https://github.com/yorelog/docker-image-pusher/releases/latest/download/docker-image-pusher-x86_64-apple-darwin
chmod +x docker-image-pusher-x86_64-apple-darwin

# macOS Apple Silicon  
wget https://github.com/yorelog/docker-image-pusher/releases/latest/download/docker-image-pusher-aarch64-apple-darwin
chmod +x docker-image-pusher-aarch64-apple-darwin
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
  -r https://registry.example.com/project/app:v1.0 \
  -f /path/to/image.tar \
  -u username \
  -p password
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

### Quick Reference Table

| Short | Long | Description | Example |
|-------|------|-------------|---------|
| `-r` | `--repository-url` | Full repository URL (required) | `https://registry.com/project/app:v1.0` |
| `-f` | `--file` | Docker image tar file path (required) | `/path/to/image.tar` |
| `-u` | `--username` | Registry username | `admin` |
| `-p` | `--password` | Registry password | `secret123` |
| `-c` | `--chunk-size` | Upload chunk size in bytes | `10485760` (10MB) |
| `-j` | `--concurrency` | Number of concurrent uploads | `4` |
| `-k` | `--skip-tls` | Skip TLS certificate verification | - |
| `-v` | `--verbose` | Enable detailed output | - |
| `-t` | `--timeout` | Network timeout in seconds | `300` |
| `-n` | `--dry-run` | Validate without uploading | - |
| `-o` | `--output` | Output format: text/json/yaml | `json` |

### Advanced Examples

#### Large Image with Custom Settings
```bash
docker-image-pusher \
  -r https://registry.example.com/ml/pytorch:latest \
  -f pytorch-15gb.tar \
  -u ml-user \
  -p $(cat ~/.registry-password) \
  --chunk-size 52428800 \    # 50MB chunks
  --concurrency 8 \          # 8 parallel uploads  
  --timeout 1800 \           # 30 minute timeout
  --retry 5 \                # Retry failed chunks 5 times
  --verbose
```

#### Enterprise Harbor Registry
```bash
docker-image-pusher \
  -r https://harbor.company.com/production/webapp:v2.1.0 \
  -f webapp-v2.1.0.tar \
  -u prod-deployer \
  -p $HARBOR_PASSWORD \
  --registry-type harbor \
  --skip-tls \               # For self-signed certificates
  --force                    # Overwrite existing image
```

#### Batch Processing Script
```bash
#!/bin/bash
# Process multiple images
for tar_file in *.tar; do
  image_name=$(basename "$tar_file" .tar)
  echo "Processing $image_name..."
  
  docker-image-pusher \
    -r "https://registry.internal.com/apps/${image_name}:latest" \
    -f "$tar_file" \
    -u "$REGISTRY_USER" \
    -p "$REGISTRY_PASS" \
    --output json | jq .
done
```

## 🔧 Configuration

### Environment Variables
```bash
# Set credentials via environment
export DOCKER_PUSHER_USERNAME=myuser
export DOCKER_PUSHER_PASSWORD=mypassword
export DOCKER_PUSHER_VERBOSE=1
export DOCKER_PUSHER_SKIP_TLS=1

# Simplified command
docker-image-pusher -r https://registry.com/app:v1.0 -f app.tar
```

### Performance Tuning

#### Network-Optimized Settings
```bash
# For slow/unstable networks
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --chunk-size 2097152 \     # 2MB chunks (smaller)
  --concurrency 2 \          # Fewer parallel connections
  --timeout 900 \            # 15 minute timeout
  --retry 10                 # More retries
```

#### High-Speed Network Settings
```bash
# For fast, stable networks
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --chunk-size 104857600 \   # 100MB chunks (larger)
  --concurrency 16 \         # More parallel connections
  --timeout 300              # Standard timeout
```

## 🏢 Enterprise Scenarios

### Financial Services - Air-Gapped Deployment
```bash
# Export in development environment
docker save trading-platform:v3.2.1 -o trading-platform-v3.2.1.tar

# Transfer via secure media to production network
# Deploy in production environment
docker-image-pusher \
  -r https://prod-registry.bank.internal/trading/platform:v3.2.1 \
  -f trading-platform-v3.2.1.tar \
  -u prod-service \
  -p "$(vault kv get -field=password secret/registry)" \
  --skip-tls \
  --registry-type harbor \
  --verbose
```

### Manufacturing - Edge Computing
```bash
# Deploy to factory edge nodes
docker-image-pusher \
  -r https://edge-registry.factory.com/iot/sensor-collector:v2.0 \
  -f sensor-collector.tar \
  -u edge-admin \
  -p $EDGE_PASSWORD \
  --chunk-size 5242880 \     # 5MB for limited bandwidth
  --timeout 1800 \           # Extended timeout
  --retry 15 \               # High retry count
  --output json > deployment-log.json
```

## 🔍 Troubleshooting

### Common Issues and Solutions

#### Authentication Failures
```bash
# Test credentials first
docker-image-pusher \
  -r https://registry.com/test/hello:v1 \
  -f hello.tar \
  -u username \
  -p password \
  --dry-run \
  --verbose
```

#### Certificate Issues
```bash
# For self-signed certificates
docker-image-pusher \
  -r https://internal-registry.com/app:latest \
  -f app.tar \
  --skip-tls \
  --verbose
```

#### Large File Upload Failures
```bash
# Optimize for large files
docker-image-pusher \
  -r https://registry.com/bigapp:latest \
  -f 20gb-image.tar \
  --chunk-size 10485760 \    # 10MB chunks
  --concurrency 4 \          # Moderate concurrency
  --timeout 3600 \           # 1 hour timeout
  --retry 10 \               # High retry count
  --verbose
```

## 📊 Output Formats

### JSON Output for Automation
```bash
docker-image-pusher -r ... -f ... --output json | jq '
{
  status: .status,
  uploaded_bytes: .uploaded_bytes,
  total_bytes: .total_bytes,
  duration_seconds: .duration_seconds
}'
```

### YAML Output for CI/CD
```bash
docker-image-pusher -r ... -f ... --output yaml > deployment-result.yaml
```

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

# Integration tests  
cargo test --test integration

# Performance tests
cargo test --release --test performance
```

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🆘 Support

- 📖 [Documentation](https://github.com/yorelog/docker-image-pusher/wiki)
- 🐛 [Report Issues](https://github.com/yorelog/docker-image-pusher/issues)
- 💬 [Discussions](https://github.com/yorelog/docker-image-pusher/discussions)
- 📧 Email: yorelog@gmail.com

---

**⚠️ Security Notice**: Always use secure authentication methods in production. Consider using environment variables or secure vaults for credentials instead of command-line arguments.