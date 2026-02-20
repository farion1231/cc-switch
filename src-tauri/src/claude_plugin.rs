use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::database::Database;
use crate::error::AppError;

const CLAUDE_DIR: &str = ".claude";
const CLAUDE_CONFIG_FILE: &str = "config.json";

fn claude_dir() -> Result<PathBuf, AppError> {
    // 优先使用设置中的覆盖目录
    if let Some(dir) = crate::settings::get_claude_override_dir() {
        return Ok(dir);
    }
    Ok(crate::config::get_home_dir().join(CLAUDE_DIR))
}

pub fn claude_config_path() -> Result<PathBuf, AppError> {
    Ok(claude_dir()?.join(CLAUDE_CONFIG_FILE))
}

pub fn ensure_claude_dir_exists() -> Result<PathBuf, AppError> {
    let dir = claude_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    }
    Ok(dir)
}

pub fn read_claude_config() -> Result<Option<String>, AppError> {
    let path = claude_config_path()?;
    if path.exists() {
        let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
        Ok(Some(content))
    } else {
        Ok(None)
    }
}

fn is_managed_config(content: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(value) => value
            .get("primaryApiKey")
            .and_then(|v| v.as_str())
            .map(|val| val == "any")
            .unwrap_or(false),
        Err(_) => false,
    }
}

pub fn write_claude_config() -> Result<bool, AppError> {
    // 增量写入：仅设置 primaryApiKey = "any"，保留其它字段
    let path = claude_config_path()?;
    ensure_claude_dir_exists()?;

    // 尝试读取并解析为对象
    let mut obj = match read_claude_config()? {
        Some(existing) => match serde_json::from_str::<serde_json::Value>(&existing) {
            Ok(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
            _ => serde_json::json!({}),
        },
        None => serde_json::json!({}),
    };

    let mut changed = false;
    if let Some(map) = obj.as_object_mut() {
        let cur = map
            .get("primaryApiKey")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if cur != "any" {
            map.insert(
                "primaryApiKey".to_string(),
                serde_json::Value::String("any".to_string()),
            );
            changed = true;
        }
    }

    if changed || !path.exists() {
        let serialized = serde_json::to_string_pretty(&obj)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        fs::write(&path, format!("{serialized}\n")).map_err(|e| AppError::io(&path, e))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn clear_claude_config() -> Result<bool, AppError> {
    let path = claude_config_path()?;
    if !path.exists() {
        return Ok(false);
    }

    let content = match read_claude_config()? {
        Some(content) => content,
        None => return Ok(false),
    };

    let mut value = match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };

    let obj = match value.as_object_mut() {
        Some(obj) => obj,
        None => return Ok(false),
    };

    if obj.remove("primaryApiKey").is_none() {
        return Ok(false);
    }

    let serialized =
        serde_json::to_string_pretty(&value).map_err(|e| AppError::JsonSerialize { source: e })?;
    fs::write(&path, format!("{serialized}\n")).map_err(|e| AppError::io(&path, e))?;
    Ok(true)
}

pub fn claude_config_status() -> Result<(bool, PathBuf), AppError> {
    let path = claude_config_path()?;
    Ok((path.exists(), path))
}

/// 写入 config.json（含 enabledPlugins，从数据库读取插件状态）
pub fn write_claude_config_with_db(db: &Arc<Database>) -> Result<bool, AppError> {
    let path = claude_config_path()?;
    ensure_claude_dir_exists()?;

    let mut obj = match read_claude_config()? {
        Some(existing) => match serde_json::from_str::<serde_json::Value>(&existing) {
            Ok(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
            _ => serde_json::json!({}),
        },
        None => serde_json::json!({}),
    };

    let map = obj.as_object_mut().unwrap();

    // 写入 primaryApiKey
    map.insert(
        "primaryApiKey".to_string(),
        serde_json::Value::String("any".to_string()),
    );

    // 写入 enabledPlugins
    let plugins_map = db.get_enabled_plugins_map().unwrap_or_default();
    let plugins_json: serde_json::Map<String, serde_json::Value> = plugins_map
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::Bool(v)))
        .collect();
    map.insert(
        "enabledPlugins".to_string(),
        serde_json::Value::Object(plugins_json),
    );

    let serialized = serde_json::to_string_pretty(&obj)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    fs::write(&path, format!("{serialized}\n")).map_err(|e| AppError::io(&path, e))?;
    Ok(true)
}

pub fn is_claude_config_applied() -> Result<bool, AppError> {
    match read_claude_config()? {
        Some(content) => Ok(is_managed_config(&content)),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serial_test::serial;
    use std::sync::Arc;

    fn setup_test_env() -> (tempfile::TempDir, Arc<Database>) {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path().to_str().unwrap());
        let db = Arc::new(Database::memory().unwrap());
        (dir, db)
    }

    #[test]
    #[serial]
    fn test_write_config_includes_enabled_plugins() {
        let (_dir, db) = setup_test_env();
        db.upsert_plugin_state("p@r", "/p", Some("1.0"), "user").unwrap();
        write_claude_config_with_db(&db).unwrap();
        let content = read_claude_config().unwrap().unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(val["primaryApiKey"], "any");
        assert_eq!(val["enabledPlugins"]["p@r"], true);
    }

    #[test]
    #[serial]
    fn test_write_config_no_plugins_writes_empty_object() {
        let (_dir, db) = setup_test_env();
        write_claude_config_with_db(&db).unwrap();
        let content = read_claude_config().unwrap().unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(val["enabledPlugins"], serde_json::json!({}));
    }

    #[test]
    #[serial]
    fn test_write_config_preserves_other_fields() {
        let (_dir, db) = setup_test_env();
        let path = claude_config_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"skipDangerousModePermissionPrompt": true}"#).unwrap();
        write_claude_config_with_db(&db).unwrap();
        let content = read_claude_config().unwrap().unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(val["skipDangerousModePermissionPrompt"], true);
        assert_eq!(val["primaryApiKey"], "any");
    }
}
