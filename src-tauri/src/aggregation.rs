//! 供应商聚合（Provider Aggregation）
//!
//! 「聚合供应商」是一种特殊的 Claude 应用供应商：它内部聚合多条上游
//! （每条含独立的 base_url / api_key / api 格式），并按请求体中的模型名把请求
//! 路由到对应上游。配置存放在 DB-only 的 `provider.meta.aggregation` 中，
//! 并以 `meta.provider_type = "aggregation"` 标记；旧版
//! `provider.settings_config.aggregation` 仅保留读取兼容。
//!
//! 运行时（代理）按请求模型名把聚合供应商解析成一个「等价的普通 Claude 供应商」
//! （合成 provider）：填入所选上游的 base_url / 凭据 / api 格式；之后完全复用现有的
//! 转发 / 格式转换 / 鉴权链路——无需改动 forwarder / adapter。

use crate::error::AppError;
use crate::provider::Provider;
use serde::Deserialize;
use serde_json::{json, Value};

/// `meta.provider_type` 标记值
pub const AGGREGATION_PROVIDER_TYPE: &str = "aggregation";

/// 聚合角色映射独占的 Claude 模型环境变量。
///
/// 通用配置会在供应商配置之后合并，因此聚合供应商必须在合并后按自身配置
/// 恢复这些键，避免共享片段中的旧模型映射改变聚合路由。
pub const AGGREGATION_MANAGED_MODEL_ENV_KEYS: [&str; 10] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "CLAUDE_CODE_SUBAGENT_MODEL",
];

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

/// 单个 Claude 模型角色 → 上游 + 实际请求模型。
///
/// 与「单一供应商」的 `ANTHROPIC_DEFAULT_*_MODEL` 映射同构：左侧是 Claude 角色
/// （Sonnet/Opus/Fable/Haiku/Subagent/默认），右侧是聚合提供的「上游 + 模型」。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AggregationRoleRoute {
    #[serde(default)]
    pub upstream_id: String,
    #[serde(default)]
    pub model: String,
}

impl AggregationRoleRoute {
    fn as_valid(&self) -> Option<(&str, &str)> {
        let model = self.model.trim();
        let upstream = self.upstream_id.trim();
        (!model.is_empty() && !upstream.is_empty()).then_some((model, upstream))
    }
}

/// 按 Claude 角色组织的路由（新版结构；驱动 env 生成与代理路由）
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AggregationRoles {
    #[serde(default)]
    pub sonnet: Option<AggregationRoleRoute>,
    #[serde(default)]
    pub opus: Option<AggregationRoleRoute>,
    #[serde(default)]
    pub fable: Option<AggregationRoleRoute>,
    #[serde(default)]
    pub haiku: Option<AggregationRoleRoute>,
    #[serde(default)]
    pub subagent: Option<AggregationRoleRoute>,
    /// 默认兜底：命中 ANTHROPIC_MODEL，并兜底一切未匹配模型
    #[serde(default)]
    pub default: Option<AggregationRoleRoute>,
}

/// 聚合配置（存于 meta.aggregation）
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AggregationConfig {
    #[serde(default)]
    pub upstreams: Vec<AggregationUpstream>,
    /// 按 Claude 角色的映射（新版）
    #[serde(default)]
    pub roles: Option<AggregationRoles>,
    /// 旧版自由映射（保留读取兼容）
    #[serde(default)]
    pub routes: Vec<AggregationRoute>,
}

impl AggregationConfig {
    /// 从 provider.meta.aggregation 解析；兼容旧版 settings_config.aggregation。
    pub fn from_provider(provider: &Provider) -> Option<Self> {
        let raw = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.aggregation.as_ref())
            .or_else(|| provider.settings_config.get("aggregation"))?;
        let cfg: AggregationConfig = serde_json::from_value(raw.clone()).ok()?;
        if cfg.upstreams.is_empty() {
            return None;
        }
        Some(cfg)
    }

    /// 展开为「模型名 → 上游」的路由列表：角色映射优先，其后追加旧版自由映射。
    ///
    /// 每个已配置角色生成一条精确路由（其模型名 → 其上游）；默认角色额外生成
    /// 一条 `*` 兜底路由，覆盖一切未显式配置的模型（如后台子任务）。
    fn effective_routes(&self) -> Vec<AggregationRoute> {
        let mut routes: Vec<AggregationRoute> = Vec::new();

        if let Some(roles) = &self.roles {
            let entries: [(&Option<AggregationRoleRoute>, bool); 6] = [
                (&roles.sonnet, false),
                (&roles.opus, false),
                (&roles.fable, false),
                (&roles.haiku, false),
                (&roles.subagent, false),
                (&roles.default, true),
            ];
            for (role, is_default) in entries {
                if let Some((model, upstream)) = role.as_ref().and_then(|r| r.as_valid()) {
                    routes.push(AggregationRoute {
                        model: model.to_string(),
                        upstream_id: upstream.to_string(),
                        upstream_model: None,
                    });
                    if is_default {
                        routes.push(AggregationRoute {
                            model: "*".to_string(),
                            upstream_id: upstream.to_string(),
                            // 默认角色不仅选择兜底上游，还必须把未知的客户端模型
                            // 改写为该角色配置的实际上游模型。
                            upstream_model: Some(model.to_string()),
                        });
                    }
                }
            }
        }

        // 旧版自由映射追加在后（角色映射优先级更高）
        routes.extend(self.routes.iter().cloned());
        routes
    }
}

