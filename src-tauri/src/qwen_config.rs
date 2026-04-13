use crate::config::{get_home_dir, read_json_file, write_json_file, write_text_file};
use crate::error::AppError;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// 获取 Qwen 配置目录路径
pub fn get_qwen_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_qwen_override_dir() {
        return custom;
    }

    get_home_dir().join(".qwen")
}

/// 获取 Qwen .env 文件路径
pub fn get_qwen_env_path() -> PathBuf {
    get_qwen_dir().join(".env")
}

/// 获取 Qwen settings.json 文件路径
pub fn get_qwen_settings_path() -> PathBuf {
    get_qwen_dir().join("settings.json")
}

/// 是否存在任一可用的 Qwen live 配置
pub fn has_qwen_live_config() -> bool {
    let env_path = get_qwen_env_path();
    if env_path.exists() {
        match read_qwen_env() {
            Ok(env) if has_qwen_provider_env(&env) => return true,
            Ok(_) => {}
            Err(_) => return true,
        }
    }

    let settings_path = get_qwen_settings_path();
    if settings_path.exists() {
        match read_qwen_settings() {
            Ok(settings) => return has_qwen_provider_settings(&settings),
            Err(_) => return true,
        }
    }

    false
}

/// 解析 .env 文件内容为键值对
///
/// 支持的格式：
/// - KEY=value
/// - KEY="value with spaces"
/// - KEY='value with spaces'
/// - # 注释行
pub fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim();
            // 处理带引号的值
            let value = if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                if value.len() >= 2 {
                    &value[1..value.len() - 1]
                } else {
                    value
                }
            } else {
                value
            }
            .to_string();
            if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                map.insert(key, value);
            }
        }
    }
    map
}

/// 将键值对序列化为 .env 文件内容
pub fn serialize_env_file(map: &HashMap<String, String>) -> String {
    let mut lines = Vec::new();
    for (key, value) in map {
        lines.push(format!("{key}={value}"));
    }
    lines.join("\n")
}

/// 读取 Qwen .env 配置
pub fn read_qwen_env() -> Result<HashMap<String, String>, AppError> {
    let env_path = get_qwen_env_path();
    if !env_path.exists() {
        return Ok(HashMap::new());
    }
    let content = fs::read_to_string(&env_path).map_err(|e| AppError::Io {
        path: env_path.display().to_string(),
        source: e,
    })?;
    Ok(parse_env_file(&content))
}

/// 读取 Qwen settings.json（不存在时返回空对象）
pub fn read_qwen_settings() -> Result<Value, AppError> {
    let settings_path = get_qwen_settings_path();
    if !settings_path.exists() {
        return Ok(json!({}));
    }

    read_json_file(&settings_path)
}

/// 写入 Qwen .env 配置
pub fn write_qwen_env(env_map: &HashMap<String, String>) -> Result<(), AppError> {
    write_qwen_env_atomic(env_map)
}

/// 清空 Qwen live 配置
pub fn clear_qwen_live() -> Result<(), AppError> {
    let env_path = get_qwen_env_path();
    let mut env_map = read_qwen_env()?;
    remove_qwen_provider_env(&mut env_map);
    if env_map.is_empty() {
        remove_file_if_exists(&env_path)?;
    } else {
        write_qwen_env(&env_map)?;
    }

    let settings_path = get_qwen_settings_path();
    let mut settings = read_qwen_settings()?;
    if settings.is_object() {
        remove_qwen_provider_settings(&mut settings);
        if settings.as_object().is_some_and(|obj| obj.is_empty()) {
            remove_file_if_exists(&settings_path)?;
        } else {
            write_json_file(&settings_path, &settings)?;
        }
    }

    Ok(())
}

/// 将环境变量转换为 JSON 格式
pub fn env_to_json(env_map: &HashMap<String, String>) -> Value {
    let mut json_map = serde_json::Map::new();

    for (key, value) in env_map {
        json_map.insert(key.clone(), Value::String(value.clone()));
    }

    serde_json::json!({ "env": json_map })
}

/// 从 Provider.settings_config (JSON Value) 提取 .env 格式
pub fn json_to_env(settings: &Value) -> Result<HashMap<String, String>, AppError> {
    let mut env_map = HashMap::new();

    if let Some(env_obj) = settings.get("env").and_then(|v| v.as_object()) {
        for (key, value) in env_obj {
            if let Some(val_str) = value.as_str() {
                env_map.insert(key.clone(), val_str.to_string());
            }
        }
    }

    Ok(env_map)
}

