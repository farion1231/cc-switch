//! Independent, opt-in usage snapshots. Never import remote rows into request logs.
//! Persist minimal event identities before rollup so copies and archives do not add usage.
use super::{
    sql_helpers::fresh_input_sql,
    sync_protocol::sha256_hex,
    usage_stats::{effective_model_sql, effective_usage_log_filter},
    webdav,
};
use crate::{
    database::{lock_conn, Database},
    error::AppError,
};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
    sync::{Arc, OnceLock},
    time::Duration,
};

const FORMAT: &str = "cc-switch-usage";
const VERSION: u32 = 1;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_EVENTS: usize = 200_000;
const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_DEVICES: usize = 100;
const INTERVAL_SECONDS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageSyncSettings {
    pub device_id: String,
    pub device_name: String,
    pub enabled: bool,
    pub last_sync_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageEvent {
    pub key: String,
    pub date: String,
    pub app_type: String,
    pub model: String,
    pub requests: u64,
    // Wire format ALWAYS uses fresh-input semantics, independent of tool.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: String,
    // Session identities and pre-opt-in history are not provably unique globally.
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageSnapshot {
    format: String,
    version: u32,
    pub device_id: String,
    pub device_name: String,
    pub revision: u64,
    pub captured_at: i64,
    pub events: Vec<UsageEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Head {
    format: String,
    version: u32,
    device_id: String,
    revision: u64,
    hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDevice {
    pub device_id: String,
    pub device_name: String,
    pub captured_at: i64,
    pub local: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSyncStatus {
    pub settings: UsageSyncSettings,
    pub devices: Vec<UsageDevice>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBucket {
    pub date: String,
    pub app_type: String,
    pub model: String,
    pub verified: bool,
    pub requests: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub devices: Vec<UsageDevice>,
    pub rows: Vec<UsageBucket>,
    pub totals: Vec<UsageBucket>,
    pub conflicting_events: usize,
    pub settings: UsageSyncSettings,
}

fn json_string<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|source| AppError::JsonSerialize { source })
}
fn json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, AppError> {
    serde_json::to_vec(value).map_err(|source| AppError::JsonSerialize { source })
}
fn json_parse<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, AppError> {
    serde_json::from_str(value).map_err(|source| AppError::json("usage snapshot", source))
}
fn json_slice<T: serde::de::DeserializeOwned>(value: &[u8]) -> Result<T, AppError> {
    serde_json::from_slice(value).map_err(|source| AppError::json("usage snapshot", source))
}

pub(crate) fn create_tables(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS usage_sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS usage_sync_events (request_id TEXT PRIMARY KEY, payload TEXT NOT NULL, archived INTEGER NOT NULL DEFAULT 0, data_source TEXT NOT NULL DEFAULT '');
        CREATE TABLE IF NOT EXISTS usage_sync_remote (namespace TEXT NOT NULL, device_id TEXT NOT NULL, revision INTEGER NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(namespace, device_id));")?;
    Ok(())
}

fn meta(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    Ok(conn
        .query_row(
            "SELECT value FROM usage_sync_meta WHERE key=?1",
            [key],
            |r| r.get(0),
        )
        .optional()?)
}
fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute("INSERT INTO usage_sync_meta VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
    Ok(())
}
fn read_settings(conn: &Connection) -> Result<UsageSyncSettings, AppError> {
    let id = match meta(conn, "device_id")? {
        Some(id) => id,
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            set_meta(conn, "device_id", &id)?;
            id
        }
    };
    Ok(UsageSyncSettings {
        device_id: id,
        device_name: meta(conn, "device_name")?.unwrap_or_else(|| "My device".into()),
        enabled: meta(conn, "enabled")?.as_deref() == Some("true"),
        last_sync_at: meta(conn, "last_sync_at")?.and_then(|s| s.parse().ok()),
        last_error: meta(conn, "last_error")?.filter(|s| !s.is_empty()),
    })
}

pub fn settings(db: &Database) -> Result<UsageSyncSettings, AppError> {
    let conn = lock_conn!(db.conn);
    read_settings(&conn)
}

/// The local dashboard needs only settings/device labels, never a usage capture.
pub fn status(db: &Database) -> Result<UsageSyncStatus, AppError> {
    let ns = crate::settings::get_webdav_sync_settings()
        .filter(|s| s.enabled)
        .map(|s| namespace(&s));
    let conn = lock_conn!(db.conn);
    read_status(&conn, ns.as_deref())
}

fn read_status(conn: &Connection, ns: Option<&str>) -> Result<UsageSyncStatus, AppError> {
    let settings = read_settings(conn)?;
    let mut devices = vec![UsageDevice {
        device_id: settings.device_id.clone(),
        device_name: settings.device_name.clone(),
        captured_at: settings.last_sync_at.unwrap_or(0),
        local: true,
    }];
    if let Some(ns) = ns {
        // Extract only metadata: do not deserialize or aggregate remote events.
        let mut stmt = conn.prepare(
            "SELECT device_id, json_extract(payload, '$.deviceName'), json_extract(payload, '$.capturedAt') FROM usage_sync_remote WHERE namespace=?1 ORDER BY device_id",
        )?;
        let remote = stmt.query_map([ns], |r| {
            Ok(UsageDevice {
                device_id: r.get(0)?,
                device_name: r.get(1)?,
                captured_at: r.get(2)?,
                local: false,
            })
        })?;
        devices.extend(remote.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    Ok(UsageSyncStatus { settings, devices })
}

pub fn configure(db: &Database, enabled: bool, name: &str) -> Result<UsageSyncSettings, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(
            "Device name must contain 1–64 printable characters".into(),
        ));
    }
    let conn = lock_conn!(db.conn);
    conn.execute_batch("SAVEPOINT usage_config")?;
    let result = (|| {
        read_settings(&conn)?;
        // History is captured ONCE, before any later local rollups accumulate.
        if enabled && meta(&conn, "initialized")?.is_none() {
            set_meta(&conn, "initialized", "true")?;
            set_meta(&conn, "enabled", "true")?;
            set_meta(&conn, "started_at", &Utc::now().timestamp().to_string())?;
            capture_history(&conn)?;
            capture_on_conn(&conn, true)?;
        }
        set_meta(&conn, "enabled", if enabled { "true" } else { "false" })?;
        set_meta(&conn, "device_name", name)?;
        read_settings(&conn)
    })();
    finish_savepoint(&conn, "usage_config", result)
}
fn finish_savepoint<T>(
    conn: &Connection,
    name: &str,
    result: Result<T, AppError>,
) -> Result<T, AppError> {
    match result {
        Ok(value) => {
            conn.execute_batch(&format!("RELEASE {name}"))?;
            Ok(value)
        }
        Err(error) => {
            let _ = conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
            Err(error)
        }
    }
}
fn store_event(
    conn: &Connection,
    id: &str,
    event: &UsageEvent,
    source: &str,
    archived: bool,
) -> Result<(), AppError> {
    let json = json_string(event)?;
    conn.execute("INSERT INTO usage_sync_events (request_id,payload,data_source,archived) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(request_id) DO UPDATE SET payload=excluded.payload,data_source=excluded.data_source WHERE usage_sync_events.payload != excluded.payload OR usage_sync_events.data_source != excluded.data_source", params![id, json, source, archived])?;
    Ok(())
}

fn capture_history(conn: &Connection) -> Result<(), AppError> {
    let fresh = fresh_input_sql("r");
    // Keep raw model fields in the history identity; project only the display model.
    let model = effective_model_sql("r");
    let mut stmt = conn.prepare(&format!("SELECT date, app_type, model, request_count, {fresh}, output_tokens, cache_read_tokens, cache_creation_tokens, total_cost_usd, provider_id, request_model, pricing_model, {model} FROM usage_daily_rollups r"))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let fields: Vec<String> = [0, 1, 2, 9, 10, 11]
            .iter()
            .map(|i| r.get(*i))
            .collect::<rusqlite::Result<_>>()?;
        let id = format!("history:{}", sha256_hex(json_string(&fields)?.as_bytes()));
        store_event(
            conn,
            &id,
            &UsageEvent {
                key: id.clone(),
                date: r.get(0)?,
                app_type: r.get(1)?,
                model: r.get(12)?,
                requests: r.get(3)?,
                input: r.get(4)?,
                output: r.get(5)?,
                cache_read: r.get(6)?,
                cache_write: r.get(7)?,
                cost: r.get(8)?,
                verified: false,
            },
            if r.get::<_, String>(9)? == "_codex_session" {
                "codex_session"
            } else {
                "historical"
            },
            true,
        )?;
    }
    Ok(())
}

/// Called within the rollup savepoint, even while network sync is disabled.
/// Only minimal usage survives pruning; no body, credentials or filesystem path.
pub(crate) fn capture_on_conn(conn: &Connection, historical: bool) -> Result<(), AppError> {
    capture_events(conn, historical, None)
}

pub(crate) fn capture_before_prune(conn: &Connection, cutoff: i64) -> Result<(), AppError> {
    capture_events(conn, false, Some(cutoff))
}

fn capture_events(
    conn: &Connection,
    historical: bool,
    cutoff: Option<i64>,
) -> Result<(), AppError> {
    if meta(conn, "initialized")?.is_none() {
        return Ok(());
    }
    let settings = read_settings(conn)?;
    let started: i64 = meta(conn, "started_at")?
        .and_then(|s| s.parse().ok())
        .unwrap_or(i64::MAX);
    if cutoff.is_none() {
        conn.execute("DELETE FROM usage_sync_events WHERE archived=0 AND request_id NOT IN (SELECT request_id FROM proxy_request_logs)", [])?;
    }
    let filter = effective_usage_log_filter("l");
    // If a live session row is now suppressed by proxy dedup, remove its earlier receipt.
    conn.execute(&format!("DELETE FROM usage_sync_events WHERE request_id IN (SELECT l.request_id FROM proxy_request_logs l WHERE NOT ({filter}) AND (?1 IS NULL OR l.created_at < ?1))"), [cutoff])?;
    let fresh = fresh_input_sql("l");
    let model = effective_model_sql("l");
    let mut stmt = conn.prepare(&format!("SELECT request_id, date(created_at, 'unixepoch'), app_type, {model}, {fresh}, output_tokens, cache_read_tokens, cache_creation_tokens, total_cost_usd, COALESCE(data_source, 'proxy'), input_token_semantics, created_at FROM proxy_request_logs l WHERE {filter} AND (?1 IS NULL OR l.created_at < ?1) ORDER BY request_id"))?;
    let mut rows = stmt.query([cutoff])?;
    while let Some(r) = rows.next()? {
        let id: String = r.get(0)?;
        let app: String = r.get(2)?;
        let source: String = r.get(9)?;
        let session = source != "proxy";
        let key = sha256_hex(
            json_string(&(
                "usage-event-v1",
                &app,
                &source,
                if session { "" } else { &settings.device_id },
                &id,
            ))?
            .as_bytes(),
        );
        // Stable session IDs deduplicate copies, but cross-device proxy/session correlation
        // and Codex positional IDs are not proof of uniqueness. Keep them unverified.
        let mut event = UsageEvent {
            key,
            date: r.get(1)?,
            app_type: app,
            model: r.get(3)?,
            requests: 1,
            input: r.get(4)?,
            output: r.get(5)?,
            cache_read: r.get(6)?,
            cache_write: r.get(7)?,
            cost: r.get(8)?,
            verified: !historical
                && !session
                && matches!(r.get::<_, i64>(10)?, 1 | 2)
                && r.get::<_, i64>(11)? >= started,
        };
        // Once historical/uncertain, refreshing a price must not promote provenance.
        if let Some(old) = conn
            .query_row(
                "SELECT payload FROM usage_sync_events WHERE request_id=?1",
                [&id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            event.verified &= json_parse::<UsageEvent>(&old)?.verified;
        }
        store_event(conn, &id, &event, &source, false)?;
    }
    Ok(())
}

/// Mark only rows about to be pruned; ordinary deletes must remove their receipts.
pub(crate) fn mark_archived(conn: &Connection, cutoff: i64) -> Result<(), AppError> {
    if meta(conn, "initialized")?.is_none() {
        return Ok(());
    }
    conn.execute("UPDATE usage_sync_events SET archived=1 WHERE request_id IN (SELECT request_id FROM proxy_request_logs WHERE created_at < ?1)", [cutoff])?;
    Ok(())
}

fn build_snapshot(db: &Database) -> Result<UsageSnapshot, AppError> {
    let conn = lock_conn!(db.conn);
    conn.execute_batch("SAVEPOINT usage_snapshot")?;
    let result = (|| {
        capture_on_conn(&conn, false)?;
        let settings = read_settings(&conn)?;
        let revision = meta(&conn, "revision")?
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| AppError::InvalidInput("Usage revision overflow".into()))?;
        set_meta(&conn, "revision", &revision.to_string())?;
        let mut stmt = conn.prepare("SELECT payload FROM usage_sync_events ORDER BY request_id")?;
        let payloads = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let snapshot = UsageSnapshot {
            format: FORMAT.into(),
            version: VERSION,
            device_id: settings.device_id,
            device_name: settings.device_name,
            revision,
            captured_at: Utc::now().timestamp(),
            events: payloads
                .iter()
                .map(|p| json_parse(p))
                .collect::<Result<_, _>>()?,
        };
        validate(&snapshot)?;
        Ok(snapshot)
    })();
    finish_savepoint(&conn, "usage_snapshot", result)
}

fn validate(snapshot: &UsageSnapshot) -> Result<(), AppError> {
    if snapshot.format != FORMAT
        || snapshot.version != VERSION
        || !valid_id(&snapshot.device_id)
        || snapshot.revision == 0
        || snapshot.revision > i64::MAX as u64
        || snapshot.device_name.chars().count() > 64
        || snapshot.device_name.chars().any(char::is_control)
        || snapshot.events.len() > MAX_EVENTS
    {
        return Err(AppError::InvalidInput(
            "Unsupported or invalid usage snapshot".into(),
        ));
    }
    let mut keys = BTreeSet::new();
    for e in &snapshot.events {
        let date = chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d");
        let cost = Decimal::from_str(&e.cost);
        if e.key.len() > 80
            || !keys.insert(&e.key)
            || date.is_err()
            || e.app_type.len() > 64
            || e.model.len() > 256
            || e.app_type.chars().any(char::is_control)
            || e.model.chars().any(char::is_control)
            || cost.is_err()
            || cost.is_ok_and(|v| v < Decimal::ZERO)
            || [e.requests, e.input, e.output, e.cache_read, e.cache_write]
                .iter()
                .any(|n| *n > 1_000_000_000_000)
        {
            return Err(AppError::InvalidInput("Invalid usage event".into()));
        }
    }
    Ok(())
}
fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|u| u.to_string() == id)
}
fn mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}
fn namespace(settings: &crate::settings::WebDavSyncSettings) -> String {
    // Remote caches from different servers/profiles must never mix.
    sha256_hex(
        json_string(&(
            &settings.base_url,
            &settings.remote_root,
            &settings.profile,
            &settings.username,
        ))
        .unwrap_or_default()
        .as_bytes(),
    )
}
fn directory(settings: &crate::settings::WebDavSyncSettings) -> Vec<String> {
    webdav::path_segments(&settings.remote_root)
        .chain(webdav::path_segments(&settings.profile))
        .map(str::to_string)
        .chain(["usage-v1".into(), "devices".into()])
        .collect()
}
fn file_url(
    settings: &crate::settings::WebDavSyncSettings,
    device: &str,
    file: &str,
) -> Result<String, AppError> {
    let mut path = directory(settings);
    path.push(device.into());
    path.push(file.into());
    webdav::build_remote_url(&settings.base_url, &path)
}
fn import_snapshot(
    db: &Database,
    namespace: &str,
    snapshot: &UsageSnapshot,
) -> Result<(), AppError> {
    validate(snapshot)?;
    let conn = lock_conn!(db.conn);
    let local = read_settings(&conn)?;
    if local.device_id == snapshot.device_id {
        return Ok(());
    }
    let json = json_string(snapshot)?;
    let previous = conn
        .query_row(
            "SELECT revision, payload FROM usage_sync_remote WHERE namespace=?1 AND device_id=?2",
            params![namespace, snapshot.device_id],
            |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some((revision, payload)) = previous {
        if snapshot.revision < revision {
            return Ok(());
        }
        if snapshot.revision == revision {
            return if payload == json {
                Ok(())
            } else {
                Err(AppError::InvalidInput(
                    "Conflicting device revision; device identity may have been cloned".into(),
                ))
            };
        }
    }
    let other_bytes: usize = conn.query_row(
        "SELECT COALESCE(SUM(length(CAST(payload AS BLOB))),0) FROM usage_sync_remote WHERE namespace=?1 AND device_id != ?2",
        params![namespace, snapshot.device_id], |r| r.get(0),
    )?;
    if other_bytes.saturating_add(json.len()) > MAX_CACHE_BYTES {
        return Err(AppError::InvalidInput(
            "Usage cache exceeds 128 MiB; retained last good data".into(),
        ));
    }
    conn.execute("INSERT INTO usage_sync_remote VALUES (?1, ?2, ?3, ?4) ON CONFLICT(namespace,device_id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload", params![namespace, snapshot.device_id, snapshot.revision, json])?;
    Ok(())
}

pub async fn sync(db: Arc<Database>) -> Result<(), AppError> {
    let _guard = mutex().lock().await;
    let result = sync_inner(db.clone()).await;
    let conn = lock_conn!(db.conn);
    match &result {
        Ok(()) => {
            set_meta(&conn, "last_sync_at", &Utc::now().timestamp().to_string())?;
            set_meta(&conn, "last_error", "")?;
        }
        Err(error) => {
            set_meta(&conn, "last_error", &error.to_string())?;
        }
    }
    result
}
async fn sync_inner(db: Arc<Database>) -> Result<(), AppError> {
    if !settings(&db)?.enabled {
        return Err(AppError::InvalidInput("Usage sync is disabled".into()));
    }
    let remote = crate::settings::get_webdav_sync_settings()
        .filter(|s| s.enabled)
        .ok_or_else(|| AppError::InvalidInput("Enable WebDAV before enabling usage sync".into()))?;
    exchange(db, &remote).await
}

async fn exchange(
    db: Arc<Database>,
    remote: &crate::settings::WebDavSyncSettings,
) -> Result<(), AppError> {
    remote.validate()?;
    let auth = webdav::auth_from_credentials(&remote.username, &remote.password);
    let snapshot_db = db.clone();
    let snapshot = tokio::task::spawn_blocking(move || build_snapshot(&snapshot_db))
        .await
        .map_err(|e| AppError::Message(format!("Usage snapshot task failed: {e}")))??;
    let mut dir = directory(remote);
    dir.push(snapshot.device_id.clone());
    webdav::ensure_remote_directories(&remote.base_url, &dir, &auth).await?;
    let head_url = file_url(remote, &snapshot.device_id, "head.json")?;
    let previous = webdav::get_bytes(&head_url, &auth, 4096).await?;
    let mut previous_hash = String::new();
    let previous_etag = if let Some((bytes, etag)) = previous {
        let previous: Head = json_slice(&bytes)?;
        if previous.format != FORMAT
            || previous.version != VERSION
            || previous.device_id != snapshot.device_id
            || previous.revision >= snapshot.revision
        {
            return Err(AppError::Conflict("Remote device revision is newer; do not reuse a device identity on another installation".into()));
        }
        previous_hash = previous.hash.clone();
        Some(etag.ok_or_else(|| {
            AppError::Conflict("WebDAV must return ETags for usage synchronization".into())
        })?)
    } else {
        None
    };
    let signature = sha256_hex(json_bytes(&(&snapshot.device_name, &snapshot.events))?.as_slice());
    let published_key = format!("published:{}", namespace(remote));
    let previous_signature = {
        let conn = lock_conn!(db.conn);
        meta(&conn, &published_key)?
    };
    let unchanged = previous_etag.is_some()
        && previous_signature.as_deref() == Some(&format!("{signature}:{previous_hash}"));
    if !unchanged {
        let bytes = json_bytes(&snapshot)?;
        if bytes.len() > MAX_BYTES {
            return Err(AppError::InvalidInput(
                "Usage snapshot exceeds 64 MiB; no data was uploaded".into(),
            ));
        }
        let hash = sha256_hex(&bytes);
        // Immutable artifact + last-published head prevents readers seeing mixed generations.
        webdav::put_bytes(
            &file_url(remote, &snapshot.device_id, &format!("{hash}.json"))?,
            &auth,
            bytes,
            "application/json",
        )
        .await?;
        let head = Head {
            format: FORMAT.into(),
            version: VERSION,
            device_id: snapshot.device_id.clone(),
            revision: snapshot.revision,
            hash,
        };
        webdav::put_usage_head(
            &head_url,
            &auth,
            json_bytes(&head)?,
            previous_etag.as_deref(),
        )
        .await?;
        {
            let conn = lock_conn!(db.conn);
            set_meta(&conn, &published_key, &format!("{signature}:{}", head.hash))?;
        }
    }
    let root = webdav::build_remote_url(&remote.base_url, &directory(remote))?;
    let ids = webdav::list_usage_devices(&root, &auth, MAX_DEVICES).await?;
    let ns = namespace(remote);
    let mut errors = Vec::new();
    for id in ids {
        if id == snapshot.device_id {
            continue;
        }
        let result: Result<(), AppError> = async {
            let Some((bytes, _)) =
                webdav::get_bytes(&file_url(remote, &id, "head.json")?, &auth, 4096).await?
            else {
                return Ok(());
            };
            let head: Head = json_slice(&bytes)?;
            if head.format != FORMAT
                || head.version != VERSION
                || head.device_id != id
                || head.hash.len() != 64
                || !head.hash.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(AppError::InvalidInput("Incompatible usage head".into()));
            }
            let Some((bytes, _)) = webdav::get_bytes(
                &file_url(remote, &id, &format!("{}.json", head.hash))?,
                &auth,
                MAX_BYTES,
            )
            .await?
            else {
                return Err(AppError::InvalidInput("Incomplete usage snapshot".into()));
            };
            if sha256_hex(&bytes) != head.hash {
                return Err(AppError::InvalidInput(
                    "Usage snapshot checksum mismatch".into(),
                ));
            }
            let incoming: UsageSnapshot = json_slice(&bytes)?;
            if incoming.device_id != id || incoming.revision != head.revision {
                return Err(AppError::InvalidInput(
                    "Usage snapshot identity mismatch".into(),
                ));
            }
            let import_db = db.clone();
            let import_ns = ns.clone();
            tokio::task::spawn_blocking(move || import_snapshot(&import_db, &import_ns, &incoming))
                .await
                .map_err(|e| AppError::Message(format!("Usage import task failed: {e}")))?
        }
        .await;
        if result.is_err() {
            errors.push(id);
        }
    }
    if !errors.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "Usage snapshots unavailable or incompatible for {} device(s); retained last good data",
            errors.len()
        )));
    }
    Ok(())
}

