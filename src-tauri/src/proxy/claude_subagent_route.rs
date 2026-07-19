//! Claude Code 子代理跨供应商路由
//!
//! Claude Code 无法按子代理选择不同 base URL，因此在 CC Switch 本地代理接管模式下：
//! 1. 向 live `CLAUDE_CODE_SUBAGENT_MODEL` 写入保留别名，使子代理请求可被识别；
//! 2. `/v1/messages` 收到该别名后，按当前供应商 meta 中的路由解析目标供应商；
//! 3. 将请求 model 改写为目标供应商自身的 `CLAUDE_CODE_SUBAGENT_MODEL` 后仅转发到该目标。
//!
//! 同供应商/无路由路径必须保持与既有行为兼容；路由不得触发全局热切换。
//!
//! ## Effective settings / common config
//!
//! 代理请求路径（含跨供应商子代理路由）始终使用数据库中的 `Provider.settings_config`
//! 作为目标供应商配置，与 `ProviderRouter::select_providers` 返回的对象一致。
//! Claude common config 片段仅在写入 live 配置文件时通过
//! `build_effective_settings_with_common_config` 合并，不会在请求路由阶段二次合并。
//! 路由 meta 只存 `providerId`，从不复制目标凭证。

use crate::provider::Provider;
#[cfg(test)]
use crate::provider::ProviderMeta;
use crate::proxy::model_mapper::strip_one_m_suffix_for_upstream;
use serde_json::Value;

/// 写入 live 配置的保留子代理路由别名（CC Switch 自有，不应对上游有意义）。
pub const CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS: &str = "cc-switch-subagent-route";

/// 解析结果：仅当请求 model 为保留别名且路由合法时返回。
#[derive(Debug, Clone)]
pub struct ResolvedClaudeSubagentRoute {
    /// 目标供应商（完整配置，含认证/格式/transforms）
    pub target_provider: Provider,
    /// 目标供应商配置的子代理模型（可能含 `[1M]`）
    pub target_subagent_model: String,
}

/// 判断请求模型是否为跨供应商子代理路由保留别名（忽略 `[1M]` 后缀）。
pub fn is_claude_subagent_route_alias(model: &str) -> bool {
    strip_one_m_suffix_for_upstream(model).eq_ignore_ascii_case(CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS)
}

/// 从供应商 meta 读取跨供应商子代理路由目标 ID。
///
/// - 无 meta / 无字段 / 空 id → None
/// - providerId 与自身相同 → None（同供应商视为默认）
pub fn foreign_subagent_route_provider_id(provider: &Provider) -> Option<&str> {
    let route = provider.meta.as_ref()?.claude_subagent_route.as_ref()?;
    let target_id = route.provider_id.trim();
    if target_id.is_empty() || target_id == provider.id {
        return None;
    }
    Some(target_id)
}

/// 当前供应商在接管模式下是否应把 live 子代理模型写成保留别名。
pub fn should_write_subagent_route_alias(provider: &Provider) -> bool {
    foreign_subagent_route_provider_id(provider).is_some()
}

/// 从供应商 settings 读取 `CLAUDE_CODE_SUBAGENT_MODEL`。
pub fn provider_subagent_model(provider: &Provider) -> Option<&str> {
    provider
        .settings_config
        .get("env")
        .and_then(|env| env.get("CLAUDE_CODE_SUBAGENT_MODEL"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// 根据 active 供应商的路由 meta 解析目标供应商与子代理模型。
///
/// 失败时返回明确错误字符串（不静默回退到当前供应商）。
///
/// 仅在调用方已确认请求 model 为保留别名 **且** active 供应商存在合法
/// foreign-route meta 时调用。保留别名本身不足以触发路由：用户可能把
/// `cc-switch-subagent-route` 当作普通子代理模型名使用。
pub fn resolve_subagent_route(
    active_provider: &Provider,
    load_target: impl FnOnce(&str) -> Result<Option<Provider>, String>,
) -> Result<ResolvedClaudeSubagentRoute, String> {
    let target_id = foreign_subagent_route_provider_id(active_provider).ok_or_else(|| {
        "Claude subagent cross-provider route is not configured on the active provider".to_string()
    })?;

    let target_provider = load_target(target_id)?.ok_or_else(|| {
        format!("Claude subagent route target provider '{target_id}' was not found")
    })?;

    if target_provider.id == active_provider.id {
        return Err("Claude subagent route target must be a different provider".to_string());
    }

    let target_subagent_model = provider_subagent_model(&target_provider)
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "Claude subagent route target provider '{}' has no CLAUDE_CODE_SUBAGENT_MODEL configured",
                target_provider.name
            )
        })?;

    // 目标真实子代理模型不得再是保留别名，否则无法区分“路由别名”与“上游模型名”。
    if is_claude_subagent_route_alias(&target_subagent_model) {
        return Err(format!(
            "Claude subagent route target provider '{}' configures CLAUDE_CODE_SUBAGENT_MODEL as the reserved alias '{}'; use a real upstream model name instead",
            target_provider.name, CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS
        ));
    }

    Ok(ResolvedClaudeSubagentRoute {
        target_provider,
        target_subagent_model,
    })
}

