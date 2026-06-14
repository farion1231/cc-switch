use crate::orchestration::config::validate_base_url;
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
#[derive(Clone)]
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

impl std::fmt::Debug for ResolvedModelCallTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedModelCallTarget")
            .field("role", &self.role)
            .field("provider_id", &self.provider_id)
            .field("provider_name", &self.provider_name)
            .field("provider_type", &self.provider_type)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("capabilities", &self.capabilities)
            .finish()
    }
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

/// Infer capabilities from provider name/id/config using substring heuristics.
///
/// This is a deliberate stopgap: the `Provider` struct does not yet carry a structured
/// capabilities field. Replace this with structured capability lookup once Provider grows
/// that field. Known false positives: a provider named "gpt-handler-service" gets Json
/// capability because "gpt" is a substring.
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
/// provider's `website_url`. Every candidate URL must pass [`validate_base_url`]
/// (HTTPS-only, no internal/private hosts). Invalid URLs are rejected with a
/// warning so a poisoned provider entry cannot exfiltrate API keys to internal
/// addresses (e.g. cloud metadata endpoints at 169.254.169.254).
fn extract_base_url(provider: &Provider) -> Option<String> {
    let env = provider.settings_config.get("env");
    let explicit = env
        .and_then(|v| v.get("ANTHROPIC_BASE_URL"))
        .or_else(|| env.and_then(|v| v.get("OPENAI_BASE_URL")))
        .or_else(|| env.and_then(|v| v.get("BASE_URL")))
        .or_else(|| provider.settings_config.get("baseUrl"))
        .or_else(|| provider.settings_config.get("base_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches('/').to_string());

    if let Some(url) = explicit {
        return validate_or_reject_base_url(provider, &url);
    }

    if let Some(homepage) = provider.website_url.clone() {
        log::warn!(
            "Provider '{}' has no explicit API base URL; falling back to website_url '{}'. \
             If this is not an API endpoint, set ANTHROPIC_BASE_URL / OPENAI_BASE_URL explicitly.",
            provider.id,
            homepage
        );
        return validate_or_reject_base_url(provider, &homepage);
    }

    None
}

/// Apply the SSRF guard to a base URL candidate. On validation failure, log
/// and return `None` so the caller's `build_target` surfaces a missing-URL
/// error rather than silently shipping credentials to an internal host.
fn validate_or_reject_base_url(provider: &Provider, url: &str) -> Option<String> {
    match validate_base_url(url) {
        Ok(()) => Some(url.to_string()),
        Err(reason) => {
            log::warn!(
                "Provider '{}' base URL '{}' rejected by SSRF guard: {}",
                provider.id,
                url,
                reason
            );
            None
        }
    }
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

    #[test]
    fn build_target_errors_when_model_missing() {
        let mut providers = IndexMap::new();
        let mut p = Provider::with_id(
            "claude-no-model".to_string(),
            "Provider claude-no-model".to_string(),
            json!({"env": {"ANTHROPIC_API_KEY": "sk", "ANTHROPIC_BASE_URL": "https://api.x.com"}}),
            Some("https://api.x.com".to_string()),
        );
        p.meta = Some(ProviderMeta {
            provider_type: Some("anthropic".to_string()),
            ..ProviderMeta::default()
        });
        providers.insert("claude-no-model".to_string(), p);

        let err = ProviderModelResolver::resolve_role("frontier", &providers, &[ModelCapability::Text])
            .unwrap_err();
        assert!(err.contains("model"), "error should mention model: {err}");
    }

    #[test]
    fn build_target_errors_when_base_url_missing() {
        let mut providers = IndexMap::new();
        let mut p = Provider::with_id(
            "claude-no-url".to_string(),
            "Provider claude-no-url".to_string(),
            json!({"env": {"ANTHROPIC_API_KEY": "sk", "ANTHROPIC_MODEL": "claude-1"}}),
            None,
        );
        p.meta = Some(ProviderMeta {
            provider_type: Some("anthropic".to_string()),
            ..ProviderMeta::default()
        });
        providers.insert("claude-no-url".to_string(), p);

        let err = ProviderModelResolver::resolve_role("frontier", &providers, &[ModelCapability::Text])
            .unwrap_err();
        assert!(
            err.contains("base URL") || err.contains("base_url"),
            "error should mention base URL: {err}"
        );
    }

    #[test]
    fn build_target_errors_when_api_key_missing() {
        let mut providers = IndexMap::new();
        let mut p = Provider::with_id(
            "claude-no-key".to_string(),
            "Provider claude-no-key".to_string(),
            json!({"env": {"ANTHROPIC_BASE_URL": "https://api.x.com", "ANTHROPIC_MODEL": "claude-1"}}),
            Some("https://api.x.com".to_string()),
        );
        p.meta = Some(ProviderMeta {
            provider_type: Some("anthropic".to_string()),
            ..ProviderMeta::default()
        });
        providers.insert("claude-no-key".to_string(), p);

        let err = ProviderModelResolver::resolve_role("frontier", &providers, &[ModelCapability::Text])
            .unwrap_err();
        assert!(
            err.contains("API key") || err.contains("api_key"),
            "error should mention API key: {err}"
        );
    }

    #[test]
    fn skips_providers_in_failover_queue() {
        let mut providers = IndexMap::new();
        let healthy = provider(
            "claude-healthy",
            "anthropic",
            "claude-A",
            "https://api.a.com",
            "sk-a",
        );
        let mut failing = provider(
            "claude-failing",
            "anthropic",
            "claude-B",
            "https://api.b.com",
            "sk-b",
        );
        failing.in_failover_queue = true;
        // Insert failing FIRST to verify the filter doesn't just pick first-inserted
        providers.insert("claude-failing".to_string(), failing);
        providers.insert("claude-healthy".to_string(), healthy);

        let target =
            ProviderModelResolver::resolve_role("frontier", &providers, &[ModelCapability::Text])
                .unwrap();
        assert_eq!(target.provider_id, "claude-healthy");
    }

    #[test]
    fn breaks_ties_deterministically_by_id_when_sort_index_equal() {
        let mut providers = IndexMap::new();
        // Insert in non-alphabetical order; both have no sort_index (defaults to usize::MAX)
        providers.insert(
            "zeta-claude".to_string(),
            provider(
                "zeta-claude",
                "anthropic",
                "claude-Z",
                "https://api.z.com",
                "sk-z",
            ),
        );
        providers.insert(
            "alpha-claude".to_string(),
            provider(
                "alpha-claude",
                "anthropic",
                "claude-A",
                "https://api.a.com",
                "sk-a",
            ),
        );

        let target =
            ProviderModelResolver::resolve_role("frontier", &providers, &[ModelCapability::Text])
                .unwrap();
        assert_eq!(
            target.provider_id, "alpha-claude",
            "ties should break by id ascending"
        );
    }

    #[test]
    fn debug_impl_redacts_api_key() {
        let p = provider(
            "claude-debug",
            "anthropic",
            "claude-1",
            "https://api.x.com",
            "sk-super-secret-key",
        );
        let target =
            ProviderModelResolver::resolve_role("frontier", &IndexMap::from([("claude-debug".to_string(), p)]), &[ModelCapability::Text])
                .unwrap();
        let dbg = format!("{target:?}");
        assert!(
            !dbg.contains("sk-super-secret-key"),
            "Debug output must not leak api_key: {dbg}"
        );
        assert!(dbg.contains("<redacted>"), "Debug output should mark redaction: {dbg}");
    }

    #[test]
    fn rejects_cloud_metadata_endpoint_via_ssrf_guard() {
        // Provider entry poisoned with AWS metadata endpoint as base URL.
        // SSRF guard must reject it instead of returning it for call_target to POST to.
        let p = provider(
            "claude-metadata",
            "anthropic",
            "claude-1",
            "https://169.254.169.254",
            "sk-exfil-target",
        );
        let result =
            ProviderModelResolver::resolve_role("frontier", &IndexMap::from([("claude-metadata".to_string(), p)]), &[ModelCapability::Text]);
        assert!(
            result.is_err(),
            "SSRF guard must reject 169.254.169.254 base URL, got: {result:?}"
        );
    }

    #[test]
    fn rejects_localhost_base_url_via_ssrf_guard() {
        let p = provider(
            "claude-local",
            "anthropic",
            "claude-1",
            "https://localhost",
            "sk-local",
        );
        let result =
            ProviderModelResolver::resolve_role("frontier", &IndexMap::from([("claude-local".to_string(), p)]), &[ModelCapability::Text]);
        assert!(
            result.is_err(),
            "SSRF guard must reject localhost base URL, got: {result:?}"
        );
    }

    #[test]
    fn rejects_http_scheme_base_url_via_ssrf_guard() {
        // validate_base_url requires HTTPS. A plaintext http:// URL must be rejected
        // even if the host is otherwise valid.
        let p = provider(
            "claude-http",
            "anthropic",
            "claude-1",
            "http://api.example.com",
            "sk-test",
        );
        let result =
            ProviderModelResolver::resolve_role("frontier", &IndexMap::from([("claude-http".to_string(), p)]), &[ModelCapability::Text]);
        assert!(
            result.is_err(),
            "SSRF guard must reject non-HTTPS base URL, got: {result:?}"
        );
    }

    #[test]
    fn rejects_website_url_fallback_pointing_to_internal_host() {
        // When explicit base URL is missing, the resolver falls back to website_url.
        // That fallback must also pass the SSRF guard, otherwise a poisoned provider
        // with a malicious homepage could still exfiltrate credentials.
        let mut p = Provider::with_id(
            "claude-internal".to_string(),
            "Provider claude-internal".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "sk-test",
                    "ANTHROPIC_MODEL": "claude-1",
                }
            }),
            Some("https://10.0.0.5".to_string()),
        );
        p.meta = Some(ProviderMeta {
            provider_type: Some("anthropic".to_string()),
            ..ProviderMeta::default()
        });
        let result = ProviderModelResolver::resolve_role(
            "frontier",
            &IndexMap::from([("claude-internal".to_string(), p)]),
            &[ModelCapability::Text],
        );
        assert!(
            result.is_err(),
            "SSRF guard must reject website_url fallback to internal host, got: {result:?}"
        );
    }
}
