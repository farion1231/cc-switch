use crate::bridges::support::{convert, fresh_core_state, map_core_err};
use crate::error::AppError;
use crate::proxy::types::{LogConfig, RectifierConfig};
use crate::settings::AppSettings;

pub fn legacy_get_settings() -> Result<AppSettings, AppError> {
    Ok(crate::settings::get_settings_for_frontend())
}

pub fn get_settings() -> Result<AppSettings, AppError> {
    let settings = cc_switch_core::SettingsService::get_settings().map_err(map_core_err)?;
    convert(settings)
}

pub fn legacy_save_settings(settings: AppSettings) -> Result<bool, AppError> {
    let existing = crate::settings::get_settings();
    let merged = if settings.webdav_sync.is_none() {
        crate::settings::AppSettings {
            webdav_sync: existing.webdav_sync,
            ..settings
        }
    } else {
        settings
    };
    crate::settings::update_settings(merged)?;
    Ok(true)
}

pub fn save_settings(
    settings: AppSettings,
) -> Result<cc_switch_core::SettingsSaveResult, AppError> {
    let settings = convert(settings)?;
    let state = fresh_core_state()?;
    cc_switch_core::SettingsService::save_settings(&state, settings).map_err(map_core_err)
}

pub fn set_auto_launch(enabled: bool) -> Result<bool, AppError> {
    if enabled {
        cc_switch_core::AutoLaunchService::enable().map_err(map_core_err)?;
    } else {
        cc_switch_core::AutoLaunchService::disable().map_err(map_core_err)?;
    }
    cc_switch_core::HostService::set_launch_on_startup(enabled).map_err(map_core_err)?;
    Ok(true)
}

pub fn get_auto_launch_status() -> Result<bool, AppError> {
    cc_switch_core::AutoLaunchService::is_enabled().map_err(map_core_err)
}

pub fn legacy_get_rectifier_config(
    state: &crate::store::AppState,
) -> Result<RectifierConfig, AppError> {
    state.db.get_rectifier_config()
}

pub fn get_rectifier_config() -> Result<RectifierConfig, AppError> {
    let state = fresh_core_state()?;
    let config =
        cc_switch_core::SettingsService::get_rectifier_config(&state).map_err(map_core_err)?;
    convert(config)
}

pub fn legacy_set_rectifier_config(
    state: &crate::store::AppState,
    config: RectifierConfig,
) -> Result<(), AppError> {
    state.db.set_rectifier_config(&config)
}

pub fn set_rectifier_config(config: RectifierConfig) -> Result<(), AppError> {
    let state = fresh_core_state()?;
    let config = convert(config)?;
    cc_switch_core::SettingsService::set_rectifier_config(&state, config).map_err(map_core_err)
}

pub fn legacy_get_log_config(state: &crate::store::AppState) -> Result<LogConfig, AppError> {
    state.db.get_log_config()
}

pub fn get_log_config() -> Result<LogConfig, AppError> {
    let state = fresh_core_state()?;
    let config = cc_switch_core::SettingsService::get_log_config(&state).map_err(map_core_err)?;
    convert(config)
}

pub fn legacy_set_log_config(
    state: &crate::store::AppState,
    config: LogConfig,
) -> Result<(), AppError> {
    state.db.set_log_config(&config)?;
    log::set_max_level(config.to_level_filter());
    Ok(())
}

pub fn set_log_config(config: LogConfig) -> Result<(), AppError> {
    let state = fresh_core_state()?;
    let config = convert(config)?;
    cc_switch_core::SettingsService::set_log_config(&state, config).map_err(map_core_err)
}
