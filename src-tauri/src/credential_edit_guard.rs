//! Opt-in protection against accidental credential edits, not an access-control boundary.
//! Compare config declarations only: never resolve an environment variable, file or command.

use crate::{error::AppError, provider::Provider, AppType};
use serde_json::{json, Map, Value};

pub(crate) fn is_protected(provider: &Provider) -> bool {
    provider
        .meta
        .as_ref()
        .is_some_and(|meta| meta.credential_edit_protected == Some(true))
}

pub(crate) fn conflict() -> AppError {
    AppError::localized(
        "provider.credential_edit_protected",
        "凭据及其发送地址受到编辑保护。请先明确解除保护，再手工修改；其他设置可以正常保存。",
        "The credential and its destination are protected from editing. Explicitly remove protection before changing them manually; other settings can still be saved.",
    )
}

fn unsupported() -> AppError {
    AppError::localized(
        "provider.credential_protection_unsupported",
        "编辑保护仅适用于含内联 API Key 的自定义供应商，不适用于官方或托管登录账号。",
        "Editing protection requires a custom provider with an inline API key; official and managed login accounts are not supported.",
    )
}

fn parse_config(settings: &Value) -> Result<Value, AppError> {
    let text = settings
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(unsupported)?;
    let parsed = text.parse::<toml::Value>().map_err(|_| unsupported())?;
    serde_json::to_value(parsed).map_err(|_| unsupported())
}

/// Credential-bearing fields, sources and destinations within a provider's native declaration.
/// Other settings (models, budgets, hooks, MCP, etc.) are not part of the comparison.
fn auth_fields(value: &Value) -> Option<Value> {
    match value {
        Value::Object(object) => {
            let mut result = Map::new();
            for (key, value) in object {
                let protected = matches!(
                    key.as_str(),
                    "apiKey"
                        | "api_key"
                        | "apiKeyHelper"
                        | "api_key_helper"
                        | "api_key_env"
                        | "api_key_env_var"
                        | "env_key"
                        | "envKey"
                        | "experimental_bearer_token"
                        | "bearerToken"
                        | "access_token"
                        | "auth"
                        | "auth_mode"
                        | "authToken"
                        | "auth_token"
                        | "baseURL"
                        | "baseUrl"
                        | "base_url"
                        | "apiEndpoint"
                        | "endpoint"
                        | "headers"
                        | "http_headers"
                        | "env_http_headers"
                        | "requires_openai_auth"
                        | "authCommand"
                        | "auth_command"
                        | "ANTHROPIC_AUTH_TOKEN"
                        | "ANTHROPIC_API_KEY"
                        | "ANTHROPIC_BASE_URL"
                        | "ANTHROPIC_CUSTOM_HEADERS"
                        | "OPENAI_API_KEY"
                        | "OPENROUTER_API_KEY"
                        | "GEMINI_API_KEY"
                        | "GOOGLE_API_KEY"
                        | "GOOGLE_GEMINI_BASE_URL"
                        | "CLAUDE_CODE_USE_BEDROCK"
                        | "CLAUDE_CODE_USE_VERTEX"
                        | "CLAUDE_CODE_USE_FOUNDRY"
                );
                if protected {
                    result.insert(key.clone(), value.clone());
                } else if let Some(nested) = auth_fields(value) {
                    result.insert(key.clone(), nested);
                }
            }
            (!result.is_empty()).then_some(Value::Object(result))
        }
        Value::Array(items) => {
            let selected: Vec<_> = items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    auth_fields(item).map(|fields| json!({"index": index, "fields": fields}))
                })
                .collect();
            (!selected.is_empty()).then_some(Value::Array(selected))
        }
        _ => None,
    }
}

fn native_auth(app: &AppType, settings: &Value) -> Result<Value, AppError> {
    Ok(match app {
        AppType::Claude | AppType::ClaudeDesktop | AppType::Gemini => json!({
            "env": auth_fields(&settings["env"]),
            "apiKeyHelper": settings.get("apiKeyHelper"),
            "auth": settings.get("auth"),
            "selectedType": settings.pointer("/settings/security/auth/selectedType"),
        }),
        AppType::Codex => {
            let config = parse_config(settings)?;
            let selected = config
                .get("model_provider")
                .and_then(Value::as_str)
                .and_then(|id| {
                    config
                        .get("model_providers")
                        .and_then(|providers| providers.get(id))
                });
            json!({"auth": settings.get("auth"), "provider": selected.and_then(auth_fields),
                "base_url": config.get("base_url"), "experimental_bearer_token": config.get("experimental_bearer_token")})
        }
        AppType::GrokBuild => {
            let config = parse_config(settings)?;
            let selected = config
                .pointer("/models/default")
                .and_then(Value::as_str)
                .and_then(|id| config.get("model").and_then(|models| models.get(id)))
                .ok_or_else(unsupported)?;
            auth_fields(selected).unwrap_or(Value::Null)
        }
        AppType::OpenCode | AppType::Mcode => json!({
            "options": auth_fields(&settings["options"]),
            "settings": auth_fields(&settings["settings"]),
            "models": auth_fields(&settings["models"]),
        }),
        AppType::OpenClaw | AppType::Hermes | AppType::Pi => {
            // These are individual native provider nodes, not the global client config.
            auth_fields(settings).unwrap_or(Value::Null)
        }
    })
}

