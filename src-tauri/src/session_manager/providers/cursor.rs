//! Cursor 会话管理。
//!
//! 数据源：
//! - `conversation-search.db`（会话索引库）——`conversations` 表（id / title / updated_at / is_archived）
//!   供列表；`conversation_fts`（FTS5）扁平转录（title + body，无角色标记）作**回退**读消息源。
//! - `state.vscdb`（主状态库，`cursorDiskKV` 表）——结构化消息读消息的**首选源**：
//!   `composerData:<会话UUID>` JSON 的 `fullConversationHeadersOnly` 给出有序消息头
//!   （`{bubbleId, type}`，type 1=用户 / 2=AI / 3=系统 / 4=工具），正文在独立的
//!   `bubbleId:<composerId>:<bubbleId>` 键（`{_v, type, text, createdAt}`）。旧版
//!   composerData 直接含 `conversation` 数组，作为次选兼容。
//!
//! source_path 格式与 opencode/hermes 一致：`sqlite:<db绝对路径>:<会话UUID>`。

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

const PROVIDER_ID: &str = "cursor";

/// Cursor 全局存储目录（含 conversation-search.db）。
/// 测试可用 `CC_SWITCH_CURSOR_SESSION_DB` 环境变量直接覆盖 DB 路径。
fn get_conversation_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("CC_SWITCH_CURSOR_SESSION_DB") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    get_cursor_global_storage_dir().join("conversation-search.db")
}

#[cfg(target_os = "macos")]
fn get_cursor_global_storage_dir() -> PathBuf {
    crate::config::get_home_dir().join("Library/Application Support/Cursor/User/globalStorage")
}

#[cfg(target_os = "windows")]
fn get_cursor_global_storage_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(crate::config::get_home_dir)
        .join("Cursor/User/globalStorage")
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn get_cursor_global_storage_dir() -> PathBuf {
    crate::config::get_home_dir().join(".config/Cursor/User/globalStorage")
}

/// Cursor 主状态库 `state.vscdb`（含 `composerHeaders` 会话索引 + `cursorDiskKV` 结构化消息）。
/// 测试可用 `CC_SWITCH_CURSOR_STATE_DB` 覆盖路径。
fn get_state_vscdb_path() -> PathBuf {
    if let Ok(p) = std::env::var("CC_SWITCH_CURSOR_STATE_DB") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    get_cursor_global_storage_dir().join("state.vscdb")
}

/// 解析 `sqlite:<db_path>:<会话UUID>` 引用。
/// Cursor 会话 id 为 UUID（无冒号），用最后一个冒号切分即可（路径可能含冒号，如 Windows 盘符）。
fn parse_sqlite_source(source: &str) -> Option<(PathBuf, String)> {
    let rest = source.strip_prefix("sqlite:")?;
    let sep = rest.rfind(':')?;
    let db_path = PathBuf::from(&rest[..sep]);
    let session_id = rest[sep + 1..].to_string();
    if !is_uuid_like(&session_id) {
        return None;
    }
    Some((db_path, session_id))
}

/// 粗略校验 UUID 形态（8-4-4-4-12），用于解析安全，不做 hex 校验。
fn is_uuid_like(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
}

/// 只读打开 Cursor 会话库。
fn open_readonly(db_path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("打开 Cursor 会话库失败 {}: {e}", db_path.display()))
}

