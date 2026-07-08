//! ZCode 配置文件读写
//!
//! ZCode 的 provider 配置位于 `~/.zcode/v2/config.json`，顶层 `provider` 对象
//! 存放多个 provider（additive mode）。与 OpenCode 不同，ZCode 使用 `kind`
//! 字段（anthropic / openai / openai-compatible）而非 AI SDK 包名，且 provider
//! 额外有 `enabled` / `source` 字段。本模块只读写 `provider` 键，保留
//! config.json 顶层其他键（如未来新增的配置）不被动到。

use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::ZCodeProviderConfig;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

/// 获取 ZCode 配置目录（`~/.zcode/v2`）。
///
/// 优先级：settings.zcode_config_dir 覆盖 > 默认 `~/.zcode/v2`。
pub fn get_zcode_dir() -> PathBuf {
    if let Some(override_dir) = crate::settings::get_zcode_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir().join(".zcode").join("v2")
}

/// 获取 ZCode provider 配置文件路径。
pub fn get_zcode_config_path() -> PathBuf {
    get_zcode_dir().join("config.json")
}

/// 获取 ZCode CLI 使用统计数据库路径（`~/.zcode/cli/db/db.sqlite`）。
pub fn get_zcode_usage_db_path() -> PathBuf {
    crate::config::get_home_dir()
        .join(".zcode")
        .join("cli")
        .join("db")
        .join("db.sqlite")
}

/// 读取 ZCode config.json（不存在则返回空对象）。
pub fn read_zcode_config() -> Result<Value, AppError> {
    let path = get_zcode_config_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str::<Value>(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse ZCode config: {}: {e}",
            path.display()
        ))
    })
}

/// 写入 ZCode config.json（保留顶层其他键，仅整体序列化）。
pub fn write_zcode_config(config: &Value) -> Result<(), AppError> {
    let path = get_zcode_config_path();
    write_json_file(&path, config)?;

    log::debug!("ZCode config written to {path:?}");
    Ok(())
}

/// 获取 `provider` 对象（键为 provider id，值为 provider 配置）。
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_zcode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置单个 provider（按 id 插入/覆盖，保留其他 provider 与顶层键）。
pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_zcode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_zcode_config(&full_config)
}

/// 移除单个 provider（保留其他 provider 与顶层键）。
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_zcode_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_zcode_config(&config)
}

/// 以强类型结构读取所有 provider（解析失败的条目会被跳过并记录警告）。
pub fn get_typed_providers() -> Result<IndexMap<String, ZCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<ZCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse ZCode provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

/// 以强类型结构写入单个 provider。
pub fn set_typed_provider(id: &str, config: &ZCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}
