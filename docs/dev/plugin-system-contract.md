# cc-switch 插件系统设计契约（内部）

> 本文档是插件系统二次开发的**唯一权威规范**。所有参与实现的模块必须严格遵守本文档的
> 类型签名、目录约定与行为语义。文档用中文撰写，代码注释风格与仓库现有代码保持一致（中文为主）。

## 0. 背景与目标

cc-switch 的本地代理（`src-tauri/src/proxy/`，基于 axum）在转发 Claude/Codex/Gemini CLI
请求时，会在固定挂点对请求/响应 JSON 做改写。插件系统的目标是：

1. 把"改写请求/响应"抽象为统一的插件扩展点（trait + 注册表 + 管线）；
2. 支持**内置插件**（Rust 编译期实现，随应用分发，不可卸载，仅可开关/调序）；
3. 支持**用户自定义插件**（外部脚本，`plugin.json` 清单 + stdin/stdout JSON 协议）；
4. 提供配置 DAO、Tauri 命令与前端管理 UI；
5. 编写用户使用文档与开发者文档。

**核心语义（不可妥协）**：插件永远不能破坏主链路。任何插件返回错误、超时、崩溃，
都只记录 `log::warn` 并跳过该插件，请求继续。即 fail-open。

## 1. 挂点定义（v1 只做三个 stage）

| Stage | 枚举值 | 执行位置 | 执行频率 | v1 范围 |
|---|---|---|---|---|
| PreRequest | `PluginStage::PreRequest` | `handlers.rs::handle_messages_for_app` 中 body 解析成功之后、`RequestContext::new` 之前（注意：`handle_messages_for_app` 有多个相似入口函数，Codex/Gemini 等各自的 `handle_*_for_app` 都要接入，或抽公共函数） | 每个客户端请求一次 | ✅ |
| PreSend | `PluginStage::PreSend` | `forwarder.rs::forward_with_retry_inner` 的 "PRE-SEND 优化器" 代码块处（约 498-512 行），对 `provider_body` 操作，**必须位于 per-provider 循环内部**（故障转移时每个供应商重跑） | 每个供应商尝试一次 | ✅ |
| PostResponse | `PluginStage::PostResponse` | `response_processor.rs::handle_non_streaming`（非流式响应返回客户端前） | 每个成功响应一次 | ✅ |
| SseChunk | （预留） | SSE 流包装层，参考 `create_logged_passthrough_stream` | 每 chunk | ❌ 留接口不实现 |

## 2. 核心类型（`src-tauri/src/proxy/plugins/`）

新模块文件划分：

```
src-tauri/src/proxy/plugins/
├── mod.rs        # 模块导出
├── types.rs      # PluginStage / PluginError / PluginRequestContext / PluginManifest
├── registry.rs   # PluginRegistry（排序、遍历、动态重载）
├── builtin.rs    # 内置插件实现
└── external.rs   # 用户自定义外部脚本插件（加载器 + 进程调用协议）
```

### 2.1 types.rs

```rust
/// 插件挂点阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginStage {
    PreRequest,
    PreSend,
    PostResponse,
    // SseChunk 预留，v1 不实现
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("插件 {plugin_id} 执行失败: {message}")]
    Execution { plugin_id: String, message: String },
    #[error("插件 {plugin_id} 超时 ({timeout_ms}ms)")]
    Timeout { plugin_id: String, timeout_ms: u64 },
    #[error("插件清单无效: {0}")]
    InvalidManifest(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
}

/// 传给插件的请求上下文（只读快照，避免直接依赖 RequestContext 降低耦合）
#[derive(Debug, Clone, Serialize)]
pub struct PluginRequestContext {
    pub app_type: String,        // "claude" | "codex" | "gemini" | ...
    pub session_id: String,
    pub request_model: String,
    pub stage: PluginStage,
    /// PreSend / PostResponse 阶段提供；PreRequest 为 None
    pub provider: Option<PluginProviderInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginProviderInfo {
    pub id: String,
    pub name: String,
    /// 是否为 Bedrock 供应商（实现说明：阶段 2a 增加）。内置优化器插件
    /// （cache-injector / thinking-optimizer）在插件内部用该字段实现 Bedrock 门，
    /// provider 为 None 时视为非 Bedrock。
    pub is_bedrock: bool,
}
```

注意：若 `thiserror` 未在 Cargo.toml 中引入，先检查再决定（引入或手写 Display）。

### 2.2 插件 trait（mod.rs 中定义）

