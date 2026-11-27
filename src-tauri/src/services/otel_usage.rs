//! Loopback OTLP/HTTP JSON usage collector for ephemeral/non-persisted CLI runs.
//!
//! Captures usage when an agent invokes another CLI in non-persisted mode (for
//! example `codex exec --ephemeral ...`, Codex side chats, `codex-plugin-cc`
//! reviews, `claude -p --no-session-persistence`, or `gemini -p`) where no
//! local session transcript is written to disk and local proxy takeover may be
//! disabled.
//!
//! Only sanitized accounting fields are retained in `otel_usage_ingest` and
//! `proxy_request_logs`; prompts, tool outputs, account emails, headers, and
//! raw telemetry bodies are never persisted.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::server::ProxyState;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{cost_columns, SessionSyncResult};
use crate::services::session_usage_codex::normalize_codex_model;
use crate::services::sql_helpers::{INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OpenFlags};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const OTEL_LOGS_PATH: &str = "/v1/cc-switch/otel/logs";
pub const CODEX_LOGS_PATH: &str = "/v1/cc-switch/codex/logs";
pub const STANDARD_LOGS_PATH: &str = "/v1/logs";
pub const OTEL_HEALTH_PATH: &str = "/cc-switch/otel/health";
pub const CODEX_HEALTH_PATH: &str = "/cc-switch/codex-otel/health";

const MAX_BATCH_RECORDS: usize = 4096;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_AGE_SECONDS: i64 = 7 * 86400;
const MAX_SESSION_ID_LEN: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OtelAppKind {
    Codex,
    Claude,
    Gemini,
}

impl OtelAppKind {
    fn app_type(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
        }
    }

    fn provider_id(self) -> &'static str {
        match self {
            Self::Codex => "_codex_otel",
            Self::Claude => "_claude_otel",
            Self::Gemini => "_gemini_otel",
        }
    }

    fn data_source(self, prewarm: bool) -> &'static str {
        match (self, prewarm) {
            (Self::Codex, true) => "codex_otel_prewarm",
            (Self::Codex, false) => "codex_otel",
            (Self::Claude, _) => "claude_otel",
            (Self::Gemini, _) => "gemini_otel",
        }
    }

    fn persisted_session_source(self) -> &'static str {
        match self {
            Self::Codex => "codex_session",
            Self::Claude => "session_log",
            Self::Gemini => "gemini_session",
        }
    }

    fn request_id_prefix(self) -> &'static str {
        match self {
            Self::Codex => "codex_otel",
            Self::Claude => "claude_otel",
            Self::Gemini => "gemini_otel",
        }
    }

    fn input_token_semantics(self) -> i64 {
        match self {
            Self::Codex | Self::Gemini => INPUT_TOKEN_SEMANTICS_TOTAL,
            Self::Claude => INPUT_TOKEN_SEMANTICS_FRESH,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsageEvent {
    app: OtelAppKind,
    session_id: String,
    model: String,
    observed_ns: u64,
    created_at: i64,
    #[serde(default)]
    latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_token_ms: Option<u64>,
    input: u32,
    output: u32,
    cached: u32,
    cache_write: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    prewarm: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl UsageEvent {
    fn id(&self) -> String {
        // Classification is not part of event identity so reclassifying an event
        // as prewarm preserves replay protection for already-ingested events.
        let mut identity = self.clone();
        identity.prewarm = false;
        let bytes = serde_json::to_vec(&identity).expect("usage event is serializable");
        format!(
            "{}:{:x}",
            self.app.request_id_prefix(),
            Sha256::digest(bytes)
        )
    }
}

fn attribute_text(value: &Value) -> Option<&str> {
    value
        .get("stringValue")
        .or_else(|| value.get("intValue"))
        .and_then(Value::as_str)
}

fn attribute_u64(value: &Value) -> Option<u64> {
    attribute_text(value)
        .and_then(|s| s.trim().parse::<u64>().ok())
        .or_else(|| value.get("intValue").and_then(Value::as_u64))
        .or_else(|| {
            value.get("doubleValue").and_then(|v| {
                let f = v.as_f64()?;
                if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f <= u64::MAX as f64 {
                    Some(f as u64)
                } else {
                    None
                }
            })
        })
}

fn valid_model_name(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 200
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._:/[]".contains(c))
}

fn sanitize_session_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_SESSION_ID_LEN {
        return None;
    }
    if let Ok(uuid) = uuid::Uuid::parse_str(trimmed) {
        return Some(uuid.hyphenated().to_string());
    }
    trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c))
        .then(|| trimmed.to_owned())
}

fn extract_timestamps(
    record: &Value,
    attributes: &HashMap<&str, &Value>,
    now: i64,
) -> Option<(u64, i64)> {
    let timestamp = attributes
        .get("event.timestamp")
        .and_then(|v| attribute_text(v))
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
    // Codex often sends timeUnixNano=0 alongside a precise observedTimeUnixNano.
    let observed_ns = ["timeUnixNano", "observedTimeUnixNano"]
        .iter()
        .filter_map(|key| {
            record.get(key).and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .or_else(|| v.as_u64())
            })
        })
        .find(|n| *n > 0)
        .or_else(|| {
            timestamp
                .as_ref()?
                .timestamp_nanos_opt()
                .and_then(|n| u64::try_from(n).ok())
        })?;
    let created_at = timestamp
        .map(|t| t.timestamp())
        .unwrap_or((observed_ns / 1_000_000_000) as i64);
    if created_at < now - MAX_AGE_SECONDS || created_at > now + 300 {
        return None;
    }
    Some((observed_ns, created_at))
}

