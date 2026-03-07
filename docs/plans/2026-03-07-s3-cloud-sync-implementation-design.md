# CC Switch S3 Cloud Sync Implementation Design

**Date**: 2026-03-07
**Status**: Approved
**Author**: Keith + Claude Opus 4.6
**Supersedes**: 2026-03-06-s3-cloud-sync-design.md (updated transport approach)

## 1. Overview

Add S3-compatible cloud storage as a new Cloud Sync backend for CC Switch, alongside the existing WebDAV sync. Users can sync provider configurations, skills, and database snapshots to any S3-compatible service across devices.

### Goals

- Full feature parity with WebDAV sync: manual upload/download + auto sync
- AKSK (Access Key + Secret Key) authentication
- Broad S3-compatible service support: AWS S3, MinIO, Cloudflare R2, Alibaba Cloud OSS, Tencent Cloud COS, Huawei OBS, and custom endpoints
- Zero new heavy dependencies — implement S3 signing on top of existing `reqwest`
- Minimal invasive changes to existing WebDAV code

### Non-Goals

- IAM Role / Instance Profile / STS Token auth (future enhancement)
- Concurrent sync to both WebDAV and S3 simultaneously (mutually exclusive)

## 2. Architecture: Hybrid Approach

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

### Why reqwest + hand-rolled Sig V4 instead of rust-s3?

1. **Broader compatibility** — Chinese cloud providers (OSS, COS, OBS) have subtle signing differences; hand-rolled signing is fully controllable
2. **Zero new heavy dependencies** — only adds `hmac = "0.12"` (~trivial); reuses existing `reqwest`, `sha2`, `base64`, `chrono`
3. **Architectural consistency** — mirrors `webdav.rs` which is also built on raw `reqwest`
4. **Minimal binary size impact** — avoids pulling in ~30+ transitive crates from `rust-s3`

## 3. Shared Protocol Module

**New file**: `src-tauri/src/services/sync_protocol.rs`

Extracted from `webdav_sync.rs`:

| Category | Contents |
|----------|----------|
| **Types** | `SyncManifest`, `ArtifactMeta`, `LocalSnapshot` |
| **Constants** | `PROTOCOL_FORMAT`, `PROTOCOL_VERSION`, `REMOTE_DB_SQL`, `REMOTE_SKILLS_ZIP`, `REMOTE_MANIFEST`, `MAX_SYNC_ARTIFACT_BYTES`, `MAX_MANIFEST_BYTES`, `MAX_DEVICE_NAME_LEN` |
| **Snapshot building** | `build_local_snapshot(db) -> LocalSnapshot` |
| **Manifest handling** | `validate_manifest_compat()`, `compute_snapshot_id()` |
| **Snapshot application** | `apply_snapshot(db, db_sql, skills_zip)` |
| **Artifact verification** | `verify_artifact(bytes, artifact_name, meta) -> Result<(), AppError>` — extracted from `download_and_verify`, checks size + sha256 |
| **Utilities** | `sha256_hex()`, `detect_system_device_name()`, `normalize_device_name()`, `validate_artifact_size_limit()` |
| **Status persistence** | `persist_sync_success_best_effort()` — generic version accepting closure |
| **Error helpers** | `localized()`, `io_context_localized()` |

