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

## 统一进度显示系统

### 系统架构

新的进度显示系统为 push 和 pull 操作提供统一的实时进度跟踪和显示功能。该系统基于以下核心组件：

#### 进度状态跟踪 (ProgressState)

```rust
pub struct ProgressState {
    pub total_tasks: usize,           // 总任务数量
    pub completed_tasks: usize,       // 已完成任务数量
    pub failed_tasks: usize,          // 失败任务数量
    pub active_tasks: Arc<RwLock<HashMap<String, TaskProgress>>>, // 活跃任务状态
    pub start_time: Instant,          // 开始时间
    pub last_update: Instant,         // 最后更新时间
    pub concurrency_adjustments: Vec<ConcurrencyAdjustment>, // 并发调整历史
}
```

#### 任务进度 (TaskProgress)

```rust
pub struct TaskProgress {
    pub task_id: String,             // 任务唯一标识
    pub description: String,         // 任务描述
    pub processed_bytes: u64,        // 已处理字节数
    pub total_bytes: u64,           // 总字节数
    pub start_time: Instant,        // 任务开始时间
    pub status: String,             // 任务状态
}
```

#### 并发调整记录 (ConcurrencyAdjustment)

```rust
pub struct ConcurrencyAdjustment {
    pub timestamp: Instant,          // 调整时间
    pub old_concurrency: usize,      // 调整前并发数
    pub new_concurrency: usize,      // 调整后并发数
    pub reason: String,              // 调整原因
}
```

### 进度显示功能

#### 实时进度显示

- **主进度条**: 显示总体完成百分比和速度统计
- **任务状态**: 显示当前活跃任务的详细进度
- **并发管理**: 实时显示当前并发数量和调整状态
- **性能指标**: 包括吞吐量、平均速度和估计剩余时间

#### 详细状态显示

提供两种显示模式：

1. **简洁模式** (`display_live_progress`):
   ```
   [████████████████████████████████████████] 85% (17/20) Speed: 2.3 MB/s ETA: 30s
   Active: [task-1: 75%] [task-2: 40%] [task-3: 90%] Concurrency: 3
   ```

2. **详细模式** (`display_detailed_progress`):
   ```
   === Progress Details ===
   Total Tasks: 20 | Completed: 17 | Failed: 0 | Active: 3
   Overall Progress: 85.0% | Runtime: 2m 30s
   
   Active Tasks:
   - task-1: Uploading layer sha256:abc123... (15.2 MB / 20.0 MB) 75%
   - task-2: Uploading layer sha256:def456... (8.1 MB / 20.0 MB) 40%
   - task-3: Uploading layer sha256:ghi789... (18.0 MB / 20.0 MB) 90%
   
   Concurrency: 3 | Adjustments: 2 | Last: Increased from 2 to 3 (high success rate)
   Performance: Avg 2.3 MB/s | Current 2.8 MB/s | ETA: 30s
   ```

### 统一管道集成 (Unified Pipeline)

#### 任务执行流程

新的统一管道 (`unified_pipeline.rs`) 集成了进度跟踪系统：

```rust
impl UnifiedPipeline {
    pub async fn execute_tasks(
        &self,
        tasks: Vec<Task>,
        max_concurrency: usize,
        progress_state: Arc<RwLock<ProgressState>>,
    ) -> Result<Vec<TaskResult>>;
    
    async fn execute_single_task_with_progress(
        &self,
        task: Task,
        progress_state: Arc<RwLock<ProgressState>>,
    ) -> Result<TaskResult>;
}
```

#### 进度通知系统

提供标准化的通知接口：

```rust
impl OutputManager {
    pub fn notify_task_start(&self, task_id: &str, description: &str);
    pub fn notify_task_complete(&self, task_id: &str, success: bool);
    pub fn notify_concurrency_adjustment(&self, old: usize, new: usize, reason: &str);
}
```

### 认证逻辑统一

#### 统一认证方法

创建了 `authenticate_with_registry` 方法，消除了 push 和 pull 操作的认证代码重复：

```rust
impl Runner {
    async fn authenticate_with_registry(
        &self,
        auth_info: Option<(String, String)>,
        registry_url: &str,
        skip_tls: bool,
    ) -> Result<(RegistryClient, Option<String>)>;
}
```

该方法支持：
- 有凭据认证（用户名/密码）
- 匿名认证
- TLS 配置管理
- 错误统一处理

### 操作模式集成

#### Mode 1: Pull and Cache
- 使用 unified pipeline 进行并行下载
- 实时显示下载进度和速度
- 支持断点续传和错误重试

#### Mode 5: Push from Tar (Optimized)  
- 使用 unified pipeline 进行并行上传
- 实时显示上传进度和并发状态
- 动态调整并发数量以优化性能

### 配置选项

进度显示系统支持以下配置：

- `--max-concurrent`: 最大并发任务数
- `--progress-detail`: 进度显示详细程度 (simple/detailed)
- `--progress-interval`: 进度更新间隔 (毫秒)
- `--disable-progress`: 禁用进度显示

## 动态并发管理系统 (重新设计)

### 系统概述

动态并发管理系统现已独立为专门的模块，提供智能的并发控制机制，能够根据实际传输性能自动调整并发任务数，以优化传输效率和资源利用率。该系统采用模块化设计，支持多种并发策略和性能监控算法。

### 架构设计

#### 核心接口 (ConcurrencyController)

