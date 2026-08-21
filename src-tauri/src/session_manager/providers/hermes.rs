use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use crate::hermes_config::get_hermes_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

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

fn scan_sessions_sqlite() -> Vec<SessionMeta> {
    let db_path = get_hermes_db_path();
    if !db_path.exists() {
        return Vec::new();
    }

    let conn = match Connection::open_with_flags(
        &db_path,
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

    // Build a lookup of "first user message content" per session in one
    // query so the list view can show a sensible title without an N+1
    // round-trip per row. We only consider active=1 rows so compacted
    // turns don't pollute the preview.
    let mut first_user_msg: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='messages'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0
    {
        let sql = "SELECT m.session_id, m.content \
                   FROM messages m \
                   JOIN ( \
                     SELECT session_id, MIN(timestamp) AS first_ts \
                     FROM messages \
                     WHERE role = 'user' AND active = 1 \
                     GROUP BY session_id \
                   ) first ON first.session_id = m.session_id AND first.first_ts = m.timestamp \
                   WHERE m.role = 'user'";
        if let Ok(mut stmt) = conn.prepare(sql) {
            if let Ok(rows) = stmt.query_map([], |row| {
                let sid: String = row.get(0)?;
                let content: String = row.get(1)?;
                Ok((sid, content))
            }) {
                for row in rows.flatten() {
                    first_user_msg.insert(row.0, row.1);
                }
            }
        }
    }

    // Query sessions — use flexible column access via pragma
    let columns = get_table_columns(&conn, "sessions");

    let query = "SELECT * FROM sessions ORDER BY rowid DESC LIMIT 500";
    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut sessions = Vec::new();
    let rows = match stmt.query_map([], |row| Ok(row_to_json(row, &columns))) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let db_source = format!("sqlite:{}", db_path.display());

    for row_result in rows.flatten() {
        if let Some(mut meta) = sqlite_row_to_session_meta(&row_result, &db_source) {
            // The sessions table doesn't carry a title column in Hermes
            // (v3.x). If `display_name` is blank — which it always is today —
            // fall back to the first user message we just looked up, so the
            // list view shows something meaningful instead of an 8-char id.
            if meta.title.is_none() || meta.title.as_deref() == Some("") {
                if let Some(msg) = first_user_msg.get(&meta.session_id) {
                    meta.title = Some(truncate_summary(msg, TITLE_MAX_CHARS).to_string());
                    meta.summary = Some(msg.clone());
                }
            }
            sessions.push(meta);
        }
    }

    sessions
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
        .get("ended_at")
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
///
/// Hermes' `messages` table has these columns we care about:
///   - role          : "user" | "assistant" | "tool" | "system"
///   - content       : TEXT (assistant/user text or tool result JSON-as-text)
///   - timestamp     : REAL Unix epoch seconds (with fractional part)
///   - tool_calls    : JSON array of OpenAI-style function calls, present on
///                     assistant turns when the model emitted one or more tool
///                     invocations (e.g. `terminal`, `web_search`).
///   - tool_name     : populated on tool-result rows (`role="tool"`).
///   - active        : 0 = compacted/superseded (skip these).
///
/// We render tool calls the same way Codex does — assistant tool_use becomes
/// `"[Tool: <name>]"` and tool result rows become their content string with
/// `role="tool"`, so the SessionManager UI can color them uniformly.
pub fn load_messages_sqlite(source: &str) -> Result<Vec<SessionMessage>, String> {
    let (db_path, session_id) = parse_sqlite_source(source)
        .ok_or_else(|| format!("Invalid SQLite source reference: {source}"))?;

    let conn = Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open Hermes database: {e}"))?;

    // Hermes messages schema (v3+):
    //   timestamp REAL NOT NULL — Unix epoch seconds
    // We deliberately skip `active = 0` rows (compacted/rolled-back turns).
    let query = "SELECT role, content, timestamp, tool_calls, tool_name, active \
                 FROM messages WHERE session_id = ?1 \
                 ORDER BY timestamp ASC, id ASC";

    let mut stmt = conn
        .prepare(query)
        .map_err(|e| format!("Failed to prepare messages query: {e}"))?;

    let rows = stmt
        .query_map([session_id.as_str()], |row| {
            let role: String = row.get(0)?;
            let content: Option<String> = row.get(1).ok();
            let ts: Option<f64> = row.get(2).ok();
            let tool_calls: Option<String> = row.get(3).ok();
            let tool_name: Option<String> = row.get(4).ok();
            let active: Option<i64> = row.get(5).ok();
            Ok((role, content, ts, tool_calls, tool_name, active))
        })
        .map_err(|e| format!("Failed to query messages: {e}"))?;

    let mut messages = Vec::new();
    for row in rows.flatten() {
        let (role, content, ts, tool_calls, tool_name, active) = row;

        // Skip compacted/superseded turns.
        if matches!(active, Some(0)) {
            continue;
        }

        // For assistant turns with tool_calls, emit one [Tool: name] line per
        // call (mirrors Codex's `function_call` rendering). If there is also
        // text content, emit that first so the reasoning narrative stays
        // visible.
        let mut emitted_any = false;
        if role == "assistant" {
            if let Some(ref text) = content {
                if !text.trim().is_empty() {
                    messages.push(SessionMessage {
                        role: "assistant".to_string(),
                        content: text.clone(),
                        ts: ts.and_then(timestamp_secs_to_ms),
                    });
                    emitted_any = true;
                }
            }
            if let Some(tc_json) = tool_calls.as_deref() {
                if let Ok(Value::Array(calls)) = serde_json::from_str::<Value>(tc_json) {
                    for call in calls {
                        let name = call
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .or_else(|| call.get("name").and_then(Value::as_str))
                            .unwrap_or("unknown");
                        messages.push(SessionMessage {
                            role: "assistant".to_string(),
                            content: format!("[Tool: {name}]"),
                            ts: ts.and_then(timestamp_secs_to_ms),
                        });
                        emitted_any = true;
                    }
                }
            }
        } else {
            // user / tool / system / other: content is the message body.
            // For tool results, prefer `tool_name` as a header so the reader
            // can tell which call produced this output.
            let body = match (role.as_str(), tool_name.as_deref()) {
                ("tool", Some(name)) if !name.is_empty() => {
                    let inner = content.unwrap_or_default();
                    if inner.trim().is_empty() {
                        format!("[Result: {name}]")
                    } else {
                        format!("[Result: {name}]\n{inner}")
                    }
                }
                _ => content.unwrap_or_default(),
            };
            if !body.trim().is_empty() {
                messages.push(SessionMessage {
                    role,
                    content: body,
                    ts: ts.and_then(timestamp_secs_to_ms),
                });
                emitted_any = true;
            }
        }

        // Suppress the unused assignment lint when no message was emitted.
        let _ = emitted_any;
    }

    Ok(messages)
}

/// Hermes stores timestamps as Unix epoch seconds (REAL). Convert to ms.
fn timestamp_secs_to_ms(secs: f64) -> Option<i64> {
    let ms = (secs * 1000.0).round() as i64;
    if ms <= 0 {
        None
    } else {
        Some(ms)
    }
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

fn parse_sqlite_source(source: &str) -> Option<(PathBuf, String)> {
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

fn parse_jsonl_session(path: &Path) -> Option<SessionMeta> {
    // Read head (metadata + first user message) and tail (last timestamp)
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
            let role = value
                .get("role")
                .or_else(|| value.get("message").and_then(|m| m.get("role")))
                .and_then(Value::as_str);

            if role == Some("user") {
                let content = value
                    .get("content")
                    .or_else(|| value.get("message").and_then(|m| m.get("content")));
                if let Some(c) = content {
                    let text = extract_text(c);
                    if !text.trim().is_empty() {
                        first_user_msg = Some(truncate_summary(&text, TITLE_MAX_CHARS).to_string());
                    }
                }
            }
        }
    }

    // Process tail lines for the most recent timestamp
    for line in tail.iter().rev() {
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
        if let Some(t) = ts {
            last_ts = Some(t);
            break;
        }
    }

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
        summary: first_user_msg,
        project_dir: cwd,
        created_at: first_ts,
        last_active_at: last_ts.or(first_ts),
        source_path: Some(source_path),
        resume_command: None,
    })
}

