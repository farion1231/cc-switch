# CC-Switch Proxy Standalone CLI 实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 cc-switch 现有 crate 内新增一个 headless CLI `cc-switch-proxy`，不依赖 Tauri 启动本地代理（Codex 协议转换）+ 管理 API，数据落独立 sqlite DB。

**Architecture:** 新增 `standalone` 模块（lib 内部，可访问 `pub(crate)`）+ `bin/cc_switch_proxy.rs` 薄壳；复用现有 `ProxyServer`/`Database`/协议转换；对现有源码仅 3 处纯加法扩展（`lib.rs` 加 mod、`server.rs` 加路由注入、`database` 加 `open_at`）。

**Tech Stack:** Rust 2021 / axum 0.7 / tokio / hyper / rusqlite / serde_json。新增依赖：`env_logger`。CLI 参数手写解析（不引入 clap）。

**参考文档:** `docs/superpowers/specs/2026-07-07-cc-switch-proxy-standalone-cli-design.md`
**分支:** `feat/cc-switch-proxy-cli`
**约定:** 所有 cargo 命令在 `src-tauri/` 目录运行（或加 `--manifest-path src-tauri/Cargo.toml`）。每个 Task 末尾 commit。

---

## File Structure

| 文件 | 动作 | 职责 |
|---|---|---|
| `src-tauri/src/database/mod.rs` | 修改 | 扩展点③：新增 `open_at` + 抽 `open_at_inner`（DRY 重构 `init`） |
| `src-tauri/src/proxy/server.rs` | 修改 | 扩展点②：`ProxyServer` 加 `extra_routes` 字段 + `with_extra_routes` + `build_router` merge |
| `src-tauri/src/lib.rs` | 修改 | 扩展点①：加 `pub mod standalone;` |
| `src-tauri/src/standalone/mod.rs` | 新增 | `run()` 入口、CLI 参数解析、DB 初始化、组装 ProxyServer、信号处理 |
| `src-tauri/src/standalone/admin.rs` | 新增 | admin DTO + handler + `build_admin_router()` |
| `src-tauri/src/bin/cc_switch_proxy.rs` | 新增 | 薄壳 main |
| `src-tauri/Cargo.toml` | 修改 | 加 `[[bin]]` + `env_logger` 依赖 |

---

## Chunk 1: 数据库扩展点③（`open_at`）

### Task 1: `Database::open_at` —— 路径可配 + TDD

**Files:**
- Modify: `src-tauri/src/database/mod.rs`（`init` 当前在 96-160 行）
- Test: `src-tauri/src/database/mod.rs` 的 `#[cfg(test)] mod tests`

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/database/mod.rs` 的 test 模块（文件末尾 `mod tests`，约 2055 行起）加入：

```rust
#[test]
fn open_at_creates_tables_at_custom_path() {
    let tmp = tempfile::NamedTempFile::new().expect("create temp file");
    let path = tmp.path().with_extension("db");
    let db = Database::open_at(&path).expect("open_at should succeed");

    // 关键表应已创建
    let conn = db.conn.lock().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='providers'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "providers 表应已创建");
    let pricing: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='model_pricing'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pricing, 1, "model_pricing 表应已创建");
    drop(conn);
}
```

- [ ] **Step 2: 运行测试，确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml open_at_creates_tables_at_custom_path
```
预期：编译失败，`no function named open_at`。

- [ ] **Step 3: 实现 `open_at` + 重构 `init`**

把现有 `init()`（mod.rs:96-160）的主体抽到私有 `open_at_inner(db_path, register_hook)`，`init` 与新 `open_at` 都调它。在 `init` 上方插入：

```rust
/// 在指定路径打开/创建 DB 并建表迁移。standalone CLI 用，与 GUI 的 `init()` 隔离。
///
/// 复用 `open_at_inner` 的全部建表/迁移/seed/维护逻辑，仅跳过 `register_db_change_hook`
/// （standalone 不启动 webdav/s3 sync worker，避免向无 receiver 的 channel 发消息）。
pub fn open_at<P: AsRef<std::path::Path>>(path: P) -> Result<Self, AppError> {
    Self::open_at_inner(path.as_ref(), false)
}
```

