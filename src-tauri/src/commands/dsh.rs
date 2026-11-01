//! The DeepSeek account login shared by DSH Web and Desktop.

/// The DeepSeek account (OAuth) login shared by DSH Web and Desktop, with profile and balance.
#[tauri::command]
pub async fn dsh_account_status() -> Result<crate::services::dsh_account::DshAccountStatus, String>
{
    crate::services::dsh_account::status().await
}

/// Sign out of the DeepSeek account for both DSH Web and Desktop.
#[tauri::command]
pub async fn dsh_account_sign_out() -> Result<(), String> {
    crate::services::dsh_account::sign_out().await
}

/// Start a DeepSeek account browser sign-in; the UI opens `authUrl` and polls.
#[tauri::command]
pub async fn dsh_account_login_start() -> Result<crate::services::dsh_account::DshLoginStart, String>
{
    crate::services::dsh_account::start_login().await
}

/// `false` while waiting for the browser, `true` once the shared login is stored.
#[tauri::command(rename_all = "camelCase")]
pub fn dsh_account_login_poll(flow_id: String) -> Result<bool, String> {
    crate::services::dsh_account::poll_login(&flow_id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn dsh_account_login_cancel(flow_id: String) {
    crate::services::dsh_account::cancel_login(&flow_id)
}

/// Only return the local account identifier; OAuth tokens never cross IPC.
#[tauri::command]
pub fn dsh_codex_account() -> Result<Option<String>, String> {
    crate::dsh_config::codex_binding().map_err(|_| "Cannot read DSH Codex binding".into())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn dsh_codex_bind(
    account_id: String,
    provider: Option<crate::provider::Provider>,
    state: tauri::State<'_, crate::commands::CodexOAuthState>,
    app_state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), String> {
    let mut provider = provider;
    if let Some(provider) = provider.as_mut() {
        if provider.id != "openai-codex" || provider.name.trim().is_empty() {
            return Err("Invalid DSH Codex provider identity".into());
        }
        if !provider.settings_config.is_object()
            || provider
                .settings_config
                .as_object()
                .unwrap()
                .keys()
                .any(|key| !matches!(key.as_str(), "name" | "models"))
        {
            return Err("Codex configuration only supports model overrides".into());
        }
        provider.name = provider.name.trim().to_owned();
        provider.settings_config["name"] = serde_json::json!(provider.name);
        crate::dsh_config::validate_provider(&provider.id, &provider.settings_config)
            .map_err(|e| e.to_string())?;
        // Preserve quota preferences and other local metadata when editing the native route.
        if let Some(saved) = app_state
            .db
            .get_provider_by_id("openai-codex", "dsh")
            .map_err(|e| e.to_string())?
        {
            provider.meta = saved.meta;
            provider.created_at = saved.created_at;
            provider.sort_index = saved.sort_index;
        }
    }
    state
        .0
        .bind_dsh_account(
            &account_id,
            provider.as_ref().map(|p| &p.settings_config),
            || {
                if let Some(provider) = &provider {
                    app_state.db.save_provider("dsh", provider)?;
                }
                Ok(())
            },
        )
        .await
        .map_err(|e| e.to_string())
}

/// Save quota preferences only; an OAuth route must not pass API-key validation.
#[tauri::command(rename_all = "camelCase")]
pub fn dsh_codex_usage_update(
    enabled: bool,
    auto_query_interval: u64,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), String> {
    if crate::dsh_config::codex_binding()
        .map_err(|_| "Cannot read DSH Codex binding")?
        .is_none()
    {
        return Err("Select a DSH Codex account first".into());
    }
    let mut providers =
        crate::services::ProviderService::list(&state, crate::app_config::AppType::Dsh)
            .map_err(|e| e.to_string())?;
    let provider = providers
        .get_mut("openai-codex")
        .ok_or("DSH Codex provider is missing")?;
    let script = serde_json::from_value(serde_json::json!({"enabled":enabled,"language":"javascript","code":"","templateType":"official_subscription","autoQueryInterval":auto_query_interval.min(1440)})).map_err(|_| "Invalid quota settings")?;
    provider
        .meta
        .get_or_insert_with(Default::default)
        .usage_script = Some(script);
    state
        .db
        .save_provider("dsh", provider)
        .map_err(|e| e.to_string())
}
