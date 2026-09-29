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

        let mut endpoint_dispatch: Option<EndpointDispatch> = None;
        // Codex-only：解析 `@<toml_id>` 后缀分发。必须在 select_providers **之后**
        // 调，原 failover 链可能含不支持后缀语义的 provider；剥离后允许 failover 用
        // 剥离名继续。target_toml_id 同时是「四轮新增的 legacy `custom` 别名不算
        // 独立端点」的边界——`@custom` 不应作为目标（继承 RFC 0002 §2.4b 规则）。
        //
        // 【为什么这个门控必须留着，删之前先读】
        // 产出 `@<id>` slug 的只有 Codex 的 merged catalog（`append_endpoint_suffixed_entries`），
        // Claude / Gemini 从来不会收到带后缀的模型名。所以本门控当前的实际影响面接近零——
        // **但它不是冗余**，它把「后缀语义」这件事的边界钉在了唯一真正使用它的 app 上。
        // 若将来给 Claude/Gemini 也做多端点 catalog，删这个门会让它们的请求被一个
        // 属于 Codex 的后缀语法重新路由，而它们的转发出口（`handle_gemini` 走
        // `forward_with_retry` 但**不**调 `strip_endpoint_suffix_from_body`）不会剥离后缀，
        // 结果是 `model@id` 字面量打到上游。
        // 反向也成立：Gemini 的模型名在 URI 路径里、body 无 `model` 字段，
        // `request_model` 恒为 `"unknown"`，即使门控被删也解析不出后缀。
        if matches!(app_type, AppType::Codex) {
            // 先把解析结果复制成 owned String——避免后续 reassign request_model 时
            // 还持有 `&request_model` 的借用。
            let (suffix_target, suffix_stripped): (Option<String>, String) =
                match parse_model_endpoint_suffix(&request_model) {
                    (Some(id), stripped) => (Some(id.to_string()), stripped.to_string()),
                    (None, original) => (None, original.to_string()),
                };
            if let Some(toml_id) = suffix_target {
                // 比的是 **TOML 表 id**，不是 cc-switch 的 provider id。两者是不同的
                // 东西：kaixuan bundle 的 provider id 是 `kaixuan-kxpms`，而 catalog
                // slug 里带的是表 id `kxpms`。按 provider.id 匹配会让真实用户永远
                // 匹配不上（而用 id==toml_id 造夹具的测试却是绿的）。
                let provider_toml_id = |p: &Provider| -> Option<String> {
                    p.settings_config
                        .get("config")
                        .and_then(|v| v.as_str())
                        .and_then(crate::codex_config::codex_provider_toml_id)
                };
                if toml_id == "custom" {
                    // `custom` 是第四轮为兼容老 session 留的**别名**（RFC 0002 §2.4b），
                    // 一义多指，不算独立端点。这里既不能分发，也不能把字面量
                    // `xxx@custom` 原样发上游——那等于请求一个不存在的模型名，
                    // 网关会 404 或静默回退到别的模型。直接报错。
                    log::error!(
                        "[{tag}] model '{suffix_stripped}@{toml_id}' 的后缀指向 legacy 别名 custom，不作为独立端点"
                    );
                    return Err(ProxyError::ConfigError(format!(
                        "model '{suffix_stripped}@{toml_id}' 无法路由：custom 是兼容别名，不是独立端点"
                    )));
                } else if let Some(idx) = providers
                    .iter()
                    .position(|p| provider_toml_id(p).as_deref() == Some(toml_id.as_str()))
                {
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
                    // 目标**不在** failover 链里。故障转移关闭时 `select_providers`
                    // 只返回当前 provider，所以「不在链里」是常态而不是异常——
                    // 必须回 DB 按 id 直取，否则带后缀的请求会被**静默送回 active
                    // 端点**：用户以为选了 local8782 的模型，实际打到 kxpms。
                    // 这正是第三轮判定的「静默错路由比报错更糟」。
                    //
                    // 查不到则**直接报错**，绝不回退到 active：catalog 里能看见的
                    // 条目必须真的可达，够不着的应该让用户看见。
                    // 故障转移关闭时链里只有当前 provider，所以要回 DB **按 TOML 表 id
                    // 扫一遍**（不是 `get_provider_by_id(toml_id)`——那是 provider id）。
                    let resolved = state
                        .db
                        .get_all_providers(app_type_str)
                        .ok()
                        .and_then(|all| {
                            all.into_iter()
                                .find(|(_, p)| {
                                    p.settings_config
                                        .get("config")
                                        .and_then(|v| v.as_str())
                                        .and_then(crate::codex_config::codex_provider_toml_id)
                                        .as_deref()
                                        == Some(toml_id.as_str())
                                })
                                .map(|(_, p)| p)
                        });
                    match resolved {
                        Some(target) => {
                            provider = target.clone();
                            providers.insert(0, target);
                            request_model = suffix_stripped.clone();
                            log::info!(
                                "[{tag}] @<toml_id> dispatch (out of chain): stripped={suffix_stripped} target={toml_id}"
                            );
                            endpoint_dispatch = Some(EndpointDispatch {
                                stripped_model: suffix_stripped,
                                target_toml_id: toml_id,
                            });
                        }
                        None => {
                            log::error!(
                                "[{tag}] @<toml_id> 指向未知端点 {toml_id}（model={suffix_stripped}），拒绝静默回退到 active 端点"
                            );
                            return Err(ProxyError::ConfigError(format!(
                                "model '{suffix_stripped}@{toml_id}' 指向的端点不存在（provider id={toml_id}）"
                            )));
                        }
                    }
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
    use crate::provider::Provider;
    use serde_json::Value;

    /// 假上游记录下来的「实际收到的请求」——端到端断言全部读它，
    /// 而不是读 proxy 的内部状态。
    #[derive(Debug, Clone, PartialEq)]
    struct Observed {
        model: String,
        authorization: Option<String>,
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

    // ── 端到端：真起两个上游 + 真起 proxy + 真发请求 ────────────────────
    //
    // 纯函数级的 `merged_catalog_enabled_path_produces_well_formed_suffixed_entries`
    // 只证明「catalog 写出了带后缀的 slug」，**不证明**那个 slug 真的能被路由。
    // 第三轮审计正是栽在「形状对但链路没通」上：catalog 里写得出
    // `claude-opus-5@kxpms`，但请求发出去没人剥离、没人分发。
    //
    // 这条把整条链路真跑一遍：
    //   Codex 请求体 model = "claude-opus-5@local8782"
    //     → proxy 解析后缀 → 按 id 选中 local8782
    //     → 转发到 local8782 的上游（**不是** active 的 kxpms）
    //     → 上游收到的 model 已被剥离成 "claude-opus-5"
    //     → 上游收到的 Authorization 是 local8782 的 key，不是 kxpms 的
    //
    // 断言「打到另一端点」+「后缀被剥离」+「鉴权取目标端点」三件事，都从**真实
    // 上游收到的请求**里读，而不是从 proxy 的内部状态里读。

    /// 起一个只会把收到的 `model` 与 `Authorization` 原样回显的假上游。
    /// 返回 `(base_url, mpsc::Receiver<Observed>)`。
    async fn spawn_recording_upstream(
        tag: &'static str,
    ) -> (String, tokio::sync::mpsc::Receiver<Observed>) {
        let (tx, rx) = tokio::sync::mpsc::channel::<Observed>(4);
        // 用 fallback 接住**任意**路径：适配器把 base_url 拼成 `{base}/responses`
        // 还是 `{base}/v1/responses` 取决于实现细节，测试不该绑死这个形状。
        let app = axum::Router::new().fallback(
            axum::routing::post(move |headers: axum::http::HeaderMap, body: axum::Json<Value>| {
                let tx = tx.clone();
                let tag = tag;
                async move {
                    let body = body.0;
                    let observed = Observed {
                        model: body
                            .get("model")
                            .and_then(|m| m.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        authorization: headers
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string),
                    };
                    let _ = tx.send(observed).await;
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "id": "resp_test",
                            "object": "response",
                            "status": "completed",
                            "output": [{
                                "type": "message",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": format!("from {tag}")}]
                            }],
                            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                        })),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}/v1"), rx)
    }

    /// 按**生产真实形状**造一个 codex provider。
    ///
    /// 注意 `provider_id` 与 **TOML 表 id 是两个不同的东西**：kaixuan bundle 的
    /// provider id 是 `kaixuan-kxpms` / `kaixuan-local-8782`，而写进
    /// `[model_providers.*]` 的表 id 是 `kxpms` / `local8782`，而 `@<id>` 后缀
    /// 携带的是**表 id**。
    ///
    /// 第一版夹具把两者设成同一个值，于是「按 provider.id 匹配后缀」这个错误
    /// 在测试里是绿的、在真实用户那里永远匹配不上。这里刻意保持分离。
    fn codex_provider(provider_id: &str, toml_id: &str, base_url: &str, api_key: &str) -> Provider {
        Provider::with_id(
            provider_id.to_string(),
            provider_id.to_string(),
            serde_json::json!({
                "config": format!(
                    "model_provider = \"{toml_id}\"\n\n[model_providers.{toml_id}]\n\
                     name = \"{toml_id}_gateway\"\nbase_url = \"{base_url}\"\n\
                     wire_api = \"responses\"\nrequires_openai_auth = true\n"
                ),
                "auth": {"OPENAI_API_KEY": api_key},
            }),
            None,
        )
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial_test::serial]
    async fn suffixed_model_is_dispatched_to_named_endpoint_with_its_own_key() {
        // 共享 env 锁：全 crate 共享那把，见 crate::test_support。
        let _env_guard = crate::test_support::env_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let prev_home = std::env::var_os("HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
        std::env::set_var("HOME", tmp.path());

        let result = run_dispatch_e2e().await;

        match prev_test_home {
            Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        result.expect("端到端分发失败");
    }

    async fn run_dispatch_e2e() -> Result<(), String> {
        use crate::app_config::AppType;
        use crate::database::Database;
        use crate::store::AppState;
        use std::sync::Arc;

        // 1) 两个**真实**上游，各自记录收到的请求
        let (kxpms_base, mut kxpms_rx) = spawn_recording_upstream("kxpms").await;
        let (local_base, mut local_rx) = spawn_recording_upstream("local").await;

        // 2) 两个 codex provider：active = kxpms，另一个 = local8782
        let db = Arc::new(Database::memory().map_err(|e| e.to_string())?);
        let db2 = db.clone();
        db.save_provider(
            AppType::Codex.as_str(),
            &codex_provider("kaixuan-kxpms", "kxpms", &kxpms_base, "sk-kxpms-only"),
        )
        .map_err(|e| e.to_string())?;
        db.save_provider(
            AppType::Codex.as_str(),
            &codex_provider("kaixuan-local-8782", "local8782", &local_base, "sk-local-only"),
        )
        .map_err(|e| e.to_string())?;
        db.set_current_provider(AppType::Codex.as_str(), "kaixuan-kxpms")
            .map_err(|e| e.to_string())?;

        // 关掉故障转移，确保「打到 local8782」只能由后缀分发解释，而不是队列顺序
        let mut cfg = db.get_proxy_config_for_app(AppType::Codex.as_str()).await
            .map_err(|e| e.to_string())?;
        cfg.auto_failover_enabled = false;
        db.update_proxy_config_for_app(cfg).await.map_err(|e| e.to_string())?;

        // 3) 真起 proxy（OS 分配端口）
        // per-app 开关打开（照抄 update_current_claude_desktop... 的写法）
        {
            let mut cfg = db2
                .get_proxy_config_for_app(AppType::Codex.as_str())
                .await
                .map_err(|e| e.to_string())?;
            cfg.enabled = true;
            db2.update_proxy_config_for_app(cfg)
                .await
                .map_err(|e| e.to_string())?;
        }
        // OS 分配端口：开发者本地正跑着 cc-switch 桌面端时，固定端口必红
        {
            let mut cfg = db2.get_proxy_config().await.map_err(|e| e.to_string())?;
            cfg.listen_port = 0;
            db2.update_proxy_config(cfg).await.map_err(|e| e.to_string())?;
        }

        let state = AppState::new(db);
        let info = state.proxy_service.start().await.map_err(|e| e.to_string())?;

        // 4) 真发一条 Codex 请求，模型名带 @<toml_id> 后缀
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{}/v1/responses", info.port))
            .json(&serde_json::json!({
                "model": "claude-opus-5@local8782",
                "input": "ping",
                "stream": false,
            }))
            .send()
            .await
            .map_err(|e| format!("发请求失败: {e}"))?;
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("proxy 返回 {status}: {body_text}"));
        }
        // 诊断：两个上游各自是否收到
        let kxpms_hit = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            kxpms_rx.recv(),
        ).await.ok().flatten();
        if kxpms_hit.is_some() {
            return Err(format!("请求打到了 active(kxpms) 端点。body={body_text}"));
        }

        // 5) 断言：**local8782 的上游**收到了请求
        let observed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            local_rx.recv(),
        )
        .await
        .map_err(|_| "local8782 上游没收到任何请求——后缀分发没生效".to_string())?
        .ok_or_else(|| "local8782 上游 channel 已关闭".to_string())?;

        let _ = kxpms_hit;

        // 后缀必须在转发前被剥离
        let _ = &body_text;
        if observed.model != "claude-opus-5" {
            return Err(format!(
                "上游收到的 model = {:?}，期望已剥离成 \"claude-opus-5\"",
                observed.model
            ));
        }
        // 鉴权必须按**目标端点**取，不能沿用 active 的 key
        let auth = observed.authorization.unwrap_or_default();
        if !auth.contains("sk-local-only") {
            return Err(format!(
                "上游收到的 Authorization = {auth:?}，期望含 local8782 的 key 而不是 kxpms 的"
            ));
        }
        if auth.contains("sk-kxpms-only") {
            return Err("鉴权沿用了 active 端点的 key".to_string());
        }

        let _ = state.proxy_service.stop().await;
        Ok(())
    }

    /// 第四轮为兼容老 session 留下的 legacy `custom` 是**别名**、不是独立端点
    /// （RFC 0002 §2.4b）。`@custom` 必须**报错**，不能"记一条 warn 就跳过"——
    /// 跳过的后果是 body 里的 `model` 仍是 `claude-opus-5@custom` 原样发给上游，
    /// 等于请求一个不存在的模型名：网关会 404，或者更糟——静默回退到别的模型，
    /// 用户拿到一个"看起来能用"的错误结果。
    ///
    /// 这条路径在本轮把关掉开关默认打开后变得真实可达（第三方 OpenAI 兼容预设的
    /// 表 id 常常就是 `custom`）。
    #[tokio::test]
    async fn suffixed_model_pointing_at_legacy_custom_alias_is_rejected() {
        // 共享 env 锁：全 crate 共享那把，见 crate::test_support。
        let _env_guard = crate::test_support::env_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let prev_home = std::env::var_os("HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
        std::env::set_var("HOME", tmp.path());

        let result = run_custom_alias_rejection_e2e().await;

        match prev_test_home {
            Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        result.expect("@custom 必须被拒绝而不是被转发");
    }

    async fn run_custom_alias_rejection_e2e() -> Result<(), String> {
        use crate::app_config::AppType;
        use crate::database::Database;
        use crate::store::AppState;
        use std::sync::Arc;

        // 两个真实上游，断言**一个都没收到**。
        let (_active_base, mut active_rx) = spawn_recording_upstream("kxpms").await;
        let (_other_base, mut other_rx) = spawn_recording_upstream("customish").await;

        let db = Arc::new(Database::memory().map_err(|e| e.to_string())?);
        let db2 = db.clone();
        // 生产真实形状：provider id 与 TOML 表 id 是**两个不同的值**
        // （`kaixuan-kxpms` vs `kxpms`）——把它们设成同值会把这个 bug 藏起来。
        let active_base = _active_base.clone();
        let other_base = _other_base.clone();
        db.save_provider(
            AppType::Codex.as_str(),
            &codex_provider("kaixuan-kxpms", "kxpms", &active_base, "sk-kxpms-only"),
        )
        .map_err(|e| e.to_string())?;
        // 第三方 OpenAI 兼容预设的表 id 就是 `custom`（legacy 别名）
        db.save_provider(
            AppType::Codex.as_str(),
            &codex_provider("kaixuan-legacy", "custom", &other_base, "sk-custom-only"),
        )
        .map_err(|e| e.to_string())?;
        db.set_current_provider(AppType::Codex.as_str(), "kaixuan-kxpms")
            .map_err(|e| e.to_string())?;

        let mut cfg = db.get_proxy_config_for_app(AppType::Codex.as_str()).await
            .map_err(|e| e.to_string())?;
        cfg.auto_failover_enabled = false;
        db.update_proxy_config_for_app(cfg).await.map_err(|e| e.to_string())?;

        {
            let mut cfg = db2
                .get_proxy_config_for_app(AppType::Codex.as_str())
                .await
                .map_err(|e| e.to_string())?;
            cfg.enabled = true;
            db2.update_proxy_config_for_app(cfg).await.map_err(|e| e.to_string())?;
        }
        {
            let mut cfg = db2.get_proxy_config().await.map_err(|e| e.to_string())?;
            cfg.listen_port = 0;
            db2.update_proxy_config(cfg).await.map_err(|e| e.to_string())?;
        }

        let state = AppState::new(db);
        let info = state.proxy_service.start().await.map_err(|e| e.to_string())?;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{}/v1/responses", info.port))
            .json(&serde_json::json!({
                "model": "claude-opus-5@custom",
                "input": "ping",
                "stream": false,
            }))
            .send()
            .await
            .map_err(|e| format!("发请求失败: {e}"))?;
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        let _ = state.proxy_service.stop().await;

        // 核心断言 1：必须**报错**，不能看起来像成功。
        if status.is_success() {
            return Err(format!(
                "@custom 被静默受理了（HTTP {status}）：{body_text}——别名不是独立端点，必须报错"
            ));
        }

        // 核心断言 2：两个上游都**不能**收到请求。
        // 只断言状态码是不够的：即使 proxy 回了 5xx，只要它已经把带后缀的
        // body 转发出去，字面量 `claude-opus-5@custom` 就已经打到网关了。
        for (tag, rx) in [("active/kxpms", &mut active_rx), ("custom 端点", &mut other_rx)] {
            if let Ok(Some(observed)) = tokio::time::timeout(
                std::time::Duration::from_millis(400),
                rx.recv(),
            )
            .await
            {
                return Err(format!(
                    "{tag} 上游收到了请求（model={:?}）——@custom 的字面量被转发出去了，\
                     等于向网关请求一个不存在的模型名",
                    observed.model
                ));
            }
        }
        Ok(())
    }
}