把现有 `init()` 整体替换为：

```rust
pub fn init() -> Result<Self, AppError> {
    Self::open_at_inner(&get_app_config_dir().join("cc-switch.db"), true)
}

fn open_at_inner(db_path: &std::path::Path, register_hook: bool) -> Result<Self, AppError> {
    let db_exists = db_path.exists();

    // 确保父目录存在
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let conn = Connection::open(db_path).map_err(|e| AppError::Database(e.to_string()))?;

    // 启用外键约束
    conn.execute("PRAGMA foreign_keys = ON;", [])
        .map_err(|e| AppError::Database(e.to_string()))?;
    if !db_exists {
        conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
    }
    if register_hook {
        register_db_change_hook(&conn);
    }

    let db = Self {
        conn: Mutex::new(conn),
    };
    db.create_tables()?;

    // Pre-migration backup: only when upgrading from an existing database
    {
        let conn = lock_conn!(db.conn);
        let version = Self::get_user_version(&conn)?;
        drop(conn);
        if version > 0 && version < SCHEMA_VERSION {
            log::info!(
                "Creating pre-migration database backup (v{version} → v{SCHEMA_VERSION})"
            );
            if let Err(e) = db.backup_database_file() {
                log::warn!("Pre-migration backup failed, continuing migration: {e}");
            }
        }
    }

    db.apply_schema_migrations()?;
    if let Err(e) = db.ensure_incremental_auto_vacuum() {
        log::warn!("Failed to ensure incremental auto-vacuum: {e}");
    }
    db.ensure_model_pricing_seeded()?;

    if let Err(e) = db.cleanup_old_stream_check_logs(7) {
        log::warn!("Startup stream_check_logs cleanup failed: {e}");
    }
    if let Err(e) = db.rollup_and_prune(30) {
        log::warn!("Startup rollup_and_prune failed: {e}");
    }
    {
        let conn = lock_conn!(db.conn);
        if let Err(e) = conn.execute_batch("PRAGMA incremental_vacuum;") {
            log::warn!("Startup incremental vacuum failed: {e}");
        }
    }

    Ok(db)
}
```

> 注意：`init` 的行为完全不变（仍传 `register_hook=true`），只是主体移到 `open_at_inner`。这是 DRY 重构。

- [ ] **Step 4: 运行测试，确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml open_at_creates_tables_at_custom_path
cargo test --manifest-path src-tauri/Cargo.toml --lib database
```
预期：新测试 PASS，现有 database 测试不回归。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/database/mod.rs
git commit -m "feat(database): add Database::open_at for standalone CLI path

Refactor init() into open_at_inner(path, register_hook); open_at reuses
all table/migration/seed logic but skips the webdav/s3 db-change hook.
init() behavior unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Chunk 2: 服务器扩展点②（`extra_routes` 路由注入）

### Task 2: `ProxyServer::with_extra_routes` —— 同端口注入 admin 路由

**Files:**
- Modify: `src-tauri/src/proxy/server.rs`（`ProxyServer` 结构在 54-60，`build_router` 在 291-360）

- [ ] **Step 1: 加 `extra_routes` 字段**

`server.rs:54-60` 的 `ProxyServer` struct 加一个字段：

```rust
pub struct ProxyServer {
    config: ProxyConfig,
    state: ProxyState,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    /// [cc-switch-proxy] 额外路由（admin API），由 standalone 注入；默认 None，Tauri 用法不受影响
    extra_routes: Option<Router<ProxyState>>,
}
```

- [ ] **Step 2: `new` 里初始化字段**

`server.rs:86-91` 的 `Self { ... }` 末尾加：

```rust
        Self {
            config,
            state,
            shutdown_tx: Arc::new(RwLock::new(None)),
            server_handle: Arc::new(RwLock::new(None)),
            extra_routes: None,
        }
