//! Omnigent 会话用量同步
//!
//! 从 Omnigent 的 SQLite 数据库 `~/.omnigent/chat.db` 中读取
//! `conversations.session_usage` 聚合 JSON，并写入统一 usage 表。

use crate::config::get_home_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::find_model_pricing;
use rust_decimal::Decimal;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
#[cfg(target_os = "windows")]
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::Command;

const OMNIGENT_PROVIDER_ID: &str = "_omnigent_session";
const OMNIGENT_APP_TYPE: &str = "omnigent";
const OMNIGENT_DATA_SOURCE: &str = "omnigent_session";

#[derive(Debug)]
struct OmnigentConversationUsage {
    conversation_id: String,
    created_at: i64,
    updated_at: i64,
    fallback_model: String,
    session_usage: String,
}

#[derive(Debug, Clone, PartialEq)]
struct OmnigentModelUsage {
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    total_cost_usd: String,
}

/// 同步 Omnigent 使用数据。
pub fn sync_omnigent_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let db_paths = omnigent_db_paths();

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        errors: vec![],
    };

    for db_path in db_paths {
        if !db_path.exists() {
            continue;
        }

        result.files_scanned += 1;
        match sync_single_omnigent_db(db, &db_path) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Omnigent 数据库同步失败 {}: {e}", db_path.display());
                log::warn!("[OMNIGENT-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[OMNIGENT-SYNC] 同步完成: 导入/更新 {} 条, 跳过 {} 条, 扫描 {} 个数据库",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

fn omnigent_db_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(path) = std::env::var("OMNIGENT_DB_PATH") {
        push_unique_path(&mut paths, PathBuf::from(path));
    }
    if let Ok(home) = std::env::var("OMNIGENT_HOME") {
        push_unique_path(&mut paths, PathBuf::from(home).join("chat.db"));
    }

    push_unique_path(&mut paths, get_home_dir().join(".omnigent").join("chat.db"));

    for path in wsl_omnigent_db_paths() {
        push_unique_path(&mut paths, path);
    }

    paths
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    let key = path.to_string_lossy().to_string();
    if !paths.iter().any(|p| p.to_string_lossy() == key) {
        paths.push(path);
    }
}

#[cfg(target_os = "windows")]
fn wsl_omnigent_db_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    for root in [r"\\wsl.localhost", r"\\wsl$"] {
        let root_path = Path::new(root);
        let Ok(distros) = fs::read_dir(root_path) else {
            continue;
        };

        for distro_entry in distros.flatten() {
            let distro_path = distro_entry.path();

            let home_path = distro_path.join("home");
            if let Ok(users) = fs::read_dir(&home_path) {
                for user_entry in users.flatten() {
                    let candidate = user_entry.path().join(".omnigent").join("chat.db");
                    let key = candidate.to_string_lossy().to_string();
                    if candidate.exists() && seen.insert(key) {
                        paths.push(candidate);
                    }
                }
            }

            let root_candidate = distro_path.join("root").join(".omnigent").join("chat.db");
            let key = root_candidate.to_string_lossy().to_string();
            if root_candidate.exists() && seen.insert(key) {
                paths.push(root_candidate);
            }
        }
    }

    if let Some(candidate) = default_wsl_omnigent_db_path() {
        let key = candidate.to_string_lossy().to_string();
        if candidate.exists() && seen.insert(key) {
            paths.push(candidate);
        }
    }

    paths
}

