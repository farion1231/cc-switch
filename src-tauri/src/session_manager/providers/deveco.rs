//! DevEco Code 本地会话（只读）
//!
//! DevEco Code 是 OpenCode 的二次开发分支，同样把会话存在 SQLite 里，但**表结构
//! 与 OpenCode 不同**：DevEco 把消息正文拆成 `message`（角色/模型/token）与
//! `part`（正文片段）两张表，并用 `session.parent_id` 表达子会话（子智能体）。
//!
//! 因此这里只复用 OpenCode 的 `sqlite:` 源引用约定与 part 文本抽取，扫描与读取
//! 逻辑按 DevEco 的列结构重写。

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::opencode::extract_part_text;
use super::utils::{path_basename, truncate_summary};

const PROVIDER_ID: &str = "deveco";

/// 以只读方式打开 DevEco Code 的会话数据库。
///
/// DevEco Code 使用 WAL，桌面端可能正在写入；只读打开不会干扰它。
pub(crate) fn open_database() -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        crate::deveco_config::get_deveco_db_path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

pub(crate) fn database_path() -> PathBuf {
    crate::deveco_config::get_deveco_db_path()
}

/// 构造恢复会话的命令。
///
/// DevEco Code 用 `--session <id>` 继续指定会话（`-s` 是它的别名）。
fn resume_command(session_id: &str) -> Option<String> {
    // 会话 id 由 DevEco 生成，但仍做一次白名单校验再拼进 shell 命令。
    session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .then(|| format!("deveco --session {session_id}"))
}

/// 扫描 DevEco Code 的根会话。
///
/// 只列出用户可感知的会话：跳过子智能体产生的子会话（`parent_id` 非空）与已归档
/// 会话，与 DevEco Code 自身的会话列表口径一致。
pub fn scan_sessions() -> Vec<SessionMeta> {
    let db_path = database_path();
    if !db_path.exists() {
        return Vec::new();
    }

    match open_database().and_then(|conn| scan(&conn)) {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("Cannot read DevEco Code sessions: {error}");
            Vec::new()
        }
    }
}

