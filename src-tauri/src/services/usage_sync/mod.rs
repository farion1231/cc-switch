//! Cross-device Usage Sync.
//!
//! Each device publishes its own usage (request records or daily aggregates,
//! plus long-term rollups) under its own remote directory and imports the
//! other devices' data into read-only cache tables. Local usage tables are
//! never written by this module.
//!
//! See `docs/usage-sync.md` for the protocol, consistency and privacy model.

pub(crate) mod protocol;
pub(crate) mod store;
pub(crate) mod transport;
pub(crate) mod worker;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_http;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use serde::Serialize;

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::sync_protocol::{
    detect_system_device_name, normalize_device_name, sha256_hex,
};
use crate::settings::{UsageSyncBackend, UsageSyncDetailLevel, UsageSyncSettings};

use protocol::{
    decode_segment, encode_segment, is_incompatible_error, manifest_path, object_name, object_path,
    parse_manifest, validate_manifest, validate_segment, DeviceManifest, PartitionMeta,
    RetiredObject, DETAIL_LEVEL_DAILY, DETAIL_LEVEL_RECORDS, MAX_MANIFEST_BYTES,
    MAX_RETIRED_OBJECTS, MAX_SEGMENT_BYTES, RETIRED_GRACE_SECS, USAGE_PROTOCOL_VERSION,
    USAGE_SYNC_FORMAT,
};
use store::DeviceIdentity;
use transport::UsageTransport;

pub use store::RemoteDeviceInfo;

/// Rewrite an unchanged manifest at least this often so other devices can
/// tell an idle-but-online device from one that stopped syncing.
pub(crate) const HEARTBEAT_SECS: i64 = 60 * 60;
/// A device whose last upload is older than this is shown as stale.
pub(crate) const STALE_AFTER_SECS: i64 = 24 * 60 * 60;

pub(crate) const IDENTITY_CONFLICT_MARKER: &str = "usage_sync.identity_conflict";

fn usage_sync_mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_sync_running() -> bool {
    SYNC_RUNNING.load(Ordering::SeqCst)
}

fn identity_conflict_error() -> AppError {
    AppError::localized(
        IDENTITY_CONFLICT_MARKER,
        "检测到另一个 CC Switch 安装正在使用本设备的同步身份（可能复制了配置目录）。已暂停用量上传，请在用量同步设置中处理。",
        "Another CC Switch installation is uploading with this device's identity (the profile may have been copied). Usage upload is paused; resolve it in Usage Sync settings.",
    )
}

pub(crate) fn is_identity_conflict(error: &AppError) -> bool {
    matches!(error, AppError::Localized { key, .. } if *key == IDENTITY_CONFLICT_MARKER)
}

fn not_enabled_error() -> AppError {
    AppError::localized(
        "usage_sync.not_enabled",
        "用量同步未启用",
        "Usage sync is not enabled.",
    )
}

fn backend_missing_error() -> AppError {
    AppError::localized(
        "usage_sync.backend_missing",
        "请先选择用量同步使用的云存储（WebDAV 或 S3）",
        "Choose which cloud storage (WebDAV or S3) carries usage data first.",
    )
}

async fn blocking<T, F>(db: &Arc<Database>, work: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> Result<T, AppError> + Send + 'static,
{
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = lock_conn!(db.conn);
        work(&conn)
    })
    .await
    .map_err(|e| AppError::Message(format!("usage sync task failed: {e}")))?
}

/// CPU-bound work (JSON, gzip, SHA-256) off the async runtime and outside the DB lock.
async fn cpu<T, F>(work: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| AppError::Message(format!("usage sync task failed: {e}")))?
}

// ─── Options & reports ───────────────────────────────────────

/// Inputs of one sync run that do not come from the database.
#[derive(Debug, Clone)]
pub(crate) struct SyncOptions {
    pub detail_level: UsageSyncDetailLevel,
    pub system_device_name: String,
    pub platform: String,
    pub app_version: String,
    pub now: i64,
}

