//! Loopback OTLP/JSON ingestion for Codex ephemeral sessions (including side chats).
//! Only sanitized usage events are retained; ordinary persisted sessions use the
//! existing JSONL importer. Codex owns the opt-in exporter configuration.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostBreakdown, CostCalculator};
use crate::proxy::usage::fast_pricing;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::SessionSyncResult;
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
    collections::{HashMap, HashSet},
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const LOGS_PATH: &str = "/v1/cc-switch/codex/logs";
const MAX_BATCH_RECORDS: usize = 4096;
const MAX_AGE_SECONDS: i64 = 7 * 86400;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsageEvent {
    thread_id: String,
    model: String,
    observed_ns: u64,
    created_at: i64,
    input: u32,
    output: u32,
    cached: u32,
    cache_write: u32,
    tier: Option<String>,
    effort: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    prewarm: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl UsageEvent {
    fn id(&self) -> String {
        // Classification is not part of event identity. Keep existing retry
        // markers valid when a formerly imported event is recognized as prewarm.
        let mut identity = self.clone();
        identity.prewarm = false;
        let bytes = serde_json::to_vec(&identity).expect("usage event is serializable");
        format!("codex_otel:{:x}", Sha256::digest(bytes))
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
        .and_then(|s| s.parse().ok())
        .or_else(|| value.get("intValue").and_then(Value::as_u64))
}

fn parse_event(record: &Value, now: i64) -> Option<UsageEvent> {
    let attributes: HashMap<&str, &Value> = record
        .get("attributes")?
        .as_array()?
        .iter()
        .filter_map(|a| Some((a.get("key")?.as_str()?, a.get("value")?)))
        .collect();
    let text = |key: &str| attributes.get(key).and_then(|v| attribute_text(v));
    if text("event.name") != Some("codex.sse_event")
        || text("event.kind") != Some("response.completed")
    {
        return None;
    }
    // Internal approval checks are not user side chats.
    let model = text("model")?;
    if model == "codex-auto-review"
        || model.is_empty()
        || model.len() > 200
        || !model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._:/".contains(c))
    {
        return None;
    }
    let thread_id = uuid::Uuid::parse_str(text("conversation.id")?)
        .ok()?
        .to_string();
    let timestamp = text("event.timestamp").and_then(|s| DateTime::parse_from_rfc3339(s).ok());
    // Codex currently sends timeUnixNano=0 and a precise observedTimeUnixNano.
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
    let tier = text("service_tier")
        .filter(|s| matches!(*s, "fast" | "priority" | "default" | "flex" | "auto"))
        .map(str::to_owned);
    let effort = text("model_reasoning_effort")
        .filter(|s| {
            matches!(
                *s,
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
            )
        })
        .map(str::to_owned);
    // Codex 0.153.4 emits response.completed for generate=false WebSocket
    // prewarm too. A local app-server/mock-API reproduction verifies this exact
    // signature: no request metadata, no first token, and input-only usage.
    // Restrict this inference to the verified version and reasoning-model
    // families; keep unfamiliar producers and normal zero-output requests.
    // Retain the accounting fields separately rather than deleting the event.
    let prewarm = text("app.version") == Some("0.153.4")
        && (model.starts_with("gpt-5.") || model.starts_with("gpt-6-"))
        && output == 0
        && cached == 0
        && cache_write == 0
        && optional_count("reasoning_token_count") == Some(0)
        && !attributes.contains_key("service_tier")
        && !attributes.contains_key("model_reasoning_effort")
        && !attributes.contains_key("ttft_ms");
    Some(UsageEvent {
        thread_id,
        model: model.to_owned(),
        observed_ns,
        created_at,
        input,
        output,
        cached,
        cache_write,
        tier,
        effort,
        prewarm,
    })
}

fn parse_batch(payload: &Value, now: i64) -> Result<Vec<UsageEvent>, &'static str> {
    let resources = payload
        .get("resourceLogs")
        .and_then(Value::as_array)
        .ok_or("Expected OTLP JSON resourceLogs")?;
    let mut events = Vec::new();
    let mut record_count = 0;
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
    // Keep replay markers longer than the maximum accepted event age. A pruned
    // request must never be restored by an exporter retry.
    tx.execute(
        "DELETE FROM codex_otel_ingest WHERE created_at < ?1",
        [now - MAX_AGE_SECONDS - 86400],
    )?;
    for event in events {
        tx.execute("INSERT OR IGNORE INTO codex_otel_ingest (event_id,event_json,created_at) VALUES (?1,?2,?3)",
            params![event.id(), serde_json::to_string(event).map_err(|e| AppError::Message(e.to_string()))?, event.created_at])?;
    }
    tx.commit()?;
    Ok(())
}

/// A read-only state lookup distinguishes ephemeral sessions from ordinary
/// sessions before their JSONL scan catches up. Failure leaves the queue intact.
fn persisted_threads(
    state_path: &Path,
    events: &[UsageEvent],
) -> Result<HashSet<String>, AppError> {
    let state = Connection::open_with_flags(
        state_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    state.busy_timeout(Duration::from_millis(250))?;
    let mut query = state.prepare("SELECT EXISTS(SELECT 1 FROM threads WHERE id=?1)")?;
    let mut persisted = HashSet::new();
    for event in events {
        if query.query_row([&event.thread_id], |r| r.get::<_, bool>(0))? {
            persisted.insert(event.thread_id.clone());
        }
    }
    Ok(persisted)
}

pub fn sync_pending(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_pending_at(
        db,
        &crate::codex_config::get_codex_config_dir().join("state_5.sqlite"),
    )
}

fn sync_pending_at(db: &Database, state_path: &Path) -> Result<SessionSyncResult, AppError> {
    let pending = {
        let conn = lock_conn!(db.conn);
        let mut query = conn.prepare("SELECT event_json FROM codex_otel_ingest WHERE event_json IS NOT NULL ORDER BY created_at LIMIT 4096")?;
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
    let persisted = persisted_threads(state_path, &pending)?;
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for event in pending {
        let id = event.id();
        let still_pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM codex_otel_ingest WHERE event_id=?1 AND event_json IS NOT NULL)", [&id], |r| r.get(0))?;
        if !still_pending {
            continue;
        }
        let has_session: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE app_type='codex' AND data_source='codex_session' AND session_id=?1)", [&event.thread_id], |r| r.get(0))?;
        if persisted.contains(&event.thread_id) || has_session {
            result.skipped += 1;
        } else if insert_usage(&tx, &event)? {
            result.imported += 1;
        } else {
            result.skipped += 1;
        }
        tx.execute(
            "UPDATE codex_otel_ingest SET event_json=NULL WHERE event_id=?1",
            [&id],
        )?;
    }
    tx.commit()?;
    Ok(result)
}

fn insert_usage(conn: &Connection, event: &UsageEvent) -> Result<bool, AppError> {
    let id = event.id();
    let key = DedupKey {
        app_type: "codex",
        model: &event.model,
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
        model: Some(event.model.clone()),
        message_id: None,
    };
    let mut cost = find_model_pricing(conn, &event.model)
        .map(|p| CostCalculator::calculate_for_app("codex", &usage, &p, Decimal::ONE))
        .unwrap_or(CostBreakdown {
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            cache_read_cost: Decimal::ZERO,
            cache_creation_cost: Decimal::ZERO,
            total_cost: Decimal::ZERO,
        });
    if let Some(factors) =
        fast_pricing::factors(&event.model, event.tier.as_deref(), u64::from(event.input))
    {
        fast_pricing::apply(&mut cost, factors, Decimal::ONE);
    }
    let inserted = conn.execute("INSERT OR IGNORE INTO proxy_request_logs (
        request_id,provider_id,app_type,model,request_model,pricing_model,
        input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,input_token_semantics,
        input_cost_usd,output_cost_usd,cache_read_cost_usd,cache_creation_cost_usd,total_cost_usd,
        latency_ms,status_code,session_id,provider_type,is_streaming,cost_multiplier,created_at,data_source,
        service_tier,service_tier_source,reasoning_effort,service_tier_pricing_version)
        VALUES (?1,'_codex_otel','codex',?2,?2,?2,?3,?4,?5,?6,1,?7,?8,?9,?10,?11,0,200,?12,'codex_otel',1,'1',?13,?17,?14,?15,?16,2)",
        params![id,event.model,event.input,event.output,event.cached,event.cache_write,cost.input_cost.to_string(),
            cost.output_cost.to_string(),cost.cache_read_cost.to_string(),cost.cache_creation_cost.to_string(),cost.total_cost.to_string(),
            event.thread_id,event.created_at,event.tier,event.tier.as_ref().map(|_| "request"),event.effort,
            if event.prewarm { "codex_otel_prewarm" } else { "codex_otel" }])?;
    Ok(inserted > 0)
}