fn inline_key(app: &AppType, settings: &Value) -> Result<Option<String>, AppError> {
    let from_paths = |paths: &[&str]| {
        paths.iter().find_map(|path| {
            settings
                .pointer(path)
                .and_then(Value::as_str)
                .filter(|key| !key.trim().is_empty())
                .map(str::to_string)
        })
    };
    Ok(match app {
        AppType::Claude | AppType::ClaudeDesktop => {
            from_paths(&["/env/ANTHROPIC_AUTH_TOKEN", "/env/ANTHROPIC_API_KEY"])
        }
        AppType::Gemini => from_paths(&["/env/GEMINI_API_KEY", "/env/GOOGLE_API_KEY"]),
        AppType::Codex => crate::codex_config::extract_codex_api_key(
            settings.get("auth"),
            settings.get("config").and_then(Value::as_str),
        ),
        AppType::GrokBuild => settings
            .get("config")
            .and_then(Value::as_str)
            .and_then(crate::grok_config::extract_model_config)
            .and_then(|config| config.api_key),
        AppType::OpenCode | AppType::Mcode => from_paths(&["/options/apiKey", "/settings/apiKey"]),
        AppType::OpenClaw | AppType::Pi => from_paths(&["/apiKey"]),
        AppType::Hermes => from_paths(&["/api_key"]),
    })
}

fn external_source(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            let declared = !value.is_null()
                && value.as_str() != Some("")
                && value.as_object().is_none_or(|object| !object.is_empty());
            (declared
                && matches!(
                    key.as_str(),
                    "apiKeyHelper"
                        | "api_key_helper"
                        | "env_key"
                        | "envKey"
                        | "api_key_env"
                        | "api_key_env_var"
                        | "env_http_headers"
                        | "authCommand"
                        | "auth_command"
                ))
                || external_source(value)
        }),
        Value::Array(items) => items.iter().any(external_source),
        Value::String(value) => {
            value.contains("${") || value.contains("{env:") || value.contains("{file:")
        }
        _ => false,
    }
}

pub(crate) fn validate_supported(app: &AppType, provider: &Provider) -> Result<(), AppError> {
    if !is_protected(provider) {
        return Ok(());
    }
    if provider.category.as_deref() == Some("official") || provider.uses_managed_account_auth() {
        return Err(unsupported());
    }
    let key = inline_key(app, &provider.settings_config)?.ok_or_else(unsupported)?;
    if key.trim().is_empty()
        || key.trim_start().starts_with('!')
        || key.contains("${")
        || key.contains("{env:")
        || key.contains("{file:")
        || key.starts_with('$')
    {
        return Err(unsupported());
    }
    if external_source(&native_auth(app, &provider.settings_config)?) {
        return Err(unsupported());
    }
    Ok(())
}

fn proxy_auth_headers(provider: &Provider) -> std::collections::BTreeMap<String, String> {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.local_proxy_request_overrides.as_ref())
        .map(|overrides| {
            overrides
                .headers
                .iter()
                .filter(|(key, _)| {
                    matches!(
                        key.to_ascii_lowercase().as_str(),
                        "authorization"
                            | "proxy-authorization"
                            | "x-api-key"
                            | "api-key"
                            | "x-goog-api-key"
                    )
                })
                .map(|(key, value)| (key.to_ascii_lowercase(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn validate_update(
    app: &AppType,
    previous: &Provider,
    next: &Provider,
) -> Result<(), AppError> {
    if !is_protected(previous) {
        // An old editor must not re-enable protection (and restore an old key) after removal.
        return if is_protected(next) {
            Err(conflict())
        } else {
            Ok(())
        };
    }
    let meta =
        |provider: &Provider| serde_json::to_value(provider.meta.as_ref()).unwrap_or(Value::Null);
    let before_meta = meta(previous);
    let after_meta = meta(next);
    if !is_protected(next)
        || proxy_auth_headers(previous) != proxy_auth_headers(next)
        || previous.id != next.id
        || before_meta
            .get("apiKeyField")
            .and_then(Value::as_str)
            .unwrap_or("ANTHROPIC_AUTH_TOKEN")
            != after_meta
                .get("apiKeyField")
                .and_then(Value::as_str)
                .unwrap_or("ANTHROPIC_AUTH_TOKEN")
        || ["authBinding", "providerType", "opencodeConfigFormat"]
            .iter()
            .any(|field| before_meta.get(field) != after_meta.get(field))
        || native_auth(app, &previous.settings_config)? != native_auth(app, &next.settings_config)?
    {
        return Err(conflict());
    }
    validate_supported(app, next)
}

#[cfg(test)]
mod tests;
