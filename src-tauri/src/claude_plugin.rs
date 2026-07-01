use std::fs;
use std::path::{Path, PathBuf};

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::ClaudeActivationMode;

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

fn resolve_claude_profile_dir(raw: &str) -> PathBuf {
    if raw == "~" {
        return crate::config::get_home_dir();
    }
    if let Some(stripped) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return crate::config::get_home_dir().join(stripped);
    }
    PathBuf::from(raw)
}

fn active_profile_and_config_dir(db: &Database) -> Result<Option<PathBuf>, AppError> {
    let Some(provider_id) = crate::settings::get_effective_current_provider(db, &AppType::Claude)?
    else {
        return Ok(None);
    };
    let Some(provider) = db.get_provider_by_id(&provider_id, AppType::Claude.as_str())? else {
        return Ok(None);
    };
    let Some(meta) = provider.meta.as_ref() else {
        return Ok(None);
    };
    if meta.claude_activation_mode.as_ref() != Some(&ClaudeActivationMode::ProfileAndConfig) {
        return Ok(None);
    }

    Ok(meta
        .claude_profile_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(resolve_claude_profile_dir))
}

fn claude_dir_for_db(db: &Database) -> Result<PathBuf, AppError> {
    if let Some(dir) = active_profile_and_config_dir(db)? {
        return Ok(dir);
    }
    claude_dir()
}

pub fn claude_config_path_for_db(db: &Database) -> Result<PathBuf, AppError> {
    Ok(claude_dir_for_db(db)?.join(CLAUDE_CONFIG_FILE))
}

fn read_claude_config_at(path: &Path) -> Result<Option<String>, AppError> {
    if path.exists() {
        let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
        Ok(Some(content))
    } else {
        Ok(None)
    }
}

