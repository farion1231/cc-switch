use crate::agent_gateway::models::{
    AgentInstance, AgentLaunchMode, AgentLog, AgentRuntimeKind, AgentStatus,
    ProviderRuntimeSnapshot,
};
use crate::agent_gateway::port_registry::{port_set, PortBinding};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::str::FromStr;

pub(crate) fn create_agent_gateway_tables(conn: &Connection) -> Result<(), AppError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_instances (
            id TEXT PRIMARY KEY,
            app_type TEXT NOT NULL DEFAULT 'claude',
            name TEXT NOT NULL,
            runtime TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            provider_name TEXT,
            model TEXT,
            launch_mode TEXT NOT NULL DEFAULT 'new',
            run_profile_id TEXT NOT NULL,
            port INTEGER NOT NULL,
            cwd TEXT,
            pid INTEGER,
            window_title TEXT,
            session_id TEXT,
            status TEXT NOT NULL,
            last_error TEXT,
            created_at TEXT NOT NULL,
            started_at TEXT,
            stopped_at TEXT,
            deleted_at TEXT
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent_instances failed: {e}")))?;
    ensure_agent_instance_columns(conn)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_run_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            runtime TEXT NOT NULL,
            kind TEXT NOT NULL,
            args_json TEXT NOT NULL DEFAULT '[]',
            env_json TEXT NOT NULL DEFAULT '[]',
            allow_custom_profiles INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent_run_profiles failed: {e}")))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_port_bindings (
            port INTEGER PRIMARY KEY,
            agent_id TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            runtime TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent_port_bindings failed: {e}")))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_logs (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            level TEXT NOT NULL,
            event TEXT NOT NULL,
            message TEXT,
            payload_json TEXT,
            created_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent_logs failed: {e}")))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_provider_snapshots (
            agent_id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL,
            provider_name TEXT NOT NULL,
            redacted_snapshot_json TEXT NOT NULL,
            provider_config_hash TEXT,
            created_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent_provider_snapshots failed: {e}")))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_mcp_bindings (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            mcp_server_id TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent_mcp_bindings failed: {e}")))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_skill_bindings (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            skill_id TEXT NOT NULL,
            install_mode TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent_skill_bindings failed: {e}")))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_session_bindings (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            runtime TEXT NOT NULL,
            session_id TEXT NOT NULL,
            project_path TEXT,
            title TEXT,
            last_used_at TEXT
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent_session_bindings failed: {e}")))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS opencode_subscription_providers (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL,
            subscription_kind TEXT NOT NULL,
            base_url TEXT,
            api_key_ref TEXT NOT NULL,
            local_adapter_enabled INTEGER NOT NULL DEFAULT 1,
            default_model TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| {
        AppError::Database(format!(
            "create opencode_subscription_providers failed: {e}"
        ))
    })?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_instances_status ON agent_instances(status)",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent status index failed: {e}")))?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_logs_agent_created ON agent_logs(agent_id, created_at DESC)",
        [],
    )
    .map_err(|e| AppError::Database(format!("create agent logs index failed: {e}")))?;
    Ok(())
}

