# Workspace Sync Provider Backup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Discover, preview, and create encrypted incremental backups for Claude Code, Codex, Grok Build, OpenCode, and Cursor while excluding credentials and unsupported data.

**Architecture:** Add read-only Provider Adapters that emit normalized inventory items and upload their native payloads through the Plan 1 BlobStore. A backup engine composes all enabled provider snapshots and publishes one immutable remote snapshot.

**Tech Stack:** Rust, rusqlite Backup API, serde_json, SHA-256, existing provider path helpers, Tauri commands, TypeScript API types.

---

## File Map

- Create `src-tauri/src/workspace_sync/adapter.rs`: adapter trait, registry, detection/capability types.
- Create `src-tauri/src/workspace_sync/security.rs`: path whitelist and credential filter.
- Create `src-tauri/src/workspace_sync/adapters/{mod,common,claude,codex,grokbuild,opencode,cursor}.rs`.
- Create `src-tauri/src/workspace_sync/inventory.rs`: combined preview and size totals.
- Create `src-tauri/src/workspace_sync/backup.rs`: encrypted backup orchestration.
- Create `src-tauri/src/commands/workspace_sync.rs`: status, scan, preview, backup, unlock/lock.
- Create `src/lib/api/workspaceSync.ts`: frontend invoke wrapper.
- Modify `src-tauri/src/settings.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src/types.ts`.
- Create fixtures under `src-tauri/tests/fixtures/workspace-sync/`.

### Task 1: Define the adapter contract and registry

**Files:**
- Create: `src-tauri/src/workspace_sync/adapter.rs`
- Create: `src-tauri/src/workspace_sync/adapters/mod.rs`
- Modify: `src-tauri/src/workspace_sync/mod.rs`

- [ ] **Step 1: Write the failing registry test**

```rust
#[test]
fn default_registry_contains_all_target_providers() {
    let ids = AdapterRegistry::default().provider_ids();
    assert_eq!(ids, vec![
        WorkspaceProviderId::Claude,
        WorkspaceProviderId::Codex,
        WorkspaceProviderId::GrokBuild,
        WorkspaceProviderId::OpenCode,
        WorkspaceProviderId::Cursor,
    ]);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapter::tests::default_registry_contains_all_target_providers -- --exact
```

Expected: FAIL because the registry does not exist.

- [ ] **Step 3: Implement the contract**

```rust
pub struct AdapterCapabilities {
    pub can_read: bool,
    pub can_write: bool,
    pub can_merge: bool,
    pub reason: Option<String>,
}

pub struct DetectionResult {
    pub provider: WorkspaceProviderId,
    pub detected: bool,
    pub native_version: Option<String>,
    pub schema_fingerprint: Option<String>,
    pub capabilities: AdapterCapabilities,
}

pub struct InventoryEntry {
    pub provider: WorkspaceProviderId,
    pub kind: DataKind,
    pub logical_id: String,
    pub relative_path: String,
    pub byte_size: u64,
    pub modified_at: Option<i64>,
    pub content_hash: String,
    pub merge_capability: MergeCapability,
    pub sensitivity: Sensitivity,
    pub source: PayloadSource,
}

pub trait WorkspaceDataAdapter: Send + Sync {
    fn provider_id(&self) -> WorkspaceProviderId;
    fn detect(&self) -> Result<DetectionResult, AppError>;
    fn inventory(&self) -> Result<Vec<InventoryEntry>, AppError>;
    fn read_payload(&self, entry: &InventoryEntry) -> Result<Vec<u8>, AppError>;
}
```

`AdapterRegistry::default()` initially registers five `UnavailableAdapter` instances that return `detected=false` and an explicit reason; Tasks 4-7 replace them with real adapters one provider at a time while keeping the registry API stable.

- [ ] **Step 4: Run the registry test**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapter::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync
git commit -m "feat(sync): add provider adapter registry"
```

### Task 2: Add path and secret filtering

**Files:**
- Create: `src-tauri/src/workspace_sync/security.rs`
- Modify: `src-tauri/src/workspace_sync/mod.rs`

- [ ] **Step 1: Write failing security tests**

```rust
#[test]
fn blocks_credentials_and_path_escape() {
    assert_eq!(classify_relative_path(Path::new("auth.json")), PathDecision::Block);
    assert_eq!(classify_relative_path(Path::new("../auth.json")), PathDecision::Block);
    assert_eq!(classify_relative_path(Path::new("sessions/a.jsonl")), PathDecision::Allow);
}

