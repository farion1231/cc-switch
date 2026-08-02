//! Claude 订阅透传（subscription passthrough）
//!
//! 供应商开启该开关后，**未填模型 ID 的模型角色**（即模型映射解析不到的
//! 带档位请求）不再发给该供应商，而是转发到官方上游，并保留客户端自带的
//! `Authorization` —— CC Switch 全程不经手、不存储任何 Claude 订阅凭据
//! （与 Codex 官方供应商放行客户端自带 ChatGPT authorization 的既有做法一致）。
//!
//! 判定口径：
//! - 请求模型带档位关键字（fable / opus / sonnet / haiku）才可能透传；
//!   自定义模型名（如手动 `/model` 指定的第三方模型）照常发给供应商；
//! - 档位自身的键或默认兜底模型（`ANTHROPIC_MODEL`）配置了即走供应商；
//!   fable→opus 的降级**不算已配置**——订阅有真实 fable 档，优于降级。

use crate::provider::Provider;
use crate::proxy::model_mapper::ModelMapping;

/// 透传解析出的合成供应商 id。
///
/// 该 id 不落库，只在单次请求的转发链路内存在；熔断器会按
/// `"{app_type}:{该 id}"` 为透传通道独立记账——订阅额度耗尽
/// 不应拉闸用户选中的真实供应商。
pub const PASSTHROUGH_PROVIDER_ID: &str = "__claude_subscription_passthrough__";

/// 判断是否为订阅透传的合成供应商。
pub fn is_passthrough_provider(provider: &Provider) -> bool {
    provider.id == PASSTHROUGH_PROVIDER_ID
}

/// 构造「Claude 订阅直连」合成供应商：指向官方上游、不含任何凭据。
///
/// 仅用于 Claude Code——已实测客户端会把自带的订阅 OAuth token 发给
/// 自定义 base URL，代理只需原样放行 `Authorization`。
fn official_subscription_provider() -> Provider {
    Provider::with_id(
        PASSTHROUGH_PROVIDER_ID.to_string(),
        "Claude 订阅直连".to_string(),
        serde_json::json!({
            "env": { "ANTHROPIC_BASE_URL": "https://api.anthropic.com" }
        }),
        None,
    )
}

/// 请求模型是否带 Claude 档位关键字。
fn has_tier_keyword(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    ["fable", "opus", "sonnet", "haiku"]
        .iter()
        .any(|tier| lower.contains(tier))
}

/// 订阅透传判定 + 合成。
///
/// 供应商开启了订阅透传、请求模型带档位关键字、且该模型在供应商的模型
/// 映射中解析不到（= 对应角色未填模型 ID）时，返回「Claude 订阅直连」
/// 合成供应商；其余情况返回 `None`，照常走供应商自身。
///
/// 没有 `model` 字段的请求无从判定角色，一律照常走供应商。
pub fn resolve_subscription_passthrough(
    provider: &Provider,
    model: Option<&str>,
) -> Option<Provider> {
    if !provider.claude_subscription_passthrough_enabled() {
        return None;
    }
    let model = model.map(str::trim).filter(|m| !m.is_empty())?;
    if !has_tier_keyword(model) {
        return None;
    }
    if ModelMapping::from_provider(provider)
        .resolve(model)
        .is_some()
    {
        return None;
    }
    Some(official_subscription_provider())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use serde_json::json;

    fn passthrough_provider_with_env(env: serde_json::Value) -> Provider {
        let mut provider = Provider::with_id(
            "prov".to_string(),
            "Prov".to_string(),
            json!({ "env": env }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            claude_subscription_passthrough: Some(true),
            ..Default::default()
        });
        provider
    }

    #[test]
    fn disabled_toggle_never_resolves() {
        let mut provider = passthrough_provider_with_env(json!({}));
        provider.meta = None;
        assert!(resolve_subscription_passthrough(&provider, Some("claude-fable-5")).is_none());
    }

    #[test]
    fn unmapped_tier_resolves_to_official_subscription() {
        let provider = passthrough_provider_with_env(json!({
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-4.7"
        }));

        // sonnet 档已填 → 走供应商
        assert!(resolve_subscription_passthrough(&provider, Some("claude-sonnet-4-6")).is_none());

        // opus 档未填 → 订阅直连（带 [1M] 修饰的模型名同样识别）
        let resolved =
            resolve_subscription_passthrough(&provider, Some("claude-opus-4-8")).expect("resolve");
        assert!(is_passthrough_provider(&resolved));
        assert_eq!(
            resolved
                .settings_config
                .pointer("/env/ANTHROPIC_BASE_URL")
                .and_then(|v| v.as_str()),
            Some("https://api.anthropic.com")
        );
        assert!(resolve_subscription_passthrough(&provider, Some("claude-fable-5[1m]")).is_some());
    }

    #[test]
    fn default_model_covers_all_tiers() {
        let provider = passthrough_provider_with_env(json!({
            "ANTHROPIC_MODEL": "default-model"
        }));
        // 默认兜底模型对所有档位兜底 → 不透传
        assert!(resolve_subscription_passthrough(&provider, Some("claude-opus-4-8")).is_none());
        assert!(resolve_subscription_passthrough(&provider, Some("claude-fable-5")).is_none());
    }

    #[test]
    fn fable_degrade_does_not_count_as_configured() {
        let provider = passthrough_provider_with_env(json!({
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped"
        }));
        // opus 已填 → 走供应商；fable 未填 → 不降级，透传真实订阅档
        assert!(resolve_subscription_passthrough(&provider, Some("claude-opus-4-8")).is_none());
        assert!(resolve_subscription_passthrough(&provider, Some("claude-fable-5")).is_some());
    }

    #[test]
    fn non_tier_models_and_missing_model_stay_on_provider() {
        let provider = passthrough_provider_with_env(json!({}));
        // 自定义模型名（无档位关键字）与缺失 model 的请求照常走供应商
        assert!(resolve_subscription_passthrough(&provider, Some("deepseek-v4-pro")).is_none());
        assert!(resolve_subscription_passthrough(&provider, None).is_none());
        assert!(resolve_subscription_passthrough(&provider, Some("   ")).is_none());
        // 全部角色未填 + 档位请求 → 全量透传
        assert!(resolve_subscription_passthrough(&provider, Some("claude-haiku-4-5")).is_some());
    }

    #[test]
    fn subagent_exact_match_counts_as_configured() {
        let provider = passthrough_provider_with_env(json!({
            "CLAUDE_CODE_SUBAGENT_MODEL": "claude-sonnet-4-6"
        }));
        assert!(resolve_subscription_passthrough(&provider, Some("claude-sonnet-4-6")).is_none());
        assert!(resolve_subscription_passthrough(&provider, Some("claude-opus-4-8")).is_some());
    }
}
