//! Provider CRUD helpers for CLI mode

use console::{style, Term};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Password, Select};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::app_config::AppType;
use crate::provider::Provider;
use crate::services::ProviderService;
use crate::store::AppState;

pub fn add_provider(
    state: &AppState,
    term: &Term,
    app_type: AppType,
    json_path: Option<&str>,
) -> Result<(), String> {
    let existing = ProviderService::list(state, app_type.clone()).map_err(|e| e.to_string())?;
    let existing_names = existing
        .values()
        .map(|p| p.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    let provider = match json_path {
        Some(path) => provider_from_json(&app_type, path, &existing_names)?,
        None => provider_from_prompts(term, &app_type, &existing_names)?,
    };

    let name = provider.name.clone();
    ProviderService::add(state, app_type.clone(), provider).map_err(|e| e.to_string())?;

    let _ = term.write_line(&format!(
        "\n{} Provider \"{}\" created for {}\n",
        style("✓").green(),
        style(&name).bold(),
        style(app_type.as_str()).bold()
    ));
    Ok(())
}

pub fn edit_provider(
    state: &AppState,
    term: &Term,
    app_type: AppType,
    provider_query: &str,
) -> Result<(), String> {
    let providers = ProviderService::list(state, app_type.clone()).map_err(|e| e.to_string())?;
    let (provider_id, provider) = find_provider(&providers, provider_query)?;

    let mut provider = provider.clone();
    let existing_names = providers
        .values()
        .filter(|p| p.id != provider.id)
        .map(|p| p.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    print_provider_summary(term, &app_type, &provider, Some(provider_id))?;

    let theme = ColorfulTheme::default();

    // Name
    let new_name: String = Input::with_theme(&theme)
        .with_prompt("Edit provider name")
        .default(provider.name.clone())
        .interact_on(term)
        .map_err(|e| format!("Input error: {e}"))?;

    if new_name.trim().is_empty() {
        return Err("Provider name cannot be empty".to_string());
    }
    if existing_names.contains(&new_name.to_ascii_lowercase()) {
        return Err(format!(
            "Provider name '{}' already exists for {}",
            new_name,
            app_type.as_str()
        ));
    }
    provider.name = new_name.trim().to_string();

    match app_type {
        AppType::Claude => edit_claude_settings(term, &theme, &mut provider)?,
        AppType::Codex => edit_codex_settings(term, &theme, &mut provider)?,
        AppType::Gemini => edit_gemini_settings(term, &theme, &mut provider)?,
    }

    ProviderService::update(state, app_type.clone(), provider).map_err(|e| e.to_string())?;

    let _ = term.write_line(&format!(
        "\n{} Provider updated successfully\n",
        style("✓").green()
    ));
    Ok(())
}

pub fn delete_provider(
    state: &AppState,
    term: &Term,
    app_type: AppType,
    provider_query: &str,
    force: bool,
) -> Result<(), String> {
    let providers = ProviderService::list(state, app_type.clone()).map_err(|e| e.to_string())?;
    let (provider_id, provider) = find_provider(&providers, provider_query)?;

    if !force {
        let theme = ColorfulTheme::default();
        let confirm = Confirm::with_theme(&theme)
            .with_prompt(format!(
                "Delete provider \"{}\" for {}?",
                provider.name,
                app_type.as_str()
            ))
            .default(false)
            .interact_on(term)
            .map_err(|e| format!("Confirmation error: {e}"))?;

        if !confirm {
            let _ = term.write_line(&format!("\n{}\n", style("Cancelled").dim()));
            return Ok(());
        }
    }

    ProviderService::delete(state, app_type.clone(), provider_id).map_err(|e| e.to_string())?;

    let _ = term.write_line(&format!(
        "\n{} Provider \"{}\" deleted\n",
        style("✓").green(),
        style(&provider.name).bold()
    ));
    Ok(())
}

pub fn show_provider(
    state: &AppState,
    term: &Term,
    app_type: AppType,
    provider_query: &str,
    as_json: bool,
) -> Result<(), String> {
    let providers = ProviderService::list(state, app_type.clone()).map_err(|e| e.to_string())?;
    let (provider_id, provider) = find_provider(&providers, provider_query)?;
    let current_id = ProviderService::current(state, app_type.clone()).ok();
    let is_active = current_id.as_deref() == Some(provider_id);

    let redacted_settings = redact_settings_config(&provider.settings_config);

    if as_json {
        let output = json!({
            "id": provider.id,
            "name": provider.name,
            "tool": app_type.as_str(),
            "active": is_active,
            "websiteUrl": provider.website_url,
            "category": provider.category,
            "createdAt": provider.created_at,
            "sortIndex": provider.sort_index,
            "notes": provider.notes,
            "icon": provider.icon,
            "iconColor": provider.icon_color,
            "inFailoverQueue": provider.in_failover_queue,
            "settingsConfig": redacted_settings,
        });

        let text = serde_json::to_string_pretty(&output)
            .map_err(|e| format!("Failed to serialize JSON output: {e}"))?;
        let _ = term.write_line(&text);
        return Ok(());
    }

    let _ = term.write_line(&format!(
        "\n{}",
        style("Provider Details").bold().underlined()
    ));
    let _ = term.write_line("");

    let _ = term.write_line(&format!(
        "  {:10} {}",
        style("Name:").bold(),
        style(&provider.name).bold()
    ));
    let _ = term.write_line(&format!(
        "  {:10} {}",
        style("Tool:").bold(),
        app_type.as_str()
    ));
    let _ = term.write_line(&format!(
        "  {:10} {}",
        style("Status:").bold(),
        if is_active {
            style("Active").green().to_string()
        } else {
            style("Inactive").dim().to_string()
        }
    ));
    let _ = term.write_line(&format!(
        "  {:10} {}",
        style("ID:").bold(),
        style(provider_id).dim()
    ));

    if let Some(url) = extract_base_url(&provider.settings_config) {
        let _ = term.write_line(&format!("  {:10} {}", style("Base URL:").bold(), url));
    }
    if let Some(model) = extract_model(&app_type, &provider.settings_config) {
        let _ = term.write_line(&format!("  {:10} {}", style("Model:").bold(), model));
    }

    if let Some(site) = provider.website_url.as_deref().filter(|s| !s.is_empty()) {
        let _ = term.write_line(&format!("  {:10} {}", style("Website:").bold(), site));
    }
    if let Some(notes) = provider.notes.as_deref().filter(|s| !s.is_empty()) {
        let _ = term.write_line(&format!("  {:10} {}", style("Notes:").bold(), notes));
    }

    let _ = term.write_line("");
    let _ = term.write_line(&format!("{}", style("Settings:").bold().underlined()));
    let pretty = serde_json::to_string_pretty(&redacted_settings)
        .map_err(|e| format!("Failed to format settings JSON: {e}"))?;
    let _ = term.write_line(&pretty);
    let _ = term.write_line("");

    Ok(())
}

fn provider_from_prompts(
    term: &Term,
    app_type: &AppType,
    existing_names: &HashSet<String>,
) -> Result<Provider, String> {
    let theme = ColorfulTheme::default();

    let template = select_template(term, &theme, app_type)?;

    if template == Template::ImportJson {
        let path: String = Input::with_theme(&theme)
            .with_prompt("Path to JSON file ('-' for stdin)")
            .interact_on(term)
            .map_err(|e| format!("Input error: {e}"))?;
        return provider_from_json(app_type, path.trim(), existing_names);
    }

    let name = prompt_unique_name(term, &theme, existing_names)?;

    let (default_base_url, default_model) = template_defaults(app_type, template);

    let api_key = prompt_required_secret(term, &theme, "Enter API key")?;

    let base_url: String = Input::with_theme(&theme)
        .with_prompt("Enter base URL")
        .default(default_base_url.to_string())
        .interact_on(term)
        .map_err(|e| format!("Input error: {e}"))?;

    if base_url.trim().is_empty() {
        return Err("Base URL cannot be empty".to_string());
    }

    let model = prompt_model(term, &theme, app_type, default_model)?;

    let settings_config = match app_type {
        AppType::Claude => build_claude_settings(&api_key, base_url.trim(), model.as_deref()),
        AppType::Codex => build_codex_settings(&name, &api_key, base_url.trim(), model.as_deref()),
        AppType::Gemini => build_gemini_settings(&api_key, base_url.trim(), model.as_deref()),
    };

    Ok(Provider {
        id: generate_provider_id(&name),
        name,
        settings_config,
        website_url: None,
        category: Some("custom".to_string()),
        created_at: Some(now_millis()),
        sort_index: None,
        notes: None,
        meta: None,
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    })
}

fn provider_from_json(
    app_type: &AppType,
    json_path: &str,
    existing_names: &HashSet<String>,
) -> Result<Provider, String> {
    let content = if json_path == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("Failed to read stdin: {e}"))?;
        buf
    } else {
        fs::read_to_string(json_path).map_err(|e| format!("Failed to read {json_path}: {e}"))?
    };

    let parsed: Value = serde_json::from_str(&content).map_err(|e| format!("Invalid JSON: {e}"))?;

    let (name, mut settings_config) = extract_name_and_settings(&parsed, json_path)?;

    if name.trim().is_empty() {
        return Err("Provider name cannot be empty".to_string());
    }
    if existing_names.contains(&name.to_ascii_lowercase()) {
        return Err(format!(
            "Provider name '{}' already exists for {}",
            name,
            app_type.as_str()
        ));
    }
    validate_imported_settings(app_type, &settings_config)?;

    // For Codex: convert new JSON format to TOML if needed
    if *app_type == AppType::Codex {
        settings_config = convert_codex_json_to_toml_if_needed(&name, settings_config)?;
    }

    Ok(Provider {
        id: generate_provider_id(&name),
        name: name.trim().to_string(),
        settings_config,
        website_url: None,
        category: Some("custom".to_string()),
        created_at: Some(now_millis()),
        sort_index: None,
        notes: None,
        meta: None,
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    })
}

