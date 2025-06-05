# Docker 镜像推送工具

[![构建状态](https://github.com/yorelog/docker-image-pusher/workflows/Build%20and%20Test/badge.svg)](https://github.com/yorelog/docker-image-pusher/actions)
[![Crates.io](https://img.shields.io/crates/v/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![许可证: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![下载量](https://img.shields.io/crates/d/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)

Docker Image Pusher 是一个用 Rust 编写的高性能命令行工具，能够将 Docker 镜像 tar 包直接上传到 Docker 镜像仓库。专为企业环境和离线部署设计，通过智能分块上传和并发处理高效处理大型镜像（>10GB）。

## [🇺🇸 English Documentation](README.md)

## ✨ 核心特性

- **🚀 高性能**：多线程分块上传，支持可配置并发数
- **📦 大镜像支持**：针对大于 10GB 的镜像优化，支持断点续传
- **🔐 企业级安全**：全面的身份验证支持，包括令牌管理
- **🌐 多仓库兼容**：兼容 Docker Hub、Harbor、AWS ECR、Google GCR、Azure ACR
- **📊 进度跟踪**：实时上传进度和详细反馈
- **🛡️ 强大的错误处理**：自动重试机制和优雅的故障恢复
- **⚙️ 灵活配置**：支持环境变量、配置文件和命令行参数
- **🔄 断点续传**：自动恢复中断的上传
- **🎯 验证模式**：在不实际上传的情况下验证配置

## 🎯 使用场景

### 企业和生产环境
- **空气隔离部署**：在无法访问互联网的内网环境中传输镜像到内部仓库
- **合规要求**：满足数据主权和安全审计要求
- **边缘计算**：部署到连接受限的远程位置
- **CI/CD 流水线**：在开发和生产环境之间自动化镜像传输
- **灾难恢复**：备份和恢复关键容器镜像

## 📥 安装

### 方式 1：下载预编译二进制文件
从 [GitHub Releases](https://github.com/yorelog/docker-image-pusher/releases) 下载：

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

### 方式 2：通过 Cargo 安装
```bash
cargo install docker-image-pusher
```

### 方式 3：从源码构建
```bash
git clone https://github.com/yorelog/docker-image-pusher.git
cd docker-image-pusher
cargo build --release
# 二进制文件位于 ./target/release/docker-image-pusher
```

## 🚀 快速开始

### 基本用法
```bash
# 简单的身份验证推送
docker-image-pusher \
  --repository-url https://registry.example.com/project/app:v1.0 \
  --file /path/to/image.tar \
  --username myuser \
  --password mypassword
```

### 常见工作流程
```bash
# 1. 从 Docker 导出镜像
docker save nginx:latest -o nginx.tar

# 2. 推送到私有仓库
docker-image-pusher \
  -r https://harbor.company.com/library/nginx:latest \
  -f nginx.tar \
  -u admin \
  -p harbor_password \
  --verbose
```

## 📖 命令参考

### 核心参数

| 短参数 | 长参数 | 描述 | 必需 | 示例 |
|-------|-------|------|------|------|
| `-f` | `--file` | Docker镜像tar文件路径 | ✅ | `/path/to/image.tar` |
| `-r` | `--repository-url` | 完整的仓库URL | ✅ | `https://registry.com/app:v1.0` |
| `-u` | `--username` | 仓库用户名 | ⚠️ | `admin` |
| `-p` | `--password` | 仓库密码 | ⚠️ | `secret123` |

### 配置选项

| 短参数 | 长参数 | 描述 | 默认值 | 示例 |
|-------|--------|------|--------|------|
| `-t` | `--timeout` | 网络超时时间（秒） | `7200` | `3600` |
| | `--large-layer-threshold` | 大层阈值（字节） | `1GB` | `2147483648` |
| | `--max-concurrent` | 最大并发上传数 | `1` | `4` |
| | `--retry-attempts` | 重试次数 | `3` | `5` |

### 控制标志

| 长参数 | 描述 | 用途 |
|--------|------|------|
| `--skip-tls` | 跳过TLS证书验证 | 用于自签名证书 |
| `--verbose` | 启用详细输出 | 调试和监控 |
| `--quiet` | 抑制除错误外的所有输出 | 自动化脚本 |
| `--dry-run` | 验证模式（不实际上传） | 配置测试 |

### 高级示例

#### 大镜像优化
```bash
# 针对 15GB PyTorch 模型优化
docker-image-pusher \
  -r https://registry.example.com/ml/pytorch:latest \
  -f pytorch-15gb.tar \
  -u ml-user \
  -p $(cat ~/.registry-password) \
  --large-layer-threshold 2147483648 \    # 2GB 阈值
  --max-concurrent 4 \                   # 4 个并行上传
  --timeout 3600 \                       # 1 小时超时
  --retry-attempts 5 \                   # 5 次重试
  --verbose
```

#### 企业 Harbor 仓库
```bash
# 生产环境部署到 Harbor
docker-image-pusher \
  -r https://harbor.company.com/production/webapp:v2.1.0 \
  -f webapp-v2.1.0.tar \
  -u prod-deployer \
  -p $HARBOR_PASSWORD \
  --skip-tls \               # 用于自签名证书
  --max-concurrent 2 \       # 生产环境保守设置
  --verbose
```

#### 批处理脚本
```bash
#!/bin/bash
# 多镜像处理与错误处理
REGISTRY_BASE="https://registry.internal.com/apps"
FAILED_IMAGES=()

for tar_file in *.tar; do
  image_name=$(basename "$tar_file" .tar)
  echo "正在处理 $image_name..."
  
  if docker-image-pusher \
    -r "${REGISTRY_BASE}/${image_name}:latest" \
    -f "$tar_file" \
    -u "$REGISTRY_USER" \
    -p "$REGISTRY_PASS" \
    --retry-attempts 3 \
    --quiet; then
    echo "✅ 成功推送 $image_name"
  else
    echo "❌ 推送失败 $image_name"
    FAILED_IMAGES+=("$image_name")
  fi
done

# 报告结果
if [ ${#FAILED_IMAGES[@]} -eq 0 ]; then
  echo "🎉 所有镜像推送成功！"
else
  echo "⚠️  失败的镜像: ${FAILED_IMAGES[*]}"
  exit 1
fi
```

## 🔧 配置

### 环境变量
通过环境变量设置凭据和默认值：

```bash
# 身份验证
export DOCKER_PUSHER_USERNAME=myuser
export DOCKER_PUSHER_PASSWORD=mypassword

# 配置
export DOCKER_PUSHER_TIMEOUT=3600
export DOCKER_PUSHER_MAX_CONCURRENT=4
export DOCKER_PUSHER_SKIP_TLS=true
export DOCKER_PUSHER_VERBOSE=true

# 简化命令
docker-image-pusher -r https://registry.com/app:v1.0 -f app.tar
```

### 性能调优

#### 网络优化设置
```bash
# 适用于慢速/不稳定网络（< 10 Mbps）
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --max-concurrent 1 \       # 单连接
  --timeout 1800 \           # 30 分钟超时
  --retry-attempts 5         # 更多重试
```

#### 高速网络设置
```bash
# 适用于快速、稳定网络（> 100 Mbps）
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --max-concurrent 4 \       # 多连接
  --timeout 600 \            # 10 分钟超时
  --retry-attempts 2         # 更少重试
```

## 🏢 企业场景

### 金融服务 - 空气隔离部署
```bash
# 开发环境
docker save trading-platform:v3.2.1 -o trading-platform-v3.2.1.tar

# 生产环境（安全传输后）
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

### 制造业 - 边缘计算
```bash
# 部署到带宽受限的工厂边缘节点
docker-image-pusher \
  -r https://edge-registry.factory.com/iot/sensor-collector:v2.0 \
  -f sensor-collector.tar \
  -u edge-admin \
  -p $EDGE_PASSWORD \
  --max-concurrent 1 \       # 单连接保证稳定性
  --timeout 3600 \           # 延长超时
  --retry-attempts 10        # 高重试次数
```

### 医疗行业 - 合规环境
```bash
# HIPAA 合规的镜像部署
docker-image-pusher \
  -r https://secure-registry.hospital.com/radiology/dicom-viewer:v1.2 \
  -f dicom-viewer.tar \
  -u $(cat /secure/credentials/username) \
  -p $(cat /secure/credentials/password) \
  --skip-tls \
  --verbose \
  --dry-run                  # 先验证
```

## 🔍 故障排除

### 常见问题和解决方案

#### 身份验证失败
```bash
# 使用 dry-run 测试凭据
docker-image-pusher \
  -r https://registry.com/test/hello:v1 \
  -f hello.tar \
  -u username \
  -p password \
  --dry-run \
  --verbose
```

**常见原因：**
- 凭据过期
- 仓库权限不足
- 仓库特定的身份验证要求

#### 证书问题
```bash
# 用于自签名证书
docker-image-pusher \
  -r https://internal-registry.com/app:latest \
  -f app.tar \
  --skip-tls \
  --verbose
```

**安全提示：** 仅在可信网络中使用 `--skip-tls`。

#### 大文件上传失败
```bash
# 针对大文件的优化设置
docker-image-pusher \
  -r https://registry.com/bigapp:latest \
  -f 20gb-image.tar \
  --large-layer-threshold 1073741824 \  # 1GB 阈值
  --max-concurrent 2 \                  # 保守的并发数
  --timeout 7200 \                      # 2 小时超时
  --retry-attempts 5 \                  # 高重试次数
  --verbose
```

#### 网络超时问题
```bash
# 适用于不稳定网络
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --timeout 1800 \           # 30 分钟
  --retry-attempts 10 \      # 更多重试
  --max-concurrent 1         # 单连接
```

### 调试信息

启用详细日志获取详细信息：

```bash
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --verbose \
  2>&1 | tee upload.log
```

详细输出包括：
- 层提取进度
- 上传尝试详情
- 重试信息
- 网络时序
- 仓库响应

## 📊 性能基准

### 典型性能指标

| 镜像大小 | 网络 | 时间 | 并发数 | 设置 |
|----------|------|------|--------|------|
| 500MB | 100 Mbps | 45秒 | 2 | 默认 |
| 2GB | 100 Mbps | 3分20秒 | 4 | 优化 |
| 10GB | 1 Gbps | 8分15秒 | 4 | 高速 |
| 25GB | 100 Mbps | 45分30秒 | 2 | 大镜像 |

### 优化建议

1. **并发数**：从 2-4 个并发上传开始
2. **超时时间**：根据网络稳定性设置
3. **重试次数**：不稳定网络使用更高值
4. **大层阈值**：根据典型层大小调整

## 🤝 贡献

我们欢迎贡献！请查看我们的 [贡献指南](CONTRIBUTING.md) 了解详情。

### 开发环境设置
```bash
git clone https://github.com/yorelog/docker-image-pusher.git
cd docker-image-pusher
cargo test
cargo run -- --help
```

### 运行测试
```bash
# 单元测试
cargo test

# Docker 仓库集成测试
cargo test --test integration -- --ignored

# 性能基准测试
cargo test --release --test performance
```

### 代码质量
```bash
# 格式化代码
cargo fmt

# 运行 linter
cargo clippy

# 安全审计
cargo audit
```

## 📄 许可证

本项目采用 MIT 许可证 - 详情请参见 [LICENSE](LICENSE) 文件。

## 🆘 支持

- 📖 [文档](https://github.com/yorelog/docker-image-pusher/wiki)
- 🐛 [报告问题](https://github.com/yorelog/docker-image-pusher/issues)
- 💬 [讨论](https://github.com/yorelog/docker-image-pusher/discussions)
- 📧 邮箱: yorelog@gmail.com

## 🏆 致谢

- Docker Registry HTTP API V2 规范
- Rust 社区提供的优秀 crates
- 所有贡献者和用户的反馈

---

**⚠️ 安全提示**：在生产环境中务必使用安全的身份验证方法。考虑使用环境变量、凭据文件或安全保险库，而不是命令行参数来处理敏感信息。

**📈 性能提示**：为了获得最佳性能，请根据你的具体网络和仓库设置测试不同的并发配置。