# CC Switch S3 Cloud Sync Design

**Date**: 2026-03-06
**Status**: Approved
**Author**: Keith + Claude Opus 4.6

## 1. Overview

Add AWS S3 as a new Cloud Sync backend for CC Switch, alongside the existing WebDAV sync. Users can sync provider configurations, skills, and database snapshots to S3 (or S3-compatible services like MinIO / Cloudflare R2) across devices.

### Goals

- Full feature parity with WebDAV sync: manual upload/download + auto sync
- AKSK (Access Key + Secret Key) authentication
- S3-compatible service support via custom endpoint
- Minimal invasive changes to existing WebDAV code

### Non-Goals

- IAM Profile / environment variable based auth (future enhancement)
- Concurrent sync to both WebDAV and S3 simultaneously

## 2. Architecture: Hybrid Approach (Option C)

Extract transport-agnostic sync protocol logic into a shared module. S3 and WebDAV each maintain independent transport implementations that call into the shared protocol.

```
  +----------------------------+
  | sync_protocol.rs (new)     |  <- shared: build_snapshot, validate_manifest, sha256...
  +-------------+--------------+
                |
       +--------+--------+
       v                  v
  webdav_sync.rs      s3_sync.rs (new)
  (adjust imports)    (s3.rs + sync_protocol)
```

### Why not full trait abstraction?

Async traits in Rust add complexity (async-trait crate or RPITIT). The hybrid approach gives code reuse without the abstraction overhead, and keeps both transport modules independently testable.

## 3. Shared Protocol Module

**New file**: `src-tauri/src/services/sync_protocol.rs`

Extracted from `webdav_sync.rs`:

- **Types**: `SyncManifest`, `ArtifactMeta`, `LocalSnapshot`
- **Constants**: `PROTOCOL_FORMAT`, `PROTOCOL_VERSION`, `REMOTE_DB_SQL`, `REMOTE_SKILLS_ZIP`, `REMOTE_MANIFEST`, `MAX_SYNC_ARTIFACT_BYTES`
- **Snapshot building**: `build_local_snapshot(db) -> LocalSnapshot`
- **Manifest handling**: `validate_manifest_compat()`, `compute_snapshot_id()`
- **Snapshot application**: `apply_snapshot(db, db_sql, skills_zip)`
- **Utilities**: `sha256_hex()`, `detect_system_device_name()`, `normalize_device_name()`
- **Status persistence helpers**: `persist_sync_success()`, `persist_sync_success_best_effort()`

Existing `webdav_sync.rs` changes to `use super::sync_protocol::*` — no behavioral change.

## 4. S3 Transport Layer

**New file**: `src-tauri/src/services/s3.rs`

**Dependency** (`Cargo.toml`):
```toml
rust-s3 = { version = "0.35", default-features = false, features = ["tokio-rustls-tls"] }
```

Uses `rustls` to match the project's existing `reqwest rustls-tls` choice.

### Functions

| Function | Description |
|----------|-------------|
| `create_bucket_client(settings) -> Bucket` | Build rust-s3 Bucket instance with region/endpoint/credentials |
| `test_connection(settings)` | HEAD Bucket or ListObjects(max=0) to verify connectivity and permissions |
| `ensure_remote_prefix(settings)` | No-op for S3 (flat object storage), optionally verify permissions |
| `put_bytes(bucket, key, bytes, content_type)` | `bucket.put_object(key, &bytes)` |
| `get_bytes(bucket, key, max_bytes) -> Option<(Vec<u8>, Option<String>)>` | GET object, return None on 404, extract ETag from headers |
| `head_etag(bucket, key) -> Option<String>` | HEAD object, extract ETag |

### S3 Key Path Convention

```
{remote_root}/v2/{profile}/{artifact}

Example: cc-switch-sync/v2/default/manifest.json
```

Mirrors the WebDAV remote directory structure. The `/` delimiter is used as a logical prefix separator (standard S3 practice).

### Custom Endpoint Support

- `endpoint` empty -> AWS official: `https://s3.{region}.amazonaws.com`
- `endpoint` non-empty -> path-style access for S3-compatible services (MinIO, R2)

## 5. S3 Sync Module

**New file**: `src-tauri/src/services/s3_sync.rs`

Calls `s3.rs` for transport + `sync_protocol.rs` for shared logic.

| Function | Description |
|----------|-------------|
| `check_connection(settings)` | `s3::test_connection` |
| `upload(db, settings)` | `sync_protocol::build_local_snapshot` -> `s3::put_bytes` (db.sql, skills.zip, manifest.json) |
| `download(db, settings)` | `s3::get_bytes` manifest -> validate -> download artifacts -> `sync_protocol::apply_snapshot` |
| `fetch_remote_info(settings)` | Download manifest.json, return device name/time/version info |

Independent `sync_mutex()` — does not interfere with WebDAV lock.

## 6. S3 Auto Sync

**New file**: `src-tauri/src/services/s3_auto_sync.rs`

Structure mirrors `webdav_auto_sync.rs`:

- Independent `DB_CHANGE_TX` channel
- Same debounce (1s) and max wait (10s) logic
- Controlled by `s3_sync.enabled && s3_sync.auto_sync`
- **Mutual exclusion**: S3 and WebDAV auto sync cannot both be enabled

