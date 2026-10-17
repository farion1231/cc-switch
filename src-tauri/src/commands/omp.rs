#[tauri::command]
pub(crate) fn get_omp_current_state() -> Result<serde_json::Value, String> {
    crate::omp_config::get_providers()
        .map(|providers| {
            serde_json::json!({
                "enabledProviderIds": providers.keys().collect::<Vec<_>>(),
                "defaultProviderId": null
            })
        })
        .map_err(|error| error.to_string())
}
