//! Database side of Usage Sync: device identity, export of this device's
//! usage, and application of remote devices into the read-only cache tables.
//!
//! All functions take a `&Connection` so they compose with the caller's lock
//! and are testable against in-memory databases.

use std::collections::{BTreeMap, HashMap};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::AppError;
use crate::services::sql_helpers::{fresh_input_sql, INPUT_TOKEN_SEMANTICS_FRESH};
use crate::services::usage_stats::{effective_usage_log_filter_for_range, provider_name_coalesce};
use crate::settings::UsageSyncDetailLevel;

use super::protocol::{
    day_partition_key, month_partition_key, AliasRecord, Coverage, DeviceManifest, LedgerRecord,
    LogRecord, PartitionMeta, RollupRecord, Segment,
};

// ─── Identity ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceIdentity {
    pub device_id: String,
    pub custom_name: Option<String>,
    pub committed_nonce: Option<String>,
    pub pending_nonce: Option<String>,
    pub manifest_seq: u64,
    /// Identity this installation was split from ("Use a new identity").
    pub parent_device_id: Option<String>,
    pub forked_at: Option<i64>,
}

fn read_identity(conn: &Connection) -> Result<Option<DeviceIdentity>, AppError> {
    conn.query_row(
        "SELECT device_id, device_name, committed_nonce, pending_nonce, manifest_seq,
                parent_device_id, forked_at
         FROM usage_sync_identity WHERE id = 1",
        [],
        |row| {
            Ok(DeviceIdentity {
                device_id: row.get(0)?,
                custom_name: row.get(1)?,
                committed_nonce: row.get(2)?,
                pending_nonce: row.get(3)?,
                manifest_seq: row.get::<_, i64>(4)?.max(0) as u64,
                parent_device_id: row.get(5)?,
                forked_at: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn new_device_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Load this installation's identity, creating a fresh random one on first use.
pub(crate) fn load_or_create_identity(conn: &Connection) -> Result<DeviceIdentity, AppError> {
    if let Some(identity) = read_identity(conn)? {
        return Ok(identity);
    }
    let device_id = new_device_id();
    conn.execute(
        "INSERT OR IGNORE INTO usage_sync_identity (id, device_id, created_at, manifest_seq)
         VALUES (1, ?1, ?2, 0)",
        params![device_id, chrono::Utc::now().timestamp()],
    )?;
    read_identity(conn)?
        .ok_or_else(|| AppError::Database("usage sync identity was not persisted".to_string()))
}

pub(crate) fn set_custom_device_name(
    conn: &Connection,
    name: Option<&str>,
) -> Result<(), AppError> {
    load_or_create_identity(conn)?;
    conn.execute(
        "UPDATE usage_sync_identity SET device_name = ?1 WHERE id = 1",
        params![name],
    )?;
    Ok(())
}

/// Record the nonce of a manifest *before* it is uploaded, so a crash between
/// the upload and [`commit_nonce`] is not later mistaken for a foreign writer.
pub(crate) fn set_pending_nonce(conn: &Connection, nonce: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE usage_sync_identity SET pending_nonce = ?1 WHERE id = 1",
        params![nonce],
    )?;
    Ok(())
}

pub(crate) fn commit_nonce(conn: &Connection, nonce: &str, seq: u64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE usage_sync_identity
         SET committed_nonce = ?1, pending_nonce = NULL, manifest_seq = ?2
         WHERE id = 1",
        params![nonce, seq as i64],
    )?;
    Ok(())
}

/// Start a brand-new identity to resolve a cloned-profile conflict.
///
/// The copied database holds history that the previous identity (the parent)
/// has already published. Request-level rows and ledger-covered rollups carry
/// request ids and are de-duplicated by id. The remaining rollup totals have
/// no ids; they are snapshotted into `usage_sync_inherited` and attributed to
/// the parent for as long as the parent's data is present (see
/// `ScopeSql::rollup_table`). Requests made after the split get new ids and
/// count for whichever installation made them.
pub(crate) fn reset_identity(conn: &Connection) -> Result<DeviceIdentity, AppError> {
    let previous = read_identity(conn)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM usage_sync_inherited", [])?;
    if previous.is_some() {
        let fresh = fresh_input_sql("r");
        // Rollup totals minus the ledger-covered part, per rollup group.
        tx.execute(
            &format!(
                "INSERT INTO usage_sync_inherited (
                    date, app_type, provider_id, model, request_model, pricing_model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd
                 )
                 SELECT r.date, r.app_type, r.provider_id, r.model, r.request_model,
                        r.pricing_model,
                        r.request_count - COALESCE(g.n, 0),
                        MAX(r.success_count - COALESCE(g.ok, 0), 0),
                        MAX({fresh} - COALESCE(g.input_tokens, 0), 0),
                        MAX(r.output_tokens - COALESCE(g.output_tokens, 0), 0),
                        MAX(r.cache_read_tokens - COALESCE(g.cache_read_tokens, 0), 0),
                        MAX(r.cache_creation_tokens - COALESCE(g.cache_creation_tokens, 0), 0),
                        CAST(MAX(CAST(r.total_cost_usd AS REAL) - COALESCE(g.cost, 0), 0) AS TEXT)
                 FROM usage_daily_rollups r
                 LEFT JOIN (
                    SELECT date, app_type, provider_id, model, request_model, pricing_model,
                           COUNT(*) AS n, SUM(success) AS ok, SUM(input_tokens) AS input_tokens,
                           SUM(output_tokens) AS output_tokens,
                           SUM(cache_read_tokens) AS cache_read_tokens,
                           SUM(cache_creation_tokens) AS cache_creation_tokens,
                           SUM(CAST(total_cost_usd AS REAL)) AS cost
                    FROM usage_rollup_ledger WHERE kind = 0
                    GROUP BY date, app_type, provider_id, model, request_model, pricing_model
                 ) g
                   ON g.date = r.date AND g.app_type = r.app_type
                  AND g.provider_id = r.provider_id AND g.model = r.model
                  AND g.request_model = r.request_model AND g.pricing_model = r.pricing_model
                 WHERE r.request_count - COALESCE(g.n, 0) > 0"
            ),
            [],
        )?;
    }
    tx.execute("DELETE FROM usage_sync_identity", [])?;
    tx.execute(
        "INSERT INTO usage_sync_identity (
            id, device_id, device_name, created_at, manifest_seq, parent_device_id, forked_at
         ) VALUES (1, ?1, ?2, ?3, 0, ?4, ?3)",
        params![
            new_device_id(),
            previous.as_ref().and_then(|p| p.custom_name.clone()),
            chrono::Utc::now().timestamp(),
            previous.as_ref().map(|p| p.device_id.clone()),
        ],
    )?;
    tx.commit()?;
    refresh_ledger_suppression(conn)?;
    load_or_create_identity(conn)
}

/// Accept the remote writer as our own lineage (used when the other copy is
/// known to be retired, e.g. after restoring this machine from a backup).
pub(crate) fn adopt_remote_writer(
    conn: &Connection,
    nonce: &str,
    seq: u64,
) -> Result<(), AppError> {
    commit_nonce(conn, nonce, seq)
}

// ─── Export ──────────────────────────────────────────────────

pub(crate) struct LocalExport {
    pub partitions: BTreeMap<String, Segment>,
    pub coverage: Coverage,
}

fn non_negative(value: i64) -> i64 {
    value.max(0)
}

fn cost_text(value: Option<String>) -> String {
    match value {
        Some(text) if !text.trim().is_empty() => text.trim().to_string(),
        _ => "0".to_string(),
    }
}

/// Build this device's partitions from its authoritative local usage tables.
///
/// Detail rows pass through the same session/proxy de-duplication filter the
/// dashboard uses, so a remote viewer sees exactly what this device reports
/// for itself. Rollups are exported as-is (they were de-duplicated at rollup time).
pub(crate) fn export_local(
    conn: &Connection,
    device_id: &str,
    detail_level: UsageSyncDetailLevel,
) -> Result<LocalExport, AppError> {
    let mut partitions: BTreeMap<String, Segment> = BTreeMap::new();
    fn segment_for<'a>(
        partitions: &'a mut BTreeMap<String, Segment>,
        device_id: &str,
        key: String,
    ) -> &'a mut Segment {
        partitions
            .entry(key)
            .or_insert_with_key(|key| Segment::new(device_id, key))
    }

    let filter = effective_usage_log_filter_for_range(conn, "l", None, None)?;
    let pname = provider_name_coalesce("l", "p");
    let join = "LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type";

    match detail_level {
        UsageSyncDetailLevel::Records => {
            let sql = format!(
                "SELECT date(l.created_at, 'unixepoch', 'localtime'), l.request_id,
                        COALESCE(l.data_source, 'proxy'), l.created_at, l.app_type, l.provider_id,
                        {pname}, l.model, l.request_model, l.pricing_model,
                        l.input_tokens, l.output_tokens, l.cache_read_tokens,
                        l.cache_creation_tokens, l.input_token_semantics,
                        CAST(l.input_cost_usd AS TEXT), CAST(l.output_cost_usd AS TEXT),
                        CAST(l.cache_read_cost_usd AS TEXT), CAST(l.cache_creation_cost_usd AS TEXT),
                        CAST(l.total_cost_usd AS TEXT), CAST(l.cost_multiplier AS TEXT),
                        l.status_code, l.latency_ms, l.first_token_ms, l.duration_ms,
                        l.is_streaming
                 FROM proxy_request_logs l {join}
                 WHERE {filter}
                 ORDER BY 1, l.request_id"
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let date: String = row.get(0)?;
                let record = LogRecord {
                    request_id: row.get(1)?,
                    data_source: row.get(2)?,
                    created_at: non_negative(row.get(3)?),
                    app_type: row.get(4)?,
                    provider_id: row.get(5)?,
                    provider_name: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    model: row.get(7)?,
                    request_model: row.get(8)?,
                    pricing_model: row.get(9)?,
                    input_tokens: non_negative(row.get(10)?),
                    output_tokens: non_negative(row.get(11)?),
                    cache_read_tokens: non_negative(row.get(12)?),
                    cache_creation_tokens: non_negative(row.get(13)?),
                    input_token_semantics: row.get(14)?,
                    input_cost_usd: cost_text(row.get(15)?),
                    output_cost_usd: cost_text(row.get(16)?),
                    cache_read_cost_usd: cost_text(row.get(17)?),
                    cache_creation_cost_usd: cost_text(row.get(18)?),
                    total_cost_usd: cost_text(row.get(19)?),
                    cost_multiplier: row
                        .get::<_, Option<String>>(20)?
                        .filter(|v| !v.trim().is_empty())
                        .unwrap_or_else(|| "1.0".to_string()),
                    status_code: row.get(21)?,
                    latency_ms: non_negative(row.get(22)?),
                    first_token_ms: row.get(23)?,
                    duration_ms: row.get(24)?,
                    is_streaming: row.get::<_, Option<i64>>(25)?.unwrap_or(0) != 0,
                };
                segment_for(&mut partitions, device_id, day_partition_key(&date))
                    .logs
                    .push(record);
            }
        }
        UsageSyncDetailLevel::Daily => {
            let fresh_input = fresh_input_sql("l");
            let sql = format!(
                "SELECT date(l.created_at, 'unixepoch', 'localtime') AS d, l.app_type,
                        l.provider_id, MAX({pname}), l.model,
                        COALESCE(l.request_model, '') AS rm, COALESCE(l.pricing_model, '') AS pm,
                        COUNT(*),
                        SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END),
                        COALESCE(SUM({fresh_input}), 0), COALESCE(SUM(l.output_tokens), 0),
                        COALESCE(SUM(l.cache_read_tokens), 0),
                        COALESCE(SUM(l.cache_creation_tokens), 0),
                        CAST(COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) AS TEXT),
                        CAST(COALESCE(AVG(l.latency_ms), 0) AS INTEGER)
                 FROM proxy_request_logs l {join}
                 WHERE {filter}
                 GROUP BY d, l.app_type, l.provider_id, l.model, rm, pm
                 ORDER BY d, l.app_type, l.provider_id, l.model, rm, pm"
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let date: String = row.get(0)?;
                let rollup = RollupRecord {
                    date: date.clone(),
                    app_type: row.get(1)?,
                    provider_id: row.get(2)?,
                    provider_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    model: row.get(4)?,
                    request_model: row.get(5)?,
                    pricing_model: row.get(6)?,
                    request_count: non_negative(row.get(7)?),
                    success_count: non_negative(row.get(8)?),
                    input_tokens: non_negative(row.get(9)?),
                    output_tokens: non_negative(row.get(10)?),
                    cache_read_tokens: non_negative(row.get(11)?),
                    cache_creation_tokens: non_negative(row.get(12)?),
                    input_token_semantics: INPUT_TOKEN_SEMANTICS_FRESH,
                    total_cost_usd: cost_text(row.get(13)?),
                    avg_latency_ms: non_negative(row.get(14)?),
                };
                segment_for(&mut partitions, device_id, day_partition_key(&date))
                    .rollups
                    .push(rollup);
            }
        }
    }

    let rollup_pname = provider_name_coalesce("r", "p");
    let sql = format!(
        "SELECT r.date, r.app_type, r.provider_id, {rollup_pname}, r.model, r.request_model,
                r.pricing_model, r.request_count, r.success_count, r.input_tokens,
                r.output_tokens, r.cache_read_tokens, r.cache_creation_tokens,
                r.input_token_semantics, CAST(r.total_cost_usd AS TEXT), r.avg_latency_ms
         FROM usage_daily_rollups r
         LEFT JOIN providers p ON r.provider_id = p.id AND r.app_type = p.app_type
         ORDER BY r.date, r.app_type, r.provider_id, r.model, r.request_model, r.pricing_model"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let date: String = row.get(0)?;
        // Malformed historic dates cannot be partitioned; skip rather than fail the sync.
        if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
            log::warn!("[UsageSync] skip rollup with malformed date {date:?}");
            continue;
        }
        let rollup = RollupRecord {
            date: date.clone(),
            app_type: row.get(1)?,
            provider_id: row.get(2)?,
            provider_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            model: row.get(4)?,
            request_model: row.get(5)?,
            pricing_model: row.get(6)?,
            request_count: non_negative(row.get(7)?),
            success_count: non_negative(row.get(8)?),
            input_tokens: non_negative(row.get(9)?),
            output_tokens: non_negative(row.get(10)?),
            cache_read_tokens: non_negative(row.get(11)?),
            cache_creation_tokens: non_negative(row.get(12)?),
            input_token_semantics: row.get(13)?,
            total_cost_usd: cost_text(row.get(14)?),
            avg_latency_ms: non_negative(row.get(15)?),
        };
        segment_for(&mut partitions, device_id, month_partition_key(&date))
            .rollups
            .push(rollup);
    }

    if detail_level == UsageSyncDetailLevel::Records {
        // Detail rows this device drops as duplicates of its own rows.
        let sql = format!(
            "SELECT date(l.created_at, 'unixepoch', 'localtime'), l.request_id,
                    COALESCE(l.data_source, 'proxy')
             FROM proxy_request_logs l
             WHERE NOT ({filter})
             ORDER BY 1, l.request_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let date: String = row.get(0)?;
            segment_for(&mut partitions, device_id, day_partition_key(&date))
                .aliases
                .push(AliasRecord {
                    request_id: row.get(1)?,
                    data_source: row.get(2)?,
                });
        }

        // Identities of rolled-up requests.
        let ledger_pname = provider_name_coalesce("g", "p");
        let sql = format!(
            "SELECT g.request_id, g.data_source, g.kind, g.date, g.app_type, g.provider_id,
                    {ledger_pname}, g.model, g.request_model, g.pricing_model, g.success,
                    g.input_tokens, g.output_tokens, g.cache_read_tokens,
                    g.cache_creation_tokens, CAST(g.total_cost_usd AS TEXT)
             FROM usage_rollup_ledger g
             LEFT JOIN providers p ON g.provider_id = p.id AND g.app_type = p.app_type
             ORDER BY g.date, g.request_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let date: String = row.get(3)?;
            if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
                continue;
            }
            let entry = LedgerRecord {
                request_id: row.get(0)?,
                data_source: row.get(1)?,
                kind: row.get(2)?,
                date: date.clone(),
                app_type: row.get(4)?,
                provider_id: row.get(5)?,
                provider_name: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                model: row.get(7)?,
                request_model: row.get(8)?,
                pricing_model: row.get(9)?,
                success: non_negative(row.get(10)?).min(1),
                input_tokens: non_negative(row.get(11)?),
                output_tokens: non_negative(row.get(12)?),
                cache_read_tokens: non_negative(row.get(13)?),
                cache_creation_tokens: non_negative(row.get(14)?),
                total_cost_usd: cost_text(row.get(15)?),
            };
            segment_for(&mut partitions, device_id, month_partition_key(&date))
                .ledger
                .push(entry);
        }
    }

    // History inherited from a parent identity (no request ids, all modes).
    let inherited_pname = provider_name_coalesce("i", "p");
    let sql = format!(
        "SELECT i.date, i.app_type, i.provider_id, {inherited_pname}, i.model, i.request_model,
                i.pricing_model, i.request_count, i.success_count, i.input_tokens,
                i.output_tokens, i.cache_read_tokens, i.cache_creation_tokens,
                CAST(i.total_cost_usd AS TEXT)
         FROM usage_sync_inherited i
         LEFT JOIN providers p ON i.provider_id = p.id AND i.app_type = p.app_type
         ORDER BY i.date, i.app_type, i.provider_id, i.model, i.request_model, i.pricing_model"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let date: String = row.get(0)?;
        if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
            continue;
        }
        let inherited = RollupRecord {
            date: date.clone(),
            app_type: row.get(1)?,
            provider_id: row.get(2)?,
            provider_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            model: row.get(4)?,
            request_model: row.get(5)?,
            pricing_model: row.get(6)?,
            request_count: non_negative(row.get(7)?),
            success_count: non_negative(row.get(8)?),
            input_tokens: non_negative(row.get(9)?),
            output_tokens: non_negative(row.get(10)?),
            cache_read_tokens: non_negative(row.get(11)?),
            cache_creation_tokens: non_negative(row.get(12)?),
            input_token_semantics: INPUT_TOKEN_SEMANTICS_FRESH,
            total_cost_usd: cost_text(row.get(13)?),
            avg_latency_ms: 0,
        };
        segment_for(&mut partitions, device_id, month_partition_key(&date))
            .inherited
            .push(inherited);
    }

    let mut coverage = Coverage::default();
    for (key, segment) in &partitions {
        let dates = segment
            .rollups
            .iter()
            .map(|r| r.date.clone())
            .chain(key.strip_prefix("d-").map(str::to_string));
        for date in dates {
            if coverage.first_date.as_ref().is_none_or(|d| &date < d) {
                coverage.first_date = Some(date.clone());
            }
            if coverage.last_date.as_ref().is_none_or(|d| &date > d) {
                coverage.last_date = Some(date.clone());
            }
        }
        if let Some(date) = key.strip_prefix("d-") {
            if !segment.logs.is_empty() && coverage.detail_from.as_deref().is_none_or(|d| date < d)
            {
                coverage.detail_from = Some(date.to_string());
            }
        }
    }

    Ok(LocalExport {
        partitions,
        coverage,
    })
}

