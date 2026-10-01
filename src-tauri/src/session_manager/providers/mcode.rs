//! MCode TUI and desktop share the local runtime database.
use crate::session_manager::{SessionMessage, SessionMeta};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) fn database_path() -> PathBuf {
    crate::mcode_config::data_dir().join("v2/sqlite/runtime-state.sqlite")
}

pub(crate) fn open_database() -> rusqlite::Result<Connection> {
    Connection::open_with_flags(database_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let Ok(data_dir) = std::path::absolute(crate::mcode_config::data_dir()) else {
        return vec![];
    };
    let database = data_dir.join("v2/sqlite/runtime-state.sqlite");
    if !database.exists() {
        return vec![];
    }
    match Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|conn| scan(&conn, &data_dir))
    {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("Cannot read MCode sessions: {error}");
            vec![]
        }
    }
}

fn scan(conn: &Connection, data_dir: &Path) -> rusqlite::Result<Vec<SessionMeta>> {
    #[cfg(not(windows))]
    let command = format!(
        "env MINIMAX_DATA_DIR={} mcode",
        crate::session_manager::terminal::shell_escape(&data_dir.to_string_lossy())
    );
    #[cfg(windows)]
    let command = format!(
        "$env:MINIMAX_DATA_DIR = '{}'; mcode",
        data_dir.to_string_lossy().replace('\'', "''")
    );
    let mut query = conn.prepare(
        "SELECT session_id, title, workspace_dir, created_at_ms, updated_at_ms
         FROM local_runtime_sessions WHERE visibility <> 'hidden' AND archived = 0
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
                .then(|| format!("{command} --session {id}")),
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
        "WITH migrated AS (
            SELECT 1 FROM local_runtime_message_row_migrations WHERE session_id = ?1
         ), display AS (
            SELECT role, data_json, created_at_ms, id AS sequence
            FROM local_runtime_message_rows
            WHERE session_id = ?1 AND EXISTS (SELECT 1 FROM migrated)
            UNION ALL
            SELECT json_extract(message.value, '$.role'), message.value, NULL, message.key
            FROM local_runtime_messages, json_each(display_messages_json) AS message
            WHERE session_id = ?1 AND NOT EXISTS (SELECT 1 FROM migrated)
         )
         SELECT role, data_json, created_at_ms FROM display
         WHERE role IN ('user', 'assistant') ORDER BY sequence",
    )?;
    let rows = query.query_map([id], |row| {
        let data: String = row.get(1)?;
        let value: Value = serde_json::from_str(&data).unwrap_or_default();
        Ok(SessionMessage {
            role: row.get(0)?,
            content: super::utils::extract_text(&value["msg_content"]),
            ts: row.get::<_, Option<i64>>(2)?.or_else(|| {
                let time = value
                    .get("timestamp")
                    .filter(|v| !v.is_null())
                    .or_else(|| value.get("created_at"))?;
                time.as_f64()
                    .or_else(|| time.as_str()?.parse::<f64>().ok())
                    .filter(|time| time.is_finite())
                    .map(|time| time.floor() as i64)
            }),
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
            updated_at_ms INTEGER, visibility TEXT, parent_session_id TEXT, session_kind TEXT, archived INTEGER);
            INSERT INTO local_runtime_sessions VALUES
            ('mvs_public','Project','/work',100,200,'visible',NULL,'conversation',0),
            ('mvs_child','Child','/work',100,200,'visible','mvs_public','task',0),
            ('mvs_hidden','Hidden','/work',100,200,'hidden',NULL,'conversation',0),
            ('mvs_archived','Archived','/work',100,200,'visible',NULL,'conversation',1);
            CREATE TABLE local_runtime_messages (session_id TEXT, display_messages_json TEXT);
            CREATE TABLE local_runtime_message_row_migrations (session_id TEXT);
            INSERT INTO local_runtime_message_row_migrations VALUES ('mvs_public');
            CREATE TABLE local_runtime_message_rows (id INTEGER, session_id TEXT, role TEXT, data_json TEXT, created_at_ms INTEGER);
            INSERT INTO local_runtime_message_rows VALUES
            (1,'mvs_public','user','{\"msg_content\":\"Fix this project\"}',100),
            (2,'mvs_public','assistant','{\"msg_content\":\"Tests passed\"}',200);").unwrap();
        let data_dir = Path::new("/tmp/MiniMax Code's $(printf expanded)");
        let sessions = scan(&conn, data_dir).unwrap();
        assert_eq!(sessions.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let bin = tempfile::tempdir().unwrap();
            let executable = bin.path().join("mcode");
            std::fs::write(
                &executable,
                "#!/bin/sh\nprintf '%s\\n' \"$MINIMAX_DATA_DIR\" \"$@\"\n",
            )
            .unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(sessions[0].resume_command.as_ref().unwrap())
                .env("PATH", format!("{}:/usr/bin:/bin", bin.path().display()))
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("{}\n--session\nmvs_public\n", data_dir.display())
            );
        }
        #[cfg(windows)]
        assert_eq!(sessions[0].resume_command.as_deref(),
            Some("$env:MINIMAX_DATA_DIR = '/tmp/MiniMax Code''s $(printf expanded)'; mcode --session mvs_public"));
        let messages = read_messages(&conn, "mvs_public").unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content, "Tests passed");
        assert_eq!(messages[1].ts, Some(200));

        let legacy = serde_json::json!([
            {"role":"user", "msg_content":"Legacy question", "timestamp":"100"},
            {"role":"assistant", "msg_content":"Legacy answer", "created_at":200},
            {"role":"tool", "msg_content":"Hidden tool output"}
        ])
        .to_string();
        conn.execute(
            "INSERT INTO local_runtime_messages VALUES (?1, ?2)",
            ["mvs_public", &legacy],
        )
        .unwrap();
        // A migrated session must ignore its legacy blob, even if it remains.
        assert_eq!(read_messages(&conn, "mvs_public").unwrap().len(), 2);
        conn.execute("DELETE FROM local_runtime_message_row_migrations", [])
            .unwrap();
        let messages = read_messages(&conn, "mvs_public").unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "Legacy question");
        assert_eq!(messages[0].ts, Some(100));
        assert_eq!(messages[1].content, "Legacy answer");
        assert_eq!(messages[1].ts, Some(200));
        assert_eq!(
            conn.query_row(
                "SELECT display_messages_json FROM local_runtime_messages",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            legacy
        );
    }
}