```

- [ ] **Step 3: 加 `with_extra_routes` builder 方法**

在 `impl ProxyServer` 中（`new` 之后）加入：

```rust
    /// [cc-switch-proxy] 注入额外路由（如 admin API），与代理路由 merge 后由
    /// `build_router` 统一 `with_state`。调用方传入的 router 必须是
    /// `Router<ProxyState>` 且**不要**自行 `with_state`。
    pub fn with_extra_routes(mut self, routes: Router<ProxyState>) -> Self {
        self.extra_routes = Some(routes);
        self
    }
```

- [ ] **Step 4: `build_router` 末尾 merge**

`server.rs:291` 的 `build_router`，把最后的：

```rust
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
            .with_state(self.state.clone())
```

改为：

```rust
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024));
        let router = if let Some(extra) = &self.extra_routes {
            router.merge(extra.clone())
        } else {
            router
        };
        router.with_state(self.state.clone())
```

> 注意：`build_router` 内部 `Router::new().route(...)` 链的结尾原本是 `.layer(...).with_state(...)`。改为先 `.layer(...)` 结束链、赋值给 `router`，再条件 merge，最后 `with_state`。保持现有所有路由不变。

- [ ] **Step 5: 验证编译 + 现有应用不回归**

```bash
cargo build --manifest-path src-tauri/Cargo.toml --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::server
```
预期：编译通过；`extra_routes` 默认 None，现有 server 测试不回归。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/proxy/server.rs
git commit -m "feat(proxy): allow ProxyServer to merge extra routes

Add extra_routes field + with_extra_routes builder + build_router merge.
Default None; Tauri usage unchanged. Enables standalone CLI to inject
admin routes on the same port.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Chunk 3: standalone 模块骨架 + 扩展点① + 组装

### Task 3: `standalone` 模块 + CLI 参数 + DB/ProxyServer 组装

**Files:**
- Create: `src-tauri/src/standalone/mod.rs`
- Modify: `src-tauri/src/lib.rs`（扩展点①）

- [ ] **Step 1: 创建 `standalone/mod.rs`**

```rust
//! Headless standalone 运行入口：不依赖 Tauri，启动本地代理 + 管理 API。
//!
//! 作为 lib 内部模块，可访问 `pub(crate)` 项（ProxyServer / Database 等）。

pub mod admin;

use std::sync::Arc;

use crate::database::Database;
use crate::proxy::server::ProxyServer;
use crate::proxy::types::ProxyConfig;

/// CLI 参数。
struct CliArgs {
    db_path: std::path::PathBuf,
    address: String,
    port: u16,
}

/// 解析 CLI 参数。手写，避免引入 clap。
///
/// 支持：--db <path>、--address <ip>、--port <num>、-h/--help。
/// 解析失败返回 None（main 走退出码 2）。
fn parse_cli_args() -> Option<CliArgs> {
    let mut db_path = dirs::config_dir()
        .map(|d| d.join("cc-switch").join("cli-proxy.db"))
        .unwrap_or_else(|| std::path::PathBuf::from("cli-proxy.db"));
    let mut address = "127.0.0.1".to_string();
    let mut port: u16 = 15721;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let Some(v) = args.next() else {
                    eprintln!("--db 需要参数");
                    return None;
                };
                db_path = std::path::PathBuf::from(v);
            }
            "--address" => {
                address = args.next().unwrap_or(address);
            }
            "--port" => {
                let Some(v) = args.next() else {
                    eprintln!("--port 需要参数");
                    return None;
                };
                port = v.parse().map_err(|_| ()).ok()?;
            }
            "-h" | "--help" => {
                eprintln!("用法: cc-switch-proxy [--db <path>] [--address <ip>] [--port <num>]");
                return None;
            }
            other => {
                eprintln!("未知参数: {other}");
                return None;
            }
        }
    }

    Some(CliArgs {
        db_path,
        address,
        port,
    })
}