```rust
pub trait ConcurrencyController: Send + Sync {
    fn current_concurrency(&self) -> usize;
    fn update_metrics(&self, bytes_transferred: u64, elapsed: Duration);
    async fn acquire_permits(&self, count: usize) -> Result<Vec<ConcurrencyPermit>, ConcurrencyError>;
    fn should_adjust_concurrency(&self) -> bool;
    fn get_statistics(&self) -> ConcurrencyStatistics;
}
```

#### 模块组织

```
src/concurrency/
├── mod.rs              # 模块入口和核心接口
├── config.rs           # 配置管理
├── manager.rs          # 并发管理器实现
├── strategy.rs         # 策略选择和实现
└── monitor.rs          # 性能监控和分析
```

### 并发管理器类型

#### 1. DynamicConcurrencyManager
- **用途**: 基于性能反馈的动态调整
- **算法**: 统计回归分析 + 策略选择
- **特性**: 自适应、高精度、实时调整

#### 2. FixedConcurrencyManager  
- **用途**: 固定并发数控制
- **算法**: 简单信号量控制
- **特性**: 稳定、可预测、低开销

#### 3. AdaptiveConcurrencyManager
- **用途**: 机器学习增强的并发控制
- **算法**: 神经网络 + 强化学习
- **特性**: 智能预测、长期优化、自我进化

### 策略系统

#### 策略类型 (ConcurrencyStrategy)

```rust
pub enum ConcurrencyStrategy {
    Conservative,           // 保守策略：稳定性优先
    Aggressive,            // 激进策略：性能优先
    Adaptive,              // 自适应策略：平衡性能和稳定性
    NetworkOptimized,      // 网络优化策略：基于网络特性
    ResourceAware,         // 资源感知策略：考虑系统资源
    MLEnhanced,           // 机器学习增强策略
}
```

#### 策略选择器 (StrategySelector)

```rust
pub struct StrategySelector {
    current_strategy: ConcurrencyStrategy,
    selector_algorithm: Box<dyn StrategyAlgorithm>,
    performance_history: Vec<PerformanceSnapshot>,
    strategy_performance: HashMap<ConcurrencyStrategy, PerformanceMetrics>,
}
```

### 性能监控系统

#### 监控组件 (PerformanceMonitor)

```rust
pub struct PerformanceMonitor {
    data_points: VecDeque<SpeedDataPoint>,
    regression_analyzer: RegressionAnalyzer,
    trend_detector: TrendDetector,
    confidence_calculator: ConfidenceCalculator,
}
```

#### 分析能力

1. **回归分析**: 预测传输速度趋势
2. **置信度计算**: 评估预测可靠性
3. **趋势检测**: 识别性能变化模式
4. **异常检测**: 发现性能异常情况

### 配置系统

#### 层次化配置 (ConcurrencyConfig)

```rust
pub struct ConcurrencyConfig {
    pub limits: ConcurrencyLimits,         // 基础限制
    pub dynamic: DynamicSettings,          // 动态调整设置
    pub monitoring: MonitoringConfig,      // 监控配置
    pub strategy: StrategyConfig,          // 策略配置
}
```

#### 预定义配置

- `ConcurrencyConfig::for_small_files()`: 小文件优化
- `ConcurrencyConfig::for_large_files()`: 大文件优化
- `ConcurrencyConfig::conservative()`: 保守配置
- `ConcurrencyConfig::aggressive()`: 激进配置

### 集成接口

#### 工厂模式 (ConcurrencyFactory)

```rust
impl ConcurrencyFactory {
    pub fn create_dynamic_manager(config: ConcurrencyConfig) -> Box<dyn ConcurrencyController>;
    pub fn create_fixed_manager(max_concurrency: usize) -> Box<dyn ConcurrencyController>;
    pub fn create_adaptive_manager(config: ConcurrencyConfig) -> Box<dyn ConcurrencyController>;
}
```

#### 与 UnifiedPipeline 集成

```rust
impl UnifiedPipeline {
    pub fn with_concurrency_manager(
        mut self, 
        manager: Box<dyn ConcurrencyController>
    ) -> Self;
    
    pub async fn execute_with_concurrency_control(
        &self,
        tasks: Vec<PipelineTask>,
        concurrency_manager: &dyn ConcurrencyController,
    ) -> Result<Vec<TaskResult>>;
}
```

### 命令行集成

#### 新增参数

```bash
# 并发管理器类型
--concurrency-manager <TYPE>        # dynamic, fixed, adaptive

# 动态并发配置
--enable-dynamic-concurrency
--min-concurrent <N>
--max-concurrent <N>
--adjustment-factor <FACTOR>
--speed-threshold <BYTES_PER_SEC>

# 性能监控配置
--enable-monitoring
--sample-interval <MILLISECONDS>
--confidence-threshold <0.0-1.0>

# 策略配置
--concurrency-strategy <STRATEGY>   # conservative, aggressive, adaptive
--auto-strategy-switching
--strategy-switch-confidence <0.0-1.0>
```

### 性能特性

#### 内存效率
- 限制历史数据大小
- 使用环形缓冲区存储样本
- 自动清理过期数据

#### 线程安全
- 所有状态使用 Arc<RwLock<>> 保护
- 原子操作用于高频更新
- 无锁数据结构优化热路径

#### 实时响应
- 亚秒级性能监控
- 快速策略切换
- 低延迟permit获取

### 扩展性设计

#### 插件系统
- 自定义策略插件接口
- 外部监控系统集成
- 第三方性能分析工具

#### 配置热更新
- 运行时配置修改
- 无中断策略切换
- 动态阈值调整

#### 可观测性
- 详细的性能指标导出
- 实时状态监控API
- 历史数据持久化
