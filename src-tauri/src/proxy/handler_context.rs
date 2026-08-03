//! 请求上下文模块
//!
//! 提供请求生命周期的上下文管理，封装通用初始化逻辑

use crate::app_config::AppType;
use crate::provider::{Provider, ProviderMeta};
use crate::proxy::{
    extract_session_id,
    forwarder::RequestForwarder,
    model_mapper::ModelMapping,
    model_router::ModelRouter,
    server::ProxyState,
    types::{AppProxyConfig, CopilotOptimizerConfig, OptimizerConfig, RectifierConfig},
    ProxyError,
};
use axum::http::HeaderMap;
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

/// 流式超时配置
#[derive(Debug, Clone, Copy)]
pub struct StreamingTimeoutConfig {
    /// 首字节超时（秒），0 表示禁用
    pub first_byte_timeout: u64,
    /// 静默期超时（秒），0 表示禁用
    pub idle_timeout: u64,
}

/// 请求上下文
///
/// 贯穿整个请求生命周期，包含：
/// - 计时信息
/// - 应用级代理配置（per-app）
/// - 选中的 Provider 列表（用于故障转移）
/// - 请求模型名称
/// - 日志标签
/// - Session ID（用于日志关联）
pub struct RequestContext {
    /// 请求开始时间
    pub start_time: Instant,
    /// 应用级代理配置（per-app，包含重试次数和超时配置）
    pub app_config: AppProxyConfig,
    /// 选中的 Provider（故障转移链的第一个）
    pub provider: Provider,
    /// 完整的 Provider 列表（用于故障转移）
    providers: Vec<Provider>,
    /// 请求开始时的"当前供应商"（用于判断是否需要同步 UI/托盘）
    ///
    /// 这里使用本地 settings 的设备级 current provider。
    /// 代理模式下如果实际使用的 provider 与此不一致，会触发切换以确保 UI 始终准确。
    pub current_provider_id: String,
    /// 请求中的模型名称
    pub request_model: String,
    /// 实际发往上游的模型名（路由接管/模型映射后的真值，forward 成功后回填）。
    ///
    /// usage 归因的兜底顺序：上游响应回显 → outbound_model → request_model。
    /// 不能直接用 request_model 兜底：接管场景下它是映射前的客户端别名。
    pub outbound_model: Option<String>,
    /// 日志标签（如 "Claude"、"Codex"、"Gemini"）
    pub tag: &'static str,
    /// 应用类型字符串（如 "claude"、"codex"、"gemini"）
    pub app_type_str: &'static str,
    /// 应用类型（预留，目前通过 app_type_str 使用）
    #[allow(dead_code)]
    pub app_type: AppType,
    /// Session ID（从客户端请求提取或新生成）
    pub session_id: String,
    /// Session ID 是否由客户端提供。生成的 UUID 不能作为上游缓存 key，否则每个请求都会换 key。
    pub session_client_provided: bool,
    /// 整流器配置
    pub rectifier_config: RectifierConfig,
    /// 优化器配置
    pub optimizer_config: OptimizerConfig,
    /// Copilot 优化器配置
    pub copilot_optimizer_config: CopilotOptimizerConfig,
}

