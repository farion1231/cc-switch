use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::settings::{self, AppSettings, VisibleApps};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostPreferences {
    pub show_in_tray: bool,
    pub minimize_to_tray_on_close: bool,
    pub launch_on_startup: bool,
    pub silent_startup: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_terminal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_apps: Option<VisibleApps>,
}

impl Default for HostPreferences {
    fn default() -> Self {
        let settings = AppSettings::default();
        Self {
            show_in_tray: settings.show_in_tray,
            minimize_to_tray_on_close: settings.minimize_to_tray_on_close,
            launch_on_startup: settings.launch_on_startup,
            silent_startup: settings.silent_startup,
            preferred_terminal: settings.preferred_terminal,
            visible_apps: settings.visible_apps,
        }
    }
}

pub struct HostService;

impl HostService {
    pub fn get_preferences() -> Result<HostPreferences, AppError> {
        let settings = settings::get_settings();
        Ok(HostPreferences {
            show_in_tray: settings.show_in_tray,
            minimize_to_tray_on_close: settings.minimize_to_tray_on_close,
            launch_on_startup: settings.launch_on_startup,
            silent_startup: settings.silent_startup,
            preferred_terminal: settings.preferred_terminal,
            visible_apps: settings.visible_apps,
        })
    }

    pub fn save_preferences(preferences: HostPreferences) -> Result<(), AppError> {
        let mut settings = settings::get_settings();
        settings.show_in_tray = preferences.show_in_tray;
        settings.minimize_to_tray_on_close = preferences.minimize_to_tray_on_close;
        settings.launch_on_startup = preferences.launch_on_startup;
        settings.silent_startup = preferences.silent_startup;
        settings.preferred_terminal = preferences.preferred_terminal;
        settings.visible_apps = preferences.visible_apps;
        settings::update_settings(settings)
    }

    pub fn set_preferred_terminal(value: Option<&str>) -> Result<(), AppError> {
        let mut settings = settings::get_settings();
        settings.preferred_terminal = value.map(|item| item.to_string());
        settings::update_settings(settings)
    }

    pub fn set_visible_apps(value: Option<VisibleApps>) -> Result<(), AppError> {
        let mut settings = settings::get_settings();
        settings.visible_apps = value;
        settings::update_settings(settings)
    }

    pub fn set_launch_on_startup(value: bool) -> Result<(), AppError> {
        let mut settings = settings::get_settings();
        settings.launch_on_startup = value;
        settings::update_settings(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::{HostPreferences, HostService};
    use crate::settings::{update_settings, AppSettings, VisibleApps};
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn save_preferences_updates_structured_host_fields() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        update_settings(AppSettings::default())?;

        HostService::save_preferences(HostPreferences {
            show_in_tray: false,
            minimize_to_tray_on_close: false,
            launch_on_startup: true,
            silent_startup: true,
            preferred_terminal: Some("wezterm".to_string()),
            visible_apps: Some(VisibleApps {
                claude: true,
                codex: false,
                gemini: true,
                opencode: false,
                openclaw: true,
            }),
        })?;

        let preferences = HostService::get_preferences()?;
        assert!(!preferences.show_in_tray);
        assert!(!preferences.minimize_to_tray_on_close);
        assert!(preferences.launch_on_startup);
        assert!(preferences.silent_startup);
        assert_eq!(preferences.preferred_terminal.as_deref(), Some("wezterm"));
        assert_eq!(
            preferences.visible_apps.as_ref().map(|item| item.codex),
            Some(false)
        );

        Ok(())
    }

    #[test]
    #[serial]
    fn set_preferred_terminal_trims_blank_to_none() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        update_settings(AppSettings::default())?;

        HostService::set_preferred_terminal(Some("  "))?;
        assert_eq!(HostService::get_preferences()?.preferred_terminal, None);

        HostService::set_preferred_terminal(Some("iterm2"))?;
        assert_eq!(
            HostService::get_preferences()?
                .preferred_terminal
                .as_deref(),
            Some("iterm2")
        );

        HostService::set_visible_apps(None)?;
        assert_eq!(HostService::get_preferences()?.visible_apps, None);

        HostService::set_launch_on_startup(true)?;
        assert!(HostService::get_preferences()?.launch_on_startup);

        Ok(())
    }
}
