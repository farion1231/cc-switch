//! 从 WorkBuddy 的会话日志导入用量：`<data dir>/projects/<cwd>/<sessionId>.jsonl`。
//!
//! 每条 assistant `message` 事件对应一次模型请求，`providerData.rawUsage` 是上游返回的
//! OpenAI 口径用量：`prompt_tokens` 已含缓存命中（`prompt_cache_hit_tokens +
//! prompt_cache_miss_tokens == prompt_tokens`），`completion_tokens` 已含思考 token。
//! 入库时把缓存读写从输入里扣掉，按「新输入」口径存（`input_token_semantics = FRESH`），
//! 计费时输入、缓存读、缓存写、输出各按各的单价算。`rawUsage.credit` 是 WorkBuddy 积分，
//! 不是美元，不当作费用。
//!
//! 日志只追加：按字节偏移增量读，只处理以换行结尾的完整行；文件变短说明被重写，从头再读，
//! `session_usage_dedup` 里的永久回执保证同一请求不会重复入库。
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::{
    session_usage::{
        load_sync_cursors, metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
    },
    sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH,
    usage_stats::find_model_pricing,
};
use crate::session_manager::providers::workbuddy;
use rust_decimal::Decimal;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::Path,
};

const DATA_SOURCE: &str = "workbuddy_session";

pub fn sync_workbuddy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_projects(db, &workbuddy::projects_dir())
}

fn sync_projects(db: &Database, projects: &Path) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let files = workbuddy::session_files(projects);
    if files.is_empty() {
        return Ok(result);
    }
    let cursors = load_sync_cursors(db)?;
    for path in files {
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        let modified = metadata_modified_nanos(&meta);
        let key = format!("workbuddy:{}", path.to_string_lossy());
        let cursor = cursors.get(&key);
        if cursor
            .is_some_and(|c| c.last_modified == modified && c.last_line_offset == meta.len() as i64)
        {
            continue;
        }
        let offset = cursor
            .map(|c| c.last_line_offset)
            .filter(|offset| *offset >= 0 && *offset <= meta.len() as i64)
            .unwrap_or(0);
        match sync_file(db, &path, &key, modified, offset) {
            Ok(part) => result.merge(part),
            Err(error) => result
                .errors
                .push(format!("WorkBuddy session {}: {error}", path.display())),
        }
    }
    Ok(result)
}

struct Entry {
    request_id: String,
    session_id: String,
    model: String,
    usage: TokenUsage,
    created_ms: i64,
}

fn entry(record: &Value) -> Option<Entry> {
    let usage = workbuddy::assistant_usage(record)?;
    if usage.is_empty() {
        return None;
    }
    let id = record.get("id").and_then(Value::as_str)?;
    let session_id = record
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let model = usage.model.clone().unwrap_or_else(|| "unknown".into());
    let clamp = |n: u64| n.min(u32::MAX as u64) as u32;
    Some(Entry {
        request_id: format!("workbuddy:{session_id}:{id}"),
        session_id,
        usage: TokenUsage {
            input_tokens: clamp(usage.fresh_input),
            output_tokens: clamp(usage.output),
            cache_read_tokens: clamp(usage.cache_read),
            cache_creation_tokens: clamp(usage.cache_write),
            cache_creation_1h_tokens: 0,
            model: Some(model.clone()),
            message_id: None,
        },
        model,
        created_ms: record.get("timestamp").and_then(Value::as_i64)?,
    })
}

fn sync_file(
    db: &Database,
    path: &Path,
    key: &str,
    modified: i64,
    offset: i64,
) -> Result<SessionSyncResult, AppError> {
    let mut file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|error| AppError::io(path, error))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut bytes = offset;
    let mut entries = Vec::new();
    loop {
        buffer.clear();
        let size = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| AppError::io(path, error))?;
        // 写入方可能只刷了最后一行的一部分，下一轮再读。
        if size == 0 || buffer.last() != Some(&b'\n') {
            break;
        }
        bytes += size as i64;
        if let Some(entry) = serde_json::from_slice::<Value>(&buffer)
            .ok()
            .as_ref()
            .and_then(entry)
        {
            entries.push(entry);
        }
    }

    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for entry in entries {
        if tx.execute(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES (?1, ?2, ?2, 1)",
            [DATA_SOURCE, &entry.request_id],
        )? == 0
        {
            result.skipped += 1;
            continue;
        }
        let usage = &entry.usage;
        let amounts = costs(&tx, &entry.model, usage);
        tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, status_code, session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
            ) VALUES (?1, '_workbuddy_session', 'workbuddy', ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, 200, ?12, ?13, 1, '1', ?14, ?13, ?15)",
            rusqlite::params![
                entry.request_id,
                entry.model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
                amounts[0],
                amounts[1],
                amounts[2],
                amounts[3],
                amounts[4],
                entry.session_id,
                DATA_SOURCE,
                entry.created_ms / 1000,
                INPUT_TOKEN_SEMANTICS_FRESH,
            ],
        )?;
        result.imported += 1;
    }
    update_sync_state_on_conn(&tx, key, modified, bytes)?;
    tx.commit()?;
    Ok(result)
}