impl RequestContext {
    /// 创建请求上下文
    ///
    /// # Arguments
    /// * `state` - 代理服务器状态
    /// * `body` - 请求体 JSON
    /// * `headers` - 请求头（用于提取 Session ID）
    /// * `app_type` - 应用类型
    /// * `tag` - 日志标签
    /// * `app_type_str` - 应用类型字符串
    ///
    /// # Errors
    /// 返回 `ProxyError` 如果 Provider 选择失败
    pub async fn new(
        state: &ProxyState,
        body: &serde_json::Value,
        headers: &HeaderMap,
        app_type: AppType,
        tag: &'static str,
        app_type_str: &'static str,
    ) -> Result<Self, ProxyError> {
        let start_time = Instant::now();

        // 从数据库读取应用级代理配置（per-app）
        let app_config = state
            .db
            .get_proxy_config_for_app(app_type_str)
            .await
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;

        // 从数据库读取整流器配置
        let rectifier_config = state.db.get_rectifier_config().unwrap_or_default();
        let optimizer_config = state.db.get_optimizer_config().unwrap_or_default();
        let copilot_optimizer_config = state.db.get_copilot_optimizer_config().unwrap_or_default();

        let current_provider_id =
            crate::settings::get_current_provider(&app_type).unwrap_or_default();

        // 从请求体提取模型名称
        let request_model = body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string();

        // 提取 Session ID
        let session_result = extract_session_id(headers, body, app_type_str);
        let session_id = session_result.session_id.clone();

        log::debug!(
            "[{}] Session ID: {} (from {:?}, client_provided: {})",
            tag,
            session_id,
            session_result.source,
            session_result.client_provided
        );

        // ─── Step 1: 尝试 UniversalProvider 路由 ───
        let universal_providers = state.db.get_all_universal_providers().unwrap_or_default();
        // 只取已启用的 UniversalProvider
        let enabled_providers: HashMap<_, _> = universal_providers
            .into_iter()
            .filter(|(_, up)| up.enabled)
            .collect();
        // 检查首页选中的供应商是否为有 routes 的 UP 同步来的
        let current_up_id = current_provider_id
            .strip_prefix("universal-claude-")
            .or_else(|| current_provider_id.strip_prefix("universal-codex-"))
            .or_else(|| current_provider_id.strip_prefix("universal-gemini-"));
        let current_up_has_routes = current_up_id
            .and_then(|up_id| enabled_providers.get(up_id))
            .is_some_and(|up| !up.routes.is_empty());
        let universal_selected: Vec<Provider> = if !enabled_providers.is_empty() {
            // 首页选中了有 routes 的 UP 时，只查该 UP 的 routes，避免交叉匹配
            let from_routes = if current_up_has_routes {
                let selected_up_id = current_up_id.unwrap();
                // 路由模型映射以 Agent 实际配置为准：取当前 provider（同步后的
                // `universal-claude-{up_id}`，即写入 Agent 的那份 env）作为映射源，
                // 而不是 UP 的 live models 配置。
                let agent_provider = state
                    .db
                    .get_provider_by_id(&current_provider_id, app_type_str)
                    .ok()
                    .flatten();
                find_matching_route(
                    &request_model,
                    &enabled_providers,
                    selected_up_id,
                    app_type_str,
                    tag,
                    agent_provider.as_ref(),
                )
            } else {
                Vec::new()
            };

            if !from_routes.is_empty() {
                from_routes
            } else if !current_up_has_routes {
                // 没有选中有 routes 的 UP，才退到 models 匹配普通 UP
                // （避免 CC Switch 代理的 models 匹配自身造成请求循环）
                ModelRouter::match_model(&request_model, &enabled_providers, app_type_str)
                    .and_then(|matched_up| {
                        let converted = match app_type_str {
                            "claude" => matched_up.to_claude_provider(),
                            "codex" => matched_up.to_codex_provider(),
                            "gemini" => matched_up.to_gemini_provider(),
                            _ => None,
                        };
                        if converted.is_some() {
                            log::info!(
                                "[{tag}] Universal route: {model} → {name} ({id})",
                                tag = tag,
                                model = request_model,
                                name = matched_up.name,
                                id = matched_up.id,
                            );
                        }
                        converted
                    })
                    .map(|p| vec![p])
                    .unwrap_or_default()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // ─── Step 2: 选择 provider（UniversalProvider > per-app）───
        let (provider, providers) = if !universal_selected.is_empty() {
            let per_app_providers = state
                .provider_router
                .select_providers(app_type_str)
                .await
                .unwrap_or_default();
            // failover 链：命中路由的完整路由链（同 UP 共享 sync id，优先级降序），
            // 再追加 per-app 供应商。路由链内任一条被上游限流/故障时，forwarder
            // 会依次尝试下一条，避免只打同一条限流上游造成无限重试风暴。
            let primary = universal_selected[0].clone();
            let mut chain = universal_selected;
            chain.extend(
                per_app_providers
                    .iter()
                    .filter(|p| p.id != primary.id)
                    .cloned(),
            );
            (primary, chain)
        } else if current_up_has_routes {
            // 首页选中的是有 routes 的 UP，但请求模型没有命中任何一条 route。
            //
            // 不能退到 Step 2 的 per-app 逻辑：当前 provider 就是该 UP 同步来的
            // `universal-claude-{id}`，其 ANTHROPIC_BASE_URL 指向本地代理自身
            // （CC Switch 代理 preset 的 baseUrl=127.0.0.1:15721）。退到 per-app
            // 会把请求再次转发回本代理 → 无限自环 → 会话静默断开且无任何错误提示。
            // 这里显式报错，让客户端能看见具体模型名。
            let up_name = current_up_id
                .and_then(|up_id| enabled_providers.get(up_id))
                .map(|up| up.name.as_str())
                .unwrap_or("<unknown>");
            log::warn!(
                "[{tag}] UP `{up_name}` 有 routes 但模型 `{model}` 未命中任何 route，拒绝转发以避免自环",
                tag = tag,
                up_name = up_name,
                model = request_model,
            );
            return Err(ProxyError::InvalidRequest(format!(
                "当前统一供应商 `{up_name}` 的路由表中没有匹配模型 `{model}` 的 route，请检查该供应商的 routes 配置或模型名",
                up_name = up_name,
                model = request_model,
            )));
        } else {
            // 走现有 per-app 逻辑
            let providers = state
                .provider_router
                .select_providers(app_type_str)
                .await
                .map_err(|e| match e {
                    crate::error::AppError::AllProvidersCircuitOpen => {
                        ProxyError::AllProvidersCircuitOpen
                    }
                    crate::error::AppError::NoProvidersConfigured => {
                        ProxyError::NoProvidersConfigured
                    }
                    _ => ProxyError::DatabaseError(e.to_string()),
                })?;

            let provider = providers
                .first()
                .cloned()
                .ok_or(ProxyError::NoAvailableProvider)?;
            (provider, providers)
        };

        log::debug!(
            "[{}] Provider: {}, model: {}, failover chain: {} providers, session: {}",
            tag,
            provider.name,
            request_model,
            providers.len(),
            session_id
        );

        Ok(Self {
            start_time,
            app_config,
            provider,
            providers,
            current_provider_id,
            request_model,
            outbound_model: None,
            tag,
            app_type_str,
            app_type,
            session_id,
            session_client_provided: session_result.client_provided,
            rectifier_config,
            optimizer_config,
            copilot_optimizer_config,
        })
    }

    /// 从 URI 提取模型名称（Gemini 专用）
    ///
    /// Gemini API 的模型名称在 URI 中，格式如：
    /// `/v1beta/models/gemini-pro:generateContent`
    pub fn with_model_from_uri(mut self, uri: &axum::http::Uri) -> Self {
        // 用 path() 而不是 path_and_query()：模型名必须从路径段中解析，
        // 否则 GET /v1beta/models/<id>?key=... 会把 query 拼到 request_model 上。
        let endpoint = uri.path();

        self.request_model =
            extract_gemini_model_from_path(endpoint).unwrap_or_else(|| "unknown".to_string());

        self
    }

    /// 用量日志中记录的 provider 标识。
    ///
    /// 路由请求（CC Switch 代理按 routes 转发到某个上游）的 provider id 是
    /// `universal-{app}-{up_id}`，用量日志按它 JOIN 回 UP 名（如 "CC Switch 代理"），
    /// 看不出实际上游。这里对路由请求返回上游名（route.name），其余返回 provider id。
    pub fn usage_provider_label(&self) -> String {
        let is_route = self
            .provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("cc_switch_route");
        if is_route {
            self.provider.name.clone()
        } else {
            self.provider.id.clone()
        }
    }

    /// 当前 provider 的类型（路由 provider 为 "cc_switch_route"），用量日志据此标记行来源。
    pub fn usage_provider_type(&self) -> Option<String> {
        self.provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.clone())
    }

    /// 创建 RequestForwarder
    ///
    /// 使用共享的 ProviderRouter，确保熔断器状态跨请求保持
    ///
    /// 配置生效规则：
    /// - 故障转移开启：超时配置正常生效（0 表示禁用超时）
    /// - 故障转移关闭：超时配置不生效（全部传入 0）
    pub fn create_forwarder(&self, state: &ProxyState) -> RequestForwarder {
        let (non_streaming_timeout, first_byte_timeout, idle_timeout) =
            if self.app_config.auto_failover_enabled {
                // 故障转移开启：使用配置的值（0 = 禁用超时）
                (
                    self.app_config.non_streaming_timeout as u64,
                    self.app_config.streaming_first_byte_timeout as u64,
                    self.app_config.streaming_idle_timeout as u64,
                )
            } else {
                // 故障转移关闭：不启用超时配置
                log::debug!(
                    "[{}] Failover disabled, timeout configs are bypassed",
                    self.tag
                );
                (0, 0, 0)
            };

        // 故障转移关闭时强制 max_retries=0（仅尝试 1 个 provider），与「不超时 + 不切换」语义一致。
        // 路由链例外：路由配了多个目标就该按优先级都试（New API 降级语义），只放宽 attempt
        // 上限，forwarder 的 failover 循环本身不变。
        let max_retries = if self.app_config.auto_failover_enabled {
            self.app_config.max_retries
        } else if self.is_route_chain() && self.providers.len() > 1 {
            (self.providers.len() - 1) as u32
        } else {
            0
        };

        // 路由链的切换基线取首个目标 id：主目标成功时 should_switch=false，不触发 try_switch
        // （路由目标在 DB 中不存在，hot_switch_provider 会无害失败），只在真正降级到次目标时才触发。
        let switch_baseline = if self.is_route_chain() {
            self.providers
                .first()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| self.current_provider_id.clone())
        } else {
            self.current_provider_id.clone()
        };

        RequestForwarder::new(
            state.provider_router.clone(),
            non_streaming_timeout,
            state.status.clone(),
            state.current_providers.clone(),
            state.gemini_shadow.clone(),
            state.codex_chat_history.clone(),
            state.failover_manager.clone(),
            state.app_handle.clone(),
            switch_baseline,
            self.session_id.clone(),
            self.session_client_provided,
            first_byte_timeout,
            idle_timeout,
            self.rectifier_config.clone(),
            self.optimizer_config.clone(),
            self.copilot_optimizer_config.clone(),
            max_retries,
        )
    }

