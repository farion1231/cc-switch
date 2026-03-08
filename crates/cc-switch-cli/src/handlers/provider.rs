//! Provider command handlers

use crate::cli::{
    ProviderCommands, ProviderCommonConfigSnippetCommands, ProviderEndpointCommands,
    ProviderStreamCheckCommands, ProviderStreamCheckConfigCommands, ProviderUsageScriptCommands,
    UniversalProviderCommands,
};
use crate::handlers::common::parse_app_type;
use crate::output::Printer;
use anyhow::Context;
use cc_switch_core::{
    config::sanitize_provider_name,
    provider::{ProviderMeta, UsageScript},
    AppState, AppType, Provider, ProviderSortUpdate, SpeedtestService, StreamCheckConfig,
    StreamCheckResult, StreamCheckService, UniversalProvider, UsageService,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NamedStreamCheckResult {
    provider_id: String,
    #[serde(flatten)]
    result: StreamCheckResult,
}

pub async fn handle(
    cmd: ProviderCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProviderCommands::List { app } => handle_list(&app, state, printer).await,
        ProviderCommands::Show { id, app } => handle_show(&id, &app, state, printer).await,
        ProviderCommands::Add {
            app,
            name,
            base_url,
            api_key,
            from_json,
        } => {
            handle_add(
                &app,
                name.as_deref(),
                base_url.as_deref(),
                api_key.as_deref(),
                from_json.as_deref(),
                state,
                printer,
            )
            .await
        }
        ProviderCommands::Edit {
            id,
            app,
            set_api_key,
            set_base_url,
            set_name,
        } => {
            handle_edit(
                &id,
                &app,
                set_api_key.as_deref(),
                set_base_url.as_deref(),
                set_name.as_deref(),
                state,
                printer,
            )
            .await
        }
        ProviderCommands::Delete { id, app, yes } => {
            handle_delete(&id, &app, yes, state, printer).await
        }
        ProviderCommands::Duplicate {
            id,
            app,
            name,
            new_id,
        } => {
            handle_duplicate(
                &id,
                &app,
                name.as_deref(),
                new_id.as_deref(),
                state,
                printer,
            )
            .await
        }
        ProviderCommands::Switch { id, app } => handle_switch(&id, &app, state, printer).await,
        ProviderCommands::ReadLive { app } => handle_read_live(&app, printer).await,
        ProviderCommands::ImportLive { app } => handle_import_live(&app, state, printer).await,
        ProviderCommands::RemoveFromLive { id, app } => {
            handle_remove_from_live(&id, &app, state, printer).await
        }
        ProviderCommands::SortOrder { id, app, index } => {
            handle_sort_order(&id, &app, index, state, printer).await
        }
        ProviderCommands::Usage { id, app } => handle_usage(&id, &app, state, printer).await,
        ProviderCommands::Endpoint { cmd } => handle_endpoint(cmd, state, printer).await,
        ProviderCommands::CommonConfigSnippet { cmd } => {
            handle_common_config_snippet(cmd, state, printer).await
        }
        ProviderCommands::UsageScript { cmd } => handle_usage_script(cmd, state, printer).await,
        ProviderCommands::StreamCheck { cmd } => handle_stream_check(cmd, state, printer).await,
        ProviderCommands::Universal(cmd) => handle_universal(cmd, state, printer).await,
    }
}

async fn handle_list(app: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let providers = cc_switch_core::ProviderService::list(state, app_type)?;
    printer.print_providers(&providers)?;
    Ok(())
}

async fn handle_show(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let providers = cc_switch_core::ProviderService::list(state, app_type)?;
    let provider = providers
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", id))?;
    printer.print_provider_detail(provider)?;
    Ok(())
}

async fn handle_add(
    app: &str,
    name: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
    from_json: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let provider = if let Some(path) = from_json {
        load_provider_from_file(path, &app_type, name, base_url, api_key)?
    } else {
        let name = name.ok_or_else(|| anyhow::anyhow!("Provider add requires --name"))?;
        let base_url =
            base_url.ok_or_else(|| anyhow::anyhow!("Provider add requires --base-url"))?;
        let api_key = api_key.ok_or_else(|| anyhow::anyhow!("Provider add requires --api-key"))?;
        build_provider(&app_type, name, base_url, api_key)
    };

    if cc_switch_core::ProviderService::list(state, app_type.clone())?.contains_key(&provider.id) {
        anyhow::bail!(
            "Provider '{}' already exists for {}. Use `provider edit` instead.",
            provider.id,
            app
        );
    }

    cc_switch_core::ProviderService::add(state, app_type, provider.clone())?;
    printer.success(format!("✓ Added provider '{}' for {}", provider.id, app));
    Ok(())
}

