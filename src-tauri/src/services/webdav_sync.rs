//! WebDAV sync protocol layer with DB compatibility subdirectories.
//!
//! Implements manifest-based synchronization on top of the HTTP transport
//! primitives in [`super::webdav`]. The current v3 protocol stores module-
//! scoped SQL artifacts plus `skills.zip`, while v2 compatibility is preserved
//! for reading legacy snapshots.

use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::sync::OnceLock;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::tempdir;

use crate::error::AppError;
use crate::services::webdav::{
    auth_from_credentials, build_remote_url, ensure_remote_directories, get_bytes, head_etag,
    path_segments, put_bytes, test_connection, WebDavAuth,
};
use crate::settings::{
    update_webdav_sync_status, WebDavSyncModules, WebDavSyncSettings, WebDavSyncStatus,
};

use super::sync_protocol::{
    io_context_localized, localized, sha256_hex,
    detect_system_device_name,
    verify_artifact, validate_artifact_size_limit,
    ArtifactMeta, RemoteLayout,
    MAX_MANIFEST_BYTES, MAX_SYNC_ARTIFACT_BYTES,
    REMOTE_DB_SQL, REMOTE_SKILLS_ZIP, REMOTE_MANIFEST,
};
#[cfg(test)]
use super::sync_protocol::normalize_device_name;

pub(crate) mod archive;

use archive::{backup_current_skills, restore_skills_from_backup, restore_skills_zip, zip_skills_ssot};

const PROTOCOL_FORMAT: &str = "cc-switch-webdav-sync";
const PROTOCOL_VERSION: u32 = 3;
const LEGACY_PROTOCOL_VERSION: u32 = 2;
/// Must stay in sync with sync_protocol::DB_COMPAT_VERSION when DB schema changes.
const DB_COMPAT_VERSION: u32 = 6;
const LEGACY_DB_COMPAT_VERSION: u32 = 5;
const REMOTE_API_SQL: &str = "api.sql";
const REMOTE_MCP_SQL: &str = "mcp.sql";
const REMOTE_PROMPTS_SQL: &str = "prompts.sql";
const REMOTE_SKILLS_SQL: &str = "skills.sql";

const API_TABLES: &[&str] = &[
    "providers",
    "provider_endpoints",
    "model_pricing",
    "settings",
    "proxy_config",
    "session_log_sync",
];
const MCP_TABLES: &[&str] = &["mcp_servers"];
const PROMPT_TABLES: &[&str] = &["prompts"];
const SKILL_TABLES: &[&str] = &["skills", "skill_repos"];

pub fn sync_mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub async fn run_with_sync_lock<T, Fut>(operation: Fut) -> Result<T, AppError>
where
    Fut: Future<Output = Result<T, AppError>>,
{
    let _guard = sync_mutex().lock().await;
    operation.await
}


// ─── Types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncManifest {
    format: String,
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    db_compat_version: Option<u32>,
    device_name: String,
    created_at: String,
    artifacts: BTreeMap<String, ArtifactMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modules: Option<ManifestModules>,
    snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestModules {
    api: Vec<String>,
    mcp: Vec<String>,
    prompts: Vec<String>,
    skills: Vec<String>,
}

