use std::time::Duration;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::hermes_config;
use crate::store::AppState;

/// Error string returned when `open_hermes_web_ui` cannot reach the Hermes
/// FastAPI server. Kept in sync with the `HERMES_WEB_OFFLINE_ERROR` constant
/// in `src/hooks/useHermes.ts` so the frontend can branch on it.
const HERMES_WEB_OFFLINE_ERROR: &str = "hermes_web_offline";

// ============================================================================
// Hermes Provider Commands
// ============================================================================

/// Import providers from Hermes live config to database.
///
/// Hermes uses additive mode — users may already have providers
/// configured in config.yaml.
#[tauri::command]
pub fn import_hermes_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_hermes_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

/// Get provider names in the Hermes live config.
#[tauri::command]
pub fn get_hermes_live_provider_ids() -> Result<Vec<String>, String> {
    hermes_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}

/// Get a single Hermes provider fragment from live config.
#[tauri::command]
pub fn get_hermes_live_provider(
    #[allow(non_snake_case)] providerId: String,
) -> Result<Option<serde_json::Value>, String> {
    hermes_config::get_provider(&providerId).map_err(|e| e.to_string())
}

/// Scan config.yaml for known configuration hazards.
#[tauri::command]
pub fn scan_hermes_config_health() -> Result<Vec<hermes_config::HermesHealthWarning>, String> {
    hermes_config::scan_hermes_config_health().map_err(|e| e.to_string())
}

// ============================================================================
// Model Configuration Commands
// ============================================================================

/// Get Hermes model config (model section of config.yaml). Read-only — writes
/// happen implicitly through `apply_switch_defaults` when switching providers.
#[tauri::command]
pub fn get_hermes_model_config() -> Result<Option<hermes_config::HermesModelConfig>, String> {
    hermes_config::get_model_config().map_err(|e| e.to_string())
}

// ============================================================================
// Memory Files Commands
// ============================================================================

#[tauri::command]
pub fn get_hermes_memory(kind: hermes_config::MemoryKind) -> Result<String, String> {
    hermes_config::read_memory(kind).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_hermes_memory(kind: hermes_config::MemoryKind, content: String) -> Result<(), String> {
    hermes_config::write_memory(kind, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_hermes_memory_limits() -> Result<hermes_config::HermesMemoryLimits, String> {
    hermes_config::read_memory_limits().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_hermes_memory_enabled(
    kind: hermes_config::MemoryKind,
    enabled: bool,
) -> Result<hermes_config::HermesWriteOutcome, String> {
    hermes_config::set_memory_enabled(kind, enabled).map_err(|e| e.to_string())
}

// ============================================================================
// Hermes Web UI launcher
// ============================================================================

/// Probe the local Hermes Web UI (FastAPI) and open it in the system browser.
///
/// Port discovery priority:
///   1. `HERMES_WEB_PORT` environment variable
///   2. Default 9119
///
/// Hermes wraps all `/api/*` routes in a Bearer-token middleware, so a GET
/// against `/api/status` returning **either 200 or 401** confirms the server
/// is live. The session token lives only in the Hermes process memory and is
/// injected into the returned HTML via `window.__HERMES_SESSION_TOKEN__`, so
/// there is no need (and no way) for CC Switch to inject it — we just open
/// the URL and let Hermes handle auth.
#[tauri::command]
pub async fn open_hermes_web_ui(app: AppHandle, path: Option<String>) -> Result<(), String> {
    let port = std::env::var("HERMES_WEB_PORT")
        .ok()
        .and_then(|raw| raw.trim().parse::<u16>().ok())
        .unwrap_or(9119);

    let base = format!("http://127.0.0.1:{port}");

    // Probe /api/status with a short timeout. Hermes returns 200 when open or
    // 401 when the session token is required — either way the server is live.
    // Only a connection error / timeout means the server isn't running.
    let probe_url = format!("{base}/api/status");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(1200))
        .no_proxy()
        .build()
        .map_err(|e| format!("failed to build probe client: {e}"))?;

    match client.get(&probe_url).send().await {
        Ok(_) => {}
        Err(_) => return Err(HERMES_WEB_OFFLINE_ERROR.to_string()),
    }

    let target = match path.as_deref() {
        Some(p) if p.starts_with('/') => format!("{base}{p}"),
        Some(p) if !p.is_empty() => format!("{base}/{p}"),
        _ => format!("{base}/"),
    };

    app.opener()
        .open_url(&target, None::<String>)
        .map_err(|e| format!("failed to open Hermes Web UI: {e}"))
}

/// Open the preferred terminal and run `hermes dashboard`. Non-blocking —
/// callers should reinvoke `open_hermes_web_ui` once the server is ready,
/// since Hermes startup can take several seconds and may fail outright if
/// the `hermes-agent[web]` extras are missing.
#[tauri::command]
pub async fn launch_hermes_dashboard() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        crate::commands::misc::launch_terminal_running("hermes dashboard", "hermes_dashboard")
    })
    .await
    .map_err(|e| format!("launch task join error: {e}"))?
}
