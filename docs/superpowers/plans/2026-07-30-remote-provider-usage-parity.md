# Remote Provider and Usage Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Provider management and the complete Usage dashboard operate on the selected local or SSH target with identical database semantics and strict cross-target isolation.

**Architecture:** Move canonical Provider/Usage data access and headless live-file behavior into `cc-switch-core`, then let both desktop Tauri commands and `cc-switch-agent` call those shared services. Extend the explicit RPC registry by domain, attach runtime generation to remote requests, and scope frontend queries by runtime target so no command silently falls back to the local host.

**Tech Stack:** Rust 2021, rusqlite, serde, Tauri 2, custom SSH stdio RPC, React 18, TypeScript, TanStack Query 5, Vitest.

---

## File Structure

The implementation must keep each boundary focused:

- `src-tauri/crates/cc-switch-core/src/state.rs`: explicit HOME and SQLite connection lifecycle.
- `src-tauri/crates/cc-switch-core/src/schema.rs`: canonical required-table initialization and compatibility validation.
- `src-tauri/crates/cc-switch-core/src/error.rs`: stable headless business errors and codes.
- `src-tauri/crates/cc-switch-core/src/provider/{mod.rs,model.rs,repository.rs,live.rs}`: complete Provider DTO, canonical SQL, transactions, and platform-aware live projection.
- `src-tauri/crates/cc-switch-core/src/usage/{mod.rs,model.rs,query.rs,mutation.rs,script.rs}`: Usage DTOs, shared aggregate SQL, pricing/limits, scripts, and cancellable long-operation entry points.
- `src-tauri/crates/cc-switch-core/src/dispatch.rs`: typed domain dispatch shared by Agent tests and stdio entry point.
- `src-tauri/crates/cc-switch-protocol/src/capabilities.rs`: explicit Provider/Usage capability metadata.
- `src-tauri/crates/cc-switch-agent/src/lib.rs`: protocol loop only; delegates business commands to Core.
- `src-tauri/src/commands/{provider.rs,usage.rs}` and `src-tauri/src/services/usage_stats.rs`: thin desktop adapters over Core.
- `src-tauri/src/remote/{runtime.rs,client.rs,ssh.rs}` and `src-tauri/src/commands/remote.rs`: generation-aware remote calls and stale-response rejection.
- `src/lib/api/usage.ts`: local/remote command mapping through `appInvoke`.
- `src/lib/runtime/{invoke.ts,queryScope.ts}`: request generation and stable query scope.
- `src/lib/query/{usage.ts,queries.ts}`: target-scoped Provider/Usage query keys.

## Task 1: Preserve the verified SSH bootstrap fix

**Files:**
- Modify: `src-tauri/src/remote/ephemeral_deploy.rs`
- Modify: `src-tauri/tests/remote_ephemeral_deploy.rs`

- [ ] **Step 1: Stop the repository Tauri development process tree**

Resolve the running `src-tauri/target/debug/cc-switch.exe` parent chain and stop only its repository-owned Tauri/Vite descendants and ancestors. Record the two embedded Agent environment paths; Windows locks the desktop build output while the process is running, so Rust RED/GREEN results are not trustworthy until this tree exits.

- [ ] **Step 2: Re-run the regression test before committing**

Run:

```powershell
cd src-tauri
$env:CC_SWITCH_AGENT_X86_64_PATH=(Resolve-Path 'target/downloaded-agent-artifacts/linux-x86_64/cc-switch-agent').Path
$env:CC_SWITCH_AGENT_AARCH64_PATH=(Resolve-Path 'target/downloaded-agent-artifacts/linux-aarch64/cc-switch-agent').Path
cargo test -j 1 --test remote_ephemeral_deploy -- --nocapture
```

Expected: 9 tests pass, including `launch_command_does_not_overwrite_zsh_path_parameter` and `remote_commands_avoid_double_quotes_preserved_by_windows_openssh`.

- [ ] **Step 3: Verify formatting and diff scope**

Run:

```powershell
cargo fmt --check
cd ..
git diff --check
git diff --name-only
```

Expected: formatting and diff checks pass; only the two SSH files above are uncommitted product changes.

- [ ] **Step 4: Commit the SSH fix separately**

```powershell
git add src-tauri/src/remote/ephemeral_deploy.rs src-tauri/tests/remote_ephemeral_deploy.rs
git commit -m "fix(remote): support zsh agent bootstrap"
```

Expected: the commit contains only the two listed files.

## Task 2: Open and validate the canonical desktop schema

**Files:**
- Create: `src-tauri/crates/cc-switch-core/src/error.rs`
- Create: `src-tauri/crates/cc-switch-core/src/schema.rs`
- Create: `src-tauri/crates/cc-switch-core/src/state.rs`
- Create: `src-tauri/crates/cc-switch-core/tests/schema_compat.rs`
- Modify: `src-tauri/crates/cc-switch-core/src/lib.rs`
- Modify: `src-tauri/crates/cc-switch-core/Cargo.toml`

- [ ] **Step 1: Write the failing desktop-schema compatibility tests**

Create `schema_compat.rs` with a canonical fixture using `app_type`, `is_current`, `provider_endpoints`, `usage_logs`, and `model_pricing`:

```rust
use cc_switch_core::{CoreError, HeadlessState, SchemaError, DESKTOP_SCHEMA_VERSION};
use rusqlite::Connection;

#[test]
fn opens_existing_v16_schema_without_mutating_it() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let db_path = home.path().join(".cc-switch").join("cc-switch.db");
    std::fs::create_dir_all(db_path.parent().expect("数据库目录")).expect("创建数据库目录");
    let connection = Connection::open(&db_path).expect("创建桌面数据库 fixture");
    seed_required_v16_schema(&connection);
    connection.pragma_update(None, "user_version", DESKTOP_SCHEMA_VERSION).expect("设置版本");
    drop(connection);

    let state = HeadlessState::open(home.path()).expect("打开规范数据库");
    assert_eq!(state.schema_version().expect("读取版本"), DESKTOP_SCHEMA_VERSION);
    drop(state);

    let reopened = Connection::open(db_path).expect("重新打开 fixture");
    assert!(!column_exists(&reopened, "providers", "app"));
    assert!(!table_exists(&reopened, "current_providers"));
}

#[test]
fn rejects_incompatible_existing_schema_before_writes() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    seed_legacy_agent_schema(home.path());
    let error = HeadlessState::open(home.path()).expect_err("旧 Agent schema 必须被拒绝");
    assert!(matches!(error, CoreError::Schema(SchemaError::Incompatible { .. })));
}
```

