use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};

use crate::codex_config::{extract_codex_model_provider, get_codex_config_dir};
use crate::error::AppError;
use crate::provider::Provider;

const DEFAULT_CODEX_PROVIDER_KEY: &str = "openai";

pub fn sync_threads_to_current_provider(provider: &Provider) -> Result<usize, AppError> {
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(|value| value.as_str())
        .unwrap_or("");

    let target_provider = extract_codex_model_provider(config_text)
        .unwrap_or_else(|| DEFAULT_CODEX_PROVIDER_KEY.to_string());

    sync_threads_to_provider_at_path(&get_codex_config_dir().join("state_5.sqlite"), &target_provider)
}

fn sync_threads_to_provider_at_path(db_path: &Path, target_provider: &str) -> Result<usize, AppError> {
    if !db_path.exists() {
        return Ok(0);
    }

    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI;
    let conn = Connection::open_with_flags(db_path, flags)
        .map_err(|e| AppError::Database(format!("打开 Codex state_5.sqlite 失败: {e}")))?;
    let _ = conn.busy_timeout(Duration::from_secs(3));

    let updated = conn
        .execute(
            "UPDATE threads SET model_provider = ?1 WHERE COALESCE(model_provider, '') <> ?1",
            params![target_provider],
        )
        .map_err(|e| AppError::Database(format!("更新 Codex threads.model_provider 失败: {e}")))?;

    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn sync_threads_to_provider_updates_all_rows() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state_5.sqlite");
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute("CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT)", [])
            .expect("create table");
        conn.execute(
            "INSERT INTO threads (id, model_provider) VALUES ('a', 'openai'), ('b', 'OpenAI'), ('c', 'sub2api')",
            [],
        )
        .expect("seed rows");
        drop(conn);

        let updated = sync_threads_to_provider_at_path(&db_path, "OpenAI").expect("sync");
        assert_eq!(updated, 2);

        let conn = Connection::open(&db_path).expect("reopen db");
        let values: Vec<String> = conn
            .prepare("SELECT model_provider FROM threads ORDER BY id")
            .expect("prepare")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");
        assert_eq!(values, vec!["OpenAI", "OpenAI", "OpenAI"]);
    }

    #[test]
    fn sync_threads_to_current_provider_defaults_to_openai_without_model_provider() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state_5.sqlite");
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute("CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT)", [])
            .expect("create table");
        conn.execute(
            "INSERT INTO threads (id, model_provider) VALUES ('a', 'OpenAI')",
            [],
        )
        .expect("seed row");
        drop(conn);

        let provider = Provider::with_id(
            "codex-official".to_string(),
            "Official".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-test" },
                "config": "model = \"gpt-5.4\"\n"
            }),
            None,
        );

        let config_text = provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let target_provider = extract_codex_model_provider(config_text)
            .unwrap_or_else(|| DEFAULT_CODEX_PROVIDER_KEY.to_string());
        assert_eq!(target_provider, "openai");

        let updated = sync_threads_to_provider_at_path(&db_path, &target_provider).expect("sync");
        assert_eq!(updated, 1);
    }
}