async fn handle_edit(
    id: &str,
    app: &str,
    set_api_key: Option<&str>,
    set_base_url: Option<&str>,
    set_name: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    if set_api_key.is_none() && set_base_url.is_none() && set_name.is_none() {
        anyhow::bail!(
            "Provider edit requires at least one of --set-name, --set-base-url or --set-api-key"
        );
    }

    let app_type = parse_app_type(app)?;
    let mut provider = cc_switch_core::ProviderService::list(state, app_type.clone())?
        .shift_remove(id)
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", id))?;

    if let Some(name) = set_name {
        provider.name = name.to_string();
    }
    if let Some(base_url) = set_base_url {
        set_provider_base_url(&mut provider.settings_config, &app_type, base_url)?;
    }
    if let Some(api_key) = set_api_key {
        set_provider_api_key(&mut provider.settings_config, &app_type, api_key)?;
    }

    cc_switch_core::ProviderService::update(state, app_type, provider)?;
    printer.success(format!("✓ Updated provider '{}' for {}", id, app));
    Ok(())
}

async fn handle_delete(
    id: &str,
    app: &str,
    yes: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    if !yes {
        anyhow::bail!("Provider delete is destructive. Re-run with --yes to confirm.");
    }

    let app_type = parse_app_type(app)?;
    cc_switch_core::ProviderService::delete(state, app_type, id)?;
    printer.success(format!("✓ Deleted provider '{}' for {}", id, app));
    Ok(())
}

async fn handle_duplicate(
    id: &str,
    app: &str,
    new_name: Option<&str>,
    new_id: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let providers = cc_switch_core::ProviderService::list(state, app_type.clone())?;
    let source = providers
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", id))?;

    let mut duplicated = source.clone();
    duplicated.name = new_name
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{} Copy", source.name));
    duplicated.id = new_id
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| provider_id_from_name(&duplicated.name));
    duplicated.created_at = Some(chrono::Utc::now().timestamp_millis());

    if duplicated.id == id {
        anyhow::bail!("Duplicated provider ID must differ from the source provider ID");
    }
    if providers.contains_key(&duplicated.id) {
        anyhow::bail!(
            "Provider '{}' already exists for {}. Choose a different --name or --new-id.",
            duplicated.id,
            app
        );
    }

    cc_switch_core::ProviderService::add(state, app_type, duplicated.clone())?;
    printer.success(format!(
        "✓ Duplicated provider '{}' to '{}' for {}",
        id, duplicated.id, app
    ));
    Ok(())
}

async fn handle_switch(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    cc_switch_core::ProviderService::switch(state, app_type, id)?;
    printer.success(format!("✓ Switched to provider '{}' for {}", id, app));
    Ok(())
}

async fn handle_read_live(app: &str, printer: &Printer) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let live = cc_switch_core::ProviderService::read_live_settings(app_type)?;
    printer.print_value(&live)?;
    Ok(())
}

async fn handle_import_live(app: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;

    match app_type {
        AppType::OpenCode => {
            let imported =
                cc_switch_core::ProviderService::import_opencode_providers_from_live(state)?;
            if imported == 0 {
                printer.warn("No OpenCode live providers were imported.");
            } else {
                printer.success(format!(
                    "✓ Imported {} provider(s) from OpenCode live config",
                    imported
                ));
            }
        }
        AppType::OpenClaw => {
            let imported =
                cc_switch_core::ProviderService::import_openclaw_providers_from_live(state)?;
            if imported == 0 {
                printer.warn("No OpenClaw live providers were imported.");
            } else {
                printer.success(format!(
                    "✓ Imported {} provider(s) from OpenClaw live config",
                    imported
                ));
            }
        }
        _ => {
            let imported = cc_switch_core::ProviderService::import_default_config(state, app_type)?;
            if imported {
                printer.success(format!(
                    "✓ Imported current live config as the default {}",
                    app
                ));
            } else {
                printer.warn(format!(
                    "Skipped importing live config for {} because providers already exist.",
                    app
                ));
            }
        }
    }

    Ok(())
}

async fn handle_remove_from_live(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    ensure_provider_exists(state, &app_type, id)?;
    cc_switch_core::ProviderService::remove_from_live_config(state, app_type, id)?;
    printer.success(format!(
        "✓ Removed provider '{}' from live config for {} (database record kept)",
        id, app
    ));
    Ok(())
}

async fn handle_sort_order(
    id: &str,
    app: &str,
    index: usize,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    ensure_provider_exists(state, &app_type, id)?;
    cc_switch_core::ProviderService::update_sort_order(
        state,
        app_type,
        vec![ProviderSortUpdate {
            id: id.to_string(),
            sort_index: index,
        }],
    )?;
    printer.success(format!(
        "✓ Updated sort order for provider '{}' to {} in {}",
        id, index, app
    ));
    Ok(())
}

fn ensure_provider_exists(state: &AppState, app_type: &AppType, id: &str) -> anyhow::Result<()> {
    if cc_switch_core::ProviderService::list(state, app_type.clone())?.contains_key(id) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Provider not found: {}", id))
    }
}

fn load_provider(state: &AppState, app_type: &AppType, id: &str) -> anyhow::Result<Provider> {
    cc_switch_core::ProviderService::list(state, app_type.clone())?
        .shift_remove(id)
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", id))
}

