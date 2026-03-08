# S3 Cloud Sync Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add S3-compatible cloud storage as a new sync backend for CC Switch, using hand-rolled AWS Sig V4 on existing reqwest.

**Architecture:** Extract shared sync protocol from webdav_sync.rs, implement S3 transport on top of reqwest + hmac, mirror the WebDAV sync/auto-sync/commands pattern for S3, and extend the frontend with S3 presets and dynamic form switching.

**Tech Stack:** Rust (Tauri 2, reqwest, sha2, hmac, chrono, base64), React 18, TypeScript, TanStack Query, react-i18next

**Design Doc:** `docs/plans/2026-03-07-s3-cloud-sync-implementation-design.md`

---

## Task 1: Add `hmac` dependency

**Files:**
- Modify: `src-tauri/Cargo.toml`

**Step 1: Add hmac to dependencies**

In `src-tauri/Cargo.toml`, add after the `sha2 = "0.10"` line:

```toml
hmac = "0.12"
```

**Step 2: Verify compilation**

Run: `cd /root/keith-space/github-search/cc-switch/src-tauri && cargo check 2>&1 | tail -5`
Expected: compilation succeeds (or downloads hmac crate)

**Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "deps: add hmac crate for S3 Sig V4 signing"
```

---

## Task 2: Extract `sync_protocol.rs` from `webdav_sync.rs`

**Files:**
- Create: `src-tauri/src/services/sync_protocol.rs`
- Modify: `src-tauri/src/services/webdav_sync.rs` (remove extracted code, add imports)
- Modify: `src-tauri/src/services/mod.rs` (register module)

**Context:** `webdav_sync.rs` contains both transport-agnostic sync protocol logic and WebDAV-specific code. We extract the protocol layer so S3 can reuse it.

**Step 1: Create `sync_protocol.rs`**

Create `src-tauri/src/services/sync_protocol.rs` with the following extracted code:

```rust
//! Transport-agnostic sync protocol layer.
//!
//! Shared types, constants, snapshot building, manifest handling, and utilities
//! used by both WebDAV and S3 sync transports.

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

use crate::error::AppError;

// Re-export archive utilities for both transports
pub(crate) use super::webdav_sync::archive::{
    backup_current_skills, restore_skills_from_backup, restore_skills_zip, zip_skills_ssot,
};

// ─── Protocol constants ──────────────────────────────────────

pub const PROTOCOL_FORMAT: &str = "cc-switch-webdav-sync";
pub const PROTOCOL_VERSION: u32 = 2;
pub const REMOTE_DB_SQL: &str = "db.sql";
pub const REMOTE_SKILLS_ZIP: &str = "skills.zip";
pub const REMOTE_MANIFEST: &str = "manifest.json";
pub const MAX_DEVICE_NAME_LEN: usize = 64;
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_SYNC_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

// ─── Error helpers ───────────────────────────────────────────

pub fn localized(key: &'static str, zh: impl Into<String>, en: impl Into<String>) -> AppError {
    AppError::localized(key, zh, en)
}

pub fn io_context_localized(
    _key: &'static str,
    zh: impl Into<String>,
    en: impl Into<String>,
    source: std::io::Error,
) -> AppError {
    let zh_msg = zh.into();
    let en_msg = en.into();
    AppError::IoContext {
        context: format!("{zh_msg} ({en_msg})"),
        source,
    }
}

// ─── Types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncManifest {
    pub format: String,
    pub version: u32,
    pub device_name: String,
    pub created_at: String,
    pub artifacts: BTreeMap<String, ArtifactMeta>,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMeta {
    pub sha256: String,
    pub size: u64,
}

pub struct LocalSnapshot {
    pub db_sql: Vec<u8>,
    pub skills_zip: Vec<u8>,
    pub manifest_bytes: Vec<u8>,
    pub manifest_hash: String,
}

// ─── Snapshot building ───────────────────────────────────────

