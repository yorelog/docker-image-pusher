@echo off
setlocal enabledelayedexpansion

echo 🔧 Building Docker Image Pusher with musl target...

REM Build the container
podman build -t docker-image-pusher:musl-build .
if !errorlevel! neq 0 (
    echo ❌ Build failed!
    exit /b 1
)

echo 📦 Extracting binary from container...

REM Create a temporary container to extract the binary
for /f "tokens=*" %%i in ('podman create --name temp-extract docker-image-pusher:musl-build') do set CONTAINER_ID=%%i

REM Copy the binary out
podman cp %CONTAINER_ID%:/output/docker-image-pusher ./docker-image-pusher-linux-musl
if !errorlevel! neq 0 (
    echo ❌ Failed to extract binary!
    podman rm %CONTAINER_ID%
    exit /b 1
)

REM Clean up temporary container
podman rm %CONTAINER_ID%

echo ✅ Binary extracted to: docker-image-pusher-linux-musl

echo 🎉 Build complete! Binary ready: docker-image-pusher-linux-musl
echo.
echo Usage examples:
echo   docker-image-pusher-linux-musl import --tar-file image.tar --image-name myapp:latest
echo   docker-image-pusher-linux-musl push --registry-url https://registry.com --image-name myapp:latest

pause