The helper SQL must create the exact required v16 columns from `src-tauri/src/database/schema.rs`, not the removed `providers.app/current_providers` layout.

- [ ] **Step 2: Run the test and verify RED**

Run:

```powershell
cd src-tauri
cargo test -j 1 -p cc-switch-core --test schema_compat -- --nocapture
```

Expected: compilation fails because `SchemaError`, `DESKTOP_SCHEMA_VERSION`, and compatibility methods do not exist.

- [ ] **Step 3: Implement explicit state and schema validation**

Add these public contracts:

```rust
pub const DESKTOP_SCHEMA_VERSION: i32 = 16;

pub struct HeadlessState {
    connection: std::sync::Mutex<rusqlite::Connection>,
    home: std::path::PathBuf,
}

impl HeadlessState {
    pub fn open(home: impl AsRef<std::path::Path>) -> Result<Self, CoreError>;
    pub fn memory(home: impl AsRef<std::path::Path>) -> Result<Self, CoreError>;
    pub fn with_connection<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Connection) -> Result<T, CoreError>,
    ) -> Result<T, CoreError>;
    pub fn schema_version(&self) -> Result<i32, CoreError>;
    pub fn home(&self) -> &std::path::Path;
}
```

`HeadlessState::open` must:

1. Create `~/.cc-switch` only when needed.
2. Open SQLite and set `foreign_keys=ON` plus a 5-second busy timeout.
3. If the file is new, create the canonical Provider and Usage tables required by this plan and set `user_version=16`.
4. If the file exists, run read-only table/column validation and reject incompatible layouts without DDL.

Use stable errors:

```rust
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("远端数据库结构不兼容: detected={detected}, supported={supported}, reason={reason}")]
    Incompatible { detected: i32, supported: i32, reason: String },
}
```

Every new or moved function must include Chinese maintenance comments explaining schema mutation boundaries and concurrency choices.

- [ ] **Step 4: Run schema and Core regression tests**

```powershell
cargo test -j 1 -p cc-switch-core --test schema_compat -- --nocapture
cargo test -j 1 -p cc-switch-core -- --nocapture
```

Expected: all Core tests pass and no test creates `providers.app` or `current_providers`.

- [ ] **Step 5: Commit canonical schema support**

```powershell
git add src-tauri/crates/cc-switch-core
git commit -m "refactor(remote): use canonical database schema"
```

## Task 3: Read complete Provider records from the canonical database

**Files:**
- Create: `src-tauri/crates/cc-switch-core/src/provider/mod.rs`
- Create: `src-tauri/crates/cc-switch-core/src/provider/model.rs`
- Create: `src-tauri/crates/cc-switch-core/src/provider/repository.rs`
- Create: `src-tauri/crates/cc-switch-core/tests/provider_parity.rs`
- Modify: `src-tauri/crates/cc-switch-core/src/lib.rs`
- Modify: `src-tauri/crates/cc-switch-core/tests/provider_core.rs`

- [ ] **Step 1: Write a failing full-record read test**

```rust
#[test]
fn lists_existing_desktop_providers_with_endpoints_and_current_state() {
    let fixture = DesktopFixture::v16();
    fixture.insert_provider("codex", "remote-codex", true);
    fixture.insert_endpoint("codex", "remote-codex", "https://backup.example/v1", 1234);

    let state = HeadlessState::open(fixture.home()).expect("打开 fixture");
    let providers = ProviderService::list(&state, "codex").expect("读取远端 Provider");
    let provider = &providers["remote-codex"];

    assert_eq!(provider.icon.as_deref(), Some("openai"));
    assert_eq!(provider.icon_color.as_deref(), Some("#111111"));
    assert!(provider.in_failover_queue);
    assert!(provider.meta.as_ref().expect("meta")["customEndpoints"]
        .get("https://backup.example/v1")
        .is_some());
    assert_eq!(ProviderService::current(&state, "codex").expect("当前项"), "remote-codex");
}
```

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -j 1 -p cc-switch-core --test provider_parity -- --nocapture
```

Expected: compilation fails because the current `ProviderRecord` lacks icon, endpoint, and failover fields or SQL still references `app`.

- [ ] **Step 3: Implement the complete shared Provider model**

Define the shared DTO with frontend-compatible camelCase serialization:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub settings_config: serde_json::Value,
    pub website_url: Option<String>,
    pub category: Option<String>,
    pub created_at: Option<i64>,
    pub sort_index: Option<usize>,
    pub notes: Option<String>,
    pub icon: Option<String>,
    pub icon_color: Option<String>,
    /// Meta 保持开放 JSON，确保远端 Agent 不丢弃比自身更新的桌面字段；
    /// 需要参与业务判断的字段由各服务通过窄类型解析。
    pub meta: Option<serde_json::Value>,
    #[serde(default)]
    pub in_failover_queue: bool,
}
```

Move canonical read SQL from `src-tauri/src/database/dao/providers.rs` into `provider/repository.rs`. Query `app_type`, merge `provider_endpoints` into the meta JSON `customEndpoints` object, and resolve current Provider through `is_current=1`. Keep unknown meta keys byte-for-byte semantically equivalent on read/write so a temporary Agent cannot erase newer desktop metadata. Accept exactly the eight `AppId` strings defined in `src/lib/api/types.ts`.

- [ ] **Step 4: Run Provider read regressions**

```powershell
cargo test -j 1 -p cc-switch-core --test provider_parity -- --nocapture
cargo test -j 1 -p cc-switch-core --test provider_core -- --nocapture
```

Expected: full-record fixture and existing Provider tests pass.

- [ ] **Step 5: Commit Provider read parity**