    /// 是否为路由链：首个 provider 是路由目标（meta.provider_type == "cc_switch_route"）
    fn is_route_chain(&self) -> bool {
        self.providers.first().is_some_and(|p| {
            p.meta.as_ref().and_then(|m| m.provider_type.as_deref()) == Some("cc_switch_route")
        })
    }

    /// 获取 Provider 列表（用于故障转移）
    ///
    /// 返回在创建上下文时已选择的 providers，避免重复调用 select_providers()
    pub fn get_providers(&self) -> Vec<Provider> {
        self.providers.clone()
    }

    /// 计算请求延迟（毫秒）
    #[inline]
    pub fn latency_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    /// 获取流式超时配置
    ///
    /// 配置生效规则：
    /// - 故障转移开启：返回配置的值（0 表示禁用超时检查）
    /// - 故障转移关闭：返回 0（禁用超时检查）
    #[inline]
    pub fn streaming_timeout_config(&self) -> StreamingTimeoutConfig {
        if self.app_config.auto_failover_enabled {
            // 故障转移开启：使用配置的值（0 = 禁用超时）
            StreamingTimeoutConfig {
                first_byte_timeout: self.app_config.streaming_first_byte_timeout as u64,
                idle_timeout: self.app_config.streaming_idle_timeout as u64,
            }
        } else {
            // 故障转移关闭：禁用流式超时检查
            StreamingTimeoutConfig {
                first_byte_timeout: 0,
                idle_timeout: 0,
            }
        }
    }
}

