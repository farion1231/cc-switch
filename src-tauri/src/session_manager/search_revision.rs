//! SQLite 正文来源的会话级 revision。不能用整库 mtime，也不能仅依赖心跳时间。
//! 每轮复用只读连接，在同一快照内散列该会话的原始行；不解析消息或重写 FTS。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use rusqlite::{types::ValueRef, Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};

use super::cache::{self, Fingerprint};
use super::content::{SourceLocation, ValidatedSource};
use super::search::StoredFingerprint;

struct CachedDatabase {
    fingerprint: Fingerprint,
    revisions: HashMap<(String, String), StoredFingerprint>,
    /// Cursor 的正文来自 `conversation-search.db` 和 `state.vscdb` 两个库，单个
    /// `(mtime, len)` 无法同时反映两者的独立变化，因此不跨轮次复用，只在单轮内
    /// 按会话复用已算出的 revision。
    reusable: bool,
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
                // Cursor 的正文分散在两个库，文件级 (mtime, len) 无法覆盖两者的独立
                // 变化；禁止跨轮次复用，每轮重新按会话散列。单轮内仍按会话复用。
                let reusable = source.provider_id != "cursor";
                let cached = if reusable {
                    CACHE
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(db)
                        .filter(|cached| cached.reusable && cached.fingerprint == fingerprint)
                } else {
                    None
                }
                .unwrap_or_else(|| CachedDatabase {
                    fingerprint,
                    revisions: HashMap::new(),
                    reusable,
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
            let fingerprint = if source.provider_id == "cursor" {
                self.cursor_revision(db, session_id)
                    .map_err(|e| e.to_string())?
            } else {
                revision(&self.connections[db], &source.provider_id, session_id)
                    .map_err(|e| e.to_string())?
            };
            self.databases
                .get_mut(db)
                .unwrap()
                .revisions
                .insert(key, fingerprint);
            Ok(fingerprint)
        })();
        result.map_err(|error| format!("Cannot read session revision: {error}"))
    }

    /// Cursor 的正文分散在两个库：`state.vscdb`（结构化 composer/bubble）和
    /// `conversation-search.db`（FTS 回退转录）。这里按需打开 state.vscdb，
    /// 读取错误必须上抛，让索引器保留旧行而不是把会话当成已删除。
    fn cursor_revision(
        &mut self,
        search_db: &Path,
        session_id: &str,
    ) -> Result<StoredFingerprint, rusqlite::Error> {
        let state_db = crate::session_manager::providers::cursor::get_state_vscdb_path();
        let state_present = match std::fs::metadata(&state_db) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(rusqlite::Error::InvalidPath(state_db)),
        };
        if state_present && !self.connections.contains_key(&state_db) {
            let conn = Connection::open_with_flags(
                &state_db,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            self.connections.insert(state_db.clone(), conn);
        }
        let search = self
            .connections
            .get(search_db)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        let state = self.connections.get(&state_db).filter(|_| state_present);
        cursor_revision(search, state, session_id)
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
        // 不可跨轮次复用的数据库（Cursor）不再放进进程级缓存。
        cache.extend(
            self.databases
                .drain()
                .filter(|(_, database)| database.reusable)
                .take(8),
        );
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

fn table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
}

fn hash_optional_text(digest: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            digest.update([1]);
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
        None => digest.update([0]),
    }
}

/// Cursor 会话级 revision：只散列该会话在 `conversation-search.db` 的 FTS 正文
/// 和 `state.vscdb` 的 composerData + 本会话 bubbleId 行。
///
/// - 别的会话写入 state.vscdb 只会改变文件级快路径，按会话散列结果不变
/// - 两个库任一正文变化（修改 / 删除 / 新增）都会改变 revision
/// - 表缺失按「无此来源」处理；文件损坏等真实读取错误仍返回 `Err`
fn cursor_revision(
    search: &Connection,
    state: Option<&Connection>,
    session: &str,
) -> rusqlite::Result<StoredFingerprint> {
    let conversations = table_exists(search, "conversations")?;
    let fts = table_exists(search, "conversation_fts")?;
    let disk = match state {
        Some(conn) => table_exists(conn, "cursorDiskKV")?,
        None => false,
    };
    if !(conversations || fts || disk) {
        return Err(rusqlite::Error::InvalidQuery);
    }

    let mut digest = Sha256::new();
    digest.update(b"session-search-revision-v1");
    digest.update(b"cursor");
    digest.update([u8::from(conversations), u8::from(fts), u8::from(disk)]);

    digest.update(b"conversations");
    let row = if conversations {
        search
            .query_row(
                "SELECT fts_rowid, title FROM conversations WHERE id = ?1",
                [session],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
    } else {
        None
    };
    match row {
        Some((fts_rowid, title)) => {
            digest.update([1]);
            digest.update(fts_rowid.to_le_bytes());
            hash_optional_text(&mut digest, Some(&title));
            digest.update(b"conversation_fts");
            let body = if fts {
                search
                    .query_row(
                        "SELECT body FROM conversation_fts WHERE rowid = ?1",
                        [fts_rowid],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten()
            } else {
                None
            };
            hash_optional_text(&mut digest, body.as_deref());
        }
        None => digest.update([0]),
    }

    digest.update(b"cursorDiskKV");
    if let Some(conn) = state.filter(|_| disk) {
        let composer = conn
            .query_row(
                "SELECT value FROM cursorDiskKV WHERE key = ?1",
                [format!("composerData:{session}")],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        hash_optional_text(&mut digest, composer.as_deref());

        let prefix = format!("bubbleId:{session}:");
        // 索引前缀范围扫描；上界用最大 Unicode 字符，覆盖任意 bubbleId。
        let upper = format!("{prefix}\u{10FFFF}");
        let mut stmt = conn.prepare(
            "SELECT key, value FROM cursorDiskKV WHERE key >= ?1 AND key < ?2 ORDER BY key",
        )?;
        let mut rows = stmt.query(rusqlite::params![prefix, upper])?;
        while let Some(row) = rows.next()? {
            let key: String = row.get(0)?;
            let value: Option<String> = row.get(1)?;
            digest.update([2]);
            digest.update((key.len() as u64).to_le_bytes());
            digest.update(key.as_bytes());
            hash_optional_text(&mut digest, value.as_deref());
        }
    }

    let hash = digest.finalize();
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
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
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