#[test]
fn redacts_structured_secret_fields() {
    let value = serde_json::json!({"title":"ok","access_token":"secret"});
    let filtered = redact_structured_secrets(value);
    assert_eq!(filtered["title"], "ok");
    assert!(filtered.get("access_token").is_none());
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::security::tests::
```

Expected: FAIL because security helpers do not exist.

- [ ] **Step 3: Implement exact decisions**

Block basename matches for `auth.json`, `cookies`, `cookie`, `tokens.json`, lock files, IPC paths, and any normalized path containing `..`. Redact case-insensitive keys matching `api_key`, `access_token`, `refresh_token`, `authorization`, `cookie`, `client_secret`, and `password`. Return `PotentialSecret` for unstructured text matching `Bearer ` or common private-key headers; do not silently alter unstructured session text.

- [ ] **Step 4: Run security tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::security::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/security.rs src-tauri/src/workspace_sync/mod.rs
git commit -m "feat(sync): filter credentials from workspace backups"
```

### Task 3: Add common filesystem inventory helpers

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/common.rs`
- Test: `src-tauri/src/workspace_sync/adapters/common.rs`

- [ ] **Step 1: Write a failing symlink-escape test**

```rust
#[cfg(unix)]
#[test]
fn scanner_does_not_follow_symlink_outside_root() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
    let entries = collect_files(root.path(), &ScanPolicy::default()).unwrap();
    assert!(entries.is_empty());
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::common::tests::scanner_does_not_follow_symlink_outside_root -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement bounded scanning**

Implement canonical-root checks, no symlink traversal, maximum entry count, maximum single-file size, relative normalized paths, extension/name allowlists, and streaming SHA-256. Return `InventoryEntry` with relative path, byte size, mtime, content hash, kind, merge capability, and source descriptor.

- [ ] **Step 4: Run helper tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::common::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/common.rs
git commit -m "feat(sync): add bounded provider file scanning"
```

### Task 4: Implement Claude and Grok Build inventory

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/claude.rs`
- Create: `src-tauri/src/workspace_sync/adapters/grokbuild.rs`
- Create: `src-tauri/tests/fixtures/workspace-sync/claude/basic/`
- Create: `src-tauri/tests/fixtures/workspace-sync/grokbuild/basic/`

- [ ] **Step 1: Write fixture inventory tests**

```rust
#[test]
fn claude_inventory_includes_sessions_and_excludes_auth() {
    let adapter = ClaudeAdapter::for_root(fixture("claude/basic"));
    let paths = adapter.inventory().unwrap().into_iter().map(|e| e.relative_path).collect::<Vec<_>>();
    assert!(paths.iter().any(|p| p.ends_with("session-1.jsonl")));
    assert!(!paths.iter().any(|p| p.ends_with("auth.json")));
}

#[test]
fn grok_inventory_groups_summary_and_history_under_one_session() {
    let adapter = GrokBuildAdapter::for_root(fixture("grokbuild/basic"));
    let entries = adapter.inventory().unwrap();
    assert_eq!(entries.iter().filter(|e| e.logical_id == "grok-session-1").count(), 2);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::
```

Expected: FAIL.

- [ ] **Step 3: Implement both adapters**

Claude uses `crate::config::get_claude_config_dir()` and inventories recognized files under `projects`, `plans`, `tasks`, `todos`, and recognized memory locations. Grok uses `crate::grok_config::get_grok_config_dir()` and groups `summary.json`, `chat_history.jsonl`, and allowed sidecars by `summary.info.id`. Assign `AppendOnly` to session JSONL, `Text` to Markdown, and `Opaque` to validated attachments.

- [ ] **Step 4: Run adapter tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::claude::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::grokbuild::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters src-tauri/tests/fixtures/workspace-sync/claude src-tauri/tests/fixtures/workspace-sync/grokbuild
git commit -m "feat(sync): inventory Claude and Grok work data"
```

### Task 5: Implement Codex read-only export

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/codex.rs`
- Create: `src-tauri/tests/fixtures/workspace-sync/codex/basic/`

- [ ] **Step 1: Write a failing export test**

```rust
#[test]
fn codex_export_contains_sessions_goals_and_memories_but_not_auth() {
    let adapter = CodexAdapter::for_root(fixture("codex/basic"));
    let snapshot = adapter.export_logical_records().unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.goals.len(), 1);
    assert_eq!(snapshot.memories.len(), 1);
    assert!(snapshot.files.iter().all(|f| f.relative_path != "auth.json"));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::codex::tests::codex_export_contains_sessions_goals_and_memories_but_not_auth -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement consistent SQLite export**

Reuse `get_codex_config_dir()` and `codex_state_db_paths`. Use `rusqlite::backup::Backup` to copy each active DB to a temporary database, then export recognized rows into canonical JSONL sorted by primary key. Include rollout files, archived rollout files, session index, recognized thread tables, goals, memories, and `memories/`; exclude auth, logs, temp, plugins, IPC, and computer-use.

- [ ] **Step 4: Run Codex tests and existing Session Manager tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::codex::tests::
cargo test --manifest-path src-tauri/Cargo.toml session_manager::providers::codex::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/codex.rs src-tauri/tests/fixtures/workspace-sync/codex
git commit -m "feat(sync): export Codex sessions goals and memories"
```

### Task 6: Implement OpenCode read-only export

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/opencode.rs`
- Create: `src-tauri/tests/fixtures/workspace-sync/opencode/basic/`

- [ ] **Step 1: Write a failing schema-adaptive export test**

```rust
#[test]
fn opencode_export_preserves_session_message_and_todo_relations() {
    let adapter = OpenCodeAdapter::for_root(fixture("opencode/basic"));
    let exported = adapter.export_logical_records().unwrap();
    assert_eq!(exported.sessions[0].id, "ses_1");
    assert_eq!(exported.messages[0].session_id, "ses_1");
    assert_eq!(exported.todos[0].session_id, "ses_1");
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::opencode::tests::opencode_export_preserves_session_message_and_todo_relations -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement export for current SQLite and legacy JSON layouts**

Use `get_opencode_base_dir()`. Fingerprint table/column sets, export recognized project/workspace/session/message/part/todo/context records in primary-key order, and fall back to legacy `storage/` inventory. Mark unknown schemas `can_read=false` unless all required columns can be safely identified. Keep `snapshot/` disabled by default.

- [ ] **Step 4: Run OpenCode tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::opencode::tests::
cargo test --manifest-path src-tauri/Cargo.toml session_manager::providers::opencode::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/opencode.rs src-tauri/tests/fixtures/workspace-sync/opencode
git commit -m "feat(sync): export OpenCode logical work data"
```

### Task 7: Implement Cursor backup-only inventory

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/cursor.rs`
- Create: `src-tauri/tests/fixtures/workspace-sync/cursor/known-schema/`
- Create: `src-tauri/tests/fixtures/workspace-sync/cursor/unknown-schema/`

- [ ] **Step 1: Write failing selective-blob tests**

```rust
#[test]
fn cursor_inventory_only_includes_blobs_referenced_by_composers() {
    let adapter = CursorAdapter::for_root(fixture("cursor/known-schema"));
    let inventory = adapter.inventory().unwrap();
    assert!(inventory.iter().any(|e| e.logical_id == "composer-1"));
    assert!(inventory.iter().any(|e| e.relative_path.contains("referenced-blob")));
    assert!(!inventory.iter().any(|e| e.relative_path.contains("unreferenced-blob")));
}

#[test]
fn unknown_cursor_schema_is_read_only_or_unsupported_never_writable() {
    let adapter = CursorAdapter::for_root(fixture("cursor/unknown-schema"));
    let detection = adapter.detect().unwrap();
    assert!(!detection.capabilities.can_write);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement backup-only Cursor discovery**

Probe OS-specific Cursor User roots plus `~/.cursor`. Read `composerHeaders` and relevant `cursorDiskKV` key prefixes from a SQLite Backup snapshot. Follow only references reachable from selected Composer IDs. Inventory recognized workspace `state.vscdb` keys and explicit Rules/Memory files. Exclude cookies, machine ID, caches, logs, telemetry, service workers, and unreferenced blobs. Set `can_write=false` in this plan.

- [ ] **Step 4: Run Cursor backup tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/cursor.rs src-tauri/tests/fixtures/workspace-sync/cursor
git commit -m "feat(sync): inventory Cursor composers and referenced blobs"
```

### Task 8: Build combined inventory and backup preview

**Files:**
- Create: `src-tauri/src/workspace_sync/inventory.rs`
- Test: `src-tauri/src/workspace_sync/inventory.rs`

- [ ] **Step 1: Write a failing aggregation test**

```rust
#[test]
fn preview_reports_per_provider_counts_sizes_and_warnings() {
    let preview = InventoryService::new(fake_registry()).scan(&enabled_all()).unwrap();
    assert_eq!(preview.providers[&WorkspaceProviderId::Claude].item_count, 2);
    assert_eq!(preview.total_bytes, 30);
    assert_eq!(preview.warning_count, 1);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::inventory::tests::preview_reports_per_provider_counts_sizes_and_warnings -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement the inventory service**

Scan enabled adapters independently, retain per-provider errors instead of aborting the whole scan, sort entries by provider/kind/logical ID/path, and return counts, plain bytes, potential-secret warnings, schema capabilities, and blocked item count.

- [ ] **Step 4: Run aggregation tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::inventory::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/inventory.rs src-tauri/src/workspace_sync/mod.rs
git commit -m "feat(sync): add cross-provider backup preview"
```

### Task 9: Orchestrate encrypted incremental backup

**Files:**
- Create: `src-tauri/src/workspace_sync/backup.rs`
- Test: `src-tauri/src/workspace_sync/backup.rs`

- [ ] **Step 1: Write a failing five-provider backup test**

```rust
#[tokio::test]
async fn backup_publishes_one_snapshot_and_skips_unchanged_blobs() {
    let harness = BackupHarness::five_providers();
    let first = harness.engine.backup().await.unwrap();
    let uploads_after_first = harness.storage.put_count();
    let second = harness.engine.backup().await.unwrap();
    assert_eq!(first.snapshot_id, second.snapshot_id);
    assert_eq!(harness.storage.put_count(), uploads_after_first);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::backup::tests::backup_publishes_one_snapshot_and_skips_unchanged_blobs -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement `BackupEngine`**

For each enabled adapter: inventory, block forbidden entries, upload payloads through `BlobStore`, create `DataItem`s, build one `SnapshotManifest`, compare its content ID to current Head, and publish only when content changed. Record transaction and per-provider result rows in `WorkspaceSyncDb`.

- [ ] **Step 4: Run backup and repository tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::backup::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::repository::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/backup.rs src-tauri/src/workspace_sync/mod.rs
git commit -m "feat(sync): create encrypted provider backups"
```

### Task 10: Add settings, commands, and frontend API

**Files:**
- Modify: `src-tauri/src/settings.rs`
- Create: `src-tauri/src/commands/workspace_sync.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/types.ts`
- Create: `src/lib/api/workspaceSync.ts`
- Create: `tests/lib/workspaceSyncApi.test.ts`
- Test: `tests/hooks/useSettings.test.tsx`

- [ ] **Step 1: Write failing settings and API contract tests**

Add Rust serde test asserting omitted settings default to disabled with all five providers enabled for scanning. Add Vitest mock asserting `workspaceSyncApi.preview()` invokes `workspace_sync_preview` and `backup()` invokes `workspace_sync_backup`.

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml settings::tests::workspace_sync_defaults_are_backward_compatible -- --exact
pnpm vitest run tests/lib/workspaceSyncApi.test.ts
```

Expected: both FAIL.

- [ ] **Step 3: Implement contracts**

Add `WorkspaceSyncSettings` to `AppSettings` with backend/profile, `credential_ref`, auto-backup fields, and five `ProviderSyncOptions`. Add commands:

```rust
workspace_sync_get_status
workspace_sync_scan
workspace_sync_preview
workspace_sync_backup
workspace_sync_unlock
workspace_sync_lock
```

Register commands in `lib.rs`; frontend types must mirror Rust camelCase names exactly. The password is accepted only by `unlock` and is not returned by status/settings.

- [ ] **Step 4: Run contract and regression tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml settings::tests::workspace_sync_defaults_are_backward_compatible -- --exact
pnpm vitest run tests/lib/workspaceSyncApi.test.ts tests/hooks/useSettings.test.tsx
pnpm typecheck
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/settings.rs src-tauri/src/commands src-tauri/src/lib.rs src/types.ts src/lib/api/workspaceSync.ts tests
git commit -m "feat(sync): expose encrypted workspace backup API"
```

### Task 11: Run the Plan 2 verification gate

- [ ] **Step 1: Run credential fixture scan**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::
```

Expected: PASS; fixtures containing `auth.json` and secret fields never appear in snapshot items.

- [ ] **Step 2: Run backend checks**

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 3: Run frontend checks**

```bash
pnpm typecheck
pnpm format:check
pnpm test:unit
```

Expected: PASS.

- [ ] **Step 4: Confirm backward compatibility**

```bash
cargo test --manifest-path src-tauri/Cargo.toml services::webdav_sync
cargo test --manifest-path src-tauri/Cargo.toml services::s3_sync
```

Expected: existing config/Skills sync tests pass unchanged.
