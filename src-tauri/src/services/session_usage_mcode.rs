//! Import MCode's committed token-usage projection without modifying its database.
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::{session_usage::SessionSyncResult, usage_stats::find_model_pricing};
use crate::session_manager::providers::mcode;
use rusqlite::{params, OptionalExtension};
use rust_decimal::Decimal;
use std::collections::HashMap;

pub fn sync_mcode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    if !mcode::database_path().exists() {
        return Ok(SessionSyncResult::default());
    }
    let source = mcode::open_database()?;
    let key = format!("mcode:{}", mcode::database_path().display());
    sync_from_database(db, &source, &key)
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
        "SELECT id, session_id, model, ts, input_tokens,
                output_tokens, reasoning_tokens, cache_read_tokens, cache_write_tokens, cost_usd
         FROM local_runtime_token_usage WHERE id > ?1 ORDER BY id",
    )?;
    let mut history = HistoryModels::prepare(source);
    let mut rows = query.query([cursor])?;
    result.files_scanned = 1;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let session_id: String = row.get(1)?;
        let ts: i64 = row.get(3)?;
        let native_model: Option<String> = row.get(2)?;
        let model = match native_model.as_deref() {
            Some(native) => native
                .split_once('/')
                .map_or(native, |(_, model)| model)
                .to_string(),
            None => history
                .lookup(&session_id, ts)
                .unwrap_or_else(|| "unknown".into()),
        };
        let model = model.as_str();
        let usage = TokenUsage {
            input_tokens: row.get(4)?,
            output_tokens: row.get::<_, u32>(5)?.saturating_add(row.get(6)?),
            cache_read_tokens: row.get(7)?,
            cache_creation_tokens: row.get(8)?,
            cache_creation_1h_tokens: 0,
            model: Some(model.into()),
            message_id: None,
        };
        let native_cost: Option<f64> = row.get(9)?;
        let cost = match native_cost {
            Some(cost) if cost.is_finite() && cost >= 0.0 => cost.to_string(),
            _ => find_model_pricing(&tx, model)
                .map(|pricing| {
                    CostCalculator::calculate_for_app("mcode", &usage, &pricing, Decimal::ONE)
                        .total_cost
                        .to_string()
                })
                .unwrap_or_else(|| "0".into()),
        };
        let request_id = format!("mcode:{session_id}:{id}");
        let changed = tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, status_code, session_id, provider_type,
                is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
             ) VALUES (?1, '_mcode_session', 'mcode', ?2, ?2, ?3, ?4, ?5, ?6,
                       '0', '0', '0', '0', ?7, 0, 200, ?8, 'mcode_session', 1, '1', ?9, 'mcode_session', ?10)",
            params![request_id, model, usage.input_tokens, usage.output_tokens,
                usage.cache_read_tokens, usage.cache_creation_tokens, cost, session_id,
                ts / 1000, INPUT_TOKEN_SEMANTICS_FRESH],
        )?;
        result.imported += changed as u32;
        result.skipped += u32::from(changed == 0);
        cursor = id;
    }
    drop(rows);
    // Commit the watermark with the imported rows; pruned logs must never be reimported.
    save_watermark(&tx, key, cursor)?;
    fill_timings(&tx, source, key, cursor)?;
    tx.commit()?;
    Ok(result)
}

fn save_watermark(tx: &rusqlite::Transaction, key: &str, id: i64) -> Result<(), AppError> {
    tx.execute(
        "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, 0, ?2, unixepoch()) ON CONFLICT(file_path) DO UPDATE SET
         last_line_offset = excluded.last_line_offset, last_synced_at = excluded.last_synced_at",
        params![key, id],
    )?;
    Ok(())
}

/// 消息行可能比用量行晚落盘：这么久以后还对不上的，就当它没有计时，不再回查。
const TIMING_SETTLE_MS: i64 = 10 * 60 * 1000;

