# Docker 镜像推送工具

Docker Image Pusher 是一个用 Rust 编写的命令行工具，允许用户将 Docker 镜像 tar 包直接推送到 Docker 镜像仓库。该工具专为高效处理大型镜像而设计，包括超过 10GB 的镜像，通过分块上传确保传输的稳定性和可靠性。

## 🎯 适用场景

### 离线环境部署
- **内网环境**：在无法访问外网的企业内网环境中，需要将镜像从外网传输到内网私有仓库
- **空气隙环境**：在完全隔离的安全环境中，通过物理介质（U盘、移动硬盘）传输镜像
- **边缘计算**：在网络条件受限的边缘节点部署应用
- **生产环境隔离**：将开发/测试环境的镜像安全传输到生产环境

### 镜像离线拷贝
- **跨云迁移**：在不同云服务商之间迁移容器化应用
- **备份恢复**：创建镜像备份并在需要时快速恢复
- **版本管理**：离线存储和管理特定版本的镜像
- **合规要求**：满足数据不出境或安全审计要求的镜像传输

## ✨ 功能特性

- **分块上传**：支持大型 Docker 镜像的分块上传，确保上传过程的稳定性和可靠性
- **Docker Registry API 交互**：直接与 Docker 镜像仓库 API 交互，实现无缝镜像上传
- **身份验证支持**：处理与 Docker 镜像仓库的身份验证，包括令牌获取和会话管理
- **进度跟踪**：提供实时的上传进度反馈
- **多种镜像仓库支持**：支持 Docker Hub、Harbor、AWS ECR、Google GCR 等主流镜像仓库
- **断点续传**：网络中断时支持断点续传，提高大文件传输成功率
- **并发上传**：支持多线程并发上传，提升传输效率
- **TLS 验证**：支持跳过 TLS 验证，适用于自签名证书的私有仓库

## 🔧 安装

### 预编译二进制文件

