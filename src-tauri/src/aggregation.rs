//! 供应商聚合（Provider Aggregation）
//!
//! 「聚合供应商」是一种特殊的 Claude 应用供应商：它内部聚合多条上游
//! （每条含独立的 base_url / api_key / api 格式），并按请求体中的模型名把请求
//! 路由到对应上游。配置存放在 `provider.settings_config.aggregation` 中，
//! 并以 `meta.provider_type = "aggregation"` 标记。
//!
//! 运行时（代理）按请求模型名把聚合供应商解析成一个「等价的普通 Claude 供应商」
//! （合成 provider）：填入所选上游的 base_url / 凭据 / api 格式；之后完全复用现有的
//! 转发 / 格式转换 / 鉴权链路——无需改动 forwarder / adapter。

use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta};
use serde::Deserialize;
use serde_json::{json, Value};

/// `meta.provider_type` 标记值
pub const AGGREGATION_PROVIDER_TYPE: &str = "aggregation";

/// 单条上游
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregationUpstream {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    /// anthropic | openai_chat | openai_responses | gemini_native
    #[serde(default)]
    pub api_format: Option<String>,
    #[serde(default)]
    pub is_full_url: Option<bool>,
    /// 认证字段名（ANTHROPIC_AUTH_TOKEN / ANTHROPIC_API_KEY）；默认 auth token
    #[serde(default)]
    pub api_key_field: Option<String>,
}

/// 单条模型路由：客户端模型名 → 某条上游（可选改写上游模型名）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregationRoute {
    /// 匹配模式：精确（gpt-4o）/ 前缀通配（gpt-*）/ 全兜底（*），大小写不敏感
    pub model: String,
    pub upstream_id: String,
    #[serde(default)]
    pub upstream_model: Option<String>,
}

/// 聚合配置（存于 settings_config.aggregation）
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AggregationConfig {
    #[serde(default)]
    pub upstreams: Vec<AggregationUpstream>,
    #[serde(default)]
    pub routes: Vec<AggregationRoute>,
}

impl AggregationConfig {
    /// 从 provider.settings_config.aggregation 解析（无上游时返回 None）
    pub fn from_provider(provider: &Provider) -> Option<Self> {
        let raw = provider.settings_config.get("aggregation")?;
        let cfg: AggregationConfig = serde_json::from_value(raw.clone()).ok()?;
        if cfg.upstreams.is_empty() {
            return None;
        }
        Some(cfg)
    }
}

/// provider 是否为聚合供应商
pub fn is_aggregation_provider(provider: &Provider) -> bool {
    let flagged = provider
        .meta
        .as_ref()
        .and_then(|m| m.provider_type.as_deref())
        == Some(AGGREGATION_PROVIDER_TYPE);
    flagged || provider.settings_config.get("aggregation").is_some()
}

/// 匹配特异度：精确=usize::MAX，前缀通配（foo*）=前缀长度，全兜底（*）=0，不匹配=None
fn match_specificity(pattern: &str, model: &str) -> Option<usize> {
    let p = pattern.trim().to_lowercase();
    let m = model.trim().to_lowercase();
    if p == "*" {
        return Some(0);
    }
    if let Some(prefix) = p.strip_suffix('*') {
        return m.starts_with(prefix).then_some(prefix.len());
    }
    (p == m).then_some(usize::MAX)
}