pub fn build_local_snapshot(
    db: &crate::database::Database,
) -> Result<LocalSnapshot, AppError> {
    let sql_string = db.export_sql_string()?;
    let db_sql = sql_string.into_bytes();

    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "sync.snapshot_tmpdir_failed",
            "创建快照临时目录失败",
            "Failed to create temporary directory for snapshot",
            e,
        )
    })?;
    let skills_zip_path = tmp.path().join(REMOTE_SKILLS_ZIP);
    zip_skills_ssot(&skills_zip_path)?;
    let skills_zip = fs::read(&skills_zip_path).map_err(|e| AppError::io(&skills_zip_path, e))?;

    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        REMOTE_DB_SQL.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(&db_sql),
            size: db_sql.len() as u64,
        },
    );
    artifacts.insert(
        REMOTE_SKILLS_ZIP.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(&skills_zip),
            size: skills_zip.len() as u64,
        },
    );

    let snapshot_id = compute_snapshot_id(&artifacts);
    let manifest = SyncManifest {
        format: PROTOCOL_FORMAT.to_string(),
        version: PROTOCOL_VERSION,
        device_name: detect_system_device_name().unwrap_or_else(|| "Unknown Device".to_string()),
        created_at: Utc::now().to_rfc3339(),
        artifacts,
        snapshot_id,
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| AppError::JsonSerialize { source: e })?;
    let manifest_hash = sha256_hex(&manifest_bytes);

    Ok(LocalSnapshot {
        db_sql,
        skills_zip,
        manifest_bytes,
        manifest_hash,
    })
}

// ─── Manifest handling ───────────────────────────────────────

pub fn compute_snapshot_id(artifacts: &BTreeMap<String, ArtifactMeta>) -> String {
    let parts: Vec<String> = artifacts
        .iter()
        .map(|(name, meta)| format!("{}:{}", name, meta.sha256))
        .collect();
    sha256_hex(parts.join("|").as_bytes())
}

pub fn validate_manifest_compat(manifest: &SyncManifest) -> Result<(), AppError> {
    if manifest.format != PROTOCOL_FORMAT {
        return Err(localized(
            "sync.manifest_format_incompatible",
            format!("远端 manifest 格式不兼容: {}", manifest.format),
            format!("Remote manifest format is incompatible: {}", manifest.format),
        ));
    }
    if manifest.version != PROTOCOL_VERSION {
        return Err(localized(
            "sync.manifest_version_incompatible",
            format!(
                "远端 manifest 协议版本不兼容: v{} (本地 v{PROTOCOL_VERSION})",
                manifest.version
            ),
            format!(
                "Remote manifest protocol version is incompatible: v{} (local v{PROTOCOL_VERSION})",
                manifest.version
            ),
        ));
    }
    Ok(())
}

// ─── Artifact verification ───────────────────────────────────

pub fn verify_artifact(
    bytes: &[u8],
    artifact_name: &str,
    meta: &ArtifactMeta,
) -> Result<(), AppError> {
    if bytes.len() as u64 != meta.size {
        return Err(localized(
            "sync.artifact_size_mismatch",
            format!(
                "artifact {artifact_name} 大小不匹配 (expected: {}, got: {})",
                meta.size,
                bytes.len(),
            ),
            format!(
                "Artifact {artifact_name} size mismatch (expected: {}, got: {})",
                meta.size,
                bytes.len(),
            ),
        ));
    }

    let actual_hash = sha256_hex(bytes);
    if actual_hash != meta.sha256 {
        return Err(localized(
            "sync.artifact_hash_mismatch",
            format!(
                "artifact {artifact_name} SHA256 校验失败 (expected: {}..., got: {}...)",
                meta.sha256.get(..8).unwrap_or(&meta.sha256),
                actual_hash.get(..8).unwrap_or(&actual_hash),
            ),
            format!(
                "Artifact {artifact_name} SHA256 verification failed (expected: {}..., got: {}...)",
                meta.sha256.get(..8).unwrap_or(&meta.sha256),
                actual_hash.get(..8).unwrap_or(&actual_hash),
            ),
        ));
    }
    Ok(())
}

pub fn validate_artifact_size_limit(artifact_name: &str, size: u64) -> Result<(), AppError> {
    if size > MAX_SYNC_ARTIFACT_BYTES {
        let max_mb = MAX_SYNC_ARTIFACT_BYTES / 1024 / 1024;
        return Err(localized(
            "sync.artifact_too_large",
            format!("artifact {artifact_name} 超过下载上限（{} MB）", max_mb),
            format!("Artifact {artifact_name} exceeds download limit ({} MB)", max_mb),
        ));
    }
    Ok(())
}

// ─── Snapshot application ────────────────────────────────────