fn scan(conn: &Connection) -> rusqlite::Result<Vec<SessionMeta>> {
    let mut query = conn.prepare(
        "SELECT id, title, directory, time_created, time_updated
         FROM session
         WHERE parent_id IS NULL AND time_archived IS NULL
         ORDER BY time_updated DESC",
    )?;

    let rows = query.query_map([], |row| {
        let session_id: String = row.get(0)?;
        let title: String = row.get(1)?;
        let directory: String = row.get(2)?;
        Ok((
            session_id,
            title,
            directory,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        let (session_id, title, directory, created, updated) = row?;

        // 原生配置允许 title 为空，此时用项目目录名兜底。
        let display_title = if title.trim().is_empty() {
            path_basename(&directory)
        } else {
            Some(title)
        };

        sessions.push(SessionMeta {
            provider_id: PROVIDER_ID.to_string(),
            session_id: session_id.clone(),
            title: display_title.clone(),
            summary: display_title,
            project_dir: (!directory.is_empty()).then_some(directory),
            created_at: Some(created),
            last_active_at: Some(updated),
            source_path: Some(format!("sqlite:{}:{session_id}", database_path().display())),
            resume_command: resume_command(&session_id),
        });
    }

    Ok(sessions)
}

/// 读取指定会话的消息。
///
/// `source` 形如 `sqlite:<db_path>:<session_id>`。
pub fn load_messages(source: &str) -> Result<Vec<SessionMessage>, String> {
    let (db_path, session_id) = super::opencode::parse_sqlite_source(source)
        .ok_or_else(|| format!("Invalid DevEco Code session source: {source}"))?;

    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open DevEco Code database: {e}"))?;

    read_messages(&conn, &session_id).map_err(|e| e.to_string())
}

fn read_messages(conn: &Connection, session_id: &str) -> rusqlite::Result<Vec<SessionMessage>> {
    let mut msg_stmt = conn.prepare(
        "SELECT id, time_created, data FROM message WHERE session_id = ?1 ORDER BY time_created ASC",
    )?;
    let msg_rows = msg_stmt.query_map([session_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    // 一次取完该会话的所有 part，按 message_id 分组后再拼接，
    // 避免 N+1 查询。
    let mut part_stmt = conn.prepare(
        "SELECT message_id, data FROM part WHERE session_id = ?1 ORDER BY time_created ASC",
    )?;
    let part_rows = part_stmt.query_map([session_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut parts_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for part in part_rows {
        let (message_id, data) = part?;
        parts_map.entry(message_id).or_default().push(data);
    }

    let mut messages = Vec::new();
    for row in msg_rows {
        let (msg_id, ts, data) = row?;
        let msg_value: Value = match serde_json::from_str(&data) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let role = msg_value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let mut texts = Vec::new();
        for part_data in parts_map.get(&msg_id).into_iter().flatten() {
            if let Ok(part_value) = serde_json::from_str::<Value>(part_data) {
                if let Some(text) = extract_part_text(&part_value) {
                    texts.push(text);
                }
            }
        }

        let content = texts.join("\n");
        if content.trim().is_empty() {
            continue;
        }

        messages.push(SessionMessage {
            role,
            content,
            ts: Some(ts),
        });
    }

    Ok(messages)
}

/// 从首条用户消息抽取一段摘要，供列表页在无标题时使用。
#[allow(dead_code)]
pub(crate) fn first_user_summary(messages: &[SessionMessage]) -> Option<String> {
    messages
        .iter()
        .find(|message| message.role == "user")
        .map(|message| truncate_summary(&message.content, 160))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_schema(conn: &Connection) {
        // 列取自本机 deveco.db 的真实 DDL（按测试需要裁剪到用到的列）。
        conn.execute_batch(
            "
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                title TEXT NOT NULL,
                directory TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                time_archived INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            ",
        )
        .expect("create schema");
    }

    fn seed(conn: &Connection) {
        conn.execute(
            "INSERT INTO session (id, parent_id, title, directory, time_created, time_updated, time_archived)
             VALUES ('ses_root', NULL, 'Root Session', 'D:/proj-a', 1000, 3000, NULL)",
            [],
        )
        .expect("insert root session");
        conn.execute(
            "INSERT INTO session (id, parent_id, title, directory, time_created, time_updated, time_archived)
             VALUES ('ses_child', 'ses_root', 'Subagent run', 'D:/proj-a', 1500, 2500, NULL)",
            [],
        )
        .expect("insert child session");
        conn.execute(
            "INSERT INTO session (id, parent_id, title, directory, time_created, time_updated, time_archived)
             VALUES ('ses_archived', NULL, 'Archived', 'D:/proj-b', 500, 500, 900)",
            [],
        )
        .expect("insert archived session");
        conn.execute(
            "INSERT INTO session (id, parent_id, title, directory, time_created, time_updated, time_archived)
             VALUES ('ses_untitled', NULL, '', 'D:/proj-c', 800, 900, NULL)",
            [],
        )
        .expect("insert untitled session");

        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES ('msg_1', 'ses_root', 1000, 1000, '{\"role\":\"user\"}')",
            [],
        )
        .expect("insert user message");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES ('msg_2', 'ses_root', 2000, 2000, '{\"role\":\"assistant\"}')",
            [],
        )
        .expect("insert assistant message");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_1', 'msg_1', 'ses_root', 1000, 1000, '{\"type\":\"text\",\"text\":\"Hello\"}')",
            [],
        )
        .expect("insert text part");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_2', 'msg_2', 'ses_root', 2000, 2000, '{\"type\":\"tool\",\"tool\":\"bash\"}')",
            [],
        )
        .expect("insert tool part");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_3', 'msg_2', 'ses_root', 2001, 2001, '{\"type\":\"text\",\"text\":\"Done\"}')",
            [],
        )
        .expect("insert text part 2");
    }

    fn db_path(temp: &tempfile::TempDir) -> std::path::PathBuf {
        let path = temp.path().join("deveco.db");
        let conn = Connection::open(&path).expect("open db");
        create_schema(&conn);
        seed(&conn);
        path
    }

    #[test]
    fn scan_lists_root_sessions_only_and_falls_back_to_directory_name() {
        let temp = tempdir().expect("tempdir");
        let path = db_path(&temp);

        let conn = Connection::open(&path).expect("open");
        let sessions = scan(&conn).expect("scan");

        // 子会话与已归档会话都不应出现
        let ids: Vec<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert!(!ids.contains(&"ses_child"), "子会话应被跳过");
        assert!(!ids.contains(&"ses_archived"), "已归档会话应被跳过");
        assert_eq!(
            ids,
            vec!["ses_root", "ses_untitled"],
            "按 time_updated 降序"
        );

        let untitled = sessions
            .iter()
            .find(|s| s.session_id == "ses_untitled")
            .expect("untitled session");
        assert_eq!(
            untitled.title.as_deref(),
            Some("proj-c"),
            "空标题应回退为目录名"
        );
        assert_eq!(untitled.project_dir.as_deref(), Some("D:/proj-c"));
    }

    #[test]
    fn resume_command_uses_the_session_flag() {
        assert_eq!(
            resume_command("ses_abc-123").as_deref(),
            Some("deveco --session ses_abc-123")
        );
        assert_eq!(
            resume_command("ses_abc;rm -rf /"),
            None,
            "非法 id 不得拼进命令"
        );
    }

    #[test]
    fn read_messages_joins_parts_in_time_order() {
        let temp = tempdir().expect("tempdir");
        let path = db_path(&temp);
        let conn = Connection::open(&path).expect("open");

        let messages = read_messages(&conn, "ses_root").expect("read messages");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[0].ts, Some(1000));
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "[Tool: bash]\nDone");
        assert_eq!(messages[1].ts, Some(2000));
    }
}
