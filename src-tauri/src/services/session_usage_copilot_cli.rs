//! GitHub Copilot CLI session usage importer.
//!
//! Copilot CLI writes cumulative per-model metrics into `session.shutdown`
//! events. The latest shutdown snapshot replaces older snapshots for the same
//! `(session, model)` identity, so resumed sessions remain idempotent.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::SessionSyncResult;
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::find_model_pricing;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const APP_TYPE: &str = "copilot-cli";
const DATA_SOURCE: &str = "copilot_cli_session";
const PROVIDER_ID: &str = "_copilot_cli_session";
const PROVIDER_NAME: &str = "Copilot CLI (Session)";
const MAX_SESSION_BYTES: u64 = 128 * 1024 * 1024;
const MAX_EVENTS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelMetric {
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

#[derive(Debug)]
struct SessionSnapshot {
    session_id: String,
    created_at: i64,
    metrics: Vec<ModelMetric>,
}

pub fn sync_copilot_cli_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_files(
        db,
        &crate::session_manager::providers::copilot_cli::session_files(),
    )
}

fn sync_files(db: &Database, files: &[std::path::PathBuf]) -> Result<SessionSyncResult, AppError> {
    sync_provider(db)?;
    let mut result = SessionSyncResult {
        files_scanned: files.len().min(u32::MAX as usize) as u32,
        ..Default::default()
    };
    result.imported = result
        .imported
        .saturating_add(migrate_legacy_unpriced_rows(db)?);

    for path in files {
        match parse_snapshot(path) {
            Ok(Some(snapshot)) => {
                for metric in &snapshot.metrics {
                    match upsert_metric(db, &snapshot, metric) {
                        Ok(true) => result.imported = result.imported.saturating_add(1),
                        Ok(false) => result.skipped = result.skipped.saturating_add(1),
                        Err(error) => result.errors.push(format!("{}: {error}", path.display())),
                    }
                }
            }
            Ok(None) => result.skipped = result.skipped.saturating_add(1),
            Err(error) => result.errors.push(format!("{}: {error}", path.display())),
        }
    }
    Ok(result)
}

fn sync_provider(db: &Database) -> Result<(), AppError> {
    let settings = json!({
        "source": DATA_SOURCE,
        "costAttribution": "model"
    });
    if db
        .get_provider_by_id(PROVIDER_ID, APP_TYPE)?
        .is_some_and(|provider| {
            provider.name == PROVIDER_NAME
                && provider.settings_config == settings
                && provider.icon.as_deref() == Some("githubcopilot")
        })
    {
        return Ok(());
    }
    let mut provider = Provider::with_id(
        PROVIDER_ID.to_string(),
        PROVIDER_NAME.to_string(),
        settings,
        Some("https://github.com/features/copilot".to_string()),
    );
    provider.category = Some("Copilot CLI".to_string());
    provider.icon = Some("githubcopilot".to_string());
    db.save_provider(APP_TYPE, &provider)
}

/// Earlier Copilot CLI imports explicitly used a zero multiplier because the
/// session log cannot identify the provider. Model-based pricing does not need
/// that provider identity: the recorded model is the pricing key. Normalize
/// existing rows before the generic missing-cost backfill runs so historical
/// sessions gain prices too, even if their JSONL file is no longer present.
fn migrate_legacy_unpriced_rows(db: &Database) -> Result<u32, AppError> {
    let changed = {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "UPDATE proxy_request_logs
             SET cost_multiplier = '1.0', input_token_semantics = ?1
             WHERE data_source = ?2
               AND (cost_multiplier != '1.0' OR input_token_semantics != ?1)",
            rusqlite::params![INPUT_TOKEN_SEMANTICS_TOTAL, DATA_SOURCE],
        )
        .map_err(|error| {
            AppError::Database(format!("迁移 Copilot CLI 模型计价记录失败: {error}"))
        })?
    };

    if changed > 0 {
        if let Err(error) = db.backfill_missing_usage_costs() {
            log::warn!("Copilot CLI 历史模型费用回填失败，将在后续价格同步时重试: {error}");
        }
    }

    Ok(changed.min(u32::MAX as usize) as u32)
}

