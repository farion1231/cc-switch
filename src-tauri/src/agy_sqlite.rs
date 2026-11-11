//! Read Antigravity's SQLite files.
//!
//! A WSL UNC path (`\\wsl.localhost\...`) is served by 9P, which has no byte-range
//! locks, so opening it with SQLite fails or hangs. Copy the database and its
//! `-wal`/`-shm` sidecars to a local temp directory and open the copy. The
//! original files are never opened by SQLite and never written.

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

const SNAPSHOT_BUSY: Duration = Duration::from_secs(3);

pub(crate) struct AgyReadDb {
    conn: Connection,
    // Dropped after `conn`, so the snapshot outlives the connection.
    _snapshot: Option<tempfile::TempDir>,
}

impl AgyReadDb {
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// `true` when `path` is a WSL UNC path and must be snapshotted before SQLite opens it.
pub(crate) fn needs_local_snapshot(path: &Path) -> bool {
    crate::config::is_wsl_path(path)
}

pub(crate) fn open_read_only(path: &Path) -> Result<AgyReadDb, String> {
    if needs_local_snapshot(path) {
        open_snapshot(path)
    } else {
        open_direct(path)
    }
}

fn open_direct(path: &Path) -> Result<AgyReadDb, String> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|err| err.to_string())?;
    // A live local writer must not stall usage refresh. WSL snapshots use a timeout.
    conn.busy_timeout(Duration::ZERO)
        .map_err(|err| err.to_string())?;
    Ok(AgyReadDb {
        conn,
        _snapshot: None,
    })
}

fn open_snapshot(path: &Path) -> Result<AgyReadDb, String> {
    let (dir, dest) = copy_sqlite_family(path)?;
    let conn = open_snapshot_conn(&dest)?;
    Ok(AgyReadDb {
        conn,
        _snapshot: Some(dir),
    })
}

fn open_snapshot_conn(dest: &Path) -> Result<Connection, String> {
    let readonly = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = match Connection::open_with_flags(dest, readonly) {
        Ok(conn) => conn,
        Err(_) => Connection::open_with_flags(
            dest,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|err| format!("open conversation database snapshot: {err}"))?,
    };
    conn.busy_timeout(SNAPSHOT_BUSY)
        .map_err(|err| err.to_string())?;
    // The copy is ours. query_only keeps a read-write open (needed so SQLite can
    // replay the copied WAL) from writing back into the snapshot.
    conn.execute_batch("PRAGMA query_only=ON;")
        .map_err(|err| err.to_string())?;
    Ok(conn)
}

fn copy_sqlite_family(src: &Path) -> Result<(tempfile::TempDir, PathBuf), String> {
    let name = src
        .file_name()
        .ok_or_else(|| "conversation database has no file name".to_string())?;
    let dir = tempfile::Builder::new()
        .prefix("cc-switch-agy-")
        .tempdir()
        .map_err(|err| err.to_string())?;
    let dest = dir.path().join(name);
    std::fs::copy(src, &dest).map_err(|err| {
        format!(
            "copy conversation database snapshot {}: {err}",
            src.display()
        )
    })?;
    for suffix in ["-wal", "-shm"] {
        let from = sidecar(src, suffix);
        if from.is_file() {
            std::fs::copy(&from, sidecar(&dest, suffix)).map_err(|err| {
                format!(
                    "copy conversation database snapshot {}: {err}",
                    from.display()
                )
            })?;
        }
    }
    Ok((dir, dest))
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unc_conversation_database_is_copied_locally_before_it_is_read() {
        let unc = Path::new(
            r"\\wsl.localhost\Ubuntu-26.04\home\sean\.gemini\antigravity-cli\conversations\0dbfa5d0.db",
        );
        assert!(needs_local_snapshot(unc));
        let err = match open_read_only(unc) {
            Err(err) => err,
            Ok(_) => panic!("a missing UNC database must not open"),
        };
        assert!(
            err.contains("snapshot"),
            "UNC open must try a local snapshot before SQLite, got {err}"
        );
        assert!(!crate::config::is_wsl_path(Path::new(
            "/home/sean/.gemini/antigravity-cli/conversations/0dbfa5d0.db"
        )));

        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("child.db");
        let writer = rusqlite::Connection::open(&path).unwrap();
        writer
            .execute_batch(
                "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data TEXT);
                 INSERT INTO gen_metadata VALUES (1, 'base');
                 PRAGMA journal_mode=WAL;
                 PRAGMA wal_autocheckpoint=0;",
            )
            .unwrap();
        writer
            .execute("UPDATE gen_metadata SET data='gemini-4-argon'", [])
            .unwrap();
        assert!(
            sidecar(&path, "-wal").is_file(),
            "the fixture must keep the new model in the WAL"
        );
        let snap = open_snapshot(&path).unwrap();
        let model: String = snap
            .conn()
            .query_row("SELECT data FROM gen_metadata WHERE idx=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(model, "gemini-4-argon");
        // The snapshot is query-only and the source file is still in place.
        assert!(snap.conn().execute("DELETE FROM gen_metadata", []).is_err());
        assert!(path.is_file());
    }
}
