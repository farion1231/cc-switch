use crate::agent_gateway::models::{
    AgentCommandError, AgentProviderMode, ProviderRuntimeSnapshot, ProviderSnapshotRequest,
};
use crate::database::Database;
use crate::provider::Provider;
use serde_json::Value;
use sha2::{Digest, Sha256};

const CLAUDE_APP_TYPE: &str = "claude";

pub fn resolve_provider_for_snapshot(
    db: &Database,
    req: &ProviderSnapshotRequest,
) -> Result<Provider, AgentCommandError> {
    let mode = req
        .provider_mode
        .clone()
        .unwrap_or(AgentProviderMode::SelectedProvider);
    let provider_id = match mode {
        AgentProviderMode::SelectedProvider => req
            .provider_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                AgentCommandError::new(
                    "PROVIDER_NOT_SELECTED",
                    "No provider was selected for the Agent runtime snapshot.",
                    "Choose a provider or switch to Use Current CC Switch Provider.",
                )
            })?,
        AgentProviderMode::CurrentCcSwitchProvider => db
            .get_current_provider(CLAUDE_APP_TYPE)
            .map_err(snapshot_db_error)?
            .ok_or_else(|| {
                AgentCommandError::new(
                    "PROVIDER_NOT_FOUND",
                    "No current CC Switch Claude provider is enabled.",
                    "Enable a Claude provider or choose a provider explicitly.",
                )
            })?,
    };

    db.get_provider_by_id(&provider_id, CLAUDE_APP_TYPE)
        .map_err(snapshot_db_error)?
        .ok_or_else(|| {
            AgentCommandError::new(
                "PROVIDER_NOT_FOUND",
                "The selected provider does not exist in the CC Switch Provider DB.",
                "Refresh Agent Gateway and choose a provider shown on the Provider page.",
            )
            .with_details(format!(
                "provider_id={provider_id}; app_type={CLAUDE_APP_TYPE}"
            ))
        })
}

pub fn build_provider_runtime_snapshot(provider: &Provider) -> ProviderRuntimeSnapshot {
    let base_url = extract_first_string(
        provider,
        &[
            "ANTHROPIC_BASE_URL",
            "OPENAI_BASE_URL",
            "base_url",
            "baseURL",
            "apiEndpoint",
        ],
    )
    .unwrap_or_default();
    let redacted_settings = redact_settings_config(&provider.settings_config);
    let redacted_settings_config_json =
        serde_json::to_string(&redacted_settings).unwrap_or_else(|_| "{}".to_string());
    let provider_type = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.clone())
        .or_else(|| extract_first_string(provider, &["providerType", "provider_type", "type"]))
        .unwrap_or_else(|| {
            provider
                .category
                .clone()
                .unwrap_or_else(|| "custom".to_string())
        });
    let api_format = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.clone())
        .or_else(|| extract_first_string(provider, &["apiFormat", "api_format", "wire_api"]));
    let upstream_models = provider_upstream_models(provider);
    let default_upstream_model = upstream_models.first().cloned();
    let hash_source = serde_json::json!({
        "provider_id": provider.id,
        "provider_name": provider.name,
        "provider_type": provider_type,
        "base_url": redact_base_url(&base_url),
        "api_format": api_format,
        "upstream_models": upstream_models,
        "redacted_settings": redacted_settings,
    });
    let provider_config_hash = Some(sha256_hex(
        serde_json::to_string(&hash_source)
            .unwrap_or_else(|_| provider.id.clone())
            .as_bytes(),
    ));

    ProviderRuntimeSnapshot {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        provider_type,
        app_type: CLAUDE_APP_TYPE.to_string(),
        base_url: base_url.clone(),
        redacted_base_url: redact_base_url(&base_url),
        auth_token_present: has_provider_api_key(provider),
        api_format,
        upstream_models,
        default_upstream_model,
        redacted_settings_config_json,
        provider_config_hash,
    }
}

pub fn validate_snapshot_launchable(
    snapshot: &ProviderRuntimeSnapshot,
) -> Result<(), AgentCommandError> {
    if snapshot.base_url.trim().is_empty() || !snapshot.auth_token_present {
        return Err(AgentCommandError::new(
            "PROVIDER_CONNECTION_FAILED",
            "The selected provider is missing Base URL or API key configuration.",
            "Open the provider settings, confirm Base URL and API key/token are saved, then retry.",
        )
        .with_details(format!(
            "provider={}; base_url_present={}; api_key_configured={}",
            snapshot.provider_name,
            !snapshot.base_url.trim().is_empty(),
            snapshot.auth_token_present
        )));
    }
    Ok(())
}

