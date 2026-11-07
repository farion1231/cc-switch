//! LiveAgent（K-brain 后端）会话日志用量导入。
//!
//! K-brain 把每个会话写成纯 JSONL：
//! - 不属于项目的会话：`<数据目录>/sessions/<会话 id>/session.jsonl`
//! - 项目会话：`<数据目录>/sessions/<项目名-哈希>/<会话 id>/session.jsonl`
//!
//! 每行 `{"type": ..., "payload": {...}}`。用量挂在两类记录上：
//!
//! ```json
//! {"type":"session_meta","payload":{"id":"...","provider":"<供应商 id>","model":"..."}}
//! {"type":"message","payload":{"seq":3,"message":{"id":"...","role":"assistant","model":"...",
//!   "sent_at":"2026-09-18T19:23:52.839Z",
//!   "usage":{"prompt_tokens":1200,"completion_tokens":80,"prompt_cache_hit_tokens":900}}}}
//! {"type":"compaction","payload":{"seq":9,"model":"...","usage":{...}}}
//! ```
//!
//! K-brain 在写入前把各家协议的用量归一成 OpenAI 形状，`prompt_tokens` 是**含缓存**
//! 的总输入（Anthropic 的 input + cache_read + cache_creation 会被加回去）。缓存命中取
//! `prompt_tokens_details.cached_tokens`，没有时取 `prompt_cache_hit_tokens`，缓存写入取
//! `prompt_cache_write_tokens`（与 K-brain `ai.Usage.Cached()` / `CacheWrite()` 一致）。
//! 这里扣掉缓存部分，按 fresh-input 语义入库。
//!
//! `session_meta.provider` 就是 LiveAgent 的供应商 id，和 CC Switch 里 LiveAgent
//! 供应商卡片的 id 相同，所以直接记在 `provider_id` 上，用量面板能显示真实供应商名。
//!
//! 有 `import_source_id` 的会话是 K-brain 从 LiveAgent 自己的旧版聊天记录
//! （`chat-history.sqlite3`）迁移过来的，不是从别的应用导入，照常计入。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::{
    session_usage::{
        estimated_latency_ms, load_sync_cursors, metadata_modified_nanos, SessionSyncResult,
        SyncCursor,
    },
    sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH,
    usage_stats::find_model_pricing,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// 写入 `proxy_request_logs.app_type` 的应用标识。
pub(crate) const LIVEAGENT_APP_TYPE: &str = "liveagent";
/// `session_usage_dedup` / `proxy_request_logs.data_source` 里的来源标识。
const LIVEAGENT_DATA_SOURCE: &str = "liveagent_session";
/// 会话没记供应商时的占位供应商 ID。
const LIVEAGENT_PROVIDER_ID: &str = "_liveagent_session";
/// `provider_type`：详情面板里标明这条记录来自会话日志导入。
const LIVEAGENT_PROVIDER_TYPE: &str = "liveagent_session";
const SESSION_FILE: &str = "session.jsonl";

/// 跨批次需要记住的上下文：续读时文件头的 `session_meta` 已经读过了。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ParserState {
    provider: Option<String>,
    model: Option<String>,
    /// 上一条消息（用户、工具）的发送时间，用来估算这次模型调用的耗时。
    last_sent_at_ms: Option<i64>,
}

struct PendingRequest {
    request_id: String,
    provider_id: String,
    model: String,
    usage: TokenUsage,
    created_at_ms: i64,
    latency_ms: i64,
    session_id: String,
}

/// 扫描并导入 LiveAgent 会话用量（后台定时任务与手动同步共用）。
pub fn sync_liveagent_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    match liveagent_sessions_root() {
        Some(root) => sync_liveagent_usage_from(db, &root),
        None => Ok(SessionSyncResult::default()),
    }
}

/// 会话根目录：与 CC Switch 读写 LiveAgent 配置用同一套数据目录规则
/// （`LIVEAGENT_HOME` / `K_BRAIN_HOME` / `data-dir.json` / `~/.liveagent`）。
pub(crate) fn liveagent_sessions_root() -> Option<PathBuf> {
    crate::liveagent_config::data_dir()
        .ok()
        .map(|dir| dir.join("sessions"))
}

/// 列出根目录下所有会话日志：`<id>/session.jsonl` 和 `<项目>/<id>/session.jsonl`。
pub(crate) fn session_logs(root: &Path) -> Vec<(String, PathBuf)> {
    let mut logs = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return logs;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let direct = dir.join(SESSION_FILE);
        if direct.is_file() {
            logs.push((entry.file_name().to_string_lossy().into_owned(), direct));
            continue;
        }
        let Ok(sessions) = fs::read_dir(&dir) else {
            continue;
        };
        for session in sessions.flatten() {
            let path = session.path().join(SESSION_FILE);
            if path.is_file() {
                logs.push((session.file_name().to_string_lossy().into_owned(), path));
            }
        }
    }
    logs.sort_by(|a, b| a.1.cmp(&b.1));
    logs
}