struct LocalSnapshot {
    artifacts: BTreeMap<String, Vec<u8>>,
    manifest_bytes: Vec<u8>,
    manifest_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RemoteLocation {
    protocol_version: u32,
    layout: RemoteLayout,
}

impl RemoteLocation {
    fn v3_current() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            layout: RemoteLayout::Current,
        }
    }

    fn v2_current() -> Self {
        Self {
            protocol_version: LEGACY_PROTOCOL_VERSION,
            layout: RemoteLayout::Current,
        }
    }

    fn v2_legacy() -> Self {
        Self {
            protocol_version: LEGACY_PROTOCOL_VERSION,
            layout: RemoteLayout::Legacy,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ModulePayloads {
    api_sql: Option<Vec<u8>>,
    mcp_sql: Option<Vec<u8>>,
    prompts_sql: Option<Vec<u8>>,
    skills_sql: Option<Vec<u8>>,
    skills_zip: Option<Vec<u8>>,
}

impl ModulePayloads {
    fn available_modules(&self) -> WebDavSyncModules {
        WebDavSyncModules {
            api: self.api_sql.is_some(),
            mcp: self.mcp_sql.is_some(),
            prompts: self.prompts_sql.is_some(),
            skills: self.skills_sql.is_some() && self.skills_zip.is_some(),
        }
    }

    fn is_empty(&self) -> bool {
        self.api_sql.is_none()
            && self.mcp_sql.is_none()
            && self.prompts_sql.is_none()
            && self.skills_sql.is_none()
            && self.skills_zip.is_none()
    }

    fn to_manifest_modules(&self) -> ManifestModules {
        ManifestModules {
            api: module_artifact_names(self.api_sql.as_ref(), None),
            mcp: module_artifact_names(self.mcp_sql.as_ref(), Some(REMOTE_MCP_SQL)),
            prompts: module_artifact_names(self.prompts_sql.as_ref(), Some(REMOTE_PROMPTS_SQL)),
            skills: {
                let mut names = Vec::new();
                if self.skills_sql.is_some() {
                    names.push(REMOTE_SKILLS_SQL.to_string());
                }
                if self.skills_zip.is_some() {
                    names.push(REMOTE_SKILLS_ZIP.to_string());
                }
                names
            },
        }
    }

    fn artifacts_map(&self) -> BTreeMap<String, Vec<u8>> {
        let mut artifacts = BTreeMap::new();
        if let Some(bytes) = self.api_sql.clone() {
            artifacts.insert(REMOTE_API_SQL.to_string(), bytes);
        }
        if let Some(bytes) = self.mcp_sql.clone() {
            artifacts.insert(REMOTE_MCP_SQL.to_string(), bytes);
        }
        if let Some(bytes) = self.prompts_sql.clone() {
            artifacts.insert(REMOTE_PROMPTS_SQL.to_string(), bytes);
        }
        if let Some(bytes) = self.skills_sql.clone() {
            artifacts.insert(REMOTE_SKILLS_SQL.to_string(), bytes);
        }
        if let Some(bytes) = self.skills_zip.clone() {
            artifacts.insert(REMOTE_SKILLS_ZIP.to_string(), bytes);
        }
        artifacts
    }
}

fn module_artifact_names(sql: Option<&Vec<u8>>, sql_name: Option<&str>) -> Vec<String> {
    let Some(name) = sql_name else {
        return if sql.is_some() {
            vec![REMOTE_API_SQL.to_string()]
        } else {
            Vec::new()
        };
    };
    if sql.is_some() {
        vec![name.to_string()]
    } else {
        Vec::new()
    }
}

struct RemoteSnapshot {
    location: RemoteLocation,
    manifest: SyncManifest,
    manifest_bytes: Vec<u8>,
    manifest_etag: Option<String>,
}
// ─── Public API ──────────────────────────────────────────────

/// Check WebDAV connectivity and ensure remote directory structure.
pub async fn check_connection(settings: &WebDavSyncSettings) -> Result<(), AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    test_connection(&settings.base_url, &auth).await?;
    let dir_segs = remote_dir_segments(settings, RemoteLocation::v3_current());
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;
    Ok(())
}

/// Upload local module snapshot to remote.
pub async fn upload(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Value, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let location = RemoteLocation::v3_current();
    let dir_segs = remote_dir_segments(settings, location);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;

    let local_payloads = build_local_module_payloads(db)?;
    let remote_payloads = match find_remote_snapshot(settings, &auth).await? {
        Some(snapshot) => Some(load_remote_module_payloads(settings, &auth, &snapshot).await?),
        None => None,
    };
    let final_payloads = merge_module_payloads(
        &local_payloads,
        remote_payloads.as_ref(),
        &settings.upload_modules,
    );
    if final_payloads.is_empty() {
        return Err(localized(
            "webdav.sync.no_artifacts_selected",
            "没有可上传的同步模块数据",
            "No selected sync module has data to upload.",
        ));
    }

    let snapshot = build_local_snapshot_from_payloads(&final_payloads)?;

    // Upload order: artifacts first, manifest last (best-effort consistency)
    for (artifact_name, bytes) in snapshot.artifacts {
        let artifact_url = remote_file_url(settings, location, &artifact_name)?;
        let content_type = if artifact_name.ends_with(".zip") {
            "application/zip"
        } else if artifact_name.ends_with(".json") {
            "application/json"
        } else {
            "application/sql"
        };
        put_bytes(&artifact_url, &auth, bytes, content_type).await?;
    }

    let manifest_url = remote_file_url(settings, location, REMOTE_MANIFEST)?;
    put_bytes(
        &manifest_url,
        &auth,
        snapshot.manifest_bytes,
        "application/json",
    )
    .await?;

    // Fetch etag (best-effort, don't fail the upload)
    let etag = match head_etag(&manifest_url, &auth).await {
        Ok(e) => e,
        Err(e) => {
            log::debug!("[WebDAV] Failed to fetch ETag after upload: {e}");
            None
        }
    };

    let _persisted = persist_sync_success_best_effort(
        settings,
        snapshot.manifest_hash,
        etag,
        persist_sync_success,
    );
    Ok(serde_json::json!({ "status": "uploaded" }))
}

/// Download remote snapshot and apply only the selected modules.
pub async fn download(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Value, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let snapshot = find_remote_snapshot(settings, &auth)
        .await?
        .ok_or_else(|| {
            localized(
                "webdav.sync.remote_empty",
                "远端没有可下载的同步数据",
                "No downloadable sync data found on the remote.",
            )
        })?;

    validate_manifest_compat(&snapshot.manifest, snapshot.location)?;

    let payloads = load_remote_module_payloads(settings, &auth, &snapshot).await?;
    ensure_requested_modules_available(&payloads, &settings.download_modules)?;
    apply_selected_modules(db, &payloads, &settings.download_modules)?;

    let manifest_hash = sha256_hex(&snapshot.manifest_bytes);
    let _persisted = persist_sync_success_best_effort(
        settings,
        manifest_hash,
        snapshot.manifest_etag,
        persist_sync_success,
    );
    Ok(serde_json::json!({
        "status": "downloaded",
        "sourceLayout": snapshot.location.layout.as_str(),
        "sourcePath": remote_dir_display(settings, snapshot.location),
    }))
}

/// Fetch remote manifest info without downloading artifacts.
pub async fn fetch_remote_info(settings: &WebDavSyncSettings) -> Result<Option<Value>, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let Some(snapshot) = find_remote_snapshot(settings, &auth).await? else {
        return Ok(None);
    };
    let compatible = validate_manifest_compat(&snapshot.manifest, snapshot.location).is_ok();
    let db_compat_version = effective_db_compat_version(&snapshot.manifest, snapshot.location);
    let available_modules = available_modules_from_manifest(&snapshot.manifest, snapshot.location);

    let payload = serde_json::json!({
        "deviceName": snapshot.manifest.device_name,
        "createdAt": snapshot.manifest.created_at,
        "snapshotId": snapshot.manifest.snapshot_id,
        "version": snapshot.manifest.version,
        "protocolVersion": snapshot.manifest.version,
        "dbCompatVersion": db_compat_version,
        "compatible": compatible,
        "artifacts": snapshot.manifest.artifacts.keys().collect::<Vec<_>>(),
        "layout": snapshot.location.layout.as_str(),
        "remotePath": remote_dir_display(settings, snapshot.location),
        "availableModules": {
            "api": available_modules.api,
            "mcp": available_modules.mcp,
            "prompts": available_modules.prompts,
            "skills": available_modules.skills,
        }
    });

    Ok(Some(payload))
}

// ─── Sync status persistence ─────────────────────────────────