// ─── Cross-device suppression of rolled-up copies ────────────

/// Rebuild `usage_sync_ledger_suppressed`: the rolled-up copies (this device's
/// ledger and remote ledgers) that lose under the cross-device identity rule
/// (`usage_stats::IdentityRules`). Request-level copies are decided at query
/// time; ledger copies are materialized because they span up to a year of
/// history. Inputs change on sync, rollup, hide/unhide and identity changes,
/// which all call this; a late local import of an already rolled-up request is
/// picked up at the next of those events.
pub(crate) fn refresh_ledger_suppression(conn: &Connection) -> Result<(), AppError> {
    let rules = crate::services::usage_stats::IdentityRules::load(conn)?;
    let local_loses = rules.local_copy_loses("g.request_id");
    let remote_loses = rules.remote_copy_loses("c.request_id", "c.data_source", "c.device_id");
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM usage_sync_remote_multi", [])?;
    tx.execute(
        "INSERT INTO usage_sync_remote_multi (request_id)
         SELECT request_id FROM (
            SELECT request_id, device_id FROM usage_sync_remote_logs
            UNION
            SELECT request_id, device_id FROM usage_sync_remote_claims
         )
         GROUP BY request_id
         HAVING COUNT(*) > 1",
        [],
    )?;
    tx.execute("DELETE FROM usage_sync_ledger_suppressed", [])?;
    tx.execute(
        &format!(
            "INSERT OR IGNORE INTO usage_sync_ledger_suppressed (device_id, request_id)
             SELECT '', g.request_id FROM usage_rollup_ledger g
             WHERE g.kind = 0 AND {local_loses}"
        ),
        [],
    )?;
    tx.execute(
        &format!(
            "INSERT OR IGNORE INTO usage_sync_ledger_suppressed (device_id, request_id)
             SELECT c.device_id, c.request_id FROM usage_sync_remote_claims c
             WHERE c.kind = 0 AND {remote_loses}"
        ),
        [],
    )?;
    tx.commit()?;
    Ok(())
}