/// 用量行只有请求开始的时间（ts）。同一轮的 assistant 消息行里，MCode 记了整段请求时长
/// `request_duration_ms` 和首字之后的解码时长 `decode_duration_ms`（首字＝第一个思考 /
/// 正文 / 工具调用片段），首字＝两者之差。按（会话、轮次、输入 / 输出 token）对上那条消息。
///
/// 单独一遍、带自己的水位：新导入的行和升级前已导入、没有计时的行走同一条路；水位只推过
/// 已经对上、或已超过 [`TIMING_SETTLE_MS`] 的行，所以每次同步只回查最近几分钟的行。
/// 旧版没有消息行表、没有 turn_id 列或没有这两个字段时什么都不做，耗时照旧记 0。
fn fill_timings(
    tx: &rusqlite::Transaction,
    source: &rusqlite::Connection,
    key: &str,
    cursor: i64,
) -> Result<(), AppError> {
    let (Ok(mut usage), Ok(mut message)) = (
        source.prepare(
            "SELECT id, session_id, turn_id, ts, input_tokens, output_tokens
             FROM local_runtime_token_usage WHERE id > ?1 AND id <= ?2 ORDER BY id",
        ),
        source.prepare(
            "SELECT json_extract(data_json, '$.usage.request_duration_ms'),
                    json_extract(data_json, '$.usage.decode_duration_ms')
             FROM local_runtime_message_rows
             WHERE session_id = ?1 AND role = 'assistant' AND (?2 IS NULL OR turn_id = ?2)
               AND created_at_ms >= ?3
               AND json_extract(data_json, '$.usage.input_tokens') = ?4
               AND json_extract(data_json, '$.usage.output_tokens') = ?5
             ORDER BY created_at_ms LIMIT 1",
        ),
    ) else {
        return Ok(());
    };
    let timing_key = format!("{key}#timing");
    let mut mark = tx.query_row(
        "SELECT COALESCE(MAX(last_line_offset), 0) FROM session_log_sync WHERE file_path = ?1",
        [&timing_key],
        |row| row.get::<_, i64>(0),
    )?;
    let settled_before = chrono::Utc::now().timestamp_millis() - TIMING_SETTLE_MS;
    let mut settling = true;
    let mut rows = usage.query([mark, cursor])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let session_id: String = row.get(1)?;
        let ts: i64 = row.get(3)?;
        let timing = message
            .query_row(
                params![
                    session_id,
                    row.get::<_, Option<String>>(2)?,
                    ts,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?
                ],
                |found| {
                    Ok((
                        found.get::<_, Option<f64>>(0)?,
                        found.get::<_, Option<f64>>(1)?,
                    ))
                },
            )
            .optional()?
            .and_then(|(request, decode)| timing_of(request, decode));
        if let Some((latency_ms, first_token_ms)) = timing {
            // 只补没有计时的行；被裁剪掉的明细不存在，更新不到。
            tx.execute(
                "UPDATE proxy_request_logs SET latency_ms = ?2, first_token_ms = ?3
                 WHERE request_id = ?1 AND latency_ms = 0",
                params![
                    format!("mcode:{session_id}:{id}"),
                    latency_ms,
                    first_token_ms
                ],
            )?;
        }
        settling &= timing.is_some() || ts < settled_before;
        if settling {
            mark = id;
        }
    }
    save_watermark(tx, &timing_key, mark)
}

/// （耗时, 首字）。缺请求时长就没有计时；解码时长不合理时只留耗时。
fn timing_of(request: Option<f64>, decode: Option<f64>) -> Option<(i64, Option<i64>)> {
    let request = request.filter(|ms| ms.is_finite() && *ms > 0.0)?;
    let first_token = decode
        .filter(|ms| ms.is_finite() && (0.0..=request).contains(ms))
        .map(|decode| (request - decode).round() as i64);
    Some((request.round() as i64, first_token))
}