impl SyncOptions {
    pub(crate) fn from_settings(settings: &UsageSyncSettings) -> Self {
        Self {
            detail_level: settings.detail_level,
            system_device_name: system_device_name(),
            platform: std::env::consts::OS.to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            now: chrono::Utc::now().timestamp(),
        }
    }
}

fn system_device_name() -> String {
    detect_system_device_name().unwrap_or_else(|| "Unknown Device".to_string())
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSyncError {
    pub device_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSyncReport {
    /// Segments uploaded in this run (unchanged partitions are skipped).
    pub uploaded_partitions: usize,
    pub manifest_written: bool,
    pub upload_error: Option<String>,
    pub identity_conflict: bool,
    pub devices_seen: usize,
    pub devices_updated: usize,
    pub device_errors: Vec<DeviceSyncError>,
    pub download_error: Option<String>,
}

impl UsageSyncReport {
    pub fn first_error(&self) -> Option<String> {
        self.upload_error
            .clone()
            .or_else(|| self.download_error.clone())
            .or_else(|| {
                self.device_errors
                    .first()
                    .map(|e| format!("{}: {}", e.device_id, e.error))
            })
    }
}

// ─── Upload ──────────────────────────────────────────────────

#[derive(Debug, Default)]
struct UploadOutcome {
    uploaded_partitions: usize,
    manifest_written: bool,
}

fn effective_device_name(identity: &DeviceIdentity, options: &SyncOptions) -> String {
    identity
        .custom_name
        .as_deref()
        .and_then(normalize_device_name)
        .unwrap_or_else(|| options.system_device_name.clone())
}

fn detail_level_wire(level: UsageSyncDetailLevel) -> &'static str {
    match level {
        UsageSyncDetailLevel::Records => DETAIL_LEVEL_RECORDS,
        UsageSyncDetailLevel::Daily => DETAIL_LEVEL_DAILY,
    }
}

/// Read our own manifest. A corrupt or foreign-shaped manifest in our own
/// directory is replaced; a manifest from a newer protocol is left alone.
async fn read_own_manifest(
    transport: &UsageTransport,
    device_id: &str,
) -> Result<Option<DeviceManifest>, AppError> {
    let Some(bytes) = transport
        .get(&manifest_path(device_id), MAX_MANIFEST_BYTES)
        .await?
    else {
        return Ok(None);
    };
    match parse_manifest(&bytes).and_then(|manifest| {
        validate_manifest(&manifest, device_id)?;
        Ok(manifest)
    }) {
        Ok(manifest) => Ok(Some(manifest)),
        Err(error) if is_incompatible_error(&error) => Err(error),
        Err(error) => {
            log::warn!("[UsageSync] own manifest unreadable, it will be rewritten: {error}");
            Ok(None)
        }
    }
}

async fn upload_phase(
    db: &Arc<Database>,
    transport: &UsageTransport,
    options: &SyncOptions,
    identity: &DeviceIdentity,
) -> Result<UploadOutcome, AppError> {
    let device_id = identity.device_id.clone();
    let detail_level = options.detail_level;
    let export = {
        let device_id = device_id.clone();
        blocking(db, move |conn| {
            store::export_local(conn, &device_id, detail_level)
        })
        .await?
    };

    let remote = read_own_manifest(transport, &device_id).await?;
    if let Some(remote) = &remote {
        let known = [
            identity.committed_nonce.as_deref(),
            identity.pending_nonce.as_deref(),
        ];
        if !known.contains(&Some(remote.writer_nonce.as_str())) {
            return Err(identity_conflict_error());
        }
    }

    let remote_partitions = remote
        .as_ref()
        .map(|m| m.partitions.clone())
        .unwrap_or_default();
    let mut outcome = UploadOutcome::default();
    let mut partitions: BTreeMap<String, PartitionMeta> = BTreeMap::new();
    let mut dir_prepared = false;

    let coverage = export.coverage.clone();
    let encoded_parts = cpu(move || {
        export
            .partitions
            .into_iter()
            .map(|(key, segment)| {
                let encoded = encode_segment(&segment)?;
                Ok((
                    key,
                    segment.logs.len() as u64,
                    segment.rollups.len() as u64,
                    encoded,
                ))
            })
            .collect::<Result<Vec<_>, AppError>>()
    })
    .await?;

    for (key, log_count, rollup_count, encoded) in encoded_parts {
        let key = &key;
        if let Some(existing) = remote_partitions
            .get(key)
            .filter(|meta| meta.content_sha256 == encoded.content_sha256)
        {
            partitions.insert(key.clone(), existing.clone());
            continue;
        }
        if encoded.gz_bytes.len() > MAX_SEGMENT_BYTES {
            return Err(AppError::localized(
                "usage_sync.segment_too_large",
                format!("用量分区 {key} 超过上传上限"),
                format!("Usage partition {key} exceeds the upload limit"),
            ));
        }
        if !dir_prepared {
            transport
                .prepare_device_dir(&protocol::device_dir(&device_id))
                .await?;
            dir_prepared = true;
        }
        let object = object_name(key, &encoded.content_sha256);
        let size = encoded.gz_bytes.len() as u64;
        transport
            .put(
                &object_path(&device_id, &object),
                encoded.gz_bytes,
                "application/gzip",
            )
            .await?;
        outcome.uploaded_partitions += 1;
        partitions.insert(
            key.clone(),
            PartitionMeta {
                object,
                sha256: encoded.sha256,
                size,
                content_sha256: encoded.content_sha256,
                logs: log_count,
                rollups: rollup_count,
            },
        );
    }

    // Superseded objects are retired, not deleted: a reader holding the
    // previous manifest may still be downloading them.
    let referenced: std::collections::HashSet<&str> =
        partitions.values().map(|m| m.object.as_str()).collect();
    let mut retired: Vec<RetiredObject> = Vec::new();
    let mut expired: Vec<String> = Vec::new();
    if let Some(remote) = &remote {
        for item in &remote.retired {
            if referenced.contains(item.object.as_str()) {
                continue;
            }
            if options.now - item.retired_at >= RETIRED_GRACE_SECS {
                expired.push(item.object.clone());
            } else {
                retired.push(item.clone());
            }
        }
        for meta in remote.partitions.values() {
            if !referenced.contains(meta.object.as_str())
                && !retired.iter().any(|r| r.object == meta.object)
            {
                retired.push(RetiredObject {
                    object: meta.object.clone(),
                    retired_at: options.now,
                });
            }
        }
    }
    // Expired objects are referenced by neither the current nor the next
    // manifest, so they can go before the manifest write. Only objects that
    // were actually deleted leave the list; failures are retried next sync.
    for object in expired {
        if let Err(error) = transport.delete(&object_path(&device_id, &object)).await {
            log::debug!("[UsageSync] retired object cleanup failed, will retry: {error}");
            retired.push(RetiredObject {
                object,
                retired_at: options.now - RETIRED_GRACE_SECS,
            });
        }
    }
    retired.sort_by(|a, b| {
        a.retired_at
            .cmp(&b.retired_at)
            .then(a.object.cmp(&b.object))
    });
    // Pathological growth guard: the oldest entries beyond the cap are
    // deleted after the new manifest is published.
    let mut overflow_objects: Vec<String> = Vec::new();
    if retired.len() > MAX_RETIRED_OBJECTS {
        let overflow = retired.len() - MAX_RETIRED_OBJECTS;
        overflow_objects.extend(retired.drain(..overflow).map(|r| r.object));
    }

    let device_name = effective_device_name(identity, options);
    let detail_level = detail_level_wire(options.detail_level).to_string();
    let utc_offset_minutes = chrono::Local::now().offset().local_minus_utc() / 60;
    let needs_write = match &remote {
        None => true,
        Some(remote) => {
            remote.partitions != partitions
                || remote.retired != retired
                || remote.device_name != device_name
                || remote.detail_level != detail_level
                || remote.app_version != options.app_version
                || remote.coverage != coverage
                || remote.version != USAGE_PROTOCOL_VERSION
                || options.now - remote.updated_at >= HEARTBEAT_SECS
        }
    };
    if !needs_write {
        return Ok(outcome);
    }

    let seq = remote
        .as_ref()
        .map(|m| m.seq)
        .unwrap_or(0)
        .max(identity.manifest_seq)
        + 1;
    let nonce = uuid::Uuid::new_v4().to_string();
    let manifest = DeviceManifest {
        format: USAGE_SYNC_FORMAT.to_string(),
        version: USAGE_PROTOCOL_VERSION,
        device_id: device_id.clone(),
        device_name,
        platform: options.platform.clone(),
        app_version: options.app_version.clone(),
        seq,
        writer_nonce: nonce.clone(),
        updated_at: options.now,
        detail_level,
        utc_offset_minutes,
        coverage,
        partitions,
        retired,
    };
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|source| AppError::JsonSerialize { source })?;

    {
        let nonce = nonce.clone();
        blocking(db, move |conn| store::set_pending_nonce(conn, &nonce)).await?;
    }
    if !dir_prepared {
        transport
            .prepare_device_dir(&protocol::device_dir(&device_id))
            .await?;
    }
    transport
        .put(&manifest_path(&device_id), bytes, "application/json")
        .await?;
    blocking(db, move |conn| store::commit_nonce(conn, &nonce, seq)).await?;
    outcome.manifest_written = true;

    for object in overflow_objects {
        if let Err(error) = transport.delete(&object_path(&device_id, &object)).await {
            log::debug!("[UsageSync] retired overflow cleanup failed: {error}");
        }
    }
    Ok(outcome)
}