```rust
/// 插件 trait：内置插件与外部插件的统一抽象
pub trait ProxyPlugin: Send + Sync {
    /// 唯一 ID：内置为 "builtin:<name>"，用户插件为 "user:<manifest.id>"
    fn id(&self) -> &str;
    /// 显示名
    fn display_name(&self) -> &str;
    /// 描述
    fn description(&self) -> &str;
    fn is_builtin(&self) -> bool;
    /// 声明支持的 stage
    fn stages(&self) -> &'static [PluginStage];
    /// 默认优先级，数字越小越先执行。内置插件占用 100-899，用户插件默认 500
    fn default_priority(&self) -> i32;

    /// 请求变换（PreRequest / PreSend 阶段调用）
    /// 返回 Ok(true) 表示修改了 body；Ok(false) 表示未修改
    fn transform_request(
        &self,
        ctx: &PluginRequestContext,
        body: &mut serde_json::Value,
    ) -> Result<bool, PluginError> {
        let _ = (ctx, body);
        Ok(false)
    }

    /// 响应变换（PostResponse 阶段调用，仅非流式）
    fn transform_response(
        &self,
        ctx: &PluginRequestContext,
        body: &mut serde_json::Value,
    ) -> Result<bool, PluginError> {
        let _ = (ctx, body);
        Ok(false)
    }
}
```

### 2.3 registry.rs

```rust
pub struct PluginRegistry {
    // 内部：RwLock<Vec<Arc<dyn ProxyPlugin>>> + 每插件的启用/优先级覆盖(RwLock<PluginsOverrides>)
    // plugins 存储时按 (stage 无关的) priority 升序排列
}
impl PluginRegistry {
    pub fn new() -> Self;
    /// 注册插件（幂等：同 id 后注册者覆盖先注册者），并应用已有 override
    pub fn register(&self, plugin: Arc<dyn ProxyPlugin>);
    /// 清除所有非内置插件（重载用户插件前调用）
    pub fn clear_user_plugins(&self);
    /// 列出全部插件元信息（供 Tauri 命令/前端），按 default_priority 排序
    pub fn list(&self) -> Vec<PluginInfo>;
    /// 按 stage 取出启用的插件（调用方逐个调用，管线函数见下）
    pub fn plugins_for_stage(&self, stage: PluginStage) -> Vec<Arc<dyn ProxyPlugin>>;
    /// 设置运行时覆盖（enabled/priority），持久化由 DAO 层负责，注册表只存内存态
    pub fn set_override(&self, plugin_id: &str, enabled: Option<bool>, priority: Option<i32>);
    pub fn overrides(&self) -> HashMap<String, PluginOverride>;
}
```

**管线执行函数**（registry.rs 提供，挂点只调这一个函数，保证 fail-open 语义集中实现）：

```rust
/// 执行某个 stage 的插件管线。任何插件错误仅 log::warn 并跳过。
/// 返回是否有插件修改了 body。
pub fn run_request_pipeline(
    registry: &PluginRegistry,
    stage: PluginStage,
    ctx: &PluginRequestContext,
    body: &mut serde_json::Value,
    apply: impl Fn(&dyn ProxyPlugin, &PluginRequestContext, &mut serde_json::Value) -> Result<bool, PluginError>,
) -> bool;
```

（`transform_response` 的调用同理，可共用一个内部实现。）

### 2.4 运行时覆盖与持久化（DAO 契约，阶段 2b 实现）

settings 表 key：`plugins_config`（复用 `database/dao/settings.rs` 中 `get_setting/set_setting`
模式，参照 `get_rectifier_config` 第 244 行的写法）：

```json
{
  "enabled": true,
  "overrides": {
    "user:my-plugin": { "enabled": true, "priority": 300 },
    "builtin:cache-injector": { "enabled": false }
  }
}
```

DAO 方法签名：

```rust
pub fn get_plugins_config(&self) -> Result<PluginsConfig, AppError>;
pub fn set_plugins_config(&self, config: &PluginsConfig) -> Result<(), AppError>;
```

`PluginsConfig` 结构体定义放在 `proxy/plugins/types.rs`（含 serde default，向后兼容）。
生效规则：插件实际执行 = manifest/内置默认启用 且 override.enabled 不为 false；
priority = override.priority 优先，否则 default_priority。

### 2.5 用户自定义插件（external.rs）

**目录**：复用应用现有配置目录助手（见 `config.rs`，即 `~/.cc-switch` 或项目实际使用的
目录——实现时以 `config.rs` 中现有函数为准），插件目录为 `<配置目录>/plugins/<插件目录名>/`，
每个插件目录必须包含 `plugin.json`。