/// 构造仅含目标供应商的列表（不继承 active 故障转移队列）。
pub fn pin_providers_to_target(target: Provider) -> Vec<Provider> {
    vec![target]
}

/// 将请求体 model 改写为目标子代理模型。
pub fn rewrite_request_model_to_target(body: &mut Value, target_subagent_model: &str) {
    if let Some(obj) = body.as_object_mut() {
        obj.insert(
            "model".to_string(),
            Value::String(target_subagent_model.to_string()),
        );
    }
}

/// 便于测试：从 meta 片段构造带路由的 Provider。
#[cfg(test)]
pub fn provider_with_route(
    id: &str,
    name: &str,
    settings: Value,
    route_to: Option<&str>,
) -> Provider {
    let mut provider = Provider::with_id(id.to_string(), name.to_string(), settings, None);
    if let Some(target) = route_to {
        provider.meta = Some(ProviderMeta {
            claude_subagent_route: Some(crate::provider::ClaudeSubagentRoute {
                provider_id: target.to_string(),
            }),
            ..ProviderMeta::default()
        });
    }
    provider
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn alias_detection_ignores_one_m_suffix_and_case() {
        assert!(is_claude_subagent_route_alias(
            CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS
        ));
        assert!(is_claude_subagent_route_alias(
            "cc-switch-subagent-route[1M]"
        ));
        assert!(is_claude_subagent_route_alias("CC-Switch-Subagent-Route"));
        assert!(!is_claude_subagent_route_alias("claude-sonnet-4-6"));
        assert!(!is_claude_subagent_route_alias("deepseek-v4-pro[1M]"));
    }

    #[test]
    fn foreign_route_ignores_empty_and_self() {
        let self_route = provider_with_route("a", "A", json!({"env": {}}), Some("a"));
        assert_eq!(foreign_subagent_route_provider_id(&self_route), None);
        assert!(!should_write_subagent_route_alias(&self_route));

        let empty = provider_with_route("a", "A", json!({"env": {}}), Some("  "));
        assert_eq!(foreign_subagent_route_provider_id(&empty), None);

        let foreign = provider_with_route("a", "A", json!({"env": {}}), Some("b"));
        assert_eq!(foreign_subagent_route_provider_id(&foreign), Some("b"));
        assert!(should_write_subagent_route_alias(&foreign));
    }

    #[test]
    fn resolve_success_uses_target_subagent_model_with_one_m() {
        let active = provider_with_route(
            "active",
            "Active",
            json!({"env": {"CLAUDE_CODE_SUBAGENT_MODEL": "active-sub"}}),
            Some("target"),
        );
        let target = Provider::with_id(
            "target".to_string(),
            "Target".to_string(),
            json!({"env": {"CLAUDE_CODE_SUBAGENT_MODEL": "target-sub[1M]"}}),
            None,
        );

        let resolved = resolve_subagent_route(&active, |_| Ok(Some(target.clone())))
            .expect("route should resolve");
        assert_eq!(resolved.target_provider.id, "target");
        assert_eq!(resolved.target_subagent_model, "target-sub[1M]");
        assert_eq!(pin_providers_to_target(resolved.target_provider).len(), 1);
    }

    #[test]
    fn resolve_fails_when_target_missing() {
        let active = provider_with_route("a", "A", json!({"env": {}}), Some("missing"));
        let err = resolve_subagent_route(&active, |_| Ok(None)).unwrap_err();
        assert!(err.contains("was not found"), "{err}");
    }

    #[test]
    fn resolve_fails_when_target_has_no_subagent_model() {
        let active = provider_with_route("a", "A", json!({"env": {}}), Some("b"));
        let target = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({"env": {"ANTHROPIC_MODEL": "only-main"}}),
            None,
        );
        let err = resolve_subagent_route(&active, |_| Ok(Some(target))).unwrap_err();
        assert!(err.contains("CLAUDE_CODE_SUBAGENT_MODEL"), "{err}");
    }

    #[test]
    fn resolve_fails_when_no_route_configured() {
        let active = Provider::with_id("a".to_string(), "A".to_string(), json!({"env": {}}), None);
        let err = resolve_subagent_route(&active, |_| Ok(None)).unwrap_err();
        assert!(err.contains("not configured"), "{err}");
    }

    #[test]
    fn resolve_rejects_target_whose_subagent_model_is_reserved_alias() {
        let active = provider_with_route("a", "A", json!({"env": {}}), Some("b"));
        let target = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({"env": {"CLAUDE_CODE_SUBAGENT_MODEL": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS}}),
            None,
        );
        let err = resolve_subagent_route(&active, |_| Ok(Some(target))).unwrap_err();
        assert!(err.contains("reserved alias"), "{err}");
        assert!(err.contains(CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS), "{err}");
    }

    #[test]
    fn resolve_rejects_target_whose_subagent_model_is_reserved_alias_with_one_m() {
        let active = provider_with_route("a", "A", json!({"env": {}}), Some("b"));
        let target = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({"env": {"CLAUDE_CODE_SUBAGENT_MODEL": "cc-switch-subagent-route[1M]"}}),
            None,
        );
        let err = resolve_subagent_route(&active, |_| Ok(Some(target))).unwrap_err();
        assert!(err.contains("reserved alias"), "{err}");
    }

    #[test]
    fn rewrite_request_model_preserves_other_fields() {
        let mut body = json!({
            "model": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS,
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}]
        });
        rewrite_request_model_to_target(&mut body, "target-sub[1M]");
        assert_eq!(body["model"], "target-sub[1M]");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["content"], "hi");
    }

    #[test]
    fn route_meta_serializes_provider_id_only() {
        let meta = ProviderMeta {
            claude_subagent_route: Some(crate::provider::ClaudeSubagentRoute {
                provider_id: "target-1".to_string(),
            }),
            ..ProviderMeta::default()
        };
        let value = serde_json::to_value(&meta).expect("serialize");
        assert_eq!(
            value
                .get("claudeSubagentRoute")
                .and_then(|v| v.get("providerId")),
            Some(&json!("target-1"))
        );
        // 不得包含任何密钥字段
        let route = value.get("claudeSubagentRoute").unwrap();
        assert!(route.get("apiKey").is_none());
        assert!(route.get("token").is_none());
    }

    #[test]
    fn routed_target_uses_raw_provider_settings_not_live_common_config_merge() {
        // Proxy request path always uses Provider.settings_config from DB —
        // identical to what select_providers returns. Claude common config is
        // applied only when writing live settings files
        // (build_effective_settings_with_common_config), not during request routing.
        let active = provider_with_route("active", "Active", json!({"env": {}}), Some("target"));
        let target_settings = json!({
            "env": {
                "ANTHROPIC_API_KEY": "target-secret-key",
                "ANTHROPIC_BASE_URL": "https://api.target.example",
                "CLAUDE_CODE_SUBAGENT_MODEL": "target-sub"
            }
        });
        let target = Provider::with_id(
            "target".to_string(),
            "Target".to_string(),
            target_settings.clone(),
            None,
        );

        let resolved =
            resolve_subagent_route(&active, |_| Ok(Some(target.clone()))).expect("resolve");

        assert_eq!(resolved.target_provider.settings_config, target_settings);
        assert_eq!(
            resolved.target_provider.settings_config["env"]["ANTHROPIC_API_KEY"],
            "target-secret-key"
        );
        // Route meta on active must never carry target credentials
        let active_meta = serde_json::to_value(&active.meta).expect("serialize");
        let active_text = active_meta.to_string();
        assert!(!active_text.contains("target-secret-key"));
        assert!(!active_text.contains("api.target.example"));
    }
}
