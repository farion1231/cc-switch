use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use crate::gemini_config::{parse_env_file, serialize_env_file};
use crate::provider::Provider;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// 获取 Hermes 配置目录路径（支持设置覆盖）
pub fn get_hermes_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_hermes_override_dir() {
        return custom;
    }

    get_home_dir().join(".hermes")
}

/// 获取 Hermes config.yaml 文件路径
pub fn get_hermes_config_path() -> PathBuf {
    get_hermes_dir().join("config.yaml")
}

/// 获取 Hermes .env 文件路径
pub fn get_hermes_env_path() -> PathBuf {
    get_hermes_dir().join(".env")
}

/// 获取 Hermes auth.json 文件路径
pub fn get_hermes_auth_path() -> PathBuf {
    get_hermes_dir().join("auth.json")
}

/// 读取 Hermes config.yaml 文件
///
/// 如果文件不存在，返回空的 YAML mapping。
pub fn read_hermes_config() -> Result<serde_yaml::Value, AppError> {
    let path = get_hermes_config_path();

    if !path.exists() {
        return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let yaml = serde_yaml::from_str(&content).map_err(|e| {
        AppError::localized(
            "hermes.config.parse_error",
            format!("Hermes config.yaml 解析失败: {e}"),
            format!("Failed to parse Hermes config.yaml: {e}"),
        )
    })?;

    Ok(yaml)
}

/// 原子写入 Hermes config.yaml 文件（temp + rename）
pub fn write_hermes_config_atomic(yaml: &serde_yaml::Value) -> Result<(), AppError> {
    let path = get_hermes_config_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let content = serde_yaml::to_string(yaml).map_err(|e| {
        AppError::localized(
            "hermes.config.serialize_error",
            format!("Hermes config.yaml 序列化失败: {e}"),
            format!("Failed to serialize Hermes config.yaml: {e}"),
        )
    })?;

    write_text_file(&path, &content)?;

    Ok(())
}

/// 读取 Hermes .env 文件
pub fn read_hermes_env() -> Result<HashMap<String, String>, AppError> {
    let path = get_hermes_env_path();

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    Ok(parse_env_file(&content))
}

/// 原子写入 Hermes .env 文件（temp + rename）
pub fn write_hermes_env_atomic(env: &HashMap<String, String>) -> Result<(), AppError> {
    let path = get_hermes_env_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;

        // 设置目录权限为 700（仅所有者可读写执行）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(parent)
                .map_err(|e| AppError::io(parent, e))?
                .permissions();
            perms.set_mode(0o700);
            fs::set_permissions(parent, perms).map_err(|e| AppError::io(parent, e))?;
        }
    }

    let content = serialize_env_file(env);
    write_text_file(&path, &content)?;

    // 设置文件权限为 600（仅所有者可读写）
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

