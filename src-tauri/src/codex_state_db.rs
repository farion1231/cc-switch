//! Locating Codex's per-thread state SQLite databases.
//!
//! Codex stores thread metadata in `state_5.sqlite`, normally inside the Codex
//! config dir (`CODEX_HOME` / `~/.codex`). The SQLite location can be moved with
//! the `sqlite_home` key in `config.toml` or the `CODEX_SQLITE_HOME` env var;
//! when set, a second DB lives there. Both history migration and the session
//! list's title lookup need the same resolution, so it lives here once.

use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use crate::config::get_home_dir;
use crate::error::AppError;

/// Filename of Codex's per-thread state database. Codex bumps the version
/// number across releases; update this single source of truth when a new state
/// DB version ships.
pub(crate) const CODEX_STATE_DB_FILENAME: &str = "state_5.sqlite";

/// Env var that overrides the Codex SQLite state directory.
const CODEX_SQLITE_HOME_ENV: &str = "CODEX_SQLITE_HOME";

/// Resolve every candidate `state_5.sqlite` path: the config-dir DB plus, when
/// Codex is configured to keep its SQLite state elsewhere, that DB too.
///
/// `config_dir` is the Codex config dir (`~/.codex`); `config_text` is the raw
/// `config.toml` contents, used to detect a `sqlite_home` override.
pub(crate) fn codex_state_db_paths(config_dir: &Path, config_text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    push_unique_path(&mut paths, config_dir.join(CODEX_STATE_DB_FILENAME));
    // Codex lets SQLite state move away from CODEX_HOME; config takes precedence.
    if let Some(sqlite_home) = sqlite_home_from_codex_config(config_text) {
        push_unique_path(&mut paths, sqlite_home.join(CODEX_STATE_DB_FILENAME));
    } else if let Some(sqlite_home) = sqlite_home_from_env() {
        push_unique_path(&mut paths, sqlite_home.join(CODEX_STATE_DB_FILENAME));
    }
    paths
}

/// Point historical Codex threads at the provider now selected in config.toml.
///
/// Codex stores the provider ID on each thread. If a provider switch removes
/// that definition from config.toml, old threads fail to load even though their
/// JSONL history is intact. Keep the state DB and live config consistent after
/// the live write succeeded. A missing top-level `model_provider` means Codex's
/// built-in `openai` provider.
///
/// If config.toml has no top-level `model`, preserve each thread's existing
/// model instead of clearing it.
pub(crate) fn sync_codex_state_db_thread_provider(
    config_dir: &Path,
    config_text: &str,
) -> Result<usize, AppError> {
    let doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Config(format!("解析 Codex config.toml 失败: {e}")))?;
    let provider = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .unwrap_or("openai")
        .to_string();
    let model = doc
        .get("model")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);

    let mut changed = 0;
    for db_path in codex_state_db_paths(config_dir, config_text) {
        if !db_path.exists() {
            continue;
        }
        changed += sync_one_codex_state_db_thread_provider(&db_path, &provider, model.as_deref())?;
    }
    Ok(changed)
}