#[cfg(target_os = "windows")]
fn default_wsl_omnigent_db_path() -> Option<PathBuf> {
    let output = Command::new("wsl.exe")
        .args(["--", "sh", "-lc", r#"wslpath -w "$HOME/.omnigent/chat.db""#])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

#[cfg(not(target_os = "windows"))]
fn wsl_omnigent_db_paths() -> Vec<PathBuf> {
    Vec::new()
}

fn sync_single_omnigent_db(db: &Database, db_path: &Path) -> Result<(u32, u32), AppError> {
    let db_path_str = db_path.to_string_lossy().to_string();
    let metadata = fs::metadata(db_path)
        .map_err(|e| AppError::Config(format!("无法读取 Omnigent DB 元数据: {e}")))?;
    let mut file_modified = metadata_modified_nanos(&metadata);

    let wal_path = db_path.with_extension("db-wal");
    if let Ok(wal_meta) = fs::metadata(&wal_path) {
        file_modified = file_modified.max(metadata_modified_nanos(&wal_meta));
    }

    let (last_modified, _) = get_sync_state(db, &db_path_str)?;
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let snapshot_path = snapshot_omnigent_db(db_path, file_modified)?;
    let omnigent_conn = rusqlite::Connection::open_with_flags(
        &snapshot_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| AppError::Database(format!("无法打开 Omnigent DB 快照: {e}")))?;

    let conversations = query_conversation_usages(&omnigent_conn)?;
    let mut imported = 0;
    let mut skipped = 0;
    let mut had_error = false;

    for conversation in &conversations {
        let usages =
            match parse_session_usage(&conversation.session_usage, &conversation.fallback_model) {
                Ok(usages) => usages,
                Err(e) => {
                    log::warn!(
                        "[OMNIGENT-SYNC] session_usage 解析失败 {}: {e}",
                        conversation.conversation_id
                    );
                    had_error = true;
                    skipped += 1;
                    continue;
                }
            };

        if usages.is_empty() {
            skipped += 1;
            continue;
        }

        for usage in usages {
            let request_id = format!(
                "omnigent_session:{}:{}",
                conversation.conversation_id, usage.model
            );
            match insert_omnigent_session_entry(
                db,
                &request_id,
                &conversation.conversation_id,
                conversation.created_at,
                &usage,
            ) {
                Ok(true) => imported += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    log::warn!("[OMNIGENT-SYNC] 插入失败 {request_id}: {e}");
                    had_error = true;
                    skipped += 1;
                }
            }
        }
    }

    if !had_error {
        let high_watermark = conversations
            .iter()
            .map(|c| c.updated_at)
            .max()
            .unwrap_or_default();
        update_sync_state(db, &db_path_str, file_modified, high_watermark)?;
    }

    Ok((imported, skipped))
}

fn snapshot_omnigent_db(db_path: &Path, file_modified: i64) -> Result<PathBuf, AppError> {
    let mut hasher = DefaultHasher::new();
    db_path.to_string_lossy().hash(&mut hasher);
    file_modified.hash(&mut hasher);

    let dir = std::env::temp_dir()
        .join("cc-switch-omnigent")
        .join(format!("{:x}", hasher.finish()));
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Config(format!("无法创建 Omnigent DB 快照目录: {e}")))?;

    let snapshot = dir.join("chat.db");
    for path in [
        snapshot.clone(),
        snapshot.with_extension("db-wal"),
        snapshot.with_extension("db-shm"),
    ] {
        let _ = fs::remove_file(path);
    }

    fs::copy(db_path, &snapshot)
        .map_err(|e| AppError::Config(format!("无法复制 Omnigent DB 快照: {e}")))?;
    for ext in ["db-wal", "db-shm"] {
        let source = db_path.with_extension(ext);
        if source.exists() {
            fs::copy(source, snapshot.with_extension(ext))
                .map_err(|e| AppError::Config(format!("无法复制 Omnigent DB {ext} 快照: {e}")))?;
        }
    }

    Ok(snapshot)
}

fn query_conversation_usages(
    conn: &rusqlite::Connection,
) -> Result<Vec<OmnigentConversationUsage>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, updated_at, COALESCE(NULLIF(model_override, ''), 'unknown'),
                    session_usage
             FROM conversations
             WHERE session_usage IS NOT NULL AND session_usage <> ''
             ORDER BY updated_at",
        )
        .map_err(|e| AppError::Database(format!("准备 Omnigent 会话查询失败: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(OmnigentConversationUsage {
                conversation_id: row.get(0)?,
                created_at: row.get(1)?,
                updated_at: row.get(2)?,
                fallback_model: row.get(3)?,
                session_usage: row.get(4)?,
            })
        })
        .map_err(|e| AppError::Database(format!("查询 Omnigent 会话失败: {e}")))?;

    let mut conversations = Vec::new();
    for row in rows {
        conversations
            .push(row.map_err(|e| AppError::Database(format!("读取 Omnigent 会话行失败: {e}")))?);
    }

    Ok(conversations)
}

