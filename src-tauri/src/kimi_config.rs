use std::path::PathBuf;

use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use serde_json::Value;
use toml_edit::DocumentMut;

pub const KIMI_DEFAULT_PROVIDER_NAME: &str = "ccswitch";
pub const KIMI_DEFAULT_MODEL: &str = "kimi-code/kimi-for-coding";

/// 获取 Kimi 配置目录路径（支持设置覆盖）
pub fn get_kimi_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_kimi_override_dir() {
        return custom;
    }
    get_home_dir().join(".kimi-code")
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

fn validate_kimi_live_values(
    base_url: &str,
    api_key: &str,
    provider_name: &str,
) -> Result<(), AppError> {
    if base_url.trim().is_empty() {
        return Err(AppError::localized(
            "kimi.validation.missing_base_url",
            "Kimi 配置缺少必需字段: KIMI_BASE_URL",
            "Kimi config missing required field: KIMI_BASE_URL",
        ));
    }
    if api_key.trim().is_empty() {
        return Err(AppError::localized(
            "kimi.validation.missing_api_key",
            "Kimi 配置缺少必需字段: KIMI_API_KEY",
            "Kimi config missing required field: KIMI_API_KEY",
        ));
    }
    if provider_name.trim().is_empty() {
        return Err(AppError::localized(
            "kimi.validation.missing_provider_name",
            "Kimi 配置缺少必需字段: KIMI_PROVIDER_NAME",
            "Kimi config missing required field: KIMI_PROVIDER_NAME",
        ));
    }
    Ok(())
}

fn active_model_key(doc: &DocumentMut) -> String {
    let configured = doc.get("default_model").and_then(|v| v.as_str());
    configured
        .filter(|model_key| {
            doc.get("models")
                .and_then(|models| models.get(*model_key))
                .and_then(|model| model.as_table())
                .is_some()
        })
        .unwrap_or(KIMI_DEFAULT_MODEL)
        .to_string()
}

