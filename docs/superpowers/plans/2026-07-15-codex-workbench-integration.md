# Codex Workbench Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 CodexElves 的 Codex App 增强、脚本、插件、降智雷达、系统提示词替换、GPT 推理续接与推理 Token 展示原生整合到 CC Switch，并先消除 Provider API Key/Base URL 被 Live 配置静默覆盖的风险。

**Architecture:** CC Switch 保持唯一代理、SQLite、Provider 和设置系统；新增边界清晰的 `provider_security`、`codex_runtime`、`codex_injection`、`codex_workbench`、`codex_scripts`、`codex_plugins`、`codex_radar`、`codex_reasoning` 模块。三个阶段按依赖顺序连续执行，每个阶段用自动测试门验证，阶段间不重新做产品决策。

**Tech Stack:** Rust 1.85、Tauri 2.8、Tokio、Axum、rusqlite、reqwest、tokio-tungstenite、React 18、TypeScript、TanStack Query、Vitest、Testing Library、SQLite、CDP。

## Global Constraints

- 当前计划只描述未来实施；执行本计划前不得提前修改实现代码、安装依赖、迁移数据库或编译。
- 目标基线固定为 CC Switch `f6e37ed` 与 CodexElves `bf1224e`；执行开始时先核对并处理上游漂移。
- 第一版增强启动只支持 Windows 10/11；其他平台返回 `unsupported`，不得假装启动成功。
- 不修改或替换 Codex 官方安装文件、官方快捷方式和官方资源。
- 已普通运行的 Codex 不强杀，只返回 `ordinary_running` 并提示用户关闭后重试。
- 第三方 Provider API Key/Base URL 以 SQLite 为唯一真源；Live 不能自动回填覆盖。
- 官方 OAuth/ChatGPT 登录材料不归入第三方 Provider Credential。
- 远程脚本和插件只允许用户手动安装/更新，使用 staging、体积限制、校验和原子替换；失败保留旧版本。
- reasoning 正文、encrypted reasoning、系统提示词正文和 Provider 明文凭据不得写入日志。
- `reasoning_tokens` 是 `output_tokens` 子集，不重复计费。
- 推理续接只覆盖 Codex + GPT + 原生 Responses，默认最多 3 个额外轮次，且固定首个成功 Provider。
- 一个客户端请求只写一条主请求日志；所有成功轮次的 Token、费用和耗时累计到该行。
- WebDAV/S3 不同步用户脚本、请求日志、推理轮次、凭据审计、回滚快照和配置不一致状态。
- 未来获得一次实施授权后，依次完成阶段 1、2、3；自动门通过后直接进入下一阶段。

---

## Planned File Structure

### Provider 安全与数据库

- Create: `src-tauri/src/services/provider_security/mod.rs` — 对外 facade、DTO 与枚举。
- Create: `src-tauri/src/services/provider_security/credentials.rs` — 按 AppType 提取、规范化、脱敏和指纹。
- Create: `src-tauri/src/services/provider_security/mutation.rs` — 应用级锁、CAS、快照、补偿事务。
- Create: `src-tauri/src/services/provider_security/audit.rs` — 审计、回滚和保留策略。
- Create: `src-tauri/src/services/provider_security/recovery.rs` — Configuration Inconsistency 与恢复验证。
- Create: `src-tauri/src/commands/provider_security.rs` — 状态、显式导入、回滚和恢复命令。
- Create: `src/types/providerSecurity.ts`、`src/lib/api/providerSecurity.ts` — 前端契约。
- Create: `src/components/providers/ProviderCredentialConflict.tsx` — 卡片/编辑页冲突展示。
- Create: `src/components/settings/ConfigurationSecuritySection.tsx` — 高级设置中的审计/恢复 UI。
- Modify: `src-tauri/src/database/mod.rs`、`schema.rs`、`backup.rs`、`dao/providers.rs`。
- Modify: `src-tauri/src/services/provider/mod.rs`、`proxy.rs`、`provider/live.rs`、`sync_protocol.rs`、`webdav_sync.rs`、`s3_sync.rs`。
- Modify: `src-tauri/src/commands/provider.rs`、`import_export.rs`、`webdav_sync.rs`、`s3_sync.rs`、`mod.rs`、`lib.rs`、`store.rs`。
- Modify: `src/components/providers/EditProviderDialog.tsx`、`ProviderList.tsx`、`src/components/settings/SettingsPage.tsx`、相关 hooks/API/tests。

### Codex 工作台

- Create: `src-tauri/src/services/codex_workbench.rs` — 工作台聚合状态与命令服务。
- Create: `src-tauri/src/services/codex_runtime/{mod.rs,discovery.rs,launcher.rs,state.rs,cdp.rs}`。
- Create: `src-tauri/src/services/codex_injection/{mod.rs,bridge.rs,bundle.rs}`。
- Create: `src-tauri/src/services/codex_scripts.rs`、`codex_plugins.rs`、`codex_radar.rs`。
- Create: `src-tauri/src/commands/codex_workbench.rs`。
- Create: `src-tauri/resources/codex-workbench/renderer-inject.js`、`renderer-features.js`、`openai-curated-remote.zip`。
- Create: `src/types/codexWorkbench.ts`、`src/lib/api/codexWorkbench.ts`、`src/lib/query/codexWorkbench.ts`。
- Create: `src/components/codex-workbench/{CodexWorkbenchPage.tsx,OverviewTab.tsx,EnhancementsTab.tsx,ScriptsTab.tsx,PluginsTab.tsx,RadarTab.tsx}`。
- Create: `tests/components/CodexWorkbenchPage.test.tsx`。
- Modify: `src-tauri/Cargo.toml`、`src-tauri/src/services/mod.rs`、`commands/mod.rs`、`lib.rs`、`settings.rs`、`src/App.tsx`、`src/types.ts`、`src/lib/schemas/settings.ts`、`src/hooks/useSettingsForm.ts`、三种 i18n locale。

### 提示词、续接与请求日志

- Create: `src-tauri/src/services/codex_reasoning/{mod.rs,prompt.rs,continuation.rs,stream.rs,usage.rs}`。
- Create: `src-tauri/src/services/codex_reasoning/tests.rs`。
- Create: `src/components/providers/forms/CodexReasoningSettings.tsx`。
- Create: `tests/components/CodexReasoningSettings.test.tsx`。
- Modify: `src-tauri/src/provider.rs`、`proxy/forwarder.rs`、`proxy/response_processor.rs`、`proxy/usage/parser.rs`、`proxy/usage/logger.rs`、`services/usage_stats.rs`、`services/session_usage_codex.rs`。
- Modify: `src/types.ts`、`src/types/usage.ts`、`src/components/usage/RequestLogTable.tsx`、`RequestDetailPanel.tsx`、`tests/components/RequestLogTable.test.tsx`。
- Create: `THIRD_PARTY_NOTICES.md` — CodexElves MIT 来源、commit 和移植文件说明。

## Spec Coverage Map

| Design requirement | Implemented by |
| --- | --- |
| Provider credentials are DB-authoritative; CAS, audit, rollback, inconsistency recovery | Tasks 1–3 |
| Cloud restore preserves local credentials; exact restore confirms impact | Task 4 |
| Nullable reasoning log contract and `Tok N` display | Tasks 1, 5, 14–16 |
| Codex workbench navigation, settings and overview | Task 6 |
| Windows enhanced launch, CDP and loopback bridge | Task 7 |
| Approved page enhancement default matrix | Task 8 |
| Local scripts and manual remote market | Task 9 |
| Effective CODEX_HOME and plugin marketplace/cache | Task 10 |
| Native degradation radar with stale cache | Task 11 |
| Provider-level prompt replacement and identity correction | Task 12 |
| 518-grid continuation and encrypted-reasoning safety gates | Task 13 |
| Pinned Provider, final-success SSE, aggregate usage and one main log row | Task 14 |
| Unique Codex JSONL session enrichment | Task 15 |
| Final UI, secret scans, attribution and end-to-end acceptance | Task 16 |

---

# Phase 1 — Provider 安全基线与请求日志契约

## Task 1: 建立数据库 v14 与 Provider 安全数据模型

**Files:**

- Modify: `src-tauri/src/database/mod.rs:55`
- Modify: `src-tauri/src/database/schema.rs:15-235,398-500,1338-1367`
- Create: `src-tauri/src/services/provider_security/mod.rs`
- Create: `src-tauri/src/services/provider_security/credentials.rs`
- Create: `src-tauri/src/services/provider_security/audit.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Test: inline tests in the files above and `src-tauri/src/database/tests.rs`

**Interfaces:**

- Produces:

```rust
pub const PROVIDER_REVISION_INITIAL: i64 = 1;
pub const ROLLBACK_MAX_VERSIONS: usize = 10;
pub const ROLLBACK_MAX_AGE_DAYS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialFields {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    ProviderEdit,
    ExplicitLiveImport,
    CloudRestore,
    ExactRestore,
    Rollback,
    SystemProjection,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialDiff {
    pub field: String,
    pub stored_masked: Option<String>,
    pub live_masked: Option<String>,
    pub stored_fingerprint: Option<String>,
    pub live_fingerprint: Option<String>,
}
```

- Consumes: existing `AppType`, `Provider`, `Provider::resolve_usage_credentials`, `sha2` and SQLite connection helpers.

- [ ] **Step 1: Write failing v14 migration and credential normalization tests**

```rust
#[test]
fn v13_to_v14_adds_security_and_reasoning_schema() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    Database::create_tables_on_conn(&conn)?;
    Database::set_user_version(&conn, 13)?;
    Database::apply_schema_migrations_on_conn(&conn)?;
    assert_eq!(Database::get_user_version(&conn)?, 14);
    assert!(Database::has_column(&conn, "providers", "revision")?);
    assert!(Database::has_column(&conn, "proxy_request_logs", "reasoning_tokens")?);
    for table in [
        "codex_reasoning_rounds",
        "provider_credential_audit",
        "provider_rollback_snapshots",
        "app_configuration_state",
    ] {
        assert!(Database::table_exists(&conn, table)?);
    }
    Ok(())
}

#[test]
fn normalized_base_url_equates_default_port_and_trailing_slash() {
    assert_eq!(
        normalize_base_url(" HTTPS://Example.COM:443/v1/ ").unwrap(),
        "https://example.com/v1"
    );
}
```

- [ ] **Step 2: Run tests and verify the expected failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml v13_to_v14_adds_security_and_reasoning_schema -- --nocapture`
Expected: FAIL because schema v14 does not exist.

