//! Codex 第三方历史会话归桶迁移。
//!
//! 只迁移本机 `~/.codex` 历史数据；完成标记写入设备级 `settings.json`，
//! 失败时不写标记，下一次启动自动重试。

use crate::codex_config::{
    get_codex_config_dir, read_codex_config_text, CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
};
use crate::config::{atomic_write, copy_file, get_app_config_dir};
use crate::database::{is_official_seed_id, Database};
use crate::error::AppError;
use crate::settings::{
    CodexProviderTemplateMigration, CodexThirdPartyHistoryProviderBucketMigration,
};
use chrono::{Local, Utc};
use rusqlite::{backup::Backup, params_from_iter, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use toml_edit::DocumentMut;

const MIGRATION_NAME: &str = "codex-history-provider-migration-v1";
const CODEX_STATE_DB_FILENAME: &str = "state_5.sqlite";
const LEGACY_CC_SWITCH_CODEX_MODEL_PROVIDER_ID: &str = "ccswitch";
// If a Codex preset ever used a temporary routing key, keep that old key here
// so local history can be bucketed under the current custom provider id.
const CC_SWITCH_LEGACY_CODEX_MODEL_PROVIDER_IDS: &[&str] = &[
    LEGACY_CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
    "aicodemirror",
    "aicoding",
    "aigocode",
    "aihubmix",
    "ark_agentplan",
    "bailian",
    "bailing",
    "byteplus",
    "claudecn",
    "compshare",
    "compshare_coding",
    "crazyrouter",
    "ctok",
    "cubence",
    "deepseek",
    "dmxapi",
    "doubaoseed",
    "eflowcode",
    "kimi",
    "lemondata",
    "longcat",
    "micu",
    "minimax",
    "minimax_en",
    "modelscope",
    "novita",
    "nvidia",
    "openrouter",
    "packycode",
    "patewayai",
    "pipellm",
    "qianfan_coding",
    "relaxycode",
    "rightcode",
    "runapi",
    "shengsuanyun",
    "siliconflow",
    "siliconflow_en",
    "sssaicode",
    "stepfun",
    "stepfun_en",
    "therouter",
    "xiaomi_mimo",
    "xiaomi_mimo_token_plan",
    "zhipu_glm",
    "zhipu_glm_en",
];

#[derive(Debug, Clone, Default)]
pub struct CodexHistoryProviderBucketMigrationOutcome {
    pub source_provider_ids: Vec<String>,
    pub migrated_jsonl_files: usize,
    pub migrated_state_rows: usize,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CodexProviderTemplateBucketMigrationOutcome {
    pub migrated_provider_ids: Vec<String>,
    pub skipped_reason: Option<String>,
}

pub fn maybe_migrate_codex_third_party_history_provider_bucket(
    db: &Database,
) -> Result<CodexHistoryProviderBucketMigrationOutcome, AppError> {
    if crate::settings::is_codex_third_party_history_provider_bucket_migrated() {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("already_migrated".to_string()),
            ..Default::default()
        });
    }

    let source_provider_ids = collect_source_model_provider_ids(db)?;
    if source_provider_ids.is_empty() {
        crate::settings::mark_codex_third_party_history_provider_bucket_migrated(
            CodexThirdPartyHistoryProviderBucketMigration {
                completed_at: Utc::now().to_rfc3339(),
                target_provider_id: CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string(),
                source_provider_ids: Vec::new(),
                migrated_jsonl_files: 0,
                migrated_state_rows: 0,
                scanned_history_files: true,
            },
        )?;
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("no_third_party_provider_ids".to_string()),
            ..Default::default()
        });
    }

    let backup_root = migration_backup_root();
    let codex_dir = get_codex_config_dir();
    let migrated_jsonl_files =
        migrate_codex_jsonl_files(&codex_dir, &source_provider_ids, &backup_root)?;
    let migrated_state_rows =
        migrate_codex_state_dbs(&codex_dir, &source_provider_ids, &backup_root)?;

    let source_provider_ids_vec: Vec<String> = source_provider_ids.iter().cloned().collect();
    crate::settings::mark_codex_third_party_history_provider_bucket_migrated(
        CodexThirdPartyHistoryProviderBucketMigration {
            completed_at: Utc::now().to_rfc3339(),
            target_provider_id: CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string(),
            source_provider_ids: source_provider_ids_vec.clone(),
            migrated_jsonl_files,
            migrated_state_rows,
            scanned_history_files: true,
        },
    )?;

    Ok(CodexHistoryProviderBucketMigrationOutcome {
        source_provider_ids: source_provider_ids_vec,
        migrated_jsonl_files,
        migrated_state_rows,
        skipped_reason: None,
    })
}

