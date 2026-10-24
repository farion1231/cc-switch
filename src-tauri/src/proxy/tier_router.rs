//! 档位路由（端点引用化 P3）
//!
//! 条目 `tier_mappings` 中带 `endpoint` 引用的档位，在代理模式下分流到
//! 目标端点：识别请求模型的档位 → 构造影子 Provider 置于候选池首位，
//! 原池保留为回落链（目标家挂了退回自家该档，不摊给主队列）。
//!
//! 边界（设计文档第六节）：
//! - 档位识别依据请求体 model 名（与 `ModelMapping::map_model` 同一分类序）
//! - 跨端点档位不写 CLI 直连 env——直连模式物理上单条目一个 base_url
//! - 熔断按端点记账（影子 Provider id = 端点 id）：B 家挂了不拖垮 A 家主档
//! - usage 归因随影子 Provider 落到目标端点

use crate::provider::{Endpoint, Provider, ProviderMeta, TierMapping};

/// 归一化：剥离 Claude Code 的 `[1M]` 本地能力标记后再比对
fn strip_one_m(model: &str) -> &str {
    const MARKER: &str = "[1m]";
    let trimmed = model.trim_end();
    if trimmed.len() >= MARKER.len()
        && trimmed[trimmed.len() - MARKER.len()..].eq_ignore_ascii_case(MARKER)
    {
        trimmed[..trimmed.len() - MARKER.len()].trim_end()
    } else {
        model
    }
}

/// 档位路由目标（已解析端点引用）
#[derive(Debug, Clone, PartialEq)]
pub struct TierRoute {
    pub endpoint_id: String,
    pub model: String,
}

/// 按请求模型识别命中的跨端点档位。
///
/// 分类序与 `ModelMapping::map_model` 一致（fable → haiku → opus → sonnet），
/// subagent 按配置模型等值匹配（含 1M 剥离），未命中具体档时回落 default。
/// 只返回带 `endpoint` 引用的档位——无引用的档位走既有模型映射路径。
pub fn resolve_tier_route(provider: &Provider, request_model: &str) -> Option<TierRoute> {
    let tiers = provider.tier_mappings.as_ref()?;
    let lower = request_model.to_lowercase();

    let tier: Option<&TierMapping> = if lower.contains("fable") {
        // fable 档未配时与官方分类器一致降级到 opus 档
        tiers.fable.as_ref().or(tiers.opus.as_ref())
    } else if lower.contains("haiku") {
        tiers.haiku.as_ref()
    } else if lower.contains("opus") {
        tiers.opus.as_ref()
    } else if lower.contains("sonnet") {
        tiers.sonnet.as_ref()
    } else if let Some(subagent) = tiers.subagent.as_ref() {
        let matches_subagent = subagent
            .model
            .as_deref()
            .is_some_and(|m| strip_one_m(m) == strip_one_m(request_model));
        if matches_subagent {
            Some(subagent)
        } else {
            tiers.default.as_ref()
        }
    } else {
        tiers.default.as_ref()
    };

    let tier = tier?;
    let endpoint_id = tier.endpoint.as_deref().filter(|s| !s.is_empty())?;
    let model = tier
        .model
        .clone()
        .unwrap_or_else(|| request_model.to_string());
    Some(TierRoute {
        endpoint_id: endpoint_id.to_string(),
        model,
    })
}

