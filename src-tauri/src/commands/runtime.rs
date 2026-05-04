#[tauri::command]
pub async fn get_runtime_info() -> Result<crate::runtime::RuntimeInfo, String> {
    Ok(crate::runtime::backend_runtime_info(
        crate::runtime::BackendMode::Desktop,
    ))
}

#[tauri::command]
pub async fn get_client_backend_connection(
) -> Result<crate::settings::ClientBackendConnectionSettings, String> {
    Ok(crate::settings::get_settings().client_backend)
}

#[tauri::command]
pub async fn save_client_backend_connection(
    connection: crate::settings::ClientBackendConnectionSettings,
) -> Result<bool, String> {
    let mut settings = crate::settings::get_settings();
    settings.client_backend = connection;
    crate::settings::update_settings(settings).map_err(|err| err.to_string())?;
    Ok(true)
}