pub fn maybe_migrate_codex_provider_template_bucket(
    db: &Database,
) -> Result<CodexProviderTemplateBucketMigrationOutcome, AppError> {
    if crate::settings::is_codex_provider_template_migrated() {
        return Ok(CodexProviderTemplateBucketMigrationOutcome {
            skipped_reason: Some("already_migrated".to_string()),
            ..Default::default()
        });
    }

    let backup_root = migration_backup_root();
    let outcome = migrate_codex_provider_templates_to_custom(db, &backup_root)?;
    crate::settings::mark_codex_provider_template_migrated(CodexProviderTemplateMigration {
        completed_at: Utc::now().to_rfc3339(),
        migrated_provider_ids: outcome.migrated_provider_ids.clone(),
    })?;

    Ok(outcome)
}

fn migrate_codex_provider_templates_to_custom(
    db: &Database,
    backup_root: &Path,
) -> Result<CodexProviderTemplateBucketMigrationOutcome, AppError> {
    let providers = db.get_all_providers("codex")?;
    let mut migrated_provider_ids = Vec::new();

    for (_, provider) in providers {
        if provider.category.as_deref() == Some("official")
            || is_official_seed_id(&provider.id)
            || provider.is_codex_oauth()
        {
            continue;
        }

        let Some(config_text) = provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str())
        else {
            continue;
        };

        let Some(migrated_config_text) = migrate_provider_config_template_to_custom(config_text)?
        else {
            continue;
        };

        let mut settings = provider.settings_config.clone();
        let Some(obj) = settings.as_object_mut() else {
            log::warn!(
                "Skipping Codex provider template migration for {}: settings_config is not an object",
                provider.id
            );
            continue;
        };
        backup_provider_settings_config(&provider.id, &provider.settings_config, backup_root)?;
        obj.insert("config".to_string(), Value::String(migrated_config_text));
        db.update_provider_settings_config("codex", &provider.id, &settings)?;
        migrated_provider_ids.push(provider.id);
    }

    Ok(CodexProviderTemplateBucketMigrationOutcome {
        migrated_provider_ids,
        skipped_reason: None,
    })
}

fn collect_source_model_provider_ids(db: &Database) -> Result<BTreeSet<String>, AppError> {
    let providers = db.get_all_providers("codex")?;
    let mut ids = BTreeSet::new();

    for provider in providers.values() {
        if provider.category.as_deref() == Some("official")
            || is_official_seed_id(&provider.id)
            || provider.is_codex_oauth()
        {
            continue;
        }

        insert_known_cc_switch_legacy_source_id(&mut ids, &provider.id);

        let Some(config_text) = provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str())
        else {
            continue;
        };

        for provider_id in trusted_legacy_codex_model_provider_ids_from_config(config_text) {
            insert_known_cc_switch_legacy_source_id(&mut ids, &provider_id);
        }
        if let Some(provider_id) =
            legacy_codex_model_provider_id_from_normalized_config(config_text)
        {
            insert_known_cc_switch_legacy_source_id(&mut ids, &provider_id);
        }
    }

    Ok(ids)
}

fn insert_known_cc_switch_legacy_source_id(ids: &mut BTreeSet<String>, provider_id: &str) {
    let trimmed = provider_id.trim();
    if is_known_cc_switch_legacy_codex_model_provider_id(trimmed) {
        ids.insert(trimmed.to_string());
    }
}

fn migration_backup_root() -> PathBuf {
    get_app_config_dir()
        .join("backups")
        .join(MIGRATION_NAME)
        .join(Local::now().format("%Y%m%d_%H%M%S").to_string())
}

fn is_known_cc_switch_legacy_codex_model_provider_id(provider_id: &str) -> bool {
    CC_SWITCH_LEGACY_CODEX_MODEL_PROVIDER_IDS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(provider_id))
}

fn legacy_codex_model_provider_id_from_normalized_config(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let provider_id = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)?;
    if provider_id != CC_SWITCH_CODEX_MODEL_PROVIDER_ID
        && provider_id != LEGACY_CC_SWITCH_CODEX_MODEL_PROVIDER_ID
    {
        return None;
    }

    let name = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|table| table.get(provider_id))
        .and_then(|item| item.as_table())
        .and_then(|table| table.get("name"))
        .and_then(|item| item.as_str())?
        .trim();

    normalized_legacy_codex_provider_name(name).map(str::to_string)
}

fn normalized_legacy_codex_provider_name(name: &str) -> Option<&'static str> {
    if is_known_cc_switch_legacy_codex_model_provider_id(name) {
        return CC_SWITCH_LEGACY_CODEX_MODEL_PROVIDER_IDS
            .iter()
            .copied()
            .find(|known| known.eq_ignore_ascii_case(name));
    }

    match name {
        "E-FlowCode" => Some("eflowcode"),
        "PIPELLM" => Some("pipellm"),
        _ => None,
    }
}

fn trusted_legacy_codex_model_provider_ids_from_config(config_text: &str) -> BTreeSet<String> {
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        return BTreeSet::new();
    };

    trusted_legacy_codex_model_provider_ids_from_doc(&doc)
}

