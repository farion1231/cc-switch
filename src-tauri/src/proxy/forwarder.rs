//! 请求转发器
//!
//! 负责将请求转发到上游Provider，支持故障转移

mod copilot;
mod diagnostics;
mod endpoint;
mod headers;
mod managed_auth;
mod opencode;
mod request_body;
mod response_check;
mod send;

use super::hyper_client::{ProxyResponse, MAX_RESPONSE_BODY_BYTES};
use super::{
    body_filter::filter_private_params_with_whitelist,
    content_encoding::{decompress_body_with_limit, get_content_encoding},
    error::*,
    failover_switch::FailoverSwitchManager,
    json_canonical::{canonicalize_value, short_value_hash},
    log_codes::fwd as log_fwd,
    opaque_state_rectifier::{
        carries_encrypted_agent_task, carries_opaque_state, detect_opaque_state_rejection,
        rectify_claude_responses_replay, rectify_opaque_state, unreadable_agent_task_error,
        OpaqueStateRejection,
    },
    provider_router::ProviderRouter,
    providers::{
        codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore, get_adapter,
        AuthInfo, AuthStrategy, ClaudeAdapter, ProviderAdapter, ProviderType,
    },
    thinking_budget_rectifier::{
        rectify_thinking_budget, rectify_thinking_disabled, should_rectify_thinking_budget,
        should_rectify_thinking_disabled,
    },
    thinking_rectifier::{
        normalize_thinking_type, rectify_anthropic_request, should_rectify_thinking_signature,
    },
    types::{
        CopilotOptimizerConfig, OptimizerConfig, PrivacyFilterConfig, ProxyStatus, RectifierConfig,
    },
    ProxyError,
};
use crate::commands::{CodexOAuthState, CopilotAuthState, XaiOAuthState};
use crate::provider_kind::{is_copilot_url, ProviderKind};
use crate::proxy::providers::copilot_auth::{CopilotAuthError, CopilotAuthManager};
use crate::proxy::providers::copilot_model_map::{
    transport_for, CopilotProtocol, CopilotTransport, ResolvedCopilotModel,
};
use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
use crate::{
    app_config::AppType,
    provider::{CodexCopilotApiFormat, LocalProxyRequestOverrides, Provider},
};
use bytes::Bytes;
use copilot::*;
use diagnostics::*;
use endpoint::*;
use futures::StreamExt;
use headers::*;
use http::Extensions;
use opencode::*;
use request_body::*;
use response_check::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::RwLock;

const PROXY_AUTH_PLACEHOLDER: &str = "PROXY_MANAGED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexUpstreamFormat {
    NativeResponses,
    ChatCompletions,
    Anthropic,
}

pub struct ForwardResult {
    pub response: ProxyResponse,
    pub provider: Provider,
    pub claude_api_format: Option<String>,
    /// Wire format selected for this specific Codex Responses request.
    ///
    /// Copilot chooses this per model, so response handlers must consume this
    /// value rather than re-deriving the format from static provider metadata.
    pub codex_upstream_format: Option<CodexUpstreamFormat>,
    /// 实际发往上游的模型名（路由接管/模型映射后的真值）。
    ///
    /// usage 归因不能依赖 ctx.request_model（映射前的客户端别名）：上游响应
    /// 缺失 model 或回显别名时，接管流量会被记成 claude-* 并按其定价计费。
    pub outbound_model: Option<String>,
    /// 活跃连接 RAII guard：随响应一起流转到 response_processor / handle_claude_transform，
    /// 最终被 move 进流式 body future（或非流式响应作用域），覆盖整个响应生命周期。
    pub(crate) connection_guard: Option<ActiveConnectionGuard>,
}

pub struct ForwardError {
    pub error: ProxyError,
    pub provider: Option<Provider>,
    /// 最后一次尝试发往上游的模型名（映射之后）。错误行记这个，不记客户端别名：
    /// 路由下 Desktop 的 claude-haiku-* 实际发的是映射后的模型，记别名会被当成调错了模型。
    pub outbound_model: Option<String>,
}

/// 活跃连接 RAII guard
///
/// 构造时记录请求并把 `ProxyStatus.active_connections` +1；Drop 时优先直接 -1，
/// 锁被占用才调度异步任务，从而支持把 guard move 进流式 body future（stream
/// 自然结束时 guard 与 future 一起 drop）。
///
/// 设计动机：之前在 `forward_with_retry` 出口处同步 -1，但流式响应的 body 实际
/// 在 `create_logged_passthrough_stream` 内还会继续 yield 字节流，导致 UI 的
/// `active_connections` 计数过早归零。RAII guard 让"减量"由 Rust 类型系统驱动，
/// 不需要每条出口路径都手动调用。
pub(crate) struct ActiveConnectionGuard {
    status: Arc<RwLock<ProxyStatus>>,
}

impl ActiveConnectionGuard {
    pub(crate) async fn acquire(status: Arc<RwLock<ProxyStatus>>) -> Self {
        let last_request_at = chrono::Utc::now().to_rfc3339();
        {
            let mut s = status.write().await;
            s.active_connections = s.active_connections.saturating_add(1);
            s.total_requests = s.total_requests.saturating_add(1);
            s.last_request_at = Some(last_request_at);
        }
        Self { status }
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        if let Ok(mut status) = self.status.try_write() {
            status.active_connections = status.active_connections.saturating_sub(1);
            return;
        }
        // Drop 不能 await：把减量操作调度到 tokio runtime
        let status = self.status.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut s = status.write().await;
                s.active_connections = s.active_connections.saturating_sub(1);
            });
        }
        // 没有 runtime 时静默丢失计数（仅 UI 展示用，可接受最终一致性）
    }
}

/// The provider and client request one failover attempt sends; the attempt
/// and its rectifier retries differ only in the body.
#[derive(Clone, Copy)]
struct Attempt<'a> {
    app_type: &'a AppType,
    method: &'a http::Method,
    provider: &'a Provider,
    endpoint: &'a str,
    headers: &'a axum::http::HeaderMap,
    extensions: &'a Extensions,
    adapter: &'a dyn ProviderAdapter,
}

pub struct RequestForwarder {
    /// 共享的 ProviderRouter（持有熔断器状态）
    router: Arc<ProviderRouter>,
    status: Arc<RwLock<ProxyStatus>>,
    current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    gemini_shadow: Arc<GeminiShadowStore>,
    codex_chat_history: Arc<CodexChatHistoryStore>,
    /// 故障转移切换管理器
    failover_manager: Arc<FailoverSwitchManager>,
    /// AppHandle，用于发射事件和更新托盘
    app_handle: Option<tauri::AppHandle>,
    /// 请求开始时的"当前供应商 ID"（用于判断是否需要同步 UI/托盘）
    current_provider_id_at_start: String,
    /// 代理会话 ID（用于 Gemini Native shadow replay）
    session_id: String,
    /// Session ID 是否由客户端提供；生成值不能作为上游缓存身份。
    session_client_provided: bool,
    /// 整流器配置
    rectifier_config: RectifierConfig,
    /// 优化器配置
    optimizer_config: OptimizerConfig,
    /// Copilot 优化器配置
    copilot_optimizer_config: CopilotOptimizerConfig,
    /// 非流式请求超时（秒）
    non_streaming_timeout: std::time::Duration,
    /// 流式请求响应头等待超时（秒）
    streaming_first_byte_timeout: std::time::Duration,
    /// 单个客户端请求最多尝试的 provider 数。
    ///
    /// 由 `AppProxyConfig.max_retries` (UI: "请求失败时的重试次数, 0-10") 派生：
    /// `max_attempts = max_retries + 1`，所以 max_retries=0 表示仅尝试一家、
    /// max_retries=3（默认）表示最多 4 家。loop 同时受 providers.len() 自然限制。
    max_attempts: usize,
    /// Stack 模型的请求（`mode::stack`）：挂在结构体上，整流器重试再次调用 `forward()` 时照样
    /// 生效。见 [`Self::routing_state_enabled`]。
    stack_request: bool,
    /// 这个请求走故障转移队列（故障转移开着）。Codex 换家时据此按目标家改模型名，见
    /// [`super::model_mapper::map_codex_failover_model`]。
    failover_chain: bool,
    /// 最近一次 `forward()` 定下的出站模型名，失败时随 [`ForwardError`] 交给日志。
    /// 每次 `forward()` 开头清空，映射失败等没走到出站的尝试留 None（日志回落到请求名）。
    last_outbound_model: std::sync::Mutex<Option<String>>,
    /// 隐私过滤配置。在 [`Self::forward`] 里对定稿的上游请求体执行，每次尝试都会执行。
    privacy_filter: PrivacyFilterConfig,
}

impl RequestForwarder {
    /// 预防式 media 降级：发送前对 text-only 模型把图片块替换为标记。
    ///
    /// 受 `enabled && request_media_fallback` 管辖；其中"启发式模型名单预测"
    /// 再受 `request_media_heuristic` 单独管辖（显式声明 text-only 始终生效）。
    /// 返回被替换的媒体块数量（0 = 未触发或开关关闭）。
    fn apply_media_prevention(&self, body: &mut Value, provider: &Provider) -> usize {
        if !(self.rectifier_config.enabled && self.rectifier_config.request_media_fallback) {
            return 0;
        }
        let replaced_media = super::media_sanitizer::replace_images_for_text_only_model(
            body,
            provider,
            self.rectifier_config.request_media_heuristic,
        );
        if replaced_media > 0 {
            let model = body.get("model").and_then(Value::as_str).unwrap_or("");
            log::info!(
                "[Media] Replaced {replaced_media} unsupported media block(s) for provider={}, model={}",
                provider.id,
                model
            );
        }
        replaced_media
    }

    /// 反应式 media 重试判定：上游因图片输入报错后，是否应替换图片块并对同一供应商重试一次。
    ///
    /// 受 `enabled && request_media_fallback` 管辖；不涉及 `request_media_heuristic`——
    /// 这里是上游"实测"错误后的纯恢复，不是预测，故启发式开关与它无关。
    fn media_retry_should_trigger(
        &self,
        adapter_name: &str,
        already_retried: bool,
        provider_body: &Value,
        error: &ProxyError,
    ) -> bool {
        matches!(adapter_name, "Claude" | "Codex")
            && self.rectifier_config.enabled
            && self.rectifier_config.request_media_fallback
            && !already_retried
            && super::media_sanitizer::contains_image_blocks(provider_body)
            && super::media_sanitizer::is_unsupported_image_error(error)
    }

    /// 反应式密文重试判定：原样转发的 Responses 上游拒绝了请求里别家签发的密文时，
    /// 返回要去掉哪些状态，对同一供应商重试一次。
    ///
    /// 转成 Chat / Anthropic 的上游收不到这些密文，不参与。受整流器总开关管辖。
    ///
    /// Claude / Claude Desktop 转 Responses 时，上游的推理条目藏在 thinking 签名里回放
    /// （#9424），请求体是 Anthropic Messages，整流用 [`rectify_claude_responses_replay`]。
    /// 这条路径不看上游格式：只有 Responses 上游会签发推理信封，也只有它会用这些措辞拒绝。
    fn opaque_state_retry_rejection(
        &self,
        app_type: &AppType,
        provider: &Provider,
        codex_upstream_format: Option<CodexUpstreamFormat>,
        already_retried: bool,
        request: &Value,
        error: &ProxyError,
    ) -> Option<OpaqueStateRejection> {
        if already_retried {
            return None;
        }
        if matches!(app_type, AppType::Claude | AppType::ClaudeDesktop) {
            return detect_opaque_state_rejection(error, &self.rectifier_config, request, false);
        }
        if !matches!(app_type, AppType::Codex | AppType::GrokBuild)
            || codex_upstream_format != Some(CodexUpstreamFormat::NativeResponses)
        {
            return None;
        }
        let codex_third_party = matches!(app_type, AppType::Codex)
            && !super::providers::is_codex_official_provider(provider);
        detect_opaque_state_rejection(error, &self.rectifier_config, request, codex_third_party)
    }

