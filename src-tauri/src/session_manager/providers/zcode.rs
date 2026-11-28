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
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) fn database_path() -> PathBuf {
    crate::zcode_config::get_zcode_usage_db_path()
}

fn open_readonly(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_millis(5_000))?;
    Ok(conn)
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let database = database_path();
    if !database.exists() {
        return vec![];
    }
    match open_readonly(&database).and_then(|conn| scan(&conn)) {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("Cannot read ZCode sessions: {error}");
            vec![]
        }
    }
}

fn scan(conn: &Connection) -> rusqlite::Result<Vec<SessionMeta>> {
    // 与 ZCode 主列表一致：只列 interactive、fork、workflow_parent。subagent / workflow 子会话
    // 和已删除的（time_deleted 软删）不进列表；fork 的 parent_id 非空，仍要保留。
    // ZCode 桌面没有 CLI，无法给出恢复命令。
    let mut query = conn.prepare(
        "SELECT id, title, directory, time_created, time_updated
         FROM session
         WHERE time_deleted IS NULL
           AND ((parent_id IS NULL AND task_type = 'interactive')
                OR task_type IN ('fork', 'workflow_parent'))
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
    let conn = open_readonly(&database_path()).map_err(|e| e.to_string())?;
    read_messages(&conn, id).map_err(|e| e.to_string())
}

fn read_messages(conn: &Connection, id: &str) -> rusqlite::Result<Vec<SessionMessage>> {
    let mut query = conn.prepare(
        "SELECT id, sequence, data FROM message
         WHERE session_id = ?1 AND json_valid(data)
           AND json_extract(data, '$.role') IN ('user', 'assistant')
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
                ('z_fork','Forked session','/work/zcode',120,350,NULL,'z_public','fork'),
                ('z_child','Subagent child','/work/zcode',100,200,NULL,'z_public','subagent_child'),
                ('z_standalone_child','Standalone child','/work/zcode',100,200,NULL,NULL,'subagent_child'),
                ('z_deleted','Deleted','/work/zcode',100,200,250,NULL,'interactive');
            CREATE TABLE message (id TEXT, session_id TEXT, sequence INTEGER, data TEXT);
            INSERT INTO message VALUES
                ('m_user','z_public',0,'{\"role\":\"user\",\"time\":{\"created\":100}}'),
                ('m_asst','z_public',1,'{\"role\":\"assistant\",\"time\":{\"created\":150},\"modelID\":\"GLM-5.3\",\"providerID\":\"zai\",\"cost\":0.01,\"tokens\":{\"input\":10,\"output\":5,\"reasoning\":0,\"cache\":{\"read\":2,\"write\":1}},\"finish\":\"stop\"}'),
                ('m_fork_u','z_fork',0,'{\"role\":\"user\",\"time\":{\"created\":160}}'),
                ('m_fork_a','z_fork',1,'{\"role\":\"assistant\",\"time\":{\"created\":170},\"modelId\":\"GLM-5.4\",\"providerId\":\"account:bigmodel-individual-coding-plan\",\"cost\":0,\"tokens\":{\"input\":500,\"output\":20,\"reasoning\":0,\"cache\":{\"read\":400,\"write\":0}},\"finish\":\"tool-calls\"}');
            CREATE TABLE part (id TEXT, message_id TEXT, session_id TEXT, sequence INTEGER, data TEXT);
            INSERT INTO part VALUES
                ('p_text','m_user','z_public',0,'{\"type\":\"text\",\"text\":\"Fix the build\"}'),
                ('p_reason','m_asst','z_public',0,'{\"type\":\"reasoning\",\"text\":\"Check the logs first\"}'),
                ('p_body','m_asst','z_public',1,'{\"type\":\"text\",\"text\":\"Build fixed\"}'),
                ('p_tool','m_asst','z_public',2,'{\"type\":\"tool\",\"callID\":\"call_1\",\"tool\":\"Bash\",\"state\":{\"status\":\"completed\",\"input\":{\"command\":\"cargo test\"},\"output\":\"ok\"}}'),
                ('p_step','m_asst','z_public',3,'{\"type\":\"step-finish\",\"reason\":\"stop\",\"cost\":0.01}'),
                ('p_fork_u','m_fork_u','z_fork',0,'{\"type\":\"text\",\"text\":\"Try branch\"}'),
                ('p_fork_a','m_fork_a','z_fork',0,'{\"type\":\"text\",\"text\":\"Fork reply\"}');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn scan_lists_visible_interactive_and_fork_sessions() {
        let sessions = scan(&zcode_db()).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "z_fork");
        assert_eq!(sessions[0].source_path.as_deref(), Some("zcode:z_fork"));

        let session = &sessions[1];
        assert_eq!(session.session_id, "z_public");
        assert_eq!(session.title.as_deref(), Some("Public session"));
        assert_eq!(session.project_dir.as_deref(), Some("/work/zcode"));
        assert_eq!(session.created_at, Some(100));
        assert_eq!(session.last_active_at, Some(300));
        assert_eq!(session.source_path.as_deref(), Some("zcode:z_public"));
        assert!(session.resume_command.is_none());
    }

    #[test]
    fn read_messages_supports_zcode_camel_case_model_fields() {
        let conn = zcode_db();
        let messages = read_messages(&conn, "z_fork").unwrap();
        assert_eq!(messages.len(), 2);
        let meta = messages[1].meta.as_ref().unwrap();
        assert_eq!(
            meta.model.as_deref(),
            Some("account:bigmodel-individual-coding-plan/GLM-5.4")
        );
        assert_eq!(meta.input_tokens, Some(500));
        assert_eq!(meta.output_tokens, Some(20));
        assert_eq!(meta.cache_read_tokens, Some(400));
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
