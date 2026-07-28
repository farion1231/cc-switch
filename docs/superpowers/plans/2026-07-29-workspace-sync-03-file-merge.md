# Workspace Sync File Provider Merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add lossless three-way merge, tombstones, restore points, and safe native write-back for Claude Code and Grok Build.

**Architecture:** Compare Base/Local/Remote normalized items, classify unchanged, one-sided, append-only, text-merge, delete, and conflict cases, then apply a reviewed MergePlan through atomic file operations. Every destructive operation records a restore point and every unresolved branch remains accessible.

**Tech Stack:** Rust, JSONL parsing, diffy three-way merge, tempfile, atomic rename/fsync, existing Claude/Grok path helpers.

---

## File Map

- Create `src-tauri/src/workspace_sync/merge/{mod,append_only,text,planner}.rs`.
- Create `src-tauri/src/workspace_sync/conflict.rs`: persistent local conflict packages and metadata.
- Create `src-tauri/src/workspace_sync/sync.rs`: remote pull, plan, apply, rescan, and merged-snapshot publish orchestration.
- Create `src-tauri/src/workspace_sync/tombstone.rs`.
- Create `src-tauri/src/workspace_sync/restore_point.rs`.
- Create `src-tauri/src/workspace_sync/apply/{mod,files}.rs`.
- Extend `src-tauri/src/workspace_sync/adapters/{claude,grokbuild}.rs` with merge/write-back.
- Extend `src-tauri/src/commands/workspace_sync.rs` with preview/apply/rollback.
- Modify `src-tauri/src/commands/session_manager.rs` to create Tombstones on synced deletion.
- Add frontend API methods and conflict/session metadata types.

### Task 1: Implement append-only stream classification

**Files:**
- Create: `src-tauri/src/workspace_sync/merge/append_only.rs`
- Create: `src-tauri/src/workspace_sync/merge/mod.rs`

- [ ] **Step 1: Write failing classification tests**

```rust
#[test]
fn classifies_equal_extended_and_forked_streams() {
    assert_eq!(classify(b"a\nb\n", b"a\nb\n"), StreamRelation::Equal);
    assert_eq!(classify(b"a\nb\nc\n", b"a\nb\n"), StreamRelation::LocalExtendsRemote);
    assert_eq!(classify(b"a\nb\nl\n", b"a\nb\nr\n"), StreamRelation::Fork { common_records: 2 });
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::merge::append_only::tests:: -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement record-boundary comparison**

Split only on complete newline-terminated records; hash each record; reject an invalid trailing partial record as `Corrupt`. Return `Equal`, `LocalExtendsRemote`, `RemoteExtendsLocal`, `Fork { common_records }`, or `Corrupt`.

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::merge::append_only::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/merge
git commit -m "feat(sync): classify append-only session branches"
```

### Task 2: Implement text three-way merge

**Files:**
- Create: `src-tauri/src/workspace_sync/merge/text.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add dependency and failing tests**

```toml
diffy = "0.4"
```

```rust
#[test]
fn merges_non_overlapping_plan_edits() {
    let merged = merge_text("a\nb\n", "A\nb\n", "a\nB\n").unwrap();
    assert_eq!(merged, "A\nB\n");
}