fn ensure_agent_instance_columns(conn: &Connection) -> Result<(), AppError> {
    for (column, definition) in [
        ("app_type", "TEXT NOT NULL DEFAULT 'claude'"),
        ("name", "TEXT NOT NULL DEFAULT 'Claude Agent'"),
        ("runtime", "TEXT NOT NULL DEFAULT 'claude_code'"),
        ("provider_id", "TEXT NOT NULL DEFAULT ''"),
        ("provider_name", "TEXT"),
        ("model", "TEXT"),
        ("launch_mode", "TEXT NOT NULL DEFAULT 'new'"),
        ("run_profile_id", "TEXT NOT NULL DEFAULT 'safe'"),
        ("port", "INTEGER NOT NULL DEFAULT 0"),
        ("cwd", "TEXT"),
        ("pid", "INTEGER"),
        ("window_title", "TEXT"),
        ("session_id", "TEXT"),
        ("status", "TEXT NOT NULL DEFAULT 'failed'"),
        ("last_error", "TEXT"),
        ("created_at", "TEXT NOT NULL DEFAULT ''"),
        ("started_at", "TEXT"),
        ("stopped_at", "TEXT"),
        ("deleted_at", "TEXT"),
    ] {
        add_column_if_missing(conn, "agent_instances", column, definition)?;
    }
    Ok(())
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), AppError> {
    if Database::has_column(conn, table, column)? {
        return Ok(());
    }
    let sql = format!("ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {definition};");
    conn.execute(&sql, [])
        .map_err(|e| AppError::Database(format!("add {table}.{column} failed: {e}")))?;
    Ok(())
}

impl Database {
    pub fn verify_agent_gateway_schema_ready(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let version = Self::get_user_version(&conn)?;
        if version != crate::database::SCHEMA_VERSION || version != 11 {
            return Err(AppError::Database(format!(
                "Agent Gateway requires schema version 11, found {version}"
            )));
        }
        create_agent_gateway_tables(&conn)?;
        for table in [
            "agent_instances",
            "agent_run_profiles",
            "agent_port_bindings",
            "agent_logs",
            "agent_provider_snapshots",
            "opencode_subscription_providers",
        ] {
            if !Self::table_exists(&conn, table)? {
                return Err(AppError::Database(format!(
                    "Agent Gateway table missing after migration: {table}"
                )));
            }
        }
        for column in [
            "id",
            "app_type",
            "name",
            "runtime",
            "provider_id",
            "provider_name",
            "model",
            "launch_mode",
            "run_profile_id",
            "port",
            "cwd",
            "pid",
            "window_title",
            "session_id",
            "status",
            "last_error",
            "created_at",
            "started_at",
            "stopped_at",
            "deleted_at",
        ] {
            if !Self::has_column(&conn, "agent_instances", column)? {
                return Err(AppError::Database(format!(
                    "Agent Gateway column missing after migration: agent_instances.{column}"
                )));
            }
        }
        Ok(())
    }