fn persist_sync_success(
    settings: &mut WebDavSyncSettings,
    manifest_hash: String,
    etag: Option<String>,
) -> Result<(), AppError> {
    let status = WebDavSyncStatus {
        last_sync_at: Some(Utc::now().timestamp()),
        last_error: None,
        last_error_source: None,
        last_local_manifest_hash: Some(manifest_hash.clone()),
        last_remote_manifest_hash: Some(manifest_hash),
        last_remote_etag: etag,
    };
    settings.status = status.clone();
    update_webdav_sync_status(status)
}

fn persist_sync_success_best_effort<F>(
    settings: &mut WebDavSyncSettings,
    manifest_hash: String,
    etag: Option<String>,
    persist_fn: F,
) -> bool
where
    F: FnOnce(&mut WebDavSyncSettings, String, Option<String>) -> Result<(), AppError>,
{
    match persist_fn(settings, manifest_hash, etag) {
        Ok(()) => true,
        Err(err) => {
            log::warn!("[WebDAV] Persist sync status failed, keep operation success: {err}");
            false
        }
    }
}

// ─── Snapshot building ───────────────────────────────────────

fn build_skills_zip() -> Result<Vec<u8>, AppError> {
    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "webdav.sync.snapshot_tmpdir_failed",
            "创建 WebDAV 快照临时目录失败",
            "Failed to create temporary directory for WebDAV snapshot",
            e,
        )
    })?;
    let skills_zip_path = tmp.path().join(REMOTE_SKILLS_ZIP);
    zip_skills_ssot(&skills_zip_path)?;
    fs::read(&skills_zip_path).map_err(|e| AppError::io(&skills_zip_path, e))
}

fn build_local_module_payloads(db: &crate::database::Database) -> Result<ModulePayloads, AppError> {
    Ok(ModulePayloads {
        api_sql: Some(db.export_sql_string_for_tables(API_TABLES)?.into_bytes()),
        mcp_sql: Some(db.export_sql_string_for_tables(MCP_TABLES)?.into_bytes()),
        prompts_sql: Some(db.export_sql_string_for_tables(PROMPT_TABLES)?.into_bytes()),
        skills_sql: Some(db.export_sql_string_for_tables(SKILL_TABLES)?.into_bytes()),
        skills_zip: Some(build_skills_zip()?),
    })
}

fn merge_module_payloads(
    local_payloads: &ModulePayloads,
    remote_payloads: Option<&ModulePayloads>,
    selection: &WebDavSyncModules,
) -> ModulePayloads {
    ModulePayloads {
        api_sql: if selection.api {
            local_payloads.api_sql.clone()
        } else {
            remote_payloads.and_then(|payloads| payloads.api_sql.clone())
        },
        mcp_sql: if selection.mcp {
            local_payloads.mcp_sql.clone()
        } else {
            remote_payloads.and_then(|payloads| payloads.mcp_sql.clone())
        },
        prompts_sql: if selection.prompts {
            local_payloads.prompts_sql.clone()
        } else {
            remote_payloads.and_then(|payloads| payloads.prompts_sql.clone())
        },
        skills_sql: if selection.skills {
            local_payloads.skills_sql.clone()
        } else {
            remote_payloads.and_then(|payloads| payloads.skills_sql.clone())
        },
        skills_zip: if selection.skills {
            local_payloads.skills_zip.clone()
        } else {
            remote_payloads.and_then(|payloads| payloads.skills_zip.clone())
        },
    }
}

fn build_local_snapshot_from_payloads(
    payloads: &ModulePayloads,
) -> Result<LocalSnapshot, AppError> {
    let artifacts_bytes = payloads.artifacts_map();
    let mut artifacts = BTreeMap::new();
    for (name, bytes) in &artifacts_bytes {
        artifacts.insert(
            name.clone(),
            ArtifactMeta {
                sha256: sha256_hex(bytes),
                size: bytes.len() as u64,
            },
        );
    }

    let snapshot_id = compute_snapshot_id(&artifacts);
    let manifest = SyncManifest {
        format: PROTOCOL_FORMAT.to_string(),
        version: PROTOCOL_VERSION,
        db_compat_version: Some(DB_COMPAT_VERSION),
        device_name: detect_system_device_name().unwrap_or_else(|| "Unknown Device".to_string()),
        created_at: Utc::now().to_rfc3339(),
        artifacts,
        modules: Some(payloads.to_manifest_modules()),
        snapshot_id,
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| AppError::JsonSerialize { source: e })?;
    let manifest_hash = sha256_hex(&manifest_bytes);

    Ok(LocalSnapshot {
        artifacts: artifacts_bytes,
        manifest_bytes,
        manifest_hash,
    })
}

/// Compute a deterministic snapshot identity from artifact hashes.
///
/// BTreeMap iteration order is sorted by key, ensuring stability.
fn compute_snapshot_id(artifacts: &BTreeMap<String, ArtifactMeta>) -> String {
    let parts: Vec<String> = artifacts
        .iter()
        .map(|(name, meta)| format!("{}:{}", name, meta.sha256))
        .collect();
    sha256_hex(parts.join("|").as_bytes())
}

fn effective_db_compat_version(manifest: &SyncManifest, location: RemoteLocation) -> Option<u32> {
    manifest.db_compat_version.or_else(|| {
        (location.protocol_version == LEGACY_PROTOCOL_VERSION
            && location.layout == RemoteLayout::Legacy)
            .then_some(LEGACY_DB_COMPAT_VERSION)
    })
}

