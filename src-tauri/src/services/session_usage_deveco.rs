//! Import DevEco Code's committed token usage without modifying its database.
//!
//! DevEco Code stores per-request usage inside the assistant rows of its `message`
//! table (`data.tokens` / `data.cost`). The table has a stable rowid, which this
//! module uses as the incremental watermark; `request_id` is keyed on the message
//! id so a re-scan of the same row can never double-count.
//!
//! `tokens.input` excludes cache reads (verified against native rows), so the
//! imported rows carry [`INPUT_TOKEN_SEMANTICS_FRESH`], matching OpenCode.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::{session_usage::SessionSyncResult, usage_stats::find_model_pricing};
use crate::session_manager::providers::deveco;
use rusqlite::params;
use rust_decimal::Decimal;
use serde_json::Value;

pub fn sync_deveco_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let db_path = deveco::database_path();
    if !db_path.exists() {
        return Ok(SessionSyncResult::default());
    }
    let source = deveco::open_database()?;
    let key = format!("deveco:{}", db_path.display());
    sync_from_database(db, &source, &key)
}

/// 从 assistant 消息的 `data` 取用量的中间表示。
#[derive(Debug, Default, PartialEq)]
struct MessageUsage {
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    cost: Option<f64>,
    created_at_ms: i64,
}

