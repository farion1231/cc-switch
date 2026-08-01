//! Codex (`~/.codex`) workspace mappings.
//!
//! `sessions/` + `archived_sessions/` are append-only transcripts and
//! `memories/` are text notes. `history.jsonl` / `session_index.jsonl` are
//! line-append JSONL logs, so they merge by **line union** (`AppendOnly`), not
//! whole-file overwrite — otherwise one device's appended lines clobber the
//! other's.
//!
//! The `state_5.sqlite` thread-index DB is synced as a whole file because the
//! Codex client lists resumable threads from it — without it, synced sessions
//! do not appear in the client. Before packing we run a WAL checkpoint so the
//! main file is self-contained; on restore we atomically replace it and drop any
//! stale `-wal`/`-shm` sidecars so SQLite doesn't replay an old WAL over the
//! imported DB. Other sqlite databases (`goals_*`, `memories_*`) remain excluded
//! (no client-visible index depends on them).

use std::path::{Path, PathBuf};

use super::{FsAdapter, Mapping, ProviderAdapter};
use crate::codex_config::get_codex_config_dir;
use crate::codex_state_db::CODEX_STATE_DB_FILENAME;
use crate::error::AppError;
use crate::workspace_sync::model::{
    DataItem, DataKind, MergeCapability, Sensitivity, WorkspaceProviderId,
};

const MAPPINGS: &[Mapping] = &[
    Mapping::dir("sessions", DataKind::Session, MergeCapability::AppendOnly),
    Mapping::dir(
        "archived_sessions",
        DataKind::Session,
        MergeCapability::AppendOnly,
    ),
    Mapping::dir("memories", DataKind::Memory, MergeCapability::Text),
    // Line-append JSONL logs → line union, not overwrite.
    Mapping::file("history.jsonl", DataKind::Index, MergeCapability::AppendOnly),
    Mapping::file(
        "session_index.jsonl",
        DataKind::Index,
        MergeCapability::AppendOnly,
    ),
    Mapping::file("AGENTS.md", DataKind::Memory, MergeCapability::Text),
];

pub fn adapter() -> CodexAdapter {
    let root = get_codex_config_dir();
    CodexAdapter {
        inner: FsAdapter::new(WorkspaceProviderId::Codex, root.clone(), MAPPINGS),
        root,
    }
}

/// Wraps the generic [`FsAdapter`] and adds whole-file sync of the Codex
/// thread-index SQLite DB (`state_5.sqlite`).
pub struct CodexAdapter {
    inner: FsAdapter,
    root: PathBuf,
}

impl CodexAdapter {
    fn state_db_path(&self) -> PathBuf {
        self.root.join(CODEX_STATE_DB_FILENAME)
    }

    /// Merge the WAL into the main DB file so a raw file read is self-contained.
    /// Best-effort: if Codex holds a write lock or the DB is busy, we skip the
    /// checkpoint and fall back to reading the file as-is.
    fn checkpoint_state_db(path: &Path) {
        use rusqlite::{Connection, OpenFlags};
        let Ok(conn) = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            return;
        };
        let _ = conn.busy_timeout(std::time::Duration::from_secs(2));
        // TRUNCATE checkpoints all frames and zeroes the WAL file.
        let _ = conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
    }
}

impl ProviderAdapter for CodexAdapter {
    fn provider(&self) -> WorkspaceProviderId {
        self.inner.provider()
    }

    fn is_installed(&self) -> bool {
        self.inner.is_installed()
    }

    fn scan(&self) -> Result<Vec<DataItem>, AppError> {
        let mut items = self.inner.scan()?;

        // Append the thread-index DB as a whole-file item (checkpoint first).
        let db_path = self.state_db_path();
        if db_path.is_file() {
            Self::checkpoint_state_db(&db_path);
            let bytes = std::fs::read(&db_path).map_err(|e| AppError::io(&db_path, e))?;
            let content_hash = crate::services::sync_protocol::sha256_hex(&bytes);
            let updated_at = std::fs::metadata(&db_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64);
            items.push(DataItem {
                provider: WorkspaceProviderId::Codex,
                kind: DataKind::Index,
                logical_id: CODEX_STATE_DB_FILENAME.to_string(),
                parent_id: None,
                native_path: CODEX_STATE_DB_FILENAME.to_string(),
                content_hash: content_hash.clone(),
                updated_at,
                schema_fingerprint: None,
                merge_capability: MergeCapability::Opaque,
                sensitivity: Sensitivity::WorkData,
                object_ids: vec![content_hash],
            });
        }

        Ok(items)
    }

