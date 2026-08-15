use std::{path::Path, sync::Mutex};

use rusqlite::Connection;

use crate::AppError;

use super::{
    domain::{NewTask, TimestampMs},
    repository::{self, TaskLedger, TaskLedgerItem},
};

const SCHEMA_V1: &str = r#"
BEGIN IMMEDIATE;
CREATE TABLE tandem_projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE tandem_tasks (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES tandem_projects(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  original_instruction TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active','needs_attention','awaiting_acceptance','paused','completed')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  completed_at INTEGER
);
CREATE INDEX idx_tandem_tasks_ledger ON tandem_tasks(status, updated_at DESC);
CREATE TABLE tandem_runs (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tandem_tasks(id) ON DELETE CASCADE,
  agent TEXT NOT NULL CHECK (agent IN ('cursor','codex','claude')),
  status TEXT NOT NULL CHECK (status IN ('active','awaiting_user','awaiting_acceptance','paused','ended')),
  native_session_id TEXT,
  native_session_ref TEXT,
  started_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  ended_at INTEGER
);
CREATE INDEX idx_tandem_runs_task ON tandem_runs(task_id, started_at DESC);
PRAGMA user_version = 1;
COMMIT;
"#;

pub struct TandemDatabase {
    pub(super) connection: Mutex<Connection>,
}

impl TandemDatabase {
    pub fn init(path: &Path) -> Result<Self, AppError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn memory() -> Result<Self, AppError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn create_task(
        &self,
        input: NewTask,
        now: TimestampMs,
    ) -> Result<TaskLedgerItem, AppError> {
        let mut connection = self.connection.lock()?;
        repository::create_task(&mut connection, input, now)
    }

    pub fn list_ledger(&self) -> Result<TaskLedger, AppError> {
        let connection = self.connection.lock()?;
        repository::list_ledger(&connection)
    }

    pub fn confirm_task_completed(
        &self,
        task_id: &str,
        now: TimestampMs,
    ) -> Result<TaskLedgerItem, AppError> {
        let mut connection = self.connection.lock()?;
        repository::confirm_task_completed(&mut connection, task_id, now)
    }

    fn from_connection(connection: Connection) -> Result<Self, AppError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let version =
            connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?;
        if version == 0 {
            connection.execute_batch(SCHEMA_V1)?;
        } else if version != 1 {
            return Err(AppError::Database(format!(
                "unsupported Tandem schema version: {version}"
            )));
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn columns(connection: &Connection, table: &str) -> Vec<(String, String, bool, bool)> {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, i64>(5)? != 0,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn schema_v1_has_exact_tables_columns_and_indexes() {
        let database = TandemDatabase::memory().unwrap();
        let connection = database.connection.lock().unwrap();

        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'tandem_%' ORDER BY name")
            .unwrap();
        let tables = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(tables, ["tandem_projects", "tandem_runs", "tandem_tasks"]);

        assert_eq!(
            columns(&connection, "tandem_projects"),
            vec![
                ("id".into(), "TEXT".into(), false, true),
                ("name".into(), "TEXT".into(), true, false),
                ("root_path".into(), "TEXT".into(), true, false),
                ("created_at".into(), "INTEGER".into(), true, false),
                ("updated_at".into(), "INTEGER".into(), true, false),
            ]
        );
        assert_eq!(
            columns(&connection, "tandem_tasks"),
            vec![
                ("id".into(), "TEXT".into(), false, true),
                ("project_id".into(), "TEXT".into(), true, false),
                ("title".into(), "TEXT".into(), true, false),
                ("original_instruction".into(), "TEXT".into(), true, false),
                ("status".into(), "TEXT".into(), true, false),
                ("created_at".into(), "INTEGER".into(), true, false),
                ("updated_at".into(), "INTEGER".into(), true, false),
                ("completed_at".into(), "INTEGER".into(), false, false),
            ]
        );
        assert_eq!(
            columns(&connection, "tandem_runs"),
            vec![
                ("id".into(), "TEXT".into(), false, true),
                ("task_id".into(), "TEXT".into(), true, false),
                ("agent".into(), "TEXT".into(), true, false),
                ("status".into(), "TEXT".into(), true, false),
                ("native_session_id".into(), "TEXT".into(), false, false),
                ("native_session_ref".into(), "TEXT".into(), false, false),
                ("started_at".into(), "INTEGER".into(), true, false),
                ("updated_at".into(), "INTEGER".into(), true, false),
                ("ended_at".into(), "INTEGER".into(), false, false),
            ]
        );

        let indexes: Vec<(String, String)> = {
            let mut statement = connection.prepare(
                "SELECT name, sql FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_tandem_%' ORDER BY name",
            ).unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            indexes,
            [
                (
                    "idx_tandem_runs_task".into(),
                    "CREATE INDEX idx_tandem_runs_task ON tandem_runs(task_id, started_at DESC)"
                        .into(),
                ),
                (
                    "idx_tandem_tasks_ledger".into(),
                    "CREATE INDEX idx_tandem_tasks_ledger ON tandem_tasks(status, updated_at DESC)"
                        .into(),
                ),
            ]
        );
    }

    #[test]
    fn schema_enforces_status_agent_and_foreign_key_constraints() {
        let database = TandemDatabase::memory().unwrap();
        let connection = database.connection.lock().unwrap();
        connection
            .execute("INSERT INTO tandem_projects VALUES ('p','P','/p',1,1)", [])
            .unwrap();
        assert!(connection
            .execute(
                "INSERT INTO tandem_tasks VALUES ('bad','p','T','I','unknown',1,1,NULL)",
                [],
            )
            .is_err());
        assert!(connection
            .execute(
                "INSERT INTO tandem_tasks VALUES ('orphan','missing','T','I','active',1,1,NULL)",
                [],
            )
            .is_err());
        connection
            .execute(
                "INSERT INTO tandem_tasks VALUES ('t','p','T','I','active',1,1,NULL)",
                [],
            )
            .unwrap();
        assert!(connection.execute(
            "INSERT INTO tandem_runs VALUES ('bad-agent','t','gemini','active',NULL,NULL,1,1,NULL)", [],
        ).is_err());
        assert!(connection.execute(
            "INSERT INTO tandem_runs VALUES ('bad-status','t','codex','unknown',NULL,NULL,1,1,NULL)", [],
        ).is_err());
        assert!(connection.execute(
            "INSERT INTO tandem_runs VALUES ('orphan-run','missing','codex','active',NULL,NULL,1,1,NULL)", [],
        ).is_err());
    }

    #[test]
    fn native_session_references_are_nullable_and_nonunique() {
        let database = TandemDatabase::memory().unwrap();
        let connection = database.connection.lock().unwrap();
        connection
            .execute("INSERT INTO tandem_projects VALUES ('p','P','/p',1,1)", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO tandem_tasks VALUES ('t','p','T','I','active',1,1,NULL)",
                [],
            )
            .unwrap();
        for id in ["r1", "r2"] {
            connection
                .execute(
                    "INSERT INTO tandem_runs VALUES (?1,'t','codex','active',NULL,NULL,1,1,NULL)",
                    params![id],
                )
                .unwrap();
        }
        for id in ["r3", "r4"] {
            connection.execute(
                "INSERT INTO tandem_runs VALUES (?1,'t','codex','active','same-id','same-ref',1,1,NULL)",
                params![id],
            ).unwrap();
        }
    }

    #[test]
    fn file_database_reopens_and_reads_persisted_task() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tandem.db");
        let task_id = {
            let database = TandemDatabase::init(&path).unwrap();
            database
                .create_task(
                    crate::tandem::domain::NewTask {
                        project_name: "Project".into(),
                        project_root_path: "/tmp/project".into(),
                        title: "Persist me".into(),
                        original_instruction: "Safe persistent instruction".into(),
                    },
                    1_000,
                )
                .unwrap()
                .task
                .id
        };
        let reopened = TandemDatabase::init(&path).unwrap();
        let ledger = reopened.list_ledger().unwrap();
        assert_eq!(ledger.active.len(), 1);
        assert_eq!(ledger.active[0].task.id, task_id);
        assert_eq!(ledger.active[0].task.title, "Persist me");
    }
}
