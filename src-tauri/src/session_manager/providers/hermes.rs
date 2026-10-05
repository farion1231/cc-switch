use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use crate::hermes_config::get_hermes_dir;
use crate::session_manager::model::{ContentRef, SessionBlock, ToolStatus};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{assign_turn_ids, openai_tool_calls, thinking_block, tool_result_block};
use super::utils::for_each_jsonl_value;
use super::utils::{
    extract_text, parse_timestamp_to_ms, read_head_tail_lines, truncate_summary, TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "hermes";

fn get_hermes_db_path() -> PathBuf {
    get_hermes_dir().join("state.db")
}

fn get_hermes_sessions_dir() -> PathBuf {
    get_hermes_dir().join("sessions")
}

/// Scan sessions from both SQLite database and JSONL transcript files,
/// with SQLite taking precedence on ID conflicts.
pub fn scan_sessions() -> Vec<SessionMeta> {
    let sqlite_sessions = scan_sessions_sqlite();
    let jsonl_sessions = scan_sessions_jsonl();

    if sqlite_sessions.is_empty() {
        return jsonl_sessions;
    }
    if jsonl_sessions.is_empty() {
        return sqlite_sessions;
    }

    let sqlite_ids: std::collections::HashSet<String> = sqlite_sessions
        .iter()
        .map(|s| s.session_id.clone())
        .collect();

    let mut merged = sqlite_sessions;
    for s in jsonl_sessions {
        if !sqlite_ids.contains(&s.session_id) {
            merged.push(s);
        }
    }
    merged
}

// ── SQLite scanning ─────────────────────────────────────────────────

/// Newest sessions listed from `state.db`; the first-user-message lookup is
/// scoped to the same window so the cap also bounds that query.
const SQLITE_SCAN_LIMIT: usize = 500;

/// Hermes stores non-string message content (text + image parts, …) as this
/// prefix followed by JSON (`SessionDB._encode_content`).
const CONTENT_JSON_PREFIX: &str = "\u{0}json:";

fn scan_sessions_sqlite() -> Vec<SessionMeta> {
    scan_sessions_sqlite_at(&get_hermes_db_path())
}

fn scan_sessions_sqlite_at(db_path: &Path) -> Vec<SessionMeta> {
    if !db_path.exists() {
        return Vec::new();
    }

    let conn = match Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Check if sessions table exists
    let has_sessions: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='sessions'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !has_sessions {
        return Vec::new();
    }

    // Query sessions — use flexible column access via pragma
    let columns = get_table_columns(&conn, "sessions");

    let query = format!("SELECT * FROM sessions ORDER BY rowid DESC LIMIT {SQLITE_SCAN_LIMIT}");
    let mut stmt = match conn.prepare(&query) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut sessions = Vec::new();
    let rows = match stmt.query_map([], |row| Ok(row_to_json(row, &columns))) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let db_source = format!("sqlite:{}", db_path.display());
    let first_user_messages = first_user_messages(&conn);
    let last_messages = last_messages(&conn);

    for row_result in rows.flatten() {
        if let Some(mut meta) = sqlite_row_to_session_meta(&row_result, &db_source) {
            let first = first_user_messages.get(&meta.session_id);
            if meta.title.is_none() {
                if let Some(text) = first {
                    meta.title = Some(truncate_summary(text, TITLE_MAX_CHARS));
                }
            }
            // 列表上标的是「最后：」，和其他应用一样取最后一条，而不是开头那句
            if let Some(text) = last_messages.get(&meta.session_id).or(first) {
                meta.summary = Some(truncate_summary(text, 160));
            }
            sessions.push(meta);
        }
    }

    sessions
}

/// First displayable user message of each listed session, decoded to text.
/// Hermes auto-titles most sessions, so this is the title only when
/// `sessions.title` is empty (and the summary fallback when a session has no
/// readable last message).
fn first_user_messages(conn: &Connection) -> HashMap<String, String> {
    edge_messages(conn, "MIN", "role = 'user'")
}

/// Last displayable user / assistant message of each listed session: what the
/// list shows after "Last:", same as the other apps' session lists.
fn last_messages(conn: &Connection) -> HashMap<String, String> {
    edge_messages(
        conn,
        "MAX",
        "role IN ('user', 'assistant') AND content IS NOT NULL AND content != ''",
    )
}

fn edge_messages(conn: &Connection, pick: &str, role_filter: &str) -> HashMap<String, String> {
    let columns = get_table_columns(conn, "messages");
    if columns.is_empty() {
        return HashMap::new();
    }
    let query = format!(
        "SELECT m.session_id, m.content FROM messages m \
         JOIN (SELECT session_id, {pick}(id) AS edge_id FROM messages \
               WHERE {role_filter}{filter} \
                 AND session_id IN (SELECT id FROM sessions ORDER BY rowid DESC LIMIT {SQLITE_SCAN_LIMIT}) \
               GROUP BY session_id) f ON m.id = f.edge_id",
        filter = display_filter(&columns),
    );
    let mut stmt = match conn.prepare(&query) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let rows = match stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    }) {
        Ok(r) => r,
        Err(_) => return HashMap::new(),
    };
    rows.flatten()
        .filter_map(|(session_id, content)| {
            let text = decode_content(content.as_deref()?);
            (!text.trim().is_empty()).then_some((session_id, text))
        })
        .collect()
}

