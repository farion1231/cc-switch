use crate::opencode_subscription::models::{
    OpenCodeSubscriptionError, SaveOpenCodeSubscriptionProviderRequest,
};
use crate::provider::{Provider, ProviderMeta};
use serde_json::json;
use url::Url;
use uuid::Uuid;

const FORBIDDEN_SECRET_CHARS: [char; 4] = ['\r', '\n', '\0', '`'];

pub fn validate_save_request(
    req: &SaveOpenCodeSubscriptionProviderRequest,
) -> Result<(), OpenCodeSubscriptionError> {
    validate_base_url(&req.base_url)?;
    validate_api_key(&req.api_key)?;
    if let Some(provider_id) = req.provider_id.as_deref() {
        validate_identifier("provider_id", provider_id)?;
    }
    if let Some(model) = req.default_model.as_deref() {
        validate_plain_value("default_model", model)?;
    }
    Ok(())
}

pub fn build_provider(req: &SaveOpenCodeSubscriptionProviderRequest) -> Provider {
    let id = req.provider_id.clone().unwrap_or_else(|| {
        format!(
            "{}-{}",
            req.subscription_kind.provider_type(),
            Uuid::new_v4()
        )
    });
    let name = req
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| req.subscription_kind.display_name().to_string());

    let mut meta = ProviderMeta::default();
    meta.provider_type = Some(req.subscription_kind.provider_type().to_string());
    meta.api_format = Some("openai_chat".to_string());
    meta.api_key_field = Some("ANTHROPIC_AUTH_TOKEN".to_string());
    meta.is_full_url = Some(is_openai_chat_completions_endpoint(&req.base_url));

    let mut env = serde_json::Map::new();
    env.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        json!(req.api_key.clone()),
    );
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        json!(req.base_url.clone()),
    );

    let default_model = req
        .default_model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty());
    if let Some(model) = default_model {
        env.insert("ANTHROPIC_MODEL".to_string(), json!(model));
    }

    Provider {
        id,
        name,
        settings_config: json!({ "env": env }),
        website_url: None,
        category: Some("third_party".to_string()),
        created_at: Some(chrono::Utc::now().timestamp_millis()),
        sort_index: None,
        notes: Some(format!(
            "{} metadata provider. Uses only user-supplied legal API key and endpoint.",
            req.subscription_kind.display_name()
        )),
        meta: Some(meta),
        icon: Some("opencode".to_string()),
        icon_color: Some("#10B981".to_string()),
        in_failover_queue: false,
    }
}

pub(crate) fn is_openai_chat_completions_endpoint(base_url: &str) -> bool {
    let trimmed = base_url.trim_end_matches('/');
    trimmed.to_ascii_lowercase().ends_with("/chat/completions")
}

pub fn validate_base_url(base_url: &str) -> Result<(), OpenCodeSubscriptionError> {
    validate_plain_value("base_url", base_url)?;
    let url = Url::parse(base_url).map_err(|e| {
        OpenCodeSubscriptionError::new(
            "PROVIDER_CONNECTION_FAILED",
            "OpenCode endpoint is not a valid URL.",
            "Enter an https:// endpoint supplied by your OpenCode Go/Zen provider.",
        )
        .with_details(e.to_string())
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(OpenCodeSubscriptionError::new(
            "PROVIDER_CONNECTION_FAILED",
            "OpenCode endpoint must use http or https.",
            "Use the official endpoint documented by your provider.",
        ));
    }
    Ok(())
}

pub fn validate_api_key(api_key: &str) -> Result<(), OpenCodeSubscriptionError> {
    if api_key.trim().is_empty() {
        return Err(OpenCodeSubscriptionError::new(
            "PROVIDER_CONNECTION_FAILED",
            "OpenCode API key is required.",
            "Paste a legal API key from your OpenCode Go/Zen account.",
        ));
    }
    validate_plain_value("api_key", api_key)
}

pub fn validate_identifier(label: &str, value: &str) -> Result<(), OpenCodeSubscriptionError> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(OpenCodeSubscriptionError::new(
            "PROVIDER_CONNECTION_FAILED",
            format!("{label} contains unsupported characters."),
            "Use ASCII letters, numbers, hyphen, or underscore.",
        ));
    }
    Ok(())
}