// ─── Remote cache ────────────────────────────────────────────

pub(crate) const DEVICE_STATUS_OK: &str = "ok";
pub(crate) const DEVICE_STATUS_ERROR: &str = "error";
pub(crate) const DEVICE_STATUS_INCOMPATIBLE: &str = "incompatible";
pub(crate) const DEVICE_STATUS_MISSING: &str = "missing";
pub(crate) const DEVICE_STATUS_HIDDEN: &str = "hidden";

#[derive(Debug, Clone, Default)]
pub(crate) struct CachedDevice {
    pub manifest_seq: u64,
    pub manifest_sha256: Option<String>,
    pub status: String,
    /// partition key → object sha256
    pub partitions: HashMap<String, String>,
}

pub(crate) fn load_cached_device(
    conn: &Connection,
    device_id: &str,
) -> Result<Option<CachedDevice>, AppError> {
    let Some(mut device) = conn
        .query_row(
            "SELECT manifest_seq, manifest_sha256, status FROM usage_sync_devices
             WHERE device_id = ?1",
            params![device_id],
            |row| {
                Ok(CachedDevice {
                    manifest_seq: row.get::<_, i64>(0)?.max(0) as u64,
                    manifest_sha256: row.get(1)?,
                    status: row.get(2)?,
                    partitions: HashMap::new(),
                })
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT partition_key, object_sha256 FROM usage_sync_remote_partitions
         WHERE device_id = ?1",
    )?;
    let rows = stmt.query_map(params![device_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (key, sha) = row?;
        device.partitions.insert(key, sha);
    }
    Ok(Some(device))
}

/// Cache tables whose rows belong to one (device, partition).
const REMOTE_PARTITIONED_TABLES: [&str; 5] = [
    "usage_sync_remote_logs",
    "usage_sync_remote_rollups",
    "usage_sync_remote_claims",
    "usage_sync_remote_inherited",
    "usage_sync_remote_partitions",
];

fn delete_device_partition(
    conn: &Connection,
    device_id: &str,
    partition: &str,
) -> Result<(), AppError> {
    for table in REMOTE_PARTITIONED_TABLES {
        conn.execute(
            &format!("DELETE FROM {table} WHERE device_id = ?1 AND partition_key = ?2"),
            params![device_id, partition],
        )?;
    }
    Ok(())
}

/// Atomically replace the changed/removed partitions of one remote device and
/// record its new manifest. Any failure rolls back the whole device so the
/// cache never mixes two manifest generations.
pub(crate) fn apply_remote_device(
    conn: &Connection,
    manifest: &DeviceManifest,
    manifest_sha256: &str,
    changed: &[(String, PartitionMeta, Segment)],
    removed: &[String],
    now: i64,
) -> Result<(), AppError> {
    let tx = conn.unchecked_transaction()?;
    let device_id = manifest.device_id.as_str();

    for key in removed.iter().chain(changed.iter().map(|(key, _, _)| key)) {
        delete_device_partition(&tx, device_id, key)?;
    }

    {
        let mut insert_log = tx.prepare(
            "INSERT INTO usage_sync_remote_logs (
                device_id, partition_key, request_id, provider_id, provider_name, app_type,
                model, request_model, pricing_model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, input_token_semantics,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, first_token_ms, duration_ms, status_code,
                is_streaming, cost_multiplier, created_at, data_source
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                       ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
        )?;
        let mut insert_rollup = tx.prepare(
            "INSERT INTO usage_sync_remote_rollups (
                device_id, partition_key, date, app_type, provider_id, provider_name, model,
                request_model, pricing_model, request_count, success_count, input_tokens,
                output_tokens, cache_read_tokens, cache_creation_tokens, input_token_semantics,
                total_cost_usd, avg_latency_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                       ?17, ?18)",
        )?;
        let mut insert_claim = tx.prepare(
            "INSERT INTO usage_sync_remote_claims (
                device_id, partition_key, request_id, data_source, kind, date, app_type,
                provider_id, provider_name, model, request_model, pricing_model, success,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                total_cost_usd
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                       ?17, ?18)",
        )?;
        let mut insert_inherited = tx.prepare(
            "INSERT INTO usage_sync_remote_inherited (
                device_id, partition_key, parent_device_id, date, app_type, provider_id,
                provider_name, model, request_model, pricing_model, request_count,
                success_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_cost_usd
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                       ?17)",
        )?;
        let mut insert_partition = tx.prepare(
            "INSERT INTO usage_sync_remote_partitions (
                device_id, partition_key, object_sha256, content_sha256, log_rows,
                rollup_rows, imported_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;

        for (key, meta, segment) in changed {
            for log in &segment.logs {
                insert_log
                    .execute(params![
                        device_id,
                        key,
                        log.request_id,
                        log.provider_id,
                        log.provider_name,
                        log.app_type,
                        log.model,
                        log.request_model,
                        log.pricing_model,
                        log.input_tokens,
                        log.output_tokens,
                        log.cache_read_tokens,
                        log.cache_creation_tokens,
                        log.input_token_semantics,
                        log.input_cost_usd,
                        log.output_cost_usd,
                        log.cache_read_cost_usd,
                        log.cache_creation_cost_usd,
                        log.total_cost_usd,
                        log.latency_ms,
                        log.first_token_ms,
                        log.duration_ms,
                        log.status_code,
                        log.is_streaming as i64,
                        log.cost_multiplier,
                        log.created_at,
                        log.data_source,
                    ])
                    .map_err(|e| {
                        AppError::Database(format!(
                            "导入远端用量记录失败（{key}/{}）: {e}",
                            log.request_id
                        ))
                    })?;
            }
            for rollup in &segment.rollups {
                insert_rollup
                    .execute(params![
                        device_id,
                        key,
                        rollup.date,
                        rollup.app_type,
                        rollup.provider_id,
                        rollup.provider_name,
                        rollup.model,
                        rollup.request_model,
                        rollup.pricing_model,
                        rollup.request_count,
                        rollup.success_count,
                        rollup.input_tokens,
                        rollup.output_tokens,
                        rollup.cache_read_tokens,
                        rollup.cache_creation_tokens,
                        rollup.input_token_semantics,
                        rollup.total_cost_usd,
                        rollup.avg_latency_ms,
                    ])
                    .map_err(|e| {
                        AppError::Database(format!("导入远端用量汇总失败（{key}）: {e}"))
                    })?;
            }
            let claim_error = |e: rusqlite::Error| {
                AppError::Database(format!("导入远端用量身份失败（{key}）: {e}"))
            };
            for alias in &segment.aliases {
                // Day partition key → date; aliases carry no contribution.
                let date = key.strip_prefix("d-").unwrap_or_default();
                insert_claim
                    .execute(params![
                        device_id,
                        key,
                        alias.request_id,
                        alias.data_source,
                        super::protocol::LEDGER_KIND_ALIAS,
                        date,
                        "",
                        "",
                        Option::<String>::None,
                        "",
                        "",
                        "",
                        0,
                        0,
                        0,
                        0,
                        0,
                        "0",
                    ])
                    .map_err(claim_error)?;
            }
            for entry in &segment.ledger {
                insert_claim
                    .execute(params![
                        device_id,
                        key,
                        entry.request_id,
                        entry.data_source,
                        entry.kind,
                        entry.date,
                        entry.app_type,
                        entry.provider_id,
                        entry.provider_name,
                        entry.model,
                        entry.request_model,
                        entry.pricing_model,
                        entry.success,
                        entry.input_tokens,
                        entry.output_tokens,
                        entry.cache_read_tokens,
                        entry.cache_creation_tokens,
                        entry.total_cost_usd,
                    ])
                    .map_err(claim_error)?;
            }
            // Inherited rows only mean something together with a lineage.
            if let Some(lineage) = &manifest.lineage {
                for row in &segment.inherited {
                    insert_inherited
                        .execute(params![
                            device_id,
                            key,
                            lineage.parent_device_id,
                            row.date,
                            row.app_type,
                            row.provider_id,
                            row.provider_name,
                            row.model,
                            row.request_model,
                            row.pricing_model,
                            row.request_count,
                            row.success_count,
                            row.input_tokens,
                            row.output_tokens,
                            row.cache_read_tokens,
                            row.cache_creation_tokens,
                            row.total_cost_usd,
                        ])
                        .map_err(claim_error)?;
                }
            }
            insert_partition.execute(params![
                device_id,
                key,
                meta.sha256,
                meta.content_sha256,
                segment.logs.len() as i64,
                segment.rollups.len() as i64,
                now,
            ])?;
        }
    }

    tx.execute(
        "INSERT INTO usage_sync_devices (
            device_id, device_name, platform, app_version, detail_level, manifest_seq,
            manifest_sha256, remote_updated_at, first_date, last_date, detail_from,
            last_fetched_at, last_checked_at, last_error, status, utc_offset_minutes,
            parent_device_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, NULL, ?13, ?14, ?15)
         ON CONFLICT(device_id) DO UPDATE SET
            device_name = excluded.device_name,
            platform = excluded.platform,
            app_version = excluded.app_version,
            detail_level = excluded.detail_level,
            manifest_seq = excluded.manifest_seq,
            manifest_sha256 = excluded.manifest_sha256,
            remote_updated_at = excluded.remote_updated_at,
            first_date = excluded.first_date,
            last_date = excluded.last_date,
            detail_from = excluded.detail_from,
            last_fetched_at = excluded.last_fetched_at,
            last_checked_at = excluded.last_checked_at,
            last_error = NULL,
            status = excluded.status,
            utc_offset_minutes = excluded.utc_offset_minutes,
            parent_device_id = excluded.parent_device_id",
        params![
            device_id,
            manifest.device_name,
            manifest.platform,
            manifest.app_version,
            manifest.detail_level,
            manifest.seq as i64,
            manifest_sha256,
            manifest.updated_at,
            manifest.coverage.first_date,
            manifest.coverage.last_date,
            manifest.coverage.detail_from,
            now,
            DEVICE_STATUS_OK,
            manifest.utc_offset_minutes,
            manifest
                .lineage
                .as_ref()
                .map(|l| l.parent_device_id.clone()),
        ],
    )?;

    tx.commit()?;
    Ok(())
}

