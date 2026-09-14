# CC-Switch Proxy Standalone CLI 设计文档

| 项 | 值 |
|---|---|
| 日期 | 2026-07-07 |
| 状态 | 草案（待 spec review） |
| 适用仓库 | cc-switch（fork） |
| 目标交付物 | 一个 headless CLI `cc-switch-proxy`，作为 `[[bin]]` 集成在现有 crate 内 |

---

## 1. 背景与动机

cc-switch 是一个 Tauri 桌面应用，核心能力之一是**本地代理服务器**（`src-tauri/src/proxy/`）：监听 `127.0.0.1:15721`，把 Claude Code / Codex / Gemini CLI 发来的请求做**协议格式转换**后转发给上游模型 provider。其中 Codex CLI 走 OpenAI **Responses API**（`/v1/responses`），而 DeepSeek / Kimi / GLM / Qwen / MiniMax / 火山方舟等国产模型走 **Chat Completions**（`/v1/chat/completions`），代理在两者之间双向转换。

**问题**：这套代理能力目前被**强绑定在 Tauri 应用壳里**——启动靠 Tauri command、provider 配置靠 GUI 增删改、状态靠 `Arc<Database>` + `tauri::AppHandle` 承载。其他 Rust 项目想复用「协议转换 + 多模型代理」能力，必须装整个 GUI 应用，无法以编程方式集成。

**目标**：fork 出 cc-switch，在**不破坏与上游主仓合并能力**的前提下，新增一个可独立运行的 headless CLI，让别的 Rust 程序（或运维脚本）能：
1. 启动一个本地 HTTP 代理，做 Codex 协议转换；
2. 通过一套 HTTP 管理 API 动态注册 / 切换 / 删除 provider（写入独立 sqlite DB）；
3. 不依赖 Tauri、不需要 GUI。

---

## 2. 目标与非目标

### 目标
- 在现有 crate 内新增 `cc-switch-proxy` binary，无 Tauri runtime 依赖即可启动代理。
- 复用 cc-switch 现有的协议转换、provider 路由、故障转移、熔断逻辑，**零重写**。
- 提供 RESTful 管理 API，操作 provider，数据落独立 sqlite DB。
- 支持 deepseek / glm / kimi / minimax / ark / qwen 等 OpenAI Chat 兼容上游。
- **对现有源码的修改最小化且为「纯加法」**，保证 fork 可长期跟随上游。

### 非目标（YAGNI，本期不做）
- 不做 Claude / Gemini 客户端方向的端到端验证（代码复用原样支持，但不做测试覆盖与文档，焦点先在 Codex）。
- 不做 OAuth 类 provider（Copilot / CodexOAuth）—— `app_handle = None` 时不可用，且非国产模型场景。
- 不做管理 API 鉴权（本地回环、用户自用，见 §12 风险）。
- 不做 CLI 交互式子命令（`provider add` 等暂不做，靠 HTTP API + 可选配置文件足够）。
- 不做请求用量计费 UI、WebDAV/S3 同步、托盘、自动启动等应用壳功能。
- 不做 tauri 依赖的 feature gate 瘦身（本期接受编译体积偏大，见 §12）。

---

## 3. 总体架构

