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
use serde_json::Value;
use std::time::Instant;

/// 流式超时配置
#[derive(Debug, Clone, Copy)]
pub struct StreamingTimeoutConfig {
    /// 首字节超时（秒），0 表示禁用
    pub first_byte_timeout: u64,
    /// 静默期超时（秒），0 表示禁用
    pub idle_timeout: u64,
}

/// Codex-only: 请求体 model 字段含 `@<toml_id>` 后缀、已被路由到目标 provider 时，
/// 记录剥离后的模型名与目标 id。`None` 表示未触发（不剥离、原样转发）。
///
/// 设计要点：
/// - 后缀剥离发生在 `select_providers` **之后**：原 failover 链可能含不支持后缀
///   语义的 provider；剥离后允许 failover 用剥离名继续（不会把 `claude-opus-5@kxpms`
///   原样发到不知后缀为何物的本地网关）。
/// - 鉴权按**目标端点**取 key：进入本 dispatch 路径后 `ctx.provider` 已经是 target，
///   forwarder 通过 `adapter.extract_auth(provider)` 取 key，不会沿用 active 端点的。
/// - target_toml_id 同时作为「四轮新增的 legacy `custom` 别名不算独立端点」的边界
///   信号——`@custom` 不应作为目标（继承 RFC 0002 §2.4b 的剔除规则）。
#[derive(Debug, Clone)]
pub struct EndpointDispatch {
    pub stripped_model: String,
    /// 冗余字段——主要用作日志与未来断言。`stripped_model` 已足够触发剥离逻辑，
    /// 但保留 target_toml_id 让测试与日志能直接断言「最终打到了哪个 provider」。
    #[allow(dead_code)]
    pub target_toml_id: String,
}