/// 查找当前选中 UP 的 routes 中匹配 model 的所有路由，按优先级降序组成
/// failover 链（同优先级内随机打乱）。返回整条链而非单条：单条路由被上游
/// 限流（429/5xx）时 forwarder 的 failover 循环会依次尝试下一条匹配路由，
/// 避免客户端反复重试同一条限流上游造成无限调用风暴。
fn find_matching_route(
    model: &str,
    providers: &HashMap<String, crate::provider::UniversalProvider>,
    selected_up_id: &str,
    app_type: &str,
    tag: &str,
    agent_provider: Option<&crate::provider::Provider>,
) -> Vec<crate::provider::Provider> {
    let up = match providers.get(selected_up_id) {
        Some(up) => up,
        None => return Vec::new(),
    };

    // 接管模式下 Claude Code 发送的是 Claude 角色别名（如 claude-sonnet-4-6[1m]），
    // 而 routes 的 modelNames 是从上游 fetch 的真实模型名。用 Agent 实际配置
    // （当前 provider 的同步 env）把请求名映射成真实上游名，再拿真实名去匹配
    // modelNames。UP 的 live models 配置只负责同步到 Agent，不在此处生效。
    let mapped_model = if app_type == "claude" {
        match agent_provider {
            Some(p) => {
                let mapping = ModelMapping::from_provider(p);
                if mapping.has_mapping() {
                    mapping.map_model(model)
                } else {
                    model.to_string()
                }
            }
            None => model.to_string(),
        }
    } else {
        model.to_string()
    };

    // 收集所有命中（enabled + 协议兼容 + 模型匹配）的路由，而不是只挑最优一条。
    let mut matched: Vec<&crate::provider::UpstreamRoute> = Vec::new();
    for route in &up.routes {
        if !route.enabled {
            continue;
        }
        // Codex 请求跳过 gemini 协议路由（CodexAdapter 不支持 Gemini 格式转换）
        if app_type == "codex" && route.protocol == "gemini" {
            continue;
        }
        // 同时匹配原始名（可能已是真实名）与映射名（Claude 别名 → 真实名）
        let is_match = ModelRouter::match_route(model, route)
            || (mapped_model != model && ModelRouter::match_route(&mapped_model, route));
        if is_match {
            matched.push(route);
        }
    }
    if matched.is_empty() {
        return Vec::new();
    }

    // 按优先级降序作为 failover 顺序；同优先级内随机打乱，保留原平局均匀随机语义。
    matched.sort_by_key(|b| std::cmp::Reverse(b.priority));
    let mut chain: Vec<crate::provider::Provider> = Vec::new();
    let mut cursor = 0;
    while cursor < matched.len() {
        let tier_priority = matched[cursor].priority;
        let tier_start = cursor;
        while cursor < matched.len() && matched[cursor].priority == tier_priority {
            cursor += 1;
        }
        let mut tier: Vec<&crate::provider::UpstreamRoute> = matched[tier_start..cursor].to_vec();
        for k in (1..tier.len()).rev() {
            let r = Uuid::new_v4().as_u128() % (k as u128 + 1);
            tier.swap(k, r as usize);
        }
        for route in tier {
            chain.push(build_route_provider(route, up, app_type, agent_provider));
        }
    }

    let chain_names: Vec<&str> = chain.iter().map(|p| p.name.as_str()).collect();
    let priorities: Vec<u32> = matched.iter().map(|r| r.priority).collect();
    log::info!(
        "[{tag}] Route match: {model} → [{chain_names}] via {up_name} ({up_id}) priorities={priorities:?}",
        tag = tag,
        model = model,
        chain_names = chain_names.join(", "),
        up_name = up.name,
        up_id = up.id,
        priorities = priorities,
    );
    if mapped_model != model {
        log::debug!(
            "[{tag}] Route model mapped: {model} → {mapped_model} (for modelNames match)",
            tag = tag,
            model = model,
            mapped_model = mapped_model,
        );
    }
    for route in &matched {
        let key_len = route.api_key.len();
        log::info!(
            "[{tag}] Route provider: {route_name} protocol={protocol} baseURL={baseUrl} apiKey_len={key_len}",
            tag = tag,
            route_name = route.name,
            protocol = route.protocol,
            baseUrl = route.base_url,
            key_len = key_len,
        );
    }
    chain
}