```powershell
git add src-tauri/crates/cc-switch-core/src/provider src-tauri/crates/cc-switch-core/src/lib.rs src-tauri/crates/cc-switch-core/tests
git commit -m "feat(remote): read canonical providers"
```

## Task 4: Implement canonical Provider transactions

**Files:**
- Modify: `src-tauri/crates/cc-switch-core/src/provider/repository.rs`
- Modify: `src-tauri/crates/cc-switch-core/src/provider/mod.rs`
- Modify: `src-tauri/crates/cc-switch-core/tests/provider_parity.rs`

- [ ] **Step 1: Write failing CRUD, sorting, and switch transaction tests**

Add tests that assert:

```rust
#[test]
fn provider_writes_use_app_type_and_is_current_atomically() {
    let fixture = DesktopFixture::v16();
    let state = HeadlessState::open(fixture.home()).expect("打开 fixture");
    ProviderService::add(&state, "claude", provider("a"), false).expect("新增 A");
    ProviderService::add(&state, "claude", provider("b"), false).expect("新增 B");
    ProviderService::switch_database_only(&state, "claude", "b").expect("切换 B");

    assert_eq!(ProviderService::current(&state, "claude").expect("当前项"), "b");
    assert_eq!(fixture.scalar_i64("SELECT COUNT(*) FROM providers WHERE app_type='claude' AND is_current=1"), 1);
    assert!(!fixture.column_exists("providers", "app"));
}
```

Also cover duplicate IDs, deleting the current Provider, ID changes, unknown app IDs, and a busy database returning `DATABASE_BUSY` without partial writes.

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -j 1 -p cc-switch-core --test provider_parity provider_writes -- --nocapture
```

Expected: old SQL fails on `app`/`current_providers` or the new transaction API is missing.

- [ ] **Step 3: Implement canonical repository writes**

Use `app_type` in every statement. Switch in one transaction:

```rust
fn set_current(tx: &rusqlite::Transaction<'_>, app: &str, id: &str) -> Result<(), CoreError> {
    let changed = tx.execute(
        "UPDATE providers SET is_current = CASE WHEN id = ?2 THEN 1 ELSE 0 END WHERE app_type = ?1",
        rusqlite::params![app, id],
    )?;
    if changed == 0 {
        return Err(CoreError::ProviderNotFound(id.to_string()));
    }
    Ok(())
}
```

CRUD must preserve endpoint rows, desktop meta serialization, existing `created_at`, and sort order. Map SQLite busy/locked errors to `DATABASE_BUSY`.

- [ ] **Step 4: Run all Core Provider tests**

```powershell
cargo test -j 1 -p cc-switch-core --test provider_parity -- --nocapture
cargo test -j 1 -p cc-switch-core --test provider_core -- --nocapture
```

Expected: all tests pass with exactly one current Provider per app.

- [ ] **Step 5: Commit Provider transactions**

```powershell
git add src-tauri/crates/cc-switch-core/src/provider src-tauri/crates/cc-switch-core/tests/provider_parity.rs
git commit -m "feat(remote): write canonical providers"
```

## Task 5: Share platform-aware Provider live projection

**Files:**
- Create: `src-tauri/crates/cc-switch-core/src/provider/live.rs`
- Create: `src-tauri/crates/cc-switch-core/tests/provider_live.rs`
- Modify: `src-tauri/crates/cc-switch-core/Cargo.toml`
- Modify: `src-tauri/crates/cc-switch-core/src/provider/mod.rs`
- Modify: `src-tauri/src/services/provider/live.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`

- [ ] **Step 1: Write failing live projection tests**

Create table-driven tests for `claude`, `codex`, `gemini`, `grokbuild`, `opencode`, `openclaw`, and `hermes`. Each test seeds an unrelated key in the live file, switches Provider, and asserts the unrelated key survives. Add this Linux condition test:

```rust
#[test]
fn claude_desktop_switch_is_rejected_before_database_change_on_linux() {
    let state = fixture_with_current("claude-desktop", "desktop-a");
    let error = ProviderService::switch(&state, "claude-desktop", "desktop-b")
        .expect_err("Linux 不能写 Claude Desktop live 配置");
    assert_eq!(error.code(), "CAPABILITY_UNAVAILABLE");
    assert_eq!(ProviderService::current(&state, "claude-desktop").expect("当前项"), "desktop-a");
}
```

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -j 1 -p cc-switch-core --test provider_live -- --nocapture
```

Expected: non-Claude writers and platform capability checks are missing.

- [ ] **Step 3: Extract headless live writers**

Expose a path-explicit API:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPlatform {
    Linux,
    Windows,
    Macos,
}

pub struct LiveContext<'a> {
    pub home: &'a std::path::Path,
    pub platform: TargetPlatform,
}