async fn handle_endpoint(
    cmd: ProviderEndpointCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProviderEndpointCommands::List { id, app } => {
            let app_type = parse_app_type(&app)?;
            ensure_provider_exists(state, &app_type, &id)?;
            let endpoints =
                cc_switch_core::ProviderService::get_custom_endpoints(state, app_type, &id)?;
            printer.print_custom_endpoints(&endpoints)?;
        }
        ProviderEndpointCommands::Add { id, app, url } => {
            let app_type = parse_app_type(&app)?;
            ensure_provider_exists(state, &app_type, &id)?;
            cc_switch_core::ProviderService::add_custom_endpoint(
                state,
                app_type,
                &id,
                url.clone(),
            )?;
            printer.success(format!(
                "✓ Added custom endpoint '{}' for provider '{}' in {}",
                url, id, app
            ));
        }
        ProviderEndpointCommands::Remove { id, app, url } => {
            let app_type = parse_app_type(&app)?;
            ensure_provider_exists(state, &app_type, &id)?;
            cc_switch_core::ProviderService::remove_custom_endpoint(
                state,
                app_type,
                &id,
                url.clone(),
            )?;
            printer.success(format!(
                "✓ Removed custom endpoint '{}' from provider '{}' in {}",
                url, id, app
            ));
        }
        ProviderEndpointCommands::MarkUsed { id, app, url } => {
            let app_type = parse_app_type(&app)?;
            ensure_provider_exists(state, &app_type, &id)?;
            cc_switch_core::ProviderService::update_endpoint_last_used(
                state,
                app_type,
                &id,
                url.clone(),
            )?;
            printer.success(format!(
                "✓ Marked endpoint '{}' as last used for provider '{}' in {}",
                url, id, app
            ));
        }
        ProviderEndpointCommands::Speedtest { id, app, timeout } => {
            let app_type = parse_app_type(&app)?;
            let provider = load_provider(state, &app_type, &id)?;
            let urls = collect_endpoint_urls(&provider, &app_type, state)?;
            if urls.is_empty() {
                anyhow::bail!("No primary or custom endpoints found for provider '{}'", id);
            }
            let results = SpeedtestService::test_endpoints(urls, timeout).await?;
            printer.print_endpoint_latencies(&results)?;
        }
    }

    Ok(())
}

fn collect_endpoint_urls(
    provider: &Provider,
    app_type: &AppType,
    state: &AppState,
) -> anyhow::Result<Vec<String>> {
    let mut urls = Vec::new();

    if let Some(primary) = extract_provider_base_url(&provider.settings_config, app_type)? {
        push_unique_endpoint(&mut urls, primary);
    }

    for endpoint in cc_switch_core::ProviderService::get_custom_endpoints(
        state,
        app_type.clone(),
        &provider.id,
    )? {
        push_unique_endpoint(&mut urls, endpoint.url);
    }

    Ok(urls)
}

fn push_unique_endpoint(urls: &mut Vec<String>, url: String) {
    let normalized = url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() || urls.iter().any(|item| item == &normalized) {
        return;
    }
    urls.push(normalized);
}

fn extract_provider_base_url(
    settings_config: &Value,
    app_type: &AppType,
) -> anyhow::Result<Option<String>> {
    let value = match app_type {
        AppType::Claude => settings_config
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(Value::as_str)
            .map(|item| item.to_string()),
        AppType::Codex => {
            let config_text = settings_config
                .get("config")
                .and_then(Value::as_str)
                .unwrap_or_default();
            config_text.lines().find_map(|line| {
                let trimmed = line.trim();
                if !trimmed.starts_with("base_url") {
                    return None;
                }
                let value = trimmed.split_once('=')?.1.trim();
                value
                    .strip_prefix('"')
                    .and_then(|item| item.strip_suffix('"'))
                    .or_else(|| {
                        value
                            .strip_prefix('\'')
                            .and_then(|item| item.strip_suffix('\''))
                    })
                    .map(|item| item.to_string())
            })
        }
        AppType::Gemini => settings_config
            .get("env")
            .and_then(|env| env.get("GOOGLE_GEMINI_BASE_URL"))
            .and_then(Value::as_str)
            .map(|item| item.to_string()),
        AppType::OpenCode => settings_config
            .get("options")
            .and_then(|options| options.get("baseURL"))
            .and_then(Value::as_str)
            .map(|item| item.to_string()),
        AppType::OpenClaw => settings_config
            .get("baseUrl")
            .and_then(Value::as_str)
            .map(|item| item.to_string()),
    };

    Ok(value.and_then(|item| {
        let trimmed = item.trim().trim_end_matches('/').to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }))
}

fn resolve_common_config_input(
    file: Option<&str>,
    value: Option<&str>,
    clear: bool,
) -> anyhow::Result<Option<String>> {
    if clear {
        return Ok(None);
    }

    if let Some(path) = file {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read snippet file: {path}"))?;
        return Ok(Some(content));
    }

    if let Some(value) = value {
        return Ok(Some(value.to_string()));
    }

    anyhow::bail!("Provider common-config-snippet set requires one of --file, --value or --clear");
}

