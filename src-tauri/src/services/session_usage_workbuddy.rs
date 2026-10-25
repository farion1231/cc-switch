//! Import WorkBuddy's per-request token usage from its local trace files
//! without modifying them.
//!
//! WorkBuddy（腾讯系桌面 Agent）在 `~/.workbuddy/traces/<pid>/` 下按工作流写
//! OpenTelemetry 风格的 trace，其中 `generation` span 的 `toolOutput` 保存了
//! 原始 LLM API 响应（OpenAI chat.completion 形态），含完整 usage。磁盘上有
//! 两种形态：
//! - `trace_*.json`  —— 旧版单文件 JSON（整文件一次写入，2026-10 中止）
//! - `trace_*.jsonl` —— 新版流式行（trace_start / span_start / span_end /
//!   trace_update / trace_end，追加写）
//!
//! 字段语义（对照 1.4 万条真实 span 验证）：
//! - `prompt_tokens` 含 `cached_tokens`（OpenAI 口径）→ 缓存内含应用，
//!   INPUT_TOKEN_SEMANTICS_TOTAL
//! - `completion_tokens` 已含 `reasoning_tokens` → 原样入库，不再叠加
//! - `spanId` 全局唯一 → request_id = `wb:<spanId>`；响应 `id` 不能当主键
//!   （MiniMax 自定义协议会跨请求复用同一 id）
//! - 会话归属：真实 sessionId 只出现在很晚的 trace_update/trace_end 行里
//!   （甚至永远不出现），因此统一用每个 trace 首行就有的 `traceId` 充当
//!   session 标识
use crate::config::get_home_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::session_usage::{
    metadata_modified_nanos, parse_timestamp_millis, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::find_model_pricing;
use rusqlite::params;
use rust_decimal::Decimal;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 一条 generation span 折算成入库所需的列。
struct GenerationRecord {
    span_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    created_at: i64,
    latency_ms: i64,
}

/// WorkBuddy 的 trace 目录。优先级：`WORKBUDDY_TRACES` 环境变量 > `~/.workbuddy/traces`
fn workbuddy_traces_dir() -> PathBuf {
    if let Ok(custom_path) = std::env::var("WORKBUDDY_TRACES") {
        if !custom_path.is_empty() {
            return PathBuf::from(custom_path);
        }
    }
    get_home_dir().join(".workbuddy").join("traces")
}

pub fn sync_workbuddy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let root = workbuddy_traces_dir();
    if !root.is_dir() {
        return Ok(SessionSyncResult::default());
    }
    sync_workbuddy_usage_from(db, &root)
}

fn sync_workbuddy_usage_from(db: &Database, root: &Path) -> Result<SessionSyncResult, AppError> {
    let files = collect_trace_files(root);
    if files.is_empty() {
        return Ok(SessionSyncResult::default());
    }
    let cursors = {
        let conn = lock_conn!(db.conn);
        load_workbuddy_cursors(&conn)?
    };
    let mut result = SessionSyncResult::default();
    for path in &files {
        // 单文件失败只记错误不推进游标，下轮重试；不影响其他文件
        if let Err(error) = sync_single_trace_file(db, path, &cursors, &mut result) {
            result.errors.push(format!(
                "WorkBuddy 文件 {} 同步失败: {error}",
                path.display()
            ));
        }
    }
    Ok(result)
}

/// 收集 traces 根目录下每个 pid 子目录里的 trace_*.json / trace_*.jsonl。
fn collect_trace_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(children) = fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            let child_path = child.path();
            let Some(name) = child_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with("trace_") && (name.ends_with(".json") || name.ends_with(".jsonl")) {
                files.push(child_path);
            }
        }
    }
    files.sort();
    files
}