    /// 压缩回合被原生 Responses 第三方以 400 拒绝时，去掉 `tool_choice` 的重试请求体。
    fn compaction_tool_choice_retry_body(
        &self,
        app_type: &AppType,
        provider: &Provider,
        codex_upstream_format: Option<CodexUpstreamFormat>,
        request: &Value,
        error: &ProxyError,
    ) -> Option<Value> {
        if !matches!(app_type, AppType::Codex)
            || codex_upstream_format != Some(CodexUpstreamFormat::NativeResponses)
            || super::providers::is_codex_official_provider(provider)
            || !matches!(error, ProxyError::UpstreamError { status: 400, .. })
        {
            return None;
        }
        super::providers::codex_compaction::compaction_retry_without_tool_choice(request)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        router: Arc<ProviderRouter>,
        non_streaming_timeout: u64,
        status: Arc<RwLock<ProxyStatus>>,
        current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
        gemini_shadow: Arc<GeminiShadowStore>,
        codex_chat_history: Arc<CodexChatHistoryStore>,
        failover_manager: Arc<FailoverSwitchManager>,
        app_handle: Option<tauri::AppHandle>,
        current_provider_id_at_start: String,
        session_id: String,
        session_client_provided: bool,
        streaming_first_byte_timeout: u64,
        _streaming_idle_timeout: u64,
        rectifier_config: RectifierConfig,
        optimizer_config: OptimizerConfig,
        copilot_optimizer_config: CopilotOptimizerConfig,
        max_retries: u32,
    ) -> Self {
        // max_retries 是「失败后重试次数」语义，attempt 上限 = retries + 1。
        // saturating_add 防止 u32::MAX + 1 溢出。
        let max_attempts = (max_retries as usize).saturating_add(1);
        Self {
            router,
            status,
            current_providers,
            gemini_shadow,
            codex_chat_history,
            failover_manager,
            app_handle,
            current_provider_id_at_start,
            session_id,
            session_client_provided,
            rectifier_config,
            optimizer_config,
            copilot_optimizer_config,
            non_streaming_timeout: std::time::Duration::from_secs(non_streaming_timeout),
            streaming_first_byte_timeout: std::time::Duration::from_secs(
                streaming_first_byte_timeout,
            ),
            max_attempts,
            stack_request: false,
            failover_chain: false,
            last_outbound_model: std::sync::Mutex::new(None),
            privacy_filter: PrivacyFilterConfig::default(),
        }
    }

    /// 设置隐私过滤配置
    pub fn privacy_filter(mut self, config: PrivacyFilterConfig) -> Self {
        self.privacy_filter = config;
        self
    }

    /// 隐私过滤：开关打开时脱敏请求体。[`Self::forward`] 每次尝试都会调用：
    ///
    /// 1. `client`：模型映射之后、主要的协议转换和 Copilot 分类 / 合并之前，请求体还是客户端
    ///    格式，分得清用户文本和工具结果（Copilot 会把用户文本并进 tool_result）。更早的
    ///    Bedrock 预处理、MiMo 历史整理、分类器重试检查只在本地读写，不外发内容。
    /// 2. `copilot`（只有 Copilot 优化器运行时）：孤儿 tool_result 降级成文本、tool_result 合并
    ///    之后，确定性请求 ID 之前，让降级出来的文本也先过滤再参与哈希。
    /// 3. `override`（只有供应商配置了本地 body 覆写时）：合并前过滤覆写的临时副本，见
    ///    [`Self::privacy_filtered_overrides`]。
    /// 4. `upstream`：所有变换（格式转换、历史回填、压缩摘要回放、本地 body 覆写）之后、
    ///    日志哈希和序列化之前，兜住代理自己补进请求的内容。转换会把孤儿工具结果降级成用户
    ///    文本，这些内容在这一遍按用户文本过滤（宁可多脱敏）。
    ///
    /// 内容派生的标识（OpenCode 会话指纹、Copilot 确定性请求 ID）仍是对已过滤正文算 SHA-256。
    /// 替换结果与原文相同的命中不改写、不计数。占位符跟随界面语言（没设过语言时按系统区域，
    /// 与托盘一致）。
    fn apply_privacy_filter(&self, app_type: &AppType, body: &mut Value, stage: &str) {
        if !self.privacy_filter.enabled {
            return;
        }
        let language = crate::tray::tray_language();
        let placeholders = super::privacy_filter::Placeholders::for_language(&language);
        let count = super::privacy_filter::redact_request_body(
            body,
            placeholders,
            self.privacy_filter.filter_tool_results,
        );
        if count > 0 {
            log::info!(
                "[{}] Privacy filter redacted {} item(s) in {} body, session: {}",
                app_type.as_str(),
                count,
                stage,
                self.session_id
            );
        }
    }

    /// Copilot 优化的第 2、3 步：清洗孤立 tool_result（降级成文本）、合并 tool_result 与文本，
    /// 然后对结果再过滤一遍隐私，降级出来的文本在参与确定性请求 ID 之前就已经脱敏。
    fn copilot_rewrite_tool_results(&self, app_type: &AppType, body: Value) -> Value {
        let mut body = super::copilot_optimizer::sanitize_orphan_tool_results(body);
        if self.copilot_optimizer_config.tool_result_merging {
            body = super::copilot_optimizer::merge_tool_results(body);
        }
        self.apply_privacy_filter(app_type, &mut body, "copilot");
        body
    }

    /// 本地 body 覆写是在客户端那一遍之后才合并进请求体的内容，合并前先过滤一份临时副本。
    /// 持久化的供应商配置不改；只用到 body，headers 不参与合并。
    fn privacy_filtered_overrides(
        &self,
        app_type: &AppType,
        overrides: &LocalProxyRequestOverrides,
    ) -> LocalProxyRequestOverrides {
        let mut body = overrides.body.clone();
        if let Some(body) = body.as_mut() {
            self.apply_privacy_filter(app_type, body, "override");
        }
        LocalProxyRequestOverrides {
            headers: Default::default(),
            body,
        }
    }

    fn set_last_outbound_model(&self, model: Option<String>) {
        if let Ok(mut slot) = self.last_outbound_model.lock() {
            *slot = model;
        }
    }

    fn last_outbound_model(&self) -> Option<String> {
        self.last_outbound_model
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }

    /// 标记为 Stack 模型的请求。
    pub fn stack_request(mut self, stack_request: bool) -> Self {
        self.stack_request = stack_request;
        self
    }

    /// 这个请求走故障转移队列（故障转移开着）。
    pub fn failover_chain(mut self, failover_chain: bool) -> Self {
        self.failover_chain = failover_chain;
        self
    }

    /// 故障转移换家时按目标家改 Codex 模型名，只对对话请求。图片、搜索这些独立接口带的是
    /// 图片 / 搜索模型，不在目标家的文本模型目录里，换成它的文本模型就发错了。
    fn maps_codex_failover_model(&self, app_type: &AppType, endpoint: &str) -> bool {
        self.failover_chain
            && matches!(app_type, AppType::Codex)
            && CodexStandaloneEndpoint::from_effective_endpoint(endpoint).is_none()
    }

    /// 这个请求读写路由状态吗：熔断器（许可、结果、健康度）、「正在使用」、代理统计
    /// （总数、成功率、故障转移次数、最近错误、活跃连接）、故障转移切换。Stack 模型的请求
    /// 和普通路由完全分开，一律不碰；只有用量日志照常按供应商记。以后新增路由状态也先问它。
    fn routing_state_enabled(&self) -> bool {
        !self.stack_request
    }

    /// 请求体里的模型名已经是这家的上游名（Stack 模型的请求）：不按角色映射、不换成行里配置
    /// 的模型。以后新增模型改写也先问它。
    fn keeps_resolved_model(&self) -> bool {
        self.stack_request
    }

    async fn record_success_result(
        &self,
        provider_id: &str,
        app_type: &AppType,
        used_half_open_permit: bool,
    ) {
        // 名额本身由这次尝试的 HalfOpenPermitGuard 归还，这里只记结果，所以传 false。
        if used_half_open_permit {
            if let Err(e) = self
                .router
                .record_result(provider_id, app_type, false, true, None)
                .await
            {
                log::warn!(
                    "[{app_type}] 记录 Provider 成功结果失败: provider_id={provider_id}, error={e}"
                );
            }
            return;
        }

        let router = self.router.clone();
        let provider_id = provider_id.to_string();
        let app_type = app_type.clone();
        tokio::spawn(async move {
            if let Err(e) = router
                .record_result(&provider_id, &app_type, false, true, None)
                .await
            {
                log::warn!(
                    "[{app_type}] 异步记录 Provider 成功结果失败: provider_id={provider_id}, error={e}"
                );
            }
        });
    }

    /// 整流（thinking signature 或 budget）重试失败后的统一收尾。
    ///
    /// `None` 表示已记录熔断器、累积 `last_error`/`last_provider`，
    /// 调用方应 `continue` 让下一家 provider 继续故障转移；
    /// `Some(ForwardError)` 表示是客户端错误，没有 provider 能修复，
    /// 调用方应直接 `return` 把错误返回给客户端。
    async fn handle_rectifier_retry_failure(
        &self,
        retry_err: ProxyError,
        provider: &Provider,
        app_type: &AppType,
        rectifier_label: &str,
        last_error: &mut Option<ProxyError>,
        last_provider: &mut Option<Provider>,
    ) -> Option<ForwardError> {
        // Provider 错误：本家上游/网络确实出问题，下一家 provider 可能可用 → 继续故障转移。
        // 客户端错误：整流后请求仍违法，下一家也修不好 → 直接返回。
        let is_provider_error = match &retry_err {
            ProxyError::Timeout(_) | ProxyError::ForwardFailed(_) => true,
            ProxyError::UpstreamError { status, .. } => *status >= 500,
            _ => false,
        };

        if is_provider_error {
            self.record_provider_failure(
                provider,
                app_type,
                &retry_err,
                format!(
                    "Provider {} {rectifier_label}重试失败: {}",
                    provider.name, retry_err
                ),
            )
            .await;
            *last_error = Some(retry_err);
            *last_provider = Some(provider.clone());
            return None;
        }

        Some(self.finish_neutral_failure(retry_err, provider).await)
    }

    // 一次尝试和一个客户端请求的收尾记账（熔断器、「正在使用」、代理统计、故障转移切换）
    // 都经下面这几个方法，不在重试循环里直接写。

    /// 开始尝试一家：界面上显示正在尝试哪家。
    async fn note_attempt(&self, provider: &Provider) {
        if !self.routing_state_enabled() {
            return;
        }
        let mut status = self.status.write().await;
        status.current_provider = Some(provider.name.clone());
        status.current_provider_id = Some(provider.id.clone());
    }

    /// 请求成功：记熔断器成功，更新「正在使用」和成功统计。实际用的不是请求开始时那家，
    /// 就计一次故障转移，并异步把当前供应商切过去（同步 UI/托盘）。
    async fn finish_success(
        &self,
        provider: &Provider,
        app_type: &AppType,
        used_half_open_permit: bool,
        (response, claude_api_format, outbound_model): (
            ProxyResponse,
            Option<String>,
            Option<String>,
        ),
        codex_upstream_format: Option<CodexUpstreamFormat>,
    ) -> ForwardResult {
        let result = ForwardResult {
            response,
            provider: provider.clone(),
            claude_api_format,
            codex_upstream_format,
            outbound_model,
            connection_guard: None,
        };
        if !self.routing_state_enabled() {
            return result;
        }

        // 普通闭合熔断状态异步记录，避免阻塞流式首包返回；
        // HalfOpen 探测仍同步等待，保证熔断状态在归还名额前就转回 Closed。
        self.record_success_result(&provider.id, app_type, used_half_open_permit)
            .await;

        {
            let mut current_providers = self.current_providers.write().await;
            current_providers.insert(
                app_type.to_string(),
                (provider.id.clone(), provider.name.clone()),
            );
        }

        {
            let mut status = self.status.write().await;
            status.success_requests += 1;
            status.last_error = None;
            if self.current_provider_id_at_start.as_str() != provider.id.as_str() {
                status.failover_count += 1;

                let fm = self.failover_manager.clone();
                let ah = self.app_handle.clone();
                let pid = provider.id.clone();
                let pname = provider.name.clone();
                let at = app_type.to_string();
                tokio::spawn(async move {
                    let _ = fm.try_switch(ah.as_ref(), &at, &pid, &pname).await;
                });
            }
            refresh_success_rate(&mut status);
        }

        result
    }

