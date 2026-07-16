# Task 4 Progress — Cloud restore credential safety

Date: 2026-07-16 (updated, backend complete)

## Scope (from plan)
Preserve local provider credentials on default cloud restore; exclude device-local security tables from WebDAV/S3 sync; provide exact-restore preview counts without applying; prepare/apply staging for opt-in remote credential fields.

## Done (backend, green)

### Protocol / tables
- `DB_COMPAT_VERSION` bumped for local-only tables exclusion from sync export
- `SYNC_LOCAL_ONLY_TABLES` + `export_sql_string_for_sync` / `import_sql_string_for_sync`
- Default cloud restore keeps existing local credential fields

### Credential merge API (`src-tauri/src/database/backup.rs`)
- `RemoteCredentialSelection { app_type, provider_id, use_remote }`
- `import_sql_string_for_sync_with_selections` / `apply_selected_credentials`
- `exact_restore_preview` counts credential field changes without applying
- Tests (5/5):
  - `cloud_restore_preserves_existing_local_credentials_by_default`
  - `cloud_restore_uses_remote_credentials_when_explicitly_selected`
  - `exact_restore_preview_counts_credential_changes_without_applying`
  - `sync_import_preserves_local_only_tables`
  - `periodic_maintenance_runs_even_when_auto_backup_disabled`

### Staging prepare/apply (`src-tauri/src/services/sync_protocol.rs`)
- `prepare_restore_preview(db, db_sql, skills_zip)` → stages under `~/.cc-switch/sync-staging/<preview_id>/`
  - writes `db.sql`, `skills.zip`, `meta.json` (hashes + RestorePreview)
  - does **not** mutate DB
- `apply_staged_restore(db, preview_id, selections)` → re-verifies hashes, applies with selections, removes staging
- `apply_snapshot` → thin wrapper over `apply_snapshot_with_selections(..., &[])`
- Tests: `prepare_restore_preview_stages_files_without_mutating_db`, `apply_staged_restore_rejects_unknown_preview_id`
- Full module: **23 passed**

### Transport wiring
- `webdav_sync::prepare_download` / `apply_download`
- `s3_sync::prepare_download` / `apply_download`
- One-shot `download` kept (default-safe keep-local credentials)

### Tauri commands + registration
- `webdav_sync_prepare_download`, `webdav_sync_apply_download`
- `s3_sync_prepare_download`, `s3_sync_apply_download`
- Registered in `src-tauri/src/lib.rs` generate_handler
- `cargo check --lib` **Finished** (no errors)

## Remaining (not done this session)
1. **Frontend UI** (explicitly deferred / skip pnpm this session):
   - `ImportExportSection` / `WebdavSyncSection` prepare → preview modal → apply with selections
   - unit tests: `pnpm test:unit -- tests/components/ImportExportSection.test.tsx tests/components/WebdavSyncSection.test.tsx`
   - `pnpm typecheck`
2. Commit restore slice (Step 7 of plan) when UI lands or as backend-only commit if desired
3. Optional: transport-level integration tests for prepare/apply (currently covered at protocol + compile)

## Verify commands
```powershell
cd "D:\LocalWork\Software Development\desktop\switch\cc-switch"
cargo test --manifest-path src-tauri/Cargo.toml --lib database::backup:: -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib services::sync_protocol:: -- --nocapture
cargo check --manifest-path src-tauri/Cargo.toml --lib
```

## Design notes / pitfalls
- `backup.rs` is CRLF; prefer Python path-normalize scripts for edits
- `apply_selected_credentials` copies fields FROM the settings Value argument INTO the target provider — for “keep local”, pass local settings; for “use remote”, simply do not re-apply local (remote already imported)
- `import_sql_string` (full import) ≠ `import_sql_string_for_sync` (preserve + merge)
- Default one-shot download already safe (keeps local credentials) via `import_sql_string_for_sync`
- preview_id path validation uses `std::path::is_separator` (avoid raw `\` char literals on Windows)
- S3 status type is reused `WebDavSyncStatus` via `update_s3_sync_status`