/// provider 是否为聚合供应商
pub fn is_aggregation_provider(provider: &Provider) -> bool {
    let flagged = provider
        .meta
        .as_ref()
        .and_then(|m| m.provider_type.as_deref())
        == Some(AGGREGATION_PROVIDER_TYPE);
    flagged
        || provider
            .meta
            .as_ref()
            .and_then(|meta| meta.aggregation.as_ref())
            .is_some()
        || provider.settings_config.get("aggregation").is_some()
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

    // 展开角色映射 + 旧版自由映射为统一路由表
    let routes = cfg.effective_routes();

    // 选特异度最高的路由；同特异度取配置更靠前的（顺序即优先级）
    let best = routes
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
        .as_deref()
        .filter(|field| *field == "ANTHROPIC_API_KEY")
        .unwrap_or("ANTHROPIC_AUTH_TOKEN")
        .to_string();

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

    // 保留聚合供应商上的请求覆盖、自定义 UA 等代理元数据，但移除聚合标记，
    // 避免合成供应商被再次识别为聚合供应商。
    let mut meta = provider.meta.clone().unwrap_or_default();
    meta.provider_type = None;
    meta.aggregation = None;
    meta.api_format = upstream.api_format.clone();
    meta.is_full_url = upstream.is_full_url;
    meta.api_key_field = Some(key_field);

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
        notes: provider.notes.clone(),
        meta: Some(meta),
        icon: provider.icon.clone(),
        icon_color: provider.icon_color.clone(),
        in_failover_queue: provider.in_failover_queue,
    }))
}