/// 启动 standalone 代理。返回进程退出码（见 spec §9）。
pub async fn run() -> i32 {
    let Some(args) = parse_cli_args() else {
        return 2;
    };

    let db = match Database::open_at(&args.db_path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("[cc-switch-proxy] 数据库初始化失败 ({}): {e}", args.db_path.display());
            return 3;
        }
    };

    let config = ProxyConfig {
        listen_address: args.address.clone(),
        listen_port: args.port,
        ..ProxyConfig::default()
    };

    let admin_router = admin::build_admin_router();
    let server = ProxyServer::new(config, db, None).with_extra_routes(admin_router);

    let info = match server.start().await {
        Ok(info) => info,
        Err(e) => {
            eprintln!("[cc-switch-proxy] 代理启动失败: {e}");
            return 4;
        }
    };

    log::info!(
        "[cc-switch-proxy] 已启动：http://{}:{}  （DB: {}）",
        info.address,
        info.port,
        args.db_path.display()
    );
    eprintln!(
        "[cc-switch-proxy] 监听 http://{}:{}  管理 API: POST http://127.0.0.1:{}/admin/providers",
        info.address, info.port, info.port
    );

    // 等待停止信号
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }

    log::info!("[cc-switch-proxy] 收到停止信号，正在关闭…");
    if let Err(e) = server.stop().await {
        eprintln!("[cc-switch-proxy] 停止异常: {e}");
    }
    0
}
```

- [ ] **Step 2: 扩展点① —— lib.rs 加 mod 声明**

`src-tauri/src/lib.rs` 在 `mod proxy;`（约 30 行）附近加入：

```rust
// [cc-switch-proxy] exposed for standalone binary; see docs/superpowers/specs/2026-07-07-cc-switch-proxy-standalone-cli-design.md
pub mod standalone;
```

- [ ] **Step 3: 占位 `admin` 子模块（Chunk 4 填充）**

创建 `src-tauri/src/standalone/admin.rs` 占位，使 `mod.rs` 的 `pub mod admin;` 能编译：

```rust
//! 管理 API（Chunk 4 实现）。
//!
//! 占位：返回空 Router，使 standalone 模块在 Chunk 3 阶段可编译。

use crate::proxy::server::ProxyState;
use axum::Router;

pub fn build_admin_router() -> Router<ProxyState> {
    Router::new()
}
```

- [ ] **Step 4: 验证 lib 编译**

```bash
cargo build --manifest-path src-tauri/Cargo.toml --lib
```
预期：编译通过（`admin::build_admin_router` 返回空 Router，`with_extra_routes` 接受）。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/standalone/ src-tauri/src/lib.rs
git commit -m "feat(standalone): add headless run() entry + CLI parsing

standalone module (lib-internal) parses --db/--address/--port, opens DB via
open_at, assembles ProxyServer with admin routes merged, handles SIGTERM/Ctrl-C.
lib.rs exposes pub mod standalone. admin module is a placeholder (filled in
next chunk).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Chunk 4: 管理 API（admin）

### Task 4: admin DTO + 映射 + handlers + router（TDD）

**Files:**
- Modify: `src-tauri/src/standalone/admin.rs`

- [ ] **Step 1: 写 DTO→Provider 映射的失败测试**

在 `src-tauri/src/standalone/admin.rs` 末尾加测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::UniversalProviderApps;

    #[test]
    fn dto_maps_to_codex_provider_with_chat_format() {
        let dto = CreateProviderRequest {
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com".into(),
            api_key: "sk-test".into(),
            model: "deepseek-chat".into(),
            reasoning_effort: None,
            api_format: Some("openai_chat".into()),
            enable: false,
        };
        let provider = build_codex_provider_from_dto(&dto).expect("map dto");

        // id 形如 universal-codex-*
        assert!(provider.id.starts_with("universal-codex-"));
        assert_eq!(provider.name, "DeepSeek");
        // api_key 进 auth.OPENAI_API_KEY
        assert_eq!(
            provider.settings_config.pointer("/auth/OPENAI_API_KEY").and_then(|v| v.as_str()),
            Some("sk-test")
        );
        // config toml 含 base_url（补 /v1）+ model
        let toml = provider.settings_config.get("config").and_then(|v| v.as_str()).unwrap();
        assert!(toml.contains("base_url = \"https://api.deepseek.com/v1\""));
        assert!(toml.contains("model = \"deepseek-chat\""));
        // api_format 透传到 meta
        assert_eq!(
            provider.meta.as_ref().unwrap().api_format.as_deref(),
            Some("openai_chat")
        );
    }

    #[test]
    fn invalid_api_format_rejected() {
        let dto = CreateProviderRequest {
            name: "X".into(),
            base_url: "https://x".into(),
            api_key: "k".into(),
            model: "m".into(),
            reasoning_effort: None,
            api_format: Some("bogus".into()),
            enable: false,
        };
        assert!(build_codex_provider_from_dto(&dto).is_err());
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib standalone::admin
```
预期：编译失败（`CreateProviderRequest` / `build_codex_provider_from_dto` 未定义）。

