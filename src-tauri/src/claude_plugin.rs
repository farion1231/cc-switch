use std::fs;
use std::path::PathBuf;

use crate::error::AppError;

const CLAUDE_DIR: &str = ".claude";
const CLAUDE_CONFIG_FILE: &str = "config.json";

fn claude_dir() -> Result<PathBuf, AppError> {
    // Prompt/plugin files belong to the configured/default Claude directory,
    // not the transient provider profile directory.
    if let Some(dir) = crate::settings::get_claude_configured_override_dir() {
        return Ok(dir);
    }
    let home = crate::config::get_home_dir();
    Ok(home.join(CLAUDE_DIR))
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

pub fn is_claude_config_applied() -> Result<bool, AppError> {
    match read_claude_config()? {
        Some(content) => Ok(is_managed_config(&content)),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }

        fn path(&self) -> &std::path::Path {
            self.dir.path()
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = crate::settings::update_settings(crate::settings::AppSettings::default());
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    #[test]
    #[serial]
    fn claude_plugin_config_path_ignores_provider_profile_override() {
        let home = TempHome::new();
        let profile_dir = home.path().join("external-profile");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_provider_config_dir: Some(profile_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set provider profile override");

        let path = claude_config_path().expect("claude plugin config path");

        assert_eq!(path, home.path().join(".claude").join("config.json"));
    }
}
