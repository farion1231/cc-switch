//! Per-model provider routing commands
//!
//! Lets the UI map a model class (opus/sonnet/haiku) to a specific provider —
//! and optionally to a specific model on that provider — so the local proxy can
//! send different Claude models to different providers/models.

use crate::store::AppState;
use serde::Serialize;
use std::collections::HashMap;

/// Model classes that can be routed to a dedicated provider.
const VALID_MODEL_CLASSES: [&str; 3] = ["opus", "sonnet", "haiku"];

/// A configured route, as exposed to the frontend.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRouteDto {
    /// Provider the model class is routed to.
    pub provider_id: String,
    /// Exact model to request on that provider (`null` = provider default).
    pub model: Option<String>,
}

/// Get all configured model-class → route entries for an app.
#[tauri::command]
pub async fn get_model_routes(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<HashMap<String, ModelRouteDto>, String> {
    let routes = state
        .db
        .get_model_routes(&app_type)
        .map_err(|e| e.to_string())?;

    Ok(routes
        .into_iter()
        .map(|(class, route)| {
            (
                class,
                ModelRouteDto {
                    provider_id: route.provider_id,
                    model: route.target_model,
                },
            )
        })
        .collect())
}

/// Set (or clear) the route for a single model class.
///
/// - Pass `provider_id = None` or an empty string to clear the route and fall
///   back to the app's normal current/failover provider selection.
/// - `target_model` is the exact model to use on that provider; pass `None` or
///   an empty string to use the provider's own model mapping / default model.
#[tauri::command]
pub async fn set_model_route(
    state: tauri::State<'_, AppState>,
    app_type: String,
    model_class: String,
    provider_id: Option<String>,
    target_model: Option<String>,
) -> Result<(), String> {
    if !VALID_MODEL_CLASSES.contains(&model_class.as_str()) {
        return Err(format!("Unsupported model class: {model_class}"));
    }

    state
        .db
        .set_model_route(
            &app_type,
            &model_class,
            provider_id.as_deref(),
            target_model.as_deref(),
        )
        .map_err(|e| e.to_string())
}
