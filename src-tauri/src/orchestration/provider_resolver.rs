use crate::provider::Provider;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Capability tags inferred from a provider's identifier, name, and settings.
/// Used by [`ProviderModelResolver`] to filter providers that can serve a role.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    Text,
    Vision,
    AudioInput,
    AudioOutput,
    Json,
}

/// Fully-resolved target for a logical orchestration role.
///
/// Produced by [`ProviderModelResolver::resolve_role`] by walking a configured
/// provider map and selecting the first provider whose inferred capabilities
/// satisfy `required` and whose identity matches the requested role.
#[derive(Debug, Clone)]
pub struct ResolvedModelCallTarget {
    pub role: String,
    pub provider_id: String,
    pub provider_name: String,
    pub provider_type: String,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub capabilities: HashSet<ModelCapability>,
}

/// Resolves logical orchestration role names (e.g. `frontier`, `cheap_coder`)
/// to concrete [`ResolvedModelCallTarget`]s against a configured provider map.
///
/// The resolver is intentionally fuzzy: role identity is matched via substring
/// heuristics over the provider id/name/category/provider_type, and capability
/// inference is similarly substring-based. This keeps the resolver tolerant of
/// new providers without requiring an explicit registry.
pub struct ProviderModelResolver;

impl ProviderModelResolver {
    /// Resolve a logical orchestration role to a concrete provider target.
    ///
    /// Walks the provider map in `(sort_index, id)` order, skipping any provider
    /// currently marked `in_failover_queue`. The first provider whose inferred
    /// capabilities satisfy all of `required` AND whose identity matches `role`
    /// is returned as a [`ResolvedModelCallTarget`].
    ///
    /// Returns an error mentioning `capability` if no provider satisfies both
    /// the capability filter and the role match.
    pub fn resolve_role(
        role: &str,
        providers: &indexmap::IndexMap<String, Provider>,
        required: &[ModelCapability],
    ) -> Result<ResolvedModelCallTarget, String> {
        let role_lower = role.to_ascii_lowercase();
        let mut candidates: Vec<&Provider> = providers
            .values()
            .filter(|provider| !provider.in_failover_queue)
            .collect();

        candidates.sort_by(|a, b| {
            a.sort_index
                .unwrap_or(usize::MAX)
                .cmp(&b.sort_index.unwrap_or(usize::MAX))
                .then_with(|| a.id.cmp(&b.id))
        });

        for provider in candidates {
            let capabilities = infer_capabilities(provider);
            if !required.iter().all(|cap| capabilities.contains(cap)) {
                continue;
            }

            if !role_matches_provider(&role_lower, provider) {
                continue;
            }

            return build_target(role, provider, capabilities);
        }

        Err(format!(
            "no provider resolved for role '{role}' satisfying required capability set {:?}",
            required
        ))
    }
}

/// Fuzzy role identity match over the provider's id/name/category/provider_type.
///
/// Known roles use curated keyword sets; unknown roles fall back to a plain
/// substring match of the role name against the same haystack.
fn role_matches_provider(role: &str, provider: &Provider) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        provider.id,
        provider.name,
        provider.category.clone().unwrap_or_default(),
        provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.clone())
            .unwrap_or_default()
    )
    .to_ascii_lowercase();

    match role {
        "frontier" | "frontier_judge" | "frontier_single" => {
            haystack.contains("claude")
                || haystack.contains("openai")
                || haystack.contains("gpt")
                || haystack.contains("sonnet")
        }
        "cheap_coder" | "cheap_reasoner" => {
            haystack.contains("deepseek")
                || haystack.contains("qwen")
                || haystack.contains("glm")
                || haystack.contains("mini")
                || haystack.contains("flash")
        }
        "qwen_coder" => haystack.contains("qwen"),
        "glm_coder" => haystack.contains("glm"),
        "vision_extractor" => haystack.contains("vision") || haystack.contains("gemini"),
        _ => haystack.contains(role),
    }
}

/// Infer capability tags from a provider's id/name/settings via substring match.
///
/// `Text` is always inferred — any provider in the map is assumed able to emit
/// plain text. Other capabilities require explicit keyword evidence.
fn infer_capabilities(provider: &Provider) -> HashSet<ModelCapability> {
    let mut caps = HashSet::new();
    caps.insert(ModelCapability::Text);

    let combined = format!(
        "{} {} {}",
        provider.id,
        provider.name,
        provider.settings_config
    )
    .to_ascii_lowercase();

    if combined.contains("json") || combined.contains("claude") || combined.contains("gpt") {
        caps.insert(ModelCapability::Json);
    }
    if combined.contains("vision")
        || combined.contains("gemini")
        || combined.contains("gpt-4o")
        || combined.contains("image")
    {
        caps.insert(ModelCapability::Vision);
    }
    if combined.contains("audio") || combined.contains("transcrib") || combined.contains("whisper")
    {
        caps.insert(ModelCapability::AudioInput);
    }
    if combined.contains("tts") || combined.contains("speech") {
        caps.insert(ModelCapability::AudioOutput);
    }

    caps
}