/// Note a successful check where the manifest was unchanged.
pub(crate) fn touch_device_checked(
    conn: &Connection,
    device_id: &str,
    now: i64,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE usage_sync_devices
         SET last_checked_at = ?2, last_error = NULL, status = ?3
         WHERE device_id = ?1 AND status <> ?4",
        params![device_id, now, DEVICE_STATUS_OK, DEVICE_STATUS_HIDDEN],
    )?;
    Ok(())
}

/// Record a failed check without touching the device's cached data.
pub(crate) fn record_device_problem(
    conn: &Connection,
    device_id: &str,
    status: &str,
    error: &str,
    now: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO usage_sync_devices (device_id, device_name, last_checked_at, last_error, status)
         VALUES (?1, ?1, ?2, ?3, ?4)
         ON CONFLICT(device_id) DO UPDATE SET
            last_checked_at = excluded.last_checked_at,
            last_error = excluded.last_error,
            status = CASE WHEN usage_sync_devices.status = 'hidden'
                          THEN usage_sync_devices.status ELSE excluded.status END",
        params![device_id, now, error, status],
    )?;
    Ok(())
}

/// Devices whose directories disappeared keep their cached history but are
/// flagged so the UI can explain why they no longer update.
pub(crate) fn mark_missing_devices(
    conn: &Connection,
    present: &[String],
    now: i64,
) -> Result<(), AppError> {
    let mut stmt = conn.prepare("SELECT device_id, status FROM usage_sync_devices")?;
    let known: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (device_id, status) in known {
        if status == DEVICE_STATUS_HIDDEN || present.contains(&device_id) {
            continue;
        }
        conn.execute(
            "UPDATE usage_sync_devices SET status = ?2, last_checked_at = ?3 WHERE device_id = ?1",
            params![device_id, DEVICE_STATUS_MISSING, now],
        )?;
    }
    Ok(())
}

