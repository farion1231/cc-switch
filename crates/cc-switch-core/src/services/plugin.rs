use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::config::{
    atomic_write, get_claude_config_dir, get_claude_mcp_path, get_default_claude_mcp_path,
    ConfigStatus,
};
use crate::error::AppError;

pub struct ClaudePluginService;

impl ClaudePluginService {
    pub fn get_status() -> Result<ConfigStatus, AppError> {
        let path = Self::config_path();
        Ok(ConfigStatus {
            exists: path.exists(),
            path: path.to_string_lossy().to_string(),
        })
    }

    pub fn read_config() -> Result<Option<String>, AppError> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
        Ok(Some(content))
    }

    pub fn apply_config(official: bool) -> Result<bool, AppError> {
        if official {
            Self::clear_config()
        } else {
            Self::write_managed_config()
        }
    }

    pub fn is_applied() -> Result<bool, AppError> {
        match Self::read_config()? {
            Some(content) => Ok(Self::is_managed_config(&content)),
            None => Ok(false),
        }
    }

    pub fn is_onboarding_skip_applied() -> Result<bool, AppError> {
        let path = Self::onboarding_path();
        if !path.exists() {
            return Ok(false);
        }

        let value = Self::read_json_value(&path)?;
        Ok(value
            .get("hasCompletedOnboarding")
            .and_then(|item| item.as_bool())
            .unwrap_or(false))
    }

    pub fn apply_onboarding_skip() -> Result<bool, AppError> {
        let path = Self::onboarding_path();
        let mut root = if path.exists() {
            Self::read_json_value(&path)?
        } else {
            json!({})
        };

        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;

        if obj
            .get("hasCompletedOnboarding")
            .and_then(|item| item.as_bool())
            .unwrap_or(false)
        {
            return Ok(false);
        }

        obj.insert("hasCompletedOnboarding".into(), Value::Bool(true));
        Self::write_json_value(&path, &root)?;
        Ok(true)
    }

    pub fn clear_onboarding_skip() -> Result<bool, AppError> {
        let path = Self::onboarding_path();
        if !path.exists() {
            return Ok(false);
        }

        let mut root = Self::read_json_value(&path)?;
        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;

        if obj.remove("hasCompletedOnboarding").is_none() {
            return Ok(false);
        }

        Self::write_json_value(&path, &root)?;
        Ok(true)
    }

    fn config_path() -> PathBuf {
        get_claude_config_dir().join("config.json")
    }

    fn onboarding_path() -> PathBuf {
        Self::ensure_onboarding_override_migrated();
        get_claude_mcp_path()
    }

    fn ensure_claude_dir_exists() -> Result<PathBuf, AppError> {
        let dir = get_claude_config_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
        }
        Ok(dir)
    }

    fn write_managed_config() -> Result<bool, AppError> {
        let path = Self::config_path();
        Self::ensure_claude_dir_exists()?;

        let mut value = match Self::read_config()? {
            Some(existing) => {
                serde_json::from_str::<Value>(&existing).unwrap_or_else(|_| json!({}))
            }
            None => json!({}),
        };

        let obj = value
            .as_object_mut()
            .ok_or_else(|| AppError::Config("Claude 插件配置必须是 JSON 对象".to_string()))?;

        let current = obj
            .get("primaryApiKey")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        if current == "any" && path.exists() {
            return Ok(false);
        }

        obj.insert("primaryApiKey".into(), Value::String("any".to_string()));
        let serialized = serde_json::to_string_pretty(&value)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        atomic_write(&path, format!("{serialized}\n").as_bytes())?;
        Ok(true)
    }

    fn clear_config() -> Result<bool, AppError> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(false);
        }

        let content = match Self::read_config()? {
            Some(content) => content,
            None => return Ok(false),
        };
        let mut value = match serde_json::from_str::<Value>(&content) {
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

        let serialized = serde_json::to_string_pretty(&value)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        atomic_write(&path, format!("{serialized}\n").as_bytes())?;
        Ok(true)
    }

    fn read_json_value(path: &Path) -> Result<Value, AppError> {
        let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
        serde_json::from_str(&content).map_err(|e| AppError::json(path, e))
    }

    fn write_json_value(path: &Path, value: &Value) -> Result<(), AppError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let json = serde_json::to_string_pretty(value)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        atomic_write(path, format!("{json}\n").as_bytes())
    }

    fn is_managed_config(content: &str) -> bool {
        serde_json::from_str::<Value>(content)
            .ok()
            .and_then(|value| {
                value
                    .get("primaryApiKey")
                    .and_then(|item| item.as_str())
                    .map(|item| item == "any")
            })
            .unwrap_or(false)
    }

    fn ensure_onboarding_override_migrated() {
        let new_path = get_claude_mcp_path();
        if new_path.exists() || new_path == get_default_claude_mcp_path() {
            return;
        }

        let legacy_path = get_default_claude_mcp_path();
        if !legacy_path.exists() {
            return;
        }

        if let Some(parent) = new_path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                log::warn!("创建 Claude onboarding 目录失败: {err}");
                return;
            }
        }

        if let Err(err) = fs::copy(&legacy_path, &new_path) {
            log::warn!(
                "复制 Claude onboarding 配置失败: {} -> {}: {}",
                legacy_path.display(),
                new_path.display(),
                err
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClaudePluginService;
    use crate::config::{get_claude_config_dir, get_claude_mcp_path};
    use crate::settings::{update_settings, AppSettings};
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn managed_plugin_config_round_trip() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        update_settings(AppSettings::default())?;

        assert!(!ClaudePluginService::is_applied()?);
        assert!(ClaudePluginService::apply_config(false)?);
        assert!(ClaudePluginService::is_applied()?);

        let path = get_claude_config_dir().join("config.json");
        let content = std::fs::read_to_string(&path).expect("config content");
        assert!(content.contains("\"primaryApiKey\": \"any\""));

        assert!(ClaudePluginService::apply_config(true)?);
        assert!(!ClaudePluginService::is_applied()?);

        Ok(())
    }

    #[test]
    #[serial]
    fn onboarding_skip_round_trip() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        update_settings(AppSettings::default())?;

        assert!(!ClaudePluginService::is_onboarding_skip_applied()?);
        assert!(ClaudePluginService::apply_onboarding_skip()?);
        assert!(ClaudePluginService::is_onboarding_skip_applied()?);

        let path = get_claude_mcp_path();
        let content = std::fs::read_to_string(&path).expect("onboarding content");
        assert!(content.contains("\"hasCompletedOnboarding\": true"));

        assert!(ClaudePluginService::clear_onboarding_skip()?);
        assert!(!ClaudePluginService::is_onboarding_skip_applied()?);

        Ok(())
    }

    #[test]
    #[serial]
    fn status_reports_target_path() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        update_settings(AppSettings::default())?;

        let status = ClaudePluginService::get_status()?;
        assert!(!status.exists);
        assert!(status.path.ends_with(".claude/config.json"));

        Ok(())
    }
}