```
┌─────────────────────────────────────────────────────────────────┐
│  cc-switch-proxy 进程（无 Tauri，app_handle = None）             │
│                                                                 │
│  src/bin/cc_switch_proxy.rs        ← 薄壳 main（新增）          │
│      │ tokio runtime                                             │
│      ▼                                                           │
│  src/standalone/（新增模块，lib 内部，可访问 pub(crate)）       │
│      │                                                           │
│      ├── 解析 CLI 参数（--db / --port / --config）              │
│      ├── Database::open_at(db_path)        ← §7 扩展点③         │
│      ├── 空表也安全（seed + DAO 兜底默认，无需写值）           │
│      ├── 构建 admin Router（/admin/*）→ 复用 ProxyState 作 state│
│      └── ProxyServer::new(cfg, db, None)                        │
│               .with_extra_routes(admin_router)  ← §7 扩展点②    │
│               .start()                                           │
│                                                                 │
│  ┌──────────── 同一 axum Router（监听 127.0.0.1:15721）──────┐  │
│  │  /admin/providers  (GET/POST/DELETE)        ← 新增        │  │
│  │  /admin/providers/:id/enable                ← 新增        │  │
│  │  /admin/status / /admin/config              ← 新增        │  │
│  │  /v1/responses /v1/chat/completions /models ← 现有复用    │  │
│  └──────────────────────┬─────────────────────────────────────┘  │
│                         │ 读写                                    │
│                         ▼                                         │
│          独立 sqlite DB（如 ~/.ccswitch/cli-proxy.db）           │
└─────────────────────────────────────────────────────────────────┘
        ▲                                    ▲
        │ HTTP（管理 API）                    │ HTTP（协议转换）
   用户的程序 / curl                  Codex CLI（指向 127.0.0.1:15721）
```

**两类 HTTP 接口共用同一端口、同一 Router**：管理路由由 standalone 注入（通过扩展点②），代理路由是 `ProxyServer::build_router()` 原有内容，二者 merge 后由 `ProxyServer::start()` 统一服务。

---

## 4. 组件设计

### 4.1 `standalone` 模块（新增，`src/standalone/mod.rs`）

lib crate 的内部模块，因此能自由访问 `pub(crate)` 项（`proxy::server::ProxyServer`、`Database`、`settings` 等）。对外只暴露一个 `pub fn run()` 给 bin 调用。

职责：
1. **CLI 参数解析**：`--db <path>`（默认 `~/.ccswitch/cli-proxy.db`）、`--address`/`--port`（默认 `127.0.0.1:15721`）、`--log-level`。
2. **DB 初始化**：调用新增的 `Database::open_at(path)`（§7 扩展点③）。空 DB 冷启动依赖 create_tables 自动 seed + DAO 兜底默认值，无需显式写入（见 §6.1）。
3. **构造 `ProxyConfig`**：基于 `ProxyConfig::default()`，用 CLI 参数覆盖 `listen_address` / `listen_port`。
4. **构建 admin router**：挂载 §4.3 的管理路由，类型为 `Router<crate::proxy::server::ProxyState>`。关键：handler 用 `State<ProxyState>` 取 db，但 router **不要**调 `.with_state(...)`——axum 要求 merge 双方 inner State 类型一致（均为 `ProxyState`），由 `ProxyServer::build_router` 末尾统一 `.with_state(self.state.clone())` 注入。若误写成 `Router<()>` 将无法 merge。
5. **组装并启动**：`ProxyServer::new(config, Arc::new(db), None).with_extra_routes(admin_router).start().await`。
6. **信号处理**：监听 Ctrl-C（`tokio::signal::ctrl_c()`）与 SIGTERM（Unix `tokio::signal::unix`；Windows 仅 Ctrl-C），调用 `ProxyServer::stop()` 优雅退出。退出码见 §9。

### 4.2 CLI 入口（`src/bin/cc_switch_proxy.rs`，新增）

极薄壳，仅初始化 tokio runtime 并转调：

```rust
fn main() {
    // 初始化日志（env_logger 或 tauri-plugin-log 的简化版）
    cc_switch_lib::standalone::run();
}
```

> `run()` 内部自带 `#[tokio::main]`，或由 main 提供 runtime——二选一，实现时定。

### 4.3 管理 API（admin，新增）

挂载于 `/admin/*` 前缀，复用 `ProxyState`（含 `db: Arc<Database>`）作 axum state。请求/响应均 JSON。