fn validate_common_config_snippet(app_type: &AppType, snippet: &str) -> anyhow::Result<()> {
    let trimmed = snippet.trim();
    if trimmed.is_empty() || matches!(app_type, AppType::Codex) {
        return Ok(());
    }

    serde_json::from_str::<Value>(trimmed)
        .with_context(|| format!("Invalid {} common config snippet JSON", app_type.as_str()))?;
    Ok(())
}

fn load_usage_script_input(file: Option<&str>, value: Option<&str>) -> anyhow::Result<UsageScript> {
    let raw = if let Some(path) = file {
        fs::read_to_string(path)
            .with_context(|| format!("Failed to read usage script file: {path}"))?
    } else if let Some(value) = value {
        value.to_string()
    } else {
        anyhow::bail!("Provider usage-script requires either --file or --value");
    };

    serde_json::from_str::<UsageScript>(&raw)
        .with_context(|| "Invalid usage script JSON".to_string())
}

fn load_stream_check_config_input(
    file: Option<&str>,
    value: Option<&str>,
) -> anyhow::Result<StreamCheckConfig> {
    let raw = if let Some(path) = file {
        fs::read_to_string(path)
            .with_context(|| format!("Failed to read stream-check config file: {path}"))?
    } else if let Some(value) = value {
        value.to_string()
    } else {
        anyhow::bail!("Provider stream-check config set requires either --file or --value");
    };

    serde_json::from_str::<StreamCheckConfig>(&raw)
        .with_context(|| "Invalid stream-check config JSON".to_string())
}

fn saved_usage_script(provider: &Provider, id: &str) -> anyhow::Result<UsageScript> {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Provider '{}' has no saved usage script", id))
}

fn normalize_provider_meta(provider: &mut Provider) {
    if provider.meta.as_ref().is_some_and(provider_meta_is_empty) {
        provider.meta = None;
    }
}

fn provider_meta_is_empty(meta: &ProviderMeta) -> bool {
    meta.custom_endpoints.is_empty()
        && meta.usage_script.is_none()
        && meta.endpoint_auto_select.is_none()
        && meta.is_partner.is_none()
        && meta.partner_promotion_key.is_none()
        && meta.cost_multiplier.is_none()
        && meta.pricing_model_source.is_none()
        && meta.limit_daily_usd.is_none()
        && meta.limit_monthly_usd.is_none()
        && meta.test_config.is_none()
        && meta.proxy_config.is_none()
        && meta.api_format.is_none()
}

async fn handle_usage(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let provider = cc_switch_core::ProviderService::list(state, app_type.clone())?
        .shift_remove(id)
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", id))?;

    let has_enabled_usage_script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .is_some_and(|script| script.enabled);

    if has_enabled_usage_script {
        let result = cc_switch_core::ProviderService::query_usage(state, app_type, id).await?;
        printer.print_value(&result)?;
    } else {
        printer.warn(format!(
            "No enabled usage script configured for '{}'; showing local proxy usage summary instead.",
            id
        ));
        let summary = UsageService::get_provider_summary_all(&state.db, app_type.as_str(), id)?;
        printer.print_usage_summary(&summary)?;
    }
    Ok(())
}

async fn handle_common_config_snippet(
    cmd: ProviderCommonConfigSnippetCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProviderCommonConfigSnippetCommands::Get { app } => {
            let app_type = parse_app_type(&app)?;
            let snippet = state.db.get_config_snippet(app_type.as_str())?;
            printer.print_common_config_snippet(app_type.as_str(), snippet.as_deref())?;
        }
        ProviderCommonConfigSnippetCommands::Set {
            app,
            file,
            value,
            clear,
        } => {
            let app_type = parse_app_type(&app)?;
            let snippet = resolve_common_config_input(file.as_deref(), value.as_deref(), clear)?;
            if let Some(ref snippet) = snippet {
                validate_common_config_snippet(&app_type, snippet)?;
            }
            let clear_effective =
                clear || snippet.as_ref().is_some_and(|item| item.trim().is_empty());
            state.db.set_config_snippet(app_type.as_str(), snippet)?;
            if clear_effective {
                printer.success(format!(
                    "✓ Cleared common config snippet for {}",
                    app_type.as_str()
                ));
            } else {
                printer.success(format!(
                    "✓ Saved common config snippet for {}",
                    app_type.as_str()
                ));
            }
        }
        ProviderCommonConfigSnippetCommands::Extract { app } => {
            let app_type = parse_app_type(&app)?;
            let snippet = cc_switch_core::ProviderService::extract_common_config_snippet(
                state,
                app_type.clone(),
            )?;
            printer.print_common_config_snippet(app_type.as_str(), Some(&snippet))?;
        }
    }

    Ok(())
}