pub fn provider_upstream_models(provider: &Provider) -> Vec<String> {
    let mut models = Vec::new();
    for key in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "model",
        "defaultModel",
        "default_model",
    ] {
        if let Some(model) = extract_first_string(provider, &[key]) {
            push_unique(&mut models, model);
        }
    }
    if let Some(models_value) = provider.settings_config.get("models") {
        match models_value {
            Value::Array(items) => {
                for item in items {
                    if let Some(model) = item.as_str() {
                        push_unique(&mut models, model.trim().to_string());
                    }
                }
            }
            Value::Object(map) => {
                for key in map.keys() {
                    push_unique(&mut models, key.trim().to_string());
                }
            }
            _ => {}
        }
    }
    if let Some(meta) = provider.meta.as_ref() {
        for route in meta.claude_desktop_model_routes.values() {
            push_unique(&mut models, route.model.trim().to_string());
        }
    }
    models
}

pub fn has_provider_api_key(provider: &Provider) -> bool {
    [
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "GEMINI_API_KEY",
        "apiKey",
        "api_key",
        "token",
        "accessToken",
    ]
    .into_iter()
    .any(|key| {
        extract_first_string(provider, &[key])
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    })
}

pub fn redact_settings_config(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    if is_secret_key(key) {
                        (key.clone(), Value::String("<redacted>".to_string()))
                    } else {
                        (key.clone(), redact_settings_config(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_settings_config).collect()),
        Value::String(value) if looks_like_secret(value) => Value::String("<redacted>".to_string()),
        _ => value.clone(),
    }
}

pub fn redact_base_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let without_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    if without_query.contains('@') {
        return without_query
            .split_once("://")
            .map(|(scheme, rest)| {
                let host_part = rest.rsplit('@').next().unwrap_or(rest);
                format!("{scheme}://<redacted>@{host_part}")
            })
            .unwrap_or_else(|| "<redacted>".to_string());
    }
    without_query.to_string()
}

fn extract_first_string(provider: &Provider, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = extract_string_from_value(&provider.settings_config, key) {
            return Some(value);
        }
    }
    None
}

fn extract_string_from_value(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(Value::as_str) {
                let trimmed = found.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            for child in map.values() {
                if let Some(found) = extract_string_from_value(child, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| extract_string_from_value(child, key)),
        _ => None,
    }
}

fn push_unique(models: &mut Vec<String>, model: String) {
    if !model.is_empty() && !models.iter().any(|item| item == &model) {
        models.push(model);
    }
}

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("key")
        || lower.contains("token")
        || lower.contains("authorization")
        || lower.contains("cookie")
        || lower.contains("secret")
        || lower.contains("session")
}

fn looks_like_secret(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("sk-")
        || trimmed.starts_with("sk_")
        || trimmed.to_ascii_lowercase().starts_with("bearer ")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn snapshot_db_error(error: crate::AppError) -> AgentCommandError {
    AgentCommandError::new(
        "DB_MIGRATION_FAILED",
        "Agent Gateway database operation failed.",
        "Restart the app. If the issue persists, export diagnostics.",
    )
    .with_details(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    fn provider(settings_config: Value) -> Provider {
        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            settings_config,
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("opencode_go_subscription".to_string()),
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        provider
    }

    #[test]
    fn snapshot_extracts_provider_runtime_fields() {
        let snapshot = build_provider_runtime_snapshot(&provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com/v1?token=leak",
                "ANTHROPIC_AUTH_TOKEN": "sk-secret",
                "ANTHROPIC_MODEL": "deepseek-v4-pro[1M]"
            }
        })));

        assert_eq!(snapshot.provider_name, "DeepSeek");
        assert_eq!(snapshot.provider_type, "opencode_go_subscription");
        assert_eq!(snapshot.api_format.as_deref(), Some("openai_chat"));
        assert_eq!(
            snapshot.default_upstream_model.as_deref(),
            Some("deepseek-v4-pro[1M]")
        );
        assert!(snapshot.auth_token_present);
        assert!(!snapshot.redacted_settings_config_json.contains("sk-secret"));
        assert!(!snapshot.redacted_base_url.contains("token=leak"));
    }

    #[test]
    fn redaction_removes_nested_secret_values() {
        let redacted = redact_settings_config(&json!({
            "headers": { "Authorization": "Bearer abc" },
            "options": { "apiKey": "sk-test", "baseURL": "https://example.com" }
        }));
        let encoded = serde_json::to_string(&redacted).unwrap();
        assert!(!encoded.contains("sk-test"));
        assert!(!encoded.contains("Bearer abc"));
        assert!(encoded.contains("https://example.com"));
    }

    #[test]
    fn snapshot_reports_missing_auth_or_url() {
        let snapshot = build_provider_runtime_snapshot(&provider(json!({
            "env": { "ANTHROPIC_MODEL": "mimo-v2.5-pro" }
        })));
        assert!(validate_snapshot_launchable(&snapshot).is_err());
    }
}
