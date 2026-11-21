//! Read Antigravity's SQLite files.
//!
//! A WSL UNC path (`\\wsl.localhost\...`) is served by 9P, which has no byte-range
//! locks, so opening it with SQLite fails or hangs. Copy the database and its
//! `-wal` sidecar to a local temp directory and open the copy (letting Windows
//! SQLite build a fresh local `-shm`). The original files are never opened by
//! SQLite and never written.

use rusqlite::{Connection, OpenFlags};
use std::io::ErrorKind;
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

    pub(crate) fn from_connection(conn: Connection) -> Self {
        Self {
            conn,
            _snapshot: None,
        }
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
    // Open the private local copy read-write first so SQLite can recover a copied
    // `-wal` without `-shm` (SQLITE_OPEN_READONLY opens lazily and fails on first
    // SELECT with SQLITE_READONLY_RECOVERY). Fall back to read-only only if opening
    // read-write fails (e.g., read-only file attributes).
    let rw = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = match Connection::open_with_flags(dest, rw) {
        Ok(conn) => conn,
        Err(_) => Connection::open_with_flags(
            dest,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|err| format!("open conversation database snapshot: {err}"))?,
    };
    conn.busy_timeout(SNAPSHOT_BUSY)
        .map_err(|err| err.to_string())?;
    // Replay the copied WAL (and build a fresh local `-shm` if needed) before
    // enforcing `query_only=ON`.
    let _ = conn.query_row("PRAGMA schema_version", [], |_| Ok(()));
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
    // Copy only `-wal` (never `-shm` across OS boundaries so Windows SQLite builds
    // a fresh local index), and copy directly instead of checking `is_file()` first
    // to avoid a TOCTOU failure if agy checkpoints and removes `-wal` concurrently.
    let wal_src = sidecar(src, "-wal");
    match std::fs::copy(&wal_src, sidecar(&dest, "-wal")) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return Err(format!(
                "copy conversation database snapshot {}: {err}",
                wal_src.display()
            ))
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
            r"\\wsl.localhost\Ubuntu-26.04\home\alice\.gemini\antigravity-cli\conversations\0dbfa5d0.db",
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
            "/home/alice/.gemini/antigravity-cli/conversations/0dbfa5d0.db"
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

    #[test]
    fn snapshot_recovers_wal_when_shm_is_missing() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("writer.db");
        let writer = rusqlite::Connection::open(&src_path).unwrap();
        writer
            .execute_batch(
                "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data TEXT);
                 INSERT INTO gen_metadata VALUES (1, 'base');
                 PRAGMA journal_mode=WAL;
                 PRAGMA wal_autocheckpoint=0;",
            )
            .unwrap();
        writer
            .execute("UPDATE gen_metadata SET data='gemini-4-neon'", [])
            .unwrap();
        let src_wal = sidecar(&src_path, "-wal");
        assert!(
            src_wal.is_file() && src_wal.metadata().unwrap().len() > 0,
            "fixture must leave uncheckpointed changes in -wal"
        );

        let no_shm_dir = tempfile::tempdir().unwrap();
        let db_only = no_shm_dir.path().join("child.db");
        std::fs::copy(&src_path, &db_only).unwrap();
        std::fs::copy(&src_wal, sidecar(&db_only, "-wal")).unwrap();
        drop(writer);
        assert!(!sidecar(&db_only, "-shm").exists());

        let snap = open_snapshot(&db_only).unwrap();
        let model: String = snap
            .conn()
            .query_row("SELECT data FROM gen_metadata WHERE idx=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(model, "gemini-4-neon");
        assert!(snap.conn().execute("DELETE FROM gen_metadata", []).is_err());
        assert!(
            !sidecar(&db_only, "-shm").exists(),
            "source directory must remain untouched without -shm"
        );
    }
}
