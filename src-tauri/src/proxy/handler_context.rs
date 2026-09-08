//! 请求上下文模块
//!
//! 提供请求生命周期的上下文管理，封装通用初始化逻辑

use crate::app_config::AppType;
use crate::provider::Provider;
use crate::proxy::{
    extract_session_id,
    forwarder::{ForwardPolicy, RequestForwarder},
    log_codes::pin as log_pin,
    provider_router::PinnedProvider,
    server::ProxyState,
    types::{AppProxyConfig, CopilotOptimizerConfig, OptimizerConfig, RectifierConfig},
    ProxyError,
};
use axum::http::HeaderMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// 会话级「钉住供应商」请求头
///
/// 客户端把它带上即可让**这一个会话**定向到指定供应商（Claude Code 用
/// `ANTHROPIC_CUSTOM_HEADERS` 注入即可），值为 provider id 或供应商名称。
/// 不带、或值为空串时行为不变，仍走默认路由（当前供应商 / 故障转移队列）。
///
/// 钉住只约束对话本体：Auto Mode 安全分类器请求仍按分类器队列分流（分流优先
/// 于钉住），队列不可用时才回落到被钉的供应商。
///
/// 这个头只在代理内部消费，由 forwarder 从出站请求里剔除，不会泄漏给上游。
pub const PROVIDER_PIN_HEADER: &str = "x-cc-provider";

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
    /// 本次请求的分类器判定结果
    pub classifier: ClassifierPlan,
    /// 本次请求由 `x-cc-provider` 钉死了供应商
    pub provider_pinned: bool,
}

/// 本次请求的分类器判定结果
#[derive(Debug, Clone, Default)]
pub struct ClassifierPlan {
    /// 实际由分类器队列供给 provider 链（false = 未命中，或已回落到常规路由链）
    pub routed: bool,
    /// 发送前强制关闭 thinking
    pub thinking_off: bool,
    /// provider_id -> 出站模型名覆写（只含队列里真正配了覆写的成员）
    ///
    /// 用 `Arc` 是因为这张表会随 `ForwardPolicy` 一起被克隆到转发器，
    /// 而它在一次请求内是只读的。
    pub models: Arc<HashMap<String, String>>,
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
        let mut app_config = state
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

        // 会话级钉住：客户端显式点名了供应商，直接锁定为唯一候选。
        let pin = headers.get(PROVIDER_PIN_HEADER).map(decode_header_value);
        let pinned_provider = match pin.as_deref().map(str::trim).filter(|pin| !pin.is_empty()) {
            Some(pin) => {
                match state
                    .provider_router
                    .resolve_pinned_provider(app_type_str, pin)
                    .map_err(|e| ProxyError::DatabaseError(e.to_string()))?
                {
                    PinnedProvider::Found(provider) => {
                        log::info!(
                            "[{tag}] [{code}] {PROVIDER_PIN_HEADER}={pin} → {name} ({id})",
                            code = log_pin::RESOLVED,
                            name = provider.name,
                            id = provider.id,
                        );
                        Some(*provider)
                    }
                    // 这里必须硬失败。静默回落到默认供应商，等于把用户明确点名的会话
                    // 打到另一个账号上、扣另一份额度，而客户端完全看不出来 ——
                    // 一个拼错的名字应当当场报错，而不是变成一笔记错账。
                    PinnedProvider::NotFound => {
                        log::warn!(
                            "[{tag}] [{code}] {PROVIDER_PIN_HEADER}={pin} 未匹配到任何 {app_type_str} 供应商",
                            code = log_pin::UNKNOWN,
                        );
                        return Err(ProxyError::InvalidRequest(format!(
                            "{PROVIDER_PIN_HEADER}: 未找到供应商 \"{pin}\"（{app_type_str}）"
                        )));
                    }
                    PinnedProvider::Ambiguous(count) => {
                        log::warn!(
                            "[{tag}] [{code}] {PROVIDER_PIN_HEADER}={pin} 匹配到 {count} 个同名供应商",
                            code = log_pin::AMBIGUOUS,
                        );
                        return Err(ProxyError::InvalidRequest(format!(
                            "{PROVIDER_PIN_HEADER}: \"{pin}\" 匹配到 {count} 个同名供应商，请改用 provider id"
                        )));
                    }
                }
            }
            None => None,
        };
        let provider_pinned = pinned_provider.is_some();

