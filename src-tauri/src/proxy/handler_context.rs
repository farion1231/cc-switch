//! 请求上下文模块
//!
//! 提供请求生命周期的上下文管理，封装通用初始化逻辑

use crate::app_config::AppType;
use crate::provider::Provider;
use crate::proxy::{
    extract_session_id,
    forwarder::RequestForwarder,
    server::ProxyState,
    types::{AppProxyConfig, CopilotOptimizerConfig, OptimizerConfig, RectifierConfig},
    ProxyError,
};
use axum::http::HeaderMap;
use std::time::Instant;

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
    /// 请求中的模型名称（客户端原始入站值；跨供应商子代理路由时保留保留别名）
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
    /// 跨供应商子代理路由：禁止把本次故意使用的目标供应商当成全局故障转移热切换。
    pub suppress_global_provider_switch: bool,
}

impl RequestContext {
    /// 创建请求上下文
    ///
    /// # Arguments
    /// * `state` - 代理服务器状态
    /// * `body` - 请求体 JSON（子代理跨供应商路由可能改写 `model`）
    /// * `headers` - 请求头（用于提取 Session ID）
    /// * `app_type` - 应用类型
    /// * `tag` - 日志标签
    /// * `app_type_str` - 应用类型字符串
    ///
    /// # Errors
    /// 返回 `ProxyError` 如果 Provider 选择失败
    pub async fn new(
        state: &ProxyState,
        body: &mut serde_json::Value,
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

        // 与 ProviderRouter / 热切换一致：优先本地 settings，并校验 DB 中仍存在
        let current_provider_id =
            crate::settings::get_effective_current_provider(state.db.as_ref(), &app_type)
                .ok()
                .flatten()
                .or_else(|| crate::settings::get_current_provider(&app_type))
                .unwrap_or_default();

        // 从请求体提取模型名称（保留客户端原始值用于日志 / usage.request_model）
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

        let mut suppress_global_provider_switch = false;

        // Claude Code 子代理跨供应商路由必须在 select_providers 之前完成：
        // 否则空/全开故障转移队列会在显式目标解析前拒绝请求。
        // 仅当「请求 model 是保留别名」且「active 供应商有合法 foreign-route meta」时路由；
        // 无路由 meta 时别名按普通模型处理（用户可能把保留串当作真实子代理模型名）。
        let routed = if matches!(app_type, AppType::Claude)
            && crate::proxy::claude_subagent_route::is_claude_subagent_route_alias(&request_model)
        {
            let active_provider = load_active_claude_provider(state, &current_provider_id)?;
            if crate::proxy::claude_subagent_route::foreign_subagent_route_provider_id(
                &active_provider,
            )
            .is_some()
            {
                let db = state.db.clone();
                let resolved = crate::proxy::claude_subagent_route::resolve_subagent_route(
                    &active_provider,
                    |target_id| {
                        db.get_provider_by_id(target_id, "claude")
                            .map_err(|e| e.to_string())
                    },
                )
                .map_err(ProxyError::ConfigError)?;

                log::info!(
                    "[{}] Claude subagent cross-provider route: active={} → target={} model={} (client_model={})",
                    tag,
                    active_provider.id,
                    resolved.target_provider.id,
                    resolved.target_subagent_model,
                    request_model
                );

                // 改写 outbound body model；request_model 保持客户端入站别名供日志使用。
                crate::proxy::claude_subagent_route::rewrite_request_model_to_target(
                    body,
                    &resolved.target_subagent_model,
                );
                // 故意使用目标供应商：禁止 forwarder 将其视为全局故障转移并热切换。
                suppress_global_provider_switch = true;
                Some(
                    crate::proxy::claude_subagent_route::pin_providers_to_target(
                        resolved.target_provider,
                    ),
                )
            } else {
                // 别名碰撞：无 foreign-route meta → 不走跨供应商路由
                None
            }
        } else {
            None
        };

        let providers = if let Some(routed_providers) = routed {
            routed_providers
        } else {
            // 使用共享的 ProviderRouter 选择 Provider（熔断器状态跨请求保持）
            // 注意：只在这里调用一次，结果传递给 forwarder，避免重复消耗 HalfOpen 名额
            state
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
                })?
        };

        let provider = providers
            .first()
            .cloned()
            .ok_or(ProxyError::NoAvailableProvider)?;

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
            suppress_global_provider_switch,
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
        let max_retries = if self.app_config.auto_failover_enabled {
            self.app_config.max_retries
        } else {
            0
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
            self.current_provider_id.clone(),
            self.session_id.clone(),
            self.session_client_provided,
            first_byte_timeout,
            idle_timeout,
            self.rectifier_config.clone(),
            self.optimizer_config.clone(),
            self.copilot_optimizer_config.clone(),
            max_retries,
            self.suppress_global_provider_switch,
        )
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

