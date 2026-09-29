//! Read OpenCode v2 credentials without modifying its database or requiring the CLI.

use crate::error::AppError;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

pub(super) fn read_providers(path: &Path) -> Result<Map<String, Value>, AppError> {
    if !path.try_exists().map_err(|err| AppError::io(path, err))? {
        return Ok(Map::new());
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_millis(250))?;
    let has_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'credential')",
        [],
        |row| row.get(0),
    )?;
    if !has_table {
        // v1 databases do not have this table.
        return Ok(Map::new());
    }

    // Match OpenCode's effective credential ordering. Select once per integration,
    // BEFORE decoding: an active OAuth/invalid value must not revive an older key.
    let mut statement = conn.prepare(
        "SELECT integration_id, label, value, connector_id
         FROM credential WHERE integration_id IS NOT NULL
         ORDER BY active DESC, time_created DESC, id DESC",
    )?;
    let mut rows = statement.query([])?;
    let mut seen = HashSet::new();
    let mut providers = Map::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        if id.trim().is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let connector: Option<String> = row.get(3)?;
        // The Console integration can supply multiple remote providers. Its key
        // is not a model API key for provider.opencode. Connectors likewise need
        // their own authentication flow.
        if connector.is_some() || id == "opencode" {
            continue;
        }
        let raw: String = row.get(2)?;
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            log::warn!("Skipping malformed OpenCode credential for '{id}'");
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("key") {
            continue;
        }
        let Some(key) = value.get("key").and_then(Value::as_str) else {
            continue;
        };
        if key.trim().is_empty() {
            continue;
        }
        // Form answers have integration-specific meaning (e.g. cloud deployment
        // and tenant settings); copying them as SDK options is not a safe mapping.
        if value.get("configuration").is_some_and(|config| {
            !config.is_null() && !config.as_object().is_some_and(Map::is_empty)
        }) {
            log::warn!(
                "Skipping OpenCode credential for '{id}' with integration-specific settings"
            );
            continue;
        }
        let label: Option<String> = row.get(1)?;
        let name = label
            .as_deref()
            .filter(|label| !label.trim().is_empty() && *label != "default")
            .unwrap_or(if id == "opencode-go" {
                "OpenCode Go"
            } else {
                &id
            });
        // Both v1 and v2 accept a built-in provider override without npm/models.
        // Keep its ID so OpenCode supplies the correct endpoints and model catalog.
        providers.insert(
            id.clone(),
            json!({ "name": name, "options": { "apiKey": key } }),
        );
    }
    Ok(providers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE credential (
                id TEXT PRIMARY KEY, integration_id TEXT, label TEXT, value TEXT,
                connector_id TEXT, active INTEGER, time_created INTEGER
            );",
        )
        .unwrap();
        conn
    }

    fn insert(
        conn: &Connection,
        id: &str,
        integration: &str,
        value: &str,
        active: bool,
        time: i64,
    ) {
        conn.execute(
            "INSERT INTO credential VALUES (?1, ?2, 'default', ?3, NULL, ?4, ?5)",
            rusqlite::params![id, integration, value, active, time],
        )
        .unwrap();
    }

    #[test]
    fn missing_and_v1_databases_are_ignored_without_creation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("opencode.db");
        assert!(read_providers(&path).unwrap().is_empty());
        assert!(!path.exists());
        Connection::open(&path).unwrap();
        assert!(read_providers(&path).unwrap().is_empty());
    }

    #[test]
    fn imports_effective_key_per_integration_from_wal_without_mutating_database() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("opencode.db");
        let conn = database(&path);
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        insert(
            &conn,
            "a",
            "opencode-go",
            r#"{"type":"key","key":"active-test-key"}"#,
            true,
            1,
        );
        insert(
            &conn,
            "b",
            "opencode-go",
            r#"{"type":"key","key":"inactive-test-key"}"#,
            false,
            9,
        );
        insert(
            &conn,
            "c",
            "openai",
            r#"{"type":"key","key":"older-test-key"}"#,
            true,
            1,
        );
        insert(
            &conn,
            "d",
            "openai",
            r#"{"type":"key","key":"newer-test-key"}"#,
            true,
            2,
        );
        insert(
            &conn,
            "e",
            "openai",
            r#"{"type":"key","key":"tie-test-key"}"#,
            true,
            2,
        );
        // OpenCode also uses the newest credential when none is marked active.
        insert(
            &conn,
            "f",
            "anthropic",
            r#"{"type":"key","key":"fallback-test-key"}"#,
            false,
            3,
        );
        let before = std::fs::read(&path).unwrap();
        let wal_path = path.with_extension("db-wal");
        let wal_before = std::fs::read(&wal_path).unwrap();

        let providers = read_providers(&path).unwrap();
        assert_eq!(providers.len(), 3);
        assert_eq!(
            providers["opencode-go"],
            json!({"name":"OpenCode Go","options":{"apiKey":"active-test-key"}})
        );
        assert_eq!(providers["openai"]["options"]["apiKey"], "tie-test-key");
        assert_eq!(
            providers["anthropic"]["options"]["apiKey"],
            "fallback-test-key"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read(&wal_path).unwrap(), wal_before);
    }

    #[test]
    fn unsupported_or_invalid_current_values_do_not_revive_old_keys() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("opencode.db");
        let conn = database(&path);
        for (i, value) in [
            r#"{"type":"oauth","access":"test-access","refresh":"test-refresh"}"#,
            "not-json",
            r#"{"type":"key","key":"  "}"#,
            r#"{"type":"key","key":42}"#,
            r#"{"type":"key","key":"test-key","configuration":{"tenant":"example"}}"#,
            r#"{"type":"unknown","key":"test-key"}"#,
        ]
        .iter()
        .enumerate()
        {
            let integration = format!("provider-{i}");
            insert(
                &conn,
                &format!("old-{i}"),
                &integration,
                r#"{"type":"key","key":"old-test-key"}"#,
                false,
                1,
            );
            insert(&conn, &format!("new-{i}"), &integration, value, true, 2);
        }
        insert(
            &conn,
            "empty",
            " ",
            r#"{"type":"key","key":"test-key"}"#,
            true,
            1,
        );
        insert(
            &conn,
            "console",
            "opencode",
            r#"{"type":"key","key":"test-key"}"#,
            true,
            1,
        );
        insert(
            &conn,
            "connector",
            "remote",
            r#"{"type":"key","key":"test-key"}"#,
            true,
            1,
        );
        conn.execute(
            "UPDATE credential SET connector_id = ?1 WHERE id = ?2",
            ["external", "connector"],
        )
        .unwrap();
        assert!(read_providers(&path).unwrap().is_empty());
    }
}
