//! Read-only discovery of an external Codex route after an official selection.
//! Only CC Switch's provider records and selection are updated; Codex keeps
//! ownership of its files and native login.

use serde_json::{json, Value};
use toml_edit::{DocumentMut, Item};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::live::engine::DeviceStore;
use crate::live::project::codex::{CodexProjection, Route, RouteAuth, RowInput};
use crate::mode::{current, state as mode};
use crate::provider::Provider;
use crate::services::ProxyService;
use crate::store::AppState;

use super::codex_direct;

const PROFILE_ROUTE_FIELDS: &[&str] = &[
    "model_provider",
    "openai_base_url",
    "experimental_bearer_token",
];

/// Resolve the same route for endpoint and proxy checks. Profile route overrides
/// are detected here but cannot be adopted: the switch engine writes top-level
/// routes and deliberately refuses to overwrite the user's profile.
fn effective_route(doc: &DocumentMut) -> Option<(DocumentMut, bool)> {
    let mut effective = doc.clone();
    let Some(profile) = doc.get("profile") else {
        return Some((effective, false));
    };
    let profile = doc
        .get("profiles")?
        .as_table_like()?
        .get(profile.as_str()?)?
        .as_table_like()?;
    let mut overrides_route = false;
    for key in PROFILE_ROUTE_FIELDS {
        if let Some(value) = profile.get(key) {
            effective[key] = value.clone();
            overrides_route = true;
        }
    }
    Some((effective, overrides_route))
}

fn project(settings: &Value) -> Result<CodexProjection, AppError> {
    CodexProjection::of(&RowInput {
        settings,
        official: false,
        proxy_injected_oauth: false,
    })
}

/// Never copy native OAuth or other login carriers into a syncable row. A legacy
/// API key is imported only when the route cannot authenticate without it. A
/// route with its own bearer/env/header credentials must not inherit auth.json.
fn import_settings(live: &Value) -> Option<Value> {
    let mut settings = json!({"auth": {}, "config": live.get("config")?});
    if let Err(error) = project(&settings) {
        if !crate::live::project::codex::is_keyless_fallback(&error) {
            return None;
        }
        let auth = live.get("auth")?;
        if !crate::codex_config::codex_live_auth_is_stale_third_party_residue(auth) {
            return None;
        }
        let key = crate::codex_config::extract_codex_auth_api_key(auth)?;
        settings["auth"] = json!({"OPENAI_API_KEY": key});
        project(&settings).ok()?;
    }
    Some(settings)
}

/// Compare the route the existing switch engine produces, not full snapshots:
/// it renames routes to `custom`, moves API keys into bearer tokens and derives
/// requires_openai_auth from the device login. MCP, comments and global settings
/// do not identify a provider. Keep all routing/auth fields, including headers.
fn identity(settings: &Value) -> Option<(RouteAuth, toml::Value)> {
    let Route::Custom { mut table, auth } = project(settings).ok()?.route else {
        return None;
    };
    table.remove("name");
    table.remove("requires_openai_auth");
    if !table.contains_key("wire_api") {
        table.insert("wire_api", toml_edit::value("responses"));
    }
    let mut doc = DocumentMut::new();
    doc["route"] = Item::Table(table);
    Some((auth, doc.to_string().parse().ok()?))
}

fn model(settings: &Value) -> Option<String> {
    let doc = settings
        .get("config")?
        .as_str()?
        .parse::<DocumentMut>()
        .ok()?;
    doc.get("model")?.as_str().map(str::to_string)
}