fn parse_event(record: &Value, now: i64) -> Option<UsageEvent> {
    let attributes: HashMap<&str, &Value> = record
        .get("attributes")?
        .as_array()?
        .iter()
        .filter_map(|a| Some((a.get("key")?.as_str()?, a.get("value")?)))
        .collect();
    let event_name = attributes
        .get("event.name")
        .and_then(|v| attribute_text(v))
        .or_else(|| record.get("body").and_then(attribute_text))?;

    match event_name {
        "codex.sse_event" => parse_codex_event(record, &attributes, now),
        "claude_code.api_request" => parse_claude_event(record, &attributes, now),
        "gemini_cli.api_response" => parse_gemini_event(record, &attributes, now),
        _ => None,
    }
}

fn parse_codex_event(
    record: &Value,
    attributes: &HashMap<&str, &Value>,
    now: i64,
) -> Option<UsageEvent> {
    let text = |key: &str| attributes.get(key).and_then(|v| attribute_text(v));
    if text("event.kind") != Some("response.completed") {
        return None;
    }
    // Internal approval checks are not billable user/agent turns.
    let model = text("model")?;
    if model == "codex-auto-review" || !valid_model_name(model) {
        return None;
    }
    let session_id = sanitize_session_id(text("conversation.id")?)?;
    let (observed_ns, created_at) = extract_timestamps(record, attributes, now)?;

    let count = |key: &str| {
        attributes
            .get(key)
            .and_then(|v| attribute_u64(v))
            .and_then(|n| u32::try_from(n).ok())
    };
    let input = count("input_token_count")?;
    let output = count("output_token_count")?;
    let optional_count = |key: &str| {
        if attributes.contains_key(key) {
            count(key)
        } else {
            Some(0)
        }
    };
    let cached = optional_count("cached_token_count")?;
    let cache_write = optional_count("cache_write_token_count")?;
    if u64::from(cached) + u64::from(cache_write) > u64::from(input) || (input == 0 && output == 0)
    {
        return None;
    }
    let latency_ms = attributes
        .get("duration_ms")
        .and_then(|v| attribute_u64(v))
        .unwrap_or(0);
    let first_token_ms = attributes
        .get("ttft_ms")
        .and_then(|v| attribute_u64(v))
        .filter(|ttft| latency_ms == 0 || *ttft <= latency_ms);

    let tier = text("service_tier")
        .map(str::to_ascii_lowercase)
        .filter(|s| {
            matches!(
                s.as_str(),
                "ultrafast" | "fast" | "priority" | "default" | "flex" | "auto"
            )
        });
    let effort = text("model_reasoning_effort")
        .map(str::to_ascii_lowercase)
        .filter(|s| {
            matches!(
                s.as_str(),
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
            )
        });
    // Codex 0.153.4 emits response.completed for generate=false WebSocket
    // prewarm too: no request metadata, no first token, and input-only usage.
    let prewarm = text("app.version") == Some("0.153.4")
        && (model.starts_with("gpt-5.") || model.starts_with("gpt-6"))
        && output == 0
        && cached == 0
        && cache_write == 0
        && optional_count("reasoning_token_count") == Some(0)
        && !attributes.contains_key("service_tier")
        && !attributes.contains_key("model_reasoning_effort")
        && !attributes.contains_key("ttft_ms");

    Some(UsageEvent {
        app: OtelAppKind::Codex,
        session_id,
        model: model.to_owned(),
        observed_ns,
        created_at,
        latency_ms,
        first_token_ms,
        input,
        output,
        cached,
        cache_write,
        tier,
        effort,
        prewarm,
    })
}

fn parse_claude_event(
    record: &Value,
    attributes: &HashMap<&str, &Value>,
    now: i64,
) -> Option<UsageEvent> {
    let text = |key: &str| attributes.get(key).and_then(|v| attribute_text(v));
    let model = text("model")?;
    if !valid_model_name(model) {
        return None;
    }
    let session_id = text("session.id")
        .or_else(|| text("conversation.id"))
        .or_else(|| text("request_id"))
        .and_then(sanitize_session_id)?;
    let (observed_ns, created_at) = extract_timestamps(record, attributes, now)?;

    let count_any = |keys: &[&str]| -> Option<u32> {
        for key in keys {
            if let Some(v) = attributes.get(key) {
                return attribute_u64(v).and_then(|n| u32::try_from(n).ok());
            }
        }
        None
    };
    let optional_count_any = |keys: &[&str]| -> Option<u32> {
        for key in keys {
            if let Some(v) = attributes.get(key) {
                return attribute_u64(v).and_then(|n| u32::try_from(n).ok());
            }
        }
        Some(0)
    };

    let input = count_any(&["input_tokens"])?;
    let output = count_any(&["output_tokens"])?;
    let cached = optional_count_any(&["cache_read_tokens", "cache_read_input_tokens"])?;
    let cache_write =
        optional_count_any(&["cache_creation_tokens", "cache_creation_input_tokens"])?;
    if input == 0 && output == 0 && cached == 0 && cache_write == 0 {
        return None;
    }
    let latency_ms = attributes
        .get("duration_ms")
        .and_then(|v| attribute_u64(v))
        .unwrap_or(0);

    Some(UsageEvent {
        app: OtelAppKind::Claude,
        session_id,
        model: model.to_owned(),
        observed_ns,
        created_at,
        latency_ms,
        first_token_ms: None,
        input,
        output,
        cached,
        cache_write,
        tier: None,
        effort: None,
        prewarm: false,
    })
}

