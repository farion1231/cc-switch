use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::AppError;

const SCHEMA_VERSION: i32 = 1;
const APPLICATION_ID: i32 = 0x5753_594e; // "WSYN"

const SCHEMA_V1: &str = r#"
    CREATE TABLE sync_transactions (
        id TEXT PRIMARY KEY,
        operation TEXT NOT NULL,
        state TEXT NOT NULL,
        base_snapshot_id TEXT,
        remote_snapshot_id TEXT,
        result_snapshot_id TEXT,
        started_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        error_code TEXT,
        error_detail TEXT
    );

    CREATE TABLE provider_results (
        transaction_id TEXT NOT NULL,
        provider_id TEXT NOT NULL,
        state TEXT NOT NULL,
        added_count INTEGER NOT NULL DEFAULT 0,
        updated_count INTEGER NOT NULL DEFAULT 0,
        deleted_count INTEGER NOT NULL DEFAULT 0,
        conflict_count INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (transaction_id, provider_id),
        FOREIGN KEY (transaction_id) REFERENCES sync_transactions(id) ON DELETE CASCADE
    );

    CREATE TABLE conflicts (
        id TEXT PRIMARY KEY,
        provider_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        logical_id TEXT NOT NULL,
        state TEXT NOT NULL,
        reason TEXT NOT NULL,
        metadata TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        resolved_at INTEGER
    );

    CREATE TABLE tombstones (
        provider_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        logical_id TEXT NOT NULL,
        last_known_hash TEXT,
        deleted_at INTEGER NOT NULL,
        deleted_by TEXT NOT NULL,
        PRIMARY KEY (provider_id, kind, logical_id)
    );

    CREATE TABLE devices (
        device_id TEXT PRIMARY KEY,
        device_name TEXT NOT NULL,
        last_snapshot_id TEXT,
        last_seen_at INTEGER NOT NULL,
        removed_at INTEGER
    );

    CREATE TABLE snapshot_cache (
        snapshot_id TEXT PRIMARY KEY,
        parent_ids TEXT NOT NULL,
        manifest_hash TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        created_by TEXT NOT NULL
    );

    CREATE TABLE blob_refs (
        object_id TEXT PRIMARY KEY,
        local_cache_path TEXT,
        plain_size INTEGER NOT NULL,
        stored_size INTEGER NOT NULL,
        ref_count INTEGER NOT NULL,
        last_accessed_at INTEGER NOT NULL
    );

    CREATE TABLE provider_schema_cache (
        provider_id TEXT NOT NULL,
        native_version TEXT NOT NULL,
        schema_fingerprint TEXT NOT NULL,
        capabilities TEXT NOT NULL,
        checked_at INTEGER NOT NULL,
        PRIMARY KEY (provider_id, schema_fingerprint)
    );
"#;

#[derive(Debug)]
struct ColumnDefinition {
    name: &'static str,
    declared_type: &'static str,
    not_null: bool,
    default_value: Option<&'static str>,
    primary_key_position: i32,
}

impl ColumnDefinition {
    const fn new(
        name: &'static str,
        declared_type: &'static str,
        not_null: bool,
        default_value: Option<&'static str>,
        primary_key_position: i32,
    ) -> Self {
        Self {
            name,
            declared_type,
            not_null,
            default_value,
            primary_key_position,
        }
    }
}

#[derive(Debug)]
struct TableDefinition {
    name: &'static str,
    columns: &'static [ColumnDefinition],
}

#[derive(Debug)]
struct ActualColumnDefinition {
    name: String,
    declared_type: String,
    not_null: bool,
    default_value: Option<String>,
    primary_key_position: i32,
}

impl ActualColumnDefinition {
    fn matches(&self, expected: &ColumnDefinition) -> bool {
        self.name == expected.name
            && self
                .declared_type
                .eq_ignore_ascii_case(expected.declared_type)
            && self.not_null == expected.not_null
            && self.default_value.as_deref() == expected.default_value
            && self.primary_key_position == expected.primary_key_position
    }
}

