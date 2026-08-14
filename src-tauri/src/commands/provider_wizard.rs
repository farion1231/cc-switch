use crate::services::provider_wizard::{
    apply_provider_install, preview_provider_install, probe_provider_capabilities,
    ApplyProviderInstallResult, ProviderInstallPreview, ProviderInstallSelection,
    ProviderProbeInput, ProviderProbeResult,
};
use tauri::Manager;

/// Probe a provider without persisting a provider or changing any IDE config.
#[tauri::command(rename_all = "camelCase")]
pub async fn probe_provider_capabilities_command(
    input: ProviderProbeInput,
) -> Result<ProviderProbeResult, String> {
    probe_provider_capabilities(input).await
}

/// Build a redacted, no-write installation preview.
#[tauri::command(rename_all = "camelCase")]
pub fn preview_provider_install_command(
    selection: ProviderInstallSelection,
) -> Result<ProviderInstallPreview, String> {
    preview_provider_install(selection)
}

/// Apply the selected Claude/Codex setup and compensate on failure.
#[tauri::command(rename_all = "camelCase")]
pub async fn apply_provider_install_command(
    app_handle: tauri::AppHandle,
    selection: ProviderInstallSelection,
) -> Result<ApplyProviderInstallResult, String> {
    let state = app_handle
        .try_state::<crate::store::AppState>()
        .ok_or_else(|| "Application state is unavailable".to_string())?;
    apply_provider_install(state.inner(), selection).await
}