fn parse_gemini_event(
    record: &Value,
    attributes: &HashMap<&str, &Value>,
    now: i64,
) -> Option<UsageEvent> {
    let text = |key: &str| attributes.get(key).and_then(|v| attribute_text(v));
    let model = text("model")?;
    if !valid_model_name(model) {
        return None;
    }
    let session_id = text("session.id")
        .or_else(|| text("conversation.id"))
        .and_then(sanitize_session_id)?;
    let (observed_ns, created_at) = extract_timestamps(record, attributes, now)?;

    let count_any = |keys: &[&str]| -> Option<u32> {
        for key in keys {
            if let Some(v) = attributes.get(key) {
                return attribute_u64(v).and_then(|n| u32::try_from(n).ok());
            }
        }
        None
    };
    let optional_count_any = |keys: &[&str]| -> Option<u32> {
        for key in keys {
            if let Some(v) = attributes.get(key) {
                return attribute_u64(v).and_then(|n| u32::try_from(n).ok());
            }
        }
        Some(0)
    };

    let input = count_any(&["input_token_count", "input_tokens"])?;
    let raw_output = count_any(&["output_token_count", "output_tokens"])?;
    let thoughts = optional_count_any(&["thoughts_token_count", "thought_token_count"])?;
    let output = raw_output.checked_add(thoughts)?;
    let cached = optional_count_any(&["cached_content_token_count", "cached_token_count"])?;
    if cached > input || (input == 0 && output == 0 && cached == 0) {
        return None;
    }
    let latency_ms = attributes
        .get("duration_ms")
        .and_then(|v| attribute_u64(v))
        .unwrap_or(0);

    Some(UsageEvent {
        app: OtelAppKind::Gemini,
        session_id,
        model: model.to_owned(),
        observed_ns,
        created_at,
        latency_ms,
        first_token_ms: None,
        input,
        output,
        cached,
        cache_write: 0,
        tier: None,
        effort: None,
        prewarm: false,
    })
}

fn parse_batch(payload: &Value, now: i64) -> Result<Vec<UsageEvent>, &'static str> {
    let resources = payload
        .get("resourceLogs")
        .and_then(Value::as_array)
        .ok_or("Expected OTLP JSON resourceLogs")?;
    let mut events = Vec::new();
    let mut record_count = 0usize;
    for resource in resources {
        let Some(scopes) = resource.get("scopeLogs").and_then(Value::as_array) else {
            continue;
        };
        for scope in scopes {
            let Some(records) = scope.get("logRecords").and_then(Value::as_array) else {
                continue;
            };
            record_count += records.len();
            if record_count > MAX_BATCH_RECORDS {
                return Err("OTLP batch has too many records");
            }
            events.extend(records.iter().filter_map(|record| parse_event(record, now)));
        }
    }
    Ok(events)
}

fn enqueue(db: &Database, events: &[UsageEvent], now: i64) -> Result<(), AppError> {
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    // Keep replay markers longer than the maximum accepted event age so a pruned
    // request can never be restored by an exporter retry.
    tx.execute(
        "DELETE FROM otel_usage_ingest WHERE created_at < ?1",
        [now - MAX_AGE_SECONDS - 86400],
    )?;
    for event in events {
        tx.execute(
            "INSERT OR IGNORE INTO otel_usage_ingest (event_id, event_json, created_at)
             VALUES (?1, ?2, ?3)",
            params![
                event.id(),
                serde_json::to_string(event).map_err(|e| AppError::Message(e.to_string()))?,
                event.created_at
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Locate the newest `state_<N>.sqlite` (or `state.sqlite`) in `codex_dir`.
/// Codex bumps the state DB version across releases (e.g. `state_5.sqlite` -> `state_6.sqlite`).
fn find_latest_codex_state_db(codex_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(codex_dir).ok()?;
    let mut best_numbered: Option<(u32, PathBuf)> = None;
    let mut fallback: Option<PathBuf> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == "state.sqlite" {
            fallback = Some(path);
            continue;
        }
        if let Some(num_str) = name
            .strip_prefix("state_")
            .and_then(|rest| rest.strip_suffix(".sqlite"))
        {
            if let Ok(num) = num_str.parse::<u32>() {
                if best_numbered.as_ref().is_none_or(|(best, _)| num > *best) {
                    best_numbered = Some((num, path));
                }
            }
        }
    }
    best_numbered.map(|(_, path)| path).or(fallback)
}

/// A read-only lookup distinguishes persisted Codex threads from ephemeral ones
/// before the JSONL scanner catches up.
///
/// If no `state_*.sqlite` exists yet (for example, a machine that has only run
/// `codex exec --ephemeral`), returns an empty set rather than stalling the queue.
/// If a state DB exists but cannot be opened/queried right now, returns `Err` so
/// queued events stay pending until the next sync retry.
fn persisted_codex_threads(
    codex_dir: &Path,
    events: &[UsageEvent],
) -> Result<HashSet<String>, AppError> {
    let codex_ids: Vec<&str> = events
        .iter()
        .filter(|e| e.app == OtelAppKind::Codex)
        .map(|e| e.session_id.as_str())
        .collect();
    if codex_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let Some(state_path) = find_latest_codex_state_db(codex_dir) else {
        return Ok(HashSet::new());
    };
    let state = Connection::open_with_flags(
        &state_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    state.busy_timeout(Duration::from_millis(250))?;
    let has_threads_table: bool = state.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'threads')",
        [],
        |r| r.get(0),
    )?;
    if !has_threads_table {
        return Ok(HashSet::new());
    }
    let has_rollout_path: bool = state.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('threads') WHERE name = 'rollout_path')",
        [],
        |r| r.get(0),
    )?;
    let sql = if has_rollout_path {
        "SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?1 AND COALESCE(TRIM(rollout_path), '') <> '')"
    } else {
        "SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?1)"
    };
    let mut query = state.prepare(sql)?;
    let mut persisted = HashSet::new();
    for id in codex_ids {
        if query.query_row([id], |r| r.get::<_, bool>(0))? {
            persisted.insert(id.to_owned());
        }
    }
    Ok(persisted)
}