pub fn apply_snapshot(
    db: &crate::database::Database,
    db_sql: &[u8],
    skills_zip: &[u8],
) -> Result<(), AppError> {
    let sql_str = std::str::from_utf8(db_sql).map_err(|e| {
        localized(
            "sync.sql_not_utf8",
            format!("SQL 非 UTF-8: {e}"),
            format!("SQL is not valid UTF-8: {e}"),
        )
    })?;
    let skills_backup = backup_current_skills()?;

    restore_skills_zip(skills_zip)?;

    if let Err(db_err) = db.import_sql_string(sql_str) {
        if let Err(rollback_err) = restore_skills_from_backup(&skills_backup) {
            return Err(localized(
                "sync.db_import_and_rollback_failed",
                format!("导入数据库失败: {db_err}; 同时回滚 Skills 失败: {rollback_err}"),
                format!("Database import failed: {db_err}; skills rollback also failed: {rollback_err}"),
            ));
        }
        return Err(db_err);
    }

    Ok(())
}

// ─── Utilities ───────────────────────────────────────────────

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn detect_system_device_name() -> Option<String> {
    let env_name = ["CC_SWITCH_DEVICE_NAME", "COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|value| normalize_device_name(&value));

    if env_name.is_some() {
        return env_name;
    }

    let output = Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let hostname = String::from_utf8(output.stdout).ok()?;
    normalize_device_name(&hostname)
}

pub fn normalize_device_name(raw: &str) -> Option<String> {
    let compact = raw
        .chars()
        .fold(String::with_capacity(raw.len()), |mut acc, ch| {
            if ch.is_whitespace() {
                acc.push(' ');
            } else if !ch.is_control() {
                acc.push(ch);
            }
            acc
        });
    let normalized = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }

    let limited = trimmed
        .chars()
        .take(MAX_DEVICE_NAME_LEN)
        .collect::<String>();
    if limited.is_empty() {
        None
    } else {
        Some(limited)
    }
}

// ─── Status persistence ──────────────────────────────────────