| 方法 | 路径 | 说明 | 对应 DAO |
|---|---|---|---|
| `GET` | `/admin/status` | 代理运行状态（端口、provider、请求数） | `ProxyServer::get_status()` |
| `GET` | `/admin/providers?app=codex` | 列出 provider | `db.get_all_providers("codex")` |
| `POST` | `/admin/providers` | 新增 provider（DTO 见 §5） | `db.save_provider("codex", &p)` |
| `DELETE` | `/admin/providers/:id` | 删除 provider | `db.delete_provider("codex", id)` |
| `POST` | `/admin/providers/:id/enable` | 设为当前启用 provider | `db.set_current_provider("codex", id)` |

> 具体 DAO 方法名以 `database/dao/providers.rs` 现有签名为准（已确认存在 `save_provider` / `delete_provider` / `set_current_provider` / `get_provider_by_id` / `get_provider_ids`，均 `pub fn`）。

### 4.4 代理服务（复用，不改）

`ProxyServer` 及其全部下游（`ProviderRouter`、`forwarder`、`handlers`、`providers/transform_*`、`providers/streaming_*`）原样复用。`ProviderRouter::select_providers()` 每次请求实时从 DB 读 provider 列表，因此管理 API 增删改后**立即生效**，无需重启。

---

## 5. 数据模型

### 5.1 admin API 创建 provider 的 DTO

基于 cc-switch 现成的 `UniversalProvider`（`provider.rs:633`）抽象，DTO 设计为面向「配置一个 Codex 上游」的简化结构：

```jsonc
// POST /admin/providers
{
  "name": "DeepSeek",
  "base_url": "https://api.deepseek.com",
  "api_key": "sk-...",
  "model": "deepseek-chat",
  "reasoning_effort": "high",      // 可选，默认 "high"
  "api_format": "openai_chat"      // "openai_chat" | "openai_responses"
}                                  // openai_chat 触发 Responses→Chat 转换
```

### 5.2 DTO → 内部 `Provider` 映射

handler 内部复用 `UniversalProvider`（零自写构造逻辑）：

1. `UniversalProvider::new(id, name, "custom".into(), base_url, api_key)`
2. 设 `apps.codex = true`、`models.codex = CodexModelConfig { model, reasoning_effort }`
3. 设 `meta.api_format = Some(api_format)`（Rust 字段名 `api_format`，serde 序列化名以 `ProviderMeta` 配置为准；决定是否触发协议转换，见 §8）
4. 调 `universal.to_codex_provider()`（`provider.rs:756`）—— 自动生成 `config.toml`、补 `/v1`、设 `wire_api`、组装 `auth.OPENAI_API_KEY`
   - 注意：生成 toml 里 `name = "NewAPI"` 是写死的（与 DTO 的 `name` 无关，仅影响 Codex 内部 provider 名）；生成的 `Provider.id` 形如 `universal-codex-{universal_id}`。DTO 的 `name` 存入 `Provider.name`（管理 API 列表 / 日志用），二者不冲突。
5. `db.save_provider("codex", &provider)`
6. 若请求带 `"enable": true`，额外 `db.set_current_provider("codex", &provider.id)`

> `to_codex_provider()` 生成的 config.toml 固定 `wire_api = "responses"`；对 Chat 上游（deepseek 等），代理通过 `meta.apiFormat = "openai_chat"` 触发 Responses→Chat 转换（判定见 `providers/codex.rs:28-74`），二者协同正确。

---

## 6. 数据流

### 6.1 启动流程

```
main → standalone::run()
  → 解析 CLI 参数
  → Database::open_at(db_path)              // 建表 + 迁移 + seed（CREATE TABLE IF NOT EXISTS）
  → （无需显式写入默认值）
       // create_tables() 自动 seed proxy_config 三行；
       // rectifier/optimizer/log 配置读取缺失时回落 impl Default；
       // get_proxy_config_for_app 空表时调 init_proxy_config_rows() 返回硬编码默认。
       // 故空 DB 冷启动不会 panic。
  → 构造 ProxyConfig（default + CLI 覆盖）
  → 构建 admin Router
  → ProxyServer::new(cfg, db, None)
        .with_extra_routes(admin_router)
        .start()                             // 绑定 127.0.0.1:port，spawn accept loop
  → 注册 Ctrl-C → stop()
  → 打印就绪日志（端口、DB 路径、示例 curl）
```