pub fn sync_pending(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_pending_at(db, &crate::codex_config::get_codex_config_dir())
}

fn sync_pending_at(db: &Database, codex_dir: &Path) -> Result<SessionSyncResult, AppError> {
    let pending = {
        let conn = lock_conn!(db.conn);
        let mut query = conn.prepare(
            "SELECT event_json FROM otel_usage_ingest
             WHERE event_json IS NOT NULL
             ORDER BY created_at
             LIMIT 4096",
        )?;
        let rows = query
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|s| {
                serde_json::from_str::<UsageEvent>(&s).map_err(|e| AppError::Message(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut result = SessionSyncResult::default();
    if pending.is_empty() {
        return Ok(result);
    }
    let persisted_codex = persisted_codex_threads(codex_dir, &pending)?;
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for event in pending {
        let id = event.id();
        let still_pending: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM otel_usage_ingest WHERE event_id = ?1 AND event_json IS NOT NULL)",
            [&id],
            |r| r.get(0),
        )?;
        if !still_pending {
            continue;
        }
        let has_persisted_session: bool = tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM proxy_request_logs
                WHERE app_type = ?1 AND data_source = ?2 AND session_id = ?3
            )",
            params![
                event.app.app_type(),
                event.app.persisted_session_source(),
                &event.session_id
            ],
            |r| r.get(0),
        )?;
        let is_persisted_codex =
            event.app == OtelAppKind::Codex && persisted_codex.contains(&event.session_id);
        if is_persisted_codex || has_persisted_session {
            result.skipped = result.skipped.saturating_add(1);
        } else if insert_usage(&tx, &event)? {
            result.imported = result.imported.saturating_add(1);
        } else {
            result.skipped = result.skipped.saturating_add(1);
        }
        tx.execute(
            "UPDATE otel_usage_ingest SET event_json = NULL WHERE event_id = ?1",
            [&id],
        )?;
    }
    tx.commit()?;
    Ok(result)
}

fn insert_usage(conn: &Connection, event: &UsageEvent) -> Result<bool, AppError> {
    let id = event.id();
    let app_type = event.app.app_type();
    let data_source = event.app.data_source(event.prewarm);
    let pricing_lookup_model = match event.app {
        OtelAppKind::Codex => normalize_codex_model(&event.model),
        OtelAppKind::Claude | OtelAppKind::Gemini => event.model.clone(),
    };
    let key = DedupKey {
        app_type,
        data_source,
        model: &pricing_lookup_model,
        input_tokens: event.input,
        output_tokens: event.output,
        cache_read_tokens: event.cached,
        cache_creation_tokens: event.cache_write,
        created_at: event.created_at,
    };
    if should_skip_session_insert(conn, &id, &key)? {
        return Ok(false);
    }
    let usage = TokenUsage {
        input_tokens: event.input,
        output_tokens: event.output,
        cache_read_tokens: event.cached,
        cache_creation_tokens: event.cache_write,
        cache_creation_1h_tokens: 0,
        model: Some(pricing_lookup_model.clone()),
        message_id: None,
    };
    let pricing = find_model_pricing(conn, &pricing_lookup_model);
    let [input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost] =
        cost_columns(app_type, &usage, pricing.as_ref(), Decimal::ONE);

    let inserted = conn.execute(
        "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_token_semantics,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 200, NULL, ?19, ?20, 1, '1.0', ?21, ?22)",
        params![
            id,
            event.app.provider_id(),
            app_type,
            pricing_lookup_model,
            event.model,
            pricing_lookup_model,
            event.input,
            event.output,
            event.cached,
            event.cache_write,
            event.app.input_token_semantics(),
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            event.latency_ms as i64,
            event.first_token_ms.map(|v| v as i64),
            event.session_id,
            data_source,
            event.created_at,
            data_source,
        ],
    )?;
    Ok(inserted > 0)
}

