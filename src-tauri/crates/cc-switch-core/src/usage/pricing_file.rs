use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, OnceLock};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{CoreError, HeadlessState};

use super::{mutation, ModelPricingInfo, ModelsDevSyncConfig, ModelsDevSyncState, PricingUpdate};

const FILE_NAME: &str = "model-pricing.json";
const FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelPricingFile {
    #[serde(default = "default_version")]
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
            version: FILE_VERSION,
            models_dev_sync: ModelsDevSyncConfig::default(),
            models: Vec::new(),
            deleted_model_ids: Vec::new(),
        }
    }
}

fn default_version() -> u32 {
    FILE_VERSION
}

fn file_guard() -> Result<MutexGuard<'static, ()>, CoreError> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| CoreError::StatePoisoned)
}

fn file_path(state: &HeadlessState) -> PathBuf {
    state.home().join(".cc-switch").join(FILE_NAME)
}

fn normalize_keys(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_config(mut config: ModelsDevSyncConfig) -> ModelsDevSyncConfig {
    config.selected_model_keys = normalize_keys(config.selected_model_keys);
    config.excluded_common_model_keys = normalize_keys(config.excluded_common_model_keys);
    config.last_sync_error = config.last_sync_error.and_then(|error| {
        let error = error.trim();
        (!error.is_empty()).then(|| error.chars().take(1000).collect())
    });
    config
}

fn normalize_price(label: &str, value: &str) -> Result<String, CoreError> {
    let value = value.trim();
    let parsed = Decimal::from_str(value)
        .map_err(|error| CoreError::InvalidPricing(format!("{label}: {error}")))?;
    if parsed < Decimal::ZERO {
        return Err(CoreError::InvalidPricing(format!("{label} 必须为非负数")));
    }
    Ok(value.to_string())
}

fn normalize_entry(entry: ModelPricingInfo) -> Result<ModelPricingInfo, CoreError> {
    let model_id = entry.model_id.trim().to_string();
    let display_name = entry.display_name.trim().to_string();
    if model_id.is_empty() || display_name.is_empty() {
        return Err(CoreError::InvalidPricing(
            "模型 ID 与显示名称不能为空".to_string(),
        ));
    }
    Ok(ModelPricingInfo {
        model_id,
        display_name,
        input_cost_per_million: normalize_price("input_cost", &entry.input_cost_per_million)?,
        output_cost_per_million: normalize_price("output_cost", &entry.output_cost_per_million)?,
        cache_read_cost_per_million: normalize_price(
            "cache_read_cost",
            &entry.cache_read_cost_per_million,
        )?,
        cache_creation_cost_per_million: normalize_price(
            "cache_creation_cost",
            &entry.cache_creation_cost_per_million,
        )?,
    })
}

fn normalize_file(mut file: ModelPricingFile) -> Result<ModelPricingFile, CoreError> {
    if file.version > FILE_VERSION {
        return Err(CoreError::InvalidPricing(format!(
            "model-pricing.json 版本 {} 高于当前支持版本 {FILE_VERSION}",
            file.version
        )));
    }
    let deleted = normalize_keys(file.deleted_model_ids)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut models = BTreeMap::new();
    for entry in file.models {
        let entry = normalize_entry(entry)?;
        if !deleted.contains(&entry.model_id) {
            models.insert(entry.model_id.clone(), entry);
        }
    }
    file.version = FILE_VERSION;
    file.models_dev_sync = normalize_config(file.models_dev_sync);
    file.models = models.into_values().collect();
    file.deleted_model_ids = deleted.into_iter().collect();
    Ok(file)
}

fn load_or_create(state: &HeadlessState) -> Result<ModelPricingFile, CoreError> {
    let path = file_path(state);
    if path.exists() {
        let bytes = fs::read(&path).map_err(|source| CoreError::Io {
            path: path.clone(),
            source,
        })?;
        return normalize_file(serde_json::from_slice(&bytes)?);
    }
    let file = ModelPricingFile::default();
    write_file(state, &file)?;
    Ok(file)
}

fn write_file(state: &HeadlessState, file: &ModelPricingFile) -> Result<(), CoreError> {
    let path = file_path(state);
    let parent = path.parent().expect("定价文件固定包含父目录");
    fs::create_dir_all(parent).map_err(|source| CoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|source| CoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut data = serde_json::to_vec_pretty(file)?;
    data.push(b'\n');
    temp.write_all(&data).map_err(|source| CoreError::Io {
        path: path.clone(),
        source,
    })?;
    temp.as_file().sync_all().map_err(|source| CoreError::Io {
        path: path.clone(),
        source,
    })?;
    temp.persist(&path).map_err(|error| CoreError::Io {
        path,
        source: error.error,
    })?;
    Ok(())
}

fn as_update(entry: &ModelPricingInfo) -> PricingUpdate {
    PricingUpdate {
        model_id: entry.model_id.clone(),
        display_name: entry.display_name.clone(),
        input_cost: entry.input_cost_per_million.clone(),
        output_cost: entry.output_cost_per_million.clone(),
        cache_read_cost: entry.cache_read_cost_per_million.clone(),
        cache_creation_cost: entry.cache_creation_cost_per_million.clone(),
    }
}

fn apply_to_database(state: &HeadlessState, file: &ModelPricingFile) -> Result<(), CoreError> {
    state.with_connection(|connection| {
        mutation::update_pricing_batch(connection, file.models.iter().map(as_update).collect())?;
        for model_id in &file.deleted_model_ids {
            mutation::delete_pricing(connection, model_id)?;
        }
        Ok(())
    })
}

/// 文件先落盘、数据库后更新；若数据库暂时 busy，下一次读取会从文件重放，
/// 因而不会出现数据库成功但用户覆盖丢失的不可恢复状态。
pub(super) fn sync_to_database(state: &HeadlessState) -> Result<(), CoreError> {
    let _guard = file_guard()?;
    let file = load_or_create(state)?;
    apply_to_database(state, &file)
}

pub(super) fn update_pricing(
    state: &HeadlessState,
    input: PricingUpdate,
) -> Result<usize, CoreError> {
    update_pricing_batch(
        state,
        vec![ModelPricingInfo {
            model_id: input.model_id,
            display_name: input.display_name,
            input_cost_per_million: input.input_cost,
            output_cost_per_million: input.output_cost,
            cache_read_cost_per_million: input.cache_read_cost,
            cache_creation_cost_per_million: input.cache_creation_cost,
        }],
    )
}

pub(super) fn update_pricing_batch(
    state: &HeadlessState,
    entries: Vec<ModelPricingInfo>,
) -> Result<usize, CoreError> {
    if entries.is_empty() {
        return Ok(0);
    }
    let mut normalized = BTreeMap::new();
    for entry in entries {
        let entry = normalize_entry(entry)?;
        normalized.insert(entry.model_id.clone(), entry);
    }
    let entries = normalized.into_values().collect::<Vec<_>>();
    let updated_ids = entries
        .iter()
        .map(|entry| entry.model_id.clone())
        .collect::<BTreeSet<_>>();

    let _guard = file_guard()?;
    let mut file = load_or_create(state)?;
    apply_to_database(state, &file)?;
    let mut file_models = file
        .models
        .into_iter()
        .map(|entry| (entry.model_id.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    for entry in &entries {
        file_models.insert(entry.model_id.clone(), entry.clone());
    }
    file.models = file_models.into_values().collect();
    file.deleted_model_ids
        .retain(|model_id| !updated_ids.contains(model_id));
    write_file(state, &file)?;

    state.with_connection(|connection| {
        mutation::update_pricing_batch(connection, entries.iter().map(as_update).collect())
    })
}

pub(super) fn delete_pricing(state: &HeadlessState, model_id: &str) -> Result<(), CoreError> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return Err(CoreError::InvalidPricing("模型 ID 不能为空".to_string()));
    }
    let _guard = file_guard()?;
    let mut file = load_or_create(state)?;
    apply_to_database(state, &file)?;
    file.models.retain(|entry| entry.model_id != model_id);
    if !file.deleted_model_ids.iter().any(|id| id == model_id) {
        file.deleted_model_ids.push(model_id.to_string());
        file.deleted_model_ids.sort();
    }
    write_file(state, &file)?;
    state.with_connection(|connection| mutation::delete_pricing(connection, model_id))
}

pub(super) fn models_dev_sync_state(
    state: &HeadlessState,
) -> Result<ModelsDevSyncState, CoreError> {
    let _guard = file_guard()?;
    let file = load_or_create(state)?;
    apply_to_database(state, &file)?;
    Ok(ModelsDevSyncState {
        config: file.models_dev_sync,
        config_path: file_path(state).display().to_string(),
    })
}

pub(super) fn save_models_dev_sync_config(
    state: &HeadlessState,
    config: ModelsDevSyncConfig,
) -> Result<(), CoreError> {
    let _guard = file_guard()?;
    let mut file = load_or_create(state)?;
    apply_to_database(state, &file)?;
    file.models_dev_sync = normalize_config(config);
    write_file(state, &file)
}

pub(super) fn record_models_dev_sync_result(
    state: &HeadlessState,
    synced_at: Option<i64>,
    error: Option<String>,
) -> Result<(), CoreError> {
    let _guard = file_guard()?;
    let mut file = load_or_create(state)?;
    apply_to_database(state, &file)?;
    if let Some(synced_at) = synced_at {
        file.models_dev_sync.last_sync_at = Some(synced_at);
    }
    file.models_dev_sync.last_sync_error = error;
    file.models_dev_sync = normalize_config(file.models_dev_sync);
    write_file(state, &file)
}