/// Run during startup, before proxy restoration and UI queries. Errors leave
/// both selection stores and any newly imported row unchanged.
pub(crate) fn startup(state: &AppState) -> Result<bool, AppError> {
    let app = AppType::Codex;
    let store = DeviceStore::for_device();
    let mode = mode::mode_state(&store, app.as_str())?;
    if mode.mode != Some(mode::Mode::Direct)
        || mode.attached
        || mode::pending(&store, app.as_str())?.is_some()
    {
        return Ok(false);
    }
    let Some(previous) = current::direct_provider(&state.db, &app)? else {
        return Ok(false);
    };
    if !codex_direct::is_official(&previous) {
        return Ok(false);
    }
    // A managed card owns refresh-token adoption on switch-away. Changing only
    // the pointer would bypass that handoff and leave the account manager with
    // an older generation if a later switch removes the live login.
    if super::ProviderService::managed_codex_oauth_account_id(&previous).is_some() {
        return Ok(false);
    }

    let live = crate::codex_config::read_codex_live_settings()?;
    let doc = live["config"]
        .as_str()
        .unwrap_or_default()
        .parse::<DocumentMut>()
        .map_err(|err| AppError::Config(format!("Invalid Codex config.toml: {err}")))?;
    let Some((effective, overrides_route)) = effective_route(&doc) else {
        return Ok(false);
    };
    let effective_settings = json!({"auth": live["auth"], "config": effective.to_string()});
    if ProxyService::config_has_proxy_placeholder(&app, &effective_settings) {
        return Ok(false);
    }
    let id = match effective.get("model_provider") {
        None => "openai",
        Some(item) => match item.as_str() {
            Some(id) if !id.trim().is_empty() => id,
            _ => return Ok(false),
        },
    };
    let (name, endpoint) = if crate::codex_config::is_custom_codex_model_provider_id(id) {
        let table = effective
            .get("model_providers")
            .and_then(Item::as_table_like)
            .and_then(|providers| providers.get(id))
            .and_then(Item::as_table_like);
        (id, table.and_then(|table| table.get("base_url")))
    } else if id == "openai" {
        ("custom", effective.get("openai_base_url"))
    } else {
        return Ok(false);
    };
    // The official history mirror has no endpoint; unused custom tables never
    // participate. Reject our proxy's address even without a placeholder key.
    let Some(endpoint) = endpoint.and_then(Item::as_str) else {
        return Ok(false);
    };
    let Ok(url) = url::Url::parse(endpoint) else {
        return Ok(false);
    };
    // An explicit native OpenAI endpoint is still official. Importing its API
    // login as a custom row would make row_facts classify the key as third-party
    // residue, so switching back to the official card would delete auth.json.
    if matches!(
        url.host_str(),
        Some("api.openai.com" | "chatgpt.com" | "chat.openai.com")
    ) {
        return Ok(false);
    }
    let (address, port) = state.db.get_proxy_listen_sync();
    if codex_direct::is_proxy_base_url(endpoint.trim().trim_end_matches('/'), &address, port)
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
    {
        return Ok(false);
    }
    if overrides_route {
        log::debug!("Codex profile owns the live route; skipping automatic provider import");
        return Ok(false);
    }
    let Some(settings) = import_settings(&live) else {
        return Ok(false);
    };
    let Some(live_identity) = identity(&settings) else {
        return Ok(false);
    };
    let providers = state.db.get_all_providers(app.as_str())?;
    let mut matches: Vec<_> = providers
        .values()
        .filter(|row| {
            !codex_direct::is_official(row)
                && !row.uses_proxy_injected_oauth()
                && identity(&row.settings_config).as_ref() == Some(&live_identity)
        })
        .collect();
    if matches.len() > 1 {
        let live_model = model(&settings);
        matches.retain(|row| model(&row.settings_config) == live_model);
        if matches.len() != 1 {
            log::debug!(
                "Multiple Codex providers match the live route; keeping the saved selection"
            );
            return Ok(false);
        }
    }
    let imported = if matches.is_empty() {
        let id = if providers.contains_key("default") {
            uuid::Uuid::new_v4().to_string()
        } else {
            "default".to_string()
        };
        let mut provider = Provider::with_id(id, name.to_string(), settings, None);
        provider.category = Some("custom".to_string());
        super::keep_common_config_for_old_versions(&mut provider);
        Some(provider)
    } else {
        None
    };
    let target = imported.as_ref().unwrap_or_else(|| matches[0]);
    let previous_local = current::local_direct_pointer(&app);
    let mut local_written = false;
    let result =
        state
            .db
            .save_provider_selection(app.as_str(), imported.as_ref(), &target.id, || {
                crate::settings::set_current_provider(&app, Some(&target.id))?;
                local_written = true;
                Ok(())
            });
    if let Err(error) = result {
        // The DB transaction has already rolled back. A commit failure after
        // publishing the local pointer also needs to restore that pointer.
        if local_written {
            crate::settings::set_current_provider(&app, previous_local.as_deref()).map_err(
                |rollback| {
                    AppError::Message(format!(
                        "{error}; restoring local selection failed: {rollback}"
                    ))
                },
            )?;
        }
        return Err(error);
    }
    log::info!(
        "Reconciled Codex current provider from {} to {} (no live files written)",
        previous.id,
        target.id
    );
    Ok(true)
}