Existing `webdav_sync.rs` changes to `use super::sync_protocol::*` — no behavioral change. The `archive` submodule stays in `webdav_sync/` (it's used by both transports via `sync_protocol`).

## 4. S3 Transport Layer

**New file**: `src-tauri/src/services/s3.rs`

**New dependency** (`Cargo.toml`):
```toml
hmac = "0.12"
```

All other dependencies (`reqwest`, `sha2`, `base64`, `chrono`) are already present.

### 4.1 AWS Signature V4 Implementation

~150 lines implementing the standard [AWS Sig V4 signing process](https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-authenticating-requests.html):

1. **Canonical Request** — method, URI, query string, headers, signed headers, payload hash
2. **String to Sign** — algorithm, timestamp, credential scope, hash of canonical request
3. **Signing Key** — HMAC chain: date key → region key → service key → signing key
4. **Authorization Header** — `AWS4-HMAC-SHA256 Credential=.../..., SignedHeaders=..., Signature=...`

Internal signing function:
```rust
fn sign_request(
    method: &str,
    url: &Url,
    headers: &mut HeaderMap,
    body_hash: &str,
    creds: &S3Credentials,
    now: DateTime<Utc>,
) -> String  // Returns Authorization header value
```

### 4.2 S3 Credentials

```rust
pub struct S3Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
    pub bucket: String,
    pub endpoint: String,    // empty = AWS official
    pub path_style: bool,    // auto-inferred or user-specified
}
```

### 4.3 Endpoint & Path Style Resolution

```rust
fn resolve_endpoint(creds: &S3Credentials) -> (String, bool)
```

| Provider | Endpoint Format | path_style |
|----------|----------------|-----------|
| AWS S3 | `https://s3.{region}.amazonaws.com` | `false` (virtual-hosted) |
| MinIO | `http://minio.local:9000` | `true` |
| Cloudflare R2 | `https://{account_id}.r2.cloudflarestorage.com` | `true` |
| Alibaba OSS | `https://oss-{region}.aliyuncs.com` | `true` |
| Tencent COS | `https://cos.{region}.myqcloud.com` | `true` |
| Huawei OBS | `https://obs.{region}.myhuaweicloud.com` | `true` |

**Auto-inference logic**: endpoint empty or contains `amazonaws.com` → virtual-hosted; otherwise → path-style. User can override via settings.

URL construction:
- **Virtual-hosted**: `https://{bucket}.s3.{region}.amazonaws.com/{key}`
- **Path-style**: `https://{endpoint}/{bucket}/{key}`

### 4.4 Transport Functions

| Function | Description |
|----------|-------------|
| `test_connection(creds) -> Result<()>` | HEAD `/{bucket}` to verify connectivity and permissions |
| `put_object(creds, key, bytes, content_type) -> Result<()>` | PUT Object |
| `get_object(creds, key, max_bytes) -> Result<Option<(Vec<u8>, Option<String>)>>` | GET Object; returns None on 404; extracts ETag |
| `head_object(creds, key) -> Result<Option<String>>` | HEAD Object; returns ETag |

All functions use the existing `crate::proxy::http_client::get()` for the reqwest client, consistent with WebDAV transport.

Timeouts: `30s` for metadata operations, `300s` for data transfers (same as WebDAV).

### 4.5 S3 Key Path Convention

```
{remote_root}/v2/{profile}/{artifact}

Example: cc-switch-sync/v2/default/manifest.json
```

Mirrors the WebDAV remote directory structure. The `/` delimiter is a logical prefix separator (standard S3 practice). No directory creation needed (unlike WebDAV MKCOL).

## 5. S3 Sync Module

**New file**: `src-tauri/src/services/s3_sync.rs`

Calls `s3.rs` for transport + `sync_protocol.rs` for shared logic.

| Function | Description |
|----------|-------------|
| `sync_mutex()` | Independent `tokio::sync::Mutex` — does not interfere with WebDAV lock |
| `run_with_sync_lock(op)` | Acquire S3 sync mutex before running operation |
| `check_connection(settings)` | `s3::test_connection` |
| `upload(db, settings)` | `sync_protocol::build_local_snapshot` → `s3::put_object` (db.sql, skills.zip, manifest.json) |
| `download(db, settings)` | `s3::get_object` manifest → validate → download artifacts → `sync_protocol::apply_snapshot` |
| `fetch_remote_info(settings)` | Download manifest.json, return device name/time/version info |

Upload order: artifacts first (db.sql, skills.zip), manifest last — same best-effort consistency as WebDAV.

## 6. S3 Auto Sync

**New file**: `src-tauri/src/services/s3_auto_sync.rs`

Structure mirrors `webdav_auto_sync.rs`:

- Independent `DB_CHANGE_TX` channel
- Same debounce (1s) and max wait (10s) logic
- Controlled by `s3_sync.enabled && s3_sync.auto_sync`
- Emits `s3-sync-status-updated` event (distinct from `webdav-sync-status-updated`)
- Reuses `should_trigger_for_table()` logic from `webdav_auto_sync` (extract to shared or duplicate — prefer duplicate for independence)
- Independent `AutoSyncSuppressionGuard` with its own atomic counter

## 7. Tauri Commands

**New file**: `src-tauri/src/commands/s3_sync.rs`

| Command | Description |
|---------|-------------|
| `s3_test_connection(settings, preserveEmptyPassword)` | Connection test with password preservation |
| `s3_sync_upload(state)` | Manual upload with sync lock |
| `s3_sync_download(state)` | Manual download with sync lock + post-import sync |
| `s3_sync_save_settings(settings, passwordTouched)` | Save S3 configuration |
| `s3_sync_fetch_remote_info()` | Fetch remote snapshot info |

Register in `commands/mod.rs` and Tauri app builder (`lib.rs`).

Pattern follows `commands/webdav_sync.rs` exactly:
- `require_enabled_s3_settings()` guard
- `resolve_secret_for_request()` for password preservation
- `persist_sync_error()` for error status tracking
- `map_sync_result()` for error handling

## 8. Settings

### 8.1 Rust (`settings.rs`)

**New sync status type** (rename + reuse):
```rust
/// Shared sync status (used by both WebDAV and S3)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub last_sync_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_source: Option<String>,
    pub last_remote_etag: Option<String>,
    pub last_local_manifest_hash: Option<String>,
    pub last_remote_manifest_hash: Option<String>,
}

// Type alias for backward compatibility
pub type WebDavSyncStatus = SyncStatus;
```

**New S3 settings**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3SyncSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub secret_access_key: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_remote_root")]
    pub remote_root: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub status: SyncStatus,
}
```

**AppSettings addition**:
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub s3_sync: Option<S3SyncSettings>,
```