从 [发布页面](https://github.com/yorelog/docker-image-pusher/releases) 下载最新版本。

### 从源码构建

确保您已安装 Rust 和 Cargo，然后运行以下命令：

```bash
git clone https://github.com/yorelog/docker-image-pusher.git
cd docker-image-pusher
cargo build --release
```

## 🚀 使用方法

### 基本用法

```bash
# 推送镜像到私有仓库
docker-image-pusher \
  -r https://your-registry.com/project/app:v1.0 \
  -f /path/to/your-image.tar \
  -u your-username \
  -p your-password
```

### 离线部署典型流程

#### 1. 在有网络的环境中导出镜像

```bash
# 拉取镜像
docker pull nginx:latest

# 导出为 tar 文件
docker save nginx:latest -o nginx-latest.tar
```

#### 2. 传输到离线环境

通过物理介质（U盘、移动硬盘）或内网文件传输工具将 tar 文件传输到目标环境。

#### 3. 在离线环境中推送到私有仓库

```bash
# 推送到内网 Harbor 仓库
docker-image-pusher \
  -r https://harbor.internal.com/library/nginx:latest \
  -f nginx-latest.tar \
  -u admin \
  -p Harbor12345 \
  --skip-tls
```

### 高级用法

#### 批量镜像处理

```bash
# 使用脚本批量处理多个镜像
for tar_file in *.tar; do
  image_name=$(basename "$tar_file" .tar)
  docker-image-pusher \
    -r "https://registry.internal.com/library/${image_name}:latest" \
    -f "$tar_file" \
    -u "$REGISTRY_USER" \
    -p "$REGISTRY_PASS" \
    -v
done
```

#### 大镜像优化上传

```bash
# 针对大镜像调整参数
docker-image-pusher \
  -r https://registry.example.com/bigdata/spark:3.2.0 \
  -f spark-3.2.0.tar \
  -u username \
  -p password \
  --chunk-size 52428800 \    # 50MB 块大小
  --concurrency 8 \          # 8 个并发连接
  --timeout 1800 \           # 30 分钟超时
  --retry 5                  # 重试 5 次
```

#### 干运行验证

```bash
# 验证配置但不实际上传
docker-image-pusher \
  -r https://registry.example.com/test/app:v1.0 \
  -f app.tar \
  -u username \
  -p password \
  --dry-run \
  --verbose
```

## 📋 命令行参数

### 短参数对照表

| 短参数 | 长参数 | 描述 | 示例 |
|--------|--------|------|------|
| `-r` | `--repository-url` | 完整的仓库URL（必需） | `https://harbor.com/project/app:v1.0` |
| `-f` | `--file` | Docker镜像tar文件路径（必需） | `/path/to/image.tar` |
| `-u` | `--username` | 仓库用户名 | `admin` |
| `-p` | `--password` | 仓库密码 | `password123` |
| `-c` | `--chunk-size` | 分块大小（字节） | `10485760` (10MB) |
| `-j` | `--concurrency` | 并发连接数 | `4` |
| `-k` | `--skip-tls` | 跳过TLS验证 | - |
| `-v` | `--verbose` | 详细输出 | - |
| `-t` | `--timeout` | 超时时间（秒） | `300` |
| `-n` | `--dry-run` | 干运行模式 | - |
| `-o` | `--output` | 输出格式 | `json`, `yaml`, `text` |

### 环境变量支持

```bash
export DOCKER_PUSHER_USERNAME=myuser
export DOCKER_PUSHER_PASSWORD=mypassword
export DOCKER_PUSHER_VERBOSE=1
export DOCKER_PUSHER_SKIP_TLS=1

# 然后可以简化命令
docker-image-pusher -r https://registry.com/app:v1.0 -f app.tar
```

## 🏢 企业级应用场景

### 场景1：金融行业离线部署

```bash
# 在外网开发环境导出
docker save trading-system:v2.1.0 -o trading-system-v2.1.0.tar

# 通过安全审计后，在生产内网部署
docker-image-pusher \
  -r https://prod-harbor.bank.com/trading/trading-system:v2.1.0 \
  -f trading-system-v2.1.0.tar \
  -u prod-admin \
  -p "$(cat /secure/registry-password)" \
  --skip-tls \
  --verbose
```

### 场景2：制造业边缘计算

```bash
# 工厂边缘节点部署
docker-image-pusher \
  -r https://edge-registry.factory.com/iot/sensor-collector:v1.5 \
  -f sensor-collector-v1.5.tar \
  -u edge-user \
  -p edge-pass \
  --chunk-size 5242880 \  # 网络条件差，使用小块
  --timeout 1800 \        # 延长超时时间
  --retry 10              # 增加重试次数
```

### 场景3：多云环境镜像迁移

```bash
# 从 AWS ECR 迁移到阿里云 ACR
docker-image-pusher \
  -r https://registry.cn-hangzhou.aliyuncs.com/namespace/app:v1.0 \
  -f app-from-aws.tar \
  -u aliyun-username \
  -p aliyun-password \
  --output json | jq .    # JSON 格式输出便于脚本处理
```

## 🔍 故障排除

### 常见问题

#### 1. 认证失败
```bash
# 检查凭据和仓库权限
docker-image-pusher -r https://registry.com/test/hello:v1 -f hello.tar -u user -p pass --dry-run -v
```

#### 2. 网络超时
```bash
# 增加超时时间和重试次数
docker-image-pusher -r ... -f ... --timeout 1800 --retry 10
```

#### 3. TLS 证书问题
```bash
# 跳过 TLS 验证（仅限内网环境）
docker-image-pusher -r ... -f ... --skip-tls
```

#### 4. 大文件上传失败
```bash
# 减小块大小，增加并发
docker-image-pusher -r ... -f ... --chunk-size 2097152 --concurrency 2
```

## 🤝 贡献

欢迎贡献代码！请在 [GitHub 仓库](https://github.com/yorelog/docker-image-pusher) 中提交问题或拉取请求。

### 开发环境设置

```bash
git clone https://github.com/yorelog/docker-image-pusher.git
cd docker-image-pusher
cargo test
cargo run -- --help
```

## 📄 许可证

本项目采用 MIT 许可证。详情请参见 LICENSE 文件。

## 📞 支持

如果您在使用过程中遇到问题，可以通过以下方式获取帮助：

- 查看 [GitHub Issues](https://github.com/yorelog/docker-image-pusher/issues)
- 提交新的 Issue
- 查看文档和示例

---

**注意**：在生产环境中使用时，请确保遵循您组织的安全策略和最佳实践。建议在测试环境中充分验证后再部署到生产环境。