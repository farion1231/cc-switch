# Workspace Sync Productization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the workspace sync subsystem with secure credential handling, complete UI, conflict/history/device management, automation, retention, streaming performance, localization, and release gates.

**Architecture:** Keep long-running sync work in the Rust engine and expose narrow Tauri commands plus progress events. The React UI consumes typed status/preview/conflict models, while schedulers and GC use the same transaction and repository APIs as manual operations.

**Tech Stack:** Rust, Tauri events, system keychain, tokio cancellation, React, TanStack Query, i18next, Vitest, CI on macOS/Windows/Linux.

---

## File Map

- Create `src-tauri/src/workspace_sync/{credentials,scheduler,gc,streaming,cancel}.rs`.
- Extend `src-tauri/src/commands/workspace_sync.rs` and `src-tauri/src/lib.rs`.
- Create `src/lib/api/workspaceSync.ts` query/mutation hooks and `src/hooks/useWorkspaceSyncProgress.ts`.
- Create `src/components/workspace-sync/` page components.
- Modify Settings and Session Manager integration points.
- Add locale keys, documentation, CI fixtures, and release flag.

### Task 1: Add system credential storage and password lifecycle

**Files:**
- Create: `src-tauri/src/workspace_sync/credentials.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/commands/workspace_sync.rs`

- [ ] **Step 1: Add dependency and failing credential tests**

```toml
keyring = "3"
```

```rust
#[test]
fn settings_store_only_reference_never_password() {
    let store = MemoryCredentialStore::default();
    let reference = store.save("profile-a", "secret").unwrap();
    let serialized = serde_json::to_string(&WorkspaceSyncSettings::with_credential(reference)).unwrap();
    assert!(!serialized.contains("secret"));
}

#[test]
fn encryption_mode_change_requires_new_profile_or_migration() {
    assert!(matches!(validate_mode_change(EncryptionMode::Encrypted, EncryptionMode::None, false), Err(CredentialError::MigrationRequired)));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::credentials::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement credential abstraction**

```rust
pub trait CredentialStore: Send + Sync {
    fn save(&self, profile: &str, secret: &str) -> Result<String, CredentialError>;
    fn load(&self, reference: &str) -> Result<Option<zeroize::Zeroizing<String>>, CredentialError>;
    fn delete(&self, reference: &str) -> Result<(), CredentialError>;
}
```

Provide `SystemCredentialStore` backed by `keyring` and `MemoryCredentialStore` for tests. Implement unlock, lock, forget, and password-change commands. Password change writes to a new remote namespace, re-encrypts reachable snapshots/blobs, verifies the new Head, then optionally removes the old namespace after explicit confirmation.

- [ ] **Step 4: Run credential tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::credentials::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/workspace_sync/credentials.rs src-tauri/src/commands/workspace_sync.rs
git commit -m "feat(sync): store workspace sync secrets in system keychain"
```

### Task 2: Build typed frontend queries, mutations, and progress events

**Files:**
- Create: `src/lib/query/workspaceSyncQueries.ts`
- Create: `src/lib/query/workspaceSyncMutations.ts`
- Create: `src/hooks/useWorkspaceSyncProgress.ts`
- Modify: `src/lib/api/workspaceSync.ts`
- Modify: `src/types.ts`

- [ ] **Step 1: Write failing hook tests**

```tsx
it("updates progress from workspace-sync events and invalidates status on completion", async () => {
  const { result } = renderHook(() => useWorkspaceSyncProgress(), { wrapper });
  emitTauri("workspace-sync://progress", { transactionId: "tx-1", phase: "uploading", completed: 2, total: 5 });
  expect(result.current.phase).toBe("uploading");
  emitTauri("workspace-sync://completed", { transactionId: "tx-1" });
  await waitFor(() => expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ["workspaceSyncStatus"] }));
});
```

- [ ] **Step 2: Run and verify failure**

```bash
pnpm vitest run tests/hooks/useWorkspaceSyncProgress.test.tsx
```

Expected: FAIL.

- [ ] **Step 3: Implement typed hooks**

Use stable query keys for status, preview, snapshots, conflicts, devices, and diagnostics. Mutations for backup, merge, resolve, restore, remove device, rollback, unlock, and cancel invalidate only affected keys. `useWorkspaceSyncProgress` subscribes/unsubscribes through the existing Tauri event helper and ignores events for non-active transactions.

- [ ] **Step 4: Run hook tests and typecheck**

```bash
pnpm vitest run tests/hooks/useWorkspaceSyncProgress.test.tsx
pnpm typecheck
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib/api/workspaceSync.ts src/lib/query src/hooks/useWorkspaceSyncProgress.ts src/types.ts tests/hooks
git commit -m "feat(sync): add typed workspace sync frontend state"
```