/// MCode 只在本轮带 executionModel 时才写用量行的 model 列，缺了就是 NULL。用量行的 ts
/// 取自产生它的那条 assistant 消息的 timestamp，而 pi 历史里的消息自带 model，按
/// （会话, 时间戳）回查同一条消息。新版历史存在 pi_history_rows：MCode 写行时把消息的
/// timestamp / role 抄进 created_at_ms / role 列，按列匹配，只解析命中的那条。旧版整段
/// 存在 pi_history_json，每个会话只解析一次。哪张表不存在就跳过哪张。
struct HistoryModels<'a> {
    rows: Option<rusqlite::Statement<'a>>,
    legacy: Option<rusqlite::Statement<'a>>,
    legacy_by_session: HashMap<String, HashMap<i64, String>>,
}

impl<'a> HistoryModels<'a> {
    fn prepare(source: &'a rusqlite::Connection) -> Self {
        Self {
            rows: source
                .prepare(
                    "SELECT json_extract(data_json, '$.model') FROM local_runtime_pi_history_rows
                     WHERE session_id = ?1 AND created_at_ms = ?2 AND role = 'assistant'",
                )
                .ok(),
            legacy: source
                .prepare(
                    "SELECT json_extract(message.value, '$.timestamp'),
                            json_extract(message.value, '$.model')
                     FROM local_runtime_messages, json_each(pi_history_json) AS message
                     WHERE session_id = ?1 AND json_extract(message.value, '$.role') = 'assistant'",
                )
                .ok(),
            legacy_by_session: HashMap::new(),
        }
    }

    fn lookup(&mut self, session_id: &str, ts: i64) -> Option<String> {
        self.lookup_rows(session_id, ts)
            .or_else(|| self.lookup_legacy(session_id, ts))
    }

    fn lookup_rows(&mut self, session_id: &str, ts: i64) -> Option<String> {
        let mut rows = self.rows.as_mut()?.query(params![session_id, ts]).ok()?;
        while let Ok(Some(row)) = rows.next() {
            if let Some(model) = non_empty(row.get(0).ok().flatten()) {
                return Some(model);
            }
        }
        None
    }

    fn lookup_legacy(&mut self, session_id: &str, ts: i64) -> Option<String> {
        if !self.legacy_by_session.contains_key(session_id) {
            let mut models = HashMap::new();
            if let Some(statement) = self.legacy.as_mut() {
                if let Ok(mut rows) = statement.query([session_id]) {
                    while let Ok(Some(row)) = rows.next() {
                        let ts = row
                            .get::<_, Option<f64>>(0)
                            .ok()
                            .flatten()
                            .filter(|ts| ts.is_finite());
                        if let (Some(ts), Some(model)) = (ts, non_empty(row.get(1).ok().flatten()))
                        {
                            models.entry(ts as i64).or_insert(model);
                        }
                    }
                }
            }
            self.legacy_by_session
                .insert(session_id.to_string(), models);
        }
        self.legacy_by_session.get(session_id)?.get(&ts).cloned()
    }
}