        // 分类器判定：只对 Claude 生效。claude-desktop / codex / gemini / grokbuild
        // 的入站体不是 Anthropic Messages 形态，特征签名永远不会命中，直接短路。
        let mut classifier = ClassifierPlan::default();
        let mut classifier_providers: Option<Vec<Provider>> = None;

        if app_type_str == AppType::Claude.as_str()
            && crate::proxy::classifier::is_security_classifier_request(body)
        {
            log::info!(
                "[{tag}] [CLS-001] 命中 Auto Mode 安全分类器请求, model={request_model}, session={session_id}"
            );

            if app_config.classifier_queue_enabled {
                // thinking 关闭是**请求形态**的事，与谁来接这一单无关：分类器请求本来
                // 就不需要 thinking，开着会拖到客户端硬超时、Auto Mode 直接卡住。
                classifier.thinking_off = app_config.classifier_force_thinking_off;

                // 会话钉住（x-cc-provider）不拦截这里的选路：分流优先于钉住。
                match state
                    .provider_router
                    .select_classifier_providers(app_type_str)
                    .await
                {
                    // 空 list 走和 None 一样的回落分支：不把「永不报错」这个保证
                    // 寄托在 select_classifier_providers 的实现细节上 —— 一旦它哪天
                    // 返回 Some(vec![])，这里就会以 NoAvailableProvider 打死分类器请求。
                    Ok(Some(selection)) if !selection.providers.is_empty() => {
                        log::info!(
                            "[{tag}] [CLS-002] 分类器队列接管, {} 个可用供应商, P1={}",
                            selection.providers.len(),
                            selection
                                .providers
                                .first()
                                .map(|p| p.name.as_str())
                                .unwrap_or("-")
                        );
                        classifier.routed = true;
                        classifier.models = Arc::new(selection.models);
                        classifier_providers = Some(selection.providers);
                    }
                    Ok(_) => {
                        log::info!("[{tag}] [CLS-003] 分类器队列为空或全部熔断, 回落到常规路由链");
                    }
                    Err(e) => {
                        log::warn!("[{tag}] [CLS-003] 读取分类器队列失败: {e}, 回落到常规路由链");
                    }
                }
            }
        }