### Task 3: Build the workspace sync settings page

**Files:**
- Create: `src/components/workspace-sync/WorkspaceSyncPage.tsx`
- Create: `src/components/workspace-sync/SyncOverview.tsx`
- Create: `src/components/workspace-sync/ProviderSyncCard.tsx`
- Create: `src/components/workspace-sync/EncryptionSetupDialog.tsx`
- Create: `src/components/workspace-sync/ProviderDiagnostics.tsx`
- Modify: `src/components/settings/SettingsPage.tsx`
- Modify: `src/components/settings/WebdavSyncSection.tsx`

- [ ] **Step 1: Write failing page tests**

```tsx
it("shows provider capabilities and keeps unknown Cursor schema read-only", async () => {
  mockWorkspaceStatus({ providers: [{ providerId: "cursor", detected: true, canRead: true, canWrite: false, reason: "unknownSchema" }] });
  render(<WorkspaceSyncPage />);
  expect(await screen.findByText("Cursor")).toBeInTheDocument();
  expect(screen.getByText(/read-only/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /merge to cursor/i })).toBeDisabled();
});
```

- [ ] **Step 2: Run and verify failure**

```bash
pnpm vitest run tests/components/workspace-sync/WorkspaceSyncPage.test.tsx
```

Expected: FAIL.

- [ ] **Step 3: Implement the page**

Display connection/encryption status, current device, last sync, current remote snapshot, pending counts, provider cards, estimated bytes, warnings, and diagnostics. Provide scan, preview, backup, merge, and history actions. Reuse stored WebDAV/S3 backend settings without placing work-data operations inside the existing config/Skills sync card.

- [ ] **Step 4: Run page tests**

```bash
pnpm vitest run tests/components/workspace-sync/WorkspaceSyncPage.test.tsx
pnpm typecheck
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/workspace-sync src/components/settings tests/components/workspace-sync
git commit -m "feat(sync): add workspace data sync settings page"
```

### Task 4: Build sync preview, progress, and cancellation UI

**Files:**
- Create: `src/components/workspace-sync/SyncPreviewDialog.tsx`
- Create: `src/components/workspace-sync/SyncProgressDialog.tsx`
- Create: `src-tauri/src/workspace_sync/cancel.rs`
- Modify: `src-tauri/src/workspace_sync/backup.rs`
- Modify: `src-tauri/src/commands/workspace_sync.rs`

- [ ] **Step 1: Write failing cancellation tests**

```rust
#[tokio::test]
async fn cancellation_stops_before_native_apply_and_marks_transaction_cancelled() {
    let harness = LongRunningSyncHarness::new();
    let id = harness.start().await;
    harness.cancel(&id).await.unwrap();
    let result = harness.join(id).await.unwrap();
    assert_eq!(result.state, TransactionState::Cancelled);
    assert!(!harness.native_data_changed());
}
```

```tsx
it("shows provider counts and sends cancel for the active transaction", async () => {
  render(<SyncProgressDialog transactionId="tx-1" />);
  await userEvent.click(screen.getByRole("button", { name: /cancel/i }));
  expect(workspaceSyncApi.cancel).toHaveBeenCalledWith("tx-1");
});
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::cancel::tests::
pnpm vitest run tests/components/workspace-sync/SyncProgressDialog.test.tsx
```

Expected: FAIL.

- [ ] **Step 3: Implement safe cancellation points and dialogs**

Use a per-transaction `CancellationToken`. Check before/after remote reads, between Blob uploads, before creating restore points, and before native apply. Once native apply starts, cancellation changes to “cancel after verification/rollback” rather than interrupting a database transaction. Preview displays additions, updates, deletes, auto-merges, conflicts, and bytes per provider.

