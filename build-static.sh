#!/bin/bash

# Build script for creating statically linked Docker Image Pusher binary
# This script builds the binary and verifies it has no external dependencies

set -e

echo "🔧 Building Docker Image Pusher with musl target..."

# Build the container
podman build -t docker-image-pusher:musl-build .

# Create a temporary container to extract the binary
echo "📦 Extracting binary from container..."
CONTAINER_ID=$(podman create --name temp-extract docker-image-pusher:musl-build)

# Copy the binary out
podman cp $CONTAINER_ID:/output/docker-image-pusher ./docker-image-pusher-linux-musl

# Clean up temporary container
podman rm $CONTAINER_ID

echo "✅ Binary extracted to: docker-image-pusher-linux-musl"

# Verify the binary
echo "🔍 Verifying binary dependencies..."
if command -v ldd >/dev/null 2>&1; then
    echo "Running ldd check:"
    ldd ./docker-image-pusher-linux-musl || echo "✅ Static binary - no dynamic dependencies found!"
else
    echo "⚠️  ldd not available, skipping dependency check"
fi

# Check file info
if command -v file >/dev/null 2>&1; then
    echo "File information:"
    file ./docker-image-pusher-linux-musl
fi

# Make executable
chmod +x ./docker-image-pusher-linux-musl

echo "🎉 Build complete! Binary ready: docker-image-pusher-linux-musl"
echo ""
echo "Usage examples:"
echo "  ./docker-image-pusher-linux-musl import --tar-file image.tar --image-name myapp:latest"
echo "  ./docker-image-pusher-linux-musl push --registry-url https://registry.com --image-name myapp:latest"