// ─── Download ────────────────────────────────────────────────

#[derive(Debug, Default)]
struct DownloadOutcome {
    devices_seen: usize,
    devices_updated: usize,
    device_errors: Vec<DeviceSyncError>,
}

enum DeviceResult {
    Unchanged,
    Updated,
}

async fn sync_remote_device(
    db: &Arc<Database>,
    transport: &UsageTransport,
    device_id: &str,
    now: i64,
) -> Result<DeviceResult, AppError> {
    let cached = {
        let id = device_id.to_string();
        blocking(db, move |conn| store::load_cached_device(conn, &id)).await?
    };
    if cached
        .as_ref()
        .is_some_and(|c| c.status == store::DEVICE_STATUS_HIDDEN)
    {
        return Ok(DeviceResult::Unchanged);
    }
    let Some(bytes) = transport
        .get(&manifest_path(device_id), MAX_MANIFEST_BYTES)
        .await?
    else {
        // Directory exists but its first manifest is not written yet.
        return Ok(DeviceResult::Unchanged);
    };
    let manifest_sha = sha256_hex(&bytes);
    let cached = cached.unwrap_or_default();
    if cached.manifest_sha256.as_deref() == Some(manifest_sha.as_str())
        && cached.status == store::DEVICE_STATUS_OK
    {
        let id = device_id.to_string();
        blocking(db, move |conn| store::touch_device_checked(conn, &id, now)).await?;
        return Ok(DeviceResult::Unchanged);
    }

    let manifest = parse_manifest(&bytes)?;
    validate_manifest(&manifest, device_id)?;
    if manifest.seq < cached.manifest_seq {
        // An older manifest (eventually consistent store or a restored copy)
        // must never roll the cache back.
        log::info!(
            "[UsageSync] ignore older manifest for {device_id} (seq {} < {})",
            manifest.seq,
            cached.manifest_seq
        );
        let id = device_id.to_string();
        blocking(db, move |conn| store::touch_device_checked(conn, &id, now)).await?;
        return Ok(DeviceResult::Unchanged);
    }

    let mut changed = Vec::new();
    for (key, meta) in &manifest.partitions {
        if cached.partitions.get(key) == Some(&meta.sha256) {
            continue;
        }
        let bytes = transport
            .get(&object_path(device_id, &meta.object), MAX_SEGMENT_BYTES)
            .await?
            .ok_or_else(|| {
                AppError::localized(
                    "usage_sync.object_missing",
                    format!("远端缺少用量分区文件: {}", meta.object),
                    format!("Remote usage object is missing: {}", meta.object),
                )
            })?;
        let segment = {
            let (meta, key, device_id) = (meta.clone(), key.clone(), device_id.to_string());
            cpu(move || {
                let segment = decode_segment(&bytes, &meta)?;
                validate_segment(&segment, &device_id, &key)?;
                Ok(segment)
            })
            .await?
        };
        changed.push((key.clone(), meta.clone(), segment));
    }
    let removed: Vec<String> = cached
        .partitions
        .keys()
        .filter(|key| !manifest.partitions.contains_key(*key))
        .cloned()
        .collect();

    blocking(db, move |conn| {
        store::apply_remote_device(conn, &manifest, &manifest_sha, &changed, &removed, now)
    })
    .await?;
    Ok(DeviceResult::Updated)
}