fn parse_snapshot(path: &Path) -> Result<Option<SessionSnapshot>, AppError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::InvalidInput(format!(
            "Copilot CLI session is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_SESSION_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Copilot CLI session exceeds {} MiB",
            MAX_SESSION_BYTES / 1024 / 1024
        )));
    }

    let file = File::open(path).map_err(|error| AppError::io(path, error))?;
    let mut session_id = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut created_at = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_else(now_epoch);
    let mut latest_metrics = None;

    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut index = 0usize;
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|error| AppError::io(path, error))?;
        if bytes == 0 {
            break;
        }
        if index >= MAX_EVENTS {
            return Err(AppError::InvalidInput(format!(
                "Copilot CLI session exceeds {MAX_EVENTS} events"
            )));
        }
        index += 1;
        if line.trim().is_empty() {
            continue;
        }
        // An active CLI process can leave an incomplete final JSONL line.
        let terminated = line.ends_with('\n');
        let event = match serde_json::from_str::<Value>(&line) {
            Ok(event) => event,
            Err(_) if !terminated => break,
            Err(error) => {
                return Err(AppError::Config(format!(
                    "Failed to parse Copilot CLI usage session {} at line {index}: {error}",
                    path.display()
                )))
            }
        };
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let data = event.get("data").unwrap_or(&event);
        if event_type == "session.start" {
            if let Some(value) = data
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                session_id = value.to_string();
            }
            if let Some(value) = data.get("startTime").and_then(timestamp_seconds) {
                created_at = value;
            }
        } else if event_type == "session.shutdown" {
            if let Some(value) = data.get("sessionStartTime").and_then(timestamp_seconds) {
                created_at = value;
            }
            if let Some(metrics) = data.get("modelMetrics").and_then(parse_model_metrics) {
                latest_metrics = Some(metrics);
            }
        }
    }

    Ok(latest_metrics.map(|metrics| SessionSnapshot {
        session_id,
        created_at,
        metrics,
    }))
}

fn parse_model_metrics(value: &Value) -> Option<Vec<ModelMetric>> {
    let object = value.as_object()?;
    let mut metrics = Vec::new();
    for (model, metric) in object {
        let usage = metric.get("usage").unwrap_or(metric);
        let parsed = ModelMetric {
            model: model.to_string(),
            input_tokens: nonnegative_count(usage.get("inputTokens")),
            output_tokens: nonnegative_count(usage.get("outputTokens")),
            cache_read_tokens: nonnegative_count(usage.get("cacheReadTokens")),
            cache_write_tokens: nonnegative_count(usage.get("cacheWriteTokens")),
        };
        if parsed.input_tokens > 0
            || parsed.output_tokens > 0
            || parsed.cache_read_tokens > 0
            || parsed.cache_write_tokens > 0
        {
            metrics.push(parsed);
        }
    }
    Some(metrics)
}

fn nonnegative_count(value: Option<&Value>) -> i64 {
    value
        .and_then(|value| {
            value.as_i64().or_else(|| {
                value
                    .as_u64()
                    .map(|value| value.min(i64::MAX as u64) as i64)
            })
        })
        .unwrap_or(0)
        .max(0)
}

fn request_id(session_id: &str, model: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"copilot-cli-session-v1\0");
    hasher.update(session_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(model.as_bytes());
    format!("copilot_cli_session:{:x}", hasher.finalize())
}

fn upsert_metric(
    db: &Database,
    snapshot: &SessionSnapshot,
    metric: &ModelMetric,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    // The session event cannot identify the provider, but it does carry the
    // actual model. Price that model against CC Switch's model-pricing table;
    // this is an API-equivalent estimate rather than a GitHub subscription
    // charge. A missing price stays at zero with multiplier 1 so adding the
    // model price later can backfill the row.
    let usage = TokenUsage {
        input_tokens: metric.input_tokens.min(u32::MAX as i64) as u32,
        output_tokens: metric.output_tokens.min(u32::MAX as i64) as u32,
        cache_read_tokens: metric.cache_read_tokens.min(u32::MAX as i64) as u32,
        cache_creation_tokens: metric.cache_write_tokens.min(u32::MAX as i64) as u32,
        model: Some(metric.model.clone()),
        message_id: None,
    };
    let costs = find_model_pricing(&conn, &metric.model)
        .map(|pricing| CostCalculator::calculate_for_app(APP_TYPE, &usage, &pricing, Decimal::ONE));
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = costs
        .map_or_else(
            || {
                (
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                )
            },
            |cost| {
                (
                    cost.input_cost.to_string(),
                    cost.output_cost.to_string(),
                    cost.cache_read_cost.to_string(),
                    cost.cache_creation_cost.to_string(),
                    cost.total_cost.to_string(),
                )
            },
        );
    let request_id = request_id(&snapshot.session_id, &metric.model);

    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_token_semantics,
            input_cost_usd, output_cost_usd, cache_read_cost_usd,
            cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
        )
        ON CONFLICT(request_id) DO UPDATE SET
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd,
            cost_multiplier = excluded.cost_multiplier,
            input_token_semantics = excluded.input_token_semantics
        WHERE data_source = 'copilot_cli_session'
          AND (input_tokens != excluded.input_tokens
            OR output_tokens != excluded.output_tokens
            OR cache_read_tokens != excluded.cache_read_tokens
            OR cache_creation_tokens != excluded.cache_creation_tokens
            OR input_cost_usd != excluded.input_cost_usd
            OR output_cost_usd != excluded.output_cost_usd
            OR cache_read_cost_usd != excluded.cache_read_cost_usd
            OR cache_creation_cost_usd != excluded.cache_creation_cost_usd
            OR total_cost_usd != excluded.total_cost_usd
            OR cost_multiplier != excluded.cost_multiplier
            OR input_token_semantics != excluded.input_token_semantics)",
        rusqlite::params![
            request_id,
            PROVIDER_ID,
            APP_TYPE,
            metric.model,
            metric.model,
            metric.model,
            metric.input_tokens,
            metric.output_tokens,
            metric.cache_read_tokens,
            metric.cache_write_tokens,
            INPUT_TOKEN_SEMANTICS_TOTAL,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            0i64,
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            snapshot.session_id,
            Some(DATA_SOURCE),
            1i64,
            "1.0",
            snapshot.created_at,
            DATA_SOURCE,
        ],
    )
    .map_err(|error| AppError::Database(format!("插入 Copilot CLI 会话用量失败: {error}")))?;
    Ok(conn.changes() > 0)
}