    pub fn save_agent_instance(&self, agent: &AgentInstance) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO agent_instances (
                id, app_type, name, runtime, provider_id, provider_name, model, launch_mode,
                run_profile_id, port, cwd, pid, window_title, session_id, status, last_error,
                created_at, started_at, stopped_at, deleted_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                agent.id,
                "claude",
                agent.name,
                agent.runtime.to_string(),
                agent.provider_id,
                agent.provider_name.as_deref(),
                agent.model.as_deref(),
                agent.launch_mode.to_string(),
                agent.run_profile_id,
                agent.port as i64,
                agent.cwd.as_deref(),
                agent.pid.map(|pid| pid as i64),
                agent.window_title.as_deref(),
                agent.session_id.as_deref(),
                agent.status.to_string(),
                agent.last_error.as_deref(),
                agent.created_at,
                agent.started_at.as_deref(),
                agent.stopped_at.as_deref(),
                agent.deleted_at.as_deref(),
            ],
        )
        .map_err(|e| AppError::Database(format!("save agent instance failed: {e}")))?;
        Ok(())
    }

    pub fn list_agent_instances(&self) -> Result<Vec<AgentInstance>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, runtime, provider_id, provider_name, model, launch_mode,
                    run_profile_id, port, cwd, pid, window_title, session_id, status, last_error,
                    created_at, started_at, stopped_at, deleted_at
                 FROM agent_instances
                 WHERE deleted_at IS NULL
                 ORDER BY created_at DESC",
            )
            .map_err(|e| AppError::Database(format!("list agent prepare failed: {e}")))?;
        let rows = stmt
            .query_map([], row_to_agent_instance)
            .map_err(|e| AppError::Database(format!("list agent query failed: {e}")))?;
        let mut agents = Vec::new();
        for row in rows {
            agents
                .push(row.map_err(|e| AppError::Database(format!("read agent row failed: {e}")))?);
        }
        Ok(agents)
    }

    pub fn get_agent_instance(&self, agent_id: &str) -> Result<Option<AgentInstance>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, runtime, provider_id, provider_name, model, launch_mode,
                    run_profile_id, port, cwd, pid, window_title, session_id, status, last_error,
                    created_at, started_at, stopped_at, deleted_at
                 FROM agent_instances
                 WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(format!("get agent prepare failed: {e}")))?;
        match stmt.query_row(params![agent_id], row_to_agent_instance) {
            Ok(agent) => Ok(Some(agent)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(format!("get agent failed: {e}"))),
        }
    }

    pub fn update_agent_status(
        &self,
        agent_id: &str,
        status: AgentStatus,
        last_error: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let stopped_at = if status.is_terminal() {
            Some(chrono::Utc::now().to_rfc3339())
        } else {
            None
        };
        conn.execute(
            "UPDATE agent_instances
             SET status = ?2, last_error = ?3, stopped_at = COALESCE(?4, stopped_at)
             WHERE id = ?1",
            params![agent_id, status.to_string(), last_error, stopped_at],
        )
        .map_err(|e| AppError::Database(format!("update agent status failed: {e}")))?;
        Ok(())
    }

    pub fn soft_delete_agent_instance(&self, agent_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let deleted_at = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE agent_instances SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
            params![agent_id, deleted_at],
        )
        .map_err(|e| AppError::Database(format!("soft delete agent failed: {e}")))?;
        Ok(())
    }

    pub fn save_agent_provider_snapshot(
        &self,
        agent_id: &str,
        snapshot: &ProviderRuntimeSnapshot,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let snapshot_json = serde_json::to_string(snapshot)
            .map_err(|e| AppError::Database(format!("serialize provider snapshot failed: {e}")))?;
        conn.execute(
            "INSERT OR REPLACE INTO agent_provider_snapshots
             (agent_id, provider_id, provider_name, redacted_snapshot_json, provider_config_hash, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                agent_id,
                snapshot.provider_id,
                snapshot.provider_name,
                snapshot_json,
                snapshot.provider_config_hash.as_deref(),
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|e| AppError::Database(format!("save provider snapshot failed: {e}")))?;
        Ok(())
    }

    pub fn list_agent_port_numbers(&self) -> Result<HashSet<u16>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT port FROM agent_port_bindings")
            .map_err(|e| AppError::Database(format!("prepare port list failed: {e}")))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(|e| AppError::Database(format!("query port list failed: {e}")))?;
        let mut ports = Vec::new();
        for row in rows {
            ports.push(
                row.map_err(|e| AppError::Database(format!("read port row failed: {e}")))? as u16,
            );
        }
        Ok(port_set(ports))
    }

    pub fn save_agent_port_binding(&self, binding: &PortBinding) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO agent_port_bindings
             (port, agent_id, provider_id, runtime, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                binding.port as i64,
                binding.agent_id.as_str(),
                binding.provider_id.as_str(),
                binding.runtime.to_string(),
                binding.created_at.as_str(),
            ],
        )
        .map_err(|e| AppError::Database(format!("save port binding failed: {e}")))?;
        Ok(())
    }

    pub fn delete_agent_port_binding(&self, port: u16) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM agent_port_bindings WHERE port = ?1",
            params![port as i64],
        )
        .map_err(|e| AppError::Database(format!("delete port binding failed: {e}")))?;
        Ok(())
    }

    pub fn get_provider_id_for_agent_port(&self, port: u16) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        match conn.query_row(
            "SELECT provider_id FROM agent_port_bindings WHERE port = ?1",
            params![port as i64],
            |row| row.get::<_, String>(0),
        ) {
            Ok(provider_id) => Ok(Some(provider_id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(format!(
                "lookup port binding failed: {e}"
            ))),
        }
    }

    pub fn append_agent_log(&self, log: &AgentLog) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO agent_logs (id, agent_id, level, event, message, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                log.id,
                log.agent_id,
                log.level,
                log.event,
                log.message.as_deref(),
                log.payload_json.as_deref(),
                log.created_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("append agent log failed: {e}")))?;
        Ok(())
    }

    pub fn list_agent_logs(&self, agent_id: &str, limit: usize) -> Result<Vec<AgentLog>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, agent_id, level, event, message, payload_json, created_at
                 FROM agent_logs
                 WHERE agent_id = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )
            .map_err(|e| AppError::Database(format!("list agent logs prepare failed: {e}")))?;
        let rows = stmt
            .query_map(params![agent_id, limit as i64], |row| {
                Ok(AgentLog {
                    id: row.get(0)?,
                    agent_id: row.get(1)?,
                    level: row.get(2)?,
                    event: row.get(3)?,
                    message: row.get(4)?,
                    payload_json: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| AppError::Database(format!("list agent logs query failed: {e}")))?;
        let mut logs = Vec::new();
        for row in rows {
            logs.push(row.map_err(|e| AppError::Database(format!("read log row failed: {e}")))?);
        }
        Ok(logs)
    }
}

fn row_to_agent_instance(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentInstance> {
    let runtime: String = row.get(2)?;
    let launch_mode: String = row.get(6)?;
    let status: String = row.get(13)?;
    let port: i64 = row.get(8)?;
    let pid: Option<i64> = row.get(10)?;
    Ok(AgentInstance {
        id: row.get(0)?,
        name: row.get(1)?,
        runtime: AgentRuntimeKind::from_str(&runtime).unwrap_or(AgentRuntimeKind::ClaudeCode),
        provider_id: row.get(3)?,
        provider_name: row.get(4)?,
        model: row.get(5)?,
        launch_mode: AgentLaunchMode::from_str(&launch_mode).unwrap_or(AgentLaunchMode::New),
        run_profile_id: row.get(7)?,
        port: port as u16,
        cwd: row.get(9)?,
        pid: pid.map(|value| value as u32),
        window_title: row.get(11)?,
        session_id: row.get(12)?,
        status: AgentStatus::from_str(&status).unwrap_or(AgentStatus::Failed),
        last_error: row.get(14)?,
        created_at: row.get(15)?,
        started_at: row.get(16)?,
        stopped_at: row.get(17)?,
        deleted_at: row.get(18)?,
    })
}

#[cfg(test)]
mod tests {
    use super::create_agent_gateway_tables;
    use crate::agent_gateway::models::{
        AgentInstance, AgentLaunchMode, AgentRuntimeKind, AgentStatus,
    };
    use crate::database::Database;
    use rusqlite::Connection;
    use std::sync::Mutex;

    #[test]
    fn v11_table_creation_repairs_legacy_agent_instances_columns() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute(
            "CREATE TABLE agent_instances (
                id TEXT PRIMARY KEY,
                app_type TEXT NOT NULL,
                runtime TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                port INTEGER NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )
        .expect("legacy table");

        create_agent_gateway_tables(&conn).expect("repair schema");

        for column in [
            "name",
            "app_type",
            "model",
            "provider_name",
            "launch_mode",
            "run_profile_id",
            "cwd",
            "pid",
            "window_title",
            "session_id",
            "last_error",
            "started_at",
            "stopped_at",
            "deleted_at",
        ] {
            assert!(
                Database::has_column(&conn, "agent_instances", column).expect("column check"),
                "missing {column}"
            );
        }
    }

    #[test]
    fn save_agent_instance_writes_legacy_app_type_not_null() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute(
            "CREATE TABLE agent_instances (
                id TEXT PRIMARY KEY,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                runtime TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                provider_name TEXT,
                model TEXT,
                launch_mode TEXT NOT NULL DEFAULT 'new',
                run_profile_id TEXT NOT NULL,
                port INTEGER NOT NULL,
                cwd TEXT,
                pid INTEGER,
                window_title TEXT,
                session_id TEXT,
                status TEXT NOT NULL,
                last_error TEXT,
                created_at TEXT NOT NULL,
                started_at TEXT,
                stopped_at TEXT,
                deleted_at TEXT
            )",
            [],
        )
        .expect("legacy table");
        create_agent_gateway_tables(&conn).expect("repair schema");

        let db = Database {
            conn: Mutex::new(conn),
        };
        db.save_agent_instance(&AgentInstance {
            id: "agent-1".to_string(),
            name: "Agent".to_string(),
            runtime: AgentRuntimeKind::ClaudeCode,
            provider_id: "provider-1".to_string(),
            provider_name: Some("Provider 1".to_string()),
            model: None,
            launch_mode: AgentLaunchMode::New,
            run_profile_id: "safe".to_string(),
            port: 15722,
            cwd: None,
            pid: None,
            window_title: Some("CCSA:agent-1".to_string()),
            session_id: None,
            status: AgentStatus::Launching,
            created_at: "2026-05-09T00:00:00Z".to_string(),
            started_at: None,
            stopped_at: None,
            last_error: None,
            deleted_at: None,
        })
        .expect("save");

        let conn = db.conn.lock().expect("lock db");
        let app_type: String = conn
            .query_row(
                "SELECT app_type FROM agent_instances WHERE id = 'agent-1'",
                [],
                |row| row.get(0),
            )
            .expect("read app_type");
        assert_eq!(app_type, "claude");
    }

    #[test]
    fn legacy_agent_instances_default_to_new_launch_mode() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute(
            "CREATE TABLE agent_instances (
                id TEXT PRIMARY KEY,
                app_type TEXT NOT NULL,
                runtime TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                port INTEGER NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )
        .expect("legacy table");
        conn.execute(
            "INSERT INTO agent_instances (id, app_type, runtime, provider_id, port, status, created_at)
             VALUES ('legacy-1', 'claude', 'claude_code', 'provider-1', 15722, 'stopped', '2026-05-09T00:00:00Z')",
            [],
        )
        .expect("legacy row");
        create_agent_gateway_tables(&conn).expect("repair schema");

        let db = Database {
            conn: Mutex::new(conn),
        };
        let agents = db.list_agent_instances().expect("agents");
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].launch_mode, AgentLaunchMode::New);
        assert!(agents[0].deleted_at.is_none());
    }

    #[test]
    fn soft_deleted_agents_are_hidden_from_default_list() {
        let db = Database::memory().expect("memory db");
        let mut agent = AgentInstance {
            id: "agent-1".to_string(),
            name: "Agent".to_string(),
            runtime: AgentRuntimeKind::ClaudeCode,
            provider_id: "provider-1".to_string(),
            provider_name: Some("Provider 1".to_string()),
            model: None,
            launch_mode: AgentLaunchMode::New,
            run_profile_id: "safe".to_string(),
            port: 15722,
            cwd: None,
            pid: None,
            window_title: Some("CCSA:agent-1".to_string()),
            session_id: None,
            status: AgentStatus::Stopped,
            created_at: "2026-05-09T00:00:00Z".to_string(),
            started_at: None,
            stopped_at: None,
            last_error: None,
            deleted_at: None,
        };
        db.save_agent_instance(&agent).expect("save");
        assert_eq!(db.list_agent_instances().expect("list").len(), 1);

        db.soft_delete_agent_instance(&agent.id).expect("delete");
        assert!(db.list_agent_instances().expect("list").is_empty());
        agent.deleted_at = db
            .get_agent_instance(&agent.id)
            .expect("get")
            .and_then(|agent| agent.deleted_at);
        assert!(agent.deleted_at.is_some());
    }
}