fn trusted_legacy_codex_model_provider_ids_from_doc(doc: &DocumentMut) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    insert_trusted_legacy_config_model_provider_id(&mut ids, doc, doc.get("model_provider"));

    if let Some(profiles) = doc.get("profiles").and_then(|item| item.as_table_like()) {
        for (_, profile_item) in profiles.iter() {
            if let Some(profile_table) = profile_item.as_table_like() {
                insert_trusted_legacy_config_model_provider_id(
                    &mut ids,
                    doc,
                    profile_table.get("model_provider"),
                );
            }
        }
    }

    ids
}

fn insert_trusted_legacy_config_model_provider_id(
    ids: &mut BTreeSet<String>,
    doc: &DocumentMut,
    item: Option<&toml_edit::Item>,
) {
    let Some(provider_id) = item.and_then(|item| item.as_str()).map(str::trim) else {
        return;
    };
    if provider_id.is_empty()
        || !is_known_cc_switch_legacy_codex_model_provider_id(provider_id)
        || !config_defines_model_provider(doc, provider_id)
    {
        return;
    }
    ids.insert(provider_id.to_string());
}

fn config_defines_model_provider(doc: &DocumentMut, provider_id: &str) -> bool {
    doc.get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|table| table.get(provider_id))
        .and_then(|item| item.as_table())
        .is_some()
}

fn migrate_provider_config_template_to_custom(
    config_text: &str,
) -> Result<Option<String>, AppError> {
    if config_text.trim().is_empty() {
        return Ok(None);
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    let source_provider_ids = trusted_legacy_codex_model_provider_ids_from_doc(&doc);
    if source_provider_ids.is_empty() {
        return Ok(None);
    }

    let active_provider_id = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|provider_id| !provider_id.is_empty())
        .map(str::to_string);

    let custom_table_exists =
        config_defines_model_provider(&doc, CC_SWITCH_CODEX_MODEL_PROVIDER_ID);
    let source_provider_id_to_move = active_provider_id
        .as_deref()
        .filter(|provider_id| source_provider_ids.contains(*provider_id))
        .map(str::to_string)
        .or_else(|| {
            if custom_table_exists {
                None
            } else {
                source_provider_ids.iter().next().cloned()
            }
        });

    let mut changed = false;

    if let Some(source_provider_id) = source_provider_id_to_move {
        let Some(model_providers) = doc
            .get_mut("model_providers")
            .and_then(|item| item.as_table_mut())
        else {
            return Ok(None);
        };

        let Some(provider_table) = model_providers.remove(source_provider_id.as_str()) else {
            return Ok(None);
        };
        model_providers[CC_SWITCH_CODEX_MODEL_PROVIDER_ID] = provider_table;
        changed = true;
    }

    if active_provider_id
        .as_deref()
        .is_some_and(|provider_id| source_provider_ids.contains(provider_id))
    {
        doc["model_provider"] = toml_edit::value(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);
        changed = true;
    }

    for source_provider_id in source_provider_ids {
        if rewrite_legacy_provider_profile_refs(&mut doc, source_provider_id.as_str()) {
            changed = true;
        }
    }

    if changed {
        Ok(Some(doc.to_string()))
    } else {
        Ok(None)
    }
}

fn rewrite_legacy_provider_profile_refs(doc: &mut DocumentMut, source_provider_id: &str) -> bool {
    let Some(profiles) = doc
        .get_mut("profiles")
        .and_then(|item| item.as_table_like_mut())
    else {
        return false;
    };

    let mut changed = false;
    let profile_keys: Vec<String> = profiles.iter().map(|(key, _)| key.to_string()).collect();
    for profile_key in profile_keys {
        let Some(profile_table) = profiles
            .get_mut(&profile_key)
            .and_then(|item| item.as_table_like_mut())
        else {
            continue;
        };

        let references_legacy = profile_table
            .get("model_provider")
            .and_then(|item| item.as_str())
            == Some(source_provider_id);
        if references_legacy {
            profile_table.insert(
                "model_provider",
                toml_edit::value(CC_SWITCH_CODEX_MODEL_PROVIDER_ID),
            );
            changed = true;
        }
    }
    changed
}

