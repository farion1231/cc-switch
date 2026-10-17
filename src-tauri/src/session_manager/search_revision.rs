//! SQLite 正文来源的会话级 revision。不能用整库 mtime，也不能仅依赖心跳时间。
//! 每轮复用只读连接，在同一快照内散列该会话的原始行；不解析消息或重写 FTS。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use rusqlite::{types::ValueRef, Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};

use super::cache::{self, Fingerprint};
use super::content::{SourceLocation, ValidatedSource};
use super::search::StoredFingerprint;

struct CachedDatabase {
    fingerprint: Fingerprint,
    revisions: HashMap<(String, String), StoredFingerprint>,
}

// The file fingerprint is only a fast path for a wholly unchanged DB. After a
// write, compare per-session revisions so unrelated sessions keep their FTS rows.
static CACHE: LazyLock<Mutex<HashMap<PathBuf, CachedDatabase>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Default)]
pub(super) struct SqliteRevisions {
    connections: HashMap<PathBuf, Connection>,
    databases: HashMap<PathBuf, CachedDatabase>,
}

#[cfg(test)]
impl SqliteRevisions {
    pub(super) fn opened_databases(&self) -> usize {
        self.connections.len()
    }
}

impl SqliteRevisions {
    pub fn fingerprint(&mut self, source: &ValidatedSource) -> Result<StoredFingerprint, String> {
        let SourceLocation::Sqlite { db, session_id } = &source.location else {
            return Err("Expected a SQLite session source".into());
        };
        let result = (|| -> Result<StoredFingerprint, String> {
            if !self.databases.contains_key(db) {
                let fingerprint =
                    cache::fingerprint(&source.location).map_err(|e| e.to_string())?;
                let previous = CACHE.lock().unwrap_or_else(|e| e.into_inner()).remove(db);
                let cached = previous
                    .filter(|cached| cached.fingerprint == fingerprint)
                    .unwrap_or_else(|| CachedDatabase {
                        fingerprint,
                        revisions: HashMap::new(),
                    });
                self.databases.insert(db.clone(), cached);
            }
            let key = (source.provider_id.clone(), session_id.clone());
            if let Some(fingerprint) = self.databases[db].revisions.get(&key) {
                return Ok(*fingerprint);
            }
            if !self.connections.contains_key(db) {
                let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(|e| e.to_string())?;
                self.connections.insert(db.clone(), conn);
            }
            let fingerprint = revision(&self.connections[db], &source.provider_id, session_id)
                .map_err(|e| e.to_string())?;
            self.databases
                .get_mut(db)
                .unwrap()
                .revisions
                .insert(key, fingerprint);
            Ok(fingerprint)
        })();
        result.map_err(|error| format!("Cannot read session revision: {error}"))
    }
}

impl Drop for SqliteRevisions {
    fn drop(&mut self) {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        // This is an optional in-memory optimization; bound obsolete DB paths
        // after settings changes instead of retaining them for the app lifetime.
        if cache.len() + self.databases.len() > 8 {
            cache.clear();
        }
        cache.extend(self.databases.drain().take(8));
    }
}