/// [`sync_liveagent_usage`] 的可注入根目录版本，测试用临时目录。
pub(crate) fn sync_liveagent_usage_from(
    db: &Database,
    root: &Path,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let cursors = load_sync_cursors(db)?;
    for (session_id, path) in session_logs(root) {
        let key = format!("liveagent:{}", path.to_string_lossy());
        match sync_file(db, &path, &session_id, &key, &cursors) {
            Ok(part) => result.merge(part),
            Err(error) => result
                .errors
                .push(format!("LiveAgent 会话 {}: {error}", path.display())),
        }
    }
    Ok(result)
}

fn sync_file(
    db: &Database,
    path: &Path,
    session_id: &str,
    key: &str,
    cursors: &HashMap<String, SyncCursor>,
) -> Result<SessionSyncResult, AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    let modified = metadata_modified_nanos(&metadata);
    let size = metadata.len() as i64;
    let cursor = cursors.get(key);
    if cursor.is_some_and(|cursor| {
        cursor.last_modified == modified
            && cursor.last_byte_offset == Some(size)
            && cursor.parser_state.is_some()
    }) {
        return Ok(SessionSyncResult::default());
    }

    // 会话日志只追加：从上次处理到的位置续读。文件变短说明被重写，从头再扫
    // （去重账本保证不会重复入库）。
    let resume = cursor
        .filter(|cursor| cursor.parser_state.is_some())
        .and_then(|cursor| cursor.last_byte_offset)
        .filter(|offset| (0..=size).contains(offset));
    let (start, mut state) = match resume {
        Some(offset) => (
            offset,
            cursor
                .and_then(|cursor| cursor.parser_state.as_deref())
                .and_then(|raw| serde_json::from_str::<ParserState>(raw).ok())
                .unwrap_or_default(),
        ),
        None => (0, ParserState::default()),
    };
    let (text, end) = read_complete_lines(path, start)?;
    if text.is_empty() {
        return Ok(SessionSyncResult {
            files_scanned: 1,
            ..Default::default()
        });
    }
    let pending = collect_requests(&text, session_id, &mut state);
    let parser_state =
        serde_json::to_string(&state).map_err(|error| AppError::Config(error.to_string()))?;

    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for request in pending {
        // 先记长期收据：分叉会话会把父会话的消息原样抄过去，按消息 id 只记一次。
        if tx.execute(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id)
             VALUES (?1, ?2, ?2, 1)",
            rusqlite::params![LIVEAGENT_DATA_SOURCE, request.request_id],
        )? == 0
        {
            result.skipped += 1;
            continue;
        }
        let amounts = costs(&tx, &request.model, &request.usage);
        tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model, pricing_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, status_code, session_id, provider_type, is_streaming,
                cost_multiplier, created_at, data_source, input_token_semantics
            ) VALUES (?1, ?2, ?3, ?4, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL, 200, ?15, ?16, 1, '1', ?17, ?18, ?19)",
            rusqlite::params![
                request.request_id,
                request.provider_id,
                LIVEAGENT_APP_TYPE,
                request.model,
                request.usage.input_tokens,
                request.usage.output_tokens,
                request.usage.cache_read_tokens,
                request.usage.cache_creation_tokens,
                amounts[0],
                amounts[1],
                amounts[2],
                amounts[3],
                amounts[4],
                request.latency_ms,
                request.session_id,
                LIVEAGENT_PROVIDER_TYPE,
                request.created_at_ms / 1000,
                LIVEAGENT_DATA_SOURCE,
                INPUT_TOKEN_SEMANTICS_FRESH,
            ],
        )?;
        result.imported += 1;
    }
    update_sync_state(&tx, key, modified, size, end, &parser_state)?;
    tx.commit()?;
    Ok(result)
}

