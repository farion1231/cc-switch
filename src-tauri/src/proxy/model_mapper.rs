//! 模型映射模块
//!
//! 在请求转发前，根据 Provider 配置替换请求中的模型名称

use crate::claude_desktop_config::ONE_M_CONTEXT_MARKER;
use crate::provider::Provider;
use crate::proxy::types::{ModelTierRoutingConfig, TierRoute};
use serde_json::Value;

/// 模型映射配置
pub struct ModelMapping {
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
    pub fable_model: Option<String>,
    pub subagent_model: Option<String>,
    pub default_model: Option<String>,
}

impl ModelMapping {
    /// 从 Provider 配置中提取模型映射
    pub fn from_provider(provider: &Provider) -> Self {
        let env = provider.settings_config.get("env");

        Self {
            haiku_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            sonnet_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            opus_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_OPUS_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            fable_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_FABLE_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            subagent_model: env
                .and_then(|e| e.get("CLAUDE_CODE_SUBAGENT_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            default_model: env
                .and_then(|e| e.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
        }
    }

    /// 检查是否配置了任何模型映射
    pub fn has_mapping(&self) -> bool {
        self.haiku_model.is_some()
            || self.sonnet_model.is_some()
            || self.opus_model.is_some()
            || self.fable_model.is_some()
            || self.subagent_model.is_some()
            || self.default_model.is_some()
    }

    /// 根据原始模型名称获取映射后的模型
    pub fn map_model(&self, original_model: &str) -> String {
        let model_lower = original_model.to_lowercase();

        // 1. 按模型类型匹配
        if model_lower.contains("fable") {
            if let Some(ref m) = self.fable_model {
                return m.clone();
            }
            // 未单独配置 fable 档时归入 opus 档，与 Claude Code 官方
            // 分类器降级方向一致（fable→opus），避免落到 default 失去层级。
            if let Some(ref m) = self.opus_model {
                return m.clone();
            }
        }
        if model_lower.contains("haiku") {
            if let Some(ref m) = self.haiku_model {
                return m.clone();
            }
        }
        if model_lower.contains("opus") {
            if let Some(ref m) = self.opus_model {
                return m.clone();
            }
        }
        if model_lower.contains("sonnet") {
            if let Some(ref m) = self.sonnet_model {
                return m.clone();
            }
        }

        if let Some(ref m) = self.subagent_model {
            if strip_one_m_suffix_for_upstream(original_model) == strip_one_m_suffix_for_upstream(m)
            {
                return original_model.to_string();
            }
        }

        // 2. 默认模型
        if let Some(ref m) = self.default_model {
            return m.clone();
        }

        // 3. 无映射，保持原样
        original_model.to_string()
    }
}

/// 对请求体应用模型映射
///
/// 返回 (映射后的请求体, 原始模型名, 映射后模型名)
pub fn apply_model_mapping(
    mut body: Value,
    provider: &Provider,
) -> (Value, Option<String>, Option<String>) {
    let mapping = ModelMapping::from_provider(provider);

    // 如果没有配置映射，直接返回
    if !mapping.has_mapping() {
        let original = body.get("model").and_then(|m| m.as_str()).map(String::from);
        return (body, original, None);
    }

    // 提取原始模型名
    let original_model = body.get("model").and_then(|m| m.as_str()).map(String::from);

    if let Some(ref original) = original_model {
        let mapped = mapping.map_model(original);

        if mapped != *original {
            log::debug!("[ModelMapper] 模型映射: {original} → {mapped}");
            body["model"] = serde_json::json!(mapped);
            return (body, Some(original.clone()), Some(mapped));
        }
    }

    (body, original_model, None)
}

/// Claude Code 通过 `[1M]` 后缀声明 100 万上下文能力；上游 API
/// 通常不接受这个本地能力标记，转发前需要剥离。
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
        log::debug!("[ModelMapper] 去除本地 1M 标记: {model} → {stripped}");
        body["model"] = serde_json::json!(stripped);
    }
    body
}

// ============================================================================
// 模型层级路由（Model Tier Routing）
// ============================================================================

/// Claude 的模型层级。Claude Code 始终以 opus/sonnet/haiku/fable 之一作为请求模型，
/// 上游中转常需要把它们分发到不同 Provider 并改写成各自的模型名（如 opus→glm-5.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Opus,
    Sonnet,
    Haiku,
    Fable,
}