fn apply_kimi_live_config(
    doc: &mut DocumentMut,
    base_url: &str,
    api_key: &str,
    provider_name: &str,
) -> Result<(), AppError> {
    validate_kimi_live_values(base_url, api_key, provider_name)?;

    let active_model = active_model_key(doc);

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
    }

    // Keep a valid user-selected default model (including highspeed), otherwise
    // fall back to the standard Kimi coding model.
    if doc.get("default_model").and_then(|v| v.as_str()) != Some(active_model.as_str()) {
        doc["default_model"] = toml_edit::value(active_model.as_str());
    }

    // Update model provider reference to point to the active provider
    if doc.get("models").is_none() {
        doc["models"] = toml_edit::table();
    }
    if let Some(models) = doc["models"].as_table_mut() {
        let model_key = KIMI_DEFAULT_MODEL;
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

        // Kimi Code may change default_model to highspeed and bind it to
        // managed:kimi-code. Switching in CC Switch must update the model that
        // is actually active, not only the legacy standard model entry.
        if active_model != model_key {
            if let Some(model_table) = models
                .get_mut(active_model.as_str())
                .and_then(|model| model.as_table_mut())
            {
                model_table["provider"] = toml_edit::value(provider_name);
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

    Ok(())
}

/// 核心：将 provider 配置写入 [providers.{provider_name}]
/// 同时更新 models 中的 provider 指向当前激活的 provider
pub fn write_kimi_live(base_url: &str, api_key: &str, provider_name: &str) -> Result<(), AppError> {
    validate_kimi_live_values(base_url, api_key, provider_name)?;

    let mut doc = match read_kimi_config()? {
        Some(doc) => doc,
        None => build_default_kimi_config(base_url, api_key, provider_name),
    };

    apply_kimi_live_config(&mut doc, base_url, api_key, provider_name)?;
    write_kimi_config(&doc)
}

/// 从 Provider.settings_config 提取 env 键值对
pub fn json_to_env(
    settings: &Value,
) -> Result<std::collections::HashMap<String, String>, AppError> {
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

/// Resolve the values written to Kimi Live, including the legacy full-TOML
/// settings shape that predates the current `{ "env": ... }` representation.
pub(crate) fn kimi_live_values_from_settings(
    settings: &Value,
) -> Result<(String, String, String), AppError> {
    let env_map = json_to_env(settings)?;
    let provider_name = env_map
        .get("KIMI_PROVIDER_NAME")
        .cloned()
        .unwrap_or_else(|| KIMI_DEFAULT_PROVIDER_NAME.to_string());
    let mut base_url = env_map.get("KIMI_BASE_URL").cloned().unwrap_or_default();
    let mut api_key = env_map.get("KIMI_API_KEY").cloned().unwrap_or_default();

    if base_url.trim().is_empty() && api_key.trim().is_empty() {
        if let Some(provider) = settings
            .get("providers")
            .and_then(|providers| providers.get(&provider_name))
        {
            base_url = provider
                .get("base_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            api_key = provider
                .get("api_key")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
        }
    }

    Ok((base_url, api_key, provider_name))
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
/// 优先返回有效的 `default_model`（包括 highspeed），否则回退到标准模型。
pub fn get_kimi_model_from_config() -> Option<String> {
    let doc = read_kimi_config().ok()??;
    Some(active_model_key(&doc))
}

/// 严格验证 Kimi 配置（切换时使用）
pub fn validate_kimi_settings_strict(settings: &Value) -> Result<(), AppError> {
    validate_kimi_settings(settings)?;
    let (base_url, api_key, provider_name) = kimi_live_values_from_settings(settings)?;
    validate_kimi_live_values(&base_url, &api_key, &provider_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apply_live_config_updates_the_actual_default_model_provider() {
        let mut doc = r#"
default_model = "kimi-code/kimi-for-coding-highspeed"

[providers.mingwei]
type = "kimi"
base_url = "https://old.example/v1"
api_key = "old-key"
oauth = { access_token = "stale" }

[providers."managed:kimi-code"]
type = "kimi"
base_url = "https://api.kimi.com/coding/v1"
api_key = "managed-key"

[models."kimi-code/kimi-for-coding"]
provider = "yufeng"
model = "kimi-for-coding"

[models."kimi-code/kimi-for-coding-highspeed"]
provider = "managed:kimi-code"
model = "kimi-for-coding-highspeed"

[services.moonshot_search]
api_key = "old-service-key"
"#
        .parse::<DocumentMut>()
        .expect("valid config");

        apply_kimi_live_config(
            &mut doc,
            "https://api.kimi.com/coding/v1",
            "mingwei-key",
            "mingwei",
        )
        .expect("apply config");

        assert_eq!(
            doc["default_model"].as_str(),
            Some("kimi-code/kimi-for-coding-highspeed")
        );
        assert_eq!(
            doc["models"]["kimi-code/kimi-for-coding"]["provider"].as_str(),
            Some("mingwei")
        );
        assert_eq!(
            doc["models"]["kimi-code/kimi-for-coding-highspeed"]["provider"].as_str(),
            Some("mingwei")
        );
        assert_eq!(
            doc["providers"]["managed:kimi-code"]["api_key"].as_str(),
            Some("managed-key")
        );
        assert!(doc["providers"]["mingwei"].get("oauth").is_none());
        assert_eq!(
            doc["services"]["moonshot_search"]["api_key"].as_str(),
            Some("mingwei-key")
        );
    }

    #[test]
    fn apply_live_config_rejects_empty_credentials_without_mutating_document() {
        let mut doc =
            build_default_kimi_config("https://api.kimi.com/coding/v1", "existing-key", "mingwei");
        let before = doc.to_string();

        assert!(
            apply_kimi_live_config(&mut doc, "https://api.kimi.com/coding/v1", "", "mingwei")
                .is_err()
        );
        assert_eq!(doc.to_string(), before);
    }

    #[test]
    fn strict_validation_requires_api_key() {
        let settings = json!({
            "env": {
                "KIMI_BASE_URL": "https://api.kimi.com/coding/v1",
                "KIMI_API_KEY": ""
            }
        });

        assert!(validate_kimi_settings_strict(&settings).is_err());
    }

    #[test]
    fn strict_validation_rejects_blank_base_url_and_provider_name() {
        let blank_base_url = json!({
            "env": {
                "KIMI_BASE_URL": "  ",
                "KIMI_API_KEY": "valid-key",
                "KIMI_PROVIDER_NAME": "mingwei"
            }
        });
        let blank_provider_name = json!({
            "env": {
                "KIMI_BASE_URL": "https://api.kimi.com/coding/v1",
                "KIMI_API_KEY": "valid-key",
                "KIMI_PROVIDER_NAME": "  "
            }
        });

        assert!(validate_kimi_settings_strict(&blank_base_url).is_err());
        assert!(validate_kimi_settings_strict(&blank_provider_name).is_err());
    }

    #[test]
    fn strict_validation_accepts_legacy_full_toml_settings() {
        let settings = json!({
            "env": { "KIMI_PROVIDER_NAME": "mingwei" },
            "providers": {
                "mingwei": {
                    "base_url": "https://api.kimi.com/coding/v1",
                    "api_key": "valid-key"
                }
            }
        });

        assert!(validate_kimi_settings_strict(&settings).is_ok());
    }
}
