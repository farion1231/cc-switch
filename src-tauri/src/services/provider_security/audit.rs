use super::{credential_fingerprint, CredentialFields, CredentialSource};
use crate::app_config::AppType;
use crate::error::AppError;
use rusqlite::{params, Connection};
use serde_json::{Map, Value};

pub const AUDIT_MAX_AGE_DAYS: i64 = 90;
const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1_000;

fn fingerprint_json(fields: &CredentialFields) -> Result<String, AppError> {
    let mut values = Map::new();
    if let Some(api_key) = fields.api_key.as_deref() {
        values.insert(
            "api_key".to_string(),
            Value::String(credential_fingerprint("api_key", api_key)),
        );
    }
    if let Some(base_url) = fields.base_url.as_deref() {
        values.insert(
            "base_url".to_string(),
            Value::String(credential_fingerprint("base_url", base_url)),
        );
    }
    serde_json::to_string(&values).map_err(|source| AppError::JsonSerialize { source })
}

#[allow(clippy::too_many_arguments)]
pub fn record_credential_audit(
    conn: &Connection,
    request_id: &str,
    provider_id: &str,
    app_type: &AppType,
    source: CredentialSource,
    fields_changed: &[&str],
    old_fields: &CredentialFields,
    new_fields: &CredentialFields,
    _outcome: &str,
    created_at_ms: i64,
) -> Result<(), AppError> {
    let changed_json = serde_json::to_string(fields_changed)
        .map_err(|source| AppError::JsonSerialize { source })?;
    let old_json = fingerprint_json(old_fields)?;
    let new_json = fingerprint_json(new_fields)?;

    conn.execute(
        "INSERT INTO provider_credential_audit (
            id, provider_id, app_type, source, changed_fields,
            before_fingerprint, after_fingerprint, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            request_id,
            provider_id,
            app_type.as_str(),
            source.as_str(),
            changed_json,
            old_json,
            new_json,
            created_at_ms,
        ],
    )
    .map_err(|e| AppError::Database(format!("write provider credential audit: {e}")))?;
    Ok(())
}

pub fn prune_credential_audits(conn: &Connection, now_ms: i64) -> Result<usize, AppError> {
    let cutoff = now_ms.saturating_sub(AUDIT_MAX_AGE_DAYS * MILLIS_PER_DAY);
    conn.execute(
        "DELETE FROM provider_credential_audit WHERE created_at < ?1",
        params![cutoff],
    )
    .map_err(|e| AppError::Database(format!("prune provider credential audits: {e}")))
}

/// Delete expired/old rollback data, then retain only the newest configured
/// number of snapshots for each `(app_type, provider_id)` pair.
pub fn prune_snapshots(conn: &Connection, now_ms: i64) -> Result<usize, AppError> {
    let cutoff = now_ms.saturating_sub(super::ROLLBACK_MAX_AGE_DAYS * MILLIS_PER_DAY);
    let mut removed = conn
        .execute(
            "DELETE FROM provider_rollback_snapshots
             WHERE expires_at <= ?1 OR created_at < ?2",
            params![now_ms, cutoff],
        )
        .map_err(|e| AppError::Database(format!("prune expired provider snapshots: {e}")))?;

    // Correlated counts avoid relying on window-function support. A row is
    // removed when at least MAX rows in its group are strictly newer (id is the
    // deterministic tie-breaker), leaving exactly the newest MAX rows.
    removed += conn
        .execute(
            "DELETE FROM provider_rollback_snapshots
             WHERE rowid IN (
                 SELECT older.rowid
                 FROM provider_rollback_snapshots AS older
                 WHERE (
                     SELECT COUNT(*)
                     FROM provider_rollback_snapshots AS newer
                     WHERE newer.app_type = older.app_type
                       AND newer.provider_id = older.provider_id
                       AND (
                           newer.created_at > older.created_at
                           OR (newer.created_at = older.created_at AND newer.rowid > older.rowid)
                       )
                 ) >= ?1
             )",
            params![super::ROLLBACK_MAX_VERSIONS as i64],
        )
        .map_err(|e| AppError::Database(format!("limit provider rollback snapshots: {e}")))?;

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        Database::create_tables_on_conn(&conn).unwrap();
        conn
    }

    #[test]
    fn audit_rows_contain_fingerprints_but_no_raw_credentials() {
        let conn = setup();
        let old = CredentialFields {
            api_key: Some("sk-old-secret".to_string()),
            base_url: Some("https://old.example/v1".to_string()),
        };
        let new = CredentialFields {
            api_key: Some("sk-new-secret".to_string()),
            base_url: Some("https://new.example/v1".to_string()),
        };
        record_credential_audit(
            &conn,
            "req-1",
            "provider-1",
            &AppType::Codex,
            CredentialSource::ProviderEdit,
            &["api_key", "base_url"],
            &old,
            &new,
            "success",
            1_000,
        )
        .unwrap();

        let row: (String, String, String) = conn
            .query_row(
                "SELECT changed_fields, before_fingerprint, after_fingerprint
                 FROM provider_credential_audit WHERE id = 'req-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let stored = format!("{}{}{}", row.0, row.1, row.2);
        assert!(!stored.contains("sk-old-secret"));
        assert!(!stored.contains("sk-new-secret"));
        assert!(!stored.contains("old.example"));
        assert!(!stored.contains("new.example"));
        assert!(stored.contains("api_key"));
        assert!(stored.contains("base_url"));
    }

    #[test]
    fn audit_retention_deletes_rows_older_than_ninety_days() {
        let conn = setup();
        let now = 100 * MILLIS_PER_DAY;
        for (request_id, created_at) in [
            ("old", now - 91 * MILLIS_PER_DAY),
            ("boundary", now - 90 * MILLIS_PER_DAY),
            ("new", now),
        ] {
            conn.execute(
                "INSERT INTO provider_credential_audit (
                    id, provider_id, app_type, source, changed_fields,
                    before_fingerprint, after_fingerprint, created_at
                 ) VALUES (?1, 'p', 'codex', 'provider_edit', '[]', '{}', '{}', ?2)",
                params![request_id, created_at],
            )
            .unwrap();
        }
        assert_eq!(prune_credential_audits(&conn, now).unwrap(), 1);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM provider_credential_audit", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn snapshot_retention_is_scoped_per_provider_and_keeps_newest_ten() {
        let conn = setup();
        let now = 40 * MILLIS_PER_DAY;
        for provider in ["p1", "p2"] {
            for index in 0..12_i64 {
                conn.execute(
                    "INSERT INTO provider_rollback_snapshots (
                        provider_id, app_type, provider_json, source_revision,
                        created_at, expires_at
                     ) VALUES (?1, 'codex', '{}', ?2, ?3, ?4)",
                    params![provider, index, now - index * 1_000, now + MILLIS_PER_DAY],
                )
                .unwrap();
            }
        }
        // A third group proves old/expired pruning is independent of count pruning.
        conn.execute(
            "INSERT INTO provider_rollback_snapshots (
                provider_id, app_type, provider_json, source_revision, created_at, expires_at
             ) VALUES ('old', 'codex', '{}', 1, ?1, ?2)",
            params![now - 31 * MILLIS_PER_DAY, now + MILLIS_PER_DAY],
        )
        .unwrap();

        assert_eq!(prune_snapshots(&conn, now).unwrap(), 5);
        for provider in ["p1", "p2"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM provider_rollback_snapshots
                     WHERE app_type = 'codex' AND provider_id = ?1",
                    params![provider],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 10);
        }
        let old_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM provider_rollback_snapshots WHERE provider_id = 'old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_count, 0);
    }
}