fn validate_imported_settings(app_type: &AppType, settings_config: &Value) -> Result<(), String> {
    let hint = "Tip: run `cc-switch cmd help` to see JSON import examples.";

    match app_type {
        AppType::Claude => {
            let Some(obj) = settings_config.as_object() else {
                return Err(format!(
                    "Invalid Claude JSON import: settingsConfig must be a JSON object. {hint}"
                ));
            };
            let Some(env) = obj.get("env").and_then(|v| v.as_object()) else {
                return Err(format!(
                    "Invalid Claude JSON import: missing required field `env` (object). {hint}"
                ));
            };

            let token = env
                .get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let base_url = env
                .get("ANTHROPIC_BASE_URL")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            let mut missing = Vec::new();
            if token.is_empty() {
                missing.push("env.ANTHROPIC_AUTH_TOKEN (or env.ANTHROPIC_API_KEY)");
            }
            if base_url.is_empty() {
                missing.push("env.ANTHROPIC_BASE_URL");
            }
            if !missing.is_empty() {
                return Err(format!(
                    "Invalid Claude JSON import: missing required fields: {}. {hint}",
                    missing.join(", ")
                ));
            }

            Ok(())
        }
        AppType::Codex => {
            let Some(obj) = settings_config.as_object() else {
                return Err(format!(
                    "Invalid Codex JSON import: settingsConfig must be a JSON object. {hint}"
                ));
            };
            let Some(auth) = obj.get("auth").and_then(|v| v.as_object()) else {
                return Err(format!(
                    "Invalid Codex JSON import: missing required field `auth` (object). {hint}"
                ));
            };
            let api_key = auth
                .get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            if api_key.is_empty() {
                return Err(format!(
                    "Invalid Codex JSON import: missing required field auth.OPENAI_API_KEY. {hint}"
                ));
            }

            // Support three formats:
            // 1. Simple JSON format: { auth, model, baseUrl } - will be converted to basic TOML
            // 2. Full JSON format: { auth, config: { ...json object... } } - will be converted to TOML
            // 3. Legacy TOML format: { auth, config: "toml string" }
            let config_value = obj.get("config");
            let has_config_string = config_value
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            let has_config_object = config_value.map(|v| v.is_object()).unwrap_or(false);
            let has_model_or_base_url = obj.contains_key("model") || obj.contains_key("baseUrl");

            if !has_config_string && !has_config_object && !has_model_or_base_url {
                return Err(format!(
                    "Invalid Codex JSON import: must provide `model`/`baseUrl`, or `config` (JSON object or TOML string). {hint}"
                ));
            }

            // If legacy TOML string format, validate it
            if has_config_string {
                let config = obj.get("config").and_then(|v| v.as_str()).unwrap_or("");
                crate::codex_config::validate_config_toml(config).map_err(|e| e.to_string())?;
            }

            Ok(())
        }
        AppType::Gemini => {
            let Some(obj) = settings_config.as_object() else {
                return Err(format!(
                    "Invalid Gemini JSON import: settingsConfig must be a JSON object. {hint}"
                ));
            };
            let Some(env) = obj.get("env").and_then(|v| v.as_object()) else {
                return Err(format!(
                    "Invalid Gemini JSON import: missing required field `env` (object). {hint}"
                ));
            };
            let api_key = env
                .get("GEMINI_API_KEY")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let base_url = env
                .get("GOOGLE_GEMINI_BASE_URL")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            let mut missing = Vec::new();
            if api_key.is_empty() {
                missing.push("env.GEMINI_API_KEY");
            }
            if base_url.is_empty() {
                missing.push("env.GOOGLE_GEMINI_BASE_URL");
            }
            if !missing.is_empty() {
                return Err(format!(
                    "Invalid Gemini JSON import: missing required fields: {}. {hint}",
                    missing.join(", ")
                ));
            }

            Ok(())
        }
    }
}

