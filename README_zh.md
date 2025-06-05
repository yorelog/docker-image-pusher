# Docker 镜像推送工具

[![构建状态](https://github.com/yorelog/docker-image-pusher/workflows/Build%20and%20Test/badge.svg)](https://github.com/yorelog/docker-image-pusher/actions)
[![Crates.io](https://img.shields.io/crates/v/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![许可证: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

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

## 🎯 使用场景

### 离线和空气隔离部署
- **企业网络**：在无法访问互联网的内网环境中传输镜像到内部仓库
- **合规要求**：满足数据主权和安全审计要求
- **边缘计算**：部署到连接受限的远程位置
- **CI/CD 流水线**：在开发和生产环境之间自动化镜像传输

## 📥 安装

### 方式 1：下载预编译二进制文件
从 [GitHub Releases](https://github.com/yorelog/docker-image-pusher/releases) 下载：

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
  -r https://registry.example.com/project/app:v1.0 \
  -f /path/to/image.tar \
  -u username \
  -p password
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

### 快速参考表

| 短参数 | 长参数 | 描述 | 示例 |
|--------|--------|------|------|
| `-r` | `--repository-url` | 完整的仓库URL（必需） | `https://registry.com/project/app:v1.0` |
| `-f` | `--file` | Docker镜像tar文件路径（必需） | `/path/to/image.tar` |
| `-u` | `--username` | 仓库用户名 | `admin` |
| `-p` | `--password` | 仓库密码 | `secret123` |
| `-c` | `--chunk-size` | 上传块大小（字节） | `10485760` (10MB) |
| `-j` | `--concurrency` | 并发上传数量 | `4` |
| `-k` | `--skip-tls` | 跳过TLS证书验证 | - |
| `-v` | `--verbose` | 启用详细输出 | - |
| `-t` | `--timeout` | 网络超时时间（秒） | `300` |
| `-n` | `--dry-run` | 验证模式（不实际上传） | - |
| `-o` | `--output` | 输出格式：text/json/yaml | `json` |

### 高级示例

#### 大镜像自定义设置
```bash
docker-image-pusher \
  -r https://registry.example.com/ml/pytorch:latest \
  -f pytorch-15gb.tar \
  -u ml-user \
  -p $(cat ~/.registry-password) \
  --chunk-size 52428800 \    # 50MB 块
  --concurrency 8 \          # 8 个并行上传
  --timeout 1800 \           # 30 分钟超时
  --retry 5 \                # 失败块重试 5 次
  --verbose
```

#### 企业 Harbor 仓库
```bash
docker-image-pusher \
  -r https://harbor.company.com/production/webapp:v2.1.0 \
  -f webapp-v2.1.0.tar \
  -u prod-deployer \
  -p $HARBOR_PASSWORD \
  --registry-type harbor \
  --skip-tls \               # 用于自签名证书
  --force                    # 覆盖现有镜像
```

#### 批处理脚本
```bash
#!/bin/bash
# 处理多个镜像
for tar_file in *.tar; do
  image_name=$(basename "$tar_file" .tar)
  echo "正在处理 $image_name..."
  
  docker-image-pusher \
    -r "https://registry.internal.com/apps/${image_name}:latest" \
    -f "$tar_file" \
    -u "$REGISTRY_USER" \
    -p "$REGISTRY_PASS" \
    --output json | jq .
done
```

## 🔧 配置

### 环境变量
```bash
# 通过环境变量设置凭据
export DOCKER_PUSHER_USERNAME=myuser
export DOCKER_PUSHER_PASSWORD=mypassword
export DOCKER_PUSHER_VERBOSE=1
export DOCKER_PUSHER_SKIP_TLS=1

# 简化命令
docker-image-pusher -r https://registry.com/app:v1.0 -f app.tar
```

### 性能调优

#### 网络优化设置
```bash
# 适用于慢速/不稳定网络
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --chunk-size 2097152 \     # 2MB 块（更小）
  --concurrency 2 \          # 更少的并行连接
  --timeout 900 \            # 15 分钟超时
  --retry 10                 # 更多重试
```

#### 高速网络设置
```bash
# 适用于快速、稳定网络
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  --chunk-size 104857600 \   # 100MB 块（更大）
  --concurrency 16 \         # 更多并行连接
  --timeout 300              # 标准超时
```

## 🏢 企业场景

### 金融服务 - 空气隔离部署
```bash
# 在开发环境导出
docker save trading-platform:v3.2.1 -o trading-platform-v3.2.1.tar

# 通过安全介质传输到生产网络
# 在生产环境部署
docker-image-pusher \
  -r https://prod-registry.bank.internal/trading/platform:v3.2.1 \
  -f trading-platform-v3.2.1.tar \
  -u prod-service \
  -p "$(vault kv get -field=password secret/registry)" \
  --skip-tls \
  --registry-type harbor \
  --verbose
```

### 制造业 - 边缘计算
```bash
# 部署到工厂边缘节点
docker-image-pusher \
  -r https://edge-registry.factory.com/iot/sensor-collector:v2.0 \
  -f sensor-collector.tar \
  -u edge-admin \
  -p $EDGE_PASSWORD \
  --chunk-size 5242880 \     # 5MB 适用于有限带宽
  --timeout 1800 \           # 延长超时
  --retry 15 \               # 高重试次数
  --output json > deployment-log.json
```

## 🔍 故障排除

### 常见问题和解决方案

#### 身份验证失败
```bash
# 首先测试凭据
docker-image-pusher \
  -r https://registry.com/test/hello:v1 \
  -f hello.tar \
  -u username \
  -p password \
  --dry-run \
  --verbose
```

#### 证书问题
```bash
# 用于自签名证书
docker-image-pusher \
  -r https://internal-registry.com/app:latest \
  -f app.tar \
  --skip-tls \
  --verbose
```

#### 大文件上传失败
```bash
# 针对大文件优化
docker-image-pusher \
  -r https://registry.com/bigapp:latest \
  -f 20gb-image.tar \
  --chunk-size 10485760 \    # 10MB 块
  --concurrency 4 \          # 适中的并发数
  --timeout 3600 \           # 1 小时超时
  --retry 10 \               # 高重试次数
  --verbose
```

## 📊 输出格式

### 用于自动化的 JSON 输出
```bash
docker-image-pusher -r ... -f ... --output json | jq '
{
  status: .status,
  uploaded_bytes: .uploaded_bytes,
  total_bytes: .total_bytes,
  duration_seconds: .duration_seconds
}'
```

### 用于 CI/CD 的 YAML 输出
```bash
docker-image-pusher -r ... -f ... --output yaml > deployment-result.yaml
```

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

# 集成测试
cargo test --test integration

# 性能测试
cargo test --release --test performance
```

## 📄 许可证

本项目采用 MIT 许可证 - 详情请参见 [LICENSE](LICENSE) 文件。

## 🆘 支持

- 📖 [文档](https://github.com/yorelog/docker-image-pusher/wiki)
- 🐛 [报告问题](https://github.com/yorelog/docker-image-pusher/issues)
- 💬 [讨论](https://github.com/yorelog/docker-image-pusher/discussions)
- 📧 邮箱: yorelog@gmail.com

---

**⚠️ 安全提示**：在生产环境中务必使用安全的身份验证方法。建议使用环境变量或安全保险库存储凭据，而不是命令行参数。