use std::path::PathBuf;

use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use serde_json::Value;
use toml_edit::DocumentMut;

pub const KIMI_PROVIDER_NAME: &str = "ccswitch";

/// 获取 Kimi 配置目录路径（支持设置覆盖）
pub fn get_kimi_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_kimi_override_dir() {
        return custom;
    }
    get_home_dir().join(".kimi")
}

/// 获取 Kimi config.toml 路径
pub fn get_kimi_config_path() -> PathBuf {
    get_kimi_dir().join("config.toml")
}

/// 读取 Kimi config.toml，若不存在返回 None
pub fn read_kimi_config() -> Result<Option<DocumentMut>, AppError> {
    let path = get_kimi_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let doc = content
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Kimi config.toml: {e}")))?;
    Ok(Some(doc))
}

/// 原子写入 Kimi config.toml
pub fn write_kimi_config(doc: &DocumentMut) -> Result<(), AppError> {
    let path = get_kimi_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    write_text_file(&path, &doc.to_string())
}

/// 生成最小默认 Kimi 配置
fn build_default_kimi_config(base_url: &str, api_key: &str) -> DocumentMut {
    let toml_str = format!(
        r#"default_model = "kimi-for-coding"

[providers.{KIMI_PROVIDER_NAME}]
type = "kimi"
base_url = "{base_url}"
api_key = "{api_key}"

[models.kimi-for-coding]
provider = "{KIMI_PROVIDER_NAME}"
model = "kimi-for-coding"
max_context_size = 262144
"#
    );
    toml_str
        .parse::<DocumentMut>()
        .expect("default kimi config should always be valid toml")
}

/// 核心：将 provider 配置写入 [providers.ccswitch]
pub fn write_kimi_live(base_url: &str, api_key: &str) -> Result<(), AppError> {
    let mut doc = match read_kimi_config()? {
        Some(doc) => doc,
        None => build_default_kimi_config(base_url, api_key),
    };

    // Ensure [providers] table exists
    if doc.get("providers").is_none() {
        doc["providers"] = toml_edit::table();
    }

    if let Some(providers) = doc["providers"].as_table_mut() {
        if !providers.contains_key(KIMI_PROVIDER_NAME) {
            providers[KIMI_PROVIDER_NAME] = toml_edit::table();
        }
        if let Some(provider_table) = providers[KIMI_PROVIDER_NAME].as_table_mut() {
            provider_table["type"] = toml_edit::value("kimi");
            provider_table["base_url"] = toml_edit::value(base_url);
            provider_table["api_key"] = toml_edit::value(api_key);
            // Clear any existing OAuth credentials so API key takes precedence
            provider_table.remove("oauth");
        }
    }

    write_kimi_config(&doc)?;
    Ok(())
}

/// 从 Provider.settings_config 提取 env 键值对
pub fn json_to_env(settings: &Value) -> Result<std::collections::HashMap<String, String>, AppError> {
    let mut env_map = std::collections::HashMap::new();
    if let Some(env_obj) = settings.get("env").and_then(|v| v.as_object()) {
        for (key, value) in env_obj {
            if let Some(val_str) = value.as_str() {
                env_map.insert(key.clone(), val_str.to_string());
            }
        }
    }
    Ok(env_map)
}

/// 验证 Kimi 配置格式
pub fn validate_kimi_settings(settings: &Value) -> Result<(), AppError> {
    if let Some(env) = settings.get("env") {
        if !env.is_object() {
            return Err(AppError::localized(
                "kimi.validation.invalid_env",
                "Kimi 配置格式错误: env 必须是对象",
                "Kimi config invalid: env must be an object",
            ));
        }
    }
    Ok(())
}

/// 严格验证 Kimi 配置（切换时使用）
pub fn validate_kimi_settings_strict(settings: &Value) -> Result<(), AppError> {
    validate_kimi_settings(settings)?;
    let env_map = json_to_env(settings)?;
    if !env_map.contains_key("KIMI_BASE_URL") {
        return Err(AppError::localized(
            "kimi.validation.missing_base_url",
            "Kimi 配置缺少必需字段: KIMI_BASE_URL",
            "Kimi config missing required field: KIMI_BASE_URL",
        ));
    }
    Ok(())
}