/// Rows Hermes itself shows in a transcript: live rows plus rows archived by
/// context compression (`compacted = 1`), but not rows hidden by
/// undo/rewind/regenerate (`active = 0, compacted = 0`). Older stores predate
/// these columns and show everything.
fn display_filter(columns: &[String]) -> &'static str {
    let has = |name: &str| columns.iter().any(|c| c == name);
    match (has("active"), has("compacted")) {
        (true, true) => " AND (active = 1 OR compacted = 1)",
        (true, false) => " AND active = 1",
        _ => "",
    }
}

/// Undo `SessionDB._encode_content`: structured content becomes its text parts.
fn decode_content(raw: &str) -> String {
    match raw.strip_prefix(CONTENT_JSON_PREFIX) {
        Some(json) => match serde_json::from_str::<Value>(json) {
            Ok(value) => extract_text(&value),
            Err(_) => raw.to_string(),
        },
        None => raw.to_string(),
    }
}

fn sqlite_row_to_session_meta(row: &Value, db_source: &str) -> Option<SessionMeta> {
    let obj = row.as_object()?;

    let session_id = obj.get("id").and_then(Value::as_str)?.to_string();

    let title = obj
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_summary(s, TITLE_MAX_CHARS).to_string());

    let cwd = obj
        .get("cwd")
        .or_else(|| obj.get("directory"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let started_at = obj
        .get("started_at")
        .or_else(|| obj.get("created_at"))
        .and_then(parse_timestamp_to_ms);

    let ended_at = obj
        .get("last_activity_at")
        .filter(|v| !v.is_null())
        .or_else(|| obj.get("ended_at"))
        .or_else(|| obj.get("updated_at"))
        .and_then(parse_timestamp_to_ms);

    let source_path = format!("{}#{}", db_source, session_id);

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id,
        title,
        summary: None,
        project_dir: cwd,
        created_at: started_at,
        last_active_at: ended_at.or(started_at),
        source_path: Some(source_path),
        resume_command: None,
    })
}

/// Get column names for a table.
fn get_table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let query = format!("PRAGMA table_info({table})");
    let mut stmt = match conn.prepare(&query) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map([], |row| {
        let name: String = row.get(1)?;
        Ok(name)
    }) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.flatten().collect()
}

/// Convert a SQLite row to a JSON Value using known column names.
fn row_to_json(row: &rusqlite::Row, columns: &[String]) -> Value {
    let mut map = serde_json::Map::new();
    for (i, col) in columns.iter().enumerate() {
        // Try string first, then integer, then float, then null
        if let Ok(val) = row.get::<_, String>(i) {
            map.insert(col.clone(), Value::String(val));
        } else if let Ok(val) = row.get::<_, i64>(i) {
            map.insert(col.clone(), Value::Number(val.into()));
        } else if let Ok(val) = row.get::<_, f64>(i) {
            if let Some(n) = serde_json::Number::from_f64(val) {
                map.insert(col.clone(), Value::Number(n));
            }
        } else {
            map.insert(col.clone(), Value::Null);
        }
    }
    Value::Object(map)
}

/// Load messages from the Hermes SQLite database.
pub fn load_messages_sqlite(source: &str) -> Result<Vec<SessionMessage>, String> {
    let (db_path, session_id) = parse_sqlite_source(source)
        .ok_or_else(|| format!("Invalid SQLite source reference: {source}"))?;

    let conn = Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open Hermes database: {e}"))?;

    load_messages_from_conn(&conn, &session_id)
}