fn extract_name_and_settings(parsed: &Value, json_path: &str) -> Result<(String, Value), String> {
    let file_stem = if json_path == "-" {
        "Imported Provider".to_string()
    } else {
        Path::new(json_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty() && *s != "-")
            .unwrap_or("Imported Provider")
            .to_string()
    };

    let Some(obj) = parsed.as_object() else {
        return Ok((file_stem, parsed.clone()));
    };

    // Provider-like shape: { name, settingsConfig }
    let settings_key = if obj.contains_key("settingsConfig") {
        Some("settingsConfig")
    } else if obj.contains_key("settings_config") {
        Some("settings_config")
    } else {
        None
    };

    if let Some(key) = settings_key {
        let settings = obj
            .get(key)
            .cloned()
            .ok_or_else(|| "Missing settingsConfig".to_string())?;
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&file_stem)
            .to_string();
        return Ok((name, settings));
    }

    // Otherwise treat the JSON object as settings_config; allow optional name hint.
    //
    // If the object looks like a settings config (env/auth/config keys), and includes a top-level
    // "name", treat that as the provider name and remove it from the settings payload.
    let looks_like_settings = obj.contains_key("env")
        || obj.contains_key("auth")
        || obj.contains_key("config")
        || obj.contains_key("apiBaseUrl");

    let name_hint = obj
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let name = name_hint.clone().unwrap_or(file_stem);

    if looks_like_settings && name_hint.is_some() {
        let mut settings = parsed.clone();
        if let Some(map) = settings.as_object_mut() {
            map.remove("name");
        }
        return Ok((name, settings));
    }

    Ok((name, parsed.clone()))
}