Run: `cargo test --manifest-path src-tauri/Cargo.toml normalized_base_url_equates_default_port_and_trailing_slash -- --nocapture`
Expected: FAIL because `normalize_base_url` does not exist.

- [ ] **Step 3: Add the v14 schema**

Set `SCHEMA_VERSION` to 14 and add `migrate_v13_to_v14`. New tables/columns must match:

```sql
ALTER TABLE providers ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE proxy_request_logs ADD COLUMN reasoning_tokens INTEGER;
ALTER TABLE proxy_request_logs ADD COLUMN reasoning_source TEXT;
ALTER TABLE proxy_request_logs ADD COLUMN continuation_status TEXT NOT NULL DEFAULT 'not_attempted';
ALTER TABLE proxy_request_logs ADD COLUMN continuation_rounds INTEGER NOT NULL DEFAULT 0;
ALTER TABLE proxy_request_logs ADD COLUMN session_enriched INTEGER NOT NULL DEFAULT 0;
ALTER TABLE proxy_request_logs ADD COLUMN turn_id TEXT;
ALTER TABLE proxy_request_logs ADD COLUMN prompt_replaced INTEGER NOT NULL DEFAULT 0;
ALTER TABLE proxy_request_logs ADD COLUMN identity_corrected INTEGER NOT NULL DEFAULT 0;
ALTER TABLE proxy_request_logs ADD COLUMN prompt_fingerprint TEXT;

CREATE TABLE codex_reasoning_rounds (
  request_id TEXT NOT NULL,
  round_index INTEGER NOT NULL,
  reasoning_tokens INTEGER,
  decision TEXT NOT NULL,
  status TEXT NOT NULL,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  error_code TEXT,
  PRIMARY KEY (request_id, round_index),
  FOREIGN KEY (request_id) REFERENCES proxy_request_logs(request_id) ON DELETE CASCADE
);

CREATE TABLE provider_credential_audit (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL,
  app_type TEXT NOT NULL,
  source TEXT NOT NULL,
  changed_fields TEXT NOT NULL,
  before_fingerprint TEXT NOT NULL,
  after_fingerprint TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE provider_rollback_snapshots (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL,
  app_type TEXT NOT NULL,
  provider_json TEXT NOT NULL,
  source_revision INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
);

CREATE TABLE app_configuration_state (
  app_type TEXT PRIMARY KEY,
  state TEXT NOT NULL,
  reason TEXT,
  detected_at INTEGER,
  updated_at INTEGER NOT NULL
);
```

Use `add_column_if_missing` and `CREATE TABLE IF NOT EXISTS` inside the migration so interrupted/legacy states remain idempotent.

- [ ] **Step 4: Implement credential extraction, normalization, masking and audit retention**

Use explicit `match AppType` extraction. SHA-256 input must include the field name and a NUL separator:

```rust
pub fn credential_fingerprint(field: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(field.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
```

`prune_snapshots` must delete rows older than 30 days, then retain only the newest 10 per `(app_type, provider_id)`.

- [ ] **Step 5: Run the focused backend tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml provider_security -- --nocapture`
Expected: PASS; credentials normalize consistently, audit rows contain no raw key, retention keeps at most 10 recent snapshots.

Run: `cargo test --manifest-path src-tauri/Cargo.toml database:: -- --nocapture`
Expected: PASS; v13 migrates to v14 and existing database regressions remain green.

- [ ] **Step 6: Commit the schema slice**

```powershell
git add src-tauri/src/database src-tauri/src/services/provider_security src-tauri/src/services/mod.rs
git commit -m "feat(security): add provider credential safety schema"
```

## Task 2: 用 CAS 与补偿事务替换危险 Provider 写路径

**Files:**

- Create: `src-tauri/src/services/provider_security/mutation.rs`
- Create: `src-tauri/src/services/provider_security/recovery.rs`
- Create: `src-tauri/src/commands/provider_security.rs`
- Modify: `src-tauri/src/store.rs`
- Modify: `src-tauri/src/database/dao/providers.rs:20-333`
- Modify: `src-tauri/src/services/provider/mod.rs:2064-2758,3309-3380`
- Modify: `src-tauri/src/services/proxy.rs:507-1094`
- Modify: `src-tauri/src/services/provider/live.rs:1661-1875`
- Modify: `src-tauri/src/commands/provider.rs:1-76`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs:1190-1415`
- Test: inline tests in mutation/recovery/provider/proxy modules

**Interfaces:**