pub fn project_provider(
    context: &LiveContext<'_>,
    app: &str,
    provider: &ProviderRecord,
) -> Result<SwitchResult, CoreError>;
```

Move the application-specific parsing and write logic used by `write_live_snapshot`, `write_gemini_live`, Codex TOML/auth projection, Grok Build, OpenCode, OpenClaw, and Hermes into Core without Tauri imports. Add only headless dependencies (`toml`, `toml_edit`, `serde_yaml`, `json5`) to `cc-switch-core`.

Every writer must use same-directory temporary files and atomic replacement. `ProviderService::switch` must check platform capability first, commit `is_current`, then project live; a projection failure returns `LIVE_WRITE_FAILED` and leaves the database state visible for reconciliation rather than reporting success.

- [ ] **Step 4: Make desktop Provider services call the shared writer**

Keep proxy-takeover and Tauri event orchestration in the desktop crate, but replace duplicate filesystem projection with `cc_switch_core::provider::live::project_provider`. Map `CoreError` to `AppError` without exposing secrets.

- [ ] **Step 5: Run Core and desktop Provider suites**

```powershell
cargo test -j 1 -p cc-switch-core --test provider_live -- --nocapture
cargo test -j 1 --test provider_service --test provider_commands -- --nocapture
```

Expected: Core live tests and existing desktop Provider behavior pass.

- [ ] **Step 6: Audit Agent dependencies and commit**

```powershell
cargo tree -p cc-switch-agent | Select-String -Pattern 'tauri|webkit2gtk|gtk' -CaseSensitive:$false
```

Expected: no matches.

```powershell
git add src-tauri/crates/cc-switch-core src-tauri/src/services/provider
git commit -m "refactor(remote): share provider live projection"
```

## Task 6: Expand protocol capabilities and generic dispatch

**Files:**
- Modify: `src-tauri/crates/cc-switch-protocol/src/capabilities.rs`
- Create: `src-tauri/crates/cc-switch-core/src/dispatch.rs`
- Modify: `src-tauri/crates/cc-switch-core/src/lib.rs`
- Modify: `src-tauri/crates/cc-switch-agent/src/lib.rs`
- Modify: `src-tauri/crates/cc-switch-agent/tests/agent_process.rs`
- Modify: `src-tauri/tests/remote_capabilities.rs`

- [ ] **Step 1: Write failing registry tests for the complete command set**

Assert the registry contains these stable names:

```rust
const USAGE_READS: &[&str] = &[
    "usage.summary", "usage.summary_by_app", "usage.trends",
    "usage.provider_stats", "usage.model_stats", "usage.logs",
    "usage.detail", "usage.data_sources", "usage.pricing.list",
    "usage.limits", "usage.provider_query",
];
const USAGE_WRITES: &[&str] = &[
    "usage.pricing.update", "usage.pricing.delete", "usage.provider_test",
    "usage.session_sync", "usage.codex_rebuild",
];
```

Reads must be read-only/idempotent with 30-second timeout. `usage.session_sync` and `usage.codex_rebuild` must be non-idempotent with 300-second timeout. Unknown commands remain denied.

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -j 1 --test remote_capabilities -- --nocapture
```

Expected: Usage commands are absent from `provider_phase()`.

- [ ] **Step 3: Implement the combined registry**

Replace `provider_phase()` with:

```rust
impl CommandCapabilityRegistry {
    pub fn remote_supported() -> Self {
        Self::from_capabilities(
            provider_capabilities().into_iter().chain(usage_capabilities()),
        )
    }
}
```

Keep command metadata in protocol crate so desktop runtime and Agent handshake use the same source.

- [ ] **Step 4: Introduce generic dispatch**

Add:

```rust
pub fn dispatch_command(
    state: &HeadlessState,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, CommandError> {
    CommandCapabilityRegistry::remote_supported().require(command)?;
    if command.starts_with("provider.") {
        return provider::dispatch(state, command, args);
    }
    usage::dispatch(state, command, args)
}
```

Rename `ProviderCommandError` to domain-neutral `CommandError`, preserving stable code mapping. Change Agent handshake and request loop to use `remote_supported()` and `dispatch_command`.

- [ ] **Step 5: Run protocol and Agent tests**

```powershell
cargo test -j 1 -p cc-switch-protocol -- --nocapture
cargo test -j 1 -p cc-switch-agent -- --nocapture
cargo test -j 1 --test remote_capabilities --test remote_protocol --test remote_client -- --nocapture
```

Expected: Provider vertical slice still passes and hello capabilities include all declared names.

- [ ] **Step 6: Commit protocol expansion**

```powershell
git add src-tauri/crates/cc-switch-protocol src-tauri/crates/cc-switch-core/src/dispatch.rs src-tauri/crates/cc-switch-core/src/lib.rs src-tauri/crates/cc-switch-agent src-tauri/tests/remote_capabilities.rs
git commit -m "feat(remote): register provider and usage commands"
```

## Task 7: Move read-only Usage queries into Core

**Files:**
- Create: `src-tauri/crates/cc-switch-core/src/usage/mod.rs`
- Create: `src-tauri/crates/cc-switch-core/src/usage/model.rs`
- Create: `src-tauri/crates/cc-switch-core/src/usage/query.rs`
- Create: `src-tauri/crates/cc-switch-core/tests/usage_query.rs`
- Modify: `src-tauri/crates/cc-switch-core/Cargo.toml`
- Modify: `src-tauri/crates/cc-switch-core/src/lib.rs`
- Modify: `src-tauri/src/services/usage_stats.rs`
- Modify: `src-tauri/src/services/sql_helpers.rs`

- [ ] **Step 1: Write failing parity tests against one canonical fixture**

Seed proxy and session rows with cache tokens, failures, pricing model, data source, and missing Provider IDs. Assert exact serialized results for every read command:

```rust
#[test]
fn usage_queries_match_dashboard_semantics() {
    let fixture = UsageFixture::v16();
    fixture.seed_proxy_and_session_rows();
    let state = HeadlessState::open(fixture.home()).expect("打开 Usage fixture");

    let summary = UsageService::summary(&state, UsageScope::all()).expect("汇总");
    assert_eq!(summary.total_requests, 3);
    assert_eq!(summary.real_total_tokens, 1_240);
    assert_eq!(summary.total_cost, "0.042000");

    let logs = UsageService::logs(&state, LogFilters::default(), 0, 20).expect("日志");
    assert_eq!(logs.total, 3);
    assert_eq!(logs.data[0].provider_name.as_deref(), Some("Codex (Session)"));
}
```

