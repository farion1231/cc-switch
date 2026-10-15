//! Resolve protocol overrides without changing the stored provider.
use crate::provider::Provider;

pub fn supports_model_api_formats(provider: &Provider) -> bool {
    !matches!(
        provider.category.as_deref(),
        Some("official" | "cloud_provider")
    ) && !provider.is_codex_oauth()
        && !provider.is_github_copilot()
        && !provider.is_xai_oauth()
}

pub fn has_model_api_formats(provider: &Provider) -> bool {
    supports_model_api_formats(provider)
        && provider
            .meta
            .as_ref()
            .and_then(|m| m.model_api_formats.as_ref())
            .is_some_and(|formats| !formats.is_empty())
}

pub fn formats(provider: &Provider) -> Option<&std::collections::HashMap<String, String>> {
    supports_model_api_formats(provider)
        .then(|| provider.meta.as_ref()?.model_api_formats.as_ref())
        .flatten()
}

pub fn format_for_model<'a>(
    formats: Option<&'a std::collections::HashMap<String, String>>,
    model: &str,
) -> Option<&'a str> {
    let model = super::super::model_mapper::strip_one_m_suffix_for_upstream(model.trim());
    let format = formats?.get(model)?;
    matches!(
        format.as_str(),
        "anthropic" | "openai_chat" | "openai_responses"
    )
    .then_some(format.as_str())
}

pub fn model_api_format<'a>(provider: &'a Provider, model: &str) -> Option<&'a str> {
    if !supports_model_api_formats(provider) {
        return None;
    }
    format_for_model(formats(provider), model)
}

/// A request-local view lets every existing adapter use the same selected format,
/// including authentication. Never persist this clone or cache it across requests.
pub fn provider_for_model(provider: &Provider, model: &str) -> Option<Provider> {
    let format = model_api_format(provider, model)?;
    let mut effective = provider.clone();
    effective.meta.as_mut()?.api_format = Some(format.to_string());
    Some(effective)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider() -> Provider {
        serde_json::from_value(json!({
            "id": "mixed", "name": "Mixed", "settingsConfig": {},
            "meta": {"apiFormat": "openai_chat", "modelApiFormats": {
                "claude-model": "anthropic", "gpt-model": "openai_responses", "future": "unknown"
            }}
        }))
        .unwrap()
    }

    #[test]
    fn model_api_formats_match_final_ids_and_preserve_stored_default() {
        let provider = provider();
        assert_eq!(
            model_api_format(&provider, "claude-model[1M]"),
            Some("anthropic")
        );
        assert_eq!(model_api_format(&provider, "Claude-model"), None);
        assert_eq!(model_api_format(&provider, "missing"), None);
        assert_eq!(model_api_format(&provider, "future"), None);
        let effective = provider_for_model(&provider, "gpt-model").unwrap();
        assert_eq!(
            effective.meta.unwrap().api_format.as_deref(),
            Some("openai_responses")
        );
        assert_eq!(
            provider.meta.as_ref().unwrap().api_format.as_deref(),
            Some("openai_chat")
        );
        let saved = serde_json::to_value(&provider).unwrap();
        assert_eq!(saved["meta"]["modelApiFormats"]["future"], "unknown");
        assert_eq!(
            serde_json::from_value::<Provider>(saved)
                .unwrap()
                .meta
                .unwrap()
                .model_api_formats,
            provider.meta.unwrap().model_api_formats
        );
    }

    #[test]
    fn model_api_formats_do_not_override_managed_or_official_protocols() {
        for kind in ["codex_oauth", "xai_oauth", "github_copilot"] {
            let mut provider = provider();
            provider.meta.as_mut().unwrap().provider_type = Some(kind.into());
            assert!(!has_model_api_formats(&provider));
            assert!(provider_for_model(&provider, "claude-model").is_none());
        }
        for category in ["official", "cloud_provider"] {
            let mut provider = provider();
            provider.category = Some(category.into());
            assert!(!has_model_api_formats(&provider));
        }
    }
}
