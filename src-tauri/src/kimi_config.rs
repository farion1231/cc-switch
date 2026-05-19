use std::path::PathBuf;

use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use serde_json::Value;
use toml_edit::DocumentMut;

pub const KIMI_DEFAULT_PROVIDER_NAME: &str = "ccswitch";

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
fn build_default_kimi_config(base_url: &str, api_key: &str, provider_name: &str) -> DocumentMut {
    let toml_str = format!(
        r#"default_model = "kimi-code/kimi-for-coding"

[providers.{provider_name}]
type = "kimi"
base_url = "{base_url}"
api_key = "{api_key}"

[models."kimi-code/kimi-for-coding"]
provider = "{provider_name}"
model = "kimi-for-coding"
max_context_size = 262144
display_name = "Kimi-k2.6"
capabilities = ["thinking", "video_in", "image_in"]
"#
    );
    toml_str
        .parse::<DocumentMut>()
        .expect("default kimi config should always be valid toml")
}

/// 核心：将 provider 配置写入 [providers.{provider_name}]
/// 同时更新 models 中的 provider 指向当前激活的 provider
pub fn write_kimi_live(base_url: &str, api_key: &str, provider_name: &str) -> Result<(), AppError> {
    let mut doc = match read_kimi_config()? {
        Some(doc) => doc,
        None => build_default_kimi_config(base_url, api_key, provider_name),
    };

    // Ensure [providers] table exists
    if doc.get("providers").is_none() {
        doc["providers"] = toml_edit::table();
    }

    if let Some(providers) = doc["providers"].as_table_mut() {
        if !providers.contains_key(provider_name) {
            providers[provider_name] = toml_edit::table();
        }
        if let Some(provider_table) = providers[provider_name].as_table_mut() {
            provider_table["type"] = toml_edit::value("kimi");
            provider_table["base_url"] = toml_edit::value(base_url);
            provider_table["api_key"] = toml_edit::value(api_key);
            // Clear any existing OAuth credentials so API key takes precedence
            provider_table.remove("oauth");
        }
        // Also clear OAuth from ALL other providers (e.g. managed:kimi-code created by Kimi CLI)
        // to prevent Kimi CLI from auto-switching back to OAuth authentication.
        for (key, value) in providers.iter_mut() {
            if key.get() != provider_name {
                if let Some(other_table) = value.as_table_mut() {
                    other_table.remove("oauth");
                }
            }
        }
    }

    // Ensure default_model points to the correct model
    if doc.get("default_model").is_none() {
        doc["default_model"] = toml_edit::value("kimi-code/kimi-for-coding");
    }

    // Update model provider reference to point to the active provider
    if doc.get("models").is_none() {
        doc["models"] = toml_edit::table();
    }
    if let Some(models) = doc["models"].as_table_mut() {
        let model_key = "kimi-code/kimi-for-coding";
        if !models.contains_key(model_key) {
            models[model_key] = toml_edit::table();
        }
        if let Some(model_table) = models[model_key].as_table_mut() {
            model_table["provider"] = toml_edit::value(provider_name);
            if !model_table.contains_key("model") {
                model_table["model"] = toml_edit::value("kimi-for-coding");
            }
            if !model_table.contains_key("max_context_size") {
                model_table["max_context_size"] = toml_edit::value(262144);
            }
            if !model_table.contains_key("display_name") {
                model_table["display_name"] = toml_edit::value("Kimi-k2.6");
            }
            if !model_table.contains_key("capabilities") {
                let mut caps = toml_edit::Array::new();
                caps.push("thinking");
                caps.push("video_in");
                caps.push("image_in");
                model_table["capabilities"] = toml_edit::value(caps);
            }
        }
    }

    // Sync services api_key with the active provider's api_key
    if let Some(services) = doc.get_mut("services").and_then(|s| s.as_table_mut()) {
        for service_name in ["moonshot_fetch", "moonshot_search"] {
            if let Some(service) = services.get_mut(service_name) {
                if let Some(service_table) = service.as_table_mut() {
                    service_table["api_key"] = toml_edit::value(api_key);
                }
            }
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

/// 从 Kimi config.toml 读取当前默认模型名
///
/// 读取 `[models."kimi-code/kimi-for-coding"]` 的完整 key 作为模型 ID
///（Kimi For Coding API 通常期望完整路径如 `kimi-code/kimi-for-coding`），
/// 若不存在则回退到 `.model` 字段。
pub fn get_kimi_model_from_config() -> Option<String> {
    let doc = read_kimi_config().ok()??;
    let models = doc.get("models")?.as_table()?;
    let model_key = "kimi-code/kimi-for-coding";
    let model_table = models.get(model_key)?.as_table()?;
    // 优先使用完整模型路径（含 namespace），API 端点通常需要这个格式
    Some(model_key.to_string())
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