## 7. Tauri Commands

**New file**: `src-tauri/src/commands/s3_sync.rs`

| Command | Description |
|---------|-------------|
| `s3_test_connection` | Connection test |
| `s3_sync_upload` | Manual upload |
| `s3_sync_download` | Manual download |
| `s3_sync_save_settings` | Save S3 configuration |
| `s3_sync_fetch_remote_info` | Fetch remote snapshot info |

Register in `commands/mod.rs` and Tauri app builder.

## 8. Settings

### Rust (`settings.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3SyncSettings {
    pub enabled: bool,
    pub auto_sync: bool,
    pub region: String,            // e.g. "us-east-1"
    pub bucket: String,            // e.g. "my-cc-switch-bucket"
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint: String,          // optional, for S3-compatible services
    pub remote_root: String,       // key prefix, default "cc-switch-sync"
    pub profile: String,           // default "default"
    pub status: SyncStatus,        // shared with WebDAV (extract from WebDavSyncStatus)
}
```

Added to `AppSettings`:
```rust
pub s3_sync: Option<S3SyncSettings>,
```

### Validation

- `s3_sync.enabled` and `webdav_sync.enabled` cannot both be `true`
- `bucket` and `region` are required when enabled
- `access_key_id` / `secret_access_key` must be non-empty when enabled

### TypeScript (`types.ts`)

```typescript
interface S3SyncSettings {
  enabled: boolean;
  autoSync: boolean;
  region: string;
  bucket: string;
  accessKeyId: string;
  secretAccessKey: string;
  endpoint: string;
  remoteRoot: string;
  profile: string;
  status?: SyncStatus;
}
```

## 9. Frontend

### 9.1 Preset Additions

Add to the sync presets dropdown (in `WebdavSyncSection.tsx` or renamed component):

```typescript
{ id: "aws-s3", label: "AWS S3", hint: "IAM Access Key with s3:PutObject/GetObject/HeadObject. Region examples: us-east-1, ap-northeast-1" },
{ id: "s3-compatible", label: "S3 Compatible (MinIO / R2)", hint: "Custom endpoint required, e.g. http://minio.local:9000. Region can be 'auto'." },
```

### 9.2 Dynamic Form Fields

When user selects an S3 preset, the form dynamically switches fields:

| WebDAV Mode | S3 Mode |
|-------------|---------|
| Server URL | (hidden) |
| Username | Access Key ID |
| Password | Secret Access Key |
| — | Region (new, placeholder: `us-east-1`) |
| — | Bucket (new, placeholder: `my-cc-switch-bucket`) |
| — | Endpoint (new, optional, placeholder: `https://s3-compatible.example.com`) |
| Remote Root | Remote Root (unchanged) |
| Profile | Profile (unchanged) |
| Auto Sync | Auto Sync (unchanged) |

### 9.3 Mutual Exclusion UX

When enabling S3 sync while WebDAV is already enabled, show a confirmation dialog:
> "Enabling S3 sync will disable the current WebDAV sync. Continue?"

### 9.4 API Layer (`lib/api/settings.ts`)

```typescript
s3TestConnection(settings: S3SyncSettings): Promise<TestResult>
s3SyncUpload(): Promise<SyncResult>
s3SyncDownload(): Promise<SyncResult>
s3SyncSaveSettings(settings: S3SyncSettings): Promise<{ success: boolean }>
s3SyncFetchRemoteInfo(): Promise<RemoteSnapshotInfo | { empty: true }>
```

### 9.5 i18n

Add Chinese, English, and Japanese translations for all new S3-related labels, hints, and error messages.

## 10. File Change Summary

| Action | File |
|--------|------|
| **New** | `src-tauri/src/services/sync_protocol.rs` |
| **New** | `src-tauri/src/services/s3.rs` |
| **New** | `src-tauri/src/services/s3_sync.rs` |
| **New** | `src-tauri/src/services/s3_auto_sync.rs` |
| **New** | `src-tauri/src/commands/s3_sync.rs` |
| **Modify** | `src-tauri/Cargo.toml` — add `rust-s3` |
| **Modify** | `src-tauri/src/services/mod.rs` — register new modules |
| **Modify** | `src-tauri/src/services/webdav_sync.rs` — import from sync_protocol |
| **Modify** | `src-tauri/src/commands/mod.rs` — register s3_sync commands |
| **Modify** | `src-tauri/src/settings.rs` — add S3SyncSettings + validation |
| **Modify** | `src-tauri/src/lib.rs` or `main.rs` — register Tauri commands + start S3 auto sync worker |
| **Modify** | `src/types.ts` — add S3SyncSettings type |
| **Modify** | `src/lib/api/settings.ts` — add S3 API methods |
| **Modify** | `src/components/settings/WebdavSyncSection.tsx` — add S3 preset + dynamic form |
| **Modify** | `src/i18n/locales/en.json` — S3 translations |
| **Modify** | `src/i18n/locales/zh.json` — S3 translations |
| **Modify** | `src/i18n/locales/ja.json` — S3 translations |