async fn handle_usage_script(
    cmd: ProviderUsageScriptCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProviderUsageScriptCommands::Show { id, app } => {
            let app_type = parse_app_type(&app)?;
            let provider = load_provider(state, &app_type, &id)?;
            printer.print_value(&serde_json::json!({
                "app": app_type.as_str(),
                "providerId": id,
                "usageScript": provider.meta.and_then(|meta| meta.usage_script),
            }))?;
        }
        ProviderUsageScriptCommands::Save {
            id,
            app,
            file,
            value,
            clear,
        } => {
            let app_type = parse_app_type(&app)?;
            let mut provider = load_provider(state, &app_type, &id)?;
            if clear {
                if let Some(meta) = provider.meta.as_mut() {
                    meta.usage_script = None;
                }
            } else {
                let script = load_usage_script_input(file.as_deref(), value.as_deref())?;
                let meta = provider.meta.get_or_insert_with(ProviderMeta::default);
                meta.usage_script = Some(script);
            }
            normalize_provider_meta(&mut provider);
            cc_switch_core::ProviderService::update(state, app_type, provider)?;
            if clear {
                printer.success(format!(
                    "✓ Cleared usage script for provider '{}' in {}",
                    id, app
                ));
            } else {
                printer.success(format!(
                    "✓ Saved usage script for provider '{}' in {}",
                    id, app
                ));
            }
        }
        ProviderUsageScriptCommands::Test {
            id,
            app,
            file,
            value,
        } => {
            let app_type = parse_app_type(&app)?;
            let provider = load_provider(state, &app_type, &id)?;
            let script = if file.is_some() || value.is_some() {
                load_usage_script_input(file.as_deref(), value.as_deref())?
            } else {
                saved_usage_script(&provider, &id)?
            };
            let result = cc_switch_core::ProviderService::test_usage_script(
                state,
                app_type,
                &id,
                &script.code,
                script.timeout.unwrap_or(10),
                script.api_key.as_deref(),
                script.base_url.as_deref(),
                script.access_token.as_deref(),
                script.user_id.as_deref(),
                script.template_type.as_deref(),
            )
            .await?;
            printer.print_value(&result)?;
        }
        ProviderUsageScriptCommands::Query { id, app } => {
            let app_type = parse_app_type(&app)?;
            ensure_provider_exists(state, &app_type, &id)?;
            let result = cc_switch_core::ProviderService::query_usage(state, app_type, &id).await?;
            printer.print_value(&result)?;
        }
    }

    Ok(())
}

async fn handle_stream_check(
    cmd: ProviderStreamCheckCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProviderStreamCheckCommands::Run { id, app } => {
            let app_type = parse_app_type(&app)?;
            let result = StreamCheckService::check_provider(state, app_type, &id).await?;
            printer.print_value(&result)?;
        }
        ProviderStreamCheckCommands::RunAll {
            app,
            proxy_targets_only,
        } => {
            let app_type = parse_app_type(&app)?;
            let results =
                StreamCheckService::check_all_providers(state, app_type, proxy_targets_only)
                    .await?;
            let rows: Vec<NamedStreamCheckResult> = results
                .into_iter()
                .map(|(provider_id, result)| NamedStreamCheckResult {
                    provider_id,
                    result,
                })
                .collect();
            printer.print_value(&rows)?;
        }
        ProviderStreamCheckCommands::Config { cmd } => match cmd {
            ProviderStreamCheckConfigCommands::Get => {
                let config = StreamCheckService::get_config(state)?;
                printer.print_value(&config)?;
            }
            ProviderStreamCheckConfigCommands::Set { file, value } => {
                let config = load_stream_check_config_input(file.as_deref(), value.as_deref())?;
                StreamCheckService::save_config(state, &config)?;
                printer.success("✓ Saved stream-check config");
            }
        },
    }

    Ok(())
}