fn edit_claude_settings(
    term: &Term,
    theme: &ColorfulTheme,
    provider: &mut Provider,
) -> Result<(), String> {
    let env = ensure_env(provider)?;

    let current_base = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.anthropic.com")
        .to_string();

    let base_url: String = Input::with_theme(theme)
        .with_prompt("Edit base URL")
        .default(current_base)
        .interact_on(term)
        .map_err(|e| format!("Input error: {e}"))?;

    if !base_url.trim().is_empty() {
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            Value::String(base_url.trim().trim_end_matches('/').to_string()),
        );
    }

    let current_key = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| env.get("ANTHROPIC_API_KEY"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let api_key = prompt_optional_secret(
        term,
        theme,
        &format!("Edit API key [{}]", redact_secret(current_key)),
    )?;

    if let Some(key) = api_key.as_deref().filter(|s| !s.trim().is_empty()) {
        env.insert(
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            Value::String(key.trim().to_string()),
        );
        env.remove("ANTHROPIC_API_KEY");
    }

    let current_model = env.get("ANTHROPIC_MODEL").and_then(|v| v.as_str());
    let model = prompt_model(term, theme, &AppType::Claude, current_model)?;
    if let Some(model) = model {
        env.insert("ANTHROPIC_MODEL".to_string(), Value::String(model.clone()));
        env.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
            Value::String(model.clone()),
        );
        env.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
            Value::String(model.clone()),
        );
        env.insert(
            "ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(),
            Value::String(model),
        );
    }

    Ok(())
}

fn edit_codex_settings(
    term: &Term,
    theme: &ColorfulTheme,
    provider: &mut Provider,
) -> Result<(), String> {
    let settings_obj = provider
        .settings_config
        .as_object_mut()
        .ok_or_else(|| "Codex settingsConfig must be a JSON object".to_string())?;

    let auth_obj = settings_obj.entry("auth").or_insert_with(|| json!({}));
    let auth_map = auth_obj
        .as_object_mut()
        .ok_or_else(|| "Codex auth must be a JSON object".to_string())?;

    let current_key = auth_map
        .get("OPENAI_API_KEY")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let api_key = prompt_optional_secret(
        term,
        theme,
        &format!("Edit API key [{}]", redact_secret(current_key)),
    )?;
    if let Some(key) = api_key.as_deref().filter(|s| !s.trim().is_empty()) {
        auth_map.insert(
            "OPENAI_API_KEY".to_string(),
            Value::String(key.trim().to_string()),
        );
    }

    let current_config = settings_obj
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let current_base_url = extract_codex_base_url(&current_config)
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let base_url: String = Input::with_theme(theme)
        .with_prompt("Edit base URL")
        .default(current_base_url)
        .interact_on(term)
        .map_err(|e| format!("Input error: {e}"))?;

    let current_model = extract_codex_model(&current_config);
    let model = prompt_model(term, theme, &AppType::Codex, current_model.as_deref())?;

    let updated = if current_config.trim().is_empty() {
        build_codex_config_toml(&provider.name, base_url.trim(), model.as_deref())
    } else {
        update_codex_config_toml(&current_config, base_url.trim(), model.as_deref())?
    };

    settings_obj.insert("config".to_string(), Value::String(updated));
    Ok(())
}