/// 把聚合供应商按请求模型名解析成一个「等价的普通 Claude 供应商」。
///
/// - 非聚合供应商 → `Ok(None)`（调用方保持原 provider）
/// - 命中路由 → `Ok(Some(synthetic))`
/// - 聚合但无匹配路由 / 上游缺失或未配 base_url → `Err`
pub fn resolve_aggregation_upstream(
    provider: &Provider,
    model: &str,
) -> Result<Option<Provider>, AppError> {
    if !is_aggregation_provider(provider) {
        return Ok(None);
    }
    let cfg = AggregationConfig::from_provider(provider)
        .ok_or_else(|| AppError::Config("聚合供应商未配置任何上游".to_string()))?;

    // 去掉本地 [1m] 上下文标记后再匹配
    let normalized = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model);

    // 选特异度最高的路由；同特异度取配置更靠前的（routes 顺序即优先级）
    let best = cfg
        .routes
        .iter()
        .filter_map(|r| match_specificity(&r.model, normalized).map(|s| (s, r)))
        .fold(
            None::<(usize, &AggregationRoute)>,
            |acc, (s, r)| match acc {
                Some((bs, _)) if bs >= s => acc,
                _ => Some((s, r)),
            },
        );

    let Some((_, route)) = best else {
        return Err(AppError::Config(format!(
            "聚合供应商未配置模型 '{model}' 的路由"
        )));
    };

    let upstream = cfg
        .upstreams
        .iter()
        .find(|u| u.id == route.upstream_id)
        .ok_or_else(|| {
            AppError::Config(format!("聚合路由指向不存在的上游: {}", route.upstream_id))
        })?;

    if upstream.base_url.trim().is_empty() {
        let label = upstream.name.clone().unwrap_or_else(|| upstream.id.clone());
        return Err(AppError::Config(format!(
            "聚合上游 '{label}' 未配置 base_url"
        )));
    }

    // 构造等价的普通 Claude 供应商：
    // - env.ANTHROPIC_BASE_URL / 认证字段 = 上游凭据
    // - env.ANTHROPIC_MODEL = upstream_model（如配置，交给 model_mapper 改写模型名）
    // - meta.api_format / is_full_url = 上游格式（现有 Claude adapter 会据此选择转换与鉴权）
    //
    // id 保持与聚合供应商一致：避免代理把它当成"切换了当前供应商"而触发持久切换。
    let key_field = upstream
        .api_key_field
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "ANTHROPIC_AUTH_TOKEN".to_string());

    let mut env = serde_json::Map::new();
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        json!(upstream.base_url.trim().trim_end_matches('/')),
    );
    env.insert(key_field.clone(), json!(upstream.api_key));
    if let Some(um) = route
        .upstream_model
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        env.insert("ANTHROPIC_MODEL".to_string(), json!(um));
    }

    let meta = ProviderMeta {
        api_format: upstream.api_format.clone(),
        is_full_url: upstream.is_full_url,
        api_key_field: Some(key_field),
        ..Default::default()
    };

    let upstream_label = upstream
        .name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| upstream.base_url.clone());

    Ok(Some(Provider {
        id: provider.id.clone(),
        name: format!("{} · {}", provider.name, upstream_label),
        settings_config: json!({ "env": Value::Object(env) }),
        website_url: provider.website_url.clone(),
        category: provider.category.clone(),
        created_at: provider.created_at,
        sort_index: provider.sort_index,
        notes: None,
        meta: Some(meta),
        icon: provider.icon.clone(),
        icon_color: provider.icon_color.clone(),
        in_failover_queue: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn agg_provider() -> Provider {
        let mut p = Provider::with_id(
            "agg1".into(),
            "聚合".into(),
            json!({
                "aggregation": {
                    "upstreams": [
                        {"id":"u1","name":"OpenAI转发","baseUrl":"https://openai.example.com","apiKey":"sk-o","apiFormat":"openai_chat"},
                        {"id":"u2","name":"Claude转发","baseUrl":"https://claude.example.com/","apiKey":"sk-c","apiFormat":"anthropic"}
                    ],
                    "routes": [
                        {"model":"gpt-4o","upstreamId":"u1"},
                        {"model":"claude-*","upstreamId":"u2","upstreamModel":"claude-3-5-sonnet-real"},
                        {"model":"*","upstreamId":"u2"}
                    ]
                }
            }),
            None,
        );
        p.meta = Some(ProviderMeta {
            provider_type: Some(AGGREGATION_PROVIDER_TYPE.to_string()),
            ..Default::default()
        });
        p
    }

    fn env_of<'a>(p: &'a Provider, k: &str) -> Option<&'a str> {
        p.settings_config.get("env")?.get(k)?.as_str()
    }

    #[test]
    fn non_aggregation_returns_none() {
        let p = Provider::with_id("p".into(), "n".into(), json!({"env":{}}), None);
        assert!(resolve_aggregation_upstream(&p, "gpt-4o")
            .unwrap()
            .is_none());
    }

    #[test]
    fn exact_route_resolves_to_upstream() {
        let s = resolve_aggregation_upstream(&agg_provider(), "gpt-4o")
            .unwrap()
            .expect("resolved");
        assert_eq!(
            env_of(&s, "ANTHROPIC_BASE_URL"),
            Some("https://openai.example.com")
        );
        assert_eq!(env_of(&s, "ANTHROPIC_AUTH_TOKEN"), Some("sk-o"));
        assert_eq!(
            s.meta.as_ref().unwrap().api_format.as_deref(),
            Some("openai_chat")
        );
        assert_eq!(s.id, "agg1"); // 保持同 id，避免触发切换
    }

    #[test]
    fn prefix_route_and_upstream_model_rewrite() {
        let s = resolve_aggregation_upstream(&agg_provider(), "claude-sonnet-4-5")
            .unwrap()
            .unwrap();
        assert_eq!(
            env_of(&s, "ANTHROPIC_BASE_URL"),
            Some("https://claude.example.com")
        );
        assert_eq!(
            s.meta.as_ref().unwrap().api_format.as_deref(),
            Some("anthropic")
        );
        // upstream_model 通过 env.ANTHROPIC_MODEL 交给 model_mapper 改写
        assert_eq!(
            env_of(&s, "ANTHROPIC_MODEL"),
            Some("claude-3-5-sonnet-real")
        );
    }

    #[test]
    fn one_m_suffix_is_stripped_before_match() {
        let s = resolve_aggregation_upstream(&agg_provider(), "gpt-4o[1m]")
            .unwrap()
            .unwrap();
        assert_eq!(
            env_of(&s, "ANTHROPIC_BASE_URL"),
            Some("https://openai.example.com")
        );
    }

    #[test]
    fn wildcard_fallback_used_when_no_specific_match() {
        // "random-model" 不匹配 gpt-4o / claude-* → 命中 * 兜底 → u2
        let s = resolve_aggregation_upstream(&agg_provider(), "random-model")
            .unwrap()
            .unwrap();
        assert_eq!(
            env_of(&s, "ANTHROPIC_BASE_URL"),
            Some("https://claude.example.com")
        );
    }

    #[test]
    fn no_matching_route_errors() {
        // 去掉兜底路由后，未知模型应报错
        let mut p = agg_provider();
        p.settings_config = json!({
            "aggregation": {
                "upstreams": [{"id":"u1","baseUrl":"https://a.example.com","apiKey":"k","apiFormat":"anthropic"}],
                "routes": [{"model":"gpt-4o","upstreamId":"u1"}]
            }
        });
        let err = resolve_aggregation_upstream(&p, "claude-sonnet").unwrap_err();
        assert!(err.to_string().contains("未配置模型"));
    }
}