- [ ] **Step 3: 实现 DTO + 映射 + handlers + router**

把 `src-tauri/src/standalone/admin.rs` 替换为完整实现：

```rust
//! 管理 API：通过 HTTP 增删改查 provider，写入独立 DB。
//!
//! 路由挂载于 /admin/*，与代理路由共用 ProxyState（含 db）作 axum state。
//! 调用方（standalone）构建 Router<ProxyState> 但不 with_state，由
//! ProxyServer::build_router 统一注入。

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta, UniversalProvider, UniversalProviderApps};
use crate::proxy::server::ProxyState;

const CODEX: &str = "codex";

/// 创建 provider 的请求 DTO。
#[derive(Debug, Deserialize)]
pub struct CreateProviderRequest {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    /// "openai_chat" 触发 Responses→Chat 转换；"openai_responses" 透传。
    pub api_format: Option<String>,
    /// true 时同时设为当前 provider。
    #[serde(default)]
    pub enable: bool,
}

/// 列表项（脱敏，不含 api_key）。
#[derive(Debug, Serialize)]
struct ProviderSummary {
    id: String,
    name: String,
    base_url: String,
    model: String,
    api_format: Option<String>,
    is_current: bool,
}

/// DTO → cc-switch Provider，复用 UniversalProvider::to_codex_provider()。
fn build_codex_provider_from_dto(dto: &CreateProviderRequest) -> Result<Provider, String> {
    let api_format = match dto.api_format.as_deref() {
        Some("openai_chat") => "openai_chat",
        Some("openai_responses") => "openai_responses",
        Some(other) => return Err(format!("不支持的 api_format: {other}")),
        None => "openai_responses",
    };

    let id = format!("cli-{}", short_random_id());
    let mut universal = UniversalProvider::new(
        id.clone(),
        dto.name.clone(),
        "custom".to_string(),
        dto.base_url.clone(),
        dto.api_key.clone(),
    );
    universal.apps = UniversalProviderApps {
        codex: true,
        ..Default::default()
    };
    universal.models.codex = Some(crate::provider::CodexModelConfig {
        model: Some(dto.model.clone()),
        reasoning_effort: dto.reasoning_effort.clone().or(Some("high".into())),
    });
    universal.meta = Some(ProviderMeta {
        api_format: Some(api_format.to_string()),
        ..Default::default()
    });

    universal
        .to_codex_provider()
        .ok_or_else(|| "to_codex_provider 返回 None（apps.codex 未启用）".to_string())
}

/// 生成短随机 id（无 Math.random 限制——这是运行时代码，可用 std）。
fn short_random_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")[..8.min(format!("{nanos:x}").len())].to_string()
}

/// AppError → (StatusCode, message)。
fn map_err(e: AppError) -> (StatusCode, String) {
    let msg = e.to_string();
    // 简单按关键字分类；DB 路径错误一律 500，业务校验错误 400。
    let status = if msg.contains("not found") || msg.contains("不存在") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, msg)
}

pub fn build_admin_router() -> Router<ProxyState> {
    Router::new()
        .route("/admin/providers", get(list_providers).post(create_provider))
        .route("/admin/providers/:id", axum::routing::delete(delete_provider))
        .route("/admin/providers/:id/enable", post(enable_provider))
        .route("/admin/status", get(status))
}

async fn create_provider(
    State(state): State<ProxyState>,
    Json(dto): Json<CreateProviderRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let provider = build_codex_provider_from_dto(&dto).map_err(|m| (StatusCode::BAD_REQUEST, m))?;
    state
        .db
        .save_provider(CODEX, &provider)
        .map_err(map_err)?;
    if dto.enable {
        state
            .db
            .set_current_provider(CODEX, &provider.id)
            .map_err(map_err)?;
    }
    Ok(Json(json!({ "ok": true, "id": provider.id, "name": provider.name })))
}

async fn list_providers(State(state): State<ProxyState>) -> Result<Json<Value>, (StatusCode, String)> {
    let all = state.db.get_all_providers(CODEX).map_err(map_err)?;
    let current = state.db.get_current_provider(CODEX).unwrap_or(None);
    let items: Vec<ProviderSummary> = all
        .values()
        .map(|p| {
            let (base_url, _api_key) = p.resolve_usage_credentials(&AppType::Codex);
            let model = p
                .settings_config
                .get("config")
                .and_then(|c| c.as_str())
                .and_then(extract_model_from_toml)
                .unwrap_or_default();
            ProviderSummary {
                id: p.id.clone(),
                name: p.name.clone(),
                base_url,
                model,
                api_format: p.meta.as_ref().and_then(|m| m.api_format.clone()),
                is_current: current.as_deref() == Some(&p.id),
            }
        })
        .collect();
    Ok(Json(json!({ "providers": items })))
}

async fn delete_provider(
    State(state): State<ProxyState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state.db.delete_provider(CODEX, &id).map_err(map_err)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn enable_provider(
    State(state): State<ProxyState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    // 校验存在性
    if state
        .db
        .get_provider_by_id(&id, CODEX)
        .map_err(map_err)?
        .is_none()
    {
        return Err((StatusCode::NOT_FOUND, format!("provider 不存在: {id}")));
    }
    state
        .db
        .set_current_provider(CODEX, &id)
        .map_err(map_err)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn status(State(state): State<ProxyState>) -> impl IntoResponse {
    let s = state.status.read().await.clone();
    Json(serde_json::to_value(s).unwrap_or(json!({})))
}

/// 从 codex config.toml 文本粗提取 model = "..." 的值（仅用于列表展示）。
fn extract_model_from_toml(toml: &str) -> Option<String> {
    let line = toml.lines().find(|l| l.trim_start().starts_with("model "))?;
    let v = line.split('=').nth(1)?;
    let v = v.trim().trim_matches('"');
    Some(v.to_string())
}
```