### 6.2 代理请求流程（Codex → 上游）

```
Codex CLI --POST /v1/responses--> ProxyServer (15721)
  → handlers::handle_responses
  → RequestContext（从 DB 读当前 provider、配置）
  → ProviderRouter::select_providers("codex")   // 实时读 DB
  → forwarder.forward_with_retry
       → CodexAdapter.extract_base_url / extract_auth
       → codex_provider_uses_chat_completions?   // api_format=openai_chat → 是
       → transform_codex_chat: Responses 请求体 → Chat 请求体
       → 端点改写 /responses → /chat/completions
       → 转发上游（hyper/reqwest）
  → 上游 Chat 响应（JSON 或 SSE）
       → 非流式: chat_completion_to_response_with_context → Responses JSON
       → 流式:   create_responses_sse_stream_from_chat → Responses SSE
  → 返回 Codex CLI
```

### 6.3 管理请求流程（用户的程序 → DB）

```
用户程序 --POST /admin/providers--> ProxyServer (15721)
  → admin handler（State<ProxyState>，取 state.db）
  → 解析 DTO → UniversalProvider → to_codex_provider() → Provider
  → db.save_provider("codex", &provider)
  → (可选) db.set_current_provider("codex", id)
  → 返回 { id, ... }
// 下一个 /v1/responses 请求即用到新 provider（ProviderRouter 实时读 DB）
```

---

## 7. 对现有源码的扩展点（核心：纯加法，不改逻辑）

全部为「新增 pub 项 / 字段」，不修改任何现有函数签名或行为路径。Tauri 应用走原有调用，运行表现与现状完全一致。

### 扩展点①  `src/lib.rs`（+1 行）

```rust
pub mod standalone;   // 新增，让 bin 能调到入口
```

### 扩展点②  `src/proxy/server.rs`（+约 8 行）：Router 注入

给 `ProxyServer` 增加注入 admin 路由的能力：

```rust
pub struct ProxyServer {
    config: ProxyConfig,
    state: ProxyState,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    extra_routes: Option<Router<ProxyState>>,   // ← 新增字段，默认 None
}

impl ProxyServer {
    /// builder：注入额外路由（admin API）。Tauri 用法不调用，行为不变。
    pub fn with_extra_routes(mut self, routes: Router<ProxyState>) -> Self {
        self.extra_routes = Some(routes);
        self
    }

    fn build_router(&self) -> Router {
        let router = Router::new()
            .route("/health", get(handlers::health_check))
            // ... 现有全部路由保持不变 ...
            ;
        let router = if let Some(extra) = &self.extra_routes {
            router.merge(extra.clone())        // ← 仅此处新增
        } else {
            router
        };
        router
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
            .with_state(self.state.clone())
    }
}
```

> 合并上游注意：即使上游改动了 `build_router` 的路由列表，我们的改动只在「路由列表之后、`with_state` 之前」插入一个 merge 分支，冲突面极小。

### 扩展点③  `src/database/mod.rs`（+约 15 行）：路径可配

```rust
impl Database {
    /// 在指定路径打开/创建 DB 并建表迁移。复用现有 create_tables / schema 迁移逻辑。
    /// 现有 init() 不动（仍指向 GUI 的 ~/.cc-switch/cc-switch.db）。
    pub fn open_at<P: AsRef<std::path::Path>>(path: P) -> Result<Self, AppError> {
        // 实现时把 init() 现有逻辑抽成私有 fn open_at_inner(path, register_hook: bool)，
        // init() 与 open_at() 共用，仅两点不同：
        //   1) db_path 从 get_app_config_dir().join("cc-switch.db") 换成入参；
        //   2) register_hook=false：standalone 不启动 webdav/s3 sync worker，
        //      故跳过 register_db_change_hook（避免向无 receiver 的 channel 发消息）。
        // 其余副作用（backup_database_file / ensure_model_pricing_seeded /
        // cleanup_old_stream_check_logs / rollup_and_prune / incremental_vacuum）
        // 原样复用——对独立 DB 无害，且保持与 GUI 一致的维护行为。
    }
}
```