fn parse_loopback_otel_endpoint(endpoint: &str) -> Option<SocketAddrV4> {
    let url = url::Url::parse(endpoint.trim()).ok()?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let host = url.host_str()?;
    if host != "127.0.0.1" && !host.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    if !matches!(
        path,
        "" | OTEL_LOGS_PATH | CODEX_LOGS_PATH | STANDARD_LOGS_PATH
    ) {
        return None;
    }
    let port = url.port()?;
    (port != 0).then_some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

fn configured_codex_address(config: &str) -> Option<SocketAddrV4> {
    let config: toml::Value = toml::from_str(config).ok()?;
    let exporter = config.get("otel")?.get("exporter")?.get("otlp-http")?;
    let protocol = exporter.get("protocol")?.as_str()?;
    if protocol != "json" && protocol != "http/json" {
        return None;
    }
    let endpoint = exporter.get("endpoint")?.as_str()?;
    let url = url::Url::parse(endpoint.trim()).ok()?;
    let path = url.path().trim_end_matches('/');
    if !matches!(path, OTEL_LOGS_PATH | CODEX_LOGS_PATH | STANDARD_LOGS_PATH) {
        return None;
    }
    parse_loopback_otel_endpoint(endpoint)
}

fn configured_claude_address(settings: &Value) -> Option<SocketAddrV4> {
    let env = settings.get("env")?.as_object()?;
    let protocol = env
        .get("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL")
        .or_else(|| env.get("OTEL_EXPORTER_OTLP_PROTOCOL"))
        .and_then(Value::as_str)?;
    if protocol != "http/json" && protocol != "json" {
        return None;
    }
    let endpoint = env
        .get("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT")
        .or_else(|| env.get("OTEL_EXPORTER_OTLP_ENDPOINT"))
        .and_then(Value::as_str)?;
    parse_loopback_otel_endpoint(endpoint)
}

fn configured_addresses() -> Vec<SocketAddrV4> {
    let mut addrs = BTreeSet::new();
    if let Ok(config) = crate::codex_config::read_codex_config_text() {
        if let Some(addr) = configured_codex_address(&config) {
            addrs.insert(addr);
        }
    }
    let claude_path = crate::config::get_claude_settings_path();
    if let Ok(settings) = crate::config::read_json_file::<Value>(&claude_path) {
        if let Some(addr) = configured_claude_address(&settings) {
            addrs.insert(addr);
        }
    }
    if let Ok(port_str) = std::env::var("CC_SWITCH_OTEL_LISTEN_PORT") {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            if port != 0 {
                addrs.insert(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
            }
        }
    }
    addrs.into_iter().collect()
}

#[derive(Clone)]
struct CollectorState {
    db: Arc<Database>,
    codex_dir: PathBuf,
}

async fn ingest_payload(
    db: Arc<Database>,
    codex_dir: PathBuf,
    headers: &HeaderMap,
    payload: Value,
) -> (StatusCode, Json<Value>) {
    // Reject browser-origin submissions (including preflightless POSTs).
    if headers.contains_key("origin") {
        return (StatusCode::FORBIDDEN, Json(json!({})));
    }
    static INGEST_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    let Ok(_permit) = INGEST_SLOTS.try_acquire() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"message":"Usage collector busy"})),
        );
    };
    let now = Utc::now().timestamp();
    let events = match parse_batch(&payload, now) {
        Ok(events) => events,
        Err(message) => return (StatusCode::BAD_REQUEST, Json(json!({"message":message}))),
    };
    if events.is_empty() {
        return (StatusCode::OK, Json(json!({})));
    }
    let result = tokio::task::spawn_blocking(move || {
        enqueue(&db, &events, Utc::now().timestamp())?;
        match sync_pending_at(&db, &codex_dir) {
            Ok(result) => crate::services::session_usage::notify_sync_result(&result),
            // Sanitized events remain durably queued; scheduled/manual scans retry.
            Err(error) => {
                log::warn!(
                    "[OTEL-USAGE] Usage queued; local session classification will retry: {error}"
                )
            }
        }
        Ok::<(), AppError>(())
    })
    .await;
    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({}))),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"message":"Usage storage temporarily unavailable"})),
        ),
    }
}

async fn receive(
    State(state): State<CollectorState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    ingest_payload(state.db, state.codex_dir, &headers, payload).await
}

/// Mounted directly on the main ProxyServer router so users who already run the
/// local proxy can send OTLP/JSON logs to the same loopback port without binding
/// a second port.
pub(crate) async fn receive_from_proxy(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> (StatusCode, Json<Value>) {
    if body.len() > MAX_BODY_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({"message":"OTLP payload exceeds 4 MiB limit"})),
        );
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"message":"Expected valid OTLP JSON payload"})),
            )
        }
    };
    ingest_payload(
        state.db,
        crate::codex_config::get_codex_config_dir(),
        &headers,
        payload,
    )
    .await
}

pub(crate) async fn health_response() -> Json<Value> {
    Json(json!({
        "service": "cc-switch-otel-usage",
        "version": 1,
        "apps": ["codex", "claude", "gemini"]
    }))
}

fn router(db: Arc<Database>, codex_dir: PathBuf) -> Router {
    Router::new()
        .route(OTEL_LOGS_PATH, post(receive))
        .route(CODEX_LOGS_PATH, post(receive))
        .route(STANDARD_LOGS_PATH, post(receive))
        .route(OTEL_HEALTH_PATH, get(health_response))
        .route(CODEX_HEALTH_PATH, get(health_response))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(CollectorState { db, codex_dir })
}