fn edit_gemini_settings(
    term: &Term,
    theme: &ColorfulTheme,
    provider: &mut Provider,
) -> Result<(), String> {
    let env = ensure_env(provider)?;

    let current_base = env
        .get("GOOGLE_GEMINI_BASE_URL")
        .and_then(|v| v.as_str())
        .unwrap_or("https://generativelanguage.googleapis.com")
        .to_string();

    let base_url: String = Input::with_theme(theme)
        .with_prompt("Edit base URL")
        .default(current_base)
        .interact_on(term)
        .map_err(|e| format!("Input error: {e}"))?;

    if !base_url.trim().is_empty() {
        env.insert(
            "GOOGLE_GEMINI_BASE_URL".to_string(),
            Value::String(base_url.trim().trim_end_matches('/').to_string()),
        );
    }

    let current_key = env
        .get("GEMINI_API_KEY")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let api_key = prompt_optional_secret(
        term,
        theme,
        &format!("Edit API key [{}]", redact_secret(current_key)),
    )?;
    if let Some(key) = api_key.as_deref().filter(|s| !s.trim().is_empty()) {
        env.insert(
            "GEMINI_API_KEY".to_string(),
            Value::String(key.trim().to_string()),
        );
    }

    let current_model = env.get("GEMINI_MODEL").and_then(|v| v.as_str());
    let model = prompt_model(term, theme, &AppType::Gemini, current_model)?;
    if let Some(model) = model {
        env.insert("GEMINI_MODEL".to_string(), Value::String(model));
    }

    Ok(())
}

fn ensure_env(provider: &mut Provider) -> Result<&mut serde_json::Map<String, Value>, String> {
    if !provider.settings_config.is_object() {
        provider.settings_config = json!({});
    }
    let obj = provider
        .settings_config
        .as_object_mut()
        .ok_or_else(|| "settingsConfig must be a JSON object".to_string())?;

    let env = obj.entry("env").or_insert_with(|| json!({}));
    env.as_object_mut()
        .ok_or_else(|| "env must be a JSON object".to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Template {
    OpenRouter,
    Official,
    Custom,
    ImportJson,
}

fn select_template(
    term: &Term,
    theme: &ColorfulTheme,
    app_type: &AppType,
) -> Result<Template, String> {
    let (items, templates) = match app_type {
        AppType::Claude => (
            vec![
                "OpenRouter (recommended)",
                "Anthropic Direct",
                "Custom API",
                "Import from JSON",
                "← Cancel",
            ],
            vec![
                Template::OpenRouter,
                Template::Official,
                Template::Custom,
                Template::ImportJson,
            ],
        ),
        AppType::Codex => (
            vec![
                "OpenAI Direct (recommended)",
                "Custom OpenAI-compatible",
                "Import from JSON",
                "← Cancel",
            ],
            vec![Template::Official, Template::Custom, Template::ImportJson],
        ),
        AppType::Gemini => (
            vec![
                "Google AI (recommended)",
                "Custom API",
                "Import from JSON",
                "← Cancel",
            ],
            vec![Template::Official, Template::Custom, Template::ImportJson],
        ),
    };

    let selection = Select::with_theme(theme)
        .with_prompt(format!(
            "Select provider template for {}",
            app_type.as_str()
        ))
        .items(&items)
        .default(0)
        .interact_on_opt(term)
        .map_err(|e| format!("Selection error: {e}"))?;

    let Some(idx) = selection else {
        return Err("Cancelled".to_string());
    };
    if idx == items.len() - 1 {
        return Err("Cancelled".to_string());
    }

    templates
        .get(idx)
        .copied()
        .ok_or_else(|| "Invalid selection".to_string())
}

fn template_defaults(
    app_type: &AppType,
    template: Template,
) -> (&'static str, Option<&'static str>) {
    match (app_type, template) {
        (AppType::Claude, Template::OpenRouter) => ("https://openrouter.ai/api", None),
        (AppType::Claude, Template::Official) => ("https://api.anthropic.com", None),
        (AppType::Claude, Template::Custom) => ("https://api.anthropic.com", None),
        (AppType::Codex, Template::Official) => ("https://api.openai.com/v1", Some("gpt-5-codex")),
        (AppType::Codex, Template::Custom) => ("https://api.openai.com/v1", Some("gpt-5-codex")),
        (AppType::Gemini, Template::Official) => (
            "https://generativelanguage.googleapis.com",
            Some("gemini-1.5-pro"),
        ),
        (AppType::Gemini, Template::Custom) => (
            "https://generativelanguage.googleapis.com",
            Some("gemini-1.5-pro"),
        ),
        _ => ("", None),
    }
}

fn prompt_unique_name(
    term: &Term,
    theme: &ColorfulTheme,
    existing_names: &HashSet<String>,
) -> Result<String, String> {
    loop {
        let name: String = Input::with_theme(theme)
            .with_prompt("Enter provider name")
            .interact_on(term)
            .map_err(|e| format!("Input error: {e}"))?;
        let name = name.trim().to_string();
        if name.is_empty() {
            let _ = term.write_line(&format!("{}", style("Name cannot be empty").red()));
            continue;
        }
        if existing_names.contains(&name.to_ascii_lowercase()) {
            let _ = term.write_line(&format!(
                "{}",
                style("Name already exists. Please choose another.").red()
            ));
            continue;
        }
        return Ok(name);
    }
}

fn prompt_required_secret(
    term: &Term,
    theme: &ColorfulTheme,
    prompt: &str,
) -> Result<String, String> {
    loop {
        let value = Password::with_theme(theme)
            .with_prompt(prompt)
            .interact_on(term)
            .map_err(|e| format!("Input error: {e}"))?;
        if value.trim().is_empty() {
            let _ = term.write_line(&format!("{}", style("Value cannot be empty").red()));
            continue;
        }
        return Ok(value);
    }
}

fn prompt_optional_secret(
    term: &Term,
    theme: &ColorfulTheme,
    prompt: &str,
) -> Result<Option<String>, String> {
    let value = Password::with_theme(theme)
        .with_prompt(prompt)
        .allow_empty_password(true)
        .interact_on(term)
        .map_err(|e| format!("Input error: {e}"))?;
    if value.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn prompt_model(
    term: &Term,
    theme: &ColorfulTheme,
    app_type: &AppType,
    current: Option<&str>,
) -> Result<Option<String>, String> {
    let (mut models, supports_skip) = match app_type {
        AppType::Claude => (
            vec![
                "Skip (keep default)",
                "claude-sonnet-4.5",
                "claude-opus-4.5",
                "claude-haiku-4.5",
                "Custom…",
            ],
            true,
        ),
        AppType::Codex => (
            vec![
                "Skip (keep default)",
                "gpt-5-codex",
                "gpt-4o",
                "gpt-4o-mini",
                "Custom…",
            ],
            true,
        ),
        AppType::Gemini => (
            vec![
                "Skip (keep default)",
                "gemini-1.5-pro",
                "gemini-1.5-flash",
                "gemini-2.0-flash",
                "Custom…",
            ],
            true,
        ),
    };

    // If current model is not in list, show it near the top for convenience.
    if let Some(cur) = current.filter(|s| !s.trim().is_empty()) {
        if !models.iter().any(|m| m == &cur) && supports_skip {
            models.insert(1, cur);
        }
    }

    let selection = Select::with_theme(theme)
        .with_prompt("Select model (optional)")
        .items(&models)
        .default(0)
        .interact_on_opt(term)
        .map_err(|e| format!("Selection error: {e}"))?;

    let Some(idx) = selection else {
        return Ok(None);
    };

    let choice = models
        .get(idx)
        .ok_or_else(|| "Invalid selection".to_string())?;

    if choice.starts_with("Skip") {
        return Ok(None);
    }
    if choice.starts_with("Custom") {
        let val: String = Input::with_theme(theme)
            .with_prompt("Enter model name")
            .interact_on(term)
            .map_err(|e| format!("Input error: {e}"))?;
        let val = val.trim().to_string();
        if val.is_empty() {
            return Ok(None);
        }
        return Ok(Some(val));
    }

    Ok(Some(choice.to_string()))
}

pub(crate) fn build_claude_settings(api_key: &str, base_url: &str, model: Option<&str>) -> Value {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    let mut env = serde_json::Map::new();
    env.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        Value::String(api_key.trim().to_string()),
    );
    env.insert("ANTHROPIC_BASE_URL".to_string(), Value::String(base_url));

    if let Some(model) = model.filter(|s| !s.trim().is_empty()) {
        let m = model.trim().to_string();
        env.insert("ANTHROPIC_MODEL".to_string(), Value::String(m.clone()));
        env.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
            Value::String(m.clone()),
        );
        env.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
            Value::String(m.clone()),
        );
        env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(), Value::String(m));
    }

    json!({ "env": env })
}

