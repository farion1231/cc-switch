//! Import DeepSeek Harness's per-turn model usage from its session logs.
//!
//! DeepSeek Harness（`~/.dsh`）把每个会话的事件流存成
//! `sessions/<workspace>/<session-id>/session.v*.jsonl.zstd`——zstd 压缩的
//! JSONL。token 用量在 `assistant/message` 事件的 `data.usage` 上（Anthropic
//! 风格：inputTokens 不含缓存读写，二者单独上报），模型名在同事件的
//! `data.message.source.model`。
//!
//! 压缩文件没法像明文 JSONL 那样按字节偏移增量读，改用事件自带的单调
//! `seq` 做水位：mtime 没变的文件直接跳过；变了就整文件重解压，只导入
//! seq 大于水位的行。这样即使 rollup 剪掉 30 天前的明细、之后文件又被
//! 追加触发重扫，旧事件也不会重导。

use crate::config::get_home_dir;
use crate::database::Database;
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::session_usage::{
    load_sync_cursors, metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
    SyncCursor,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::usage_stats::find_model_pricing;
use rusqlite::params;
use rust_decimal::Decimal;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

fn sessions_root() -> PathBuf {
    std::env::var_os("DSH_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".dsh"))
        .join("sessions")
}

pub fn sync_dsh_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_from_root(db, &sessions_root())
}

pub(super) fn sync_from_root(db: &Database, root: &Path) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let cursors = load_sync_cursors(db)?;
    let mut conn = crate::database::lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let Ok(workspaces) = fs::read_dir(root) else {
        return Ok(result);
    };
    for workspace in workspaces.flatten() {
        let Ok(sessions) = fs::read_dir(workspace.path()) else {
            continue;
        };
        for session in sessions.flatten() {
            let Ok(entries) = fs::read_dir(session.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !is_session_log(&path.file_name().and_then(|n| n.to_str()).unwrap_or("")) {
                    continue;
                }
                sync_session_file(&tx, &path, &cursors, &mut result)?;
            }
        }
    }
    tx.commit()?;
    Ok(result)
}

/// 只认 `session.v*.jsonl.zstd`；版本号升到 v5 也继续适用。
fn is_session_log(name: &str) -> bool {
    name.starts_with("session.") && name.ends_with(".jsonl.zstd")
}

fn sync_session_file(
    tx: &rusqlite::Transaction,
    path: &Path,
    cursors: &std::collections::HashMap<String, SyncCursor>,
    result: &mut SessionSyncResult,
) -> Result<(), AppError> {
    result.files_scanned += 1;
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let key = format!("dsh:{}", path.display());
    let modified_nanos = metadata_modified_nanos(&metadata);
    let Some(cursor) = cursors.get(&key) else {
        return import_file(tx, path, &key, modified_nanos, 0, result);
    };
    // mtime 与上次一致说明没有新事件，跳过重解压（8MB 级会话目录不值得每轮全读）
    if cursor.last_modified == modified_nanos {
        return Ok(());
    }
    import_file(
        tx,
        path,
        &key,
        modified_nanos,
        cursor.last_line_offset,
        result,
    )
}

fn import_file(
    tx: &rusqlite::Transaction,
    path: &Path,
    key: &str,
    modified_nanos: i64,
    seq_watermark: i64,
    result: &mut SessionSyncResult,
) -> Result<(), AppError> {
    let file = File::open(path).map_err(|source| AppError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let decoder = match zstd::stream::read::Decoder::new(file) {
        Ok(decoder) => decoder,
        Err(error) => {
            result
                .errors
                .push(format!("{} 解压失败: {error}", path.display()));
            return Ok(());
        }
    };
    // 会话正在写入时流可能在尾部截断：已解出的完整行照常处理，读错只终止本文件
    let lines = BufReader::new(decoder).lines();
    let session_id = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut watermark = seq_watermark;
    for line in lines {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                log::debug!("dsh session log truncated ({}): {error}", path.display());
                break;
            }
        };
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(seq) = event.get("seq").and_then(Value::as_i64) else {
            continue;
        };
        if seq <= watermark {
            continue;
        }
        if event.get("type").and_then(Value::as_str) == Some("assistant/message") {
            import_message(tx, &event, &session_id, seq, result)?;
        }
        watermark = seq;
    }
    update_sync_state_on_conn(tx, key, modified_nanos, watermark)?;
    Ok(())
}

