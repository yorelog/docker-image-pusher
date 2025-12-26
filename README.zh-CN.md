# Docker Image Pusher

[English](README.md) | 简体中文

[![Build Status](https://github.com/yorelog/docker-image-pusher/workflows/Build/badge.svg)](https://github.com/yorelog/docker-image-pusher/actions)
[![Crates.io](https://img.shields.io/crates/v/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![Downloads](https://img.shields.io/crates/d/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![License](https://img.shields.io/github/license/yorelog/docker-image-pusher)](https://github.com/yorelog/docker-image-pusher)

一个轻量、对大镜像与低配主机友好的 Docker/OCI 镜像推送工具。

亮点：
- 全程流式上传，内存占用稳定可控
- 大层分片上传（可按注册表提示自适应）
- 清晰的进度展示，上传会话可恢复
- 既支持 tar 包，也支持直接从 containerd 推送
- 目标覆盖超直观：`docker.io/nginx:v1` + `--target gitea.corp.com/proj` -> `gitea.corp.com/proj/nginx:v1`

库复用：见 crates/oci-core（MIT）。提供 `reference`、`auth` 与异步 `client` 等 OCI 基础能力。

## 🛠️ 安装

- 发布版：从 GitHub Releases 下载对应平台二进制
- crates.io：`cargo install docker-image-pusher`
- 源码构建：
  ```bash
  git clone https://github.com/yorelog/docker-image-pusher
  cd docker-image-pusher && cargo build --release
  ```

## 🛠️ 安装

### 预编译二进制（推荐）

到 [GitHub Releases](https://github.com/yorelog/docker-image-pusher/releases) 下载对应平台的已编译二进制：

- `docker-image-pusher-linux-x86_64` - Linux 64-bit
- `docker-image-pusher-macos-x86_64` - macOS Intel
- `docker-image-pusher-macos-aarch64` - macOS Apple Silicon (M1/M2)
- `docker-image-pusher-windows-x86_64.exe` - Windows 64-bit

安装示例：

```bash
# Linux/macOS
chmod +x docker-image-pusher-*
sudo mv docker-image-pusher-* /usr/local/bin/docker-image-pusher

# Windows: 将 exe 放入 PATH，必要时重命名为 docker-image-pusher.exe
```

二进制位于 `target/release/docker-image-pusher`。

## 📖 使用

### 快速上手（三步）

1. 登录（每个注册表一次）
    ```bash
    docker-image-pusher login registry.example.com --username user --password pass
    ```
    凭证保存在 `.docker-image-pusher/credentials.json`，后续自动复用。

2. 保存镜像为 tar（可选，亦可直接从 containerd 推）
    ```bash
    docker-image-pusher save nginx:latest --out ./
    # 生成 ./nginx_latest.tar
    ```

3. 推送 tar 到仓库
    ```bash
    docker-image-pusher push --tar ./nginx_latest.tar
    ```
    - 若未指定 `--target`，工具将基于 tar 元数据与最近推送历史推断目标，并在必要时进行一次确认。

### 命令参考

| 命令 | 适用场景 | 关键参数 |
|------|----------|----------|
| `save [IMAGE ...]` | 导出本地镜像到 tar 或可交付目录 | `--out`, `--root`, `--namespace`, `--digest` |
| `push --tar <TAR>` | 将 docker-save 的 tar 直接推送到注册表 | `-t/--target`, `--registry`, `--username/--password`, `--blob-chunk` |
| `push --image <IMAGE>` | 直接从 containerd 推送到注册表（无需生成 tar） | `--root`, `--namespace`, `-t/--target`, `--username/--password`, `--blob-chunk` |
| `login <REGISTRY>` | 保存注册表凭证，供后续复用 | `--username`, `--password` |

### 目标覆盖规则（overrides）

- 若 tar 中包含 `docker.io/nginx:v1`，并传入 `--target gitea.corp.com/project1`，
  最终目标将解析为 `gitea.corp.com/project1/nginx:v1`（仓库名与 tag 来自 tar 元数据）。
- 若传入完整目标 `--target gitea.corp.com/project1/nginx:custom`，则按该目标原样使用。
- 未提供 `--target` 时，`--registry` 仅用于推断目标时覆盖注册表主机；仓库路径与 tag 仍取自 tar 元数据。

说明：`push` 会尽可能自动完成推断与确认；传入 `--username/--password` 将覆盖已保存的凭证。

## 🧭 使用场景（便于镜像交付）

### 1) 将 docker-save 的 tar 包推送到私有仓库

前置：先登录保存凭证（或在命令行显式传入 `--username/--password`）

```bash
docker-image-pusher login harbor.xxx.com --username USER --password PASS

# 将本地 tar 推送到私有仓库
docker-image-pusher push --tar ./app_1.0.0.tar \
  --target harbor.xxx.com/org/app:1.0.0
```

要点：
- 使用 `--target` 指定目标镜像（域名/组织/仓库:tag）。
- 如需强制覆盖注册表主机，可加 `--registry harbor.xxx.com`（通常不需要）。
- `--blob-chunk`（MiB）可调整大层分片大小。

### 2) 直接从 containerd 推送到私有仓库（无需中间 tar）

```bash
docker-image-pusher push \
  --root ~/.local/share/containerd \
  --namespace default \
  --image org/app:1.0.0 \
  --target harbor.xxx.com/org/app:1.0.0
```

可选：先“导出用于交付/审计”，再按需推送（导出与推送互不依赖）：

```bash
# 导出 containerd 中的镜像内容到可移交目录
docker-image-pusher save \
  --root ~/.local/share/containerd \
  --namespace default \
  --out ./export \
  org/app:1.0.0
```

小贴士：首次推送到某个新目标时，工具会基于历史与 tar 元数据进行合理推断，并在必要时进行一次确认，之后会记住你的选择以便下次自动继续。

## 📚 更多

- 详细架构：ARCHITECTURE.md
- 可复用的 OCI 库：crates/oci-core/README.md

## 🤝 参与贡献

欢迎提交 PR/Issue，一起让镜像传输更轻、更稳、更好用！🐳
