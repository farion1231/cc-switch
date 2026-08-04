//! Provider-scoped Codex model mapping.
//!
//! The model catalog controls what Codex displays and sends. This module is
//! deliberately separate: it rewrites only the top-level request `model`
//! after a provider has been selected and leaves every other request field
//! untouched.

use crate::provider::Provider;
use serde_json::Value;

/// Rewrite a Codex request model using the selected provider's mapping.
///
/// Returns `(request_model, upstream_model)` when a non-empty mapping matched.
/// Missing, non-string, and unmatched model values are passed through unchanged.
pub fn apply_codex_model_mapping(
    provider: &Provider,
    body: &mut Value,
) -> Option<(String, String)> {
    let request_model = body.get("model")?.as_str()?.to_string();
    let upstream_model = provider
        .meta
        .as_ref()?
        .codex_model_mapping
        .get(&request_model)?
        .trim();

    if upstream_model.is_empty() {
        return None;
    }

    let upstream_model = upstream_model.to_string();
    if request_model != upstream_model {
        log::debug!("[CodexModelMapper] model mapping: {request_model} -> {upstream_model}");
        body["model"] = Value::String(upstream_model.clone());
    }

    Some((request_model, upstream_model))
}

/// Whether `model` is a configured upstream target.
///
/// Chat and Anthropic conversion paths use this to avoid replacing an already
/// mapped model with the provider's single default model.
pub fn is_codex_model_mapping_target(provider: &Provider, model: &str) -> bool {
    provider.meta.as_ref().is_some_and(|meta| {
        meta.codex_model_mapping
            .values()
            .any(|target| !target.trim().is_empty() && target.trim() == model)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;
    use std::collections::HashMap;

    fn provider_with_mapping(entries: &[(&str, &str)]) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Codex".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: Some("codex".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                codex_model_mapping: entries
                    .iter()
                    .map(|(request, upstream)| ((*request).to_string(), (*upstream).to_string()))
                    .collect::<HashMap<_, _>>(),
                ..ProviderMeta::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn maps_the_selected_codex_model() {
        let provider = provider_with_mapping(&[("gpt-5.6-sol", "zy-gpt-5.6-sol")]);
        let mut body = json!({
            "model": "gpt-5.6-sol",
            "stream": true,
            "tools": [{ "type": "function", "name": "shell" }],
            "input": "hello"
        });
        let original = body.clone();

        let mapped = apply_codex_model_mapping(&provider, &mut body);

        assert_eq!(
            mapped,
            Some(("gpt-5.6-sol".to_string(), "zy-gpt-5.6-sol".to_string()))
        );
        assert_eq!(body["model"], "zy-gpt-5.6-sol");
        assert_eq!(body["stream"], original["stream"]);
        assert_eq!(body["tools"], original["tools"]);
        assert_eq!(body["input"], original["input"]);
    }

    #[test]
    fn supports_multiple_independent_mappings() {
        let provider = provider_with_mapping(&[
            ("gpt-5.6-sol", "zy-gpt-5.6-sol"),
            ("gpt-5.6-terra", "zy-gpt-5.6-terra"),
            ("gpt-5.6-luna", "zy-gpt-5.6-luna"),
            ("gpt-5.5", "zy-gpt-5.5"),
        ]);
        let mut body = json!({ "model": "gpt-5.6-terra" });

        apply_codex_model_mapping(&provider, &mut body);

        assert_eq!(body["model"], "zy-gpt-5.6-terra");
        assert!(is_codex_model_mapping_target(&provider, "zy-gpt-5.6-luna"));
    }

    #[test]
    fn leaves_unmatched_models_unchanged() {
        let provider = provider_with_mapping(&[("gpt-5.6-sol", "zy-gpt-5.6-sol")]);
        let mut body = json!({ "model": "gpt-5.6-terra", "stream": false });
        let original = body.clone();

        let mapped = apply_codex_model_mapping(&provider, &mut body);

        assert!(mapped.is_none());
        assert_eq!(body, original);
    }

    #[test]
    fn ignores_missing_or_non_string_models() {
        let provider = provider_with_mapping(&[("gpt-5.6-sol", "zy-gpt-5.6-sol")]);
        let mut missing = json!({ "input": "hello" });
        let mut non_string = json!({ "model": 56, "input": "hello" });

        assert!(apply_codex_model_mapping(&provider, &mut missing).is_none());
        assert!(apply_codex_model_mapping(&provider, &mut non_string).is_none());
        assert_eq!(missing, json!({ "input": "hello" }));
        assert_eq!(non_string, json!({ "model": 56, "input": "hello" }));
    }

    #[test]
    fn ignores_empty_upstream_targets() {
        let provider = provider_with_mapping(&[("gpt-5.6-sol", "   ")]);
        let mut body = json!({ "model": "gpt-5.6-sol" });

        assert!(apply_codex_model_mapping(&provider, &mut body).is_none());
        assert_eq!(body["model"], "gpt-5.6-sol");
        assert!(!is_codex_model_mapping_target(&provider, ""));
    }
}
