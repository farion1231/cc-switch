# Codex Provider Switch Consistency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Codex official/third-party switching credential-safe and make persisted threads follow the selected provider when unified history is enabled.

**Architecture:** Keep native `openai` and third-party `custom` route identities. Add exhaustive state-DB discovery, field-level history reconciliation with a restoration ledger, safe dual-route live snapshots, and a coordinated Codex switch commit/rollback path.

**Tech Stack:** Rust, rusqlite, serde/serde_json, toml_edit, Tauri, TypeScript/React, Vitest.

---

### Task 1: Discover every Codex state database

**Files:**
- Modify: `src-tauri/src/codex_state_db.rs`
- Test: `src-tauri/src/codex_state_db.rs`

- [ ] **Step 1: Write failing discovery tests**

Add tests that create `state_4.sqlite`, `state_5.sqlite`, and `sqlite/state_6.sqlite`, plus configured and environment SQLite homes, then assert canonical deduplicated paths:

```rust
#[test]
fn discovers_versioned_root_and_nested_state_databases() {
    let dir = tempdir().unwrap();
    touch(&dir.path().join("state_4.sqlite"));
    touch(&dir.path().join("state_5.sqlite"));
    touch(&dir.path().join("sqlite/state_6.sqlite"));

    let paths = codex_state_db_paths(dir.path(), "");

    assert_eq!(paths.len(), 3);
    assert!(paths.iter().any(|p| p.ends_with("state_4.sqlite")));
    assert!(paths.iter().any(|p| p.ends_with("state_5.sqlite")));
    assert!(paths.iter().any(|p| p.ends_with("state_6.sqlite")));
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test codex_state_db::tests -- --nocapture`

Expected: the nested/versioned discovery assertion fails because only the fixed root `state_5.sqlite` is returned.

- [ ] **Step 3: Implement glob-free directory discovery**

Replace the fixed filename assumption with a helper that reads each candidate directory and accepts only regular files matching `state_*.sqlite`:

```rust
fn collect_state_databases(dir: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if path.is_file() && name.starts_with("state_") && name.ends_with(".sqlite") {
            push_unique_path(paths, path);
        }
    }
}
```

- [ ] **Step 4: Verify GREEN and commit**

Run: `cargo test codex_state_db::tests -- --nocapture`

Expected: all discovery tests pass.

Commit: `fix(codex): discover all state databases`

### Task 2: Build credential-safe live snapshots

**Files:**
- Modify: `src-tauri/src/codex_config.rs`
- Test: `src-tauri/src/codex_config.rs`
- Modify: `src-tauri/tests/provider_service.rs`

- [ ] **Step 1: Write failing auth and route tests**

Specify these behaviors:

```rust
#[test]
fn third_party_live_scopes_key_without_replacing_official_auth() {
    let output = prepare_codex_provider_live_config(
        &json!({"OPENAI_API_KEY": "third-party-key"}),
        CUSTOM_CONFIG,
    ).unwrap();
    assert!(output.contains("experimental_bearer_token = \"third-party-key\""));
}

#[test]
fn preservation_replaces_incomplete_custom_placeholder() {
    let output = preserve_inactive_custom_provider(OFFICIAL_WITH_PLACEHOLDER, LIVE_CUSTOM).unwrap();
    assert!(output.contains("base_url = \"https://third.example/v1\""));
    assert!(!output.contains("name = \"OpenAI\""));
}
```

Update the integration expectation so a third-party switch retains official OAuth and does not place the third-party key in `auth.json`.

- [ ] **Step 2: Verify RED**

Run: `cargo test codex_config::tests::preservation_ -- --nocapture`

Run: `cargo test --test provider_service provider_service_switch_codex_default_overwrites_official_auth_when_preservation_off -- --exact --nocapture`

Expected: preservation is missing and the integration test observes the old global-key overwrite.

- [ ] **Step 3: Implement the minimal safe snapshot rules**

Add a shape validator and preservation helper:

```rust
fn custom_provider_is_routable(item: &toml_edit::Item) -> bool {
    item.as_table_like()
        .and_then(|table| table.get("base_url"))
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.trim().is_empty())
}
```

For non-official providers, always write config-only with a scoped bearer token. For official providers, write native auth and preserve the last routable inactive `custom` table. Remove official-as-custom injection from the live write path.

- [ ] **Step 4: Verify GREEN and commit**