pub(crate) fn build_codex_settings(
    name: &str,
    api_key: &str,
    base_url: &str,
    model: Option<&str>,
) -> Value {
    let config_toml = build_codex_config_toml(name, base_url, model);
    json!({
        "auth": { "OPENAI_API_KEY": api_key.trim() },
        "config": config_toml
    })
}

pub(crate) fn build_gemini_settings(api_key: &str, base_url: &str, model: Option<&str>) -> Value {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    let mut env = serde_json::Map::new();
    env.insert(
        "GEMINI_API_KEY".to_string(),
        Value::String(api_key.trim().to_string()),
    );
    env.insert(
        "GOOGLE_GEMINI_BASE_URL".to_string(),
        Value::String(base_url),
    );

    if let Some(model) = model.filter(|s| !s.trim().is_empty()) {
        env.insert(
            "GEMINI_MODEL".to_string(),
            Value::String(model.trim().to_string()),
        );
    }

    json!({ "env": env })
}

/// Convert Codex JSON format to TOML if the new format is used.
/// Supports three formats:
/// 1. Legacy TOML string: { auth, config: "toml string" }
/// 2. Simple JSON: { auth, model, baseUrl } - generates basic TOML
/// 3. Full JSON: { auth, config: { ...json object... } } - converts JSON to TOML
fn convert_codex_json_to_toml_if_needed(name: &str, settings: Value) -> Result<Value, String> {
    let Some(obj) = settings.as_object() else {
        return Ok(settings);
    };

    // Extract auth
    let auth = obj.get("auth").cloned().unwrap_or(json!({}));

    // Check config field
    let config_value = obj.get("config");

    // Case 1: config is a string (legacy TOML format) - return as-is
    if let Some(config_str) = config_value.and_then(|v| v.as_str()) {
        if !config_str.trim().is_empty() {
            return Ok(settings);
        }
    }

    // Case 2: config is a JSON object - convert to TOML
    if let Some(config_obj) = config_value.and_then(|v| v.as_object()) {
        let toml_value: toml::Value = serde_json::from_value(Value::Object(config_obj.clone()))
            .map_err(|e| format!("Failed to parse config JSON: {e}"))?;
        let config_toml = toml::to_string_pretty(&toml_value)
            .map_err(|e| format!("Failed to convert config to TOML: {e}"))?;

        return Ok(json!({
            "auth": auth,
            "config": config_toml
        }));
    }

    // Case 3: Simple format with model/baseUrl - generate basic TOML
    let model = obj
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let base_url = obj
        .get("baseUrl")
        .or_else(|| obj.get("base_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.openai.com/v1")
        .to_string();

    // Build TOML config
    let config_toml = build_codex_config_toml(name, &base_url, model.as_deref());

    Ok(json!({
        "auth": auth,
        "config": config_toml
    }))
}

pub(crate) fn build_codex_config_toml(name: &str, base_url: &str, model: Option<&str>) -> String {
    let key = sanitize_codex_provider_key(name);
    let model = model.unwrap_or("gpt-5-codex");

    let base_url = base_url.trim().trim_end_matches('/').to_string();

    format!(
        r#"model_provider = "{key}"
model = "{model}"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.{key}]
name = "{key}"
base_url = "{base_url}"
wire_api = "responses"
requires_openai_auth = true
"#
    )
}