/// 预取本模块的同步游标（file_path 以 `workbuddy:` 开头的行）。
/// 键为完整 file_path，值为 (last_modified 纳秒, last_byte_offset)。
fn load_workbuddy_cursors(
    conn: &rusqlite::Connection,
) -> Result<HashMap<String, (i64, Option<i64>)>, AppError> {
    let mut stmt = conn
        .prepare("SELECT file_path, last_modified, last_byte_offset FROM session_log_sync")
        .map_err(|e| AppError::Database(format!("预取 WorkBuddy 游标失败: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .map_err(|e| AppError::Database(format!("预取 WorkBuddy 游标失败: {e}")))?;
    let mut cursors = HashMap::new();
    for row in rows.flatten() {
        if row.0.starts_with("workbuddy:") {
            cursors.insert(row.0, (row.1, row.2));
        }
    }
    Ok(cursors)
}

fn sync_single_trace_file(
    db: &Database,
    path: &Path,
    cursors: &HashMap<String, (i64, Option<i64>)>,
    result: &mut SessionSyncResult,
) -> Result<(), AppError> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let modified_nanos = metadata_modified_nanos(&metadata);
    let file_size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
    let key = format!("workbuddy:{}", path.display());
    let cursor = cursors.get(&key).copied();
    if let Some((last_modified, Some(last_byte_offset))) = cursor {
        // mtime 与大小都没变 → 文件未追加，整文件跳过
        if last_modified == modified_nanos && last_byte_offset == file_size {
            return Ok(());
        }
    }
    result.files_scanned += 1;
    let is_jsonl = path.extension().and_then(|e| e.to_str()) == Some("jsonl");
    let resume_byte = cursor
        .and_then(|(_, offset)| offset)
        .filter(|offset| *offset > 0)
        .unwrap_or(0);
    // session 标识：优先 trace 内容里的 traceId，缺失时退回文件名主干
    // （traceId 与文件名 uuid 一致，只是不依赖这个实现细节）
    let fallback_session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    // 解析不持有数据库锁：大 trace（可达十几 MB）解析以秒计，锁内做会
    // 阻塞代理写日志；拿到解析结果后每文件一个短事务落库
    let (session_id, records, committed_bytes) = if is_jsonl {
        let (trace_id, records, committed) = parse_jsonl_trace(path, resume_byte, file_size)?;
        (
            trace_id.unwrap_or_else(|| fallback_session_id.clone()),
            records,
            committed,
        )
    } else {
        let (trace_id, records) = parse_json_trace(path)?;
        (
            trace_id.unwrap_or_else(|| fallback_session_id.clone()),
            records,
            file_size,
        )
    };
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for record in records {
        insert_generation_record(&tx, &session_id, record, result)?;
    }
    // 游标与导入行同事务提交；trace 文件被 WorkBuddy 保留策略清理后，
    // 已导入的行靠 request_id 去重不受影响
    tx.execute(
        "INSERT INTO session_log_sync
             (file_path, last_modified, last_line_offset, last_byte_offset, last_synced_at)
         VALUES (?1, ?2, 0, ?3, unixepoch())
         ON CONFLICT(file_path) DO UPDATE SET
             last_modified = excluded.last_modified,
             last_byte_offset = excluded.last_byte_offset,
             last_synced_at = excluded.last_synced_at",
        params![key, modified_nanos, committed_bytes],
    )?;
    tx.commit()?;
    Ok(())
}

/// 解析新版流式 trace：每行一个事件，只关心 trace_*（取 traceId）和
/// span_end（generation 用量）。从 `resume_byte` 续读，只推进到最后一个
/// 完整行；没有换行符的尾部半行留给下一轮。
fn parse_jsonl_trace(
    path: &Path,
    resume_byte: i64,
    file_size: i64,
) -> Result<(Option<String>, Vec<GenerationRecord>, i64), AppError> {
    let mut file =
        File::open(path).map_err(|e| AppError::Config(format!("打开 trace 失败: {e}")))?;
    // trace_start 永远是首行；续读时先读首行拿 traceId，再跳到字节游标
    let trace_id = {
        let mut reader = BufReader::new(&mut file);
        let mut line = Vec::new();
        reader
            .read_until(b'\n', &mut line)
            .map_err(|e| AppError::Config(format!("读取 trace 首行失败: {e}")))?;
        parse_trace_id(&line)
    };
    // 首次同步从 0 读；游标越界（文件被重写变小）同样从头重读，spanId 去重兜底
    let start = if resume_byte > 0 && resume_byte <= file_size {
        resume_byte
    } else {
        0
    };
    file.seek(SeekFrom::Start(start as u64))
        .map_err(|e| AppError::Config(format!("定位 trace 游标失败: {e}")))?;
    let mut reader = BufReader::new(file);
    let mut committed = start;
    let mut records = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        let read = reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| AppError::Config(format!("读取 trace 行失败: {e}")))?;
        if read == 0 {
            break;
        }
        if !buf.ends_with(b"\n") {
            break;
        }
        committed += read as i64;
        if buf.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&buf) else {
            continue;
        };
        if value.get("type").and_then(|t| t.as_str()) != Some("span_end") {
            continue;
        }
        let span = value.get("span").unwrap_or(&Value::Null);
        if span.get("type").and_then(|t| t.as_str()) != Some("generation") {
            continue;
        }
        if let Some(record) = extract_generation_record(span) {
            records.push(record);
        }
    }
    Ok((trace_id, records, committed))
}