fn timestamp_seconds(value: &Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return Some(if value.unsigned_abs() > 100_000_000_000 {
            value / 1000
        } else {
            value
        });
    }
    value
        .as_str()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp())
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_shutdown_snapshot_replaces_older_cumulative_metrics() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("events.jsonl");
        fs::write(
            &path,
            concat!(
                r#"{"type":"session.start","data":{"sessionId":"session-1","startTime":"2026-08-18T01:00:00Z"}}"#,
                "\n",
                r#"{"type":"session.shutdown","data":{"modelMetrics":{"gpt-test":{"usage":{"inputTokens":10,"outputTokens":2,"cacheReadTokens":1,"cacheWriteTokens":0}}}}}"#,
                "\n",
                r#"{"type":"session.shutdown","data":{"modelMetrics":{"gpt-test":{"usage":{"inputTokens":25,"outputTokens":7,"cacheReadTokens":3,"cacheWriteTokens":1}}}}}"#,
                "\n",
            ),
        )
        .expect("session file");

        let snapshot = parse_snapshot(&path)
            .expect("parse snapshot")
            .expect("shutdown");
        assert_eq!(snapshot.session_id, "session-1");
        assert_eq!(snapshot.metrics.len(), 1);
        assert_eq!(snapshot.metrics[0].input_tokens, 25);
        assert_eq!(snapshot.metrics[0].cache_write_tokens, 1);
    }

    #[test]
    fn cumulative_session_usage_upserts_one_stable_row() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempfile::tempdir().expect("temp directory");
        let session_dir = temp.path().join("session-1");
        fs::create_dir_all(&session_dir).expect("session directory");
        let path = session_dir.join("events.jsonl");
        let write_snapshot = |input_tokens: i64| {
            fs::write(
                &path,
                format!(
                    "{}\n{}\n",
                    r#"{"type":"session.start","data":{"sessionId":"session-1","startTime":"2026-08-18T01:00:00Z"}}"#,
                    json!({
                        "type": "session.shutdown",
                        "data": {
                            "modelMetrics": {
                                "gpt-test": {
                                    "usage": {
                                        "inputTokens": input_tokens,
                                        "outputTokens": 2
                                    }
                                }
                            }
                        }
                    })
                ),
            )
            .expect("session snapshot");
        };

        sync_provider(&db)?;
        write_snapshot(10);
        let first = parse_snapshot(&path)?.expect("first snapshot");
        assert!(upsert_metric(&db, &first, &first.metrics[0])?);
        write_snapshot(25);
        let second = parse_snapshot(&path)?.expect("second snapshot");
        assert!(upsert_metric(&db, &second, &second.metrics[0])?);
        assert!(!upsert_metric(&db, &second, &second.metrics[0])?);

        let conn = lock_conn!(db.conn);
        let (rows, input_tokens): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), MAX(input_tokens) FROM proxy_request_logs WHERE data_source = ?1",
                [DATA_SOURCE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        assert_eq!(rows, 1);
        assert_eq!(input_tokens, 25);
        Ok(())
    }

    #[test]
    fn session_usage_is_priced_by_recorded_model_with_total_input_semantics() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        sync_provider(&db)?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE providers SET name = 'Copilot CLI'
                 WHERE id = ?1 AND app_type = ?2",
                rusqlite::params![PROVIDER_ID, APP_TYPE],
            )?;
        }
        sync_provider(&db)?;
        let snapshot = SessionSnapshot {
            session_id: "priced-session".to_string(),
            created_at: 1_786_123_456,
            metrics: vec![ModelMetric {
                model: "MiniMax-M3".to_string(),
                input_tokens: 1_000_000,
                output_tokens: 100_000,
                cache_read_tokens: 200_000,
                cache_write_tokens: 100_000,
            }],
        };

        assert!(upsert_metric(&db, &snapshot, &snapshot.metrics[0])?);
        let conn = lock_conn!(db.conn);
        let row: (String, String, String, String, String, String, i64) = conn
            .query_row(
                "SELECT input_cost_usd, output_cost_usd, cache_read_cost_usd,
                        cache_creation_cost_usd, total_cost_usd, cost_multiplier,
                        input_token_semantics
                 FROM proxy_request_logs WHERE request_id = ?1",
                [request_id("priced-session", "MiniMax-M3")],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .map_err(|error| AppError::Database(error.to_string()))?;

        // MiniMax M3: $0.30 input / $1.20 output / $0.06 cache-read per M.
        // Fresh input is 1M - 200k read - 100k write = 700k.
        assert_eq!(row.0.parse::<Decimal>().unwrap(), Decimal::new(21, 2));
        assert_eq!(row.1.parse::<Decimal>().unwrap(), Decimal::new(12, 2));
        assert_eq!(row.2.parse::<Decimal>().unwrap(), Decimal::new(12, 3));
        assert_eq!(row.3.parse::<Decimal>().unwrap(), Decimal::ZERO);
        assert_eq!(row.4.parse::<Decimal>().unwrap(), Decimal::new(342, 3));
        assert_eq!(row.5, "1.0");
        assert_eq!(row.6, INPUT_TOKEN_SEMANTICS_TOTAL);
        let provider_name: String = conn
            .query_row(
                "SELECT name FROM providers WHERE id = ?1 AND app_type = ?2",
                rusqlite::params![PROVIDER_ID, APP_TYPE],
                |provider_row| provider_row.get(0),
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        assert_eq!(provider_name, PROVIDER_NAME);
        Ok(())
    }

    #[test]
    fn legacy_zero_multiplier_rows_are_repriced_by_model() -> Result<(), AppError> {
        let db = Database::memory()?;
        let snapshot = SessionSnapshot {
            session_id: "legacy-session".to_string(),
            created_at: 1_786_123_456,
            metrics: vec![ModelMetric {
                model: "MiniMax-M3".to_string(),
                input_tokens: 1_000_000,
                output_tokens: 100_000,
                cache_read_tokens: 200_000,
                cache_write_tokens: 100_000,
            }],
        };
        upsert_metric(&db, &snapshot, &snapshot.metrics[0])?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE proxy_request_logs
                 SET cost_multiplier = '0', input_token_semantics = 2,
                     input_cost_usd = '0', output_cost_usd = '0',
                     cache_read_cost_usd = '0', cache_creation_cost_usd = '0',
                     total_cost_usd = '0'
                 WHERE data_source = ?1",
                [DATA_SOURCE],
            )?;
        }

        assert_eq!(migrate_legacy_unpriced_rows(&db)?, 1);
        let conn = lock_conn!(db.conn);
        let (multiplier, semantics, total): (String, i64, String) = conn.query_row(
            "SELECT cost_multiplier, input_token_semantics, total_cost_usd
             FROM proxy_request_logs WHERE data_source = ?1",
            [DATA_SOURCE],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(multiplier, "1.0");
        assert_eq!(semantics, INPUT_TOKEN_SEMANTICS_TOTAL);
        assert_eq!(total.parse::<Decimal>().unwrap(), Decimal::new(342, 3));
        Ok(())
    }

    #[test]
    fn malformed_complete_event_is_reported_but_incomplete_final_event_is_tolerated() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("events.jsonl");
        fs::write(&path, "{not-json}\n").expect("malformed session");
        assert!(parse_snapshot(&path).is_err());

        fs::write(
            &path,
            concat!(
                r#"{"type":"session.shutdown","data":{"modelMetrics":{"gpt-test":{"usage":{"inputTokens":1}}}}}"#,
                "\n",
                r#"{"type":"assistant.message""#,
            ),
        )
        .expect("active session");
        let snapshot = parse_snapshot(&path)
            .expect("incomplete final event is transient")
            .expect("complete shutdown remains usable");
        assert_eq!(snapshot.metrics[0].input_tokens, 1);
    }
}