async fn download_phase(
    db: &Arc<Database>,
    transport: &UsageTransport,
    self_device_id: &str,
    now: i64,
) -> Result<DownloadOutcome, AppError> {
    let device_ids = transport.list_device_ids().await?;
    let others: Vec<String> = device_ids
        .iter()
        .filter(|id| id.as_str() != self_device_id)
        .cloned()
        .collect();
    {
        let present = others.clone();
        blocking(db, move |conn| {
            store::mark_missing_devices(conn, &present, now)
        })
        .await?;
    }

    let mut outcome = DownloadOutcome {
        devices_seen: others.len(),
        ..Default::default()
    };
    for device_id in others {
        match sync_remote_device(db, transport, &device_id, now).await {
            Ok(DeviceResult::Updated) => outcome.devices_updated += 1,
            Ok(DeviceResult::Unchanged) => {}
            Err(error) => {
                let status = if is_incompatible_error(&error) {
                    store::DEVICE_STATUS_INCOMPATIBLE
                } else {
                    store::DEVICE_STATUS_ERROR
                };
                let message = error.to_string();
                log::warn!("[UsageSync] device {device_id} not imported: {message}");
                {
                    let id = device_id.clone();
                    let message = message.clone();
                    blocking(db, move |conn| {
                        store::record_device_problem(conn, &id, status, &message, now)
                    })
                    .await?;
                }
                outcome.device_errors.push(DeviceSyncError {
                    device_id,
                    error: message,
                });
            }
        }
    }
    Ok(outcome)
}