/// 扫描 Cursor 会话（conversation-search.db `conversations` 表）。
/// 跳过已归档会话，按 updated_at 降序。
pub fn scan_sessions() -> Vec<SessionMeta> {
    let db_path = get_conversation_db_path();
    if !db_path.exists() {
        return Vec::new();
    }

    let conn = match open_readonly(&db_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut stmt = match conn.prepare(
        "SELECT id, title, updated_at, is_archived FROM conversations \
         WHERE is_archived = 0 ORDER BY updated_at DESC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = match stmt.query_map([], |row| {
        let session_id: String = row.get(0)?;
        let title: String = row.get(1)?;
        let updated_at: i64 = row.get(2)?;
        let is_archived: i64 = row.get(3)?;
        Ok((session_id, title, updated_at, is_archived))
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };

    let db_display = db_path.display().to_string();
    let mut sessions = Vec::new();
    for row in rows.flatten() {
        let (session_id, title, updated_at, is_archived) = row;
        if is_archived != 0 {
            continue;
        }
        let title = if title.trim().is_empty() {
            None
        } else {
            Some(title.trim().to_string())
        };
        sessions.push(SessionMeta {
            provider_id: PROVIDER_ID.to_string(),
            session_id: session_id.clone(),
            title: title.clone(),
            summary: title,
            project_dir: None,
            created_at: None,
            last_active_at: Some(updated_at),
            source_path: Some(format!("sqlite:{db_display}:{session_id}")),
            resume_command: None,
        });
    }
    sessions
}

/// 将扁平转录拆成「用户 → AI」两轮。
///
/// Cursor 的 FTS body 是把消息文本按顺序拼接的搜索索引，无角色标记；经实测，
/// **首段 = 用户的初始提示，其后 = AI 的连续回复**（多行提示后有空行，否则首行即提示）。
/// 因此拆成：
/// - user：首个段落（空行分隔）或首行
/// - assistant：其余全文
///
/// 局限：多轮会话中穿插的后续用户提示无法从搜索索引可靠还原（无标记）。
fn split_transcript_into_turns(body: &str) -> Vec<SessionMessage> {
    let (user, rest) = if let Some((a, b)) = body.split_once("\n\n") {
        (a.trim(), b.trim())
    } else {
        match body.split_once('\n') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (body.trim(), ""),
        }
    };

    let mut msgs = Vec::new();
    if !user.is_empty() {
        msgs.push(SessionMessage {
            role: "user".to_string(),
            content: user.to_string(),
            ts: None,
        });
    }
    if !rest.is_empty() {
        msgs.push(SessionMessage {
            role: "assistant".to_string(),
            content: rest.to_string(),
            ts: None,
        });
    }
    msgs
}

/// 消息 `type` → 角色：1 = 用户，2 = AI，3 = 系统，4 = 工具。
fn composer_message_role(msg_type: i64) -> &'static str {
    match msg_type {
        1 => "user",
        2 => "assistant",
        3 => "system",
        4 => "tool",
        _ => "user",
    }
}