fn non_empty(model: Option<String>) -> Option<String> {
    model.filter(|model| !model.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mcode_usage_preserves_model_namespaces() {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                "CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL);
            INSERT INTO local_runtime_token_usage VALUES
            (1,'s1','custom_provider:router/vendor/model',100000,1,2,0,0,0,0.25),
            (2,'s1','custom_provider:router/another/model',101000,1,2,0,0,0,0.5);",
            )
            .unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "namespaces")
                .unwrap()
                .imported,
            2
        );
        let conn = db.conn.lock().unwrap();
        let mut query = conn.prepare("SELECT model, SUM(CAST(total_cost_usd AS REAL)) FROM proxy_request_logs GROUP BY model ORDER BY model").unwrap();
        let costs = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            costs,
            vec![("another/model".into(), 0.5), ("vendor/model".into(), 0.25)]
        );
    }
    #[test]
    fn mcode_usage_recovers_missing_model_from_pi_history() {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                r#"CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL);
            CREATE TABLE local_runtime_pi_history_rows (
            id INTEGER PRIMARY KEY,session_id TEXT,role TEXT,created_at_ms INTEGER,data_json TEXT);
            CREATE TABLE local_runtime_messages (
            session_id TEXT PRIMARY KEY,display_messages_json TEXT,pi_history_json TEXT);
            INSERT INTO local_runtime_pi_history_rows VALUES
            (1,'s1','user',100000,'{"role":"user","timestamp":100000}'),
            (2,'s1','assistant',100000,'{"role":"assistant","provider":"minimax","model":"MiniMax-M3","timestamp":100000}');
            INSERT INTO local_runtime_messages VALUES ('s2','[]',
            '[{"role":"user","timestamp":200000},
              {"role":"assistant","model":"vendor/legacy-model","timestamp":200000},
              {"role":"assistant","model":"legacy-next","timestamp":250000}]');
            INSERT INTO local_runtime_token_usage VALUES
            (1,'s1',NULL,100000,1,2,0,0,0,NULL),
            (2,'s2',NULL,200000,1,2,0,0,0,NULL),
            (3,'s2',NULL,250000,1,2,0,0,0,NULL),
            (4,'s1',NULL,300000,1,2,0,0,0,NULL);"#,
            )
            .unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "history")
                .unwrap()
                .imported,
            4
        );
        let conn = db.conn.lock().unwrap();
        let mut query = conn
            .prepare("SELECT model FROM proxy_request_logs ORDER BY created_at")
            .unwrap();
        let models = query
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        // 旧版 JSON 里的模型名原样保留（不像 model 列那样剥 provider 前缀）；查不到仍记 unknown
        assert_eq!(
            models,
            [
                "MiniMax-M3",
                "vendor/legacy-model",
                "legacy-next",
                "unknown"
            ]
        );
    }

    #[test]
    fn mcode_usage_preserves_native_zero_cost_and_does_not_reimport_pruned_rows() {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source.execute_batch("CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL);
            INSERT INTO local_runtime_token_usage VALUES (1,'s1','custom_provider:test/model',100000,10,20,3,40,5,0);").unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "mcode-test")
                .unwrap()
                .imported,
            1
        );
        {
            let conn = db.conn.lock().unwrap();
            let values=conn.query_row("SELECT input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,total_cost_usd FROM proxy_request_logs WHERE app_type='mcode'",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?))).unwrap();
            assert_eq!(values, (10, 23, 40, 5, "0".into()));
            let semantics: i64 = conn
                .query_row(
                    "SELECT input_token_semantics FROM proxy_request_logs WHERE app_type='mcode'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(semantics, INPUT_TOKEN_SEMANTICS_FRESH);
            conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        }
        assert_eq!(
            sync_from_database(&db, &source, "mcode-test")
                .unwrap()
                .imported,
            0
        );
        source.execute_batch("INSERT INTO local_runtime_token_usage VALUES (2,'s1','custom_provider:test/model',101000,1,2,0,0,0,0.25)").unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "mcode-test")
                .unwrap()
                .imported,
            1
        );
    }

    #[test]
    fn mcode_timing_comes_from_the_matching_assistant_message_and_backfills_late_rows() {
        let now = chrono::Utc::now().timestamp_millis();
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                "CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,turn_id TEXT,model TEXT,ts INTEGER,
            input_tokens INTEGER,output_tokens INTEGER,reasoning_tokens INTEGER,
            cache_read_tokens INTEGER,cache_write_tokens INTEGER,cost_usd REAL);
            CREATE TABLE local_runtime_message_rows (
            id INTEGER PRIMARY KEY,session_id TEXT,role TEXT,turn_id TEXT,created_at_ms INTEGER,
            data_json TEXT);",
            )
            .unwrap();
        let usage = |id: i64, turn: &str, ts: i64, input: i64, output: i64| {
            source
                .execute(
                    "INSERT INTO local_runtime_token_usage
                     VALUES (?1,'s1',?2,'custom_provider:p/k3',?3,?4,?5,0,0,0,0)",
                    params![id, turn, ts, input, output],
                )
                .unwrap();
        };
        let message = |turn: &str, end: i64, input: i64, output: i64, durations: &str| {
            source
                .execute(
                    "INSERT INTO local_runtime_message_rows (session_id,role,turn_id,created_at_ms,data_json)
                     VALUES ('s1','assistant',?1,?2,?3)",
                    params![
                        turn,
                        end,
                        format!(r#"{{"usage":{{"input_tokens":{input},"output_tokens":{output}{durations}}}}}"#)
                    ],
                )
                .unwrap();
        };
        // Values from a real `mcode exec` run (0.6.13): one long reply, then a tool-call turn.
        let old = now - 3_600_000;
        usage(1, "t1", old, 12828, 384);
        message(
            "t1",
            old + 16_610,
            12828,
            384,
            r#","decode_duration_ms":14595,"request_duration_ms":16616"#,
        );
        usage(2, "t2", old + 20_000, 797, 62);
        // Same tokens in another turn must not be picked.
        message(
            "t9",
            old + 21_000,
            797,
            62,
            r#","decode_duration_ms":1,"request_duration_ms":1000"#,
        );
        message(
            "t2",
            old + 28_880,
            797,
            62,
            r#","decode_duration_ms":797,"request_duration_ms":8880"#,
        );
        // No durations recorded (older build / no detailed usage): no timing, settles anyway.
        usage(3, "t3", old + 40_000, 354, 160);
        message("t3", old + 43_000, 354, 160, "");
        // Recent row whose message is not written yet.
        usage(4, "t4", now, 238, 130);

        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "timing").unwrap().imported,
            4
        );
        let timing = |db: &Database| -> Vec<(i64, Option<i64>)> {
            let conn = db.conn.lock().unwrap();
            let mut query = conn
                .prepare(
                    "SELECT latency_ms, first_token_ms FROM proxy_request_logs ORDER BY request_id",
                )
                .unwrap();
            query
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            timing(&db),
            [
                (16616, Some(2021)),
                (8880, Some(8083)),
                (0, None),
                (0, None)
            ]
        );

        // The late message is picked up on the next sync without reimporting anything.
        message(
            "t4",
            now + 14_250,
            238,
            130,
            r#","decode_duration_ms":1626,"request_duration_ms":14250"#,
        );
        assert_eq!(
            sync_from_database(&db, &source, "timing").unwrap().imported,
            0
        );
        assert_eq!(timing(&db)[3], (14250, Some(12624)));
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT last_line_offset FROM session_log_sync WHERE file_path='timing#timing'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap(),
            4
        );
    }
}