/// Load messages from a Hermes JSONL transcript file.
pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Support both flat messages and nested {type:"message", message:{...}} format
        let (role_val, content_val, ts_val) =
            if value.get("type").and_then(Value::as_str) == Some("message") {
                let msg = match value.get("message") {
                    Some(m) => m,
                    None => continue,
                };
                (
                    msg.get("role"),
                    msg.get("content"),
                    value.get("timestamp").or_else(|| msg.get("ts")),
                )
            } else {
                (
                    value.get("role"),
                    value.get("content"),
                    value.get("timestamp").or_else(|| value.get("ts")),
                )
            };

        let role = match role_val.and_then(Value::as_str) {
            Some(r) => r.to_string(),
            None => continue,
        };

        let content = content_val.map(extract_text).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }

        let ts = ts_val.and_then(parse_timestamp_to_ms);
        messages.push(SessionMessage { role, content, ts });
    }

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

    // ── SQLite integration tests (real schema, in-memory) ──────────────
    //
    // These guard against the regressions that originally made Hermes
    // sessions invisible in the SessionManager:
    //   1. `load_messages_sqlite` must query the `timestamp` column, not
    //      `created_at` (Hermes' schema uses REAL seconds).
    //   2. tool_calls JSON must be rendered as `[Tool: name]`, not blank.
    //   3. `active = 0` rows must be skipped.

    use rusqlite::Connection;

    fn build_hermes_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                cwd TEXT,
                started_at REAL,
                ended_at REAL,
                message_count INTEGER DEFAULT 0
            );
            CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                tool_calls TEXT,
                tool_name TEXT,
                timestamp REAL NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            );
            "#,
        )
        .expect("create schema");
        conn
    }

    #[test]
    fn load_messages_sqlite_reads_timestamp_column_and_orders_by_it() {
        let conn = build_hermes_db();
        // Insert out-of-order to make sure ORDER BY timestamp actually sorts.
        conn.execute(
            "INSERT INTO messages (session_id, role, content, timestamp) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["s1", "assistant", "second reply", 1000.5],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, timestamp) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["s1", "user", "first question", 1000.0],
        ).unwrap();

        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("state.db");
        conn.execute_batch(&format!(
            "ATTACH DATABASE '{}' AS disk; CREATE TABLE disk.sessions AS SELECT * FROM sessions; CREATE TABLE disk.messages AS SELECT * FROM messages; DETACH DATABASE disk;",
            db_path.display()
        )).expect("materialize db");

        let source = format!("sqlite:{}#s1", db_path.display());
        let msgs = load_messages_sqlite(&source).expect("load_messages_sqlite");

        assert_eq!(msgs.len(), 2, "should load both rows from a `timestamp` column");
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "first question");
        assert_eq!(msgs[0].ts, Some(1_000_000));
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].ts, Some(1_000_500));
    }

    #[test]
    fn load_messages_sqlite_renders_tool_calls_as_tool_markers() {
        let conn = build_hermes_db();
        let tool_calls_json = r#"[{"id":"c1","function":{"name":"terminal","arguments":"{\"command\":\"ls\"}"}}]"#;
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_calls, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["s2", "assistant", "let me check", tool_calls_json, 2000.0],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_name, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["s2", "tool", "file1.txt\nfile2.txt", "terminal", 2001.0],
        ).unwrap();

        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("state.db");
        conn.execute_batch(&format!(
            "ATTACH DATABASE '{}' AS disk; CREATE TABLE disk.sessions AS SELECT * FROM sessions; CREATE TABLE disk.messages AS SELECT * FROM messages; DETACH DATABASE disk;",
            db_path.display()
        )).expect("materialize db");

        let source = format!("sqlite:{}#s2", db_path.display());
        let msgs = load_messages_sqlite(&source).expect("load_messages_sqlite");

        // assistant turn should produce: (text) + ([Tool: terminal])
        assert_eq!(msgs.len(), 3, "expected text + tool + tool_result");
        assert_eq!(msgs[0].content, "let me check");
        assert_eq!(msgs[1].content, "[Tool: terminal]");
        // tool result gets a header from tool_name
        assert!(
            msgs[2].content.starts_with("[Result: terminal]"),
            "got {:?}",
            msgs[2].content
        );
        assert!(msgs[2].content.contains("file1.txt"));
    }

    #[test]
    fn load_messages_sqlite_skips_inactive_rows() {
        let conn = build_hermes_db();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, timestamp, active) VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params!["s3", "assistant", "compacted thought", 3000.0],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, timestamp) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["s3", "user", "live question", 3001.0],
        ).unwrap();

        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("state.db");
        conn.execute_batch(&format!(
            "ATTACH DATABASE '{}' AS disk; CREATE TABLE disk.sessions AS SELECT * FROM sessions; CREATE TABLE disk.messages AS SELECT * FROM messages; DETACH DATABASE disk;",
            db_path.display()
        )).expect("materialize db");

        let source = format!("sqlite:{}#s3", db_path.display());
        let msgs = load_messages_sqlite(&source).expect("load_messages_sqlite");

        assert_eq!(msgs.len(), 1, "active=0 row must be filtered out");
        assert_eq!(msgs[0].content, "live question");
    }

    #[test]
    fn timestamp_secs_to_ms_handles_fractional_seconds() {
        assert_eq!(timestamp_secs_to_ms(1_700_000_000.5), Some(1_700_000_000_500));
        assert_eq!(timestamp_secs_to_ms(0.0), None);
        assert_eq!(timestamp_secs_to_ms(-1.0), None);
    }
}