/// 解析 `<model>@<toml_id>` 后缀。返回 `(Some(toml_id), stripped_model)` 当且仅当：
/// - model 末尾含 `@<id>`，`rsplit_once` 保证前缀里的 `@` 不会误命中；
/// - id 非空、字符集限于 `[A-Za-z0-9_-]`（TOML bare key 合法子集），避免把
///   URL fragment / 邮箱 / 路径里的 `@` 当 toml_id；
/// - 前缀非空（不接受 `@local` 这种「没真模型名」的形式）。
pub fn parse_model_endpoint_suffix(model: &str) -> (Option<&str>, &str) {
    let Some((prefix, suffix)) = model.rsplit_once('@') else {
        return (None, model);
    };
    if suffix.is_empty() || prefix.is_empty() {
        return (None, model);
    }
    if !suffix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return (None, model);
    }
    (Some(suffix), prefix)
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
    /// Codex-only: `@<toml_id>` 后缀分发元数据，None 表示未触发。
    pub endpoint_dispatch: Option<EndpointDispatch>,
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
        let mut request_model = body
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

        // 使用共享的 ProviderRouter 选择 Provider（熔断器状态跨请求保持）
        // 注意：只在这里调用一次，结果传递给 forwarder，避免重复消耗 HalfOpen 名额
        let mut providers = state
            .provider_router
            .select_providers(app_type_str)
            .await
            .map_err(|e| match e {
                crate::error::AppError::AllProvidersCircuitOpen => {
                    ProxyError::AllProvidersCircuitOpen
                }
                crate::error::AppError::NoProvidersConfigured => ProxyError::NoProvidersConfigured,
                _ => ProxyError::DatabaseError(e.to_string()),
            })?;

        let mut provider = providers
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

        // Codex-only: 解析 `@<toml_id>` 后缀分发。必须在 select_providers **之后**
        // 调，原 failover 链可能含不支持后缀语义的 provider；剥离后允许 failover 用
        // 剥离名继续。target_toml_id 同时是「四轮新增的 legacy `custom` 别名不算
        // 独立端点」的边界——`@custom` 不应作为目标（继承 RFC 0002 §2.4b 规则）。
        let mut endpoint_dispatch: Option<EndpointDispatch> = None;
        if matches!(app_type, AppType::Codex) {
            // 先把解析结果复制成 owned String——避免后续 reassign request_model 时
            // 还持有 `&request_model` 的借用。
            let (suffix_target, suffix_stripped): (Option<String>, String) =
                match parse_model_endpoint_suffix(&request_model) {
                    (Some(id), stripped) => (Some(id.to_string()), stripped.to_string()),
                    (None, original) => (None, original.to_string()),
                };
            if let Some(toml_id) = suffix_target {
                if toml_id == "custom" {
                    log::warn!(
                        "[{tag}] @custom 是四轮引入的 legacy 别名（RFC 0002 §2.4b），不作为独立目标；忽略后缀"
                    );
                } else if let Some(idx) = providers.iter().position(|p| p.id == toml_id) {
                    let target = providers[idx].clone();
                    let mut reordered = Vec::with_capacity(providers.len());
                    reordered.push(target.clone());
                    reordered.extend(
                        providers
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| *i != idx)
                            .map(|(_, p)| p.clone()),
                    );
                    provider = target;
                    providers = reordered;
                    request_model = suffix_stripped.clone();
                    log::info!(
                        "[{tag}] @<toml_id> dispatch: stripped={suffix_stripped} target={toml_id} chain_len={}",
                        providers.len()
                    );
                    endpoint_dispatch = Some(EndpointDispatch {
                        stripped_model: suffix_stripped,
                        target_toml_id: toml_id,
                    });
                } else {
                    log::warn!(
                        "[{tag}] @<toml_id> suffix found ({suffix_stripped}@{toml_id}) but {toml_id} is not in current providers; falling back to active"
                    );
                }
            }
        }

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
            endpoint_dispatch,
        })
    }

    /// Codex-only: 如果 ctx 已应用 `@<toml_id>` dispatch，把 body.model 改写为剥离
    /// 后的名字。handler 必须在 `forward_with_retry` 之前调用本方法，否则上游会
    /// 收到带后缀的模型名（多数 provider 不识别，触发 4xx）。
    ///
    /// 非 Codex app 上永远是 no-op（`endpoint_dispatch` 永远 None）。
    pub fn strip_endpoint_suffix_from_body(&self, body: &mut Value) {
        if let Some(dispatch) = &self.endpoint_dispatch {
            if let Some(model_field) = body.get_mut("model") {
                if let Some(model_str) = model_field.as_str() {
                    if model_str == dispatch.stripped_model {
                        // body 已经是剥离后的状态（罕见，但避免无谓覆盖）
                        return;
                    }
                }
                *model_field = Value::String(dispatch.stripped_model.clone());
            }
        }
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
    use super::{extract_gemini_model_from_path, parse_model_endpoint_suffix};

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

    // ── parse_model_endpoint_suffix（@<toml_id> 分发入口）─────────────────

    #[test]
    fn parse_suffix_accepts_alnum_underscore_dash() {
        let (id, stripped) = parse_model_endpoint_suffix("claude-opus-5@kxpms");
        assert_eq!(id, Some("kxpms"));
        assert_eq!(stripped, "claude-opus-5");
    }

    #[test]
    fn parse_suffix_accepts_digits_in_id() {
        // 本机端点的 id 是 `local8782`（名字里的 8782 是历史遗留，不是端口契约
        // ——端口由 base_url 承载）。解析器必须接受 id 里的数字。
        let (id, stripped) = parse_model_endpoint_suffix("claude-opus-5@local8782");
        assert_eq!(id, Some("local8782"));
        assert_eq!(stripped, "claude-opus-5");
    }

    #[test]
    fn parse_suffix_keeps_rsplit_behavior_for_inner_at_signs() {
        // 内嵌 `@` 不应误命中——只剥末尾。
        let (id, stripped) = parse_model_endpoint_suffix("user@example@kxpms");
        assert_eq!(id, Some("kxpms"));
        assert_eq!(stripped, "user@example");
    }

    #[test]
    fn parse_suffix_rejects_empty_id() {
        let (id, stripped) = parse_model_endpoint_suffix("claude-opus-5@");
        assert_eq!(id, None);
        assert_eq!(stripped, "claude-opus-5@");
    }

    #[test]
    fn parse_suffix_rejects_empty_model() {
        let (id, stripped) = parse_model_endpoint_suffix("@kxpms");
        assert_eq!(id, None);
        assert_eq!(stripped, "@kxpms");
    }

    #[test]
    fn parse_suffix_rejects_url_punctuation() {
        // URL/邮箱里的 `@` 不应被当成 toml_id 边界。带 `.`、`:``/` 等都不是合法 TOML bare key。
        for bad in [
            "foo@host/path",
            "foo@host:8080",
            "foo@v1.beta",
            "foo@host.com",
        ] {
            let (id, _) = parse_model_endpoint_suffix(bad);
            assert_eq!(id, None, "should reject {bad:?}");
        }
    }

    #[test]
    fn parse_suffix_accepts_plain_model_without_suffix() {
        let (id, stripped) = parse_model_endpoint_suffix("claude-opus-5");
        assert_eq!(id, None);
        assert_eq!(stripped, "claude-opus-5");
    }

    /// 解析器的字符集不能因为自家 id 的形状而收窄：本机端点 id 是 `local8782`
    /// （含数字），第三方 / 历史 provider 也可能是 `qwen3-v2`、`v2` 这类形式。
    #[test]
    fn parse_suffix_accepts_id_with_digits() {
        let (id, stripped) = parse_model_endpoint_suffix("glm-5.2@qwen3-v2");
        assert_eq!(id, Some("qwen3-v2"));
        assert_eq!(stripped, "glm-5.2");
    }
}