pub fn persist_sync_success_best_effort<S, F>(
    settings: &mut S,
    manifest_hash: String,
    etag: Option<String>,
    persist_fn: F,
) -> bool
where
    F: FnOnce(&mut S, String, Option<String>) -> Result<(), AppError>,
{
    match persist_fn(settings, manifest_hash, etag) {
        Ok(()) => true,
        Err(err) => {
            log::warn!("[Sync] Persist sync status failed, keep operation success: {err}");
            false
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(sha256: &str, size: u64) -> ArtifactMeta {
        ArtifactMeta {
            sha256: sha256.to_string(),
            size,
        }
    }

    #[test]
    fn snapshot_id_is_stable() {
        let mut artifacts = BTreeMap::new();
        artifacts.insert("db.sql".to_string(), artifact("abc123", 100));
        artifacts.insert("skills.zip".to_string(), artifact("def456", 200));
        let id1 = compute_snapshot_id(&artifacts);
        let id2 = compute_snapshot_id(&artifacts);
        assert_eq!(id1, id2);
    }

    #[test]
    fn snapshot_id_changes_with_artifacts() {
        let mut a1 = BTreeMap::new();
        a1.insert("db.sql".to_string(), artifact("hash-a", 1));
        let mut a2 = BTreeMap::new();
        a2.insert("db.sql".to_string(), artifact("hash-b", 1));
        assert_ne!(compute_snapshot_id(&a1), compute_snapshot_id(&a2));
    }

    #[test]
    fn sha256_hex_is_correct() {
        let hash = sha256_hex(b"hello");
        assert_eq!(hash, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[test]
    fn normalize_device_name_returns_none_for_blank_input() {
        assert_eq!(normalize_device_name("   \n\t  "), None);
    }

    #[test]
    fn normalize_device_name_collapses_whitespace() {
        assert_eq!(
            normalize_device_name("  Mac\tBook \n Pro\u{0007} "),
            Some("Mac Book Pro".to_string())
        );
    }

    #[test]
    fn normalize_device_name_truncates_to_max_len() {
        let long = "a".repeat(80);
        assert_eq!(normalize_device_name(&long).map(|s| s.len()), Some(64));
    }

    #[test]
    fn validate_manifest_compat_accepts_supported() {
        let mut artifacts = BTreeMap::new();
        artifacts.insert("db.sql".to_string(), artifact("abc", 1));
        let manifest = SyncManifest {
            format: PROTOCOL_FORMAT.to_string(),
            version: PROTOCOL_VERSION,
            device_name: "Test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            artifacts,
            snapshot_id: "snap".to_string(),
        };
        assert!(validate_manifest_compat(&manifest).is_ok());
    }

    #[test]
    fn validate_manifest_compat_rejects_wrong_format() {
        let manifest = SyncManifest {
            format: "other".to_string(),
            version: PROTOCOL_VERSION,
            device_name: "Test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            artifacts: BTreeMap::new(),
            snapshot_id: "snap".to_string(),
        };
        assert!(validate_manifest_compat(&manifest).is_err());
    }

    #[test]
    fn verify_artifact_rejects_size_mismatch() {
        let meta = artifact("abc", 100);
        let bytes = vec![0u8; 50];
        assert!(verify_artifact(&bytes, "test.bin", &meta).is_err());
    }

    #[test]
    fn verify_artifact_rejects_hash_mismatch() {
        let bytes = b"hello";
        let meta = ArtifactMeta {
            sha256: "wrong_hash".to_string(),
            size: bytes.len() as u64,
        };
        assert!(verify_artifact(bytes, "test.bin", &meta).is_err());
    }

    #[test]
    fn verify_artifact_accepts_correct_data() {
        let bytes = b"hello";
        let meta = ArtifactMeta {
            sha256: sha256_hex(bytes),
            size: bytes.len() as u64,
        };
        assert!(verify_artifact(bytes, "test.bin", &meta).is_ok());
    }

    #[test]
    fn validate_artifact_size_limit_rejects_oversized() {
        assert!(validate_artifact_size_limit("big.zip", MAX_SYNC_ARTIFACT_BYTES + 1).is_err());
    }

    #[test]
    fn validate_artifact_size_limit_accepts_boundary() {
        assert!(validate_artifact_size_limit("ok.zip", MAX_SYNC_ARTIFACT_BYTES).is_ok());
    }

    #[test]
    fn persist_best_effort_returns_true_on_success() {
        let mut dummy = 0u32;
        let ok = persist_sync_success_best_effort(
            &mut dummy,
            "hash".to_string(),
            Some("etag".to_string()),
            |_s, _h, _e| Ok(()),
        );
        assert!(ok);
    }

    #[test]
    fn persist_best_effort_returns_false_on_error() {
        let mut dummy = 0u32;
        let ok = persist_sync_success_best_effort(
            &mut dummy,
            "hash".to_string(),
            None,
            |_s, _h, _e| Err(AppError::Config("boom".to_string())),
        );
        assert!(!ok);
    }
}
```

**Step 2: Update `webdav_sync.rs` to use sync_protocol**

Replace the extracted code in `webdav_sync.rs` with imports from `sync_protocol`. The file should retain only WebDAV-specific logic (transport calls, remote path helpers, auth). Key changes:

- Remove all type definitions, constants, utility functions that are now in `sync_protocol.rs`
- Add `use super::sync_protocol::*;` at the top
- `build_local_snapshot` calls become `sync_protocol::build_local_snapshot(db)`
- `download_and_verify` now uses `sync_protocol::verify_artifact` instead of inline checks
- Existing tests that test sync_protocol logic should be removed (they live in sync_protocol now)
- Keep WebDAV-specific tests (remote_dir_segments, auth, etc.)

**Step 3: Register module in `services/mod.rs`**

Add after `pub mod webdav_sync;`:
```rust
pub mod sync_protocol;
```

**Step 4: Run tests**

Run: `cd /root/keith-space/github-search/cc-switch/src-tauri && cargo test --lib`
Expected: All existing tests pass, plus new sync_protocol tests pass.

**Step 5: Commit**

```bash
git add src-tauri/src/services/sync_protocol.rs src-tauri/src/services/webdav_sync.rs src-tauri/src/services/mod.rs
git commit -m "refactor: extract sync_protocol.rs from webdav_sync.rs for shared use"
```

---

## Task 3: Implement S3 transport layer (`s3.rs`)

**Files:**
- Create: `src-tauri/src/services/s3.rs`
- Modify: `src-tauri/src/services/mod.rs`

**Context:** This implements AWS Signature V4 signing and S3 HTTP operations using the existing `reqwest` client. Key references:
- AWS Sig V4 spec: `Authorization = AWS4-HMAC-SHA256 Credential=AKID/date/region/s3/aws4_request, SignedHeaders=..., Signature=...`
- The existing `webdav.rs` pattern (`src-tauri/src/services/webdav.rs`) for transport function structure
- Uses `crate::proxy::http_client::get()` for the reqwest client

**Step 1: Create `s3.rs` with Sig V4 signing + transport functions**

Create `src-tauri/src/services/s3.rs`. The file should contain:

1. `S3Credentials` struct
2. `resolve_url(creds, key)` — builds full URL with path-style or virtual-hosted detection
3. `sign_request(method, url, headers, body_hash, creds, now)` — AWS Sig V4 signing
4. `hmac_sha256(key, data)` — HMAC helper using `hmac` crate
5. `test_connection(creds)` — HEAD bucket
6. `put_object(creds, key, bytes, content_type)` — PUT object
7. `get_object(creds, key, max_bytes)` — GET object, returns `Option<(Vec<u8>, Option<String>)>`
8. `head_object(creds, key)` — HEAD object, returns `Option<String>` (ETag)
9. Unit tests for signing (use AWS test vectors), URL construction, endpoint detection

**Key implementation detail for signing:**
```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;
type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}
```

**Endpoint auto-detection:**
```rust
fn is_virtual_hosted(endpoint: &str) -> bool {
    endpoint.is_empty() || endpoint.contains("amazonaws.com")
}
```

Timeouts: `30s` default, `300s` for PUT/GET transfers (matching `webdav.rs` constants).

**Step 2: Register module**

In `src-tauri/src/services/mod.rs`, add:
```rust
pub mod s3;
```

**Step 3: Run tests**

Run: `cd /root/keith-space/github-search/cc-switch/src-tauri && cargo test services::s3 --lib -v`
Expected: Sig V4 signing tests pass, URL construction tests pass.

**Step 4: Commit**

```bash
git add src-tauri/src/services/s3.rs src-tauri/src/services/mod.rs
git commit -m "feat: add S3 transport layer with AWS Sig V4 signing"
```

---

## Task 4: Add S3SyncSettings to `settings.rs`

**Files:**
- Modify: `src-tauri/src/settings.rs`

**Context:** Read current `settings.rs` (line 66-703). The `WebDavSyncStatus` struct (line 67-82) and `WebDavSyncSettings` (line 91-164) are the patterns to follow.

**Step 1: Add `S3SyncSettings` struct**

After `WebDavSyncSettings` impl block (~line 164), add:

```rust
/// S3 同步设置
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
    pub status: WebDavSyncStatus,
}

impl Default for S3SyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_sync: false,
            region: String::new(),
            bucket: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            endpoint: String::new(),
            remote_root: default_remote_root(),
            profile: default_profile(),
            status: WebDavSyncStatus::default(),
        }
    }
}

