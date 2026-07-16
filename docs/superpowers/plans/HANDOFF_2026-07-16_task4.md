# Handoff — Task 4 Cloud Restore Credential Safety (2026-07-16)

## Status
**Backend complete and green.** Frontend UI for prepare/preview/apply deferred (pnpm skipped by session preference).

Task2 base remains at commit `357a7b5` context from earlier work; Task4 changes are local working tree (not necessarily committed).

## What landed

| Layer | Path | API |
|-------|------|-----|
| DB merge | `src-tauri/src/database/backup.rs` | `RemoteCredentialSelection`, `import_sql_string_for_sync_with_selections`, `exact_restore_preview` |
| Staging | `src-tauri/src/services/sync_protocol.rs` | `prepare_restore_preview`, `apply_staged_restore`, `apply_snapshot_with_selections` |
| WebDAV | `src-tauri/src/services/webdav_sync.rs` | `prepare_download`, `apply_download` (+ existing `download`) |
| S3 | `src-tauri/src/services/s3_sync.rs` | `prepare_download`, `apply_download` (+ existing `download`) |
| Commands | `commands/webdav_sync.rs`, `commands/s3_sync.rs` | `*_prepare_download`, `*_apply_download` |
| Register | `src-tauri/src/lib.rs` | handlers registered |

## Test results (last run)
- `database::backup::` → 5 passed
- `services::sync_protocol::` → 23 passed
- `cargo check --lib` → Finished (warnings only, pre-existing unused imports in provider_security)

## User-facing command contract (for UI)

### Prepare (download + stage, no DB mutation)
- `webdav_sync_prepare_download` / `s3_sync_prepare_download`
- Returns JSON roughly:
  - `status: "preview"`
  - `previewId`
  - `changedProviders`, `addedProviders`, `removedProviders`
  - `credentialFieldChanges`, `exactRestoreCredentialFieldCount`
  - transport metadata (`sourceLayout` / `sourcePath` for WebDAV)

### Apply (consume staging)
- `webdav_sync_apply_download` / `s3_sync_apply_download`
- Args: `previewId: string`, `selections?: RemoteCredentialSelection[]`
- `RemoteCredentialSelection`: `{ appType, providerId, useRemote }`
- Empty selections = keep all local credentials (safe default)
- Returns `{ status: "applied", previewId }` (+ post-sync warning attach if any)

### Legacy one-shot
- `webdav_sync_download` / `s3_sync_download` still apply immediately with **keep-local** semantics

## Staging layout
`{app_config_dir}/sync-staging/{preview_id}/`
- `db.sql`, `skills.zip`, `meta.json` (hashes + preview payload)
- Apply re-verifies size/hash; deletes staging dir on success

## Next agent steps (UI)
1. Read plan Task 4 remaining steps around restore UI in:
   `docs/superpowers/plans/2026-07-15-codex-workbench-integration.md`
2. Wire WebDAV/S3 download buttons to prepare → show preview modal → apply with per-provider toggles
3. Run:
   ```
   pnpm test:unit -- tests/components/ImportExportSection.test.tsx tests/components/WebdavSyncSection.test.tsx
   pnpm typecheck
   ```
4. Commit restore slice

## Pitfalls
- Prefer Python normalize CRLF for `backup.rs` / Windows path edits
- Do not use raw `'\\'` char literals carelessly in Rust source via PS/Python double-escaping
- Never claim UI tests green unless pnpm actually run
- Keys: do not read/move secret files

## Related progress doc
`docs/superpowers/plans/TASK4_PROGRESS.md`