/// Hide a device from this installation's statistics (its cached rows are
/// dropped; the remote data is untouched). Unhiding re-imports on next sync.
pub(crate) fn set_device_hidden(
    conn: &Connection,
    device_id: &str,
    hidden: bool,
) -> Result<(), AppError> {
    let tx = conn.unchecked_transaction()?;
    for table in REMOTE_PARTITIONED_TABLES {
        tx.execute(
            &format!("DELETE FROM {table} WHERE device_id = ?1"),
            params![device_id],
        )?;
    }
    if hidden {
        tx.execute(
            "INSERT INTO usage_sync_devices (device_id, device_name, status)
             VALUES (?1, ?1, ?2)
             ON CONFLICT(device_id) DO UPDATE SET status = excluded.status,
                manifest_sha256 = NULL, manifest_seq = 0",
            params![device_id, DEVICE_STATUS_HIDDEN],
        )?;
    } else {
        tx.execute(
            "UPDATE usage_sync_devices
             SET status = ?2, manifest_sha256 = NULL, manifest_seq = 0
             WHERE device_id = ?1",
            params![device_id, DEVICE_STATUS_OK],
        )?;
    }
    tx.commit()?;
    refresh_ledger_suppression(conn)
}

/// Remote device as shown in the Usage page and sync settings.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDeviceInfo {
    pub device_id: String,
    pub device_name: String,
    pub platform: Option<String>,
    pub app_version: Option<String>,
    pub detail_level: String,
    /// When the device last uploaded (its manifest timestamp).
    pub last_upload_at: Option<i64>,
    /// When this installation last imported the device's data.
    pub last_fetched_at: Option<i64>,
    pub last_checked_at: Option<i64>,
    pub status: String,
    pub last_error: Option<String>,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub detail_from: Option<String>,
    pub log_rows: i64,
    pub rollup_rows: i64,
    /// Device's UTC offset at its last upload; day-level rows carry its calendar.
    pub utc_offset_minutes: Option<i64>,
    /// Identity this device was split from (copied installation).
    pub parent_device_id: Option<String>,
    /// History that carries no request identities and therefore cannot be
    /// checked for duplicates across devices.
    pub unverified: UnverifiedHistory,
}