**清单格式**（`PluginManifest`，serde snake_case，未知字段忽略）：

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "version": "0.1.0",
  "description": "示例",
  "stages": ["pre_request"],
  "mode": "persistent",
  "priority": 500,
  "command": ["node", "index.js"],
  "timeout_ms": 10000,
  "enabled": true,
  "settings": {}
}
```

- `id`：必填，`[a-zA-Z0-9_-]{1,64}`，注册后前缀为 `user:`；
- `command`：必填，argv 数组（第一个元素为可执行文件，相对路径时相对于插件目录解析）；
  子进程以插件目录为工作目录（`current_dir`），其余参数中的相对路径同样按插件目录解析；
- `stages`：必填，只能是 `pre_request` / `pre_send` / `post_response`
  （**v1 不允许外部插件做 pre_send？——允许，pre_send 同样是整份 JSON 改写，无流式问题**）；
  `sse_chunk` 仅 `mode: "persistent"` 下允许（oneshot 每事件拉进程不可行）；
- `mode`：可选，`"oneshot"`（缺省）/ `"persistent"`，语义见下；
- `timeout_ms`：可选，默认 10000，上限 60000；
- `enabled`：可选，默认 true；
- `settings`：可选，任意 JSON，随调用透传给插件。

**进程模式**：

- **oneshot（缺省）**：每次调用拉起新进程，stdin 写入一个 JSON 对象（写完即关闭），
  stdout 期望恰好一个 JSON 对象；无跨调用状态。实现经 `PluginProcessRunner` trait
  注入（真实实现 `TokioProcessRunner`，单元测试用 mock，**不要**在测试里真的 spawn
  外部脚本）；
- **persistent（常驻）**：首次调用时在专属 worker 线程（自建 current-thread
  runtime，进程异步 IO 与 runtime 绑定）上拉起进程并保持存活，之后按行交换 JSON
  （写一行请求 + `\n`、读一行响应，JSON 形状与 oneshot 一致）。调用由 worker 循环
  逐条串行投递，插件进程可自带跨调用状态（如流式 per-stream 缓冲）；单次调用超时或
  进程崩溃（EOF / 退出）→ kill 旧进程、重新拉起并重试一次，再失败按插件错误处理
  （fail-open）；stderr 由后台任务持续排空（防管道写满阻塞）；插件实例 Drop →
  worker 收 Shutdown → 进程终止。`sse_chunk` 请求额外携带 `event` / `data` 字段，
  响应为 `{"body": {"data": "…"}}`（替换事件负载）或 `{}`。实现经
  `PersistentTransport` trait 注入（真实实现 `PersistentProcessTransport`，测试 mock）。

**oneshot 协议细节（两种模式通用的 JSON 形状）**：

- 请求（stdin）：

```json
{
  "stage": "pre_request",
  "app_type": "claude",
  "session_id": "…",
  "request_model": "…",
  "provider": null,
  "settings": {},
  "body": { "...原始请求体...": "" }
}
```

- 响应（stdout）**恰好一个** JSON 对象：`{"body": {…修改后的请求体…}}`（无修改则 `{}`）；
- 其余 stdout 输出（日志等）必须写到 stderr，否则解析失败按插件错误处理（fail-open）；
- 退出码非 0 / 超时 / stdout 非法 JSON → 记 warn 跳过；oneshot 超时 kill 进程
  （Windows 上至少 kill 直接子进程）。

**加载**：`load_user_plugins(dir) -> (Vec<Arc<dyn ProxyPlugin>>, Vec<LoadError>)`，
遍历子目录读取 `plugin.json`，失败条目收集错误不 panic。重载 = clear_user_plugins + load +
re-apply overrides + 重新排序。

### 2.6 内置插件 v1（builtin.rs）

v1 内置插件清单（把现有硬编码变换器收编为插件，**行为必须与现状等价**）：

1. `builtin:cache-injector` — PreSend。包装 `cache_injector::inject`，内部条件与现状一致：
   `optimizer_config.enabled && optimizer_config.cache_injection`。
   **Bedrock 门**：原 forwarder.rs 中的 `is_bedrock_provider(provider)` gate 已由阶段 2a
   下沉到插件内部——插件读取 `ctx.provider.is_bedrock`（provider 为 None 视为非 Bedrock，
   不执行）；挂点处不再重复判断。内置插件持有 `Arc<Database>`，每次变换实时读取
   `OptimizerConfig`（读取失败 log::warn 跳过，fail-open）。
2. `builtin:thinking-optimizer` — PreSend。包装 `thinking_optimizer::optimize`，条件同上。

阶段 1 实现者：先定义这两个内置插件的骨架与单元测试（不接线）；阶段 2a 实现者负责把
forwarder.rs 中原硬编码调用**替换**为插件管线调用（避免双重注入），替换后必须通过
`cargo test` 全量测试与 clippy。

### 2.7 ProxyState 接线（阶段 2a）

- `ProxyState` 增加 `pub plugins: Arc<PluginRegistry>`；
- `ProxyServer::new` 中创建 registry、注册内置插件、从 DAO 加载 overrides、
  调用 `load_user_plugins`（目录不存在则静默创建/跳过）；
- registry 的 `Arc` 需要同时可从 Tauri 命令层访问：检查 `AppState`（`commands/proxy.rs`
  中 `tauri::State<'_, AppState>` 的定义处），若 AppState 与 ProxyState 生命周期一致则透传，
  否则在 AppState 上新增 `plugins: Arc<PluginRegistry>` 字段（代理未启动时命令仍可用）。

