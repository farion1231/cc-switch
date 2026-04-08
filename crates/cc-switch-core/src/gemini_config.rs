use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::config::{get_gemini_config_dir, write_json_file, write_text_file};
use crate::error::AppError;

pub fn get_gemini_dir() -> PathBuf {
    get_gemini_config_dir()
}

pub fn get_gemini_env_path() -> PathBuf {
    get_gemini_dir().join(".env")
}

pub fn get_gemini_settings_path() -> PathBuf {
    get_gemini_dir().join("settings.json")
}

pub fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                map.insert(key.to_string(), value.to_string());
            }
        }
    }

    map
}

pub fn parse_env_file_strict(content: &str) -> Result<HashMap<String, String>, AppError> {
    let mut map = HashMap::new();

    for (index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line_number = index + 1;
        let (key, value) = line.split_once('=').ok_or_else(|| {
            AppError::localized(
                "gemini.env.parse_error.no_equals",
                format!("Gemini .env 文件格式错误（第 {line_number} 行）：缺少 '=' 分隔符"),
                format!("Invalid Gemini .env format (line {line_number}): missing '=' separator"),
            )
        })?;

        let key = key.trim();
        let value = value.trim();

        if key.is_empty() {
            return Err(AppError::localized(
                "gemini.env.parse_error.empty_key",
                format!("Gemini .env 文件格式错误（第 {line_number} 行）：环境变量名不能为空"),
                format!("Invalid Gemini .env format (line {line_number}): variable name cannot be empty"),
            ));
        }

        if !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(AppError::localized(
                "gemini.env.parse_error.invalid_key",
                format!("Gemini .env 文件格式错误（第 {line_number} 行）：环境变量名只能包含字母、数字和下划线"),
                format!("Invalid Gemini .env format (line {line_number}): variable name can only contain letters, numbers, and underscores"),
            ));
        }

        map.insert(key.to_string(), value.to_string());
    }

    Ok(map)
}

pub fn serialize_env_file(map: &HashMap<String, String>) -> String {
    let mut keys: Vec<_> = map.keys().collect();
    keys.sort();
    keys.into_iter()
        .filter_map(|key| map.get(key).map(|value| format!("{key}={value}")))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn read_gemini_env() -> Result<HashMap<String, String>, AppError> {
    let path = get_gemini_env_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    Ok(parse_env_file(&content))
}

pub fn write_gemini_env_atomic(map: &HashMap<String, String>) -> Result<(), AppError> {
    let path = get_gemini_env_path();
    let content = serialize_env_file(map);
    write_text_file(&path, &content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(&path)
            .map_err(|e| AppError::io(&path, e))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms).map_err(|e| AppError::io(&path, e))?;
    }

    Ok(())
}

pub fn env_to_json(env_map: &HashMap<String, String>) -> Value {
    let mut json_map = serde_json::Map::new();
    for (key, value) in env_map {
        json_map.insert(key.clone(), Value::String(value.clone()));
    }
    serde_json::json!({ "env": json_map })
}

pub fn json_to_env(settings: &Value) -> Result<HashMap<String, String>, AppError> {
    let mut env_map = HashMap::new();
    if let Some(env_obj) = settings.get("env").and_then(|value| value.as_object()) {
        for (key, value) in env_obj {
            if let Some(text) = value.as_str() {
                env_map.insert(key.clone(), text.to_string());
            }
        }
    }
    Ok(env_map)
}

pub fn validate_gemini_settings(settings: &Value) -> Result<(), AppError> {
    if let Some(env) = settings.get("env") {
        if !env.is_object() {
            return Err(AppError::localized(
                "gemini.validation.invalid_env",
                "Gemini 配置格式错误: env 必须是对象",
                "Gemini config invalid: env must be an object",
            ));
        }
    }

    if let Some(config) = settings.get("config") {
        if !(config.is_object() || config.is_null()) {
            return Err(AppError::localized(
                "gemini.validation.invalid_config",
                "Gemini 配置格式错误: config 必须是对象或 null",
                "Gemini config invalid: config must be an object or null",
            ));
        }
    }

    Ok(())
}

pub fn validate_gemini_settings_strict(settings: &Value) -> Result<(), AppError> {
    validate_gemini_settings(settings)?;
    let env_map = json_to_env(settings)?;
    if env_map.is_empty() {
        return Ok(());
    }

    if !env_map.contains_key("GEMINI_API_KEY") {
        return Err(AppError::localized(
            "gemini.validation.missing_api_key",
            "Gemini 配置缺少必需字段: GEMINI_API_KEY",
            "Gemini config missing required field: GEMINI_API_KEY",
        ));
    }

    Ok(())
}

fn update_selected_type(selected_type: &str) -> Result<(), AppError> {
    let path = get_gemini_settings_path();
    let mut settings = if path.exists() {
        crate::config::read_json_file::<Value>(&path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(root) = settings.as_object_mut() {
        let security = root
            .entry("security")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(security_obj) = security.as_object_mut() {
            let auth = security_obj
                .entry("auth")
                .or_insert_with(|| serde_json::json!({}));
            if let Some(auth_obj) = auth.as_object_mut() {
                auth_obj.insert(
                    "selectedType".to_string(),
                    Value::String(selected_type.to_string()),
                );
            }
        }
    }

    write_json_file(&path, &settings)
}

pub fn write_packycode_settings() -> Result<(), AppError> {
    update_selected_type("gemini-api-key")
}

pub fn write_google_oauth_settings() -> Result<(), AppError> {
    update_selected_type("oauth-personal")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    fn strict_env_parser_rejects_invalid_lines() {
        let err = parse_env_file_strict("BAD-LINE").expect_err("missing equals should fail");
        assert!(err.to_string().contains("Invalid Gemini .env format"));
    }

    #[test]
    #[serial]
    fn gemini_settings_selected_type_round_trip() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        write_packycode_settings()?;
        let settings: Value = crate::config::read_json_file(&get_gemini_settings_path())?;

        assert_eq!(
            settings
                .get("security")
                .and_then(|value| value.get("auth"))
                .and_then(|value| value.get("selectedType"))
                .and_then(|value| value.as_str()),
            Some("gemini-api-key")
        );

        Ok(())
    }
}