/// Day-level history without request identities (legacy rollups made before
/// the identity ledger existed, entries past ledger retention, daily-only
/// sharing). Counted once per device that holds it.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnverifiedHistory {
    pub requests: i64,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
}

const GROUP_COLUMNS: &str = "date, app_type, provider_id, model, request_model, pricing_model";

/// Residual per rollup group = rollup requests − ledger-covered − inherited.
fn unverified_sql(rollups: &str, ledger: &str, inherited: &str, key: &str) -> String {
    format!(
        "WITH rr AS (SELECT {key} AS k, {GROUP_COLUMNS}, SUM(request_count) AS n
                     FROM {rollups} GROUP BY k, {GROUP_COLUMNS}),
              gg AS (SELECT {key} AS k, {GROUP_COLUMNS}, COUNT(*) AS n
                     FROM {ledger} WHERE kind = 0 GROUP BY k, {GROUP_COLUMNS}),
              ii AS (SELECT {key} AS k, {GROUP_COLUMNS}, SUM(request_count) AS n
                     FROM {inherited} GROUP BY k, {GROUP_COLUMNS}),
              res AS (SELECT rr.k, rr.date,
                             MAX(rr.n - COALESCE(gg.n, 0) - COALESCE(ii.n, 0), 0) AS n
                      FROM rr
                      LEFT JOIN gg USING (k, {GROUP_COLUMNS})
                      LEFT JOIN ii USING (k, {GROUP_COLUMNS}))
         SELECT k, SUM(n), MIN(CASE WHEN n > 0 THEN date END), MAX(CASE WHEN n > 0 THEN date END)
         FROM res GROUP BY k"
    )
}