fn migrate_codex_jsonl_files(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
) -> Result<usize, AppError> {
    let mut files = Vec::new();
    collect_jsonl_files(&codex_dir.join("sessions"), &mut files, 0, 8);
    collect_jsonl_files(&codex_dir.join("archived_sessions"), &mut files, 0, 4);

    let source_provider_ids: HashSet<String> = source_provider_ids.iter().cloned().collect();
    let mut migrated = 0;
    for file_path in files {
        if rewrite_codex_session_file_for_provider_bucket(
            &file_path,
            codex_dir,
            &source_provider_ids,
            backup_root,
        )? {
            migrated += 1;
        }
    }
    Ok(migrated)
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>, depth: u8, max_depth: u8) {
    if depth > max_depth || !dir.is_dir() {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            log::debug!(
                "Failed to read Codex session directory {}: {err}",
                dir.display()
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn rewrite_codex_session_file_for_provider_bucket(
    path: &Path,
    codex_dir: &Path,
    source_provider_ids: &HashSet<String>,
    backup_root: &Path,
) -> Result<bool, AppError> {
    let metadata_before = fs::metadata(path).map_err(|e| AppError::io(path, e))?;
    let modified_before = metadata_before.modified().ok();
    let len_before = metadata_before.len();
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;

    let mut rewritten = String::with_capacity(content.len());
    let mut changed = false;
    for segment in content.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if let Some(next_line) = rewrite_codex_session_meta_line(line, source_provider_ids) {
            rewritten.push_str(&next_line);
            changed = true;
        } else {
            rewritten.push_str(line);
        }
        rewritten.push_str(newline);
    }

    if !changed {
        return Ok(false);
    }

    ensure_codex_session_file_unchanged(path, modified_before, len_before)?;
    backup_codex_jsonl_file(path, codex_dir, backup_root)?;
    ensure_codex_session_file_unchanged(path, modified_before, len_before)?;
    atomic_write(path, rewritten.as_bytes())?;
    Ok(true)
}

fn ensure_codex_session_file_unchanged(
    path: &Path,
    modified_before: Option<SystemTime>,
    len_before: u64,
) -> Result<(), AppError> {
    let metadata_after = fs::metadata(path).map_err(|e| AppError::io(path, e))?;
    if metadata_after.modified().ok() != modified_before || metadata_after.len() != len_before {
        return Err(AppError::Message(format!(
            "Codex session file changed during migration: {}",
            path.display()
        )));
    }
    Ok(())
}

fn rewrite_codex_session_meta_line(
    line: &str,
    source_provider_ids: &HashSet<String>,
) -> Option<String> {
    if !line.contains("\"session_meta\"") || !line.contains("\"model_provider\"") {
        return None;
    }

    let mut value: Value = serde_json::from_str(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }

    let payload = value.get_mut("payload")?.as_object_mut()?;
    let current_provider = payload.get("model_provider")?.as_str()?;
    if !source_provider_ids.contains(current_provider) {
        return None;
    }

    payload.insert(
        "model_provider".to_string(),
        Value::String(CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string()),
    );
    serde_json::to_string(&value).ok()
}

fn migrate_codex_state_dbs(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
) -> Result<usize, AppError> {
    let config_text = read_codex_config_text().unwrap_or_default();
    let mut migrated = 0;
    for db_path in codex_state_db_paths(codex_dir, &config_text) {
        migrated += migrate_codex_state_db_provider_bucket(
            &db_path,
            codex_dir,
            source_provider_ids,
            backup_root,
        )?;
    }
    Ok(migrated)
}

fn codex_state_db_paths(codex_dir: &Path, config_text: &str) -> Vec<PathBuf> {
    let mut paths = vec![codex_dir.join(CODEX_STATE_DB_FILENAME)];
    if let Some(sqlite_home) = sqlite_home_from_codex_config(config_text) {
        let db_path = sqlite_home.join(CODEX_STATE_DB_FILENAME);
        if !paths.contains(&db_path) {
            paths.push(db_path);
        }
    }
    paths
}

fn sqlite_home_from_codex_config(config_text: &str) -> Option<PathBuf> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let raw = doc.get("sqlite_home")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn resolve_user_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return crate::config::get_home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return crate::config::get_home_dir().join(rest);
    }
    if let Some(rest) = raw.strip_prefix("~\\") {
        return crate::config::get_home_dir().join(rest);
    }
    PathBuf::from(raw)
}

fn migrate_codex_state_db_provider_bucket(
    db_path: &Path,
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
) -> Result<usize, AppError> {
    if !db_path.exists() || source_provider_ids.is_empty() {
        return Ok(0);
    }

    let mut conn = Connection::open(db_path)
        .map_err(|e| AppError::Database(format!("打开 Codex state DB 失败: {e}")))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| AppError::Database(format!("设置 Codex state DB busy_timeout 失败: {e}")))?;

    if !Database::table_exists(&conn, "threads")?
        || !Database::has_column(&conn, "threads", "model_provider")?
    {
        return Ok(0);
    }

    let placeholders = placeholders(source_provider_ids.len());
    let count_sql =
        format!("SELECT COUNT(*) FROM threads WHERE model_provider IN ({placeholders})");
    let matching_rows: i64 = conn
        .query_row(
            &count_sql,
            params_from_iter(source_provider_ids.iter()),
            |row| row.get(0),
        )
        .map_err(|e| AppError::Database(format!("统计 Codex state DB 待迁移行失败: {e}")))?;
    if matching_rows == 0 {
        return Ok(0);
    }

    backup_codex_state_db(db_path, codex_dir, backup_root, &conn)?;

    let update_sql =
        format!("UPDATE threads SET model_provider = ? WHERE model_provider IN ({placeholders})");
    let mut values = Vec::with_capacity(source_provider_ids.len() + 1);
    values.push(CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string());
    values.extend(source_provider_ids.iter().cloned());
    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(format!("开启 Codex state DB 迁移事务失败: {e}")))?;
    let changed = tx
        .execute(&update_sql, params_from_iter(values.iter()))
        .map_err(|e| AppError::Database(format!("迁移 Codex state DB provider 失败: {e}")))?;
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交 Codex state DB 迁移事务失败: {e}")))?;
    Ok(changed)
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn backup_codex_jsonl_file(
    path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    let backup_path = backup_root
        .join("jsonl")
        .join(relative_backup_path(path, codex_dir));
    copy_existing_file(path, &backup_path)
}