fn sync_one_codex_state_db_thread_provider(
    db_path: &Path,
    provider: &str,
    model: Option<&str>,
) -> Result<usize, AppError> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| {
        AppError::Database(format!(
            "打开 Codex state DB 失败 {}: {e}",
            db_path.display()
        ))
    })?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| AppError::Database(format!("设置 Codex state DB busy_timeout 失败: {e}")))?;

    if !crate::database::Database::table_exists(&conn, "threads")?
        || !crate::database::Database::has_column(&conn, "threads", "model_provider")?
    {
        return Ok(0);
    }

    let has_model_column = crate::database::Database::has_column(&conn, "threads", "model")?;
    let changed = if has_model_column {
        let sql = "UPDATE threads
            SET model_provider = ?, model = COALESCE(?, model)
            WHERE model_provider IS NOT ?
               OR model IS NOT COALESCE(?, model)";
        let changed = conn
            .execute(sql, rusqlite::params![provider, model, provider, model,])
            .map_err(|e| {
                AppError::Database(format!(
                    "同步 Codex 历史会话 provider 失败 {}: {e}",
                    db_path.display()
                ))
            })?;
        changed
    } else {
        let sql = "UPDATE threads SET model_provider = ? WHERE model_provider IS NOT ?";
        let changed = conn
            .execute(sql, rusqlite::params![provider, provider])
            .map_err(|e| {
                AppError::Database(format!(
                    "同步 Codex 历史会话 provider 失败 {}: {e}",
                    db_path.display()
                ))
            })?;
        changed
    };
    Ok(changed)
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn sqlite_home_from_codex_config(config_text: &str) -> Option<PathBuf> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let raw = doc.get("sqlite_home")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn sqlite_home_from_env() -> Option<PathBuf> {
    let raw = std::env::var(CODEX_SQLITE_HOME_ENV).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn resolve_user_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return get_home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return get_home_dir().join(rest);
    }
    if let Some(rest) = raw.strip_prefix("~\\") {
        return get_home_dir().join(rest);
    }
    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn includes_config_sqlite_home() {
        let temp = tempdir().expect("tempdir");
        let sqlite_home = temp.path().join("sqlite-home");
        // 用 TOML 字面量字符串(单引号)承载路径：Windows 路径含反斜杠，basic string(双引号)
        // 会把 `\U`/`\s` 等当作非法转义导致解析失败。
        let config_text = format!("sqlite_home = '{}'\n", sqlite_home.display());

        let paths = codex_state_db_paths(temp.path(), &config_text);

        assert_eq!(
            paths,
            vec![
                temp.path().join(CODEX_STATE_DB_FILENAME),
                sqlite_home.join(CODEX_STATE_DB_FILENAME),
            ]
        );
    }

    #[test]
    fn syncs_thread_provider_and_model_across_all_state_dbs() {
        let temp = tempdir().expect("tempdir");
        let codex_dir = temp.path().join("codex");
        let sqlite_home = temp.path().join("sqlite-home");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::create_dir_all(&sqlite_home).expect("create sqlite home");
        let config_text = format!(
            "model = \"gpt-test\"\nsqlite_home = '{}'\nmodel_provider = \"custom\"\n",
            sqlite_home.display()
        );

        for db_dir in [&codex_dir, sqlite_home.as_path()] {
            let db_path = db_dir.join(CODEX_STATE_DB_FILENAME);
            let conn = rusqlite::Connection::open(&db_path).expect("create state db");
            conn.execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    model_provider TEXT NOT NULL,
                    model TEXT
                );
                INSERT INTO threads (id, model_provider, model)
                VALUES ('old', 'openai', 'gpt-old'), ('current', 'custom', NULL);",
            )
            .expect("seed state db");
        }

        let changed =
            sync_codex_state_db_thread_provider(&codex_dir, &config_text).expect("sync state dbs");

        assert_eq!(changed, 4);
        for db_dir in [&codex_dir, sqlite_home.as_path()] {
            let db_path = db_dir.join(CODEX_STATE_DB_FILENAME);
            let conn = rusqlite::Connection::open(&db_path).expect("reopen state db");
            let providers: Vec<String> = conn
                .prepare("SELECT model_provider FROM threads ORDER BY id")
                .expect("select providers")
                .query_map([], |row| row.get(0))
                .expect("query providers")
                .map(|row| row.expect("provider row"))
                .collect();
            let models: Vec<Option<String>> = conn
                .prepare("SELECT model FROM threads ORDER BY id")
                .expect("select models")
                .query_map([], |row| row.get(0))
                .expect("query models")
                .map(|row| row.expect("model row"))
                .collect();
            assert_eq!(providers, vec!["custom".to_string(), "custom".to_string()]);
            assert_eq!(
                models,
                vec![Some("gpt-test".to_string()), Some("gpt-test".to_string())]
            );
        }
    }

    #[test]
    fn official_mode_defaults_to_openai_and_preserves_missing_model() {
        let temp = tempdir().expect("tempdir");
        let codex_dir = temp.path().join("codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        let db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = rusqlite::Connection::open(&db_path).expect("create state db");
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, model TEXT);
             INSERT INTO threads VALUES ('thread', 'custom', 'third-party-model');",
        )
        .expect("seed state db");
        drop(conn);

        let changed =
            sync_codex_state_db_thread_provider(&codex_dir, "").expect("sync official state db");

        assert_eq!(changed, 1);
        let conn = rusqlite::Connection::open(&db_path).expect("reopen state db");
        let (provider, model): (String, Option<String>) = conn
            .query_row("SELECT model_provider, model FROM threads", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .expect("read synced thread");
        assert_eq!(provider, "openai");
        assert_eq!(model, Some("third-party-model".to_string()));
    }
}