        // 使用共享的 ProviderRouter 选择 Provider（熔断器状态跨请求保持）
        // 注意：只在这里调用一次，结果传递给 forwarder，避免重复消耗 HalfOpen 名额
        //
        // 优先级：分类器队列 > 会话钉住 > 常规路由。
        //
        // 分流压过钉住是用户拍板的语义：x-cc-provider 管的是「这个会话的对话本体走
        // 哪家」，而 Auto Mode 判定请求是会话里的后台杂务，仍归队列接单 —— 否则钉住
        // 一家贵渠道后，每条 Bash 命令的判定请求也得按全价走它。队列为空/全熔断时，
        // 判定请求回落到钉住的供应商（没钉住则走常规链）。
        let providers = match classifier_providers {
            Some(list) => list,
            None => match pinned_provider {
                // 钉住 = 单元素链：转发器据此天然跳过熔断器与故障转移，不会替用户换家
                Some(provider) => vec![provider],
                None => state
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
                    })?,
            },
        };

        if classifier.routed {
            // app_config 是 create_forwarder / streaming_timeout_config / handlers 里
            // 非流式超时的唯一真源；就地改写这份**内存副本**（不写库）即可让
            // 「分类器专属重试 + 短超时」在所有 handler 上自动生效。
            //
            // 必须解开 auto_failover_enabled 这道闸门：它关着时 create_forwarder 会把
            // max_retries 和三个超时全部强制为 0，分类器队列只会试第一家，且永远比
            // 客户端截止晚放弃 —— 特性等于没做。
            //
            // 只在队列真正接管时收紧。回落到常规链路时一个字段都不碰：那条链路是
            // 用户自己配的，把 600 秒超时压到十几秒会把「20 秒能成功」变成「硬失败」，
            // 而客户端本来还愿意等 —— 严格更差。
            let (attempt_timeout, max_retries) =
                crate::proxy::classifier::attempt_budget(providers.len());

            app_config.auto_failover_enabled = true;
            app_config.non_streaming_timeout = attempt_timeout;
            // 分类器请求是非流式的，这两项只在上游意外以流式返回时兜底
            app_config.streaming_first_byte_timeout = attempt_timeout;
            app_config.streaming_idle_timeout = attempt_timeout;
            app_config.max_retries = max_retries;

            log::debug!(
                "[{tag}] [CLS-002] 分类器预算: {} 家可用, 单次 {attempt_timeout}s, max_retries={max_retries}",
                providers.len()
            );
        }

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
            classifier,
            provider_pinned,
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
            ForwardPolicy {
                classifier_thinking_off: self.classifier.thinking_off,
                classifier_routed: self.classifier.routed,
                classifier_models: self.classifier.models.clone(),
                provider_pinned: self.provider_pinned,
            },
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

/// 把请求头的原始字节解成字符串
///
/// 不能用 `HeaderValue::to_str()`：它只接受可见 ASCII，带中文或重音的供应商名
/// 会被判成 `Err`，再 `.ok()` 一下就和「压根没带这个头」无法区分 —— 于是静默
/// 回落到默认供应商，正是钉住这个特性最要杜绝的那种记错账。
///
/// 所以这里自己解码，且**不会失败**：先按 UTF-8（curl / 自定义客户端直传原始
/// 字节），失败再按 latin-1 逐字节（Node 系客户端按 latin-1 落字节，例如 Claude
/// Code 的 `ANTHROPIC_CUSTOM_HEADERS`）。哪怕解出来是一串乱码，也会走到「未找到
/// 供应商」的硬失败分支，而不是变成一次悄悄换家。
fn decode_header_value(value: &axum::http::HeaderValue) -> String {
    match std::str::from_utf8(value.as_bytes()) {
        Ok(text) => text.to_string(),
        Err(_) => value.as_bytes().iter().map(|&b| b as char).collect(),
    }
}

#[cfg(test)]
mod header_decode_tests {
    use super::decode_header_value;
    use axum::http::HeaderValue;

    #[test]
    fn decodes_utf8_and_latin1_without_ever_dropping_the_value() {
        // 可见 ASCII：老路径也能过
        assert_eq!(
            decode_header_value(&HeaderValue::from_static("Provider B")),
            "Provider B"
        );

        // UTF-8 原始字节（curl / 自定义客户端）：`to_str()` 在这里会返回 Err，
        // 而静默丢弃就等于悄悄换家
        let utf8 = HeaderValue::from_bytes("我的备用渠道".as_bytes()).expect("utf-8 header value");
        assert!(utf8.to_str().is_err(), "前提：to_str 确实吃不下非 ASCII");
        assert_eq!(decode_header_value(&utf8), "我的备用渠道");

        // latin-1 字节（Node 系客户端按 latin-1 落字节，如 ANTHROPIC_CUSTOM_HEADERS）
        let latin1 = HeaderValue::from_bytes(&[b'C', b'a', b'f', 0xE9]).expect("latin-1 header");
        assert_eq!(decode_header_value(&latin1), "Café");
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
    use super::extract_gemini_model_from_path;

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