pub fn read_claude_config_for_db(db: &Database) -> Result<Option<String>, AppError> {
    read_claude_config_at(&claude_config_path_for_db(db)?)
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

fn write_claude_config_at(path: &Path) -> Result<bool, AppError> {
    // 增量写入：仅设置 primaryApiKey = "any"，保留其它字段
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // 尝试读取并解析为对象
    let mut obj = match read_claude_config_at(path)? {
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
        fs::write(path, format!("{serialized}\n")).map_err(|e| AppError::io(path, e))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn write_claude_config_for_db(db: &Database) -> Result<bool, AppError> {
    write_claude_config_at(&claude_config_path_for_db(db)?)
}

fn clear_claude_config_at(path: &Path) -> Result<bool, AppError> {
    if !path.exists() {
        return Ok(false);
    }

    let content = match read_claude_config_at(path)? {
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
    fs::write(path, format!("{serialized}\n")).map_err(|e| AppError::io(path, e))?;
    Ok(true)
}

pub fn clear_claude_config_for_db(db: &Database) -> Result<bool, AppError> {
    let active_path = claude_config_path_for_db(db)?;
    let mut changed = clear_claude_config_at(&active_path)?;

    let default_path = claude_dir()?.join(CLAUDE_CONFIG_FILE);
    if default_path != active_path {
        changed |= clear_claude_config_at(&default_path)?;
    }

    Ok(changed)
}

pub fn claude_config_status_for_db(db: &Database) -> Result<(bool, PathBuf), AppError> {
    let path = claude_config_path_for_db(db)?;
    Ok((path.exists(), path))
}

pub fn is_claude_config_applied_for_db(db: &Database) -> Result<bool, AppError> {
    match read_claude_config_for_db(db)? {
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

    fn save_current_claude_provider(
        db: &crate::database::Database,
        id: &str,
        activation_mode: crate::provider::ClaudeActivationMode,
        profile_dir: Option<&std::path::Path>,
    ) {
        let provider = crate::provider::Provider {
            id: id.to_string(),
            name: id.to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(1),
            sort_index: Some(0),
            notes: None,
            meta: Some(crate::provider::ProviderMeta {
                claude_profile_dir: profile_dir.map(|path| path.to_string_lossy().into_owned()),
                claude_activation_mode: Some(activation_mode),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        db.save_provider(crate::app_config::AppType::Claude.as_str(), &provider)
            .expect("save Claude provider");
        db.set_current_provider(crate::app_config::AppType::Claude.as_str(), id)
            .expect("set current Claude provider");
        crate::settings::set_current_provider(&crate::app_config::AppType::Claude, Some(id))
            .expect("set local current Claude provider");
    }

    #[test]
    #[serial]
    fn claude_plugin_config_path_ignores_provider_profile_override() {
        let home = TempHome::new();
        let db = crate::database::Database::memory().expect("memory database");
        let profile_dir = home.path().join("external-profile");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_provider_config_dir: Some(profile_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set provider profile override");

        let path = claude_config_path_for_db(&db).expect("claude plugin config path");

        assert_eq!(path, home.path().join(".claude").join("config.json"));
    }

    #[test]
    #[serial]
    fn claude_plugin_config_path_uses_active_profile_for_profile_and_config() {
        let home = TempHome::new();
        let db = crate::database::Database::memory().expect("memory database");
        let configured_dir = home.path().join("configured-claude");
        let profile_dir = home.path().join("profile-and-config");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_config_dir: Some(configured_dir.to_string_lossy().into_owned()),
            claude_provider_config_dir: Some(profile_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set Claude dirs");
        save_current_claude_provider(
            &db,
            "profile-and-config",
            crate::provider::ClaudeActivationMode::ProfileAndConfig,
            Some(&profile_dir),
        );

        let path = claude_config_path_for_db(&db).expect("Claude plugin config path");

        assert_eq!(path, profile_dir.join("config.json"));
    }

    #[test]
    #[serial]
    fn claude_plugin_config_path_keeps_configured_dir_for_profile_only() {
        let home = TempHome::new();
        let db = crate::database::Database::memory().expect("memory database");
        let configured_dir = home.path().join("configured-claude");
        let profile_dir = home.path().join("profile-only");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_config_dir: Some(configured_dir.to_string_lossy().into_owned()),
            claude_provider_config_dir: Some(profile_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set Claude dirs");
        save_current_claude_provider(
            &db,
            "profile-only",
            crate::provider::ClaudeActivationMode::ProfileOnly,
            Some(&profile_dir),
        );

        let path = claude_config_path_for_db(&db).expect("Claude plugin config path");

        assert_eq!(path, configured_dir.join("config.json"));
    }

    #[test]
    #[serial]
    fn clear_profile_and_config_plugin_config_also_clears_configured_dir() {
        let home = TempHome::new();
        let db = crate::database::Database::memory().expect("memory database");
        let configured_dir = home.path().join("configured-claude");
        let profile_dir = home.path().join("profile-and-config");
        let configured_config = configured_dir.join("config.json");
        let profile_config = profile_dir.join("config.json");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_config_dir: Some(configured_dir.to_string_lossy().into_owned()),
            claude_provider_config_dir: Some(profile_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set Claude dirs");
        save_current_claude_provider(
            &db,
            "profile-and-config",
            crate::provider::ClaudeActivationMode::ProfileAndConfig,
            Some(&profile_dir),
        );
        fs::create_dir_all(&configured_dir).expect("create configured Claude dir");
        fs::create_dir_all(&profile_dir).expect("create profile Claude dir");
        fs::write(
            &configured_config,
            r#"{"primaryApiKey":"any","configured":true}"#,
        )
        .expect("write configured plugin config");
        fs::write(&profile_config, r#"{"primaryApiKey":"any","profile":true}"#)
            .expect("write profile plugin config");

        let changed = clear_claude_config_for_db(&db).expect("clear plugin config");

        assert!(changed);
        let configured_content =
            fs::read_to_string(&configured_config).expect("read configured plugin config");
        let profile_content =
            fs::read_to_string(&profile_config).expect("read profile plugin config");
        assert!(!configured_content.contains("primaryApiKey"));
        assert!(!profile_content.contains("primaryApiKey"));
        assert!(configured_content.contains("configured"));
        assert!(profile_content.contains("profile"));
    }
}
