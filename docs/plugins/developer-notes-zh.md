# cc-switch 插件系统开发者笔记

> 本文档面向 cc-switch 维护者，描述插件系统的模块划分、挂点位置、扩展方法与测试方式。
> 所有 file:line 以当前代码为准（撰写于本仓库工作副本）。用户视角文档见
> [user-guide-zh.md](./user-guide-zh.md)；原始设计契约见
> [docs/dev/plugin-system-contract.md](../dev/plugin-system-contract.md)
> （注意：实现与契约有少量偏离，见文末清单）。

## 目录

1. [架构总览](#1-架构总览)
2. [三个挂点的位置](#2-三个挂点的位置)
3. [核心类型与 ProxyPlugin trait](#3-核心类型与-proxyplugin-trait)
4. [PluginRegistry 与 fail-open 管线](#4-pluginregistry-与-fail-open-管线)
5. [内置插件与"新增内置插件"步骤](#5-内置插件与新增内置插件步骤)
6. [用户外部插件（external.rs）](#6-用户外部插件externalrs)
7. [初始化、重载与持久化](#7-初始化重载与持久化)
8. [Tauri 命令层与前端管理面板](#8-tauri-命令层与前端管理面板)
9. [测试方式](#9-测试方式)
10. [实现与契约的偏离清单](#10-实现与契约的偏离清单)

---

## 1. 架构总览

```
src-tauri/src/proxy/plugins/
├── mod.rs        # ProxyPlugin trait 定义（含 SseChunk 两个方法）+ 模块导出
├── types.rs      # PluginStage / PluginError / PluginRequestContext /
│                 # PluginManifest（含 validate） / PluginsConfig / PluginInfo
├── registry.rs   # PluginRegistry（排序/过滤/override/全局开关）
│                 # + run_request_pipeline / run_sse_pipeline（fail-open 管线，唯一挂点入口）
├── builtin.rs    # 内置插件：cache-injector / thinking-optimizer（PreSend，Bedrock 专属）
├── privacy.rs    # 内置插件：隐私替换（PreRequest 替换 / PostResponse + SseChunk 还原，
│                 # SQLite 映射表、config_read/write 面板配置，详见模块文档）
├── external.rs   # 用户外部脚本插件：oneshot 进程调用（PluginProcessRunner）+
│                 # 常驻进程（PersistentTransport，专属 worker 线程）+ 加载器
└── init.rs       # 注册表组装：init_registry / reload_user_plugins / plugins_dir
```

数据流：

```
handlers.rs (PreRequest)
        │
        ▼
handler_context.rs::run_pre_request_plugins ──► run_request_pipeline
        │
        ▼
forwarder.rs (PreSend, per-provider) ──────────► run_request_pipeline
        │
        ▼
response_processor.rs (PostResponse) ──────────► run_request_pipeline
        │
        ▼
ProxyPlugin::transform_request / transform_response
   ├── builtin.rs   （Rust 实现，读 Database 里的 OptimizerConfig）
   └── external.rs  （spawn 子进程，stdin/stdout JSON 协议）
```

注册表实例：`ProxyService` 构造时 `init_registry(db)` 创建
（`src-tauri/src/services/proxy.rs:420`），经 `ProxyState.plugins`
（`src-tauri/src/proxy/server.rs:53`）与 `AppState.plugins`
（`src-tauri/src/store.rs:17`，取自 `proxy_service.plugins()`，`store.rs:29`）
共享同一 `Arc<PluginRegistry>`——代理未启动时命令层也可访问。

## 2. 三个挂点的位置

### PreRequest（每个客户端请求一次）

- 管线封装：`src-tauri/src/proxy/handler_context.rs:290`
  `run_pre_request_plugins(state, headers, body, app_type_str)`。
  - 无启用的 PreRequest 插件时直接返回（避免 session/model 提取开销）；
  - `provider` 恒为 `None`（尚未选定供应商）；
  - `session_id` 来自 `extract_session_id`，`request_model` 取 `body.model`。
- 6 个调用入口（均在 `src-tauri/src/proxy/handlers.rs`，位于 body 解析成功后、
  `RequestContext::new` 之前）：

| 行号 | 函数 | app_type |
|---|---|---|
| 188 | `handle_messages_for_app` | 动态（claude 等） |
| 784 | `handle_chat_completions` | `codex` |
| 877 | `handle_responses_for_app` | 动态 |
| 1031 | `handle_codex_standalone_passthrough` | `codex` |
| 1110 | `handle_responses_compact_for_app` | 动态 |
| 2119 | `handle_gemini`（仅当 body 为 JSON object 才调用；GET 只读请求 body 为 Null 跳过） | `gemini` |

### PreSend（每个供应商尝试一次，故障转移重跑）

- 管线封装：`src-tauri/src/proxy/forwarder.rs:201`
  `RequestForwarder::run_pre_send_plugins(app_type_str, provider, body, provider_body)`。
  `provider` 提供 `PluginProviderInfo { id, name, is_bedrock: is_bedrock_provider(provider) }`
  （`is_bedrock_provider` 在 `forwarder.rs:2867`）。
- 调用点：`forwarder.rs:543`，位于 `forward_with_retry_inner`（`forwarder.rs:469`）的
  per-provider 循环（`for provider in providers.iter()`，`forwarder.rs:504`）内，
  `provider_body = body.clone()` 之后立即执行——每个 provider 独立 clone，
  插件改写不会泄漏到故障转移链上的其他 provider。
- Bedrock 门**不在挂点处判断**，已下沉到内置插件内部（`ctx.provider.is_bedrock`）。

### PostResponse（每个成功的非流式响应一次）

- 调用点：`src-tauri/src/proxy/response_processor.rs:311-356`，
  位于 `handle_non_streaming`（`response_processor.rs:214`）。
- 条件：`status.is_success()` 且注册表存在启用的 PostResponse 插件，且响应体可解析
  为 JSON（非 JSON 静默跳过）。usage 统计基于插件改写前的 body（计费与上游一致）。
- 改写生效时用 `strip_entity_headers_for_rebuilt_body` 剥离失真实体头；
  改写后序列化失败则回退原始响应（fail-open）。
- **流式/SSE 路径与格式转换路径（`handle_claude_transform` 等）不接入本挂点。**

### SseChunk（流式逐事件）

已接入框架；外部插件仅在 persistent 模式（`mode: "persistent"`，见第 6 节）下
支持，oneshot 清单声明 `sse_chunk` 仍被 `PluginManifest::validate` 拒绝：

- trait 新增两个带默认实现的方法（`mod.rs:82` / `mod.rs:89`）：
  `new_sse_state()` 返回每条流一份的私有状态（`Box<dyn Any + Send>`，跨 chunk 携带）；
  `transform_sse_event(ctx, event_name, data, state)` 对单个 SSE 事件的 data 负载
  （原始字符串）做变换，返回 `Ok(true)` 表示修改了 data。
- 管线执行器 `run_sse_pipeline`（`registry.rs:231`）：与 `run_request_pipeline` 同构的
  fail-open 执行器；`states: Vec<Option<Box<dyn Any + Send>>>` 与插件列表同序，槽位
  惰性初始化（无状态插件传 `&mut ()` 占位），数量不一致时自动补齐（防御注册表热重载）。
- 包装器 `create_sse_plugin_transform_stream`（`response_processor.rs:261`）：
  仅当 `plugins_for_stage(SseChunk)` 非空时由 `handle_streaming` 包装响应流；
  字节 → UTF-8 安全缓冲（`append_utf8_safe`）→ 事件块切分 → 逐块调管线；
  **未修改的块按原始字节透传**；被修改的块重 emitted（event/id/retry/注释行保序保留，
  data 行合并为单行，行尾风格与块分隔符沿用原样式）；流结束把残余不完整块原样冲出。
- 调用点：`response_processor.rs` `handle_streaming`（约 202 行）。

### PostResponse 与 SseChunk 的分工

`transform_response` 只触达非流式 JSON 响应；流式响应由 SseChunk 挂点逐事件处理。
插件在两个方向都声明 stage 即可同时参与（例如流式/非流式一致还原的场景）。

## 3. 核心类型与 ProxyPlugin trait

`types.rs`：

- `PluginStage`：`snake_case` 序列化（`"pre_request"` / `"pre_send"` /
  `"post_response"` / 预留 `"sse_chunk"`）。
- `PluginError`：`thiserror`，变体 `Execution` / `Timeout` / `InvalidManifest` /
  `Io` / `Json`。
- `PluginRequestContext`（`Serialize`，**未加 rename_all，字段名为 snake_case 原样**）：
  `app_type` / `session_id` / `request_model` / `stage` /
  `provider: Option<PluginProviderInfo>`。
- `PluginProviderInfo`（`Serialize`，**字段名原样**）：`id` / `name` /
  `is_bedrock`——外部插件收到的 stdin JSON 中 provider 即为
  `{"id":..., "name":..., "is_bedrock":...}`。
- `PluginManifest`（`snake_case`，未知字段忽略）：字段与约束见
  [用户文档字段表](./user-guide-zh.md#6-pluginjson-字段表)；
  `validate()` 校验 id 正则、stages 非空且不含 SseChunk、command 非空、
  `timeout_ms ∈ [1, 60000]`（上限常量 `MAX_PLUGIN_TIMEOUT_MS = 60_000`）。
- `PluginsConfig`：`{ enabled: bool, overrides: HashMap<String, PluginOverride> }`，
  camelCase serde，`Default` 为 `enabled: true`。settings 表 key =
  `plugins_config`。
- `PluginInfo`：命令层/前端展示类型（camelCase），含 `error`（加载失败条目）。

`mod.rs` 中的 trait：

```rust
pub trait ProxyPlugin: Send + Sync {
    fn id(&self) -> &str;              // "builtin:<name>" 或 "user:<manifest.id>"
    fn display_name(&self) -> &str;
    fn description(&self) -> &str;
    fn is_builtin(&self) -> bool;
    fn stages(&self) -> &'static [PluginStage];
    fn default_priority(&self) -> i32; // 数字越小越先；内置占 100-899，用户默认 500
    fn default_enabled(&self) -> bool { true }   // 外部插件来自 manifest.enabled
    fn version(&self) -> Option<String> { None }
    fn source(&self) -> Option<String> { None }  // 外部插件 manifest 路径
    fn transform_request(&self, ctx, body) -> Result<bool, PluginError> { Ok(false) }
    fn transform_response(&self, ctx, body) -> Result<bool, PluginError> { Ok(false) }
}
```

`transform_request` 服务于 PreRequest/PreSend，`transform_response` 服务于
PostResponse；返回 `Ok(true)` 表示修改了 body。

## 4. PluginRegistry 与 fail-open 管线

`registry.rs` 的 `PluginRegistry`（内部 `RwLock<RegistryInner>` +
`global_enabled: AtomicBool`）：

- `register`：幂等（同 id 后注册者覆盖），插入后按**生效优先级升序**稳定排序；
- `set_override(id, enabled: Option<bool>, priority: Option<i32>)`：双 None 移除覆盖；
  生效规则：`priority = override.priority ?? default_priority`、
  `enabled = default_enabled && override.enabled != Some(false)`；
- `set_global_enabled` / `global_enabled`：全局开关内存态，关闭时
  `plugins_for_stage` 返回空、`list()` 全部条目展示为禁用（注册表内容保留）；
- `clear_user_plugins`：移除非内置插件与失败条目（重载前调用）；
- `add_failed_entry`：加载失败的插件以 `PluginInfo { error: Some(...) }` 形式进入
  list 展示（id 取 `user:<目录名>`）；
- `plugins_for_stage(stage)`：过滤启用且声明该 stage 的插件，保持优先级升序。

**fail-open 集中实现**：`run_request_pipeline`（`registry.rs:163`）。挂点只调这一个
函数；内部对每个插件用
`std::panic::catch_unwind(AssertUnwindSafe(...))` 包裹调用：

- `Ok(Ok(true))` → 记 changed；`Ok(Ok(false))` → 无修改；
- `Ok(Err(e))` → `log::warn!("[PLUGIN] 插件 {id} 执行失败，已跳过(fail-open): {e}")`；
- `Err(panic)` → `log::warn!("[PLUGIN] 插件 {id} 发生 panic，已跳过(fail-open)")`。

任一插件失败不阻断后续插件，失败不触碰 `body`。

SseChunk 阶段的管线执行器 `run_sse_pipeline`（`registry.rs:231`）与
`run_request_pipeline` 同构（fail-open + catch_unwind），额外管理每插件的
`states` 状态槽位（见第 2 节 SseChunk 小节）。

## 5. 内置插件与"新增内置插件"步骤

现有内置插件（`builtin.rs`）均为 PreSend，持 `Arc<Database>`，每次变换实时读
`OptimizerConfig`（读取失败 log::warn 跳过），并以 `bedrock_gate(ctx)` 判断
`ctx.provider.is_bedrock`（provider 为 None 视为非 Bedrock）：

- `BuiltinThinkingOptimizerPlugin`（`builtin:thinking-optimizer`，优先级 100）：
  `config.enabled && config.thinking_optimizer` 时调用
  `proxy::thinking_optimizer::optimize`；
- `BuiltinCacheInjectorPlugin`（`builtin:cache-injector`，优先级 200）：
  `config.enabled && config.cache_injection` 时调用 `proxy::cache_injector::inject`。

**新增内置插件的步骤：****新增内置插件的步骤：**

1. 在 `src-tauri/src/proxy/plugins/builtin.rs` 中实现一个结构体，实现
   `ProxyPlugin`：`id` 用 `"builtin:<name>"`，`is_builtin() -> true`，
   `default_priority` 在 100–899 段位内选取，按需实现
   `transform_request` / `transform_response`（内部自做业务开关门，失败返回
   `Err` 或 `Ok(false)` 均为 fail-open）；
2. 在 `src-tauri/src/proxy/plugins/init.rs` 的 `build_registry`（约 82–84 行，
   `registry.register(...)` 两处之后）注册新插件；
3. 补单元测试（参照 `builtin.rs` 测试模块：内存库 `Database::memory()` +
   `db.set_optimizer_config` 构造配置快照）；
4. 若需要新配置项，注意内置插件读配置走 `db.get_optimizer_config()` /
   `Database`，不要引入静态全局。

## 6. 用户外部插件（external.rs）

- **进程调用抽象**：`trait PluginProcessRunner { fn run(&self, cmd, input, timeout) }`。
  真实实现 `TokioProcessRunner`（`tokio::process::Command`，`kill_on_drop(true)`，
  stdin 写入后立即关闭，stdin 写入与 stdout 收集都纳入超时；超时
  `start_kill` + `wait` 回收直接子进程；退出码非 0 报 `Execution`）。
  `run` 的阻塞策略：处于多线程 tokio 运行时内用 `block_in_place`，否则临时
  current-thread runtime。
- **协议**：stdin 输入 JSON 字段与含义见
  [用户文档第 7 节](./user-guide-zh.md#7-进程调用协议)
  （`external.rs:181` `run_transform` 组装；`provider`/`settings` 按字段名原样
  序列化，`settings` 未配置时为 `null`）。输出解析 `parse_plugin_output`
  （`external.rs:280`）：`{"body": ...}` 应用并返回 `Ok(true)`；`{}` 返回
  `Ok(false)`；空/非法 JSON/缺 `body`/非对象 → `Err`（fail-open）。
- **命令解析** `resolve_command`：argv[0] 含 `/` 或 `\`
  且为相对路径 → 相对于插件目录 join；纯命令名交给 PATH；绝对路径原样。
  此外子进程以 `current_dir(插件目录)` 启动，因此其余参数中的相对路径
  （如 `["node", "index.js"]` 的 `index.js`）同样按插件目录解析。
- **进程 IO**：stderr 与 stdout 并发读取（无人读取 stderr 时，子进程写满
  管道缓冲区会阻塞造成假超时）；插件以非零码退出时，错误信息附带
  stderr 摘要（截断至 500 字符），便于排查脚本错误。
- **防御**：`run_transform` 先检查 `manifest.stages` 是否含当前 stage，
  不含则不 spawn 进程直接 `Ok(false)`。
- **加载器** `load_user_plugins(dir)`（`external.rs:332`）：按目录名排序遍历
  `dir` 的子目录，读 `plugin.json` → `PluginManifest` 反序列化 →
  `validate()` → 构造 `ExternalPlugin`（id 前缀 `user:`）。目录不存在静默返回空；
  任何失败收集为 `LoadError`（dir_name / manifest_path / message），不 panic。
  普通文件（非目录）被忽略。
- **常驻模式（`mode: "persistent"`）**：`PersistentProcessTransport` 在专属
  worker 线程（自建 current-thread runtime）上首次调用时拉起进程并保持存活，
  按行交换 JSON；调用天然串行（worker 循环逐条处理），单次超时/进程崩溃自动
  重启重试一次；stderr 后台任务持续排空防管道阻塞；协议与 oneshot 一致，
  `sse_chunk` 请求额外携带 `event` / `data`，响应 `{"body": {"data": "…"}}`。
  传输经 `PersistentTransport` trait 注入（测试 mock `MockPersistentTransport`，
  不 spawn 真实进程）；插件实例 Drop → worker 收 Shutdown → 进程终止。

## 7. 初始化、重载与持久化

`init.rs`：

- `plugins_dir()`：`crate::config::get_app_config_dir().join("plugins")`
  （`config.rs:203`，默认 `~/.cc-switch`，Windows 为
  `C:\Users\<用户名>\.cc-switch`；支持 `CC_SWITCH_TEST_HOME` 与自定义配置目录
  override，Windows 下有 v3.10.3 legacy 数据库回退逻辑）；
- `init_registry(db)`：注册内置插件 → `load_user_plugins_into` →
  `apply_overrides` → `set_global_enabled(config.enabled)`；
- `reload_user_plugins(registry, db)`：`clear_user_plugins`（清除非内置插件与失败
  条目）→ 重新扫描目录 → 重新应用 overrides 与全局开关（内置插件保留）；
- `load_plugins_config`：settings 表 key `plugins_config`；缺失/解析失败回退
  `PluginsConfig::default()`（全局开启、无覆盖）。

持久化 DAO：`src-tauri/src/database/dao/settings.rs:334` `get_plugins_config`、
`:347` `set_plugins_config`。

## 8. Tauri 命令层与前端管理面板

命令（`src-tauri/src/commands/plugin_system.rs`，均在 `lib.rs:1417-1424` 的
`generate_handler!` 注册）：

| 命令 | 签名 | 用途 |
|---|---|---|
| `plugin_list` | `(state) -> Result<Vec<PluginInfo>, String>` | 列出全部插件（含加载失败条目；全局开关关闭时 enabled 全 false） |
| `plugin_set_enabled` | `(state, id: String, enabled: bool) -> Result<bool, String>` | 设置单个插件启用/禁用（合并既有 override 保留 priority），更新注册表内存态并持久化 |
| `plugin_set_priority` | `(state, id: String, priority: i32) -> Result<bool, String>` | 设置单个插件优先级（合并既有 override 保留 enabled），同上持久化 |
| `plugin_set_all_enabled` | `(state, enabled: bool) -> Result<bool, String>` | 插件系统全局开关（写 `plugins_config.enabled` 并 `set_global_enabled`） |
| `plugin_reload` | `(state) -> Result<PluginReloadResult, String>` | 重扫用户插件目录并重新应用 overrides / 全局开关；返回 `{ loaded: usize, errors: Vec<String> }`（camelCase） |
| `plugin_open_dir` | `(app: AppHandle) -> Result<bool, String>` | 打开插件目录（不存在则先 `create_dir_all`），用 `tauri_plugin_opener` |

注意：`plugin_set_enabled`/`plugin_set_priority` 的 set 语义是"合并 override 的
单个字段"，实现上先从 `registry.overrides()` 取现有 override 再覆盖单字段，
避免互相清掉。

前端面板：`src/components/settings/PluginSettingsPanel.tsx`，挂载于
`src/components/settings/SettingsPage.tsx:525`（设置 → 高级）。通过
`@tauri-apps/api/core` 的 `invoke` 调用上述命令；文案 i18n key 前缀
`settings.advanced.plugins`（`src/i18n/locales/zh.json` / `zh-TW.json` /
`en.json` 等均有对应段）。功能：全局总开关、插件列表（内置/用户徽章、stages、
优先级输入、启用开关）、重新加载、打开插件目录、加载错误展示。

## 9. 测试方式

在 `src-tauri/` 目录运行：

```bash
cargo test proxy::plugins          # 只跑插件系统单元测试
cargo test --lib proxy::plugins    # 等价（仅 lib 目标）
cargo test                         # 全量回归（含挂点接线测试）
cargo clippy                       # 无新增 error
```

测试基础设施（**不 spawn 真实进程**）：

- **mock 进程 runner**（`external.rs` 测试模块）：实现
  `PluginProcessRunner` 的 `MockRunner`，记录 `(cmd, input, timeout_ms)` 到
  `Mutex<Vec<...>>`，按预设 `MockOutput::Text/Fail` 返回 stdout 或错误。
  断言协议输入字段（stage/app_type/session_id/settings/body）与超时取自
  manifest；
- **mock 插件**（`registry.rs` 测试模块）：`MockPlugin` 实现 `ProxyPlugin`，
  可配置优先级/启用/stages，`MockOutcome::Noop/Modify/Fail` 与 `with_panic()`
  用于验证 fail-open（错误插件跳过且后续插件继续执行）；
- **目录注入**：`init.rs` 的 `build_registry(db, dir)` /
  `reload_user_plugins_from(registry, db, dir)` 为可注入目录版本，
  测试用 `tempfile::tempdir()` 造插件目录（合法/坏清单/非 JSON/缺 manifest/
  非法 id 各 case），不触碰真实配置目录；
- **注册表接线测试**：`forwarder.rs` 测试模块构造带 mock 插件的
  `RequestForwarder` 验证 PreSend 管线（含 Bedrock gate）；
  `response_processor.rs` 测试中以 `Arc::new(PluginRegistry::new())` 注入空注册表。
- **SSE 包装器测试**（`response_processor.rs` 测试模块）：`SseReplacePlugin` 把 data
  中子串替换，覆盖未修改块字节级一致、CRLF/LF 行尾保留、事件跨 chunk 切分、多行 data
  合并、UTF-8 多字节跨 chunk、残余不完整块冲出等场景；
  `server.rs` 测试模块有 mock 上游 + 真实代理的 SseChunk 端到端用例。
- **转换路径挂点探针测试**：`handlers.rs` `transform_hook_e2e_tests`——mock 上游
  + 真实代理 + 哨兵替换探针插件，钉住转换路径（needs_transform provider）的
  SseChunk / PostResponse 挂点与空注册表透传行为；
- **常驻模式测试**：`external.rs` `MockPersistentTransport` 验证请求/响应往返、
  sse_chunk 事件协议、传输失败不改写 body、清单 mode 解析与 sse_chunk 校验。

## 10. 实现与契约的偏离清单

对照 `docs/dev/plugin-system-contract.md`，以下为**实现与契约不一致**之处
（文档以实现为准记录；契约是否回写由维护者决定）：

1. **`PluginStage::SseChunk` 已实现**（契约 2.1 的代码片段未包含该变体）：SSE 管线
   已接入框架（`new_sse_state` / `transform_sse_event` / `run_sse_pipeline` /
   `response_processor` 包装器）；外部插件中 oneshot 清单声明 `sse_chunk` 被
   `validate()` 拒绝，persistent 模式（`mode: "persistent"`）支持。
2. **全局开关实现方式**：契约 2.7 说"全局开关关闭则返回空注册表（不注册任何
   插件）"；实现改为注册表内容保留，通过 `global_enabled: AtomicBool` 让
   `plugins_for_stage` 返回空、`list()` 全部展示禁用（支持运行时切换）。
3. **命令数量**：契约 2.8 列了 5 个命令；实现多出 `plugin_set_all_enabled`
   （全局开关命令），且 `plugin_set_enabled` / `plugin_set_priority` 返回
   `Result<bool, String>` 而非契约的 `()`。
4. **ProxyPlugin trait 扩展**：契约 2.2 的 trait 没有
   `default_enabled` / `version` / `source` 三个带默认实现的方法；
   实现新增（外部插件的 enabled/version/manifest 路径需要从 trait 读出）。
5. **失败条目进入注册表**：契约的 `load_user_plugins` 只返回
   `(plugins, errors)`；实现额外在 `init.rs::load_user_plugins_into` 把错误
   包装成 `PluginInfo` 失败条目经 `registry.add_failed_entry` 进入 list
   （契约 2.9 有 `error` 字段但未定义进入 registry 的路径）。
6. **超时 kill 范围**：契约 2.5 要求"超时 kill 进程树"；实现只显式 kill 直接
   子进程（`start_kill` + `kill_on_drop(true)`），孙进程依赖子进程自身退出，
   Windows 上未用 Job Object。
7. **`settings` 默认值为 JSON `null`**：契约清单示例写 `"settings": {}`；
   实现的 `#[serde(default)]` 在缺失时反序列化为 `null`（用户文档已按 `null`
   记录）。
8. **重载同时重置全局开关**：契约 2.5 的重载流程是 clear + load +
   re-apply overrides；实现额外按最新 `plugins_config.enabled` 调用
   `set_global_enabled`（契约未提及）。
9. **命令注册位置**：契约 2.8 说在 `lib.rs` `generate_handler!` 约 1371 行处
   按字母序插入；实际注册在 `lib.rs:1417-1424`（行号随代码漂移，属正常偏移）。
10. **`PluginRequestContext` / `PluginProviderInfo` 的 serde 字段名**：契约
    提示需核实；实现未加 `rename_all`，字段保持 snake_case 原样
    （`session_id` / `request_model` / `is_bedrock`），与协议文档一致。

---

## 相关文档

- 用户使用指南：[user-guide-zh.md](./user-guide-zh.md)
- 原始设计契约：[docs/dev/plugin-system-contract.md](../dev/plugin-system-contract.md)