#[cfg(test)]
mod native_validation {
    use super::*;
    #[test]
    #[ignore = "requires the real MCode workflow in an isolated CC_SWITCH_TEST_HOME"]
    fn mcode_reads_real_sessions_and_usage() -> Result<(), AppError> {
        assert!(std::env::var("CC_SWITCH_TEST_HOME").is_ok());
        let sessions = mcode::scan_sessions();
        assert!(!sessions.is_empty());
        let mut messages = 0;
        for session in &sessions {
            messages += mcode::load_messages(session.source_path.as_deref().unwrap())
                .unwrap()
                .len();
        }
        assert!(messages >= 2);
        let db = Database::memory()?;
        let imported = sync_mcode_usage(&db)?;
        assert!(imported.imported > 0);
        assert_eq!(sync_mcode_usage(&db)?.imported, 0);
        let native = mcode::open_database()?;
        let expected=native.query_row("SELECT SUM(input_tokens), SUM(output_tokens + reasoning_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens) FROM local_runtime_token_usage",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?)))?;
        let conn = lock_conn!(db.conn);
        let actual=conn.query_row("SELECT SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(cache_creation_tokens) FROM proxy_request_logs WHERE app_type='mcode'",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?)))?;
        assert_eq!(actual, expected);
        Ok(())
    }
}