Also compare summary-by-app, trends, Provider stats, model stats, detail, data sources, pricing list, limits, and Provider usage query.

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -j 1 -p cc-switch-core --test usage_query -- --nocapture
```

Expected: `UsageService` and shared DTOs are missing.

- [ ] **Step 3: Move DTOs and pure SQL helpers**

Move the serializable structs and query SQL from `src-tauri/src/services/usage_stats.rs` into Core. Move `fresh_input_sql`, token semantics constants, Provider-name fallback, data-source normalization, and `row_to_request_log_detail` with Chinese comments documenting column order and legacy-row handling.

Expose:

```rust
impl UsageService {
    pub fn summary(state: &HeadlessState, scope: UsageScope) -> Result<UsageSummary, CoreError>;
    pub fn summary_by_app(state: &HeadlessState, scope: UsageScope) -> Result<Vec<UsageSummaryByApp>, CoreError>;
    pub fn trends(state: &HeadlessState, scope: UsageScope) -> Result<Vec<DailyStats>, CoreError>;
    pub fn provider_stats(state: &HeadlessState, scope: UsageScope) -> Result<Vec<ProviderStats>, CoreError>;
    pub fn model_stats(state: &HeadlessState, scope: UsageScope) -> Result<Vec<ModelStats>, CoreError>;
    pub fn logs(state: &HeadlessState, filters: LogFilters, page: u32, page_size: u32) -> Result<PaginatedLogs, CoreError>;
    pub fn detail(state: &HeadlessState, request_id: &str) -> Result<Option<RequestLogDetail>, CoreError>;
    pub fn data_sources(state: &HeadlessState) -> Result<Vec<DataSourceSummary>, CoreError>;
}
```

Reject a serialized logs response above 16 MiB before protocol framing and return `PAYLOAD_TOO_LARGE`.

- [ ] **Step 4: Make desktop Usage queries delegate to Core**

Keep Tauri command names and parameter casing unchanged. Desktop wrappers lock the existing database connection, call the shared query functions, and map `CoreError` to `AppError`. Remove duplicate SQL only after parity tests pass.

- [ ] **Step 5: Run Core and desktop Usage regressions**

```powershell
cargo test -j 1 -p cc-switch-core --test usage_query -- --nocapture
cargo test -j 1 --lib services::usage_stats -- --nocapture
cargo test -j 1 --test provider_service -- --nocapture
```

Expected: shared fixture queries and desktop Usage tests pass.

- [ ] **Step 6: Commit shared Usage reads**

```powershell
git add src-tauri/crates/cc-switch-core src-tauri/src/services/usage_stats.rs src-tauri/src/services/sql_helpers.rs
git commit -m "feat(remote): share usage queries"
```

## Task 8: Add Usage pricing, limits, scripts, and long operations

**Files:**
- Create: `src-tauri/crates/cc-switch-core/src/usage/mutation.rs`
- Create: `src-tauri/crates/cc-switch-core/src/usage/script.rs`
- Create: `src-tauri/crates/cc-switch-core/tests/usage_mutation.rs`
- Modify: `src-tauri/crates/cc-switch-core/src/usage/mod.rs`
- Modify: `src-tauri/crates/cc-switch-core/Cargo.toml`
- Modify: `src-tauri/src/commands/usage.rs`
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src-tauri/src/services/session_usage.rs`
- Modify: `src-tauri/src/services/session_usage_codex.rs`

- [ ] **Step 1: Write failing mutation and recovery tests**

Cover non-negative pricing validation, history backfill, pricing deletion, Provider limits, Provider usage scripts, session sync against the explicit remote HOME, and Codex rebuild backup ordering:

```rust
#[test]
fn codex_rebuild_creates_backup_before_reset_and_preserves_it_on_import_failure() {
    let fixture = UsageFixture::v16();
    fixture.seed_codex_usage();
    fixture.make_codex_session_import_fail();
    let state = HeadlessState::open(fixture.home()).expect("打开 fixture");

    let error = UsageService::rebuild_codex(&state).expect_err("导入失败必须上报");
    assert_eq!(error.code(), "REMOTE_BUSINESS_ERROR");
    assert!(fixture.latest_database_backup().is_file());
}
```

Add a cancellation test that cancels between backup and reset and asserts the original Usage rows remain intact:

```rust
#[test]
fn cancelled_codex_rebuild_stops_before_reset() {
    let fixture = UsageFixture::v16();
    fixture.seed_codex_usage();
    let state = HeadlessState::open(fixture.home()).expect("打开 fixture");
    let cancellation = OperationCancellation::cancelled();
    let error = UsageService::rebuild_codex(&state, &cancellation)
        .expect_err("取消的重建不能继续");
    assert_eq!(error.code(), "REMOTE_OPERATION_CANCELLED");
    assert_eq!(fixture.codex_usage_count(), 1);
}
```

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -j 1 -p cc-switch-core --test usage_mutation -- --nocapture
```

Expected: Core mutation/long-operation APIs are absent.

- [ ] **Step 3: Implement headless Usage mutations**

Expose synchronous Core operations suitable for Agent worker threads. Network-backed Provider scripts use a blocking rustls client inside the worker and the same sandboxed script semantics as the desktop service; they must not create a nested Tauri/Tokio dependency:

```rust
impl UsageService {
    pub fn update_pricing(state: &HeadlessState, input: PricingUpdate) -> Result<(), CoreError>;
    pub fn delete_pricing(state: &HeadlessState, model_id: &str) -> Result<(), CoreError>;
    pub fn limits(state: &HeadlessState, provider_id: &str, app: &str) -> Result<ProviderLimitStatus, CoreError>;
    pub fn provider_query(state: &HeadlessState, input: ProviderUsageInput) -> Result<UsageResult, CoreError>;
    pub fn provider_test(state: &HeadlessState, input: ProviderUsageTestInput) -> Result<UsageResult, CoreError>;
    pub fn sync_sessions(state: &HeadlessState, cancellation: &OperationCancellation) -> Result<SessionSyncResult, CoreError>;
    pub fn rebuild_codex(state: &HeadlessState, cancellation: &OperationCancellation) -> Result<SessionSyncResult, CoreError>;
}
```

Define `OperationCancellation` as a clonable `Arc<AtomicBool>` with `active()`, `cancelled()`, `cancel()`, and `check()` methods. All filesystem reads derive from `state.home()`. Preserve the existing session synchronization mutex semantics inside one Agent process. Codex rebuild must call `check()` before backup, before reset, and before import; it executes backup, reset, and import in that order and never auto-retries.

- [ ] **Step 4: Make desktop commands delegate to Core**

Keep Tauri async boundaries for UI responsiveness, but call the same Core functions inside `spawn_blocking`. Preserve local Usage events after successful writes; Agent responses trigger frontend invalidation instead of Tauri events.

- [ ] **Step 5: Run mutation, command, and Agent tests**

```powershell
cargo test -j 1 -p cc-switch-core --test usage_mutation -- --nocapture
cargo test -j 1 --lib commands::usage -- --nocapture
cargo test -j 1 -p cc-switch-agent -- --nocapture
```

Expected: all mutation and recovery tests pass; Agent process remains headless.

- [ ] **Step 6: Commit Usage writes and long tasks**

```powershell
git add src-tauri/crates/cc-switch-core src-tauri/src/commands/usage.rs src-tauri/src/commands/provider.rs src-tauri/src/services/session_usage.rs src-tauri/src/services/session_usage_codex.rs
git commit -m "feat(remote): execute usage operations"
```

## Task 9: Multiplex responses, enforce timeouts, and cancel long operations

**Files:**
- Modify: `src-tauri/crates/cc-switch-protocol/src/protocol.rs`
- Modify: `src-tauri/crates/cc-switch-agent/src/lib.rs`
- Modify: `src-tauri/crates/cc-switch-agent/tests/agent_process.rs`
- Modify: `src-tauri/src/remote/client.rs`
- Modify: `src-tauri/src/remote/ssh.rs`
- Modify: `src-tauri/tests/remote_client.rs`

- [ ] **Step 1: Write failing out-of-order, timeout, and cancellation tests**

Add a fake duplex transport that returns responses in reverse request order and assert each caller receives its own result. Add a request that never responds and assert the client sends a Cancel frame after the capability timeout:

```rust
#[test]
fn timed_out_request_sends_cancel_for_the_same_operation() {
    let transport = FakeDuplex::new();
    let session = RemoteSession::connect(transport.reader(), transport.writer(), "3.18.0")
        .expect("建立会话");
    let error = session
        .invoke_with_id("request-1", "operation-1", "usage.session_sync", json!({}), 20)
        .expect_err("无响应请求必须超时");
    assert_eq!(error.code(), "REMOTE_OPERATION_TIMEOUT");
    assert_eq!(transport.last_cancel(), CancelRequest {
        request_id: "request-1".to_string(),
        operation_id: "operation-1".to_string(),
    });
}
```

In the Agent process test, start `usage.codex_rebuild`, send `CancelRequest`, and assert the response code is `REMOTE_OPERATION_CANCELLED` while the Agent remains available for a following ping.

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -j 1 --test remote_client -- --nocapture
cargo test -j 1 -p cc-switch-agent --test agent_process -- --nocapture
```

