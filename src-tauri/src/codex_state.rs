use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codex_config::{effective_codex_model_provider_id_from_config, get_codex_config_dir};
use crate::config::read_json_file;
use crate::error::AppError;

const CODEX_STATE_DB: &str = "state_5.sqlite";
const THREADS_TABLE: &str = "threads";
const MODEL_PROVIDER_COLUMN: &str = "model_provider";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexModelProviderCount {
    pub model_provider: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexStateDiagnosis {
    pub config_model_provider: Option<String>,
    pub effective_model_provider: String,
    pub auth_mode: String,
    pub state_db_path: Option<String>,
    pub provider_counts: Vec<CodexModelProviderCount>,
    pub config_auth_mismatch: bool,
    pub index_mismatch: bool,
    pub inconsistent: bool,
    pub repairable_rows: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexStateRepairResult {
    pub dry_run: bool,
    pub target_model_provider: String,
    pub affected_rows: i64,
    pub backup_path: Option<String>,
    pub diagnosis_before: CodexStateDiagnosis,
    pub diagnosis_after: Option<CodexStateDiagnosis>,
}

pub fn diagnose_codex_state() -> Result<CodexStateDiagnosis, AppError> {
    diagnose_codex_state_at(get_codex_config_dir())
}

pub fn repair_codex_state(dry_run: bool) -> Result<CodexStateRepairResult, AppError> {
    repair_codex_state_at(get_codex_config_dir(), dry_run)
}

fn diagnose_codex_state_at(codex_dir: PathBuf) -> Result<CodexStateDiagnosis, AppError> {
    let config_text = read_codex_config_text_at(&codex_dir).unwrap_or_default();
    diagnose_codex_state_with_config(codex_dir, &config_text)
}

fn diagnose_codex_state_with_config(
    codex_dir: PathBuf,
    config_text: &str,
) -> Result<CodexStateDiagnosis, AppError> {
    let config_model_provider = parse_config_model_provider(config_text);
    let effective_model_provider = effective_codex_model_provider_id_from_config(config_text);
    let auth_mode = detect_codex_auth_mode_at(&codex_dir);
    let state_db_path = codex_dir.join(CODEX_STATE_DB);
    let provider_counts = read_provider_counts(&state_db_path)?;
    let repairable_rows = provider_counts
        .iter()
        .filter(|row| row.model_provider != effective_model_provider)
        .map(|row| row.count)
        .sum();

    let config_auth_mismatch = match auth_mode.as_str() {
        "apikey" => effective_model_provider == "openai",
        "chatgpt" | "empty" | "missing" => effective_model_provider != "openai",
        _ => false,
    };
    let index_mismatch = repairable_rows > 0;

    Ok(CodexStateDiagnosis {
        config_model_provider,
        effective_model_provider,
        auth_mode,
        state_db_path: state_db_path
            .exists()
            .then(|| state_db_path.to_string_lossy().to_string()),
        provider_counts,
        config_auth_mismatch,
        index_mismatch,
        inconsistent: config_auth_mismatch || index_mismatch,
        repairable_rows,
    })
}

fn repair_codex_state_at(
    codex_dir: PathBuf,
    dry_run: bool,
) -> Result<CodexStateRepairResult, AppError> {
    let config_text = read_codex_config_text_at(&codex_dir).unwrap_or_default();
    let diagnosis_before = diagnose_codex_state_with_config(codex_dir.clone(), &config_text)?;
    let target_model_provider = diagnosis_before.effective_model_provider.clone();
    let affected_rows = diagnosis_before.repairable_rows;

    if dry_run || affected_rows == 0 {
        return Ok(CodexStateRepairResult {
            dry_run,
            target_model_provider,
            affected_rows,
            backup_path: None,
            diagnosis_before,
            diagnosis_after: None,
        });
    }

    let state_db_path = codex_dir.join(CODEX_STATE_DB);
    if !state_db_path.exists() {
        return Err(AppError::Config(format!(
            "Codex state database not found: {}",
            state_db_path.display()
        )));
    }

    let mut conn = open_state_db(&state_db_path, false)?;
    ensure_threads_table(&conn)?;
    let backup_path = backup_state_db(&conn, &state_db_path)?;

    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(e.to_string()))?;
    let changed = tx
        .execute(
            "UPDATE threads SET model_provider = ?1 WHERE model_provider <> ?1",
            params![target_model_provider],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
    tx.commit().map_err(|e| AppError::Database(e.to_string()))?;

    let diagnosis_after = diagnose_codex_state_with_config(codex_dir, &config_text)?;

    Ok(CodexStateRepairResult {
        dry_run,
        target_model_provider,
        affected_rows: changed as i64,
        backup_path: Some(backup_path.to_string_lossy().to_string()),
        diagnosis_before,
        diagnosis_after: Some(diagnosis_after),
    })
}

fn read_codex_config_text_at(codex_dir: &Path) -> Result<String, AppError> {
    let config_path = codex_dir.join("config.toml");
    if config_path.exists() {
        std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))
    } else {
        Ok(String::new())
    }
}

fn detect_codex_auth_mode_at(codex_dir: &Path) -> String {
    let auth_path = codex_dir.join("auth.json");
    let auth = match read_json_file::<Value>(&auth_path) {
        Ok(value) => value,
        Err(_) if !auth_path.exists() => return "missing".to_string(),
        Err(_) => return "unknown".to_string(),
    };

    let Some(obj) = auth.as_object() else {
        return "unknown".to_string();
    };

    if obj
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        return "apikey".to_string();
    }

    if obj.is_empty() {
        return "empty".to_string();
    }

    "chatgpt".to_string()
}

