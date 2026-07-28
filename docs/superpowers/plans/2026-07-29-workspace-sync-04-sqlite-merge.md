# Workspace Sync SQLite Provider Merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Safely merge and write back Codex and OpenCode SQLite-backed sessions, goals, memories, todos, and related metadata without broken references or partial updates.

**Architecture:** Never merge database files. Snapshot live databases with SQLite Backup, export recognized rows into typed logical records, merge by stable primary keys, rewrite IDs for forks, validate a temporary result, then apply the same changes transactionally to the native database with before-images and rollback.

**Tech Stack:** Rust, rusqlite backup/hooks/transactions, SQLite PRAGMA integrity checks, existing Codex/OpenCode path discovery.

---

## File Map

- Create `src-tauri/src/workspace_sync/sqlite/{mod,snapshot,schema,records,apply,preflight}.rs`.
- Create `src-tauri/src/workspace_sync/adapters/codex/{mod,schema,records,merge,apply}.rs`; replace the single backup-only file with a module.
- Create `src-tauri/src/workspace_sync/adapters/opencode/{mod,schema,records,merge,apply}.rs`.
- Create `src-tauri/src/workspace_sync/process.rs`.
- Extend `src-tauri/src/workspace_sync/state_db.rs` with before-image metadata.
- Add integration fixtures for current and unknown schemas.

### Task 1: Add consistent SQLite snapshots and schema fingerprints

**Files:**
- Create: `src-tauri/src/workspace_sync/sqlite/mod.rs`
- Create: `src-tauri/src/workspace_sync/sqlite/snapshot.rs`
- Create: `src-tauri/src/workspace_sync/sqlite/schema.rs`

- [ ] **Step 1: Write failing WAL and fingerprint tests**

```rust
#[test]
fn backup_includes_committed_wal_rows() {
    let source = wal_database_with_committed_row();
    let snapshot = snapshot_database(source.path()).unwrap();
    let count: i64 = snapshot.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn fingerprint_is_stable_across_row_changes() {
    let a = database_with_schema_and_value("a");
    let b = database_with_schema_and_value("b");
    assert_eq!(schema_fingerprint(&a).unwrap(), schema_fingerprint(&b).unwrap());
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite:: -- --nocapture
```

Expected: FAIL because SQLite helpers do not exist.

- [ ] **Step 3: Implement snapshot and fingerprint helpers**

Use `rusqlite::backup::Backup` into a tempfile. Fingerprint sorted `sqlite_master` table/index SQL plus sorted `PRAGMA table_info` output; exclude row content, temp tables, WAL state, and SQLite sequence values. Return both a hex fingerprint and a structured `SchemaDescription`.