/// 加载 Claude 当前（active）供应商：优先 effective current，否则 DB current。
fn load_active_claude_provider(
    state: &ProxyState,
    current_provider_id: &str,
) -> Result<Provider, ProxyError> {
    if !current_provider_id.is_empty() {
        if let Some(provider) = state
            .db
            .get_provider_by_id(current_provider_id, "claude")
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?
        {
            return Ok(provider);
        }
    }

    // settings / effective id 缺失时回退 DB current
    if let Some(db_current) = state
        .db
        .get_current_provider("claude")
        .map_err(|e| ProxyError::DatabaseError(e.to_string()))?
    {
        if let Some(provider) = state
            .db
            .get_provider_by_id(&db_current, "claude")
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?
        {
            return Ok(provider);
        }
    }

    Err(ProxyError::ConfigError(
        "Claude subagent cross-provider route requires an active Claude provider".to_string(),
    ))
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
    use super::extract_gemini_model_from_path;
    use super::RequestContext;
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::provider::{ClaudeSubagentRoute, Provider, ProviderMeta};
    use crate::proxy::circuit_breaker::CircuitBreakerConfig;
    use crate::proxy::claude_subagent_route::CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS;
    use crate::proxy::failover_switch::FailoverSwitchManager;
    use crate::proxy::provider_router::ProviderRouter;
    use crate::proxy::providers::codex_chat_history::CodexChatHistoryStore;
    use crate::proxy::providers::gemini_shadow::GeminiShadowStore;
    use crate::proxy::server::ProxyState;
    use crate::proxy::types::{ProxyConfig, ProxyStatus};
    use crate::proxy::ProxyError;
    use axum::http::HeaderMap;
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::collections::HashMap;
    use std::env;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::RwLock;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn build_state(db: Arc<Database>) -> ProxyState {
        ProxyState {
            db: db.clone(),
            config: Arc::new(RwLock::new(ProxyConfig::default())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            provider_router: Arc::new(ProviderRouter::new(db.clone())),
            gemini_shadow: Arc::new(GeminiShadowStore::default()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            app_handle: None,
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
        }
    }

    fn provider_a_with_route_to_b() -> Provider {
        let mut provider = Provider::with_id(
            "a".to_string(),
            "Active A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "a-key",
                    "ANTHROPIC_BASE_URL": "https://api.a.example",
                    "CLAUDE_CODE_SUBAGENT_MODEL": "a-sub"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            claude_subagent_route: Some(ClaudeSubagentRoute {
                provider_id: "b".to_string(),
            }),
            ..ProviderMeta::default()
        });
        provider
    }

    fn provider_b_target() -> Provider {
        Provider::with_id(
            "b".to_string(),
            "Target B".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "b-key",
                    "ANTHROPIC_BASE_URL": "https://api.b.example",
                    "CLAUDE_CODE_SUBAGENT_MODEL": "b-sub[1M]"
                }
            }),
            None,
        )
    }

    async fn seed_active_a_route_to_b(db: &Database) {
        db.save_provider("claude", &provider_a_with_route_to_b())
            .expect("save a");
        db.save_provider("claude", &provider_b_target())
            .expect("save b");
        db.set_current_provider("claude", "a").expect("set current");
        crate::settings::set_current_provider(&AppType::Claude, Some("a"))
            .expect("set local current");
    }

    async fn new_claude_ctx(
        state: &ProxyState,
        body: &mut Value,
    ) -> Result<RequestContext, ProxyError> {
        RequestContext::new(
            state,
            body,
            &HeaderMap::new(),
            AppType::Claude,
            "Claude",
            "claude",
        )
        .await
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

    #[tokio::test]
    #[serial]
    async fn request_context_alias_route_selects_only_target_and_rewrites_model() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().expect("memory db"));
        seed_active_a_route_to_b(&db).await;
        let state = build_state(db);

        let mut body = json!({
            "model": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let ctx = new_claude_ctx(&state, &mut body).await.expect("ctx");

        assert_eq!(ctx.provider.id, "b");
        assert_eq!(ctx.get_providers().len(), 1);
        assert_eq!(ctx.get_providers()[0].id, "b");
        assert_eq!(body["model"], "b-sub[1M]");
        // Logging semantics: preserve client alias
        assert_eq!(ctx.request_model, CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS);
        assert!(ctx.outbound_model.is_none());
        assert!(ctx.suppress_global_provider_switch);
        // Global current remains active A
        assert_eq!(ctx.current_provider_id, "a");
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("a")
        );
        // Target settings used as stored (raw provider config)
        assert_eq!(
            ctx.provider.settings_config["env"]["ANTHROPIC_API_KEY"],
            "b-key"
        );
    }

    #[tokio::test]
    #[serial]
    async fn request_context_alias_route_bypasses_empty_active_failover_queue() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().expect("memory db"));
        seed_active_a_route_to_b(&db).await;

        // Failover enabled + empty queue would normally reject select_providers
        let mut config = db
            .get_proxy_config_for_app("claude")
            .await
            .expect("proxy config");
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config)
            .await
            .expect("update proxy config");
        db.clear_failover_queue("claude").expect("clear queue");

        let state = build_state(db);
        // Non-alias path must fail with empty queue
        let mut normal_body = json!({"model": "claude-sonnet-4-6"});
        let err = new_claude_ctx(&state, &mut normal_body)
            .await
            .err()
            .expect("empty failover queue should reject non-alias");
        assert!(matches!(err, ProxyError::NoProvidersConfigured));

        // Alias + foreign route must still pin target B
        let mut route_body = json!({"model": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS});
        let ctx = new_claude_ctx(&state, &mut route_body)
            .await
            .expect("explicit route must bypass empty queue");
        assert_eq!(ctx.provider.id, "b");
        assert_eq!(route_body["model"], "b-sub[1M]");
        assert_eq!(ctx.current_provider_id, "a");
        assert!(ctx.suppress_global_provider_switch);
    }

    #[tokio::test]
    #[serial]
    async fn request_context_alias_route_bypasses_all_open_failover_queue() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().expect("memory db"));
        seed_active_a_route_to_b(&db).await;

        // Put only A in failover queue and trip its circuit fully open
        db.add_to_failover_queue("claude", "a").expect("queue a");
        let mut config = db
            .get_proxy_config_for_app("claude")
            .await
            .expect("proxy config");
        config.auto_failover_enabled = true;
        config.circuit_failure_threshold = 1;
        config.circuit_timeout_seconds = 3600;
        db.update_proxy_config_for_app(config)
            .await
            .expect("update");
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 3600,
            ..Default::default()
        })
        .await
        .expect("cb config");

        let state = build_state(db.clone());
        state
            .provider_router
            .record_result("a", "claude", false, false, Some("fail".into()))
            .await
            .expect("trip breaker");

        // Non-alias path should see all circuit open
        let mut normal_body = json!({"model": "claude-sonnet-4-6"});
        let err = new_claude_ctx(&state, &mut normal_body)
            .await
            .err()
            .expect("all-open queue should reject non-alias");
        assert!(matches!(err, ProxyError::AllProvidersCircuitOpen));

        // Explicit route still selects B (B is not in failover queue; route ignores it)
        let mut route_body = json!({"model": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS});
        let ctx = new_claude_ctx(&state, &mut route_body)
            .await
            .expect("explicit route must bypass all-open active queue");
        assert_eq!(ctx.provider.id, "b");
        assert_eq!(ctx.get_providers().len(), 1);
        assert_eq!(ctx.current_provider_id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn request_context_missing_target_returns_config_error() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().expect("memory db"));
        let mut active = provider_a_with_route_to_b();
        // Point to missing provider
        active.meta = Some(ProviderMeta {
            claude_subagent_route: Some(ClaudeSubagentRoute {
                provider_id: "missing-target".to_string(),
            }),
            ..ProviderMeta::default()
        });
        db.save_provider("claude", &active).expect("save a");
        db.set_current_provider("claude", "a").expect("current");
        crate::settings::set_current_provider(&AppType::Claude, Some("a")).expect("local");

        let state = build_state(db);
        let mut body = json!({"model": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS});
        let err = new_claude_ctx(&state, &mut body)
            .await
            .err()
            .expect("missing target");
        match err {
            ProxyError::ConfigError(msg) => assert!(msg.contains("was not found"), "{msg}"),
            other => panic!("expected ConfigError, got {other:?}"),
        }
        // Global current unchanged
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("a")
        );
    }

    #[tokio::test]
    #[serial]
    async fn request_context_missing_target_subagent_model_returns_config_error() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().expect("memory db"));
        db.save_provider("claude", &provider_a_with_route_to_b())
            .expect("save a");
        let b_no_sub = Provider::with_id(
            "b".to_string(),
            "Target B".to_string(),
            json!({"env": {"ANTHROPIC_API_KEY": "b-key"}}),
            None,
        );
        db.save_provider("claude", &b_no_sub).expect("save b");
        db.set_current_provider("claude", "a").expect("current");
        crate::settings::set_current_provider(&AppType::Claude, Some("a")).expect("local");

        let state = build_state(db);
        let mut body = json!({"model": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS});
        let err = new_claude_ctx(&state, &mut body)
            .await
            .err()
            .expect("missing subagent model");
        match err {
            ProxyError::ConfigError(msg) => {
                assert!(msg.contains("CLAUDE_CODE_SUBAGENT_MODEL"), "{msg}")
            }
            other => panic!("expected ConfigError, got {other:?}"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn request_context_rejects_target_with_reserved_alias_subagent_model() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().expect("memory db"));
        db.save_provider("claude", &provider_a_with_route_to_b())
            .expect("save a");
        let b_alias = Provider::with_id(
            "b".to_string(),
            "Target B".to_string(),
            json!({
                "env": {
                    "CLAUDE_CODE_SUBAGENT_MODEL": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS
                }
            }),
            None,
        );
        db.save_provider("claude", &b_alias).expect("save b");
        db.set_current_provider("claude", "a").expect("current");
        crate::settings::set_current_provider(&AppType::Claude, Some("a")).expect("local");

        let state = build_state(db);
        let mut body = json!({"model": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS});
        let err = new_claude_ctx(&state, &mut body)
            .await
            .err()
            .expect("reserved alias as target model");
        match err {
            ProxyError::ConfigError(msg) => assert!(msg.contains("reserved alias"), "{msg}"),
            other => panic!("expected ConfigError, got {other:?}"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn request_context_alias_without_route_meta_is_not_cross_provider_route() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().expect("memory db"));
        // Active A has NO foreign route; its real subagent model happens to be the reserved alias
        let active = Provider::with_id(
            "a".to_string(),
            "Active A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "a-key",
                    "CLAUDE_CODE_SUBAGENT_MODEL": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS
                }
            }),
            None,
        );
        let other = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({"env": {"CLAUDE_CODE_SUBAGENT_MODEL": "b-sub"}}),
            None,
        );
        db.save_provider("claude", &active).expect("save a");
        db.save_provider("claude", &other).expect("save b");
        db.set_current_provider("claude", "a").expect("current");
        crate::settings::set_current_provider(&AppType::Claude, Some("a")).expect("local");

        let state = build_state(db);
        let mut body = json!({"model": CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS});
        let ctx = new_claude_ctx(&state, &mut body)
            .await
            .expect("alias without route is ordinary model");

        // Stays on active A; body model not rewritten to a foreign target
        assert_eq!(ctx.provider.id, "a");
        assert_eq!(body["model"], CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS);
        assert!(!ctx.suppress_global_provider_switch);
        assert_eq!(ctx.request_model, CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS);
    }

    #[tokio::test]
    #[serial]
    async fn request_context_non_alias_path_unchanged() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().expect("memory db"));
        seed_active_a_route_to_b(&db).await;
        let state = build_state(db);

        let mut body = json!({"model": "claude-sonnet-4-6"});
        let ctx = new_claude_ctx(&state, &mut body)
            .await
            .expect("normal path");

        assert_eq!(ctx.provider.id, "a");
        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(ctx.request_model, "claude-sonnet-4-6");
        assert!(!ctx.suppress_global_provider_switch);
        assert_eq!(ctx.current_provider_id, "a");
    }
}