fn validate_manifest_compat(
    manifest: &SyncManifest,
    location: RemoteLocation,
) -> Result<(), AppError> {
    if manifest.format != PROTOCOL_FORMAT {
        return Err(localized(
            "webdav.sync.manifest_format_incompatible",
            format!("远端 manifest 格式不兼容: {}", manifest.format),
            format!(
                "Remote manifest format is incompatible: {}",
                manifest.format
            ),
        ));
    }
    if manifest.version != location.protocol_version {
        return Err(localized(
            "webdav.sync.manifest_version_incompatible",
            format!(
                "远端 manifest 协议版本不兼容: v{} (当前路径期望 v{})",
                manifest.version, location.protocol_version
            ),
            format!(
                "Remote manifest protocol version is incompatible: v{} (expected v{} for this path)",
                manifest.version, location.protocol_version
            ),
        ));
    }
    if location.protocol_version == PROTOCOL_VERSION && manifest.modules.is_none() {
        return Err(localized(
            "webdav.sync.manifest_modules_missing",
            "远端 v3 manifest 缺少 modules 字段",
            "Remote v3 manifest is missing the modules field.",
        ));
    }
    let Some(db_compat_version) = effective_db_compat_version(manifest, location) else {
        return Err(localized(
            "webdav.sync.manifest_db_version_missing",
            "远端 manifest 缺少数据库兼容版本",
            "Remote manifest is missing the database compatibility version.",
        ));
    };
    match location.layout {
        RemoteLayout::Current if db_compat_version != DB_COMPAT_VERSION => {
            return Err(localized(
                "webdav.sync.manifest_db_version_incompatible",
                format!(
                    "远端数据库快照版本不兼容: db-v{db_compat_version} (本地 db-v{DB_COMPAT_VERSION})"
                ),
                format!(
                    "Remote database snapshot version is incompatible: db-v{db_compat_version} (local db-v{DB_COMPAT_VERSION})"
                ),
            ));
        }
        RemoteLayout::Legacy if db_compat_version > DB_COMPAT_VERSION => {
            return Err(localized(
                "webdav.sync.manifest_db_version_incompatible",
                format!(
                    "远端数据库快照版本不兼容: db-v{db_compat_version} (本地最高支持 db-v{DB_COMPAT_VERSION})"
                ),
                format!(
                    "Remote database snapshot version is incompatible: db-v{db_compat_version} (local supports up to db-v{DB_COMPAT_VERSION})"
                ),
            ));
        }
        _ => {}
    }
    Ok(())
}

fn available_modules_from_manifest(
    manifest: &SyncManifest,
    location: RemoteLocation,
) -> WebDavSyncModules {
    if location.protocol_version == LEGACY_PROTOCOL_VERSION {
        return WebDavSyncModules {
            api: true,
            mcp: true,
            prompts: true,
            skills: true,
        };
    }

    if let Some(modules) = &manifest.modules {
        return WebDavSyncModules {
            api: modules.api.iter().any(|name| name == REMOTE_API_SQL),
            mcp: modules.mcp.iter().any(|name| name == REMOTE_MCP_SQL),
            prompts: modules
                .prompts
                .iter()
                .any(|name| name == REMOTE_PROMPTS_SQL),
            skills: modules.skills.iter().any(|name| name == REMOTE_SKILLS_SQL)
                && modules.skills.iter().any(|name| name == REMOTE_SKILLS_ZIP),
        };
    }

    WebDavSyncModules {
        api: manifest.artifacts.contains_key(REMOTE_API_SQL),
        mcp: manifest.artifacts.contains_key(REMOTE_MCP_SQL),
        prompts: manifest.artifacts.contains_key(REMOTE_PROMPTS_SQL),
        skills: manifest.artifacts.contains_key(REMOTE_SKILLS_SQL)
            && manifest.artifacts.contains_key(REMOTE_SKILLS_ZIP),
    }
}

async fn find_remote_snapshot(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
) -> Result<Option<RemoteSnapshot>, AppError> {
    for location in [
        RemoteLocation::v3_current(),
        RemoteLocation::v2_current(),
        RemoteLocation::v2_legacy(),
    ] {
        if let Some(snapshot) = fetch_remote_snapshot(settings, auth, location).await? {
            return Ok(Some(snapshot));
        }
    }
    Ok(None)
}

async fn fetch_remote_snapshot(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    location: RemoteLocation,
) -> Result<Option<RemoteSnapshot>, AppError> {
    let manifest_url = remote_file_url(settings, location, REMOTE_MANIFEST)?;
    let Some((manifest_bytes, manifest_etag)) =
        get_bytes(&manifest_url, auth, MAX_MANIFEST_BYTES).await?
    else {
        return Ok(None);
    };

    let manifest: SyncManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| AppError::Json {
            path: REMOTE_MANIFEST.to_string(),
            source: e,
        })?;

    Ok(Some(RemoteSnapshot {
        location,
        manifest,
        manifest_bytes,
        manifest_etag,
    }))
}
// ─── Download & verify ───────────────────────────────────────

async fn download_and_verify(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    location: RemoteLocation,
    artifact_name: &str,
    artifacts: &BTreeMap<String, ArtifactMeta>,
) -> Result<Vec<u8>, AppError> {
    let meta = artifacts.get(artifact_name).ok_or_else(|| {
        localized(
            "webdav.sync.manifest_missing_artifact",
            format!("manifest 中缺少 artifact: {artifact_name}"),
            format!("Manifest missing artifact: {artifact_name}"),
        )
    })?;
    validate_artifact_size_limit(artifact_name, meta.size)?;

    let url = remote_file_url(settings, location, artifact_name)?;
    let (bytes, _) = get_bytes(&url, auth, MAX_SYNC_ARTIFACT_BYTES as usize)
        .await?
        .ok_or_else(|| {
            localized(
                "webdav.sync.remote_missing_artifact",
                format!("远端缺少 artifact 文件: {artifact_name}"),
                format!("Remote artifact file missing: {artifact_name}"),
            )
        })?;

    verify_artifact(&bytes, artifact_name, meta)?;
    Ok(bytes)
}

async fn download_optional_artifact(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    location: RemoteLocation,
    artifact_name: &str,
    artifacts: &BTreeMap<String, ArtifactMeta>,
) -> Result<Option<Vec<u8>>, AppError> {
    if !artifacts.contains_key(artifact_name) {
        return Ok(None);
    }
    download_and_verify(settings, auth, location, artifact_name, artifacts)
        .await
        .map(Some)
}

