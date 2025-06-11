# Usage Examples

This document provides practical examples of using the Docker Image Pusher for various scenarios.

## 🚀 Basic Examples

### Example 1: Pull a Small Public Image

```bash
# Pull a lightweight image from Docker Hub
docker-image-pusher pull alpine:latest
```

**Expected Output:**
```
🚀 Pulling and caching image: alpine:latest
📋 Pulling image: alpine:latest
🔍 Parsed reference: docker.io/library/alpine:latest
📄 Fetching manifest...
💾 Streaming 1 layers to cache...
📦 Streaming layer 1/1: sha256:4f4fb700ef54461cfa02571...
✅ Successfully cached image with 1 layers
✅ Successfully cached image: alpine:latest
```

### Example 2: Pull a Large Application Image

```bash
# Pull a larger image (this showcases memory optimization)
docker-image-pusher pull registry.cn-beijing.aliyuncs.com/yoce/vllm-openai:v0.9.0.1
```

**Expected Output:**
```
🚀 Pulling and caching image: registry.cn-beijing.aliyuncs.com/yoce/vllm-openai:v0.9.0.1
📋 Pulling image: registry.cn-beijing.aliyuncs.com/yoce/vllm-openai:v0.9.0.1
🔍 Parsed reference: registry.cn-beijing.aliyuncs.com/yoce/vllm-openai:v0.9.0.1
📄 Fetching manifest...
💾 Streaming 15 layers to cache...
📦 Streaming layer 1/15: sha256:7b8b314ca7c1...
📦 Streaming layer 2/15: sha256:8f5dc8b2b431...
...
✅ Successfully cached image with 15 layers
✅ Successfully cached image: registry.cn-beijing.aliyuncs.com/yoce/vllm-openai:v0.9.0.1
```

## 🔄 Transfer Examples

### Example 3: Transfer to Docker Hub

```bash
# First pull the image
docker-image-pusher pull nginx:latest

# Then push to your Docker Hub account
docker-image-pusher push nginx:latest myusername/nginx:custom \
  --username myusername \
  --password mypassword
```

**Expected Output:**
```
📤 Pushing image from cache: nginx:latest -> myusername/nginx:custom
🔐 Authenticating with registry...
✅ Authentication successful!
📤 Uploading 6 cached layers with memory optimization...
📦 Uploading layer 1/6: sha256:7b8b314ca7c1... (25.4 MB)
   ✅ Successfully uploaded layer sha256:7b8b314ca7c1...
📦 Uploading layer 2/6: sha256:8f5dc8b2b431... (155.2 MB)
   🔄 Using chunked upload for large layer...
   ✅ Successfully uploaded layer sha256:8f5dc8b2b431...
...
⚙️  Uploading config: sha256:config123...
📋 Pushing manifest to registry: myusername/nginx:custom
🎉 Successfully pushed 6 layers to myusername/nginx:custom
✅ Successfully pushed image: myusername/nginx:custom
```

### Example 4: Transfer to Private Registry

```bash
# Pull from public registry
docker-image-pusher pull python:3.9-slim

# Push to private registry
docker-image-pusher push python:3.9-slim registry.company.com/python:3.9-slim \
  --username deployment-user \
  --password $DEPLOY_PASSWORD
```

## 🏢 Enterprise Scenarios

### Example 5: CI/CD Pipeline Integration

```bash
#!/bin/bash
# ci-deploy.sh - CI/CD deployment script

set -e

IMAGE_NAME="myapp:${BUILD_NUMBER}"
REGISTRY="registry.company.com"
TARGET_IMAGE="${REGISTRY}/${IMAGE_NAME}"

echo "🚀 Deploying ${IMAGE_NAME} to ${REGISTRY}"

# Pull the built image from build registry
docker-image-pusher pull "build-registry.company.com/${IMAGE_NAME}"

# Push to production registry
docker-image-pusher push \
  "build-registry.company.com/${IMAGE_NAME}" \
  "${TARGET_IMAGE}" \
  --username "${DEPLOY_USER}" \
  --password "${DEPLOY_PASSWORD}"

echo "✅ Successfully deployed ${TARGET_IMAGE}"
```

### Example 6: Multi-Region Replication

```bash
#!/bin/bash
# replicate.sh - Replicate image across regions

SOURCE_IMAGE="nginx:latest"
REGIONS=("us-east-1" "eu-west-1" "ap-southeast-1")

# Pull once
docker-image-pusher pull "${SOURCE_IMAGE}"

# Push to multiple regions
for region in "${REGIONS[@]}"; do
  echo "🌍 Replicating to ${region}"
  
  docker-image-pusher push \
    "${SOURCE_IMAGE}" \
    "${region}.registry.company.com/nginx:latest" \
    --username "${REGISTRY_USER}" \
    --password "${REGISTRY_PASSWORD}"
done
```