pub async fn serve_if_configured(db: Arc<Database>) {
    let addresses = configured_addresses();
    if addresses.is_empty() {
        return;
    }
    let codex_dir = crate::codex_config::get_codex_config_dir();
    for address in addresses {
        let db = db.clone();
        let codex_dir = codex_dir.clone();
        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(address).await {
                Ok(listener) => listener,
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                    log::info!(
                        "[OTEL-USAGE] Loopback port {address} already bound (e.g. served by CC Switch proxy)"
                    );
                    return;
                }
                Err(error) => {
                    log::error!("[OTEL-USAGE] Cannot listen on {address}: {error}");
                    return;
                }
            };
            log::info!("[OTEL-USAGE] Local OTLP/JSON usage collector listening on {address}");
            if let Err(error) = axum::serve(listener, router(db, codex_dir)).await {
                log::error!("[OTEL-USAGE] Collector on {address} stopped: {error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::usage_stats::LogFilters;
    use tempfile::tempdir;

    const THREAD: &str = "12345678-1234-4234-8234-123456789012";
    const NOW: i64 = 1_788_676_853;

    fn codex_record() -> Value {
        let fields = [
            ("event.name", json!({"stringValue": "codex.sse_event"})),
            ("event.kind", json!({"stringValue": "response.completed"})),
            (
                "event.timestamp",
                json!({"stringValue": "2026-09-06T06:40:53.786Z"}),
            ),
            ("conversation.id", json!({"stringValue": THREAD})),
            ("model", json!({"stringValue": "gpt-6.2-sol"})),
            ("input_token_count", json!({"stringValue": "1200"})),
            ("output_token_count", json!({"stringValue": "200"})),
            ("cached_token_count", json!({"intValue": "1000"})),
            ("cache_write_token_count", json!({"intValue": "0"})),
            ("reasoning_token_count", json!({"intValue": "50"})),
            ("duration_ms", json!({"stringValue": "2400"})),
            ("ttft_ms", json!({"intValue": "450"})),
            ("service_tier", json!({"stringValue": "ultrafast"})),
            ("model_reasoning_effort", json!({"stringValue": "high"})),
            ("originator", json!({"stringValue": "codex_exec"})),
            (
                "user.email",
                json!({"stringValue": "private-account@example.test"}),
            ),
            ("prompt", json!({"stringValue": "private-prompt"})),
            ("tool_output", json!({"stringValue": "private-tool-output"})),
        ];
        json!({
            "timeUnixNano": "0",
            "observedTimeUnixNano": "1788676853786995800",
            "attributes": fields
                .into_iter()
                .map(|(key, value)| json!({"key": key, "value": value}))
                .collect::<Vec<_>>(),
            "body": {"stringValue": "private-body"}
        })
    }

    fn claude_record() -> Value {
        let fields = [
            (
                "event.name",
                json!({"stringValue": "claude_code.api_request"}),
            ),
            (
                "event.timestamp",
                json!({"stringValue": "2026-09-06T06:40:53.786Z"}),
            ),
            ("session.id", json!({"stringValue": "claude-ephemeral-1"})),
            ("model", json!({"stringValue": "claude-sonnet-4-5"})),
            ("input_tokens", json!({"intValue": "500"})),
            ("output_tokens", json!({"intValue": "120"})),
            ("cache_read_tokens", json!({"intValue": "3000"})),
            ("cache_creation_tokens", json!({"intValue": "200"})),
            ("duration_ms", json!({"intValue": "1800"})),
            ("prompt", json!({"stringValue": "private-claude-prompt"})),
        ];
        json!({
            "timeUnixNano": "1788676853786000000",
            "attributes": fields
                .into_iter()
                .map(|(key, value)| json!({"key": key, "value": value}))
                .collect::<Vec<_>>(),
        })
    }

    fn gemini_record() -> Value {
        let fields = [
            (
                "event.name",
                json!({"stringValue": "gemini_cli.api_response"}),
            ),
            (
                "event.timestamp",
                json!({"stringValue": "2026-09-06T06:40:53.786Z"}),
            ),
            ("session.id", json!({"stringValue": "gemini-cli-1"})),
            ("model", json!({"stringValue": "gemini-2.5-pro"})),
            ("input_token_count", json!({"intValue": "1500"})),
            ("output_token_count", json!({"intValue": "300"})),
            ("thoughts_token_count", json!({"intValue": "100"})),
            ("cached_content_token_count", json!({"intValue": "500"})),
            ("duration_ms", json!({"intValue": "1250"})),
        ];
        json!({
            "timeUnixNano": "1788676853786000000",
            "attributes": fields
                .into_iter()
                .map(|(key, value)| json!({"key": key, "value": value}))
                .collect::<Vec<_>>(),
        })
    }

    fn payload(records: Vec<Value>) -> Value {
        json!({
            "resourceLogs": [{
                "resource": {
                    "attributes": [{"key": "host.name", "value": {"stringValue": "private-host"}}]
                },
                "scopeLogs": [{"logRecords": records}]
            }]
        })
    }

    fn replace(record: &mut Value, key: &str, value: Value) {
        let attribute = record["attributes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|a| a["key"] == key)
            .unwrap();
        attribute["value"] = value;
    }

    fn prewarm_record() -> Value {
        let mut record = codex_record();
        record["attributes"].as_array_mut().unwrap().retain(|a| {
            !matches!(
                a["key"].as_str(),
                Some("service_tier" | "model_reasoning_effort" | "ttft_ms")
            )
        });
        record["attributes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"key": "app.version", "value": {"stringValue": "0.153.4"}}));
        for field in [
            "output_token_count",
            "cached_token_count",
            "reasoning_token_count",
        ] {
            replace(&mut record, field, json!({"intValue": "0"}));
        }
        record
    }

    #[test]
    fn codex_exec_ephemeral_ultrafast_event_imports_without_state_db() -> Result<(), AppError> {
        let events = parse_batch(&payload(vec![codex_record()]), NOW).unwrap();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.app, OtelAppKind::Codex);
        assert_eq!(event.model, "gpt-6.2-sol");
        assert_eq!(event.tier.as_deref(), Some("ultrafast"));
        assert_eq!(event.effort.as_deref(), Some("high"));
        assert_eq!(event.latency_ms, 2400);
        assert_eq!(event.first_token_ms, Some(450));
        assert_eq!(
            (event.input, event.output, event.cached, event.cache_write),
            (1200, 200, 1000, 0)
        );

        let db = Database::memory()?;
        enqueue(&db, &events, NOW)?;
        {
            let conn = lock_conn!(db.conn);
            let saved: String =
                conn.query_row("SELECT event_json FROM otel_usage_ingest", [], |r| r.get(0))?;
            assert!(!saved.contains("private-"));
        }

        // Empty codex_dir (no state_5.sqlite yet): ephemeral event MUST still import!
        let empty_codex_dir = tempdir().unwrap();
        let sync = sync_pending_at(&db, empty_codex_dir.path())?;
        assert_eq!((sync.imported, sync.skipped), (1, 0));

        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.total_input_tokens, 200); // 1200 total - 1000 cached = 200 fresh
        assert_eq!(summary.total_output_tokens, 200);
        assert_eq!(summary.total_cache_read_tokens, 1000);

        let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
        assert_eq!(logs.total, 1);
        assert_eq!(
            logs.data[0].provider_name.as_deref(),
            Some("Codex (Temporary)")
        );
        assert_eq!(logs.data[0].data_source.as_deref(), Some("codex_otel"));

        // Replay marker prevents duplicate import after retry.
        enqueue(&db, &events, NOW)?;
        let retry = sync_pending_at(&db, empty_codex_dir.path())?;
        assert_eq!((retry.imported, retry.skipped), (0, 0));
        Ok(())
    }

    #[test]
    fn codex_state_db_skips_persisted_threads_but_keeps_ephemeral_rows() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        // Verify newest numbered state DB (state_6.sqlite > state_5.sqlite) is selected
        let state5 = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
        state5
            .execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT)")
            .unwrap();
        let state6 = Connection::open(dir.path().join("state_6.sqlite")).unwrap();
        state6
            .execute_batch(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT);
                 INSERT INTO threads (id, rollout_path) VALUES ('12345678-1234-4234-8234-123456789012', '/tmp/rollout.jsonl');
                 INSERT INTO threads (id, rollout_path) VALUES ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', '');",
            )
            .unwrap();

        let persisted_rec = codex_record();
        let mut ephemeral_rec = codex_record();
        replace(
            &mut ephemeral_rec,
            "conversation.id",
            json!({"stringValue": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"}),
        );

        let events = parse_batch(&payload(vec![persisted_rec, ephemeral_rec]), NOW).unwrap();
        let db = Database::memory()?;
        enqueue(&db, &events, NOW)?;
        let sync = sync_pending_at(&db, dir.path())?;
        assert_eq!((sync.imported, sync.skipped), (1, 1));
        Ok(())
    }

    #[test]
    fn claude_and_gemini_otel_events_import_with_correct_semantics() -> Result<(), AppError> {
        let events = parse_batch(&payload(vec![claude_record(), gemini_record()]), NOW).unwrap();
        assert_eq!(events.len(), 2);

        let db = Database::memory()?;
        enqueue(&db, &events, NOW)?;
        let empty_dir = tempdir().unwrap();
        let sync = sync_pending_at(&db, empty_dir.path())?;
        assert_eq!((sync.imported, sync.skipped), (2, 0));

        let by_app = db.get_usage_summary_by_app(None, None, None, None)?;
        let claude = by_app.iter().find(|r| r.app_type == "claude").unwrap();
        // Claude uses fresh input semantics: 500 stays 500
        assert_eq!(claude.summary.total_input_tokens, 500);
        assert_eq!(claude.summary.total_output_tokens, 120);
        assert_eq!(claude.summary.total_cache_read_tokens, 3000);
        assert_eq!(claude.summary.total_cache_creation_tokens, 200);

        let gemini = by_app.iter().find(|r| r.app_type == "gemini").unwrap();
        // Gemini uses total input semantics: 1500 - 500 = 1000 fresh; output = 300 + 100 thoughts = 400
        assert_eq!(gemini.summary.total_input_tokens, 1000);
        assert_eq!(gemini.summary.total_output_tokens, 400);
        assert_eq!(gemini.summary.total_cache_read_tokens, 500);
        Ok(())
    }

    #[test]
    fn prewarm_and_late_persisted_session_are_excluded_from_stats() -> Result<(), AppError> {
        let db = Database::memory()?;
        let empty_dir = tempdir().unwrap();

        let events = parse_batch(&payload(vec![prewarm_record(), codex_record()]), NOW).unwrap();
        enqueue(&db, &events, NOW)?;
        let sync = sync_pending_at(&db, empty_dir.path())?;
        assert_eq!(sync.imported, 2);

        // Prewarm is excluded from summary, request list, and session summary.
        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.total_requests, 1);
        let session_summary = db.get_session_usage_summary("codex", THREAD)?;
        assert_eq!(session_summary.total_requests, 1);

        // Simulate late-arriving persisted codex_session row for the same thread:
        // persisted session must take precedence over codex_otel (no double counting!).
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_token_semantics, input_cost_usd, output_cost_usd,
                    cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                    latency_ms, status_code, session_id, created_at, data_source
                ) VALUES (
                    'codex_session:thread-v1:late:1', '_codex_session', 'codex', 'gpt-6.2-sol', 'gpt-6.2-sol',
                    1200, 200, 1000, 0, 1, '0', '0', '0', '0', '0',
                    0, 200, ?1, ?2, 'codex_session'
                )",
                params![THREAD, NOW],
            )?;
        }

        let summary_after = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary_after.total_requests, 1);
        let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
        assert_eq!(logs.total, 1);
        assert_eq!(logs.data[0].data_source.as_deref(), Some("codex_session"));
        Ok(())
    }

    #[test]
    fn late_proxy_row_dedups_codex_otel_row() -> Result<(), AppError> {
        let db = Database::memory()?;
        let empty_dir = tempdir().unwrap();

        let events = parse_batch(&payload(vec![codex_record()]), NOW).unwrap();
        enqueue(&db, &events, NOW)?;
        sync_pending_at(&db, empty_dir.path())?;
        assert_eq!(
            db.get_usage_summary(None, None, None, None, None)?
                .total_requests,
            1
        );

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_token_semantics, input_cost_usd, output_cost_usd,
                    cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                    latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'proxy-req-1', 'prov-1', 'codex', 'gpt-6.2-sol', 'gpt-6.2-sol',
                    1200, 200, 1000, 0, 1, '0', '0', '0', '0', '0',
                    100, 200, ?1, 'proxy'
                )",
                params![NOW + 5],
            )?;
        }

        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.total_requests, 1);
        let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
        assert_eq!(logs.total, 1);
        assert_eq!(logs.data[0].data_source.as_deref(), Some("proxy"));
        Ok(())
    }

    #[test]
    fn endpoint_config_accepts_only_loopback_json_paths() {
        let codex_cfg = |endpoint: &str, protocol: &str| {
            format!(
                "[otel]\nexporter = {{ otlp-http = {{ endpoint = '{endpoint}', protocol = '{protocol}' }} }}"
            )
        };
        assert_eq!(
            configured_codex_address(&codex_cfg(
                &format!("http://127.0.0.1:4319{CODEX_LOGS_PATH}"),
                "json"
            ))
            .unwrap()
            .port(),
            4319
        );
        assert_eq!(
            configured_codex_address(&codex_cfg(
                &format!("http://localhost:4319{OTEL_LOGS_PATH}"),
                "json"
            ))
            .unwrap()
            .port(),
            4319
        );
        assert_eq!(
            configured_codex_address(&codex_cfg("http://127.0.0.1:4319/v1/logs", "json"))
                .unwrap()
                .port(),
            4319
        );
        assert!(configured_codex_address(&codex_cfg(
            &format!("http://127.0.0.1:4319{CODEX_LOGS_PATH}"),
            "binary"
        ))
        .is_none());
        assert!(configured_codex_address(&codex_cfg(
            &format!("http://0.0.0.0:4319{CODEX_LOGS_PATH}"),
            "json"
        ))
        .is_none());
        assert!(configured_codex_address(&codex_cfg(
            &format!("https://127.0.0.1:4319{CODEX_LOGS_PATH}"),
            "json"
        ))
        .is_none());

        let claude_cfg = json!({
            "env": {
                "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
                "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json",
                "OTEL_EXPORTER_OTLP_ENDPOINT": "http://127.0.0.1:4319"
            }
        });
        assert_eq!(configured_claude_address(&claude_cfg).unwrap().port(), 4319);
    }

    #[test]
    fn ignores_invalid_events_and_oversized_batches() {
        let base = codex_record();
        for (field, value) in [
            ("event.name", json!({"stringValue": "codex.tool_result"})),
            ("event.kind", json!({"stringValue": "message"})),
            ("input_token_count", json!({"stringValue": "-1"})),
            ("input_token_count", json!({"stringValue": "4294967296"})),
            ("output_token_count", json!({"doubleValue": 20.5})),
            ("cached_token_count", json!({"intValue": "1201"})),
            ("cache_write_token_count", json!({"intValue": "201"})),
            ("model", json!({"stringValue": "codex-auto-review"})),
        ] {
            let mut invalid = base.clone();
            replace(&mut invalid, field, value);
            assert!(parse_event(&invalid, NOW).is_none(), "{field}");
        }
        assert!(parse_event(&base, NOW + MAX_AGE_SECONDS + 1).is_none());
        assert!(parse_event(&base, NOW - 301).is_none());
        assert!(parse_batch(&json!({}), NOW).is_err());
        assert!(parse_batch(&payload(vec![json!({}); MAX_BATCH_RECORDS + 1]), NOW).is_err());
    }

    #[tokio::test]
    async fn http_router_accepts_loopback_otlp_and_rejects_browser_origin() -> Result<(), AppError>
    {
        use axum::body::Body;
        use http::Request;
        use tower::Service;

        let db = Arc::new(Database::memory()?);
        let dir = tempdir().unwrap();
        let app = router(db.clone(), dir.path().to_path_buf());
        let send = |mut router: Router, req: Request<Body>| async move {
            <Router as Service<Request<Body>>>::call(&mut router, req)
                .await
                .unwrap()
        };

        // Health endpoint
        let health = send(
            app.clone(),
            Request::builder()
                .uri(OTEL_HEALTH_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(health.status(), StatusCode::OK);

        // Browser Origin is rejected with 403
        let mut live_codex = codex_record();
        let now_rfc3339 = Utc::now().to_rfc3339();
        replace(
            &mut live_codex,
            "event.timestamp",
            json!({"stringValue": now_rfc3339}),
        );
        let body_bytes = serde_json::to_vec(&payload(vec![live_codex])).unwrap();

        let forbidden = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(CODEX_LOGS_PATH)
                .header("content-type", "application/json")
                .header("origin", "https://evil.example")
                .body(Body::from(body_bytes.clone()))
                .unwrap(),
        )
        .await;
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        // Valid CLI POST to /v1/cc-switch/codex/logs ingests immediately
        let ok = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(CODEX_LOGS_PATH)
                .header("content-type", "application/json")
                .body(Body::from(body_bytes))
                .unwrap(),
        )
        .await;
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(
            db.get_usage_summary(None, None, None, None, None)?
                .total_requests,
            1
        );

        // Valid Claude CLI POST to standard /v1/logs path ingests immediately
        let mut live_claude = claude_record();
        replace(
            &mut live_claude,
            "event.timestamp",
            json!({"stringValue": Utc::now().to_rfc3339()}),
        );
        let claude_bytes = serde_json::to_vec(&payload(vec![live_claude])).unwrap();
        let ok_claude = send(
            app,
            Request::builder()
                .method("POST")
                .uri(STANDARD_LOGS_PATH)
                .header("content-type", "application/json")
                .body(Body::from(claude_bytes))
                .unwrap(),
        )
        .await;
        assert_eq!(ok_claude.status(), StatusCode::OK);
        assert_eq!(
            db.get_usage_summary(None, None, None, None, None)?
                .total_requests,
            2
        );
        Ok(())
    }
}