/// 读取 Hermes 当前配置，返回 JSON `{model, base_url, api_key}`
///
/// 从 config.yaml 读取 model.default、model.provider、model.base_url，
/// 从 .env 读取 API key（OPENROUTER_API_KEY 或 ANTHROPIC_API_KEY 等）。
pub fn read_hermes_live_settings() -> Result<JsonValue, AppError> {
    let yaml = read_hermes_config()?;
    let env = read_hermes_env()?;

    let model = yaml
        .get("model")
        .and_then(|m| m.get("default"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let base_url = yaml
        .get("model")
        .and_then(|m| m.get("base_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Pick the first API key found in the env file
    let api_key = env
        .get("OPENROUTER_API_KEY")
        .or_else(|| env.get("ANTHROPIC_API_KEY"))
        .or_else(|| env.get("OPENAI_API_KEY"))
        .cloned()
        .unwrap_or_default();

    Ok(serde_json::json!({
        "model": model,
        "base_url": base_url,
        "api_key": api_key,
    }))
}

/// 将 Provider 配置写入 Hermes 的 config.yaml 和 .env
///
/// 更新 config.yaml 中的 model.provider、model.default、model.base_url，
/// 保留其他所有字段。API key 写入 .env，保留其他 env 变量。
pub fn write_hermes_live(provider: &Provider) -> Result<(), AppError> {
    log::info!(
        "[hermes_config] write_hermes_live: provider_id={}, provider_name={}",
        provider.id,
        provider.name
    );

    // --- config.yaml ---
    let mut yaml = read_hermes_config()?;

    // Ensure yaml is a mapping
    if !yaml.is_mapping() {
        yaml = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    }

    let settings = &provider.settings_config;

    let model_str = settings
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let base_url_str = settings
        .get("base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let api_key_str = settings
        .get("api_key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let provider_name = provider.name.clone();

    // Patch model section in YAML, preserving other fields
    {
        let mapping = yaml.as_mapping_mut().expect("yaml is a mapping");
        let model_key = serde_yaml::Value::String("model".to_string());

        let model_section = mapping
            .entry(model_key)
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

        if let Some(model_map) = model_section.as_mapping_mut() {
            let default_key = serde_yaml::Value::String("default".to_string());
            let base_url_key = serde_yaml::Value::String("base_url".to_string());

            if !model_str.is_empty() {
                model_map.insert(default_key, serde_yaml::Value::String(model_str));
            } else {
                model_map.remove(&default_key);
            }
            model_map.insert(
                serde_yaml::Value::String("provider".to_string()),
                serde_yaml::Value::String(provider_name),
            );
            if !base_url_str.is_empty() {
                model_map.insert(base_url_key, serde_yaml::Value::String(base_url_str));
            } else {
                model_map.remove(&base_url_key);
            }
        }
    }

    write_hermes_config_atomic(&yaml)?;

    // --- .env ---
    if !api_key_str.is_empty() {
        let mut env = read_hermes_env()?;

        // Determine which key name to use based on provider settings_config
        let settings_str = serde_json::to_string(&provider.settings_config).unwrap_or_default();
        let key_name = if settings_str.contains("ANTHROPIC_API_KEY") {
            "ANTHROPIC_API_KEY"
        } else if settings_str.contains("OPENROUTER_API_KEY") {
            "OPENROUTER_API_KEY"
        } else {
            "HERMES_API_KEY"
        };

        // Clear all known API key vars before writing the correct one
        env.remove("ANTHROPIC_API_KEY");
        env.remove("OPENROUTER_API_KEY");
        env.remove("OPENAI_API_KEY");
        env.remove("HERMES_API_KEY");

        env.insert(key_name.to_string(), api_key_str);
        write_hermes_env_atomic(&env)?;

        log::info!(
            "[hermes_config] write_hermes_live: wrote api_key to .env key={}",
            key_name
        );
    }

    log::info!("[hermes_config] write_hermes_live: done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::collections::HashMap;
    use std::env;

    #[test]
    #[serial]
    fn test_read_write_hermes_config() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let old_test_home = env::var_os("CC_SWITCH_TEST_HOME");
        env::set_var("CC_SWITCH_TEST_HOME", tmp.path());

        let mut yaml = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let mut model_map = serde_yaml::Mapping::new();
        model_map.insert(
            serde_yaml::Value::String("default".to_string()),
            serde_yaml::Value::String("claude-3-5-sonnet".to_string()),
        );
        model_map.insert(
            serde_yaml::Value::String("provider".to_string()),
            serde_yaml::Value::String("anthropic".to_string()),
        );
        model_map.insert(
            serde_yaml::Value::String("base_url".to_string()),
            serde_yaml::Value::String("https://api.anthropic.com".to_string()),
        );
        yaml.as_mapping_mut()
            .unwrap()
            .insert(serde_yaml::Value::String("model".to_string()), serde_yaml::Value::Mapping(model_map));

        let mut other_section = serde_yaml::Mapping::new();
        other_section.insert(
            serde_yaml::Value::String("key".to_string()),
            serde_yaml::Value::String("value".to_string()),
        );
        yaml.as_mapping_mut()
            .unwrap()
            .insert(serde_yaml::Value::String("other_section".to_string()), serde_yaml::Value::Mapping(other_section));

        write_hermes_config_atomic(&yaml).unwrap();
        let read_yaml = read_hermes_config().unwrap();

        assert_eq!(
            read_yaml
                .get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("claude-3-5-sonnet")
        );
        assert_eq!(
            read_yaml
                .get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("anthropic")
        );
        assert_eq!(
            read_yaml
                .get("model")
                .and_then(|m| m.get("base_url"))
                .and_then(|v| v.as_str()),
            Some("https://api.anthropic.com")
        );
        assert_eq!(
            read_yaml
                .get("other_section")
                .and_then(|s| s.get("key"))
                .and_then(|v| v.as_str()),
            Some("value")
        );

        match old_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn test_write_hermes_env_preserves_other_keys() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let old_test_home = env::var_os("CC_SWITCH_TEST_HOME");
        env::set_var("CC_SWITCH_TEST_HOME", tmp.path());

        let mut env: HashMap<String, String> = HashMap::new();
        env.insert("OPENROUTER_API_KEY".to_string(), "sk-or-test".to_string());
        env.insert("OTHER_KEY".to_string(), "other-value".to_string());
        env.insert("CUSTOM_VAR".to_string(), "custom".to_string());

        write_hermes_env_atomic(&env).unwrap();

        // Update only the API key
        let mut updated_env = read_hermes_env().unwrap();
        updated_env.insert("OPENROUTER_API_KEY".to_string(), "sk-or-new".to_string());
        write_hermes_env_atomic(&updated_env).unwrap();

        let final_env = read_hermes_env().unwrap();

        assert_eq!(
            final_env.get("OPENROUTER_API_KEY"),
            Some(&"sk-or-new".to_string())
        );
        assert_eq!(final_env.get("OTHER_KEY"), Some(&"other-value".to_string()));
        assert_eq!(final_env.get("CUSTOM_VAR"), Some(&"custom".to_string()));

        match old_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn test_write_hermes_live() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let old_test_home = env::var_os("CC_SWITCH_TEST_HOME");
        env::set_var("CC_SWITCH_TEST_HOME", tmp.path());

        let provider = Provider::with_id(
            "test-1".to_string(),
            "Test Provider".to_string(),
            serde_json::json!({
                "model": "claude-3-5-sonnet",
                "base_url": "https://api.anthropic.com",
                "api_key": "sk-ant-test",
                "env": {
                    "ANTHROPIC_API_KEY": "sk-ant-test"
                }
            }),
            None,
        );

        write_hermes_live(&provider).unwrap();

        let yaml = read_hermes_config().unwrap();
        let env_map = read_hermes_env().unwrap();

        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("claude-3-5-sonnet")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("Test Provider")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("base_url"))
                .and_then(|v| v.as_str()),
            Some("https://api.anthropic.com")
        );
        assert_eq!(
            env_map.get("ANTHROPIC_API_KEY"),
            Some(&"sk-ant-test".to_string())
        );

        match old_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }
}