fn merge_json(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, source_value) in source_map {
                match target_map.get_mut(key) {
                    Some(target_value) => merge_json(target_value, source_value),
                    None => {
                        target_map.insert(key.clone(), source_value.clone());
                    }
                }
            }
        }
        (target_value, source_value) => {
            *target_value = source_value.clone();
        }
    }
}

fn extract_settings_env(settings: &Value) -> HashMap<String, String> {
    let mut env_map = HashMap::new();

    if let Some(env_obj) = settings.get("env").and_then(Value::as_object) {
        for (key, value) in env_obj {
            if let Some(val_str) = value.as_str() {
                env_map.insert(key.clone(), val_str.to_string());
            }
        }
    }

    env_map
}

fn has_qwen_provider_env(env_map: &HashMap<String, String>) -> bool {
    ["OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_MODEL"]
        .iter()
        .any(|key| env_map.contains_key(*key))
}

fn remove_qwen_provider_env(env_map: &mut HashMap<String, String>) {
    env_map.remove("OPENAI_API_KEY");
    env_map.remove("OPENAI_BASE_URL");
    env_map.remove("OPENAI_MODEL");
}

fn has_qwen_provider_settings(settings: &Value) -> bool {
    if extract_settings_env(settings).keys().any(|key| {
        matches!(
            key.as_str(),
            "OPENAI_API_KEY" | "OPENAI_BASE_URL" | "OPENAI_MODEL"
        )
    }) {
        return true;
    }

    settings.get("modelProviders").is_some()
        || settings.pointer("/security/auth/selectedType").is_some()
        || settings.pointer("/model/name").is_some()
}

fn remove_qwen_provider_settings(settings: &mut Value) {
    if let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) {
        env.remove("OPENAI_API_KEY");
        env.remove("OPENAI_BASE_URL");
        env.remove("OPENAI_MODEL");
        if env.is_empty() {
            settings.as_object_mut().map(|obj| obj.remove("env"));
        }
    }

    if let Some(obj) = settings.as_object_mut() {
        obj.remove("modelProviders");

        if let Some(security) = obj.get_mut("security").and_then(Value::as_object_mut) {
            if let Some(auth) = security.get_mut("auth").and_then(Value::as_object_mut) {
                auth.remove("selectedType");
                if auth.is_empty() {
                    security.remove("auth");
                }
            }
            if security.is_empty() {
                obj.remove("security");
            }
        }

        if let Some(model) = obj.get_mut("model").and_then(Value::as_object_mut) {
            model.remove("name");
            if model.is_empty() {
                obj.remove("model");
            }
        }
    }
}

fn remove_file_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(AppError::io(path, err)),
    }
}

fn normalize_qwen_provider_config(settings: &Value) -> Value {
    let mut normalized = serde_json::Map::new();

    if let Some(model_providers) = settings.get("modelProviders") {
        normalized.insert("modelProviders".to_string(), model_providers.clone());
    }

    if let Some(selected_type) = settings
        .get("security")
        .and_then(|v| v.get("auth"))
        .and_then(|v| v.get("selectedType"))
    {
        normalized.insert(
            "security".to_string(),
            json!({
                "auth": {
                    "selectedType": selected_type.clone()
                }
            }),
        );
    }

    if let Some(model_name) = settings.get("model").and_then(|v| v.get("name")) {
        normalized.insert(
            "model".to_string(),
            json!({
                "name": model_name.clone()
            }),
        );
    }

    Value::Object(normalized)
}

/// 读取当前 Qwen live 配置并归一化为 Provider.settings_config 结构
///
/// 返回结构：
/// `{ "env": {...}, "config": {...} }`
///
/// 规则：
/// - `settings.json` 中的 `env` 优先
/// - `.env` 仅为缺失字段提供回退
/// - `config` 只保留供应商相关字段，避免把共享设置直接吸收到 provider 中
pub fn read_qwen_live_config() -> Result<Value, AppError> {
    let settings = read_qwen_settings()?;

    let mut env_map = extract_settings_env(&settings);
    for (key, value) in read_qwen_env()? {
        env_map.entry(key).or_insert(value);
    }

    let mut live = env_to_json(&env_map);
    if let Some(obj) = live.as_object_mut() {
        obj.insert(
            "config".to_string(),
            normalize_qwen_provider_config(&settings),
        );
    }

    Ok(live)
}