> 实现方式：把 `init()` 现有逻辑抽成私有 `open_at_inner(path, register_hook)`，`init()` 与 `open_at()` 都调它，仅路径与 hook 注册开关不同。这属于「重构现有函数但保持行为」，合并时需留意上游对 `init` 的改动；若想完全零触碰 `init`，可在 `open_at` 里复制连接建立代码（略重复但 `init` 一字不改）。

---

## 8. 国产模型支持

无需为每个厂商写代码。复用 cc-switch 现有机制：

| 上游 | `api_format` | 代理行为 |
|---|---|---|
| 官方 OpenAI / Responses 兼容聚合 | `openai_responses`（或不设） | Responses 透传 |
| DeepSeek / Kimi / GLM / Qwen / MiniMax / 硅基流动 / 自建 vLLM | `openai_chat` | Responses → Chat 转换（请求/非流/流式三路径） |

**reasoning 参数兼容**已内置于 `providers/codex.rs:210-288`（`resolve_codex_chat_reasoning_config`，按 base_url/model 字符串匹配 deepseek/kimi/glm/qwen/minimax 等各自的 `thinking`/`effort` 参数方言），admin 创建 provider 后**自动生效**，无需额外配置。

> 未来若需厂商特化（如新的 reasoning 方言），通过 `UniversalProvider.meta.codex_chat_reasoning`（`CodexChatReasoningConfig`，已存在）显式声明即可，不必改代理核心。

---

## 9. 错误处理

- 复用现有 `ProxyError`（代理路径）与 `AppError`（DB 路径）。
- admin handler 把 `AppError` / 校验错误映射为合适的 HTTP 状态码：
  - 400：DTO 字段缺失 / `api_format` 非法 / base_url 格式错
  - 404：`DELETE`/`enable` 的 provider id 不存在
  - 500：DB 写入失败、`to_codex_provider()` 失败
- admin 响应统一 JSON：`{ "ok": bool, "error"?: string, "data"?: ... }`。
- 启动期错误（端口占用、DB 路径不可写）以非零退出码 + stderr 日志终止。退出码映射：

| exit | 触发条件 |
|------|----------|
| 0 | 正常停止（Ctrl-C / SIGTERM → `ProxyServer::stop()` 成功） |
| 2 | CLI 参数解析错误 |
| 3 | `Database::open_at` 失败（路径不可写 / 建表失败）→ `AppError` |
| 4 | `ProxyServer::start` 失败（`BindFailed` 端口占用 / `AlreadyRunning`） |

---

## 10. 测试策略

- **单元测试**（standalone 模块内）：
  - DTO → `UniversalProvider` → `to_codex_provider()` → `Provider` 的字段映射正确性（base_url 补 `/v1`、`api_format` 透传、auth 写入）。
  - 复用 cc-switch 既有测试风格（`#[cfg(test)] mod tests`）。
- **集成测试**（`src-tauri/tests/` 或 standalone 内 `#[cfg(test)]`）：
  - 用临时 sqlite DB（tempfile），`Database::open_at` 建表成功。
  - 起一个内存/随机端口的 ProxyServer，模拟一次 Codex `/v1/responses` 请求，mock 上游返回 Chat SSE，断言转换后的 Responses SSE 正确。
- **手工验收脚本**：文档附 curl 示例（创建 deepseek provider → enable → 用 codex CLI 连代理发一条请求）。

> 现有 cc-switch 已有针对 `transform_codex_chat` / `streaming_codex_chat` 的测试，本设计不改动它们，回归由既有测试保障。

---

## 11. 合并上游策略

