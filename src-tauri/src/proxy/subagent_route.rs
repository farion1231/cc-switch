//! Subagent 跨供应商路由决策
//!
//! 接管模式下，模型名匹配 subagent 角色的请求改道到目标供应商（B），
//! 候选列表变为 [B, 其余]，失败转移、熔断等机制复用现有转发循环。
//! 决策为纯函数，便于单测。设计见
//! docs/superpowers/specs/2026-09-06-subagent-cross-provider-routing-design.md §5

use crate::app_config::AppType;
use crate::provider::Provider;
use crate::proxy::model_mapper::strip_one_m_suffix_for_upstream;
use crate::proxy::types::SubagentRoute;

/// 决策结果
pub struct SubagentRoutePlan {
    /// 改道后的候选列表（B 在首位，其余保持原顺序）
    pub providers: Vec<Provider>,
    /// 非空时 handler 在转发前把 body.model 覆盖为该值
    pub model_override: Option<String>,
}

/// 生效的 subagent 模型名（识别基准，与接管 live 注入同源，spec §5.1）：
/// 供应商显式设置的 CLAUDE_CODE_SUBAGENT_MODEL 优先，否则取规则中的 model。
pub fn effective_subagent_model(
    current: &Provider,
    route: Option<&SubagentRoute>,
) -> Option<String> {
    let env_value = current
        .settings_config
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_SUBAGENT_MODEL"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    env_value.or_else(|| {
        route
            .and_then(|r| r.model.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    })
}

/// 按 spec §5 计算 subagent 改道。
///
/// 前置条件任一不满足即原样返回：非 claude 应用、规则关闭、识别基准缺失、
/// 请求模型不匹配、目标等于当前、目标不存在。
pub fn plan_subagent_route(
    providers: Vec<Provider>,
    app_type: &AppType,
    route: Option<&SubagentRoute>,
    request_model: &str,
    resolve_provider: impl Fn(&str) -> Option<Provider>,
) -> SubagentRoutePlan {
    let unchanged = SubagentRoutePlan {
        providers,
        model_override: None,
    };
    if *app_type != AppType::Claude {
        return unchanged;
    }
    let Some(route) = route else {
        return unchanged;
    };
    let Some(current) = unchanged.providers.first() else {
        return unchanged;
    };
    let Some(baseline) = effective_subagent_model(current, Some(route)) else {
        return unchanged;
    };
    if strip_one_m_suffix_for_upstream(request_model) != strip_one_m_suffix_for_upstream(&baseline)
    {
        return unchanged;
    }
    let current_id = unchanged.providers[0].id.clone();
    if route.provider_id == current_id {
        return unchanged;
    }
    let Some(target) = resolve_provider(&route.provider_id) else {
        log::warn!(
            "[SubagentRoute] 规则指向的供应商 {} 不存在，忽略改道",
            route.provider_id
        );
        return unchanged;
    };

    let mut providers = Vec::with_capacity(unchanged.providers.len() + 1);
    providers.push(target);
    for provider in unchanged.providers {
        if providers.iter().all(|p| p.id != provider.id) {
            providers.push(provider);
        }
    }
    SubagentRoutePlan {
        model_override: route
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        providers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider_with_env(id: &str, env: serde_json::Value) -> Provider {
        Provider::with_id(id.to_string(), id.to_string(), json!({"env": env}), None)
    }

    fn route(provider_id: &str, model: Option<&str>) -> SubagentRoute {
        SubagentRoute {
            provider_id: provider_id.to_string(),
            model: model.map(String::from),
        }
    }

    fn resolve_exists(id: &str) -> Option<Provider> {
        Some(provider_with_env(id, json!({})))
    }

    #[test]
    fn reroutes_matching_subagent_request_to_target() {
        let a = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}));
        let providers = vec![a];
        let plan = plan_subagent_route(
            providers,
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(
            plan.providers
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(plan.model_override, Some("glm-5.5-flash".to_string()));
    }

    #[test]
    fn baseline_falls_back_to_route_model_without_explicit_env() {
        // 供应商未显式设置 subagent env，识别基准取规则 model（与接管注入同源）
        let a = provider_with_env("a", json!({}));
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "b");
        assert_eq!(plan.model_override, Some("glm-5.5-flash".to_string()));
    }

    #[test]
    fn explicit_env_wins_over_route_model_as_baseline() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "my-subagent-alias"}),
        );
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "my-subagent-alias",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "b");
        // 覆盖值仍取规则 model
        assert_eq!(plan.model_override, Some("glm-5.5-flash".to_string()));
    }

    #[test]
    fn non_matching_request_is_unchanged() {
        let a = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}));
        let original = vec![a];
        let plan = plan_subagent_route(
            original.clone(),
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "claude-sonnet-4-5",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn one_m_suffix_is_normalized_for_matching() {
        let a = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}));
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("b", None)),
            "glm-5.5-flash[1M]",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "b");
        // 规则无 model → 透传请求中的模型名
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn disabled_route_is_unchanged() {
        let a = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}));
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            None,
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn non_claude_app_short_circuits() {
        let a = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}));
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Codex,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn target_equals_current_is_noop() {
        let a = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}));
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("a", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(plan.providers.len(), 1);
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn missing_target_falls_back_to_current() {
        let a = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}));
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("deleted", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            |_| None,
        );
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn target_in_failover_chain_is_deduplicated() {
        let a = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}));
        let b = provider_with_env("b", json!({}));
        let c = provider_with_env("c", json!({}));
        // 故障转移叠加：候选队列 [c, b, a]，规则目标 b 不在首位但已在链中，
        // 改道后应提到首位且去重（不重复入列）。
        let plan = plan_subagent_route(
            vec![c, b, a],
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        let ids = plan
            .providers
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["b", "c", "a"]);
        assert_eq!(ids.iter().filter(|id| **id == "b").count(), 1);
    }

    #[test]
    fn empty_providers_is_unchanged() {
        let plan = plan_subagent_route(
            Vec::new(),
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert!(plan.providers.is_empty());
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn effective_subagent_model_prefers_explicit_env() {
        let a = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "  my-alias  "}));
        assert_eq!(
            effective_subagent_model(&a, Some(&route("b", Some("other")))),
            Some("my-alias".to_string())
        );
        let blank = provider_with_env("a", json!({"CLAUDE_CODE_SUBAGENT_MODEL": "   "}));
        assert_eq!(
            effective_subagent_model(&blank, Some(&route("b", Some("glm-5.5-flash")))),
            Some("glm-5.5-flash".to_string())
        );
    }
}