/// Unverified history of this device.
pub(crate) fn local_unverified(conn: &Connection) -> Result<UnverifiedHistory, AppError> {
    let sql = unverified_sql(
        "usage_daily_rollups",
        "usage_rollup_ledger",
        "usage_sync_inherited",
        "''",
    );
    let found = conn
        .query_row(&sql, [], |row| {
            Ok(UnverifiedHistory {
                requests: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                first_date: row.get(2)?,
                last_date: row.get(3)?,
            })
        })
        .optional()?;
    Ok(found.unwrap_or_default())
}

fn remote_unverified(conn: &Connection) -> Result<HashMap<String, UnverifiedHistory>, AppError> {
    let sql = unverified_sql(
        "usage_sync_remote_rollups",
        "usage_sync_remote_claims",
        "usage_sync_remote_inherited",
        "device_id",
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            UnverifiedHistory {
                requests: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                first_date: row.get(2)?,
                last_date: row.get(3)?,
            },
        ))
    })?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(Into::into)
}

pub(crate) fn list_remote_devices(conn: &Connection) -> Result<Vec<RemoteDeviceInfo>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT d.device_id, d.device_name, d.platform, d.app_version, d.detail_level,
                d.remote_updated_at, d.last_fetched_at, d.last_checked_at, d.status,
                d.last_error, d.first_date, d.last_date, d.detail_from,
                COALESCE((SELECT SUM(log_rows) FROM usage_sync_remote_partitions p
                          WHERE p.device_id = d.device_id), 0),
                COALESCE((SELECT SUM(rollup_rows) FROM usage_sync_remote_partitions p
                          WHERE p.device_id = d.device_id), 0),
                d.utc_offset_minutes, d.parent_device_id
         FROM usage_sync_devices d
         ORDER BY d.device_name COLLATE NOCASE, d.device_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(RemoteDeviceInfo {
            device_id: row.get(0)?,
            device_name: row.get(1)?,
            platform: row.get(2)?,
            app_version: row.get(3)?,
            detail_level: row.get(4)?,
            last_upload_at: row.get(5)?,
            last_fetched_at: row.get(6)?,
            last_checked_at: row.get(7)?,
            status: row.get(8)?,
            last_error: row.get(9)?,
            first_date: row.get(10)?,
            last_date: row.get(11)?,
            detail_from: row.get(12)?,
            log_rows: row.get(13)?,
            rollup_rows: row.get(14)?,
            utc_offset_minutes: row.get(15)?,
            parent_device_id: row.get(16)?,
            unverified: UnverifiedHistory::default(),
        })
    })?;
    let mut devices = rows.collect::<Result<Vec<_>, _>>()?;
    let unverified = remote_unverified(conn)?;
    for device in &mut devices {
        if let Some(found) = unverified.get(&device.device_id) {
            device.unverified = found.clone();
        }
    }
    Ok(devices)
}