fn update_sync_state(
    conn: &rusqlite::Connection,
    file_path: &str,
    last_modified: i64,
    file_size: i64,
    byte_offset: i64,
    parser_state: &str,
) -> Result<(), AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    conn.prepare_cached(
        "INSERT OR REPLACE INTO session_log_sync
             (file_path, last_modified, last_line_offset, last_synced_at, last_byte_offset, parser_state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .and_then(|mut stmt| {
        stmt.execute(rusqlite::params![
            file_path,
            last_modified,
            file_size,
            now,
            byte_offset,
            parser_state
        ])
    })
    .map_err(|error| AppError::Database(format!("更新同步状态失败: {error}")))?;
    Ok(())
}

/// 从 `start` 读到最后一个完整行，返回文本和它在文件里的结束位置；写了一半的
/// 末行留给下一轮。
fn read_complete_lines(path: &Path, start: i64) -> Result<(Vec<u8>, i64), AppError> {
    let mut file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    if start > 0 {
        file.seek(SeekFrom::Start(start as u64))
            .map_err(|error| AppError::io(path, error))?;
    }
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)
        .map_err(|error| AppError::io(path, error))?;
    let end = tail
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    tail.truncate(end);
    Ok((tail, start + end as i64))
}

fn collect_requests(text: &[u8], session_id: &str, state: &mut ParserState) -> Vec<PendingRequest> {
    let mut requests = Vec::new();
    for line in text.split(|byte| *byte == b'\n') {
        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let payload = &value["payload"];
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                // 后面的 session_meta 会更新前面的（换模型、换供应商）。
                if let Some(provider) = non_empty(payload.get("provider")) {
                    state.provider = Some(provider.to_owned());
                }
                if let Some(model) = model_name(payload.get("model")) {
                    state.model = Some(model.to_owned());
                }
            }
            Some("message") => {
                let message = &payload["message"];
                let sent_at = message
                    .get("sent_at")
                    .and_then(Value::as_str)
                    .and_then(parse_time_ms);
                if message.get("role").and_then(Value::as_str) == Some("assistant") {
                    let id = non_empty(message.get("id"))
                        .map(str::to_owned)
                        .unwrap_or_else(|| line_fingerprint(line));
                    if let Some(request) = request_from(
                        &message["usage"],
                        format!("liveagent:{id}"),
                        model_name(message.get("model")),
                        sent_at.or(state.last_sent_at_ms),
                        session_id,
                        state,
                    ) {
                        requests.push(request);
                    }
                }
                if sent_at.is_some() {
                    state.last_sent_at_ms = sent_at;
                }
            }
            Some("compaction") => {
                // 压缩上下文本身也是一次模型调用；记录没有时间，用上一条消息的时间。
                let seq = payload.get("seq").and_then(Value::as_u64).unwrap_or(0);
                if let Some(mut request) = request_from(
                    &payload["usage"],
                    format!(
                        "liveagent:compaction:{session_id}:{seq}:{}",
                        line_fingerprint(line)
                    ),
                    model_name(payload.get("model")),
                    state.last_sent_at_ms,
                    session_id,
                    state,
                ) {
                    request.latency_ms = 0;
                    requests.push(request);
                }
            }
            _ => {}
        }
    }
    requests
}

fn request_from(
    usage: &Value,
    request_id: String,
    model: Option<&str>,
    created_at_ms: Option<i64>,
    session_id: &str,
    state: &ParserState,
) -> Option<PendingRequest> {
    let usage = token_usage(usage)?;
    let created_at_ms = created_at_ms?;
    let model = model
        .map(str::to_owned)
        .or_else(|| state.model.clone())
        .unwrap_or_else(|| "unknown".to_owned());
    let latency_ms = state
        .last_sent_at_ms
        .and_then(|start| estimated_latency_ms(start, created_at_ms))
        .unwrap_or(0);
    Some(PendingRequest {
        request_id,
        provider_id: state
            .provider
            .clone()
            .unwrap_or_else(|| LIVEAGENT_PROVIDER_ID.to_owned()),
        usage: TokenUsage {
            model: Some(model.clone()),
            ..usage
        },
        model,
        created_at_ms,
        latency_ms,
        session_id: session_id.to_owned(),
    })
}

/// K-brain 归一后的用量 → fresh-input 语义的 [`TokenUsage`]；全零返回 `None`。
fn token_usage(usage: &Value) -> Option<TokenUsage> {
    if !usage.is_object() {
        return None;
    }
    let prompt = token_count(usage.get("prompt_tokens"));
    let output = token_count(usage.get("completion_tokens"));
    let details = token_count(usage.pointer("/prompt_tokens_details/cached_tokens"));
    let cache_read = if details > 0 {
        details
    } else {
        token_count(usage.get("prompt_cache_hit_tokens"))
    };
    let cache_write = token_count(usage.get("prompt_cache_write_tokens"));
    if prompt == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        return None;
    }
    Some(TokenUsage {
        input_tokens: prompt
            .saturating_sub(cache_read)
            .saturating_sub(cache_write),
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_write,
        cache_creation_1h_tokens: 0,
        model: None,
        message_id: None,
    })
}

