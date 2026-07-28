# Cursor Session Manager and Write-Back Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Cursor Agent/Chat/Composer data visible in Session Manager and safely merge/write known database schemas while preserving a read-only fallback for unknown versions.

**Architecture:** Split Cursor support into a versioned schema registry, read-only session projection, referenced-blob graph, and known-schema writer. Never copy the full globalStorage database; query only selected Composer keys and apply row-level before-images.

**Tech Stack:** Rust, rusqlite, Cursor `state.vscdb` fixtures, existing Session Manager types, React provider filters.

---

## File Map

- Replace `src-tauri/src/workspace_sync/adapters/cursor.rs` with `adapters/cursor/{mod,paths,schema_registry,global_storage,workspace_storage,graph,merge,apply,rules}.rs`.
- Create `src-tauri/src/session_manager/providers/cursor.rs`.
- Modify `src-tauri/src/session_manager/{mod.rs,providers/mod.rs}`.
- Modify `src/components/sessions/{SessionManagerPage,SessionItem,utils}.tsx` and `src/types.ts`.
- Add known, legacy, unknown, large-blob, and fork fixtures.

### Task 1: Build the Cursor path resolver and schema registry

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/cursor/paths.rs`
- Create: `src-tauri/src/workspace_sync/adapters/cursor/schema_registry.rs`
- Create: `src-tauri/src/workspace_sync/adapters/cursor/mod.rs`
- Remove: `src-tauri/src/workspace_sync/adapters/cursor.rs`

- [ ] **Step 1: Write failing path and capability tests**

```rust
#[test]
fn resolves_platform_user_storage_without_treating_dot_cursor_as_global_storage() {
    let roots = CursorRoots::for_platform(Platform::MacOS, Path::new("/Users/u"));
    assert_eq!(roots.user_dir, PathBuf::from("/Users/u/Library/Application Support/Cursor/User"));
    assert_eq!(roots.dot_cursor, PathBuf::from("/Users/u/.cursor"));
}

