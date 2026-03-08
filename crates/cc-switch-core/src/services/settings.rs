use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::proxy::{LogConfig, RectifierConfig};
use crate::services::{ClaudePluginService, ProviderService};
use crate::settings::{self, AppSettings};
use crate::store::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSaveResult {
    pub settings: AppSettings,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub claude_plugin_synced: bool,
    #[serde(default)]
    pub claude_onboarding_synced: bool,
    #[serde(default)]
    pub current_providers_synced: bool,
}

pub struct SettingsService;

impl SettingsService {
    pub fn get_settings() -> Result<AppSettings, AppError> {
        Ok(settings::get_settings_for_frontend())
    }

    pub fn get_raw_settings() -> Result<AppSettings, AppError> {
        Ok(settings::get_settings())
    }

    pub fn save_settings(
        state: &AppState,
        incoming: AppSettings,
    ) -> Result<SettingsSaveResult, AppError> {
        let previous = settings::get_settings();
        let merged = Self::merge_settings_for_save(incoming, &previous);
        settings::update_settings(merged.clone())?;

        let mut result = SettingsSaveResult {
            settings: settings::get_settings_for_frontend(),
            warnings: Vec::new(),
            claude_plugin_synced: false,
            claude_onboarding_synced: false,
            current_providers_synced: false,
        };

        if merged.enable_claude_plugin_integration != previous.enable_claude_plugin_integration {
            match ClaudePluginService::apply_config(!merged.enable_claude_plugin_integration) {
                Ok(_) => result.claude_plugin_synced = true,
                Err(err) => result
                    .warnings
                    .push(format!("同步 Claude plugin 配置失败: {err}")),
            }
        }

        if merged.skip_claude_onboarding != previous.skip_claude_onboarding {
            let sync_result = if merged.skip_claude_onboarding {
                ClaudePluginService::apply_onboarding_skip()
            } else {
                ClaudePluginService::clear_onboarding_skip()
            };

            match sync_result {
                Ok(_) => result.claude_onboarding_synced = true,
                Err(err) => result
                    .warnings
                    .push(format!("同步 Claude onboarding 设置失败: {err}")),
            }
        }

        if Self::provider_dir_changed(&previous, &merged) {
            match ProviderService::sync_current_to_live(state) {
                Ok(_) => result.current_providers_synced = true,
                Err(err) => result
                    .warnings
                    .push(format!("同步当前 provider 到 live 配置失败: {err}")),
            }
        }

        Ok(result)
    }

    pub fn get_rectifier_config(state: &AppState) -> Result<RectifierConfig, AppError> {
        state.db.get_rectifier_config()
    }

    pub fn set_rectifier_config(state: &AppState, config: RectifierConfig) -> Result<(), AppError> {
        state.db.set_rectifier_config(&config)
    }

    pub fn get_log_config(state: &AppState) -> Result<LogConfig, AppError> {
        state.db.get_log_config()
    }

    pub fn set_log_config(state: &AppState, config: LogConfig) -> Result<(), AppError> {
        state.db.set_log_config(&config)?;
        log::set_max_level(config.to_level_filter());
        Ok(())
    }

    fn merge_settings_for_save(mut incoming: AppSettings, existing: &AppSettings) -> AppSettings {
        if incoming.webdav_sync.is_none() {
            incoming.webdav_sync = existing.webdav_sync.clone();
        }
        incoming
    }

    fn provider_dir_changed(previous: &AppSettings, next: &AppSettings) -> bool {
        previous.claude_config_dir != next.claude_config_dir
            || previous.codex_config_dir != next.codex_config_dir
            || previous.gemini_config_dir != next.gemini_config_dir
            || previous.opencode_config_dir != next.opencode_config_dir
            || previous.openclaw_config_dir != next.openclaw_config_dir
    }
}

#[cfg(test)]
mod tests {
    use super::SettingsService;
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::proxy::{LogConfig, RectifierConfig};
    use crate::settings::{update_settings, AppSettings, WebDavSyncSettings};
    use crate::store::AppState;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn save_settings_preserves_existing_webdav_when_omitted() -> Result<(), crate::error::AppError>
    {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        let state = AppState::new(Database::memory()?);

        update_settings(AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.example.com".to_string(),
                username: "alice".to_string(),
                password: "secret".to_string(),
                ..WebDavSyncSettings::default()
            }),
            ..AppSettings::default()
        })?;

        let result = SettingsService::save_settings(
            &state,
            AppSettings {
                language: Some("zh".to_string()),
                ..AppSettings::default()
            },
        )?;

        assert!(result.warnings.is_empty());
        assert_eq!(result.settings.language.as_deref(), Some("zh"));
        assert_eq!(
            crate::settings::get_settings()
                .webdav_sync
                .as_ref()
                .map(|item| item.base_url.as_str()),
            Some("https://dav.example.com")
        );

        Ok(())
    }

    #[test]
    #[serial]
    fn save_settings_syncs_claude_plugin_and_onboarding() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        update_settings(AppSettings::default())?;
        let state = AppState::new(Database::memory()?);

        let result = SettingsService::save_settings(
            &state,
            AppSettings {
                enable_claude_plugin_integration: true,
                skip_claude_onboarding: true,
                ..AppSettings::default()
            },
        )?;

        assert!(result.warnings.is_empty());
        assert!(result.claude_plugin_synced);
        assert!(result.claude_onboarding_synced);
        assert!(crate::services::plugin::ClaudePluginService::is_applied()?);
        assert!(crate::services::plugin::ClaudePluginService::is_onboarding_skip_applied()?);

        Ok(())
    }

    #[test]
    #[serial]
    fn save_settings_syncs_current_provider_when_dir_changes() -> Result<(), crate::error::AppError>
    {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        update_settings(AppSettings::default())?;

        let db = Database::memory()?;
        let state = AppState::new(db);
        let provider = Provider::with_id(
            "provider-a".to_string(),
            "Provider A".to_string(),
            serde_json::json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token-a",
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            None,
        );
        state.db.save_provider("claude", &provider)?;
        state.db.set_current_provider("claude", "provider-a")?;

        let custom_dir = temp.path().join("custom-claude");
        let result = SettingsService::save_settings(
            &state,
            AppSettings {
                claude_config_dir: Some(custom_dir.to_string_lossy().to_string()),
                ..AppSettings::default()
            },
        )?;

        assert!(result.warnings.is_empty());
        assert!(result.current_providers_synced);

        let live_path = custom_dir.join("settings.json");
        let content = std::fs::read_to_string(&live_path).expect("claude live settings");
        assert!(content.contains("ANTHROPIC_AUTH_TOKEN"));
        assert!(content.contains("token-a"));

        Ok(())
    }

    #[test]
    fn rectifier_and_log_config_round_trip() -> Result<(), crate::error::AppError> {
        let state = AppState::new(Database::memory()?);

        SettingsService::set_rectifier_config(
            &state,
            RectifierConfig {
                enabled: false,
                request_thinking_signature: false,
                request_thinking_budget: true,
            },
        )?;
        SettingsService::set_log_config(
            &state,
            LogConfig {
                enabled: true,
                level: "debug".to_string(),
            },
        )?;

        let rectifier = SettingsService::get_rectifier_config(&state)?;
        let log_config = SettingsService::get_log_config(&state)?;

        assert!(!rectifier.enabled);
        assert!(!rectifier.request_thinking_signature);
        assert_eq!(log_config.level, "debug");

        Ok(())
    }
}