- [ ] **Step 4: Run cancellation/UI tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::cancel::tests::
pnpm vitest run tests/components/workspace-sync/SyncPreviewDialog.test.tsx tests/components/workspace-sync/SyncProgressDialog.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/cancel.rs src-tauri/src/workspace_sync/backup.rs src-tauri/src/commands/workspace_sync.rs src/components/workspace-sync tests/components/workspace-sync
git commit -m "feat(sync): add preview progress and safe cancellation"
```

### Task 5: Build conflict center, snapshot history, and device manager

**Files:**
- Create: `src/components/workspace-sync/ConflictCenter.tsx`
- Create: `src/components/workspace-sync/ConflictDetail.tsx`
- Create: `src/components/workspace-sync/SnapshotHistory.tsx`
- Create: `src/components/workspace-sync/DeviceManager.tsx`
- Create: `src/components/workspace-sync/RestorePointList.tsx`
- Modify: `src-tauri/src/commands/workspace_sync.rs`
- Modify: `src-tauri/src/workspace_sync/repository.rs`

- [ ] **Step 1: Write failing conflict default test**

```tsx
it("defaults session conflicts to keep both", async () => {
  mockConflict({ id: "c1", kind: "session", availableActions: ["keepLocal", "keepRemote", "keepBoth"] });
  render(<ConflictDetail conflictId="c1" />);
  expect(await screen.findByRole("radio", { name: /keep both/i })).toBeChecked();
});
```

- [ ] **Step 2: Run and verify failure**

```bash
pnpm vitest run tests/components/workspace-sync/ConflictDetail.test.tsx
```

Expected: FAIL.

- [ ] **Step 3: Implement management views**

Add backend commands `workspace_sync_list_conflicts`, `workspace_sync_resolve_conflict`, `workspace_sync_list_snapshots`, `workspace_sync_restore_snapshot`, `workspace_sync_list_devices`, `workspace_sync_remove_device`, `workspace_sync_list_restore_points`, and `workspace_sync_rollback`. Conflict actions: keep local, keep remote, keep both, accept clean text merge, postpone, delete remote, export. Snapshot history shows device/time/provider counts and supports preview-before-restore. Device removal requires confirmation and records `removedAt`; restore points expose rollback only while compatible with current provider schema.

- [ ] **Step 4: Run management tests**

```bash
pnpm vitest run tests/components/workspace-sync/ConflictDetail.test.tsx tests/components/workspace-sync/SnapshotHistory.test.tsx tests/components/workspace-sync/DeviceManager.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/workspace-sync tests/components/workspace-sync
git commit -m "feat(sync): add conflict history and device management"
```

### Task 6: Add scheduler and lifecycle-triggered backup

**Files:**
- Create: `src-tauri/src/workspace_sync/scheduler.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Enable Tokio test time and write failing scheduling tests**

Add `"test-util"` to the existing Tokio feature list in `src-tauri/Cargo.toml`.

```rust
#[tokio::test(start_paused = true)]
async fn scheduler_runs_backup_once_per_interval_and_never_overlaps() {
    let harness = SchedulerHarness::new(Duration::from_secs(3600));
    harness.start();
    tokio::time::advance(Duration::from_secs(7201)).await;
    assert_eq!(harness.backup_start_count(), 2);
    assert_eq!(harness.max_concurrent_backups(), 1);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::scheduler::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement scheduling rules**

Support optional interval backup, remote-change check at startup, and local-upload on clean application exit. Reuse the global workspace sync mutex; skip when locked, no credential is available, another operation runs, or preview contains blocked secrets. Startup never applies remote changes automatically. Exit backup has a bounded timeout and never blocks shutdown indefinitely.

- [ ] **Step 4: Run scheduler tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::scheduler::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/scheduler.rs src-tauri/src/lib.rs src-tauri/src/settings.rs
git commit -m "feat(sync): schedule incremental workspace backups"
```

### Task 7: Add retention and garbage collection

**Files:**
- Create: `src-tauri/src/workspace_sync/gc.rs`
- Modify: `src-tauri/src/workspace_sync/repository.rs`
- Modify: `src-tauri/src/workspace_sync/state_db.rs`

- [ ] **Step 1: Write failing reachability tests**

```rust
#[test]
fn gc_keeps_head_ancestors_active_tombstones_and_unacked_device_data() {
    let graph = GcFixture::snapshot_graph();
    let plan = build_gc_plan(&graph, RetentionPolicy::default(), now()).unwrap();
    assert!(!plan.delete_snapshots.contains("head"));
    assert!(!plan.delete_blobs.contains("blob-referenced-by-tombstone"));
    assert!(plan.delete_blobs.contains("orphan-older-than-retention"));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::gc::tests::
```

Expected: FAIL.

- [ ] **Step 3: Implement mark-and-sweep GC**

Mark snapshots reachable from Head and retained history, then mark Blob refs, unresolved conflicts, restore-in-progress data, and unexpired Tombstones. Delete only unmarked objects older than the safety window. GC produces a preview and requires explicit confirmation until Stable release.