impl S3SyncSettings {
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        if self.bucket.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.bucket.required",
                "S3 Bucket 不能为空",
                "S3 Bucket is required.",
            ));
        }
        if self.region.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.region.required",
                "S3 Region 不能为空",
                "S3 Region is required.",
            ));
        }
        if self.access_key_id.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.access_key_id.required",
                "Access Key ID 不能为空",
                "Access Key ID is required.",
            ));
        }
        if self.secret_access_key.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.secret_access_key.required",
                "Secret Access Key 不能为空",
                "Secret Access Key is required.",
            ));
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.region = self.region.trim().to_string();
        self.bucket = self.bucket.trim().to_string();
        self.access_key_id = self.access_key_id.trim().to_string();
        self.endpoint = self.endpoint.trim().to_string();
        self.remote_root = self.remote_root.trim().to_string();
        self.profile = self.profile.trim().to_string();
        if self.remote_root.is_empty() {
            self.remote_root = default_remote_root();
        }
        if self.profile.is_empty() {
            self.profile = default_profile();
        }
    }

    fn is_empty(&self) -> bool {
        self.bucket.is_empty()
            && self.access_key_id.is_empty()
            && self.secret_access_key.is_empty()
    }
}
```

**Step 2: Add `s3_sync` field to `AppSettings`**

After `webdav_sync` field (~line 245), add:

```rust
    // ===== S3 同步设置 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3_sync: Option<S3SyncSettings>,
```

**Step 3: Update `Default` for `AppSettings`**

In the `Default` impl (~line 307), add after `webdav_backup: None,`:

```rust
            s3_sync: None,
```

**Step 4: Update `normalize_paths`**

After the webdav_sync normalization block (~line 369), add:

```rust
        if let Some(s3) = &mut self.s3_sync {
            s3.normalize();
            if s3.is_empty() {
                self.s3_sync = None;
            }
        }
```

**Step 5: Update `get_settings_for_frontend`**

After clearing webdav password (~line 472), add:

```rust
    if let Some(s3) = &mut settings.s3_sync {
        s3.secret_access_key.clear();
    }
```

**Step 6: Add S3 settings management functions**

After the WebDAV functions section (~line 702), add:

```rust
// ===== S3 同步设置管理函数 =====

pub fn get_s3_sync_settings() -> Option<S3SyncSettings> {
    settings_store().read().ok()?.s3_sync.clone()
}