fn backup_codex_state_db(
    db_path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
    source_conn: &Connection,
) -> Result<(), AppError> {
    let backup_path = backup_root
        .join("state")
        .join(relative_backup_path(db_path, codex_dir));
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let mut backup_conn = Connection::open(&backup_path)
        .map_err(|e| AppError::Database(format!("创建 Codex state DB 备份失败: {e}")))?;
    let backup = Backup::new(source_conn, &mut backup_conn)
        .map_err(|e| AppError::Database(format!("初始化 Codex state DB 备份失败: {e}")))?;
    backup
        .run_to_completion(5, Duration::from_millis(25), None)
        .map_err(|e| AppError::Database(format!("写入 Codex state DB 备份失败: {e}")))?;
    Ok(())
}

fn backup_provider_settings_config(
    provider_id: &str,
    settings_config: &Value,
    backup_root: &Path,
) -> Result<(), AppError> {
    let backup_path = backup_root
        .join("providers")
        .join(provider_settings_backup_filename(provider_id));
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let payload = serde_json::json!({
        "providerId": provider_id,
        "settingsConfig": settings_config,
    });
    let bytes =
        serde_json::to_vec_pretty(&payload).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&backup_path, &bytes)
}

fn provider_settings_backup_filename(provider_id: &str) -> String {
    let safe_id: String = provider_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let safe_id = if safe_id.is_empty() {
        "provider".to_string()
    } else {
        safe_id
    };
    // Keep the hash stable across processes while avoiding collisions after sanitization.
    let digest = Sha256::digest(provider_id.as_bytes());
    let hash = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{hash}-{safe_id}.settings_config.json")
}

fn copy_existing_file(source: &Path, target: &Path) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    copy_file(source, target)
}