/// 写入 Qwen live 配置，优先更新 settings.json，并同步 .env 作为兼容回退
pub fn write_qwen_live_settings(settings: &Value) -> Result<(), AppError> {
    let settings_obj = settings.as_object().ok_or_else(|| {
        AppError::localized(
            "qwen.validation.invalid_settings",
            "Qwen 配置必须是 JSON 对象",
            "Qwen config must be a JSON object",
        )
    })?;

    let provider_env = json_to_env(settings)?;
    let mut merged_settings = read_qwen_settings()?;
    if !merged_settings.is_object() {
        merged_settings = json!({});
    }

    let mut merged_env = extract_settings_env(&merged_settings);
    for (key, value) in read_qwen_env()? {
        merged_env.entry(key).or_insert(value);
    }
    for (key, value) in provider_env {
        merged_env.insert(key, value);
    }

    if let Some(obj) = merged_settings.as_object_mut() {
        obj.insert(
            "env".to_string(),
            env_to_json(&merged_env)
                .get("env")
                .cloned()
                .unwrap_or_else(|| json!({})),
        );
    }

    if let Some(config_value) = settings_obj.get("config") {
        if config_value.is_object() {
            merge_json(&mut merged_settings, config_value);
        } else if !config_value.is_null() {
            return Err(AppError::localized(
                "qwen.validation.invalid_config",
                "Qwen 配置格式错误: config 必须是对象或 null",
                "Qwen config invalid: config must be an object or null",
            ));
        }
    }

    write_json_file(&get_qwen_settings_path(), &merged_settings)?;
    write_qwen_env_atomic(&merged_env)?;

    Ok(())
}