fn configured_address(config: &str) -> Option<SocketAddrV4> {
    let config: toml::Value = toml::from_str(config).ok()?;
    let exporter = config.get("otel")?.get("exporter")?.get("otlp-http")?;
    if exporter.get("protocol")?.as_str()? != "json" {
        return None;
    }
    let url = url::Url::parse(exporter.get("endpoint")?.as_str()?).ok()?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.path() != LOGS_PATH
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let port = url.port()?;
    (port != 0).then_some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

#[derive(Clone)]
struct CollectorState {
    db: Arc<Database>,
    codex_state_path: PathBuf,
}

async fn receive(
    State(state): State<CollectorState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    // Never accept browser-origin telemetry submissions, including preflightless clients.
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
    let events = match parse_batch(&payload, Utc::now().timestamp()) {
        Ok(events) => events,
        Err(message) => return (StatusCode::BAD_REQUEST, Json(json!({"message":message}))),
    };
    let result = tokio::task::spawn_blocking(move || {
        enqueue(&state.db, &events, Utc::now().timestamp())?;
        match sync_pending_at(&state.db, &state.codex_state_path) {
            Ok(result) => crate::services::session_usage::notify_sync_result(&result),
            // The sanitized queue is durable. Scheduled/manual scans retry it.
            Err(_) => {
                log::warn!("[CODEX-OTEL] Usage queued; local session classification will retry")
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

fn router(db: Arc<Database>, codex_state_path: PathBuf) -> Router {
    Router::new()
        .route(LOGS_PATH, post(receive))
        .route(
            "/cc-switch/codex-otel/health",
            get(|| async { Json(json!({"service":"cc-switch-codex-otel","version":1})) }),
        )
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
        .with_state(CollectorState {
            db,
            codex_state_path,
        })
}

pub async fn serve_if_configured(db: Arc<Database>) {
    let Ok(config) = crate::codex_config::read_codex_config_text() else {
        return;
    };
    let Some(address) = configured_address(&config) else {
        return;
    };
    let listener = match tokio::net::TcpListener::bind(address).await {
        Ok(listener) => listener,
        Err(error) => {
            log::error!("[CODEX-OTEL] Cannot listen on {address}: {error}");
            return;
        }
    };
    log::info!("[CODEX-OTEL] Local usage collector listening on {address}");
    let state_path = crate::codex_config::get_codex_config_dir().join("state_5.sqlite");
    if let Err(error) = axum::serve(listener, router(db, state_path)).await {
        log::error!("[CODEX-OTEL] Collector stopped: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const THREAD: &str = "12345678-1234-4234-8234-123456789012";
    const NOW: i64 = 1_788_676_853;

    fn record() -> Value {
        // Matches a real Codex 0.153.4 ephemeral run against a local mock model.
        // account/prompt/tool fields deliberately exercise the persistence allowlist.
        let fields = [
            ("event.name", json!({"stringValue":"codex.sse_event"})),
            ("event.kind", json!({"stringValue":"response.completed"})),
            (
                "event.timestamp",
                json!({"stringValue":"2026-09-06T06:40:53.786Z"}),
            ),
            ("conversation.id", json!({"stringValue":THREAD})),
            ("model", json!({"stringValue":"gpt-5.4"})),
            ("input_token_count", json!({"stringValue":"1200"})),
            ("output_token_count", json!({"stringValue":"20"})),
            ("cached_token_count", json!({"intValue":"1000"})),
            ("cache_write_token_count", json!({"intValue":"0"})),
            ("reasoning_token_count", json!({"intValue":"10"})),
            ("service_tier", json!({"stringValue":"priority"})),
            ("model_reasoning_effort", json!({"stringValue":"low"})),
            (
                "user.email",
                json!({"stringValue":"private-account@example.test"}),
            ),
            ("prompt", json!({"stringValue":"private-prompt"})),
            ("tool_output", json!({"stringValue":"private-tool-output"})),
        ];
        json!({"timeUnixNano":"0","observedTimeUnixNano":"1788676853786995800",
            "attributes":fields.into_iter().map(|(key,value)| json!({"key":key,"value":value})).collect::<Vec<_>>(),
            "body":{"stringValue":"private-body"}})
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
        let mut record = record();
        record["attributes"]
            .as_array_mut()
            .unwrap()
            .retain(|a| a["key"] != "service_tier" && a["key"] != "model_reasoning_effort");
        record["attributes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"key":"app.version","value":{"stringValue":"0.153.4"}}));
        for field in [
            "output_token_count",
            "cached_token_count",
            "reasoning_token_count",
        ] {
            replace(&mut record, field, json!({"intValue":"0"}));
        }
        record
    }

    fn payload(records: Vec<Value>) -> Value {
        json!({"resourceLogs":[{"resource":{"attributes":[{"key":"host.name","value":{"stringValue":"private-host"}}]},
            "scopeLogs":[{"logRecords":records}]}]})
    }

    fn state_db(folder: &Path) -> std::path::PathBuf {
        let path = folder.join("state_5.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY)")
            .unwrap();
        path
    }

    fn configure_prices(db: &Database) {
        let conn = db.conn.lock().unwrap();
        conn.execute("UPDATE model_pricing SET input_cost_per_million='2',output_cost_per_million='10',cache_read_cost_per_million='0.2',cache_creation_cost_per_million='0.5' WHERE model_id='gpt-5.4'", []).unwrap();
    }

    #[test]
    fn real_otlp_shape_keeps_exact_counts_and_only_usage_fields() -> Result<(), AppError> {
        let events = parse_batch(&payload(vec![record()]), NOW).unwrap();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(
            (event.input, event.output, event.cached, event.cache_write),
            (1200, 20, 1000, 0)
        );
        assert_eq!(event.observed_ns, 1788676853786995800);
        assert_eq!(event.tier.as_deref(), Some("priority"));
        assert_eq!(event.effort.as_deref(), Some("low"));
        let db = Database::memory()?;
        enqueue(&db, &events, NOW)?;
        let conn = lock_conn!(db.conn);
        let saved: String =
            conn.query_row("SELECT event_json FROM codex_otel_ingest", [], |r| r.get(0))?;
        assert!(!saved.contains("private-"));
        assert!(!saved.contains("reasoning_token_count")); // already included in output
        Ok(())
    }

    #[test]
    fn ignores_non_usage_invalid_counts_and_stale_events() {
        let base = record();
        for (field, value) in [
            ("event.name", json!({"stringValue":"codex.tool_result"})),
            ("event.kind", json!({"stringValue":"message"})),
            ("input_token_count", json!({"stringValue":"-1"})),
            ("input_token_count", json!({"stringValue":"4294967296"})),
            ("output_token_count", json!({"doubleValue":20.5})),
            ("cached_token_count", json!({"intValue":"1201"})),
            ("cache_write_token_count", json!({"intValue":"201"})),
            ("model", json!({"stringValue":"codex-auto-review"})),
            ("conversation.id", json!({"stringValue":"not-a-thread"})),
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

    #[test]
    fn collector_only_accepts_explicit_loopback_json_configuration() {
        let config = |endpoint: &str, protocol: &str| {
            format!("[otel]\nexporter = {{ otlp-http = {{ endpoint = '{endpoint}', protocol = '{protocol}' }} }}")
        };
        let endpoint = format!("http://127.0.0.1:4319{LOGS_PATH}");
        assert_eq!(
            configured_address(&config(&endpoint, "json"))
                .unwrap()
                .port(),
            4319
        );
        assert!(configured_address(&config(&endpoint, "binary")).is_none());
        for url in [
            format!("http://0.0.0.0:4319{LOGS_PATH}"),
            format!("http://example.com:4319{LOGS_PATH}"),
            format!("https://127.0.0.1:4319{LOGS_PATH}"),
            "http://127.0.0.1:4319/v1/logs".to_string(),
            format!("{endpoint}?key=test"),
        ] {
            assert!(configured_address(&config(&url, "json")).is_none(), "{url}");
        }
        assert!(configured_address("[otel]\nexporter='none'").is_none());
    }

    #[test]
    fn prewarm_signature_does_not_exclude_normal_zero_output_or_unknown_producers() {
        let record = prewarm_record();
        let event = parse_event(&record, NOW).unwrap();
        assert!(event.prewarm);
        let mut legacy = event.clone();
        legacy.prewarm = false;
        assert_eq!(
            event.id(),
            legacy.id(),
            "classification must preserve retry identity"
        );
        let legacy_json = serde_json::to_string(&legacy).unwrap();
        assert!(!legacy_json.contains("prewarm"));
        assert!(
            !serde_json::from_str::<UsageEvent>(&legacy_json)
                .unwrap()
                .prewarm
        );

        for (key, value) in [
            ("service_tier", json!({"stringValue":"default"})),
            ("model_reasoning_effort", json!({"stringValue":"none"})),
            (
                "model_reasoning_effort",
                json!({"stringValue":"future-effort"}),
            ),
            ("ttft_ms", json!({"intValue":"0"})),
            ("output_token_count", json!({"intValue":"1"})),
            ("cached_token_count", json!({"intValue":"1"})),
            ("cache_write_token_count", json!({"intValue":"1"})),
            ("reasoning_token_count", json!({"intValue":"1"})),
            ("app.version", json!({"stringValue":"0.153.5"})),
            ("model", json!({"stringValue":"gpt-4.1"})),
        ] {
            let mut actual = record.clone();
            actual["attributes"]
                .as_array_mut()
                .unwrap()
                .retain(|a| a["key"] != key);
            actual["attributes"]
                .as_array_mut()
                .unwrap()
                .push(json!({"key":key,"value":value}));
            assert!(!parse_event(&actual, NOW).unwrap().prewarm, "{key}");
        }
    }

    #[test]
    fn prewarm_is_retained_but_excluded_from_usage_lists_and_rollups() -> Result<(), AppError> {
        let db = Database::memory()?;
        configure_prices(&db);
        let temp = tempfile::tempdir().unwrap();
        let state = state_db(temp.path());
        let mut prewarm = parse_event(&prewarm_record(), NOW).unwrap();
        let mut actual = parse_event(&record(), NOW).unwrap();
        let created_at = Utc::now().timestamp() - 40 * 86400;
        prewarm.created_at = created_at;
        actual.created_at = created_at + 5;
        enqueue(&db, &[prewarm.clone(), actual], created_at)?;
        assert_eq!(sync_pending_at(&db, &state)?.imported, 2);
        enqueue(&db, &[prewarm], created_at)?;
        assert_eq!(sync_pending_at(&db, &state)?.imported, 0);
        {
            let conn = lock_conn!(db.conn);
            let stored: (i64, i64, String) = conn.query_row(
                "SELECT input_tokens,output_tokens,total_cost_usd FROM proxy_request_logs WHERE data_source='codex_otel_prewarm'",
                [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            assert_eq!(stored, (1200, 0, "0.0024".into()));
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r
                    .get::<_, i64>(0))?,
                2
            );
        }
        assert_eq!(db.get_request_logs(&Default::default(), 0, 10)?.total, 1);
        let summary = db.get_usage_summary(None, None, Some("codex"), None, None)?;
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.real_total_tokens, 1220);
        assert_eq!(
            Decimal::from_str(&summary.total_cost).unwrap(),
            Decimal::new(16, 4)
        );
        let sources = crate::services::session_usage::get_data_source_breakdown(&db)?;
        assert!(!sources
            .iter()
            .any(|s| s.data_source == "codex_otel_prewarm"));
        db.rollup_and_prune(30)?;
        let summary = db.get_usage_summary(None, None, Some("codex"), None, None)?;
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.real_total_tokens, 1220);
        assert_eq!(
            Decimal::from_str(&summary.total_cost).unwrap(),
            Decimal::new(16, 4)
        );
        Ok(())
    }

    #[test]
    fn ephemeral_usage_is_priced_once_and_replay_cannot_restore_pruned_rows() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        configure_prices(&db);
        let temp = tempfile::tempdir().unwrap();
        let state = state_db(temp.path());
        let event = parse_event(&record(), NOW).unwrap();
        enqueue(&db, &[event.clone(), event.clone()], NOW)?;
        assert_eq!(sync_pending_at(&db, &state)?.imported, 1);
        enqueue(&db, &[event.clone()], NOW)?;
        assert_eq!(sync_pending_at(&db, &state)?.imported, 0);
        let summary = db.get_usage_summary(None, None, Some("codex"), None, None)?;
        assert_eq!(summary.total_requests, 1);
        let conn = lock_conn!(db.conn);
        let row: (String,i64,i64,i64,String) = conn.query_row("SELECT total_cost_usd,input_tokens,output_tokens,cache_read_tokens,service_tier_source FROM proxy_request_logs", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
        assert_eq!(Decimal::from_str(&row.0).unwrap(), Decimal::new(16, 4));
        assert_eq!(
            (row.1, row.2, row.3, row.4.as_str()),
            (1200, 20, 1000, "request")
        );
        conn.execute("DELETE FROM proxy_request_logs", [])?;
        drop(conn);
        enqueue(&db, &[event.clone()], NOW)?;
        assert_eq!(sync_pending_at(&db, &state)?.imported, 0);
        // Same token counts in another actual response remain billable.
        let mut next = event;
        next.observed_ns += 1;
        enqueue(&db, &[next], NOW)?;
        assert_eq!(sync_pending_at(&db, &state)?.imported, 1);
        Ok(())
    }

    #[test]
    fn persisted_sessions_are_excluded_before_the_jsonl_import() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempfile::tempdir().unwrap();
        let state = state_db(temp.path());
        Connection::open(&state)?.execute("INSERT INTO threads(id) VALUES (?1)", [THREAD])?;
        enqueue(&db, &[parse_event(&record(), NOW).unwrap()], NOW)?;
        let result = sync_pending_at(&db, &state)?;
        assert_eq!((result.imported, result.skipped), (0, 1));
        assert_eq!(sync_pending_at(&db, &state)?.imported, 0);
        Ok(())
    }

    #[test]
    fn unavailable_state_and_failed_insert_leave_pending_usage_for_retry() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state_5.sqlite");
        enqueue(&db, &[parse_event(&record(), NOW).unwrap()], NOW)?;
        assert!(sync_pending_at(&db, &state).is_err());
        assert!(!state.exists()); // read-only open cannot create a fake empty state DB
        state_db(temp.path());
        {
            let conn = lock_conn!(db.conn);
            conn.execute_batch("CREATE TRIGGER fail_otel BEFORE INSERT ON proxy_request_logs BEGIN SELECT RAISE(ABORT,'simulated write failure'); END;")?;
        }
        assert!(sync_pending_at(&db, &state).is_err());
        {
            let conn = lock_conn!(db.conn);
            let pending: i64 = conn.query_row(
                "SELECT count(*) FROM codex_otel_ingest WHERE event_json IS NOT NULL",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(pending, 1);
            conn.execute_batch("DROP TRIGGER fail_otel")?;
        }
        assert_eq!(sync_pending_at(&db, &state)?.imported, 1);
        Ok(())
    }

    #[test]
    fn late_proxy_and_late_session_rows_are_excluded_from_effective_stats() -> Result<(), AppError>
    {
        for source in ["proxy", "codex_session"] {
            let db = Database::memory()?;
            let temp = tempfile::tempdir().unwrap();
            let state = state_db(temp.path());
            let mut event = parse_event(&record(), NOW).unwrap();
            let created_at = Utc::now().timestamp() - 40 * 86400;
            event.created_at = created_at;
            enqueue(&db, &[event], created_at)?;
            assert_eq!(sync_pending_at(&db, &state)?.imported, 1);
            {
                let conn = lock_conn!(db.conn);
                conn.execute("INSERT INTO proxy_request_logs (request_id,provider_id,app_type,model,input_tokens,output_tokens,cache_read_tokens,input_token_semantics,total_cost_usd,latency_ms,status_code,session_id,created_at,data_source) VALUES ('other','p','codex','gpt-5.4',1200,20,1000,1,'1',0,200,?1,?2,?3)", params![THREAD,created_at,source])?;
            }
            let summary = db.get_usage_summary(None, None, Some("codex"), None, None)?;
            assert_eq!(summary.total_requests, 1, "{source}");
            assert_eq!(
                Decimal::from_str(&summary.total_cost).unwrap(),
                Decimal::ONE,
                "{source}"
            );
            assert_eq!(db.rollup_and_prune(30)?, 2);
            let rolled_up = db.get_usage_summary(None, None, Some("codex"), None, None)?;
            assert_eq!(rolled_up.total_requests, 1, "rollup {source}");
            assert_eq!(
                Decimal::from_str(&rolled_up.total_cost).unwrap(),
                Decimal::ONE,
                "rollup {source}"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn http_receiver_rejects_browser_origin_invalid_and_oversized_payloads() {
        let db = Arc::new(Database::memory().unwrap());
        let temp = tempfile::tempdir().unwrap();
        let state = state_db(temp.path());
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router(db, state)).await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("http://{address}{LOGS_PATH}");
        assert_eq!(
            client
                .post(&url)
                .json(&payload(vec![]))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            client
                .post(&url)
                .header("origin", "https://example.test")
                .json(&payload(vec![]))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            client
                .post(&url)
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            client
                .post(&url)
                .header("content-type", "application/x-protobuf")
                .body("fake")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(
            client
                .post(&url)
                .header("content-type", "application/json")
                .body(" ".repeat(4 * 1024 * 1024 + 1))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        task.abort();
    }

    /// Opt-in integration check against an installed Codex executable. Both the
    /// model API and telemetry destination are local test servers; no real key.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires CC_SWITCH_CODEX_OTEL_PROBE_BIN; uses only a local mock model"]
    async fn real_ephemeral_codex_exports_into_collector() {
        use std::process::{Command, Stdio};
        let executable =
            std::env::var("CC_SWITCH_CODEX_OTEL_PROBE_BIN").expect("set probe executable");
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("codex-home");
        std::fs::create_dir(&home).unwrap();
        let db = Arc::new(Database::memory().unwrap());
        configure_prices(&db);
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let app = router(db.clone(),home.join("state_5.sqlite")).route("/v1/responses",post(|| async {
            let item = json!({"id":"msg_ccs_probe","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"OK","annotations":[]}]});
            let response = json!({"id":"resp_ccs_probe","object":"response","status":"completed","model":"gpt-5.4","output":[item.clone()],
                "usage":{"input_tokens":1200,"input_tokens_details":{"cached_tokens":1000},"output_tokens":20,"output_tokens_details":{"reasoning_tokens":10},"total_tokens":1220}});
            let frames = [json!({"type":"response.created","response":{"id":"resp_ccs_probe","object":"response","status":"in_progress","output":[]}}),
                json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_ccs_probe","type":"message","role":"assistant","status":"in_progress","content":[]}}),
                json!({"type":"response.output_text.delta","item_id":"msg_ccs_probe","output_index":0,"content_index":0,"delta":"OK"}),
                json!({"type":"response.output_item.done","output_index":0,"item":item}),
                json!({"type":"response.completed","response":response})];
            let body = frames.iter().map(|f| format!("data: {f}\n\n")).collect::<String>();
            ([("content-type","text/event-stream")],body)
        }));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        std::fs::write(
            home.join("config.toml"),
            format!(
                r#"model = "gpt-5.4"
model_provider = "mock"
model_reasoning_effort = "low"
service_tier = "fast"
[model_providers.mock]
name = "CCS local mock"
base_url = "http://{address}/v1"
env_key = "CCS_OTEL_PROBE_KEY"
wire_api = "responses"
supports_websockets = false
[analytics]
enabled = false
[otel]
log_user_prompt = false
metrics_exporter = "none"
trace_exporter = "none"
exporter = {{ otlp-http = {{ endpoint = "http://{address}{LOGS_PATH}", protocol = "json" }} }}
"#
            ),
        )
        .unwrap();
        let stdout = std::fs::File::create(temp.path().join("stdout.txt")).unwrap();
        let stderr = std::fs::File::create(temp.path().join("stderr.txt")).unwrap();
        let mut command = Command::new(executable);
        command
            .args([
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "--json",
                "Reply OK.",
            ])
            .current_dir(temp.path())
            .env("CODEX_HOME", &home)
            .env("CCS_OTEL_PROBE_KEY", "synthetic-local-key")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("OTEL_") {
                command.env_remove(key);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().ok();
                child.wait().ok();
                panic!("local Codex probe timed out");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert!(
            status.success(),
            "probe failed: {}",
            std::fs::read_to_string(temp.path().join("stderr.txt")).unwrap()
        );
        // The exporter flushes on process exit. The handler may still be finishing.
        for _ in 0..20 {
            if db
                .get_usage_summary(None, None, Some("codex"), None, None)
                .unwrap()
                .total_requests
                == 1
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let summary = db
            .get_usage_summary(None, None, Some("codex"), None, None)
            .unwrap();
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.real_total_tokens, 1220);
        assert_eq!(
            Decimal::from_str(&summary.total_cost).unwrap(),
            Decimal::new(16, 4)
        );
        let state = Connection::open_with_flags(
            home.join("state_5.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            state
                .query_row("SELECT count(*) FROM threads", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        server.abort();
    }
}