/// Build a [`ResolvedModelCallTarget`] from a matched provider, failing with a
/// descriptive error if model/base_url/api_key cannot be extracted.
fn build_target(
    role: &str,
    provider: &Provider,
    capabilities: HashSet<ModelCapability>,
) -> Result<ResolvedModelCallTarget, String> {
    let provider_type = provider
        .meta
        .as_ref()
        .and_then(|m| m.provider_type.clone())
        .unwrap_or_else(|| "openai_chat".to_string());
    let model = extract_model(provider).ok_or_else(|| {
        format!(
            "provider '{}' matched role '{}' but no model field was found",
            provider.id, role
        )
    })?;
    let base_url = extract_base_url(provider).ok_or_else(|| {
        format!(
            "provider '{}' matched role '{}' but no base URL was found",
            provider.id, role
        )
    })?;
    let api_key = extract_api_key(provider).ok_or_else(|| {
        format!(
            "provider '{}' matched role '{}' but no API key was found",
            provider.id, role
        )
    })?;

    Ok(ResolvedModelCallTarget {
        role: role.to_string(),
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        provider_type,
        model,
        base_url,
        api_key,
        capabilities,
    })
}

/// Extract the model name from common env/settings keys, in priority order.
fn extract_model(provider: &Provider) -> Option<String> {
    let env = provider.settings_config.get("env");
    env.and_then(|v| v.get("ANTHROPIC_MODEL"))
        .or_else(|| env.and_then(|v| v.get("OPENAI_MODEL")))
        .or_else(|| env.and_then(|v| v.get("MODEL")))
        .or_else(|| provider.settings_config.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extract the base URL from common env/settings keys, falling back to the
/// provider's `website_url`. Trailing slashes are trimmed for callers that
/// concatenate paths.
fn extract_base_url(provider: &Provider) -> Option<String> {
    let env = provider.settings_config.get("env");
    env.and_then(|v| v.get("ANTHROPIC_BASE_URL"))
        .or_else(|| env.and_then(|v| v.get("OPENAI_BASE_URL")))
        .or_else(|| env.and_then(|v| v.get("BASE_URL")))
        .or_else(|| provider.settings_config.get("baseUrl"))
        .or_else(|| provider.settings_config.get("base_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches('/').to_string())
        .or_else(|| provider.website_url.clone())
}

/// Extract the API key from common env/settings keys, in priority order.
fn extract_api_key(provider: &Provider) -> Option<String> {
    let env = provider.settings_config.get("env");
    env.and_then(|v| v.get("ANTHROPIC_API_KEY"))
        .or_else(|| env.and_then(|v| v.get("ANTHROPIC_AUTH_TOKEN")))
        .or_else(|| env.and_then(|v| v.get("OPENAI_API_KEY")))
        .or_else(|| env.and_then(|v| v.get("API_KEY")))
        .or_else(|| provider.settings_config.pointer("/auth/OPENAI_API_KEY"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};
    use indexmap::IndexMap;
    use serde_json::json;

    fn provider(
        id: &str,
        provider_type: &str,
        model: &str,
        base_url: &str,
        api_key: &str,
    ) -> Provider {
        let mut p = Provider::with_id(
            id.to_string(),
            format!("Provider {id}"),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": api_key,
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_MODEL": model
                }
            }),
            Some(base_url.to_string()),
        );
        p.meta = Some(ProviderMeta {
            provider_type: Some(provider_type.to_string()),
            ..ProviderMeta::default()
        });
        p
    }

    #[test]
    fn resolves_text_role_from_provider_map() {
        let mut providers = IndexMap::new();
        providers.insert(
            "claude-primary".to_string(),
            provider(
                "claude-primary",
                "anthropic",
                "claude-sonnet-4-20250514",
                "https://api.anthropic.com",
                "sk-test",
            ),
        );

        let target = ProviderModelResolver::resolve_role(
            "frontier",
            &providers,
            &[ModelCapability::Text, ModelCapability::Json],
        )
        .unwrap();

        assert_eq!(target.role, "frontier");
        assert_eq!(target.provider_id, "claude-primary");
        assert_eq!(target.provider_type, "anthropic");
        assert_eq!(target.model, "claude-sonnet-4-20250514");
        assert_eq!(target.base_url, "https://api.anthropic.com");
        assert_eq!(target.api_key, "sk-test");
        assert!(target.capabilities.contains(&ModelCapability::Text));
    }

    #[test]
    fn rejects_missing_capability() {
        let mut providers = IndexMap::new();
        providers.insert(
            "text-only".to_string(),
            provider(
                "text-only",
                "openai_chat",
                "gpt-5-mini",
                "https://example.com/v1",
                "sk-test",
            ),
        );

        let err = ProviderModelResolver::resolve_role(
            "vision_extractor",
            &providers,
            &[ModelCapability::Vision],
        )
        .unwrap_err();

        assert!(err.contains("capability"));
    }
}