    /// 这家本身出了问题（下一家可能可用）：记熔断器失败，更新最近错误。调用方接着试下一家。
    async fn record_provider_failure(
        &self,
        provider: &Provider,
        app_type: &AppType,
        error: &ProxyError,
        last_error: String,
    ) {
        if !self.routing_state_enabled() {
            return;
        }
        let _ = self
            .router
            .record_result(
                &provider.id,
                app_type,
                false,
                false,
                Some(error.to_string()),
            )
            .await;
        self.status.write().await.last_error = Some(last_error);
    }

    /// 不计入熔断的失败（客户端错误、整流后仍被拒）：不记熔断器，这个请求记为失败。
    /// 占用的 HalfOpen 名额随这次尝试的 guard 归还。
    async fn finish_neutral_failure(&self, error: ProxyError, provider: &Provider) -> ForwardError {
        if self.routing_state_enabled() {
            self.record_failed_request(error.to_string()).await;
        }
        ForwardError {
            error,
            provider: Some(provider.clone()),
            outbound_model: self.last_outbound_model(),
        }
    }

    /// 这个客户端请求最终失败：失败数加一，记下错误，重算成功率。
    async fn record_failed_request(&self, last_error: String) {
        if !self.routing_state_enabled() {
            return;
        }
        let mut status = self.status.write().await;
        status.failed_requests += 1;
        status.last_error = Some(last_error);
        refresh_success_rate(&mut status);
    }

    /// 转发请求（带故障转移）
    ///
    /// 这是 thin wrapper：在客户端请求维度记一次 `total_requests` / 调整
    /// `active_connections` / 刷新 `last_request_at`，无论 inner 走哪条出口路径，
    /// 出口处都会把 `active_connections` 回收。Per-attempt 维度（成功/失败/熔断
    /// 等）仍由 inner 内自行更新 `success_requests` / `failed_requests`。
    #[allow(clippy::too_many_arguments)]
    pub async fn forward_with_retry(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        body: &Value,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: &[Provider],
    ) -> Result<ForwardResult, ForwardError> {
        if !self.routing_state_enabled() {
            return self
                .forward_with_retry_inner(
                    app_type, method, endpoint, body, headers, extensions, providers,
                )
                .await;
        }
        let guard = ActiveConnectionGuard::acquire(self.status.clone()).await;
        let result = self
            .forward_with_retry_inner(
                app_type, method, endpoint, body, headers, extensions, providers,
            )
            .await;
        // 把 guard 注入到 Ok 结果，让它随响应一起流转到 response_processor，
        // 在流式 body 的 future 内才真正 drop。
        // Err 路径：guard 在函数 scope 内随返回值落地时自动 drop。
        result.map(|mut fr| {
            fr.connection_guard = Some(guard);
            fr
        })
    }