/// 解析 ISO8601 时间戳（如 `2026-06-15T02:55:50.493Z`）为 epoch 毫秒。
fn parse_iso_ts(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// 从 composerData JSON 解析 `conversation` 数组为角色消息（**旧版**结构兼容）。
/// 消息 `type`：1 = 用户，2 = AI；`text` 为内容（空 text 的中间块跳过）。
fn parse_composer_messages(composer_json: &str) -> Vec<SessionMessage> {
    let value: Value = match serde_json::from_str(composer_json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(conversation) = value.get("conversation").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut msgs = Vec::new();
    for item in conversation {
        let msg_type = item.get("type").and_then(Value::as_i64).unwrap_or(0);
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if text.is_empty() {
            continue;
        }
        msgs.push(SessionMessage {
            role: composer_message_role(msg_type).to_string(),
            content: text.to_string(),
            ts: None,
        });
    }
    msgs
}

/// 从 composerData JSON 解析 `fullConversationHeadersOnly` 有序消息头。
/// 返回 `(bubbleId, type)` 列表（按会话顺序）。
fn parse_composer_headers(composer_json: &str) -> Vec<(String, i64)> {
    let value: Value = match serde_json::from_str(composer_json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(headers) = value
        .get("fullConversationHeadersOnly")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for header in headers {
        let Some(bubble_id) = header.get("bubbleId").and_then(Value::as_str) else {
            continue;
        };
        let msg_type = header.get("type").and_then(Value::as_i64).unwrap_or(0);
        out.push((bubble_id.to_string(), msg_type));
    }
    out
}

/// 按消息头顺序读取每条 `bubbleId:<composerId>:<bubbleId>` 正文，构造角色消息。
/// 返回 None 表示无可用消息头或无任何可展示文本。
fn load_messages_from_headers(
    conn: &Connection,
    session_id: &str,
    composer_json: &str,
) -> Option<Vec<SessionMessage>> {
    let headers = parse_composer_headers(composer_json);
    if headers.is_empty() {
        return None;
    }

    let mut msgs = Vec::new();
    for (bubble_id, header_type) in headers {
        let bubble_json: Option<String> = conn
            .query_row(
                "SELECT value FROM cursorDiskKV WHERE key = ?1",
                [format!("bubbleId:{session_id}:{bubble_id}")],
                |row| row.get(0),
            )
            .unwrap_or(None);
        let Some(bubble_json) = bubble_json else {
            continue;
        };
        let bubble: Value = match serde_json::from_str(&bubble_json) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let text = bubble
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if text.is_empty() {
            continue;
        }

        let msg_type = bubble
            .get("type")
            .and_then(Value::as_i64)
            .unwrap_or(header_type);
        let ts = bubble
            .get("createdAt")
            .and_then(Value::as_str)
            .and_then(parse_iso_ts);

        msgs.push(SessionMessage {
            role: composer_message_role(msg_type).to_string(),
            content: text.to_string(),
            ts,
        });
    }

    if msgs.is_empty() {
        None
    } else {
        Some(msgs)
    }
}

/// 从 state.vscdb `cursorDiskKV` 读取结构化会话消息。
/// 首选：`composerData:<id>` 的 `fullConversationHeadersOnly` + `bubbleId:` 正文；
/// 次选：旧版 `conversation` 数组。返回 None 表示无可用 composer 数据（回退 FTS）。
fn load_messages_from_composer(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<Vec<SessionMessage>>, String> {
    let composer_json: Option<String> = conn
        .query_row(
            "SELECT value FROM cursorDiskKV WHERE key = ?1",
            [format!("composerData:{session_id}")],
            |row| row.get(0),
        )
        .map_err(|e| format!("读取 Cursor composerData 失败: {e}"))?;
    let Some(composer_json) = composer_json else {
        return Ok(None);
    };

    if let Some(msgs) = load_messages_from_headers(conn, session_id, &composer_json) {
        return Ok(Some(msgs));
    }

    let legacy_msgs = parse_composer_messages(&composer_json);
    if !legacy_msgs.is_empty() {
        return Ok(Some(legacy_msgs));
    }

    Ok(None)
}

/// 加载会话消息：优先从 state.vscdb 的 `cursorDiskKV` 读取结构化消息
/// （`composerData.<fullConversationHeadersOnly>` → `bubbleId:` 正文，准确区分角色）；
/// 无 composer 数据时回退到 conversation-search.db 的 FTS 扁平转录（启发式拆分）。
pub fn load_messages_sqlite(source: &str) -> Result<Vec<SessionMessage>, String> {
    let (db_path, session_id) =
        parse_sqlite_source(source).ok_or_else(|| format!("无效的 Cursor 会话引用: {source}"))?;

    // 1) 结构化 composer 消息（state.vscdb cursorDiskKV）
    let state_db = get_state_vscdb_path();
    if state_db.exists() {
        if let Ok(conn) = open_readonly(&state_db) {
            match load_messages_from_composer(&conn, &session_id) {
                Ok(Some(msgs)) => return Ok(msgs),
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }
    }

    // 2) 回退：conversation-search.db FTS 扁平转录
    let conn = open_readonly(&db_path)?;
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM conversation_fts \
             WHERE rowid = (SELECT fts_rowid FROM conversations WHERE id = ?1)",
            [session_id.as_str()],
            |row| row.get(0),
        )
        .map_err(|e| format!("读取 Cursor 会话转录失败: {e}"))?;

    let body = body.unwrap_or_default();
    if body.trim().is_empty() {
        return Err(format!("会话 {session_id} 无转录内容"));
    }

    let messages = split_transcript_into_turns(&body);
    if messages.is_empty() {
        return Err(format!("会话 {session_id} 无可展示的转录内容"));
    }
    Ok(messages)
}

/// 从会话库删除会话（conversations 行 + 对应 FTS 条目，rowid 对齐）。
/// 仅允许删除本机的 conversation-search.db；会话不存在返回 Ok(false)。
pub fn delete_session_sqlite(session_id: &str, source: &str) -> Result<bool, String> {
    let (db_path, ref_session_id) =
        parse_sqlite_source(source).ok_or_else(|| format!("无效的 Cursor 会话引用: {source}"))?;
    if ref_session_id != session_id {
        return Err(format!(
            "Cursor 会话 ID 不匹配: expected {session_id}, found {ref_session_id}"
        ));
    }

    let expected_db = get_conversation_db_path()
        .canonicalize()
        .map_err(|e| format!("解析 Cursor 会话库路径失败: {e}"))?;
    let actual_db = db_path
        .canonicalize()
        .map_err(|e| format!("解析删除目标路径失败: {e}"))?;
    if actual_db != expected_db {
        return Err("删除路径不是本机 conversation-search.db".to_string());
    }

    let conn = Connection::open(&db_path).map_err(|e| format!("打开 Cursor 会话库失败: {e}"))?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("开始事务失败: {e}"))?;

    // 删除 FTS 条目（先删，避免孤儿索引）；rowid 与 conversations.fts_rowid 对齐
    tx.execute(
        "DELETE FROM conversation_fts WHERE rowid = \
         (SELECT fts_rowid FROM conversations WHERE id = ?1)",
        [session_id],
    )
    .map_err(|e| format!("删除 Cursor 会话索引失败: {e}"))?;

    let deleted = tx
        .execute("DELETE FROM conversations WHERE id = ?1", [session_id])
        .map_err(|e| format!("删除 Cursor 会话失败: {e}"))?;

    tx.commit().map_err(|e| format!("提交删除失败: {e}"))?;

    Ok(deleted > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn cursor_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn create_schema(conn: &Connection) {
        conn.execute_batch(
            "
            CREATE VIRTUAL TABLE conversation_fts USING fts5(title, body);
            CREATE TABLE conversations (
                fts_rowid INTEGER PRIMARY KEY,
                source TEXT NOT NULL,
                scope TEXT NOT NULL,
                id TEXT NOT NULL,
                title TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                is_archived INTEGER NOT NULL,
                root_fingerprint TEXT,
                cache_fingerprint TEXT
            );
            ",
        )
        .expect("create cursor session schema");
    }

    #[allow(clippy::too_many_arguments)]
    fn seed_conversation(
        conn: &Connection,
        fts_rowid: i64,
        id: &str,
        title: &str,
        updated_at: i64,
        is_archived: bool,
        body: &str,
    ) {
        conn.execute(
            "INSERT INTO conversation_fts(rowid, title, body) VALUES (?1, ?2, ?3)",
            rusqlite::params![fts_rowid, title, body],
        )
        .expect("insert fts");
        conn.execute(
            "INSERT INTO conversations(fts_rowid, source, scope, id, title, updated_at, is_archived, root_fingerprint, cache_fingerprint) \
             VALUES (?1, 'local', '', ?2, ?3, ?4, ?5, 'fp', NULL)",
            rusqlite::params![fts_rowid, id, title, updated_at, is_archived as i64],
        )
        .expect("insert conversation");
    }

    /// 在临时目录建会话库，设置 `CC_SWITCH_CURSOR_SESSION_DB`（conversation-search.db）与
    /// `CC_SWITCH_CURSOR_STATE_DB`（state.vscdb）指向临时路径（state 默认不存在 → 走回退路径）。
    /// Drop 时恢复环境变量，panic 也不会污染后续测试。
    fn with_temp_db<T>(test: impl FnOnce(PathBuf, PathBuf) -> T) -> T {
        let _guard = cursor_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("conversation-search.db");
        let state_path = temp.path().join("state.vscdb");
        let conn = Connection::open(&db_path).expect("open db");
        create_schema(&conn);
        drop(conn);

        let old_session = std::env::var_os("CC_SWITCH_CURSOR_SESSION_DB");
        let old_state = std::env::var_os("CC_SWITCH_CURSOR_STATE_DB");
        #[allow(deprecated)]
        std::env::set_var("CC_SWITCH_CURSOR_SESSION_DB", &db_path);
        #[allow(deprecated)]
        std::env::set_var("CC_SWITCH_CURSOR_STATE_DB", &state_path);

        struct RestoreEnv(Option<std::ffi::OsString>, Option<std::ffi::OsString>);
        impl Drop for RestoreEnv {
            fn drop(&mut self) {
                for (var, value) in [
                    ("CC_SWITCH_CURSOR_SESSION_DB", &self.0),
                    ("CC_SWITCH_CURSOR_STATE_DB", &self.1),
                ] {
                    match value {
                        Some(v) => {
                            #[allow(deprecated)]
                            std::env::set_var(var, v);
                        }
                        None => {
                            #[allow(deprecated)]
                            std::env::remove_var(var);
                        }
                    }
                }
            }
        }
        let _restore = RestoreEnv(old_session, old_state);
        test(db_path.clone(), state_path.clone())
    }

    const UUID_1: &str = "f531dd2f-e7dd-4090-a36e-2a534618bd7f";
    const UUID_2: &str = "505a66c0-3c61-4be5-b755-c5f774aea1a1";
    const BUBBLE_1: &str = "b1-00000000-0000-0000-0000-000000000001";
    const BUBBLE_2: &str = "b2-00000000-0000-0000-0000-000000000002";

    #[test]
    fn scan_sessions_reads_non_archived_sorted() {
        with_temp_db(|db_path, _state_path| {
            let conn = Connection::open(&db_path).expect("open");
            seed_conversation(&conn, 1, UUID_1, "First", 1_000, false, "prompt1");
            seed_conversation(&conn, 2, UUID_2, "Second", 3_000, false, "prompt2");
            seed_conversation(
                &conn,
                3,
                "f0000000-0000-0000-0000-000000000000",
                "Archived",
                9_999,
                true,
                "prompt3",
            );
            drop(conn);

            let sessions = scan_sessions();
            assert_eq!(sessions.len(), 2, "archived sessions must be skipped");
            assert_eq!(sessions[0].session_id, UUID_2, "newest first");
            assert_eq!(sessions[0].title.as_deref(), Some("Second"));
            assert_eq!(sessions[0].last_active_at, Some(3_000));
            assert_eq!(sessions[0].provider_id, "cursor");
            let expected_source = format!("sqlite:{}:{UUID_2}", db_path.display());
            assert_eq!(
                sessions[0].source_path.as_deref(),
                Some(expected_source.as_str())
            );
        });
    }

    #[test]
    fn scan_sessions_returns_empty_when_no_sessions() {
        with_temp_db(|_db, _state_path| {
            let sessions = scan_sessions();
            assert!(
                sessions.is_empty(),
                "empty db must yield empty session list"
            );
        });
    }

    #[test]
    fn load_messages_sqlite_splits_user_and_assistant_turns() {
        with_temp_db(|db_path, _state_path| {
            let conn = Connection::open(&db_path).expect("open");
            seed_conversation(
                &conn,
                1,
                UUID_1,
                "First",
                1_234,
                false,
                "帮我查一下权限\n好的，已查出如下结果",
            );
            drop(conn);

            let source = format!("sqlite:{}:{UUID_1}", db_path.display());
            let msgs = load_messages_sqlite(&source).expect("load");
            assert_eq!(msgs.len(), 2, "transcript must split into user + assistant");
            assert_eq!(msgs[0].role, "user");
            assert_eq!(msgs[0].content, "帮我查一下权限");
            assert_eq!(msgs[1].role, "assistant");
            assert_eq!(msgs[1].content, "好的，已查出如下结果");
        });
    }

    #[test]
    fn split_transcript_single_line_is_user_only() {
        let msgs = split_transcript_into_turns("只有一句话");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "只有一句话");
    }

    #[test]
    fn split_transcript_uses_blank_line_for_multi_line_user_prompt() {
        let msgs = split_transcript_into_turns("第一行提示\n第二行提示\n\nAI 回复内容");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "第一行提示\n第二行提示");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "AI 回复内容");
    }

    #[test]
    fn split_transcript_empty_returns_empty() {
        assert!(split_transcript_into_turns("   ").is_empty());
        assert!(split_transcript_into_turns("").is_empty());
    }

    #[test]
    fn parse_composer_messages_separates_user_and_assistant_roles() {
        let json = r#"{
            "composerId": "abc",
            "conversation": [
                {"type": 1, "text": "fix bug"},
                {"type": 2, "text": "Based on the error, here's the fix..."},
                {"type": 2, "text": ""},
                {"type": 1, "text": "还是不行"}
            ]
        }"#;
        let msgs = parse_composer_messages(json);
        assert_eq!(msgs.len(), 3, "empty intermediate chunks must be skipped");
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "fix bug");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "Based on the error, here's the fix...");
        assert_eq!(msgs[2].role, "user");
        assert_eq!(msgs[2].content, "还是不行");
    }

    #[test]
    fn parse_composer_messages_handles_invalid_or_empty() {
        assert!(parse_composer_messages("not json").is_empty());
        assert!(parse_composer_messages(r#"{"conversation":[]}"#).is_empty());
        assert!(parse_composer_messages(r#"{"composerId":"x"}"#).is_empty());
    }

    #[test]
    fn load_messages_prefers_composer_bubble_messages() {
        with_temp_db(|db_path, state_path| {
            // 建 state.vscdb（cursorDiskKV）：composerData 头 + bubbleId 正文
            let state_conn = Connection::open(&state_path).expect("open state db");
            state_conn
                .execute_batch("CREATE TABLE cursorDiskKV (key TEXT, value TEXT);")
                .expect("create cursorDiskKV");
            state_conn
                .execute(
                    "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
                    rusqlite::params![
                        format!("composerData:{UUID_1}"),
                        format!(
                            r#"{{"composerId":"x","fullConversationHeadersOnly":[
                                {{"bubbleId":"{BUBBLE_1}","type":1}},
                                {{"bubbleId":"{BUBBLE_2}","type":2}}
                            ]}}"#
                        )
                    ],
                )
                .expect("insert composerData");
            state_conn
                .execute(
                    "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
                    rusqlite::params![
                        format!("bubbleId:{UUID_1}:{BUBBLE_1}"),
                        r#"{"_v":3,"type":1,"text":"user1","createdAt":"2026-06-15T02:55:50.493Z"}"#
                    ],
                )
                .expect("insert bubble1");
            state_conn
                .execute(
                    "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
                    rusqlite::params![
                        format!("bubbleId:{UUID_1}:{BUBBLE_2}"),
                        r#"{"_v":3,"type":2,"text":"ai1","createdAt":"2026-06-15T02:55:51.000Z"}"#
                    ],
                )
                .expect("insert bubble2");
            drop(state_conn);

            // conversation-search.db 也建（供 id 关联 + 回退转录）
            let conn = Connection::open(&db_path).expect("open");
            seed_conversation(
                &conn,
                1,
                UUID_1,
                "First",
                1_234,
                false,
                "fallback transcript",
            );
            drop(conn);

            let source = format!("sqlite:{}:{UUID_1}", db_path.display());
            let msgs = load_messages_sqlite(&source).expect("load");
            assert_eq!(
                msgs.len(),
                2,
                "composer bubble messages must take precedence"
            );
            assert_eq!(msgs[0].role, "user");
            assert_eq!(msgs[0].content, "user1");
            assert_eq!(msgs[1].role, "assistant");
            assert_eq!(msgs[1].content, "ai1");
            assert_eq!(msgs[0].ts, Some(1_781_492_150_493));
            assert_eq!(msgs[1].ts, Some(1_781_492_151_000));
        });
    }

    #[test]
    fn load_messages_falls_back_to_conversation_array_in_composer() {
        with_temp_db(|db_path, state_path| {
            // 旧版 composerData 直接含 conversation 数组
            let state_conn = Connection::open(&state_path).expect("open state db");
            state_conn
                .execute_batch("CREATE TABLE cursorDiskKV (key TEXT, value TEXT);")
                .expect("create cursorDiskKV");
            state_conn
                .execute(
                    "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
                    rusqlite::params![
                        format!("composerData:{UUID_1}"),
                        r#"{"composerId":"x","conversation":[{"type":1,"text":"legacy user"},{"type":2,"text":"legacy ai"}]}"#
                    ],
                )
                .expect("insert composerData");
            drop(state_conn);

            let conn = Connection::open(&db_path).expect("open");
            seed_conversation(
                &conn,
                1,
                UUID_1,
                "First",
                1_234,
                false,
                "fallback transcript",
            );
            drop(conn);

            let source = format!("sqlite:{}:{UUID_1}", db_path.display());
            let msgs = load_messages_sqlite(&source).expect("load");
            assert_eq!(msgs.len(), 2);
            assert_eq!(msgs[0].role, "user");
            assert_eq!(msgs[0].content, "legacy user");
            assert_eq!(msgs[1].role, "assistant");
            assert_eq!(msgs[1].content, "legacy ai");
        });
    }

    #[test]
    fn parse_composer_headers_extracts_bubble_ids_and_types() {
        let json = r#"{
            "composerId": "x",
            "fullConversationHeadersOnly": [
                {"bubbleId": "b-1", "type": 1, "grouping": {}},
                {"bubbleId": "b-2", "type": 2}
            ]
        }"#;
        let headers = parse_composer_headers(json);
        assert_eq!(
            headers,
            vec![("b-1".to_string(), 1), ("b-2".to_string(), 2)]
        );
        assert!(parse_composer_headers("not json").is_empty());
        assert!(parse_composer_headers(r#"{"composerId":"x"}"#).is_empty());
    }

    #[test]
    fn parse_iso_ts_parses_rfc3339() {
        assert_eq!(
            parse_iso_ts("2020-01-01T00:00:00.000Z"),
            Some(1_577_836_800_000)
        );
        assert_eq!(
            parse_iso_ts("2020-01-01T00:00:00Z"),
            Some(1_577_836_800_000)
        );
        assert_eq!(parse_iso_ts("garbage"), None);
    }

    #[test]
    fn load_messages_falls_back_to_transcript_without_composer() {
        with_temp_db(|db_path, _state_path| {
            let conn = Connection::open(&db_path).expect("open");
            seed_conversation(
                &conn,
                1,
                UUID_1,
                "First",
                1_234,
                false,
                "帮我查一下\n好的，已查出",
            );
            drop(conn);

            let source = format!("sqlite:{}:{UUID_1}", db_path.display());
            let msgs = load_messages_sqlite(&source).expect("load");
            assert_eq!(msgs.len(), 2, "fallback to FTS transcript split");
            assert_eq!(msgs[0].role, "user");
            assert_eq!(msgs[1].role, "assistant");
        });
    }

    #[test]
    fn load_messages_sqlite_rejects_missing_session() {
        with_temp_db(|db_path, _state_path| {
            let source = format!("sqlite:{}:{UUID_1}", db_path.display());
            assert!(load_messages_sqlite(&source).is_err());
        });
    }

    #[test]
    fn delete_session_sqlite_removes_conversation_and_fts() {
        with_temp_db(|db_path, _state_path| {
            let conn = Connection::open(&db_path).expect("open");
            seed_conversation(&conn, 1, UUID_1, "First", 1_000, false, "prompt");
            drop(conn);

            let source = format!("sqlite:{}:{UUID_1}", db_path.display());
            let deleted = delete_session_sqlite(UUID_1, &source).expect("delete");
            assert!(deleted);

            let conn = Connection::open(&db_path).expect("reopen");
            let conv_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM conversations WHERE id = ?1",
                    [UUID_1],
                    |r| r.get(0),
                )
                .expect("count conversations");
            let fts_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM conversation_fts WHERE rowid = 1",
                    [],
                    |r| r.get(0),
                )
                .expect("count fts");
            assert_eq!(conv_count, 0);
            assert_eq!(fts_count, 0);
        });
    }

    #[test]
    fn delete_session_sqlite_returns_false_when_session_missing() {
        with_temp_db(|db_path, _state_path| {
            let source = format!("sqlite:{}:{UUID_1}", db_path.display());
            let deleted = delete_session_sqlite(UUID_1, &source).expect("delete");
            assert!(!deleted, "missing session should return Ok(false)");
        });
    }

    #[test]
    fn delete_session_sqlite_rejects_foreign_db_path() {
        with_temp_db(|db_path, _state_path| {
            let foreign = db_path.with_file_name("foreign.db");
            std::fs::write(&foreign, b"").expect("create foreign db file");
            let source = format!("sqlite:{}:{UUID_1}", foreign.display());
            let err =
                delete_session_sqlite(UUID_1, &source).expect_err("foreign db must be rejected");
            assert!(err.contains("conversation-search.db"), "got: {err}");
        });
    }
}