impl ModelTier {
    /// 路由表里的字符串 key，与 `ModelTierRoutingConfig.routes` 的 tier key 对齐。
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelTier::Opus => "opus",
            ModelTier::Sonnet => "sonnet",
            ModelTier::Haiku => "haiku",
            ModelTier::Fable => "fable",
        }
    }
}

/// 把客户端请求模型名归类为 Claude 层级。
///
/// 子串匹配顺序与 `ModelMapping::map_model` 一致（fable→haiku→opus→sonnet）。
/// 大小写不敏感；`[1m]` 等后缀不影响子串判定。Codex/Gemini 的真实模型名
/// （不含这些关键词）返回 `None`，因此层级路由天然只对 Claude 生效。
pub fn classify_model_tier(model: &str) -> Option<ModelTier> {
    let m = model.to_lowercase();
    if m.contains("fable") {
        Some(ModelTier::Fable)
    } else if m.contains("haiku") {
        Some(ModelTier::Haiku)
    } else if m.contains("opus") {
        Some(ModelTier::Opus)
    } else if m.contains("sonnet") {
        Some(ModelTier::Sonnet)
    } else {
        None
    }
}

/// 层级路由解析结果。
#[derive(Debug, Clone, Default)]
pub struct TierRoutingOutcome {
    /// 重排后的 Provider 链（命中时目标 Provider 提到首位，去重）。
    pub providers: Vec<Provider>,
    /// 命中的目标 Provider id（仅当发生路由覆写时有值）。
    pub routed_provider_id: Option<String>,
    /// 改写后的上游模型名（仅当发生路由覆写时有值）。
    pub model_override: Option<String>,
}