Expected: the current client blocks on one response, does not enforce `timeout_ms`, and the Agent ignores Cancel frames.

- [ ] **Step 3: Add a typed cancellation frame payload**

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelRequest {
    pub request_id: String,
    pub operation_id: String,
}
```

Keep `RpcRequest.operation_id` populated for every write and long read. Request IDs identify responses; operation IDs identify cancellation and must never be reused.

- [ ] **Step 4: Implement a multiplexed desktop client**

Split `RemoteSession` into:

- one writer protected by `Mutex<W>`;
- one background reader that routes `RpcResponse` by ID into a `HashMap<String, Sender<_>>`;
- a pending-request registry removed on success, timeout, disconnect, or cancellation;
- an event channel preserved for later Tauri event forwarding.

`invoke_with_id` must accept both IDs, call `recv_timeout`, send `FrameKind::Cancel` on timeout, remove the pending entry, and return `REMOTE_OPERATION_TIMEOUT`. EOF fails every pending request with `REMOTE_OFFLINE`.

- [ ] **Step 5: Run Agent requests in cancellable workers**

Change the Agent session loop to own `Arc<HeadlessState>`, an `Arc<Mutex<W>>`, and an operation registry of cancellation tokens. Each Request runs in a worker thread and writes its response under the writer lock. The main reader loop remains free to process Ping and Cancel frames; Cancel looks up the operation ID and flips its token. On stdin EOF, cancel and join all workers before process exit.

- [ ] **Step 6: Run protocol/client/Agent regressions**

```powershell
cargo test -j 1 -p cc-switch-protocol -- --nocapture
cargo test -j 1 --test remote_client -- --nocapture
cargo test -j 1 -p cc-switch-agent -- --nocapture
```

Expected: reverse-order responses, timeout cancellation, post-cancel ping, and EOF cleanup all pass.

- [ ] **Step 7: Commit multiplexing and cancellation**

```powershell
git add src-tauri/crates/cc-switch-protocol/src/protocol.rs src-tauri/crates/cc-switch-agent src-tauri/src/remote/client.rs src-tauri/src/remote/ssh.rs src-tauri/tests/remote_client.rs
git commit -m "feat(remote): cancel timed out operations"
```

## Task 10: Enforce runtime generation at the desktop gateway

**Files:**
- Modify: `src-tauri/src/commands/remote.rs`
- Modify: `src-tauri/src/remote/runtime.rs`
- Modify: `src-tauri/src/remote/client.rs`
- Modify: `src-tauri/src/remote/ssh.rs`
- Create: `src-tauri/tests/remote_generation.rs`
- Modify: `src-tauri/tests/remote_runtime.rs`

- [ ] **Step 1: Write failing stale-generation tests**

```rust
#[test]
fn rejects_request_from_previous_runtime_generation() {
    let runtime = connected_fake_runtime(7);
    let error = runtime
        .invoke_remote(6, "usage.summary", serde_json::json!({}))
        .expect_err("旧 generation 必须被拒绝");
    assert_eq!(error.code(), "STALE_RUNTIME");
    assert_eq!(fake_session_request_count(), 0);
}
```

Add a delayed-response test that switches generation after send and verifies the result is rejected after receive.

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -j 1 --test remote_generation -- --nocapture
```

Expected: `invoke_remote` has no generation parameter and accepts stale work.

- [ ] **Step 3: Implement double-sided generation checks**

Change the API to:

```rust
pub fn invoke_remote(
    &self,
    expected_generation: u64,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, RemoteRuntimeError>;
```

Check the snapshot before locking the session and again after the response arrives. Update `remote_invoke` to require camelCase `generation`. Map mismatches to `STALE_RUNTIME`; never resend writes.

- [ ] **Step 4: Use the combined capability timeout**

