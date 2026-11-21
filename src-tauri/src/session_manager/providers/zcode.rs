//! ZCode 的会话历史：`~/.zcode/cli/db/db.sqlite`（与用量同步共用一个库）。
//!
//! `session`/`message`/`part` 三表与 OpenCode 的 wire 格式同构（text/reasoning/
//! tool/step-*），解析直接复用 [`super::opencode_blocks`] 的 part → block 映射，
//! 只在读库和会话元数据上不同。库是 WAL 模式，只读连接照常读到未 checkpoint 的行；
//! 缓存指纹已把 `-wal` 的 mtime 并入。
//!
//! 源路径用 `zcode:<sessionId>`（同 MCode 的 `mcode:<id>` 方案）。

use super::blocks::assign_turn_ids;
use crate::session_manager::{SessionMessage, SessionMeta};
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;

pub(crate) fn database_path() -> PathBuf {
    crate::zcode_config::get_zcode_usage_db_path()
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let database = database_path();
    if !database.exists() {
        return vec![];
    }
    match Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|conn| scan(&conn))
    {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("Cannot read ZCode sessions: {error}");
            vec![]
        }
    }
}

fn scan(conn: &Connection) -> rusqlite::Result<Vec<SessionMeta>> {
    // subagent 子会话（parent_id / task_type = subagent_child）和已删除的
    // （time_deleted 软删）不进列表；ZCode 桌面没有 CLI，无法给出恢复命令。
    let mut query = conn.prepare(
        "SELECT id, title, directory, time_created, time_updated
         FROM session
         WHERE time_deleted IS NULL AND parent_id IS NULL
           AND task_type = 'interactive'
         ORDER BY time_updated DESC",
    )?;
    let rows = query.query_map([], |row| {
        let id: String = row.get(0)?;
        Ok(SessionMeta {
            provider_id: "zcode".into(),
            source_path: Some(format!("zcode:{id}")),
            resume_command: None,
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
        .strip_prefix("zcode:")
        .filter(|id| !id.is_empty())
        .ok_or("Invalid ZCode session source")?;
    let conn = Connection::open_with_flags(database_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    read_messages(&conn, id).map_err(|e| e.to_string())
}

fn read_messages(conn: &Connection, id: &str) -> rusqlite::Result<Vec<SessionMessage>> {
    let mut query = conn.prepare(
        "SELECT id, sequence, data FROM message
         WHERE session_id = ?1 AND json_extract(data, '$.role') IN ('user', 'assistant')
         ORDER BY sequence",
    )?;
    let rows = query.query_map([id], |row| {
        let message_id: String = row.get(0)?;
        let data: String = row.get(2)?;
        let info: serde_json::Value = serde_json::from_str(&data).unwrap_or_default();
        let role = info
            .get("role")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("user")
            .to_string();
        let ts = info
            .pointer("/time/created")
            .and_then(serde_json::Value::as_i64);
        let parts = load_parts(conn, &message_id, id)?;
        Ok(super::opencode_blocks::message_from_parts(
            &role,
            Some(message_id),
            ts,
            &info,
            &parts,
        ))
    })?;
    let mut messages = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    assign_turn_ids(&mut messages);
    Ok(messages)
}

fn load_parts(
    conn: &Connection,
    message_id: &str,
    session_id: &str,
) -> rusqlite::Result<Vec<(super::opencode_blocks::PartLocator, serde_json::Value)>> {
    let mut query = conn.prepare(
        "SELECT id, data FROM part WHERE message_id = ?1 AND session_id = ?2 ORDER BY sequence",
    )?;
    let rows = query.query_map([message_id, session_id], |row| {
        let part_id: String = row.get(0)?;
        let data: String = row.get(1)?;
        let value: serde_json::Value = serde_json::from_str(&data).unwrap_or_default();
        Ok((
            super::opencode_blocks::PartLocator::Sqlite {
                table: "part",
                id: part_id,
                base: String::new(),
            },
            value,
        ))
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zcode_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY, title TEXT, directory TEXT, time_created INTEGER,
                time_updated INTEGER, time_deleted INTEGER, parent_id TEXT, task_type TEXT);
            INSERT INTO session VALUES
                ('z_public','Public session','/work/zcode',100,300,NULL,NULL,'interactive'),
                ('z_child','Subagent child','/work/zcode',100,200,NULL,'z_public','subagent_child'),
                ('z_standalone_child','Standalone child','/work/zcode',100,200,NULL,NULL,'subagent_child'),
                ('z_deleted','Deleted','/work/zcode',100,200,250,NULL,'interactive');
            CREATE TABLE message (id TEXT, session_id TEXT, sequence INTEGER, data TEXT);
            INSERT INTO message VALUES
                ('m_user','z_public',0,'{\"role\":\"user\",\"time\":{\"created\":100}}'),
                ('m_asst','z_public',1,'{\"role\":\"assistant\",\"time\":{\"created\":150},\"modelID\":\"GLM-5.3\",\"providerID\":\"zai\",\"cost\":0.01,\"tokens\":{\"input\":10,\"output\":5,\"reasoning\":0,\"cache\":{\"read\":2,\"write\":1}},\"finish\":\"stop\"}');
            CREATE TABLE part (id TEXT, message_id TEXT, session_id TEXT, sequence INTEGER, data TEXT);
            INSERT INTO part VALUES
                ('p_text','m_user','z_public',0,'{\"type\":\"text\",\"text\":\"Fix the build\"}'),
                ('p_reason','m_asst','z_public',0,'{\"type\":\"reasoning\",\"text\":\"Check the logs first\"}'),
                ('p_body','m_asst','z_public',1,'{\"type\":\"text\",\"text\":\"Build fixed\"}'),
                ('p_tool','m_asst','z_public',2,'{\"type\":\"tool\",\"callID\":\"call_1\",\"tool\":\"Bash\",\"state\":{\"status\":\"completed\",\"input\":{\"command\":\"cargo test\"},\"output\":\"ok\"}}'),
                ('p_step','m_asst','z_public',3,'{\"type\":\"step-finish\",\"reason\":\"stop\",\"cost\":0.01}');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn scan_lists_only_visible_interactive_roots() {
        let sessions = scan(&zcode_db()).unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.session_id, "z_public");
        assert_eq!(session.title.as_deref(), Some("Public session"));
        assert_eq!(session.project_dir.as_deref(), Some("/work/zcode"));
        assert_eq!(session.created_at, Some(100));
        assert_eq!(session.last_active_at, Some(300));
        assert_eq!(session.source_path.as_deref(), Some("zcode:z_public"));
        assert!(session.resume_command.is_none());
    }

    #[test]
    fn read_messages_maps_parts_to_blocks() {
        let conn = zcode_db();
        let messages = read_messages(&conn, "z_public").unwrap();
        assert_eq!(messages.len(), 2);

        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Fix the build");

        assert_eq!(messages[1].role, "assistant");
        // text + reasoning + tool + step 都映射进 blocks，内容投影包含正文
        assert!(messages[1].content.contains("Build fixed"));
        let block_kinds: Vec<&str> = messages[1]
            .blocks
            .iter()
            .map(|block| match block {
                crate::session_manager::model::SessionBlock::Text { .. } => "text",
                crate::session_manager::model::SessionBlock::Thinking { .. } => "thinking",
                crate::session_manager::model::SessionBlock::ToolCall { .. } => "tool",
                crate::session_manager::model::SessionBlock::ToolResult { .. } => "result",
                crate::session_manager::model::SessionBlock::Step { .. } => "step",
                _ => "other",
            })
            .collect();
        assert!(block_kinds.contains(&"text"));
        assert!(block_kinds.contains(&"thinking"));
        assert!(block_kinds.contains(&"tool"));
        let meta = messages[1].meta.as_ref().unwrap();
        assert_eq!(meta.model.as_deref(), Some("zai/GLM-5.3"));
        assert_eq!(meta.input_tokens, Some(10));
        assert_eq!(meta.cache_read_tokens, Some(2));
        assert_eq!(messages[0].turn_id.as_deref(), Some("t1"));
        assert_eq!(messages[1].turn_id.as_deref(), Some("t1"));
    }

    #[test]
    fn load_messages_rejects_non_zcode_source() {
        assert!(load_messages("mcode:abc").is_err());
        assert!(load_messages("zcode:").is_err());
    }
}