fn parse_config_model_provider(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<toml_edit::DocumentMut>().ok()?;
    doc.get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn read_provider_counts(db_path: &Path) -> Result<Vec<CodexModelProviderCount>, AppError> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let conn = open_state_db(db_path, true)?;
    if !threads_table_exists(&conn)? {
        return Ok(Vec::new());
    }

    let mut stmt = conn
        .prepare(
            "SELECT model_provider, COUNT(*) FROM threads GROUP BY model_provider ORDER BY COUNT(*) DESC, model_provider ASC",
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CodexModelProviderCount {
                model_provider: row.get(0)?,
                count: row.get(1)?,
            })
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut counts = Vec::new();
    for row in rows {
        counts.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(counts)
}

fn open_state_db(db_path: &Path, read_only: bool) -> Result<Connection, AppError> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    Connection::open_with_flags(db_path, flags).map_err(|e| AppError::Database(e.to_string()))
}

fn threads_table_exists(conn: &Connection) -> Result<bool, AppError> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![THREADS_TABLE],
            |row| row.get(0),
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(exists > 0)
}

fn ensure_threads_table(conn: &Connection) -> Result<(), AppError> {
    if !threads_table_exists(conn)? {
        return Err(AppError::Database(
            "Codex state database has no threads table".to_string(),
        ));
    }

    let has_column = conn
        .prepare("PRAGMA table_info(threads)")
        .and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            for row in rows {
                if row? == MODEL_PROVIDER_COLUMN {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    if !has_column {
        return Err(AppError::Database(
            "Codex state threads table has no model_provider column".to_string(),
        ));
    }

    Ok(())
}

fn backup_state_db(conn: &Connection, db_path: &Path) -> Result<PathBuf, AppError> {
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S%3f");
    let backup_path = db_path.with_file_name(format!("{CODEX_STATE_DB}.backup-{ts}"));
    let backup_str = backup_path.to_string_lossy().to_string();

    conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
        .map_err(|e| AppError::Database(e.to_string()))?;
    conn.execute("VACUUM main INTO ?1", params![backup_str])
        .map_err(|e| AppError::Database(format!("Failed to backup Codex state database: {e}")))?;

    Ok(backup_path)
}

#[allow(dead_code)]
fn counts_to_map(counts: &[CodexModelProviderCount]) -> BTreeMap<String, i64> {
    counts
        .iter()
        .map(|row| (row.model_provider.clone(), row.count))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct EnvGuard {
        old_home: Option<std::ffi::OsString>,
        old_test_home: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_home(path: &Path) -> Self {
            let guard = Self {
                old_home: std::env::var_os("HOME"),
                old_test_home: std::env::var_os("CC_SWITCH_TEST_HOME"),
            };
            std::env::set_var("HOME", path);
            std::env::set_var("CC_SWITCH_TEST_HOME", path);
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.old_home.take() {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
            match self.old_test_home.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn create_state_db(path: &Path) {
        let conn = Connection::open(path).expect("open test state db");
        conn.execute_batch(
            r#"
            CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT NOT NULL
            );
            INSERT INTO threads (id, model_provider) VALUES
                ('a', 'bold_ai_api'),
                ('b', 'bold_ai_api'),
                ('c', 'openai');
            "#,
        )
        .expect("create threads");
    }

    #[test]
    fn repair_codex_state_dry_run_does_not_modify_threads() {
        let temp = tempdir().expect("tempdir");
        let _guard = EnvGuard::set_home(temp.path());
        let codex_dir = temp.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).expect("create codex dir");
        std::fs::write(codex_dir.join("auth.json"), "{}").expect("write auth");
        std::fs::write(codex_dir.join("config.toml"), "").expect("write config");
        create_state_db(&codex_dir.join(CODEX_STATE_DB));

        let result = repair_codex_state_at(codex_dir.clone(), true).expect("dry run");
        assert_eq!(result.affected_rows, 2);
        assert!(result.backup_path.is_none());

        let counts = read_provider_counts(&codex_dir.join(CODEX_STATE_DB)).expect("counts");
        let map = counts_to_map(&counts);
        assert_eq!(map.get("bold_ai_api"), Some(&2));
        assert_eq!(map.get("openai"), Some(&1));
    }

    #[test]
    fn repair_codex_state_updates_threads_and_creates_backup() {
        let temp = tempdir().expect("tempdir");
        let _guard = EnvGuard::set_home(temp.path());
        let codex_dir = temp.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).expect("create codex dir");
        std::fs::write(codex_dir.join("auth.json"), "{}").expect("write auth");
        std::fs::write(codex_dir.join("config.toml"), "").expect("write config");
        create_state_db(&codex_dir.join(CODEX_STATE_DB));

        let result = repair_codex_state_at(codex_dir.clone(), false).expect("repair");
        assert_eq!(result.affected_rows, 2);
        let backup_path = result.backup_path.expect("backup path");
        assert!(Path::new(&backup_path).exists());

        let counts = read_provider_counts(&codex_dir.join(CODEX_STATE_DB)).expect("counts");
        let map = counts_to_map(&counts);
        assert_eq!(map.get("openai"), Some(&3));
        assert!(map.get("bold_ai_api").is_none());
    }
}
