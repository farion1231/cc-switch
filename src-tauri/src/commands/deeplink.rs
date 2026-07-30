use crate::deeplink::{
    import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
    import_skill_from_deeplink, parse_deeplink_request_url, DeepLinkImportRequest, DeepLinkRequest,
    ProviderSwitchRequest,
};
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::ProviderService;
use crate::store::{AppState, ProviderSwitchStateSnapshot};
use crate::AppType;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use url::Url;

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSwitchPreview {
    pub name: String,
    pub hostname: String,
    pub is_current: bool,
    pub review_token: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedProviderSwitch {
    pub name: String,
    pub hostname: String,
    pub is_current: bool,
    pub has_warnings: bool,
}

fn validate_provider_switch_request(request: &ProviderSwitchRequest) -> Result<(), AppError> {
    if request.version != "v1"
        || request.resource != "provider-switch"
        || request.app != "codex"
        || request.id.trim().is_empty()
    {
        return Err(AppError::InvalidInput(
            "Invalid provider-switch request".to_string(),
        ));
    }
    Ok(())
}

fn is_chatgpt_oauth_without_api_key(auth: &serde_json::Map<String, serde_json::Value>) -> bool {
    let has_api_key = match auth.get("OPENAI_API_KEY") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::String(value)) => !value.trim().is_empty(),
        Some(_) => true,
    };
    let has_oauth_token = auth
        .get("tokens")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|tokens| {
            ["id_token", "access_token", "refresh_token"]
                .iter()
                .any(|key| {
                    tokens
                        .get(*key)
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty())
                })
        });

    auth.get("auth_mode").and_then(serde_json::Value::as_str) == Some("chatgpt")
        && !has_api_key
        && has_oauth_token
}

fn read_validated_live_chatgpt_oauth() -> Result<(serde_json::Value, Vec<u8>), AppError> {
    let live = crate::codex_config::read_codex_live_settings()
        .map_err(|_| AppError::InvalidInput("Live ChatGPT OAuth is unavailable".to_string()))?;
    let auth = live
        .get("auth")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| AppError::InvalidInput("Live ChatGPT OAuth is unavailable".to_string()))?;
    if !is_chatgpt_oauth_without_api_key(auth) {
        return Err(AppError::InvalidInput(
            "Live ChatGPT OAuth is unavailable".to_string(),
        ));
    }
    let auth_bytes = std::fs::read(crate::get_codex_auth_path())
        .map_err(|_| AppError::InvalidInput("Live ChatGPT OAuth is unavailable".to_string()))?;
    Ok((live, auth_bytes))
}

fn reject_proxy_takeover(state: &AppState) -> Result<(), AppError> {
    let proxy_enabled = state
        .db
        .get_proxy_enabled_read_only(AppType::Codex.as_str())?;
    let has_live_backup =
        futures::executor::block_on(state.db.get_live_backup(AppType::Codex.as_str()))?.is_some();
    let live_taken_over = state
        .proxy_service
        .detect_takeover_in_live_config_for_app(&AppType::Codex);
    if proxy_enabled || has_live_backup || live_taken_over {
        return Err(AppError::InvalidInput(
            "Codex proxy takeover is active".to_string(),
        ));
    }
    Ok(())
}

fn toml_value_is_configured(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(value) => !value.trim().is_empty(),
        _ => true,
    }
}

#[derive(Clone, Eq, PartialEq)]
struct MixedProviderIdentity {
    model_provider: String,
    base_url: String,
    bearer: String,
}

fn mixed_provider_identity_from_config(config: &str) -> Result<MixedProviderIdentity, AppError> {
    let invalid = || {
        AppError::InvalidInput(
            "Codex provider does not satisfy mixed-auth requirements".to_string(),
        )
    };
    let document = config.parse::<toml::Value>().map_err(|_| invalid())?;
    if document
        .get("experimental_bearer_token")
        .is_some_and(toml_value_is_configured)
    {
        return Err(invalid());
    }
    let model_provider = document
        .get("model_provider")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(&invalid)?;
    let selected = document
        .get("model_providers")
        .and_then(|providers| providers.get(&model_provider))
        .and_then(toml::Value::as_table)
        .ok_or_else(&invalid)?;
    let has_unsupported_auth = selected.iter().any(|(key, value)| {
        (matches!(
            key.as_str(),
            "env_key"
                | "bearer_token_command"
                | "experimental_bearer_token_command"
                | "auth_command"
        ) || key.starts_with("aws_"))
            && toml_value_is_configured(value)
    });
    let bearer = selected
        .get("experimental_bearer_token")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(&invalid)?;
    if has_unsupported_auth
        || selected
            .get("requires_openai_auth")
            .and_then(toml::Value::as_bool)
            != Some(true)
    {
        return Err(invalid());
    }
    let base_url = selected
        .get("base_url")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(invalid)?;
    Ok(MixedProviderIdentity {
        model_provider,
        base_url,
        bearer,
    })
}