- 所有新增代码集中在：`src/standalone/`、`src/bin/cc_switch_proxy.rs`、`Cargo.toml` 的 `[[bin]]`。
- 对现有文件的改动仅 §7 三处，且每处都用醒目注释标记，例如：
  ```rust
  // [cc-switch-proxy] exposed for standalone binary; see docs/superpowers/specs/2026-07-07-...md
  pub mod standalone;
  ```
- 合并上游时：新增文件无冲突；三处扩展点即使冲突也只是「pub 声明 / merge 分支」级别，手动解决代价极低；上游对 `build_router`/`init` 的改动不影响扩展点的 merge 位置。
- 建议 fork 后立即建一个 `cc-switch-proxy` 特性的长期分支，定期 rebase 上游 main，验证三处扩展点仍可干净 apply。

---

## 12. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| bin 链接整个 lib（含 tauri/rusqlite/rquickjs），编译慢、体积大 | 编译体验 | 本期接受；后续可用 cargo feature 把 tauri 那部分 gate 掉（非必需，列为未来扩展） |
| 管理 API 无鉴权，本机任何进程可改 provider | 安全（仅本地） | 只绑定 `127.0.0.1`（绝不绑 `0.0.0.0`）；文档明确告知；未来可加 `--token` |
| 独立 DB 与 GUI DB 不互通 | 用户需在 CLI 侧重新配 provider | 本期按用户决定接受（独立 DB 换来可与 GUI 并存）；未来可加 `--db` 指向 GUI DB 实现共享（但禁止同时开） |
| `app_handle=None` 时 failover 的 UI 回路（托盘/事件）跳过 | 故障转移仍工作（写 DB），仅无 UI 通知 | CLI 场景无需 UI；状态可通过 `/admin/status` 查询 |
| `to_codex_provider()` 固定 `wire_api=responses`，依赖 `apiFormat=openai_chat` 触发转换 | 配置错误时可能 404 | admin DTO 强制 `api_format` 字段 + 文档示例；集成测试覆盖 chat 上游路径 |
| cc-switch 上游重构 ProxyServer/Database 导致扩展点失效 | 合并冲突 | 三处扩展点集中在稳定结构上（`ProxyServer` 字段、`build_router`、`Database::init`），变动概率低；rebase 时人工核对 |

---

## 13. 未来扩展（不在本期）

- Claude / Gemini 客户端方向的完整支持与测试。
- CLI 子命令（`provider add/list/enable`）、交互式 TUI。
- 管理 API 鉴权（`--token`）。
- cargo feature gate 剥离 tauri 依赖，缩小 bin 体积。
- 配置文件（TOML）作为管理 API 的补充初始化方式。
- 共享 GUI DB 模式（带互斥锁防并发写）。

---

## 附录 A：关键文件与行号索引（基于探索时仓库状态）

| 内容 | 位置 |
|---|---|
| `ProxyServer::new` / `start` / `build_router` | `src-tauri/src/proxy/server.rs:62-360` |
| `ProxyState`（字段全 pub，`#[derive(Clone)]`） | `src-tauri/src/proxy/server.rs:32-51` |
| `ProxyConfig`（有 `Default`，端口 15721） | `src-tauri/src/proxy/types.rs:4-56` |
| `Database::init`（路径写死） | `src-tauri/src/database/mod.rs:96` |
| provider DAO（`save_provider` 等） | `src-tauri/src/database/dao/providers.rs:130-523` |
| `UniversalProvider` + `to_codex_provider()` | `src-tauri/src/provider.rs:633-854` |
| Codex 协议转换核心 | `src-tauri/src/proxy/providers/transform_codex_chat.rs`、`streaming_codex_chat.rs` |
| Codex chat 上游判定 / reasoning 兼容 | `src-tauri/src/proxy/providers/codex.rs:28-74,210-288` |
| `lib.rs` 模块声明（全私有 `mod`） | `src-tauri/src/lib.rs:1-38` |