    /// 实际转发逻辑（不包含客户端维度的入口/出口计数）
    ///
    /// # Arguments
    /// * `app_type` - 应用类型
    /// * `method` - 客户端请求的 HTTP 方法（透传给上游，支持 GET/POST 等）
    /// * `endpoint` - API 端点
    /// * `body` - 请求体
    /// * `headers` - 请求头
    /// * `providers` - 已选择的 Provider 列表（由 RequestContext 提供，避免重复调用 select_providers）
    #[allow(clippy::too_many_arguments)]
    async fn forward_with_retry_inner(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        body: &Value,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: &[Provider],
    ) -> Result<ForwardResult, ForwardError> {
        if matches!(app_type, AppType::Claude | AppType::ClaudeDesktop)
            && super::classifier_guard::classifier_retry_loop(body)
        {
            let error = ProxyError::InvalidRequest(
                super::classifier_guard::CLASSIFIER_LOOP_MESSAGE.to_string(),
            );
            self.record_failed_request(error.to_string()).await;
            return Err(ForwardError {
                error,
                provider: providers.first().cloned(),
                outbound_model: None,
            });
        }
        // 获取适配器
        let adapter = get_adapter(app_type).ok_or_else(|| ForwardError {
            error: ProxyError::ConfigError(format!(
                "{} does not support proxy routing",
                app_type.as_str()
            )),
            provider: None,
            outbound_model: None,
        })?;
        let app_type_str = app_type.as_str();

        if providers.is_empty() {
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
                outbound_model: None,
            });
        }

        let mut last_error = None;
        let mut last_provider = None;
        let mut attempted_providers = 0usize;

        // 单 Provider 场景下跳过熔断器检查（故障转移关闭时）；Stack 模型的请求不碰熔断器。
        let bypass_circuit_breaker = providers.len() == 1 || !self.routing_state_enabled();

        // 依次尝试每个供应商
        for provider in providers.iter() {
            // 整流器重试标记：每个 provider 独立持有，避免标记跨 provider 短路故障转移
            // —— 首家 provider 整流后被 5xx/timeout 击落时，下家仍能用整流后的请求体走整流流程
            let mut rectifier_retried = false;
            let mut budget_rectifier_retried = false;
            let mut media_rectifier_retried = false;
            let mut opaque_rectifier_retried = false;

            // 上限检查：尊重用户在 AppProxyConfig.max_retries 上配置的「重试次数」。
            // 放在熔断器 allow 检查之前，避免在已经超限时还占用 HalfOpen 探测名额。
            if attempted_providers >= self.max_attempts {
                log::warn!(
                    "[{app_type_str}] 已达最大尝试次数上限 ({}/{}), 停止故障转移",
                    attempted_providers,
                    self.max_attempts
                );
                break;
            }

            // 发起请求前先获取熔断器放行许可（HalfOpen 会占用探测名额）
            // 单 Provider 场景下跳过此检查，避免熔断器阻塞所有请求
            // 占用了 HalfOpen 名额时，`_half_open_permit` 一直持有到这次尝试结束（continue /
            // return，或客户端断开把整个 future 丢掉），drop 时归还名额。
            let (allowed, used_half_open_permit, _half_open_permit) = if bypass_circuit_breaker {
                (true, false, None)
            } else {
                let permit = self
                    .router
                    .allow_provider_request(&provider.id, app_type)
                    .await;
                (permit.allowed, permit.used_half_open_permit, Some(permit))
            };

            if !allowed {
                // 熔断器没放行（Open 未到恢复时间，或 HalfOpen 的探测名额被占用）：
                // 这家没有发出请求，留一行日志，和「试了但失败」区分开。
                log::info!(
                    "[{app_type_str}] [{}] Provider {} ({}) 熔断器未放行，跳过",
                    log_fwd::CIRCUIT_SKIPPED,
                    provider.name,
                    provider.id
                );
                continue;
            }

            // PRE-SEND 优化器：每个 provider 独立决定是否优化
            // 只有优化器或错误整流要改写时才复制，避免每家额外复制整段会话历史。
            let mut provider_body = Cow::Borrowed(body);
            if self.optimizer_config.enabled && is_bedrock_provider(provider) {
                if self.optimizer_config.thinking_optimizer {
                    super::thinking_optimizer::optimize(
                        provider_body.to_mut(),
                        &self.optimizer_config,
                    );
                }
                if self.optimizer_config.cache_injection {
                    super::cache_injector::inject(provider_body.to_mut(), &self.optimizer_config);
                }
            }

            attempted_providers += 1;

            // 更新状态中的当前 Provider 信息（per-attempt 维度的标识）
            //
            // total_requests / last_request_at / active_connections 已由
            // forward_with_retry wrapper 在客户端请求维度统一处理，这里只刷
            // 新「正在尝试哪个 provider」的展示字段。
            self.note_attempt(provider).await;
            let attempt = Attempt {
                app_type,
                method: &method,
                provider,
                endpoint,
                headers: &headers,
                extensions: &extensions,
                adapter: adapter.as_ref(),
            };

            // 转发请求（每个 Provider 只尝试一次，重试由客户端控制）
            let mut attempted_codex_upstream_format = None;
            match self
                .forward(
                    &attempt,
                    &provider_body,
                    &mut attempted_codex_upstream_format,
                )
                .await
            {
                Ok(forwarded) => {
                    return Ok(self
                        .finish_success(
                            provider,
                            app_type,
                            used_half_open_permit,
                            forwarded,
                            attempted_codex_upstream_format,
                        )
                        .await);
                }
                Err(mut e) => {
                    // 检测是否需要触发整流器（上游为 Anthropic Messages 的供应商）
                    let is_anthropic_provider = uses_anthropic_rectifiers(
                        matches!(app_type, AppType::Claude | AppType::ClaudeDesktop)
                            .then(|| ClaudeAdapter::new().provider_type(provider)),
                    );
                    let mut signature_rectifier_non_retryable_client_error = false;

                    if self.media_retry_should_trigger(
                        adapter.name(),
                        media_rectifier_retried,
                        &provider_body,
                        &e,
                    ) {
                        let mut media_body = provider_body.as_ref().clone();
                        let replaced_images =
                            super::media_sanitizer::replace_image_blocks_with_marker(
                                &mut media_body,
                            );

                        if replaced_images > 0 {
                            let _ = std::mem::replace(&mut media_rectifier_retried, true);
                            let model = media_body
                                .get("model")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            log::info!(
                                "[{app_type_str}] [Media] Upstream rejected image input; retrying provider={} model={} with {replaced_images} image block(s) replaced by {}",
                                provider.id,
                                model,
                                super::media_sanitizer::UNSUPPORTED_IMAGE_MARKER
                            );

                            let mut media_retry_codex_upstream_format = None;
                            match self
                                .forward(
                                    &attempt,
                                    &media_body,
                                    &mut media_retry_codex_upstream_format,
                                )
                                .await
                            {
                                Ok(forwarded) => {
                                    log::info!(
                                        "[{app_type_str}] [Media] Unsupported-image retry succeeded"
                                    );
                                    return Ok(self
                                        .finish_success(
                                            provider,
                                            app_type,
                                            used_half_open_permit,
                                            forwarded,
                                            media_retry_codex_upstream_format,
                                        )
                                        .await);
                                }
                                Err(retry_err) => {
                                    log::warn!(
                                        "[{app_type_str}] [Media] Unsupported-image retry still failed: {retry_err}"
                                    );
                                    if let Some(err) = self
                                        .handle_rectifier_retry_failure(
                                            retry_err,
                                            provider,
                                            app_type,
                                            "media 降级",
                                            &mut last_error,
                                            &mut last_provider,
                                        )
                                        .await
                                    {
                                        return Err(err);
                                    }
                                    continue;
                                }
                            }
                        }
                    }

                    // 压缩回合被原生 Responses 第三方拒绝：有的上游不收 `tool_choice: "none"`
                    // （见 `compaction_retry_without_tool_choice`），去掉它对同一家重试一次。
                    if let Some(retry_body) = self.compaction_tool_choice_retry_body(
                        app_type,
                        provider,
                        attempted_codex_upstream_format,
                        &provider_body,
                        &e,
                    ) {
                        log::info!(
                            "[{app_type_str}] [RECT-030] provider={} 拒绝了压缩请求，去掉 tool_choice 后重试一次",
                            provider.id
                        );
                        let mut retry_codex_upstream_format = None;
                        match self
                            .forward(&attempt, &retry_body, &mut retry_codex_upstream_format)
                            .await
                        {
                            Ok(forwarded) => {
                                log::info!("[{app_type_str}] [RECT-031] 压缩重试成功");
                                return Ok(self
                                    .finish_success(
                                        provider,
                                        app_type,
                                        used_half_open_permit,
                                        forwarded,
                                        retry_codex_upstream_format,
                                    )
                                    .await);
                            }
                            // 重试仍失败：按这次的错误走常规分类。
                            Err(retry_err) => {
                                log::warn!(
                                    "[{app_type_str}] [RECT-032] 压缩重试仍失败: {retry_err}"
                                );
                                e = retry_err;
                            }
                        }
                    }

                    if let Some(rejection) = self.opaque_state_retry_rejection(
                        app_type,
                        provider,
                        attempted_codex_upstream_format,
                        opaque_rectifier_retried,
                        &provider_body,
                        &e,
                    ) {
                        // 这一轮是派给子 agent 的加密任务：整流不动它（见
                        // `opaque_state_rectifier`），去掉别家的状态后仍被拒，就是任务本身解不开。
                        let agent_task = carries_encrypted_agent_task(&provider_body);
                        let mut opaque_body = provider_body.as_ref().clone();
                        let rectified =
                            if matches!(app_type, AppType::Claude | AppType::ClaudeDesktop) {
                                rectify_claude_responses_replay(&mut opaque_body, rejection)
                            } else {
                                rectify_opaque_state(&mut opaque_body, rejection)
                            };
                        if !rectified.applied && agent_task {
                            log::warn!(
                                "[{app_type_str}] [RECT-023] provider={} 解不开子 agent 任务的密文（主、子 agent 不在同一家），不重试",
                                provider.id
                            );
                            return Err(self
                                .finish_neutral_failure(unreadable_agent_task_error(), provider)
                                .await);
                        }
                        if rectified.applied {
                            let _ = std::mem::replace(&mut opaque_rectifier_retried, true);
                            log::info!(
                                "[{app_type_str}] [RECT-020] 上游拒绝了请求里别家签发的状态，去掉 {} 个推理条目、{} 个加密片段、{} 个别家 id，换掉 {} 个压缩条目后对 provider={} 重试一次",
                                rectified.removed_reasoning_items,
                                rectified.replaced_encrypted_parts,
                                rectified.removed_foreign_ids,
                                rectified.replaced_compaction_items,
                                provider.id
                            );

                            let mut opaque_retry_codex_upstream_format = None;
                            match self
                                .forward(
                                    &attempt,
                                    &opaque_body,
                                    &mut opaque_retry_codex_upstream_format,
                                )
                                .await
                            {
                                Ok(forwarded) => {
                                    log::info!("[{app_type_str}] [RECT-021] 密文整流重试成功");
                                    return Ok(self
                                        .finish_success(
                                            provider,
                                            app_type,
                                            used_half_open_permit,
                                            forwarded,
                                            opaque_retry_codex_upstream_format,
                                        )
                                        .await);
                                }
                                // 重试仍失败：按这次的错误走常规分类，和没整流过一样
                                // 决定是否计入熔断、是否换下一家。
                                Err(retry_err) => {
                                    log::warn!(
                                        "[{app_type_str}] [RECT-022] 密文整流重试仍失败: {retry_err}"
                                    );
                                    if agent_task
                                        && self
                                            .opaque_state_retry_rejection(
                                                app_type,
                                                provider,
                                                opaque_retry_codex_upstream_format,
                                                false,
                                                &opaque_body,
                                                &retry_err,
                                            )
                                            .is_some()
                                    {
                                        log::warn!(
                                            "[{app_type_str}] [RECT-023] provider={} 解不开子 agent 任务的密文（主、子 agent 不在同一家），不再重试",
                                            provider.id
                                        );
                                        return Err(self
                                            .finish_neutral_failure(
                                                unreadable_agent_task_error(),
                                                provider,
                                            )
                                            .await);
                                    }
                                    e = retry_err;
                                }
                            }
                        }
                    }

                    // 密文整流已经去掉推理块重试过一次：签名整流的「invalid request」兜底
                    // 拿的还是没整流的请求体，再命中就会发第三次（#9424），这里不再叠加。
                    if is_anthropic_provider && !opaque_rectifier_retried {
                        let error_message = extract_error_message(&e);
                        if should_rectify_thinking_signature(
                            error_message.as_deref(),
                            &self.rectifier_config,
                        ) {
                            // 已经重试过：直接返回错误（不可重试客户端错误）
                            if rectifier_retried {
                                log::warn!("[{app_type_str}] [RECT-005] 整流器已触发过，不再重试");
                                // 不记录熔断器，这是客户端兼容性问题
                                return Err(self.finish_neutral_failure(e, provider).await);
                            }

                            // 首次触发：整流请求体
                            let rectified = rectify_anthropic_request(provider_body.to_mut());

                            // 整流未生效：继续尝试 budget 整流路径，避免误判后短路
                            if !rectified.applied {
                                log::warn!(
                                    "[{app_type_str}] [RECT-006] thinking 签名整流器触发但无可整流内容，继续检查 budget；若 budget 也未命中则按客户端错误返回"
                                );
                                signature_rectifier_non_retryable_client_error = true;
                            } else {
                                log::info!(
                                    "[{}] [RECT-001] thinking 签名整流器触发, 移除 {} thinking blocks, {} redacted_thinking blocks, {} signature fields",
                                    app_type_str,
                                    rectified.removed_thinking_blocks,
                                    rectified.removed_redacted_thinking_blocks,
                                    rectified.removed_signature_fields
                                );

                                // 标记已重试（当前逻辑下重试后必定 return，保留标记以备将来扩展）
                                let _ = std::mem::replace(&mut rectifier_retried, true);

                                // 使用同一供应商重试（不计入熔断器）
                                let mut signature_retry_codex_upstream_format = None;
                                match self
                                    .forward(
                                        &attempt,
                                        &provider_body,
                                        &mut signature_retry_codex_upstream_format,
                                    )
                                    .await
                                {
                                    Ok(forwarded) => {
                                        log::info!("[{app_type_str}] [RECT-002] 整流重试成功");
                                        return Ok(self
                                            .finish_success(
                                                provider,
                                                app_type,
                                                used_half_open_permit,
                                                forwarded,
                                                signature_retry_codex_upstream_format,
                                            )
                                            .await);
                                    }
                                    Err(retry_err) => {
                                        log::warn!(
                                            "[{app_type_str}] [RECT-003] 整流重试仍失败: {retry_err}"
                                        );
                                        if let Some(err) = self
                                            .handle_rectifier_retry_failure(
                                                retry_err,
                                                provider,
                                                app_type,
                                                "整流",
                                                &mut last_error,
                                                &mut last_provider,
                                            )
                                            .await
                                        {
                                            return Err(err);
                                        }
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // 检测是否需要触发 budget 整流器（上游为 Anthropic Messages 的供应商）；
                    // 上游拒绝关思考（#8887、#9671）也走这条重试路径
                    if is_anthropic_provider {
                        let error_message = extract_error_message(&e);
                        let budget_hit = should_rectify_thinking_budget(
                            error_message.as_deref(),
                            &self.rectifier_config,
                        );
                        let disabled_hit = !budget_hit
                            && should_rectify_thinking_disabled(
                                error_message.as_deref(),
                                &self.rectifier_config,
                            );
                        if budget_hit || disabled_hit {
                            // 已经重试过：直接返回错误（不可重试客户端错误）
                            if budget_rectifier_retried {
                                log::warn!(
                                    "[{app_type_str}] [RECT-013] budget 整流器已触发过，不再重试"
                                );
                                return Err(self.finish_neutral_failure(e, provider).await);
                            }

                            let budget_rectified = if budget_hit {
                                rectify_thinking_budget(provider_body.to_mut())
                            } else {
                                rectify_thinking_disabled(provider_body.to_mut())
                            };
                            if !budget_rectified.applied {
                                log::warn!(
                                    "[{app_type_str}] [RECT-014] budget 整流器触发但无可整流内容，不做无意义重试"
                                );
                                return Err(self.finish_neutral_failure(e, provider).await);
                            }

                            log::info!(
                                "[{}] [RECT-010] thinking {} 整流器触发, before={:?}, after={:?}",
                                app_type_str,
                                if budget_hit { "budget" } else { "disabled" },
                                budget_rectified.before,
                                budget_rectified.after
                            );

                            let _ = std::mem::replace(&mut budget_rectifier_retried, true);

                            // 使用同一供应商重试（不计入熔断器）
                            let mut budget_retry_codex_upstream_format = None;
                            match self
                                .forward(
                                    &attempt,
                                    &provider_body,
                                    &mut budget_retry_codex_upstream_format,
                                )
                                .await
                            {
                                Ok(forwarded) => {
                                    log::info!("[{app_type_str}] [RECT-011] budget 整流重试成功");
                                    return Ok(self
                                        .finish_success(
                                            provider,
                                            app_type,
                                            used_half_open_permit,
                                            forwarded,
                                            budget_retry_codex_upstream_format,
                                        )
                                        .await);
                                }
                                Err(retry_err) => {
                                    log::warn!(
                                        "[{app_type_str}] [RECT-012] budget 整流重试仍失败: {retry_err}"
                                    );
                                    if let Some(err) = self
                                        .handle_rectifier_retry_failure(
                                            retry_err,
                                            provider,
                                            app_type,
                                            "budget 整流",
                                            &mut last_error,
                                            &mut last_provider,
                                        )
                                        .await
                                    {
                                        return Err(err);
                                    }
                                    continue;
                                }
                            }
                        }
                    }

                    if signature_rectifier_non_retryable_client_error {
                        return Err(self.finish_neutral_failure(e, provider).await);
                    }

                    // 先分类错误，决定是否计入 provider 健康度
                    // —— NonRetryable 是客户端层错误，无论换哪家 provider 都会被拒绝，
                    //    不应污染熔断器和数据库健康度（与 finish_neutral_failure 同语义）。
                    let category = self.categorize_proxy_error(&e, provider);

                    match category {
                        ErrorCategory::Retryable => {
                            // 可重试：真正的 provider 故障 → 记录失败并更新熔断器/DB 健康度
                            self.record_provider_failure(
                                provider,
                                app_type,
                                &e,
                                format!("Provider {} 失败: {}", provider.name, e),
                            )
                            .await;

                            let (log_code, log_message) = build_retryable_failure_log(
                                &provider.name,
                                attempted_providers,
                                providers.len(),
                                &e,
                            );
                            log::warn!("[{app_type_str}] [{log_code}] {log_message}");

                            last_error = Some(e);
                            last_provider = Some(provider.clone());
                            // 继续尝试下一个供应商
                            continue;
                        }
                        ErrorCategory::NonRetryable => {
                            // 不可重试：客户端层错误 → 不污染健康度，仅释放 HalfOpen permit
                            return Err(self.finish_neutral_failure(e, provider).await);
                        }
                    }
                }
            }
        }

        if attempted_providers == 0 {
            // providers 列表非空，但全部被熔断器拒绝（典型：HalfOpen 探测名额被占用）
            self.record_failed_request("所有供应商暂时不可用（熔断器限制）".to_string())
                .await;
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
                outbound_model: None,
            });
        }

        // 所有供应商都失败了
        self.record_failed_request("所有供应商都失败".to_string())
            .await;

        if let Some((log_code, log_message)) =
            build_terminal_failure_log(attempted_providers, providers.len(), last_error.as_ref())
        {
            log::warn!("[{app_type_str}] [{log_code}] {log_message}");
        }

        Err(ForwardError {
            error: last_error.unwrap_or(ProxyError::MaxRetriesExceeded),
            provider: last_provider,
            outbound_model: self.last_outbound_model(),
        })
    }

    /// 转发单个请求（使用适配器）
    ///
    /// 成功时返回 `(response, claude_api_format, outbound_model)`，其中
    /// `outbound_model` 是最终发往上游的模型名（所有映射/改写之后）。
    /// `codex_upstream_format_out` 记下这次实际选中的 Codex 上游格式：成功时交给
    /// `finish_success`，失败时给整流判断用（Copilot 按模型逐次选协议，不能从静态配置推）。
    async fn forward(
        &self,
        attempt: &Attempt<'_>,
        body: &Value,
        codex_upstream_format_out: &mut Option<CodexUpstreamFormat>,
    ) -> Result<(ProxyResponse, Option<String>, Option<String>), ProxyError> {
        let Attempt {
            app_type,
            method,
            provider,
            endpoint,
            headers,
            extensions,
            adapter,
        } = *attempt;
        *codex_upstream_format_out = None;
        self.set_last_outbound_model(None);
        // 使用适配器提取 base_url
        let mut base_url = adapter.extract_base_url(provider)?;

        let is_full_url = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.is_full_url)
            .unwrap_or(false)
            && !provider.is_codex_oauth()
            && !provider.is_xai_oauth()
            && !provider.is_github_copilot();

        // GitHub Copilot API 使用 /chat/completions（无 /v1 前缀）
        let is_copilot = is_managed_copilot_request(app_type, provider, &base_url);

        // Codex upstream conversion mode — computed early because the [1m]-suffix strip
        // below must be skipped on the Anthropic path (the marker has to survive to
        // catalog matching and to the transform's own strip+beta detection).
        let mut codex_responses_to_chat = matches!(app_type, AppType::Codex | AppType::GrokBuild)
            && super::providers::should_convert_codex_responses_to_chat(provider, endpoint);
        let mut codex_responses_to_anthropic =
            matches!(app_type, AppType::Codex | AppType::GrokBuild)
                && super::providers::should_convert_codex_responses_to_anthropic(
                    provider, endpoint,
                );
        let mut copilot_endpoint_override: Option<String> = None;
        let codex_official_auth_passthrough = matches!(app_type, AppType::Codex)
            && super::providers::is_codex_official_provider(provider);
        let codex_stack_request = self.stack_request && matches!(app_type, AppType::Codex);
        let mut quota_token = None;

        if codex_official_auth_passthrough {
            // 暂停使用官方登录时 Codex 带的是占位 Key，没有官方凭据可透传：本地拒绝并说明
            // 怎么恢复，不发往上游。
            if crate::settings::codex_proxy_without_official_login() {
                return Err(ProxyError::CodexOfficialLoginPaused);
            }
            let (expected_chatgpt_account_id, managed_session_matches) = match provider
                .meta
                .as_ref()
                .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
            {
                Some(local_account_id) => {
                    let app_handle = self.app_handle.as_ref().ok_or_else(|| {
                        ProxyError::AuthError("Codex OAuth 认证不可用（无 AppHandle）".to_string())
                    })?;
                    let codex_state = app_handle.state::<CodexOAuthState>();
                    let chatgpt_account_id = codex_state
                        .0
                        .chatgpt_account_id_for_account(&local_account_id)
                        .await
                        .map_err(|error| {
                            ProxyError::AuthError(format!("Codex OAuth 账号解析失败: {error}"))
                        })?;
                    let session_matches = match codex_bearer_access_token(headers) {
                        Some(access_token) => {
                            crate::codex_config::codex_live_auth_matches_managed_request(
                                &local_account_id,
                                access_token,
                            )
                            .map_err(|error| {
                                ProxyError::AuthError(format!("Codex OAuth 会话校验失败: {error}"))
                            })?
                        }
                        None => false,
                    };
                    (Some(chatgpt_account_id), Some(session_matches))
                }
                None => (None, None),
            };
            validate_codex_official_authorization(
                headers,
                provider,
                expected_chatgpt_account_id.as_deref(),
                managed_session_matches,
            )?;
            if method == http::Method::POST {
                let token = codex_bearer_access_token(headers).ok_or_else(|| {
                    ProxyError::AuthError(
                        "Codex official route requires a bearer token".to_string(),
                    )
                })?;
                let request_model = body.get("model").and_then(|value| value.as_str());
                super::codex_quota_protection::admit(token, headers, request_model).await?;
                quota_token = Some(token);
            }
        }

        // 应用模型映射（独立于格式转换）
        // Claude Desktop proxy 模式必须先把 Desktop 可见的 claude-* route
        // 映射成真实上游模型名，并且未知 route 要直接报错，不能使用默认模型兜底。
        // 聚合模型的请求已经解出了上游名，不再查映射表（查不到会报错，按档位回落会发错模型）。
        let mapped_body = if matches!(app_type, AppType::ClaudeDesktop) {
            if self.keeps_resolved_model() {
                crate::claude_desktop_config::normalize_upstream_request(body.clone(), provider)
            } else {
                crate::claude_desktop_config::map_proxy_request_model(body.clone(), provider)
                    .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?
            }
        } else if self.keeps_resolved_model() {
            body.clone()
        } else {
            let (mapped_body, _original_model, _mapped_model) =
                super::model_mapper::apply_model_mapping(body.clone(), provider);
            // Codex 行没有档位映射，客户端发的是路由那家的模型名：故障转移换到不认这个名字的
            // 那家时，换成它自己的模型。
            if self.maps_codex_failover_model(app_type, endpoint) {
                super::model_mapper::map_codex_failover_model(mapped_body, provider)
            } else {
                mapped_body
            }
        };

        // 与 CCH 对齐：请求前不做 thinking 主动改写（仅保留兼容入口）
        let mut mapped_body = normalize_thinking_type(mapped_body);
        // 隐私过滤第一遍：必须在 Copilot 分类 / 合并和内容派生的请求 ID 之前
        self.apply_privacy_filter(app_type, &mut mapped_body, "client");
        let is_copilot_codex_responses = matches!(app_type, AppType::Codex)
            && is_copilot
            && super::providers::is_codex_responses_endpoint(endpoint);
        let is_copilot_claude_body = is_copilot && !matches!(app_type, AppType::Codex);

        // The OpenCode gateway wants a stable per-conversation session id (see the
        // injection site below). It has to be derived here: `mapped_body` is moved
        // by the format conversion further down.
        //
        // Only that upstream consumes it, and only when the client neither sent
        // `x-opencode-session` nor supplied a session id, so guard the work: the
        // digest serialises the system prompt plus the first user message, which
        // routinely carry files and images and can reach megabytes. The host check
        // uses `base_url` - none of the URL-construction branches below rewrites the
        // authority (the only one that replaces the host wholesale is Copilot's
        // dynamic endpoint, which points at GitHub, never opencode.ai).
        let base_url_host = base_url
            .parse::<http::Uri>()
            .ok()
            .and_then(|u| u.authority().map(|a| a.to_string()));
        let opencode_conversation_fingerprint = if !self.session_client_provided
            && !headers.contains_key("x-opencode-session")
            && native_session_header(headers).is_none()
            && is_opencode_upstream(base_url_host.as_deref())
        {
            conversation_fingerprint(&mapped_body)
        } else {
            None
        };

        // Grok Build exposes a stable client-side model profile in config.toml.
        // Route requests to the provider's real upstream model before applying
        // the optional Responses -> Chat/Anthropic bridge.
        if matches!(app_type, AppType::GrokBuild) {
            super::providers::apply_codex_upstream_model(provider, &mut mapped_body);
        }

        if is_copilot_claude_body {
            mapped_body =
                super::providers::copilot_model_map::apply_copilot_model_normalization(mapped_body);
            self.apply_copilot_live_model_resolution(provider, &mut mapped_body)
                .await;
            // Strip the [1M] context marker after Copilot normalization/resolve.
            // A user's mapped value (e.g. "gpt-5.6-sol[1M]") carries [1M] as a
            // Claude Code context-capability declaration that upstream APIs reject
            // as part of the model name. The preceding normalization step already
            // rewrites claude-xxx[1M] into the "-1m" dash form Copilot accepts, and
            // the strip helper only touches the "[1m]" bracket form, so "-1m"
            // variants pass through unchanged.
            mapped_body =
                super::model_mapper::strip_one_m_suffix_for_upstream_from_body(mapped_body);
        } else if !is_copilot_codex_responses && !codex_responses_to_anthropic {
            // Skip on the Codex→Anthropic path: stripping [1m] here would break both the
            // model-catalog match (apply_codex_upstream_model) and the transform's own
            // strip+`context-1m` beta detection. The marker is stripped later, on the
            // final anthropic_body.
            mapped_body =
                super::model_mapper::strip_one_m_suffix_for_upstream_from_body(mapped_body);
        }

        // --- Copilot 优化器：分类 + 请求体优化（在格式转换之前执行） ---
        // 注意：确定性 ID 也在此处计算，因为 mapped_body 在格式转换时会被 move
        //
        // 执行顺序（与 copilot-api 对齐）：
        //   1. 先在原始 body 上分类（保留 tool_result 语义，避免误判为 user）
        //   2. 再清洗孤立 tool_result（防止上游 API 报错）
        //   3. 再合并 tool_result + text（减少 premium 计费）
        let run_copilot_optimizer = is_copilot_claude_body && self.copilot_optimizer_config.enabled;
        let copilot_request_context = if run_copilot_optimizer {
            // 1. 在原始 body 上分类 — 必须在清洗/合并之前执行
            //    孤立 tool_result 仍保持 tool_result 类型，分类能正确识别为 agent
            let has_anthropic_beta = headers.contains_key("anthropic-beta");
            let classification = super::copilot_optimizer::classify_request(
                &mapped_body,
                has_anthropic_beta,
                self.copilot_optimizer_config.compact_detection,
                self.copilot_optimizer_config.subagent_detection,
            );

            log::debug!(
                "[Copilot] 优化器分类: initiator={}, is_warmup={}, is_compact={}, is_subagent={}",
                classification.initiator,
                classification.is_warmup,
                classification.is_compact,
                classification.is_subagent
            );

            // 2. 孤立 tool_result 清理 — 分类完成后再清洗
            //    防止上游 API 因不匹配的 tool_result 报错导致重试/重复计费
            // 3. Tool result 合并 — 将 [tool_result, text] 变为 [tool_result(含text)]
            //    之后再过滤一遍隐私，降级出来的文本先脱敏再参与下面的确定性请求 ID
            mapped_body = self.copilot_rewrite_tool_results(app_type, mapped_body);

            // 4. Warmup 小模型降级
            if self.copilot_optimizer_config.warmup_downgrade && classification.is_warmup {
                log::info!(
                    "[Copilot] Warmup 请求降级到模型: {}",
                    self.copilot_optimizer_config.warmup_model
                );
                mapped_body["model"] =
                    serde_json::json!(&self.copilot_optimizer_config.warmup_model);
            }

            // 预计算确定性 Request ID（在 body 被 move 之前）
            // Session 提取优先级（与 session.rs extract_from_metadata 对齐）：
            //   1. metadata.user_id 中的 _session_ 后缀
            //   2. metadata.session_id（直接字段）
            //   3. raw metadata.user_id（整串 fallback）
            //   4. x-session-id header
            let metadata = body.get("metadata");
            let session_id = metadata
                .and_then(|m| m.get("user_id"))
                .and_then(|v| v.as_str())
                .and_then(super::session::parse_session_from_user_id)
                .or_else(|| {
                    metadata
                        .and_then(|m| m.get("session_id"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    metadata
                        .and_then(|m| m.get("user_id"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    headers
                        .get("x-session-id")
                        .and_then(|v| v.to_str().ok())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            let det_request_id = if self.copilot_optimizer_config.deterministic_request_id {
                Some(super::copilot_optimizer::deterministic_request_id(
                    &mapped_body,
                    &session_id,
                ))
            } else {
                None
            };

            // 从 session ID 派生稳定的 interaction ID（同一主对话共享）
            let interaction_id =
                super::copilot_optimizer::deterministic_interaction_id(&session_id);

            Some(CopilotRequestContext {
                classification,
                request_id: det_request_id,
                interaction_id,
            })
        } else {
            self.codex_copilot_request_context(is_copilot_codex_responses, &mapped_body)
        };

        // Codex always speaks Responses to the local proxy. Resolve the final
        // Copilot model after every model rewrite, then select only a protocol
        // that the model advertises. Messages is intentionally not supported
        // on the Codex bridge. Only auto mode may fall back from Responses to Chat.
        if is_copilot_codex_responses {
            let api_format = CodexCopilotApiFormat::from_meta(provider.meta.as_ref());
            let resolved = self
                .resolve_codex_copilot_model(provider, &mapped_body, api_format)
                .await?;
            let transport = apply_codex_copilot_model(&mut mapped_body, resolved, api_format)?;
            copilot_endpoint_override = Some(transport.endpoint);
            codex_responses_to_chat = matches!(transport.protocol, CopilotProtocol::Chat);
            codex_responses_to_anthropic = false;
        }

        let codex_upstream_format = if matches!(app_type, AppType::Codex | AppType::GrokBuild)
            && super::providers::is_codex_responses_endpoint(endpoint)
        {
            Some(if codex_responses_to_anthropic {
                CodexUpstreamFormat::Anthropic
            } else if codex_responses_to_chat {
                CodexUpstreamFormat::ChatCompletions
            } else {
                CodexUpstreamFormat::NativeResponses
            })
        } else {
            None
        };
        *codex_upstream_format_out = codex_upstream_format;

        // GitHub Copilot 动态 endpoint 路由
        // 从 CopilotAuthManager 获取缓存的 API endpoint（支持企业版等非默认 endpoint）
        if is_copilot && !is_full_url {
            if let Some(app_handle) = &self.app_handle {
                let copilot_state = app_handle.state::<CopilotAuthState>();
                let copilot_auth = copilot_state.0.read().await;

                // 从 provider.meta 获取关联的 GitHub 账号 ID
                let account_id = provider
                    .meta
                    .as_ref()
                    .and_then(|m| m.managed_account_id_for("github_copilot"));

                let dynamic_endpoint = match &account_id {
                    Some(id) => copilot_auth.get_api_endpoint(id).await,
                    None => copilot_auth.get_default_api_endpoint().await,
                };

                // 只在动态 endpoint 与当前 base_url 不同时替换
                if dynamic_endpoint != base_url {
                    log::debug!(
                        "[Copilot] 使用动态 API endpoint: {} (原: {})",
                        dynamic_endpoint,
                        base_url
                    );
                    base_url = dynamic_endpoint;
                }
            }
        }
        let resolved_claude_api_format = if adapter.name() == "Claude" {
            Some(
                self.resolve_claude_api_format(provider, &mapped_body, is_copilot)
                    .await,
            )
        } else {
            None
        };
        if adapter.name() == "Claude" {
            if let Some(api_format) = resolved_claude_api_format.as_deref() {
                // 主动剥离 thinking block — Copilot 的 OpenAI 兼容端点不识别该块，避免上游拒绝后
                // 由 rectifier 反应式重试（首次请求已消耗 quota）。原生 Messages 透传保留它们：
                // 工具循环里 Anthropic 会校验 thinking block。
                if run_copilot_optimizer
                    && self.copilot_optimizer_config.strip_thinking
                    && super::providers::claude_api_format_needs_transform(api_format)
                {
                    mapped_body = super::copilot_optimizer::strip_thinking_blocks(mapped_body);
                }
                super::providers::normalize_anthropic_messages_for_provider(
                    &mut mapped_body,
                    provider,
                    api_format,
                );
                self.apply_media_prevention(&mut mapped_body, provider);
            }
        }
        let needs_transform = match resolved_claude_api_format.as_deref() {
            Some(api_format) => super::providers::claude_api_format_needs_transform(api_format),
            None => adapter.needs_transform(provider),
        };
        // Codex → Anthropic: Claude Code emulation is off by default and only
        // enabled when the user explicitly turns it on in the UI, so requests can
        // pass a gateway's "Claude Code only" fingerprint check (User-Agent /
        // anthropic-beta / x-app / system prompt first line). Defaulting to off
        // avoids leaking the Claude Code fingerprint and identity prompt to
        // general-purpose gateways.
        let codex_impersonate_claude_code = codex_responses_to_anthropic
            && provider
                .meta
                .as_ref()
                .and_then(|meta| meta.impersonate_claude_code)
                == Some(true);
        let (effective_endpoint, passthrough_query) =
            if let Some(copilot_endpoint) = copilot_endpoint_override.as_deref() {
                rewrite_codex_endpoint_for_copilot(endpoint, copilot_endpoint)
            } else if codex_responses_to_chat {
                rewrite_codex_responses_endpoint_to_chat(endpoint)
            } else if codex_responses_to_anthropic {
                rewrite_codex_responses_endpoint_to_anthropic(endpoint)
            } else if needs_transform && adapter.name() == "Claude" {
                let api_format = resolved_claude_api_format
                    .as_deref()
                    .unwrap_or_else(|| super::providers::get_claude_api_format(provider));
                rewrite_claude_transform_endpoint(endpoint, api_format, is_copilot, &mapped_body)
            } else {
                (
                    endpoint.to_string(),
                    split_endpoint_and_query(endpoint)
                        .1
                        .map(ToString::to_string),
                )
            };

        let codex_chat_base_is_full_endpoint =
            codex_responses_to_chat && base_url_is_full_endpoint(&base_url, "/chat/completions");

        // Defensive fallback mirroring `codex_chat_base_is_full_endpoint`: if a user pastes
        // a base URL already ending in the Anthropic `/v1/messages` endpoint but leaves the
        // "full URL" switch off, treat it as a full endpoint so we don't double-append
        // `/v1/messages` (→ `.../v1/messages/v1/messages`, a non-retryable 400). Matches the
        // exact endpoint suffix, so prefixed gateways like `.../api/v1/messages` are covered.
        let codex_anthropic_base_is_full_endpoint =
            codex_responses_to_anthropic && base_url_is_full_endpoint(&base_url, "/v1/messages");

        let codex_standalone_endpoint = matches!(app_type, AppType::Codex)
            .then(|| CodexStandaloneEndpoint::from_effective_endpoint(&effective_endpoint))
            .flatten();
        let is_copilot_unversioned_endpoint = is_copilot
            && matches!(
                split_endpoint_and_query(&effective_endpoint).0,
                "/chat/completions" | "/responses" | "/responses/compact"
            );

        let url = if matches!(resolved_claude_api_format.as_deref(), Some("gemini_native")) {
            super::gemini_url::resolve_gemini_native_url(
                &base_url,
                &effective_endpoint,
                is_full_url,
            )
        } else if is_full_url {
            if let Some(endpoint) = codex_standalone_endpoint {
                rewrite_codex_standalone_full_url(
                    &base_url,
                    passthrough_query.as_deref(),
                    endpoint,
                )?
            } else {
                append_query_to_full_url(&base_url, passthrough_query.as_deref())
            }
        } else if let Some(endpoint) = codex_standalone_endpoint
            .filter(|endpoint| endpoint.base_url_is_source_endpoint(&base_url))
        {
            // Same tolerance as `codex_chat_base_is_full_endpoint` below: a base URL
            // pasted as a complete endpoint with the full-URL switch off would
            // otherwise become `.../chat/completions/images/generations`.
            rewrite_codex_standalone_full_url(&base_url, passthrough_query.as_deref(), endpoint)?
        } else if codex_chat_base_is_full_endpoint || codex_anthropic_base_is_full_endpoint {
            append_query_to_full_url(&base_url, passthrough_query.as_deref())
        } else if is_copilot_unversioned_endpoint {
            build_copilot_unversioned_url(&base_url, &effective_endpoint)
        } else {
            adapter.build_url(&base_url, &effective_endpoint)
        };

        let (listen_address, listen_port) = {
            let status = self.status.read().await;
            (status.address.clone(), status.port)
        };
        super::upstream_guard::reject_self_forwarding(&url, &listen_address, listen_port).await?;

        // 记录映射后的出站模型名（此时 mapped_body 已完成接管映射 / [1m] 剥离 /
        // Copilot 归一化）。格式转换后若 body 仍带 model 字段会在下方刷新覆盖；
        // gemini_native 等模型在 URL 中的格式则保留此处的转换前真值。
        let mut outbound_model = mapped_body
            .get("model")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
            .map(str::to_string);
        self.set_last_outbound_model(outbound_model.clone());

        // Codex→Anthropic: when the model name carries the [1m] marker, strip the
        // suffix and add the context-1m beta header.
        let mut codex_anthropic_one_m = false;

        // 转换请求体（如果需要）
        let mut request_body = if codex_responses_to_chat {
            let mut mapped_body = mapped_body;
            let explicit_prompt_cache_key = mapped_body
                .get("prompt_cache_key")
                .and_then(|value| value.as_str())
                .map(ToString::to_string);
            let restored = self
                .codex_chat_history
                .enrich_request(
                    &mut mapped_body,
                    self.session_client_provided
                        .then_some(self.session_id.as_str()),
                )
                .await;
            if restored > 0 {
                log::debug!(
                    "[Codex] Restored or enriched {restored} cached function call item(s) for Chat upstream"
                );
            }
            if !self.keeps_resolved_model() && !is_copilot_codex_responses {
                super::providers::apply_codex_chat_upstream_model(provider, &mut mapped_body);
            }
            let reasoning_config =
                super::providers::resolve_codex_chat_reasoning_config(provider, &mapped_body);
            // Mistral 的 Chat 端点对未知字段是 `extra_forbidden`：assistant 历史里带
            // reasoning_content 会整条 422。该字段只是 DeepSeek / Kimi 经第三方网关时的兼容
            // 约定，按端点契约优先于模型名；Claude → Chat 路径用的是同一个判断。
            let preserve_reasoning_content = !super::providers::is_mistral_api(&base_url);
            let mut chat_body = super::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning(
                mapped_body,
                reasoning_config.as_ref(),
                preserve_reasoning_content,
            )?;
            let signature_fallbacks =
                super::providers::codex_chat_common::fill_gemini_thought_signature_fallback(
                    &mut chat_body,
                );
            if signature_fallbacks > 0 {
                log::debug!(
                    "[Codex] Filled {signature_fallbacks} missing Gemini thought signature(s) with the skip placeholder"
                );
            }
            super::providers::inject_codex_chat_prompt_cache_key(
                provider,
                &mut chat_body,
                explicit_prompt_cache_key.as_deref(),
                self.session_client_provided
                    .then_some(self.session_id.as_str()),
            );
            chat_body
        } else if codex_responses_to_anthropic {
            let mut mapped_body = mapped_body;
            if !self.keeps_resolved_model() {
                super::providers::apply_codex_upstream_model(provider, &mut mapped_body);
            }
            // Per-provider output ceiling override. Codex does not forward its
            // `model_max_output_tokens` in the request body, so honor the value
            // configured on the provider here — it takes precedence over any
            // request-supplied `max_output_tokens` and over the default below.
            // Injecting it into the body (rather than overriding after transform)
            // lets the thinking-budget clamp size its headroom against the real
            // ceiling too. Kept per-provider to avoid a global large default that
            // would 400 on low-output-ceiling gateways.
            if let Some(max_out) = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.max_output_tokens)
                .filter(|v| *v > 0)
            {
                mapped_body["max_output_tokens"] = Value::from(max_out);
            }
            // Anthropic requires max_tokens, and Codex never sends max_output_tokens,
            // so this default applies to every request without a provider override.
            let mut anthropic_body =
                super::providers::transform_codex_anthropic::responses_request_to_anthropic(
                    mapped_body,
                    super::providers::transform_codex_anthropic::DEFAULT_CODEX_ANTHROPIC_MAX_TOKENS,
                )?;
            // Handle the 1M-context marker [1m]: strip the model-name suffix (the
            // gateway doesn't recognize it) and set the flag so the beta header is
            // added. apply_codex_upstream_model may have just written back a model
            // name carrying [1m] from the provider config, so strip it once more on
            // the final body here.
            if let Some(model) = anthropic_body.get("model").and_then(|v| v.as_str()) {
                let stripped = super::model_mapper::strip_one_m_suffix_for_upstream(model);
                if stripped != model {
                    codex_anthropic_one_m = true;
                    anthropic_body["model"] = Value::String(stripped.to_string());
                }
            }
            if codex_impersonate_claude_code {
                prepend_claude_code_system_prompt(&mut anthropic_body);
            }
            // Enable Anthropic prompt caching (no beta header required). Reuse the
            // configured TTL rather than silently forcing 5m on this conversion path.
            // otherwise system/tools/history are re-sent at full price every round,
            // inflating cost and first-token latency. The injector handles the
            // string→array `system` conversion and the new-breakpoint budget.
            super::cache_injector::inject(
                &mut anthropic_body,
                &codex_anthropic_cache_config(&self.optimizer_config),
            );
            anthropic_body
        } else if needs_transform {
            if adapter.name() == "Claude" {
                let api_format = resolved_claude_api_format
                    .as_deref()
                    .unwrap_or_else(|| super::providers::get_claude_api_format(provider));
                let mut mapped_body = mapped_body;
                // 映射后的模型名不再带「省略 thinking 仍默认开思考」的语义（5.x 的
                // Fable / Mythos / Opus / Sonnet），按客户端原模型补成显式 adaptive，
                // effort 换算才不会当成关闭。
                if matches!(
                    api_format,
                    "openai_chat" | "openai_responses" | "gemini_native"
                ) {
                    super::providers::transform::make_default_adaptive_thinking_explicit(
                        &mut mapped_body,
                        body.get("model").and_then(|m| m.as_str()),
                    );
                }
                super::providers::transform_claude_request_for_api_format(
                    mapped_body,
                    provider,
                    api_format,
                    self.session_client_provided
                        .then_some(self.session_id.as_str()),
                    Some(self.gemini_shadow.as_ref()),
                )?
            } else {
                adapter.transform_request(mapped_body, provider)?
            }
        } else {
            mapped_body
        };

        // Codex 远程压缩，以及同一线程里别家回合留下的状态（见 `codex_compaction`）。
        // Chat / Anthropic 转换在转换器里处理；这里管原样转发的两种上游：官方清掉
        // CC Switch 产出、官方一定会拒的条目，原生 Responses 第三方把压缩触发和
        // CC Switch 包装的压缩摘要改成普通消息。看不出来源的密文原样发出，被拒后由
        // `opaque_state_rectifier` 处理。请求里没有这些条目时逐字节不变。
        if matches!(app_type, AppType::Codex)
            && !codex_responses_to_chat
            && !codex_responses_to_anthropic
        {
            if codex_official_auth_passthrough {
                if super::providers::codex_compaction::scrub_ccswitch_state_for_official(
                    &mut request_body,
                ) {
                    log::debug!(
                        "[Codex] Scrubbed third-party turn state before the official upstream (provider={})",
                        provider.id
                    );
                }
            } else if super::providers::codex_compaction::prepare_native_third_party_request(
                &mut request_body,
            ) {
                log::debug!(
                    "[Codex] Rewrote compaction items for a native Responses upstream (provider={})",
                    provider.id
                );
            }

            // Kimi 会在工具调用后再吐一条空助手消息，下一轮又拒收这段历史；DeepSeek 同样
            // 拒收，别家留在同一线程里的这种排列续到它就会中（见
            // `transform_codex_responses_kimi_history`）。只对这几家上游、只在出现这种排列
            // 时改写。
            if !codex_official_auth_passthrough
                && super::providers::transform_codex_responses_kimi_history::upstream_rejects_messages_inside_tool_calls(
                    &base_url,
                )
                && super::providers::transform_codex_responses_kimi_history::repair_tool_history(
                    &mut request_body,
                )
            {
                log::debug!(
                    "[Codex] Reordered assistant messages inside tool-call groups (provider={})",
                    provider.id
                );
            }

            // 原生 Responses 直连：历史里的 `function_call.arguments` 是模型产出的原始字符串。
            // 模型偶尔会在一个参数对象里重复写同一个键（实测 `_web_run` 的 `time` 两次），
            // 宽松解析（Codex / serde_json）取后值照常执行，但严格校验的上游（火山方舟实测）
            // 会以 `input[189].arguments ... duplicate field "time"` 拒掉整条请求；这条历史
            // 每轮都重放，线程从此不可用。只合并真正含重复键的条目，其余字节不变。
            //
            // 不按厂商收窄：Chat / Anthropic 转换路径对**所有**上游都会 canonical 化
            // `arguments`（见 `transform_codex_chat::responses_function_call_to_chat_tool_call`），
            // 原生透传只是补齐同一层规范化；按域名判断反而会漏掉中转站、BytePlus 和自定义域名。
            // 官方上游不参与：OpenAI 接受重复键，保持请求逐字节原样。
            if !codex_official_auth_passthrough {
                let repaired =
                    super::providers::codex_responses_arguments::dedupe_duplicate_argument_keys(
                        &mut request_body,
                    );
                if repaired > 0 {
                    log::debug!(
                        "[Codex] Merged repeated JSON keys in {repaired} replayed function_call argument object(s) (provider={})",
                        provider.id
                    );
                }
            }
        }

        // Native Responses passthrough to xAI, directly or via Copilot/Grok.
        // One gate so rebase conflicts stay here plus the isolate file, not
        // scattered across sanitizers. Flatten namespaces first; then apply
        // xAI request rewrites (schema, agent_message, unknown models).
        if matches!(app_type, AppType::Codex | AppType::GrokBuild)
            && !codex_responses_to_chat
            && !codex_responses_to_anthropic
            && (!provider.is_github_copilot() || is_copilot_codex_responses)
        {
            apply_native_responses_request_compat(
                provider,
                &mut request_body,
                self.keeps_resolved_model(),
            )?;
        }

        // Codex 以为在跟内置 openai 说话时才发的字段（ChatGPT 后端私有）：第三方与本地原生
        // Responses 网关收到会整条 400（#9042），去掉后和 Codex 对自定义表发的一样。
        if should_strip_codex_openai_only_fields(
            app_type,
            provider,
            codex_responses_to_chat,
            codex_responses_to_anthropic,
        ) && super::providers::strip_codex_openai_only_request_fields(&mut request_body)
        {
            log::debug!(
                "[Codex] Dropped OpenAI-only request fields for a third-party upstream (provider={})",
                provider.id
            );
        }

        // Stack 请求发往拒收托管 `web_search` 的原生 Responses 上游：去掉这个工具，和 Chat、
        // Anthropic 转换丢掉托管工具是同一件事。只对 Stack 请求、只对名单上的上游生效；
        // 路由请求和其他 Stack 请求逐字节不变。
        if codex_stack_request && !codex_responses_to_chat && !codex_responses_to_anthropic {
            let request_model = request_body
                .get("model")
                .and_then(|model| model.as_str())
                .map(str::to_string);
            if super::providers::codex_stack_upstream_rejects_web_search(
                provider,
                request_model.as_deref(),
            ) && super::providers::strip_codex_hosted_web_search(&mut request_body)
            {
                log::debug!(
                    "[Codex] Dropped hosted web_search for a stacked model (provider={})",
                    provider.id
                );
            }
        }

        // Moonshot / Kimi Chat Completions reject `$ref` nodes that carry sibling
        // keywords, which Codex Desktop's built-in tool schemas do (#6867). Move
        // each such `$ref` into `allOf` for that upstream only; every other
        // provider keeps byte-identical tool schemas (prompt-cache prefix intact).
        if codex_responses_to_chat
            && super::providers::transform_codex_chat_moonshot_schema::upstream_requires_ref_sibling_all_of(
                &base_url,
            )
        {
            let rewritten =
                super::providers::transform_codex_chat_moonshot_schema::wrap_ref_siblings_in_chat_tools(
                    &mut request_body,
                );
            if rewritten > 0 {
                log::debug!(
                    "[Codex] Moved `$ref` siblings into allOf for {rewritten} tool schema(s) (Moonshot upstream, provider={})",
                    provider.id
                );
            }
        }

        if matches!(app_type, AppType::Codex | AppType::GrokBuild) {
            self.apply_media_prevention(&mut request_body, provider);
        }

        // 过滤私有参数（以 `_` 开头的字段），防止内部信息泄露到上游
        // 默认使用空白名单，过滤所有 _ 前缀字段
        let mut filtered_body = prepare_upstream_request_body(request_body);
        if !is_copilot {
            if let Some(overrides) = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.local_proxy_request_overrides.as_ref())
            {
                let redacted_overrides;
                let overrides = if self.privacy_filter.enabled && overrides.body.is_some() {
                    redacted_overrides = self.privacy_filtered_overrides(app_type, overrides);
                    &redacted_overrides
                } else {
                    overrides
                };
                if apply_local_proxy_body_overrides(&mut filtered_body, overrides) {
                    filtered_body = prepare_upstream_request_body(filtered_body);
                }
            }
        }
        // 请求体到这里定稿；之后只读不改（日志哈希、序列化）
        self.apply_privacy_filter(app_type, &mut filtered_body, "upstream");
        // 出站 body 定稿后刷新真值（覆盖 Codex chat 上游模型覆写、转换层模型改写）
        if let Some(m) = filtered_body
            .get("model")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
        {
            outbound_model = Some(m.to_string());
        }
        self.set_last_outbound_model(outbound_model.clone());
        log_prompt_cache_trace(
            app_type,
            provider,
            &effective_endpoint,
            resolved_claude_api_format.as_deref(),
            &filtered_body,
            self.session_client_provided,
        );
        let request_is_streaming =
            is_streaming_request(&effective_endpoint, &filtered_body, headers);
        let force_identity_encoding = needs_transform
            || codex_responses_to_chat
            || codex_responses_to_anthropic
            || request_is_streaming;

        // Codex OAuth 需要注入的 ChatGPT-Account-Id（在动态 token 获取期间填充）
        let mut codex_oauth_account_id: Option<String> = None;
        let mut should_send_codex_oauth_session_headers = false;

        // 获取认证头（提前准备，用于内联替换），同时保留仅用于日志脱敏的
        // 精确认证材料。实际日志永远不输出这些值。
        let mut log_secrets: Vec<String> = Vec::new();
        let mut auth_headers = if let Some(mut auth) = adapter.extract_auth(provider) {
            // GitHub Copilot 特殊处理：从 CopilotAuthManager 获取真实 token
            if auth.strategy == AuthStrategy::GitHubCopilot {
                let token = self.copilot_access_token(provider).await?;
                auth = AuthInfo::new(token, AuthStrategy::GitHubCopilot);
            }

            // Codex OAuth 特殊处理：从 CodexOAuthManager 获取真实 access_token
            if auth.strategy == AuthStrategy::CodexOAuth {
                let (token, account_id) = self.codex_oauth_access_token(provider).await?;
                auth = AuthInfo::new(token, AuthStrategy::CodexOAuth);
                should_send_codex_oauth_session_headers = true;
                codex_oauth_account_id = account_id;
            }

            // xAI OAuth: resolve a managed account token immediately before
            // sending the request.
            if auth.strategy == AuthStrategy::XaiOAuth {
                let token = self.xai_oauth_access_token(provider).await?;
                auth = AuthInfo::new(token, AuthStrategy::XaiOAuth);
            }

            for secret in std::iter::once(&auth.api_key).chain(auth.access_token.iter()) {
                if !secret.is_empty() && !log_secrets.contains(secret) {
                    log_secrets.push(secret.clone());
                }
            }

            adapter.get_auth_headers(&auth)?
        } else {
            Vec::new()
        };

        let codex_oauth_session_headers =
            if should_send_codex_oauth_session_headers && self.session_client_provided {
                build_codex_oauth_session_headers(&self.session_id)
            } else {
                Vec::new()
            };

        // 自定义 User-Agent：与 stream_check / model_fetch 共用 parse_custom_user_agent，
        // 运行时静默忽略非法值（前端在输入处给非阻断提示，不在保存时阻断）。
        // Copilot 指纹 UA 不可覆盖。
        let custom_user_agent = if is_copilot {
            None
        } else {
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.custom_user_agent_header().ok().flatten())
        };
        // Codex→Anthropic emulation: when there is no custom UA, override Codex's
        // codex_cli_rs UA with the Claude Code UA.
        let custom_user_agent = if custom_user_agent.is_none() && codex_impersonate_claude_code {
            Some(http::HeaderValue::from_static(CLAUDE_CODE_USER_AGENT))
        } else {
            custom_user_agent
        };

        // --- Copilot 优化器：动态 header 注入 ---
        if let Some(context) = copilot_request_context {
            context.apply_headers(
                &mut auth_headers,
                self.copilot_optimizer_config.request_classification,
            )?;
        }

        // 预计算上游 host 值（用于在原位替换 host header）
        let upstream_host = url
            .parse::<http::Uri>()
            .ok()
            .and_then(|u| u.authority().map(|a| a.to_string()));

        // OpenCode Go places two hard requirements on clients
        // (<https://opencode.ai/docs/go/>); satisfy both, only for its upstream.
        //
        // 1) User-Agent: a client that identifies itself is passed through
        //    untouched (Claude Code, Codex, ...). Only a request that carries no
        //    UA at all gets this fallback - the gateway sits behind Cloudflare and
        //    rejects unidentified clients (403 Access denied; a UA clears it).
        //    The fallback is therefore applied after the header loop, next to the
        //    custom UA. An explicitly configured custom UA still wins.
        let opencode_fallback_user_agent =
            if custom_user_agent.is_none() && is_opencode_upstream(upstream_host.as_deref()) {
                Some(http::HeaderValue::from_static(concat!(
                    "cc-switch/",
                    env!("CARGO_PKG_VERSION")
                )))
            } else {
                None
            };

        // 2) Session header: without `x-opencode-session` the gateway answers
        //    400 MissingSessionID. Pass it through when the client already sends
        //    one, otherwise send the most stable identity we have
        //    (`pick_opencode_session`) - a native client header, the client's
        //    session id, the conversation fingerprint, or the generated id.
        if is_opencode_upstream(upstream_host.as_deref())
            && !headers.contains_key("x-opencode-session")
        {
            let client_id = self
                .session_client_provided
                .then_some(self.session_id.as_str());
            if let Some(session) = pick_opencode_session(
                native_session_header(headers),
                client_id,
                opencode_conversation_fingerprint.as_deref(),
                &self.session_id,
            ) {
                if let Ok(value) = http::HeaderValue::from_str(&session) {
                    auth_headers.push((http::HeaderName::from_static("x-opencode-session"), value));
                }
            }
        }

        let should_send_anthropic_headers = adapter.name() == "Claude"
            && matches!(resolved_claude_api_format.as_deref(), Some("anthropic"));

        let anthropic_beta_value = anthropic_beta_value(
            headers,
            should_send_anthropic_headers,
            codex_impersonate_claude_code,
            codex_anthropic_one_m,
        );

        let mut ordered_headers = OutboundHeaderPlan {
            upstream_host,
            auth_headers: &auth_headers,
            codex_official_auth_passthrough,
            codex_impersonate_claude_code,
            codex_responses_to_anthropic,
            codex_responses_to_chat,
            codex_stack_request,
            codex_unmanaged_upstream: matches!(app_type, AppType::Codex)
                && !provider.is_codex_oauth(),
            effective_endpoint: &effective_endpoint,
            force_identity_encoding,
            is_copilot,
            custom_user_agent,
            opencode_fallback_user_agent,
            anthropic_beta_value,
            should_send_anthropic_headers,
            codex_oauth_session_headers,
        }
        .build(headers);

        // 序列化请求体。GET/HEAD 是 idempotent/safe 方法，按 HTTP 语义不应携带 body；
        // 强行附带 JSON body 会让某些上游（如 Google Gemini 的 models.list）拒绝请求。
        let body_bytes = if matches!(method, &http::Method::GET | &http::Method::HEAD) {
            Vec::new()
        } else {
            serde_json::to_vec(&filtered_body).map_err(|e| {
                ProxyError::Internal(format!("Failed to serialize request body: {e}"))
            })?
        };

        // 确保 content-type 存在
        if !ordered_headers.contains_key(http::header::CONTENT_TYPE) {
            ordered_headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            );
        }

        apply_local_proxy_header_overrides(
            &mut ordered_headers,
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.local_proxy_request_overrides.as_ref()),
            is_copilot,
        );

        // 托管 OAuth 的 workspace 由账号绑定决定，覆盖客户端或本地代理配置的旧值。
        if let Some(ref account_id) = codex_oauth_account_id {
            if let Ok(value) = http::HeaderValue::from_str(account_id) {
                ordered_headers.insert("chatgpt-account-id", value);
            }
        }

        reject_proxy_placeholder_for_managed_account_upstream(&url, &ordered_headers)?;

        // 日志目标 URL 的脱敏分两种情形：
        // - 有已知密钥(log_secrets 非空)：记录脱敏后的完整 URL，剥 userinfo/query
        //   并抹掉已知密钥值，保留 host+path 便于诊断 base_url 配错路径导致的 404。
        // - 无已知密钥：凭据可能整个内嵌在 path 里且无从脱敏，只记 origin，
        //   避免默认 Info 级把形如 https://gw/<KEY>/v1 的 path 完整落盘。
        let target_for_log = if log_secrets.is_empty() {
            crate::redact_url_origin_for_log(&url)
        } else {
            crate::redact_url_for_log_with_secrets(&url, &log_secrets)
        };

        // 输出请求信息日志
        let tag = adapter.name();
        let request_model = filtered_body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("<none>");
        log::info!("[{tag}] >>> 请求目标: {target_for_log} (model={request_model})");
        log::debug!(
            "[{tag}] >>> 请求体已准备: bytes={}, hash={} (content omitted)",
            body_bytes.len(),
            short_value_hash(Some(&filtered_body))
        );

        // 序列化后释放工作副本；只有带着上游签发状态的流式请求，判流内拒绝时还要用请求体。
        let opaque_state_request = (matches!(app_type, AppType::Codex | AppType::GrokBuild)
            && !codex_responses_to_chat
            && !codex_responses_to_anthropic
            && request_is_streaming
            && self.rectifier_config.enabled
            && carries_opaque_state(&filtered_body))
        .then_some(filtered_body);

        let preserve_exact_header_case = should_preserve_exact_header_case(
            adapter.name(),
            provider,
            resolved_claude_api_format.as_deref(),
            is_copilot,
        );

        let response = self
            .send_upstream(
                &url,
                &target_for_log,
                method,
                ordered_headers,
                extensions,
                body_bytes,
                request_is_streaming,
                preserve_exact_header_case,
            )
            .await?;

        // Official responses (errors included) carry the account's usage headers;
        // they feed the next request's quota check without an extra lookup.
        if let Some(token) = quota_token {
            super::codex_quota_protection::observe_response(token, headers, response.headers());
        }

        // 检查响应状态
        let status = response.status();

        if status.is_success() {
            let response = self
                .validate_success_response(
                    response,
                    request_is_streaming,
                    codex_responses_to_anthropic,
                    resolved_claude_api_format.as_deref(),
                    opaque_state_request.as_ref(),
                )
                .await?;
            Ok((response, resolved_claude_api_format, outbound_model))
        } else {
            Err(upstream_error(response).await?)
        }
    }

    fn categorize_proxy_error(&self, error: &ProxyError, provider: &Provider) -> ErrorCategory {
        // Authentication belongs to the Codex client for an official route.
        // Every retry would reuse the selected account's inbound Authorization
        // header against another card, so no official-route error may fail over.
        if super::providers::is_codex_official_provider(provider) {
            return ErrorCategory::NonRetryable;
        }

        // xAI OAuth mirrors the same rule for token acquisition: a local
        // AuthError means the managed account needs re-login. Failing over
        // would silently move the conversation off the selected Grok account
        // and poison the provider's health state for an account-level issue.
        if provider.is_xai_oauth() && matches!(error, ProxyError::AuthError(_)) {
            return ErrorCategory::NonRetryable;
        }

        match error {
            // 网络和上游错误：都应该尝试下一个供应商
            ProxyError::Timeout(_) => ErrorCategory::Retryable,
            ProxyError::ForwardFailed(_) => ErrorCategory::Retryable,
            // 上游 HTTP 错误：按状态码分桶。
            //
            // 客户端请求自身有问题的状态码无论换哪个 provider 都会被拒绝，
            // 继续轮询只会放大错误率、污染熔断器健康度、浪费配额：
            //   400 Bad Request / 422 Unprocessable Entity   ← 请求体格式或语义错误
            //   405 Method Not Allowed / 406 Not Acceptable  ← 方法或 Accept 错误
            //   413 Payload Too Large / 414 URI Too Long     ← 客户端构造超限
            //   415 Unsupported Media Type                    ← Content-Type 错误
            //   501 Not Implemented                           ← 上游协议确实不支持
            //
            // 其他 4xx（401/403/404/408/409/429/451 等）和全部 5xx 都保留
            // Retryable —— 换一家 provider 可能持有不同的 key、配额、地域或模型映射。
            ProxyError::UpstreamError { status, .. } => match *status {
                400 | 405 | 406 | 413 | 414 | 415 | 422 | 501 => ErrorCategory::NonRetryable,
                _ => ErrorCategory::Retryable,
            },
            // Provider 级配置/转换问题：换一个 Provider 可能就能成功
            ProxyError::ConfigError(_) => ErrorCategory::Retryable,
            ProxyError::TransformError(_) => ErrorCategory::Retryable,
            ProxyError::AuthError(_) => ErrorCategory::Retryable,
            // 无可用供应商：所有供应商都试过了，无法重试
            ProxyError::NoAvailableProvider => ErrorCategory::NonRetryable,
            // 其他错误（数据库/内部错误等）：不是换供应商能解决的问题
            _ => ErrorCategory::NonRetryable,
        }
    }
}

/// 检测 Provider 是否为 Bedrock（通过 CLAUDE_CODE_USE_BEDROCK 环境变量判断）
fn is_bedrock_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_USE_BEDROCK"))
        .and_then(|v| v.as_str())
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn refresh_success_rate(status: &mut ProxyStatus) {
    if status.total_requests > 0 {
        status.success_rate =
            (status.success_requests as f32 / status.total_requests as f32) * 100.0;
    }
}

/// Providers whose errors can come from an Anthropic Messages endpoint and therefore
/// need the thinking signature / budget rectifiers. GitHub Copilot passes Anthropic
/// models through to its native `/v1/messages`, where history replayed from another
/// provider or from the Responses bridge fails signature validation; errors from its
/// Chat / Responses bridges never match the rectifier keywords.
fn uses_anthropic_rectifiers(provider_type: Option<ProviderType>) -> bool {
    matches!(
        provider_type,
        Some(ProviderType::Claude | ProviderType::ClaudeAuth | ProviderType::GitHubCopilot)
    )
}

#[cfg(test)]
mod tests;