/// 按请求模型解析完整故障转移链中的所有聚合供应商。
///
/// 首个供应商配置错误会直接返回给客户端；后续故障转移项配置错误时跳过该项，
/// 避免一个尚未实际使用的无效备用项阻断前面的正常供应商。
pub fn resolve_aggregation_chain(
    providers: Vec<Provider>,
    model: &str,
) -> Result<Vec<Provider>, AppError> {
    let mut resolved = Vec::with_capacity(providers.len());

    for (index, provider) in providers.into_iter().enumerate() {
        match resolve_aggregation_upstream(&provider, model) {
            Ok(Some(synthetic)) => resolved.push(synthetic),
            Ok(None) => resolved.push(provider),
            Err(error) if index == 0 => return Err(error),
            Err(error) => {
                log::warn!(
                    "跳过无效的聚合故障转移供应商 '{}'：{}",
                    provider.name,
                    error
                );
            }
        }
    }

    if resolved.is_empty() {
        return Err(AppError::Config("聚合故障转移链为空".to_string()));
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use serde_json::json;

    fn agg_config() -> Value {
        json!({
            "upstreams": [
                {"id":"u1","name":"OpenAI转发","baseUrl":"https://openai.example.com","apiKey":"sk-o","apiFormat":"openai_chat"},
                {"id":"u2","name":"Claude转发","baseUrl":"https://claude.example.com/","apiKey":"sk-c","apiFormat":"anthropic"}
            ],
            "routes": [
                {"model":"gpt-4o","upstreamId":"u1"},
                {"model":"claude-*","upstreamId":"u2","upstreamModel":"claude-3-5-sonnet-real"},
                {"model":"*","upstreamId":"u2"}
            ]
        })
    }

    fn agg_provider() -> Provider {
        let mut p = Provider::with_id("agg1".into(), "聚合".into(), json!({}), None);
        p.meta = Some(ProviderMeta {
            provider_type: Some(AGGREGATION_PROVIDER_TYPE.to_string()),
            aggregation: Some(agg_config()),
            custom_user_agent: Some("aggregation-test-agent".to_string()),
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
        assert_eq!(
            s.meta.as_ref().unwrap().custom_user_agent.as_deref(),
            Some("aggregation-test-agent")
        );
        assert!(s.meta.as_ref().unwrap().aggregation.is_none());
        assert!(s.meta.as_ref().unwrap().provider_type.is_none());
        assert_eq!(s.id, "agg1"); // 保持同 id，避免触发切换
    }

    #[test]
    fn legacy_settings_config_is_still_supported() {
        let mut p = Provider::with_id(
            "legacy".into(),
            "旧聚合".into(),
            json!({"aggregation": agg_config()}),
            None,
        );
        p.meta = Some(ProviderMeta {
            provider_type: Some(AGGREGATION_PROVIDER_TYPE.to_string()),
            ..Default::default()
        });

        let resolved = resolve_aggregation_upstream(&p, "gpt-4o").unwrap().unwrap();
        assert_eq!(
            env_of(&resolved, "ANTHROPIC_BASE_URL"),
            Some("https://openai.example.com")
        );
    }

    #[test]
    fn anthropic_api_key_field_uses_x_api_key_env() {
        let mut p = agg_provider();
        p.meta.as_mut().unwrap().aggregation.as_mut().unwrap()["upstreams"][1]["apiKeyField"] =
            json!("ANTHROPIC_API_KEY");

        let resolved = resolve_aggregation_upstream(&p, "claude-sonnet-4-5")
            .unwrap()
            .unwrap();
        assert_eq!(env_of(&resolved, "ANTHROPIC_API_KEY"), Some("sk-c"));
        assert_eq!(env_of(&resolved, "ANTHROPIC_AUTH_TOKEN"), None);
    }

    #[test]
    fn aggregation_chain_preserves_and_resolves_failover_entries() {
        let normal = Provider::with_id(
            "normal".into(),
            "普通".into(),
            json!({"env":{"ANTHROPIC_BASE_URL":"https://normal.example.com"}}),
            None,
        );

        let resolved =
            resolve_aggregation_chain(vec![normal.clone(), agg_provider()], "gpt-4o").unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].id, "normal");
        assert_eq!(
            env_of(&resolved[1], "ANTHROPIC_BASE_URL"),
            Some("https://openai.example.com")
        );

        let resolved = resolve_aggregation_chain(vec![agg_provider(), normal], "gpt-4o").unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].id, "agg1");
        assert_eq!(resolved[1].id, "normal");
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
        p.meta.as_mut().unwrap().aggregation = Some(json!({
                "upstreams": [{"id":"u1","baseUrl":"https://a.example.com","apiKey":"k","apiFormat":"anthropic"}],
                "routes": [{"model":"gpt-4o","upstreamId":"u1"}]
        }));
        let err = resolve_aggregation_upstream(&p, "claude-sonnet").unwrap_err();
        assert!(err.to_string().contains("未配置模型"));
    }

    #[test]
    fn role_based_routing_and_default_fallback() {
        let mut p = agg_provider();
        p.meta.as_mut().unwrap().aggregation = Some(json!({
            "upstreams": [
                {"id":"u1","name":"A","baseUrl":"https://a.example.com","apiKey":"ka","apiFormat":"anthropic"},
                {"id":"u2","name":"B","baseUrl":"https://b.example.com","apiKey":"kb","apiFormat":"openai_chat"}
            ],
            "roles": {
                "sonnet": {"upstreamId":"u1","model":"claude-sonnet-real"},
                "opus": {"upstreamId":"u2","model":"grok-4.5"},
                "default": {"upstreamId":"u1","model":"claude-default"}
            }
        }));

        // sonnet 角色模型 → u1
        let s = resolve_aggregation_upstream(&p, "claude-sonnet-real")
            .unwrap()
            .unwrap();
        assert_eq!(
            env_of(&s, "ANTHROPIC_BASE_URL"),
            Some("https://a.example.com")
        );
        // opus 角色模型（带 [1m]）→ u2，且格式跟随上游
        let s = resolve_aggregation_upstream(&p, "grok-4.5[1m]")
            .unwrap()
            .unwrap();
        assert_eq!(
            env_of(&s, "ANTHROPIC_BASE_URL"),
            Some("https://b.example.com")
        );
        assert_eq!(
            s.meta.as_ref().unwrap().api_format.as_deref(),
            Some("openai_chat")
        );
        // 未配置的模型 → 默认角色的 * 兜底 → u1
        let s = resolve_aggregation_upstream(&p, "something-unmapped")
            .unwrap()
            .unwrap();
        assert_eq!(
            env_of(&s, "ANTHROPIC_BASE_URL"),
            Some("https://a.example.com")
        );
        assert_eq!(env_of(&s, "ANTHROPIC_MODEL"), Some("claude-default"));
    }
}