#[test]
fn unknown_schema_never_has_write_capability() {
    let schema = CursorSchemaRegistry::detect(&fixture_db("cursor/unknown-schema/state.vscdb")).unwrap();
    assert!(!schema.capabilities.can_write_sessions);
    assert!(!schema.capabilities.can_rewrite_composer_id);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::paths::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::schema_registry::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement registry entries**

Fingerprint table sets, columns, `PRAGMA user_version`, key prefixes, and optional Cursor version. Define capabilities:

```rust
pub struct CursorSchemaCapabilities {
    pub can_read_sessions: bool,
    pub can_write_sessions: bool,
    pub can_merge_bubbles: bool,
    pub can_rewrite_composer_id: bool,
    pub can_restore_rules: bool,
}
```

Include explicit entries for all committed fixtures and an `Unknown` entry with write flags false.

- [ ] **Step 4: Run registry tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::schema_registry::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/cursor src-tauri/src/workspace_sync/adapters/mod.rs
git rm src-tauri/src/workspace_sync/adapters/cursor.rs
git commit -m "refactor(sync): version Cursor storage schemas"
```

### Task 2: Parse Composer headers and referenced Bubble graphs

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/cursor/global_storage.rs`
- Create: `src-tauri/src/workspace_sync/adapters/cursor/graph.rs`

- [ ] **Step 1: Write failing graph tests**

```rust
#[test]
fn loads_only_bubbles_and_blobs_reachable_from_selected_composer() {
    let db = fixture_db("cursor/known-schema/state.vscdb");
    let graph = CursorGraphReader::open(&db).unwrap().load_composer("composer-1").unwrap();
    assert_eq!(graph.header.composer_id, "composer-1");
    assert_eq!(graph.bubbles.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), vec!["bubble-1", "bubble-2"]);
    assert_eq!(graph.blobs.iter().map(|b| b.key.as_str()).collect::<Vec<_>>(), vec!["agentKv:blob:referenced"]);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::graph::tests::loads_only_bubbles_and_blobs_reachable_from_selected_composer -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement graph reads**

Read `composerHeaders` ordered by `lastUpdatedAt`. Query `cursorDiskKV` by exact key and bounded prefix pages; parse payload references without loading unrelated rows. Enforce maximum Bubble count, Blob count, and single Blob size. Return opaque payload bytes alongside parsed IDs so round-trip does not discard unknown fields.

- [ ] **Step 4: Run graph tests including large-blob fixture**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::graph::tests::
```

Expected: PASS and memory-bounded pagination assertions succeed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/cursor/global_storage.rs src-tauri/src/workspace_sync/adapters/cursor/graph.rs
git commit -m "feat(cursor): read composer bubble graphs"
```

### Task 3: Parse legacy workspace Chat/Composer data

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/cursor/workspace_storage.rs`

- [ ] **Step 1: Write a failing legacy projection test**

```rust
#[test]
fn projects_legacy_ai_service_records_into_one_session() {
    let sessions = read_workspace_sessions(fixture_dir("cursor/legacy-workspace")).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].messages[0].role, "user");
    assert_eq!(sessions[0].messages[1].role, "assistant");
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::workspace_storage::tests::projects_legacy_ai_service_records_into_one_session -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement recognized key projections**

Read only `aiService.prompts`, `aiService.generations`, `composer.composerData`, and `workbench.backgroundComposer.workspacePersistentData`. Associate workspace path from `workspace.json`; store a redacted/display-safe project name separately from the native path. Unknown key formats remain opaque backup items, not Session Manager messages.

- [ ] **Step 4: Run legacy tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::workspace_storage::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/cursor/workspace_storage.rs
git commit -m "feat(cursor): read legacy workspace chats"
```

### Task 4: Add Cursor to Session Manager

**Files:**
- Create: `src-tauri/src/session_manager/providers/cursor.rs`
- Modify: `src-tauri/src/session_manager/providers/mod.rs`
- Modify: `src-tauri/src/session_manager/mod.rs`
- Modify: `src/types.ts`

- [ ] **Step 1: Write failing scan/message tests**

```rust
#[test]
fn cursor_sessions_are_scannable_and_messages_are_lazy_loaded() {
    let provider = CursorSessionProvider::for_root(fixture_dir("cursor/known-schema"));
    let sessions = provider.scan_sessions();
    assert_eq!(sessions[0].provider_id, "cursor");
    assert_eq!(sessions[0].session_id, "composer-1");
    let messages = provider.load_messages(&sessions[0].source_path.clone().unwrap()).unwrap();
    assert_eq!(messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), vec!["user", "assistant"]);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml session_manager::providers::cursor::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement the provider**

Use a `cursor:` source reference containing database path plus Composer ID, parsed with a rightmost stable separator. Scan headers only; load Bubble bodies on detail open. Set `resume_command=None`. Add Cursor to parallel scanning and message dispatch; do not add it to provider switching `AppType`.

- [ ] **Step 4: Run Session Manager tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml session_manager::providers::cursor::tests::
cargo test --manifest-path src-tauri/Cargo.toml session_manager::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/session_manager src/types.ts
git commit -m "feat(cursor): show Cursor sessions in session manager"
```

### Task 5: Make the frontend Provider filter dynamic

**Files:**
- Modify: `src/components/sessions/SessionManagerPage.tsx`
- Modify: `src/components/sessions/utils.ts`
- Modify: `src/components/sessions/SessionItem.tsx`
- Test: `tests/components/SessionManagerPage.test.tsx`

- [ ] **Step 1: Write a failing Cursor filter test**

```tsx
it("derives provider filters from returned sessions", async () => {
  mockSessions([{ providerId: "cursor", sessionId: "composer-1", title: "Cursor chat" }]);
  render(<SessionManagerPage />);
  await userEvent.click(screen.getByRole("combobox", { name: /provider/i }));
  expect(screen.getByText("Cursor")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run and verify failure**

```bash
pnpm vitest run tests/components/SessionManagerPage.test.tsx -t "derives provider filters"
```

Expected: FAIL because the list is hard-coded.

- [ ] **Step 3: Implement dynamic provider metadata**

Derive unique provider IDs from session data, merge with a small metadata map for label/icon/sort order, and render the Select items from that array. Add `cursor` icon mapping with a safe fallback. Display sync/conflict/read-only badges from extended `SessionMeta` fields.

- [ ] **Step 4: Run UI tests**

```bash
pnpm vitest run tests/components/SessionManagerPage.test.tsx tests/components/SessionItem.test.tsx
pnpm typecheck
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/sessions tests/components/SessionManagerPage.test.tsx tests/components/SessionItem.test.tsx
git commit -m "feat(cursor): add dynamic session provider filters"
```

### Task 6: Implement known-schema Composer merge and fork

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/cursor/merge.rs`

- [ ] **Step 1: Write failing merge/fork tests**

```rust
#[test]
fn non_conflicting_bubbles_merge_and_conflicting_bubble_forks_composer() {
    let clean = merge_fixture("cursor/non-conflicting").unwrap();
    assert_eq!(clean.composers.len(), 1);
    assert_eq!(clean.composers[0].bubbles.len(), 3);
    let forked = merge_fixture("cursor/fork").unwrap();
    assert_eq!(forked.composers.len(), 2);
    assert_ne!(forked.composers[0].id, forked.composers[1].id);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::merge::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement deterministic graph merge**

Merge same Composer ID by Bubble ID and content hash. Preserve ordering/parent relations from schema-declared fields. Same Bubble ID with different payload forks the whole Composer. Generate a new UUID, then rewrite header, Bubble key prefixes, checkpoint links, subagent links, and all declared Composer references. If the schema lacks a declared reference, return a Conflict instead of writing.

- [ ] **Step 4: Run merge tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::merge::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/cursor/merge.rs
git commit -m "feat(cursor): merge and fork composer graphs"
```

### Task 7: Implement row-level Cursor write-back and rollback

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/cursor/apply.rs`

- [ ] **Step 1: Write a failing before-image rollback test**

```rust
#[test]
fn failed_cursor_apply_restores_header_and_kv_rows() {
    let harness = CursorApplyHarness::known_schema();
    harness.fail_after_writes(2);
    assert!(harness.apply(fork_plan()).is_err());
    assert_eq!(harness.database_hash(), harness.original_hash());
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::apply::tests::failed_cursor_apply_restores_header_and_kv_rows -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement write-back**

Require Cursor process preflight to be stopped. Snapshot the live DB with Backup API, validate the plan against the snapshot, store before-images for touched `composerHeaders`, `cursorDiskKV`, and declared `ItemTable` rows, then apply in one `IMMEDIATE` transaction. Run integrity checks after commit. Unknown schema returns `WriteBackUnsupported` before any write.

- [ ] **Step 4: Run apply and unknown-schema tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::apply::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::schema_registry::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/cursor/apply.rs
git commit -m "feat(cursor): safely write known composer schemas"
```

### Task 8: Merge Cursor Rules and Memory files

**Files:**
- Create: `src-tauri/src/workspace_sync/adapters/cursor/rules.rs`

- [ ] **Step 1: Write failing project-selection and conflict tests**

```rust
#[test]
fn only_selected_or_session_linked_project_rules_are_inventoried() {
    let result = inventory_rules(&fixture_projects(), &selected_projects(["project-a"])).unwrap();
    assert!(result.iter().all(|r| r.project_id == "project-a"));
}

#[test]
fn overlapping_rule_edit_creates_conflict_copy() {
    let result = merge_rule("base", "local", "remote").unwrap();
    assert!(matches!(result, RuleMerge::ConflictCopy { .. }));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::rules::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement bounded rule discovery and text merge**

Discover `.cursor/rules` only for workspaces linked by selected sessions or explicit settings. Apply the Plan 3 text merge; conflicting files keep local at the original path and write remote to `.conflict-<device>-<timestamp>`. Do not traverse or upload other project files.

- [ ] **Step 4: Run rules tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::rules::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/cursor/rules.rs
git commit -m "feat(cursor): sync selected rules and memory files"
```

### Task 9: Run the Plan 5 verification gate

- [ ] **Step 1: Run Cursor backend suites**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::cursor::
cargo test --manifest-path src-tauri/Cargo.toml session_manager::providers::cursor::
```

Expected: PASS for known, legacy, unknown, large-blob, and fork fixtures.

- [ ] **Step 2: Run Cursor UI suites**

```bash
pnpm vitest run tests/components/SessionManagerPage.test.tsx tests/components/SessionItem.test.tsx
pnpm typecheck
```

Expected: PASS.

- [ ] **Step 3: Run all checks**

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm format:check
pnpm test:unit
```

Expected: PASS.
