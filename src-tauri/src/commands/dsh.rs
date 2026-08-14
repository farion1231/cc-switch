//! Tauri commands for the dedicated DeepSeek Harness live configuration page.

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::dsh_config::{
    self, DshCustomInput, DshDefaultModel, DshModelDiscoveryResult, DshNativeInput, DshSnapshot,
};

#[tauri::command]
pub fn dsh_get_snapshot() -> Result<DshSnapshot, String> {
    dsh_config::snapshot()
}

#[tauri::command]
pub fn dsh_refresh() -> Result<DshSnapshot, String> {
    dsh_config::snapshot()
}

#[tauri::command]
pub fn dsh_get_home() -> Result<String, String> {
    Ok(dsh_config::get_home().to_string_lossy().to_string())
}

#[tauri::command]
pub fn dsh_get_default_home() -> Result<String, String> {
    Ok(dsh_config::get_default_home().to_string_lossy().to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn dsh_upsert_native(
    base_url: Option<String>,
    models: Option<Vec<dsh_config::DshModel>>,
    api_key_env: Option<String>,
    expected_revision: Option<String>,
) -> Result<DshSnapshot, String> {
    dsh_config::upsert_native(DshNativeInput {
        base_url,
        models,
        api_key_env,
        expected_revision,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn dsh_reset_native(expected_revision: Option<String>) -> Result<DshSnapshot, String> {
    dsh_config::reset_native(expected_revision)
}

#[tauri::command(rename_all = "camelCase")]
pub fn dsh_create_custom(
    route: String,
    display_name: Option<String>,
    api: String,
    base_url: String,
    models: Vec<dsh_config::DshModel>,
    api_key_env: Option<String>,
    expected_revision: Option<String>,
) -> Result<DshSnapshot, String> {
    dsh_config::create_custom(DshCustomInput {
        route,
        display_name,
        api,
        base_url,
        models,
        api_key_env,
        expected_revision,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn dsh_update_custom(
    route: String,
    display_name: Option<String>,
    api: String,
    base_url: String,
    models: Vec<dsh_config::DshModel>,
    api_key_env: Option<String>,
    expected_revision: Option<String>,
) -> Result<DshSnapshot, String> {
    dsh_config::update_custom(DshCustomInput {
        route,
        display_name,
        api,
        base_url,
        models,
        api_key_env,
        expected_revision,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn dsh_remove_custom(
    route: String,
    expected_revision: Option<String>,
) -> Result<DshSnapshot, String> {
    dsh_config::remove_custom(route, expected_revision)
}

#[tauri::command(rename_all = "camelCase")]
pub fn dsh_set_default_model(
    selection: DshDefaultModel,
    expected_revision: Option<String>,
) -> Result<DshSnapshot, String> {
    dsh_config::set_default_model(selection, expected_revision)
}

#[tauri::command(rename_all = "camelCase")]
pub fn dsh_set_credential(
    reference: String,
    value: String,
    expected_revision: Option<String>,
) -> Result<(), String> {
    dsh_config::set_credential(reference, value, expected_revision)
}

#[tauri::command(rename_all = "camelCase")]
pub fn dsh_unset_credential(
    reference: String,
    expected_revision: Option<String>,
) -> Result<(), String> {
    dsh_config::unset_credential(reference, expected_revision)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn dsh_discover_models(
    base_url: String,
    api: String,
    api_key: Option<String>,
    credential_ref: Option<String>,
) -> Result<DshModelDiscoveryResult, String> {
    dsh_config::discover_models(base_url, api, api_key, credential_ref).await
}

#[tauri::command]
pub fn dsh_open_home(handle: AppHandle) -> Result<(), String> {
    let home = dsh_config::get_home();
    dsh_config::ensure_secure_home(&home)?;
    handle
        .opener()
        .open_path(home.to_string_lossy().to_string(), None::<String>)
        .map_err(|source| source.to_string())
}
