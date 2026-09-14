//! MCode TUI and desktop share the local runtime database.
use crate::session_manager::{SessionMessage, SessionMeta};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::PathBuf;

pub(crate) fn database_path() -> PathBuf {
    crate::mcode_config::data_dir().join("v2/sqlite/runtime-state.sqlite")
}

pub(crate) fn open_database() -> rusqlite::Result<Connection> {
    Connection::open_with_flags(database_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    if !database_path().exists() {
        return vec![];
    }
    match open_database().and_then(|conn| scan(&conn)) {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("Cannot read MCode sessions: {error}");
            vec![]
        }
    }
}

fn scan(conn: &Connection) -> rusqlite::Result<Vec<SessionMeta>> {
    let mut query = conn.prepare(
        "SELECT session_id, title, workspace_dir, created_at_ms, updated_at_ms
         FROM local_runtime_sessions WHERE visibility <> 'hidden'
         AND parent_session_id IS NULL AND session_kind NOT IN ('peek', 'channel', 'cron')
         ORDER BY updated_at_ms DESC",
    )?;
    let rows = query.query_map([], |row| {
        let id: String = row.get(0)?;
        Ok(SessionMeta {
            provider_id: "mcode".into(),
            source_path: Some(format!("mcode:{id}")),
            resume_command: id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                .then(|| format!("mcode --session {id}")),
            session_id: id,
            title: row.get(1)?,
            summary: None,
            project_dir: row.get(2)?,
            created_at: row.get(3)?,
            last_active_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn load_messages(source: &str) -> Result<Vec<SessionMessage>, String> {
    let id = source
        .strip_prefix("mcode:")
        .ok_or("Invalid MCode session source")?;
    let conn = open_database().map_err(|e| e.to_string())?;
    read_messages(&conn, id).map_err(|e| e.to_string())
}

fn read_messages(conn: &Connection, id: &str) -> rusqlite::Result<Vec<SessionMessage>> {
    let mut query = conn.prepare(
        "SELECT role, data_json, created_at_ms FROM local_runtime_message_rows
         WHERE session_id = ?1 AND role IN ('user', 'assistant') ORDER BY id",
    )?;
    let rows = query.query_map([id], |row| {
        let data: String = row.get(1)?;
        let value: Value = serde_json::from_str(&data).unwrap_or_default();
        Ok(SessionMessage {
            role: row.get(0)?,
            content: super::utils::extract_text(&value["msg_content"]),
            ts: row.get(2)?,
        })
    })?;
    rows.filter_map(|r| match r {
        Ok(m) if m.content.trim().is_empty() => None,
        other => Some(other),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mcode_history_uses_visible_roots_and_display_messages() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE local_runtime_sessions (
            session_id TEXT, title TEXT, workspace_dir TEXT, created_at_ms INTEGER,
            updated_at_ms INTEGER, visibility TEXT, parent_session_id TEXT, session_kind TEXT);
            INSERT INTO local_runtime_sessions VALUES
            ('mvs_public','Project','/work',100,200,'visible',NULL,'conversation'),
            ('mvs_child','Child','/work',100,200,'visible','mvs_public','task'),
            ('mvs_hidden','Hidden','/work',100,200,'hidden',NULL,'conversation');
            CREATE TABLE local_runtime_message_rows (id INTEGER, session_id TEXT, role TEXT, data_json TEXT, created_at_ms INTEGER);
            INSERT INTO local_runtime_message_rows VALUES
            (1,'mvs_public','user','{\"msg_content\":\"Fix this project\"}',100),
            (2,'mvs_public','assistant','{\"msg_content\":\"Tests passed\"}',200);").unwrap();
        let sessions = scan(&conn).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].resume_command.as_deref(),
            Some("mcode --session mvs_public")
        );
        let messages = read_messages(&conn, "mvs_public").unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content, "Tests passed");
        assert_eq!(messages[1].ts, Some(200));
    }
}
