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
#[allow(dead_code)]
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

/// 读取 Hermes 当前配置
///
/// 从 config.yaml 的 custom_providers 列表中获取当前 provider 的配置。
/// 当前 provider 由 model.provider 字段指定。
/// 返回 JSON `{model, base_url, api_key, provider_name}`
pub fn read_hermes_live_settings() -> Result<JsonValue, AppError> {
    let yaml = read_hermes_config()?;

    // 获取当前 model 设置
    let model_default = yaml
        .get("model")
        .and_then(|m| m.get("default"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let model_provider = yaml
        .get("model")
        .and_then(|m| m.get("provider"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // 从 custom_providers 列表中查找当前 provider
    let custom_providers = yaml.get("custom_providers").and_then(|p| p.as_sequence());

    let mut base_url = String::new();
    let mut api_key = String::new();

    if let Some(providers) = custom_providers {
        for provider in providers {
            if let Some(name) = provider.get("name").and_then(|n| n.as_str()) {
                if name == model_provider {
                    base_url = provider
                        .get("base_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    api_key = provider
                        .get("api_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    break;
                }
            }
        }
    }

    Ok(serde_json::json!({
        "model": model_default,
        "base_url": base_url,
        "api_key": api_key,
        "provider_name": model_provider,
    }))
}

/// 将 Provider 配置写入 Hermes 的 config.yaml
///
/// Hermes 使用 custom_providers 列表存储 provider 配置。
/// 更新 model.provider、model.default 指向当前 provider，
/// 并更新或添加 custom_providers 中的对应条目。
pub fn write_hermes_live(provider: &Provider) -> Result<(), AppError> {
    log::info!(
        "[hermes_config] write_hermes_live: provider_id={}, provider_name={}",
        provider.id,
        provider.name
    );

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

    // 1. 更新 model section
    {
        let mapping = yaml.as_mapping_mut().expect("yaml is a mapping");
        let model_key = serde_yaml::Value::String("model".to_string());

        let model_section = mapping
            .entry(model_key)
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

        if let Some(model_map) = model_section.as_mapping_mut() {
            // 更新 default model
            if !model_str.is_empty() {
                model_map.insert(
                    serde_yaml::Value::String("default".to_string()),
                    serde_yaml::Value::String(model_str.clone()),
                );
            }
            // 更新 provider name
            model_map.insert(
                serde_yaml::Value::String("provider".to_string()),
                serde_yaml::Value::String(provider_name.clone()),
            );
        }
    }

    // 2. 更新或添加 custom_providers
    {
        let mapping = yaml.as_mapping_mut().expect("yaml is a mapping");
        let providers_key = serde_yaml::Value::String("custom_providers".to_string());

        // 确保 custom_providers 存在
        if !mapping.contains_key(&providers_key) {
            mapping.insert(providers_key.clone(), serde_yaml::Value::Sequence(vec![]));
        }

        let providers = mapping
            .get_mut(&providers_key)
            .expect("custom_providers exists");

        if let Some(providers_seq) = providers.as_sequence_mut() {
            // 查找是否已存在同名 provider
            let mut found = false;
            for existing_provider in providers_seq.iter_mut() {
                if let Some(existing_name) = existing_provider.get("name").and_then(|n| n.as_str())
                {
                    if existing_name == provider_name {
                        // 更新现有 provider
                        if let Some(provider_map) = existing_provider.as_mapping_mut() {
                            if !base_url_str.is_empty() {
                                provider_map.insert(
                                    serde_yaml::Value::String("base_url".to_string()),
                                    serde_yaml::Value::String(base_url_str.clone()),
                                );
                            }
                            if !api_key_str.is_empty() {
                                provider_map.insert(
                                    serde_yaml::Value::String("api_key".to_string()),
                                    serde_yaml::Value::String(api_key_str.clone()),
                                );
                            }
                            if !model_str.is_empty() {
                                provider_map.insert(
                                    serde_yaml::Value::String("model".to_string()),
                                    serde_yaml::Value::String(model_str.clone()),
                                );
                            }
                            // 确保 transport 字段存在
                            if !provider_map
                                .contains_key(serde_yaml::Value::String("transport".to_string()))
                            {
                                provider_map.insert(
                                    serde_yaml::Value::String("transport".to_string()),
                                    serde_yaml::Value::String("openai_chat".to_string()),
                                );
                            }
                        }
                        found = true;
                        break;
                    }
                }
            }

            // 如果不存在，添加新的 provider
            if !found {
                let mut new_provider = serde_yaml::Mapping::new();
                new_provider.insert(
                    serde_yaml::Value::String("name".to_string()),
                    serde_yaml::Value::String(provider_name.clone()),
                );
                if !base_url_str.is_empty() {
                    new_provider.insert(
                        serde_yaml::Value::String("base_url".to_string()),
                        serde_yaml::Value::String(base_url_str),
                    );
                }
                if !api_key_str.is_empty() {
                    new_provider.insert(
                        serde_yaml::Value::String("api_key".to_string()),
                        serde_yaml::Value::String(api_key_str),
                    );
                }
                if !model_str.is_empty() {
                    new_provider.insert(
                        serde_yaml::Value::String("model".to_string()),
                        serde_yaml::Value::String(model_str),
                    );
                }
                new_provider.insert(
                    serde_yaml::Value::String("transport".to_string()),
                    serde_yaml::Value::String("openai_chat".to_string()),
                );
                providers_seq.push(serde_yaml::Value::Mapping(new_provider));
            }
        }
    }

    write_hermes_config_atomic(&yaml)?;

    log::info!("[hermes_config] write_hermes_live: done");
    Ok(())
}

/// 从 Hermes config.yaml 的 custom_providers 中移除指定 provider
///
/// 仅删除 custom_providers 列表中的条目，不修改 model section。
/// 如果 provider 不存在，静默返回成功。
pub fn remove_hermes_provider_from_live(provider_name: &str) -> Result<(), AppError> {
    log::info!(
        "[hermes_config] remove_hermes_provider_from_live: provider_name={}",
        provider_name
    );

    // 检查 Hermes 配置目录是否存在
    if !get_hermes_dir().exists() {
        log::debug!("Hermes config directory doesn't exist, skipping removal");
        return Ok(());
    }

    let mut yaml = read_hermes_config()?;

    // 确保 yaml 是一个 mapping
    if !yaml.is_mapping() {
        return Ok(());
    }

    let mapping = yaml.as_mapping_mut().expect("yaml is a mapping");
    let providers_key = serde_yaml::Value::String("custom_providers".to_string());

    // 检查 custom_providers 是否存在
    let Some(providers) = mapping.get_mut(&providers_key) else {
        return Ok(());
    };

    let Some(providers_seq) = providers.as_sequence_mut() else {
        return Ok(());
    };

    // 查找并移除指定 provider
    let initial_len = providers_seq.len();
    providers_seq.retain(|p| {
        p.get("name")
            .and_then(|n| n.as_str())
            .is_none_or(|n| n != provider_name)
    });

    if providers_seq.len() < initial_len {
        log::info!(
            "[hermes_config] Removed provider '{}' from custom_providers",
            provider_name
        );
        write_hermes_config_atomic(&yaml)?;
    } else {
        log::debug!(
            "[hermes_config] Provider '{}' not found in custom_providers",
            provider_name
        );
    }

    Ok(())
}

/// 仅更新 Hermes config.yaml 的 model section（切换当前 provider）
///
/// 更新 model.provider 和 model.default，不修改 custom_providers 列表。
/// 用于切换当前激活的 provider。
#[allow(dead_code)]
pub fn set_current_hermes_provider(
    provider_name: &str,
    model: Option<&str>,
) -> Result<(), AppError> {
    log::info!(
        "[hermes_config] set_current_hermes_provider: provider_name={}, model={:?}",
        provider_name,
        model
    );

    // 检查 Hermes 配置目录是否存在
    if !get_hermes_dir().exists() {
        log::debug!("Hermes config directory doesn't exist, skipping set current");
        return Ok(());
    }

    let mut yaml = read_hermes_config()?;

    // 确保 yaml 是一个 mapping
    if !yaml.is_mapping() {
        yaml = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    }

    let mapping = yaml.as_mapping_mut().expect("yaml is a mapping");
    let model_key = serde_yaml::Value::String("model".to_string());

    let model_section = mapping
        .entry(model_key)
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

    if let Some(model_map) = model_section.as_mapping_mut() {
        // 更新 provider name
        model_map.insert(
            serde_yaml::Value::String("provider".to_string()),
            serde_yaml::Value::String(provider_name.to_string()),
        );

        // 如果提供了 model，也更新 default
        if let Some(model_str) = model {
            if !model_str.is_empty() {
                model_map.insert(
                    serde_yaml::Value::String("default".to_string()),
                    serde_yaml::Value::String(model_str.to_string()),
                );
            }
        }
    }

    write_hermes_config_atomic(&yaml)?;

    log::info!("[hermes_config] set_current_hermes_provider: done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
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
            serde_yaml::Value::String("claude-sonnet-4-6".to_string()),
        );
        model_map.insert(
            serde_yaml::Value::String("provider".to_string()),
            serde_yaml::Value::String("api-proxy-claude".to_string()),
        );
        yaml.as_mapping_mut().unwrap().insert(
            serde_yaml::Value::String("model".to_string()),
            serde_yaml::Value::Mapping(model_map),
        );

        let mut custom_providers = vec![];
        let mut provider_map = serde_yaml::Mapping::new();
        provider_map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String("api-proxy-claude".to_string()),
        );
        provider_map.insert(
            serde_yaml::Value::String("base_url".to_string()),
            serde_yaml::Value::String("https://api.example.com/v1".to_string()),
        );
        provider_map.insert(
            serde_yaml::Value::String("api_key".to_string()),
            serde_yaml::Value::String("sk-test-key".to_string()),
        );
        provider_map.insert(
            serde_yaml::Value::String("model".to_string()),
            serde_yaml::Value::String("claude-sonnet-4-6".to_string()),
        );
        provider_map.insert(
            serde_yaml::Value::String("transport".to_string()),
            serde_yaml::Value::String("openai_chat".to_string()),
        );
        custom_providers.push(serde_yaml::Value::Mapping(provider_map));
        yaml.as_mapping_mut().unwrap().insert(
            serde_yaml::Value::String("custom_providers".to_string()),
            serde_yaml::Value::Sequence(custom_providers),
        );

        write_hermes_config_atomic(&yaml).unwrap();
        let read_yaml = read_hermes_config().unwrap();

        assert_eq!(
            read_yaml
                .get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("claude-sonnet-4-6")
        );
        assert_eq!(
            read_yaml
                .get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("api-proxy-claude")
        );

        // Verify custom_providers
        let providers = read_yaml
            .get("custom_providers")
            .and_then(|p| p.as_sequence())
            .unwrap();
        assert_eq!(providers.len(), 1);
        let first_provider = &providers[0];
        assert_eq!(
            first_provider.get("name").and_then(|n| n.as_str()),
            Some("api-proxy-claude")
        );
        assert_eq!(
            first_provider.get("base_url").and_then(|v| v.as_str()),
            Some("https://api.example.com/v1")
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
        env.insert("TELEGRAM_BOT_TOKEN".to_string(), "test-token".to_string());
        env.insert("OTHER_KEY".to_string(), "other-value".to_string());

        write_hermes_env_atomic(&env).unwrap();

        let read_env = read_hermes_env().unwrap();
        assert_eq!(
            read_env.get("TELEGRAM_BOT_TOKEN"),
            Some(&"test-token".to_string())
        );
        assert_eq!(read_env.get("OTHER_KEY"), Some(&"other-value".to_string()));

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
            "My Custom Provider".to_string(),
            serde_json::json!({
                "model": "claude-sonnet-4-6",
                "base_url": "https://api.custom.com/v1",
                "api_key": "sk-custom-key",
            }),
            None,
        );

        write_hermes_live(&provider).unwrap();

        let yaml = read_hermes_config().unwrap();

        // 验证 model section
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("claude-sonnet-4-6")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("My Custom Provider")
        );

        // 验证 custom_providers
        let providers = yaml
            .get("custom_providers")
            .and_then(|p| p.as_sequence())
            .unwrap();
        assert!(providers.len() >= 1);
        let found = providers
            .iter()
            .any(|p| p.get("name").and_then(|n| n.as_str()) == Some("My Custom Provider"));
        assert!(
            found,
            "custom_providers should contain 'My Custom Provider'"
        );

        match old_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn test_remove_hermes_provider_from_live() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let old_test_home = env::var_os("CC_SWITCH_TEST_HOME");
        env::set_var("CC_SWITCH_TEST_HOME", tmp.path());

        // 创建两个 provider
        let provider1 = Provider::with_id(
            "test-1".to_string(),
            "Provider One".to_string(),
            serde_json::json!({
                "model": "claude-sonnet-4-6",
                "base_url": "https://api.one.com/v1",
                "api_key": "sk-key-one",
            }),
            None,
        );

        let provider2 = Provider::with_id(
            "test-2".to_string(),
            "Provider Two".to_string(),
            serde_json::json!({
                "model": "claude-opus-4",
                "base_url": "https://api.two.com/v1",
                "api_key": "sk-key-two",
            }),
            None,
        );

        // 写入两个 provider
        write_hermes_live(&provider1).unwrap();
        write_hermes_live(&provider2).unwrap();

        // 验证两个 provider 都存在
        let yaml = read_hermes_config().unwrap();
        let providers = yaml
            .get("custom_providers")
            .and_then(|p| p.as_sequence())
            .unwrap();
        assert_eq!(providers.len(), 2);

        // 删除第一个 provider
        remove_hermes_provider_from_live("Provider One").unwrap();

        // 验证只剩一个 provider
        let yaml = read_hermes_config().unwrap();
        let providers = yaml
            .get("custom_providers")
            .and_then(|p| p.as_sequence())
            .unwrap();
        assert_eq!(providers.len(), 1);

        // 验证剩余的是 Provider Two
        let remaining_name = providers[0].get("name").and_then(|n| n.as_str()).unwrap();
        assert_eq!(remaining_name, "Provider Two");

        // 验证删除不存在的 provider 不会出错
        remove_hermes_provider_from_live("Non-existent Provider").unwrap();

        // 验证 provider 数量不变
        let yaml = read_hermes_config().unwrap();
        let providers = yaml
            .get("custom_providers")
            .and_then(|p| p.as_sequence())
            .unwrap();
        assert_eq!(providers.len(), 1);

        match old_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn test_set_current_hermes_provider() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let old_test_home = env::var_os("CC_SWITCH_TEST_HOME");
        env::set_var("CC_SWITCH_TEST_HOME", tmp.path());

        // 创建一个 provider
        let provider = Provider::with_id(
            "test-1".to_string(),
            "Test Provider".to_string(),
            serde_json::json!({
                "model": "claude-sonnet-4-6",
                "base_url": "https://api.test.com/v1",
                "api_key": "sk-test-key",
            }),
            None,
        );
        write_hermes_live(&provider).unwrap();

        // 切换到不同的 provider（仅更新 model section）
        set_current_hermes_provider("Another Provider", Some("claude-opus-4")).unwrap();

        let yaml = read_hermes_config().unwrap();

        // 验证 model section 已更新
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("Another Provider")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("claude-opus-4")
        );

        // 验证 custom_providers 仍然只有一个（没有被修改）
        let providers = yaml
            .get("custom_providers")
            .and_then(|p| p.as_sequence())
            .unwrap();
        assert_eq!(providers.len(), 1);

        match old_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }
}