- Produces:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSecurityStatus {
    pub provider_id: String,
    pub app_type: String,
    pub revision: i64,
    pub credential_valid: bool,
    pub conflicts: Vec<CredentialDiff>,
    pub configuration_state: ConfigurationState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationState { Consistent, Inconsistent }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryMode { ProjectDbToLive, ImportLiveToDb }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryResult {
    pub state: ConfigurationState,
    pub revision: i64,
    pub live_fingerprint_verified: bool,
    pub audit_written: bool,
}

pub struct ProviderMutationRequest {
    pub app_type: AppType,
    pub provider: Provider,
    pub expected_revision: i64,
    pub source: CredentialSource,
    pub confirmed_credential_fields: BTreeSet<String>,
}

pub struct ProviderMutationCoordinator {
    db: Arc<Database>,
    app_locks: HashMap<String, Arc<tokio::sync::Mutex<()>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum MutationOutcome {
    Saved { revision: i64, warnings: Vec<String> },
    Conflict { current_revision: i64, diff: Vec<CredentialDiff> },
}

impl ProviderMutationCoordinator {
    pub async fn mutate(&self, request: ProviderMutationRequest) -> Result<MutationOutcome, AppError>;
    pub async fn recover(&self, app_type: AppType, mode: RecoveryMode) -> Result<RecoveryResult, AppError>;
}
```

- Consumes: Task 1 schema/types and existing atomic Live writers.

- [ ] **Step 1: Write tests for stale revision, DB-authoritative switching and compensation failure**

```rust
#[test]
fn stale_revision_is_rejected_without_overwrite() -> Result<(), AppError> {
    let fixture = SecurityFixture::new()?;
    let first = fixture.update_key("sk-first", 1)?;
    assert_eq!(first.revision(), Some(2));
    let stale = fixture.update_key("sk-stale", 1)?;
    assert!(matches!(stale, MutationOutcome::Conflict { current_revision: 2, .. }));
    assert_eq!(fixture.stored_key()?, "sk-first");
    Ok(())
}

#[test]
fn switching_projects_db_credentials_over_different_live_credentials() -> Result<(), AppError> {
    let fixture = SecurityFixture::with_live_key("sk-live")?;
    fixture.switch_to_stored_provider("sk-db")?;
    assert_eq!(fixture.stored_key()?, "sk-db");
    assert_eq!(fixture.live_key()?, "sk-db");
    assert_eq!(fixture.audit_count()?, 0, "projection is not a DB credential mutation");
    Ok(())
}
```

- [ ] **Step 2: Run tests and verify they fail on current unconditional saves**

Run: `cargo test --manifest-path src-tauri/Cargo.toml stale_revision_is_rejected_without_overwrite -- --nocapture`
Expected: FAIL because current DAO updates whole JSON without CAS.

Run: `cargo test --manifest-path src-tauri/Cargo.toml switching_projects_db_credentials_over_different_live_credentials -- --nocapture`
Expected: FAIL because switch can backfill Live into DB.

- [ ] **Step 3: Add field-level DAO operations and CAS**

Replace full-save call sites with these explicit methods:

```rust
pub fn update_provider_cas(
    &self,
    app_type: &str,
    provider: &Provider,
    expected_revision: i64,
) -> Result<Option<i64>, AppError>;

pub fn update_provider_sort_index(&self, app_type: &str, id: &str, sort_index: usize) -> Result<(), AppError>;
pub fn update_provider_failover_membership(&self, app_type: &str, id: &str, enabled: bool) -> Result<(), AppError>;
pub fn update_provider_meta_cas(&self, app_type: &str, id: &str, meta: &ProviderMeta, expected_revision: i64) -> Result<Option<i64>, AppError>;
pub fn provider_revision(&self, app_type: &str, id: &str) -> Result<i64, AppError>;
```

The CAS SQL must update only when `revision = expected_revision`, increment revision once, and distinguish “missing row” from “stale row”.

- [ ] **Step 4: Route cross-boundary writes through `ProviderMutationCoordinator`**

Use one `tokio::sync::Mutex<()>` per AppType in `AppState`. Mutation order:

```text
lock app
→ reject persisted inconsistent state
→ validate DB provider/revision and target Live structure
→ capture DB + Live snapshots
→ write DB CAS + audit + rollback snapshot in one SQLite transaction
→ atomically project DB credentials to Live
→ read Live back and compare normalized fingerprints/current pointer
→ on failure compensate DB and Live
→ if compensation fails persist ConfigurationInconsistency
```

Delete the Live → DB backfill at `services/provider/mod.rs:2634-2655`, the proxy takeover token backfill at `services/proxy.rs:851-1010`, and automatic existing-provider overwrites in `services/provider/live.rs`. Startup import may create a provider that does not exist; it may not mutate credentials of an existing row.

- [ ] **Step 5: Add Tauri commands and explicit error codes**

```rust
#[tauri::command]
pub fn get_provider_security_status(state: State<'_, AppState>, app: String, id: String)
    -> Result<ProviderSecurityStatus, String>;

#[tauri::command]
pub async fn import_live_provider_credentials(
    state: State<'_, AppState>, app: String, id: String,
    expected_revision: i64, fields: Vec<String>
) -> Result<MutationOutcome, String>;

#[tauri::command]
pub async fn recover_app_configuration(
    state: State<'_, AppState>, app: String, mode: RecoveryMode
) -> Result<RecoveryResult, String>;
```

Errors exposed to UI must include stable codes `provider_revision_conflict`, `provider_credentials_missing`, `configuration_inconsistent`, and `live_projection_failed`, without raw values.

- [ ] **Step 6: Run focused and regression tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml provider_security -- --nocapture`
Expected: PASS; stale updates reject, compensation failure locks only one app, and recovery requires write/read fingerprint agreement.

Run: `cargo test --manifest-path src-tauri/Cargo.toml services::provider -- --nocapture`
Expected: PASS; switching never imports Live credentials.

Run: `cargo test --manifest-path src-tauri/Cargo.toml services::proxy -- --nocapture`
Expected: PASS; enabling takeover never imports Live credentials.

- [ ] **Step 7: Commit the mutation slice**

```powershell
git add src-tauri/src/services/provider_security src-tauri/src/services/provider src-tauri/src/services/proxy.rs src-tauri/src/database/dao/providers.rs src-tauri/src/commands src-tauri/src/lib.rs src-tauri/src/store.rs
git commit -m "fix(security): make provider credentials database authoritative"
```

## Task 3: 在供应商编辑、卡片和高级设置暴露冲突与恢复

**Files:**

- Create: `src/types/providerSecurity.ts`
- Create: `src/lib/api/providerSecurity.ts`
- Create: `src/components/providers/ProviderCredentialConflict.tsx`
- Create: `src/components/settings/ConfigurationSecuritySection.tsx`
- Modify: `src/lib/api/index.ts`
- Modify: `src/lib/api/providers.ts:45-76`
- Modify: `src/components/providers/EditProviderDialog.tsx:13-230`
- Modify: `src/components/providers/ProviderList.tsx`
- Modify: `src/components/settings/SettingsPage.tsx`
- Modify: `src/hooks/useProviderActions.ts:138-152`
- Test: `tests/components/EditProviderDialog.test.tsx`
- Test: `tests/components/ProviderList.test.tsx`
- Test: `tests/components/SettingsDialog.test.tsx`

**Interfaces:**

- Produces TypeScript mirrors of Task 2 and:

```ts
export interface ProviderUpdateOptions {
  originalId?: string;
  expectedRevision: number;
  confirmedCredentialFields?: Array<"apiKey" | "baseUrl">;
}

export interface ExplicitCredentialImport {
  appId: AppId;
  providerId: string;
  expectedRevision: number;
  fields: Array<"apiKey" | "baseUrl">;
}

export interface CredentialAuditRecord {
  id: string;
  appType: AppId;
  providerId: string;
  source: string;
  changedFields: string[];
  beforeFingerprint: string;
  afterFingerprint: string;
  createdAt: number;
}

export interface ProviderSecurityApi {
  status(appId: AppId, providerId: string): Promise<ProviderSecurityStatus>;
  importLiveCredentials(args: ExplicitCredentialImport): Promise<MutationOutcome>;
  listAudit(appId?: AppId, providerId?: string): Promise<CredentialAuditRecord[]>;
  rollback(snapshotId: string, expectedRevision: number): Promise<MutationOutcome>;
  recover(appId: AppId, mode: "project_db_to_live" | "import_live_to_db"): Promise<RecoveryResult>;
}
```

- [ ] **Step 1: Write UI tests proving Live no longer seeds the edit form**

```tsx
it("uses stored provider credentials and shows a redacted live conflict", async () => {
  render(<EditProviderDialog open provider={storedProvider} appId="codex" {...props} />);
  expect(await screen.findByDisplayValue("sk-db-secret")).toBeInTheDocument();
  expect(screen.queryByDisplayValue("sk-live-secret")).not.toBeInTheDocument();
  expect(await screen.findByText(/Live.*冲突/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /从 Live 导入/ })).toBeInTheDocument();
});
```

Add tests for stale-save dialog, masked differences, rollback confirmation and one-app recovery lock.

- [ ] **Step 2: Run tests and verify current behavior fails**

Run: `pnpm test:unit -- tests/components/EditProviderDialog.test.tsx tests/components/ProviderList.test.tsx tests/components/SettingsDialog.test.tsx`
Expected: FAIL because `EditProviderDialog` currently uses `liveSettings ?? provider.settingsConfig` and no security UI exists.

- [ ] **Step 3: Make stored Provider the form source and Live read-only comparison data**

Change the edit initialization to:

```ts
const initialSettingsConfig = useMemo(
  () => (provider?.settingsConfig ?? {}) as Record<string, unknown>,
  [provider?.settingsConfig],
);
```

Load `ProviderSecurityStatus` separately. A save sends the revision captured when the form opened. On `provider_revision_conflict`, keep the panel open and show “重新加载” and “另存副本”; do not merge fields automatically.

- [ ] **Step 4: Add conflict badge, explicit import and advanced recovery UI**

The import dialog defaults all conflicting fields unchecked, displays masked DB/Live values and requires confirmation. Configuration Security lists audit metadata and rollback snapshots without raw values. Configuration Inconsistency displays the two recovery modes and the exact unlock checks.

- [ ] **Step 5: Run UI tests and typecheck**

Run: `pnpm test:unit -- tests/components/EditProviderDialog.test.tsx tests/components/ProviderList.test.tsx tests/components/SettingsDialog.test.tsx`
Expected: PASS.

Run: `pnpm typecheck`
Expected: PASS with `expectedRevision` required on full provider updates.

- [ ] **Step 6: Commit the UI slice**

```powershell
git add src/types src/lib/api src/components/providers src/components/settings src/hooks tests/components
git commit -m "feat(security): surface provider credential conflicts and recovery"
```

## Task 4: 让云同步默认保留本机凭据，让精确恢复显式确认

**Files:**

- Modify: `src-tauri/src/database/backup.rs:18-145`
- Modify: `src-tauri/src/services/sync_protocol.rs:1-330`
- Modify: `src-tauri/src/services/webdav_sync.rs:64-160`
- Modify: `src-tauri/src/services/s3_sync.rs:49-132`
- Modify: `src-tauri/src/commands/webdav_sync.rs`
- Modify: `src-tauri/src/commands/s3_sync.rs`
- Modify: `src-tauri/src/commands/import_export.rs`
- Modify: `src/hooks/useImportExport.ts`
- Modify: `src/components/settings/WebdavSyncSection.tsx`
- Modify: S3 settings UI in `src/components/settings/SettingsPage.tsx`
- Modify: `src/components/settings/ImportExportSection.tsx`
- Test: sync/backup inline Rust tests and `tests/components/ImportExportSection.test.tsx`

**Interfaces:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCredentialSelection {
    pub app_type: String,
    pub provider_id: String,
    pub use_remote_api_key: bool,
    pub use_remote_base_url: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub preview_id: String,
    pub new_provider_count: u32,
    pub existing_provider_count: u32,
    pub credential_conflicts: Vec<CredentialDiff>,
    pub exact_restore_credential_field_count: u32,
}
```

- [x] **Step 1: Write tests for cloud merge and exact restore preview**

```rust
#[test]
fn cloud_restore_preserves_existing_local_credentials_by_default() -> Result<(), AppError> {
    let result = apply_sync_fixture(local("sk-local"), remote("sk-remote"), &[])?;
    assert_eq!(result.provider_key("codex", "p1")?, "sk-local");
    assert_eq!(result.provider_name("codex", "p1")?, "Remote Renamed Provider");
    Ok(())
}

#[test]
fn exact_restore_preview_counts_credential_changes_without_applying() -> Result<(), AppError> {
    let preview = preview_exact_restore(&fixture_sql("sk-remote"))?;
    assert_eq!(preview.exact_restore_credential_field_count, 1);
    assert_eq!(current_key()?, "sk-local");
    Ok(())
}
```

- [x] **Step 2: Run tests and verify whole-table import fails the policy**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cloud_restore_preserves_existing_local_credentials_by_default -- --nocapture`
Expected: FAIL because current sync import replaces Provider rows without credential merge.

Run: `cargo test --manifest-path src-tauri/Cargo.toml exact_restore_preview_counts_credential_changes_without_applying -- --nocapture`
Expected: FAIL because exact restore has no preview.

- [x] **Step 3: Add prepare/apply restore flow**

Transport download verifies manifest/hash once, stores `db.sql` and `skills.zip` in `~/.cc-switch/sync-staging/<preview_id>/`, then returns `RestorePreview`. Apply validates the preview nonce and file hashes again, applies selected remote credential fields, and deletes staging on success. Staging older than 24 hours is removed on startup.

Default merge policy:

```text
local provider exists → keep local api_key/base_url, accept remote non-credential fields
local provider absent → import full remote provider
explicit per-field remote selection → use selected remote credential and audit it
```

- [x] **Step 4: Exclude and preserve local security tables**

Add to both `SYNC_SKIP_TABLES` and `SYNC_PRESERVE_TABLES`:

```rust
"codex_reasoning_rounds",
"provider_credential_audit",
"provider_rollback_snapshots",
"app_configuration_state",
```

Increment `DB_COMPAT_VERSION` from 6 to 7. Do not change manual `export_sql_string()`; exact backups remain complete.

- [x] **Step 5: Add preview UI and double confirmation**

Cloud restore preview preselects local credentials. Manual SQL/local DB restore shows Provider and credential-field counts, then requires the existing confirm action plus a second credential-impact confirmation before apply.

- [x] **Step 6: Run backend, UI and type checks**

Run: `cargo test --manifest-path src-tauri/Cargo.toml sync_protocol -- --nocapture`
Expected: PASS.

Run: `cargo test --manifest-path src-tauri/Cargo.toml backup -- --nocapture`
Expected: PASS.

Run: `pnpm test:unit -- tests/components/ImportExportSection.test.tsx tests/components/WebdavSyncSection.test.tsx`
Expected: PASS.

Run: `pnpm typecheck`
Expected: PASS.

- [x] **Step 7: Commit the restore slice**

```powershell
git add src-tauri/src/database/backup.rs src-tauri/src/services/sync_protocol.rs src-tauri/src/services/webdav_sync.rs src-tauri/src/services/s3_sync.rs src-tauri/src/commands src/components/settings src/hooks tests
git commit -m "feat(sync): preserve local provider credentials on cloud restore"
```

## Task 5: 扩展请求日志契约并添加“推理”列空态

**Files:**

- Modify: `src-tauri/src/proxy/usage/parser.rs`
- Modify: `src-tauri/src/proxy/usage/logger.rs:14-116,312-369`
- Modify: `src-tauri/src/services/usage_stats.rs:100-215,1466-1597`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src/types/usage.ts:1-45`
- Modify: `src/components/usage/RequestLogTable.tsx`
- Modify: `src/components/usage/RequestDetailPanel.tsx`
- Modify: `tests/components/RequestLogTable.test.tsx`

**Interfaces:**

```rust
#[derive(Debug, Clone, Default)]
pub struct CodexReasoningUsage {
    pub reasoning_tokens: Option<u32>,
    pub reasoning_source: Option<String>,
    pub continuation_status: String,
    pub continuation_rounds: u32,
    pub turn_id: Option<String>,
    pub prompt_replaced: bool,
    pub identity_corrected: bool,
    pub prompt_fingerprint: Option<String>,
}
```

```ts
export interface RequestLog {
  // existing fields unchanged
  reasoningTokens?: number;
  reasoningSource?: "proxy_response" | "codex_session";
  continuationStatus:
    | "not_attempted" | "not_eligible" | "not_triggered"
    | "continued" | "skipped" | "partial_failed";
  continuationRounds: number;
  sessionEnriched: boolean;
  turnId?: string;
  promptReplaced: boolean;
  identityCorrected: boolean;
  promptFingerprint?: string;
}
```

- [ ] **Step 1: Write table rendering tests for unknown, zero, value and continuation**

```tsx
it.each([
  [undefined, 0, "not_attempted", "—"],
  [0, 0, "not_triggered", "Tok 0"],
  [500, 0, "not_triggered", "Tok 500"],
  [500, 2, "continued", "Tok 500 ✨2"],
  [500, 1, "partial_failed", "Tok 500 ⚠"],
])("renders reasoning token semantics", async (tokens, rounds, status, label) => {
  useRequestLogsMock.mockReturnValue(logResult({
    reasoningTokens: tokens,
    continuationRounds: rounds,
    continuationStatus: status,
  }));
  render(<RequestLogTable {...defaultProps} />);
  expect(await screen.findByText(label)).toBeInTheDocument();
});
```

- [ ] **Step 2: Run the test and verify fields are missing**

Run: `pnpm test:unit -- tests/components/RequestLogTable.test.tsx`
Expected: FAIL because the “推理” column does not exist.

- [ ] **Step 3: Thread nullable reasoning metadata through logger and SQL mapping**

Extend `RequestLog`, `log_with_calculation`, `RequestLogDetail`, every SELECT list and `row_to_request_log_detail`. Preserve `Option<u32>` so SQL NULL and integer 0 remain different. Do not change `CostCalculator`; reasoning is already part of output.

- [ ] **Step 4: Add table and detail rendering**

Use a single formatter:

```ts
export function formatReasoning(log: Pick<RequestLog,
  "reasoningTokens" | "continuationRounds" | "continuationStatus"
>): string {
  if (log.reasoningTokens === undefined) return "—";
  const base = `Tok ${log.reasoningTokens.toLocaleString()}`;
  if (log.continuationStatus === "partial_failed") return `${base} ⚠`;
  return log.continuationRounds > 0 ? `${base} ✨${log.continuationRounds}` : base;
}
```

Details show metadata only. No response/reasoning body field is added.

- [ ] **Step 5: Run focused tests and checks**

Run: `cargo test --manifest-path src-tauri/Cargo.toml usage::logger -- --nocapture`
Expected: PASS, including NULL vs 0 persistence.

Run: `cargo test --manifest-path src-tauri/Cargo.toml usage_stats -- --nocapture`
Expected: PASS, including NULL vs 0 query round-trip.

Run: `pnpm test:unit -- tests/components/RequestLogTable.test.tsx tests/components/usageFormat.test.ts`
Expected: PASS.

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 6: Commit the log-contract slice**

```powershell
git add src-tauri/src/proxy/usage src-tauri/src/services/usage_stats.rs src/types/usage.ts src/components/usage tests/components
git commit -m "feat(usage): add reasoning token log contract"
```

## Phase 1 Gate

- [ ] Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` — Expected: PASS.
- [ ] Run: `cargo test --manifest-path src-tauri/Cargo.toml` — Expected: PASS.
- [ ] Run: `pnpm test:unit` — Expected: PASS.
- [ ] Run: `pnpm typecheck` — Expected: PASS.
- [ ] Manual fixture: alter current Codex Live Key, focus CC Switch, edit/switch/take over — Expected: stored Key unchanged, conflict shown, Live reprojected only after an allowed operation.
- [ ] Continue directly to Phase 2 when all checks pass.

---

# Phase 2 — Codex 专属工作台与页面增强

## Task 6: 建立工作台设置、状态契约和原生页面入口

**Files:**

- Create: `src-tauri/src/services/codex_workbench.rs`
- Create: `src-tauri/src/commands/codex_workbench.rs`
- Create: `src/types/codexWorkbench.ts`
- Create: `src/lib/api/codexWorkbench.ts`
- Create: `src/lib/query/codexWorkbench.ts`
- Create: `src/components/codex-workbench/CodexWorkbenchPage.tsx`
- Create: `src/components/codex-workbench/OverviewTab.tsx`
- Create: `src/components/codex-workbench/EnhancementsTab.tsx`
- Modify: `src-tauri/src/settings.rs:339-580`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs:1-65,1190-1415`
- Modify: `src/types.ts:350-430`
- Modify: `src/lib/schemas/settings.ts`
- Modify: `src/hooks/useSettingsForm.ts`
- Modify: `src/App.tsx:90-170,889-970,1400-1580`
- Modify: `src/i18n/locales/zh.json`, `en.json`, `ja.json`
- Test: `tests/components/CodexWorkbenchPage.test.tsx`
- Test: `tests/integration/App.test.tsx`

**Interfaces:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnhancementSettings {
    pub plugin_unlock: bool,
    pub auto_expand: bool,
    pub session_delete: bool,
    pub wide_conversation: bool,
    pub native_menu: bool,
    pub user_script_runtime: bool,
    pub markdown_export: bool,
    pub project_move: bool,
    pub service_tier: bool,
    pub upstream_worktree: bool,
    pub devtools: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexWorkbenchSettings {
    pub enhancements: CodexEnhancementSettings,
    pub script_market_url: String,
    pub radar_cache_ttl_minutes: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexWorkbenchStatus {
    pub platform_supported: bool,
    pub install_state: String,
    pub runtime_state: String,
    pub cdp_port: Option<u16>,
    pub bridge_state: String,
    pub current_provider_id: Option<String>,
    pub proxy_running: bool,
    pub proxy_takeover: bool,
    pub diagnostics: Vec<String>,
}
```

Default implementation must encode the approved matrix exactly: the first six enhancement flags true, the final five false; script market URL is the CodexElves source URL; radar TTL is 30.

- [ ] **Step 1: Write default-setting and navigation tests**

```rust
#[test]
fn codex_workbench_defaults_match_approved_matrix() {
    let value = CodexWorkbenchSettings::default();
    assert!(value.enhancements.plugin_unlock);
    assert!(value.enhancements.user_script_runtime);
    assert!(!value.enhancements.devtools);
    assert_eq!(value.radar_cache_ttl_minutes, 30);
}
```

```tsx
it("shows the Codex workbench only for the Codex app", async () => {
  render(<App />);
  await switchActiveApp("codex");
  await user.click(screen.getByTitle("Codex 工作台"));
  expect(screen.getByRole("heading", { name: "Codex 工作台" })).toBeInTheDocument();
  expect(screen.getByRole("tab", { name: "概览/运行" })).toBeInTheDocument();
});
```

- [ ] **Step 2: Run tests and verify the shell is absent**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_workbench_defaults_match_approved_matrix -- --nocapture`
Expected: FAIL because the settings type is absent.

Run: `pnpm test:unit -- tests/components/CodexWorkbenchPage.test.tsx tests/integration/App.test.tsx`
Expected: FAIL because `codexWorkbench` is not a `View`.

- [ ] **Step 3: Add settings with serde defaults and frontend schema support**

Append `codex_workbench: CodexWorkbenchSettings` to `AppSettings` with `#[serde(default)]`. Mirror every field in TypeScript and Zod. Existing settings files without the field must load with approved defaults, and saving unrelated settings must preserve workbench state.

- [ ] **Step 4: Add the workbench view and five tabs**

Add `codexWorkbench` to `View`/`VALID_VIEWS`; render it only when `activeApp === "codex"`. On switching away while the view is active, return to `providers`. Add tabs `overview`, `enhancements`, `scripts`, `plugins`, `radar`; later tasks fill the last three without changing navigation.

- [ ] **Step 5: Add read-only status command and query**

`get_codex_workbench_status` initially returns platform/install/proxy/provider state and `runtime_state="stopped"`; no launch mutation yet. Poll every 2 seconds only while the workbench is visible.

- [ ] **Step 6: Run focused tests and typecheck**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_workbench -- --nocapture`
Expected: PASS.

Run: `pnpm test:unit -- tests/components/CodexWorkbenchPage.test.tsx tests/integration/App.test.tsx tests/hooks/useSettingsForm.test.tsx`
Expected: PASS.

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 7: Commit the shell slice**

```powershell
git add src-tauri/src/settings.rs src-tauri/src/services/codex_workbench.rs src-tauri/src/commands src-tauri/src/lib.rs src-tauri/src/services/mod.rs src/types src/lib src/hooks src/App.tsx src/components/codex-workbench src/i18n tests
git commit -m "feat(codex): add Codex workbench shell"
```

## Task 7: 实现 Windows 增强启动、CDP 状态和安全桥接

**Files:**

- Create: `src-tauri/src/services/codex_runtime/mod.rs`
- Create: `src-tauri/src/services/codex_runtime/discovery.rs`
- Create: `src-tauri/src/services/codex_runtime/launcher.rs`
- Create: `src-tauri/src/services/codex_runtime/state.rs`
- Create: `src-tauri/src/services/codex_runtime/cdp.rs`
- Create: `src-tauri/src/services/codex_injection/mod.rs`
- Create: `src-tauri/src/services/codex_injection/bridge.rs`
- Create: `src-tauri/src/services/codex_injection/bundle.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/store.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/services/codex_workbench.rs`
- Modify: `src-tauri/src/commands/codex_workbench.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/api/codexWorkbench.ts`
- Modify: `src/components/codex-workbench/OverviewTab.tsx`
- Test: inline Rust tests in new modules
- Test: `tests/components/CodexWorkbenchPage.test.tsx`

**Interfaces:**

```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexRuntimeState {
    Stopped, Launching, Injecting, Running,
    OrdinaryRunning, Degraded, StaleLock, Unsupported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchEnhancedCodexResult {
    pub state: CodexRuntimeState,
    pub pid: Option<u32>,
    pub cdp_port: Option<u16>,
    pub bridge_port: Option<u16>,
    pub message_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexInstall {
    pub executable: PathBuf,
    pub app_dir: PathBuf,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexProcess {
    pub pid: u32,
    pub executable: PathBuf,
    pub cdp_port: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CdpTarget {
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub websocket_url: Option<String>,
}

pub trait CodexProcessInspector: Send + Sync {
    fn discover_install(&self) -> Result<Option<CodexInstall>, AppError>;
    fn running_processes(&self) -> Result<Vec<CodexProcess>, AppError>;
}

pub trait CdpClient: Send + Sync {
    async fn list_targets(&self, port: u16) -> Result<Vec<CdpTarget>, AppError>;
    async fn add_new_document_script(&self, websocket_url: &str, source: &str) -> Result<(), AppError>;
    async fn evaluate(&self, websocket_url: &str, source: &str) -> Result<Value, AppError>;
}
```

- [ ] **Step 1: Write discovery/state/target/nonce tests before adding dependencies**

```rust
#[test]
fn ordinary_running_is_never_killed_or_relaunched() {
    let hooks = FakeHooks::ordinary_codex_without_cdp();
    let result = launch_with_hooks(&hooks).unwrap();
    assert_eq!(result.state, CodexRuntimeState::OrdinaryRunning);
    assert_eq!(hooks.kill_calls(), 0);
    assert_eq!(hooks.spawn_calls(), 0);
}

#[test]
fn picks_only_injectable_codex_page_target() {
    let target = pick_codex_target(&[
        target("service_worker", "https://chatgpt.com/sw"),
        target("page", "https://chatgpt.com/codex/tasks/1"),
    ]).unwrap();
    assert_eq!(target.kind, "page");
}

#[tokio::test]
async fn bridge_rejects_missing_nonce() {
    let bridge = TestBridge::start().await;
    assert_eq!(bridge.get("/health", None).await.status(), 401);
}
```

- [ ] **Step 2: Run tests and verify runtime modules are missing**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_runtime -- --nocapture`
Expected: FAIL because the modules do not exist.

- [ ] **Step 3: Add only required dependencies and Windows features**

```toml
futures-util = "0.3"
tokio-tungstenite = { version = "0.26", features = ["rustls-tls-webpki-roots"] }
```

Extend existing `windows-sys` features only for process enumeration/activation used by the implementation. Do not add CodexElves encryption, directory or second SQLite dependencies.

- [ ] **Step 4: Implement discovery, launch state and lock validation**

Port the relevant behavior from CodexElves `launcher.rs`/`watcher.rs`, adapted to CC Switch. Scan ports `19222..=19242`; generate the bridge nonce from two UUID v4 values hashed with SHA-256; store only the hash in diagnostics. Lock JSON:

```json
{
  "instanceId": "uuid",
  "pid": 1234,
  "cdpPort": 19222,
  "bridgePort": 19243,
  "startedAt": 1780000000
}
```

Validate PID plus executable identity before trusting a lock. Never call a Codex process termination API.

- [ ] **Step 5: Implement CDP and loopback bridge**

Use 3-second HTTP target-list timeout and 5-second WebSocket command timeout. The bridge binds `127.0.0.1:0`, limits bodies to 1 MiB, applies a 5-second request timeout, and requires `Authorization: Bearer <nonce>`. Route whitelist initially contains `/health`, `/runtime/bundle`, `/session/delete`; unimplemented actions return a typed 501.

- [ ] **Step 6: Wire launch/reinject commands and UI state**

```rust
#[tauri::command]
pub async fn launch_enhanced_codex(state: State<'_, AppState>) -> Result<LaunchEnhancedCodexResult, String>;

#[tauri::command]
pub async fn reinject_codex_enhancements(state: State<'_, AppState>) -> Result<CodexWorkbenchStatus, String>;
```

The Overview tab disables launch while `launching/injecting`; for `ordinary_running`, show the close-and-retry instruction and no terminate button.

- [ ] **Step 7: Run backend/UI tests and typecheck**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_runtime -- --nocapture`
Expected: PASS on all platforms; Windows-specific tests use fakes when not on Windows.

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_injection -- --nocapture`
Expected: PASS; unauthorized bridge requests fail and target selection ignores non-page targets.

Run: `pnpm test:unit -- tests/components/CodexWorkbenchPage.test.tsx`
Expected: PASS.

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 8: Commit the runtime slice**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/services/codex_runtime src-tauri/src/services/codex_injection src-tauri/src/services/codex_workbench.rs src-tauri/src/commands/codex_workbench.rs src-tauri/src/store.rs src-tauri/src/lib.rs src/lib src/components/codex-workbench tests
git commit -m "feat(codex): launch and inject enhanced Codex on Windows"
```

## Task 8: 移植可独立启停的页面增强

**Files:**

- Create: `src-tauri/resources/codex-workbench/renderer-inject.js`
- Create: `src-tauri/resources/codex-workbench/renderer-features.js`
- Modify: `src-tauri/src/services/codex_injection/bundle.rs`
- Modify: `src-tauri/src/services/codex_injection/mod.rs`
- Modify: `src-tauri/src/services/codex_workbench.rs`
- Modify: `src-tauri/src/commands/codex_workbench.rs`
- Modify: `src/components/codex-workbench/EnhancementsTab.tsx`
- Test: inline Rust bundle tests
- Test: `tests/components/CodexWorkbenchPage.test.tsx`

**Interfaces:**

The injected runtime exposes only this namespaced contract:

```ts
interface CcSwitchCodexRuntime {
  instanceId: string;
  version: 1;
  configure(flags: CodexEnhancementSettings): void;
  status(): Record<string, { state: "loaded" | "disabled" | "failed"; error?: string }>;
  dispose(): void;
}

declare global {
  interface Window { __ccSwitchCodex?: CcSwitchCodexRuntime }
}
```

- [ ] **Step 1: Write bundle tests for approved flags, idempotence and secret exclusion**

```rust
#[test]
fn bootstrap_contains_feature_flags_but_no_secrets() {
    let bundle = build_injection_bundle(&fixture_settings(), "instance-1", "nonce-value")?;
    assert!(bundle.contains("pluginUnlock"));
    assert!(bundle.contains("wideConversation"));
    assert!(!bundle.contains("OPENAI_API_KEY"));
    assert_eq!(bundle.matches("instance-1").count() > 0, true);
}
```

- [ ] **Step 2: Run tests and verify assets/bundle are absent**

Run: `cargo test --manifest-path src-tauri/Cargo.toml bootstrap_contains_feature_flags -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Port and split CodexElves DOM capabilities**

Use `renderer-inject.js` and `renderer-features.js` from commit `bf1224e` as references. Remove CodexElves manager endpoints, launcher control and branding. Implement every feature as `{ mount, update, dispose }`; catch errors per feature and report them through `status()` without stopping the rest.

Approved on-by-default: plugin unlock, auto expand, session delete, wide conversation, native menu, user scripts. Approved off-by-default: Markdown export, project move, Service Tier, upstream Worktree, DevTools.

- [ ] **Step 4: Make configuration updates live and injection idempotent**

If `window.__ccSwitchCodex?.instanceId` matches, call `configure` instead of injecting again. If it differs, call `dispose`, replace the runtime and mount once. Use `Page.addScriptToEvaluateOnNewDocument` plus one immediate `Runtime.evaluate`.

- [ ] **Step 5: Add per-feature UI status and reset defaults**

Toggles persist through `AppSettings`, trigger `configure` on a running enhanced instance, and show `loaded/disabled/failed`. “恢复推荐默认值” applies the exact default matrix in one save.

- [ ] **Step 6: Run tests and a local enhanced-Codex smoke check**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_injection -- --nocapture`
Expected: PASS.

Run: `pnpm test:unit -- tests/components/CodexWorkbenchPage.test.tsx`
Expected: PASS.

Manual: launch enhanced Codex, navigate between two tasks, toggle wide conversation off/on.
Expected: runtime remains single-instanced; navigation reinjects; one feature toggle does not reload Codex.

- [ ] **Step 7: Commit the enhancement slice**

```powershell
git add src-tauri/resources/codex-workbench src-tauri/src/services/codex_injection src-tauri/src/services/codex_workbench.rs src-tauri/src/commands/codex_workbench.rs src/components/codex-workbench tests
git commit -m "feat(codex): add configurable Codex page enhancements"
```

## Task 9: 添加本地用户脚本与手动远程市场

**Files:**

- Create: `src-tauri/src/services/codex_scripts.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/services/codex_injection/bundle.rs`
- Modify: `src-tauri/src/commands/codex_workbench.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/components/codex-workbench/ScriptsTab.tsx`
- Modify: `src/types/codexWorkbench.ts`
- Modify: `src/lib/api/codexWorkbench.ts`
- Modify: `src/lib/query/codexWorkbench.ts`
- Test: inline Rust tests
- Test: `tests/components/CodexWorkbenchPage.test.tsx`

**Interfaces:**

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserScriptInfo {
    pub key: String,
    pub name: String,
    pub source: String,
    pub enabled: bool,
    pub version: Option<String>,
    pub sha256: String,
    pub runtime_state: String,
    pub runtime_error: Option<String>,
}

pub struct ScriptInstallRequest {
    pub market_id: String,
    pub expected_version: String,
    pub expected_sha256: Option<String>,
}
```

- [ ] **Step 1: Write path traversal, hash mismatch and old-version preservation tests**

```rust
#[test]
fn market_script_install_is_atomic_and_preserves_old_version() -> Result<(), AppError> {
    let fixture = ScriptFixture::with_installed("demo", "old code")?;
    let error = fixture.install_with_hash("demo", "new code", "wrong-hash").unwrap_err();
    assert!(error.to_string().contains("SHA-256"));
    assert_eq!(fixture.contents("demo")?, "old code");
    Ok(())
}

#[test]
fn market_id_cannot_escape_script_root() {
    assert!(sanitize_market_id("../../outside").is_err());
}
```

- [ ] **Step 2: Run tests and verify manager is absent**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_scripts -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Implement local inventory and safe bundle construction**

Store under `get_app_config_dir()/codex-workbench/scripts`. Config writes use `config::atomic_write`. Read only regular `.js` files whose canonical paths remain under builtin/user roots. Wrap each enabled script in its own try/catch and expose runtime status in `window.__ccSwitchCodexUserScripts`.

- [ ] **Step 4: Implement explicit market refresh/install/update**

Use the configured HTTPS market URL, 15-second timeout and 10 MiB script limit. If manifest `sha256` is non-empty it is mandatory; if empty return `verification="unavailable"` for UI display. Never fetch/install/update from a background timer.

- [ ] **Step 5: Add script UI and reinjection**

Scripts tab supports refresh market, local import, enable/disable, delete user script, install/update and open containing folder. Every mutation rebuilds the bundle and reinjects if enhanced Codex is running. Deleting builtin scripts is disabled.

- [ ] **Step 6: Run tests and smoke check**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_scripts -- --nocapture`
Expected: PASS.

Run: `pnpm test:unit -- tests/components/CodexWorkbenchPage.test.tsx`
Expected: PASS; no update occurs without clicking.

Manual: install a fixture script, break the next update hash, refresh Codex page.
Expected: old script still loads and failed update is visible.

- [ ] **Step 7: Commit the scripts slice**

```powershell
git add src-tauri/src/services/codex_scripts.rs src-tauri/src/services/codex_injection src-tauri/src/commands/codex_workbench.rs src-tauri/src/lib.rs src/types/codexWorkbench.ts src/lib src/components/codex-workbench tests
git commit -m "feat(codex): manage Codex user scripts and market"
```

## Task 10: 添加 Codex 插件市场与缓存管理

**Files:**

- Create: `src-tauri/src/services/codex_plugins.rs`
- Create: `src-tauri/resources/codex-workbench/openai-curated-remote.zip`
- Modify: `src-tauri/src/codex_config.rs:158-165`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/commands/codex_workbench.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/components/codex-workbench/PluginsTab.tsx`
- Modify: `src/types/codexWorkbench.ts`
- Modify: `src/lib/api/codexWorkbench.ts`
- Modify: `src/lib/query/codexWorkbench.ts`
- Test: inline Rust tests
- Test: `tests/components/CodexWorkbenchPage.test.tsx`

**Interfaces:**

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCacheInfo {
    pub id: String,
    pub marketplace: String,
    pub source_version: Option<String>,
    pub current_version: Option<String>,
    pub cached_versions: Vec<String>,
    pub can_refresh: bool,
    pub refresh_reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceResult {
    pub initialized: bool,
    pub configured: bool,
    pub marketplace_root: Option<String>,
}

pub fn effective_codex_home() -> PathBuf;
pub async fn initialize_curated_marketplace(home: &Path) -> Result<MarketplaceResult, AppError>;
pub fn refresh_plugin_cache(home: &Path, plugin_id: &str) -> Result<PluginCacheInfo, AppError>;
```

- [ ] **Step 1: Write effective-home, ZIP escape and downgrade-block tests**

```rust
#[test]
fn codex_home_priority_is_override_then_env_then_default() {
    let env = HomeFixture::new().with_env("C:/env-codex").with_override("C:/override");
    assert_eq!(effective_codex_home_with(&env), PathBuf::from("C:/override"));
}

#[test]
fn plugin_refresh_blocks_version_downgrade() {
    let fixture = PluginFixture::cached("2.0.0").with_source("1.9.0");
    assert!(fixture.refresh().unwrap_err().to_string().contains("降级"));
}
```

- [ ] **Step 2: Run tests and verify the service is absent**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_plugins -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Resolve effective CODEX_HOME and port marketplace behavior**

Update `get_codex_config_dir()` priority to explicit override > `CODEX_HOME` > `~/.codex`. Port only curated marketplace, remote marketplace, cache inspection and refresh behavior from CodexElves `plugin_marketplace.rs` at `bf1224e`.

- [ ] **Step 4: Harden ZIP and TOML writes**

Download OpenAI plugins ZIP only after explicit click, with 128 MiB cap. Reject absolute/parent/symlink entries. Extract to a sibling staging directory, validate marketplace and plugin manifests, then atomically rename. Merge marketplace TOML sections with `toml_edit`; preserve all unrelated config text.

- [ ] **Step 5: Add plugin status/UI actions**

Plugins tab displays effective home, marketplace repair status and plugin cache/source versions. “初始化/修复” and “刷新缓存” are separate manual actions. A downgrade remains disabled with its reason.

- [ ] **Step 6: Run tests and smoke check**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_plugins -- --nocapture`
Expected: PASS.

Run: `pnpm test:unit -- tests/components/CodexWorkbenchPage.test.tsx`
Expected: PASS.

Manual: initialize marketplace in a temporary `CODEX_HOME`, refresh one plugin, reopen Codex.
Expected: config retains unrelated entries and plugin cache points to the validated version.

- [ ] **Step 7: Commit the plugin slice**

```powershell
git add src-tauri/src/codex_config.rs src-tauri/src/services/codex_plugins.rs src-tauri/resources/codex-workbench/openai-curated-remote.zip src-tauri/src/commands/codex_workbench.rs src-tauri/src/lib.rs src/types/codexWorkbench.ts src/lib src/components/codex-workbench tests
git commit -m "feat(codex): manage Codex plugin marketplaces and cache"
```

## Task 11: 添加原生降智雷达与 30 分钟过期缓存

**Files:**

- Create: `src-tauri/src/services/codex_radar.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/commands/codex_workbench.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/components/codex-workbench/RadarTab.tsx`
- Modify: `src/types/codexWorkbench.ts`
- Modify: `src/lib/api/codexWorkbench.ts`
- Modify: `src/lib/query/codexWorkbench.ts`
- Test: inline Rust parser/cache tests
- Test: `tests/components/CodexWorkbenchPage.test.tsx`

**Interfaces:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarModelIq {
    pub model: String,
    pub score: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarIqComparison {
    pub left_model: String,
    pub right_model: String,
    pub delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarSnapshot {
    pub fetched_at: i64,
    pub source_url: String,
    pub models: Vec<CodexRadarModelIq>,
    pub comparisons: Vec<CodexRadarIqComparison>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadarResult {
    pub snapshot: Option<CodexRadarSnapshot>,
    pub stale: bool,
    pub from_cache: bool,
    pub error: Option<String>,
}
```

- [ ] **Step 1: Write fixture parser and stale-cache tests**

```rust
#[test]
fn failed_refresh_returns_old_cache_marked_stale() -> Result<(), AppError> {
    let fixture = RadarFixture::with_cache(31 * 60).with_fetch_error("offline");
    let result = fixture.fetch(false)?;
    assert!(result.stale);
    assert!(result.from_cache);
    assert_eq!(result.snapshot.unwrap().models.len(), 2);
    assert_eq!(result.error.as_deref(), Some("offline"));
    Ok(())
}
```

- [ ] **Step 2: Run tests and verify radar service is absent**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_radar -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Port parser and implement cache policy**

Port parsing from CodexElves `codex_radar.rs`, returning DTOs only. Cache JSON under `codex-workbench/cache/radar.json`; validate cache schema before use. `refresh=false` honors 30-minute TTL; `refresh=true` always fetches. A failed fetch with valid old cache returns both snapshot and error.

- [ ] **Step 4: Add Radar tab**

Render native cards/table, fetched time, cache/stale badge, manual refresh and external source link. Never inject remote HTML into React.

- [ ] **Step 5: Run tests and typecheck**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_radar -- --nocapture`
Expected: PASS.

Run: `pnpm test:unit -- tests/components/CodexWorkbenchPage.test.tsx`
Expected: PASS for fresh, stale and empty-error states.

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 6: Commit the radar slice**

```powershell
git add src-tauri/src/services/codex_radar.rs src-tauri/src/services/mod.rs src-tauri/src/commands/codex_workbench.rs src-tauri/src/lib.rs src/types/codexWorkbench.ts src/lib src/components/codex-workbench tests
git commit -m "feat(codex): add cached degradation radar"
```

## Phase 2 Gate

- [ ] Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` — Expected: PASS.
- [ ] Run: `cargo test --manifest-path src-tauri/Cargo.toml` — Expected: PASS.
- [ ] Run: `pnpm test:unit` — Expected: PASS.
- [ ] Run: `pnpm typecheck` — Expected: PASS.
- [ ] Run: `pnpm build:renderer` — Expected: PASS.
- [ ] Windows manual smoke: ordinary-running protection, enhanced launch, page navigation reinjection, toggle one enhancement, script failure preservation, plugin temp-home initialization, stale radar cache.
- [ ] Continue directly to Phase 3 when all checks pass.

---

# Phase 3 — 系统提示词、推理续接与 Token 闭环

## Task 12: 添加 Provider 级提示词/续接设置与确定的请求重写顺序

**Files:**

- Create: `src-tauri/src/services/codex_reasoning/mod.rs`
- Create: `src-tauri/src/services/codex_reasoning/prompt.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/provider.rs:390-520`
- Modify: `src-tauri/src/proxy/forwarder.rs:1114-1510`
- Create: `src/components/providers/forms/CodexReasoningSettings.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`
- Modify: `src/types.ts:172-230`
- Modify: `src/utils/providerMetaUtils.ts`
- Modify: `src/i18n/locales/zh.json`, `en.json`, `ja.json`
- Test: inline Rust tests in `prompt.rs`
- Test: `tests/components/CodexReasoningSettings.test.tsx`
- Test: `tests/utils/providerMetaUtils.test.ts`

**Interfaces:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexSystemPromptConfig {
    pub enabled: bool,
    pub replacement: String,
    #[serde(default = "default_true")]
    pub correct_model_identity: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexReasoningContinuationConfig {
    pub enabled: bool,
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptRewriteMetadata {
    pub replaced: bool,
    pub identity_corrected: bool,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexRequestProtocol { Responses, ChatCompletions }

fn default_true() -> bool { true }
fn default_max_rounds() -> u8 { 3 }

pub fn rewrite_codex_system_prompt(
    request: &mut Value,
    model: &str,
    config: Option<&CodexSystemPromptConfig>,
    protocol: CodexRequestProtocol,
) -> Result<PromptRewriteMetadata, AppError>;
```

Append `codex_system_prompt` and `codex_reasoning_continuation` to Rust/TypeScript `ProviderMeta`. Maximum rounds must be clamped to `0..=3`; UI allows 1–3 when enabled and stores 3 by default.

- [ ] **Step 1: Write prompt rewrite tests before implementation**

```rust
#[test]
fn responses_replaces_only_system_layers_and_corrects_identity() -> Result<(), AppError> {
    let mut request = json!({
        "model": "gpt-5.4",
        "instructions": "old system",
        "input": [
            {"role":"developer","content":"old developer"},
            {"role":"user","content":"do not change GPT-4 in my text"}
        ]
    });
    let meta = rewrite_codex_system_prompt(
        &mut request,
        "gpt-5.4",
        Some(&prompt_config("You are GPT-4.")),
        CodexRequestProtocol::Responses,
    )?;
    assert_eq!(request["instructions"], "You are gpt-5.4.");
    assert_eq!(request["input"].as_array().unwrap().len(), 1);
    assert_eq!(request.pointer("/input/0/content").unwrap(), "do not change GPT-4 in my text");
    assert!(meta.replaced && meta.identity_corrected);
    assert_eq!(meta.fingerprint.as_deref().unwrap().len(), 64);
    Ok(())
}

#[test]
fn continuation_and_prompt_toggles_are_independent() {
    let meta = provider_meta(false, true);
    assert!(!meta.codex_system_prompt.unwrap().enabled);
    assert!(meta.codex_reasoning_continuation.unwrap().enabled);
}
```

- [ ] **Step 2: Run tests and verify types/functions are absent**

Run: `cargo test --manifest-path src-tauri/Cargo.toml responses_replaces_only_system_layers -- --nocapture`
Expected: FAIL.

Run: `pnpm test:unit -- tests/components/CodexReasoningSettings.test.tsx tests/utils/providerMetaUtils.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implement complete replacement and identity correction**

For Responses, set `instructions` to the replacement and remove system/developer input items; for Chat, remove system/developer messages and insert exactly one system message at index 0. Apply identity correction only to that replacement string. Fingerprint the final effective system text; return no text in metadata.

- [ ] **Step 4: Wire request order before protocol conversion**

At the Codex branch in `forwarder.rs`, enforce this explicit order:

```rust
let mut outbound_body = selected_request_body;
let prompt_meta = rewrite_codex_system_prompt(
    &mut outbound_body,
    &selected_model,
    provider.meta.as_ref().and_then(|m| m.codex_system_prompt.as_ref()),
    request_protocol,
)?;
let transformed = transform_for_selected_provider(outbound_body, provider)?;
```

Do not perform the rewrite inside Chat/Anthropic converters; they must receive the already rewritten body. Preserve `prompt_meta` in the per-request context for logging.

- [ ] **Step 5: Add Provider form section**

Show only for `appId="codex"`. The replacement textarea is stored in ProviderMeta but never echoed to toast/error/log output. Prompt and continuation toggles remain separate. Link from workbench overview to edit the current Provider.

- [ ] **Step 6: Run focused tests and checks**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_reasoning::prompt -- --nocapture`
Expected: PASS for Responses, Chat, missing config, user-content preservation and fingerprints.

Run: `pnpm test:unit -- tests/components/CodexReasoningSettings.test.tsx tests/utils/providerMetaUtils.test.ts`
Expected: PASS.

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 7: Commit the prompt slice**

```powershell
git add src-tauri/src/services/codex_reasoning src-tauri/src/provider.rs src-tauri/src/proxy/forwarder.rs src/components/providers/forms src/types.ts src/utils/providerMetaUtils.ts src/i18n tests
git commit -m "feat(codex): add provider-level system prompt replacement"
```

## Task 13: 实现 518 网格判定与续接请求构造核心

**Files:**

- Create: `src-tauri/src/services/codex_reasoning/continuation.rs`
- Create: `src-tauri/src/services/codex_reasoning/stream.rs`
- Create: `src-tauri/src/services/codex_reasoning/usage.rs`
- Create: `src-tauri/src/services/codex_reasoning/tests.rs`
- Modify: `src-tauri/src/services/codex_reasoning/mod.rs`

**Interfaces:**

```rust
pub const GRID_STEP: u64 = 518;
pub const GRID_OFFSET: u64 = 2;
pub const MIN_GRID_MULTIPLE: u64 = 3;
pub const MAX_CONTINUE_ROUNDS: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationStopReason {
    Disabled,
    UnsupportedModel,
    UnsupportedProtocol,
    MissingReasoningTokens,
    NotLowGrid,
    ToolCallPresent,
    EncryptedReasoningMissing,
    MaximumRoundsReached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationDecision {
    Continue { grid_multiple: u64 },
    Stop(ContinuationStopReason),
}

#[derive(Debug, Clone)]
pub struct ContinuationEligibility {
    pub enabled: bool,
    pub model: String,
    pub native_responses: bool,
    pub completed_rounds: u8,
    pub max_rounds: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoundUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct ContinuationRoundResult {
    pub round_index: u8,
    pub sse: Bytes,
    pub usage: RoundUsage,
    pub reasoning_tokens: Option<u32>,
    pub duration_ms: u64,
    pub terminal_output: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct ContinuationRoundRecord {
    pub round_index: u8,
    pub reasoning_tokens: Option<u32>,
    pub decision: String,
    pub status: String,
    pub duration_ms: u64,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RoundUsageAccumulator {
    pub usage: RoundUsage,
    pub reasoning_tokens: Option<u32>,
    pub total_cost: Option<CostBreakdown>,
}

impl RoundUsageAccumulator {
    pub fn add_round(&mut self, round: &ContinuationRoundResult, cost: Option<&CostBreakdown>)
        -> Result<(), AppError>;
}

pub fn grid_multiple(reasoning_tokens: u64) -> Option<u64>;
pub fn decide_continuation(terminal: &Value, eligibility: &ContinuationEligibility) -> ContinuationDecision;
pub fn build_continue_request(original_effective_request: &Value, previous_output: &[Value], round: u8) -> Result<Value, AppError>;
```

- [ ] **Step 1: Write the exact CodexElves grid and safety tests**

```rust
#[test]
fn grid_matches_observed_values() {
    assert_eq!(grid_multiple(516), Some(1));
    assert_eq!(grid_multiple(1034), Some(2));
    assert_eq!(grid_multiple(1552), Some(3));
    assert_eq!(grid_multiple(500), None);
    assert_eq!(grid_multiple(0), None);
}

#[test]
fn only_low_grid_multiples_continue() {
    assert!(matches!(decide_tokens(Some(516)), ContinuationDecision::Continue { grid_multiple: 1 }));
    assert!(matches!(decide_tokens(Some(1034)), ContinuationDecision::Continue { grid_multiple: 2 }));
    assert!(matches!(decide_tokens(Some(1552)), ContinuationDecision::Stop(_)));
}

#[test]
fn tool_call_or_missing_encrypted_reasoning_skips_continuation() {
    assert_eq!(decide(fixture_with_tool_call()), stop("tool_call_present"));
    assert_eq!(decide(fixture_without_encrypted_reasoning()), stop("encrypted_reasoning_missing"));
}
```

- [ ] **Step 2: Run tests and verify core is missing**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_reasoning::tests -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Port the algorithm without the estimation fallback**

Use CodexElves `continue_thinking.rs` as the behavioral reference. Accept reasoning only from `/usage/output_tokens_details/reasoning_tokens`; never estimate from text or byte counts. Recognize function, custom, MCP, web, file, computer, shell, code interpreter and any `*_tool_call` output item as a tool call.

- [ ] **Step 4: Build continuation input from the effective first request**

Clone the post-prompt-rewrite, pre-send Responses request. Append only previous `reasoning` output items with non-empty `encrypted_content`; omit previous answer messages. Append one `continue_thinking` function call/output and the tool declaration if absent. Round call IDs are `call_continue_thinking_<round>`.

- [ ] **Step 5: Add SSE terminal parser and usage accumulator**

Parse the last `response.completed`, `response.incomplete` or `response.failed`. `RoundUsageAccumulator` sums input/output/cache/reasoning and per-round cost; client-visible `first_token_ms` is measured when the selected final successful SSE is ready to begin returning. It must use checked/saturating integer conversion and return an explicit overflow diagnostic.

- [ ] **Step 6: Run core tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_reasoning -- --nocapture`
Expected: PASS for exact grid, eligibility, encrypted item filtering, round IDs, maximum rounds and multi-round usage sums.

- [ ] **Step 7: Commit the continuation core**

```powershell
git add src-tauri/src/services/codex_reasoning
git commit -m "feat(codex): add GPT reasoning continuation core"
```

## Task 14: 将续接接入代理并保持单行记账与固定 Provider

**Files:**

- Modify: `src-tauri/src/proxy/forwarder.rs:346-1110,1114-2315`
- Modify: `src-tauri/src/proxy/response_processor.rs:146-321,475-671`
- Modify: `src-tauri/src/proxy/usage/parser.rs`
- Modify: `src-tauri/src/proxy/usage/logger.rs`
- Modify: `src-tauri/src/services/codex_reasoning/mod.rs`
- Modify: `src-tauri/src/services/codex_reasoning/stream.rs`
- Modify: `src-tauri/src/services/codex_reasoning/usage.rs`
- Modify: `src-tauri/src/services/usage_stats.rs`
- Test: forwarder/response_processor/logger inline tests
- Test: `src-tauri/src/services/codex_reasoning/tests.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone)]
pub struct LogicalCodexRequestResult {
    pub client_sse: Bytes,
    pub pinned_provider_id: String,
    pub aggregate_usage: TokenUsage,
    pub aggregate_cost: Option<CostBreakdown>,
    pub reasoning: CodexReasoningUsage,
    pub rounds: Vec<ContinuationRoundRecord>,
    pub first_token_ms: Option<u64>,
    pub duration_ms: u64,
}

pub trait PinnedResponsesSender: Send + Sync {
    fn send_round<'a>(
        &'a self,
        provider: &'a Provider,
        body: Value,
        round_index: u8,
    ) -> futures_util::future::BoxFuture<'a, Result<ContinuationRoundResult, AppError>>;
}
```

- [ ] **Step 1: Write integration tests for pinning, single log row and partial failure**

```rust
#[tokio::test]
async fn continuation_pins_first_successful_provider_and_logs_once() -> Result<(), AppError> {
    let fixture = ProxyFixture::first_provider_fails_then_second_returns(vec![516, 1034, 1552]);
    let response = fixture.request().await?;
    assert_eq!(response.upstream_provider_ids(), vec!["provider-b", "provider-b", "provider-b"]);
    assert_eq!(fixture.main_log_count()?, 1);
    assert_eq!(fixture.round_log_count()?, 3);
    assert_eq!(fixture.main_log()?.continuation_rounds, 2);
    Ok(())
}

#[tokio::test]
async fn failed_second_round_returns_first_success_and_marks_partial_failed() -> Result<(), AppError> {
    let fixture = ProxyFixture::rounds(vec![ok_round(516), failed_round("timeout")]);
    let response = fixture.request().await?;
    assert!(response.body_contains_first_round());
    assert_eq!(response.status(), 200);
    assert_eq!(fixture.main_log()?.continuation_status, "partial_failed");
    assert_eq!(fixture.main_log()?.continuation_rounds, 0);
    Ok(())
}
```

- [ ] **Step 2: Run tests and verify current proxy cannot continue**

Run: `cargo test --manifest-path src-tauri/Cargo.toml continuation_pins_first_successful_provider -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Buffer only eligible native Responses streams**

Leave all non-Codex, non-GPT, converted Chat/Anthropic, disabled and tool-call paths on the current streaming implementation. For eligible native Responses, capture a bounded round stream, parse the terminal event, then either finalize or issue the next round. Reuse existing decoded-body size guards; reject a round exceeding the bound with `continuation_response_too_large`.

- [ ] **Step 4: Separate initial failover from pinned continuation sends**

Initial request remains in `forward_with_retry`. After the first success is validated, store its Provider snapshot in `PinnedResponsesSender`; further sends call that Provider directly and never invoke `FailoverSwitchManager`, update current Provider, or record other Provider health as a substitute.

- [ ] **Step 5: Return the last successful stream and write one main log row**

Do not concatenate intermediate answers. Keep each completed SSE in memory until its continuation decision is known; when the process stops, return only the last complete successful SSE. Build `LogicalCodexRequestResult`, call `UsageLogger` once, then insert `codex_reasoning_rounds` in the same DB transaction. Set:

```text
reasoning_tokens = sum(authoritative reasoning per successful round)
continuation_rounds = number of extra rounds completed successfully
continuation_status = continued | not_triggered | skipped | partial_failed
first_token_ms = time until the selected final SSE can begin returning to the client
duration_ms = wall time across all rounds
```

Cost is the sum of per-round costs. Do not add reasoning to `output_tokens` again.

- [ ] **Step 6: Preserve explicit partial-failure diagnostics**

A later-round failure stops further attempts, returns the previous complete successful SSE, writes the failed-attempt error code and `partial_failed`, and logs a warning without body/credential/prompt. The client receives HTTP 200 because a complete valid prior result exists; the detail panel exposes the failure status.

- [ ] **Step 7: Run proxy and log tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_reasoning -- --nocapture`
Expected: PASS.

Run: `cargo test --manifest-path src-tauri/Cargo.toml proxy::forwarder -- --nocapture`
Expected: PASS, including existing failover regressions.

Run: `cargo test --manifest-path src-tauri/Cargo.toml proxy::response_processor -- --nocapture`
Expected: PASS.

- [ ] **Step 8: Commit the proxy integration**

```powershell
git add src-tauri/src/proxy src-tauri/src/services/codex_reasoning src-tauri/src/services/usage_stats.rs
git commit -m "feat(codex): integrate pinned multi-round reasoning continuation"
```

## Task 15: 用 Codex Session 唯一匹配补全推理 Token

**Files:**

- Modify: `src-tauri/src/services/session_usage_codex.rs:24-75,300-705`
- Modify: `src-tauri/src/services/usage_stats.rs`
- Modify: `src-tauri/src/database/schema.rs` indexes section
- Modify: `src-tauri/src/proxy/usage/parser.rs`
- Test: inline tests in `session_usage_codex.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, Default)]
struct CodexTurnContext {
    thread_id: Option<String>,
    turn_id: Option<String>,
    model: String,
}

#[derive(Debug, Clone)]
struct CodexSessionUsage {
    input_tokens: u32,
    cached_input_tokens: u32,
    output_tokens: u32,
    reasoning_output_tokens: Option<u32>,
    total_tokens: Option<u64>,
}

enum EnrichmentOutcome {
    Enriched { request_id: String },
    InsertedSessionOnly { request_id: String },
    SkippedAmbiguous,
}
```

- [ ] **Step 1: Write exact-turn, unique-fallback and ambiguity tests**

```rust
#[test]
fn turn_id_exact_match_enriches_proxy_row_without_session_duplicate() -> Result<(), AppError> {
    let fixture = SessionFixture::proxy_row("turn-1").jsonl_turn("turn-1", 500);
    fixture.sync()?;
    let row = fixture.proxy_row_by_turn("turn-1")?;
    assert_eq!(row.reasoning_tokens, Some(500));
    assert!(row.session_enriched);
    assert_eq!(fixture.codex_session_row_count()?, 0);
    Ok(())
}

#[test]
fn ambiguous_fallback_does_not_mutate_proxy_rows() -> Result<(), AppError> {
    let fixture = SessionFixture::two_matching_proxy_rows().jsonl_without_turn_id(500);
    fixture.sync()?;
    assert_eq!(fixture.enriched_proxy_count()?, 0);
    assert_eq!(fixture.codex_session_row_count()?, 1);
    Ok(())
}
```

- [ ] **Step 2: Run tests and verify current sync inserts session-only deltas**

Run: `cargo test --manifest-path src-tauri/Cargo.toml turn_id_exact_match_enriches_proxy_row -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Parse turn and authoritative reasoning fields**

On `turn_context`, read `payload.turn_id`/`turnId` and model. For token_count, prefer `last_token_usage` for the turn-level authoritative values when present; read `reasoning_output_tokens` as `Option<u32>`. `total_token_usage.total_tokens` is context usage only and must not populate reasoning.

- [ ] **Step 4: Match and enrich in one transaction**

Match order:

```text
exact turn_id → exactly one proxy row
else same session_id + normalized model + input/output/cache tuple + ±10 min → exactly one proxy row
else insert/retain _codex_session row
```

Only fill `reasoning_tokens/reasoning_source/turn_id` when target columns are NULL; never overwrite proxy authority. Set `session_enriched=1`. Add indexes for `(turn_id, app_type)` and `(session_id, app_type, created_at)`.

- [ ] **Step 5: Run session and usage regressions**

Run: `cargo test --manifest-path src-tauri/Cargo.toml session_usage_codex -- --nocapture`
Expected: PASS for parent/subagent replay, incremental sync, exact enrichment, unique fallback and ambiguity.

Run: `cargo test --manifest-path src-tauri/Cargo.toml usage_stats -- --nocapture`
Expected: PASS; enriched proxy rows are not double-counted.

- [ ] **Step 6: Commit the session enrichment**

```powershell
git add src-tauri/src/services/session_usage_codex.rs src-tauri/src/services/usage_stats.rs src-tauri/src/database/schema.rs src-tauri/src/proxy/usage/parser.rs
git commit -m "feat(usage): enrich Codex proxy logs from unique session turns"
```

## Task 16: 完成推理日志 UI、第三方说明与全链路硬化

**Files:**

- Modify: `src/components/usage/RequestLogTable.tsx`
- Modify: `src/components/usage/RequestDetailPanel.tsx`
- Modify: `tests/components/RequestLogTable.test.tsx`
- Modify: `tests/components/UsageDashboard.test.tsx`
- Modify: `src/components/codex-workbench/OverviewTab.tsx`
- Modify: `src/i18n/locales/zh.json`, `en.json`, `ja.json`
- Create: `THIRD_PARTY_NOTICES.md`
- Modify: ported Rust/JS file headers under `src-tauri/src/services/codex_*` and `src-tauri/resources/codex-workbench/`

**Interfaces:**

No new storage interface. This task closes display, attribution and regression gaps only.

- [ ] **Step 1: Add final UI tests for source, status and no-body contract**

```tsx
it("shows continuation metadata without reasoning body", async () => {
  render(<RequestDetailPanel request={continuedLog} />);
  expect(screen.getByText("proxy_response")).toBeInTheDocument();
  expect(screen.getByText("2 个续接轮次")).toBeInTheDocument();
  expect(screen.queryByText(/encrypted_content/)).not.toBeInTheDocument();
  expect(screen.queryByText(/reasoning body/i)).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run tests and verify any remaining UI gaps**

Run: `pnpm test:unit -- tests/components/RequestLogTable.test.tsx tests/components/UsageDashboard.test.tsx`
Expected: FAIL only for missing final labels/source/status details; if it already passes, add the assertions without weakening them.

- [ ] **Step 3: Complete table/detail/workbench status rendering**

The table uses `Tok N`, `✨rounds`, `⚠` exactly as specified. Details show reasoning source, continuation status/round count, session enrichment, turn ID, prompt-replaced/identity-corrected booleans and fingerprint. Workbench overview shows the current Provider's prompt/continuation state and a link to edit; it never retrieves prompt text for the status card.

- [ ] **Step 4: Add MIT attribution and provenance**

`THIRD_PARTY_NOTICES.md` must name `junxin367/CodexElves`, MIT, commit `bf1224e`, and list the algorithms/assets adapted. Each substantially ported file gets a short header identifying the source and that it was modified for CC Switch. Do not claim CodexElves endorsement.

- [ ] **Step 5: Run secret/body scans**

Run: `rg -n "OPENAI_API_KEY|encrypted_content|replacement" src-tauri/src/services/codex_* src-tauri/resources/codex-workbench src/components/codex-workbench src/components/usage`
Expected: matches only in request transformation, eligibility checks, source comments/tests and local in-memory fields; no log formatting, serialized diagnostics or React status DTO contains secret/prompt/reasoning body values.

Run: `rg -n "log::(trace|debug|info|warn|error)!.*(body|prompt|api.?key|encrypted)" src-tauri/src`
Expected: no newly introduced unsafe logging.

- [ ] **Step 6: Run all automated verification**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
Expected: PASS.

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: PASS.

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS.

Run: `pnpm format:check`
Expected: PASS.

Run: `pnpm typecheck`
Expected: PASS.

Run: `pnpm test:unit`
Expected: PASS.

Run: `pnpm build:renderer`
Expected: PASS.

Run: `git diff --check`
Expected: no whitespace errors.

- [ ] **Step 7: Run the Windows end-to-end acceptance matrix**

```text
1. Mutate Live Key → focus/edit/switch/takeover → DB key unchanged, conflict visible.
2. Save two stale edit copies → second rejected by revision conflict.
3. Preview cloud restore → local existing credentials selected by default.
4. Start ordinary Codex → enhanced launch refuses without killing.
5. Close Codex and enhanced launch → CDP/bridge running, navigation reinjects once.
6. Toggle every approved page feature; one selector failure does not stop peers.
7. Install/update script with valid hash, then invalid hash → old version retained.
8. Initialize plugin marketplace in temporary CODEX_HOME → unrelated TOML retained.
9. Radar online/fresh and offline/stale states both render correctly.
10. Prompt replacement request → only system layer changes; log has fingerprint, no text.
11. 516 → 1034 → 1552 fixture → two extra rounds, one Provider, one main log row.
12. Tool call/missing encrypted/non-GPT/Chat fixture → no continuation, explicit skip status.
13. Second-round timeout → prior output returned, partial_failed logged, no failover.
14. Unique JSONL turn → proxy row enriched; ambiguous turn → session-only row retained.
15. Usage UI distinguishes — / Tok 0 / Tok N / ✨rounds / ⚠ and total cost is unchanged by reasoning split.
```

Expected: every item passes with no raw Key, prompt, encrypted reasoning or reasoning body in application logs/SQLite diagnostics.

- [ ] **Step 8: Commit the final hardening slice**

```powershell
git add src tests src-tauri THIRD_PARTY_NOTICES.md
git commit -m "feat(codex): complete Codex workbench reasoning integration"
```

## Phase 3 Gate and Completion

- [ ] Confirm all Phase 1–3 gates and the 15-item Windows acceptance matrix pass.
- [ ] Confirm `git status --short` contains only intended integration files and does not include the pre-existing `.superpowers/` directory.
- [ ] Confirm no database, settings or Codex App files from the developer's real profile were used in automated tests; all mutation tests use temp directories/databases.
- [ ] Record the final test command outputs and any deliberately unsupported platform behavior in the implementation handoff.
- [ ] Stop only after the complete three-phase outcome is working; do not call a partially completed phase “finished integration.”
