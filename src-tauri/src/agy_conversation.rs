//! Shared read-only access to Antigravity's native conversation resources.
pub(crate) mod models;

use rusqlite::{Connection, OpenFlags};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // Include committed WAL records, but never wait on an exclusive native writer.
    conn.busy_timeout(Duration::ZERO)?;
    Ok(conn)
}

pub(crate) fn for_transcript(transcript: &Path) -> Result<Option<PathBuf>, String> {
    let session = transcript
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("Missing transcript session directory")?;
    let root = session
        .parent()
        .filter(|brain| brain.file_name().is_some_and(|name| name == "brain"))
        .and_then(Path::parent)
        .ok_or("Missing Antigravity root")?;
    let id = session.file_name().ok_or("Missing session id")?;
    let path = root.join("conversations").join(id).with_extension("db");
    if !path.is_file() {
        return Ok(None);
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if !path.starts_with(root) {
        return Err("Conversation database is outside the Antigravity root".into());
    }
    Ok(Some(path))
}
