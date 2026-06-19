//! Per-model provider routing commands
//!
//! Lets the UI map a model class (opus/sonnet/haiku) to a specific provider so
//! the local proxy can send different models to different providers.

use crate::store::AppState;
use std::collections::HashMap;

/// Model classes that can be routed to a dedicated provider.
const VALID_MODEL_CLASSES: [&str; 3] = ["opus", "sonnet", "haiku"];

/// Get all configured model-class → provider routes for an app.
#[tauri::command]
pub async fn get_model_routes(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<HashMap<String, String>, String> {
    state
        .db
        .get_model_routes(&app_type)
        .map_err(|e| e.to_string())
}

/// Set (or clear) the provider route for a single model class.
///
/// Pass `provider_id = None` or an empty string to clear the route and fall
/// back to the app's normal current/failover provider selection.
#[tauri::command]
pub async fn set_model_route(
    state: tauri::State<'_, AppState>,
    app_type: String,
    model_class: String,
    provider_id: Option<String>,
) -> Result<(), String> {
    if !VALID_MODEL_CLASSES.contains(&model_class.as_str()) {
        return Err(format!("Unsupported model class: {model_class}"));
    }

    state
        .db
        .set_model_route(&app_type, &model_class, provider_id.as_deref())
        .map_err(|e| e.to_string())
}