fn validate_mixed_provider_target(provider: &Provider) -> Result<MixedProviderIdentity, AppError> {
    let invalid = || {
        AppError::InvalidInput(
            "Codex provider does not satisfy mixed-auth requirements".to_string(),
        )
    };
    if provider.category.as_deref() == Some("official") {
        return Err(invalid());
    }
    let auth = provider
        .settings_config
        .get("auth")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(&invalid)?;
    if !is_chatgpt_oauth_without_api_key(auth) {
        return Err(invalid());
    }
    let config = provider
        .settings_config
        .get("config")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(&invalid)?;
    mixed_provider_identity_from_config(config)
}

struct ProviderSwitchInspection {
    name: String,
    hostname: String,
    is_current: bool,
    snapshot: ProviderSwitchStateSnapshot,
}

fn provider_value(
    provider: Option<crate::provider::Provider>,
) -> Result<Option<serde_json::Value>, AppError> {
    provider
        .map(|provider| {
            serde_json::to_value(provider).map_err(|source| AppError::JsonSerialize { source })
        })
        .transpose()
}

fn inspect_provider_switch_internal(
    state: &AppState,
    request: &ProviderSwitchRequest,
) -> Result<ProviderSwitchInspection, AppError> {
    validate_provider_switch_request(request)?;
    if !crate::settings::preserve_codex_official_auth_on_switch() {
        return Err(AppError::InvalidInput(
            "Codex official-auth preservation is disabled".to_string(),
        ));
    }
    reject_proxy_takeover(state)?;
    let (live_settings, live_auth_bytes) = read_validated_live_chatgpt_oauth()?;
    let provider = state
        .db
        .get_provider_by_id(&request.id, AppType::Codex.as_str())?
        .ok_or_else(|| AppError::InvalidInput("Codex provider not found".to_string()))?;
    let target_identity = validate_mixed_provider_target(&provider)?;
    let endpoint = Url::parse(&target_identity.base_url)
        .map_err(|_| AppError::InvalidInput("Codex provider endpoint is invalid".to_string()))?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(AppError::InvalidInput(
            "Codex provider endpoint is invalid".to_string(),
        ));
    }
    let hostname = endpoint
        .host_str()
        .filter(|hostname| !hostname.is_empty())
        .ok_or_else(|| AppError::InvalidInput("Codex provider endpoint is invalid".to_string()))?;
    let device_current_provider_id = crate::settings::get_current_provider(&AppType::Codex);
    let database_current_provider_id = state.db.get_current_provider(AppType::Codex.as_str())?;
    let device_current_provider = match device_current_provider_id.as_deref() {
        Some(id) => state.db.get_provider_by_id(id, AppType::Codex.as_str())?,
        None => None,
    };
    let database_current_provider = match database_current_provider_id.as_deref() {
        Some(id) => state.db.get_provider_by_id(id, AppType::Codex.as_str())?,
        None => None,
    };
    let effective_current_provider_id = if device_current_provider.is_some() {
        device_current_provider_id.clone()
    } else if database_current_provider.is_some() {
        database_current_provider_id.clone()
    } else {
        None
    };
    let is_current = effective_current_provider_id.as_deref() == Some(request.id.as_str());
    let snapshot = ProviderSwitchStateSnapshot {
        target_provider: serde_json::to_value(&provider)
            .map_err(|source| AppError::JsonSerialize { source })?,
        device_current_provider_id,
        device_current_provider: provider_value(device_current_provider)?,
        database_current_provider_id,
        database_current_provider: provider_value(database_current_provider)?,
        effective_current_provider_id,
        live_settings,
        live_auth_bytes,
    };

    Ok(ProviderSwitchInspection {
        name: provider.name,
        hostname: hostname.to_string(),
        is_current,
        snapshot,
    })
}