fn relative_backup_path(path: &Path, root: &Path) -> PathBuf {
    if let Ok(relative) = path.strip_prefix(root) {
        return relative.to_path_buf();
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    let hash = hasher.finish();
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    PathBuf::from("external").join(format!("{hash:016x}-{file_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use tempfile::tempdir;

    fn source_ids(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn migrate_provider_templates_for_test(
        db: &Database,
    ) -> (
        CodexProviderTemplateBucketMigrationOutcome,
        tempfile::TempDir,
    ) {
        let backup_dir = tempdir().expect("backup dir");
        let outcome = migrate_codex_provider_templates_to_custom(db, backup_dir.path())
            .expect("migrate template");
        (outcome, backup_dir)
    }

    #[test]
    fn simulates_local_codex_provider_bucket_migration_end_to_end() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let backup_root = dir.path().join("backup");
        fs::create_dir_all(&codex_dir).expect("create codex dir");

        let db = Database::memory().expect("memory db");
        let providers = [
            Provider::with_id(
                "rightcode".to_string(),
                "RightCode".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "aihubmix"

[model_providers.aihubmix]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
"#
                }),
                None,
            ),
            Provider::with_id(
                "legacy-ccswitch".to_string(),
                "Legacy CC Switch".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "ccswitch"

[model_providers.ccswitch]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
"#
                }),
                None,
            ),
            Provider::with_id(
                "normalized-aihubmix".to_string(),
                "Already Normalized".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "custom"

[model_providers.custom]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
"#
                }),
                None,
            ),
            Provider::with_id(
                "manual-relay".to_string(),
                "Manual Relay".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "my-private-relay"

[model_providers.my-private-relay]
name = "Manual Relay"
base_url = "http://localhost:8080/v1"
"#
                }),
                None,
            ),
            Provider::with_id(
                "custom-openai".to_string(),
                "Custom OpenAI".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "openai"

[model_providers.openai]
name = "Custom OpenAI"
base_url = "https://proxy.example/v1"
"#
                }),
                None,
            ),
        ];
        for provider in providers {
            db.save_provider("codex", &provider).expect("save provider");
        }

        let mut official = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            serde_json::json!({"auth": {}, "config": "model_provider = \"openai\""}),
            None,
        );
        official.category = Some("official".to_string());
        db.save_provider("codex", &official).expect("save official");

        let source_provider_ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert_eq!(
            source_provider_ids,
            source_ids(&["aihubmix", "ccswitch", "rightcode"])
        );

        let session_dir = codex_dir.join("sessions/2026/05/28");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let session_path = session_dir.join("local-sim.jsonl");
        fs::write(
            &session_path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"rightcode\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s2\",\"model_provider\":\"aihubmix\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s3\",\"model_provider\":\"ccswitch\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s4\",\"model_provider\":\"my-private-relay\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s5\",\"model_provider\":\"openai\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s6\",\"model_provider\":\"custom\"}}\n",
            ),
        )
        .expect("write session");

        let migrated_jsonl =
            migrate_codex_jsonl_files(&codex_dir, &source_provider_ids, &backup_root)
                .expect("migrate jsonl");
        assert_eq!(migrated_jsonl, 1);
        let session_text = fs::read_to_string(&session_path).expect("read session");
        assert_eq!(
            session_text
                .matches("\"model_provider\":\"custom\"")
                .count(),
            4
        );
        assert!(session_text.contains("\"model_provider\":\"my-private-relay\""));
        assert!(session_text.contains("\"model_provider\":\"openai\""));
        assert!(backup_root
            .join("jsonl/sessions/2026/05/28/local-sim.jsonl")
            .exists());

        let state_db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&state_db_path).expect("open state db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT NOT NULL
            );
            INSERT INTO threads (id, model_provider) VALUES
                ('rightcode-thread', 'rightcode'),
                ('aihubmix-thread', 'aihubmix'),
                ('ccswitch-thread', 'ccswitch'),
                ('manual-thread', 'my-private-relay'),
                ('openai-thread', 'openai'),
                ('custom-thread', 'custom');",
        )
        .expect("seed state db");
        drop(conn);

        let migrated_state_rows = migrate_codex_state_db_provider_bucket(
            &state_db_path,
            &codex_dir,
            &source_provider_ids,
            &backup_root,
        )
        .expect("migrate state db");
        assert_eq!(migrated_state_rows, 3);

        let conn = Connection::open(&state_db_path).expect("reopen state db");
        let count_provider = |provider_id: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = ?1",
                [provider_id],
                |row| row.get(0),
            )
            .expect("count provider")
        };
        assert_eq!(count_provider("custom"), 4);
        assert_eq!(count_provider("my-private-relay"), 1);
        assert_eq!(count_provider("openai"), 1);
        assert!(backup_root
            .join("state")
            .join(CODEX_STATE_DB_FILENAME)
            .exists());
        drop(conn);

        let template_outcome = migrate_codex_provider_templates_to_custom(&db, &backup_root)
            .expect("migrate provider templates");
        assert!(!template_outcome
            .migrated_provider_ids
            .iter()
            .any(|id| id == "normalized-aihubmix"));
        assert_eq!(
            source_ids(
                &template_outcome
                    .migrated_provider_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
            ),
            source_ids(&["legacy-ccswitch", "rightcode"])
        );

        let config_provider_id = |provider_id: &str| -> String {
            db.get_provider_by_id(provider_id, "codex")
                .expect("get provider")
                .expect("provider exists")
                .settings_config
                .get("config")
                .and_then(Value::as_str)
                .expect("config text")
                .to_string()
        };

        let rightcode_config: toml::Value =
            toml::from_str(&config_provider_id("rightcode")).expect("parse rightcode config");
        assert_eq!(
            rightcode_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert!(rightcode_config
            .get("model_providers")
            .and_then(|value| value.get("aihubmix"))
            .is_none());

        let ccswitch_config: toml::Value =
            toml::from_str(&config_provider_id("legacy-ccswitch")).expect("parse ccswitch config");
        assert_eq!(
            ccswitch_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert!(ccswitch_config
            .get("model_providers")
            .and_then(|value| value.get("ccswitch"))
            .is_none());

        let manual_config: toml::Value =
            toml::from_str(&config_provider_id("manual-relay")).expect("parse manual config");
        assert_eq!(
            manual_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("my-private-relay")
        );

        let openai_config: toml::Value =
            toml::from_str(&config_provider_id("custom-openai")).expect("parse openai config");
        assert_eq!(
            openai_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("openai")
        );

        let normalized_config: toml::Value =
            toml::from_str(&config_provider_id("normalized-aihubmix"))
                .expect("parse normalized config");
        assert_eq!(
            normalized_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
    }

    #[test]
    fn rewrites_only_codex_session_meta_provider_ids() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let backup_root = dir.path().join("backup");
        let session_dir = codex_dir.join("sessions/2026/05/20");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let path = session_dir.join("rollout-test.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"rightcode\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hi\"}}\n"
            ),
        )
        .expect("write session");

        let changed = rewrite_codex_session_file_for_provider_bucket(
            &path,
            &codex_dir,
            &HashSet::from(["rightcode".to_string()]),
            &backup_root,
        )
        .expect("rewrite");

        assert!(changed);
        let next = fs::read_to_string(&path).expect("read rewritten");
        assert!(next.contains("\"model_provider\":\"custom\""));
        assert!(backup_root
            .join("jsonl/sessions/2026/05/20/rollout-test.jsonl")
            .exists());
    }

    #[test]
    fn does_not_rewrite_unknown_jsonl_history_without_trusted_source_id() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let session_dir = codex_dir.join("sessions/2026/05/20");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let path = session_dir.join("rollout-rightcode.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"rightcode\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hi\"}}\n"
            ),
        )
        .expect("write session");

        let backup_root = dir.path().join("backup");
        let changed = migrate_codex_jsonl_files(
            &codex_dir,
            &source_ids(&["some-trusted-provider"]),
            &backup_root,
        )
        .expect("migrate jsonl");

        assert_eq!(changed, 0);
        let next = fs::read_to_string(&path).expect("read session");
        assert!(next.contains("\"model_provider\":\"rightcode\""));
        assert!(!backup_root.exists());
    }

    #[test]
    fn does_not_update_unknown_state_db_history_without_trusted_source_id() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        let db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT NOT NULL
            );
            INSERT INTO threads (id, model_provider) VALUES
                ('a', 'aihubmix'),
                ('b', 'openai'),
                ('c', 'custom');",
        )
        .expect("seed db");
        drop(conn);

        let backup_root = dir.path().join("backup");
        let changed = migrate_codex_state_db_provider_bucket(
            &db_path,
            &codex_dir,
            &source_ids(&["rightcode"]),
            &backup_root,
        )
        .expect("migrate state db");

        assert_eq!(changed, 0);
        let conn = Connection::open(&db_path).expect("reopen db");
        let aihubmix_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'aihubmix'",
                [],
                |row| row.get(0),
            )
            .expect("count aihubmix");
        assert_eq!(aihubmix_count, 1);
        assert!(!backup_root.exists());
    }

    #[test]
    fn updates_codex_state_db_thread_provider_ids() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        let db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT NOT NULL
            );
            INSERT INTO threads (id, model_provider) VALUES
                ('a', 'rightcode'),
                ('b', 'openai'),
                ('c', 'aihubmix');",
        )
        .expect("seed db");
        drop(conn);

        let backup_root = dir.path().join("backup");
        let changed = migrate_codex_state_db_provider_bucket(
            &db_path,
            &codex_dir,
            &source_ids(&["rightcode", "aihubmix"]),
            &backup_root,
        )
        .expect("migrate state db");

        assert_eq!(changed, 2);
        let conn = Connection::open(&db_path).expect("reopen db");
        let custom_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'custom'",
                [],
                |row| row.get(0),
            )
            .expect("count custom");
        let openai_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'openai'",
                [],
                |row| row.get(0),
            )
            .expect("count openai");
        assert_eq!(custom_count, 2);
        assert_eq!(openai_count, 1);

        let backup_path = backup_root.join("state").join(CODEX_STATE_DB_FILENAME);
        let backup_conn = Connection::open(&backup_path).expect("open backup db");
        let backed_up_source_count: i64 = backup_conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider IN ('rightcode', 'aihubmix')",
                [],
                |row| row.get(0),
            )
            .expect("count backed up source providers");
        assert_eq!(backed_up_source_count, 2);
    }

    #[test]
    fn collects_third_party_provider_ids_from_codex_providers() {
        let db = Database::memory().expect("memory db");
        let third_party = Provider::with_id(
            "rightcode".to_string(),
            "RightCode".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"aihubmix\"\n\n[model_providers.aihubmix]\nname = \"AIHubMix\"\nbase_url = \"https://example.com/v1\""
            }),
            None,
        );
        let mut official = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            serde_json::json!({"auth": {}, "config": "model_provider = \"openai\""}),
            None,
        );
        official.category = Some("official".to_string());

        db.save_provider("codex", &third_party)
            .expect("save third-party");
        db.save_provider("codex", &official).expect("save official");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(ids.contains("rightcode"));
        assert!(ids.contains("aihubmix"));
        assert!(!ids.contains("openai"));
        assert!(!ids.contains("codex-official"));
    }

    #[test]
    fn skips_unknown_provider_model_provider_id_from_existing_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "manual-aggregator".to_string(),
            "Manual Aggregator".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"my-private-relay\"\n\n[model_providers.my-private-relay]\nname = \"Manual Relay\"\nbase_url = \"http://localhost:8080/v1\""
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-private-relay"));
    }

    #[test]
    fn skips_undefined_provider_model_provider_id_from_existing_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "manual-aggregator".to_string(),
            "Manual Aggregator".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"my-private-relay\"\n"
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-private-relay"));
    }

    #[test]
    fn skips_unknown_profile_model_provider_id_from_existing_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "manual-aggregator".to_string(),
            "Manual Aggregator".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"profile = "work"