## 🔧 Advanced Usage

### Example 7: Using Environment Variables

```bash
# Set credentials via environment
export DOCKER_USERNAME="myuser"
export DOCKER_PASSWORD="mypassword"

# Use in script
docker-image-pusher push alpine:latest myregistry/alpine:latest \
  --username "${DOCKER_USERNAME}" \
  --password "${DOCKER_PASSWORD}"
```

### Example 8: Batch Processing

```bash
#!/bin/bash
# batch-transfer.sh - Transfer multiple images

IMAGES=(
  "nginx:latest"
  "redis:alpine"
  "postgres:13"
  "node:16-alpine"
)

TARGET_REGISTRY="registry.company.com"

for image in "${IMAGES[@]}"; do
  echo "🔄 Processing ${image}"
  
  # Extract image name and tag
  name_tag=$(echo $image | sed 's/.*\///')
  
  # Pull image
  docker-image-pusher pull "${image}"
  
  # Push to target registry
  docker-image-pusher push \
    "${image}" \
    "${TARGET_REGISTRY}/${name_tag}" \
    --username "${REGISTRY_USER}" \
    --password "${REGISTRY_PASSWORD}"
    
  echo "✅ Completed ${image}"
done
```

### Example 9: Large Image Handling

```bash
# Pull a very large ML/AI image (several GB)
docker-image-pusher pull nvidia/cuda:11.8-devel-ubuntu20.04

# The tool will automatically:
# - Process layers sequentially 
# - Use chunked reading for large layers (>100MB)
# - Add delays to prevent memory pressure
# - Stream directly to disk without loading into memory
```

**Expected behavior for large images:**
- Memory usage stays constant (~50MB) regardless of image size
- Progress shown for each layer with size information
- Automatic chunked processing for layers >100MB
- Rate limiting to prevent registry overload

## 🧪 Testing and Validation

### Example 10: Local Registry Testing

```bash
# Start local registry for testing
docker run -d -p 5000:5000 --name registry registry:2

# Test with local registry
docker-image-pusher pull alpine:latest
docker-image-pusher push alpine:latest localhost:5000/alpine:test \
  --username "" \
  --password ""

# Verify the push worked
curl http://localhost:5000/v2/alpine/tags/list
```

### Example 11: Performance Testing

```bash
#!/bin/bash
# performance-test.sh - Test with images of different sizes

echo "🧪 Performance Testing"

# Small image (~5MB)
time docker-image-pusher pull alpine:latest

# Medium image (~100MB)  
time docker-image-pusher pull nginx:latest

# Large image (~1GB+)
time docker-image-pusher pull tensorflow/tensorflow:latest

echo "✅ Performance test completed"
```

## ❗ Error Handling Examples

### Example 12: Handling Authentication Errors

```bash
# This will fail with authentication error
docker-image-pusher push alpine:latest registry.company.com/alpine:latest \
  --username wrong-user \
  --password wrong-pass

# Expected error:
# Error: Push error: Authentication failed: 401 Unauthorized
```

### Example 13: Handling Missing Cache

```bash
# This will automatically pull if not cached
docker-image-pusher push nonexistent:image registry.company.com/app:v1.0 \
  --username user \
  --password pass

# Expected behavior:
# ⚠️  Image not found in cache, pulling first...
# 🚀 Pulling and caching image: nonexistent:image
```

### Example 14: Handling Network Issues

```bash
# For unreliable networks, the tool will show detailed error information
docker-image-pusher pull registry.unreliable.com/app:latest

# May show errors like:
# Error: Pull error: Failed to stream layer sha256:abc123...: network timeout
```

## 💡 Tips and Best Practices

### Tip 1: Pre-pulling for Faster Pushes

```bash
# Pull images during off-peak hours
docker-image-pusher pull large-image:latest

# Later, push will be much faster (no re-download needed)
docker-image-pusher push large-image:latest registry.company.com/large-image:latest \
  --username user --password pass
```

### Tip 2: Disk Space Management

```bash
# Check cache size
du -sh .cache/

# Clean old cache entries manually
rm -rf .cache/old_image_name/

# Or implement automated cleanup based on timestamp in index.json
```

### Tip 3: Monitoring Progress

```bash
# For very large images, monitor in separate terminal
docker-image-pusher pull huge-image:latest &
PID=$!

# Monitor memory usage
while kill -0 $PID 2>/dev/null; do
  ps -p $PID -o pid,rss,vsz,comm
  sleep 5
done
```

---

These examples demonstrate the tool's capabilities across different use cases, from simple image transfers to complex enterprise scenarios. The memory optimization ensures consistent performance regardless of image size.