Run: `cargo test codex_config::tests -- --nocapture`

Run: `cargo test --test provider_service codex -- --nocapture --test-threads=1`

Expected: all Codex config and provider-service tests pass.

Commit: `fix(codex): isolate provider credentials on switch`

### Task 3: Reconcile JSONL and SQLite provider/model metadata

**Files:**
- Create: `src-tauri/src/codex_history_reconcile.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/codex_history_reconcile.rs`

- [ ] **Step 1: Write failing line-rewrite tests**

Define the desired pure API in tests:

```rust
let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
let (line, original) = rewrite_history_line(THREAD_SETTINGS_LINE, &target).unwrap();
assert_eq!(line["payload"]["thread_settings"]["model_provider_id"], "custom");
assert_eq!(line["payload"]["thread_settings"]["model"], "gpt-5.6-sol");
assert_eq!(original.provider, "openai");
```

Cover `session_meta`, `thread_settings_applied`, unknown events, missing model targets, already-correct lines, and legacy provider IDs.

- [ ] **Step 2: Verify RED**

Run: `cargo test codex_history_reconcile::tests::rewrite_ -- --nocapture`

Expected: compilation fails because the new module/API does not exist.

- [ ] **Step 3: Implement pure recognized-field rewriting**

Create focused types:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryTarget {
    pub provider: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct OriginalRoute {
    provider: String,
    model: Option<String>,
}
```

Parse once with `serde_json`, modify only the two recognized event shapes, and return `None` for no-op/unknown lines.

- [ ] **Step 4: Write failing artifact tests**

Create temporary active/archived JSONL files and two state databases. Assert:

- both JSONL roots are updated;
- all discovered DB rows are updated;
- original JSONL modified times remain equal;
- messages and unknown events are byte-equivalent;
- a second run is idempotent.

- [ ] **Step 5: Verify RED, implement artifact reconciliation, verify GREEN**

Run: `cargo test codex_history_reconcile::tests::reconcile_ -- --nocapture`

Expected RED: no artifact reconciler exists.

Implement atomic JSONL replacement with timestamp restoration and SQLite transactions using `busy_timeout`.

Run the same command again.

Expected GREEN: all reconciliation tests pass.

Commit: `fix(codex): reconcile persisted thread routes`

### Task 4: Persist original routes and support exact restore

**Files:**
- Modify: `src-tauri/src/codex_history_reconcile.rs`
- Modify: `src-tauri/src/commands/settings.rs`
- Modify: `src-tauri/src/settings.rs`
- Test: `src-tauri/src/codex_history_reconcile.rs`

- [ ] **Step 1: Write failing ledger-cycle tests**

Test `openai -> custom -> openai -> restore` for pre-existing and newly created threads. Assert original entries are captured once and restore returns each provider/model exactly without deleting appended messages.

- [ ] **Step 2: Verify RED**

Run: `cargo test codex_history_reconcile::tests::ledger_ -- --nocapture`

Expected: no persistent ledger/restore operation exists.

- [ ] **Step 3: Implement a versioned field ledger**

Use a single atomic JSON document per canonical Codex directory:

```rust
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteLedger {
    version: u32,
    codex_config_dir: String,
    jsonl: BTreeMap<String, OriginalRoute>,
    threads: BTreeMap<String, OriginalRoute>,
}
```

Keys contain the session ID and recognized event identity. Never replace an existing original entry. Store no credential fields.

- [ ] **Step 4: Wire enable/disable behavior and verify GREEN**

Enabling calls reconcile for the current provider. Optional disable restore calls the ledger restore API; successful restore clears only that directory's ledger.

Run: `cargo test codex_history_reconcile::tests -- --nocapture`

Expected: all cycle, idempotency, and restore tests pass.

Commit: `fix(codex): restore original thread routes from ledger`

### Task 5: Make Codex switching coordinated and rollback-safe

**Files:**
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/services/provider/live.rs`
- Modify: `src-tauri/src/codex_history_reconcile.rs`
- Modify: `src-tauri/tests/provider_service.rs`

- [ ] **Step 1: Write failing bidirectional service tests**

Add integration tests with real temporary live files/SQLite:

```rust
ProviderService::switch(&state, AppType::Codex, "third-party").unwrap();
assert_eq!(state.db.get_current_provider("codex").unwrap().as_deref(), Some("third-party"));
assert_eq!(thread_route(&state_db, THREAD_ID), ("custom", "gpt-5.6-sol"));

ProviderService::switch(&state, AppType::Codex, "official").unwrap();
assert_eq!(thread_route(&state_db, THREAD_ID).0, "openai");
```

Add fault-injection tests showing a live/history failure leaves local settings, DB current provider, live files, JSONL, and SQLite unchanged.

- [ ] **Step 2: Verify RED**

Run: `cargo test --test provider_service codex_unified_switch -- --nocapture --test-threads=1`

Expected: current service updates current-provider stores before history and has no coordinated rollback.

- [ ] **Step 3: Implement the Codex-specific switch transaction**

Keep other apps on the existing path. For Codex, hold one mutex, snapshot current state, write safe live, reconcile history, then publish settings/DB current. On error call rollback in reverse order and combine rollback errors with the primary failure.

- [ ] **Step 4: Verify GREEN and commit**

Run: `cargo test --test provider_service codex -- --nocapture --test-threads=1`

Run: `cargo test services::provider::tests::codex -- --nocapture --test-threads=1`

Expected: bidirectional and rollback tests pass.

Commit: `fix(codex): switch provider state transactionally`

### Task 6: Align the existing toggle and UI copy

**Files:**
- Modify: `src/components/settings/CodexAuthSettings.tsx`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/zh-TW.json`
- Modify: `src/i18n/locales/ja.json`
- Modify: `src/types.ts`
- Test: `tests/components/CodexAuthSettings.test.tsx`

- [ ] **Step 1: Write failing UI behavior test**

Assert the toggle enable payload remains `unifyCodexSessionHistory: true`, the optional existing-history choice remains available, and the description no longer claims official traffic runs under `custom`.

- [ ] **Step 2: Verify RED**

Run: `pnpm test:unit tests/components/CodexAuthSettings.test.tsx`

Expected: old copy/behavior assertion fails.

- [ ] **Step 3: Make minimal UI/copy changes and verify GREEN**

Describe current-provider-follow behavior and the reopen requirement for already-open threads. Do not add a new toggle or unrelated settings.

Run: `pnpm test:unit tests/components/CodexAuthSettings.test.tsx`

Expected: test passes.

Commit: `fix(codex): clarify unified history switching`

### Task 7: Remove obsolete one-way routing code

**Files:**
- Modify: `src-tauri/src/codex_config.rs`
- Modify: `src-tauri/src/codex_history_migration.rs`
- Modify: `src-tauri/src/commands/settings.rs`
- Modify: `src-tauri/src/settings.rs`

- [ ] **Step 1: Remove only superseded official-as-custom helpers and markers**

Delete code that injects official `custom` routing or runs the old one-time official-only migration. Retain unrelated legacy third-party provider-ID migration and its tests.

- [ ] **Step 2: Search for dead references**

Run: `rg -n "inject_codex_unified_session_bucket|codex_official_history_unify_v1" src-tauri src tests`

Expected: no production references remain.

- [ ] **Step 3: Format, compile focused tests, and commit**

Run: `cargo fmt --all -- --check`

Run: `cargo test codex_ -- --nocapture --test-threads=1`

Expected: formatting and Codex tests pass.

Commit: `refactor(codex): remove one-way history routing`

### Task 8: Final verification and Windows package

**Files:**
- No production changes unless a failing check is reproduced with a new regression test first.

- [ ] **Step 1: Run source hygiene checks**

Run: `git diff --check`

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --all-targets -- -D warnings`

- [ ] **Step 2: Run backend and frontend tests**

Run: `cargo test --all-targets -- --test-threads=1`

Run: `pnpm typecheck`

Run: `pnpm test:unit -- --testTimeout=15000`

Run: `pnpm format:check`

Expected: all task-related checks pass. Record the unrelated upstream fixed-port baseline test separately if the host still reserves port 15721.

- [ ] **Step 3: Compile and package Windows release**

Run: `pnpm build`

Expected: Tauri release build succeeds and produces Windows installer artifacts under `src-tauri/target/release/bundle/`.

- [ ] **Step 4: Review the final diff**

Run: `git diff origin/main...HEAD --stat`

Run: `git diff origin/main...HEAD -- src-tauri/src src-tauri/tests src tests`

Confirm no credentials, generated dependency files, debug output, unrelated refactors, or obsolete helpers are included.
