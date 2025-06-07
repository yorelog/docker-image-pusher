# Docker 镜像推送工具 - 设计文档

## 功能概述

Docker 镜像推送工具实现以下核心功能：

1. **从 repository 拉取镜像** - 拉取 manifest 和 blob 并本地缓存
2. **从 tar 文件提取镜像** - 提取 manifest 和 blob 并本地缓存
3. **从缓存推送镜像（使用manifest）** - 指定 manifest 从缓存推送镜像
4. **从缓存推送镜像（使用tar）** - 指定 tar 文件从缓存推送镜像

## 4种操作模式

### 模式1: PullAndCache
从远程registry拉取镜像并缓存到本地
- 输入：repository, reference, registry地址, 认证信息
- 流程：认证 → 拉取manifest → 拉取config blob → 拉取所有layer blobs → 保存到缓存
- 输出：缓存中的完整镜像

### 模式2: ExtractAndCache  
从tar文件提取镜像并缓存到本地
- 输入：tar文件路径, repository, reference
- 流程：解析tar → 提取manifest.json → 提取config → 提取所有layers → 保存到缓存
- 输出：缓存中的完整镜像

### 模式3: PushFromCacheUsingManifest
从缓存推送镜像到远程registry（基于manifest）
- 输入：repository, reference, 目标registry, 认证信息
- 流程：从缓存读取manifest → 从缓存读取所有blobs → 认证 → 推送blobs → 推送manifest
- 输出：远程registry中的镜像

### 模式4: PushFromCacheUsingTar
从缓存推送镜像到远程registry（基于tar）
- 输入：repository, reference, 目标registry, 认证信息  
- 流程：与模式3相同（因为缓存格式统一）
- 输出：远程registry中的镜像

## 缓存设计

### 目录结构

```
.cache/
  manifests/
    {repository}/
      {reference}          # manifest 文件
  blobs/
    sha256/
      {digest}             # blob 文件
  index.json               # 缓存索引
```

### 索引文件格式

`index.json` 文件使用以下 JSON 格式：

```json
{
  "{repository}/{reference}": {
    "repository": "string",
    "reference": "string", 
    "manifest_path": "string",
    "config_digest": "string",
    "blobs": {
      "sha256:{digest}": {
        "digest": "string",
        "size": number,
        "path": "string",
        "is_config": boolean,
        "compressed": boolean,
        "media_type": "string"
      }
    }
  }
}
```

## 核心组件

### ImageManager
综合镜像管理器，协调所有操作模式：
```rust
pub struct ImageManager {
    cache: Cache,
    output: OutputManager,
}

impl ImageManager {
    pub async fn execute_operation(
        &mut self,
        mode: &OperationMode,
        client: Option<&RegistryClient>,
        auth_token: Option<&str>,
    ) -> Result<()>
}
```

### Cache
本地缓存管理，符合Docker Registry API规范：
```rust
pub struct Cache {
    cache_dir: PathBuf,
    index: HashMap<String, CacheEntry>,
}
```

### RegistryClient  
Registry客户端，处理所有网络操作：
```rust
pub struct RegistryClient {
    client: Client,
    auth: Auth,
    address: String,
    output: OutputManager,
}
```

## 代码复用策略

1. **统一的blob上传方法**：`upload_blob_with_token()` 
2. **统一的manifest上传方法**：`upload_manifest_with_token()`
3. **统一的缓存推送逻辑**：模式3和4复用相同的`push_from_cache()`方法
4. **统一的tar解析**：所有tar操作使用`TarUtils::parse_image_info()`
5. **统一的错误处理**：使用`error::handlers`模块标准化错误处理

## 命令行接口

现有命令行接口支持新功能：

```
USAGE:
    docker-image-pusher [SUBCOMMAND]

SUBCOMMANDS:
    pull       从 repository 拉取镜像并缓存
    extract    从 tar 文件提取镜像并缓存  
    push       推送镜像到 repository
    list       列出缓存中的镜像
    clean      清理缓存
    help       显示帮助信息
```

### pull 子命令

```
USAGE:
    docker-image-pusher pull [OPTIONS] --repository <REPOSITORY> --reference <REFERENCE>

OPTIONS:
    -r, --repository <REPOSITORY>    Repository 名称 (例如: library/ubuntu)
    -t, --reference <REFERENCE>      标签或摘要 (例如: latest 或 sha256:...)
    -u, --username <USERNAME>        Registry 用户名
    -p, --password <PASSWORD>        Registry 密码
    --registry <REGISTRY>            Registry 地址 (默认: https://registry-1.docker.io)
    --skip-tls                       跳过 TLS 证书验证
    --cache-dir <CACHE_DIR>          缓存目录 (默认: .cache)
```

### extract 子命令

```
USAGE:
    docker-image-pusher extract [OPTIONS] --file <FILE>

OPTIONS:
    -f, --file <FILE>                Docker 镜像 tar 文件路径
    --cache-dir <CACHE_DIR>          缓存目录 (默认: .cache)
```

### push 子命令

```
USAGE:
    docker-image-pusher push [OPTIONS] --source <SOURCE> --repository <REPOSITORY> --reference <REFERENCE>

OPTIONS:
    -s, --source <SOURCE>            源镜像 (格式: repository:tag 或 tar 文件路径)
    -r, --repository <REPOSITORY>    目标 repository 名称
    -t, --reference <REFERENCE>      目标标签
    -u, --username <USERNAME>        Registry 用户名
    -p, --password <PASSWORD>        Registry 密码
    --registry <REGISTRY>            Registry 地址 (默认: https://registry-1.docker.io)
    --skip-tls                       跳过 TLS 证书验证
    --cache-dir <CACHE_DIR>          缓存目录 (默认: .cache)
    --retry-attempts <ATTEMPTS>      失败重试次数 (默认: 3)
    --max-concurrent <MAX>           最大并发上传数 (默认: 1)
    --skip-existing                  跳过已存在的 blob
    --force-upload                   强制上传即使 blob 已存在
```

## 实现要点

### 数据完整性
- 所有blob的SHA256摘要验证
- 保持Docker tar文件中的原始gzip格式
- 不对缓存内容进行不必要的修改

### 性能优化  
- 跳过已存在的blob减少传输
- 支持并发上传（可配置）
- 缓存认证token减少请求
- 流式处理减少内存使用

### 错误处理
- 标准化的错误类型和消息
- 详细的上下文信息
- 网络错误重试机制
- 存储后端错误识别

### 进度报告
- 实时上传进度显示
- 详细的统计信息
- 可配置的详细程度
- 结构化的日志输出

## Docker Registry API 兼容性

支持 Docker Registry API v2 的所有核心端点：

1. `/v2/` - 基本连接测试
2. `/v2/<name>/manifests/<reference>` - manifest操作
3. `/v2/<name>/blobs/<digest>` - blob获取
4. `/v2/<name>/blobs/uploads/` - blob上传
5. `/v2/<name>/tags/list` - 标签列表

## 安全考虑

1. 认证信息不记录到日志
2. 默认使用HTTPS，可选跳过TLS验证
3. 数据完整性校验
4. 权限最小化原则