fn revision(
    conn: &Connection,
    provider: &str,
    session: &str,
) -> rusqlite::Result<StoredFingerprint> {
    // Tables are fixed identifiers, never input from a source path or session ID.
    let tables: &[&str] = match provider {
        "opencode" => &["message", "part", "session_message", "session_v2"],
        "hermes" => &["messages"],
        "mcode" => &[
            "local_runtime_message_row_migrations",
            "local_runtime_message_rows",
            "local_runtime_messages",
        ],
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let tx = conn.unchecked_transaction()?;
    let mut digest = Sha256::new();
    digest.update(b"session-search-revision-v1");
    digest.update(provider.as_bytes());
    let mut found_content = false;
    for table in tables {
        let schema: Option<String> = tx
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()?;
        digest.update(table.as_bytes());
        let Some(schema) = schema else {
            digest.update([0]);
            continue;
        };
        digest.update([1]);
        digest.update(schema.as_bytes());
        // session_v2 membership controls which parser OpenCode uses in a mixed DB.
        let column = if *table == "session_v2" {
            "id"
        } else {
            "session_id"
        };
        let mut stmt = tx.prepare(&format!(
            "SELECT * FROM {table} WHERE {column} = ?1 ORDER BY rowid"
        ))?;
        let columns = stmt.column_count();
        let mut rows = stmt.query([session])?;
        while let Some(row) = rows.next()? {
            digest.update([2]);
            for column in 0..columns {
                match row.get_ref(column)? {
                    ValueRef::Null => digest.update([0]),
                    ValueRef::Integer(value) => {
                        digest.update([1]);
                        digest.update(value.to_le_bytes());
                    }
                    ValueRef::Real(value) => {
                        digest.update([2]);
                        digest.update(value.to_bits().to_le_bytes());
                    }
                    ValueRef::Text(value) | ValueRef::Blob(value) => {
                        digest.update([if matches!(row.get_ref(column)?, ValueRef::Text(_)) {
                            3
                        } else {
                            4
                        }]);
                        digest.update((value.len() as u64).to_le_bytes());
                        digest.update(value);
                    }
                }
            }
        }
        found_content |= *table != "session_v2";
    }
    if !found_content {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let hash = digest.finalize();
    // Negative mtime marks revisions, avoiding overlap with legacy file timestamps.
    Ok(StoredFingerprint {
        modified_ns: Some(i64::from_le_bytes(hash[..8].try_into().unwrap()) | i64::MIN),
        len: i64::from_le_bytes(hash[8..16].try_into().unwrap()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_detect_edits_deletions_and_schema_changes_without_timestamps() {
        for (provider, table) in [
            ("hermes", "messages"),
            ("opencode", "part"),
            ("opencode", "session_message"),
            ("mcode", "local_runtime_messages"),
            ("mcode", "local_runtime_message_rows"),
            ("mcode", "local_runtime_message_row_migrations"),
        ] {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(&format!(
                "CREATE TABLE {table}(session_id TEXT, body TEXT, timestamp INTEGER);
                 INSERT INTO {table} VALUES ('a', 'original', 1), ('b', 'other', NULL);"
            ))
            .unwrap();
            let initial = revision(&conn, provider, "a").unwrap();
            let other = revision(&conn, provider, "b").unwrap();
            conn.execute(
                &format!("UPDATE {table} SET body = 'edited' WHERE session_id = 'a'"),
                [],
            )
            .unwrap();
            let edited = revision(&conn, provider, "a").unwrap();
            assert_ne!(initial, edited, "{provider}/{table}");
            assert_eq!(other, revision(&conn, provider, "b").unwrap());
            conn.execute(&format!("DELETE FROM {table} WHERE session_id = 'a'"), [])
                .unwrap();
            assert_ne!(edited, revision(&conn, provider, "a").unwrap());
            conn.execute(
                &format!("ALTER TABLE {table} ADD COLUMN active INTEGER"),
                [],
            )
            .unwrap();
            assert_ne!(other, revision(&conn, provider, "b").unwrap());
        }
    }

    #[test]
    fn checkpoint_does_not_change_revision_and_failed_reads_can_retry() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
            CREATE TABLE messages(session_id TEXT, body TEXT);
            INSERT INTO messages VALUES ('a', 'body');",
        )
        .unwrap();
        let source = ValidatedSource {
            provider_id: "hermes".into(),
            raw: "unused".into(),
            location: SourceLocation::Sqlite {
                db,
                session_id: "a".into(),
            },
        };
        let initial = SqliteRevisions::default().fingerprint(&source).unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        assert_eq!(
            initial,
            SqliteRevisions::default().fingerprint(&source).unwrap()
        );
        conn.execute_batch("ALTER TABLE messages RENAME TO unavailable")
            .unwrap();
        assert!(SqliteRevisions::default().fingerprint(&source).is_err());
        conn.execute_batch("ALTER TABLE unavailable RENAME TO messages")
            .unwrap();
        // ALTER TABLE may normalize the CREATE statement; recovery must retry,
        // rather than treating the earlier failed read as a cached revision.
        let recovered = SqliteRevisions::default().fingerprint(&source).unwrap();
        assert_eq!(recovered, revision(&conn, "hermes", "a").unwrap());
    }
}