> 注意点：
> - `get_provider_by_id` 签名是 `(id, app_type)`（id 在前），其余是 `(app_type, id)`——已按源码顺序调用。
> - `resolve_usage_credentials(&AppType::Codex)` 复用现有凭证提取，从 config.toml 解析 base_url。
> - `ProviderSummary` 不回传 api_key（脱敏）。

- [ ] **Step 4: 运行测试，确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib standalone::admin
```
预期：两个测试 PASS（dto 映射正确、非法 api_format 被拒）。

- [ ] **Step 5: 验证整体 lib 编译**

```bash
cargo build --manifest-path src-tauri/Cargo.toml --lib
```
预期：通过。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/standalone/admin.rs
git commit -m "feat(standalone): implement admin API (providers CRUD + status)

POST/GET/DELETE /admin/providers, POST /admin/providers/:id/enable,
GET /admin/status. DTO maps to UniversalProvider::to_codex_provider.
api_format=openai_chat triggers Responses->Chat conversion.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Chunk 5: bin + Cargo.toml + 端到端验证

### Task 5: 薄壳 main + `[[bin]]` + 编译 + 端到端

**Files:**
- Create: `src-tauri/src/bin/cc_switch_proxy.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 加 `env_logger` 依赖 + `[[bin]]`**