type DedupeKey = (
    String,
    Option<String>,
    Option<u64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

struct MessageRow {
    id: i64,
    role: String,
    content: Option<String>,
    timestamp: Option<f64>,
    tool_call_id: Option<String>,
    tool_calls: Option<String>,
    tool_name: Option<String>,
    active: i64,
    display_metadata: Option<String>,
    /// 助手的推理文本（较新的 Hermes 才有该列，待核实）
    reasoning: Option<String>,
}

impl MessageRow {
    /// Hermes' display identity (`SessionDB._display_dedupe_key`): compression
    /// re-inserts protected head/tail rows with their original timestamp, so
    /// the archived original and the live copy are the same message.
    fn dedupe_key(&self) -> DedupeKey {
        (
            self.role.clone(),
            self.content.clone(),
            self.timestamp.map(f64::to_bits),
            self.tool_call_id.clone(),
            self.tool_calls.clone(),
            self.tool_name.clone(),
        )
    }

    /// Rows the model reads but nobody typed as one message (micro-compaction
    /// merges) carry `display_metadata.model_only`; their sources stay visible.
    fn is_model_only(&self) -> bool {
        let Some(raw) = self.display_metadata.as_deref() else {
            return false;
        };
        let mut meta: Value = Value::String(raw.to_string());
        // Pre-guard rows are double-encoded.
        for _ in 0..2 {
            if let Value::String(s) = &meta {
                meta = match serde_json::from_str(s) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
            }
        }
        match meta.get("model_only") {
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_f64() != Some(0.0),
            Some(Value::String(s)) => !s.is_empty(),
            _ => false,
        }
    }
}

fn load_messages_from_conn(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<SessionMessage>, String> {
    let columns = get_table_columns(conn, "messages");
    let col = |name: &str| {
        if columns.iter().any(|c| c == name) {
            name.to_string()
        } else {
            format!("NULL AS {name}")
        }
    };
    // Insertion order, as Hermes reads it: timestamps can regress (clock skew,
    // compaction re-inserting rows with their original time).
    let query = format!(
        "SELECT id, role, content, {timestamp}, {tool_call_id}, {tool_calls}, {tool_name}, \
                {active}, {display_metadata}, {reasoning} \
         FROM messages WHERE session_id = ?1{filter} ORDER BY id ASC",
        timestamp = col("timestamp"),
        tool_call_id = col("tool_call_id"),
        tool_calls = col("tool_calls"),
        tool_name = col("tool_name"),
        active = col("active"),
        display_metadata = col("display_metadata"),
        reasoning = col("reasoning"),
        filter = display_filter(&columns),
    );

    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| format!("Failed to prepare messages query: {e}"))?;

    let rows = stmt
        .query_map([session_id], |row| {
            Ok(MessageRow {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2).ok().flatten(),
                timestamp: row.get(3).ok().flatten(),
                tool_call_id: row.get(4).ok().flatten(),
                tool_calls: row.get(5).ok().flatten(),
                tool_name: row.get(6).ok().flatten(),
                active: row.get::<_, Option<i64>>(7).ok().flatten().unwrap_or(1),
                display_metadata: row.get(8).ok().flatten(),
                reasoning: row.get(9).ok().flatten(),
            })
        })
        .map_err(|e| format!("Failed to query messages: {e}"))?;

    // Collapse duplicates the way Hermes' display projection does: keep the
    // first position, show the most live copy.
    let mut order: Vec<MessageRow> = Vec::new();
    let mut index: HashMap<DedupeKey, usize> = HashMap::new();
    for row in rows.flatten() {
        if row.is_model_only() {
            continue;
        }
        match index.entry(row.dedupe_key()) {
            Entry::Occupied(slot) => {
                let kept = &mut order[*slot.get()];
                if (row.active, row.id) > (kept.active, kept.id) {
                    *kept = row;
                }
            }
            Entry::Vacant(slot) => {
                slot.insert(order.len());
                order.push(row);
            }
        }
    }

    let mut messages = Vec::new();
    for row in order {
        let ts = row.timestamp.and_then(timestamp_secs_to_ms);
        let raw_content = row.content.as_deref().unwrap_or_default();
        let text = decode_content(raw_content);
        let blocks = match row.role.as_str() {
            // 工具输出：按 tool_call_id 配对；Hermes 不记录成败 → Unknown
            "tool" => {
                // 结构化编码（`\0json:` 前缀）的内容取整列没有意义，只给预览
                let plain = !raw_content.starts_with(CONTENT_JSON_PREFIX);
                vec![tool_result_block(
                    row.tool_call_id.clone().unwrap_or_default(),
                    ToolStatus::Unknown,
                    &text,
                    || plain.then(|| messages_cell(row.id, "content", String::new())),
                )]
            }
            // 助手：推理 → 正文 → tool_calls（OpenAI 形状）
            "assistant" => {
                let mut blocks = Vec::new();
                if let Some(reasoning) = row.reasoning.as_deref().filter(|r| !r.trim().is_empty()) {
                    blocks.push(thinking_block(reasoning, None, None, || {
                        Some(messages_cell(row.id, "reasoning", String::new()))
                    }));
                }
                if !text.trim().is_empty() {
                    blocks.push(SessionBlock::text(text));
                }
                let calls = row
                    .tool_calls
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
                blocks.extend(openai_tool_calls(calls.as_ref(), |pointer| {
                    Some(messages_cell(row.id, "tool_calls", pointer))
                }));
                blocks
            }
            _ if text.trim().is_empty() => Vec::new(),
            _ => vec![SessionBlock::text(text)],
        };
        let mut message = SessionMessage::from_blocks(row.role.clone(), ts, blocks);
        if message.is_empty() {
            continue;
        }
        message.id = Some(row.id.to_string());
        messages.push(message);
    }

    assign_turn_ids(&mut messages);
    Ok(messages)
}

/// `messages` 表某行某列的引用（`content::sqlite_allowed` 白名单内的列）
fn messages_cell(id: i64, column: &str, pointer: String) -> ContentRef {
    ContentRef::Sqlite {
        table: "messages".into(),
        id: id.to_string(),
        column: column.into(),
        pointer,
    }
}

/// Hermes stores timestamps as Unix epoch seconds (REAL).
fn timestamp_secs_to_ms(secs: f64) -> Option<i64> {
    (secs.is_finite() && secs > 0.0).then(|| (secs * 1000.0).round() as i64)
}