async fn handle_universal(
    cmd: UniversalProviderCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        UniversalProviderCommands::List => {
            let providers = cc_switch_core::ProviderService::list_universal(state)?;
            printer.print_universal_providers(&providers)?;
        }
        UniversalProviderCommands::Show { id } => {
            let provider = cc_switch_core::ProviderService::get_universal(state, &id)?
                .ok_or_else(|| anyhow::anyhow!("Universal provider not found: {}", id))?;
            printer.print_value(&provider)?;
        }
        UniversalProviderCommands::Add {
            name,
            apps,
            base_url,
            api_key,
        } => {
            let base_url = base_url
                .ok_or_else(|| anyhow::anyhow!("Universal provider add requires --base-url"))?;
            let api_key = api_key
                .ok_or_else(|| anyhow::anyhow!("Universal provider add requires --api-key"))?;
            let id = provider_id_from_name(&name);

            if cc_switch_core::ProviderService::list_universal(state)?.contains_key(&id) {
                anyhow::bail!(
                    "Universal provider '{}' already exists. Use a different name or delete it first.",
                    id
                );
            }

            let mut provider = UniversalProvider::new(
                id.clone(),
                name,
                "openai-compatible".to_string(),
                base_url,
                api_key,
            );
            provider.apps = parse_universal_apps(&apps)?;

            cc_switch_core::ProviderService::upsert_universal(state, provider)?;
            let saved = cc_switch_core::ProviderService::get_universal(state, &id)?
                .ok_or_else(|| anyhow::anyhow!("Universal provider not found after add: {}", id))?;
            printer.print_value(&saved)?;
        }
        UniversalProviderCommands::Edit {
            id,
            set_name,
            set_apps,
            set_base_url,
            set_api_key,
        } => {
            if set_name.is_none()
                && set_apps.is_none()
                && set_base_url.is_none()
                && set_api_key.is_none()
            {
                anyhow::bail!(
                    "Universal provider edit requires at least one of --set-name, --set-apps, --set-base-url or --set-api-key"
                );
            }

            let mut provider = cc_switch_core::ProviderService::get_universal(state, &id)?
                .ok_or_else(|| anyhow::anyhow!("Universal provider not found: {}", id))?;

            if let Some(name) = set_name {
                provider.name = name;
            }
            if let Some(apps) = set_apps {
                provider.apps = parse_universal_apps(&apps)?;
            }
            if let Some(base_url) = set_base_url {
                provider.base_url = base_url;
            }
            if let Some(api_key) = set_api_key {
                provider.api_key = api_key;
            }

            cc_switch_core::ProviderService::upsert_universal(state, provider)?;
            let saved = cc_switch_core::ProviderService::get_universal(state, &id)?
                .ok_or_else(|| anyhow::anyhow!("Universal provider not found after edit: {}", id))?;
            printer.print_value(&saved)?;
        }
        UniversalProviderCommands::SaveAndSync {
            name,
            id,
            apps,
            base_url,
            api_key,
        } => {
            let id = id.unwrap_or_else(|| provider_id_from_name(&name));
            let mut provider = UniversalProvider::new(
                id.clone(),
                name,
                "openai-compatible".to_string(),
                base_url,
                api_key,
            );
            provider.apps = parse_universal_apps(&apps)?;

            cc_switch_core::ProviderService::upsert_universal(state, provider)?;
            cc_switch_core::ProviderService::sync_universal_to_apps(state, &id)?;
            let saved = cc_switch_core::ProviderService::get_universal(state, &id)?.ok_or_else(
                || anyhow::anyhow!("Universal provider not found after save-and-sync: {}", id),
            )?;
            printer.print_value(&json!({
                "provider": saved,
                "syncedApps": enabled_universal_apps(&saved.apps),
            }))?;
        }
        UniversalProviderCommands::Sync { id } => {
            cc_switch_core::ProviderService::sync_universal_to_apps(state, &id)?;
            let provider = cc_switch_core::ProviderService::get_universal(state, &id)?
                .ok_or_else(|| anyhow::anyhow!("Universal provider not found: {}", id))?;
            printer.print_value(&json!({
                "provider": provider,
                "syncedApps": enabled_universal_apps(&provider.apps),
            }))?;
        }
        UniversalProviderCommands::Delete { id, yes } => {
            if !yes {
                anyhow::bail!(
                    "Universal provider delete is destructive. Re-run with --yes to confirm."
                );
            }

            cc_switch_core::ProviderService::delete_universal(state, &id)?;
            printer.success(format!("✓ Deleted universal provider '{}'", id));
        }
    }
    Ok(())
}

fn enabled_universal_apps(apps: &cc_switch_core::provider::UniversalProviderApps) -> Vec<String> {
    let mut result = Vec::new();
    if apps.claude {
        result.push("claude".to_string());
    }
    if apps.codex {
        result.push("codex".to_string());
    }
    if apps.gemini {
        result.push("gemini".to_string());
    }
    result
}