pub fn set_s3_sync_settings(settings: Option<S3SyncSettings>) -> Result<(), AppError> {
    mutate_settings(|current| {
        current.s3_sync = settings;
    })
}

pub fn update_s3_sync_status(status: WebDavSyncStatus) -> Result<(), AppError> {
    mutate_settings(|current| {
        if let Some(s3) = current.s3_sync.as_mut() {
            s3.status = status;
        }
    })
}
```

**Step 7: Run tests**

Run: `cd /root/keith-space/github-search/cc-switch/src-tauri && cargo test --lib 2>&1 | tail -10`

**Step 8: Commit**

```bash
git add src-tauri/src/settings.rs
git commit -m "feat: add S3SyncSettings to AppSettings"
```

---

## Task 5: Implement S3 sync module (`s3_sync.rs`)

**Files:**
- Create: `src-tauri/src/services/s3_sync.rs`
- Modify: `src-tauri/src/services/mod.rs`

**Context:** Mirror the structure of `webdav_sync.rs` (read it at `src-tauri/src/services/webdav_sync.rs`). Uses `sync_protocol` for shared logic and `s3` for transport.

**Step 1: Create `s3_sync.rs`**

The file implements:
- `sync_mutex()` — independent tokio Mutex
- `run_with_sync_lock(op)` — acquire lock before running
- `check_connection(settings)` — calls `s3::test_connection`
- `upload(db, settings)` — builds snapshot, puts db.sql + skills.zip + manifest.json
- `download(db, settings)` — gets manifest, validates, downloads artifacts, applies snapshot
- `fetch_remote_info(settings)` — downloads manifest only

Key helper: `s3_key(settings, artifact)` builds the S3 key path: `{remote_root}/v2/{profile}/{artifact}`

Construct `S3Credentials` from `S3SyncSettings` for each operation.

**Step 2: Register module**

In `services/mod.rs`, add:
```rust
pub mod s3_sync;
```

**Step 3: Run tests**

Run: `cd /root/keith-space/github-search/cc-switch/src-tauri && cargo test services::s3_sync --lib -v`

**Step 4: Commit**

```bash
git add src-tauri/src/services/s3_sync.rs src-tauri/src/services/mod.rs
git commit -m "feat: add S3 sync module with upload/download/fetch"
```

---

## Task 6: Implement S3 auto sync (`s3_auto_sync.rs`)

**Files:**
- Create: `src-tauri/src/services/s3_auto_sync.rs`
- Modify: `src-tauri/src/services/mod.rs`

**Context:** Mirror `webdav_auto_sync.rs` (read it at `src-tauri/src/services/webdav_auto_sync.rs`). Independent channel, debounce, suppression guard.

**Step 1: Create `s3_auto_sync.rs`**

Structure:
- Independent `DB_CHANGE_TX: OnceLock<Sender<String>>`
- Independent `AUTO_SYNC_SUPPRESS_DEPTH: AtomicUsize`
- `AutoSyncSuppressionGuard` (same pattern)
- `should_trigger_for_table(table)` — same logic as WebDAV (duplicate for independence)
- `notify_db_changed(table)` — enqueue signal
- `start_worker(db, app)` — spawn async worker loop
- Worker emits `s3-sync-status-updated` event
- Uses `s3_sync::run_with_sync_lock` and `s3_sync::upload`

**Step 2: Register module**

In `services/mod.rs`, add:
```rust
pub mod s3_auto_sync;
```

**Step 3: Run tests**

Run: `cd /root/keith-space/github-search/cc-switch/src-tauri && cargo test services::s3_auto_sync --lib -v`

**Step 4: Commit**

```bash
git add src-tauri/src/services/s3_auto_sync.rs src-tauri/src/services/mod.rs
git commit -m "feat: add S3 auto sync worker with debounce"
```

---

## Task 7: Implement S3 Tauri commands (`commands/s3_sync.rs`)

**Files:**
- Create: `src-tauri/src/commands/s3_sync.rs`
- Modify: `src-tauri/src/commands/mod.rs`

**Context:** Mirror `commands/webdav_sync.rs` (read it at `src-tauri/src/commands/webdav_sync.rs`).

**Step 1: Create `commands/s3_sync.rs`**

Implement 5 Tauri commands:
- `s3_test_connection(settings, preserveEmptyPassword)` — resolve secret, call check_connection
- `s3_sync_upload(state)` — require enabled, lock, upload
- `s3_sync_download(state)` — require enabled, lock, download, post-import sync
- `s3_sync_save_settings(settings, passwordTouched)` — resolve secret, validate, persist
- `s3_sync_fetch_remote_info()` — require enabled, fetch remote info

Use `resolve_secret_for_request` pattern (like `resolve_password_for_request` in webdav_sync).

**Step 2: Register in `commands/mod.rs`**

Add after `mod webdav_sync;`:
```rust
mod s3_sync;
```

Add after `pub use webdav_sync::*;`:
```rust
pub use s3_sync::*;
```

**Step 3: Register commands in `lib.rs`**

In the `invoke_handler` block (~line 941-945), add after `webdav_sync_fetch_remote_info`:
```rust
            commands::s3_test_connection,
            commands::s3_sync_upload,
            commands::s3_sync_download,
            commands::s3_sync_save_settings,
            commands::s3_sync_fetch_remote_info,
