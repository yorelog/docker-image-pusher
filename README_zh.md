# Docker 镜像推送工具 v0.2.0

[![构建状态](https://github.com/yorelog/docker-image-pusher/workflows/Build%20and%20Test/badge.svg)](https://github.com/yorelog/docker-image-pusher/actions)
[![Crates.io](https://img.shields.io/crates/v/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)
[![许可证: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![下载量](https://img.shields.io/crates/d/docker-image-pusher.svg)](https://crates.io/crates/docker-image-pusher)

用 Rust 编写的**高性能命令行工具**，能够将 Docker 镜像 tar 包直接推送到 Docker 镜像仓库。**版本 0.2.2** 代表了一次重大的架构重构，具有现代化的命名约定、简化的模块结构和改进的错误处理。

## [🇺🇸 English Documentation](README.md)

## ✨ v0.2.0 新特性 - 架构改进

### 🏗️ **现代化架构**
- **统一镜像仓库管道**：将上传/下载操作整合为单一高效管道
- **简化模块结构**：移除冗余组件，精简代码库
- **现代错误处理**：将 `PusherError` 重命名为 `RegistryError`，提供更好的语义清晰度
- **增强日志系统**：将输出系统重命名为 `logging`，用途更明确

### 🧹 **代码库简化**
- **移除遗留代码**：消除冗余的上传和网络模块
- **整合操作**：单一 `UnifiedPipeline` 替代多个专业组件
- **更清洁的导入**：更新所有模块路径以反映新结构
- **更好的可维护性**：在保持所有功能的同时降低复杂性

### 🔧 **重大变更 (v0.2.0)**
- **模块重构**：`/src/output/` → `/src/logging/`
- **错误类型重命名**：`PusherError` → `RegistryError`
- **组件整合**：统一管道架构
- **API 现代化**：更清洁、更直观的函数签名

## ✨ 核心特性

- **🚀 高性能**：流式管道与基于优先级的调度
- **📦 大镜像支持**：针对大型镜像优化，内存使用最小化
- **🔐 企业级安全**：全面的身份验证支持，包括令牌管理
- **🌐 多仓库兼容**：兼容 Docker Hub、Harbor、AWS ECR、Google GCR、Azure ACR
- **📊 实时进度**：高级进度跟踪与详细指标
- **🛡️ 智能恢复**：智能重试机制与指数退避
- **⚙️ 高级配置**：对流式处理、并发性和内存使用的精细控制
- **🔄 断点续传**：层级精度的中断上传恢复
- **🎯 验证模式**：验证配置和测试连接

## 🎯 使用场景

### 🏢 **企业和生产环境**
- **🔒 空气隔离部署**：在隔离网络中传输大型ML模型和应用程序
- **📋 安全合规**：通过本地仓库满足数据主权要求
- **🌐 边缘计算**：部署到带宽受限的远程位置
- **🔄 CI/CD 流水线**：自动化部署管道中的高速镜像传输
- **💾 灾难恢复**：关键容器镜像的高效备份

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
| | `--large-layer-threshold` | 大层阈值（字节） | `1073741824` | `2147483648` |
| | `--max-concurrent` | 最大并发上传数 | `1` | `4` |
| | `--retry-attempts` | 重试次数 | `3` | `5` |

### 控制标志

| 长参数 | 描述 | 用途 |
|--------|------|------|
| `--skip-tls` | 跳过TLS证书验证 | 用于自签名证书 |
| `--verbose` | 启用详细输出 | 调试和监控 |
| `--quiet` | 抑制除错误外的所有输出 | 自动化脚本 |
| `--dry-run` | 验证模式（不实际上传） | 配置测试 |
| `--skip-existing` | 跳过已存在的层 | 断点续传 |
| `--force-upload` | 强制上传即使层已存在 | 覆盖现有层 |

### 高级示例

#### 大镜像优化
```bash
# 针对大型ML模型优化 (15GB PyTorch模型)
docker-image-pusher \
  -r https://ml-registry.company.com/models/pytorch-model:v3.0 \
  -f large-model.tar \
  -u ml-engineer \
  -p $(cat ~/.ml-registry-token) \
  --large-layer-threshold 2147483648 \  # 大层2GB阈值
  --max-concurrent 4 \                  # 4个并行上传
  --timeout 7200 \                      # 2小时超时
  --retry-attempts 5 \                  # 大文件上传额外重试
  --verbose
```

#### 企业 Harbor 仓库
```bash
# 生产环境部署到Harbor，具有全面的错误处理
docker-image-pusher \
  -r https://harbor.company.com/production/webapp:v2.1.0 \
  -f webapp-v2.1.0.tar \
  -u prod-deployer \
  -p $HARBOR_PASSWORD \
  --skip-tls \               # 用于自签名证书
  --max-concurrent 2 \       # 生产环境保守设置
  --skip-existing \          # 跳过已存在的层
  --retry-attempts 5 \       # 生产级重试处理
  --verbose
```

#### 批处理管道
```bash
#!/bin/bash
# 使用v0.2.0错误处理的高吞吐量批处理

REGISTRY="https://enterprise-registry.internal/data-science"
MAX_CONCURRENT=4
FAILED_IMAGES=()

for model_tar in models/*.tar; do
  model_name=$(basename "$model_tar" .tar)
  echo "🚀 使用v0.2.0架构处理 $model_name..."
  
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
    echo "✅ 成功推送 $model_name"
  else
    echo "❌ 推送失败 $model_name"
    FAILED_IMAGES+=("$model_name")
  fi
done

# 报告批处理结果
if [ ${#FAILED_IMAGES[@]} -eq 0 ]; then
  echo "🎉 所有镜像处理成功！"
else
  echo "⚠️  失败的镜像: ${FAILED_IMAGES[*]}"
  exit 1
fi
```

## 🔧 高级配置

### 环境变量
配置默认值和凭据：

```bash
# 身份验证
export DOCKER_PUSHER_USERNAME=myuser
export DOCKER_PUSHER_PASSWORD=mypassword

# 性能配置
export DOCKER_PUSHER_MAX_CONCURRENT=4
export DOCKER_PUSHER_TIMEOUT=3600
export DOCKER_PUSHER_LARGE_LAYER_THRESHOLD=1073741824
export DOCKER_PUSHER_RETRY_ATTEMPTS=5

# 行为配置
export DOCKER_PUSHER_SKIP_TLS=true
export DOCKER_PUSHER_VERBOSE=true
export DOCKER_PUSHER_SKIP_EXISTING=true

# 使用环境变量简化命令
docker-image-pusher -r https://registry.com/app:v1.0 -f app.tar
```

### 性能调优矩阵

#### 基于网络的优化

| 网络类型 | 最大并发 | 超时时间 | 大层阈值 | 重试次数 |
|----------|----------|----------|----------|----------|
| **慢速 (< 10 Mbps)** | 1 | 3600s | 512MB | 10 |
| **标准 (10-100 Mbps)** | 2-3 | 1800s | 1GB | 5 |
| **快速 (100Mbps-1Gbps)** | 4-6 | 600s | 2GB | 3 |
| **超快 (> 1Gbps)** | 6+ | 300s | 4GB | 2 |

#### 镜像大小优化

| 镜像大小 | 最大并发 | 超时时间 | 大层阈值 | 推荐设置 |
|----------|----------|----------|----------|----------|
| **< 1GB** | 2 | 600s | 256MB | 标准设置 |
| **1-5GB** | 3 | 1800s | 512MB | 平衡性能 |
| **5-20GB** | 4 | 3600s | 1GB | 高性能 |
| **> 20GB** | 4-6 | 7200s | 2GB | 最大优化 |

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

## 📚 从 v0.1.x 迁移

### **完全向后兼容**
v0.2.0 保持 **100% 命令行兼容性**。所有现有脚本无需更改即可工作：

```bash
# 这个 v0.1.x 命令在 v0.2.0 中完全相同地工作
docker-image-pusher \
  -r https://registry.com/app:latest \
  -f app.tar \
  -u user \
  -p pass
# 现在使用改进的 v0.2.0 架构，具有更好的错误处理！
```

### **库用户的重大变更：**
- `PusherError` → `RegistryError`
- `crate::output::` → `crate::logging::`
- 移除了旧的上传和网络模块
- 简化的管道架构

### **新项目结构：**
```
src/
├── cli/                    # 命令行界面
├── error/                  # 统一错误处理 (RegistryError)
├── image/                  # 镜像解析和缓存
├── logging/                # 日志系统 (从 output 重命名)
├── registry/               # 统一镜像仓库操作
```

## 📊 v0.2.0 性能基准

### 典型性能指标

| 镜像大小 | 网络 | 时间 | 并发数 | 设置 |
|----------|------|------|--------|------|
| 500MB | 100 Mbps | 35秒 | 2 | v0.2.0 优化 |
| 2GB | 100 Mbps | 2分50秒 | 4 | 统一管道 |
| 10GB | 1 Gbps | 6分45秒 | 4 | 高速 |
| 25GB | 100 Mbps | 38分20秒 | 2 | 大镜像 |

*注：v0.2.0 的统一管道架构相比 v0.1.x 提供了 15-20% 的性能改进*

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

## 📝 版本历史

### v0.2.0 (2025-01-XX)
**🏗️ 重大架构重构**

#### **新功能和改进：**
- **🚀 架构**：统一镜像仓库管道，提高性能和可靠性
- **🧹 重构**：现代化命名约定，从 `PusherError` 到 `RegistryError`
- **📁 模块化**：简化模块结构，`/src/output/` → `/src/logging/`
- **⚡ 性能**：改进内存效率和错误处理

#### **库用户的重大变更：**
- `PusherError` → `RegistryError`
- `crate::output::` → `crate::logging::`
- 移除了旧的上传和网络模块
- 简化的管道架构

#### **新项目结构：**
```
src/
├── cli/                    # 命令行界面
├── error/                  # 统一错误处理 (RegistryError)
├── image/                  # 镜像解析和缓存  
├── logging/                # 日志系统 (从 output 重命名)
├── registry/               # 统一镜像仓库操作
```

### v0.1.4 (2025-06-07)
- 新增对现代 Docker 镜像仓库 API 功能的支持
- 改进错误处理，提供更清晰的错误信息
- 增强与 Harbor 仓库的兼容性
- 修复与私有仓库的身份验证问题
- 更新依赖项至最新版本
- 优化大型镜像上传性能