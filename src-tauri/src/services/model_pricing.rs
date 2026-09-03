use crate::config::{atomic_write, get_app_config_dir};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Transaction};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};

const MODEL_PRICING_FILE_NAME: &str = "model-pricing.json";
const MODEL_PRICING_FILE_VERSION: u32 = 1;

static MODEL_PRICING_FILE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn file_lock() -> &'static Mutex<()> {
    MODEL_PRICING_FILE_LOCK.get_or_init(|| Mutex::new(()))
}

fn default_true() -> bool {
    true
}

fn default_file_version() -> u32 {
    MODEL_PRICING_FILE_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingInfo {
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDevSyncConfig {
    #[serde(default)]
    pub auto_sync_enabled: bool,
    #[serde(default = "default_true")]
    pub include_common_models: bool,
    #[serde(default)]
    pub selected_model_keys: Vec<String>,
    #[serde(default)]
    pub excluded_common_model_keys: Vec<String>,
    #[serde(default)]
    pub last_sync_at: Option<i64>,
    #[serde(default)]
    pub last_sync_error: Option<String>,
}

impl Default for ModelsDevSyncConfig {
    fn default() -> Self {
        Self {
            auto_sync_enabled: false,
            include_common_models: true,
            selected_model_keys: Vec::new(),
            excluded_common_model_keys: Vec::new(),
            last_sync_at: None,
            last_sync_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelPricingFile {
    #[serde(default = "default_file_version")]
    version: u32,
    #[serde(default)]
    models_dev_sync: ModelsDevSyncConfig,
    #[serde(default)]
    models: Vec<ModelPricingInfo>,
    #[serde(default)]
    deleted_model_ids: Vec<String>,
}

impl Default for ModelPricingFile {
    fn default() -> Self {
        Self {
            version: MODEL_PRICING_FILE_VERSION,
            models_dev_sync: ModelsDevSyncConfig::default(),
            models: Vec::new(),
            deleted_model_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDevSyncState {
    pub config: ModelsDevSyncConfig,
    pub config_path: String,
}

pub fn model_pricing_file_path() -> PathBuf {
    get_app_config_dir().join(MODEL_PRICING_FILE_NAME)
}

fn normalize_decimal(label: &str, value: &str) -> Result<String, AppError> {
    let value = value.trim();
    let parsed = Decimal::from_str(value).map_err(|error| {
        AppError::localized(
            "usage.invalidPrice",
            format!("{label} 价格无效: {value} - {error}"),
            format!("{label} price is invalid: {value} - {error}"),
        )
    })?;
    if parsed < Decimal::ZERO {
        return Err(AppError::localized(
            "usage.invalidPrice",
            format!("{label} 价格必须为非负数: {value}"),
            format!("{label} price must be non-negative: {value}"),
        ));
    }
    Ok(value.to_string())
}

fn normalize_pricing(entry: ModelPricingInfo) -> Result<ModelPricingInfo, AppError> {
    let model_id = entry.model_id.trim().to_string();
    let display_name = entry.display_name.trim().to_string();
    if model_id.is_empty() {
        return Err(AppError::localized(
            "usage.modelIdRequired",
            "模型 ID 不能为空",
            "Model ID is required",
        ));
    }
    if display_name.is_empty() {
        return Err(AppError::localized(
            "usage.displayNameRequired",
            "显示名称不能为空",
            "Display name is required",
        ));
    }

    Ok(ModelPricingInfo {
        model_id,
        display_name,
        input_cost_per_million: normalize_decimal("input_cost", &entry.input_cost_per_million)?,
        output_cost_per_million: normalize_decimal("output_cost", &entry.output_cost_per_million)?,
        cache_read_cost_per_million: normalize_decimal(
            "cache_read_cost",
            &entry.cache_read_cost_per_million,
        )?,
        cache_creation_cost_per_million: normalize_decimal(
            "cache_creation_cost",
            &entry.cache_creation_cost_per_million,
        )?,
    })
}

fn normalize_key_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_sync_config(mut config: ModelsDevSyncConfig) -> ModelsDevSyncConfig {
    config.selected_model_keys = normalize_key_list(config.selected_model_keys);
    config.excluded_common_model_keys = normalize_key_list(config.excluded_common_model_keys);
    config.last_sync_error = config.last_sync_error.and_then(|error| {
        let trimmed = error.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.chars().take(1000).collect())
        }
    });
    config
}

fn normalize_file(mut file: ModelPricingFile) -> Result<ModelPricingFile, AppError> {
    if file.version > MODEL_PRICING_FILE_VERSION {
        return Err(AppError::Config(format!(
            "model-pricing.json version {} is newer than supported version {}",
            file.version, MODEL_PRICING_FILE_VERSION
        )));
    }

    let deleted = normalize_key_list(file.deleted_model_ids)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut models = BTreeMap::new();
    for entry in file.models {
        let entry = normalize_pricing(entry)?;
        if !deleted.contains(&entry.model_id) {
            models.insert(entry.model_id.clone(), entry);
        }
    }

    file.version = MODEL_PRICING_FILE_VERSION;
    file.models_dev_sync = normalize_sync_config(file.models_dev_sync);
    file.models = models.into_values().collect();
    file.deleted_model_ids = deleted.into_iter().collect();
    Ok(file)
}

fn read_file_unlocked() -> Result<Option<ModelPricingFile>, AppError> {
    let path = model_pricing_file_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    let file = serde_json::from_str(&content).map_err(|error| AppError::json(&path, error))?;
    normalize_file(file).map(Some)
}

fn write_file_unlocked(file: &ModelPricingFile) -> Result<(), AppError> {
    let path = model_pricing_file_path();
    let mut data = serde_json::to_vec_pretty(file)
        .map_err(|error| AppError::Config(format!("序列化模型定价配置失败: {error}")))?;
    data.push(b'\n');
    atomic_write(&path, &data)
}

fn load_or_create_file_unlocked() -> Result<ModelPricingFile, AppError> {
    if let Some(file) = read_file_unlocked()? {
        return Ok(file);
    }

    // The local file stores user/models.dev overrides only. Exporting the
    // complete seeded table here would turn built-in prices into overrides and
    // roll back future repair_current_model_pricing corrections on startup.
    let file = ModelPricingFile::default();
    write_file_unlocked(&file)?;
    Ok(file)
}

fn upsert_pricing(
    transaction: &Transaction<'_>,
    entry: &ModelPricingInfo,
) -> Result<usize, AppError> {
    transaction
        .execute(
            "INSERT INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(model_id) DO UPDATE SET
                display_name = excluded.display_name,
                input_cost_per_million = excluded.input_cost_per_million,
                output_cost_per_million = excluded.output_cost_per_million,
                cache_read_cost_per_million = excluded.cache_read_cost_per_million,
                cache_creation_cost_per_million = excluded.cache_creation_cost_per_million
            WHERE display_name <> excluded.display_name
               OR input_cost_per_million <> excluded.input_cost_per_million
               OR output_cost_per_million <> excluded.output_cost_per_million
               OR cache_read_cost_per_million <> excluded.cache_read_cost_per_million
               OR cache_creation_cost_per_million <> excluded.cache_creation_cost_per_million",
            params![
                entry.model_id,
                entry.display_name,
                entry.input_cost_per_million,
                entry.output_cost_per_million,
                entry.cache_read_cost_per_million,
                entry.cache_creation_cost_per_million
            ],
        )
        .map_err(|error| AppError::Database(format!("更新模型定价失败: {error}")))
}

fn apply_file_to_database(
    db: &Database,
    file: &ModelPricingFile,
) -> Result<(usize, usize), AppError> {
    let mut conn = lock_conn!(db.conn);
    let transaction = conn.transaction()?;
    let mut upserted = 0;
    for entry in &file.models {
        upserted += upsert_pricing(&transaction, entry)?;
    }
    let mut deleted = 0;
    for model_id in &file.deleted_model_ids {
        deleted += transaction.execute(
            "DELETE FROM model_pricing WHERE model_id = ?1",
            params![model_id],
        )?;
    }
    transaction.commit()?;
    Ok((upserted, deleted))
}

/// Load user-maintained overrides from `~/.cc-switch/model-pricing.json`.
/// Built-in rows remain database-owned so application updates can repair them;
/// the file contains only explicit overrides and deletion tombstones.
pub fn sync_local_model_pricing(db: &Database) -> Result<usize, AppError> {
    let (upserted, deleted) = {
        let _file_guard = file_lock()
            .lock()
            .map_err(|error| AppError::Config(format!("模型定价文件锁失败: {error}")))?;
        let file = load_or_create_file_unlocked()?;
        apply_file_to_database(db, &file)?
    };

    // Deleting pricing cannot make a zero-cost usage row calculable. In
    // particular, seeded rows covered by tombstones may be reinserted and
    // deleted on every startup; they must not trigger a full-table backfill.
    if upserted > 0 {
        if let Err(error) = db.backfill_missing_usage_costs() {
            log::warn!("本地模型定价同步后回填历史用量成本失败: {error}");
        }
    }
    Ok(upserted + deleted)
}

pub fn get_models_dev_sync_state(db: &Database) -> Result<ModelsDevSyncState, AppError> {
    sync_local_model_pricing(db)?;
    let _file_guard = file_lock()
        .lock()
        .map_err(|error| AppError::Config(format!("模型定价文件锁失败: {error}")))?;
    let file = load_or_create_file_unlocked()?;
    Ok(ModelsDevSyncState {
        config: file.models_dev_sync,
        config_path: model_pricing_file_path().display().to_string(),
    })
}

pub fn save_models_dev_sync_config(
    db: &Database,
    config: ModelsDevSyncConfig,
) -> Result<(), AppError> {
    sync_local_model_pricing(db)?;
    let _file_guard = file_lock()
        .lock()
        .map_err(|error| AppError::Config(format!("模型定价文件锁失败: {error}")))?;
    let mut file = load_or_create_file_unlocked()?;
    file.models_dev_sync = normalize_sync_config(config);
    write_file_unlocked(&file)
}

/// Persist only the outcome of a models.dev sync. Keeping this separate from
/// `save_models_dev_sync_config` prevents a slow startup fetch from restoring
/// stale switches or model selections that the user changed in the meantime.
pub fn record_models_dev_sync_result(
    db: &Database,
    synced_at: Option<i64>,
    error: Option<String>,
) -> Result<(), AppError> {
    sync_local_model_pricing(db)?;
    let _file_guard = file_lock()
        .lock()
        .map_err(|lock_error| AppError::Config(format!("模型定价文件锁失败: {lock_error}")))?;
    let mut file = load_or_create_file_unlocked()?;
    if let Some(synced_at) = synced_at {
        file.models_dev_sync.last_sync_at = Some(synced_at);
    }
    file.models_dev_sync.last_sync_error = error;
    file.models_dev_sync = normalize_sync_config(file.models_dev_sync);
    write_file_unlocked(&file)
}

fn update_model_pricing_batch_inner(
    db: &Database,
    entries: Vec<ModelPricingInfo>,
    backfill_all: bool,
) -> Result<usize, AppError> {
    if entries.is_empty() {
        return Ok(0);
    }
    let mut normalized = BTreeMap::new();
    for entry in entries {
        let entry = normalize_pricing(entry)?;
        normalized.insert(entry.model_id.clone(), entry);
    }
    let entries = normalized.into_values().collect::<Vec<_>>();
    let model_ids = entries
        .iter()
        .map(|entry| entry.model_id.clone())
        .collect::<Vec<_>>();

    sync_local_model_pricing(db)?;
    let changed = {
        let _file_guard = file_lock()
            .lock()
            .map_err(|error| AppError::Config(format!("模型定价文件锁失败: {error}")))?;
        let mut file = load_or_create_file_unlocked()?;
        let mut file_models = file
            .models
            .into_iter()
            .map(|entry| (entry.model_id.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        let updated_ids = entries
            .iter()
            .map(|entry| entry.model_id.clone())
            .collect::<BTreeSet<_>>();
        for entry in &entries {
            file_models.insert(entry.model_id.clone(), entry.clone());
        }
        file.models = file_models.into_values().collect();
        file.deleted_model_ids
            .retain(|model_id| !updated_ids.contains(model_id));

        let mut conn = lock_conn!(db.conn);
        let transaction = conn.transaction()?;
        let mut changed = 0;
        for entry in &entries {
            changed += upsert_pricing(&transaction, entry)?;
        }
        write_file_unlocked(&file)?;
        transaction.commit()?;
        changed
    };

    if changed > 0 {
        if backfill_all {
            if let Err(error) = db.backfill_missing_usage_costs() {
                log::warn!("批量更新模型定价后回填历史用量成本失败: {error}");
            }
        } else {
            for model_id in model_ids {
                if let Err(error) = db.backfill_missing_usage_costs_for_model(&model_id) {
                    log::warn!("模型定价更新后回填历史用量成本失败 (model_id={model_id}): {error}");
                }
            }
        }
    }
    Ok(changed)
}

pub fn update_model_pricing(db: &Database, entry: ModelPricingInfo) -> Result<usize, AppError> {
    update_model_pricing_batch_inner(db, vec![entry], false)
}

pub fn update_model_pricing_batch(
    db: &Database,
    entries: Vec<ModelPricingInfo>,
) -> Result<usize, AppError> {
    update_model_pricing_batch_inner(db, entries, true)
}

pub fn delete_model_pricing(db: &Database, model_id: &str) -> Result<(), AppError> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return Err(AppError::localized(
            "usage.modelIdRequired",
            "模型 ID 不能为空",
            "Model ID is required",
        ));
    }

    sync_local_model_pricing(db)?;
    let _file_guard = file_lock()
        .lock()
        .map_err(|error| AppError::Config(format!("模型定价文件锁失败: {error}")))?;
    let mut file = load_or_create_file_unlocked()?;
    file.models.retain(|entry| entry.model_id != model_id);
    if !file.deleted_model_ids.iter().any(|entry| entry == model_id) {
        file.deleted_model_ids.push(model_id.to_string());
        file.deleted_model_ids.sort();
    }

    let mut conn = lock_conn!(db.conn);
    let transaction = conn.transaction()?;
    transaction.execute(
        "DELETE FROM model_pricing WHERE model_id = ?1",
        params![model_id],
    )?;
    write_file_unlocked(&file)?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn with_test_home(test: impl FnOnce(&Database, &PathBuf)) {
        let temp = tempfile::tempdir().expect("tempdir");
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let db = Database::memory().expect("memory database");
        let path = model_pricing_file_path();
        test(&db, &path);

        match previous {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    fn sample_pricing() -> ModelPricingInfo {
        ModelPricingInfo {
            model_id: "custom-model".to_string(),
            display_name: "Custom Model".to_string(),
            input_cost_per_million: "1.25".to_string(),
            output_cost_per_million: "5".to_string(),
            cache_read_cost_per_million: "0.1".to_string(),
            cache_creation_cost_per_million: "1.5".to_string(),
        }
    }

    #[test]
    #[serial]
    fn creates_local_file_with_auto_sync_disabled_by_default() {
        with_test_home(|db, path| {
            let state = get_models_dev_sync_state(db).expect("sync state");
            assert!(path.exists());
            assert!(!state.config.auto_sync_enabled);
            assert!(state.config.include_common_models);
            assert_eq!(state.config_path, path.display().to_string());

            let content = fs::read_to_string(path).expect("read pricing file");
            let file: ModelPricingFile = serde_json::from_str(&content).expect("parse file");
            assert!(file.models.is_empty());
        });
    }

    #[test]
    #[serial]
    fn empty_override_file_does_not_roll_back_builtin_pricing_repairs() {
        with_test_home(|db, path| {
            get_models_dev_sync_state(db).expect("create override file");
            {
                let conn = db.conn.lock().expect("lock test database");
                assert_eq!(
                    conn.execute(
                        "UPDATE model_pricing
                         SET input_cost_per_million = '99'
                         WHERE model_id = 'claude-sonnet-5'",
                        [],
                    )
                    .expect("simulate built-in pricing repair"),
                    1
                );
            }

            assert_eq!(sync_local_model_pricing(db).expect("reload overrides"), 0);

            let conn = db.conn.lock().expect("lock test database");
            let input: String = conn
                .query_row(
                    "SELECT input_cost_per_million
                     FROM model_pricing WHERE model_id = 'claude-sonnet-5'",
                    [],
                    |row| row.get(0),
                )
                .expect("query repaired pricing");
            drop(conn);
            assert_eq!(input, "99");

            let content = fs::read_to_string(path).expect("read override file");
            let file: ModelPricingFile = serde_json::from_str(&content).expect("parse file");
            assert!(file.models.is_empty());
        });
    }

    #[test]
    #[serial]
    fn models_dev_batch_sync_overwrites_existing_manual_pricing() {
        with_test_home(|db, path| {
            let mut manual = sample_pricing();
            manual.input_cost_per_million = "9".to_string();
            manual.output_cost_per_million = "18".to_string();
            update_model_pricing(db, manual).expect("save manual pricing");

            let synced = sample_pricing();
            update_model_pricing_batch(db, vec![synced.clone()]).expect("sync models.dev pricing");

            let conn = db.conn.lock().expect("lock test database");
            let input: String = conn
                .query_row(
                    "SELECT input_cost_per_million FROM model_pricing WHERE model_id = ?1",
                    params!["custom-model"],
                    |row| row.get(0),
                )
                .expect("query synced pricing");
            drop(conn);
            assert_eq!(input, synced.input_cost_per_million);

            let content = fs::read_to_string(path).expect("read pricing file");
            let file: ModelPricingFile = serde_json::from_str(&content).expect("parse file");
            let saved = file
                .models
                .iter()
                .find(|entry| entry.model_id == "custom-model")
                .expect("saved synced pricing");
            assert_eq!(saved, &synced);
        });
    }

    #[test]
    #[serial]
    fn batch_update_and_delete_are_persisted_to_local_file() {
        with_test_home(|db, path| {
            assert_eq!(
                update_model_pricing_batch(db, vec![sample_pricing()]).expect("batch update"),
                1
            );
            let content = fs::read_to_string(path).expect("read pricing file");
            let file: ModelPricingFile = serde_json::from_str(&content).expect("parse file");
            assert!(file
                .models
                .iter()
                .any(|entry| entry.model_id == "custom-model"));

            delete_model_pricing(db, "custom-model").expect("delete pricing");
            let content = fs::read_to_string(path).expect("read updated file");
            let file: ModelPricingFile =
                serde_json::from_str(&content).expect("parse updated file");
            assert!(!file
                .models
                .iter()
                .any(|entry| entry.model_id == "custom-model"));
            assert!(file
                .deleted_model_ids
                .iter()
                .any(|entry| entry == "custom-model"));
        });
    }

    #[test]
    #[serial]
    fn reloads_manual_file_edits_and_deletion_tombstones() {
        with_test_home(|db, path| {
            get_models_dev_sync_state(db).expect("create pricing file");
            let content = fs::read_to_string(path).expect("read pricing file");
            let mut file: ModelPricingFile =
                serde_json::from_str(&content).expect("parse pricing file");
            file.models.push(sample_pricing());
            fs::write(
                path,
                serde_json::to_vec_pretty(&file).expect("serialize file"),
            )
            .expect("write manual edit");

            assert_eq!(sync_local_model_pricing(db).expect("reload file"), 1);
            {
                let conn = db.conn.lock().expect("lock test database");
                let input: String = conn
                    .query_row(
                        "SELECT input_cost_per_million FROM model_pricing WHERE model_id = ?1",
                        params!["custom-model"],
                        |row| row.get(0),
                    )
                    .expect("query manually added pricing");
                assert_eq!(input, "1.25");
            }

            let content = fs::read_to_string(path).expect("read updated pricing file");
            let mut file: ModelPricingFile =
                serde_json::from_str(&content).expect("parse updated pricing file");
            file.deleted_model_ids.push("custom-model".to_string());
            fs::write(
                path,
                serde_json::to_vec_pretty(&file).expect("serialize tombstone"),
            )
            .expect("write tombstone");

            assert_eq!(sync_local_model_pricing(db).expect("apply tombstone"), 1);
            let conn = db.conn.lock().expect("lock test database");
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM model_pricing WHERE model_id = ?1",
                    params!["custom-model"],
                    |row| row.get(0),
                )
                .expect("query deleted pricing");
            assert_eq!(count, 0);
        });
    }

    #[test]
    #[serial]
    fn repeated_seeded_tombstone_deletion_does_not_backfill_unrelated_usage() {
        with_test_home(|db, _path| {
            get_models_dev_sync_state(db).expect("create override file");
            {
                let conn = db.conn.lock().expect("lock test database");
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, request_model,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        input_cost_usd, output_cost_usd, cache_read_cost_usd,
                        cache_creation_cost_usd, total_cost_usd, latency_ms,
                        status_code, created_at, data_source
                    ) VALUES (
                        'pending-cost', 'test-provider', 'codex', 'gpt-5', 'gpt-5',
                        1000000, 0, 0, 0, '0', '0', '0', '0', '0', 100, 200, 1, 'proxy'
                    )",
                    [],
                )
                .expect("insert zero-cost usage");
            }

            delete_model_pricing(db, "claude-sonnet-5").expect("create tombstone");
            db.ensure_model_pricing_seeded()
                .expect("reseed built-in pricing");
            assert_eq!(sync_local_model_pricing(db).expect("apply tombstone"), 1);

            let conn = db.conn.lock().expect("lock test database");
            let deleted_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM model_pricing
                     WHERE model_id = 'claude-sonnet-5'",
                    [],
                    |row| row.get(0),
                )
                .expect("query tombstoned pricing");
            let total_cost: f64 = conn
                .query_row(
                    "SELECT CAST(total_cost_usd AS REAL)
                     FROM proxy_request_logs WHERE request_id = 'pending-cost'",
                    [],
                    |row| row.get(0),
                )
                .expect("query pending usage cost");
            assert_eq!(deleted_count, 0);
            assert_eq!(total_cost, 0.0);
        });
    }

    #[test]
    #[serial]
    fn recording_sync_result_preserves_user_selection_and_switches() {
        with_test_home(|db, _path| {
            let config = ModelsDevSyncConfig {
                auto_sync_enabled: false,
                include_common_models: false,
                selected_model_keys: vec!["relay/custom-model".to_string()],
                excluded_common_model_keys: vec!["openai/gpt-5".to_string()],
                last_sync_at: Some(123),
                last_sync_error: Some("old error".to_string()),
            };
            save_models_dev_sync_config(db, config.clone()).expect("save sync config");

            record_models_dev_sync_result(db, Some(456), None).expect("record success");
            let state = get_models_dev_sync_state(db).expect("read sync state");
            assert_eq!(state.config.auto_sync_enabled, config.auto_sync_enabled);
            assert_eq!(
                state.config.include_common_models,
                config.include_common_models
            );
            assert_eq!(state.config.selected_model_keys, config.selected_model_keys);
            assert_eq!(
                state.config.excluded_common_model_keys,
                config.excluded_common_model_keys
            );
            assert_eq!(state.config.last_sync_at, Some(456));
            assert_eq!(state.config.last_sync_error, None);

            record_models_dev_sync_result(db, None, Some("offline".to_string()))
                .expect("record failure");
            let state = get_models_dev_sync_state(db).expect("read failure state");
            assert_eq!(state.config.last_sync_at, Some(456));
            assert_eq!(state.config.last_sync_error.as_deref(), Some("offline"));
        });
    }
}