fn parse_session_usage(
    raw: &str,
    fallback_model: &str,
) -> Result<Vec<OmnigentModelUsage>, AppError> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|e| AppError::Config(format!("Omnigent session_usage JSON 解析失败: {e}")))?;

    let mut usages = Vec::new();
    if let Some(by_model) = value.get("by_model").and_then(|v| v.as_object()) {
        for (model, model_usage) in by_model {
            if let Some(usage) = parse_model_usage(model, model_usage) {
                usages.push(usage);
            }
        }
    }

    if usages.is_empty() {
        if let Some(usage) = parse_model_usage(fallback_model, &value) {
            usages.push(usage);
        }
    }

    Ok(usages)
}

fn parse_model_usage(model: &str, value: &Value) -> Option<OmnigentModelUsage> {
    let input_tokens = value_u32(value, &["input_tokens"]);
    let output_tokens = value_u32(value, &["output_tokens"]);
    let cache_read_tokens = value_u32(
        value,
        &[
            "cache_read_input_tokens",
            "cache_read_tokens",
            "cached_tokens",
        ],
    );
    let cache_creation_tokens = value_u32(
        value,
        &["cache_creation_input_tokens", "cache_creation_tokens"],
    );
    let total_cost_usd = value_cost_string(value, &["total_cost_usd", "cost_usd", "cost"])
        .unwrap_or_else(|| "0".to_string());

    let has_tokens =
        input_tokens > 0 || output_tokens > 0 || cache_read_tokens > 0 || cache_creation_tokens > 0;
    let has_cost = total_cost_usd.parse::<f64>().unwrap_or(0.0) > 0.0;
    if !has_tokens && !has_cost {
        return None;
    }

    Some(OmnigentModelUsage {
        model: model.trim().to_string(),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        total_cost_usd,
    })
}

fn value_u32(value: &Value, keys: &[&str]) -> u32 {
    for key in keys {
        if let Some(raw) = value.get(*key) {
            if let Some(n) = raw.as_u64() {
                return n.min(u32::MAX as u64) as u32;
            }
            if let Some(s) = raw.as_str() {
                if let Ok(n) = s.trim().parse::<u64>() {
                    return n.min(u32::MAX as u64) as u32;
                }
            }
        }
    }
    0
}

fn value_cost_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(raw) = value.get(*key) else {
            continue;
        };
        let parsed = match raw {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        };
        let Some(parsed) = parsed else {
            continue;
        };
        if parsed.is_finite() && parsed >= 0.0 {
            return Some(parsed.to_string());
        }
    }
    None
}

