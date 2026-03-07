//! Provider command handlers

use crate::cli::{ProviderCommands, UniversalProviderCommands};
use crate::handlers::common::parse_app_type;
use crate::output::Printer;
use anyhow::Context;
use cc_switch_core::{
    config::sanitize_provider_name, AppState, AppType, Provider, UniversalProvider,
};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

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
        ProviderCommands::Switch { id, app } => handle_switch(&id, &app, state, printer).await,
        ProviderCommands::Usage { id, app } => handle_usage(&id, &app, state, printer).await,
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

async fn handle_usage(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let result = cc_switch_core::ProviderService::query_usage(state, app_type, id).await?;
    printer.print_value(&result)?;
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
            printer.success(format!("✓ Added universal provider '{}'", id));
        }
        UniversalProviderCommands::Sync { id } => {
            cc_switch_core::ProviderService::sync_universal_to_apps(state, &id)?;
            printer.success(format!("✓ Synced universal provider '{}' to apps", id));
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
}