/// Delete a session from the Hermes SQLite database.
pub fn delete_session_sqlite(session_id: &str, source: &str) -> Result<bool, String> {
    let (db_path, ref_session_id) = parse_sqlite_source(source)
        .ok_or_else(|| format!("Invalid SQLite source reference: {source}"))?;
    let db_path = db_path
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize Hermes database path: {e}"))?;
    let expected_db_path = get_hermes_db_path()
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize expected Hermes database path: {e}"))?;

    if ref_session_id != session_id {
        return Err(format!(
            "Hermes SQLite session ID mismatch: expected {session_id}, found {ref_session_id}"
        ));
    }
    if db_path != expected_db_path {
        return Err("SQLite path does not match expected Hermes database".to_string());
    }

    let conn =
        Connection::open(&db_path).map_err(|e| format!("Failed to open Hermes database: {e}"))?;

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to begin transaction: {e}"))?;

    // Delete messages first (child records)
    let _ = tx.execute("DELETE FROM messages WHERE session_id = ?1", [session_id]);

    let deleted = tx
        .execute("DELETE FROM sessions WHERE id = ?1", [session_id])
        .map_err(|e| format!("Failed to delete Hermes session: {e}"))?;

    tx.commit()
        .map_err(|e| format!("Failed to commit session deletion: {e}"))?;

    Ok(deleted > 0)
}

pub(crate) fn parse_sqlite_source(source: &str) -> Option<(PathBuf, String)> {
    let rest = source.strip_prefix("sqlite:")?;
    let hash_pos = rest.rfind('#')?;
    let db_path = PathBuf::from(&rest[..hash_pos]);
    let session_id = rest[hash_pos + 1..].to_string();
    if session_id.is_empty() {
        return None;
    }
    Some((db_path, session_id))
}

// ── JSONL scanning ──────────────────────────────────────────────────

fn scan_sessions_jsonl() -> Vec<SessionMeta> {
    let sessions_dir = get_hermes_sessions_dir();
    if !sessions_dir.exists() {
        return Vec::new();
    }

    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("jsonl") && ext != Some("json") {
            continue;
        }
        if let Some(meta) = parse_jsonl_session(&path) {
            sessions.push(meta);
        }
    }
    sessions
}

/// 一行 JSONL 里的消息角色和非空正文（扁平和 `{type:"message", message:{…}}` 两种格式）。
fn jsonl_message(value: &Value) -> Option<(&str, String)> {
    let role = value
        .get("role")
        .or_else(|| value.get("message").and_then(|m| m.get("role")))
        .and_then(Value::as_str)?;
    let content = value
        .get("content")
        .or_else(|| value.get("message").and_then(|m| m.get("content")))?;
    let text = extract_text(content);
    if text.trim().is_empty() {
        return None;
    }
    Some((role, text))
}

fn parse_jsonl_session(path: &Path) -> Option<SessionMeta> {
    // Read head (metadata + first user message) and tail (last timestamp + last message)
    let (head, tail) = read_head_tail_lines(path, 30, 10).ok()?;

    let mut first_user_msg: Option<String> = None;
    let mut first_ts: Option<i64> = None;
    let mut last_ts: Option<i64> = None;
    let mut session_id: Option<String> = None;
    let mut title: Option<String> = None;
    let mut cwd: Option<String> = None;

    // Process head lines for metadata and first user message
    for line in &head {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let ts = value
            .get("timestamp")
            .or_else(|| value.get("ts"))
            .and_then(parse_timestamp_to_ms);

        if first_ts.is_none() {
            first_ts = ts;
        }
        last_ts = ts.or(last_ts);

        let line_type = value.get("type").and_then(Value::as_str).unwrap_or("");

        // Extract session metadata from session-type lines
        if line_type == "session" || line_type == "init" {
            if session_id.is_none() {
                session_id = value
                    .get("id")
                    .or_else(|| value.get("sessionId"))
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
            }
            if title.is_none() {
                title = value
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
            }
            if cwd.is_none() {
                cwd = value
                    .get("cwd")
                    .or_else(|| value.get("directory"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
            }
        }

        if first_user_msg.is_none() {
            if let Some(("user", text)) = jsonl_message(&value) {
                first_user_msg = Some(truncate_summary(&text, TITLE_MAX_CHARS));
            }
        }
    }

    // Process tail lines for the most recent timestamp and the last displayable message:
    // the summary is the last user/assistant message, same as the SQLite scan.
    let mut tail_ts: Option<i64> = None;
    let mut last_msg: Option<String> = None;
    for line in tail.iter().rev() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if tail_ts.is_none() {
            tail_ts = value
                .get("timestamp")
                .or_else(|| value.get("ts"))
                .and_then(parse_timestamp_to_ms);
        }
        if last_msg.is_none() {
            if let Some(("user" | "assistant", text)) = jsonl_message(&value) {
                last_msg = Some(truncate_summary(&text, 160));
            }
        }
        if tail_ts.is_some() && last_msg.is_some() {
            break;
        }
    }
    let last_ts = tail_ts.or(last_ts);

    // Fall back to filename as session ID
    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let source_path = path.to_string_lossy().to_string();

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id,
        title: title.or_else(|| first_user_msg.clone()),
        summary: last_msg.or(first_user_msg),
        project_dir: cwd,
        created_at: first_ts,
        last_active_at: last_ts.or(first_ts),
        source_path: Some(source_path),
        resume_command: None,
    })
}