fn reviewed_provider_from_snapshot(
    snapshot: &ProviderSwitchStateSnapshot,
) -> Result<Provider, AppError> {
    serde_json::from_value(snapshot.target_provider.clone())
        .map_err(|source| AppError::json("provider-switch-review", source))
}

fn verify_reviewed_provider_switch_postcondition(
    state: &AppState,
    request: &ProviderSwitchRequest,
    snapshot: &ProviderSwitchStateSnapshot,
) -> Result<(), AppError> {
    let failed = || {
        AppError::Message(
            "Provider switch could not prove the reviewed mixed-auth target".to_string(),
        )
    };
    if !crate::settings::preserve_codex_official_auth_on_switch() {
        return Err(failed());
    }
    reject_proxy_takeover(state).map_err(|_| failed())?;

    let reviewed_provider = reviewed_provider_from_snapshot(snapshot).map_err(|_| failed())?;
    let reviewed_identity =
        validate_mixed_provider_target(&reviewed_provider).map_err(|_| failed())?;
    let stored_provider = state
        .db
        .get_provider_by_id(&request.id, AppType::Codex.as_str())?
        .ok_or_else(&failed)?;
    let stored_value = serde_json::to_value(&stored_provider)
        .map_err(|source| AppError::JsonSerialize { source })?;
    if stored_value != snapshot.target_provider
        || crate::settings::get_current_provider(&AppType::Codex).as_deref()
            != Some(request.id.as_str())
        || state
            .db
            .get_current_provider(AppType::Codex.as_str())?
            .as_deref()
            != Some(request.id.as_str())
    {
        return Err(failed());
    }

    let (live, auth_bytes) = read_validated_live_chatgpt_oauth().map_err(|_| failed())?;
    if auth_bytes != snapshot.live_auth_bytes {
        return Err(failed());
    }
    let live_config = live
        .get("config")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(&failed)?;
    let live_identity = mixed_provider_identity_from_config(live_config).map_err(|_| failed())?;
    if live_identity != reviewed_identity {
        return Err(failed());
    }
    Ok(())
}

