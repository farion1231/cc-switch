//! 模型路由模块
//!
//! 在请求转发前，根据供应商配置将请求路由到目标模型。

use crate::claude_desktop_config::ONE_M_CONTEXT_MARKER;
use crate::provider::Provider;
use regex::Regex;
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct ProviderModelRoute {
    pub enabled: bool,
    pub match_mode: String,
    pub source: String,
    pub target: String,
}

/// 模型路由配置。
pub struct ModelRouter {
    pub routes: Vec<ProviderModelRoute>,
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
    pub fable_model: Option<String>,
    pub subagent_model: Option<String>,
    pub default_model: Option<String>,
}

impl ModelRouter {
    /// 从供应商配置中提取模型路由。
    pub fn from_provider(provider: &Provider) -> Self {
        let env = provider.settings_config.get("env");
        let routes = provider
            .settings_config
            .get("modelRoutes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let object = entry.as_object()?;
                let source = object.get("source")?.as_str()?.trim();
                let target = object.get("target")?.as_str()?.trim();
                if source.is_empty() || target.is_empty() {
                    return None;
                }
                Some(ProviderModelRoute {
                    enabled: object
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    match_mode: object
                        .get("matchMode")
                        .and_then(Value::as_str)
                        .unwrap_or("exact")
                        .to_string(),
                    source: source.to_string(),
                    target: target.to_string(),
                })
            })
            .collect();

