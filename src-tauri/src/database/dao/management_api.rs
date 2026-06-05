use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTokenRecord {
    pub id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiTokenLookup {
    pub record: ApiTokenRecord,
    pub token_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiPairingSessionRecord {
    pub id: String,
    pub client_name: String,
    pub requested_scopes: Vec<String>,
    pub approved_scopes: Option<Vec<String>>,
    pub status: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub approved_token_id: Option<String>,
    pub token_delivered_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ApiPairingSessionLookup {
    pub record: ApiPairingSessionRecord,
    pub poll_secret_hash: String,
}

#[derive(Debug, Clone)]
pub struct ConsumedPairingToken {
    pub token: String,
    pub approved_scopes: Vec<String>,
}

impl Database {
    pub(crate) fn create_management_api_tables_on_conn(
        conn: &rusqlite::Connection,
    ) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS api_tokens (
                id TEXT PRIMARY KEY,
                token_hash TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                scopes TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER,
                last_used_at INTEGER,
                revoked_at INTEGER,
                source TEXT
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_tokens_hash ON api_tokens(token_hash)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS api_pairing_sessions (
                id TEXT PRIMARY KEY,
                client_name TEXT NOT NULL,
                poll_secret_hash TEXT NOT NULL,
                requested_scopes TEXT NOT NULL,
                approved_scopes TEXT,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                approved_token_id TEXT,
                approved_token_secret TEXT,
                token_delivered_at INTEGER
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_pairing_status
             ON api_pairing_sessions(status, expires_at)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS api_audit_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                token_id TEXT,
                scope TEXT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                status INTEGER NOT NULL,
                request_id TEXT NOT NULL,
                remote_ip TEXT,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_audit_created_at
             ON api_audit_logs(created_at DESC)",
            [],
        )?;
        Ok(())
    }

    pub fn create_api_token(
        &self,
        id: &str,
        token_hash: &str,
        name: &str,
        scopes: &[String],
        expires_at: Option<i64>,
        source: Option<&str>,
    ) -> Result<ApiTokenRecord, AppError> {
        let created_at = chrono::Utc::now().timestamp_millis();
        let scopes_json = serde_json::to_string(scopes).map_err(|e| {
            AppError::Database(format!("Failed to serialize API token scopes: {e}"))
        })?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO api_tokens
             (id, token_hash, name, scopes, created_at, expires_at, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                token_hash,
                name,
                scopes_json,
                created_at,
                expires_at,
                source
            ],
        )?;
        Ok(ApiTokenRecord {
            id: id.to_string(),
            name: name.to_string(),
            scopes: scopes.to_vec(),
            created_at,
            expires_at,
            last_used_at: None,
            revoked_at: None,
            source: source.map(str::to_string),
        })
    }

    pub fn list_api_tokens(&self) -> Result<Vec<ApiTokenRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, scopes, created_at, expires_at, last_used_at, revoked_at, source
             FROM api_tokens
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let scopes_json: String = row.get(2)?;
            let scopes = serde_json::from_str::<Vec<String>>(&scopes_json).unwrap_or_default();
            Ok(ApiTokenRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                scopes,
                created_at: row.get(3)?,
                expires_at: row.get(4)?,
                last_used_at: row.get(5)?,
                revoked_at: row.get(6)?,
                source: row.get(7)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_api_token_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<ApiTokenLookup>, AppError> {
        let conn = lock_conn!(self.conn);
        let row = conn
            .query_row(
                "SELECT id, token_hash, name, scopes, created_at, expires_at,
                        last_used_at, revoked_at, source
                 FROM api_tokens WHERE token_hash = ?1",
                params![token_hash],
                |row| {
                    let scopes_json: String = row.get(3)?;
                    let scopes =
                        serde_json::from_str::<Vec<String>>(&scopes_json).unwrap_or_default();
                    Ok(ApiTokenLookup {
                        token_hash: row.get(1)?,
                        record: ApiTokenRecord {
                            id: row.get(0)?,
                            name: row.get(2)?,
                            scopes,
                            created_at: row.get(4)?,
                            expires_at: row.get(5)?,
                            last_used_at: row.get(6)?,
                            revoked_at: row.get(7)?,
                            source: row.get(8)?,
                        },
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn touch_api_token(&self, id: &str) -> Result<(), AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE api_tokens SET last_used_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn revoke_api_token(&self, id: &str) -> Result<bool, AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        let conn = lock_conn!(self.conn);
        let changed = conn.execute(
            "UPDATE api_tokens SET revoked_at = COALESCE(revoked_at, ?1) WHERE id = ?2",
            params![now, id],
        )?;
        Ok(changed > 0)
    }

    pub fn active_api_token_count(&self) -> Result<i64, AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT COUNT(*) FROM api_tokens
             WHERE revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?1)",
            params![now],
            |row| row.get(0),
        )
        .map_err(Into::into)
    }

    pub fn create_api_pairing_session(
        &self,
        id: &str,
        client_name: &str,
        poll_secret_hash: &str,
        requested_scopes: &[String],
        expires_at: i64,
    ) -> Result<ApiPairingSessionRecord, AppError> {
        let created_at = chrono::Utc::now().timestamp_millis();
        let scopes_json = serde_json::to_string(requested_scopes)
            .map_err(|e| AppError::Database(format!("Failed to serialize pairing scopes: {e}")))?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO api_pairing_sessions
             (id, client_name, poll_secret_hash, requested_scopes, status, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6)",
            params![
                id,
                client_name,
                poll_secret_hash,
                scopes_json,
                created_at,
                expires_at
            ],
        )?;
        Ok(ApiPairingSessionRecord {
            id: id.to_string(),
            client_name: client_name.to_string(),
            requested_scopes: requested_scopes.to_vec(),
            approved_scopes: None,
            status: "pending".to_string(),
            created_at,
            expires_at,
            approved_token_id: None,
            token_delivered_at: None,
        })
    }

    pub fn list_api_pairing_sessions(
        &self,
        include_consumed: bool,
    ) -> Result<Vec<ApiPairingSessionRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let sql = if include_consumed {
            "SELECT id, client_name, requested_scopes, approved_scopes, status,
                    created_at, expires_at, approved_token_id, token_delivered_at
             FROM api_pairing_sessions ORDER BY created_at DESC"
        } else {
            "SELECT id, client_name, requested_scopes, approved_scopes, status,
                    created_at, expires_at, approved_token_id, token_delivered_at
             FROM api_pairing_sessions
             WHERE status != 'consumed'
             ORDER BY created_at DESC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            let requested_json: String = row.get(2)?;
            let approved_json: Option<String> = row.get(3)?;
            Ok(ApiPairingSessionRecord {
                id: row.get(0)?,
                client_name: row.get(1)?,
                requested_scopes: serde_json::from_str(&requested_json).unwrap_or_default(),
                approved_scopes: approved_json
                    .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok()),
                status: row.get(4)?,
                created_at: row.get(5)?,
                expires_at: row.get(6)?,
                approved_token_id: row.get(7)?,
                token_delivered_at: row.get(8)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_api_pairing_session(
        &self,
        id: &str,
    ) -> Result<Option<ApiPairingSessionLookup>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT id, client_name, poll_secret_hash, requested_scopes, approved_scopes,
                    status, created_at, expires_at, approved_token_id, token_delivered_at
             FROM api_pairing_sessions WHERE id = ?1",
            params![id],
            |row| {
                let requested_json: String = row.get(3)?;
                let approved_json: Option<String> = row.get(4)?;
                Ok(ApiPairingSessionLookup {
                    poll_secret_hash: row.get(2)?,
                    record: ApiPairingSessionRecord {
                        id: row.get(0)?,
                        client_name: row.get(1)?,
                        requested_scopes: serde_json::from_str(&requested_json).unwrap_or_default(),
                        approved_scopes: approved_json
                            .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok()),
                        status: row.get(5)?,
                        created_at: row.get(6)?,
                        expires_at: row.get(7)?,
                        approved_token_id: row.get(8)?,
                        token_delivered_at: row.get(9)?,
                    },
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn approve_api_pairing_session(
        &self,
        pairing_id: &str,
        approved_scopes: &[String],
        token_id: &str,
        raw_token: &str,
    ) -> Result<bool, AppError> {
        let scopes_json = serde_json::to_string(approved_scopes).map_err(|e| {
            AppError::Database(format!("Failed to serialize approved pairing scopes: {e}"))
        })?;
        let conn = lock_conn!(self.conn);
        let changed = conn.execute(
            "UPDATE api_pairing_sessions
             SET status = 'approved', approved_scopes = ?1, approved_token_id = ?2,
                 approved_token_secret = ?3
             WHERE id = ?4 AND status = 'pending'",
            params![scopes_json, token_id, raw_token, pairing_id],
        )?;
        Ok(changed > 0)
    }

    pub fn get_approved_pairing_token_secret(
        &self,
        pairing_id: &str,
    ) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT approved_token_secret FROM api_pairing_sessions
             WHERE id = ?1 AND status = 'approved' AND token_delivered_at IS NULL",
            params![pairing_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn consume_approved_pairing_token(
        &self,
        pairing_id: &str,
    ) -> Result<Option<ConsumedPairingToken>, AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction()?;
        let token = tx
            .query_row(
                "SELECT approved_token_secret, approved_scopes FROM api_pairing_sessions
                 WHERE id = ?1 AND status = 'approved' AND token_delivered_at IS NULL",
                params![pairing_id],
                |row| {
                    let token: String = row.get(0)?;
                    let scopes_json: Option<String> = row.get(1)?;
                    let approved_scopes = scopes_json
                        .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
                        .unwrap_or_default();
                    Ok(ConsumedPairingToken {
                        token,
                        approved_scopes,
                    })
                },
            )
            .optional()?;

        if token.is_none() {
            tx.commit()?;
            return Ok(None);
        }

        let changed = tx.execute(
            "UPDATE api_pairing_sessions
             SET status = 'consumed', token_delivered_at = ?1, approved_token_secret = NULL
             WHERE id = ?2 AND status = 'approved' AND token_delivered_at IS NULL",
            params![now, pairing_id],
        )?;
        tx.commit()?;

        if changed == 1 {
            Ok(token)
        } else {
            Ok(None)
        }
    }

    pub fn reject_api_pairing_session(&self, pairing_id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let changed = conn.execute(
            "UPDATE api_pairing_sessions SET status = 'rejected'
             WHERE id = ?1 AND status = 'pending'",
            params![pairing_id],
        )?;
        Ok(changed > 0)
    }

    pub fn consume_api_pairing_session(&self, pairing_id: &str) -> Result<bool, AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        let conn = lock_conn!(self.conn);
        let changed = conn.execute(
            "UPDATE api_pairing_sessions
             SET status = 'consumed', token_delivered_at = ?1, approved_token_secret = NULL
             WHERE id = ?2 AND status = 'approved' AND token_delivered_at IS NULL",
            params![now, pairing_id],
        )?;
        Ok(changed > 0)
    }

    pub fn count_recent_api_pairing_sessions(&self, since_millis: i64) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT COUNT(*) FROM api_pairing_sessions WHERE created_at >= ?1",
            params![since_millis],
            |row| row.get(0),
        )
        .map_err(Into::into)
    }

    pub fn cleanup_expired_api_pairing_sessions(&self, now_millis: i64) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE api_pairing_sessions
             SET status = 'expired', approved_token_secret = NULL
             WHERE expires_at <= ?1 AND status IN ('pending', 'approved')",
            params![now_millis],
        )
        .map_err(Into::into)
    }

    pub fn insert_api_audit_log(
        &self,
        token_id: Option<&str>,
        scope: Option<&str>,
        method: &str,
        path: &str,
        status: u16,
        request_id: &str,
        remote_ip: Option<&str>,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO api_audit_logs
             (token_id, scope, method, path, status, request_id, remote_ip, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                token_id,
                scope,
                method,
                path,
                status as i64,
                request_id,
                remote_ip,
                now
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn management_api_tables_exist_on_memory_database() {
        let db = Database::memory().expect("memory db");
        let conn = db.conn.lock().expect("lock connection");

        for table in ["api_tokens", "api_pairing_sessions", "api_audit_logs"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![table],
                    |row| row.get(0),
                )
                .expect("query sqlite schema");
            assert_eq!(count, 1, "{table} should exist");
        }
    }

    #[test]
    fn token_lifecycle_tracks_active_lookup_touch_and_revoke() {
        let db = Database::memory().expect("memory db");
        let active = db
            .create_api_token(
                "active",
                "hash-active",
                "Active Token",
                &["api:read".to_string()],
                None,
                Some("test"),
            )
            .expect("create active token");
        let expired_at = chrono::Utc::now().timestamp_millis() - 1;
        db.create_api_token(
            "expired",
            "hash-expired",
            "Expired Token",
            &["api:read".to_string()],
            Some(expired_at),
            Some("test"),
        )
        .expect("create expired token");

        assert_eq!(db.active_api_token_count().expect("active count"), 1);
        let lookup = db
            .get_api_token_by_hash("hash-active")
            .expect("lookup active")
            .expect("active token");
        assert_eq!(lookup.token_hash, "hash-active");
        assert_eq!(lookup.record.id, active.id);
        assert_eq!(lookup.record.last_used_at, None);

        db.touch_api_token("active").expect("touch token");
        let touched = db
            .get_api_token_by_hash("hash-active")
            .expect("lookup touched")
            .expect("touched token");
        assert!(touched.record.last_used_at.is_some());

        assert!(db.revoke_api_token("active").expect("revoke active"));
        assert_eq!(
            db.active_api_token_count()
                .expect("active count after revoke"),
            0
        );
        assert!(!db
            .revoke_api_token("missing")
            .expect("missing revoke should be false"));
    }

    #[test]
    fn pairing_lifecycle_approves_delivers_once_and_hides_consumed_by_default() {
        let db = Database::memory().expect("memory db");
        let expires_at = chrono::Utc::now().timestamp_millis() + 60_000;
        let requested = vec!["api:read".to_string(), "providers:read".to_string()];
        let session = db
            .create_api_pairing_session("pairing-1", "client", "poll-hash", &requested, expires_at)
            .expect("create pairing");
        assert_eq!(session.status, "pending");

        let lookup = db
            .get_api_pairing_session("pairing-1")
            .expect("lookup pairing")
            .expect("pairing session");
        assert_eq!(lookup.poll_secret_hash, "poll-hash");
        assert_eq!(lookup.record.requested_scopes, requested);

        let approved = vec!["api:read".to_string()];
        assert!(db
            .approve_api_pairing_session("pairing-1", &approved, "token-1", "raw-token")
            .expect("approve pairing"));
        assert!(!db
            .reject_api_pairing_session("pairing-1")
            .expect("cannot reject approved session"));

        let secret = db
            .get_approved_pairing_token_secret("pairing-1")
            .expect("get approved token")
            .expect("approved token secret");
        assert_eq!(secret, "raw-token");

        let consumed = db
            .consume_approved_pairing_token("pairing-1")
            .expect("consume approved token")
            .expect("token should be delivered once");
        assert_eq!(consumed.token, "raw-token");
        assert_eq!(consumed.approved_scopes, approved);
        assert!(db
            .consume_approved_pairing_token("pairing-1")
            .expect("second consume should not fail")
            .is_none());

        assert!(db
            .list_api_pairing_sessions(false)
            .expect("list unconsumed sessions")
            .is_empty());
        assert_eq!(
            db.list_api_pairing_sessions(true)
                .expect("list all sessions")
                .len(),
            1
        );
    }

    #[test]
    fn pairing_cleanup_marks_expired_sessions_and_recent_count_tracks_created_at() {
        let db = Database::memory().expect("memory db");
        let now = chrono::Utc::now().timestamp_millis();
        db.create_api_pairing_session(
            "expired-pairing",
            "client",
            "poll-hash",
            &["api:read".to_string()],
            now - 1,
        )
        .expect("create expired pairing");
        db.create_api_pairing_session(
            "fresh-pairing",
            "client",
            "poll-hash-2",
            &["api:read".to_string()],
            now + 60_000,
        )
        .expect("create fresh pairing");

        assert_eq!(
            db.count_recent_api_pairing_sessions(now - 60_000)
                .expect("recent count"),
            2
        );
        assert_eq!(
            db.cleanup_expired_api_pairing_sessions(now)
                .expect("cleanup expired"),
            1
        );
        let expired = db
            .get_api_pairing_session("expired-pairing")
            .expect("lookup expired")
            .expect("expired session");
        assert_eq!(expired.record.status, "expired");
    }

    #[test]
    fn legacy_pairing_consume_remains_idempotent() {
        let db = Database::memory().expect("memory db");
        let expires_at = chrono::Utc::now().timestamp_millis() + 60_000;
        db.create_api_pairing_session(
            "pairing-1",
            "client",
            "poll-hash",
            &["api:read".to_string()],
            expires_at,
        )
        .expect("create pairing");
        assert!(db
            .approve_api_pairing_session(
                "pairing-1",
                &["api:read".to_string()],
                "token-1",
                "raw-token"
            )
            .expect("approve pairing"));
        assert!(db
            .consume_api_pairing_session("pairing-1")
            .expect("consume approved session"));
        assert_eq!(
            db.get_approved_pairing_token_secret("pairing-1")
                .expect("get consumed token"),
            None
        );
        assert!(db
            .list_api_pairing_sessions(false)
            .expect("list unconsumed sessions")
            .is_empty());
        assert_eq!(
            db.list_api_pairing_sessions(true)
                .expect("list all sessions")
                .len(),
            1
        );
    }

    #[test]
    fn audit_log_inserts_without_sensitive_payload_columns() {
        let db = Database::memory().expect("memory db");
        db.insert_api_audit_log(
            Some("token-1"),
            Some("api:read"),
            "GET",
            "/v1/me",
            200,
            "request-1",
            Some("127.0.0.1"),
        )
        .expect("insert audit log");

        let conn = db.conn.lock().expect("lock connection");
        let row: (String, String, String, i64, String) = conn
            .query_row(
                "SELECT token_id, scope, method, status, remote_ip FROM api_audit_logs WHERE request_id = ?1",
                params!["request-1"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .expect("query audit log");
        assert_eq!(
            row,
            (
                "token-1".to_string(),
                "api:read".to_string(),
                "GET".to_string(),
                200,
                "127.0.0.1".to_string(),
            )
        );
    }
}
