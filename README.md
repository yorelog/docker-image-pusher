# Docker Image Pusher v0.2.0

[![Build Status](https://github.com/yorelog/docker-image-pusher/workflows/Build%20and%20Test/badge.svg)](https://github.com/yorelog/docker-image-pusher/actions)
[![Crates.io](https://img.shields.io/crates/v/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://img.shields.io/crates/d/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)

A **high-performance command-line tool** written in Rust for pushing Docker image tar packages directly to Docker registries. **Version 0.2.0** represents a major architectural refactoring with modernized naming conventions, simplified module structure, and improved error handling.

## [🇨🇳 中文文档](README_zh.md)

## ✨ NEW in v0.2.0 - Architecture Improvements

### 🏗️ **Modernized Architecture**
- **Unified Registry Pipeline**: Consolidated upload/download operations into a single, efficient pipeline
- **Simplified Module Structure**: Removed redundant components and streamlined codebase
- **Modern Error Handling**: Renamed `PusherError` to `RegistryError` for better semantic clarity
- **Enhanced Logging**: Renamed output system to `logging` for clearer purpose

### 🧹 **Codebase Simplification**
- **Removed Legacy Code**: Eliminated redundant upload and network modules
- **Consolidated Operations**: Single `UnifiedPipeline` replaces multiple specialized components
- **Cleaner Imports**: Updated all module paths to reflect new structure
- **Better Maintainability**: Reduced complexity while maintaining all functionality

### 🔧 **Breaking Changes (v0.2.0)**
- **Module Restructuring**: `/src/output/` → `/src/logging/`
- **Error Type Renaming**: `PusherError` → `RegistryError`
- **Component Consolidation**: Unified pipeline architecture
- **API Modernization**: Cleaner, more intuitive function signatures

## ✨ Core Features

- **🚀 High Performance**: Streaming pipeline with priority-based scheduling
- **📦 Large Image Support**: Optimized for large images with minimal memory usage
- **🔐 Enterprise Security**: Comprehensive authentication support including token management
- **🌐 Multi-Registry**: Compatible with Docker Hub, Harbor, AWS ECR, Google GCR, Azure ACR
- **📊 Real-time Progress**: Advanced progress tracking with detailed metrics
- **🛡️ Intelligent Recovery**: Smart retry mechanisms with exponential backoff
- **⚙️ Advanced Configuration**: Fine-tuned control over streaming, concurrency, and memory usage
- **🔄 Resume Support**: Resume interrupted uploads with layer-level precision
- **🎯 Dry Run Mode**: Validate configurations and test connectivity

## 🎯 Use Cases

### 🏢 **Enterprise & Production Environments**
- **🔒 Air-Gapped Deployments**: Transfer massive ML models and applications to isolated networks
- **📋 Security Compliance**: Meet data sovereignty requirements with on-premises registries
- **🌐 Edge Computing**: Deploy to remote locations with bandwidth constraints
- **🔄 CI/CD Pipelines**: High-speed image transfers in automated deployment pipelines
- **💾 Disaster Recovery**: Efficient backup of critical container images

### 🧠 **AI/ML & Big Data**
- **🤖 Model Deployment**: Push large PyTorch/TensorFlow models efficiently
- **📊 Data Science**: Transfer images with large datasets and dependencies
- **🔬 Research Computing**: Distribute complex computational environments

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

### **Basic Usage**
Simple, straightforward image pushing:

```bash
# Basic push with authentication
docker-image-pusher \
  --repository-url https://registry.example.com/project/app:v1.0 \
  --file /path/to/image.tar \
  --username myuser \
  --password mypassword \
  --verbose
```

### **Common Workflow**
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

### **Advanced Usage with Error Handling**
```bash
# Production-ready command with comprehensive error handling
docker-image-pusher \
  --repository-url https://enterprise-registry.com/production/app:v2.0 \
  --file /path/to/large-app.tar \
  --username production-user \
  --password $REGISTRY_PASSWORD \
  --timeout 3600 \
  --retry-attempts 5 \
  --skip-existing \
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
|  | `--large-layer-threshold` | Large layer threshold (bytes) | `1073741824` | `2147483648` |
|  | `--max-concurrent` | Maximum concurrent uploads | `1` | `4` |
|  | `--retry-attempts` | Number of retry attempts | `3` | `5` |

### Control Flags

| Long | Description | Usage |
|------|-------------|-------|
| `--skip-tls` | Skip TLS certificate verification | For self-signed certificates |
| `--verbose` | Enable detailed output | Debugging and monitoring |
| `--quiet` | Suppress all output except errors | Automated scripts |
| `--dry-run` | Validate without uploading | Configuration testing |
| `--skip-existing` | Skip uploading layers that already exist | Resume interrupted uploads |
| `--force-upload` | Force upload even if layers exist | Overwrite existing layers |

## 🏎️ Advanced Examples

### **Large Image Optimization**
```bash
# Optimized for large ML models (15GB PyTorch model)
docker-image-pusher \
  -r https://ml-registry.company.com/models/pytorch-model:v3.0 \
  -f large-model.tar \
  -u ml-engineer \
  -p $(cat ~/.ml-registry-token) \
  --large-layer-threshold 2147483648 \  # 2GB threshold for large layers
  --max-concurrent 4 \                  # 4 parallel uploads
  --timeout 7200 \                      # 2 hour timeout
  --retry-attempts 5 \                  # Extra retries for large uploads
  --verbose
```

### **Enterprise Harbor Registry**
```bash
# Production deployment to Harbor with comprehensive error handling
docker-image-pusher \
  -r https://harbor.company.com/production/webapp:v2.1.0 \
  -f webapp-v2.1.0.tar \
  -u prod-deployer \
  -p $HARBOR_PASSWORD \
  --skip-tls \               # For self-signed certificates
  --max-concurrent 2 \       # Conservative for production stability
  --skip-existing \          # Skip layers that already exist
  --retry-attempts 5 \       # Production-grade retry handling
  --verbose
```

### **Batch Processing Pipeline**
```bash
#!/bin/bash
# High-throughput batch processing with v0.2.0 error handling

REGISTRY="https://enterprise-registry.internal/data-science"
MAX_CONCURRENT=4
FAILED_IMAGES=()

for model_tar in models/*.tar; do
  model_name=$(basename "$model_tar" .tar)
  echo "🚀 Processing $model_name with v0.2.0 architecture..."
  
  if docker-image-pusher \
    -r "${REGISTRY}/${model_name}:latest" \
    -f "$model_tar" \
    -u "$DATA_SCIENCE_USER" \
    -p "$DATA_SCIENCE_TOKEN" \
    --max-concurrent $MAX_CONCURRENT \
    --large-layer-threshold 1073741824 \
    --timeout 3600 \
    --retry-attempts 3 \
    --skip-existing \
    --verbose; then
    echo "✅ Successfully pushed $model_name"
  else
    echo "❌ Failed to push $model_name"
    FAILED_IMAGES+=("$model_name")
  fi
done

# Report batch results
if [ ${#FAILED_IMAGES[@]} -eq 0 ]; then
  echo "🎉 All images processed successfully!"
else
  echo "⚠️  Failed images: ${FAILED_IMAGES[*]}"
  exit 1
fi
```

### **Edge Computing Deployment (Bandwidth Constrained)**
```bash
# Optimized for limited bandwidth environments
docker-image-pusher \
  -r https://edge-registry.factory.local/iot/sensor-hub:v2.1 \
  -f sensor-hub.tar \
  -u edge-deploy \
  -p $EDGE_PASSWORD \
  --max-concurrent 1 \               # Single connection for stability
  --large-layer-threshold 536870912 \ # 512MB threshold (smaller for edge)
  --timeout 3600 \                   # Extended timeout for slow networks
  --retry-attempts 10 \              # High retry count for unreliable networks
  --verbose
```

### **Multi-Architecture Deployment**
```bash
# Deploy multi-arch images efficiently with v0.2.0 skip-existing optimization
for arch in amd64 arm64 arm; do
  echo "🏗️  Deploying $arch architecture..."
  docker-image-pusher \
    -r "https://registry.company.com/multiarch/webapp:v1.0-${arch}" \
    -f "webapp-${arch}.tar" \
    -u cicd-deploy \
    -p "$CICD_TOKEN" \
    --max-concurrent 3 \
    --skip-existing \                   # Skip common base layers between architectures
    --retry-attempts 3 \
    --verbose
done
```

## 🔧 Advanced Configuration

### **Environment Variables**
Configure defaults and credentials:

```bash
# Authentication
export DOCKER_PUSHER_USERNAME=myuser
export DOCKER_PUSHER_PASSWORD=mypassword

# Performance Configuration
export DOCKER_PUSHER_MAX_CONCURRENT=4
export DOCKER_PUSHER_TIMEOUT=3600
export DOCKER_PUSHER_LARGE_LAYER_THRESHOLD=1073741824
export DOCKER_PUSHER_RETRY_ATTEMPTS=5

# Behavior Configuration
export DOCKER_PUSHER_SKIP_TLS=true
export DOCKER_PUSHER_VERBOSE=true
export DOCKER_PUSHER_SKIP_EXISTING=true

# Simplified command with env vars
docker-image-pusher -r https://registry.com/app:v1.0 -f app.tar
```

### **Performance Tuning Matrix**

#### **Network-Based Optimization**

| Network Type | Max Concurrent | Timeout | Large Layer Threshold | Retry Attempts |
|--------------|----------------|---------|----------------------|----------------|
| **Slow (< 10 Mbps)** | 1 | 3600s | 512MB | 10 |
| **Standard (10-100 Mbps)** | 2-3 | 1800s | 1GB | 5 |
| **Fast (100Mbps-1Gbps)** | 4-6 | 600s | 2GB | 3 |
| **Ultra-Fast (> 1Gbps)** | 6+ | 300s | 4GB | 2 |

#### **Image Size Optimization**

| Image Size | Max Concurrent | Timeout | Large Layer Threshold | Recommended |
|------------|----------------|---------|----------------------|-------------|
| **< 1GB** | 2 | 600s | 256MB | Standard settings |
| **1-5GB** | 3 | 1800s | 512MB | Balanced performance |
| **5-20GB** | 4 | 3600s | 1GB | High performance |
| **> 20GB** | 4-6 | 7200s | 2GB | Maximum optimization |

## 📊 Performance Benchmarks v0.2.0

## 🔍 Troubleshooting

### **Common Issues and Solutions**

#### **Performance Optimization**
```bash
# For slow upload speeds
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --max-concurrent 4 \              # Increase parallelism
  --large-layer-threshold 536870912 \ # 512MB threshold
  --verbose
```

#### **Memory Usage Optimization**
```bash
# For memory-constrained environments
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f large-app.tar \
  --max-concurrent 1 \              # Reduce parallelism
  --large-layer-threshold 268435456 \ # 256MB threshold
  --verbose
```

#### **Network Issues**
```bash
# For unstable or high-latency networks
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --max-concurrent 1 \              # Single connection for stability
  --timeout 3600 \                  # Extended timeout
  --retry-attempts 10 \             # More retries
  --verbose
```

#### **Certificate Issues**
```bash
# For self-signed certificates
docker-image-pusher \
  -r https://internal-registry.com/app:latest \
  -f app.tar \
  --skip-tls \                      # Skip TLS verification
  --verbose
```

### **Debug and Validation**
```bash
# Test configuration without uploading
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --dry-run \                       # Validate without uploading
  --verbose \
  2>&1 | tee debug.log
```

### **Resume Interrupted Uploads**
```bash
# Resume uploads that were previously interrupted
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --skip-existing \                 # Skip already uploaded layers
  --retry-attempts 5 \              # Higher retry count
  --verbose
```

## 📚 Migration from v0.1.x

### **Full Backward Compatibility**
v0.2.0 maintains **100% command-line compatibility**. All existing scripts work without changes:

```bash
# This v0.1.x command works identically in v0.2.0
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  -u user \
  -p pass
# Now uses improved v0.2.0 architecture with better error handling!
```

### **Internal Architecture Changes (No User Impact)**
The v0.2.0 refactoring includes:
- **Modernized Error Types**: `PusherError` → `RegistryError` (internal only)
- **Unified Pipeline**: Consolidated upload/download operations
- **Simplified Modules**: Removed redundant components
- **Enhanced Logging**: Better structured logging system

### **Performance Improvements Available**
```bash
# Take advantage of v0.2.0 performance optimizations
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  -u user \
  -p pass \
  --max-concurrent 4 \              # Add parallelism
  --large-layer-threshold 1073741824 \ # Optimize for large layers
  --skip-existing                   # Smart layer skipping
```

### **Enhanced Error Handling**
```bash
# Benefit from improved error handling and retry logic
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f large-app.tar \
  -u user \
  -p pass \
  --retry-attempts 5 \              # Better retry handling
  --timeout 3600 \                  # Configurable timeouts
  --verbose                         # Improved error messages
```

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

## 📝 Version History

### **v0.2.0 (2025-01-XX) - Architecture Refactoring** 🏗️
- **BREAKING**: Major module restructuring and naming improvements
- **NEW**: Unified pipeline architecture replacing redundant components
- **NEW**: Modern error handling with `RegistryError` type
- **NEW**: Enhanced logging system (renamed from output)
- **REMOVED**: Legacy upload/network modules and redundant components
- **IMPROVED**: Simplified codebase with better maintainability
- **IMPROVED**: Cleaner module structure and import paths
- **COMPATIBILITY**: Command-line interface remains fully compatible
- **PERFORMANCE**: Improved memory efficiency and error handling

#### **Breaking Changes for Library Users:**
- `PusherError` → `RegistryError`
- `crate::output::` → `crate::logging::`
- Removed legacy upload and network modules
- Simplified pipeline architecture

#### **New Project Structure:**
```
src/
├── cli/                    # Command line interface
├── error/                  # Unified error handling (RegistryError)
├── image/                  # Image parsing and caching  
├── logging/                # Logging system (renamed from output)
├── registry/               # Unified registry operations
```

### v0.1.4 (2025-06-07)
- Added support for modern Docker registry API features
- Improved error handling with clearer messages
- Enhanced compatibility with Harbor repositories
- Fixed authentication issues with private registries
- Updated dependencies to latest versions
- Performance optimizations for large image uploads

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🆘 Support

- 📖 [Documentation](https://github.com/yorelog/docker-image-pusher/wiki)
- 🐛 [Report Issues](https://github.com/yorelog/docker-image-pusher/issues)
- 💬 [Discussions](https://github.com/yorelog/docker-image-pusher/discussions)
- 📧 Email: yorelog@gmail.com

## 🏆 Acknowledgments

- Docker Registry HTTP API V2 specification
- Rust async ecosystem for enabling high-performance networking
- All contributors and users providing feedback
- Enterprise users who provided requirements for the v0.2.0 architecture

---

**⚠️ Security Notice**: Always use secure authentication methods in production. Consider using environment variables, credential files, or secure vaults instead of command-line arguments for sensitive information.

**🚀 v0.2.0 Architecture Tip**: The new unified pipeline architecture provides better error handling and performance. Monitor the verbose output to understand upload progress and optimize settings for your environment.