        Self {
            routes,
            haiku_model: env
                .and_then(|value| value.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(String::from),
            sonnet_model: env
                .and_then(|value| value.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(String::from),
            opus_model: env
                .and_then(|value| value.get("ANTHROPIC_DEFAULT_OPUS_MODEL"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(String::from),
            fable_model: env
                .and_then(|value| value.get("ANTHROPIC_DEFAULT_FABLE_MODEL"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(String::from),
            subagent_model: env
                .and_then(|value| value.get("CLAUDE_CODE_SUBAGENT_MODEL"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(String::from),
            default_model: env
                .and_then(|value| value.get("ANTHROPIC_MODEL"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(String::from),
        }
    }

    /// 检查是否配置了任何模型路由。
    pub fn has_routing(&self) -> bool {
        !self.routes.is_empty()
            || self.haiku_model.is_some()
            || self.sonnet_model.is_some()
            || self.opus_model.is_some()
            || self.fable_model.is_some()
            || self.subagent_model.is_some()
            || self.default_model.is_some()
    }

    /// 根据原始模型名称获取路由目标模型。
    pub fn route_model(&self, original_model: &str) -> String {
        if let Some(route) = self
            .routes
            .iter()
            .find(|route| route.enabled && route_matches_model(route, original_model))
        {
            return route.target.clone();
        }

        let model_lower = original_model.to_lowercase();

        if model_lower.contains("fable") {
            if let Some(ref model) = self.fable_model {
                return model.clone();
            }
            // 未单独配置 fable 档时归入 opus 档，与 Claude Code 官方分类器的降级方向一致。
            if let Some(ref model) = self.opus_model {
                return model.clone();
            }
        }
        if model_lower.contains("haiku") {
            if let Some(ref model) = self.haiku_model {
                return model.clone();
            }
        }
        if model_lower.contains("opus") {
            if let Some(ref model) = self.opus_model {
                return model.clone();
            }
        }
        if model_lower.contains("sonnet") {
            if let Some(ref model) = self.sonnet_model {
                return model.clone();
            }
        }

        if let Some(ref model) = self.subagent_model {
            if strip_one_m_suffix_for_upstream(original_model)
                == strip_one_m_suffix_for_upstream(model)
            {
                return original_model.to_string();
            }
        }

        if let Some(ref model) = self.default_model {
            return model.clone();
        }

        original_model.to_string()
    }

    fn has_provider_target(&self, target_model: &str) -> bool {
        let target_model = target_model.trim();
        self.routes
            .iter()
            .any(|route| route.enabled && route.target == target_model)
    }
}

fn route_matches_model(route: &ProviderModelRoute, model: &str) -> bool {
    match route.match_mode.as_str() {
        "prefix" => model.starts_with(&route.source),
        "suffix" => model.ends_with(&route.source),
        "contains" | "keyword" => model.contains(&route.source),
        "regex" => Regex::new(&route.source)
            .map(|regex| regex.is_match(model))
            .unwrap_or(false),
        _ => model == route.source,
    }
}

/// 使用当前供应商的本地规则路由模型标识。
pub fn route_provider_model(provider: &Provider, model: &str) -> String {
    ModelRouter::from_provider(provider).route_model(model)
}

/// 判断模型是否为当前供应商某条已启用路由的目标。
pub fn is_provider_model_route_target(provider: &Provider, model: &str) -> bool {
    ModelRouter::from_provider(provider).has_provider_target(model)
}

/// Gemini 原生协议将模型放在请求路径中，而不是 JSON 请求体中。
pub fn apply_model_route_to_endpoint(mut endpoint: String, provider: &Provider) -> String {
    let Some(models_index) = endpoint.find("/models/") else {
        return endpoint;
    };
    let model_start = models_index + "/models/".len();
    let Some(relative_end) = endpoint[model_start..].find(':') else {
        return endpoint;
    };
    let model_end = model_start + relative_end;
    let model = &endpoint[model_start..model_end];
    let routed = route_provider_model(provider, model);
    if routed != model {
        endpoint.replace_range(model_start..model_end, &routed);
    }
    endpoint
}

/// 对请求体应用模型路由。
///
/// 返回路由后的请求体、原始模型名和路由目标模型名。
pub fn apply_model_routing(
    mut body: Value,
    provider: &Provider,
) -> (Value, Option<String>, Option<String>) {
    let router = ModelRouter::from_provider(provider);

    if !router.has_routing() {
        let original = body.get("model").and_then(Value::as_str).map(String::from);
        return (body, original, None);
    }

    let original_model = body.get("model").and_then(Value::as_str).map(String::from);

    if let Some(ref original) = original_model {
        let routed = router.route_model(original);
        if routed != *original {
            log::debug!("[ModelRouter] 模型路由: {original} → {routed}");
            body["model"] = json!(routed);
            return (body, Some(original.clone()), Some(routed));
        }
    }

    (body, original_model, None)
}

/// Claude Code 通过 `[1M]` 后缀声明 100 万上下文能力，上游 API 通常不接受该标记。
pub fn strip_one_m_suffix_for_upstream(model: &str) -> &str {
    let trimmed = model.trim_end();
    let marker = ONE_M_CONTEXT_MARKER.as_bytes();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= marker.len()
        && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
    {
        return trimmed[..trimmed.len() - marker.len()].trim_end();
    }
    model
}

pub fn strip_one_m_suffix_for_upstream_from_body(mut body: Value) -> Value {
    let Some(model) = body.get("model").and_then(Value::as_str) else {
        return body;
    };

    let stripped = strip_one_m_suffix_for_upstream(model);
    if stripped != model {
        log::debug!("[ModelRouter] 去除本地 1M 标记: {model} → {stripped}");
        body["model"] = json!(stripped);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_provider(settings_config: Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn provider_without_routing() -> Provider {
        create_provider(json!({}))
    }

    #[test]
    fn routes_claude_model_tiers_and_default() {
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-routed",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-routed",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-routed",
                "ANTHROPIC_DEFAULT_FABLE_MODEL": "fable-routed"
            }
        }));

        for (source, target) in [
            ("claude-haiku-4-5", "haiku-routed"),
            ("claude-sonnet-4-5", "sonnet-routed"),
            ("claude-opus-4-5", "opus-routed"),
            ("claude-fable-5", "fable-routed"),
            ("unknown-model", "default-model"),
        ] {
            let (body, _, routed) = apply_model_routing(json!({"model": source}), &provider);
            assert_eq!(body["model"], target);
            assert_eq!(routed, Some(target.to_string()));
        }
    }

    #[test]
    fn fable_falls_back_to_opus() {
        let provider = create_provider(json!({
            "env": {"ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-routed"}
        }));
        let (body, _, _) = apply_model_routing(json!({"model": "claude-fable-5"}), &provider);
        assert_eq!(body["model"], "opus-routed");
    }

    #[test]
    fn preserves_subagent_model_before_default_fallback() {
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model",
                "CLAUDE_CODE_SUBAGENT_MODEL": "gpt-5.4-mini"
            }
        }));
        let (body, _, routed) =
            apply_model_routing(json!({"model": "gpt-5.4-mini[1M]"}), &provider);
        assert_eq!(body["model"], "gpt-5.4-mini[1M]");
        assert!(routed.is_none());
    }

    #[test]
    fn leaves_body_unchanged_without_routing() {
        let provider = provider_without_routing();
        let body = json!({"model": "gpt-5"});
        let (result, original, routed) = apply_model_routing(body, &provider);
        assert_eq!(result["model"], "gpt-5");
        assert_eq!(original, Some("gpt-5".to_string()));
        assert!(routed.is_none());
    }

    #[test]
    fn provider_routes_are_isolated() {
        let first = create_provider(json!({
            "modelRoutes": [
                {"matchMode": "exact", "source": "gpt-5", "target": "model-a"}
            ]
        }));
        let second = create_provider(json!({
            "modelRoutes": [
                {"matchMode": "exact", "source": "gpt-5", "target": "model-b"}
            ]
        }));

        let (first_body, _, _) = apply_model_routing(json!({"model": "gpt-5"}), &first);
        let (second_body, _, _) = apply_model_routing(json!({"model": "gpt-5"}), &second);
        assert_eq!(first_body["model"], "model-a");
        assert_eq!(second_body["model"], "model-b");
    }

    #[test]
    fn supports_ordered_match_modes_and_disabled_routes() {
        let provider = create_provider(json!({
            "modelRoutes": [
                {"enabled": false, "matchMode": "exact", "source": "disabled", "target": "wrong"},
                {"enabled": true, "matchMode": "prefix", "source": "claude-", "target": "prefix-target"},
                {"enabled": true, "matchMode": "suffix", "source": "-fast", "target": "suffix-target"},
                {"enabled": true, "matchMode": "contains", "source": "special", "target": "contains-target"},
                {"enabled": true, "matchMode": "regex", "source": "^vendor-[0-9]+$", "target": "regex-target"}
            ]
        }));

        for (source, target) in [
            ("disabled", "disabled"),
            ("claude-sonnet", "prefix-target"),
            ("model-fast", "suffix-target"),
            ("my-special-model", "contains-target"),
            ("vendor-42", "regex-target"),
        ] {
            let (body, _, _) = apply_model_routing(json!({"model": source}), &provider);
            assert_eq!(body["model"], target);
        }
    }

    #[test]
    fn first_matching_route_wins() {
        let provider = create_provider(json!({
            "modelRoutes": [
                {"matchMode": "prefix", "source": "gpt-", "target": "first"},
                {"matchMode": "exact", "source": "gpt-5", "target": "second"}
            ]
        }));
        let (body, _, _) = apply_model_routing(json!({"model": "gpt-5"}), &provider);
        assert_eq!(body["model"], "first");
    }

    #[test]
    fn ignores_invalid_regex_routes() {
        let provider = create_provider(json!({
            "modelRoutes": [
                {"matchMode": "regex", "source": "[", "target": "invalid"},
                {"matchMode": "exact", "source": "fallback", "target": "valid"}
            ]
        }));
        let (body, _, _) = apply_model_routing(json!({"model": "fallback"}), &provider);
        assert_eq!(body["model"], "valid");
    }

    #[test]
    fn detects_only_enabled_route_targets() {
        let provider = create_provider(json!({
            "modelRoutes": [
                {"enabled": true, "source": "source-a", "target": " target-a "},
                {"enabled": false, "source": "source-b", "target": "target-b"}
            ]
        }));
        assert!(is_provider_model_route_target(&provider, "target-a"));
        assert!(!is_provider_model_route_target(&provider, "target-b"));
    }

    #[test]
    fn routes_gemini_model_in_endpoint() {
        let provider = create_provider(json!({
            "modelRoutes": [
                {"matchMode": "exact", "source": "gemini-3-pro", "target": "gemini-3.6-flash"}
            ]
        }));
        let endpoint = apply_model_route_to_endpoint(
            "/v1beta/models/gemini-3-pro:streamGenerateContent?alt=sse".to_string(),
            &provider,
        );
        assert_eq!(
            endpoint,
            "/v1beta/models/gemini-3.6-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn strips_one_m_suffix_before_upstream() {
        assert_eq!(
            strip_one_m_suffix_for_upstream("deepseek-v4-pro[1M]"),
            "deepseek-v4-pro"
        );
        assert_eq!(
            strip_one_m_suffix_for_upstream("deepseek-v4-pro"),
            "deepseek-v4-pro"
        );
    }
}