/// 影子 Provider：目标端点凭据 + 档位目标模型。
///
/// - `ANTHROPIC_MODEL` 设为档位目标模型：forwarder 的 `apply_model_mapping`
///   对任意请求模型名都落到 default 档 = 目标模型，不再需要逐档 env
/// - `meta.api_format` 来自端点：transform 管线据此在 Claude/OpenAI/Gemini
///   wire 格式间转换（跨家组合的关键）
/// - id = 端点 id：熔断、usage、在途计数天然按端点归因与细分
pub fn shadow_provider(endpoint: &Endpoint, route: &TierRoute) -> Provider {
    let mut provider = Provider::with_id(
        endpoint.id.clone(),
        endpoint.name.clone(),
        serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": endpoint.base_url,
                "ANTHROPIC_AUTH_TOKEN": endpoint.api_key,
                "ANTHROPIC_MODEL": route.model,
            }
        }),
        endpoint.website_url.clone(),
    );
    provider.category = endpoint.category.clone();
    provider.icon = endpoint.icon.clone();
    provider.icon_color = endpoint.icon_color.clone();
    if let Some(api_format) = endpoint.api_format.as_deref() {
        provider.meta = Some(ProviderMeta {
            api_format: Some(api_format.to_string()),
            ..Default::default()
        });
    }
    provider
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::TierMappings;
    use serde_json::json;

    fn provider_with_tiers(tiers: TierMappings) -> Provider {
        let mut p = Provider::with_id(
            "entry".to_string(),
            "Entry".to_string(),
            json!({"env": {"ANTHROPIC_BASE_URL": "https://self.example", "ANTHROPIC_AUTH_TOKEN": "sk-self"}}),
            None,
        );
        p.tier_mappings = Some(tiers);
        p
    }

    fn cross(tier_model: &str) -> TierMapping {
        TierMapping {
            endpoint: Some("ep-cheap".to_string()),
            model: Some(tier_model.to_string()),
        }
    }

    #[test]
    fn haiku_tier_routes_to_cross_endpoint() {
        let tiers = TierMappings {
            haiku: Some(cross("gemini-3-flash")),
            ..Default::default()
        };
        let route = resolve_tier_route(&provider_with_tiers(tiers), "claude-haiku-4-5").unwrap();
        assert_eq!(route.endpoint_id, "ep-cheap");
        assert_eq!(route.model, "gemini-3-flash");
    }

    #[test]
    fn fable_tier_falls_back_to_opus_tier_config() {
        // fable 请求命中 opus 档的跨端点配置（与 map_model 的降级方向一致）
        let tiers = TierMappings {
            opus: Some(cross("kimi-k2.5")),
            ..Default::default()
        };
        let route = resolve_tier_route(&provider_with_tiers(tiers), "claude-fable-5").unwrap();
        assert_eq!(route.model, "kimi-k2.5");
    }

    #[test]
    fn one_m_suffix_is_stripped_for_subagent_match() {
        let mut subagent = cross("gpt-5.4-mini");
        subagent.endpoint = Some("ep-openai".to_string());
        let tiers = TierMappings {
            subagent: Some(subagent),
            ..Default::default()
        };
        let provider = provider_with_tiers(tiers);
        assert_eq!(
            resolve_tier_route(&provider, "gpt-5.4-mini[1M]").map(|r| r.model),
            Some("gpt-5.4-mini".to_string())
        );
    }

    #[test]
    fn tier_without_endpoint_is_not_routed() {
        // 无 endpoint 引用 = 仅改模型名，走既有 ModelMapping 路径
        let tiers = TierMappings {
            haiku: Some(TierMapping {
                endpoint: None,
                model: Some("self-haiku".to_string()),
            }),
            ..Default::default()
        };
        assert!(resolve_tier_route(&provider_with_tiers(tiers), "claude-haiku-4-5").is_none());
    }

    #[test]
    fn unknown_model_hits_default_tier_when_cross_endpoint() {
        let tiers = TierMappings {
            default: Some(cross("glm-5")),
            ..Default::default()
        };
        let route = resolve_tier_route(&provider_with_tiers(tiers), "some-unknown-model").unwrap();
        assert_eq!(route.model, "glm-5");
    }

    #[test]
    fn no_tier_mappings_means_no_route() {
        let provider = Provider::with_id("e".into(), "E".into(), json!({}), None);
        assert!(resolve_tier_route(&provider, "claude-haiku-4-5").is_none());
    }

    #[test]
    fn shadow_provider_carries_endpoint_identity_and_api_format() {
        let mut endpoint = Endpoint::new(
            "ep-cheap".to_string(),
            "Cheap Relay".to_string(),
            "https://cheap.example".to_string(),
            "sk-cheap".to_string(),
        );
        endpoint.api_format = Some("openai_chat".to_string());
        let route = TierRoute {
            endpoint_id: "ep-cheap".to_string(),
            model: "gemini-3-flash".to_string(),
        };
        let shadow = shadow_provider(&endpoint, &route);
        // id = 端点 id：熔断/usage 按端点归因
        assert_eq!(shadow.id, "ep-cheap");
        assert_eq!(shadow.name, "Cheap Relay");
        assert_eq!(
            shadow.settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://cheap.example"
        );
        assert_eq!(
            shadow.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            "sk-cheap"
        );
        // default 档 = 目标模型：任意请求模型经 map_model 都落到该值
        assert_eq!(
            shadow.settings_config["env"]["ANTHROPIC_MODEL"],
            "gemini-3-flash"
        );
        assert_eq!(
            shadow.meta.as_ref().and_then(|m| m.api_format.as_deref()),
            Some("openai_chat")
        );
    }
}