/// 把一条 UpstreamRoute 转成请求链里的 Provider：携带该路由的 baseURL/apiKey
/// 与 Agent 实际配置的模型映射 env（映射源跟随 Agent，不读 UP live models）。
fn build_route_provider(
    route: &crate::provider::UpstreamRoute,
    up: &crate::provider::UniversalProvider,
    app_type: &str,
    agent_provider: Option<&crate::provider::Provider>,
) -> crate::provider::Provider {
    let (api_format, auth_var) = match route.protocol.as_str() {
        "anthropic" => ("anthropic", "ANTHROPIC_API_KEY"),
        "openai_chat" => ("openai_chat", "ANTHROPIC_AUTH_TOKEN"),
        "openai_responses" => ("openai_responses", "ANTHROPIC_AUTH_TOKEN"),
        "gemini" => ("gemini_native", "GEMINI_API_KEY"),
        _ => ("anthropic", "ANTHROPIC_AUTH_TOKEN"),
    };
    // 并入模型映射 env（ANTHROPIC_MODEL / ANTHROPIC_DEFAULT_*_MODEL 等），
    // 使 forward() 里的 apply_model_mapping 能把请求模型名从接管别名转成
    // Agent 中生效的真实上游名再发给上游。映射源跟随 Agent 实际配置
    // （当前 provider 的同步 env），不用 UP 的 live models 配置，避免
    // UP 改动未同步时路由模型错乱。
    let mut env = serde_json::Map::new();
    if app_type == "claude" {
        if let Some(env_obj) = agent_provider
            .and_then(|p| p.settings_config.get("env"))
            .and_then(|v| v.as_object())
        {
            for key in [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_DEFAULT_FABLE_MODEL",
                "CLAUDE_CODE_SUBAGENT_MODEL",
            ] {
                if let Some(v) = env_obj.get(key) {
                    env.insert(key.to_string(), v.clone());
                }
            }
        }
    }
    env.insert(
        "ANTHROPIC_BASE_URL".into(),
        serde_json::Value::String(route.base_url.clone()),
    );
    env.insert(
        auth_var.into(),
        serde_json::Value::String(route.api_key.clone()),
    );
    // 每个路由目标独立 id → 独立熔断器。原共享 id 使任意目标熔断（如商汤限流）
    // 会连带阻断整条路由链（其余目标也命中同一熔断器）→ 503 无可用 Provider。
    // 路由目标 id 在 DB 中不存在，failover 后 try_switch 到它会无害失败（不改状态）。
    let sync_id = format!("universal-{}-{}-{}", app_type, up.id, route.name);
    crate::provider::Provider {
        id: sync_id,
        name: route.name.clone(),
        settings_config: serde_json::json!({
            "baseURL": route.base_url,
            "apiKey": route.api_key,
            "env": env,
        }),
        website_url: None,
        category: Some("custom".to_string()),
        created_at: None,
        sort_index: None,
        notes: None,
        icon: up.icon.clone(),
        icon_color: up.icon_color.clone(),
        meta: Some(ProviderMeta {
            api_format: Some(api_format.to_string()),
            provider_type: Some("cc_switch_route".to_string()),
            api_key_field: route
                .protocol
                .as_str()
                .eq("anthropic")
                .then(|| "ANTHROPIC_API_KEY".to_string()),
            ..Default::default()
        }),
        in_failover_queue: false,
    }
}