`src-tauri/Cargo.toml`：
- 在 `[dependencies]` 段加：`env_logger = "0.11"`
- 在 `[lib]` 段之后加：

```toml
[[bin]]
name = "cc-switch-proxy"
path = "src/bin/cc_switch_proxy.rs"
```

- [ ] **Step 2: 创建 bin main**

`src-tauri/src/bin/cc_switch_proxy.rs`：

```rust
//! cc-switch-proxy: headless 本地代理 + 管理 API，不依赖 Tauri。

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp_secs()
    .init();

    let exit_code = cc_switch_lib::standalone::run().await;
    std::process::exit(exit_code);
}
```

> `cc_switch_lib` 是 `[lib] name`（见 Cargo.toml:14）。

- [ ] **Step 3: 编译 bin**

```bash
cargo build --manifest-path src-tauri/Cargo.toml --bin cc-switch-proxy
```
预期：编译通过（首次较慢，链接整个 lib）。产物在 `src-tauri/target/debug/cc-switch-proxy`（或 `.exe`）。

- [ ] **Step 4: 端到端冒烟验证**

启动（用一个临时 DB + 随机端口，避免与已有实例冲突）：

```bash
./src-tauri/target/debug/cc-switch-proxy --db /tmp/cli-proxy-smoke.db --port 15921 &
PROXY_PID=$!
sleep 2
```

通过管理 API 创建一个 mock 友好的 provider（用一个本地 mock 上游，或直接用一个公开 chat 兼容端点；此处用占位，验证 API 通畅）：

```bash
curl -s http://127.0.0.1:15921/admin/status | head
# 预期：返回 {"running":true,"port":15921,...}

curl -s -X POST http://127.0.0.1:15921/admin/providers \
  -H 'Content-Type: application/json' \
  -d '{"name":"Test","base_url":"https://api.deepseek.com","api_key":"sk-test","model":"deepseek-chat","api_format":"openai_chat","enable":true}'
# 预期：{"ok":true,"id":"universal-codex-cli-...","name":"Test"}

curl -s 'http://127.0.0.1:15921/admin/providers'
# 预期：providers 列表含刚创建的，is_current=true

kill $PROXY_PID
```

验证点：status 返回 running；创建 provider 成功且返回 `universal-codex-` 前缀 id；列表含该 provider 且 is_current=true。

> 真实的 Codex→上游 转换链路验证（需要有效 api_key 上游）作为手工验收，不在自动化测试内（见 spec §10）。可选：写一个集成测试用 `wiremock` mock 一个 chat 上游，断言 `/v1/responses` 转换正确——若时间允许补，否则留作后续。

- [ ] **Step 5: 回归现有 Tauri 应用编译**

```bash
cargo build --manifest-path src-tauri/Cargo.toml --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib
```
预期：lib 编译通过，全部测试通过（扩展点未改变现有行为）。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/bin/cc_switch_proxy.rs src-tauri/Cargo.toml
git commit -m "feat(standalone): add cc-switch-proxy binary + env_logger

Wire thin main -> standalone::run under tokio. Cargo [[bin]] target.
End-to-end: /admin/status + /admin/providers CRUD verified via curl.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 完成标准（Definition of Done）

- [ ] `cargo build --bin cc-switch-proxy` 成功。
- [ ] `cargo test --lib` 全绿（含新增的 open_at、admin 映射测试，且现有测试不回归）。
- [ ] `cc-switch-proxy` 启动后 `GET /admin/status` 返回 running。
- [ ] `POST /admin/providers` 能创建 deepseek 类 provider（`api_format=openai_chat`），`enable` 后 `GET /admin/providers` 显示 is_current=true。
- [ ] 现有 Tauri 应用编译与行为不受影响（`init()` 行为不变、`ProxyServer` 默认不 merge 额外路由）。
- [ ] 对现有源码的改动仅 3 处（lib.rs / server.rs / database/mod.rs），均为纯加法/等价重构。
