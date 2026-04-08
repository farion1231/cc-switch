use serde_json::Value;

use crate::error::AppError;
use crate::settings::WebDavSyncSettings;

use super::support::{convert, fresh_core_state, map_core_err};

pub fn legacy_get_settings_as_core() -> Result<Option<cc_switch_core::WebDavSyncSettings>, AppError>
{
    crate::settings::get_webdav_sync_settings()
        .map(convert)
        .transpose()
}

pub fn get_settings_as_core() -> Result<Option<cc_switch_core::WebDavSyncSettings>, AppError> {
    cc_switch_core::settings::get_webdav_sync_settings()
        .map(convert)
        .transpose()
}

pub fn legacy_save_settings(settings: WebDavSyncSettings) -> Result<(), AppError> {
    crate::settings::set_webdav_sync_settings(Some(settings))
}

pub fn legacy_save_settings_from_core(
    settings: cc_switch_core::WebDavSyncSettings,
) -> Result<(), AppError> {
    legacy_save_settings(convert(settings)?)
}

pub async fn test_connection(settings: WebDavSyncSettings) -> Result<(), AppError> {
    let settings = convert(settings)?;
    cc_switch_core::webdav_check_connection(&settings)
        .await
        .map_err(map_core_err)
}

pub async fn upload() -> Result<Value, AppError> {
    let state = fresh_core_state()?;
    let mut settings = cc_switch_core::settings::get_webdav_sync_settings()
        .ok_or_else(|| AppError::Config("未配置 WebDAV 同步".to_string()))?;
    cc_switch_core::services::webdav_sync::upload(&state.db, &mut settings)
        .await
        .map_err(map_core_err)
}

pub async fn download() -> Result<Value, AppError> {
    let state = fresh_core_state()?;
    let mut settings = cc_switch_core::settings::get_webdav_sync_settings()
        .ok_or_else(|| AppError::Config("未配置 WebDAV 同步".to_string()))?;
    cc_switch_core::services::webdav_sync::download(&state.db, &mut settings)
        .await
        .map_err(map_core_err)
}

pub fn save_settings(settings: WebDavSyncSettings) -> Result<(), AppError> {
    let settings = convert(settings)?;
    cc_switch_core::settings::set_webdav_sync_settings(Some(settings)).map_err(map_core_err)
}

pub fn save_settings_from_core(
    settings: cc_switch_core::WebDavSyncSettings,
) -> Result<(), AppError> {
    save_settings(convert(settings)?)
}

pub async fn fetch_remote_info() -> Result<Option<Value>, AppError> {
    let settings = cc_switch_core::settings::get_webdav_sync_settings()
        .ok_or_else(|| AppError::Config("未配置 WebDAV 同步".to_string()))?;
    cc_switch_core::fetch_remote_info(&settings)
        .await
        .map_err(map_core_err)
}