fn non_empty(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// K-brain 记的模型是 `<模型> @ <供应商>`（如 `deepseek-flash @ openai`），计价和
/// 模型分组只要前半段。
fn model_name(value: Option<&Value>) -> Option<&str> {
    non_empty(value)
        .and_then(|model| model.split(" @ ").next())
        .map(str::trim)
        .filter(|model| !model.is_empty())
}

fn parse_time_ms(raw: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|time| time.timestamp_millis())
}

fn line_fingerprint(line: &[u8]) -> String {
    format!("{:x}", Sha256::digest(line))
}

fn token_count(value: Option<&Value>) -> u32 {
    value
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u64::from(u32::MAX)) as u32
}

/// 按模型定价算五项费用；没有定价的模型记 0（与其余会话导入器一致）。
fn costs(conn: &rusqlite::Connection, model: &str, usage: &TokenUsage) -> [String; 5] {
    find_model_pricing(conn, model)
        .map(|pricing| {
            let cost = CostCalculator::calculate_for_app(
                LIVEAGENT_APP_TYPE,
                usage,
                &pricing,
                Decimal::ONE,
            );
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
    use serde_json::json;

    fn meta(provider: &str, model: &str) -> Value {
        json!({"type":"session_meta","payload":{"id":"s","provider":provider,"model":model,
            "created_at":"2026-09-18T19:23:34.963Z","import_source_id":""}})
    }
    fn user(id: &str, at: &str) -> Value {
        json!({"type":"message","payload":{"seq":1,"message":{"id":id,"role":"user","sent_at":at,"content":"hi"}}})
    }
    fn assistant(id: &str, at: &str, model: &str, usage: Value) -> Value {
        json!({"type":"message","payload":{"seq":2,"message":{"id":id,"role":"assistant",
            "model":model,"sent_at":at,"usage":usage,"content":"ok"}}})
    }
    fn write(path: &Path, lines: &[Value]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            lines
                .iter()
                .map(|line| format!("{line}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }
    fn append(path: &Path, lines: &[Value]) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }
    type Row = (String, String, String, i64, i64, i64, i64, i64, i64);
    fn rows(db: &Database) -> Vec<Row> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT request_id, provider_id, model, input_tokens, output_tokens,
                        cache_read_tokens, cache_creation_tokens, created_at, latency_ms
                 FROM proxy_request_logs WHERE app_type = 'liveagent' ORDER BY request_id",
            )
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
                row.get(7)?,
                row.get(8)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    #[test]
    fn liveagent_usage_reads_both_layouts_and_normalizes_cache() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 不属于项目的会话；DeepSeek 风格的 prompt_cache_hit_tokens
        write(
            &root.join("aaa").join(SESSION_FILE),
            &[
                meta("prov-1", "deepseek-flash"),
                user("u1", "2026-09-18T19:23:50.000Z"),
                assistant(
                    "a1",
                    "2026-09-18T19:23:52.500Z",
                    "deepseek-flash",
                    json!({"prompt_tokens":1000,"completion_tokens":50,"prompt_cache_hit_tokens":900}),
                ),
                // 没有用量的助手消息不入库
                json!({"type":"message","payload":{"message":{"id":"a0","role":"assistant","sent_at":"2026-09-18T19:24:00Z"}}}),
            ],
        );
        // 项目会话；Anthropic 归一后的 cached_tokens + 缓存写入；模型取 meta
        write(
            &root.join("proj-1234").join("bbb").join(SESSION_FILE),
            &[
                meta("prov-2", "claude-sonnet"),
                user("u2", "2026-09-18T20:00:00Z"),
                json!({"type":"message","payload":{"message":{"id":"a2","role":"assistant",
                    "sent_at":"2026-09-18T20:00:03Z",
                    "usage":{"prompt_tokens":500,"completion_tokens":20,
                        "prompt_tokens_details":{"cached_tokens":300},"prompt_cache_write_tokens":100}}}}),
                json!({"type":"compaction","payload":{"seq":5,"model":"claude-sonnet",
                    "usage":{"prompt_tokens":40,"completion_tokens":10}}}),
                "not json".into(),
            ],
        );
        // 不是会话的东西：忽略
        fs::create_dir_all(root.join("backend-events")).unwrap();
        fs::write(root.join("backend-events").join("x.runs.json"), "{}").unwrap();
        fs::write(root.join("planning.json"), "{}").unwrap();

        let result = sync_liveagent_usage_from(&db, root).unwrap();
        assert_eq!(result.imported, 3, "{:?}", result.errors);
        assert!(result.errors.is_empty());
        let rows = rows(&db);
        let a1 = rows.iter().find(|r| r.0 == "liveagent:a1").unwrap();
        assert_eq!(
            (&a1.1[..], &a1.2[..], a1.3, a1.4, a1.5, a1.6),
            ("prov-1", "deepseek-flash", 100, 50, 900, 0)
        );
        assert_eq!(a1.8, 2500, "latency from the previous message");
        let a2 = rows.iter().find(|r| r.0 == "liveagent:a2").unwrap();
        assert_eq!(
            (&a2.1[..], &a2.2[..], a2.3, a2.5, a2.6),
            ("prov-2", "claude-sonnet", 100, 300, 100)
        );
        let compaction = rows
            .iter()
            .find(|r| r.0.starts_with("liveagent:compaction:bbb:5"))
            .unwrap();
        assert_eq!((compaction.3, compaction.4), (40, 10));
        assert_eq!(compaction.7, a2.7, "compaction uses the last message time");

        // 重复同步不重复计数
        let again = sync_liveagent_usage_from(&db, root).unwrap();
        assert_eq!(again.imported, 0);
        assert_eq!(super::tests::rows(&db).len(), 3);
    }

    #[test]
    fn liveagent_usage_resumes_appended_lines_and_keeps_meta() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ccc").join(SESSION_FILE);
        write(
            &path,
            &[
                meta("prov-1", "m"),
                user("u1", "2026-09-18T19:00:00Z"),
                assistant(
                    "a1",
                    "2026-09-18T19:00:01Z",
                    "m",
                    json!({"prompt_tokens":10,"completion_tokens":1}),
                ),
            ],
        );
        assert_eq!(
            sync_liveagent_usage_from(&db, dir.path()).unwrap().imported,
            1
        );
        // 追加：一行完整、一行写了一半
        append(
            &path,
            &[
                user("u2", "2026-09-18T19:01:00Z"),
                assistant(
                    "a2",
                    "2026-09-18T19:01:04Z",
                    "",
                    json!({"prompt_tokens":20,"completion_tokens":2}),
                ),
            ],
        );
        {
            use std::io::Write;
            let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
            write!(file, "{{\"type\":\"message\",\"payl").unwrap();
        }
        let result = sync_liveagent_usage_from(&db, dir.path()).unwrap();
        assert_eq!(result.imported, 1, "{:?}", result.errors);
        let a2 = rows(&db)
            .into_iter()
            .find(|r| r.0 == "liveagent:a2")
            .unwrap();
        // 续读时 meta 不在新增部分里，仍要记到同一个供应商和模型上
        assert_eq!((&a2.1[..], &a2.2[..], a2.8), ("prov-1", "m", 4000));
    }

    #[test]
    fn liveagent_usage_counts_forked_messages_once_and_handles_missing_root() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let line = assistant(
            "same",
            "2026-09-18T19:00:01Z",
            "m",
            json!({"prompt_tokens":10,"completion_tokens":1}),
        );
        write(
            &dir.path().join("p1").join(SESSION_FILE),
            std::slice::from_ref(&line),
        );
        write(
            &dir.path().join("proj").join("p2").join(SESSION_FILE),
            &[line],
        );
        let result = sync_liveagent_usage_from(&db, dir.path()).unwrap();
        assert_eq!((result.imported, result.skipped), (1, 1));
        // 没有会话的供应商记到占位 id
        assert_eq!(rows(&db)[0].1, LIVEAGENT_PROVIDER_ID);

        let missing = sync_liveagent_usage_from(&db, &dir.path().join("nope")).unwrap();
        assert_eq!(missing.imported, 0);
        assert!(missing.errors.is_empty());
    }

    #[test]
    fn liveagent_usage_prices_known_models() {
        let db = Database::memory().unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO model_pricing (model_id, display_name, input_cost_per_million,
                    output_cost_per_million, cache_read_cost_per_million, cache_creation_cost_per_million)
                 VALUES ('la-priced', 'Priced', '1', '2', '0.1', '0')",
                [],
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("p").join(SESSION_FILE),
            &[assistant(
                "a",
                "2026-09-18T19:00:01Z",
                "la-priced @ some-provider",
                json!({"prompt_tokens":2_000_000,"completion_tokens":1_000_000,"prompt_cache_hit_tokens":1_000_000}),
            )],
        );
        sync_liveagent_usage_from(&db, dir.path()).unwrap();
        let total: String = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'liveagent:a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // `<模型> @ <供应商>` 按模型名计价：1M fresh × $1 + 1M output × $2 + 1M cache read × $0.1
        assert_eq!(total.parse::<f64>().unwrap(), 3.1);
    }
}
