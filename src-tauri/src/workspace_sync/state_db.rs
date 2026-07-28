use std::fs;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::AppError;

pub struct WorkspaceSyncDb {
    conn: Mutex<Connection>,
}

impl WorkspaceSyncDb {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
        }

        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::initialize(conn)
    }

    pub fn memory() -> Result<Self, AppError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    pub fn user_version(&self) -> Result<i32, AppError> {
        let conn = self.conn.lock()?;
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(AppError::from)
    }

    fn initialize(mut conn: Connection) -> Result<Self, AppError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let transaction = conn.transaction()?;
        transaction.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sync_transactions (
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

            CREATE TABLE IF NOT EXISTS provider_results (
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

            CREATE TABLE IF NOT EXISTS conflicts (
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

            CREATE TABLE IF NOT EXISTS tombstones (
                provider_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                logical_id TEXT NOT NULL,
                last_known_hash TEXT,
                deleted_at INTEGER NOT NULL,
                deleted_by TEXT NOT NULL,
                PRIMARY KEY (provider_id, kind, logical_id)
            );

            CREATE TABLE IF NOT EXISTS devices (
                device_id TEXT PRIMARY KEY,
                device_name TEXT NOT NULL,
                last_snapshot_id TEXT,
                last_seen_at INTEGER NOT NULL,
                removed_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS snapshot_cache (
                snapshot_id TEXT PRIMARY KEY,
                parent_ids TEXT NOT NULL,
                manifest_hash TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                created_by TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS blob_refs (
                object_id TEXT PRIMARY KEY,
                local_cache_path TEXT,
                plain_size INTEGER NOT NULL,
                stored_size INTEGER NOT NULL,
                ref_count INTEGER NOT NULL,
                last_accessed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS provider_schema_cache (
                provider_id TEXT NOT NULL,
                native_version TEXT NOT NULL,
                schema_fingerprint TEXT NOT NULL,
                capabilities TEXT NOT NULL,
                checked_at INTEGER NOT NULL,
                PRIMARY KEY (provider_id, schema_fingerprint)
            );

            PRAGMA user_version = 1;
            "#,
        )?;
        transaction.commit()?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    fn table_exists(&self, table: &str) -> Result<bool, AppError> {
        let conn = self.conn.lock()?;
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(AppError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceSyncDb;
    use crate::error::AppError;
    use rusqlite::Connection;

    #[test]
    fn initializes_workspace_sync_schema_v1() -> Result<(), AppError> {
        let db = WorkspaceSyncDb::memory()?;

        assert_eq!(db.user_version()?, 1);
        for table in [
            "sync_transactions",
            "provider_results",
            "conflicts",
            "tombstones",
            "devices",
            "snapshot_cache",
            "blob_refs",
            "provider_schema_cache",
        ] {
            assert!(db.table_exists(table)?, "missing table: {table}");
        }

        Ok(())
    }

    #[test]
    fn reopening_disk_database_is_idempotent() -> Result<(), AppError> {
        let temp =
            tempfile::tempdir().map_err(|error| AppError::io("workspace-sync-test", error))?;
        let path = temp.path().join("workspace-sync.db");

        let first = WorkspaceSyncDb::open(&path)?;
        assert_eq!(first.user_version()?, 1);
        drop(first);

        let reopened = WorkspaceSyncDb::open(&path)?;
        assert_eq!(reopened.user_version()?, 1);
        assert!(reopened.table_exists("sync_transactions")?);
        let journal_mode: String =
            reopened
                .conn
                .lock()?
                .pragma_query_value(None, "journal_mode", |row| row.get(0))?;
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
        assert_eq!(db.user_version()?, 1);

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
