use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CredentialType {
    ApiKey,
    Oauth,
    Unknown,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OmpLogin {
    provider: String,
    credential_types: Vec<CredentialType>,
    total: u32,
    active: u32,
}

pub(crate) fn list_logins() -> Vec<OmpLogin> {
    let Ok(dir) = crate::omp_config::get_omp_agent_dir() else {
        log::warn!("Could not resolve Oh My Pi agent directory for logins");
        return Vec::new();
    };
    let path = dir.join("agent.db");
    match path.try_exists() {
        Ok(false) => return Vec::new(),
        Err(_) => {
            log::warn!("Could not access Oh My Pi login database");
            return Vec::new();
        }
        Ok(true) => {}
    }
    read_logins(&path).unwrap_or_else(|_| {
        // SQLite errors may contain database content; do not log their text.
        log::warn!("Could not read Oh My Pi login database");
        Vec::new()
    })
}

fn open_read_only(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if path.with_extension("db-wal").exists() {
        return Connection::open_with_flags(path, flags);
    }
    // omp's clean exit checkpoints and removes -wal/-shm, so every commit is in the main
    // file. A plain read-only open of a WAL database would recreate those sidecars.
    Connection::open_with_flags(
        format!("{}?immutable=1", file_uri(path)),
        flags | OpenFlags::SQLITE_OPEN_URI,
    )
}

fn file_uri(path: &std::path::Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from(if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

fn read_logins(path: &std::path::Path) -> rusqlite::Result<Vec<OmpLogin>> {
    let conn = open_read_only(path)?;
    conn.busy_timeout(Duration::from_millis(100))?;
    let mut stmt = match conn.prepare(
        "SELECT provider, credential_type, disabled_cause FROM auth_credentials ORDER BY provider, credential_type",
    ) {
        Ok(stmt) => stmt,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message == "no such table: auth_credentials" =>
        {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    let mut rows = stmt.query([])?;
    let mut logins = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let provider: String = row.get(0)?;
        let credential_type = match row.get::<_, String>(1)?.as_str() {
            "api_key" => CredentialType::ApiKey,
            "oauth" => CredentialType::Oauth,
            _ => CredentialType::Unknown,
        };
        let disabled: Option<String> = row.get(2)?;
        let login = logins.entry(provider.clone()).or_insert_with(|| OmpLogin {
            provider,
            credential_types: Vec::new(),
            total: 0,
            active: 0,
        });
        if !login.credential_types.contains(&credential_type) {
            login.credential_types.push(credential_type);
        }
        login.total += 1;
        login.active += u32::from(disabled.is_none());
    }
    Ok(logins.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omp_config::test_support::TestAgentDir;
    use serial_test::serial;
    use std::fs;

    #[test]
    #[serial]
    fn groups_logins_without_changing_database_bytes() {
        let _agent = TestAgentDir::new();
        let path = crate::omp_config::get_omp_agent_dir()
            .unwrap()
            .join("agent.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE auth_credentials (
                 provider TEXT NOT NULL, credential_type TEXT NOT NULL,
                 data TEXT NOT NULL, disabled_cause TEXT
             );
             INSERT INTO auth_credentials VALUES
                 ('openai-codex', 'oauth', 'secret', NULL),
                 ('opencode-go', 'api_key', 'secret', 'expired'),
                 ('openai-codex', 'oauth', 'secret', ''),
                 ('openai-codex', 'api_key', 'secret', NULL),
                 ('opencode-go', 'future', 'secret', 'disabled'),
                 ('opencode-go', 'other', 'secret', 'disabled');",
        )
        .unwrap();
        let before = fs::read(&path).unwrap();
        let wal_path = path.with_extension("db-wal");
        let wal_before = fs::read(&wal_path).unwrap();
        let logins = list_logins();
        assert_eq!(
            serde_json::to_value(logins).unwrap(),
            serde_json::json!([
                { "provider": "openai-codex", "credentialTypes": ["apiKey", "oauth"], "total": 3, "active": 2 },
                { "provider": "opencode-go", "credentialTypes": ["apiKey", "unknown"], "total": 3, "active": 0 }
            ])
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read(wal_path).unwrap(), wal_before);
    }

    #[test]
    #[serial]
    fn missing_file_is_not_created() {
        let _agent = TestAgentDir::new();
        let path = crate::omp_config::get_omp_agent_dir()
            .unwrap()
            .join("agent.db");
        assert!(list_logins().is_empty());
        assert!(!path.exists());
    }

    #[test]
    #[serial]
    fn missing_table_returns_empty_without_changes() {
        let _agent = TestAgentDir::new();
        let path = crate::omp_config::get_omp_agent_dir()
            .unwrap()
            .join("agent.db");
        Connection::open(&path).unwrap();
        let before = fs::read(&path).unwrap();
        assert!(list_logins().is_empty());
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[test]
    #[serial]
    fn wal_database_without_sidecars_is_read_without_creating_them() {
        let _agent = TestAgentDir::new();
        let path = crate::omp_config::get_omp_agent_dir()
            .unwrap()
            .join("agent.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE auth_credentials (
                 provider TEXT NOT NULL, credential_type TEXT NOT NULL,
                 data TEXT NOT NULL, disabled_cause TEXT
             );
             INSERT INTO auth_credentials VALUES ('opencode-go', 'api_key', 'secret', NULL);",
        )
        .unwrap();
        // Closing the last connection checkpoints and removes -wal/-shm, as omp's exit does.
        drop(conn);
        assert!(!path.with_extension("db-wal").exists());
        let before = fs::read(&path).unwrap();
        assert_eq!(list_logins().len(), 1);
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!path.with_extension("db-wal").exists());
        assert!(!path.with_extension("db-shm").exists());
    }

    #[test]
    #[serial]
    fn corrupt_database_returns_empty_without_changes() {
        let _agent = TestAgentDir::new();
        let path = crate::omp_config::get_omp_agent_dir()
            .unwrap()
            .join("agent.db");
        fs::write(&path, b"not a database").unwrap();
        assert!(list_logins().is_empty());
        assert_eq!(fs::read(path).unwrap(), b"not a database");
    }
}