fn sanitize_codex_provider_key(name: &str) -> String {
    let raw: String = name.chars().filter(|c| !c.is_control()).collect();
    let lower = raw.to_lowercase();
    let mut key: String = lower
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '_' => c,
            _ => '_',
        })
        .collect();

    while key.starts_with('_') {
        key.remove(0);
    }
    while key.ends_with('_') {
        key.pop();
    }

    if key.is_empty() {
        "custom".to_string()
    } else {
        key
    }
}

pub(crate) fn update_codex_config_toml(
    current: &str,
    base_url: &str,
    model: Option<&str>,
) -> Result<String, String> {
    let mut doc = current
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("TOML parse error: {e}"))?;

    if let Some(model) = model.filter(|s| !s.trim().is_empty()) {
        doc["model"] = toml_edit::value(model.trim());
    }

    let base_url = base_url.trim().trim_end_matches('/').to_string();

    let provider_key = doc
        .get("model_provider")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(key) = provider_key {
        if doc.get("model_providers").is_some() {
            doc["model_providers"][&key]["base_url"] = toml_edit::value(base_url);
        } else {
            doc["base_url"] = toml_edit::value(base_url);
        }
    } else if doc.get("base_url").is_some() {
        doc["base_url"] = toml_edit::value(base_url);
    } else {
        // Fallback: set the first model_providers.*.base_url if possible
        if let Some(mp) = doc
            .get_mut("model_providers")
            .and_then(|v| v.as_table_mut())
        {
            if let Some((_, table)) = mp.iter_mut().next() {
                if let Some(t) = table.as_table_mut() {
                    t["base_url"] = toml_edit::value(base_url);
                }
            }
        } else {
            doc["base_url"] = toml_edit::value(base_url);
        }
    }

    Ok(doc.to_string())
}

fn extract_base_url(settings: &Value) -> Option<String> {
    // Common env keys
    if let Some(env) = settings.get("env").and_then(|v| v.as_object()) {
        if let Some(url) = env
            .get("ANTHROPIC_BASE_URL")
            .or_else(|| env.get("GOOGLE_GEMINI_BASE_URL"))
            .and_then(|v| v.as_str())
        {
            return Some(url.trim_end_matches('/').to_string());
        }
    }

    // Codex config TOML
    let config_str = settings.get("config").and_then(|v| v.as_str())?;
    extract_codex_base_url(config_str)
}