/// Load messages from a Hermes JSONL transcript file.
///
/// 字段名按 SQLite 同构推断（`tool_calls` / `tool_call_id`，待核实）。
pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();

    for_each_jsonl_value(path, |span, value| {
        // Support both flat messages and nested {type:"message", message:{...}} format
        let (msg, base, ts_val) = if value.get("type").and_then(Value::as_str) == Some("message") {
            let Some(msg) = value.get("message") else {
                return Ok(());
            };
            (
                msg,
                "/message",
                value.get("timestamp").or_else(|| msg.get("ts")),
            )
        } else {
            (
                &value,
                "",
                value.get("timestamp").or_else(|| value.get("ts")),
            )
        };

        let Some(role) = msg.get("role").and_then(Value::as_str) else {
            return Ok(());
        };
        let content = msg.get("content");
        let text = content.map(extract_text).unwrap_or_default();
        let content_ref = || {
            content
                .filter(|c| c.is_string())
                .map(|_| span.content_ref(format!("{base}/content")))
        };

        let blocks = match role {
            "tool" => vec![tool_result_block(
                msg.get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                ToolStatus::Unknown,
                &text,
                content_ref,
            )],
            "assistant" => {
                let mut blocks = Vec::new();
                if !text.trim().is_empty() {
                    blocks.push(SessionBlock::text(text));
                }
                blocks.extend(openai_tool_calls(msg.get("tool_calls"), |pointer| {
                    Some(span.content_ref(format!("{base}/tool_calls{pointer}")))
                }));
                blocks
            }
            _ if text.trim().is_empty() => Vec::new(),
            _ => vec![SessionBlock::text(text)],
        };

        let ts = ts_val.and_then(parse_timestamp_to_ms);
        let message = SessionMessage::from_blocks(role, ts, blocks);
        if !message.is_empty() {
            messages.push(message);
        }
        Ok(())
    })?;

    assign_turn_ids(&mut messages);
    Ok(messages)
}