async fn load_remote_module_payloads(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    snapshot: &RemoteSnapshot,
) -> Result<ModulePayloads, AppError> {
    if snapshot.location.protocol_version == LEGACY_PROTOCOL_VERSION {
        let db_sql = download_and_verify(
            settings,
            auth,
            snapshot.location,
            REMOTE_DB_SQL,
            &snapshot.manifest.artifacts,
        )
        .await?;
        let skills_zip = download_and_verify(
            settings,
            auth,
            snapshot.location,
            REMOTE_SKILLS_ZIP,
            &snapshot.manifest.artifacts,
        )
        .await?;
        return split_v2_snapshot_into_module_payloads(&db_sql, &skills_zip);
    }

    Ok(ModulePayloads {
        api_sql: download_optional_artifact(
            settings,
            auth,
            snapshot.location,
            REMOTE_API_SQL,
            &snapshot.manifest.artifacts,
        )
        .await?,
        mcp_sql: download_optional_artifact(
            settings,
            auth,
            snapshot.location,
            REMOTE_MCP_SQL,
            &snapshot.manifest.artifacts,
        )
        .await?,
        prompts_sql: download_optional_artifact(
            settings,
            auth,
            snapshot.location,
            REMOTE_PROMPTS_SQL,
            &snapshot.manifest.artifacts,
        )
        .await?,
        skills_sql: download_optional_artifact(
            settings,
            auth,
            snapshot.location,
            REMOTE_SKILLS_SQL,
            &snapshot.manifest.artifacts,
        )
        .await?,
        skills_zip: download_optional_artifact(
            settings,
            auth,
            snapshot.location,
            REMOTE_SKILLS_ZIP,
            &snapshot.manifest.artifacts,
        )
        .await?,
    })
}

fn split_v2_snapshot_into_module_payloads(
    db_sql: &[u8],
    skills_zip: &[u8],
) -> Result<ModulePayloads, AppError> {
    let sql_str = std::str::from_utf8(db_sql).map_err(|e| {
        localized(
            "webdav.sync.sql_not_utf8",
            format!("SQL 非 UTF-8: {e}"),
            format!("SQL is not valid UTF-8: {e}"),
        )
    })?;
    let conn =
        rusqlite::Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;
    conn.execute_batch(sql_str)
        .map_err(|e| AppError::Database(format!("执行 SQL 导入失败: {e}")))?;
    crate::database::Database::create_tables_on_conn(&conn)?;
    crate::database::Database::apply_schema_migrations_on_conn(&conn)?;

    Ok(ModulePayloads {
        api_sql: Some(
            crate::database::Database::export_sql_string_from_connection_for_tables(
                &conn, API_TABLES,
            )?
            .into_bytes(),
        ),
        mcp_sql: Some(
            crate::database::Database::export_sql_string_from_connection_for_tables(
                &conn, MCP_TABLES,
            )?
            .into_bytes(),
        ),
        prompts_sql: Some(
            crate::database::Database::export_sql_string_from_connection_for_tables(
                &conn,
                PROMPT_TABLES,
            )?
            .into_bytes(),
        ),
        skills_sql: Some(
            crate::database::Database::export_sql_string_from_connection_for_tables(
                &conn,
                SKILL_TABLES,
            )?
            .into_bytes(),
        ),
        skills_zip: Some(skills_zip.to_vec()),
    })
}

fn ensure_requested_modules_available(
    payloads: &ModulePayloads,
    selection: &WebDavSyncModules,
) -> Result<(), AppError> {
    let available = payloads.available_modules();
    let missing = [
        (selection.api && !available.api, "API"),
        (selection.mcp && !available.mcp, "MCP"),
        (selection.prompts && !available.prompts, "Prompts"),
        (selection.skills && !available.skills, "Skills"),
    ]
    .into_iter()
    .filter_map(|(missing, label)| missing.then_some(label))
    .collect::<Vec<_>>();

    if missing.is_empty() {
        return Ok(());
    }

    Err(localized(
        "webdav.sync.requested_modules_missing",
        format!("远端缺少所选同步模块: {}", missing.join(", ")),
        format!(
            "The remote snapshot is missing the selected sync modules: {}",
            missing.join(", ")
        ),
    ))
}

fn decode_sql_payload(bytes: &[u8]) -> Result<String, AppError> {
    std::str::from_utf8(bytes)
        .map(|sql| sql.to_string())
        .map_err(|e| {
            localized(
                "webdav.sync.sql_not_utf8",
                format!("SQL 非 UTF-8: {e}"),
                format!("SQL is not valid UTF-8: {e}"),
            )
        })
}

fn apply_selected_modules(
    db: &crate::database::Database,
    payloads: &ModulePayloads,
    selection: &WebDavSyncModules,
) -> Result<(), AppError> {
    let mut sql_documents = Vec::new();
    let mut selected_tables = Vec::new();

    if selection.api {
        sql_documents.push(decode_sql_payload(
            payloads.api_sql.as_deref().ok_or_else(|| {
                localized(
                    "webdav.sync.api_missing",
                    "缺少 API SQL",
                    "Missing API SQL.",
                )
            })?,
        )?);
        selected_tables.extend_from_slice(API_TABLES);
    }
    if selection.mcp {
        sql_documents.push(decode_sql_payload(
            payloads.mcp_sql.as_deref().ok_or_else(|| {
                localized(
                    "webdav.sync.mcp_missing",
                    "缺少 MCP SQL",
                    "Missing MCP SQL.",
                )
            })?,
        )?);
        selected_tables.extend_from_slice(MCP_TABLES);
    }
    if selection.prompts {
        sql_documents.push(decode_sql_payload(
            payloads.prompts_sql.as_deref().ok_or_else(|| {
                localized(
                    "webdav.sync.prompts_missing",
                    "缺少 Prompts SQL",
                    "Missing Prompts SQL.",
                )
            })?,
        )?);
        selected_tables.extend_from_slice(PROMPT_TABLES);
    }
    if selection.skills {
        sql_documents.push(decode_sql_payload(
            payloads.skills_sql.as_deref().ok_or_else(|| {
                localized(
                    "webdav.sync.skills_sql_missing",
                    "缺少 Skills SQL",
                    "Missing Skills SQL.",
                )
            })?,
        )?);
        selected_tables.extend_from_slice(SKILL_TABLES);
    }

    let skills_backup = if selection.skills {
        Some(backup_current_skills()?)
    } else {
        None
    };

    if selection.skills {
        restore_skills_zip(payloads.skills_zip.as_deref().ok_or_else(|| {
            localized(
                "webdav.sync.skills_zip_missing",
                "缺少 skills.zip",
                "Missing skills.zip.",
            )
        })?)?;
    }

    let sql_refs = sql_documents.iter().map(String::as_str).collect::<Vec<_>>();
    if let Err(db_err) = db.replace_tables_from_sql_strings(&sql_refs, &selected_tables) {
        if let Some(skills_backup) = skills_backup.as_ref() {
            if let Err(rollback_err) = restore_skills_from_backup(skills_backup) {
                return Err(localized(
                    "webdav.sync.db_import_and_rollback_failed",
                    format!("导入数据库失败: {db_err}; 同时回滚 Skills 失败: {rollback_err}"),
                    format!(
                        "Database import failed: {db_err}; skills rollback also failed: {rollback_err}"
                    ),
                ));
            }
        }
        return Err(db_err);
    }

    Ok(())
}