/// 应用层级路由：纯函数，便于单测。
///
/// 任一条件不满足都「不改」原选择，原样返回：
/// 配置未启用、请求模型无法归类为层级、该 app/tier 无路由项、目标 Provider 查不到或不可路由。
///
/// 命中则把目标 Provider 提到链首并去重，同时回填 `routed_provider_id` 与 `model_override`。
/// 调用方负责把 `model_override` 应用到目标 Provider 的请求体副本（仅对该 Provider 生效，
/// 故障转移队列中的其它 Provider 仍用客户端原始别名）。
pub fn apply_tier_routing(
    request_model: &str,
    app_type: &str,
    config: &ModelTierRoutingConfig,
    selected: &[Provider],
    lookup: impl Fn(&str) -> Option<Provider>,
) -> TierRoutingOutcome {
    let fallback = || TierRoutingOutcome {
        providers: selected.to_vec(),
        routed_provider_id: None,
        model_override: None,
    };

    if !config.is_enabled_for_app(app_type) {
        return fallback();
    }

    let Some(tier) = classify_model_tier(request_model) else {
        return fallback();
    };

    let TierRoute {
        provider_id, model, ..
    } = config
        .routes
        .get(app_type)
        .and_then(|tiers| tiers.get(tier.as_str()))
        .cloned()
        .unwrap_or_default();

    if provider_id.is_empty() || model.trim().is_empty() {
        return fallback();
    }

    let Some(routed) = lookup(&provider_id) else {
        // 路由项指向不存在的 Provider（被删除等），不改，避免把请求送进死路。
        log::warn!(
            "[ModelRouter] 层级 {} 路由到不存在的 Provider {provider_id}，回退默认选择",
            tier.as_str()
        );
        return fallback();
    };

    if !routed.supports_routing() {
        // 路由项指向不可路由的 Provider（如官方账号：无可劫持 base_url、认证为 1P
        // OAuth，代理无法转发）。这是脏数据/旧配置——前端下拉已过滤，但后端是执行点，
        // 必须同样拦住，否则请求仍会被改写 model 并送进代理无法转发的上游。
        // 与前端 supportsRouting 同判据（Provider::supports_routing）。
        log::warn!(
            "[ModelRouter] 层级 {} 路由到不可路由的 Provider {provider_id}（category={:?}），回退默认选择",
            tier.as_str(),
            routed.category
        );
        return fallback();
    }

    // 去重后把目标 Provider 提到链首：保持故障转移语义（目标失败时仍按原队列降级）。
    let mut chain: Vec<Provider> = vec![routed.clone()];
    for p in selected.iter() {
        if p.id != routed.id {
            chain.push(p.clone());
        }
    }
    // 若目标 Provider 原本不在 selected 里，chain 会比原链多 1 个——这是预期行为
    // （路由可以把「未在故障转移队列中」的 Provider 提为首选）。

    log::debug!(
        "[ModelRouter] {request_model}({}) → Provider {} (model: {model})，链长 {}",
        tier.as_str(),
        routed.id,
        chain.len()
    );

    TierRoutingOutcome {
        providers: chain,
        routed_provider_id: Some(routed.id),
        model_override: Some(model),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn create_provider_with_mapping() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-mapped",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-mapped",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped",
                    "ANTHROPIC_DEFAULT_FABLE_MODEL": "fable-mapped"
                }
            }),
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

    fn create_provider_without_mapping() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({}),
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

    #[test]
    fn test_sonnet_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-sonnet-4-5-20250929"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(original, Some("claude-sonnet-4-5-20250929".to_string()));
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_haiku_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-haiku-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "haiku-mapped");
        assert_eq!(mapped, Some("haiku-mapped".to_string()));
    }

    #[test]
    fn test_opus_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-opus-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "opus-mapped");
        assert_eq!(mapped, Some("opus-mapped".to_string()));
    }

    #[test]
    fn test_fable_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-fable-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "fable-mapped");
        assert_eq!(mapped, Some("fable-mapped".to_string()));
    }

    #[test]
    fn test_fable_with_one_m_suffix_mapping() {
        // Claude Code 实际会发 claude-fable-5[1m] 形态（issue #3980）
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-fable-5[1m]"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "fable-mapped");
        assert_eq!(mapped, Some("fable-mapped".to_string()));
    }

    #[test]
    fn test_fable_falls_back_to_opus_when_unset() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped"
            }
        });
        let body = json!({"model": "claude-fable-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "opus-mapped");
        assert_eq!(mapped, Some("opus-mapped".to_string()));
    }

    #[test]
    fn test_fable_falls_back_to_default_without_opus() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model"
            }
        });
        let body = json!({"model": "claude-fable-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "default-model");
        assert_eq!(mapped, Some("default-model".to_string()));
    }

    #[test]
    fn test_thinking_does_not_affect_model_mapping() {
        // Issue #2081: thinking 参数不应影响模型映射
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "enabled"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_thinking_adaptive_does_not_affect_model_mapping() {
        // Issue #2081: adaptive thinking 也不应影响模型映射
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "adaptive"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_thinking_disabled() {
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "disabled"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_unknown_model_uses_default() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "some-unknown-model"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "default-model");
        assert_eq!(mapped, Some("default-model".to_string()));
    }

    #[test]
    fn test_subagent_model_preserved_before_default_fallback() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model",
                "CLAUDE_CODE_SUBAGENT_MODEL": "gpt-5.4-mini"
            }
        });

        let body = json!({"model": "gpt-5.4-mini"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);

        assert_eq!(result["model"], "gpt-5.4-mini");
        assert_eq!(original, Some("gpt-5.4-mini".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_subagent_model_preserved_with_one_m_suffix_before_default_fallback() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model",
                "CLAUDE_CODE_SUBAGENT_MODEL": "gpt-5.4-mini"
            }
        });

        let body = json!({"model": "gpt-5.4-mini[1M]"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);

        assert_eq!(result["model"], "gpt-5.4-mini[1M]");
        assert_eq!(original, Some("gpt-5.4-mini[1M]".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_no_mapping_configured() {
        let provider = create_provider_without_mapping();
        let body = json!({"model": "claude-sonnet-4-5"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "claude-sonnet-4-5");
        assert_eq!(original, Some("claude-sonnet-4-5".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "Claude-SONNET-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn strips_one_m_suffix_before_upstream() {
        let body = json!({"model": "deepseek-v4-pro[1M]"});
        let result = strip_one_m_suffix_for_upstream_from_body(body);
        assert_eq!(result["model"], "deepseek-v4-pro");
    }

    #[test]
    fn strips_one_m_suffix_after_mapping() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro [1M]"
            }
        });

        let body = json!({"model": "claude-sonnet-4-6"});
        let (mapped, _, _) = apply_model_mapping(body, &provider);
        let result = strip_one_m_suffix_for_upstream_from_body(mapped);

        assert_eq!(result["model"], "deepseek-v4-pro");
    }

    #[test]
    fn keeps_model_without_one_m_suffix() {
        let body = json!({"model": "deepseek-v4-pro"});
        let result = strip_one_m_suffix_for_upstream_from_body(body);
        assert_eq!(result["model"], "deepseek-v4-pro");
    }

    // ---- 模型层级路由 (Model Tier Routing) ----

    fn route(provider_id: &str, model: &str) -> TierRoute {
        TierRoute {
            provider_id: provider_id.to_string(),
            model: model.to_string(),
            display_name: String::new(),
        }
    }

    fn provider_with_id(id: &str) -> Provider {
        Provider {
            id: id.to_string(),
            name: format!("Provider {id}"),
            settings_config: json!({}),
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

    fn provider_with_category(id: &str, category: &str) -> Provider {
        let mut p = provider_with_id(id);
        p.category = Some(category.to_string());
        p
    }

    #[test]
    fn classify_tier_matches_each_level() {
        assert_eq!(
            classify_model_tier("claude-opus-4-8"),
            Some(ModelTier::Opus)
        );
        assert_eq!(
            classify_model_tier("claude-sonnet-4-6"),
            Some(ModelTier::Sonnet)
        );
        assert_eq!(
            classify_model_tier("claude-haiku-4-5"),
            Some(ModelTier::Haiku)
        );
        assert_eq!(
            classify_model_tier("claude-fable-5"),
            Some(ModelTier::Fable)
        );
    }

    #[test]
    fn classify_tier_case_insensitive_and_1m_suffix() {
        assert_eq!(
            classify_model_tier("Claude-OPUS-4-8"),
            Some(ModelTier::Opus)
        );
        // [1m] 后缀不影响子串匹配
        assert_eq!(
            classify_model_tier("claude-opus-4-8[1m]"),
            Some(ModelTier::Opus)
        );
    }

    #[test]
    fn classify_tier_unknown_is_none() {
        // Codex/Gemini 真实模型名不含层级关键词 → 不归类 → 路由不生效
        assert_eq!(classify_model_tier("glm-5.2"), None);
        assert_eq!(classify_model_tier("gpt-5"), None);
        assert_eq!(classify_model_tier("unknown"), None);
        assert_eq!(classify_model_tier(""), None);
    }

    fn build_config(routes: &[(&str, &str, TierRoute)]) -> ModelTierRoutingConfig {
        let mut map: HashMap<String, HashMap<String, TierRoute>> = HashMap::new();
        for (app, tier, r) in routes {
            map.entry((*app).to_string())
                .or_default()
                .insert((*tier).to_string(), r.clone());
        }
        ModelTierRoutingConfig {
            enabled: true,
            routes: map,
            ..Default::default()
        }
    }

    #[test]
    fn tier_routing_disabled_passes_through() {
        let selected = vec![provider_with_id("a"), provider_with_id("b")];
        let cfg = ModelTierRoutingConfig {
            enabled: false,
            ..Default::default()
        };
        let out = apply_tier_routing("claude-opus-4-8", "claude", &cfg, &selected, |id| {
            Some(provider_with_id(id))
        });
        assert_eq!(out.providers.len(), 2);
        assert_eq!(out.providers[0].id, "a");
        assert!(out.routed_provider_id.is_none());
        assert!(out.model_override.is_none());
    }

    #[test]
    fn tier_routing_hits_moves_target_to_front_and_overrides_model() {
        let selected = vec![provider_with_id("current"), provider_with_id("backup")];
        let cfg = build_config(&[("claude", "opus", route("zhipu", "glm-5.2"))]);
        let out = apply_tier_routing("claude-opus-4-8[1m]", "claude", &cfg, &selected, |id| {
            Some(provider_with_id(id))
        });
        assert_eq!(out.providers[0].id, "zhipu");
        assert_eq!(out.providers.len(), 3, "目标 provider 加入链首，原链保留");
        assert_eq!(out.routed_provider_id.as_deref(), Some("zhipu"));
        assert_eq!(out.model_override.as_deref(), Some("glm-5.2"));
    }

    #[test]
    fn tier_routing_dedups_when_target_already_in_chain() {
        let selected = vec![provider_with_id("a"), provider_with_id("zhipu")];
        let cfg = build_config(&[("claude", "opus", route("zhipu", "glm-5.2"))]);
        let out = apply_tier_routing("claude-opus-4-8", "claude", &cfg, &selected, |id| {
            Some(provider_with_id(id))
        });
        assert_eq!(out.providers[0].id, "zhipu");
        assert_eq!(out.providers.len(), 2, "目标已在链中时不重复");
        assert_eq!(out.providers[1].id, "a");
    }

    #[test]
    fn tier_routing_no_route_for_tier_passes_through() {
        let selected = vec![provider_with_id("a")];
        // 只配了 opus，发 sonnet 请求不命中
        let cfg = build_config(&[("claude", "opus", route("zhipu", "glm-5.2"))]);
        let out = apply_tier_routing("claude-sonnet-4-6", "claude", &cfg, &selected, |id| {
            Some(provider_with_id(id))
        });
        assert_eq!(out.providers[0].id, "a");
        assert!(out.routed_provider_id.is_none());
        assert!(out.model_override.is_none());
    }

    #[test]
    fn tier_routing_unknown_provider_falls_back() {
        let selected = vec![provider_with_id("a")];
        let cfg = build_config(&[("claude", "opus", route("ghost", "glm-5.2"))]);
        // lookup 对 "ghost" 返回 None（Provider 被删除）
        let out = apply_tier_routing("claude-opus-4-8", "claude", &cfg, &selected, |id| {
            if id == "a" {
                Some(provider_with_id(id))
            } else {
                None
            }
        });
        assert_eq!(out.providers[0].id, "a");
        assert!(out.routed_provider_id.is_none());
        assert!(out.model_override.is_none());
    }

    #[test]
    fn tier_routing_skips_when_app_has_no_routes() {
        // claude 配了路由，但请求是 codex app（app_type 不匹配）
        let selected = vec![provider_with_id("a")];
        let cfg = build_config(&[("claude", "opus", route("zhipu", "glm-5.2"))]);
        let out = apply_tier_routing("claude-opus-4-8", "codex", &cfg, &selected, |id| {
            Some(provider_with_id(id))
        });
        assert_eq!(out.providers[0].id, "a");
        assert!(out.routed_provider_id.is_none());
    }

    #[test]
    fn tier_routing_respects_per_app_enabled_for_claude_desktop() {
        let selected = vec![provider_with_id("current")];
        let mut cfg = build_config(&[("claude-desktop", "opus", route("zhipu", "glm-5.2"))]);
        let disabled_out =
            apply_tier_routing("claude-opus-4-8", "claude-desktop", &cfg, &selected, |id| {
                Some(provider_with_id(id))
            });
        assert_eq!(disabled_out.providers[0].id, "current");
        assert!(disabled_out.routed_provider_id.is_none());

        cfg.enabled_apps.insert("claude-desktop".to_string(), true);
        let enabled_out =
            apply_tier_routing("claude-opus-4-8", "claude-desktop", &cfg, &selected, |id| {
                Some(provider_with_id(id))
            });
        assert_eq!(enabled_out.providers[0].id, "zhipu");
        assert_eq!(enabled_out.model_override.as_deref(), Some("glm-5.2"));
    }

    #[test]
    fn tier_routing_empty_model_is_ignored() {
        let selected = vec![provider_with_id("a")];
        let cfg = build_config(&[("claude", "opus", route("zhipu", ""))]);
        let out = apply_tier_routing("claude-opus-4-8", "claude", &cfg, &selected, |id| {
            Some(provider_with_id(id))
        });
        assert_eq!(out.providers[0].id, "a");
        assert!(out.routed_provider_id.is_none());
    }

    #[test]
    fn tier_routing_skips_non_routable_provider() {
        // 脏数据/旧配置：opus 路由项指向官方 provider（category == "official"）。
        // 前端下拉已过滤掉官方项，但若配置已存在，后端执行点必须同样跳过——
        // 否则请求会被改写 model 并送进代理无法转发的官方上游。
        let selected = vec![provider_with_id("current")];
        let cfg = build_config(&[("claude", "opus", route("official-acct", "claude-opus-4-8"))]);
        let out = apply_tier_routing("claude-opus-4-8", "claude", &cfg, &selected, |id| {
            if id == "official-acct" {
                Some(provider_with_category("official-acct", "official"))
            } else {
                Some(provider_with_id(id))
            }
        });
        // 回退默认选择：官方 provider 不进链首、不触发 model 覆写
        assert_eq!(out.providers[0].id, "current");
        assert!(out.routed_provider_id.is_none());
        assert!(out.model_override.is_none());
    }
}