/// Delete a Hermes JSONL session file.
pub fn delete_session(_root: &Path, path: &Path, _session_id: &str) -> Result<bool, String> {
    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete Hermes session file {}: {e}",
            path.display()
        )
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::model::ToolKind;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parse_sqlite_source_valid() {
        let (path, id) = parse_sqlite_source("sqlite:/home/user/.hermes/state.db#session-123")
            .expect("should parse");
        assert_eq!(path, PathBuf::from("/home/user/.hermes/state.db"));
        assert_eq!(id, "session-123");
    }

    #[test]
    fn parse_sqlite_source_invalid() {
        assert!(parse_sqlite_source("not-sqlite").is_none());
        assert!(parse_sqlite_source("sqlite:").is_none());
        assert!(parse_sqlite_source("sqlite:/path#").is_none());
    }

    #[test]
    fn parse_jsonl_session_extracts_metadata() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test-session.jsonl");
        let mut f = File::create(&path).expect("create");
        writeln!(
            f,
            r#"{{"type":"session","id":"s1","title":"My Session","cwd":"/home/user/project"}}"#
        )
        .unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"user","content":"Hello world"}},"timestamp":"2026-01-01T00:00:00Z"}}"#).unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"assistant","content":"Hi there"}},"timestamp":"2026-01-01T00:01:00Z"}}"#).unwrap();
        f.flush().unwrap();

        let meta = parse_jsonl_session(&path).expect("should parse");
        assert_eq!(meta.session_id, "s1");
        assert_eq!(meta.title.as_deref(), Some("My Session"));
        assert_eq!(meta.project_dir.as_deref(), Some("/home/user/project"));
        assert!(meta.created_at.is_some());
        assert!(meta.last_active_at.is_some());
    }

    /// JSONL 行和 SQLite 行并排显示：摘要同样取最后一条 user/assistant，首条只做标题回退。
    #[test]
    fn parse_jsonl_session_summary_is_the_last_message() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("s2.jsonl");
        let mut f = File::create(&path).expect("create");
        writeln!(f, r#"{{"type":"session","id":"s2"}}"#).unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"user","content":"first question"}},"timestamp":"2026-01-01T00:00:00Z"}}"#).unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"assistant","content":"first answer"}},"timestamp":"2026-01-01T00:01:00Z"}}"#).unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"user","content":"second question"}},"timestamp":"2026-01-01T00:02:00Z"}}"#).unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"assistant","content":"last answer"}},"timestamp":"2026-01-01T00:03:00Z"}}"#).unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"tool","content":"ignored"}},"timestamp":"2026-01-01T00:04:00Z"}}"#).unwrap();
        f.flush().unwrap();

        let meta = parse_jsonl_session(&path).expect("should parse");
        assert_eq!(meta.title.as_deref(), Some("first question"));
        assert_eq!(meta.summary.as_deref(), Some("last answer"));
    }

    #[test]
    fn parse_jsonl_session_fallback_to_filename() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("my-session.jsonl");
        let mut f = File::create(&path).expect("create");
        writeln!(f, r#"{{"role":"user","content":"Hello","ts":1700000000}}"#).unwrap();
        f.flush().unwrap();

        let meta = parse_jsonl_session(&path).expect("should parse");
        assert_eq!(meta.session_id, "my-session");
        assert!(meta.title.is_some()); // Falls back to first user message
    }

    #[test]
    fn load_messages_flat_format() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let mut f = File::create(&path).expect("create");
        writeln!(
            f,
            r#"{{"role":"user","content":"What is Rust?","ts":1700000000}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"role":"assistant","content":"A systems programming language.","ts":1700000001}}"#
        )
        .unwrap();
        f.flush().unwrap();

        let msgs = load_messages(&path).expect("should load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
    }

    #[test]
    fn load_messages_nested_format() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let mut f = File::create(&path).expect("create");
        writeln!(f, r#"{{"type":"session","id":"s1"}}"#).unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"user","content":"Hello"}},"timestamp":"2026-01-01T00:00:00Z"}}"#).unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"assistant","content":"Hi"}},"timestamp":"2026-01-01T00:01:00Z"}}"#).unwrap();
        f.flush().unwrap();

        let msgs = load_messages(&path).expect("should load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert!(msgs[0].ts.is_some());
    }

    #[test]
    fn delete_session_removes_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        File::create(&path).expect("create");
        assert!(path.exists());

        delete_session(dir.path(), &path, "session").expect("should delete");
        assert!(!path.exists());
    }

    // ── SQLite (state.db) ───────────────────────────────────────────

    /// A `state.db` with the columns of Hermes' schema these readers touch.
    fn hermes_db(dir: &Path) -> (PathBuf, Connection) {
        let path = dir.join("state.db");
        let conn = Connection::open(&path).expect("open db");
        conn.execute_batch(
            "CREATE TABLE sessions (
                 id TEXT PRIMARY KEY, cwd TEXT, title TEXT,
                 started_at REAL NOT NULL, ended_at REAL, last_activity_at REAL);
             CREATE TABLE messages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
                 role TEXT NOT NULL, content TEXT, tool_call_id TEXT, tool_calls TEXT,
                 tool_name TEXT, timestamp REAL NOT NULL,
                 active INTEGER NOT NULL DEFAULT 1, compacted INTEGER NOT NULL DEFAULT 0,
                 display_metadata TEXT);",
        )
        .expect("create schema");
        (path, conn)
    }

    fn insert_message(conn: &Connection, session: &str, role: &str, content: &str, ts: f64) {
        conn.execute(
            "INSERT INTO messages (session_id, role, content, timestamp) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![session, role, content, ts],
        )
        .expect("insert message");
    }

    fn contents(conn: &Connection, session: &str) -> Vec<String> {
        load_messages_from_conn(conn, session)
            .expect("load messages")
            .into_iter()
            .map(|m| m.content)
            .collect()
    }

    #[test]
    fn load_messages_sqlite_reads_timestamp_in_insertion_order() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let (path, conn) = hermes_db(dir.path());
        // Clock went backwards between the two rows: id order still wins.
        insert_message(&conn, "s1", "user", "question", 1000.5);
        insert_message(&conn, "s1", "assistant", "answer", 999.0);
        drop(conn);

        let msgs = load_messages_sqlite(&format!("sqlite:{}#s1", path.display()))
            .expect("load from source");
        assert_eq!(msgs.len(), 2);
        assert_eq!(
            (msgs[0].role.as_str(), msgs[0].content.as_str()),
            ("user", "question")
        );
        assert_eq!(msgs[0].ts, Some(1_000_500));
        assert_eq!(msgs[1].content, "answer");
    }

    #[test]
    fn load_messages_sqlite_renders_tool_calls() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let (_path, conn) = hermes_db(dir.path());
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_calls, timestamp)
             VALUES ('s1', 'assistant', 'let me check', ?1, 1.0)",
            [r#"[{"id":"c1","type":"function","function":{"name":"terminal","arguments":"{}"}}]"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_call_id, timestamp)
             VALUES ('s1', 'tool', 'file1.txt', 'c1', 2.0)",
            [],
        )
        .unwrap();

        // 正文与工具调用合在同一条助手消息里
        assert_eq!(
            contents(&conn, "s1"),
            vec!["let me check\n\n[Tool: terminal]", "file1.txt"]
        );
    }

    #[test]
    fn load_messages_sqlite_keeps_compacted_history_and_hides_rewound_rows() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let (_path, conn) = hermes_db(dir.path());
        insert_message(&conn, "s1", "user", "first question", 1.0);
        insert_message(&conn, "s1", "assistant", "first answer", 2.0);
        insert_message(&conn, "s1", "user", "second question", 3.0);
        insert_message(&conn, "s1", "assistant", "second answer", 4.0);
        // Compression: the carried tail is archived rewind-style, the rest is
        // compaction-archived, then protected head + summary + tail are
        // re-inserted with their original timestamps.
        conn.execute_batch(
            "UPDATE messages SET active = 0, compacted = 0 WHERE id IN (3, 4);
             UPDATE messages SET active = 0, compacted = 1 WHERE id IN (1, 2);",
        )
        .unwrap();
        insert_message(&conn, "s1", "user", "first question", 1.0);
        insert_message(
            &conn,
            "s1",
            "assistant",
            "[CONTEXT COMPACTION] summary",
            10.0,
        );
        insert_message(&conn, "s1", "user", "second question", 3.0);
        insert_message(&conn, "s1", "assistant", "second answer", 4.0);
        // A regenerated-away reply, then the live one.
        insert_message(&conn, "s1", "assistant", "discarded reply", 11.0);
        conn.execute("UPDATE messages SET active = 0 WHERE id = 9", [])
            .unwrap();
        insert_message(&conn, "s1", "assistant", "kept reply", 12.0);

        assert_eq!(
            contents(&conn, "s1"),
            vec![
                "first question",
                "first answer",
                "[CONTEXT COMPACTION] summary",
                "second question",
                "second answer",
                "kept reply",
            ]
        );
    }

    #[test]
    fn load_messages_sqlite_skips_model_only_rows() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let (_path, conn) = hermes_db(dir.path());
        insert_message(&conn, "s1", "user", "part one", 1.0);
        insert_message(&conn, "s1", "user", "part two", 2.0);
        conn.execute(
            "INSERT INTO messages (session_id, role, content, timestamp, display_metadata)
             VALUES ('s1', 'user', 'part one\npart two', 3.0, ?1)",
            // Pre-guard rows are double-encoded.
            [r#""{\"model_only\": true}""#],
        )
        .unwrap();

        assert_eq!(contents(&conn, "s1"), vec!["part one", "part two"]);
    }

    #[test]
    fn load_messages_sqlite_decodes_structured_content() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let (_path, conn) = hermes_db(dir.path());
        insert_message(
            &conn,
            "s1",
            "user",
            "\u{0}json:[{\"type\": \"text\", \"text\": \"look at this\"}, \
             {\"type\": \"image_url\", \"image_url\": {\"url\": \"data:image/png;base64,AAAA\"}}]",
            1.0,
        );

        assert_eq!(contents(&conn, "s1"), vec!["look at this"]);
    }

    #[test]
    fn load_messages_sqlite_reads_stores_without_newer_columns() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let conn = Connection::open(dir.path().join("state.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
                 role TEXT NOT NULL, content TEXT, tool_calls TEXT, tool_name TEXT,
                 timestamp REAL NOT NULL);",
        )
        .unwrap();
        insert_message(&conn, "s1", "user", "hi", 1.0);

        assert_eq!(contents(&conn, "s1"), vec!["hi"]);
    }

    #[test]
    fn scan_sessions_sqlite_uses_first_user_message_for_missing_title_and_last_for_summary() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let (path, conn) = hermes_db(dir.path());
        conn.execute_batch(
            "INSERT INTO sessions (id, title, started_at, ended_at, last_activity_at)
                 VALUES ('titled', 'Hermes title', 100.0, NULL, 200.0);
             INSERT INTO sessions (id, title, started_at) VALUES ('untitled', NULL, 300.0);",
        )
        .unwrap();
        insert_message(&conn, "titled", "user", "titled question", 101.0);
        // The original first message survives compaction as an archived row.
        insert_message(&conn, "untitled", "user", "original question", 301.0);
        conn.execute(
            "UPDATE messages SET active = 0, compacted = 1 WHERE id = 2",
            [],
        )
        .unwrap();
        insert_message(
            &conn,
            "untitled",
            "user",
            "\u{0}json:[{\"type\":\"text\",\"text\":\"later\"}]",
            302.0,
        );
        drop(conn);

        let sessions = scan_sessions_sqlite_at(&path);
        let titled = sessions.iter().find(|s| s.session_id == "titled").unwrap();
        assert_eq!(titled.title.as_deref(), Some("Hermes title"));
        assert_eq!(titled.summary.as_deref(), Some("titled question"));
        assert_eq!(titled.last_active_at, Some(200_000));

        let untitled = sessions
            .iter()
            .find(|s| s.session_id == "untitled")
            .unwrap();
        assert_eq!(untitled.title.as_deref(), Some("original question"));
        // 摘要是最后一条（结构化内容解码后的文字），不是开头那句
        assert_eq!(untitled.summary.as_deref(), Some("later"));
        assert_eq!(untitled.last_active_at, Some(300_000));
    }

    #[test]
    fn scan_sessions_sqlite_summary_is_the_last_user_or_assistant_message() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let (path, conn) = hermes_db(dir.path());
        conn.execute_batch(
            "INSERT INTO sessions (id, title, started_at) VALUES ('s1', 'T', 100.0);",
        )
        .unwrap();
        insert_message(&conn, "s1", "user", "first question", 101.0);
        insert_message(&conn, "s1", "assistant", "the answer", 102.0);
        // 工具行和被 undo 藏起来的行都不算「最后」
        insert_message(&conn, "s1", "tool", "tool output", 103.0);
        insert_message(&conn, "s1", "user", "rewound question", 104.0);
        conn.execute(
            "UPDATE messages SET active = 0, compacted = 0 WHERE id = 4",
            [],
        )
        .unwrap();
        drop(conn);

        let sessions = scan_sessions_sqlite_at(&path);
        let s1 = sessions.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.summary.as_deref(), Some("the answer"));
    }

    #[test]
    fn first_user_messages_is_scoped_to_listed_sessions() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let (_path, conn) = hermes_db(dir.path());
        for i in 0..=SQLITE_SCAN_LIMIT {
            let id = format!("s{i}");
            conn.execute(
                "INSERT INTO sessions (id, started_at) VALUES (?1, 1.0)",
                [&id],
            )
            .unwrap();
            insert_message(&conn, &id, "user", "hello", 1.0);
        }

        let found = first_user_messages(&conn);
        assert_eq!(found.len(), SQLITE_SCAN_LIMIT);
        assert!(
            !found.contains_key("s0"),
            "oldest session is outside the list"
        );
    }

    #[test]
    fn load_messages_sqlite_pairs_tool_calls_with_results() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().expect("tempdir");
        let (_path, conn) = hermes_db(dir.path());
        conn.execute_batch("ALTER TABLE messages ADD COLUMN reasoning TEXT;")
            .unwrap();
        insert_message(&conn, "s1", "user", "看看磁盘", 1.0);
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_calls, reasoning, timestamp)
             VALUES ('s1', 'assistant', '我先看一下。', ?1, 'need df', 2.0)",
            [r#"[{"id":"call_1","type":"function","function":{"name":"terminal","arguments":"{\"command\": \"df -h /\", \"timeout\": 30}"}},
                 {"id":"call_2","type":"function","function":{"name":"web_search","arguments":"{\"query\": \"ext4 tune2fs\"}"}}]"#],
        )
        .unwrap();
        let long: String = (1..=40).map(|i| format!("result {i}\n")).collect();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_call_id, timestamp)
             VALUES ('s1', 'tool', 'Filesystem  Size', 'call_1', 3.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_call_id, timestamp)
             VALUES ('s1', 'tool', ?1, 'call_2', 4.0)",
            [&long],
        )
        .unwrap();
        // 缺 tool_call_id 的旧数据：callId 为空，前端显示为通用「工具输出」
        insert_message(&conn, "s1", "tool", "orphan output", 5.0);

        let msgs = load_messages_from_conn(&conn, "s1").expect("load");
        assert_eq!(msgs.len(), 5);
        let turns: Vec<_> = msgs.iter().map(|m| m.turn_id.as_deref().unwrap()).collect();
        assert_eq!(turns, ["t1"; 5]);
        assert_eq!(msgs[1].id.as_deref(), Some("2"));

        let a = &msgs[1].blocks;
        assert!(matches!(&a[0], SessionBlock::Thinking { text, .. } if text == "need df"));
        assert!(matches!(&a[1], SessionBlock::Text { text, .. } if text == "我先看一下。"));
        match (&a[2], &a[3]) {
            (
                SessionBlock::ToolCall {
                    id, kind, title, ..
                },
                SessionBlock::ToolCall {
                    id: id2,
                    kind: kind2,
                    title: title2,
                    ..
                },
            ) => {
                assert_eq!(
                    (id.as_str(), *kind, title.as_str()),
                    ("call_1", ToolKind::Shell, "df -h /")
                );
                assert_eq!(
                    (id2.as_str(), *kind2, title2.as_str()),
                    ("call_2", ToolKind::Web, "ext4 tune2fs")
                );
            }
            other => panic!("{other:?}"),
        }

        let result = |i: usize| match &msgs[i].blocks[0] {
            SessionBlock::ToolResult {
                call_id,
                status,
                full,
                truncated,
                ..
            } => (call_id.clone(), *status, full.clone(), *truncated),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            result(2),
            ("call_1".into(), ToolStatus::Unknown, None, false)
        );
        let (call_id, _, full, truncated) = result(3);
        assert_eq!(call_id, "call_2");
        assert!(truncated);
        assert_eq!(
            full,
            Some(ContentRef::Sqlite {
                table: "messages".into(),
                id: "4".into(),
                column: "content".into(),
                pointer: String::new(),
            })
        );
        assert_eq!(result(4).0, "");
        assert_eq!(msgs[4].role, "tool");
    }

    #[test]
    fn load_messages_jsonl_pairs_tool_calls() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let mut f = File::create(&path).expect("create");
        writeln!(
            f,
            r#"{{"role":"user","content":"列一下文件","ts":1700000000}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"role":"assistant","content":"","tool_calls":[{{"id":"c1","function":{{"name":"terminal","arguments":"{{\"command\":\"ls\"}}"}}}}],"ts":1700000001}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"role":"tool","tool_call_id":"c1","content":"a.txt","ts":1700000002}}"#
        )
        .unwrap();
        f.flush().unwrap();

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1].content, "[Tool: terminal] ls");
        assert!(matches!(
            &msgs[2].blocks[0],
            SessionBlock::ToolResult { call_id, status: ToolStatus::Unknown, preview, .. }
                if call_id == "c1" && preview == "a.txt"
        ));
    }
}
