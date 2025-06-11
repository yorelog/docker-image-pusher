# Multi-stage build for static Rust binary
FROM registry.cn-beijing.aliyuncs.com/yoce/ubuntu:22.04 as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    musl-tools \
    musl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_DIST_SERVER="https://rsproxy.cn"
ENV RUSTUP_UPDATE_ROOT="https://rsproxy.cn/rustup"

# Install Rust toolchain with musl target
RUN curl --proto '=https' --tlsv1.2 -sSf https://rsproxy.cn/rustup-init.sh | sh -s -- -y --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"
COPY config.toml /root/.cargo/config.

# Add musl target for static linking
RUN rustup target add x86_64-unknown-linux-musl

# Set working directory
WORKDIR /app

# Copy dependency files first for better caching
COPY Cargo.toml  ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies only (this layer will be cached)
RUN cargo build --release --target x86_64-unknown-linux-musl

# Remove dummy source
RUN rm -rf src

# Copy actual source code
COPY src/ ./src/

# Build the actual application with static linking
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN cargo build --release --target x86_64-unknown-linux-musl

# Create a minimal runtime image
FROM scratch as runtime

# Copy the statically linked binary
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/docker-image-pusher /docker-image-pusher

# Set the binary as entrypoint
ENTRYPOINT ["/docker-image-pusher"]

# Build stage for extracting binary
FROM registry.cn-beijing.aliyuncs.com/yoce/ubuntu:22.04 as extractor

# Copy the binary from builder stage
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/docker-image-pusher /docker-image-pusher

# Create a directory for output
RUN mkdir -p /output

# Copy binary to output directory
RUN cp /docker-image-pusher /output/docker-image-pusher

# Make sure it's executable
RUN chmod +x /output/docker-image-pusher