fn costs(conn: &rusqlite::Connection, model: &str, usage: &TokenUsage) -> [String; 5] {
    find_model_pricing(conn, model)
        .map(|pricing| {
            let cost =
                CostCalculator::calculate_for_app("workbuddy", usage, &pricing, Decimal::ONE);
            [
                cost.input_cost,
                cost.output_cost,
                cost.cache_read_cost,
                cost.cache_creation_cost,
                cost.total_cost,
            ]
        })
        .unwrap_or([Decimal::ZERO; 5])
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn assistant(
        id: &str,
        model: &str,
        prompt: u64,
        hit: u64,
        write: u64,
        completion: u64,
    ) -> String {
        serde_json::json!({
            "id": id, "parentId": "p", "timestamp": 1792764379226_i64, "type": "message",
            "role": "assistant", "status": "completed", "sessionId": "s1", "cwd": "/w",
            "content": [{"type": "output_text", "text": "ok"}],
            "providerData": {"model": model, "requestModelId": "alias", "rawUsage": {
                "prompt_tokens": prompt, "completion_tokens": completion,
                "total_tokens": prompt + completion,
                "prompt_cache_hit_tokens": hit, "prompt_cache_miss_tokens": prompt - hit,
                "cache_read_input_tokens": 0, "cache_creation_input_tokens": write,
                "prompt_cache_write_tokens": 0, "completion_thinking_tokens": 3,
                "credit": 0.5, "cached_tokens": 0
            }}
        })
        .to_string()
    }

    const USER: &str = r#"{"id":"u1","timestamp":1792764357348,"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}],"sessionId":"s1","cwd":"/w"}"#;
    const REASONING: &str = r#"{"id":"r1","timestamp":1792764379203,"type":"reasoning","rawContent":[{"type":"reasoning_text","text":"..."}],"providerData":{"model":"m"},"sessionId":"s1"}"#;

    fn rows(db: &Database) -> Vec<(String, String, i64, i64, i64, i64, i64)> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT request_id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, input_token_semantics FROM proxy_request_logs WHERE app_type = 'workbuddy' ORDER BY request_id")
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn session_file(root: &Path) -> PathBuf {
        let project = root.join("Users-me-proj");
        fs::create_dir_all(&project).unwrap();
        project.join("s1.jsonl")
    }

    #[test]
    fn workbuddy_usage_imports_assistant_messages_incrementally() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        fs::write(
            &path,
            format!(
                "{USER}\n{REASONING}\n{}\n",
                assistant("a1", "glm-test", 1200, 1000, 0, 34)
            ),
        )
        .unwrap();
        let db = Database::memory().unwrap();
        let first = sync_projects(&db, dir.path()).unwrap();
        assert_eq!(first.imported, 1);
        assert_eq!(
            rows(&db),
            vec![(
                "workbuddy:s1:a1".into(),
                "glm-test".into(),
                200,
                34,
                1000,
                0,
                INPUT_TOKEN_SEMANTICS_FRESH
            )]
        );

        // 文件没变就跳过；追加的请求只导一次；没写完的最后一行等下一轮。
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 0);
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", assistant("a2", "glm-test", 500, 0, 100, 10)).unwrap();
        write!(file, "{{\"id\":\"a3\",").unwrap();
        drop(file);
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 1);
        let imported = rows(&db);
        assert_eq!(imported.len(), 2);
        assert_eq!((imported[1].2, imported[1].4, imported[1].5), (400, 0, 100));
    }

    #[test]
    fn workbuddy_usage_receipts_survive_file_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        let line = assistant("a1", "m", 300, 0, 0, 5);
        fs::write(&path, format!("{line}\n{line}\n")).unwrap();
        let db = Database::memory().unwrap();
        let first = sync_projects(&db, dir.path()).unwrap();
        assert_eq!((first.imported, first.skipped), (1, 1));
        // 文件被改短重写：从头再读，回执挡住重复。
        fs::write(&path, format!("{line}\n")).unwrap();
        let file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 0);
        assert_eq!(rows(&db).len(), 1);
    }

    #[test]
    fn workbuddy_usage_skips_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::memory().unwrap();
        let result = sync_projects(&db, &dir.path().join("missing")).unwrap();
        assert_eq!(result.imported, 0);
        assert!(result.errors.is_empty());
    }
}