```

**Step 4: Start S3 auto sync worker in `lib.rs`**

After the WebDAV auto sync start (~line 720-723), add:
```rust
            crate::services::s3_auto_sync::start_worker(
                app_state.db.clone(),
                app.handle().clone(),
            );
```

**Step 5: Run tests**

Run: `cd /root/keith-space/github-search/cc-switch/src-tauri && cargo test --lib 2>&1 | tail -10`

**Step 6: Commit**

```bash
git add src-tauri/src/commands/s3_sync.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat: add S3 sync Tauri commands and auto sync worker startup"
```

---

## Task 8: Frontend — TypeScript types and API layer

**Files:**
- Modify: `src/types.ts`
- Modify: `src/lib/api/settings.ts`

**Context:** Read `src/types.ts` lines 168-198 for WebDAV types, and `src/lib/api/settings.ts` for the API pattern.

**Step 1: Add S3 types to `types.ts`**

After `WebDavSyncSettings` interface (~line 188), add:

```typescript
// S3 同步配置
export interface S3SyncSettings {
  enabled?: boolean;
  autoSync?: boolean;
  region?: string;
  bucket?: string;
  accessKeyId?: string;
  secretAccessKey?: string;
  endpoint?: string;
  remoteRoot?: string;
  profile?: string;
  status?: WebDavSyncStatus;
}
```

In the `Settings` interface (~line 255), add after `webdavSync`:

```typescript
  // ===== S3 同步设置 =====
  s3Sync?: S3SyncSettings;
```

**Step 2: Add S3 API methods to `settings.ts`**

In `src/lib/api/settings.ts`, add after the WebDAV methods:

```typescript
  // ===== S3 Sync API =====

  async s3TestConnection(
    settings: S3SyncSettings,
    preserveEmptyPassword = true,
  ): Promise<WebDavTestResult> {
    return await invoke("s3_test_connection", {
      settings,
      preserveEmptyPassword,
    });
  },

  async s3SyncUpload(): Promise<WebDavSyncResult> {
    return await invoke("s3_sync_upload");
  },

  async s3SyncDownload(): Promise<WebDavSyncResult> {
    return await invoke("s3_sync_download");
  },

  async s3SyncSaveSettings(
    settings: S3SyncSettings,
    passwordTouched: boolean,
  ): Promise<{ success: boolean }> {
    return await invoke("s3_sync_save_settings", {
      settings,
      passwordTouched,
    });
  },

  async s3SyncFetchRemoteInfo(): Promise<
    RemoteSnapshotInfo | { empty: true }
  > {
    return await invoke("s3_sync_fetch_remote_info");
  },
```

Also update the import line to include `S3SyncSettings`:
```typescript
import type { Settings, WebDavSyncSettings, S3SyncSettings, RemoteSnapshotInfo } from "@/types";
```

**Step 3: Commit**

```bash
git add src/types.ts src/lib/api/settings.ts
git commit -m "feat: add S3 sync TypeScript types and API layer"
```

---

## Task 9: Frontend — S3 presets and dynamic form in WebdavSyncSection

**Files:**
- Modify: `src/components/settings/WebdavSyncSection.tsx`

**Context:** Read `WebdavSyncSection.tsx` (820 lines). It has WEBDAV_PRESETS, a form with baseUrl/username/password/remoteRoot/profile, and test/upload/download buttons.

**Step 1: Add S3 presets**

After `WEBDAV_PRESETS` array (~line 76), add:

```typescript
interface S3Preset {
  id: string;
  label: string;
  hint: string;
  defaultEndpoint?: string;
  regionPlaceholder?: string;
}