fn import_message(
    tx: &rusqlite::Transaction,
    event: &Value,
    session_id: &str,
    seq: i64,
    result: &mut SessionSyncResult,
) -> Result<(), AppError> {
    let Some(data) = event.get("data") else {
        return Ok(());
    };
    let usage = data
        .get("usage")
        .and_then(|usage| usage.as_object())
        .filter(|usage| usage.values().any(|value| value.as_u64().unwrap_or(0) > 0));
    let Some(usage) = usage else {
        return Ok(());
    };
    let token = |field: &str| -> u32 {
        usage
            .get(field)
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32
    };
    let message = data.get("message");
    let model = message
        .and_then(|m| m.get("source"))
        .and_then(|s| s.get("model"))
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty())
        .unwrap_or("unknown")
        .to_string();
    let message_id = message
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let request_id = match message_id {
        Some(id) => format!("dsh:{id}"),
        // 兜底：无消息 ID 时用（会话, seq）保证幂等
        None => format!("dsh:{session_id}:{seq}"),
    };
    let created_at_ms = event.get("time").and_then(Value::as_i64).unwrap_or(0);
    let usage = TokenUsage {
        input_tokens: token("inputTokens"),
        output_tokens: token("outputTokens"),
        cache_read_tokens: token("cacheReadTokens"),
        cache_creation_tokens: token("cacheWriteTokens"),
        cache_creation_1h_tokens: 0,
        model: Some(model.clone()),
        message_id: None,
    };
    let cost = find_model_pricing(tx, &model)
        .map(|pricing| {
            CostCalculator::calculate_for_app("dsh", &usage, &pricing, Decimal::ONE)
                .total_cost
                .to_string()
        })
        .unwrap_or_else(|| "0".into());
    let changed = tx.execute(
        "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
            total_cost_usd, latency_ms, status_code, session_id, provider_type,
            is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
         ) VALUES (?1, '_dsh_session', 'dsh', ?2, ?2, ?3, ?4, ?5, ?6,
                   '0', '0', '0', '0', ?7, 0, 200, ?8, 'dsh_session',
                   1, '1', ?9, 'dsh_session', ?10)",
        params![
            request_id,
            model,
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            usage.cache_creation_tokens,
            cost,
            session_id,
            created_at_ms / 1000,
            INPUT_TOKEN_SEMANTICS_FRESH
        ],
    )?;
    result.imported += changed as u32;
    result.skipped += u32::from(changed == 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write_session(root: &Path, session: &str, events: &[Value]) -> PathBuf {
        let dir = root.join("ws").join(session);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.v4.jsonl.zstd");
        let file = File::create(&path).unwrap();
        let mut encoder = zstd::stream::write::Encoder::new(file, 3).unwrap();
        use std::io::Write;
        for event in events {
            writeln!(encoder, "{event}").unwrap();
        }
        encoder.finish().unwrap();
        path
    }

    fn message_event(seq: i64, msg_id: &str, model: &str, usage: Value) -> Value {
        serde_json::json!({
            "type": "assistant/message",
            "seq": seq,
            "time": 1792510633226 + seq * 1000,
            "data": {
                "message": {
                    "id": msg_id,
                    "role": "assistant",
                    "source": {"kind": "model", "provider": "deepseek-account", "model": model},
                },
                "usage": usage,
            },
        })
    }

    fn usage_json(input: u64, output: u64, cache_read: u64, cache_write: u64) -> Value {
        serde_json::json!({
            "inputTokens": input, "outputTokens": output,
            "cacheReadTokens": cache_read, "cacheWriteTokens": cache_write,
            "totalTokens": input + output + cache_read + cache_write,
        })
    }

    #[test]
    fn dsh_usage_imports_fresh_semantics_rows() {
        let root = std::env::temp_dir().join(format!("dsh-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_session(
            &root,
            "session-abc",
            &[
                serde_json::json!({"type": "session", "seq": 1, "id": "session-abc", "version": 4}),
                serde_json::json!({"type": "model/selection", "seq": 2, "data": {"model": "deepseek-flash"}}),
                message_event(
                    19,
                    "2eed003e-c3d4-4b7d-bbff-d14c506fbfa7",
                    "deepseek-flash",
                    usage_json(830, 329, 9472, 0),
                ),
                message_event(20, "no-usage-msg", "deepseek-flash", serde_json::json!({})),
            ],
        );
        let db = Database::memory().unwrap();
        let result = sync_from_root(&db, &root).unwrap();
        assert_eq!(result.imported, 1);
        {
            let conn = db.conn.lock().unwrap();
            let row = conn
                .query_row(
                    "SELECT model, input_tokens, output_tokens, cache_read_tokens,
                            cache_creation_tokens, status_code, data_source,
                            input_token_semantics, session_id, created_at
                     FROM proxy_request_logs WHERE app_type='dsh'",
                    [],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, i64>(4)?,
                            r.get::<_, i64>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, i64>(7)?,
                            r.get::<_, String>(8)?,
                            r.get::<_, i64>(9)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(row.0, "deepseek-flash");
            assert_eq!((row.1, row.2, row.3, row.4), (830, 329, 9472, 0));
            assert_eq!(row.5, 200);
            assert_eq!(row.6, "dsh_session");
            assert_eq!(row.7, INPUT_TOKEN_SEMANTICS_FRESH);
            assert_eq!(row.8, "session-abc");
            assert_eq!(row.9, (1792510633226 + 19000) / 1000);
            // fresh 语义：input 不扣缓存
            let fresh: i64 = conn
                .query_row(
                    &format!(
                        "SELECT {} FROM proxy_request_logs WHERE app_type='dsh'",
                        crate::services::sql_helpers::fresh_input_sql("")
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(fresh, 830);
        }
        // mtime 未变：跳过重解压，无新增
        assert_eq!(sync_from_root(&db, &root).unwrap().imported, 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_usage_resumes_by_seq_watermark_and_never_reimports_pruned_rows() {
        let root = std::env::temp_dir().join(format!("dsh-resume-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_session(
            &root,
            "session-resume",
            &[message_event(
                5,
                "msg-5",
                "deepseek-flash",
                usage_json(10, 10, 0, 0),
            )],
        );
        let db = Database::memory().unwrap();
        assert_eq!(sync_from_root(&db, &root).unwrap().imported, 1);
        // 模拟 rollup 剪掉明细 + 文件被追加触发整文件重扫
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        }
        write_session(
            &root,
            "session-resume",
            &[
                message_event(5, "msg-5", "deepseek-flash", usage_json(10, 10, 0, 0)),
                message_event(6, "msg-6", "deepseek-flash", usage_json(20, 20, 0, 0)),
            ],
        );
        let result = sync_from_root(&db, &root).unwrap();
        assert_eq!(result.imported, 1, "只应补导 seq=6，被剪的 seq=5 不得重导");
        {
            let conn = db.conn.lock().unwrap();
            let watermark: i64 = conn
                .query_row(
                    "SELECT last_line_offset FROM session_log_sync WHERE file_path LIKE 'dsh:%'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(watermark, 6);
            let inputs: Vec<i64> = {
                let mut stmt = conn
                    .prepare("SELECT input_tokens FROM proxy_request_logs ORDER BY input_tokens")
                    .unwrap();
                stmt.query_map([], |r| r.get(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            };
            assert_eq!(inputs, vec![20]);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_usage_falls_back_when_model_or_message_id_missing() {
        let root = std::env::temp_dir().join(format!("dsh-fallback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut no_model = message_event(3, "msg-3", "", usage_json(1, 2, 3, 4));
        no_model["data"]["message"]
            .as_object_mut()
            .unwrap()
            .remove("id");
        write_session(&root, "session-fb", &[no_model]);
        let db = Database::memory().unwrap();
        assert_eq!(sync_from_root(&db, &root).unwrap().imported, 1);
        {
            let conn = db.conn.lock().unwrap();
            let (model, request_id) = conn
                .query_row(
                    "SELECT model, request_id FROM proxy_request_logs WHERE app_type='dsh'",
                    [],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .unwrap();
            assert_eq!(model, "unknown");
            assert_eq!(request_id, "dsh:session-fb:3");
        }
        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod native_validation {
    use super::*;
    #[test]
    #[ignore = "requires the real DeepSeek Harness sessions; set DSH_HOME to run"]
    fn dsh_reads_real_sessions() -> Result<(), AppError> {
        assert!(std::env::var("DSH_HOME").is_ok());
        let db = Database::memory()?;
        let imported = sync_dsh_usage(&db)?;
        assert!(imported.imported > 0);
        assert_eq!(sync_dsh_usage(&db)?.imported, 0);
        let conn = db.conn.lock().unwrap();
        let rows = conn.query_row(
            "SELECT COUNT(*), SUM(input_tokens), SUM(output_tokens),
                    SUM(cache_read_tokens), SUM(cache_creation_tokens)
             FROM proxy_request_logs WHERE app_type='dsh'",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            },
        )?;
        assert!(rows.0 > 0);
        assert!(rows.1.unwrap_or(0) > 0 && rows.2.unwrap_or(0) > 0);
        Ok(())
    }
}