const REQUIRED_SCHEMA: &[TableDefinition] = &[
    TableDefinition {
        name: "sync_transactions",
        columns: &[
            ColumnDefinition::new("id", "TEXT", false, None, 1),
            ColumnDefinition::new("operation", "TEXT", true, None, 0),
            ColumnDefinition::new("state", "TEXT", true, None, 0),
            ColumnDefinition::new("base_snapshot_id", "TEXT", false, None, 0),
            ColumnDefinition::new("remote_snapshot_id", "TEXT", false, None, 0),
            ColumnDefinition::new("result_snapshot_id", "TEXT", false, None, 0),
            ColumnDefinition::new("started_at", "INTEGER", true, None, 0),
            ColumnDefinition::new("updated_at", "INTEGER", true, None, 0),
            ColumnDefinition::new("error_code", "TEXT", false, None, 0),
            ColumnDefinition::new("error_detail", "TEXT", false, None, 0),
        ],
    },
    TableDefinition {
        name: "provider_results",
        columns: &[
            ColumnDefinition::new("transaction_id", "TEXT", true, None, 1),
            ColumnDefinition::new("provider_id", "TEXT", true, None, 2),
            ColumnDefinition::new("state", "TEXT", true, None, 0),
            ColumnDefinition::new("added_count", "INTEGER", true, Some("0"), 0),
            ColumnDefinition::new("updated_count", "INTEGER", true, Some("0"), 0),
            ColumnDefinition::new("deleted_count", "INTEGER", true, Some("0"), 0),
            ColumnDefinition::new("conflict_count", "INTEGER", true, Some("0"), 0),
        ],
    },
    TableDefinition {
        name: "conflicts",
        columns: &[
            ColumnDefinition::new("id", "TEXT", false, None, 1),
            ColumnDefinition::new("provider_id", "TEXT", true, None, 0),
            ColumnDefinition::new("kind", "TEXT", true, None, 0),
            ColumnDefinition::new("logical_id", "TEXT", true, None, 0),
            ColumnDefinition::new("state", "TEXT", true, None, 0),
            ColumnDefinition::new("reason", "TEXT", true, None, 0),
            ColumnDefinition::new("metadata", "TEXT", true, None, 0),
            ColumnDefinition::new("created_at", "INTEGER", true, None, 0),
            ColumnDefinition::new("resolved_at", "INTEGER", false, None, 0),
        ],
    },
    TableDefinition {
        name: "tombstones",
        columns: &[
            ColumnDefinition::new("provider_id", "TEXT", true, None, 1),
            ColumnDefinition::new("kind", "TEXT", true, None, 2),
            ColumnDefinition::new("logical_id", "TEXT", true, None, 3),
            ColumnDefinition::new("last_known_hash", "TEXT", false, None, 0),
            ColumnDefinition::new("deleted_at", "INTEGER", true, None, 0),
            ColumnDefinition::new("deleted_by", "TEXT", true, None, 0),
        ],
    },
    TableDefinition {
        name: "devices",
        columns: &[
            ColumnDefinition::new("device_id", "TEXT", false, None, 1),
            ColumnDefinition::new("device_name", "TEXT", true, None, 0),
            ColumnDefinition::new("last_snapshot_id", "TEXT", false, None, 0),
            ColumnDefinition::new("last_seen_at", "INTEGER", true, None, 0),
            ColumnDefinition::new("removed_at", "INTEGER", false, None, 0),
        ],
    },
    TableDefinition {
        name: "snapshot_cache",
        columns: &[
            ColumnDefinition::new("snapshot_id", "TEXT", false, None, 1),
            ColumnDefinition::new("parent_ids", "TEXT", true, None, 0),
            ColumnDefinition::new("manifest_hash", "TEXT", true, None, 0),
            ColumnDefinition::new("created_at", "INTEGER", true, None, 0),
            ColumnDefinition::new("created_by", "TEXT", true, None, 0),
        ],
    },
    TableDefinition {
        name: "blob_refs",
        columns: &[
            ColumnDefinition::new("object_id", "TEXT", false, None, 1),
            ColumnDefinition::new("local_cache_path", "TEXT", false, None, 0),
            ColumnDefinition::new("plain_size", "INTEGER", true, None, 0),
            ColumnDefinition::new("stored_size", "INTEGER", true, None, 0),
            ColumnDefinition::new("ref_count", "INTEGER", true, None, 0),
            ColumnDefinition::new("last_accessed_at", "INTEGER", true, None, 0),
        ],
    },
    TableDefinition {
        name: "provider_schema_cache",
        columns: &[
            ColumnDefinition::new("provider_id", "TEXT", true, None, 1),
            ColumnDefinition::new("native_version", "TEXT", true, None, 0),
            ColumnDefinition::new("schema_fingerprint", "TEXT", true, None, 2),
            ColumnDefinition::new("capabilities", "TEXT", true, None, 0),
            ColumnDefinition::new("checked_at", "INTEGER", true, None, 0),
        ],
    },
];