fn build_provider(app_type: &AppType, name: &str, base_url: &str, api_key: &str) -> Provider {
    Provider {
        id: provider_id_from_name(name),
        name: name.to_string(),
        settings_config: build_provider_settings(app_type, base_url, api_key),
        website_url: None,
        category: None,
        created_at: Some(chrono::Utc::now().timestamp_millis()),
        sort_index: None,
        notes: None,
        meta: None,
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

fn load_provider_from_file(
    path: &str,
    app_type: &AppType,
    name_override: Option<&str>,
    base_url_override: Option<&str>,
    api_key_override: Option<&str>,
) -> anyhow::Result<Provider> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read file: {}", path))?;
    let value: Value =
        serde_json::from_str(&content).with_context(|| format!("Invalid JSON file: {}", path))?;

    let mut provider = match serde_json::from_value::<Provider>(value.clone()) {
        Ok(provider) => provider,
        Err(_) => {
            let name = name_override
                .map(ToOwned::to_owned)
                .or_else(|| file_stem(path))
                .unwrap_or_else(|| format!("imported-{}", chrono::Utc::now().timestamp()));

            Provider {
                id: provider_id_from_name(&name),
                name,
                settings_config: value,
                website_url: None,
                category: None,
                created_at: Some(chrono::Utc::now().timestamp_millis()),
                sort_index: None,
                notes: None,
                meta: None,
                icon: None,
                icon_color: None,
                in_failover_queue: false,
            }
        }
    };

    if let Some(name) = name_override {
        provider.name = name.to_string();
    }
    if provider.name.trim().is_empty() {
        provider.name = file_stem(path).unwrap_or_else(|| "imported-provider".to_string());
    }
    if provider.id.trim().is_empty() {
        provider.id = provider_id_from_name(&provider.name);
    }
    if let Some(base_url) = base_url_override {
        set_provider_base_url(&mut provider.settings_config, app_type, base_url)?;
    }
    if let Some(api_key) = api_key_override {
        set_provider_api_key(&mut provider.settings_config, app_type, api_key)?;
    }

    Ok(provider)
}

fn build_provider_settings(app_type: &AppType, base_url: &str, api_key: &str) -> Value {
    match app_type {
        AppType::Claude => json!({
            "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": api_key,
            }
        }),
        AppType::Codex => json!({
            "auth": {
                "OPENAI_API_KEY": api_key
            },
            "config": format!("base_url = \"{}\"\n", base_url)
        }),
        AppType::Gemini => json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": base_url,
                "GEMINI_API_KEY": api_key,
            },
            "config": {}
        }),
        AppType::OpenCode => json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {
                "baseURL": base_url,
                "apiKey": api_key,
            },
            "models": {}
        }),
        AppType::OpenClaw => json!({
            "baseUrl": base_url,
            "apiKey": api_key,
            "api": "openai-completions",
            "models": []
        }),
    }
}

fn set_provider_base_url(
    settings_config: &mut Value,
    app_type: &AppType,
    base_url: &str,
) -> anyhow::Result<()> {
    match app_type {
        AppType::Claude => {
            ensure_object_field(settings_config, "env")?
                .insert("ANTHROPIC_BASE_URL".to_string(), json!(base_url));
        }
        AppType::Codex => {
            let root = ensure_root_object(settings_config)?;
            let existing = root
                .get("config")
                .and_then(Value::as_str)
                .unwrap_or_default();
            root.insert(
                "config".to_string(),
                json!(upsert_codex_base_url(existing, base_url)),
            );
        }
        AppType::Gemini => {
            ensure_object_field(settings_config, "env")?
                .insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(base_url));
            ensure_root_object(settings_config)?
                .entry("config".to_string())
                .or_insert_with(|| json!({}));
        }
        AppType::OpenCode => {
            let root = ensure_root_object(settings_config)?;
            root.entry("npm".to_string())
                .or_insert_with(|| json!("@ai-sdk/openai-compatible"));
            root.entry("models".to_string())
                .or_insert_with(|| json!({}));
            ensure_object_field(settings_config, "options")?
                .insert("baseURL".to_string(), json!(base_url));
        }
        AppType::OpenClaw => {
            let root = ensure_root_object(settings_config)?;
            root.insert("baseUrl".to_string(), json!(base_url));
            root.entry("api".to_string())
                .or_insert_with(|| json!("openai-completions"));
            root.entry("models".to_string())
                .or_insert_with(|| json!([]));
        }
    }
    Ok(())
}

fn set_provider_api_key(
    settings_config: &mut Value,
    app_type: &AppType,
    api_key: &str,
) -> anyhow::Result<()> {
    match app_type {
        AppType::Claude => {
            ensure_object_field(settings_config, "env")?
                .insert("ANTHROPIC_AUTH_TOKEN".to_string(), json!(api_key));
        }
        AppType::Codex => {
            ensure_object_field(settings_config, "auth")?
                .insert("OPENAI_API_KEY".to_string(), json!(api_key));
        }
        AppType::Gemini => {
            ensure_object_field(settings_config, "env")?
                .insert("GEMINI_API_KEY".to_string(), json!(api_key));
            ensure_root_object(settings_config)?
                .entry("config".to_string())
                .or_insert_with(|| json!({}));
        }
        AppType::OpenCode => {
            let root = ensure_root_object(settings_config)?;
            root.entry("npm".to_string())
                .or_insert_with(|| json!("@ai-sdk/openai-compatible"));
            root.entry("models".to_string())
                .or_insert_with(|| json!({}));
            ensure_object_field(settings_config, "options")?
                .insert("apiKey".to_string(), json!(api_key));
        }
        AppType::OpenClaw => {
            let root = ensure_root_object(settings_config)?;
            root.insert("apiKey".to_string(), json!(api_key));
            root.entry("api".to_string())
                .or_insert_with(|| json!("openai-completions"));
            root.entry("models".to_string())
                .or_insert_with(|| json!([]));
        }
    }
    Ok(())
}

fn ensure_root_object(value: &mut Value) -> anyhow::Result<&mut serde_json::Map<String, Value>> {
    if !value.is_object() {
        *value = json!({});
    }

    value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Provider settings must be a JSON object"))
}