// ─── Remote path helpers ─────────────────────────────────────

fn remote_dir_segments(settings: &WebDavSyncSettings, location: RemoteLocation) -> Vec<String> {
    let mut segs = Vec::new();
    segs.extend(path_segments(&settings.remote_root).map(str::to_string));
    segs.push(format!("v{}", location.protocol_version));
    if location.layout == RemoteLayout::Current {
        segs.push(format!("db-v{DB_COMPAT_VERSION}"));
    }
    segs.extend(path_segments(&settings.profile).map(str::to_string));
    segs
}

fn remote_file_url(
    settings: &WebDavSyncSettings,
    location: RemoteLocation,
    file_name: &str,
) -> Result<String, AppError> {
    let mut segs = remote_dir_segments(settings, location);
    segs.extend(path_segments(file_name).map(str::to_string));
    build_remote_url(&settings.base_url, &segs)
}

fn remote_dir_display(settings: &WebDavSyncSettings, location: RemoteLocation) -> String {
    let segs = remote_dir_segments(settings, location);
    format!("/{}", segs.join("/"))
}

fn auth_for(settings: &WebDavSyncSettings) -> WebDavAuth {
    auth_from_credentials(&settings.username, &settings.password)
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;

    fn artifact(hash: &str, size: u64) -> ArtifactMeta {
        ArtifactMeta {
            sha256: hash.to_string(),
            size,
        }
    }

    #[test]
    fn remote_dir_segments_uses_current_layout() {
        let settings = WebDavSyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        };
        let segs = remote_dir_segments(&settings, RemoteLocation::v3_current());
        assert_eq!(segs, vec!["cc-switch-sync", "v3", "db-v6", "default"]);
    }

    #[test]
    fn remote_dir_segments_uses_legacy_layout() {
        let settings = WebDavSyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        };
        let segs = remote_dir_segments(&settings, RemoteLocation::v2_legacy());
        assert_eq!(segs, vec!["cc-switch-sync", "v2", "default"]);
    }

    #[test]
    fn sha256_hex_is_correct() {
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn persist_best_effort_returns_true_on_success() {
        let mut settings = WebDavSyncSettings::default();
        let ok = persist_sync_success_best_effort(
            &mut settings,
            "hash".to_string(),
            Some("etag".to_string()),
            |_settings, _hash, _etag| Ok(()),
        );
        assert!(ok);
    }

    #[test]
    fn persist_best_effort_returns_false_on_error() {
        let mut settings = WebDavSyncSettings::default();
        let ok = persist_sync_success_best_effort(
            &mut settings,
            "hash".to_string(),
            None,
            |_settings, _hash, _etag| Err(AppError::Config("boom".to_string())),
        );
        assert!(!ok);
    }

    fn manifest_with(format: &str, version: u32, db_compat_version: Option<u32>) -> SyncManifest {
        let mut artifacts = BTreeMap::new();
        let modules = if version == PROTOCOL_VERSION {
            artifacts.insert(REMOTE_API_SQL.to_string(), artifact("abc", 1));
            artifacts.insert(REMOTE_MCP_SQL.to_string(), artifact("def", 2));
            artifacts.insert(REMOTE_PROMPTS_SQL.to_string(), artifact("ghi", 3));
            artifacts.insert(REMOTE_SKILLS_SQL.to_string(), artifact("jkl", 4));
            artifacts.insert(REMOTE_SKILLS_ZIP.to_string(), artifact("mno", 5));
            Some(ManifestModules {
                api: vec![REMOTE_API_SQL.to_string()],
                mcp: vec![REMOTE_MCP_SQL.to_string()],
                prompts: vec![REMOTE_PROMPTS_SQL.to_string()],
                skills: vec![REMOTE_SKILLS_SQL.to_string(), REMOTE_SKILLS_ZIP.to_string()],
            })
        } else {
            artifacts.insert(REMOTE_DB_SQL.to_string(), artifact("abc", 1));
            artifacts.insert(REMOTE_SKILLS_ZIP.to_string(), artifact("def", 2));
            None
        };
        SyncManifest {
            format: format.to_string(),
            version,
            db_compat_version,
            device_name: "My MacBook".to_string(),
            created_at: "2026-02-12T00:00:00Z".to_string(),
            artifacts,
            modules,
            snapshot_id: "snap-1".to_string(),
        }
    }

    #[test]
    fn validate_manifest_compat_accepts_supported_manifest() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        assert!(validate_manifest_compat(&manifest, RemoteLocation::v3_current()).is_ok());
    }

    #[test]
    fn validate_manifest_compat_rejects_wrong_format() {
        let manifest = manifest_with("other-format", PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        assert!(validate_manifest_compat(&manifest, RemoteLocation::v3_current()).is_err());
    }

    #[test]
    fn validate_manifest_compat_rejects_wrong_version() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION + 1,
            Some(DB_COMPAT_VERSION),
        );
        assert!(validate_manifest_compat(&manifest, RemoteLocation::v3_current()).is_err());
    }

    #[test]
    fn validate_manifest_compat_accepts_legacy_manifest_without_db_compat() {
        let manifest = manifest_with(PROTOCOL_FORMAT, LEGACY_PROTOCOL_VERSION, None);
        assert!(validate_manifest_compat(&manifest, RemoteLocation::v2_legacy()).is_ok());
    }

    #[test]
    fn validate_manifest_compat_rejects_current_manifest_with_wrong_db_compat() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION,
            Some(LEGACY_DB_COMPAT_VERSION),
        );
        assert!(validate_manifest_compat(&manifest, RemoteLocation::v3_current()).is_err());
    }

    #[test]
    fn validate_manifest_compat_rejects_legacy_manifest_from_newer_db_generation() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            LEGACY_PROTOCOL_VERSION,
            Some(DB_COMPAT_VERSION + 1),
        );
        assert!(validate_manifest_compat(&manifest, RemoteLocation::v2_legacy()).is_err());
    }

    #[test]
    fn effective_db_compat_version_defaults_legacy_layout_to_v5() {
        let manifest = manifest_with(PROTOCOL_FORMAT, LEGACY_PROTOCOL_VERSION, None);
        assert_eq!(
            effective_db_compat_version(&manifest, RemoteLocation::v2_legacy()),
            Some(LEGACY_DB_COMPAT_VERSION)
        );
        assert_eq!(
            effective_db_compat_version(&manifest, RemoteLocation::v2_current()),
            None
        );
    }

    #[test]
    fn available_modules_defaults_to_all_enabled_for_v2_snapshots() {
        let manifest = manifest_with(PROTOCOL_FORMAT, LEGACY_PROTOCOL_VERSION, None);
        let modules = available_modules_from_manifest(&manifest, RemoteLocation::v2_legacy());
        assert_eq!(
            modules,
            WebDavSyncModules {
                api: true,
                mcp: true,
                prompts: true,
                skills: true,
            }
        );
    }

    #[test]
    fn available_modules_respects_v3_manifest_entries() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        let modules = available_modules_from_manifest(&manifest, RemoteLocation::v3_current());
        assert!(modules.api);
        assert!(modules.mcp);
        assert!(modules.prompts);
        assert!(modules.skills);
    }

    #[test]
    fn build_local_module_payloads_includes_model_pricing_in_api_sql() -> Result<(), AppError> {
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let test_home = std::env::temp_dir().join("cc-switch-webdav-model-pricing-payload-test");
        let _ = fs::remove_dir_all(&test_home);
        fs::create_dir_all(&test_home).expect("create isolated test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);

        let result = (|| -> Result<(), AppError> {
            let db = crate::database::Database::memory()?;
            {
                let conn = crate::database::lock_conn!(db.conn);
                conn.execute("DELETE FROM model_pricing", [])?;
                conn.execute(
                    "INSERT INTO model_pricing (
                        model_id, display_name, input_cost_per_million, output_cost_per_million
                    ) VALUES ('pricing-from-api-sync', 'Pricing From Sync', 1.23, 4.56)",
                    [],
                )?;
            }

            let payloads = build_local_module_payloads(&db)?;
            let api_sql = std::str::from_utf8(
                payloads
                    .api_sql
                    .as_deref()
                    .expect("api payload should be present"),
            )
            .expect("api sql should be utf-8");

            assert!(
                api_sql.contains("INSERT INTO \"model_pricing\""),
                "api module export should include model_pricing rows"
            );
            assert!(
                api_sql.contains("pricing-from-api-sync"),
                "api module export should include model_pricing data"
            );

            Ok(())
        })();

        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }

        result
    }

    #[test]
    fn normalize_device_name_returns_none_for_blank_input() {
        assert_eq!(normalize_device_name("   \n\t  "), None);
    }

    #[test]
    fn normalize_device_name_collapses_whitespace_and_drops_control_chars() {
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
    fn manifest_serialization_uses_device_name_only() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        let value = serde_json::to_value(&manifest).expect("serialize manifest");
        assert!(
            value.get("deviceName").is_some(),
            "manifest should contain deviceName"
        );
        assert_eq!(
            value.get("dbCompatVersion").and_then(|v| v.as_u64()),
            Some(DB_COMPAT_VERSION as u64)
        );
        assert!(
            value.get("deviceId").is_none(),
            "manifest should not contain deviceId"
        );
    }

    #[test]
    fn validate_artifact_size_limit_rejects_oversized_artifacts() {
        let err = validate_artifact_size_limit("skills.zip", MAX_SYNC_ARTIFACT_BYTES + 1)
            .expect_err("artifact larger than limit should be rejected");
        assert!(
            err.to_string().contains("too large") || err.to_string().contains("超过"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_artifact_size_limit_accepts_limit_boundary() {
        assert!(validate_artifact_size_limit("skills.zip", MAX_SYNC_ARTIFACT_BYTES).is_ok());
    }

    #[tokio::test]
    #[ignore]
    #[serial]
    async fn live_webdav_module_sync_roundtrip_preserves_unselected_modules() {
        let base_url = std::env::var("CC_SWITCH_LIVE_WEBDAV_URL")
            .expect("CC_SWITCH_LIVE_WEBDAV_URL must be set");
        let username = std::env::var("CC_SWITCH_LIVE_WEBDAV_USERNAME")
            .expect("CC_SWITCH_LIVE_WEBDAV_USERNAME must be set");
        let password = std::env::var("CC_SWITCH_LIVE_WEBDAV_PASSWORD")
            .expect("CC_SWITCH_LIVE_WEBDAV_PASSWORD must be set");

        let test_home = std::env::temp_dir().join(format!(
            "cc-switch-webdav-live-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = fs::remove_dir_all(&test_home);
        fs::create_dir_all(&test_home).expect("create live test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);
        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset app settings");

        let remote_root = format!("csl-{}", chrono::Utc::now().timestamp());
        let profile = "module-sync".to_string();

        let first_db = crate::database::Database::memory().expect("create db");
        seed_provider(&first_db, "provider-remote-a");
        seed_mcp(&first_db, "mcp-remote-a");
        seed_prompt(&first_db, "prompt-remote-a", "remote prompt A");
        seed_skill_dir("skill-remote-a");

        let mut first_settings = live_settings(
            &base_url,
            &username,
            &password,
            &remote_root,
            &profile,
            WebDavSyncModules::default(),
            WebDavSyncModules::default(),
        );
        check_connection(&first_settings)
            .await
            .expect("check connection");
        upload(&first_db, &mut first_settings)
            .await
            .expect("initial upload");

        reset_skill_dir("skill-remote-b");
        replace_mcp(&first_db, "mcp-remote-b");
        first_settings.upload_modules = WebDavSyncModules {
            api: false,
            mcp: true,
            prompts: false,
            skills: false,
        };
        upload(&first_db, &mut first_settings)
            .await
            .expect("mcp-only upload");

        let second_db = crate::database::Database::memory().expect("create second db");
        seed_provider(&second_db, "provider-local");
        seed_mcp(&second_db, "mcp-local");
        seed_prompt(&second_db, "prompt-local", "local prompt");
        reset_skill_dir("skill-local");

        let mut second_settings = live_settings(
            &base_url,
            &username,
            &password,
            &remote_root,
            &profile,
            WebDavSyncModules::default(),
            WebDavSyncModules {
                api: false,
                mcp: false,
                prompts: true,
                skills: false,
            },
        );
        download(&second_db, &mut second_settings)
            .await
            .expect("prompts-only download");

        assert_provider_exists(&second_db, "provider-local");
        assert_mcp_exists(&second_db, "mcp-local");
        assert_prompt_exists(&second_db, "prompt-remote-a");
        assert_skill_dir_contains("skill-local");
    }

    fn live_settings(
        base_url: &str,
        username: &str,
        password: &str,
        remote_root: &str,
        profile: &str,
        upload_modules: WebDavSyncModules,
        download_modules: WebDavSyncModules,
    ) -> WebDavSyncSettings {
        WebDavSyncSettings {
            enabled: true,
            auto_sync: false,
            base_url: base_url.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            remote_root: remote_root.to_string(),
            profile: profile.to_string(),
            upload_modules,
            download_modules,
            ..WebDavSyncSettings::default()
        }
    }

    fn seed_provider(db: &crate::database::Database, id: &str) {
        let conn = db.conn.lock().expect("lock db");
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES (?1, 'claude', ?1, '{}', '{}')",
            [id],
        )
        .expect("insert provider");
    }

    fn seed_mcp(db: &crate::database::Database, id: &str) {
        let conn = db.conn.lock().expect("lock db");
        conn.execute(
            "INSERT INTO mcp_servers (
                id, name, server_config, tags,
                enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_hermes
            ) VALUES (?1, ?1, '{}', '[]', 1, 0, 0, 0, 0)",
            [id],
        )
        .expect("insert mcp");
    }

    fn replace_mcp(db: &crate::database::Database, id: &str) {
        let conn = db.conn.lock().expect("lock db");
        conn.execute("DELETE FROM mcp_servers", [])
            .expect("clear mcp");
        conn.execute(
            "INSERT INTO mcp_servers (
                id, name, server_config, tags,
                enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_hermes
            ) VALUES (?1, ?1, '{}', '[]', 1, 1, 0, 0, 0)",
            [id],
        )
        .expect("insert replacement mcp");
    }

    fn seed_prompt(db: &crate::database::Database, id: &str, content: &str) {
        let conn = db.conn.lock().expect("lock db");
        conn.execute(
            "INSERT INTO prompts (id, app_type, name, content, enabled)
             VALUES (?1, 'claude', ?1, ?2, 1)",
            rusqlite::params![id, content],
        )
        .expect("insert prompt");
    }

    fn seed_skill_dir(name: &str) {
        let ssot_dir =
            crate::services::skill::SkillService::get_ssot_dir().expect("resolve skills dir");
        let skill_dir = ssot_dir.join(name);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("# {name}\n\nLive WebDAV test skill\n"),
        )
        .expect("write skill file");
    }

    fn reset_skill_dir(name: &str) {
        let ssot_dir =
            crate::services::skill::SkillService::get_ssot_dir().expect("resolve skills dir");
        let _ = fs::remove_dir_all(&ssot_dir);
        seed_skill_dir(name);
    }

    fn assert_provider_exists(db: &crate::database::Database, id: &str) {
        let conn = db.conn.lock().expect("lock db");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM providers WHERE id = ?1 AND app_type = 'claude'",
                [id],
                |row| row.get(0),
            )
            .expect("query provider count");
        assert_eq!(count, 1, "expected provider {id} to remain local");
    }

    fn assert_mcp_exists(db: &crate::database::Database, id: &str) {
        let conn = db.conn.lock().expect("lock db");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mcp_servers WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .expect("query mcp count");
        assert_eq!(count, 1, "expected MCP {id} to remain local");
    }

    fn assert_prompt_exists(db: &crate::database::Database, id: &str) {
        let conn = db.conn.lock().expect("lock db");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM prompts WHERE id = ?1 AND app_type = 'claude'",
                [id],
                |row| row.get(0),
            )
            .expect("query prompt count");
        assert_eq!(count, 1, "expected prompt {id} to be downloaded");
    }

    fn assert_skill_dir_contains(name: &str) {
        let ssot_dir =
            crate::services::skill::SkillService::get_ssot_dir().expect("resolve skills dir");
        assert!(
            ssot_dir.join(name).join("SKILL.md").exists(),
            "expected local skill directory {name} to remain untouched"
        );
    }
}
