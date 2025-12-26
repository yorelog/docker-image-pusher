# Docker Image Pusher

[English](README.md) | [简体中文](README.zh-CN.md)

[![Build Status](https://github.com/yorelog/docker-image-pusher/workflows/Build/badge.svg)](https://github.com/yorelog/docker-image-pusher/actions)
[![Crates.io](https://img.shields.io/crates/v/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![Downloads](https://img.shields.io/crates/d/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![License](https://img.shields.io/github/license/yorelog/docker-image-pusher)](https://github.com/yorelog/docker-image-pusher)

A tiny, memory‑friendly Docker/OCI image pusher for large images and constrained hosts.

Highlights:
- Streaming layer uploads with bounded memory
- Chunked uploads for large layers (auto‑adapts to registry hints)
- Clear progress reporting; resumable upload sessions
- Works with tar archives or directly from containerd
- Simple override: `docker.io/nginx:v1` + `--target gitea.corp.com/proj` -> `gitea.corp.com/proj/nginx:v1`

## 📦 Install

- Releases: download binaries from [GitHub Releases](https://github.com/yorelog/docker-image-pusher/releases)
- Crates.io: `cargo install docker-image-pusher`
- From source:
    ```bash
    git clone https://github.com/yorelog/docker-image-pusher
    cd docker-image-pusher && cargo build --release
    ```


## 📖 Usage

### Quick Start (three commands)

1. **Login (once per registry)**
    ```bash
    docker-image-pusher login registry.example.com --username user --password pass
    ```
    Credentials are saved under `.docker-image-pusher/credentials.json` and reused automatically.

2. **Save a local image to a tarball (optional)**
    ```bash
    docker-image-pusher save nginx:latest --out ./
    # produces ./nginx_latest.tar
    ```

3. **Push the tar archive**
    ```bash
    docker-image-pusher push --tar ./nginx_latest.tar
    ```
    - Use `--target` to set the exact destination. If omitted, we infer from tar metadata and (optionally) `--registry`.
    - If the destination was confirmed previously, we auto-continue after a short pause; otherwise we prompt once.

### Command Reference

| Command | When to use | Key flags |
|---------|-------------|-----------|
| `save [IMAGE ...]` | Export local images to tar/portable folder | `--out`, `--root`, `--namespace`, `--digest` |
| `push --tar <TAR>` | Upload a docker-save tar directly to a registry | `-t/--target`, `--registry`, `--username/--password`, `--blob-chunk` |
| `push --image <IMAGE>` | Push directly from containerd (no tar) | `--root`, `--namespace`, `-t/--target`, `--username/--password`, `--blob-chunk` |
| `login <registry>` | Persist credentials for future pushes | `--username`, `--password` |

### Destination overrides

- If the tar contains `docker.io/nginx:v1` and you pass `--target gitea.corp.com/project1`,
    the final destination resolves to `gitea.corp.com/project1/nginx:v1` (repo/tag taken from tar).
- If you pass a full target like `--target gitea.corp.com/project1/nginx:custom`, it is used as-is.
- `--registry` only affects inference when `--target` is not provided; it forces the registry host while keeping repo/tag from tar metadata.

## 🧭 Scenarios

### 1) Push a docker-save tar to a private registry

```bash
docker-image-pusher login harbor.xxx.com --username USER --password PASS
docker-image-pusher push --tar ./app_1.0.0.tar \
    --target harbor.xxx.com/org/app:1.0.0
```

Notes:
- Use `--target` to specify the destination (registry/org/repo:tag)
- Use `--registry harbor.xxx.com` only if a host override is needed
- `--blob-chunk` sets chunk size (MiB) for large layers

### 2) Push directly from containerd (no tar)

```bash
docker-image-pusher push \
    --root ~/.local/share/containerd \
    --namespace default \
    --image org/app:1.0.0 \
    --target harbor.xxx.com/org/app:1.0.0
```

Optional: export for offline delivery/audit, then push when needed:

```bash
docker-image-pusher save \
    --root ~/.local/share/containerd \
    --namespace default \
    --out ./export \
    org/app:1.0.0
```

Tip: First push to a new target may ask for a one‑time confirmation based on history/metadata; subsequent pushes auto‑continue.


## 📚 More

- Advanced architecture: see ARCHITECTURE.md
- Reusable OCI library: crates/oci-core/README.md

## 🤝 Contributing

PRs and issues welcome. Happy pushing! 🐳