/// 解析旧版单文件 trace：整文件一个 JSON 对象，spans 数组里筛 generation。
fn parse_json_trace(path: &Path) -> Result<(Option<String>, Vec<GenerationRecord>), AppError> {
    let file = File::open(path).map_err(|e| AppError::Config(format!("打开 trace 失败: {e}")))?;
    let root: Value = serde_json::from_reader(file)
        .map_err(|e| AppError::Config(format!("解析 trace 失败: {e}")))?;
    let trace_id = root
        .pointer("/trace/traceId")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let mut records = Vec::new();
    if let Some(spans) = root.get("spans").and_then(|v| v.as_array()) {
        for span in spans {
            if span.get("type").and_then(|t| t.as_str()) == Some("generation") {
                if let Some(record) = extract_generation_record(span) {
                    records.push(record);
                }
            }
        }
    }
    Ok((trace_id, records))
}

fn parse_trace_id(line: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(line)
        .ok()?
        .pointer("/trace/traceId")?
        .as_str()
        .map(str::to_string)
}

/// 从 generation span 折算一条入库记录。
/// 准入规则：span_end 行 + 带可解析 usage 的完整响应。`status == "running"`
/// 的中间检查点跳过（若带 usage，留给最终 span_end 的完整数据）；
/// cancelled/error 但带 usage 的照常入库——token 已实际消耗。
fn extract_generation_record(span: &Value) -> Option<GenerationRecord> {
    if span.get("status").and_then(|s| s.as_str()) == Some("running") {
        return None;
    }
    let span_id = span.get("spanId")?.as_str()?;
    // toolOutput 是"JSON 字符串"：数组里一个聚合后的 chat.completion
    let payload: Value = serde_json::from_str(span.get("toolOutput")?.as_str()?).ok()?;
    let item = payload
        .as_array()
        .and_then(|items| items.iter().find(|item| item.get("usage").is_some()))
        .or_else(|| payload.get("usage").map(|_| &payload))?;
    let usage = item.get("usage")?;
    let input_tokens = usage.get("prompt_tokens")?.as_u64()?;
    let output_tokens = usage.get("completion_tokens")?.as_u64()?;
    let cache_read_tokens = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let model_raw = item.get("model")?.as_str()?;
    if model_raw.is_empty() {
        return None;
    }
    // WorkBuddy 给用户自配模型加 custom-local: 前缀，剥掉后才能对上定价表，
    // 也让同名模型在报表里归并到一起
    let model = model_raw
        .strip_prefix("custom-local:")
        .unwrap_or(model_raw)
        .to_string();
    // 时间戳优先用响应里的 created（服务端秒级）；退回 span.startedAt（RFC3339）
    let created_at = item.get("created").and_then(|v| v.as_i64()).or_else(|| {
        span.get("startedAt")
            .and_then(|v| v.as_str())
            .and_then(parse_timestamp_millis)
            .map(|ms| ms / 1000)
    })?;
    if created_at <= 0 {
        return None;
    }
    Some(GenerationRecord {
        span_id: span_id.to_string(),
        model,
        input_tokens: u32::try_from(input_tokens).unwrap_or(u32::MAX),
        output_tokens: u32::try_from(output_tokens).unwrap_or(u32::MAX),
        cache_read_tokens: u32::try_from(cache_read_tokens).unwrap_or(u32::MAX),
        created_at,
        latency_ms: span.get("duration").and_then(|v| v.as_i64()).unwrap_or(0),
    })
}