### 8.2 Validation

- `s3_sync.enabled` and `webdav_sync.enabled` cannot both be `true`
- `bucket` and `region` are required when enabled
- `access_key_id` / `secret_access_key` must be non-empty when enabled

### 8.3 Frontend-safe serialization

In `get_settings_for_frontend()`:
```rust
if let Some(s3) = &mut settings.s3_sync {
    s3.secret_access_key.clear();
}
```

### 8.4 Settings management functions

```rust
pub fn get_s3_sync_settings() -> Option<S3SyncSettings>
pub fn set_s3_sync_settings(settings: Option<S3SyncSettings>) -> Result<(), AppError>
pub fn update_s3_sync_status(status: SyncStatus) -> Result<(), AppError>
```

### 8.5 TypeScript (`types.ts`)

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

Add S3 presets to the sync provider dropdown in `WebdavSyncSection.tsx`:

```typescript
{ id: "aws-s3", label: "AWS S3", hint: "IAM Access Key. Region: us-east-1, ap-northeast-1, etc." },
{ id: "s3-minio", label: "MinIO", hint: "Custom endpoint required, e.g. http://minio.local:9000" },
{ id: "s3-r2", label: "Cloudflare R2", hint: "Endpoint: https://<account_id>.r2.cloudflarestorage.com" },
{ id: "s3-oss", label: "Alibaba Cloud OSS", hint: "Endpoint: https://oss-<region>.aliyuncs.com" },
{ id: "s3-cos", label: "Tencent Cloud COS", hint: "Endpoint: https://cos.<region>.myqcloud.com" },
{ id: "s3-obs", label: "Huawei OBS", hint: "Endpoint: https://obs.<region>.myhuaweicloud.com" },
{ id: "s3-custom", label: "S3 Compatible (Custom)", hint: "Any S3-compatible service with custom endpoint" },
```

