use std::path::PathBuf;

use crate::app_config::AppType;
use crate::codex_config::get_codex_auth_path;
use crate::config::get_home_dir;
use crate::database::Database;
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::openclaw_config::get_openclaw_dir;
use crate::opencode_config::get_opencode_dir;
use crate::provider::ClaudeActivationMode;

/// 返回指定应用所使用的提示词文件路径。
pub fn prompt_file_path(app: &AppType) -> Result<PathBuf, AppError> {
    if matches!(app, AppType::ClaudeDesktop) {
        return Err(AppError::localized(
            "claude_desktop.prompts_unsupported",
            "Claude Desktop 暂不支持 Prompts",
            "Claude Desktop does not support Prompts",
        ));
    }

    let base_dir: PathBuf = match app {
        AppType::Claude => crate::settings::get_claude_configured_override_dir()
            .unwrap_or_else(|| get_home_dir().join(".claude")),
        AppType::Codex => get_base_dir_with_fallback(get_codex_auth_path(), ".codex")?,
        AppType::Gemini => get_gemini_dir(),
        AppType::OpenCode => get_opencode_dir(),
        AppType::OpenClaw => get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
        AppType::ClaudeDesktop => unreachable!("handled above"),
    };

    let filename = match app {
        AppType::Claude => "CLAUDE.md",
        AppType::Codex => "AGENTS.md",
        AppType::Gemini => "GEMINI.md",
        AppType::OpenCode | AppType::OpenClaw | AppType::Hermes => "AGENTS.md",
        AppType::ClaudeDesktop => unreachable!("handled above"),
    };

    Ok(base_dir.join(filename))
}

pub fn prompt_file_path_for_db(db: &Database, app: &AppType) -> Result<PathBuf, AppError> {
    if matches!(app, AppType::Claude) {
        if let Some(profile_prompt_path) = current_profile_and_config_claude_prompt_path(db)? {
            return Ok(profile_prompt_path);
        }
    }

    prompt_file_path(app)
}

fn current_profile_and_config_claude_prompt_path(
    db: &Database,
) -> Result<Option<PathBuf>, AppError> {
    let Some(current_id) = crate::settings::get_effective_current_provider(db, &AppType::Claude)?
    else {
        return Ok(None);
    };

    let Some(provider) = db.get_provider_by_id(&current_id, AppType::Claude.as_str())? else {
        return Ok(None);
    };
    let Some(meta) = provider.meta.as_ref() else {
        return Ok(None);
    };
    if !matches!(
        meta.claude_activation_mode.as_ref(),
        Some(ClaudeActivationMode::ProfileAndConfig)
    ) {
        return Ok(None);
    }

    let raw_profile_dir = meta
        .claude_profile_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::Message(
                "Claude profile-and-config prompt sync requires a profile directory".to_string(),
            )
        })?;
    let profile_dir = resolve_profile_path(raw_profile_dir);
    if !profile_dir.is_absolute() {
        return Err(AppError::Message(format!(
            "Claude profile-and-config prompt sync requires an absolute profile directory: {raw_profile_dir}"
        )));
    }

    Ok(Some(profile_dir.join("CLAUDE.md")))
}

fn resolve_profile_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return get_home_dir();
    }
    if let Some(stripped) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return get_home_dir().join(stripped);
    }
    PathBuf::from(raw)
}

fn get_base_dir_with_fallback(
    primary_path: PathBuf,
    fallback_dir: &str,
) -> Result<PathBuf, AppError> {
    primary_path
        .parent()
        .map(|p| p.to_path_buf())
        .or_else(|| dirs::home_dir().map(|h| h.join(fallback_dir)))
        .ok_or_else(|| {
            AppError::localized(
                "home_dir_not_found",
                format!("无法确定 {fallback_dir} 配置目录：用户主目录不存在"),
                format!("Cannot determine {fallback_dir} config directory: user home not found"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::{ClaudeActivationMode, Provider, ProviderMeta};
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use std::sync::Arc;
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

    fn claude_provider_with_profile(
        id: &str,
        profile_dir: &std::path::Path,
        activation_mode: ClaudeActivationMode,
    ) -> Provider {
        Provider {
            id: id.to_string(),
            name: format!("Provider {id}"),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "test-token",
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(1),
            sort_index: Some(0),
            notes: None,
            meta: Some(ProviderMeta {
                claude_profile_dir: Some(profile_dir.to_string_lossy().to_string()),
                claude_activation_mode: Some(activation_mode),
                ..ProviderMeta::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    #[serial]
    fn claude_prompt_path_ignores_provider_profile_override() {
        let home = TempHome::new();
        let profile_dir = home.path().join("external-profile");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_provider_config_dir: Some(profile_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set provider profile override");

        let path = prompt_file_path(&AppType::Claude).expect("prompt path");

        assert_eq!(path, home.path().join(".claude").join("CLAUDE.md"));
    }

    #[test]
    #[serial]
    fn claude_prompt_path_for_db_uses_profile_and_config_profile() {
        let home = TempHome::new();
        let configured_claude_dir = home.path().join("configured-claude");
        let profile_dir = home.path().join("profile-and-config");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_config_dir: Some(configured_claude_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set configured claude dir");

        let db = Arc::new(Database::memory().expect("memory db"));
        let provider = claude_provider_with_profile(
            "profile-and-config",
            &profile_dir,
            ClaudeActivationMode::ProfileAndConfig,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        db.set_current_provider(AppType::Claude.as_str(), "profile-and-config")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("profile-and-config"))
            .expect("set local current provider");

        let path = prompt_file_path_for_db(&db, &AppType::Claude).expect("prompt path");

        assert_eq!(path, profile_dir.join("CLAUDE.md"));
    }
}