fn preview_provider_switch_internal(
    state: &AppState,
    request: &ProviderSwitchRequest,
) -> Result<ProviderSwitchPreview, AppError> {
    let inspected = inspect_provider_switch_internal(state, request)?;
    let review_token = state.issue_provider_switch_review(request.clone(), inspected.snapshot)?;

    Ok(ProviderSwitchPreview {
        name: inspected.name,
        hostname: inspected.hostname,
        is_current: inspected.is_current,
        review_token,
    })
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn preview_provider_switch_test_hook(
    state: &AppState,
    request: &ProviderSwitchRequest,
) -> Result<ProviderSwitchPreview, AppError> {
    preview_provider_switch_internal(state, request)
}

#[allow(non_snake_case)]
#[tauri::command(rename_all = "camelCase")]
pub async fn previewProviderSwitch(
    state: State<'_, AppState>,
    request: ProviderSwitchRequest,
) -> Result<ProviderSwitchPreview, String> {
    let _guard = crate::services::session_usage::session_sync_mutex()
        .lock()
        .await;
    preview_provider_switch_internal(state.inner(), &request).map_err(|error| error.to_string())
}

fn cancel_provider_switch_internal(state: &AppState, review_token: &str) -> Result<(), AppError> {
    state.cancel_provider_switch_review(review_token)
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn cancel_provider_switch_test_hook(
    state: &AppState,
    review_token: &str,
) -> Result<(), AppError> {
    cancel_provider_switch_internal(state, review_token)
}

#[allow(non_snake_case)]
#[tauri::command(rename_all = "camelCase")]
pub async fn cancelProviderSwitch(
    state: State<'_, AppState>,
    review_token: String,
) -> Result<(), String> {
    let _guard = crate::services::session_usage::session_sync_mutex()
        .lock()
        .await;
    cancel_provider_switch_internal(state.inner(), &review_token).map_err(|error| error.to_string())
}

fn confirm_provider_switch_internal(
    state: &AppState,
    review_token: &str,
) -> Result<ConfirmedProviderSwitch, AppError> {
    let review = state.take_provider_switch_review(review_token)?;
    let request = review.request.clone();
    let reviewed_provider = reviewed_provider_from_snapshot(&review.snapshot)?;
    let reviewed_identity = validate_mixed_provider_target(&reviewed_provider)?;
    let endpoint = Url::parse(&reviewed_identity.base_url)
        .map_err(|_| AppError::InvalidInput("Codex provider endpoint is invalid".to_string()))?;
    let hostname = endpoint
        .host_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::InvalidInput("Codex provider endpoint is invalid".to_string()))?
        .to_string();
    let precondition_snapshot = review.snapshot.clone();
    let postcondition_snapshot = review.snapshot;
    let result = ProviderService::switch_reviewed(
        state,
        AppType::Codex,
        &request.id,
        |locked_state| {
            let reviewed = inspect_provider_switch_internal(locked_state, &request)?;
            if reviewed.snapshot != precondition_snapshot {
                return Err(AppError::InvalidInput(
                    "Provider changed after preview; review the request again".to_string(),
                ));
            }
            Ok(!reviewed.is_current)
        },
        |locked_state| {
            verify_reviewed_provider_switch_postcondition(
                locked_state,
                &request,
                &postcondition_snapshot,
            )
        },
    )?;

    Ok(ConfirmedProviderSwitch {
        name: reviewed_provider.name,
        hostname,
        is_current: true,
        has_warnings: !result.warnings.is_empty(),
    })
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn confirm_provider_switch_test_hook(
    state: &AppState,
    review_token: &str,
) -> Result<ConfirmedProviderSwitch, AppError> {
    confirm_provider_switch_internal(state, review_token)
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn lock_codex_provider_switch_test_hook(state: &AppState) -> tokio::sync::OwnedMutexGuard<()> {
    futures::executor::block_on(
        state
            .proxy_service
            .lock_switch_for_app(AppType::Codex.as_str()),
    )
}

#[allow(non_snake_case)]
#[tauri::command(rename_all = "camelCase")]
pub async fn confirmProviderSwitch(
    app_handle: tauri::AppHandle,
    review_token: String,
) -> Result<ConfirmedProviderSwitch, String> {
    let _guard = crate::services::session_usage::session_sync_mutex()
        .lock()
        .await;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle
            .try_state::<AppState>()
            .ok_or_else(|| "Application state is unavailable".to_string())?;
        confirm_provider_switch_internal(state.inner(), &review_token)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "Provider switch task failed".to_string())?
}

/// Parse a deep link URL and return the parsed request for frontend confirmation
#[tauri::command]
pub fn parse_deeplink(url: String) -> Result<DeepLinkRequest, String> {
    log::info!("Parsing deep link URL: {}", crate::url_for_log(&url));
    parse_deeplink_request_url(&url).map_err(|e| e.to_string())
}

/// Merge configuration from Base64/URL into a deep link request
/// This is used by the frontend to show the complete configuration in the confirmation dialog
#[tauri::command]
pub fn merge_deeplink_config(
    request: DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, String> {
    log::info!("Merging config for deep link request: {:?}", request.name);
    crate::deeplink::parse_and_merge_config(&request).map_err(|e| e.to_string())
}

/// Import a provider from a deep link request (legacy, kept for compatibility)
#[tauri::command]
pub fn import_from_deeplink(
    state: State<AppState>,
    request: DeepLinkImportRequest,
) -> Result<String, String> {
    log::info!(
        "Importing provider from deep link: {:?} for app {:?}",
        request.name,
        request.app
    );

    let provider_id = import_provider_from_deeplink(&state, request).map_err(|e| e.to_string())?;

    log::info!("Successfully imported provider with ID: {provider_id}");

    Ok(provider_id)
}

/// Import resource from a deep link request (unified handler)
#[tauri::command]
pub async fn import_from_deeplink_unified(
    state: State<'_, AppState>,
    request: DeepLinkImportRequest,
) -> Result<serde_json::Value, String> {
    log::info!("Importing {} resource from deep link", request.resource);

    match request.resource.as_str() {
        "provider" => {
            let provider_id =
                import_provider_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "provider",
                "id": provider_id
            }))
        }
        "prompt" => {
            let prompt_id =
                import_prompt_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "prompt",
                "id": prompt_id
            }))
        }
        "mcp" => {
            let result = import_mcp_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            // Add type field to the result
            Ok(serde_json::json!({
                "type": "mcp",
                "importedCount": result.imported_count,
                "importedIds": result.imported_ids,
                "failed": result.failed
            }))
        }
        "skill" => {
            let skill_key =
                import_skill_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "skill",
                "key": skill_key
            }))
        }
        _ => Err(format!("Unsupported resource type: {}", request.resource)),
    }
}
