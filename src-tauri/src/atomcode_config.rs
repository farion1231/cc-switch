use std::path::PathBuf;

use serde_json::Value;
use toml_edit::DocumentMut;

use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;

/// atomcode 配置目录解析：
/// 1) cc-switch 覆盖设置 2) 环境变量 ATOMCODE_HOME 3) ~/.atomcode
pub fn get_atomcode_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_atomcode_override_dir() {
        return custom;
    }
    if let Some(home) = std::env::var("ATOMCODE_HOME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(home);
    }
    get_home_dir().join(".atomcode")
}

pub fn get_atomcode_config_path() -> PathBuf {
    get_atomcode_config_dir().join("config.toml")
}

pub fn read_atomcode_config_text() -> Result<String, AppError> {
    let path = get_atomcode_config_path();
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
    } else {
        Ok(String::new())
    }
}

/// 将 JSON 标量转为 toml_edit 值；返回 None 表示该字段应被跳过/移除
/// （null 或空字符串）。
fn json_scalar_to_toml(value: &Value) -> Option<toml_edit::Value> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(toml_edit::Value::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml_edit::Value::from(i))
            } else {
                n.as_f64().map(toml_edit::Value::from)
            }
        }
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(toml_edit::Value::from(s.as_str()))
            }
        }
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// 纯函数：把一个 atomcode provider（settings）合并进现有 config.toml 文本。
pub fn merge_atomcode_provider_into_config(
    existing: &str,
    settings: &Value,
) -> Result<String, AppError> {
    let obj = settings.as_object().ok_or_else(|| {
        AppError::Config("atomcode 供应商配置必须是 JSON 对象".to_string())
    })?;
    let provider_key = obj
        .get("providerKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Config("atomcode 供应商配置缺少 providerKey".to_string()))?
        .to_string();

    let mut doc = if existing.trim().is_empty() {
        DocumentMut::new()
    } else {
        existing
            .parse::<DocumentMut>()
            .map_err(|e| AppError::Message(format!("Invalid atomcode config.toml: {e}")))?
    };

    doc["default_provider"] = toml_edit::value(provider_key.as_str());

    if doc.get("providers").is_none() {
        let mut providers = toml_edit::Table::new();
        providers.set_implicit(true);
        doc["providers"] = toml_edit::Item::Table(providers);
    }
    let providers = doc["providers"].as_table_mut().ok_or_else(|| {
        AppError::Message("atomcode config.toml 的 providers 不是表".to_string())
    })?;

    let mut block = toml_edit::Table::new();
    for (key, value) in obj {
        if key == "providerKey" {
            continue;
        }
        if let Some(tv) = json_scalar_to_toml(value) {
            block[key] = toml_edit::value(tv);
        }
    }
    providers[provider_key.as_str()] = toml_edit::Item::Table(block);

    Ok(doc.to_string())
}

/// 纯函数：从 config.toml 文本反解出当前（default_provider 指向的，否则第一个）provider 的 settingsConfig。
pub fn extract_atomcode_provider_settings(config_text: &str) -> Result<Value, AppError> {
    if config_text.trim().is_empty() {
        return Err(AppError::localized(
            "atomcode.live.missing",
            "atomcode 配置文件不存在或为空",
            "atomcode configuration is missing or empty",
        ));
    }

    let toml_val: toml::Value = toml::from_str(config_text)
        .map_err(|e| AppError::Message(format!("Invalid atomcode config.toml: {e}")))?;

    let Some(providers) = toml_val.get("providers").and_then(|p| p.as_table()) else {
        return Err(AppError::localized(
            "atomcode.live.no_providers",
            "atomcode 配置中没有任何 provider",
            "atomcode config has no providers",
        ));
    };

    let default_key = toml_val
        .get("default_provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty() && providers.contains_key(*s))
        .map(str::to_string);

    let key = match default_key {
        Some(k) => k,
        None => providers
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| AppError::localized(
                "atomcode.live.no_providers",
                "atomcode 配置中没有任何 provider",
                "atomcode config has no providers",
            ))?,
    };

    let block = providers
        .get(&key)
        .ok_or_else(|| AppError::Message("atomcode provider 块缺失".to_string()))?;

    let mut json_block = serde_json::to_value(block)
        .map_err(|e| AppError::Message(format!("无法转换 atomcode provider 块: {e}")))?;
    if let Some(map) = json_block.as_object_mut() {
        map.insert("providerKey".to_string(), Value::String(key));
    }
    Ok(json_block)
}

/// IO 包装：把当前 provider 写入 live config.toml（合并保留其他内容）。
pub fn write_atomcode_provider_live(settings: &Value) -> Result<(), AppError> {
    let existing = read_atomcode_config_text()?;
    let merged = merge_atomcode_provider_into_config(&existing, settings)?;
    let path = get_atomcode_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    write_text_file(&path, &merged)
}

/// IO 包装：读取 live config.toml 并反解为 settingsConfig（导入用）。
pub fn read_atomcode_live_settings() -> Result<Value, AppError> {
    let text = read_atomcode_config_text()?;
    extract_atomcode_provider_settings(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_settings() -> Value {
        json!({
            "providerKey": "deepseek",
            "type": "openai",
            "model": "deepseek-chat",
            "api_key": "sk-test",
            "base_url": "https://api.deepseek.com/v1",
            "context_window": 64000
        })
    }

    #[test]
    fn merge_into_empty_creates_provider_and_default() {
        let out = merge_atomcode_provider_into_config("", &sample_settings()).unwrap();
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(parsed.get("default_provider").and_then(|v| v.as_str()), Some("deepseek"));
        let block = parsed.get("providers").and_then(|p| p.get("deepseek")).unwrap();
        assert_eq!(block.get("type").and_then(|v| v.as_str()), Some("openai"));
        assert_eq!(block.get("context_window").and_then(|v| v.as_integer()), Some(64000));
        assert!(block.get("providerKey").is_none());
    }

    #[test]
    fn merge_preserves_other_sections_and_providers() {
        let existing = r#"default_provider = "old"

[providers.old]
type = "claude"
model = "claude-opus-4-6"
api_key = "sk-old"

[datalog]
enabled = true
"#;
        let out = merge_atomcode_provider_into_config(existing, &sample_settings()).unwrap();
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        assert!(parsed.get("providers").and_then(|p| p.get("old")).is_some());
        assert_eq!(parsed.get("datalog").and_then(|d| d.get("enabled")).and_then(|v| v.as_bool()), Some(true));
        assert_eq!(parsed.get("default_provider").and_then(|v| v.as_str()), Some("deepseek"));
    }

    #[test]
    fn merge_upserts_existing_block_fields() {
        let existing = r#"default_provider = "deepseek"

[providers.deepseek]
type = "openai"
model = "old-model"
api_key = "sk-old"
base_url = "https://old.example/v1"
"#;
        let out = merge_atomcode_provider_into_config(existing, &sample_settings()).unwrap();
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        let block = parsed.get("providers").and_then(|p| p.get("deepseek")).unwrap();
        assert_eq!(block.get("model").and_then(|v| v.as_str()), Some("deepseek-chat"));
        assert_eq!(block.get("base_url").and_then(|v| v.as_str()), Some("https://api.deepseek.com/v1"));
    }

    #[test]
    fn merge_skips_empty_string_fields() {
        let settings = json!({
            "providerKey": "p1", "type": "openai", "model": "m", "api_key": "", "base_url": "   "
        });
        let out = merge_atomcode_provider_into_config("", &settings).unwrap();
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        let block = parsed.get("providers").and_then(|p| p.get("p1")).unwrap();
        assert!(block.get("api_key").is_none());
        assert!(block.get("base_url").is_none());
    }

    #[test]
    fn merge_requires_provider_key() {
        let settings = json!({ "type": "openai", "model": "m" });
        assert!(merge_atomcode_provider_into_config("", &settings).is_err());
    }

    #[test]
    fn extract_uses_default_provider() {
        let text = r#"default_provider = "b"

[providers.a]
type = "openai"
model = "ma"

[providers.b]
type = "claude"
model = "mb"
api_key = "sk-b"
"#;
        let out = extract_atomcode_provider_settings(text).unwrap();
        assert_eq!(out.get("providerKey").and_then(|v| v.as_str()), Some("b"));
        assert_eq!(out.get("type").and_then(|v| v.as_str()), Some("claude"));
        assert_eq!(out.get("model").and_then(|v| v.as_str()), Some("mb"));
    }

    #[test]
    fn extract_falls_back_to_first_provider_without_default() {
        let text = r#"[providers.only]
type = "ollama"
model = "llama3"
"#;
        let out = extract_atomcode_provider_settings(text).unwrap();
        assert_eq!(out.get("providerKey").and_then(|v| v.as_str()), Some("only"));
    }

    #[test]
    fn extract_errors_on_empty() {
        assert!(extract_atomcode_provider_settings("").is_err());
    }

    #[test]
    fn round_trip_merge_then_extract() {
        let merged = merge_atomcode_provider_into_config("", &sample_settings()).unwrap();
        let extracted = extract_atomcode_provider_settings(&merged).unwrap();
        assert_eq!(extracted.get("providerKey").and_then(|v| v.as_str()), Some("deepseek"));
        assert_eq!(extracted.get("base_url").and_then(|v| v.as_str()), Some("https://api.deepseek.com/v1"));
    }
}