enum OpenAction {
    Initialize,
    Verify,
}

pub struct WorkspaceSyncDb {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl WorkspaceSyncDb {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| database_error(path, "open", error))?;
        }

        let conn = Connection::open(path).map_err(|error| database_error(path, "open", error))?;
        Self::prepare(conn, path, true)
    }

    pub fn memory() -> Result<Self, AppError> {
        Self::initialize(
            Connection::open_in_memory()
                .map_err(|error| database_error(Path::new(":memory:"), "open", error))?,
        )
    }

    pub fn user_version(&self) -> Result<i32, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|error| database_error(&self.path, "verify", error))?;
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| database_error(&self.path, "verify", error))
    }

    fn initialize(conn: Connection) -> Result<Self, AppError> {
        Self::prepare(conn, Path::new(":memory:"), false)
    }

    fn prepare(mut conn: Connection, path: &Path, use_wal: bool) -> Result<Self, AppError> {
        let action = Self::inspect_identity(&conn, path)?;
        Self::enable_foreign_keys(&conn, path)?;

        match action {
            OpenAction::Initialize => {
                if use_wal {
                    Self::set_wal(&conn, path)?;
                }
                Self::migrate_v1(&mut conn, path)?;
            }
            OpenAction::Verify => {
                Self::verify_schema(&conn, path)?;
                if use_wal {
                    Self::set_wal(&conn, path)?;
                }
            }
        }

        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        })
    }

    fn inspect_identity(conn: &Connection, path: &Path) -> Result<OpenAction, AppError> {
        let application_id: i32 = conn
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(|error| database_error(path, "identity", error))?;
        let user_version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| database_error(path, "identity", error))?;

        if application_id != 0 && application_id != APPLICATION_ID {
            return Err(database_error(
                path,
                "identity",
                format!(
                    "application_id {application_id:#010x} does not match expected {APPLICATION_ID:#010x}"
                ),
            ));
        }

        if user_version > SCHEMA_VERSION {
            return Err(database_error(
                path,
                "identity",
                format!(
                    "future schema version {user_version} is newer than supported version {SCHEMA_VERSION}"
                ),
            ));
        }

        match user_version {
            0 => {
                let is_empty: bool = conn
                    .query_row(
                        "SELECT NOT EXISTS(\
                            SELECT 1 FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'\
                         )",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| database_error(path, "identity", error))?;
                if !is_empty {
                    return Err(database_error(
                        path,
                        "identity",
                        "refusing to initialize a non-empty database with user_version 0",
                    ));
                }
                Ok(OpenAction::Initialize)
            }
            SCHEMA_VERSION => {
                if application_id != APPLICATION_ID {
                    return Err(database_error(
                        path,
                        "identity",
                        format!(
                            "schema version {SCHEMA_VERSION} is missing expected application_id {APPLICATION_ID:#010x}"
                        ),
                    ));
                }
                Ok(OpenAction::Verify)
            }
            unsupported => Err(database_error(
                path,
                "identity",
                format!("unsupported schema version {unsupported}"),
            )),
        }
    }

    fn enable_foreign_keys(conn: &Connection, path: &Path) -> Result<(), AppError> {
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| database_error(path, "verify", error))?;
        let enabled: i32 = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .map_err(|error| database_error(path, "verify", error))?;
        if enabled != 1 {
            return Err(database_error(
                path,
                "verify",
                format!("foreign_keys is {enabled}, expected 1"),
            ));
        }
        Ok(())
    }

    fn set_wal(conn: &Connection, path: &Path) -> Result<(), AppError> {
        let actual: String = conn
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
            .map_err(|error| database_error(path, "set_wal", error))?;
        if !actual.eq_ignore_ascii_case("wal") {
            return Err(database_error(
                path,
                "set_wal",
                format!("journal_mode is {actual}, expected wal"),
            ));
        }
        Ok(())
    }

    fn migrate_v1(conn: &mut Connection, path: &Path) -> Result<(), AppError> {
        let transaction = conn
            .transaction()
            .map_err(|error| database_error(path, "migrate", error))?;
        transaction
            .execute_batch(SCHEMA_V1)
            .map_err(|error| database_error(path, "migrate", error))?;
        transaction
            .pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(|error| database_error(path, "migrate", error))?;
        transaction
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|error| database_error(path, "migrate", error))?;

        Self::verify_schema(&transaction, path)?;
        transaction
            .commit()
            .map_err(|error| database_error(path, "migrate", error))
    }

    fn verify_schema(conn: &Connection, path: &Path) -> Result<(), AppError> {
        let application_id: i32 = conn
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(|error| database_error(path, "verify", error))?;
        let user_version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| database_error(path, "verify", error))?;
        if application_id != APPLICATION_ID || user_version != SCHEMA_VERSION {
            return Err(database_error(
                path,
                "verify",
                format!(
                    "identity mismatch: application_id={application_id:#010x}, user_version={user_version}"
                ),
            ));
        }

        for table in REQUIRED_SCHEMA {
            let actual_columns = Self::raw_table_columns(conn, table.name)
                .map_err(|error| database_error(path, "verify", error))?;
            let columns_match = actual_columns.len() == table.columns.len()
                && actual_columns
                    .iter()
                    .zip(table.columns)
                    .all(|(actual, expected)| actual.matches(expected));
            if !columns_match {
                return Err(database_error(
                    path,
                    "verify",
                    format!(
                        "table {} columns mismatch: actual={actual_columns:?}, expected={:?}",
                        table.name, table.columns
                    ),
                ));
            }
        }

        let mut statement = conn
            .prepare("PRAGMA foreign_key_list(provider_results)")
            .map_err(|error| database_error(path, "verify", error))?;
        let foreign_keys = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .map_err(|error| database_error(path, "verify", error))?;
        let foreign_keys = foreign_keys
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| database_error(path, "verify", error))?;
        let has_exact_foreign_key = matches!(
            foreign_keys.as_slice(),
            [(table, from, to, on_update, on_delete, match_clause)]
                if table == "sync_transactions"
                    && from == "transaction_id"
                    && to == "id"
                    && on_update.eq_ignore_ascii_case("no action")
                    && on_delete.eq_ignore_ascii_case("cascade")
                    && match_clause.eq_ignore_ascii_case("none")
        );
        if !has_exact_foreign_key {
            return Err(database_error(
                path,
                "verify",
                format!(
                    "provider_results foreign keys mismatch: actual={foreign_keys:?}, expected transaction_id -> sync_transactions(id) ON UPDATE NO ACTION ON DELETE CASCADE MATCH NONE"
                ),
            ));
        }

        Ok(())
    }

    fn raw_table_columns(
        conn: &Connection,
        table: &str,
    ) -> rusqlite::Result<Vec<ActualColumnDefinition>> {
        let mut statement = conn.prepare(
            r#"SELECT name, type, "notnull", dflt_value, pk
               FROM pragma_table_info(?1) ORDER BY cid"#,
        )?;
        let rows = statement.query_map([table], |row| {
            Ok(ActualColumnDefinition {
                name: row.get(0)?,
                declared_type: row.get(1)?,
                not_null: row.get::<_, i32>(2)? != 0,
                default_value: row.get(3)?,
                primary_key_position: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    fn raw_table_exists(conn: &Connection, table: &str) -> Result<bool, AppError> {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(AppError::from)
    }

    #[cfg(test)]
    fn table_exists(&self, table: &str) -> Result<bool, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|error| database_error(&self.path, "verify", error))?;
        Self::raw_table_exists(&conn, table)
            .map_err(|error| database_error(&self.path, "verify", error))
    }
}

fn database_error(path: &Path, stage: &str, detail: impl Display) -> AppError {
    AppError::Database(format!(
        "workspace sync database path={} stage={stage}: {detail}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use rusqlite::Connection;

    use super::{WorkspaceSyncDb, APPLICATION_ID, SCHEMA_V1, SCHEMA_VERSION};
    use crate::error::AppError;

    fn temp_db_path(name: &str) -> Result<(tempfile::TempDir, PathBuf), AppError> {
        let temp =
            tempfile::tempdir().map_err(|error| AppError::io("workspace-sync-test", error))?;
        let path = temp.path().join(name);
        Ok((temp, path))
    }

    fn rejected_open(path: &Path) -> Result<String, AppError> {
        match WorkspaceSyncDb::open(path) {
            Ok(_) => Err(AppError::Message(format!(
                "expected workspace sync DB open to reject {}",
                path.display()
            ))),
            Err(error) => Ok(error.to_string()),
        }
    }

    fn pragma_i32(conn: &Connection, name: &str) -> Result<i32, AppError> {
        conn.pragma_query_value(None, name, |row| row.get(0))
            .map_err(AppError::from)
    }

    fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, AppError> {
        let mut statement = conn.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")?;
        let rows = statement.query_map([table], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    #[test]
    fn initializes_workspace_sync_schema_v1() -> Result<(), AppError> {
        let db = WorkspaceSyncDb::memory()?;
        {
            let conn = db.conn.lock()?;
            assert_eq!(pragma_i32(&conn, "application_id")?, APPLICATION_ID);
            assert_eq!(pragma_i32(&conn, "user_version")?, SCHEMA_VERSION);
        }

        let expected_schema: &[(&str, &[&str])] = &[
            (
                "sync_transactions",
                &[
                    "id",
                    "operation",
                    "state",
                    "base_snapshot_id",
                    "remote_snapshot_id",
                    "result_snapshot_id",
                    "started_at",
                    "updated_at",
                    "error_code",
                    "error_detail",
                ],
            ),
            (
                "provider_results",
                &[
                    "transaction_id",
                    "provider_id",
                    "state",
                    "added_count",
                    "updated_count",
                    "deleted_count",
                    "conflict_count",
                ],
            ),
            (
                "conflicts",
                &[
                    "id",
                    "provider_id",
                    "kind",
                    "logical_id",
                    "state",
                    "reason",
                    "metadata",
                    "created_at",
                    "resolved_at",
                ],
            ),
            (
                "tombstones",
                &[
                    "provider_id",
                    "kind",
                    "logical_id",
                    "last_known_hash",
                    "deleted_at",
                    "deleted_by",
                ],
            ),
            (
                "devices",
                &[
                    "device_id",
                    "device_name",
                    "last_snapshot_id",
                    "last_seen_at",
                    "removed_at",
                ],
            ),
            (
                "snapshot_cache",
                &[
                    "snapshot_id",
                    "parent_ids",
                    "manifest_hash",
                    "created_at",
                    "created_by",
                ],
            ),
            (
                "blob_refs",
                &[
                    "object_id",
                    "local_cache_path",
                    "plain_size",
                    "stored_size",
                    "ref_count",
                    "last_accessed_at",
                ],
            ),
            (
                "provider_schema_cache",
                &[
                    "provider_id",
                    "native_version",
                    "schema_fingerprint",
                    "capabilities",
                    "checked_at",
                ],
            ),
        ];

        for (table, expected_columns) in expected_schema {
            assert!(db.table_exists(table)?, "missing table: {table}");
            let conn = db.conn.lock()?;
            assert_eq!(table_columns(&conn, table)?, *expected_columns);
        }

        let conn = db.conn.lock()?;
        let mut statement = conn.prepare("PRAGMA foreign_key_list(provider_results)")?;
        let foreign_keys = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        let foreign_keys = foreign_keys.collect::<Result<Vec<_>, _>>()?;
        assert!(foreign_keys.iter().any(|(table, from, to, on_delete)| {
            table == "sync_transactions"
                && from == "transaction_id"
                && to == "id"
                && on_delete.eq_ignore_ascii_case("cascade")
        }));

        Ok(())
    }

    #[test]
    fn reopening_disk_database_is_idempotent() -> Result<(), AppError> {
        let (_temp, path) = temp_db_path("workspace-sync.db")?;

        let first = WorkspaceSyncDb::open(&path)?;
        {
            let conn = first.conn.lock()?;
            conn.execute(
                "INSERT INTO sync_transactions (id, operation, state, started_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["sentinel-tx", "push", "complete", 10_i64, 20_i64],
            )?;
            conn.execute(
                "INSERT INTO provider_results \
                 (transaction_id, provider_id, state, added_count, updated_count) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["sentinel-tx", "claude", "complete", 3_i64, 4_i64],
            )?;
        }
        drop(first);

        let reopened = WorkspaceSyncDb::open(&path)?;
        assert_eq!(reopened.user_version()?, SCHEMA_VERSION);
        let conn = reopened.conn.lock()?;
        let sentinel: (String, i64, i64) = conn.query_row(
            "SELECT state, added_count, updated_count FROM provider_results \
             WHERE transaction_id = ?1 AND provider_id = ?2",
            rusqlite::params!["sentinel-tx", "claude"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(sentinel, ("complete".to_string(), 3, 4));

        let journal_mode: String =
            conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        assert_eq!(journal_mode, "wal");
        Ok(())
    }

    #[test]
    fn creates_parent_directory_for_disk_database() -> Result<(), AppError> {
        let temp =
            tempfile::tempdir().map_err(|error| AppError::io("workspace-sync-test", error))?;
        let parent = temp.path().join("nested").join("sync");
        let path = parent.join("workspace-sync.db");

        let db = WorkspaceSyncDb::open(&path)?;

        assert!(parent.is_dir());
        assert!(path.is_file());
        assert_eq!(db.user_version()?, SCHEMA_VERSION);

        Ok(())
    }

    #[test]
    fn rejects_future_schema_version_without_enabling_wal() -> Result<(), AppError> {
        let (_temp, path) = temp_db_path("future.db")?;
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)?;
        drop(conn);

        let error = rejected_open(&path)?;
        assert!(error.contains(&path.display().to_string()));
        assert!(error.contains("stage=identity"));
        assert!(error.contains("future"));

        let conn = Connection::open(&path)?;
        assert_eq!(pragma_i32(&conn, "application_id")?, APPLICATION_ID);
        assert_eq!(pragma_i32(&conn, "user_version")?, SCHEMA_VERSION + 1);
        let journal_mode: String =
            conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        assert_ne!(journal_mode.to_ascii_lowercase(), "wal");
        Ok(())
    }

    #[test]
    fn rejects_wrong_application_id_without_modifying_database() -> Result<(), AppError> {
        let (_temp, path) = temp_db_path("wrong-app.db")?;
        let wrong_application_id = APPLICATION_ID ^ 0x0000_00ff;
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "application_id", wrong_application_id)?;
        drop(conn);

        let error = rejected_open(&path)?;
        assert!(error.contains(&path.display().to_string()));
        assert!(error.contains("stage=identity"));
        assert!(error.contains("application_id"));

        let conn = Connection::open(&path)?;
        assert_eq!(pragma_i32(&conn, "application_id")?, wrong_application_id);
        assert_eq!(pragma_i32(&conn, "user_version")?, 0);
        assert!(!WorkspaceSyncDb::raw_table_exists(
            &conn,
            "sync_transactions"
        )?);
        let journal_mode: String =
            conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        assert_ne!(journal_mode.to_ascii_lowercase(), "wal");
        Ok(())
    }

    #[test]
    fn rejects_nonempty_version_zero_without_modifying_database() -> Result<(), AppError> {
        let (_temp, path) = temp_db_path("nonempty-v0.db")?;
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "CREATE TABLE sentinel (value TEXT NOT NULL); \
             INSERT INTO sentinel (value) VALUES ('keep-me');",
        )?;
        drop(conn);

        let error = rejected_open(&path)?;
        assert!(error.contains(&path.display().to_string()));
        assert!(error.contains("stage=identity"));
        assert!(error.contains("non-empty"));

        let conn = Connection::open(&path)?;
        let sentinel: String =
            conn.query_row("SELECT value FROM sentinel", [], |row| row.get(0))?;
        assert_eq!(sentinel, "keep-me");
        assert_eq!(pragma_i32(&conn, "application_id")?, 0);
        assert_eq!(pragma_i32(&conn, "user_version")?, 0);
        let journal_mode: String =
            conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        assert_ne!(journal_mode.to_ascii_lowercase(), "wal");
        Ok(())
    }

    #[test]
    fn rejects_malformed_schema_v1_without_rewriting_it() -> Result<(), AppError> {
        let (_temp, path) = temp_db_path("malformed-v1.db")?;
        let conn = Connection::open(&path)?;
        conn.execute_batch("CREATE TABLE sync_transactions (id TEXT PRIMARY KEY);")?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        drop(conn);

        let error = rejected_open(&path)?;
        assert!(error.contains(&path.display().to_string()));
        assert!(error.contains("stage=verify"));
        assert!(error.contains("sync_transactions"));

        let conn = Connection::open(&path)?;
        assert_eq!(table_columns(&conn, "sync_transactions")?, ["id"]);
        assert!(!WorkspaceSyncDb::raw_table_exists(
            &conn,
            "provider_results"
        )?);
        let journal_mode: String =
            conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        assert_ne!(journal_mode.to_ascii_lowercase(), "wal");
        Ok(())
    }

    #[test]
    fn rejects_schema_v1_with_inexact_column_metadata() -> Result<(), AppError> {
        let (_temp, path) = temp_db_path("inexact-columns-v1.db")?;
        let conn = Connection::open(&path)?;
        conn.execute_batch(SCHEMA_V1)?;
        conn.execute_batch(
            "DROP TABLE provider_results;
             CREATE TABLE provider_results (
                 transaction_id TEXT NOT NULL,
                 provider_id TEXT NOT NULL,
                 state TEXT NOT NULL,
                 added_count TEXT NOT NULL DEFAULT 1,
                 updated_count INTEGER DEFAULT 0,
                 deleted_count INTEGER NOT NULL DEFAULT 0,
                 conflict_count INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (provider_id, transaction_id),
                 FOREIGN KEY (transaction_id)
                     REFERENCES sync_transactions(id) ON DELETE CASCADE
             );",
        )?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        drop(conn);

        let error = rejected_open(&path)?;
        assert!(error.contains(&path.display().to_string()));
        assert!(error.contains("stage=verify"));
        assert!(error.contains("provider_results"), "{error}");
        assert!(error.contains("columns mismatch"), "{error}");
        Ok(())
    }

    #[test]
    fn deleting_transaction_cascades_provider_results() -> Result<(), AppError> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "OFF")?;
        let db = WorkspaceSyncDb::initialize(connection)?;
        let conn = db.conn.lock()?;

        conn.execute(
            "INSERT INTO sync_transactions (id, operation, state, started_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["tx-1", "push", "running", 1_i64, 1_i64],
        )?;
        conn.execute(
            "INSERT INTO provider_results (transaction_id, provider_id, state) \
             VALUES (?1, ?2, ?3)",
            rusqlite::params!["tx-1", "claude", "pending"],
        )?;
        conn.execute("DELETE FROM sync_transactions WHERE id = ?1", ["tx-1"])?;

        let remaining: i64 = conn.query_row(
            "SELECT COUNT(*) FROM provider_results WHERE transaction_id = ?1",
            ["tx-1"],
            |row| row.get(0),
        )?;
        assert_eq!(remaining, 0);

        Ok(())
    }
}