### 2.8 Tauri 命令（阶段 2b，`commands/plugin_system.rs`）

```
plugin_list() -> Vec<PluginInfo>            # 合并 manifest/内置信息 + override
plugin_set_enabled(id: String, enabled: bool) -> ()
plugin_set_priority(id: String, priority: i32) -> ()
plugin_reload() -> { loaded: usize, errors: Vec<String> }   # 重扫用户插件目录
plugin_open_dir() -> ()                      # 打开插件目录（不存在则先创建）
plugin_import(source_dir: String) -> { pluginId: String, dirName: String }
# 校验源目录的 plugin.json → 递归拷贝进 plugins/<目录名> → 重载注册表；
# 同名目录已存在时报错（用户重命名后重试）；拷贝中断清理半成品
```

- 全部 `#[tauri::command]`，返回 `Result<T, String>`；
- set_* 需同时更新 registry 内存态并持久化 DAO（注意 DAO 是同步 sqlite，参考其他命令的做法）；
- 在 `lib.rs` 的 `generate_handler!` 中注册（约 1371 行处，按字母序插入）。

### 2.9 PluginInfo（命令返回/前端类型）

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub is_builtin: bool,
    pub stages: Vec<PluginStage>,
    pub priority: i32,
    pub enabled: bool,
    pub version: Option<String>,   // 用户插件来自 manifest；内置为 None
    pub source: Option<String>,    // 用户插件 manifest 路径；内置为 None
    pub error: Option<String>,     // 加载失败的插件（list 也要展示失败条目）
}
```

## 3. 前端（阶段 3a）

- 新组件 `src/components/settings/PluginSettingsPanel.tsx`，挂到 `SettingsPage.tsx`
  （参考 `RectifierConfigPanel.tsx` / `LogConfigPanel.tsx` 的组织方式）；
- 功能：插件列表（名称、ID、内置/用户徽章、stages 标签、优先级数字输入、启用开关）、
  重新加载按钮、打开插件目录按钮、加载错误展示；
- 通过 `@tauri-apps/api/core` 的 `invoke` 调用 2.8 的命令；
- i18n：项目使用 `src/i18n`，新增文案 zh/en 都要加（检查现有 key 组织方式并遵循）；
- 类型定义放 `src/types` 或组件内（遵循现有习惯）。

## 4. 文档（阶段 3b）

- `docs/plugins/user-guide-zh.md`：用户视角——插件目录、安装步骤、manifest 字段表、
  调用协议（输入/输出 JSON 示例）、Node.js 与 Python 最小示例插件、常见问题（超时、
  fail-open 语义、SSE 不支持说明、内置插件列表）；
- `docs/plugins/developer-notes-zh.md`：维护者视角——架构图（挂点位置引用文件:行号）、
  trait 说明、如何新增内置插件、外部插件进程协议细节、测试方式；
- 文档中的协议、字段名必须与本文档一致；若实现中不得已偏离契约，实现者必须回写更新本文档。

## 5. 验证标准（集成阶段由主控执行）

- `cargo test`（src-tauri 下）全部通过；`cargo clippy` 无新增 error（新增 warning 需说明）；
- `pnpm typecheck`、`pnpm test:unit` 通过；
- 手动冒烟：启动应用 → 代理启动 → 用 curl 发一个 `/v1/messages` 请求，验证
  PreRequest 插件（示例用户插件）确实改写了请求且请求成功；
- 回归红线：不启用任何插件时，代理行为与主线完全一致（已有全部测试不变绿失败）。

## 6. 各阶段分工与文件所有权（避免子代理冲突）

| 阶段 | 负责文件（独占写权限） |
|---|---|
| 1 核心框架 | `src-tauri/src/proxy/plugins/**`（新建）、`proxy/mod.rs`（加一行 pub mod）、`Cargo.toml`（如需 thiserror） |
| 2a 挂点接入 | `proxy/server.rs`、`proxy/handlers.rs`、`proxy/forwarder.rs`、`proxy/response_processor.rs`、`proxy/handler_context.rs`、commands/proxy.rs 中 AppState 定义处 |
| 2b 配置与命令 | `database/dao/settings.rs`、`commands/plugin_system.rs`（新建）、`commands/mod.rs`、`lib.rs` |
| 3a 前端 | `src/components/settings/PluginSettingsPanel.tsx`（新建）、`SettingsPage.tsx`、`src/i18n/**` |
| 3b 文档 | `docs/plugins/**` |

阶段 1 → （2a ∥ 2b）→ （3a ∥ 3b）→ 集成验证。
