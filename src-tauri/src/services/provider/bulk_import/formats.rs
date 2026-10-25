use crate::{
    app_config::AppType,
    deeplink::DeepLinkImportRequest,
    error::AppError,
    provider::{ClaudeDesktopMode, ClaudeDesktopModelRoute, Provider},
};
use serde_json::json;

pub(super) struct Format {
    pub default: &'static str,
    pub allowed: &'static [&'static str],
    pub model_required: bool,
}

pub(super) fn format(app: &AppType) -> Format {
    let all = &[
        "anthropic",
        "openai_chat",
        "openai_responses",
        "gemini_native",
    ] as &[_];
    let three = &["anthropic", "openai_chat", "openai_responses"] as &[_];
    let (default, allowed) = match app {
        AppType::Claude | AppType::ClaudeDesktop => ("anthropic", all),
        AppType::Codex | AppType::GrokBuild => ("openai_responses", three),
        AppType::Gemini | AppType::Agy => ("gemini_native", &["gemini_native"] as &[_]),
        AppType::Mcode => ("anthropic", three),
        AppType::Hermes => ("openai_chat", three),
        AppType::OpenCode | AppType::OpenClaw | AppType::Pi | AppType::Omp | AppType::Kimi => {
            ("openai_chat", all)
        }
    };
    Format {
        default,
        allowed,
        model_required: app.is_additive_mode(),
    }
}

fn api(format: &str) -> &'static str {
    match format {
        "anthropic" => "anthropic-messages",
        "openai_responses" => "openai-responses",
        "gemini_native" => "google-generative-ai",
        _ => "openai-completions",
    }
}

pub(super) fn dynamic_value(app: &AppType, value: &str, key: bool) -> bool {
    if !app.is_additive_mode() {
        return false;
    }
    if value.contains("${") || value.contains("{env:") || value.contains("{file:") {
        return true;
    }
    if matches!(app, AppType::Pi | AppType::Omp) && key && value.starts_with('!') {
        return true;
    }
    if *app == AppType::Pi
        && value.char_indices().any(|(i, c)| {
            c == '$'
                && value[i + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        })
    {
        return true;
    }
    // An ordinary OMP key may also be an environment-variable name. The preview
    // explains that ambiguity; its spelling alone does not make the key invalid.
    false
}

pub(super) fn build(
    app: &AppType,
    request: &DeepLinkImportRequest,
    protocol: &str,
    context: Option<u64>,
) -> Result<Provider, AppError> {
    let id = format!("bulk-{}", uuid::Uuid::new_v4());
    let model = request.model.as_deref().unwrap_or_default();
    let endpoint = request.endpoint.as_deref().unwrap_or_default();
    let key = request.api_key.as_deref().unwrap_or_default();
    let name = request.name.as_deref().unwrap_or_default();
    let custom = match app {
        AppType::Mcode => Some(
            json!({"kind":"custom", "enabled":true, "api":api(protocol), "options":{"baseURL":endpoint, "apiKey":key}, "models":{model:{"name":model}}}),
        ),
        AppType::Omp => Some(
            json!({"name":name,"baseUrl":endpoint,"apiKey":key,"api":api(protocol),"models":[{"id":model,"name":model}]}),
        ),
        AppType::Kimi => {
            let kind = match protocol {
                "anthropic" => "anthropic",
                "openai_responses" => "openai_responses",
                "gemini_native" => "google-genai",
                _ => "openai",
            };
            Some(
                json!({"provider":{"type":kind,"base_url":endpoint,"api_key":key},"models":{format!("{id}/{model}"):{"provider":id,"model":model,"max_context_size":context}}}),
            )
        }
        _ => None,
    };
    let mut provider = if let Some(config) = custom {
        Provider::with_id(id.clone(), name.into(), config, None)
    } else if *app == AppType::ClaudeDesktop {
        // Desktop stores selectable models in its route map, not ANTHROPIC_MODEL.
        let mut provider = crate::deeplink::build_provider_from_request(&AppType::Claude, request)?;
        provider.settings_config["env"]
            .as_object_mut()
            .unwrap()
            .remove("ANTHROPIC_MODEL");
        let routed = protocol != "anthropic"
            || (!model.is_empty() && !crate::claude_desktop_config::is_claude_safe_model_id(model));
        let meta = provider.meta.get_or_insert_with(Default::default);
        meta.claude_desktop_mode = Some(if routed {
            ClaudeDesktopMode::Proxy
        } else {
            ClaudeDesktopMode::Direct
        });
        if !model.is_empty() {
            let route = if routed {
                crate::claude_desktop_config::DEFAULT_PROXY_ROUTES[0].route_id
            } else {
                model
            };
            meta.claude_desktop_model_routes.insert(
                route.into(),
                ClaudeDesktopModelRoute {
                    model: model.into(),
                    label_override: routed.then(|| model.into()),
                    supports_1m: None,
                },
            );
        }
        provider
    } else {
        crate::deeplink::build_provider_from_request(app, request)?
    };
    provider.id = id;
    provider.category = Some("custom".into());
    if app.is_additive_mode() {
        provider
            .meta
            .get_or_insert_with(Default::default)
            .live_config_managed = Some(false);
    }
    super::ProviderService::prepare_new_provider(app, &mut provider)?;
    Ok(provider)
}