- [ ] **Step 4: Run helper tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite::snapshot::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite::schema::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/sqlite
git commit -m "feat(sync): snapshot and fingerprint provider databases"
```

### Task 2: Add typed logical record graphs and ID remapping

**Files:**
- Create: `src-tauri/src/workspace_sync/sqlite/records.rs`

- [ ] **Step 1: Write a failing reference-remap test**

```rust
#[test]
fn remap_updates_parent_and_child_foreign_keys() {
    let graph = RecordGraph::fixture_session("ses_1");
    let remapped = graph.remap_id(EntityKind::Session, "ses_1", "ses_fork").unwrap();
    assert_eq!(remapped.session("ses_fork").unwrap().id, "ses_fork");
    assert!(remapped.messages().all(|m| m.session_id == "ses_fork"));
    assert!(remapped.todos().all(|t| t.session_id == "ses_fork"));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite::records::tests::remap_updates_parent_and_child_foreign_keys -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement graph types**

```rust
pub struct LogicalRecord {
    pub entity: EntityKind,
    pub primary_key: RecordKey,
    pub fields: BTreeMap<String, serde_json::Value>,
    pub foreign_keys: Vec<ForeignKeyRef>,
    pub content_hash: String,
}

pub struct RecordGraph {
    records: BTreeMap<RecordKey, LogicalRecord>,
    children: BTreeMap<RecordKey, BTreeSet<RecordKey>>,
}
```

Require adapters to declare primary-key columns, foreign-key columns, insertion order, and deletion order. `remap_id` must fail if an undeclared textual reference to the old ID is detected in a field marked `contains_ids=true`.

- [ ] **Step 4: Run graph tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite::records::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/sqlite/records.rs
git commit -m "feat(sync): add logical SQLite record graphs"
```

### Task 3: Add process/preflight checks

**Files:**
- Create: `src-tauri/src/workspace_sync/process.rs`
- Create: `src-tauri/src/workspace_sync/sqlite/preflight.rs`

- [ ] **Step 1: Write failing decision tests**

```rust
#[test]
fn running_client_allows_backup_but_blocks_native_write() {
    let state = ClientProcessState::Running { pids: vec![123] };
    assert!(Preflight::for_backup(state.clone()).is_ok());
    assert!(matches!(Preflight::for_write(state), Err(PreflightError::ClientRunning { .. })));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite::preflight::tests::running_client_allows_backup_but_blocks_native_write -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement platform process discovery and preflight**

Detect known executable/process names for Codex, OpenCode, and Cursor without shell interpolation. Preflight checks disk capacity, writable parent directory, recognized schema, unresolved previous transaction, and client process state. Return structured errors containing provider and PID count, never command lines or environment variables.

- [ ] **Step 4: Run preflight tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite::preflight::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::process::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/process.rs src-tauri/src/workspace_sync/sqlite/preflight.rs
git commit -m "feat(sync): guard native database writes"
```

### Task 4: Implement the generic validated SQLite apply transaction

**Files:**
- Create: `src-tauri/src/workspace_sync/sqlite/apply.rs`
- Modify: `src-tauri/src/workspace_sync/state_db.rs`

- [ ] **Step 1: Write a failing rollback test**

```rust
#[test]
fn constraint_failure_restores_all_before_images() {
    let fixture = SqlApplyFixture::new();
    let result = fixture.applier.apply(fixture.plan_with_invalid_child());
    assert!(result.is_err());
    assert_eq!(fixture.read_session_title("ses_1"), "before");
    assert_eq!(fixture.message_count("ses_1"), 1);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite::apply::tests::constraint_failure_restores_all_before_images -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement validate-then-apply**

Apply the plan first to a temporary Backup snapshot. Run:

```sql
PRAGMA integrity_check;
PRAGMA foreign_key_check;
```

Require `integrity_check = ok` and zero foreign-key rows. Before native write, persist encrypted before-images in the restore-point directory and transaction metadata in `workspace-sync.db`. Apply native changes in one `IMMEDIATE` transaction; rollback on any error; verify again after commit.

- [ ] **Step 4: Run apply tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite::apply::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/sqlite/apply.rs src-tauri/src/workspace_sync/state_db.rs
git commit -m "feat(sync): apply validated SQLite merge transactions"
```

### Task 5: Convert Codex backup adapter into schema modules

**Files:**
- Replace: `src-tauri/src/workspace_sync/adapters/codex.rs`
- Create: `src-tauri/src/workspace_sync/adapters/codex/mod.rs`
- Create: `src-tauri/src/workspace_sync/adapters/codex/schema.rs`
- Create: `src-tauri/src/workspace_sync/adapters/codex/records.rs`

- [ ] **Step 1: Write failing recognized/unknown schema tests**

```rust
#[test]
fn current_codex_schema_is_read_write_and_unknown_is_read_only() {
    let known = CodexSchemaRegistry::detect(&fixture_db("codex/current/state_5.sqlite")).unwrap();
    assert!(known.capabilities.can_write);
    let unknown = CodexSchemaRegistry::detect(&fixture_db("codex/unknown/state.sqlite")).unwrap();
    assert!(!unknown.capabilities.can_write);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::codex::schema::tests:: -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement declared Codex mappings**

Declare current recognized tables/columns for threads, dynamic tools, spawn edges, goals, and memory outputs. Export fields into `RecordGraph`; mark rollout JSONL as external payload referenced by thread ID. Unknown columns are preserved only when round-trippable JSON/primitive values; unknown required tables disable write capability.

- [ ] **Step 4: Run Codex schema/export tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::codex::schema::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::codex::records::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/codex src-tauri/src/workspace_sync/adapters/mod.rs
git rm src-tauri/src/workspace_sync/adapters/codex.rs
git commit -m "refactor(sync): version Codex database schemas"
```

### Task 6: Implement Codex merge and write-back

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/codex/merge.rs`
- Create: `src-tauri/src/workspace_sync/adapters/codex/apply.rs`

- [ ] **Step 1: Write a failing thread-fork test**

```rust
#[test]
fn codex_thread_fork_rewrites_rollout_edges_goals_and_memories() {
    let merged = merge_fixture("codex/fork").unwrap();
    let fork = merged.thread_named("remote fork").unwrap();
    assert_ne!(fork.id, "thread-1");
    assert!(merged.dynamic_tools_for(&fork.id).count() > 0);
    assert!(merged.goals_for(&fork.id).count() > 0);
    assert!(merged.rollout_path_for(&fork.id).unwrap().exists());
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::codex::merge::tests::codex_thread_fork_rewrites_rollout_edges_goals_and_memories -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement Codex-specific merge rules**

Use rollout prefix/fork classification. For a fork, generate a new UUID thread ID, a collision-free rollout path, and remap dynamic tools, spawn edges, goals, and memory rows. Do not auto-resolve different goal objective/status changes; emit a Conflict. Preserve active and complete goal branches separately.

- [ ] **Step 4: Apply to fixture DB and verify**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::codex::merge::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::codex::apply::tests::
```

Expected: PASS with `integrity_check=ok`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/codex
git commit -m "feat(sync): merge Codex threads goals and memories"
```

### Task 7: Convert OpenCode backup adapter into schema modules

**Files:**
- Replace: `src-tauri/src/workspace_sync/adapters/opencode.rs`
- Create: `src-tauri/src/workspace_sync/adapters/opencode/{mod,schema,records}.rs`

- [ ] **Step 1: Write failing old/current schema tests**

```rust
#[test]
fn registry_recognizes_message_part_and_session_message_layouts() {
    assert_eq!(detect_fixture("opencode/message-part").layout, OpenCodeLayout::MessagePart);
    assert_eq!(detect_fixture("opencode/session-message").layout, OpenCodeLayout::SessionMessage);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::opencode::schema::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement both declared layouts**

Map project, project_directory, workspace, session, message/session_message, part, session_input, todo, and context epoch tables. Export in deterministic primary-key order. Legacy JSON storage remains readable but writable only through the existing validated file adapter path.

- [ ] **Step 4: Run schema tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::opencode::schema::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::opencode::records::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/opencode src-tauri/src/workspace_sync/adapters/mod.rs
git rm src-tauri/src/workspace_sync/adapters/opencode.rs
git commit -m "refactor(sync): version OpenCode database schemas"
```

### Task 8: Implement OpenCode merge and write-back

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/opencode/merge.rs`
- Create: `src-tauri/src/workspace_sync/adapters/opencode/apply.rs`

- [ ] **Step 1: Write a failing session-fork test**

```rust
#[test]
fn opencode_fork_rewrites_all_session_children() {
    let merged = merge_fixture("opencode/fork").unwrap();
    let fork = merged.remote_fork().unwrap();
    assert!(merged.messages().filter(|m| m.session_id == fork.id).count() > 0);
    assert!(merged.todos().filter(|t| t.session_id == fork.id).count() > 0);
    assert!(merged.contexts().all(|c| c.session_id != "missing"));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::opencode::merge::tests::opencode_fork_rewrites_all_session_children -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement OpenCode-specific merge rules**

Merge projects/workspaces by stable IDs. Merge messages by primary key and content hash. A same-ID/different-content message forks the entire session, generates a new `ses_` ID, and remaps messages, parts, inputs, todos, context, and session-diff records. Insert in declared parent-before-child order and delete in reverse.

- [ ] **Step 4: Run OpenCode apply tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::opencode::merge::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::opencode::apply::tests::
```

Expected: PASS with no foreign-key violations.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/opencode
git commit -m "feat(sync): merge OpenCode sessions messages and todos"
```

### Task 9: Add crash-recovery integration tests

**Files:**
- Create: `src-tauri/src/workspace_sync/integration/sqlite_recovery.rs`

- [ ] **Step 1: Add failure injection points**

Define test-only injection after temporary validation, after before-image persistence, before commit, and after native commit/before verification.

- [ ] **Step 2: Write restart recovery tests**

```rust
#[test]
fn restart_rolls_back_transaction_interrupted_after_native_commit() {
    let harness = RecoveryHarness::codex();
    harness.run_until(FailurePoint::AfterNativeCommit).unwrap_err();
    harness.restart().recover().unwrap();
    assert_eq!(harness.native_state(), harness.original_state());
    assert_eq!(harness.transaction_state(), "rolled_back");
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::integration::sqlite_recovery:: -- --nocapture
```

Expected: PASS for every failure point and both providers.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/workspace_sync/integration/sqlite_recovery.rs src-tauri/src/workspace_sync/sqlite
git commit -m "test(sync): verify SQLite crash recovery"
```

### Task 10: Run the Plan 4 verification gate

- [ ] **Step 1: Run provider suites**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::codex::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::opencode::
```

Expected: PASS.

- [ ] **Step 2: Run integrity and recovery suites**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::sqlite::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::integration::sqlite_recovery::
```

Expected: PASS.

- [ ] **Step 3: Run all checks**

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm typecheck
pnpm test:unit
```

Expected: PASS.