fn extract_model(app_type: &AppType, settings: &Value) -> Option<String> {
    match app_type {
        AppType::Claude => settings
            .pointer("/env/ANTHROPIC_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        AppType::Gemini => settings
            .pointer("/env/GEMINI_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        AppType::Codex => {
            let cfg = settings.get("config").and_then(|v| v.as_str())?;
            extract_codex_model(cfg)
        }
    }
}

fn extract_codex_model(config_toml: &str) -> Option<String> {
    let Ok(value) = toml::from_str::<toml::Value>(config_toml) else {
        return None;
    };
    value
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_codex_base_url(config_toml: &str) -> Option<String> {
    let Ok(value) = toml::from_str::<toml::Value>(config_toml) else {
        return None;
    };

    if let Some(url) = value.get("base_url").and_then(|v| v.as_str()) {
        return Some(url.trim_end_matches('/').to_string());
    }

    let provider_key = value.get("model_provider").and_then(|v| v.as_str());
    if let (Some(key), Some(model_providers)) = (provider_key, value.get("model_providers")) {
        if let Some(url) = model_providers
            .get(key)
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str())
        {
            return Some(url.trim_end_matches('/').to_string());
        }
    }

    None
}

fn redact_settings_config(settings: &Value) -> Value {
    let mut v = settings.clone();
    redact_key_path(&mut v, &["env", "ANTHROPIC_AUTH_TOKEN"]);
    redact_key_path(&mut v, &["env", "ANTHROPIC_API_KEY"]);
    redact_key_path(&mut v, &["auth", "OPENAI_API_KEY"]);
    redact_key_path(&mut v, &["env", "OPENAI_API_KEY"]);
    redact_key_path(&mut v, &["env", "GEMINI_API_KEY"]);
    v
}

fn redact_key_path(value: &mut Value, path: &[&str]) {
    if path.is_empty() {
        return;
    }
    let mut cur = value;
    for (idx, key) in path.iter().enumerate() {
        let is_last = idx == path.len() - 1;
        if is_last {
            if let Some(obj) = cur.as_object_mut() {
                if let Some(Value::String(s)) = obj.get_mut(*key) {
                    let redacted = redact_secret(s);
                    *s = redacted;
                }
            }
            return;
        }

        cur = match cur.get_mut(*key) {
            Some(next) => next,
            None => return,
        };
    }
}

fn redact_secret(secret: &str) -> String {
    let s = secret.trim();
    if s.is_empty() {
        return String::new();
    }
    if s.len() <= 8 {
        return "****".to_string();
    }
    let prefix = &s[..4];
    let suffix = &s[s.len() - 4..];
    format!("{prefix}****...{suffix}")
}

pub(crate) fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(crate) fn generate_provider_id(name: &str) -> String {
    let timestamp = now_millis();
    let sanitized = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase();
    if sanitized.is_empty() {
        format!("provider-{timestamp}")
    } else {
        format!("{sanitized}-{timestamp}")
    }
}

fn find_provider<'a>(
    providers: &'a indexmap::IndexMap<String, Provider>,
    query: &str,
) -> Result<(&'a str, &'a Provider), String> {
    // Prefer exact provider ID match first.
    if let Some((_, id, provider)) = providers.get_full(query) {
        return Ok((id.as_str(), provider));
    }

    let q = query.to_ascii_lowercase();
    let matching: Vec<_> = providers
        .iter()
        .filter(|(_, p)| p.name.to_ascii_lowercase().contains(&q))
        .collect();

    match matching.len() {
        0 => {
            let mut msg = format!("Provider '{}' not found.", query);
            if !providers.is_empty() {
                msg.push_str("\n\nAvailable providers (use ID to disambiguate):");
                for (id, p) in providers.iter() {
                    msg.push_str(&format!("\n  • {} (id: {id})", p.name));
                }
            }
            Err(msg)
        }
        1 => Ok((matching[0].0.as_str(), matching[0].1)),
        _ => {
            let exact: Vec<_> = matching
                .iter()
                .filter(|(_, p)| p.name.eq_ignore_ascii_case(query))
                .collect();

            if exact.len() == 1 {
                Ok((exact[0].0.as_str(), exact[0].1))
            } else {
                let mut msg = format!(
                    "Multiple providers match '{}'. Please use a provider ID.",
                    query
                );
                msg.push_str("\nTip: run `cc-switch cmd list <tool> --ids` to see IDs.");
                for (id, p) in matching {
                    msg.push_str(&format!("\n  • {} (id: {id})", p.name));
                }
                Err(msg)
            }
        }
    }
}

fn print_provider_summary(
    term: &Term,
    app_type: &AppType,
    provider: &Provider,
    provider_id: Option<&str>,
) -> Result<(), String> {
    let _ = term.write_line(&format!(
        "\n{}",
        style(format!("Editing {}", app_type.as_str()))
            .bold()
            .underlined()
    ));
    let _ = term.write_line(&format!("  {:10} {}", style("Name:").bold(), provider.name));
    if let Some(id) = provider_id {
        let _ = term.write_line(&format!("  {:10} {}", style("ID:").bold(), style(id).dim()));
    }
    if let Some(url) = extract_base_url(&provider.settings_config) {
        let _ = term.write_line(&format!("  {:10} {}", style("Base URL:").bold(), url));
    }
    if let Some(model) = extract_model(app_type, &provider.settings_config) {
        let _ = term.write_line(&format!("  {:10} {}", style("Model:").bold(), model));
    }
    let _ = term.write_line("");
    Ok(())
}