/// A dedicated periodic worker: one snapshot per minute at most; no config export.
/// Always retries on subsequent ticks, including after restart or offline failures.
pub fn start_worker(db: Arc<Database>) {
    tauri::async_runtime::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(INTERVAL_SECONDS));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            let enabled = db.conn.lock().is_ok_and(|conn| {
                meta(&conn, "enabled").is_ok_and(|s| s.as_deref() == Some("true"))
            });
            if enabled {
                if let Err(error) = sync(db.clone()).await {
                    log::debug!("Usage sync retry pending: {error}");
                }
            }
        }
    });
}

pub fn report(
    db: &Database,
    scope: &str,
    start: Option<String>,
    end: Option<String>,
    app: Option<String>,
    model: Option<String>,
) -> Result<UsageReport, AppError> {
    let local = build_snapshot(db)?;
    let settings = settings(db)?;
    let mut snapshots = vec![local];
    if let Some(remote) = crate::settings::get_webdav_sync_settings().filter(|s| s.enabled) {
        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT payload FROM usage_sync_remote WHERE namespace=?1 ORDER BY device_id",
        )?;
        let values = stmt
            .query_map([namespace(&remote)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for value in values {
            snapshots.push(json_parse(&value)?);
        }
    }
    aggregate(snapshots, settings, scope, start, end, app, model)
}
fn aggregate(
    snapshots: Vec<UsageSnapshot>,
    settings: UsageSyncSettings,
    scope: &str,
    start: Option<String>,
    end: Option<String>,
    app: Option<String>,
    model: Option<String>,
) -> Result<UsageReport, AppError> {
    if scope != "all" && scope != "local" && !valid_id(scope) {
        return Err(AppError::InvalidInput("Invalid device scope".into()));
    }
    for date in [&start, &end].into_iter().flatten() {
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| AppError::InvalidInput("Invalid usage date".into()))?;
    }
    let devices = snapshots
        .iter()
        .map(|s| UsageDevice {
            device_id: s.device_id.clone(),
            device_name: s.device_name.clone(),
            captured_at: s.captured_at,
            local: s.device_id == settings.device_id,
        })
        .collect();
    let mut unique: BTreeMap<String, UsageEvent> = BTreeMap::new();
    let mut conflicts = BTreeSet::new();
    for snapshot in snapshots {
        if scope == "local" && snapshot.device_id != settings.device_id
            || scope != "local" && scope != "all" && scope != snapshot.device_id
        {
            continue;
        }
        for event in snapshot.events {
            // Unknown historical rollup IDs are device scoped; never silently dedup sums.
            let key = if event.key.starts_with("history:") {
                format!("{}:{}", snapshot.device_id, event.key)
            } else {
                event.key.clone()
            };
            if let Some(previous) = unique.get(&key) {
                let mut a = previous.clone();
                let mut b = event.clone();
                a.verified = false;
                b.verified = false;
                if a != b {
                    conflicts.insert(key);
                } else if !event.verified {
                    unique.get_mut(&key).unwrap().verified = false;
                }
            } else {
                unique.insert(key, event);
            }
        }
    }
    let mut buckets: BTreeMap<(String, String, String, bool), (UsageBucket, Decimal)> =
        BTreeMap::new();
    for (key, e) in unique {
        // Conflicting observations are omitted entirely instead of selecting an arbitrary winner.
        if conflicts.contains(&key)
            || start.as_ref().is_some_and(|s| e.date < *s)
            || end.as_ref().is_some_and(|s| e.date > *s)
            || app.as_ref().is_some_and(|a| e.app_type != *a)
            || model.as_ref().is_some_and(|m| e.model != *m)
        {
            continue;
        }
        let k = (
            e.date.clone(),
            e.app_type.clone(),
            e.model.clone(),
            e.verified,
        );
        let (bucket, cost) = buckets.entry(k).or_insert_with(|| {
            (
                UsageBucket {
                    date: e.date,
                    app_type: e.app_type,
                    model: e.model,
                    verified: e.verified,
                    ..Default::default()
                },
                Decimal::ZERO,
            )
        });
        for (target, value) in [
            (&mut bucket.requests, e.requests),
            (&mut bucket.input, e.input),
            (&mut bucket.output, e.output),
            (&mut bucket.cache_read, e.cache_read),
            (&mut bucket.cache_write, e.cache_write),
        ] {
            *target = target
                .checked_add(value)
                .ok_or_else(|| AppError::InvalidInput("Usage total overflow".into()))?;
        }
        *cost = cost
            .checked_add(
                Decimal::from_str(&e.cost)
                    .map_err(|_| AppError::InvalidInput("Invalid usage cost".into()))?,
            )
            .ok_or_else(|| AppError::InvalidInput("Usage cost overflow".into()))?;
    }
    let rows: Vec<UsageBucket> = buckets
        .into_values()
        .map(|(mut b, c)| {
            b.cost = c.normalize().to_string();
            b
        })
        .collect();
    let mut totals = Vec::new();
    for verified in [true, false] {
        let mut total = UsageBucket {
            verified,
            ..Default::default()
        };
        let mut cost = Decimal::ZERO;
        for row in rows.iter().filter(|r| r.verified == verified) {
            for (target, value) in [
                (&mut total.requests, row.requests),
                (&mut total.input, row.input),
                (&mut total.output, row.output),
                (&mut total.cache_read, row.cache_read),
                (&mut total.cache_write, row.cache_write),
            ] {
                *target = target
                    .checked_add(value)
                    .filter(|v| *v <= 9_007_199_254_740_991)
                    .ok_or_else(|| {
                        AppError::InvalidInput("Usage total exceeds supported precision".into())
                    })?;
            }
            cost = cost
                .checked_add(
                    Decimal::from_str(&row.cost)
                        .map_err(|_| AppError::InvalidInput("Invalid usage cost".into()))?,
                )
                .ok_or_else(|| AppError::InvalidInput("Usage cost overflow".into()))?;
        }
        total.cost = cost.normalize().to_string();
        totals.push(total);
    }
    Ok(UsageReport {
        devices,
        rows,
        totals,
        conflicting_events: conflicts.len(),
        settings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn add(
        db: &Database,
        id: &str,
        source: &str,
        app: &str,
        semantics: i64,
        timestamp: i64,
    ) {
        let conn = db.conn.lock().unwrap();
        conn.execute("INSERT INTO proxy_request_logs (request_id,provider_id,app_type,model,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,input_token_semantics,total_cost_usd,status_code,created_at,data_source,latency_ms) VALUES (?1,'fixture',?2,'synthetic-model',100,10,30,20,?3,'0.123456789',200,?4,?5,1)", params![id,app,semantics,timestamp,source]).unwrap();
    }
    fn initialized() -> Database {
        let db = Database::memory().unwrap();
        configure(&db, true, "Fixture device").unwrap();
        set_meta(&db.conn.lock().unwrap(), "started_at", "0").unwrap();
        db
    }
    fn combined(snapshots: Vec<UsageSnapshot>, settings: UsageSyncSettings) -> UsageReport {
        aggregate(snapshots, settings, "all", None, None, None, None).unwrap()
    }
    #[test]
    fn snapshot_models_match_local_pricing_models_for_detail_and_history() {
        for history in [false, true] {
            let db = Database::memory().unwrap();
            for (index, pricing) in ["request-a", "request-b", ""].iter().enumerate() {
                let cost = format!("0.{}", index + 1);
                if history {
                    db.conn.lock().unwrap().execute(
                        "INSERT INTO usage_daily_rollups (date,app_type,provider_id,model,request_model,pricing_model,request_count,success_count,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,input_token_semantics,total_cost_usd,avg_latency_ms) VALUES ('2020-01-01','claude','fixture','upstream-model','',?1,1,1,100,10,30,20,2,?2,1)",
                        params![pricing, cost],
                    ).unwrap();
                } else {
                    let id = format!("pricing-{index}");
                    add(&db, &id, "proxy", "claude", 2, 1_700_000_000);
                    db.conn.lock().unwrap().execute(
                        "UPDATE proxy_request_logs SET model='upstream-model', pricing_model=?1, total_cost_usd=?3 WHERE request_id=?2",
                        params![pricing, id, cost],
                    ).unwrap();
                }
            }
            configure(&db, true, "Pricing fixture").unwrap();
            let snapshot = build_snapshot(&db).unwrap();
            let keys: Vec<_> = snapshot.events.iter().map(|e| e.key.clone()).collect();
            let report = combined(vec![snapshot.clone()], settings(&db).unwrap());
            let local = db.get_model_stats(None, None, None, None, None).unwrap();
            assert_eq!(report.rows.len(), 3);
            assert_eq!(local.len(), 3);
            for stat in local {
                let bucket = report.rows.iter().find(|r| r.model == stat.model).unwrap();
                assert_eq!(bucket.requests, stat.request_count);
                assert_eq!(
                    Decimal::from_str(&bucket.cost).unwrap(),
                    Decimal::from_str(&stat.total_cost).unwrap()
                );
            }
            if history {
                let conn = db.conn.lock().unwrap();
                let fields = vec![
                    "2020-01-01",
                    "claude",
                    "upstream-model",
                    "fixture",
                    "",
                    "request-a",
                ];
                let expected = format!(
                    "history:{}",
                    sha256_hex(json_string(&fields).unwrap().as_bytes())
                );
                assert!(keys.contains(&expected));
                capture_history(&conn).unwrap();
            } else {
                assert_eq!(db.rollup_and_prune(30).unwrap(), 3);
            }
            assert_eq!(snapshot.events, build_snapshot(&db).unwrap().events);
        }
    }
    #[test]
    fn local_status_does_not_capture_usage_or_increment_revision() {
        let local = initialized();
        let remote = initialized();
        let snapshot = build_snapshot(&remote).unwrap();
        import_snapshot(&local, "fixture-namespace", &snapshot).unwrap();
        let conn = local.conn.lock().unwrap();
        // Status must work without the local logs, rollups, or a valid event ledger.
        conn.execute_batch("DROP TABLE proxy_request_logs; DROP TABLE usage_daily_rollups; INSERT INTO usage_sync_events VALUES ('fixture', 'not-json', 0, 'proxy');
            CREATE TRIGGER status_must_not_write_meta BEFORE UPDATE ON usage_sync_meta BEGIN SELECT RAISE(ABORT, 'status wrote meta'); END;
            CREATE TRIGGER status_must_not_write_events BEFORE DELETE ON usage_sync_events BEGIN SELECT RAISE(ABORT, 'status wrote events'); END;").unwrap();
        let status = read_status(&conn, Some("fixture-namespace")).unwrap();
        assert_eq!(status.devices.len(), 2);
        assert!(status.devices[0].local);
        assert_eq!(status.devices[0].captured_at, 0);
        assert_eq!(status.devices[1].device_id, snapshot.device_id);
        assert_eq!(meta(&conn, "revision").unwrap(), None);
        assert_eq!(
            read_status(&conn, Some("other-namespace"))
                .unwrap()
                .devices
                .len(),
            1
        );
    }

    #[test]
    fn never_enabled_sync_does_not_capture_during_local_pruning() {
        let db = Database::memory().unwrap();
        add(
            &db,
            "old",
            "proxy",
            "claude",
            2,
            Utc::now().timestamp() - 100 * 86400,
        );
        db.conn.lock().unwrap().execute_batch(
            "CREATE TRIGGER unused_feature_must_not_capture BEFORE INSERT ON usage_sync_events BEGIN SELECT RAISE(ABORT, 'unexpected capture'); END;",
        ).unwrap();
        assert_eq!(db.rollup_and_prune(30).unwrap(), 1);
        let conn = db.conn.lock().unwrap();
        assert_eq!(meta(&conn, "device_id").unwrap(), None);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM usage_sync_events", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT request_count FROM usage_daily_rollups", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            1
        );
    }

    #[test]
    fn archive_capture_does_not_scan_recent_logs() {
        let db = initialized();
        let now = Utc::now().timestamp();
        add(&db, "old", "proxy", "claude", 2, now - 100 * 86400);
        add(&db, "recent", "proxy", "claude", 2, now);
        // A recent row that cannot be captured must not block unrelated old-log pruning.
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE proxy_request_logs SET input_tokens=-1 WHERE request_id='recent'",
                [],
            )
            .unwrap();
        assert_eq!(db.rollup_and_prune(30).unwrap(), 1);
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT request_id FROM usage_sync_events", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "old"
        );
        assert_eq!(
            conn.query_row("SELECT request_id FROM proxy_request_logs", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "recent"
        );
    }
    #[test]
    fn snapshots_are_idempotent_isolated_and_reject_stale_or_conflicting_revisions() {
        let local = initialized();
        let remote = initialized();
        add(
            &remote,
            "synthetic-1",
            "proxy",
            "claude",
            2,
            Utc::now().timestamp(),
        );
        let snapshot = build_snapshot(&remote).unwrap();
        let id = settings(&local).unwrap().device_id;
        import_snapshot(&local, "fixture-namespace", &snapshot).unwrap();
        import_snapshot(&local, "fixture-namespace", &snapshot).unwrap();
        let conn = local.conn.lock().unwrap();
        let logs: i64 = conn
            .query_row("SELECT count(*) FROM proxy_request_logs", [], |r| r.get(0))
            .unwrap();
        let cache: i64 = conn
            .query_row("SELECT count(*) FROM usage_sync_remote", [], |r| r.get(0))
            .unwrap();
        assert_eq!((logs, cache), (0, 1));
        drop(conn);
        assert_eq!(settings(&local).unwrap().device_id, id);
        assert!(
            build_snapshot(&local).unwrap().events.is_empty(),
            "remote data must never be re-uploaded"
        );
        let mut conflict = snapshot.clone();
        conflict.events[0].output += 1;
        assert!(import_snapshot(&local, "fixture-namespace", &conflict).is_err());
        let newer = build_snapshot(&remote).unwrap();
        import_snapshot(&local, "fixture-namespace", &newer).unwrap();
        import_snapshot(&local, "fixture-namespace", &snapshot).unwrap();
        assert_eq!(
            local
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT revision FROM usage_sync_remote", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            newer.revision
        );
        import_snapshot(&local, "other-server", &snapshot).unwrap();
        assert_eq!(
            local
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM usage_sync_remote", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    #[test]
    fn pruning_keeps_identity_and_never_adds_rollup_to_the_same_event() {
        let db = initialized();
        add(
            &db,
            "old-proxy",
            "proxy",
            "codex",
            1,
            Utc::now().timestamp() - 100 * 86400,
        );
        let before = build_snapshot(&db).unwrap();
        assert_eq!(
            before.events[0].input, 50,
            "total semantics subtract both cache reads and writes"
        );
        assert_eq!(db.rollup_and_prune(30).unwrap(), 1);
        let after = build_snapshot(&db).unwrap();
        assert_eq!(before.events, after.events);
        assert_eq!(db.rollup_and_prune(30).unwrap(), 0);
        let totals = combined(vec![after], settings(&db).unwrap());
        assert_eq!(totals.rows[0].requests, 1);
        assert_eq!(totals.rows[0].cost, "0.123456789");
    }
    #[test]
    fn copies_dedup_but_conflicts_do_not_silently_choose_a_winner() {
        let a = initialized();
        let b = initialized();
        add(
            &a,
            "session:synthetic-message",
            "session_log",
            "claude",
            2,
            Utc::now().timestamp(),
        );
        add(
            &b,
            "session:synthetic-message",
            "session_log",
            "claude",
            2,
            Utc::now().timestamp(),
        );
        let first = build_snapshot(&a).unwrap();
        let mut second = build_snapshot(&b).unwrap();
        let report = combined(vec![first.clone(), second.clone()], settings(&a).unwrap());
        assert_eq!(report.rows[0].requests, 1);
        assert!(!report.rows[0].verified);
        second.events[0].output += 1;
        let report = combined(vec![second, first], settings(&a).unwrap());
        assert_eq!(report.conflicting_events, 1);
        assert!(report.rows.is_empty());
    }
    #[test]
    fn legacy_input_history_and_daily_rollups_remain_unverified() {
        let db = Database::memory().unwrap();
        add(&db, "historical-proxy", "proxy", "codex", 0, 1_700_000_000);
        db.conn.lock().unwrap().execute("INSERT INTO usage_daily_rollups (date,app_type,provider_id,model,request_model,pricing_model,request_count,success_count,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,input_token_semantics,total_cost_usd,avg_latency_ms) VALUES ('2020-01-01','codex','fixture','synthetic-model','','',1,1,100,10,30,20,1,'0.1',1)",[]).unwrap();
        configure(&db, true, "History fixture").unwrap();
        let initial = build_snapshot(&db).unwrap();
        assert_eq!(initial.events.len(), 2);
        assert!(initial.events.iter().all(|e| !e.verified));
        assert_eq!(
            initial
                .events
                .iter()
                .find(|e| e.key.starts_with("history:"))
                .unwrap()
                .input,
            50
        );
        assert_eq!(
            initial
                .events
                .iter()
                .find(|e| !e.key.starts_with("history:"))
                .unwrap()
                .input,
            70
        );
        db.rollup_and_prune(30).unwrap();
        assert_eq!(initial.events, build_snapshot(&db).unwrap().events);
        configure(&db, false, "History fixture").unwrap();
        configure(&db, true, "Renamed").unwrap();
        assert_eq!(build_snapshot(&db).unwrap().events.len(), 2);
    }
    #[test]
    fn wire_has_no_credentials_request_ids_paths_or_bodies() {
        let db = initialized();
        add(
            &db,
            "secret-local-id",
            "proxy",
            "claude",
            2,
            Utc::now().timestamp(),
        );
        let wire = json_string(&build_snapshot(&db).unwrap()).unwrap();
        for forbidden in [
            "secret-local-id",
            "requestId",
            "providerId",
            "sessionId",
            "apiKey",
            "filePath",
            "body",
            "errorMessage",
        ] {
            assert!(!wire.contains(forbidden));
        }
        let mut snapshot = build_snapshot(&db).unwrap();
        snapshot.version += 1;
        assert!(validate(&snapshot).is_err());
        snapshot.version = VERSION;
        snapshot.events.push(snapshot.events[0].clone());
        assert!(validate(&snapshot).is_err());
    }
    #[test]
    fn date_app_model_and_device_filters_are_consistent() {
        let a = initialized();
        let b = initialized();
        add(&a, "a", "proxy", "claude", 2, 1_700_000_000);
        add(&b, "b", "proxy", "codex", 1, 1_700_000_000);
        let settings = settings(&a).unwrap();
        let sa = build_snapshot(&a).unwrap();
        let sb = build_snapshot(&b).unwrap();
        let report = aggregate(
            vec![sa.clone(), sb.clone()],
            settings.clone(),
            "all",
            Some("2023-11-14".into()),
            Some("2023-11-14".into()),
            Some("codex".into()),
            Some("synthetic-model".into()),
        )
        .unwrap();
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].input, 50);
        let report = aggregate(
            vec![sa, sb.clone()],
            settings,
            &sb.device_id,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].app_type, "codex");
    }
    #[test]
    fn config_export_skips_device_identity_ledger_and_remote_data() {
        let db = initialized();
        add(
            &db,
            "local-only",
            "proxy",
            "claude",
            2,
            Utc::now().timestamp(),
        );
        build_snapshot(&db).unwrap();
        let id = settings(&db).unwrap().device_id;
        let sql = db.export_sql_string_for_sync().unwrap();
        assert!(!sql.contains(&id));
        assert!(!sql.contains("local-only"));
        assert!(!sql.contains("Fixture device"));
    }
    #[test]
    fn capture_failure_rolls_back_configuration_and_pruning() {
        let db = Database::memory().unwrap();
        add(
            &db,
            "invalid",
            "proxy",
            "claude",
            2,
            Utc::now().timestamp() - 100 * 86400,
        );
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE proxy_request_logs SET input_tokens=-1", [])
            .unwrap();
        assert!(configure(&db, true, "Fixture").is_err());
        assert_eq!(meta(&db.conn.lock().unwrap(), "initialized").unwrap(), None);
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE proxy_request_logs SET input_tokens=1", [])
            .unwrap();
        configure(&db, true, "Fixture").unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE proxy_request_logs SET input_tokens=-1", [])
            .unwrap();
        assert!(db.rollup_and_prune(30).is_err());
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM proxy_request_logs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn ordinary_delete_and_codex_rebuild_remove_receipts() {
        let db = Database::memory().unwrap();
        configure(&db, true, "Lifecycle fixture").unwrap();
        super::tests::add(
            &db,
            "session:delete",
            "session_log",
            "claude",
            2,
            Utc::now().timestamp(),
        );
        super::tests::add(
            &db,
            "codex_session:fixture:1",
            "codex_session",
            "codex",
            1,
            Utc::now().timestamp(),
        );
        assert_eq!(build_snapshot(&db).unwrap().events.len(), 2);
        db.conn
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM proxy_request_logs WHERE request_id='session:delete'",
                [],
            )
            .unwrap();
        assert_eq!(build_snapshot(&db).unwrap().events.len(), 1);
        super::super::session_usage_codex::reset_codex_usage_on_conn(
            &db.conn.lock().unwrap(),
            std::path::Path::new("/synthetic/codex"),
        )
        .unwrap();
        assert!(build_snapshot(&db).unwrap().events.is_empty());
    }
    #[test]
    #[serial_test::serial]
    fn configuration_download_preserves_device_and_usage_tables() {
        let local = Database::memory().unwrap();
        let remote = Database::memory().unwrap();
        configure(&local, true, "Local installation").unwrap();
        configure(&remote, true, "Remote installation").unwrap();
        super::tests::add(
            &local,
            "keep-local",
            "proxy",
            "claude",
            2,
            Utc::now().timestamp(),
        );
        build_snapshot(&local).unwrap();
        let before = settings(&local).unwrap();
        local
            .import_sql_string_for_sync(&remote.export_sql_string_for_sync().unwrap())
            .unwrap();
        let after = settings(&local).unwrap();
        assert_eq!(after.device_id, before.device_id);
        assert_eq!(after.device_name, before.device_name);
        assert!(after.enabled);
        assert_eq!(build_snapshot(&local).unwrap().events.len(), 1);
        assert_ne!(after.device_id, settings(&remote).unwrap().device_id);
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use axum::{
        body::{Body, Bytes},
        extract::State,
        http::{Request, StatusCode},
        response::IntoResponse,
        routing::any,
        Router,
    };
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };
    #[derive(Default)]
    struct Dav {
        files: Mutex<BTreeMap<String, (Vec<u8>, u64)>>,
        fail_next_head: AtomicBool,
    }
    async fn handler(
        State(dav): State<Arc<Dav>>,
        request: Request<Body>,
    ) -> axum::response::Response {
        let path = request.uri().path().to_string();
        let method = request.method().as_str().to_string();
        let if_match = request
            .headers()
            .get("if-match")
            .and_then(|s| s.to_str().ok())
            .map(str::to_string);
        let if_none = request.headers().contains_key("if-none-match");
        let body = axum::body::to_bytes(request.into_body(), MAX_BYTES)
            .await
            .unwrap();
        let mut files = dav.files.lock().unwrap();
        match method.as_str() {
            "MKCOL" => StatusCode::CREATED.into_response(),
            "PUT" => {
                if path.ends_with("head.json") && dav.fail_next_head.swap(false, Ordering::SeqCst) {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                let existing = files.get(&path);
                if if_none && existing.is_some()
                    || if_match.as_ref().is_some_and(|etag| {
                        existing.is_none_or(|(_, rev)| etag != &format!("\"{rev}\""))
                    })
                {
                    return StatusCode::PRECONDITION_FAILED.into_response();
                }
                let revision = existing.map_or(1, |(_, rev)| rev + 1);
                files.insert(path, (body.to_vec(), revision));
                (StatusCode::CREATED, [("etag", format!("\"{revision}\""))]).into_response()
            }
            "GET" => match files.get(&path) {
                Some((bytes, revision)) => (
                    StatusCode::OK,
                    [("etag", format!("\"{revision}\""))],
                    Bytes::copy_from_slice(bytes),
                )
                    .into_response(),
                None => StatusCode::NOT_FOUND.into_response(),
            },
            "PROPFIND" => {
                let root = format!("{}/", path.trim_end_matches('/'));
                let ids: BTreeSet<_> = files
                    .keys()
                    .filter_map(|p| p.strip_prefix(&root))
                    .filter_map(|p| p.strip_suffix("/head.json"))
                    .collect();
                let body = format!(
                    "<d:multistatus xmlns:d='DAV:'>{}</d:multistatus>",
                    ids.iter()
                        .map(|id| format!("<d:response><d:href>{root}{id}/</d:href></d:response>"))
                        .collect::<String>()
                );
                (StatusCode::MULTI_STATUS, body).into_response()
            }
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        }
    }
    async fn server() -> (
        Arc<Dav>,
        crate::settings::WebDavSyncSettings,
        tokio::task::JoinHandle<()>,
    ) {
        let state = Arc::new(Dav::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/*path", any(handler))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let settings = crate::settings::WebDavSyncSettings {
            enabled: true,
            base_url: format!("http://{address}"),
            remote_root: "fixture".into(),
            profile: "test".into(),
            username: "synthetic-user".into(),
            password: "synthetic-password".into(),
            ..Default::default()
        };
        (state, settings, task)
    }
    fn db() -> Arc<Database> {
        let db = Arc::new(Database::memory().unwrap());
        configure(&db, true, "Synthetic device").unwrap();
        db
    }
    #[tokio::test]
    async fn offline_retry_atomic_publish_two_devices_and_last_good_cache() {
        let (dav, remote, server) = server().await;
        crate::usage_events::take_test_notify_count();
        let first = db();
        let second = db();
        dav.fail_next_head.store(true, Ordering::SeqCst);
        assert!(exchange(first.clone(), &remote).await.is_err());
        assert!(!dav
            .files
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.ends_with("head.json")));
        exchange(first.clone(), &remote).await.unwrap();
        let before = dav.files.lock().unwrap().len();
        exchange(first.clone(), &remote).await.unwrap();
        assert_eq!(
            dav.files.lock().unwrap().len(),
            before,
            "unchanged usage must not upload new artifacts"
        );
        exchange(second.clone(), &remote).await.unwrap();
        exchange(first.clone(), &remote).await.unwrap();
        assert_eq!(
            first
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM usage_sync_remote", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(build_snapshot(&first).unwrap().events.is_empty());
        let id = settings(&second).unwrap().device_id;
        let head_path = reqwest::Url::parse(&file_url(&remote, &id, "head.json").unwrap())
            .unwrap()
            .path()
            .to_string();
        {
            let mut files = dav.files.lock().unwrap();
            let (bytes, _) = files.get_mut(&head_path).unwrap();
            let mut head: Head = json_slice(bytes).unwrap();
            head.version += 1;
            *bytes = json_bytes(&head).unwrap();
        }
        assert!(exchange(first.clone(), &remote).await.is_err());
        assert_eq!(
            first
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM usage_sync_remote", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            crate::usage_events::take_test_notify_count(),
            0,
            "remote usage exchange must not announce changes to local request logs"
        );
        server.abort();
    }
    #[tokio::test]
    async fn conditional_heads_reject_concurrent_writers() {
        let (_dav, remote, server) = server().await;
        let db = db();
        exchange(db.clone(), &remote).await.unwrap();
        let id = settings(&db).unwrap().device_id;
        let url = file_url(&remote, &id, "head.json").unwrap();
        let (bytes, etag) = webdav::get_bytes(&url, &None, 4096).await.unwrap().unwrap();
        let (a, b) = tokio::join!(
            webdav::put_usage_head(&url, &None, bytes.clone(), etag.as_deref()),
            webdav::put_usage_head(&url, &None, bytes, etag.as_deref())
        );
        assert_ne!(a.is_ok(), b.is_ok());
        server.abort();
    }
}