- [ ] **Step 4: Run GC tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::gc::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/gc.rs src-tauri/src/workspace_sync/repository.rs src-tauri/src/workspace_sync/state_db.rs
git commit -m "feat(sync): retain snapshots and collect orphan blobs"
```

### Task 8: Stream large files and enforce resource limits

**Files:**
- Create: `src-tauri/src/workspace_sync/streaming.rs`
- Modify: `src-tauri/src/workspace_sync/blob_store.rs`
- Modify: `src-tauri/src/workspace_sync/security.rs`

- [ ] **Step 1: Write failing memory/limit tests**

```rust
#[tokio::test]
async fn one_gibibyte_stream_never_buffers_the_entire_payload() {
    let source = CountingReader::repeating(1024 * 1024 * 1024);
    let metrics = stream_encrypt_to_sink(source, test_keys(), NullSink::default()).await.unwrap();
    assert!(metrics.max_buffered_bytes <= 8 * 1024 * 1024);
}

#[test]
fn rejects_manifest_and_entry_limits_before_allocation() {
    assert!(validate_manifest_size(16 * 1024 * 1024 + 1).is_err());
    assert!(validate_entry_count(1_000_001).is_err());
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::streaming::tests:: -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::security::tests::rejects_manifest -- --exact
```

Expected: FAIL.

- [ ] **Step 3: Implement bounded streaming**

Hash, encrypt, and upload in fixed-size chunks using a framed authenticated format for large blobs. Enforce 16 MB Manifest, 16 MB structured record, 256 MB ordinary Blob, configurable 5 GB sync total, 1,000,000 entries, and 100:1 extraction ratio. Cursor queries page by key prefix and never materialize all `cursorDiskKV` rows. Add a log-sanitization test that captures sync logs and asserts they contain no password, session body, raw project path, authorization header, or plaintext hash.

- [ ] **Step 4: Run performance/security tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::streaming::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::security::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/streaming.rs src-tauri/src/workspace_sync/blob_store.rs src-tauri/src/workspace_sync/security.rs
git commit -m "perf(sync): stream encrypted workspace blobs"
```

### Task 9: Add localization, docs, feature flags, and CI coverage

**Files:**
- Modify: `src/i18n/locales/{zh,en,ja}.json`
- Modify: `README.md`, `README_ZH.md`, `README_JA.md`
- Create: `docs/guides/workspace-data-sync-guide-{en,zh,ja}.md`
- Modify: `.github/workflows/ci.yml`
- Modify: `src-tauri/src/settings.rs`

- [ ] **Step 1: Write failing locale-key coverage test**

```ts
it("has matching workspaceSync keys in required locales", () => {
  expect(flattenKeys(zh.workspaceSync)).toEqual(flattenKeys(en.workspaceSync));
  expect(flattenKeys(ja.workspaceSync)).toEqual(flattenKeys(en.workspaceSync));
});
```

- [ ] **Step 2: Run and verify failure**

```bash
pnpm vitest run tests/i18n/workspaceSyncLocales.test.ts
```

Expected: FAIL.

- [ ] **Step 3: Add release controls and documentation**

Add `workspaceSyncExperimental` defaulting false for Alpha. Document encryption password loss, credential exclusion, provider capability levels, backup/merge flow, restore points, unknown-schema behavior, and storage limits. CI must run backend fixtures on macOS/Windows/Linux and frontend locale/UI tests on Linux.

- [ ] **Step 4: Run docs/locale/build checks**

```bash
pnpm vitest run tests/i18n/workspaceSyncLocales.test.ts
pnpm typecheck
pnpm format:check
cargo fmt --check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/i18n README.md README_ZH.md README_JA.md docs/guides .github/workflows/ci.yml src-tauri/src/settings.rs tests/i18n
git commit -m "docs(sync): document and gate workspace data sync"
```

### Task 10: Run the Stable verification gate

- [ ] **Step 1: Run frontend CI commands**

```bash
pnpm typecheck
pnpm format:check
pnpm test:unit
```

Expected: PASS.

- [ ] **Step 2: Run backend CI commands**

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS on macOS, Windows, and Linux.

- [ ] **Step 3: Run security scenarios**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::security::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::integration::tamper
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::integration::path_escape
```

Expected: PASS; tampered ciphertext, traversal, symlink escape, oversized data, and credential fixtures are rejected.

- [ ] **Step 4: Run multi-device and crash-recovery scenarios**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::integration::two_device
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::integration::sqlite_recovery
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::integration::head_race
```

Expected: PASS.

- [ ] **Step 5: Verify the legacy sync protocol**

```bash
cargo test --manifest-path src-tauri/Cargo.toml services::webdav_sync
cargo test --manifest-path src-tauri/Cargo.toml services::s3_sync
cargo test --manifest-path src-tauri/Cargo.toml services::sync_protocol
```

Expected: PASS with no protocol or artifact behavior regression.