### 9.2 Dynamic Form Fields

When user selects an S3 preset, the form dynamically switches fields:

| WebDAV Mode | S3 Mode |
|-------------|---------|
| Server URL | (hidden) |
| Username | Access Key ID |
| Password | Secret Access Key |
| -- | Region (new, placeholder varies by preset) |
| -- | Bucket (new, placeholder: `my-cc-switch-bucket`) |
| -- | Endpoint (new, auto-filled for known providers, editable) |
| Remote Root | Remote Root (unchanged) |
| Profile | Profile (unchanged) |
| Auto Sync | Auto Sync (unchanged) |

For known providers (OSS, COS, OBS), the endpoint field is auto-populated based on the region input.

### 9.3 Mutual Exclusion UX

When enabling S3 sync while WebDAV is already enabled, show a confirmation dialog:
> "Enabling S3 sync will disable the current WebDAV sync. Continue?"

And vice versa.

### 9.4 API Layer (`lib/api/settings.ts`)

```typescript
s3TestConnection(settings: S3SyncSettings): Promise<TestResult>
s3SyncUpload(): Promise<SyncResult>
s3SyncDownload(): Promise<SyncResult>
s3SyncSaveSettings(settings: S3SyncSettings, passwordTouched: boolean): Promise<{ success: boolean }>
s3SyncFetchRemoteInfo(): Promise<RemoteSnapshotInfo | { empty: true }>
```

### 9.5 i18n

Add Chinese, English, and Japanese translations for all new S3-related labels, hints, and error messages in `src/i18n/locales/{zh,en,ja}.json`.

## 10. File Change Summary

| Action | File |
|--------|------|
| **New** | `src-tauri/src/services/sync_protocol.rs` |
| **New** | `src-tauri/src/services/s3.rs` |
| **New** | `src-tauri/src/services/s3_sync.rs` |
| **New** | `src-tauri/src/services/s3_auto_sync.rs` |
| **New** | `src-tauri/src/commands/s3_sync.rs` |
| **Modify** | `src-tauri/Cargo.toml` — add `hmac = "0.12"` |
| **Modify** | `src-tauri/src/services/mod.rs` — register new modules |
| **Modify** | `src-tauri/src/services/webdav_sync.rs` — import from sync_protocol |
| **Modify** | `src-tauri/src/commands/mod.rs` — register s3_sync commands |
| **Modify** | `src-tauri/src/settings.rs` — add S3SyncSettings, SyncStatus rename, mutual exclusion validation, settings management functions |
| **Modify** | `src-tauri/src/lib.rs` — register Tauri commands + start S3 auto sync worker |
| **Modify** | `src/types.ts` — add S3SyncSettings type |
| **Modify** | `src/lib/api/settings.ts` — add S3 API methods |
| **Modify** | `src/components/settings/WebdavSyncSection.tsx` — add S3 presets + dynamic form |
| **Modify** | `src/i18n/locales/en.json` — S3 translations |
| **Modify** | `src/i18n/locales/zh.json` — S3 translations |
| **Modify** | `src/i18n/locales/ja.json` — S3 translations |

## 11. Testing Strategy

### Unit Tests (Rust)

- `sync_protocol.rs`: snapshot building, manifest validation, sha256, device name normalization (migrated from webdav_sync tests)
- `s3.rs`: Sig V4 signing correctness against AWS test vectors, endpoint resolution, URL construction
- `s3_sync.rs`: sync mutex singleton, key path construction
- `s3_auto_sync.rs`: trigger table filtering, suppression guard, debounce timing
- `commands/s3_sync.rs`: password preservation, enabled guard, error persistence

### Integration Tests

- Manual testing against MinIO (local Docker) for end-to-end upload/download
- Verify mutual exclusion between S3 and WebDAV settings