fn ensure_object_field<'a>(
    value: &'a mut Value,
    key: &str,
) -> anyhow::Result<&'a mut serde_json::Map<String, Value>> {
    let root = ensure_root_object(value)?;
    if !root.get(key).is_some_and(Value::is_object) {
        root.insert(key.to_string(), json!({}));
    }

    root.get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("Provider settings field '{}' must be an object", key))
}

fn upsert_codex_base_url(config_text: &str, base_url: &str) -> String {
    let mut replaced = false;
    let mut lines = Vec::new();

    for line in config_text.lines() {
        if line.trim_start().starts_with("base_url") {
            lines.push(format!("base_url = \"{}\"", base_url));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !replaced {
        lines.push(format!("base_url = \"{}\"", base_url));
    }

    let rendered = lines.join("\n");
    if rendered.ends_with('\n') {
        rendered
    } else {
        format!("{rendered}\n")
    }
}

fn parse_universal_apps(
    apps: &str,
) -> anyhow::Result<cc_switch_core::provider::UniversalProviderApps> {
    let mut result = cc_switch_core::provider::UniversalProviderApps::default();
    let mut has_any = false;

    for app in apps
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        has_any = true;
        match app {
            "claude" => result.claude = true,
            "codex" => result.codex = true,
            "gemini" => result.gemini = true,
            other => {
                anyhow::bail!(
                    "Universal providers currently support only claude,codex,gemini. Unsupported app: {}",
                    other
                );
            }
        }
    }

    if !has_any {
        anyhow::bail!("Universal provider add requires at least one app in --apps");
    }

    Ok(result)
}

fn provider_id_from_name(name: &str) -> String {
    let sanitized = sanitize_provider_name(name)
        .chars()
        .map(|ch| if ch.is_whitespace() { '-' } else { ch })
        .collect::<String>();
    let trimmed = sanitized.trim_matches('-').to_string();
    if trimmed.is_empty() {
        format!("provider-{}", chrono::Utc::now().timestamp())
    } else {
        trimmed
    }
}

fn file_stem(path: &str) -> Option<String> {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn provider_id_from_name_normalizes_whitespace_and_case() {
        assert_eq!(provider_id_from_name("Open Router"), "open-router");
        assert_eq!(provider_id_from_name("  Claude/Proxy  "), "claude-proxy");
    }

    #[test]
    fn upsert_codex_base_url_replaces_existing_line() {
        let original = "model = \"gpt-5\"\nbase_url = \"https://old.example/v1\"\n";
        let updated = upsert_codex_base_url(original, "https://new.example/v1");

        assert!(updated.contains("model = \"gpt-5\""));
        assert!(updated.contains("base_url = \"https://new.example/v1\""));
        assert!(!updated.contains("https://old.example/v1"));
    }

    #[test]
    fn parse_universal_apps_rejects_unsupported_targets() {
        let err = parse_universal_apps("claude,opencode").expect_err("should reject opencode");
        assert!(err.to_string().contains("claude,codex,gemini"));
    }

    #[test]
    fn load_provider_from_file_builds_raw_settings_with_overrides() {
        let temp = tempdir().expect("tempdir");
        let file = temp.path().join("provider.json");
        fs::write(
            &file,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://old.example","ANTHROPIC_AUTH_TOKEN":"sk-old"}}"#,
        )
        .expect("write provider json");

        let provider = load_provider_from_file(
            file.to_str().expect("utf-8 path"),
            &AppType::Claude,
            Some("Imported Router"),
            Some("https://new.example"),
            Some("sk-new"),
        )
        .expect("provider should load");

        assert_eq!(provider.id, "imported-router");
        assert_eq!(provider.name, "Imported Router");
        assert_eq!(
            provider
                .settings_config
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .and_then(Value::as_str),
            Some("https://new.example")
        );
        assert_eq!(
            provider
                .settings_config
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
                .and_then(Value::as_str),
            Some("sk-new")
        );
    }

    #[test]
    fn duplicate_uses_copy_suffix_and_regenerates_id() {
        let source = Provider {
            id: "claude-source".to_string(),
            name: "Claude Source".to_string(),
            settings_config: json!({"env": {}}),
            website_url: None,
            category: None,
            created_at: Some(1),
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        let mut duplicated = source.clone();
        duplicated.name = format!("{} Copy", source.name);
        duplicated.id = provider_id_from_name(&duplicated.name);

        assert_eq!(duplicated.name, "Claude Source Copy");
        assert_eq!(duplicated.id, "claude-source-copy");
        assert_ne!(duplicated.id, source.id);
    }

    #[test]
    fn extract_provider_base_url_reads_codex_toml() {
        let settings = json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test"
            },
            "config": "model = \"gpt-5\"\nbase_url = \"https://codex.example/v1\"\n"
        });

        let base_url = extract_provider_base_url(&settings, &AppType::Codex)
            .expect("codex base url should parse");
        assert_eq!(base_url.as_deref(), Some("https://codex.example/v1"));
    }
}