    fn materialize(&self, item: &DataItem, bytes: &[u8]) -> Result<(), AppError> {
        if item.native_path == CODEX_STATE_DB_FILENAME {
            // Atomic replace + drop stale WAL/SHM so SQLite doesn't replay an old
            // WAL over the freshly imported DB.
            let target = self.state_db_path();
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
            }
            let tmp = target.with_extension("sqlite.cc-switch-tmp");
            std::fs::write(&tmp, bytes).map_err(|e| AppError::io(&tmp, e))?;
            std::fs::rename(&tmp, &target).map_err(|e| AppError::io(&target, e))?;
            let _ = std::fs::remove_file(target.with_file_name(format!(
                "{CODEX_STATE_DB_FILENAME}-wal"
            )));
            let _ = std::fs::remove_file(target.with_file_name(format!(
                "{CODEX_STATE_DB_FILENAME}-shm"
            )));
            return Ok(());
        }
        self.inner.materialize(item, bytes)
    }

    fn read_blob(&self, item: &DataItem) -> Result<Vec<u8>, AppError> {
        if item.native_path == CODEX_STATE_DB_FILENAME {
            let path = self.state_db_path();
            return std::fs::read(&path).map_err(|e| AppError::io(&path, e));
        }
        self.inner.read_blob(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn make_state_db(path: &Path, thread_ids: &[&str]) {
        let conn = Connection::open(path).expect("open");
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT NOT NULL, first_user_message TEXT)",
            [],
        )
        .unwrap();
        for id in thread_ids {
            conn.execute(
                "INSERT INTO threads (id, title, first_user_message) VALUES (?1, ?2, NULL)",
                (id, format!("title-{id}")),
            )
            .unwrap();
        }
        // Leave the connection open with WAL frames not yet checkpointed to
        // exercise the checkpoint-before-pack path.
        std::mem::forget(conn);
    }

    fn thread_count(path: &Path) -> i64 {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .expect("open ro");
        conn.query_row("SELECT COUNT(*) FROM threads", [], |r| r.get(0))
            .expect("count")
    }

    #[test]
    fn state_db_scan_checkpoints_and_materialize_roundtrips() {
        let src = tempfile::tempdir().expect("src");
        let dst = tempfile::tempdir().expect("dst");
        let src_db = src.path().join(CODEX_STATE_DB_FILENAME);
        make_state_db(&src_db, &["t1", "t2", "t3"]);

        let src_adapter = CodexAdapter {
            inner: FsAdapter::new(WorkspaceProviderId::Codex, src.path().to_path_buf(), MAPPINGS),
            root: src.path().to_path_buf(),
        };
        let items = src_adapter.scan().expect("scan");
        let db_item = items
            .iter()
            .find(|i| i.native_path == CODEX_STATE_DB_FILENAME)
            .expect("state db item present");
        let bytes = src_adapter.read_blob(db_item).expect("read blob");

        let dst_adapter = CodexAdapter {
            inner: FsAdapter::new(WorkspaceProviderId::Codex, dst.path().to_path_buf(), MAPPINGS),
            root: dst.path().to_path_buf(),
        };
        dst_adapter.materialize(db_item, &bytes).expect("materialize");

        let dst_db = dst.path().join(CODEX_STATE_DB_FILENAME);
        assert!(dst_db.is_file(), "state db written");
        // No stale WAL/SHM left behind.
        assert!(!dst
            .path()
            .join(format!("{CODEX_STATE_DB_FILENAME}-wal"))
            .exists());
        // The imported DB opens and has all threads (checkpoint merged the WAL).
        assert_eq!(thread_count(&dst_db), 3);
    }
}