// ─── Orchestration ───────────────────────────────────────────

/// One full sync cycle against `transport`: publish this device, then import
/// every other device. Upload and download failures are reported separately
/// so an offline-capable read path keeps working when uploads fail.
pub(crate) async fn sync_with(
    db: &Arc<Database>,
    transport: &UsageTransport,
    options: &SyncOptions,
) -> Result<UsageSyncReport, AppError> {
    let identity = blocking(db, store::load_or_create_identity).await?;
    let mut report = UsageSyncReport::default();

    match upload_phase(db, transport, options, &identity).await {
        Ok(outcome) => {
            report.uploaded_partitions = outcome.uploaded_partitions;
            report.manifest_written = outcome.manifest_written;
        }
        Err(error) => {
            report.identity_conflict = is_identity_conflict(&error);
            report.upload_error = Some(error.to_string());
        }
    }

    match download_phase(db, transport, &identity.device_id, options.now).await {
        Ok(outcome) => {
            report.devices_seen = outcome.devices_seen;
            report.devices_updated = outcome.devices_updated;
            report.device_errors = outcome.device_errors;
        }
        Err(error) => report.download_error = Some(error.to_string()),
    }

    if report.devices_updated > 0 {
        crate::usage_events::notify_log_recorded();
    }
    Ok(report)
}