const S3_PRESETS: S3Preset[] = [
  {
    id: "aws-s3",
    label: "settings.s3Sync.presets.awsS3",
    hint: "settings.s3Sync.presets.awsS3Hint",
    regionPlaceholder: "us-east-1",
  },
  {
    id: "s3-minio",
    label: "settings.s3Sync.presets.minio",
    hint: "settings.s3Sync.presets.minioHint",
    regionPlaceholder: "us-east-1",
  },
  {
    id: "s3-r2",
    label: "settings.s3Sync.presets.r2",
    hint: "settings.s3Sync.presets.r2Hint",
    regionPlaceholder: "auto",
  },
  {
    id: "s3-oss",
    label: "settings.s3Sync.presets.oss",
    hint: "settings.s3Sync.presets.ossHint",
    regionPlaceholder: "cn-hangzhou",
  },
  {
    id: "s3-cos",
    label: "settings.s3Sync.presets.cos",
    hint: "settings.s3Sync.presets.cosHint",
    regionPlaceholder: "ap-guangzhou",
  },
  {
    id: "s3-obs",
    label: "settings.s3Sync.presets.obs",
    hint: "settings.s3Sync.presets.obsHint",
    regionPlaceholder: "cn-north-4",
  },
  {
    id: "s3-custom",
    label: "settings.s3Sync.presets.custom",
    hint: "settings.s3Sync.presets.customHint",
    regionPlaceholder: "us-east-1",
  },
];

function isS3Preset(presetId: string): boolean {
  return presetId.startsWith("s3-") || presetId === "aws-s3";
}
```

**Step 2: Add sync type selector and dynamic form**

The component needs a top-level "Sync Type" selector (WebDAV vs S3). When S3 is selected, show S3-specific fields instead of WebDAV fields. The general flow:

1. Add a `syncType` state: `"webdav" | "s3"` — derived from current settings
2. Add a `<Select>` at the top for sync type (before preset selector)
3. When `syncType === "s3"`, show S3 preset selector + S3 form fields (region, bucket, access key, secret key, endpoint)
4. When `syncType === "webdav"`, show existing WebDAV UI
5. Connect S3 form to `settingsApi.s3TestConnection`, `s3SyncUpload`, etc.
6. Add mutual exclusion confirmation dialog

**Step 3: Commit**

```bash
git add src/components/settings/WebdavSyncSection.tsx
git commit -m "feat: add S3 sync presets and dynamic form to sync settings"
```

---

## Task 10: i18n translations

**Files:**
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/ja.json`

**Step 1: Add S3 translation keys**

Add under `settings` in all three locale files:

Keys to add:
```
settings.syncType.label
settings.syncType.webdav
settings.syncType.s3
settings.s3Sync.presets.awsS3
settings.s3Sync.presets.awsS3Hint
settings.s3Sync.presets.minio
settings.s3Sync.presets.minioHint
settings.s3Sync.presets.r2
settings.s3Sync.presets.r2Hint
settings.s3Sync.presets.oss
settings.s3Sync.presets.ossHint
settings.s3Sync.presets.cos
settings.s3Sync.presets.cosHint
settings.s3Sync.presets.obs
settings.s3Sync.presets.obsHint
settings.s3Sync.presets.custom
settings.s3Sync.presets.customHint
settings.s3Sync.region
settings.s3Sync.regionPlaceholder
settings.s3Sync.bucket
settings.s3Sync.bucketPlaceholder
settings.s3Sync.accessKeyId
settings.s3Sync.secretAccessKey
settings.s3Sync.endpoint
settings.s3Sync.endpointPlaceholder
settings.s3Sync.mutualExclusionTitle
settings.s3Sync.mutualExclusionMessage
settings.s3Sync.testConnection
settings.s3Sync.connectionOk
settings.s3Sync.upload
settings.s3Sync.download
settings.s3Sync.save
settings.s3Sync.notConfigured
settings.s3Sync.disabled
```

**Step 2: Commit**

```bash
git add src/i18n/locales/
git commit -m "feat: add S3 sync i18n translations (en/zh/ja)"
```

---

## Task 11: Integration verification

**Step 1: Full Rust compilation check**

Run: `cd /root/keith-space/github-search/cc-switch/src-tauri && cargo check 2>&1 | tail -10`

**Step 2: Run all Rust tests**

Run: `cd /root/keith-space/github-search/cc-switch/src-tauri && cargo test --lib 2>&1 | tail -20`

**Step 3: TypeScript type check**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm typecheck 2>&1 | tail -10`
(Skip if pnpm/node not available)

**Step 4: Final commit if any fixups needed**

```bash
git add -A
git commit -m "fix: integration fixups for S3 cloud sync"
```