Replace every `provider_phase()` runtime lookup with `remote_supported()`. Ensure Usage long tasks receive 300-second timeout and read commands 30 seconds.

- [ ] **Step 5: Run remote gateway regressions**

```powershell
cargo test -j 1 --test remote_generation --test remote_runtime --test remote_client -- --nocapture
```

Expected: stale requests never reach the session and existing offline behavior remains stable.

- [ ] **Step 6: Commit generation enforcement**

```powershell
git add src-tauri/src/commands/remote.rs src-tauri/src/remote src-tauri/tests/remote_generation.rs src-tauri/tests/remote_runtime.rs
git commit -m "fix(remote): reject stale runtime requests"
```

## Task 11: Route every Usage API through the selected runtime

**Files:**
- Modify: `src/lib/runtime/invoke.ts`
- Modify: `src/lib/api/usage.ts`
- Create: `src/lib/api/usage.test.ts`
- Modify: `tests/msw/tauriMocks.ts`

- [ ] **Step 1: Write failing local/remote routing tests**

Mock Tauri invoke and assert both paths:

```ts
it("routes usage summary to the online remote target", async () => {
  setRuntimeSnapshot({ status: "online", generation: 9, activeTargetId: "server-a" });
  await usageApi.getUsageSummary(100, 200, "codex");
  expect(invoke).toHaveBeenCalledWith("remote_invoke", {
    command: "usage.summary",
    args: { startDate: 100, endDate: 200, appType: "codex", providerName: undefined, model: undefined },
    generation: 9,
  });
});

it("keeps usage summary local in local mode", async () => {
  setRuntimeSnapshot({ status: "local", generation: 10 });
  await usageApi.getUsageSummary();
  expect(invoke).toHaveBeenCalledWith("get_usage_summary", expect.any(Object));
});
```

Add one assertion for every Usage API method and verify offline mode rejects without invoking a local business command.

- [ ] **Step 2: Run and verify RED**

```powershell
pnpm test:unit -- src/lib/api/usage.test.ts
```

Expected: Usage methods call direct `invoke` and remote routing assertions fail.

- [ ] **Step 3: Extend appInvoke with generation**

```ts
return await localInvoke<T>("remote_invoke", {
  command: options.remoteCommand,
  args: args ?? {},
  generation: runtime.generation,
});
```

Keep local-only commands explicit. Do not add a fallback from remote errors to local invoke.

- [ ] **Step 4: Map the complete Usage API**

Replace direct imports of Tauri `invoke` with `appInvoke` and map every method to the command names declared in Task 6. Preserve local Tauri command names and existing argument casing.

- [ ] **Step 5: Run unit tests and typecheck**

```powershell
pnpm test:unit -- src/lib/api/usage.test.ts
pnpm typecheck
```

Expected: routing tests and TypeScript compilation pass.

- [ ] **Step 6: Commit Usage routing**

```powershell
git add src/lib/runtime/invoke.ts src/lib/api/usage.ts src/lib/api/usage.test.ts tests/msw/tauriMocks.ts
git commit -m "feat(remote): route usage API by target"
```

## Task 12: Scope Provider and Usage queries by runtime target

**Files:**
- Create: `src/lib/runtime/queryScope.ts`
- Create: `src/lib/runtime/queryScope.test.ts`
- Modify: `src/lib/query/usage.ts`
- Modify: `src/lib/query/queries.ts`
- Modify: `src/contexts/RuntimeTargetContext.tsx`
- Modify: `src/hooks/useUsageEventBridge.ts`
- Modify: `src/hooks/useUsageCacheBridge.ts`

- [ ] **Step 1: Write failing query-scope tests**

```ts
it("creates different scopes for local and remote generations", () => {
  expect(runtimeQueryScope({ status: "local", generation: 2 })).toEqual(["local", 2]);
  expect(runtimeQueryScope({ status: "online", generation: 3, activeTargetId: "server-a" }))
    .toEqual(["remote", "server-a", 3]);
});
```

Render one Usage hook, switch snapshots, and assert TanStack Query performs a second request under a different key rather than reusing local data.

- [ ] **Step 2: Run and verify RED**

```powershell
pnpm test:unit -- src/lib/runtime/queryScope.test.ts
```

Expected: `runtimeQueryScope` is missing and current Usage keys are target-agnostic.

- [ ] **Step 3: Implement a synchronous runtime scope helper**

```ts
export type RuntimeQueryScope = readonly ["local", number] |
  readonly ["remote", string, number] |
  readonly ["transition", string | null, number];

export function runtimeQueryScope(snapshot = getRuntimeSnapshot()): RuntimeQueryScope {
  if (snapshot.status === "local") return ["local", snapshot.generation] as const;
  if (snapshot.status === "online" && snapshot.activeTargetId) {
    return ["remote", snapshot.activeTargetId, snapshot.generation] as const;
  }
  return ["transition", snapshot.activeTargetId ?? null, snapshot.generation] as const;
}
```

Include the scope immediately after each domain root in Provider and Usage query keys.

- [ ] **Step 4: Cancel old queries during target transitions**

Before publishing connecting state, call `queryClient.cancelQueries()`; after backend confirmation, clear environment-scoped queries and publish the final snapshot. Event bridges must invalidate only the current runtime scope.

- [ ] **Step 5: Run frontend regressions**

```powershell
pnpm test:unit -- src/lib/runtime/queryScope.test.ts src/lib/api/usage.test.ts
pnpm test:unit
pnpm typecheck
pnpm format:check
```

Expected: all unit tests, typecheck, and formatting pass.

- [ ] **Step 6: Commit cache isolation**

```powershell
git add src/lib/runtime src/lib/query src/contexts/RuntimeTargetContext.tsx src/hooks
git commit -m "fix(remote): isolate target query caches"
```

## Task 13: Verify desktop adapters and stable errors end to end

**Files:**
- Modify: `src-tauri/src/remote/runtime.rs`
- Modify: `src-tauri/src/remote/ssh.rs`
- Modify: `src-tauri/src/commands/remote.rs`
- Modify: `src-tauri/tests/remote_agent_minimal.rs`
- Create: `src-tauri/tests/remote_provider_usage.rs`