#[test]
fn reports_overlapping_plan_edits_as_conflict() {
    assert!(matches!(merge_text("a\n", "l\n", "r\n"), Err(TextMergeConflict { .. })));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::merge::text::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement with `diffy::merge`**

Return merged UTF-8 text for clean merges. On conflict, return a struct containing Base/Local/Remote hashes and conflict-marker preview, but do not write marker text into a provider-native file.

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::merge::text::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/workspace_sync/merge/text.rs
git commit -m "feat(sync): add three-way plan text merge"
```

### Task 3: Build the generic three-way MergePlan

**Files:**
- Create: `src-tauri/src/workspace_sync/merge/planner.rs`
- Create: `src-tauri/src/workspace_sync/conflict.rs`
- Modify: `src-tauri/src/workspace_sync/model.rs`

- [ ] **Step 1: Write a failing no-loss test**

```rust
#[test]
fn every_changed_item_is_applied_or_conflicted() {
    let plan = plan_merge(base_fixture(), local_fixture(), remote_fixture()).unwrap();
    let accounted = plan.operations.len() + plan.conflicts.len();
    assert_eq!(accounted, plan.changed_logical_ids.len());
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::merge::planner::tests::every_changed_item_is_applied_or_conflicted -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement operation types and planning rules**

```rust
pub enum MergeOperation {
    KeepLocal(ItemKey),
    ApplyRemote(ItemKey),
    WriteMerged { key: ItemKey, bytes: Vec<u8> },
    CreateFork { source: ItemKey, new_logical_id: String },
    DeleteLocal(ItemKey),
}

pub struct MergePlan {
    pub base_snapshot_id: Option<String>,
    pub local_snapshot_id: String,
    pub remote_snapshot_id: String,
    pub operations: Vec<MergeOperation>,
    pub conflicts: Vec<ConflictPlan>,
    pub changed_logical_ids: BTreeSet<ItemKey>,
}
```

Implement deterministic sorting and explicit delete/modify conflict handling. A newer Tombstone may delete only when the opposing side equals Base. Persist each unresolved conflict under `<sync-root>/conflicts/<provider>/<conflict-id>/` with encrypted or permission-restricted Local/Remote payloads plus `conflict.json`; never store full payloads in the main cc-switch database.

- [ ] **Step 4: Run planner tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::merge::planner::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/model.rs src-tauri/src/workspace_sync/merge
git commit -m "feat(sync): plan lossless three-way merges"
```

### Task 4: Add Tombstone persistence and deletion interception

**Files:**
- Create: `src-tauri/src/workspace_sync/tombstone.rs`
- Modify: `src-tauri/src/workspace_sync/state_db.rs`
- Modify: `src-tauri/src/commands/session_manager.rs`
- Modify: `src/lib/api/sessions.ts`

- [ ] **Step 1: Write a failing delete/modify conflict test**

```rust
#[test]
fn deletion_does_not_win_over_post_base_modification() {
    let result = resolve_tombstone(&base_item(), None, Some(modified_remote()), tombstone_after_base());
    assert!(matches!(result, TombstoneResolution::ConflictDeleteModify));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::tombstone::tests::deletion_does_not_win_over_post_base_modification -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement Tombstone recording**

Add DAO methods `upsert_tombstone`, `list_tombstones`, and `acknowledge_tombstone`. Extend session deletion request with `recordTombstone: bool`; after successful provider deletion, record provider/kind/logical ID/last hash/deletedAt/device ID. Existing callers default to `true`; tests may pass `false` for isolated deletion fixtures.

- [ ] **Step 4: Run deletion regressions**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::tombstone::tests::
cargo test --manifest-path src-tauri/Cargo.toml session_manager::tests::
pnpm vitest run tests/hooks/useDeleteSessionMutation.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync src-tauri/src/commands/session_manager.rs src/lib/api/sessions.ts tests
git commit -m "feat(sync): record synchronized session deletions"
```

### Task 5: Add restore points and atomic file writes

**Files:**
- Create: `src-tauri/src/workspace_sync/restore_point.rs`
- Create: `src-tauri/src/workspace_sync/apply/mod.rs`
- Create: `src-tauri/src/workspace_sync/apply/files.rs`

- [ ] **Step 1: Write a failing rollback test**

```rust
#[test]
fn failed_second_write_restores_first_file() {
    let root = fixture_root_with_two_files();
    let applier = FileApplier::with_failure_after(1);
    assert!(applier.apply(root.path(), two_write_plan()).is_err());
    assert_eq!(std::fs::read(root.path().join("a.jsonl")).unwrap(), b"old-a");
    assert_eq!(std::fs::read(root.path().join("b.jsonl")).unwrap(), b"old-b");
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::apply::files::tests::failed_second_write_restores_first_file -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement atomic apply and rollback**

Before modification, copy only affected files/directories into `<sync-root>/restore-points/<tx-id>/`. Write `<name>.cc-switch-tmp`, call `sync_all`, rename original to transaction backup, rename temp into place, sync parent directory, and remove transaction backup only after full verification. Preserve restore points for 10 successful transactions or 30 days; never prune an unfinished transaction.

- [ ] **Step 4: Run file apply tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::apply::files::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::restore_point::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/apply src-tauri/src/workspace_sync/restore_point.rs
git commit -m "feat(sync): add atomic file merge rollback"
```

### Task 6: Implement Claude write-back

**Files:**
- Modify: `src-tauri/src/workspace_sync/adapters/claude.rs`
- Test: `src-tauri/src/workspace_sync/adapters/claude.rs`

- [ ] **Step 1: Write a failing fork test**

```rust
#[test]
fn claude_fork_preserves_both_jsonl_branches_and_sidecars() {
    let result = apply_fixture_merge("claude/fork").unwrap();
    assert_eq!(result.sessions.len(), 2);
    assert!(result.sessions.iter().any(|s| s.forked_from.as_deref() == Some("session-1")));
    assert!(result.sessions.iter().all(|s| s.path.exists()));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::claude::tests::claude_fork_preserves_both_jsonl_branches_and_sidecars -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement Claude merge application**

For prefix extension, write the longer JSONL. For fork, preserve local at its original path, create a collision-free filename for remote, copy/rewrite only fields that are known safe to rewrite, and persist `forkedFrom` in cc-switch conflict metadata. For text Plan clean merges, write merged text; for conflicts, keep original and create `.conflict-<device>-<timestamp>.md`.

- [ ] **Step 4: Run Claude tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::claude::tests::
cargo test --manifest-path src-tauri/Cargo.toml session_manager::providers::claude::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/claude.rs
git commit -m "feat(sync): merge and fork Claude work data"
```

### Task 7: Implement Grok Build write-back

**Files:**
- Modify: `src-tauri/src/workspace_sync/adapters/grokbuild.rs`

- [ ] **Step 1: Write a failing session-directory fork test**

```rust
#[test]
fn grok_fork_rewrites_summary_id_and_keeps_both_histories() {
    let result = apply_fixture_merge("grokbuild/fork").unwrap();
    assert_eq!(result.sessions.len(), 2);
    for session in result.sessions {
        let summary: serde_json::Value = serde_json::from_slice(&std::fs::read(session.path.join("summary.json")).unwrap()).unwrap();
        assert_eq!(summary["info"]["id"], session.id);
    }
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::grokbuild::tests::grok_fork_rewrites_summary_id_and_keeps_both_histories -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement session-directory apply**

Treat the session directory as the write unit. Prefix extension replaces `chat_history.jsonl` through `FileApplier`. Fork copies the remote directory under a generated UUID, rewrites `summary.info.id`, preserves opaque sidecars, and records the original ID in conflict metadata.

- [ ] **Step 4: Run Grok tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::adapters::grokbuild::tests::
cargo test --manifest-path src-tauri/Cargo.toml session_manager::providers::grokbuild::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/adapters/grokbuild.rs
git commit -m "feat(sync): merge and fork Grok sessions"
```

### Task 8: Expose preview, apply, conflict, and rollback commands

**Files:**
- Create: `src-tauri/src/workspace_sync/sync.rs`
- Modify: `src-tauri/src/commands/workspace_sync.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/api/workspaceSync.ts`
- Modify: `src/types.ts`

- [ ] **Step 1: Write failing API tests**

Assert exact invokes for `workspace_sync_preview_merge`, `workspace_sync_apply_merge`, `workspace_sync_list_conflicts`, and `workspace_sync_rollback`, including transaction ID and conflict resolution payloads.

- [ ] **Step 2: Run and verify failure**

```bash
pnpm vitest run tests/lib/workspaceSyncApi.test.ts
```

Expected: FAIL.

- [ ] **Step 3: Implement command contracts**

Commands must return MergePlan summaries without full session text. `apply_merge` requires the preview transaction ID and rejects stale local/remote snapshot IDs. The sync orchestrator downloads and authenticates the remote snapshot, applies the reviewed plan, rescans native data, creates a Merge Snapshot with both Local and Remote parents, and publishes it through CAS Head update. If Head changed, it must stop before declaring success, fetch the new Head, re-plan, and require confirmation when the effective plan changed. Emit progress events and record completed/rolled-back states in `workspace-sync.db`.

- [ ] **Step 4: Run frontend and command tests**

```bash
pnpm vitest run tests/lib/workspaceSyncApi.test.ts
pnpm typecheck
cargo test --manifest-path src-tauri/Cargo.toml commands::workspace_sync
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/workspace_sync.rs src-tauri/src/lib.rs src/lib/api/workspaceSync.ts src/types.ts tests
git commit -m "feat(sync): expose file merge and rollback commands"
```

### Task 9: Run the Plan 3 verification gate

- [ ] **Step 1: Run two-device branch/delete scenarios**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::integration::file_two_device
```

Expected: PASS for independent additions, session forks, delete propagation, and delete/modify conflicts.

- [ ] **Step 2: Run all checks**

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm typecheck
pnpm test:unit
```

Expected: PASS.