/// 写入 Qwen .env 文件（原子操作）
pub fn write_qwen_env_atomic(map: &HashMap<String, String>) -> Result<(), AppError> {
    let path = get_qwen_env_path();

    // 确保目录存在
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

    let content = serialize_env_file(map);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    fn with_test_home<T>(test: impl FnOnce() -> T) -> T {
        let _guard = test_guard();
        let temp = tempfile::tempdir().expect("tempdir");
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let old_home = std::env::var_os("HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::set_var("HOME", temp.path());
        let result = test();
        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        result
    }

    #[test]
    fn read_qwen_live_config_prefers_settings_json_env_and_keeps_provider_config() {
        with_test_home(|| {
            let qwen_dir = get_qwen_dir();
            fs::create_dir_all(&qwen_dir).expect("create qwen dir");
            fs::write(
                get_qwen_env_path(),
                "OPENAI_API_KEY=env-file-key\nOPENAI_BASE_URL=https://env.example/v1\n",
            )
            .expect("write env");
            crate::config::write_json_file(
                &get_qwen_settings_path(),
                &json!({
                    "env": {
                        "OPENAI_API_KEY": "settings-key",
                        "OPENAI_MODEL": "qwen3-coder-plus"
                    },
                    "modelProviders": {
                        "dashscope": {
                            "baseURL": "https://dashscope.aliyuncs.com/compatible-mode/v1"
                        }
                    },
                    "security": {
                        "auth": {
                            "selectedType": "openai"
                        }
                    },
                    "model": {
                        "name": "qwen3-coder-plus"
                    },
                    "mcpServers": {
                        "keep-me": {
                            "command": "uvx"
                        }
                    }
                }),
            )
            .expect("write settings");

            let live = read_qwen_live_config().expect("read live config");

            assert_eq!(live["env"]["OPENAI_API_KEY"], json!("settings-key"));
            assert_eq!(
                live["env"]["OPENAI_BASE_URL"],
                json!("https://env.example/v1")
            );
            assert_eq!(live["env"]["OPENAI_MODEL"], json!("qwen3-coder-plus"));
            assert!(live["config"].get("modelProviders").is_some());
            assert_eq!(
                live["config"]["security"]["auth"]["selectedType"],
                json!("openai")
            );
            assert_eq!(live["config"]["model"]["name"], json!("qwen3-coder-plus"));
            assert!(
                live["config"].get("mcpServers").is_none(),
                "shared fields should not be normalized into provider config"
            );
        });
    }

    #[test]
    fn write_qwen_live_settings_preserves_existing_shared_settings() {
        with_test_home(|| {
            let qwen_dir = get_qwen_dir();
            fs::create_dir_all(&qwen_dir).expect("create qwen dir");
            crate::config::write_json_file(
                &get_qwen_settings_path(),
                &json!({
                    "mcpServers": {
                        "persisted": {
                            "command": "uvx"
                        }
                    },
                    "theme": "dark"
                }),
            )
            .expect("write initial settings");

            write_qwen_live_settings(&json!({
                "env": {
                    "OPENAI_API_KEY": "new-key",
                    "OPENAI_BASE_URL": "https://dashscope.aliyuncs.com/compatible-mode/v1",
                    "OPENAI_MODEL": "qwen3-coder-plus"
                },
                "config": {
                    "modelProviders": {
                        "dashscope": {
                            "baseURL": "https://dashscope.aliyuncs.com/compatible-mode/v1"
                        }
                    },
                    "security": {
                        "auth": {
                            "selectedType": "openai"
                        }
                    },
                    "model": {
                        "name": "qwen3-coder-plus"
                    }
                }
            }))
            .expect("write live settings");

            let saved_settings: Value =
                crate::config::read_json_file(&get_qwen_settings_path()).expect("read settings");
            let env_map = read_qwen_env().expect("read env");

            assert_eq!(saved_settings["theme"], json!("dark"));
            assert!(saved_settings["mcpServers"]["persisted"].is_object());
            assert_eq!(
                saved_settings["security"]["auth"]["selectedType"],
                json!("openai")
            );
            assert_eq!(saved_settings["model"]["name"], json!("qwen3-coder-plus"));
            assert_eq!(env_map.get("OPENAI_API_KEY"), Some(&"new-key".to_string()));
        });
    }

    #[test]
    fn has_qwen_live_config_detects_settings_json_without_env_file() {
        with_test_home(|| {
            let qwen_dir = get_qwen_dir();
            fs::create_dir_all(&qwen_dir).expect("create qwen dir");
            crate::config::write_json_file(
                &get_qwen_settings_path(),
                &json!({
                    "model": {
                        "name": "qwen3-coder-plus"
                    }
                }),
            )
            .expect("write settings");

            assert!(has_qwen_live_config());
        });
    }

    #[test]
    fn clear_qwen_live_removes_provider_fields_but_preserves_shared_settings() {
        with_test_home(|| {
            let qwen_dir = get_qwen_dir();
            fs::create_dir_all(&qwen_dir).expect("create qwen dir");
            fs::write(
                get_qwen_env_path(),
                "OPENAI_API_KEY=env-key\nOPENAI_BASE_URL=https://env.example/v1\nOPENAI_MODEL=qwen3-coder-plus\nQWEN_TRACE=1\n",
            )
            .expect("write env");
            crate::config::write_json_file(
                &get_qwen_settings_path(),
                &json!({
                    "env": {
                        "OPENAI_API_KEY": "settings-key",
                        "OPENAI_BASE_URL": "https://settings.example/v1",
                        "OPENAI_MODEL": "qwen3-coder-plus",
                        "QWEN_TRACE": "1"
                    },
                    "modelProviders": {
                        "dashscope": {
                            "baseURL": "https://settings.example/v1"
                        }
                    },
                    "security": {
                        "auth": {
                            "selectedType": "openai"
                        }
                    },
                    "model": {
                        "name": "qwen3-coder-plus"
                    },
                    "mcpServers": {
                        "shared": {
                            "command": "uvx"
                        }
                    },
                    "ui": {
                        "locale": "zh-CN"
                    }
                }),
            )
            .expect("write settings");

            clear_qwen_live().expect("clear qwen live");

            let saved_settings: Value =
                crate::config::read_json_file(&get_qwen_settings_path()).expect("read settings");
            let env_map = read_qwen_env().expect("read env");

            assert_eq!(saved_settings["env"]["QWEN_TRACE"], json!("1"));
            assert!(saved_settings["env"].get("OPENAI_API_KEY").is_none());
            assert!(saved_settings.get("modelProviders").is_none());
            assert!(saved_settings.get("model").is_none());
            assert!(saved_settings.get("security").is_none());
            assert_eq!(
                saved_settings["mcpServers"]["shared"]["command"],
                json!("uvx")
            );
            assert_eq!(saved_settings["ui"]["locale"], json!("zh-CN"));
            assert_eq!(env_map.get("QWEN_TRACE"), Some(&"1".to_string()));
            assert!(!env_map.contains_key("OPENAI_API_KEY"));
            assert!(
                !has_qwen_live_config(),
                "shared-only settings should not count as provider live config"
            );
        });
    }
}