fn insert_omnigent_session_entry(
    db: &Database,
    request_id: &str,
    conversation_id: &str,
    created_at: i64,
    usage: &OmnigentModelUsage,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        if usage.total_cost_usd.parse::<f64>().unwrap_or(0.0) > 0.0 {
            (
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                usage.total_cost_usd.clone(),
            )
        } else {
            let token_usage = TokenUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cache_read_tokens: usage.cache_read_tokens,
                cache_creation_tokens: usage.cache_creation_tokens,
                model: Some(usage.model.clone()),
                message_id: None,
            };

            match find_model_pricing(&conn, &usage.model) {
                Some(pricing) => {
                    let cost = CostCalculator::calculate_for_app(
                        OMNIGENT_APP_TYPE,
                        &token_usage,
                        &pricing,
                        Decimal::from(1),
                    );
                    (
                        cost.input_cost.to_string(),
                        cost.output_cost.to_string(),
                        cost.cache_read_cost.to_string(),
                        cost.cache_creation_cost.to_string(),
                        cost.total_cost.to_string(),
                    )
                }
                None => (
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                ),
            }
        };

    let changed = conn
        .execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, first_token_ms, status_code, error_message, session_id,
                provider_type, is_streaming, cost_multiplier, created_at, data_source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)
            ON CONFLICT(request_id) DO UPDATE SET
                model = excluded.model,
                request_model = excluded.request_model,
                input_tokens = excluded.input_tokens,
                output_tokens = excluded.output_tokens,
                cache_read_tokens = excluded.cache_read_tokens,
                cache_creation_tokens = excluded.cache_creation_tokens,
                input_cost_usd = excluded.input_cost_usd,
                output_cost_usd = excluded.output_cost_usd,
                cache_read_cost_usd = excluded.cache_read_cost_usd,
                cache_creation_cost_usd = excluded.cache_creation_cost_usd,
                total_cost_usd = excluded.total_cost_usd,
                session_id = excluded.session_id,
                created_at = excluded.created_at
            WHERE model != excluded.model
               OR input_tokens != excluded.input_tokens
               OR output_tokens != excluded.output_tokens
               OR cache_read_tokens != excluded.cache_read_tokens
               OR cache_creation_tokens != excluded.cache_creation_tokens
               OR total_cost_usd != excluded.total_cost_usd
               OR created_at != excluded.created_at",
            rusqlite::params![
                request_id,
                OMNIGENT_PROVIDER_ID,
                OMNIGENT_APP_TYPE,
                usage.model,
                usage.model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                0i64,
                Option::<i64>::None,
                200i64,
                Option::<String>::None,
                Some(conversation_id.to_string()),
                Some(OMNIGENT_DATA_SOURCE),
                1i64,
                "1.0",
                created_at,
                OMNIGENT_DATA_SOURCE,
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 Omnigent 会话日志失败: {e}")))?
        > 0;

    if changed {
        crate::usage_events::notify_log_recorded();
    }

    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_usage_by_model() -> Result<(), AppError> {
        let raw = r#"{
            "input_tokens": 9771,
            "output_tokens": 3936,
            "cache_read_input_tokens": 86083,
            "cache_creation_input_tokens": 32678,
            "total_cost_usd": 0.394534,
            "by_model": {
                "claude-opus-4-8": {
                    "input_tokens": 9771,
                    "output_tokens": 3936,
                    "total_tokens": 13707,
                    "cache_read_input_tokens": 86083,
                    "cache_creation_input_tokens": 32678,
                    "total_cost_usd": 0.394534
                }
            }
        }"#;

        let usages = parse_session_usage(raw, "unknown")?;
        assert_eq!(usages.len(), 1);
        assert_eq!(
            usages[0],
            OmnigentModelUsage {
                model: "claude-opus-4-8".to_string(),
                input_tokens: 9771,
                output_tokens: 3936,
                cache_read_tokens: 86083,
                cache_creation_tokens: 32678,
                total_cost_usd: "0.394534".to_string(),
            }
        );

        Ok(())
    }

    #[test]
    fn test_parse_session_usage_cost_only_model() -> Result<(), AppError> {
        let raw = r#"{
            "total_cost_usd": 0.818066,
            "by_model": {
                "claude-opus-4-8": {
                    "total_cost_usd": 0.818066
                }
            },
            "policy_cost_usd": 0.818066
        }"#;

        let usages = parse_session_usage(raw, "unknown")?;
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].model, "claude-opus-4-8");
        assert_eq!(usages[0].input_tokens, 0);
        assert_eq!(usages[0].output_tokens, 0);
        assert_eq!(usages[0].total_cost_usd, "0.818066");

        Ok(())
    }

    #[test]
    fn test_parse_session_usage_fallback_model() -> Result<(), AppError> {
        let raw = r#"{
            "input_tokens": "100",
            "output_tokens": "20",
            "cache_read_tokens": "300",
            "cost": "0.05"
        }"#;

        let usages = parse_session_usage(raw, "gpt-5.5")?;
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].model, "gpt-5.5");
        assert_eq!(usages[0].input_tokens, 100);
        assert_eq!(usages[0].output_tokens, 20);
        assert_eq!(usages[0].cache_read_tokens, 300);
        assert_eq!(usages[0].total_cost_usd, "0.05");

        Ok(())
    }

    #[test]
    fn test_insert_omnigent_session_entry_upserts_changed_usage() -> Result<(), AppError> {
        let db = Database::memory()?;
        let first = OmnigentModelUsage {
            model: "gpt-5.5".to_string(),
            input_tokens: 100,
            output_tokens: 10,
            cache_read_tokens: 500,
            cache_creation_tokens: 0,
            total_cost_usd: "0.1".to_string(),
        };
        let second = OmnigentModelUsage {
            output_tokens: 25,
            total_cost_usd: "0.2".to_string(),
            ..first.clone()
        };

        assert!(insert_omnigent_session_entry(
            &db,
            "omnigent_session:conv:model",
            "conv",
            1000,
            &first,
        )?);
        assert!(insert_omnigent_session_entry(
            &db,
            "omnigent_session:conv:model",
            "conv",
            1000,
            &second,
        )?);
        assert!(!insert_omnigent_session_entry(
            &db,
            "omnigent_session:conv:model",
            "conv",
            1000,
            &second,
        )?);

        let conn = lock_conn!(db.conn);
        let (output_tokens, total_cost): (i64, String) = conn.query_row(
            "SELECT output_tokens, total_cost_usd
             FROM proxy_request_logs
             WHERE request_id = 'omnigent_session:conv:model'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(output_tokens, 25);
        assert_eq!(total_cost, "0.2");

        Ok(())
    }

    #[test]
    fn test_sync_single_omnigent_db_imports_conversation_usage() -> Result<(), AppError> {
        let temp =
            tempfile::tempdir().map_err(|e| AppError::Config(format!("创建临时目录失败: {e}")))?;
        let db_path = temp.path().join("chat.db");
        {
            let conn = rusqlite::Connection::open(&db_path)?;
            conn.execute_batch(
                "CREATE TABLE conversations (
                    id TEXT PRIMARY KEY,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    model_override TEXT,
                    session_usage TEXT
                );",
            )?;
            conn.execute(
                "INSERT INTO conversations
                    (id, created_at, updated_at, model_override, session_usage)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    "conv_1",
                    1000i64,
                    1100i64,
                    Option::<String>::None,
                    r#"{"by_model":{"gpt-5.5":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":300,"total_cost_usd":0.12}}}"#
                ],
            )?;
        }

        let usage_db = Database::memory()?;
        let (imported, skipped) = sync_single_omnigent_db(&usage_db, &db_path)?;
        assert_eq!(imported, 1);
        assert_eq!(skipped, 0);

        let conn = lock_conn!(usage_db.conn);
        let (provider_id, app_type, model, data_source, input_tokens, cache_read_tokens): (
            String,
            String,
            String,
            String,
            i64,
            i64,
        ) = conn.query_row(
            "SELECT provider_id, app_type, model, data_source, input_tokens, cache_read_tokens
             FROM proxy_request_logs
             WHERE request_id = 'omnigent_session:conv_1:gpt-5.5'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        assert_eq!(provider_id, OMNIGENT_PROVIDER_ID);
        assert_eq!(app_type, OMNIGENT_APP_TYPE);
        assert_eq!(model, "gpt-5.5");
        assert_eq!(data_source, OMNIGENT_DATA_SOURCE);
        assert_eq!(input_tokens, 100);
        assert_eq!(cache_read_tokens, 300);

        Ok(())
    }

    #[test]
    fn test_snapshot_omnigent_db_copies_and_clears_sidecars() -> Result<(), AppError> {
        let temp =
            tempfile::tempdir().map_err(|e| AppError::Config(format!("创建临时目录失败: {e}")))?;
        let db_path = temp.path().join("chat.db");
        std::fs::write(&db_path, b"db").expect("write db");
        std::fs::write(db_path.with_extension("db-wal"), b"wal").expect("write wal");
        std::fs::write(db_path.with_extension("db-shm"), b"shm").expect("write shm");

        let snapshot = snapshot_omnigent_db(&db_path, 1)?;
        assert_eq!(std::fs::read(&snapshot).expect("read db"), b"db");
        assert_eq!(
            std::fs::read(snapshot.with_extension("db-wal")).expect("read wal"),
            b"wal"
        );
        assert_eq!(
            std::fs::read(snapshot.with_extension("db-shm")).expect("read shm"),
            b"shm"
        );

        std::fs::remove_file(db_path.with_extension("db-wal")).expect("remove wal");
        std::fs::remove_file(db_path.with_extension("db-shm")).expect("remove shm");
        let snapshot = snapshot_omnigent_db(&db_path, 1)?;
        assert!(!snapshot.with_extension("db-wal").exists());
        assert!(!snapshot.with_extension("db-shm").exists());

        Ok(())
    }
}