fn parse_usage(data: &str) -> Option<MessageUsage> {
    let value: Value = serde_json::from_str(data).ok()?;
    if value.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }

    let tokens = value.get("tokens")?;
    let num = |value: Option<&Value>| -> u32 {
        value
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32
    };

    Some(MessageUsage {
        model: value
            .get("modelID")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        input_tokens: num(tokens.get("input")),
        // DevEco reports reasoning separately from visible output tokens.
        output_tokens: num(tokens.get("output")).saturating_add(num(tokens.get("reasoning"))),
        cache_read_tokens: num(tokens.pointer("/cache/read")),
        cache_creation_tokens: num(tokens.pointer("/cache/write")),
        cost: value.get("cost").and_then(Value::as_f64),
        created_at_ms: value
            .pointer("/time/created")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

fn sync_from_database(
    db: &Database,
    source: &rusqlite::Connection,
    key: &str,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let mut cursor = tx.query_row(
        "SELECT COALESCE(MAX(last_line_offset), 0) FROM session_log_sync WHERE file_path = ?1",
        [&key],
        |row| row.get::<_, i64>(0),
    )?;

    let mut query = source.prepare(
        "SELECT rowid, session_id, id, data FROM message WHERE rowid > ?1 ORDER BY rowid",
    )?;
    let mut rows = query.query([cursor])?;
    result.files_scanned = 1;

    while let Some(row) = rows.next()? {
        let row_id: i64 = row.get(0)?;
        let session_id: String = row.get(1)?;
        let message_id: String = row.get(2)?;
        let data: String = row.get(3)?;

        // The cursor advances even for rows we don't import (user messages, malformed
        // JSON): the watermark tracks how far we've read, not how much we kept.
        cursor = row_id;

        let Some(usage) = parse_usage(&data) else {
            continue;
        };

        let token_usage = TokenUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_creation_tokens: usage.cache_creation_tokens,
            model: Some(usage.model.clone()),
            message_id: None,
        };

        let cost = match usage.cost {
            Some(cost) if cost.is_finite() && cost >= 0.0 => cost.to_string(),
            _ => find_model_pricing(&tx, &usage.model)
                .map(|pricing| {
                    CostCalculator::calculate_for_app(
                        "deveco",
                        &token_usage,
                        &pricing,
                        Decimal::ONE,
                    )
                    .total_cost
                    .to_string()
                })
                .unwrap_or_else(|| "0".into()),
        };

        let request_id = format!("deveco:{session_id}:{message_id}");
        let changed = tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, status_code, session_id, provider_type,
                is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
             ) VALUES (?1, '_deveco_session', 'deveco', ?2, ?2, ?3, ?4, ?5, ?6,
                       '0', '0', '0', '0', ?7, 0, 200, ?8, 'deveco_session', 1, '1', ?9, 'deveco_session', ?10)",
            params![
                request_id,
                usage.model,
                token_usage.input_tokens,
                token_usage.output_tokens,
                token_usage.cache_read_tokens,
                token_usage.cache_creation_tokens,
                cost,
                session_id,
                usage.created_at_ms / 1000,
                INPUT_TOKEN_SEMANTICS_FRESH
            ],
        )?;
        result.imported += changed as u32;
        result.skipped += u32::from(changed == 0);
    }

    // Commit the watermark with the imported rows; pruned logs must never be reimported.
    tx.execute(
        "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, 0, ?2, unixepoch()) ON CONFLICT(file_path) DO UPDATE SET
         last_line_offset = excluded.last_line_offset, last_synced_at = excluded.last_synced_at",
        params![key, cursor],
    )?;
    tx.commit()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_with_messages() -> rusqlite::Connection {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                "CREATE TABLE message (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL,
                    time_updated INTEGER NOT NULL,
                    data TEXT NOT NULL
                );",
            )
            .unwrap();
        source
    }

    fn insert_message(source: &rusqlite::Connection, id: &str, session: &str, data: &str) {
        source
            .execute(
                "INSERT INTO message (id, session_id, time_created, time_updated, data)
                 VALUES (?1, ?2, 0, 0, ?3)",
                rusqlite::params![id, session, data],
            )
            .unwrap();
    }

    #[test]
    fn parse_usage_sums_output_and_reasoning_and_requires_assistant() {
        let parsed = parse_usage(
            r#"{"role":"assistant","modelID":"glm-5.3-flash",
                "cost":0.5,
                "tokens":{"total":12467,"input":12314,"output":32,"reasoning":121,
                          "cache":{"write":7,"read":12288}},
                "time":{"created":1788502434605}}"#,
        )
        .expect("assistant row parses");

        assert_eq!(parsed.model, "glm-5.3-flash");
        assert_eq!(parsed.input_tokens, 12314);
        assert_eq!(parsed.output_tokens, 153, "output + reasoning");
        assert_eq!(parsed.cache_read_tokens, 12288);
        assert_eq!(parsed.cache_creation_tokens, 7);
        assert_eq!(parsed.cost, Some(0.5));
        assert_eq!(parsed.created_at_ms, 1788502434605);

        assert_eq!(parse_usage(r#"{"role":"user"}"#), None, "用户消息不计用量");
        assert_eq!(parse_usage("not json"), None, "坏 JSON 不得 panic");
        assert_eq!(
            parse_usage(r#"{"role":"assistant"}"#),
            None,
            "缺少 tokens 的 assistant 行跳过"
        );
    }

    #[test]
    fn sync_imports_assistant_rows_once_and_advances_the_watermark() {
        let source = source_with_messages();
        insert_message(&source, "msg_1", "s1", r#"{"role":"user"}"#);
        insert_message(
            &source,
            "msg_2",
            "s1",
            r#"{"role":"assistant","modelID":"test/model","cost":0.25,
                "tokens":{"input":10,"output":20,"reasoning":3,
                          "cache":{"write":5,"read":40}},
                "time":{"created":100000}}"#,
        );
        let db = Database::memory().unwrap();

        assert_eq!(
            sync_from_database(&db, &source, "deveco-test")
                .unwrap()
                .imported,
            1,
            "仅 assistant 行入库"
        );

        {
            let conn = db.conn.lock().unwrap();
            let values = conn
                .query_row(
                    "SELECT input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,
                            total_cost_usd, created_at, model
                     FROM proxy_request_logs WHERE app_type='deveco'",
                    [],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, i64>(5)?,
                            r.get::<_, String>(6)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(
                values,
                (10, 23, 40, 5, "0.25".into(), 100, "test/model".into())
            );

            let semantics: i64 = conn
                .query_row(
                    "SELECT input_token_semantics FROM proxy_request_logs WHERE app_type='deveco'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(semantics, INPUT_TOKEN_SEMANTICS_FRESH);
        }

        // 水位已推进：原样重跑不重复导入
        assert_eq!(
            sync_from_database(&db, &source, "deveco-test")
                .unwrap()
                .imported,
            0
        );

        // 追加新行后继续增量导入
        insert_message(
            &source,
            "msg_3",
            "s1",
            r#"{"role":"assistant","modelID":"test/model","cost":0.5,
                "tokens":{"input":1,"output":2,"cache":{"write":0,"read":0}},
                "time":{"created":101000}}"#,
        );
        assert_eq!(
            sync_from_database(&db, &source, "deveco-test")
                .unwrap()
                .imported,
            1
        );
    }

    #[test]
    fn sync_falls_back_to_pricing_when_native_cost_is_missing() {
        let source = source_with_messages();
        // 无 cost 字段：应回退到定价表，且不得报错（未知模型则记 0）
        insert_message(
            &source,
            "msg_1",
            "s1",
            r#"{"role":"assistant","modelID":"unknown-model",
                "tokens":{"input":10,"output":20,"cache":{"write":0,"read":0}},
                "time":{"created":100000}}"#,
        );
        let db = Database::memory().unwrap();

        assert_eq!(
            sync_from_database(&db, &source, "deveco-nocost")
                .unwrap()
                .imported,
            1
        );

        let conn = db.conn.lock().unwrap();
        let cost: String = conn
            .query_row(
                "SELECT total_cost_usd FROM proxy_request_logs WHERE app_type='deveco'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cost, "0", "未知模型回退为 0 而非报错");
    }
}