pub fn validate_plain_value(label: &str, value: &str) -> Result<(), OpenCodeSubscriptionError> {
    if let Some(ch) = value.chars().find(|ch| FORBIDDEN_SECRET_CHARS.contains(ch)) {
        return Err(OpenCodeSubscriptionError::new(
            "PROVIDER_CONNECTION_FAILED",
            format!("{label} contains a forbidden control character."),
            "Remove control characters and retry.",
        )
        .with_details(format!("forbidden character: {ch:?}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opencode_subscription::models::OpenCodeSubscriptionKind;

    #[test]
    fn provider_metadata_marks_opencode_subscription() {
        let req = SaveOpenCodeSubscriptionProviderRequest {
            provider_id: Some("oc-go".to_string()),
            name: None,
            subscription_kind: OpenCodeSubscriptionKind::Go,
            base_url: "https://example.com".to_string(),
            api_key: "test-key".to_string(),
            default_model: Some("auto".to_string()),
        };

        let provider = build_provider(&req);
        assert_eq!(
            provider.meta.as_ref().unwrap().provider_type.as_deref(),
            Some("opencode_go_subscription")
        );
        assert_eq!(provider.meta.as_ref().unwrap().is_full_url, Some(false));
        assert_eq!(provider.category.as_deref(), Some("third_party"));
        assert_eq!(
            provider
                .settings_config
                .pointer("/env/ANTHROPIC_MODEL")
                .and_then(|value| value.as_str()),
            Some("auto")
        );
        assert!(provider
            .settings_config
            .pointer("/env/ANTHROPIC_DEFAULT_SONNET_MODEL")
            .is_none());
    }

    #[test]
    fn go_provider_without_default_model_keeps_models_empty_without_hardcoded_secret() {
        let req = SaveOpenCodeSubscriptionProviderRequest {
            provider_id: Some("oc-go".to_string()),
            name: None,
            subscription_kind: OpenCodeSubscriptionKind::Go,
            base_url: "https://opencode.ai/zen/go/v1".to_string(),
            api_key: "user-supplied-key".to_string(),
            default_model: None,
        };

        let provider = build_provider(&req);
        assert_eq!(
            provider
                .settings_config
                .pointer("/env/ANTHROPIC_MODEL")
                .and_then(|value| value.as_str()),
            None
        );
        assert!(provider
            .settings_config
            .pointer("/env/ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .is_none());
        assert!(provider
            .settings_config
            .pointer("/env/ANTHROPIC_DEFAULT_SONNET_MODEL")
            .is_none());
        assert!(provider
            .settings_config
            .pointer("/env/ANTHROPIC_DEFAULT_OPUS_MODEL")
            .is_none());
        assert!(provider
            .settings_config
            .pointer("/env/ENABLE_TOOL_SEARCH")
            .is_none());
        assert!(provider
            .settings_config
            .pointer("/env/CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS")
            .is_none());
        assert_eq!(
            provider
                .settings_config
                .pointer("/env/ANTHROPIC_AUTH_TOKEN")
                .and_then(|value| value.as_str()),
            Some("user-supplied-key")
        );
        let encoded = serde_json::to_string(&provider.settings_config).unwrap();
        assert!(!encoded.contains("sk-"));
    }

    #[test]
    fn zen_provider_writes_only_user_supplied_default_model() {
        let req = SaveOpenCodeSubscriptionProviderRequest {
            provider_id: Some("oc-zen".to_string()),
            name: None,
            subscription_kind: OpenCodeSubscriptionKind::Zen,
            base_url: "https://opencode.ai/zen/go/v1/chat/completions".to_string(),
            api_key: "user-supplied-key".to_string(),
            default_model: Some("deepseek-v4-pro".to_string()),
        };

        let provider = build_provider(&req);
        assert_eq!(provider.meta.as_ref().unwrap().is_full_url, Some(true));
        let env = provider
            .settings_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should be an object");

        assert_eq!(
            env.get("ANTHROPIC_MODEL").and_then(|value| value.as_str()),
            Some("deepseek-v4-pro")
        );
        assert!(!env.contains_key("ANTHROPIC_DEFAULT_HAIKU_MODEL"));
        assert!(!env.contains_key("ANTHROPIC_DEFAULT_SONNET_MODEL"));
        assert!(!env.contains_key("ANTHROPIC_DEFAULT_OPUS_MODEL"));
    }

    #[test]
    fn rejects_control_characters_in_api_key() {
        let err = validate_api_key("abc\nsecret").expect_err("invalid key");
        assert_eq!(err.code, "PROVIDER_CONNECTION_FAILED");
    }
}