fn insert_generation_record(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
    record: GenerationRecord,
    result: &mut SessionSyncResult,
) -> Result<(), AppError> {
    let usage = TokenUsage {
        // prompt_tokens 含缓存读取（OpenAI 口径）
        input_tokens: record.input_tokens,
        // completion_tokens 已含 reasoning，不再叠加
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        model: Some(record.model.clone()),
        message_id: None,
    };
    let cost = find_model_pricing(tx, &record.model)
        .map(|pricing| {
            CostCalculator::calculate_for_app("workbuddy", &usage, &pricing, Decimal::ONE)
                .total_cost
                .to_string()
        })
        .unwrap_or_else(|| "0".into());
    let request_id = format!("wb:{}", record.span_id);
    let changed = tx.execute(
        "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
            total_cost_usd, latency_ms, status_code, session_id, provider_type,
            is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
         ) VALUES (?1, '_workbuddy_session', 'workbuddy', ?2, ?2, ?3, ?4, ?5, 0,
                   '0', '0', '0', '0', ?6, ?7, 200, ?8, 'workbuddy_session', 1, '1', ?9, 'workbuddy_session', ?10)",
        params![
            request_id,
            record.model,
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            cost,
            record.latency_ms,
            session_id,
            record.created_at,
            INPUT_TOKEN_SEMANTICS_TOTAL
        ],
    )?;
    result.imported += changed as u32;
    result.skipped += u32::from(changed == 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn chat_completion(id: &str, model: &str, prompt: u64, completion: u64, cached: u64) -> String {
        serde_json::json!({
            "id": id, "created": 1790000000, "model": model, "object": "chat.completion",
            "choices": [],
            "usage": {
                "prompt_tokens": prompt, "completion_tokens": completion,
                "total_tokens": prompt + completion,
                "prompt_tokens_details": {"cached_tokens": cached},
                "completion_tokens_details": {"reasoning_tokens": 10}
            }
        })
        .to_string()
    }

    fn generation_span_end(span_id: &str, status: &str, output: &str) -> String {
        serde_json::json!({
            "type": "span_end",
            "span": {
                "traceId": "trace_t1", "spanId": span_id, "name": "generation",
                "type": "generation", "status": status,
                "startedAt": "2026-10-25T12:07:55.243Z", "duration": 91416,
                "toolOutput": output
            }
        })
        .to_string()
    }

    fn write_jsonl_trace(dir: &Path, pid: &str, name: &str, lines: &[String]) {
        let sub = dir.join(pid);
        fs::create_dir_all(&sub).unwrap();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(sub.join(name))
            .unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }

    fn row(db: &Database, request_id: &str) -> (i64, i64, i64, i64, i64, i64, String, String) {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens, input_token_semantics,
                    latency_ms, created_at, model, session_id
             FROM proxy_request_logs WHERE request_id = ?1",
            [request_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .unwrap()
    }

    #[test]
    fn workbuddy_jsonl_imports_generation_spans_with_total_semantics() {
        let temp = tempdir().unwrap();
        let lines = vec![
            serde_json::json!({
                "type": "trace_start",
                "trace": {"traceId": "trace_t1", "name": "Agent workflow", "status": "running"}
            })
            .to_string(),
            generation_span_end(
                "span_a",
                "ok",
                &format!(
                    "[{}]",
                    chat_completion("r1", "custom-local:glm-5.3-flash", 59146, 12485, 448)
                ),
            ),
            // running 检查点（即使带 usage）不导入
            generation_span_end(
                "span_b",
                "running",
                &format!("[{}]", chat_completion("r2", "glm-5.3-flash", 100, 10, 0)),
            ),
            // 非 generation span 不导入
            serde_json::json!({
                "type": "span_end",
                "span": {"spanId": "span_c", "type": "function", "status": "ok"}
            })
            .to_string(),
        ];
        write_jsonl_trace(temp.path(), "30040", "trace_t1.jsonl", &lines);

        let db = Database::memory().unwrap();
        let result = sync_workbuddy_usage_from(&db, temp.path()).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.files_scanned, 1);

        // input 原样（含缓存），output 不重复叠加 reasoning，语义为 total，
        // custom-local: 前缀剥掉，session 取 traceId
        let (input, output, cache_read, semantics, latency, created, model, session) =
            row(&db, "wb:span_a");
        assert_eq!(
            (input, output, cache_read, semantics),
            (59146, 12485, 448, INPUT_TOKEN_SEMANTICS_TOTAL)
        );
        assert_eq!((latency, created), (91416, 1790000000));
        assert_eq!(
            (model.as_str(), session.as_str()),
            ("glm-5.3-flash", "trace_t1")
        );

        let conn = db.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE app_type = 'workbuddy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);
        assert_eq!(count, 1);
    }

    #[test]
    fn workbuddy_jsonl_syncs_incrementally_on_append() {
        let temp = tempdir().unwrap();
        let first_lines = vec![
            serde_json::json!({
                "type": "trace_start",
                "trace": {"traceId": "trace_t2"}
            })
            .to_string(),
            generation_span_end(
                "span_1",
                "ok",
                &format!("[{}]", chat_completion("id-1", "glm-5.3-flash", 100, 20, 0)),
            ),
        ];
        write_jsonl_trace(temp.path(), "111", "trace_t2.jsonl", &first_lines);

        let db = Database::memory().unwrap();
        let first = sync_workbuddy_usage_from(&db, temp.path()).unwrap();
        assert_eq!(first.imported, 1);
        // 未变更 → 整文件跳过
        let unchanged = sync_workbuddy_usage_from(&db, temp.path()).unwrap();
        assert_eq!(unchanged.imported, 0);
        assert_eq!(unchanged.files_scanned, 0);
        // 追加新 span → 只导入新增部分
        let appended = vec![generation_span_end(
            "span_2",
            "ok",
            &format!(
                "[{}]",
                chat_completion("id-2", "glm-5.3-flash", 200, 30, 50)
            ),
        )];
        write_jsonl_trace(temp.path(), "111", "trace_t2.jsonl", &appended);
        let result = sync_workbuddy_usage_from(&db, temp.path()).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.files_scanned, 1);
        assert_eq!(row(&db, "wb:span_2").0, 200);
    }

    #[test]
    fn workbuddy_imports_duplicate_response_ids_as_separate_requests() {
        // MiniMax 自定义协议会跨请求复用响应 id；去重键是 spanId，两行都要入库
        let temp = tempdir().unwrap();
        let shared_output = chat_completion("reused-id", "MiniMax-M3.1-Flash", 132669, 280, 0);
        let lines = vec![
            serde_json::json!({"type": "trace_start", "trace": {"traceId": "trace_t3"}})
                .to_string(),
            generation_span_end("span_x", "ok", &format!("[{shared_output}]")),
            generation_span_end("span_y", "ok", &format!("[{shared_output}]")),
        ];
        write_jsonl_trace(temp.path(), "222", "trace_t3.jsonl", &lines);

        let db = Database::memory().unwrap();
        let result = sync_workbuddy_usage_from(&db, temp.path()).unwrap();
        assert_eq!(result.imported, 2);
    }

    #[test]
    fn workbuddy_parses_legacy_json_traces() {
        let temp = tempdir().unwrap();
        let sub = temp.path().join("9980");
        fs::create_dir_all(&sub).unwrap();
        let legacy = serde_json::json!({
            "trace": {"traceId": "trace_legacy", "sessionId": "real-session",
                       "status": "ok", "modelInfo": {"callCount": 43}},
            "spans": [
                {"spanId": "span_l1", "type": "generation", "status": "ok",
                 "startedAt": "2026-10-02T15:09:20.234Z", "duration": 10158,
                 "toolOutput": format!("[{}]", chat_completion("legacy-1", "space-bunny", 31741, 513, 27975))},
                {"spanId": "span_l2", "type": "custom", "status": "ok"}
            ]
        });
        fs::write(sub.join("trace_legacy.json"), legacy.to_string()).unwrap();

        let db = Database::memory().unwrap();
        let first = sync_workbuddy_usage_from(&db, temp.path()).unwrap();
        let second = sync_workbuddy_usage_from(&db, temp.path()).unwrap();
        assert_eq!(first.imported, 1);
        assert_eq!(second.imported, 0);
        assert_eq!(second.files_scanned, 0);

        // 旧格式的 session 标识同样取 traceId，保持两种格式一致
        let (_, _, cache_read, _, _, _, _, session) = row(&db, "wb:span_l1");
        assert_eq!(cache_read, 27975);
        assert_eq!(session, "trace_legacy");
    }

    #[test]
    fn workbuddy_resolves_cost_from_model_pricing() {
        let temp = tempdir().unwrap();
        let lines = vec![
            serde_json::json!({"type": "trace_start", "trace": {"traceId": "trace_t4"}})
                .to_string(),
            generation_span_end(
                "span_p",
                "ok",
                &format!(
                    "[{}]",
                    chat_completion("p1", "custom-local:test-model", 200, 100, 0)
                ),
            ),
        ];
        write_jsonl_trace(temp.path(), "333", "trace_t4.jsonl", &lines);

        let db = Database::memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million,
                                            output_cost_per_million,
                                            cache_read_cost_per_million, cache_creation_cost_per_million)
                 VALUES ('test-model', 'Test Model', '1', '2', '0.1', '1')",
                [],
            )
            .unwrap();
        }
        sync_workbuddy_usage_from(&db, temp.path()).unwrap();
        let conn = db.conn.lock().unwrap();
        // 前缀剥掉后命中定价：200×1/1M + 100×2/1M = 0.0004
        let cost: String = conn
            .query_row(
                "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'wb:span_p'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cost, "0.0004");
    }
}

#[cfg(test)]
mod native_validation {
    use super::*;

    #[test]
    #[ignore = "requires the real WorkBuddy traces directory on this machine"]
    fn workbuddy_reads_real_traces_and_stays_idempotent() -> Result<(), AppError> {
        let path = workbuddy_traces_dir();
        if !path.is_dir() {
            eprintln!("WorkBuddy traces 目录不存在，跳过: {}", path.display());
            return Ok(());
        }
        let db = Database::memory()?;
        let first = sync_workbuddy_usage(&db)?;
        assert!(
            first.imported > 0,
            "真实 traces 应有可导入的 generation span，实际 {}",
            first.imported
        );
        assert_eq!(sync_workbuddy_usage(&db)?.imported, 0, "第二次同步必须幂等");
        let conn = lock_conn!(db.conn);
        let (rows, input, output, cache_read): (i64, i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cache_read_tokens), 0)
             FROM proxy_request_logs WHERE app_type = 'workbuddy'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        eprintln!("真实数据: {rows} 行, input={input}, output={output}, cache_read={cache_read}");
        Ok(())
    }
}