/// Pull the Gemini model name out of an API path.
///
/// Accepts forms like `/v1beta/models/gemini-pro:generateContent`,
/// `/v1/models/gemini-1.5-flash`, `gemini/v1beta/models/<model>:streamGenerateContent`.
/// Returns `None` when no `models/<name>` segment is present.
pub(crate) fn extract_gemini_model_from_path(endpoint: &str) -> Option<String> {
    let segments: Vec<&str> = endpoint.split('/').collect();
    segments
        .iter()
        .position(|s| *s == "models")
        .and_then(|i| segments.get(i + 1).copied())
        // 防御性裁剪：即便调用方传入带 ? 或 :action 的字符串，也只保留 model id 本身
        .map(|s| s.split('?').next().unwrap_or(s))
        .map(|s| s.split(':').next().unwrap_or(s))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::{extract_gemini_model_from_path, find_matching_route};
    use crate::provider::{
        ClaudeModelConfig, UniversalProvider, UniversalProviderApps, UniversalProviderModels,
        UpstreamRoute,
    };
    use std::collections::HashMap;

    fn make_up_with_route(
        up_id: &str,
        sonnet_model: &str,
        route_models: &[&str],
    ) -> UniversalProvider {
        UniversalProvider {
            id: up_id.to_string(),
            name: up_id.to_string(),
            provider_type: "cc_switch".to_string(),
            apps: UniversalProviderApps {
                claude: true,
                codex: false,
                gemini: false,
            },
            base_url: "http://127.0.0.1:15721".to_string(),
            api_key: "".to_string(),
            models: UniversalProviderModels {
                claude: Some(ClaudeModelConfig {
                    model: Some("deepseek-v4-flash".to_string()),
                    haiku_model: None,
                    sonnet_model: Some(sonnet_model.to_string()),
                    opus_model: None,
                }),
                codex: None,
                gemini: None,
            },
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            meta: None,
            created_at: None,
            sort_index: None,
            enabled: true,
            routes: vec![UpstreamRoute {
                id: "r1".to_string(),
                name: "r1".to_string(),
                protocol: "openai_chat".to_string(),
                base_url: "https://upstream.example.com".to_string(),
                api_key: "sk-route".to_string(),
                model_names: route_models.iter().map(|m| m.to_string()).collect(),
                enabled: true,
                priority: 0,
            }],
        }
    }

    /// 构造「Agent 实际配置」的 provider：同步后写入 Agent 的那份 env
    /// （routes 映射以此为准，而非 UP 的 live models 配置）。
    fn make_agent_provider(sonnet: &str) -> crate::provider::Provider {
        crate::provider::Provider {
            id: "universal-claude-up1".to_string(),
            name: "agent".to_string(),
            settings_config: serde_json::json!({
                "env": {
                    "ANTHROPIC_MODEL": "deepseek-v4-flash",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "mimo-v2.5",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": sonnet,
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "qwen3.7-plus",
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

    #[test]
    fn find_matching_route_maps_claude_alias_to_upstream_model() {
        // 接管模式下 Claude Code 发 claude-sonnet-4-6[1m]（Claude 别名），
        // routes 的 modelNames 是从上游 fetch 的真实名 deepseek-v4-flash。
        // 映射源是 Agent 实际配置（当前 provider 的同步 env），而非 UP 的
        // live models 配置：这里 UP 的 sonnetModel 是 hy3，Agent 配置是
        // deepseek-v4-flash，路由必须跟随 Agent 映射成 deepseek-v4-flash。
        let up = make_up_with_route("up1", "hy3", &["deepseek-v4-flash"]);
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);
        let agent = make_agent_provider("deepseek-v4-flash");

        let result = find_matching_route(
            "claude-sonnet-4-6[1m]",
            &map,
            "up1",
            "claude",
            "test",
            Some(&agent),
        );
        let provider = result
            .first()
            .expect("应跟随 Agent 配置映射成 deepseek-v4-flash 并命中 route");
        assert_eq!(provider.name, "r1");
        // 路由 provider 需带标记，用量日志据此记录上游名而非 UP 名
        assert_eq!(
            provider.meta.as_ref().unwrap().provider_type.as_deref(),
            Some("cc_switch_route")
        );
        // provider env 必须携带 Agent 的映射表，供 forward 里的 apply_model_mapping
        // 把 body.model 从 claude-sonnet-4-6 转成 deepseek-v4-flash 发给上游
        let env = provider.settings_config["env"].as_object().unwrap();
        assert_eq!(
            env["ANTHROPIC_DEFAULT_SONNET_MODEL"].as_str(),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            env["ANTHROPIC_BASE_URL"].as_str(),
            Some("https://upstream.example.com")
        );
        assert_eq!(env["ANTHROPIC_AUTH_TOKEN"].as_str(), Some("sk-route"));
    }

    #[test]
    fn find_matching_route_plain_model_without_mapping() {
        // 请求名就是真实名，无需映射也应命中
        let up = make_up_with_route("up1", "deepseek-v4-flash", &["deepseek-v4-flash"]);
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = find_matching_route("deepseek-v4-flash", &map, "up1", "claude", "test", None);
        assert!(!result.is_empty());
    }

    #[test]
    fn find_matching_route_no_match_returns_none() {
        // 别名映射后（deepseek-v4-flash）仍不在 routes 的 modelNames 里 → 空链
        let up = make_up_with_route("up1", "deepseek-v4-flash", &["gpt-5"]);
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = find_matching_route("claude-sonnet-4-6", &map, "up1", "claude", "test", None);
        assert!(result.is_empty());
    }

    fn make_up_with_two_routes(
        up_id: &str,
        models: &[&str],
        p1: u32,
        p2: u32,
    ) -> UniversalProvider {
        let mk = |id: &str, models: Vec<String>, priority: u32| UpstreamRoute {
            id: id.to_string(),
            name: id.to_string(),
            protocol: "openai_chat".to_string(),
            base_url: format!("https://{}.example.com", id),
            api_key: "sk-route".to_string(),
            model_names: models,
            enabled: true,
            priority,
        };
        UniversalProvider {
            id: up_id.to_string(),
            name: up_id.to_string(),
            provider_type: "cc_switch".to_string(),
            apps: UniversalProviderApps {
                claude: true,
                codex: false,
                gemini: false,
            },
            base_url: "http://127.0.0.1:15721".to_string(),
            api_key: "".to_string(),
            models: UniversalProviderModels {
                claude: Some(ClaudeModelConfig {
                    model: Some("deepseek-v4-flash".to_string()),
                    haiku_model: None,
                    sonnet_model: Some("deepseek-v4-flash".to_string()),
                    opus_model: None,
                }),
                codex: None,
                gemini: None,
            },
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            meta: None,
            created_at: None,
            sort_index: None,
            enabled: true,
            routes: vec![
                mk("r1", models.iter().map(|m| m.to_string()).collect(), p1),
                mk("r2", models.iter().map(|m| m.to_string()).collect(), p2),
            ],
        }
    }

    #[test]
    fn find_matching_route_prefers_higher_priority() {
        // 两条都命中，r2 优先级 5 > r1 优先级 1 → 必须选 r2
        let up = make_up_with_two_routes("up1", &["deepseek-v4-flash"], 1, 5);
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = find_matching_route("deepseek-v4-flash", &map, "up1", "claude", "test", None);
        assert_eq!(result.first().expect("命中").name, "r2");
    }

    #[test]
    fn find_matching_route_unset_priority_defaults_to_0() {
        // 未配置优先级默认 0：r1 默认 0、r2 配置 5 → 选 r2
        let up = make_up_with_two_routes("up1", &["deepseek-v4-flash"], 0, 5);
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = find_matching_route("deepseek-v4-flash", &map, "up1", "claude", "test", None);
        assert_eq!(result.first().expect("命中").name, "r2");
    }

    #[test]
    fn find_matching_route_returns_failover_chain_ordered_by_priority() {
        // deepseek-v4-flash 同时命中 r1(priority 1) 与 r2(priority 0) →
        // 返回整条 failover 链 [r1, r2]，而非只挑 r1。上游 429/5xx 时
        // forwarder 会依次尝试下一条，避免只打同一条限流上游造成无限重试风暴。
        let up = make_up_with_two_routes("up1", &["deepseek-v4-flash"], 1, 0);
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = find_matching_route("deepseek-v4-flash", &map, "up1", "claude", "test", None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "r1");
        assert_eq!(result[1].name, "r2");
        // 每个路由目标独立 id → 独立熔断器：商汤限流熔断不阻断 OpenCode Go。
        // try_switch 到路由目标 id 会无害失败（DB 中不存在），不影响原版逻辑。
        assert_ne!(result[0].id, result[1].id);
    }

    #[test]
    fn find_matching_route_tie_priority_is_random() {
        // 同优先级平局 → 随机：跑多轮两种都被选中过
        let up = make_up_with_two_routes("up1", &["deepseek-v4-flash"], 5, 5);
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let mut saw_r1 = false;
        let mut saw_r2 = false;
        for _ in 0..200 {
            let result =
                find_matching_route("deepseek-v4-flash", &map, "up1", "claude", "test", None);
            if result.first().expect("命中").name == "r1" {
                saw_r1 = true;
            }
            if result.first().expect("命中").name == "r2" {
                saw_r2 = true;
            }
            if saw_r1 && saw_r2 {
                break;
            }
        }
        assert!(saw_r1 && saw_r2, "同优先级应随机，两种路由都应被选中");
    }

    #[test]
    fn extract_model_with_action() {
        assert_eq!(
            extract_gemini_model_from_path("/v1beta/models/gemini-pro:generateContent").as_deref(),
            Some("gemini-pro"),
        );
    }

    #[test]
    fn extract_model_with_dotted_version() {
        assert_eq!(
            extract_gemini_model_from_path("/v1beta/models/gemini-1.5-flash:streamGenerateContent")
                .as_deref(),
            Some("gemini-1.5-flash"),
        );
    }

    #[test]
    fn extract_model_without_action() {
        assert_eq!(
            extract_gemini_model_from_path("/v1/models/gemini-1.5-pro").as_deref(),
            Some("gemini-1.5-pro"),
        );
    }

    #[test]
    fn extract_model_with_proxy_prefix() {
        assert_eq!(
            extract_gemini_model_from_path("/gemini/v1beta/models/gemini-2.0-flash:countTokens")
                .as_deref(),
            Some("gemini-2.0-flash"),
        );
    }

    #[test]
    fn extract_model_with_query_string() {
        assert_eq!(
            extract_gemini_model_from_path("/v1beta/models/gemini-pro:generateContent?key=abc")
                .as_deref(),
            Some("gemini-pro"),
        );
    }

    #[test]
    fn extract_model_missing_segment() {
        assert_eq!(extract_gemini_model_from_path("/v1beta/operations"), None);
    }

    #[test]
    fn extract_model_trailing_models_segment() {
        // `/v1beta/models` (list endpoint) has no following segment → None.
        assert_eq!(extract_gemini_model_from_path("/v1beta/models"), None);
    }

    #[test]
    fn extract_model_get_with_query_only() {
        // GET /v1beta/models/<id>?key=... 无 action verb，仅靠 ':' 拆分会把 query 带进 model 名。
        // 修复后应该把 query 剥掉。
        assert_eq!(
            extract_gemini_model_from_path("/v1beta/models/gemini-pro?key=abc").as_deref(),
            Some("gemini-pro"),
        );
    }

    #[test]
    fn extract_model_get_with_proxy_prefix_and_query() {
        assert_eq!(
            extract_gemini_model_from_path("/gemini/v1beta/models/gemini-2.0-flash?key=abc")
                .as_deref(),
            Some("gemini-2.0-flash"),
        );
    }
}