- [ ] **Step 1: Write a process-level Provider/Usage Agent test**

Start `cc-switch-agent --stdio` with a canonical fixture HOME, perform hello, list an existing Provider, switch it, query Usage summary/logs, update pricing, and assert unknown commands are denied. Close stdin and assert process cleanup.

The test must assert stable codes for:

```rust
for code in [
    "DATABASE_INCOMPATIBLE", "DATABASE_BUSY", "COMMAND_NOT_EXPOSED",
    "CAPABILITY_UNAVAILABLE", "STALE_RUNTIME", "LIVE_WRITE_FAILED",
    "REMOTE_PERMISSION_DENIED", "REMOTE_OPERATION_TIMEOUT",
    "REMOTE_OPERATION_CANCELLED",
] {
    assert!(documented_error_codes().contains(&code));
}
```

- [ ] **Step 2: Run and verify RED**

```powershell
cd src-tauri
cargo test -j 1 --test remote_provider_usage -- --nocapture
```

Expected: one or more command mappings/error codes are absent until all adapters are complete.

- [ ] **Step 3: Complete error mapping and sanitization**

Map Core errors through Agent `RpcError`, SSH client, `RemoteRuntimeError`, and serialized Tauri payload without flattening stable codes. Preserve the 4096-character/control-character sanitization and never include settings JSON or tokens.

- [ ] **Step 4: Run full scoped Rust verification**

```powershell
cargo fmt --check
cargo test -j 1 -p cc-switch-protocol -- --nocapture
cargo test -j 1 -p cc-switch-core -- --nocapture
cargo test -j 1 -p cc-switch-agent -- --nocapture
cargo test -j 1 --test remote_provider_usage --test remote_capabilities --test remote_generation --test remote_runtime --test remote_client --test provider_commands --test provider_service -- --nocapture
cargo clippy -j 1 --workspace --all-targets -- -D warnings
```

Expected: all commands exit 0 with no warnings.

- [ ] **Step 5: Commit end-to-end adapters**

```powershell
git add src-tauri/src/remote src-tauri/src/commands/remote.rs src-tauri/tests
git commit -m "test(remote): cover provider usage parity"
```

## Task 14: Build embedded Agents and validate the real SSH target

**Files:**
- Modify only if the live test exposes a verified defect in files already owned by Tasks 2-13.
- Verify: `.github/workflows/ci.yml`
- Verify: `.github/workflows/release.yml`

- [ ] **Step 1: Stop the Tauri development process tree**

Identify the `cc-switch.exe` parent chain and stop only the repository's Tauri/Vite tree so Windows releases Rust artifacts. Record the prior dev environment paths for restart.

- [ ] **Step 2: Build or obtain fresh x86_64 and aarch64 musl Agents**

Use the existing CI workflow when local cross tooling is unavailable. Verify both ELF files are static using the workflow's ELF-header checks, then set:

```powershell
$env:CC_SWITCH_AGENT_X86_64_PATH=(Resolve-Path 'src-tauri/target/downloaded-agent-artifacts/linux-x86_64/cc-switch-agent').Path
$env:CC_SWITCH_AGENT_AARCH64_PATH=(Resolve-Path 'src-tauri/target/downloaded-agent-artifacts/linux-aarch64/cc-switch-agent').Path
```

- [ ] **Step 3: Run a credential-safe live integration probe**

The opt-in test must connect to `root@172.16.0.108`, then assert:

1. `provider.list` returns at least one already configured remote Provider without logging settings.
2. `provider.current` matches the remote database.
3. Switching to a designated test Provider changes only the remote `is_current` and remote live file, then restores the original Provider in cleanup.
4. `usage.summary` and `usage.logs` return remote fixture/real data.
5. Switching runtime back to local restores local query scope.
6. `/tmp/cc-switch-agent-*` is empty after disconnect.

The probe must never print API keys, settings JSON, request bodies, or full database rows.

- [ ] **Step 4: Restart development mode with embedded Agent paths**

```powershell
$env:CC_SWITCH_AGENT_X86_64_PATH=(Resolve-Path 'src-tauri/target/downloaded-agent-artifacts/linux-x86_64/cc-switch-agent').Path
$env:CC_SWITCH_AGENT_AARCH64_PATH=(Resolve-Path 'src-tauri/target/downloaded-agent-artifacts/linux-aarch64/cc-switch-agent').Path
Start-Process -FilePath 'D:/app/node/pnpm.cmd' -ArgumentList @('run','dev') -WorkingDirectory (Get-Location) -WindowStyle Hidden
```

Expected: Vite returns HTTP 200 at `http://localhost:3000/` and `src-tauri/target/debug/cc-switch.exe` is running.

- [ ] **Step 5: Perform final repository verification**

```powershell
git diff --check
git status --short --branch
git log --oneline -15
```

Expected: no temporary live-probe source, root-level diagnostic `target/`, secrets, or untracked Agent binaries. Only intentional commits and ignored build output remain.

- [ ] **Step 6: Commit any verified live-only correction separately**

If Step 3 required a source correction, stage only its owned files and use:

```powershell
git commit -m "fix(remote): pass live provider usage probe"
```

If no correction was required, do not create an empty commit.

---

## Completion Checklist

- [ ] Existing remote desktop schema opens without mutation.
- [ ] Remote Provider lists include full fields, endpoints, and current state for all eight app IDs.
- [ ] Linux-supported Provider switches update only the selected host and project the correct live format.
- [ ] Linux Claude Desktop switching fails before database mutation with `CAPABILITY_UNAVAILABLE`.
- [ ] Every Usage dashboard read/write method routes through the selected runtime.
- [ ] Long Usage operations use 300-second non-retrying capabilities, honor Cancel frames, and preserve backups on failure.
- [ ] Runtime generation and query keys reject cross-target stale results.
- [ ] Agent dependency tree contains no desktop GUI stack.
- [ ] Scoped Rust, frontend, Clippy, formatting, and real SSH verification pass.
- [ ] Tauri/Vite development state is restored with embedded Agent artifacts.