/// Run a sync with the persisted settings and record the outcome.
pub async fn run_sync(db: &Arc<Database>, source: &str) -> Result<UsageSyncReport, AppError> {
    let settings = crate::settings::get_usage_sync_settings();
    if !settings.enabled {
        return Err(not_enabled_error());
    }
    let backend = settings.backend.ok_or_else(backend_missing_error)?;

    let _guard = usage_sync_mutex().lock().await;
    SYNC_RUNNING.store(true, Ordering::SeqCst);
    struct RunningGuard;
    impl Drop for RunningGuard {
        fn drop(&mut self) {
            SYNC_RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _running = RunningGuard;

    let options = SyncOptions::from_settings(&settings);
    let result = match UsageTransport::resolve(backend) {
        Ok(transport) => sync_with(db, &transport, &options).await,
        Err(error) => Err(error),
    };

    let mut status = crate::settings::get_usage_sync_settings().status;
    let now = chrono::Utc::now().timestamp();
    match &result {
        Ok(report) => {
            status.last_sync_at = Some(now);
            if report.upload_error.is_none() {
                status.last_upload_at = Some(now);
            }
            status.identity_conflict = report.identity_conflict;
            match report.first_error() {
                Some(error) => {
                    status.last_error = Some(error);
                    status.last_error_at = Some(now);
                    status.last_error_source = Some(source.to_string());
                }
                None => {
                    status.last_error = None;
                    status.last_error_at = None;
                    status.last_error_source = None;
                }
            }
        }
        Err(error) => {
            status.last_error = Some(error.to_string());
            status.last_error_at = Some(now);
            status.last_error_source = Some(source.to_string());
        }
    }
    if let Err(error) = crate::settings::update_usage_sync_status(status) {
        log::warn!("[UsageSync] failed to persist status: {error}");
    }
    worker::emit_status_changed();
    result
}

// ─── Overview for the UI ─────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSyncThisDevice {
    pub device_id: String,
    pub device_name: String,
    pub custom_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSyncBackendAvailability {
    pub webdav: bool,
    pub s3: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSyncOverview {
    pub settings: UsageSyncSettings,
    pub available_backends: UsageSyncBackendAvailability,
    pub remote_location: Option<String>,
    pub transport_secure: Option<bool>,
    pub this_device: UsageSyncThisDevice,
    pub devices: Vec<RemoteDeviceInfo>,
    pub syncing: bool,
    pub stale_after_secs: i64,
}

pub(crate) fn backend_availability() -> UsageSyncBackendAvailability {
    UsageSyncBackendAvailability {
        webdav: crate::settings::get_webdav_sync_settings().is_some_and(|s| s.validate().is_ok()),
        s3: crate::settings::get_s3_sync_settings().is_some_and(|s| s.validate().is_ok()),
    }
}

pub fn get_overview(db: &Database) -> Result<UsageSyncOverview, AppError> {
    let settings = crate::settings::get_usage_sync_settings();
    let (identity, devices) = {
        let conn = lock_conn!(db.conn);
        (
            store::load_or_create_identity(&conn)?,
            store::list_remote_devices(&conn)?,
        )
    };
    let transport = settings
        .backend
        .and_then(|backend| UsageTransport::resolve(backend).ok());
    let device_name = identity
        .custom_name
        .as_deref()
        .and_then(normalize_device_name)
        .unwrap_or_else(system_device_name);
    Ok(UsageSyncOverview {
        available_backends: backend_availability(),
        remote_location: transport.as_ref().map(UsageTransport::display_location),
        transport_secure: transport.as_ref().map(UsageTransport::is_transport_secure),
        this_device: UsageSyncThisDevice {
            device_id: identity.device_id,
            device_name,
            custom_name: identity.custom_name,
        },
        devices,
        syncing: is_sync_running(),
        stale_after_secs: STALE_AFTER_SECS,
        settings,
    })
}

/// Validate and persist user-editable preferences. The status block and any
/// field not listed here are kept.
pub fn save_preferences(
    enabled: bool,
    auto_sync: bool,
    backend: Option<UsageSyncBackend>,
    detail_level: UsageSyncDetailLevel,
    interval_minutes: u32,
) -> Result<UsageSyncSettings, AppError> {
    let availability = backend_availability();
    if enabled {
        let backend = backend.ok_or_else(backend_missing_error)?;
        let configured = match backend {
            UsageSyncBackend::Webdav => availability.webdav,
            UsageSyncBackend::S3 => availability.s3,
        };
        if !configured {
            return Err(AppError::localized(
                "usage_sync.backend_not_configured",
                "所选云存储尚未完成配置",
                "The selected cloud storage is not fully configured.",
            ));
        }
    }
    let mut settings = crate::settings::get_usage_sync_settings();
    settings.enabled = enabled;
    settings.auto_sync = auto_sync;
    settings.backend = backend;
    settings.detail_level = detail_level;
    settings.interval_minutes = interval_minutes;
    settings.interval_minutes = settings.normalized_interval_minutes();
    if !enabled {
        settings.status.identity_conflict = false;
    }
    crate::settings::set_usage_sync_settings(settings.clone())?;
    worker::notify_settings_changed();
    Ok(settings)
}

pub fn set_device_name(db: &Database, name: Option<String>) -> Result<(), AppError> {
    let normalized = name.as_deref().and_then(normalize_device_name);
    let conn = lock_conn!(db.conn);
    store::set_custom_device_name(&conn, normalized.as_deref())
}

pub fn set_device_hidden(db: &Database, device_id: &str, hidden: bool) -> Result<(), AppError> {
    if !protocol::is_valid_device_id(device_id) {
        return Err(AppError::InvalidInput(format!(
            "invalid device id: {device_id}"
        )));
    }
    {
        let conn = lock_conn!(db.conn);
        store::set_device_hidden(&conn, device_id, hidden)?;
    }
    crate::usage_events::notify_log_recorded();
    Ok(())
}

/// How to resolve an identity conflict.
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictResolution {
    /// Start a new device identity; the old directory remains as history.
    NewIdentity,
    /// Continue as the remote writer (only when the other copy is retired).
    TakeOver,
}

pub async fn resolve_conflict(
    db: &Arc<Database>,
    resolution: ConflictResolution,
) -> Result<(), AppError> {
    let _guard = usage_sync_mutex().lock().await;
    match resolution {
        ConflictResolution::NewIdentity => {
            blocking(db, |conn| store::reset_identity(conn).map(|_| ())).await?;
        }
        ConflictResolution::TakeOver => {
            let settings = crate::settings::get_usage_sync_settings();
            let backend = settings.backend.ok_or_else(backend_missing_error)?;
            let transport = UsageTransport::resolve(backend)?;
            let identity = blocking(db, store::load_or_create_identity).await?;
            if let Some(remote) = read_own_manifest(&transport, &identity.device_id).await? {
                let nonce = remote.writer_nonce.clone();
                let seq = remote.seq;
                blocking(db, move |conn| {
                    store::adopt_remote_writer(conn, &nonce, seq)
                })
                .await?;
            }
        }
    }
    let mut status = crate::settings::get_usage_sync_settings().status;
    status.identity_conflict = false;
    status.last_error = None;
    status.last_error_at = None;
    status.last_error_source = None;
    crate::settings::update_usage_sync_status(status)?;
    worker::emit_status_changed();
    Ok(())
}