[model_providers.my-private-relay]
name = "Manual Relay"
base_url = "http://localhost:8080/v1"

[profiles.work]
model_provider = "my-private-relay"
"#
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-private-relay"));
    }

    #[test]
    fn collects_known_legacy_provider_id_from_normalized_preset_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "generated-uuid".to_string(),
            "AIHubMix".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"AIHubMix\"\nbase_url = \"https://aihubmix.example/v1\""
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(ids.contains("aihubmix"));
        assert!(!ids.contains("generated-uuid"));
    }

    #[test]
    fn collects_legacy_ccswitch_provider_id_from_stored_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "generated-uuid".to_string(),
            "Legacy Stable".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"ccswitch\"\n\n[model_providers.ccswitch]\nname = \"AIHubMix\"\nbase_url = \"https://aihubmix.example/v1\""
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(ids.contains("ccswitch"));
        assert!(ids.contains("aihubmix"));
        assert!(!ids.contains("generated-uuid"));
    }

    #[test]
    fn migrates_stored_provider_template_to_custom() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "legacy".to_string(),
            "Legacy Stable".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "aihubmix"
model = "gpt-5.4"
profile = "work"

[model_providers.aihubmix]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"

[profiles.work]
model_provider = "aihubmix"
model = "gpt-5.4"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, backup_dir) = migrate_provider_templates_for_test(&db);
        assert_eq!(outcome.migrated_provider_ids, vec!["legacy".to_string()]);

        let saved = db
            .get_provider_by_id("legacy", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert!(parsed
            .get("model_providers")
            .and_then(|value| value.get("aihubmix"))
            .is_none());
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("custom"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("https://aihubmix.example/v1")
        );
        assert_eq!(
            parsed
                .get("profiles")
                .and_then(|value| value.get("work"))
                .and_then(|value| value.get("model_provider"))
                .and_then(|value| value.as_str()),
            Some("custom")
        );

        let backups: Vec<_> = fs::read_dir(backup_dir.path().join("providers"))
            .expect("provider backups")
            .flatten()
            .collect();
        assert_eq!(backups.len(), 1);
        let backup_text = fs::read_to_string(backups[0].path()).expect("read provider backup");
        assert!(backup_text.contains(r#""providerId": "legacy""#));
        assert!(backup_text.contains(r#"model_provider = \"aihubmix\""#));

        let (second, _second_backup_dir) = migrate_provider_templates_for_test(&db);
        assert!(second.migrated_provider_ids.is_empty());
    }

    #[test]
    fn migrates_legacy_ccswitch_provider_template_to_custom() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "legacy-ccswitch".to_string(),
            "Legacy CC Switch".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "ccswitch"

[model_providers.ccswitch]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, _backup_dir) = migrate_provider_templates_for_test(&db);
        assert_eq!(
            outcome.migrated_provider_ids,
            vec!["legacy-ccswitch".to_string()]
        );

        let saved = db
            .get_provider_by_id("legacy-ccswitch", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert!(parsed
            .get("model_providers")
            .and_then(|value| value.get("ccswitch"))
            .is_none());
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("custom"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("https://aihubmix.example/v1")
        );
    }

    #[test]
    fn skips_unknown_stored_provider_template() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "manual".to_string(),
            "Manual Relay".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "my-private-relay"

[model_providers.my-private-relay]
name = "Manual Relay"
base_url = "http://localhost:8080/v1"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, _backup_dir) = migrate_provider_templates_for_test(&db);
        assert!(outcome.migrated_provider_ids.is_empty());

        let saved = db
            .get_provider_by_id("manual", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("my-private-relay")
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("my-private-relay"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("http://localhost:8080/v1")
        );
    }

    #[test]
    fn skips_reserved_key_in_non_official_stored_provider_template() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "custom-openai".to_string(),
            "Custom OpenAI".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "openai"

[model_providers.openai]
name = "Custom OpenAI"
base_url = "https://proxy.example/v1"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, _backup_dir) = migrate_provider_templates_for_test(&db);
        assert!(outcome.migrated_provider_ids.is_empty());

        let saved = db
            .get_provider_by_id("custom-openai", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("openai")
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("openai"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("https://proxy.example/v1")
        );
    }

    #[test]
    fn migrates_profile_model_provider_refs_to_custom_when_top_level_is_already_custom() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "profiled".to_string(),
            "Profiled Relay".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "custom"
profile = "work"

[model_providers.custom]
name = "Current"
base_url = "https://current.example/v1"

[model_providers.aihubmix]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"

[profiles.work]
model_provider = "aihubmix"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, _backup_dir) = migrate_provider_templates_for_test(&db);
        assert_eq!(outcome.migrated_provider_ids, vec!["profiled".to_string()]);

        let saved = db
            .get_provider_by_id("profiled", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("profiles")
                .and_then(|value| value.get("work"))
                .and_then(|value| value.get("model_provider"))
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("custom"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("https://current.example/v1")
        );
    }

    #[test]
    fn skips_custom_category_unknown_provider_when_created_by_cc_switch() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "generated-uuid".to_string(),
            "Manual Relay".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"my-private-relay\"\n\n[model_providers.my-private-relay]\nname = \"Manual Relay\"\nbase_url = \"http://localhost:8080/v1\""
            }),
            None,
        );
        provider.category = Some("custom".to_string());
        provider.created_at = Some(1);

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-private-relay"));
        assert!(!ids.contains("generated-uuid"));
    }

    #[test]
    fn skips_custom_category_unknown_provider_model_provider_id() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "manual".to_string(),
            "Manual Relay".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"my-local-relay\"\n\n[model_providers.my-local-relay]\nname = \"Manual Relay\"\nbase_url = \"http://localhost:8080/v1\""
            }),
            None,
        );
        provider.category = Some("custom".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-local-relay"));
    }
}
