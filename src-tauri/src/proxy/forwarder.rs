//! 璇锋眰杞彂鍣?//!
//! 璐熻矗灏嗚姹傝浆鍙戝埌涓婃父Provider锛屾敮鎸佹晠闅滆浆绉?
use super::hyper_client::ProxyResponse;
use super::{
    body_filter::filter_private_params_with_whitelist,
    error::*,
    failover_switch::FailoverSwitchManager,
    json_canonical::{canonicalize_value, short_value_hash},
    log_codes::fwd as log_fwd,
    provider_router::ProviderRouter,
    providers::{
        codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore, get_adapter,
        AuthInfo, AuthStrategy, ProviderAdapter, ProviderType,
    },
    thinking_budget_rectifier::{rectify_thinking_budget, should_rectify_thinking_budget},
    thinking_rectifier::{
        normalize_thinking_type, rectify_anthropic_request, should_rectify_thinking_signature,
    },
    types::{CopilotOptimizerConfig, OptimizerConfig, ProxyStatus, RectifierConfig},
    ProxyError,
};
use crate::commands::{CodexOAuthState, CopilotAuthState};
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::proxy::providers::copilot_auth::CopilotAuthManager;
use crate::{app_config::AppType, provider::Provider};
use futures::StreamExt;
use http::Extensions;
use serde_json::Value;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::RwLock;

const PROXY_AUTH_PLACEHOLDER: &str = "PROXY_MANAGED";

pub struct ForwardResult {
    pub response: ProxyResponse,
    pub provider: Provider,
    pub claude_api_format: Option<String>,
    /// 娲昏穬杩炴帴 RAII guard锛氶殢鍝嶅簲涓€璧锋祦杞埌 response_processor / handle_claude_transform锛?    /// 鏈€缁堣 move 杩涙祦寮?body future锛堟垨闈炴祦寮忓搷搴斾綔鐢ㄥ煙锛夛紝瑕嗙洊鏁翠釜鍝嶅簲鐢熷懡鍛ㄦ湡銆?    pub(crate) connection_guard: Option<ActiveConnectionGuard>,
}

pub struct ForwardError {
    pub error: ProxyError,
    pub provider: Option<Provider>,
}

/// 娲昏穬杩炴帴 RAII guard
///
/// 鏋勯€犳椂鎶?`ProxyStatus.active_connections` +1锛汥rop 鏃跺湪 tokio runtime 涓婅皟搴?/// 涓€涓紓姝ヤ换鍔℃墽琛?-1锛屼粠鑰屾敮鎸佹妸 guard move 杩涙祦寮?body future锛坰tream 鑷劧缁撴潫
/// 鏃?guard 涓?future 涓€璧?drop锛夈€?///
/// 璁捐鍔ㄦ満锛氫箣鍓嶅湪 `forward_with_retry` 鍑哄彛澶勫悓姝?-1锛屼絾娴佸紡鍝嶅簲鐨?body 瀹為檯
/// 鍦?`create_logged_passthrough_stream` 鍐呰繕浼氱户缁?yield 瀛楄妭娴侊紝瀵艰嚧 UI 鐨?/// `active_connections` 璁℃暟杩囨棭褰掗浂銆俁AII guard 璁?鍑忛噺"鐢?Rust 绫诲瀷绯荤粺椹卞姩锛?/// 涓嶉渶瑕佹瘡鏉″嚭鍙ｈ矾寰勯兘鎵嬪姩璋冪敤銆?pub(crate) struct ActiveConnectionGuard {
    status: Arc<RwLock<ProxyStatus>>,
}

impl ActiveConnectionGuard {
    pub(crate) async fn acquire(status: Arc<RwLock<ProxyStatus>>) -> Self {
        {
            let mut s = status.write().await;
            s.active_connections = s.active_connections.saturating_add(1);
        }
        Self { status }
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        // Drop 涓嶈兘 await锛氭妸鍑忛噺鎿嶄綔璋冨害鍒?tokio runtime
        let status = self.status.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut s = status.write().await;
                s.active_connections = s.active_connections.saturating_sub(1);
            });
        }
        // 娌℃湁 runtime 鏃堕潤榛樹涪澶辫鏁帮紙浠?UI 灞曠ず鐢紝鍙帴鍙楁渶缁堜竴鑷存€э級
    }
}

pub struct RequestForwarder {
    /// 鍏变韩鐨?ProviderRouter锛堟寔鏈夌啍鏂櫒鐘舵€侊級
    router: Arc<ProviderRouter>,
    status: Arc<RwLock<ProxyStatus>>,
    current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    gemini_shadow: Arc<GeminiShadowStore>,
    codex_chat_history: Arc<CodexChatHistoryStore>,
    /// 鏁呴殰杞Щ鍒囨崲绠＄悊鍣?    failover_manager: Arc<FailoverSwitchManager>,
    /// AppHandle锛岀敤浜庡彂灏勪簨浠跺拰鏇存柊鎵樼洏
    app_handle: Option<tauri::AppHandle>,
    /// 璇锋眰寮€濮嬫椂鐨?褰撳墠渚涘簲鍟?ID"锛堢敤浜庡垽鏂槸鍚﹂渶瑕佸悓姝?UI/鎵樼洏锛?    current_provider_id_at_start: String,
    /// 浠ｇ悊浼氳瘽 ID锛堢敤浜?Gemini Native shadow replay锛?    session_id: String,
    /// Session ID 鏄惁鐢卞鎴风鎻愪緵锛涚敓鎴愬€间笉鑳戒綔涓轰笂娓哥紦瀛樿韩浠姐€?    session_client_provided: bool,
    /// 鏁存祦鍣ㄩ厤缃?    rectifier_config: RectifierConfig,
    /// 浼樺寲鍣ㄩ厤缃?    optimizer_config: OptimizerConfig,
    /// Copilot 浼樺寲鍣ㄩ厤缃?    copilot_optimizer_config: CopilotOptimizerConfig,
    /// 闈炴祦寮忚姹傝秴鏃讹紙绉掞級
    non_streaming_timeout: std::time::Duration,
    /// 娴佸紡璇锋眰鍝嶅簲澶寸瓑寰呰秴鏃讹紙绉掞級
    streaming_first_byte_timeout: std::time::Duration,
    /// 鍗曚釜瀹㈡埛绔姹傛渶澶氬皾璇曠殑 provider 鏁般€?    ///
    /// 鐢?`AppProxyConfig.max_retries` (UI: "璇锋眰澶辫触鏃剁殑閲嶈瘯娆℃暟, 0-10") 娲剧敓锛?    /// `max_attempts = max_retries + 1`锛屾墍浠?max_retries=0 琛ㄧず浠呭皾璇曚竴瀹躲€?    /// max_retries=3锛堥粯璁わ級琛ㄧず鏈€澶?4 瀹躲€俵oop 鍚屾椂鍙?providers.len() 鑷劧闄愬埗銆?    max_attempts: usize,
}

impl RequestForwarder {
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
        // max_retries 鏄€屽け璐ュ悗閲嶈瘯娆℃暟銆嶈涔夛紝attempt 涓婇檺 = retries + 1銆?        // saturating_add 闃叉 u32::MAX + 1 婧㈠嚭銆?        let max_attempts = (max_retries as usize).saturating_add(1);
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
        }
    }

    async fn record_success_result(
        &self,
        provider_id: &str,
        app_type: &str,
        used_half_open_permit: bool,
    ) {
        if used_half_open_permit {
            if let Err(e) = self
                .router
                .record_result(provider_id, app_type, true, true, None)
                .await
            {
                log::warn!(
                    "[{app_type}] 璁板綍 Provider 鎴愬姛缁撴灉澶辫触: provider_id={provider_id}, error={e}"
                );
            }
            return;
        }

        let router = self.router.clone();
        let provider_id = provider_id.to_string();
        let app_type = app_type.to_string();
        tokio::spawn(async move {
            if let Err(e) = router
                .record_result(&provider_id, &app_type, false, true, None)
                .await
            {
                log::warn!(
                    "[{app_type}] 寮傛璁板綍 Provider 鎴愬姛缁撴灉澶辫触: provider_id={provider_id}, error={e}"
                );
            }
        });
    }

    /// 鏁存祦锛坱hinking signature 鎴?budget锛夐噸璇曞け璐ュ悗鐨勭粺涓€鏀跺熬銆?    ///
    /// `None` 琛ㄧず宸茶褰曠啍鏂櫒銆佺疮绉?`last_error`/`last_provider`锛?    /// 璋冪敤鏂瑰簲 `continue` 璁╀笅涓€瀹?provider 缁х画鏁呴殰杞Щ锛?    /// `Some(ForwardError)` 琛ㄧず鏄鎴风閿欒锛屾病鏈?provider 鑳戒慨澶嶏紝
    /// 璋冪敤鏂瑰簲鐩存帴 `return` 鎶婇敊璇繑鍥炵粰瀹㈡埛绔€?    #[allow(clippy::too_many_arguments)]
    async fn handle_rectifier_retry_failure(
        &self,
        retry_err: ProxyError,
        provider: &Provider,
        app_type_str: &str,
        used_half_open_permit: bool,
        rectifier_label: &str,
        last_error: &mut Option<ProxyError>,
        last_provider: &mut Option<Provider>,
    ) -> Option<ForwardError> {
        // Provider 閿欒锛氭湰瀹朵笂娓?缃戠粶纭疄鍑洪棶棰橈紝涓嬩竴瀹?provider 鍙兘鍙敤 鈫?缁х画鏁呴殰杞Щ銆?        // 瀹㈡埛绔敊璇細鏁存祦鍚庤姹備粛杩濇硶锛屼笅涓€瀹朵篃淇笉濂?鈫?鐩存帴杩斿洖銆?        let is_provider_error = match &retry_err {
            ProxyError::Timeout(_) | ProxyError::ForwardFailed(_) => true,
            ProxyError::UpstreamError { status, .. } => *status >= 500,
            _ => false,
        };

        if is_provider_error {
            let _ = self
                .router
                .record_result(
                    &provider.id,
                    app_type_str,
                    used_half_open_permit,
                    false,
                    Some(retry_err.to_string()),
                )
                .await;
            {
                let mut status = self.status.write().await;
                status.last_error = Some(format!(
                    "Provider {} {rectifier_label}閲嶈瘯澶辫触: {}",
                    provider.name, retry_err
                ));
            }
            *last_error = Some(retry_err);
            *last_provider = Some(provider.clone());
            return None;
        }

        self.router
            .release_permit_neutral(&provider.id, app_type_str, used_half_open_permit)
            .await;
        let mut status = self.status.write().await;
        status.failed_requests += 1;
        status.last_error = Some(retry_err.to_string());
        if status.total_requests > 0 {
            status.success_rate =
                (status.success_requests as f32 / status.total_requests as f32) * 100.0;
        }
        Some(ForwardError {
            error: retry_err,
            provider: Some(provider.clone()),
        })
    }

    /// 杞彂璇锋眰锛堝甫鏁呴殰杞Щ锛?    ///
    /// 杩欐槸 thin wrapper锛氬湪瀹㈡埛绔姹傜淮搴﹁涓€娆?`total_requests` / 璋冩暣
    /// `active_connections` / 鍒锋柊 `last_request_at`锛屾棤璁?inner 璧板摢鏉″嚭鍙ｈ矾寰勶紝
    /// 鍑哄彛澶勯兘浼氭妸 `active_connections` 鍥炴敹銆侾er-attempt 缁村害锛堟垚鍔?澶辫触/鐔旀柇
    /// 绛夛級浠嶇敱 inner 鍐呰嚜琛屾洿鏂?`success_requests` / `failed_requests`銆?    #[allow(clippy::too_many_arguments)]
    pub async fn forward_with_retry(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        body: Value,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        let guard = ActiveConnectionGuard::acquire(self.status.clone()).await;
        {
            let mut s = self.status.write().await;
            s.total_requests = s.total_requests.saturating_add(1);
            s.last_request_at = Some(chrono::Utc::now().to_rfc3339());
        }
        let result = self
            .forward_with_retry_inner(
                app_type, method, endpoint, body, headers, extensions, providers,
            )
            .await;
        // 鎶?guard 娉ㄥ叆鍒?Ok 缁撴灉锛岃瀹冮殢鍝嶅簲涓€璧锋祦杞埌 response_processor锛?        // 鍦ㄦ祦寮?body 鐨?future 鍐呮墠鐪熸 drop銆?        // Err 璺緞锛歡uard 鍦ㄥ嚱鏁?scope 鍐呴殢杩斿洖鍊艰惤鍦版椂鑷姩 drop銆?        result.map(|mut fr| {
            fr.connection_guard = Some(guard);
            fr
        })
    }

    /// 瀹為檯杞彂閫昏緫锛堜笉鍖呭惈瀹㈡埛绔淮搴︾殑鍏ュ彛/鍑哄彛璁℃暟锛?    ///
    /// # Arguments
    /// * `app_type` - 搴旂敤绫诲瀷
    /// * `method` - 瀹㈡埛绔姹傜殑 HTTP 鏂规硶锛堥€忎紶缁欎笂娓革紝鏀寔 GET/POST 绛夛級
    /// * `endpoint` - API 绔偣
    /// * `body` - 璇锋眰浣?    /// * `headers` - 璇锋眰澶?    /// * `providers` - 宸查€夋嫨鐨?Provider 鍒楄〃锛堢敱 RequestContext 鎻愪緵锛岄伩鍏嶉噸澶嶈皟鐢?select_providers锛?    #[allow(clippy::too_many_arguments)]
    async fn forward_with_retry_inner(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        body: Value,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        // 鑾峰彇閫傞厤鍣?        let adapter = get_adapter(app_type);
        let app_type_str = app_type.as_str();

        if providers.is_empty() {
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
        }

        let mut last_error = None;
        let mut last_provider = None;
        let mut attempted_providers = 0usize;

        // 鍗?Provider 鍦烘櫙涓嬭烦杩囩啍鏂櫒妫€鏌ワ紙鏁呴殰杞Щ鍏抽棴鏃讹級
        let bypass_circuit_breaker = providers.len() == 1;

        // 渚濇灏濊瘯姣忎釜渚涘簲鍟?        for provider in providers.iter() {
            // 鏁存祦鍣ㄩ噸璇曟爣璁帮細姣忎釜 provider 鐙珛鎸佹湁锛岄伩鍏嶆爣璁拌法 provider 鐭矾鏁呴殰杞Щ
            // 鈥斺€?棣栧 provider 鏁存祦鍚庤 5xx/timeout 鍑昏惤鏃讹紝涓嬪浠嶈兘鐢ㄦ暣娴佸悗鐨勮姹備綋璧版暣娴佹祦绋?            let mut rectifier_retried = false;
            let mut budget_rectifier_retried = false;

            // 涓婇檺妫€鏌ワ細灏婇噸鐢ㄦ埛鍦?AppProxyConfig.max_retries 涓婇厤缃殑銆岄噸璇曟鏁般€嶃€?            // 鏀惧湪鐔旀柇鍣?allow 妫€鏌ヤ箣鍓嶏紝閬垮厤鍦ㄥ凡缁忚秴闄愭椂杩樺崰鐢?HalfOpen 鎺㈡祴鍚嶉銆?            if attempted_providers >= self.max_attempts {
                log::warn!(
                    "[{app_type_str}] 宸茶揪鏈€澶у皾璇曟鏁颁笂闄?({}/{}), 鍋滄鏁呴殰杞Щ",
                    attempted_providers,
                    self.max_attempts
                );
                break;
            }

            // 鍙戣捣璇锋眰鍓嶅厛鑾峰彇鐔旀柇鍣ㄦ斁琛岃鍙紙HalfOpen 浼氬崰鐢ㄦ帰娴嬪悕棰濓級
            // 鍗?Provider 鍦烘櫙涓嬭烦杩囨妫€鏌ワ紝閬垮厤鐔旀柇鍣ㄩ樆濉炴墍鏈夎姹?            let (allowed, used_half_open_permit) = if bypass_circuit_breaker {
                (true, false)
            } else {
                let permit = self
                    .router
                    .allow_provider_request(&provider.id, app_type_str)
                    .await;
                (permit.allowed, permit.used_half_open_permit)
            };

            if !allowed {
                continue;
            }

            // PRE-SEND 浼樺寲鍣細姣忎釜 provider 鐙珛鍐冲畾鏄惁浼樺寲
            // clone body 浠ラ伩鍏?Bedrock 浼樺寲瀛楁娉勬紡鍒伴潪 Bedrock provider锛坒ailover 鍦烘櫙锛?            let mut provider_body =
                if self.optimizer_config.enabled && is_bedrock_provider(provider) {
                    let mut b = body.clone();
                    if self.optimizer_config.thinking_optimizer {
                        super::thinking_optimizer::optimize(&mut b, &self.optimizer_config);
                    }
                    if self.optimizer_config.cache_injection {
                        super::cache_injector::inject(&mut b, &self.optimizer_config);
                    }
                    b
                } else {
                    body.clone()
                };

            attempted_providers += 1;

            // 鏇存柊鐘舵€佷腑鐨勫綋鍓?Provider 淇℃伅锛坧er-attempt 缁村害鐨勬爣璇嗭級
            //
            // total_requests / last_request_at / active_connections 宸茬敱
            // forward_with_retry wrapper 鍦ㄥ鎴风璇锋眰缁村害缁熶竴澶勭悊锛岃繖閲屽彧鍒?            // 鏂般€屾鍦ㄥ皾璇曞摢涓?provider銆嶇殑灞曠ず瀛楁銆?            {
                let mut status = self.status.write().await;
                status.current_provider = Some(provider.name.clone());
                status.current_provider_id = Some(provider.id.clone());
            }

            // 杞彂璇锋眰锛堟瘡涓?Provider 鍙皾璇曚竴娆★紝閲嶈瘯鐢卞鎴风鎺у埗锛?            match self
                .forward(
                    app_type,
                    &method,
                    provider,
                    endpoint,
                    &provider_body,
                    &headers,
                    &extensions,
                    adapter.as_ref(),
                )
                .await
            {
                Ok((response, claude_api_format)) => {
                    // 鎴愬姛锛氭櫘閫氶棴鍚堢啍鏂姸鎬佸紓姝ヨ褰曪紝閬垮厤闃诲娴佸紡棣栧寘杩斿洖锛?                    // HalfOpen 鎺㈡祴浠嶅悓姝ョ瓑寰咃紝淇濊瘉 permit 涓庣啍鏂姸鎬佸強鏃堕噴鏀俱€?                    self.record_success_result(&provider.id, app_type_str, used_half_open_permit)
                        .await;

                    // 鏇存柊褰撳墠搴旂敤绫诲瀷浣跨敤鐨?provider
                    {
                        let mut current_providers = self.current_providers.write().await;
                        current_providers.insert(
                            app_type_str.to_string(),
                            (provider.id.clone(), provider.name.clone()),
                        );
                    }

                    // 鏇存柊鎴愬姛缁熻
                    {
                        let mut status = self.status.write().await;
                        status.success_requests += 1;
                        status.last_error = None;
                        let should_switch =
                            self.current_provider_id_at_start.as_str() != provider.id.as_str();
                        if should_switch {
                            status.failover_count += 1;

                            // 寮傛瑙﹀彂渚涘簲鍟嗗垏鎹紝鏇存柊 UI/鎵樼洏锛屽苟鎶娾€滃綋鍓嶄緵搴斿晢鈥濆悓姝ヤ负瀹為檯浣跨敤鐨?provider
                            let fm = self.failover_manager.clone();
                            let ah = self.app_handle.clone();
                            let pid = provider.id.clone();
                            let pname = provider.name.clone();
                            let at = app_type_str.to_string();

                            tokio::spawn(async move {
                                let _ = fm.try_switch(ah.as_ref(), &at, &pid, &pname).await;
                            });
                        }
                        // 閲嶆柊璁＄畻鎴愬姛鐜?                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                    }

                    return Ok(ForwardResult {
                        response,
                        provider: provider.clone(),
                        claude_api_format,
                        connection_guard: None,
                    });
                }
                Err(e) => {
                    // 妫€娴嬫槸鍚﹂渶瑕佽Е鍙戞暣娴佸櫒锛堜粎 Claude/ClaudeAuth 渚涘簲鍟嗭級
                    let provider_type = ProviderType::from_app_type_and_config(app_type, provider);
                    let is_anthropic_provider = matches!(
                        provider_type,
                        ProviderType::Claude | ProviderType::ClaudeAuth
                    );
                    let mut signature_rectifier_non_retryable_client_error = false;

                    if is_anthropic_provider {
                        let error_message = extract_error_message(&e);
                        if should_rectify_thinking_signature(
                            error_message.as_deref(),
                            &self.rectifier_config,
                        ) {
                            // 宸茬粡閲嶈瘯杩囷細鐩存帴杩斿洖閿欒锛堜笉鍙噸璇曞鎴风閿欒锛?                            if rectifier_retried {
                                log::warn!("[{app_type_str}] [RECT-005] 鏁存祦鍣ㄥ凡瑙﹀彂杩囷紝涓嶅啀閲嶈瘯");
                                // 閲婃斁 HalfOpen permit锛堜笉璁板綍鐔旀柇鍣紝杩欐槸瀹㈡埛绔吋瀹规€ч棶棰橈級
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                                return Err(ForwardError {
                                    error: e,
                                    provider: Some(provider.clone()),
                                });
                            }

                            // 棣栨瑙﹀彂锛氭暣娴佽姹備綋
                            let rectified = rectify_anthropic_request(&mut provider_body);

                            // 鏁存祦鏈敓鏁堬細缁х画灏濊瘯 budget 鏁存祦璺緞锛岄伩鍏嶈鍒ゅ悗鐭矾
                            if !rectified.applied {
                                log::warn!(
                                    "[{app_type_str}] [RECT-006] thinking 绛惧悕鏁存祦鍣ㄨЕ鍙戜絾鏃犲彲鏁存祦鍐呭锛岀户缁鏌?budget锛涜嫢 budget 涔熸湭鍛戒腑鍒欐寜瀹㈡埛绔敊璇繑鍥?
                                );
                                signature_rectifier_non_retryable_client_error = true;
                            } else {
                                log::info!(
                                    "[{}] [RECT-001] thinking 绛惧悕鏁存祦鍣ㄨЕ鍙? 绉婚櫎 {} thinking blocks, {} redacted_thinking blocks, {} signature fields",
                                    app_type_str,
                                    rectified.removed_thinking_blocks,
                                    rectified.removed_redacted_thinking_blocks,
                                    rectified.removed_signature_fields
                                );

                                // 鏍囪宸查噸璇曪紙褰撳墠閫昏緫涓嬮噸璇曞悗蹇呭畾 return锛屼繚鐣欐爣璁颁互澶囧皢鏉ユ墿灞曪級
                                let _ = std::mem::replace(&mut rectifier_retried, true);

                                // 浣跨敤鍚屼竴渚涘簲鍟嗛噸璇曪紙涓嶈鍏ョ啍鏂櫒锛?                                match self
                                    .forward(
                                        app_type,
                                        &method,
                                        provider,
                                        endpoint,
                                        &provider_body,
                                        &headers,
                                        &extensions,
                                        adapter.as_ref(),
                                    )
                                    .await
                                {
                                    Ok((response, claude_api_format)) => {
                                        log::info!("[{app_type_str}] [RECT-002] 鏁存祦閲嶈瘯鎴愬姛");
                                        self.record_success_result(
                                            &provider.id,
                                            app_type_str,
                                            used_half_open_permit,
                                        )
                                        .await;

                                        // 鏇存柊褰撳墠搴旂敤绫诲瀷浣跨敤鐨?provider
                                        {
                                            let mut current_providers =
                                                self.current_providers.write().await;
                                            current_providers.insert(
                                                app_type_str.to_string(),
                                                (provider.id.clone(), provider.name.clone()),
                                            );
                                        }

                                        // 鏇存柊鎴愬姛缁熻
                                        {
                                            let mut status = self.status.write().await;
                                            status.success_requests += 1;
                                            status.last_error = None;
                                            let should_switch =
                                                self.current_provider_id_at_start.as_str()
                                                    != provider.id.as_str();
                                            if should_switch {
                                                status.failover_count += 1;

                                                // 寮傛瑙﹀彂渚涘簲鍟嗗垏鎹紝鏇存柊 UI/鎵樼洏
                                                let fm = self.failover_manager.clone();
                                                let ah = self.app_handle.clone();
                                                let pid = provider.id.clone();
                                                let pname = provider.name.clone();
                                                let at = app_type_str.to_string();

                                                tokio::spawn(async move {
                                                    let _ = fm
                                                        .try_switch(ah.as_ref(), &at, &pid, &pname)
                                                        .await;
                                                });
                                            }
                                            if status.total_requests > 0 {
                                                status.success_rate = (status.success_requests
                                                    as f32
                                                    / status.total_requests as f32)
                                                    * 100.0;
                                            }
                                        }

                                        return Ok(ForwardResult {
                                            response,
                                            provider: provider.clone(),
                                            claude_api_format,
                                            connection_guard: None,
                                        });
                                    }
                                    Err(retry_err) => {
                                        log::warn!(
                                            "[{app_type_str}] [RECT-003] 鏁存祦閲嶈瘯浠嶅け璐? {retry_err}"
                                        );
                                        if let Some(err) = self
                                            .handle_rectifier_retry_failure(
                                                retry_err,
                                                provider,
                                                app_type_str,
                                                used_half_open_permit,
                                                "鏁存祦",
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

                    // 妫€娴嬫槸鍚﹂渶瑕佽Е鍙?budget 鏁存祦鍣紙浠?Claude/ClaudeAuth 渚涘簲鍟嗭級
                    if is_anthropic_provider {
                        let error_message = extract_error_message(&e);
                        if should_rectify_thinking_budget(
                            error_message.as_deref(),
                            &self.rectifier_config,
                        ) {
                            // 宸茬粡閲嶈瘯杩囷細鐩存帴杩斿洖閿欒锛堜笉鍙噸璇曞鎴风閿欒锛?                            if budget_rectifier_retried {
                                log::warn!(
                                    "[{app_type_str}] [RECT-013] budget 鏁存祦鍣ㄥ凡瑙﹀彂杩囷紝涓嶅啀閲嶈瘯"
                                );
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                                return Err(ForwardError {
                                    error: e,
                                    provider: Some(provider.clone()),
                                });
                            }

                            let budget_rectified = rectify_thinking_budget(&mut provider_body);
                            if !budget_rectified.applied {
                                log::warn!(
                                    "[{app_type_str}] [RECT-014] budget 鏁存祦鍣ㄨЕ鍙戜絾鏃犲彲鏁存祦鍐呭锛屼笉鍋氭棤鎰忎箟閲嶈瘯"
                                );
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                                return Err(ForwardError {
                                    error: e,
                                    provider: Some(provider.clone()),
                                });
                            }

                            log::info!(
                                "[{}] [RECT-010] thinking budget 鏁存祦鍣ㄨЕ鍙? before={:?}, after={:?}",
                                app_type_str,
                                budget_rectified.before,
                                budget_rectified.after
                            );

                            let _ = std::mem::replace(&mut budget_rectifier_retried, true);

                            // 浣跨敤鍚屼竴渚涘簲鍟嗛噸璇曪紙涓嶈鍏ョ啍鏂櫒锛?                            match self
                                .forward(
                                    app_type,
                                    &method,
                                    provider,
                                    endpoint,
                                    &provider_body,
                                    &headers,
                                    &extensions,
                                    adapter.as_ref(),
                                )
                                .await
                            {
                                Ok((response, claude_api_format)) => {
                                    log::info!("[{app_type_str}] [RECT-011] budget 鏁存祦閲嶈瘯鎴愬姛");
                                    self.record_success_result(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;

                                    {
                                        let mut current_providers =
                                            self.current_providers.write().await;
                                        current_providers.insert(
                                            app_type_str.to_string(),
                                            (provider.id.clone(), provider.name.clone()),
                                        );
                                    }

                                    {
                                        let mut status = self.status.write().await;
                                        status.success_requests += 1;
                                        status.last_error = None;
                                        let should_switch =
                                            self.current_provider_id_at_start.as_str()
                                                != provider.id.as_str();
                                        if should_switch {
                                            status.failover_count += 1;
                                            let fm = self.failover_manager.clone();
                                            let ah = self.app_handle.clone();
                                            let pid = provider.id.clone();
                                            let pname = provider.name.clone();
                                            let at = app_type_str.to_string();
                                            tokio::spawn(async move {
                                                let _ = fm
                                                    .try_switch(ah.as_ref(), &at, &pid, &pname)
                                                    .await;
                                            });
                                        }
                                        if status.total_requests > 0 {
                                            status.success_rate = (status.success_requests as f32
                                                / status.total_requests as f32)
                                                * 100.0;
                                        }
                                    }

                                    return Ok(ForwardResult {
                                        response,
                                        provider: provider.clone(),
                                        claude_api_format,
                                        connection_guard: None,
                                    });
                                }
                                Err(retry_err) => {
                                    log::warn!(
                                        "[{app_type_str}] [RECT-012] budget 鏁存祦閲嶈瘯浠嶅け璐? {retry_err}"
                                    );
                                    if let Some(err) = self
                                        .handle_rectifier_retry_failure(
                                            retry_err,
                                            provider,
                                            app_type_str,
                                            used_half_open_permit,
                                            "budget 鏁存祦",
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
                        self.router
                            .release_permit_neutral(
                                &provider.id,
                                app_type_str,
                                used_half_open_permit,
                            )
                            .await;
                        let mut status = self.status.write().await;
                        status.failed_requests += 1;
                        status.last_error = Some(e.to_string());
                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                        return Err(ForwardError {
                            error: e,
                            provider: Some(provider.clone()),
                        });
                    }

                    // 鍏堝垎绫婚敊璇紝鍐冲畾鏄惁璁″叆 provider 鍋ュ悍搴?                    // 鈥斺€?NonRetryable / ClientAbort 鏄鎴风灞傞敊璇紝鏃犺鎹㈠摢瀹?provider 閮戒細琚嫆缁濓紝
                    //    涓嶅簲姹℃煋鐔旀柇鍣ㄥ拰鏁版嵁搴撳仴搴峰害锛堜笌 release_permit_neutral 鍚岃涔夛級銆?                    let category = self.categorize_proxy_error(&e);

                    match category {
                        ErrorCategory::Retryable => {
                            // 鍙噸璇曪細鐪熸鐨?provider 鏁呴殰 鈫?璁板綍澶辫触骞舵洿鏂扮啍鏂櫒/DB 鍋ュ悍搴?                            let _ = self
                                .router
                                .record_result(
                                    &provider.id,
                                    app_type_str,
                                    used_half_open_permit,
                                    false,
                                    Some(e.to_string()),
                                )
                                .await;

                            {
                                let mut status = self.status.write().await;
                                status.last_error =
                                    Some(format!("Provider {} 澶辫触: {}", provider.name, e));
                            }

                            let (log_code, log_message) = build_retryable_failure_log(
                                &provider.name,
                                attempted_providers,
                                providers.len(),
                                &e,
                            );
                            log::warn!("[{app_type_str}] [{log_code}] {log_message}");

                            last_error = Some(e);
                            last_provider = Some(provider.clone());
                            // 缁х画灏濊瘯涓嬩竴涓緵搴斿晢
                            continue;
                        }
                        ErrorCategory::NonRetryable | ErrorCategory::ClientAbort => {
                            // 涓嶅彲閲嶈瘯锛氬鎴风灞傞敊璇垨瀹㈡埛绔柇杩?鈫?涓嶆薄鏌撳仴搴峰害锛屼粎閲婃斁 HalfOpen permit
                            self.router
                                .release_permit_neutral(
                                    &provider.id,
                                    app_type_str,
                                    used_half_open_permit,
                                )
                                .await;
                            {
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                            }
                            return Err(ForwardError {
                                error: e,
                                provider: Some(provider.clone()),
                            });
                        }
                    }
                }
            }
        }

        if attempted_providers == 0 {
            // providers 鍒楄〃闈炵┖锛屼絾鍏ㄩ儴琚啍鏂櫒鎷掔粷锛堝吀鍨嬶細HalfOpen 鎺㈡祴鍚嶉琚崰鐢級
            {
                let mut status = self.status.write().await;
                status.failed_requests += 1;
                status.last_error = Some("鎵€鏈変緵搴斿晢鏆傛椂涓嶅彲鐢紙鐔旀柇鍣ㄩ檺鍒讹級".to_string());
                if status.total_requests > 0 {
                    status.success_rate =
                        (status.success_requests as f32 / status.total_requests as f32) * 100.0;
                }
            }
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
        }

        // 鎵€鏈変緵搴斿晢閮藉け璐ヤ簡
        {
            let mut status = self.status.write().await;
            status.failed_requests += 1;
            status.last_error = Some("鎵€鏈変緵搴斿晢閮藉け璐?.to_string());
            if status.total_requests > 0 {
                status.success_rate =
                    (status.success_requests as f32 / status.total_requests as f32) * 100.0;
            }
        }

        if let Some((log_code, log_message)) =
            build_terminal_failure_log(attempted_providers, providers.len(), last_error.as_ref())
        {
            log::warn!("[{app_type_str}] [{log_code}] {log_message}");
        }

        Err(ForwardError {
            error: last_error.unwrap_or(ProxyError::MaxRetriesExceeded),
            provider: last_provider,
        })
    }

    /// 杞彂鍗曚釜璇锋眰锛堜娇鐢ㄩ€傞厤鍣級
    #[allow(clippy::too_many_arguments)]
    async fn forward(
        &self,
        app_type: &AppType,
        method: &http::Method,
        provider: &Provider,
        endpoint: &str,
        body: &Value,
        headers: &axum::http::HeaderMap,
        extensions: &Extensions,
        adapter: &dyn ProviderAdapter,
    ) -> Result<(ProxyResponse, Option<String>), ProxyError> {
        // 浣跨敤閫傞厤鍣ㄦ彁鍙?base_url
        let mut base_url = adapter.extract_base_url(provider)?;

        let is_full_url = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.is_full_url)
            .unwrap_or(false);

        // GitHub Copilot API 浣跨敤 /chat/completions锛堟棤 /v1 鍓嶇紑锛?        let is_copilot = provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("github_copilot")
            || base_url.contains("githubcopilot.com");

        // 搴旂敤妯″瀷鏄犲皠锛堢嫭绔嬩簬鏍煎紡杞崲锛?        // Claude Desktop proxy 妯″紡蹇呴』鍏堟妸 Desktop 鍙鐨?claude-* route
        // 鏄犲皠鎴愮湡瀹炰笂娓告ā鍨嬪悕锛屽苟涓旀湭鐭?route 瑕佺洿鎺ユ姤閿欙紝涓嶈兘浣跨敤榛樿妯″瀷鍏滃簳銆?        let mapped_body = if matches!(app_type, AppType::ClaudeDesktop) {
            crate::claude_desktop_config::map_proxy_request_model(body.clone(), provider)
                .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?
        } else {
            let (mapped_body, original_model_before_mapping, _mapped_model) =
                super::model_mapper::apply_model_mapping(body.clone(), provider);
            mapped_body
        };

        // 涓?CCH 瀵归綈锛氳姹傚墠涓嶅仛 thinking 涓诲姩鏀瑰啓锛堜粎淇濈暀鍏煎鍏ュ彛锛?        let mut mapped_body = normalize_thinking_type(mapped_body);

        // 澶氭ā鎬侀檷绾э細妫€娴嬭姹備腑鐨勫浘鐗囧唴瀹癸紝鑷姩鍒囨崲鍒伴閰嶇疆鐨勫妯℃€佹ā鍨?        // 鍙傝€?Copilot warmup 闄嶇骇妯″紡锛岄€傜敤浜?MiMo-v2.5-pro 鈫?mimo-v2.5 绛夊満鏅?        let has_images = super::model_mapper::request_contains_images(&mapped_body);
        if has_images {
            if let Some(fallback_model) = provider
                .meta
                .as_ref()
                .and_then(|m| m.multimodal_fallback_model.as_deref())
            {
                log::info!(
                    "[ModelMapper] 妫€娴嬪埌鍥剧墖鍐呭锛岄檷绾фā鍨? {} 鈫?{}",
                    mapped_body["model"].as_str().unwrap_or("?"),
                    fallback_model
                );
                mapped_body["model"] = serde_json::json!(fallback_model);
            } else {
                // 鏈厤缃妯℃€侀檷绾фā鍨嬶紝鎻愬墠杩斿洖鍙嬪ソ閿欒锛岄伩鍏嶆棤鎰忎箟鐨勪笂娓歌姹?                let current_model = mapped_body["model"].as_str().unwrap_or("unknown");
                log::warn!(
                    "[ModelMapper] 妫€娴嬪埌鍥剧墖鍐呭锛屼絾褰撳墠妯″瀷 {} 涓嶆敮鎸佸妯℃€佷笖鏈厤缃檷绾фā鍨?,
                    current_model
                );
                return Err(ProxyError::InvalidRequest(format!(
                    "The current model \"{}\" does not support image input. \
                     To enable automatic fallback, set a \"Multimodal Fallback Model\" in the provider settings. \
                     Alternatively, remove images from your request and try again.\n\
                     褰撳墠妯″瀷 \"{}\" 涓嶆敮鎸佸浘鐗囪緭鍏ャ€傝鍦ㄤ緵搴斿晢璁剧疆涓厤缃甛"澶氭ā鎬侀檷绾фā鍨媆"浠ヨ嚜鍔ㄥ垏鎹紝鎴栫Щ闄ゅ浘鐗囧悗閲嶈瘯銆?,
                    current_model, current_model
                )));
            }
        }

        if is_copilot {
            mapped_body =
                super::providers::copilot_model_map::apply_copilot_model_normalization(mapped_body);
            self.apply_copilot_live_model_resolution(provider, &mut mapped_body)
                .await;
        } else {
            mapped_body =
                super::model_mapper::strip_one_m_suffix_for_upstream_from_body(mapped_body);
        }

        // --- Copilot 浼樺寲鍣細鍒嗙被 + 璇锋眰浣撲紭鍖栵紙鍦ㄦ牸寮忚浆鎹箣鍓嶆墽琛岋級 ---
        // 娉ㄦ剰锛氱‘瀹氭€?ID 涔熷湪姝ゅ璁＄畻锛屽洜涓?mapped_body 鍦ㄦ牸寮忚浆鎹㈡椂浼氳 move
        //
        // 鎵ц椤哄簭锛堜笌 copilot-api 瀵归綈锛夛細
        //   1. 鍏堝湪鍘熷 body 涓婂垎绫伙紙淇濈暀 tool_result 璇箟锛岄伩鍏嶈鍒や负 user锛?        //   2. 鍐嶆竻娲楀绔?tool_result锛堥槻姝笂娓?API 鎶ラ敊锛?        //   3. 鍐嶅悎骞?tool_result + text锛堝噺灏?premium 璁¤垂锛?        let copilot_optimization = if is_copilot && self.copilot_optimizer_config.enabled {
            // 1. 鍦ㄥ師濮?body 涓婂垎绫?鈥?蹇呴』鍦ㄦ竻娲?鍚堝苟涔嬪墠鎵ц
            //    瀛ょ珛 tool_result 浠嶄繚鎸?tool_result 绫诲瀷锛屽垎绫昏兘姝ｇ‘璇嗗埆涓?agent
            let has_anthropic_beta = headers.contains_key("anthropic-beta");
            let classification = super::copilot_optimizer::classify_request(
                &mapped_body,
                has_anthropic_beta,
                self.copilot_optimizer_config.compact_detection,
                self.copilot_optimizer_config.subagent_detection,
            );

            log::debug!(
                "[Copilot] 浼樺寲鍣ㄥ垎绫? initiator={}, is_warmup={}, is_compact={}, is_subagent={}",
                classification.initiator,
                classification.is_warmup,
                classification.is_compact,
                classification.is_subagent
            );

            // 2. 瀛ょ珛 tool_result 娓呯悊 鈥?鍒嗙被瀹屾垚鍚庡啀娓呮礂
            //    闃叉涓婃父 API 鍥犱笉鍖归厤鐨?tool_result 鎶ラ敊瀵艰嚧閲嶈瘯/閲嶅璁¤垂
            mapped_body = super::copilot_optimizer::sanitize_orphan_tool_results(mapped_body);

            // 3. Tool result 鍚堝苟 鈥?灏?[tool_result, text] 鍙樹负 [tool_result(鍚玹ext)]
            if self.copilot_optimizer_config.tool_result_merging {
                mapped_body = super::copilot_optimizer::merge_tool_results(mapped_body);
            }

            // 3.5. 涓诲姩鍓ョ thinking block 鈥?Copilot 璧?OpenAI 鍏煎绔偣涓嶈瘑鍒鍧?            //      閬垮厤涓婃父鎷掔粷鍚庣敱 rectifier 鍙嶅簲寮忛噸璇曪紙棣栨璇锋眰宸叉秷鑰?quota锛?            if self.copilot_optimizer_config.strip_thinking {
                mapped_body = super::copilot_optimizer::strip_thinking_blocks(mapped_body);
            }

            // 4. Warmup 灏忔ā鍨嬮檷绾?            if self.copilot_optimizer_config.warmup_downgrade && classification.is_warmup {
                log::info!(
                    "[Copilot] Warmup 璇锋眰闄嶇骇鍒版ā鍨? {}",
                    self.copilot_optimizer_config.warmup_model
                );
                mapped_body["model"] =
                    serde_json::json!(&self.copilot_optimizer_config.warmup_model);
            }

            // 棰勮绠楃‘瀹氭€?Request ID锛堝湪 body 琚?move 涔嬪墠锛?            // Session 鎻愬彇浼樺厛绾э紙涓?session.rs extract_from_metadata 瀵归綈锛夛細
            //   1. metadata.user_id 涓殑 _session_ 鍚庣紑
            //   2. metadata.session_id锛堢洿鎺ュ瓧娈碉級
            //   3. raw metadata.user_id锛堟暣涓?fallback锛?            //   4. x-session-id header
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

            // 浠?session ID 娲剧敓绋冲畾鐨?interaction ID锛堝悓涓€涓诲璇濆叡浜級
            let interaction_id =
                super::copilot_optimizer::deterministic_interaction_id(&session_id);

            Some((classification, det_request_id, interaction_id))
        } else {
            None
        };

        // GitHub Copilot 鍔ㄦ€?endpoint 璺敱
        // 浠?CopilotAuthManager 鑾峰彇缂撳瓨鐨?API endpoint锛堟敮鎸佷紒涓氱増绛夐潪榛樿 endpoint锛?        if is_copilot && !is_full_url {
            if let Some(app_handle) = &self.app_handle {
                let copilot_state = app_handle.state::<CopilotAuthState>();
                let copilot_auth = copilot_state.0.read().await;

                // 浠?provider.meta 鑾峰彇鍏宠仈鐨?GitHub 璐﹀彿 ID
                let account_id = provider
                    .meta
                    .as_ref()
                    .and_then(|m| m.managed_account_id_for("github_copilot"));

                let dynamic_endpoint = match &account_id {
                    Some(id) => copilot_auth.get_api_endpoint(id).await,
                    None => copilot_auth.get_default_api_endpoint().await,
                };

                // 鍙湪鍔ㄦ€?endpoint 涓庡綋鍓?base_url 涓嶅悓鏃舵浛鎹?                if dynamic_endpoint != base_url {
                    log::debug!(
                        "[Copilot] 浣跨敤鍔ㄦ€?API endpoint: {} (鍘? {})",
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
                super::providers::normalize_anthropic_tool_thinking_history_for_provider(
                    &mut mapped_body,
                    provider,
                    api_format,
                );
            }
        }
        let needs_transform = match resolved_claude_api_format.as_deref() {
            Some(api_format) => super::providers::claude_api_format_needs_transform(api_format),
            None => adapter.needs_transform(provider),
        };
        let codex_responses_to_chat = matches!(app_type, AppType::Codex)
            && super::providers::should_convert_codex_responses_to_chat(provider, endpoint);
        let (effective_endpoint, passthrough_query) = if codex_responses_to_chat {
            rewrite_codex_responses_endpoint_to_chat(endpoint)
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

        let codex_chat_base_is_full_endpoint = codex_responses_to_chat
            && base_url
                .trim_end_matches('/')
                .to_ascii_lowercase()
                .ends_with("/chat/completions");

        let url = if matches!(resolved_claude_api_format.as_deref(), Some("gemini_native")) {
            super::gemini_url::resolve_gemini_native_url(
                &base_url,
                &effective_endpoint,
                is_full_url,
            )
        } else if is_full_url || codex_chat_base_is_full_endpoint {
            append_query_to_full_url(&base_url, passthrough_query.as_deref())
        } else {
            adapter.build_url(&base_url, &effective_endpoint)
        };

        // 杞崲璇锋眰浣擄紙濡傛灉闇€瑕侊級
        let request_body = if codex_responses_to_chat {
            let mut mapped_body = mapped_body;
            let restored = self
                .codex_chat_history
                .enrich_request(&mut mapped_body)
                .await;
            if restored > 0 {
                log::debug!(
                    "[Codex] Restored {restored} cached function call(s) for Chat upstream"
                );
            }
            super::providers::apply_codex_chat_upstream_model(provider, &mut mapped_body);
            let reasoning_config =
                super::providers::resolve_codex_chat_reasoning_config(provider, &mapped_body);
            super::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning(
                mapped_body,
                reasoning_config.as_ref(),
            )?
        } else if needs_transform {
            if adapter.name() == "Claude" {
                let api_format = resolved_claude_api_format
                    .as_deref()
                    .unwrap_or_else(|| super::providers::get_claude_api_format(provider));
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

        // 杩囨护绉佹湁鍙傛暟锛堜互 `_` 寮€澶寸殑瀛楁锛夛紝闃叉鍐呴儴淇℃伅娉勯湶鍒颁笂娓?        // 榛樿浣跨敤绌虹櫧鍚嶅崟锛岃繃婊ゆ墍鏈?_ 鍓嶇紑瀛楁
        let filtered_body = prepare_upstream_request_body(request_body);
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
        let force_identity_encoding =
            needs_transform || codex_responses_to_chat || request_is_streaming;

        // Codex OAuth 闇€瑕佹敞鍏ョ殑 ChatGPT-Account-Id锛堝湪鍔ㄦ€?token 鑾峰彇鏈熼棿濉厖锛?        let mut codex_oauth_account_id: Option<String> = None;
        let mut should_send_codex_oauth_session_headers = false;

        // 鑾峰彇璁よ瘉澶达紙鎻愬墠鍑嗗锛岀敤浜庡唴鑱旀浛鎹級
        let mut auth_headers = if let Some(mut auth) = adapter.extract_auth(provider) {
            // GitHub Copilot 鐗规畩澶勭悊锛氫粠 CopilotAuthManager 鑾峰彇鐪熷疄 token
            if auth.strategy == AuthStrategy::GitHubCopilot {
                if let Some(app_handle) = &self.app_handle {
                    let copilot_state = app_handle.state::<CopilotAuthState>();
                    let copilot_auth: tokio::sync::RwLockReadGuard<'_, CopilotAuthManager> =
                        copilot_state.0.read().await;

                    // 浠?provider.meta 鑾峰彇鍏宠仈鐨?GitHub 璐﹀彿 ID锛堝璐﹀彿鏀寔锛?                    let account_id = provider
                        .meta
                        .as_ref()
                        .and_then(|m| m.managed_account_id_for("github_copilot"));

                    // 鏍规嵁璐﹀彿 ID 鑾峰彇瀵瑰簲 token锛堝悜鍚庡吋瀹癸細鏃犺处鍙?ID 鏃朵娇鐢ㄧ涓€涓处鍙凤級
                    let token_result = match &account_id {
                        Some(id) => {
                            log::debug!("[Copilot] 浣跨敤鎸囧畾璐﹀彿 {id} 鑾峰彇 token");
                            copilot_auth.get_valid_token_for_account(id).await
                        }
                        None => {
                            log::debug!("[Copilot] 浣跨敤榛樿璐﹀彿鑾峰彇 token");
                            copilot_auth.get_valid_token().await
                        }
                    };

                    match token_result {
                        Ok(token) => {
                            auth = AuthInfo::new(token, AuthStrategy::GitHubCopilot);
                            log::debug!(
                                "[Copilot] 鎴愬姛鑾峰彇 Copilot token (account={})",
                                account_id.as_deref().unwrap_or("default")
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "[Copilot] 鑾峰彇 Copilot token 澶辫触 (account={}): {e}",
                                account_id.as_deref().unwrap_or("default")
                            );
                            return Err(ProxyError::AuthError(format!(
                                "GitHub Copilot 璁よ瘉澶辫触: {e}"
                            )));
                        }
                    }
                } else {
                    log::error!("[Copilot] AppHandle 涓嶅彲鐢?);
                    return Err(ProxyError::AuthError(
                        "GitHub Copilot 璁よ瘉涓嶅彲鐢紙鏃?AppHandle锛?.to_string(),
                    ));
                }
            }

            // Codex OAuth 鐗规畩澶勭悊锛氫粠 CodexOAuthManager 鑾峰彇鐪熷疄 access_token
            if auth.strategy == AuthStrategy::CodexOAuth {
                if let Some(app_handle) = &self.app_handle {
                    let codex_state = app_handle.state::<CodexOAuthState>();
                    let codex_auth: tokio::sync::RwLockReadGuard<'_, CodexOAuthManager> =
                        codex_state.0.read().await;

                    // 浠?provider.meta 鑾峰彇鍏宠仈鐨?ChatGPT 璐﹀彿 ID
                    let account_id = provider
                        .meta
                        .as_ref()
                        .and_then(|m| m.managed_account_id_for("codex_oauth"));

                    let token_result = match &account_id {
                        Some(id) => {
                            log::debug!("[CodexOAuth] 浣跨敤鎸囧畾璐﹀彿 {id} 鑾峰彇 token");
                            codex_auth.get_valid_token_for_account(id).await
                        }
                        None => {
                            log::debug!("[CodexOAuth] 浣跨敤榛樿璐﹀彿鑾峰彇 token");
                            codex_auth.get_valid_token().await
                        }
                    };

                    match token_result {
                        Ok(token) => {
                            auth = AuthInfo::new(token, AuthStrategy::CodexOAuth);
                            should_send_codex_oauth_session_headers = true;
                            // 瑙ｆ瀽浣跨敤鐨?account_id锛堢敤浜庢敞鍏?ChatGPT-Account-Id header锛?                            codex_oauth_account_id = match account_id {
                                Some(id) => Some(id),
                                None => codex_auth.default_account_id().await,
                            };
                            log::debug!(
                                "[CodexOAuth] 鎴愬姛鑾峰彇 access_token (account={})",
                                codex_oauth_account_id.as_deref().unwrap_or("default")
                            );
                        }
                        Err(e) => {
                            log::error!("[CodexOAuth] 鑾峰彇 access_token 澶辫触: {e}");
                            return Err(ProxyError::AuthError(format!(
                                "Codex OAuth 璁よ瘉澶辫触: {e}"
                            )));
                        }
                    }
                } else {
                    log::error!("[CodexOAuth] AppHandle 涓嶅彲鐢?);
                    return Err(ProxyError::AuthError(
                        "Codex OAuth 璁よ瘉涓嶅彲鐢紙鏃?AppHandle锛?.to_string(),
                    ));
                }
            }

            adapter.get_auth_headers(&auth)?
        } else {
            Vec::new()
        };

        // 娉ㄥ叆 Codex OAuth 鐨?ChatGPT-Account-Id header锛堝鏋滄湁 account_id锛?        if let Some(ref account_id) = codex_oauth_account_id {
            if let Ok(hv) = http::HeaderValue::from_str(account_id) {
                auth_headers.push((http::HeaderName::from_static("chatgpt-account-id"), hv));
            }
        }

        let codex_oauth_session_headers =
            if should_send_codex_oauth_session_headers && self.session_client_provided {
                build_codex_oauth_session_headers(&self.session_id)
            } else {
                Vec::new()
            };

        // --- Copilot 浼樺寲鍣細鍔ㄦ€?header 娉ㄥ叆 ---
        if let Some((ref classification, ref det_request_id, ref interaction_id)) =
            copilot_optimization
        {
            for (name, value) in auth_headers.iter_mut() {
                match name.as_str() {
                    "x-initiator" if self.copilot_optimizer_config.request_classification => {
                        *value = http::HeaderValue::from_static(classification.initiator);
                    }
                    "x-interaction-type" if classification.is_subagent => {
                        // 瀛愪唬鐞嗚姹傦細conversation-subagent 涓嶈 premium interaction
                        *value = http::HeaderValue::from_static("conversation-subagent");
                    }
                    "x-request-id" | "x-agent-task-id" => {
                        if let Some(ref det_id) = det_request_id {
                            if let Ok(hv) = http::HeaderValue::from_str(det_id) {
                                *value = hv;
                            }
                        }
                    }
                    _ => {}
                }
            }

            // x-interaction-id锛氫粎鍦ㄦ湁 session 鏃舵敞鍏ワ紙涓嶅湪 get_auth_headers 涓級
            if let Some(ref iid) = interaction_id {
                if let Ok(hv) = http::HeaderValue::from_str(iid) {
                    auth_headers.push((http::HeaderName::from_static("x-interaction-id"), hv));
                }
            }

            if classification.is_subagent {
                log::info!(
                    "[Copilot] 瀛愪唬鐞嗚姹? x-initiator=agent, x-interaction-type=conversation-subagent"
                );
            }
        }

        // Copilot 鎸囩汗澶村悕锛堢敱 get_auth_headers 娉ㄥ叆锛岄渶鍦ㄥ師濮嬪ご涓幓閲嶏級
        let copilot_fingerprint_headers: &[&str] = if is_copilot {
            &[
                "user-agent",
                "editor-version",
                "editor-plugin-version",
                "copilot-integration-id",
                "x-github-api-version",
                "openai-intent",
                // 鏂板 headers
                "x-initiator",
                "x-interaction-type",
                "x-interaction-id",
                "x-vscode-user-agent-library-version",
                "x-request-id",
                "x-agent-task-id",
            ]
        } else {
            &[]
        };

        // 棰勮绠椾笂娓?host 鍊硷紙鐢ㄤ簬鍦ㄥ師浣嶆浛鎹?host header锛?        let upstream_host = url
            .parse::<http::Uri>()
            .ok()
            .and_then(|u| u.authority().map(|a| a.to_string()));

        let should_send_anthropic_headers = adapter.name() == "Claude"
            && matches!(resolved_claude_api_format.as_deref(), Some("anthropic"));

        // 棰勮绠?anthropic-beta 鍊硷紙浠?Claude锛?        let anthropic_beta_value = if should_send_anthropic_headers {
            const CLAUDE_CODE_BETA: &str = "claude-code-20250219";
            Some(if let Some(beta) = headers.get("anthropic-beta") {
                if let Ok(beta_str) = beta.to_str() {
                    if beta_str.contains(CLAUDE_CODE_BETA) {
                        beta_str.to_string()
                    } else {
                        format!("{CLAUDE_CODE_BETA},{beta_str}")
                    }
                } else {
                    CLAUDE_CODE_BETA.to_string()
                }
            } else {
                CLAUDE_CODE_BETA.to_string()
            })
        } else {
            None
        };

        // ============================================================
        // 鏋勫缓鏈夊簭 HeaderMap 鈥?鍐呰仈鏇挎崲锛屼繚鎸佸鎴风鍘熷椤哄簭
        // ============================================================
        let mut ordered_headers = http::HeaderMap::new();
        let mut saw_auth = false;
        let mut saw_accept_encoding = false;
        let mut saw_anthropic_beta = false;
        let mut saw_anthropic_version = false;

        for (key, value) in headers {
            let key_str = key.as_str();

            // --- host 鈥?鍘熶綅鏇挎崲涓轰笂娓?host锛堜繚鎸佸鎴风鍘熷浣嶇疆锛?---
            if key_str.eq_ignore_ascii_case("host") {
                if let Some(ref host_val) = upstream_host {
                    if let Ok(hv) = http::HeaderValue::from_str(host_val) {
                        ordered_headers.append(key.clone(), hv);
                    }
                }
                continue;
            }

            // --- 杩炴帴 / 杩借釜 / CDN 绫?鈥?鏃犳潯浠惰烦杩?---
            if matches!(
                key_str,
                "content-length"
                    | "transfer-encoding"
                    | "x-forwarded-host"
                    | "x-forwarded-port"
                    | "x-forwarded-proto"
                    | "forwarded"
                    | "cf-connecting-ip"
                    | "cf-ipcountry"
                    | "cf-ray"
                    | "cf-visitor"
                    | "true-client-ip"
                    | "fastly-client-ip"
                    | "x-azure-clientip"
                    | "x-azure-fdid"
                    | "x-azure-ref"
                    | "akamai-origin-hop"
                    | "x-akamai-config-log-detail"
                    | "x-request-id"
                    | "x-correlation-id"
                    | "x-trace-id"
                    | "x-amzn-trace-id"
                    | "x-b3-traceid"
                    | "x-b3-spanid"
                    | "x-b3-parentspanid"
                    | "x-b3-sampled"
                    | "traceparent"
                    | "tracestate"
            ) {
                continue;
            }

            // --- 璁よ瘉绫?鈥?鐢?adapter 鎻愪緵鐨勮璇佸ご鏇挎崲锛堝湪鍘熷浣嶇疆锛?---
            if key_str.eq_ignore_ascii_case("authorization")
                || key_str.eq_ignore_ascii_case("x-api-key")
                || key_str.eq_ignore_ascii_case("x-goog-api-key")
            {
                if !saw_auth {
                    saw_auth = true;
                    for (ah_name, ah_value) in &auth_headers {
                        ordered_headers.append(ah_name.clone(), ah_value.clone());
                    }
                }
                continue;
            }

            // --- accept-encoding 鈥?transform / SSE 璺緞寮哄埗 identity锛屽叾浣欎繚鐣欏師鍊?---
            if key_str.eq_ignore_ascii_case("accept-encoding") {
                if !saw_accept_encoding {
                    saw_accept_encoding = true;
                    if force_identity_encoding {
                        ordered_headers.append(
                            http::header::ACCEPT_ENCODING,
                            http::HeaderValue::from_static("identity"),
                        );
                    } else {
                        ordered_headers.append(key.clone(), value.clone());
                    }
                }
                continue;
            }

            // --- anthropic-beta 鈥?鐢ㄩ噸寤哄€兼浛鎹紙纭繚鍚?claude-code 鏍囪锛?---
            if key_str.eq_ignore_ascii_case("anthropic-beta") {
                if !saw_anthropic_beta {
                    saw_anthropic_beta = true;
                    if let Some(ref beta_val) = anthropic_beta_value {
                        if let Ok(hv) = http::HeaderValue::from_str(beta_val) {
                            ordered_headers.append("anthropic-beta", hv);
                        }
                    }
                }
                continue;
            }

            // --- anthropic-version 鈥?閫忎紶瀹㈡埛绔€?---
            if key_str.eq_ignore_ascii_case("anthropic-version") {
                if should_send_anthropic_headers {
                    saw_anthropic_version = true;
                    ordered_headers.append(key.clone(), value.clone());
                }
                continue;
            }

            // --- Copilot 鎸囩汗澶?鈥?璺宠繃锛堢敱 auth_headers 鎻愪緵锛?---
            if copilot_fingerprint_headers
                .iter()
                .any(|h| key_str.eq_ignore_ascii_case(h))
            {
                continue;
            }

            // --- 榛樿锛氶€忎紶 ---
            ordered_headers.append(key.clone(), value.clone());
        }

        // 濡傛灉鍘熷璇锋眰涓病鏈夎璇佸ご锛屽湪鏈熬杩藉姞
        if !saw_auth && !auth_headers.is_empty() {
            for (ah_name, ah_value) in &auth_headers {
                ordered_headers.append(ah_name.clone(), ah_value.clone());
            }
        }

        // transform / SSE 璺緞鍦ㄧ己澶辨椂琛?identity锛涙櫘閫氶€忎紶涓嶄富鍔ㄨˉ accept-encoding
        if !saw_accept_encoding && force_identity_encoding {
            ordered_headers.append(
                http::header::ACCEPT_ENCODING,
                http::HeaderValue::from_static("identity"),
            );
        }

        // 濡傛灉鍘熷璇锋眰涓病鏈?anthropic-beta 涓旀湁鍊奸渶瑕佹坊鍔狅紝杩藉姞
        if !saw_anthropic_beta {
            if let Some(ref beta_val) = anthropic_beta_value {
                if let Ok(hv) = http::HeaderValue::from_str(beta_val) {
                    ordered_headers.append("anthropic-beta", hv);
                }
            }
        }

        // anthropic-version锛氫粎鍦ㄧ己澶辨椂琛ュ厖榛樿鍊?        if should_send_anthropic_headers && !saw_anthropic_version {
            ordered_headers.append(
                "anthropic-version",
                http::HeaderValue::from_static("2023-06-01"),
            );
        }

        // Codex OAuth 鍙嶄唬灏介噺瀵归綈瀹樻柟 Codex CLI 鐨勪細璇濊矾鐢变俊鍙枫€?        // 鍙彂閫佸鎴风鎻愪緵鐨?session_id锛涚敓鎴愮殑 UUID 姣忔涓嶅悓锛屽弽鑰屼細鐮村潖鍓嶇紑缂撳瓨銆?        for (name, value) in codex_oauth_session_headers {
            ordered_headers.insert(name, value);
        }

        // 搴忓垪鍖栬姹備綋銆侴ET/HEAD 鏄?idempotent/safe 鏂规硶锛屾寜 HTTP 璇箟涓嶅簲鎼哄甫 body锛?        // 寮鸿闄勫甫 JSON body 浼氳鏌愪簺涓婃父锛堝 Google Gemini 鐨?models.list锛夋嫆缁濊姹傘€?        let body_bytes = if matches!(method, &http::Method::GET | &http::Method::HEAD) {
            Vec::new()
        } else {
            serde_json::to_vec(&filtered_body).map_err(|e| {
                ProxyError::Internal(format!("Failed to serialize request body: {e}"))
            })?
        };

        // 纭繚 content-type 瀛樺湪
        if !ordered_headers.contains_key(http::header::CONTENT_TYPE) {
            ordered_headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            );
        }

        reject_proxy_placeholder_for_managed_account_upstream(&url, &ordered_headers)?;

        // 杈撳嚭璇锋眰淇℃伅鏃ュ織
        let tag = adapter.name();
        let request_model = filtered_body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("<none>");
        log::info!("[{tag}] >>> 璇锋眰 URL: {url} (model={request_model})");
        if log::log_enabled!(log::Level::Debug) {
            if let Ok(body_str) = serde_json::to_string(&filtered_body) {
                log::debug!(
                    "[{tag}] >>> 璇锋眰浣撳唴瀹?({}瀛楄妭): {}",
                    body_str.len(),
                    body_str
                );
            }
        }

        // 纭畾瓒呮椂
        let timeout = if self.non_streaming_timeout.is_zero() {
            std::time::Duration::from_secs(600) // 榛樿 600 绉?        } else {
            self.non_streaming_timeout
        };

        // 鑾峰彇鍏ㄥ眬浠ｇ悊 URL
        let upstream_proxy_url: Option<String> = super::http_client::get_current_proxy_url();

        // SOCKS5 浠ｇ悊涓嶆敮鎸?CONNECT 闅ч亾锛岄渶瑕佺敤 reqwest
        let is_socks_proxy = upstream_proxy_url
            .as_deref()
            .map(|u| u.starts_with("socks5"))
            .unwrap_or(false);

        let preserve_exact_header_case = should_preserve_exact_header_case(
            adapter.name(),
            provider,
            resolved_claude_api_format.as_deref(),
            is_copilot,
        );

        // 鍙戦€佽姹?        let response = if is_socks_proxy || !preserve_exact_header_case {
            // OpenAI / Copilot / Codex 绫诲悗绔笉渚濊禆鍘熷 header 澶у皬鍐欙紱璧?reqwest
            // 杩炴帴姹狅紝閬垮厤 raw TCP/TLS path 姣忔璇锋眰閮介噸鏂版彙鎵嬨€係OCKS5 涔熷彧鑳借蛋 reqwest銆?            log::debug!(
                "[Forwarder] Using pooled reqwest client (preserve_exact_header_case={preserve_exact_header_case}, socks_proxy={is_socks_proxy})"
            );
            let client = super::http_client::get();
            let mut request = client.request(method.clone(), &url);
            if request_is_streaming {
                // reqwest 鐨?timeout 鏄暣璇锋眰瓒呮椂锛涙祦寮忚姹備氦缁?response_processor
                // 鐨勯鍖?闈欓粯鏈熻秴鏃舵帶鍒讹紝閬垮厤闀挎祦琚€绘椂闀胯鏉€銆?                request = request.timeout(std::time::Duration::from_secs(24 * 60 * 60));
            } else if !self.non_streaming_timeout.is_zero() {
                request = request.timeout(self.non_streaming_timeout);
            }
            for (key, value) in &ordered_headers {
                request = request.header(key, value);
            }
            let send = request.body(body_bytes).send();
            let send_result = if request_is_streaming {
                let header_timeout = if self.streaming_first_byte_timeout.is_zero() {
                    timeout
                } else {
                    self.streaming_first_byte_timeout
                };
                tokio::time::timeout(header_timeout, send)
                    .await
                    .map_err(|_| {
                        ProxyError::Timeout(format!(
                            "娴佸紡鍝嶅簲棣栧寘瓒呮椂: {}s锛堜笂娓告湭杩斿洖鍝嶅簲澶达級",
                            header_timeout.as_secs()
                        ))
                    })?
            } else {
                send.await
            };
            let reqwest_resp = send_result.map_err(map_reqwest_send_error)?;
            ProxyResponse::Reqwest(reqwest_resp)
        } else {
            // HTTP 浠ｇ悊鎴栫洿杩烇細璧?hyper raw write锛堜繚鎸?header 澶у皬鍐欙級
            // 濡傛灉鏈?HTTP 浠ｇ悊锛宧yper_client 浼氱敤 CONNECT 闅ч亾绌胯繃浠ｇ悊
            let uri: http::Uri = url
                .parse()
                .map_err(|e| ProxyError::ForwardFailed(format!("Invalid URL '{url}': {e}")))?;
            super::hyper_client::send_request(
                uri,
                method.clone(),
                ordered_headers,
                extensions.clone(),
                body_bytes,
                timeout,
                upstream_proxy_url.as_deref(),
            )
            .await?
        };

        // 妫€鏌ュ搷搴旂姸鎬?        let status = response.status();

        if status.is_success() {
            let response = self
                .prepare_success_response_for_failover(response, request_is_streaming)
                .await?;
            Ok((response, resolved_claude_api_format))
        } else {
            let status_code = status.as_u16();
            let body_text = String::from_utf8(response.bytes().await?.to_vec()).ok();

            Err(ProxyError::UpstreamError {
                status: status_code,
                body: body_text,
            })
        }
    }

    /// 鏁呴殰杞Щ寮€鍚椂锛屾垚鍔熶笉鑳藉彧鐪嬩笂娓稿搷搴斿ご銆?    ///
    /// - 闈炴祦寮忥細鍏堟妸瀹屾暣 body 璇诲埌鍐呭瓨锛岃瓒呮椂/杩炴帴涓柇浼氬洖鍒?retry loop 灏濊瘯涓嬩竴瀹躲€?    /// - 娴佸紡锛氳嚦灏戠瓑棣栦釜 chunk 鍒拌揪锛岄伩鍏嶄笂娓歌繑鍥?200 鍚庝竴鐩翠笉鍚?SSE 鏃惰璇鎴愬姛銆?    async fn prepare_success_response_for_failover(
        &self,
        response: ProxyResponse,
        request_is_streaming: bool,
    ) -> Result<ProxyResponse, ProxyError> {
        if request_is_streaming {
            return self.prime_streaming_response(response).await;
        }

        if self.non_streaming_timeout.is_zero() {
            return Ok(response);
        }

        let status = response.status();
        let headers = response.headers().clone();
        let body_timeout = self.non_streaming_timeout;
        let body = tokio::time::timeout(body_timeout, response.bytes())
            .await
            .map_err(|_| {
                ProxyError::Timeout(format!(
                    "鍝嶅簲浣撹鍙栬秴鏃? {}s锛堜笂娓稿彂瀹屽搷搴斿ご鍚?body 鏈埌杈撅級",
                    body_timeout.as_secs()
                ))
            })??;

        Ok(ProxyResponse::buffered(status, headers, body))
    }

    async fn prime_streaming_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        if self.streaming_first_byte_timeout.is_zero() {
            return Ok(response);
        }

        let status = response.status();
        let headers = response.headers().clone();
        let timeout = self.streaming_first_byte_timeout;
        let mut stream = Box::pin(response.bytes_stream());

        let first = tokio::time::timeout(timeout, stream.next())
            .await
            .map_err(|_| {
                ProxyError::Timeout(format!(
                    "娴佸紡鍝嶅簲棣栧寘瓒呮椂: {}s锛堜笂娓稿凡杩斿洖鍝嶅簲澶翠絾鏈繑鍥炴暟鎹級",
                    timeout.as_secs()
                ))
            })?;

        let Some(first) = first else {
            return Err(ProxyError::ForwardFailed(
                "娴佸紡鍝嶅簲鍦ㄩ鍖呭埌杈惧墠缁撴潫".to_string(),
            ));
        };

        let first =
            first.map_err(|e| ProxyError::ForwardFailed(format!("璇诲彇娴佸紡鍝嶅簲棣栧寘澶辫触: {e}")))?;

        let replay = futures::stream::once(async move { Ok(first) }).chain(stream);
        Ok(ProxyResponse::streamed(status, headers, replay))
    }

    async fn resolve_claude_api_format(
        &self,
        provider: &Provider,
        body: &Value,
        is_copilot: bool,
    ) -> String {
        if !is_copilot {
            return super::providers::get_claude_api_format(provider).to_string();
        }

        let model = body.get("model").and_then(|value| value.as_str());
        if let Some(model_id) = model {
            if self
                .is_copilot_openai_vendor_model(provider, model_id)
                .await
            {
                return "openai_responses".to_string();
            }
        }

        "openai_chat".to_string()
    }

    /// 鐢?Copilot live `/models` 鍒楄〃纭 model ID 鐪熷疄鍙敤锛屾壘涓嶅埌鏃舵寜 family 闄嶇骇銆?    /// 鍛戒腑缂撳瓨鍚庢槸鍚屾鐨勶紱棣栨璇锋眰鎴?5 min 缂撳瓨杩囨湡鍚庝細瑙﹀彂涓€娆?HTTP銆?    async fn apply_copilot_live_model_resolution(
        &self,
        provider: &Provider,
        body: &mut serde_json::Value,
    ) {
        let Some(model_id) = body.get("model").and_then(|v| v.as_str()) else {
            return;
        };
        let model_id = model_id.to_string();

        let Some(app_handle) = &self.app_handle else {
            return;
        };
        let copilot_state = app_handle.state::<CopilotAuthState>();
        let copilot_auth = copilot_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"));

        let models_result = match account_id.as_deref() {
            Some(id) => copilot_auth.fetch_models_for_account(id).await,
            None => copilot_auth.fetch_models().await,
        };

        let models = match models_result {
            Ok(m) => m,
            Err(err) => {
                log::debug!("[Copilot] live model list unavailable, skip resolution: {err}");
                return;
            }
        };

        if let Some(resolved) =
            super::providers::copilot_model_map::resolve_against_models(&model_id, &models)
        {
            log::info!("[Copilot] live-model resolve: {model_id} 鈫?{resolved}");
            body["model"] = serde_json::Value::String(resolved);
        }
    }

    async fn is_copilot_openai_vendor_model(&self, provider: &Provider, model_id: &str) -> bool {
        let Some(app_handle) = &self.app_handle else {
            log::debug!("[Copilot] AppHandle unavailable, fallback to chat/completions");
            return false;
        };

        let copilot_state = app_handle.state::<CopilotAuthState>();
        let copilot_auth = copilot_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"));

        let vendor_result = match account_id.as_deref() {
            Some(id) => {
                copilot_auth
                    .get_model_vendor_for_account(id, model_id)
                    .await
            }
            None => copilot_auth.get_model_vendor(model_id).await,
        };

        match vendor_result {
            Ok(Some(vendor)) => vendor.eq_ignore_ascii_case("openai"),
            Ok(None) => {
                log::debug!(
                    "[Copilot] Model vendor unavailable for {model_id}, fallback to chat/completions"
                );
                false
            }
            Err(err) => {
                log::warn!(
                    "[Copilot] Failed to resolve model vendor for {model_id}, fallback to chat/completions: {err}"
                );
                false
            }
        }
    }

    fn categorize_proxy_error(&self, error: &ProxyError) -> ErrorCategory {
        match error {
            // 缃戠粶鍜屼笂娓搁敊璇細閮藉簲璇ュ皾璇曚笅涓€涓緵搴斿晢
            ProxyError::Timeout(_) => ErrorCategory::Retryable,
            ProxyError::ForwardFailed(_) => ErrorCategory::Retryable,
            ProxyError::ProviderUnhealthy(_) => ErrorCategory::Retryable,
            // 涓婃父 HTTP 閿欒锛氭寜鐘舵€佺爜鍒嗘《銆?            //
            // 瀹㈡埛绔姹傝嚜韬湁闂鐨勭姸鎬佺爜鏃犺鎹㈠摢涓?provider 閮戒細琚嫆缁濓紝
            // 缁х画杞鍙細鏀惧ぇ閿欒鐜囥€佹薄鏌撶啍鏂櫒鍋ュ悍搴︺€佹氮璐归厤棰濓細
            //   400 Bad Request / 422 Unprocessable Entity   鈫?璇锋眰浣撴牸寮忔垨璇箟閿欒
            //   405 Method Not Allowed / 406 Not Acceptable  鈫?鏂规硶鎴?Accept 閿欒
            //   413 Payload Too Large / 414 URI Too Long     鈫?瀹㈡埛绔瀯閫犺秴闄?            //   415 Unsupported Media Type                    鈫?Content-Type 閿欒
            //   501 Not Implemented                           鈫?涓婃父鍗忚纭疄涓嶆敮鎸?            //
            // 鍏朵粬 4xx锛?01/403/404/408/409/429/451 绛夛級鍜屽叏閮?5xx 閮戒繚鐣?            // Retryable 鈥斺€?鎹竴瀹?provider 鍙兘鎸佹湁涓嶅悓鐨?key銆侀厤棰濄€佸湴鍩熸垨妯″瀷鏄犲皠銆?            ProxyError::UpstreamError { status, .. } => match *status {
                400 | 405 | 406 | 413 | 414 | 415 | 422 | 501 => ErrorCategory::NonRetryable,
                _ => ErrorCategory::Retryable,
            },
            // Provider 绾ч厤缃?杞崲闂锛氭崲涓€涓?Provider 鍙兘灏辫兘鎴愬姛
            ProxyError::ConfigError(_) => ErrorCategory::Retryable,
            ProxyError::TransformError(_) => ErrorCategory::Retryable,
            ProxyError::AuthError(_) => ErrorCategory::Retryable,
            ProxyError::StreamIdleTimeout(_) => ErrorCategory::Retryable,
            // 鏃犲彲鐢ㄤ緵搴斿晢锛氭墍鏈変緵搴斿晢閮借瘯杩囦簡锛屾棤娉曢噸璇?            ProxyError::NoAvailableProvider => ErrorCategory::NonRetryable,
            // 鍏朵粬閿欒锛堟暟鎹簱/鍐呴儴閿欒绛夛級锛氫笉鏄崲渚涘簲鍟嗚兘瑙ｅ喅鐨勯棶棰?            _ => ErrorCategory::NonRetryable,
        }
    }
}

/// 浠?ProxyError 涓彁鍙栭敊璇秷鎭?fn extract_error_message(error: &ProxyError) -> Option<String> {
    match error {
        ProxyError::UpstreamError { body, .. } => body.clone(),
        _ => Some(error.to_string()),
    }
}

/// 妫€娴?Provider 鏄惁涓?Bedrock锛堥€氳繃 CLAUDE_CODE_USE_BEDROCK 鐜鍙橀噺鍒ゆ柇锛?fn is_bedrock_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_USE_BEDROCK"))
        .and_then(|v| v.as_str())
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn build_retryable_failure_log(
    provider_name: &str,
    attempted_providers: usize,
    total_providers: usize,
    error: &ProxyError,
) -> (&'static str, String) {
    let error_summary = summarize_proxy_error(error);

    if total_providers <= 1 {
        (
            log_fwd::SINGLE_PROVIDER_FAILED,
            format!("Provider {provider_name} 璇锋眰澶辫触: {error_summary}"),
        )
    } else {
        (
            log_fwd::PROVIDER_FAILED_RETRY,
            format!(
                "Provider {provider_name} 澶辫触锛岀户缁皾璇曚笅涓€涓?({attempted_providers}/{total_providers}): {error_summary}"
            ),
        )
    }
}

fn build_terminal_failure_log(
    attempted_providers: usize,
    total_providers: usize,
    last_error: Option<&ProxyError>,
) -> Option<(&'static str, String)> {
    if total_providers <= 1 {
        return None;
    }

    let error_summary = last_error
        .map(summarize_proxy_error)
        .unwrap_or_else(|| "鏈煡閿欒".to_string());

    Some((
        log_fwd::ALL_PROVIDERS_FAILED,
        format!(
            "宸插皾璇?{attempted_providers}/{total_providers} 涓?Provider锛屽潎澶辫触銆傛渶鍚庨敊璇? {error_summary}"
        ),
    ))
}

fn summarize_proxy_error(error: &ProxyError) -> String {
    match error {
        ProxyError::UpstreamError { status, body } => {
            let body_summary = body
                .as_deref()
                .map(summarize_upstream_body)
                .filter(|summary| !summary.is_empty());

            match body_summary {
                Some(summary) => format!("涓婃父 HTTP {status}: {summary}"),
                None => format!("涓婃父 HTTP {status}"),
            }
        }
        ProxyError::Timeout(message) => {
            format!("璇锋眰瓒呮椂: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ForwardFailed(message) => {
            format!("璇锋眰杞彂澶辫触: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::TransformError(message) => {
            format!("鍝嶅簲杞崲澶辫触: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ConfigError(message) => {
            format!("閰嶇疆閿欒: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::AuthError(message) => {
            format!("璁よ瘉澶辫触: {}", summarize_text_for_log(message, 180))
        }
        _ => summarize_text_for_log(&error.to_string(), 180),
    }
}

fn summarize_upstream_body(body: &str) -> String {
    if let Ok(json_body) = serde_json::from_str::<Value>(body) {
        if let Some(message) = extract_json_error_message(&json_body) {
            return summarize_text_for_log(&message, 180);
        }

        if let Ok(compact_json) = serde_json::to_string(&json_body) {
            return summarize_text_for_log(&compact_json, 180);
        }
    }

    summarize_text_for_log(body, 180)
}

fn extract_json_error_message(body: &Value) -> Option<String> {
    let candidates = [
        body.pointer("/error/message"),
        body.pointer("/message"),
        body.pointer("/detail"),
        body.pointer("/error"),
    ];

    candidates
        .into_iter()
        .flatten()
        .find_map(|value| value.as_str().map(ToString::to_string))
}

fn split_endpoint_and_query(endpoint: &str) -> (&str, Option<&str>) {
    endpoint
        .split_once('?')
        .map_or((endpoint, None), |(path, query)| (path, Some(query)))
}

fn strip_beta_query(query: Option<&str>) -> Option<String> {
    let filtered = query.map(|query| {
        query
            .split('&')
            .filter(|pair| !pair.is_empty() && !pair.starts_with("beta="))
            .collect::<Vec<_>>()
            .join("&")
    });

    match filtered.as_deref() {
        Some("") | None => None,
        Some(_) => filtered,
    }
}

fn is_claude_messages_path(path: &str) -> bool {
    matches!(path, "/v1/messages" | "/claude/v1/messages")
}

fn rewrite_codex_responses_endpoint_to_chat(endpoint: &str) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    let target_path = "/chat/completions";
    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

fn rewrite_claude_transform_endpoint(
    endpoint: &str,
    api_format: &str,
    is_copilot: bool,
    body: &Value,
) -> (String, Option<String>) {
    let (path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = if is_claude_messages_path(path) {
        strip_beta_query(query)
    } else {
        query.map(ToString::to_string)
    };

    if !is_claude_messages_path(path) {
        return (endpoint.to_string(), passthrough_query);
    }

    if api_format == "gemini_native" {
        let model =
            super::providers::transform_gemini::extract_gemini_model(body).unwrap_or("unknown");
        // Accept both bare ids (`gemini-2.5-pro`) and the resource-name
        // form (`models/gemini-2.5-pro`) that Gemini SDKs emit. See
        // `normalize_gemini_model_id` for rationale.
        let model = super::gemini_url::normalize_gemini_model_id(model);
        let is_stream = body
            .get("stream")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let target_path = if is_stream {
            format!("/v1beta/models/{model}:streamGenerateContent")
        } else {
            format!("/v1beta/models/{model}:generateContent")
        };

        let rewritten_query = merge_query_params(
            passthrough_query.as_deref(),
            if is_stream { Some("alt=sse") } else { None },
        );

        let rewritten = match rewritten_query.as_deref() {
            Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
            _ => target_path,
        };

        return (rewritten, rewritten_query);
    }

    let target_path = if is_copilot && api_format == "openai_responses" {
        "/v1/responses"
    } else if is_copilot {
        "/chat/completions"
    } else if api_format == "openai_responses" {
        "/v1/responses"
    } else {
        "/v1/chat/completions"
    };

    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

fn merge_query_params(base_query: Option<&str>, extra_param: Option<&str>) -> Option<String> {
    let mut params: Vec<String> = base_query
        .into_iter()
        .flat_map(|query| query.split('&'))
        .filter(|pair| !pair.is_empty())
        .filter(|pair| !pair.starts_with("alt="))
        .map(ToString::to_string)
        .collect();

    if let Some(extra_param) = extra_param {
        params.push(extra_param.to_string());
    }

    if params.is_empty() {
        None
    } else {
        Some(params.join("&"))
    }
}

fn append_query_to_full_url(base_url: &str, query: Option<&str>) -> String {
    match query {
        Some(query) if !query.is_empty() => {
            if base_url.contains('?') {
                format!("{base_url}&{query}")
            } else {
                format!("{base_url}?{query}")
            }
        }
        _ => base_url.to_string(),
    }
}

fn build_codex_oauth_session_headers(
    session_id: &str,
) -> Vec<(http::HeaderName, http::HeaderValue)> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Vec::new();
    }

    let mut headers = Vec::new();
    if let Ok(value) = http::HeaderValue::from_str(session_id) {
        headers.push((http::HeaderName::from_static("session_id"), value.clone()));
        headers.push((http::HeaderName::from_static("x-client-request-id"), value));
    }

    let window_id = format!("{session_id}:0");
    if let Ok(value) = http::HeaderValue::from_str(&window_id) {
        headers.push((http::HeaderName::from_static("x-codex-window-id"), value));
    }

    headers
}

fn reject_proxy_placeholder_for_managed_account_upstream(
    url: &str,
    headers: &http::HeaderMap,
) -> Result<(), ProxyError> {
    if !is_managed_account_upstream_url(url) || !headers_contain_proxy_placeholder(headers) {
        return Ok(());
    }

    Err(ProxyError::AuthError(
        "Managed account proxy auth was not resolved; PROXY_MANAGED must not be sent upstream"
            .to_string(),
    ))
}

fn is_managed_account_upstream_url(url: &str) -> bool {
    let Ok(uri) = url.parse::<http::Uri>() else {
        return false;
    };

    let Some(host) = uri.host().map(str::to_ascii_lowercase) else {
        return false;
    };

    host == "githubcopilot.com"
        || host.ends_with(".githubcopilot.com")
        || (host == "chatgpt.com" && uri.path().starts_with("/backend-api/codex"))
}

fn headers_contain_proxy_placeholder(headers: &http::HeaderMap) -> bool {
    headers.values().any(|value| {
        value
            .to_str()
            .map(|value| value.contains(PROXY_AUTH_PLACEHOLDER))
            .unwrap_or(false)
    })
}

fn should_preserve_exact_header_case(
    adapter_name: &str,
    provider: &Provider,
    resolved_claude_api_format: Option<&str>,
    is_copilot: bool,
) -> bool {
    if matches!(adapter_name, "Codex" | "Gemini") {
        return false;
    }

    if is_copilot || provider.is_codex_oauth() {
        return false;
    }

    matches!(resolved_claude_api_format, None | Some("anthropic"))
}

fn is_streaming_request(endpoint: &str, body: &Value, headers: &axum::http::HeaderMap) -> bool {
    if body
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return true;
    }

    if endpoint.contains("streamGenerateContent") || endpoint.contains("alt=sse") {
        return true;
    }

    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|accept| accept.contains("text/event-stream"))
        .unwrap_or(false)
}

#[cfg(test)]
fn should_force_identity_encoding(
    endpoint: &str,
    body: &Value,
    headers: &axum::http::HeaderMap,
) -> bool {
    is_streaming_request(endpoint, body, headers)
}

fn map_reqwest_send_error(error: reqwest::Error) -> ProxyError {
    if error.is_timeout() {
        ProxyError::Timeout(format!("璇锋眰瓒呮椂: {error}"))
    } else if error.is_connect() {
        ProxyError::ForwardFailed(format!("杩炴帴澶辫触: {error}"))
    } else {
        ProxyError::ForwardFailed(error.to_string())
    }
}

fn summarize_text_for_log(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();

    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let truncated: String = trimmed.chars().take(max_chars).collect();
    let truncated = truncated.trim_end();
    format!("{truncated}...")
}

fn prepare_upstream_request_body(request_body: Value) -> Value {
    canonicalize_value(filter_private_params_with_whitelist(request_body, &[]))
}

fn log_prompt_cache_trace(
    app_type: &AppType,
    provider: &Provider,
    endpoint: &str,
    api_format: Option<&str>,
    body: &Value,
    session_client_provided: bool,
) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    let prompt_cache_key = body
        .get("prompt_cache_key")
        .and_then(|value| value.as_str())
        .map(|key| format!("present(len={})", key.len()))
        .unwrap_or_else(|| "absent".to_string());
    let store = body
        .get("store")
        .map(value_for_log)
        .unwrap_or_else(|| "absent".to_string());
    let stream = body
        .get("stream")
        .map(value_for_log)
        .unwrap_or_else(|| "absent".to_string());

    log::debug!(
        "[CacheTrace] app={}, provider={}, endpoint={}, api_format={}, session_client_provided={}, prompt_cache_key={}, store={}, stream={}, instructions_hash={}, tools_hash={}, input_hash={}, include_hash={}, body_hash={}",
        app_type.as_str(),
        provider.id,
        endpoint,
        api_format.unwrap_or("native"),
        session_client_provided,
        prompt_cache_key,
        store,
        stream,
        short_value_hash(body.get("instructions")),
        short_value_hash(body.get("tools")),
        short_value_hash(body.get("input")),
        short_value_hash(body.get("include")),
        short_value_hash(Some(body)),
    );
}

fn value_for_log(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Null => "null".to_string(),
        Value::Array(values) => format!("array(len={})", values.len()),
        Value::Object(values) => format!("object(len={})", values.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use axum::http::header::{HeaderValue, ACCEPT};
    use axum::http::HeaderMap;
    use bytes::Bytes;
    use http::StatusCode;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::Duration;

    fn test_provider_with_type(provider_type: Option<&str>) -> Provider {
        Provider {
            id: "provider-1".to_string(),
            name: "Provider 1".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: provider_type.map(|value| crate::provider::ProviderMeta {
                provider_type: Some(value.to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn test_forwarder(
        non_streaming_timeout: Duration,
        streaming_first_byte_timeout: Duration,
    ) -> RequestForwarder {
        let db = Arc::new(Database::memory().expect("memory db"));

        RequestForwarder {
            router: Arc::new(ProviderRouter::new(db.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            gemini_shadow: Arc::new(GeminiShadowStore::new()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
            app_handle: None,
            current_provider_id_at_start: String::new(),
            session_id: String::new(),
            session_client_provided: false,
            rectifier_config: RectifierConfig::default(),
            optimizer_config: OptimizerConfig::default(),
            copilot_optimizer_config: CopilotOptimizerConfig::default(),
            non_streaming_timeout,
            streaming_first_byte_timeout,
            max_attempts: 1,
        }
    }

    #[test]
    fn single_provider_retryable_log_uses_single_provider_code() {
        let error = ProxyError::UpstreamError {
            status: 429,
            body: Some(r#"{"error":{"message":"rate limit exceeded"}}"#.to_string()),
        };

        let (code, message) = build_retryable_failure_log("PackyCode-response", 1, 1, &error);

        assert_eq!(code, log_fwd::SINGLE_PROVIDER_FAILED);
        assert!(message.contains("Provider PackyCode-response 璇锋眰澶辫触"));
        assert!(message.contains("涓婃父 HTTP 429"));
        assert!(message.contains("rate limit exceeded"));
        assert!(!message.contains("鍒囨崲涓嬩竴涓?));
    }

    #[test]
    fn multi_provider_retryable_log_keeps_failover_wording() {
        let error = ProxyError::Timeout("upstream timed out after 30s".to_string());

        let (code, message) = build_retryable_failure_log("primary", 1, 3, &error);

        assert_eq!(code, log_fwd::PROVIDER_FAILED_RETRY);
        assert!(message.contains("缁х画灏濊瘯涓嬩竴涓?(1/3)"));
        assert!(message.contains("璇锋眰瓒呮椂"));
    }

    #[test]
    fn single_provider_has_no_terminal_all_failed_log() {
        assert!(build_terminal_failure_log(1, 1, None).is_none());
    }

    #[test]
    fn multi_provider_terminal_log_contains_last_error_summary() {
        let error = ProxyError::ForwardFailed("connection reset by peer".to_string());

        let (code, message) =
            build_terminal_failure_log(2, 2, Some(&error)).expect("expected terminal log");

        assert_eq!(code, log_fwd::ALL_PROVIDERS_FAILED);
        assert!(message.contains("宸插皾璇?2/2 涓?Provider锛屽潎澶辫触"));
        assert!(message.contains("connection reset by peer"));
    }

    #[test]
    fn summarize_upstream_body_prefers_json_message() {
        let body = json!({
            "error": {
                "message": "invalid_request_error: unsupported field"
            },
            "request_id": "req_123"
        });

        let summary = summarize_upstream_body(&body.to_string());

        assert_eq!(summary, "invalid_request_error: unsupported field");
    }

    #[test]
    fn summarize_text_for_log_collapses_whitespace_and_truncates() {
        let summary = summarize_text_for_log("line1\n\n line2   line3", 12);

        assert_eq!(summary, "line1 line2...");
    }

    #[test]
    fn canonical_json_sorts_object_keys_for_cache_trace_hashes() {
        let left = json!({
            "tools": [
                {
                    "parameters": {
                        "properties": {
                            "b": {"type": "string"},
                            "a": {"type": "number"}
                        },
                        "type": "object"
                    },
                    "name": "lookup"
                }
            ]
        });
        let right = json!({
            "tools": [
                {
                    "name": "lookup",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "a": {"type": "number"},
                            "b": {"type": "string"}
                        }
                    }
                }
            ]
        });

        assert_eq!(
            crate::proxy::json_canonical::canonical_json_string(&left),
            crate::proxy::json_canonical::canonical_json_string(&right)
        );
        assert_eq!(
            short_value_hash(Some(&left)),
            short_value_hash(Some(&right))
        );
    }

    #[test]
    fn prepare_upstream_request_body_filters_private_fields_and_canonicalizes_order() {
        let body = json!({
            "z": 1,
            "_internal": "drop",
            "tools": [
                {
                    "name": "lookup",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "_id": {
                                "_private_note": "drop",
                                "type": "string"
                            },
                            "b": {"type": "number"},
                            "a": {"type": "string"}
                        }
                    }
                }
            ],
            "a": 2
        });

        let prepared = prepare_upstream_request_body(body);

        assert!(prepared.get("_internal").is_none());
        assert!(prepared["tools"][0]["parameters"]["properties"]
            .get("_id")
            .is_some());
        assert!(prepared["tools"][0]["parameters"]["properties"]["_id"]
            .get("_private_note")
            .is_none());
        assert_eq!(
            serde_json::to_string(&prepared).unwrap(),
            r#"{"a":2,"tools":[{"name":"lookup","parameters":{"properties":{"_id":{"type":"string"},"a":{"type":"string"},"b":{"type":"number"}},"type":"object"}}],"z":1}"#
        );
    }

    #[tokio::test]
    async fn non_streaming_success_is_buffered_before_marking_provider_successful() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"{\"ok\":true}"))
            }),
        );

        let prepared = forwarder
            .prepare_success_response_for_failover(response, false)
            .await
            .expect("response should be buffered");

        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from_static(b"{\"ok\":true}")
        );
    }

    #[tokio::test]
    async fn non_streaming_body_read_error_is_retryable_before_success_record() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                Err::<Bytes, std::io::Error>(std::io::Error::other("body boom"))
            }),
        );

        let err = match forwarder
            .prepare_success_response_for_failover(response, false)
            .await
        {
            Ok(_) => panic!("body read errors should fail the attempt"),
            Err(err) => err,
        };

        assert!(matches!(err, ProxyError::ForwardFailed(_)));
    }

    #[tokio::test]
    async fn streaming_success_primes_first_chunk_and_replays_it() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::iter(vec![
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"first")),
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"second")),
            ]),
        );

        let prepared = forwarder
            .prepare_success_response_for_failover(response, true)
            .await
            .expect("stream should be primed");

        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from_static(b"firstsecond")
        );
    }

    #[tokio::test]
    async fn streaming_first_chunk_error_is_retryable_before_success_record() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                Err::<Bytes, std::io::Error>(std::io::Error::other("first chunk boom"))
            }),
        );

        let err = match forwarder
            .prepare_success_response_for_failover(response, true)
            .await
        {
            Ok(_) => panic!("first chunk errors should fail the attempt"),
            Err(err) => err,
        };

        assert!(matches!(err, ProxyError::ForwardFailed(_)));
    }

    #[test]
    fn codex_oauth_session_headers_match_codex_cache_identity() {
        let headers = build_codex_oauth_session_headers("session-123");
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(name, value);
        }

        assert_eq!(
            map.get("session_id"),
            Some(&HeaderValue::from_static("session-123"))
        );
        assert_eq!(
            map.get("x-client-request-id"),
            Some(&HeaderValue::from_static("session-123"))
        );
        assert_eq!(
            map.get("x-codex-window-id"),
            Some(&HeaderValue::from_static("session-123:0"))
        );
    }

    #[test]
    fn managed_account_upstream_rejects_proxy_managed_placeholder_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        let err = reject_proxy_placeholder_for_managed_account_upstream(
            "https://api.githubcopilot.com/chat/completions",
            &headers,
        )
        .expect_err("placeholder should be rejected before upstream");

        assert!(matches!(
            err,
            ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
        ));
    }

    #[test]
    fn codex_oauth_upstream_rejects_proxy_managed_placeholder_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        let err = reject_proxy_placeholder_for_managed_account_upstream(
            "https://chatgpt.com/backend-api/codex/responses",
            &headers,
        )
        .expect_err("placeholder should be rejected before upstream");

        assert!(matches!(
            err,
            ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
        ));
    }

    #[test]
    fn non_managed_upstream_allows_proxy_managed_placeholder_guard() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        reject_proxy_placeholder_for_managed_account_upstream(
            "https://api.example.com/v1/messages",
            &headers,
        )
        .expect("guard is scoped to managed-account upstreams");
    }

    #[test]
    fn exact_header_case_preserved_for_native_claude_only() {
        let provider = test_provider_with_type(None);

        assert!(should_preserve_exact_header_case(
            "Claude",
            &provider,
            Some("anthropic"),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Claude",
            &provider,
            Some("openai_responses"),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Codex", &provider, None, false
        ));
        assert!(!should_preserve_exact_header_case(
            "Gemini", &provider, None, false
        ));
    }

    #[test]
    fn exact_header_case_skipped_for_codex_oauth_and_copilot() {
        let codex_oauth = test_provider_with_type(Some("codex_oauth"));
        let copilot = test_provider_with_type(Some("github_copilot"));

        assert!(!should_preserve_exact_header_case(
            "Claude",
            &codex_oauth,
            Some("openai_responses"),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Claude",
            &copilot,
            Some("openai_chat"),
            true
        ));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_strips_beta_for_chat_completions() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&foo=bar",
            "openai_chat",
            false,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_strips_beta_for_responses() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/claude/v1/messages?beta=true&x-id=1",
            "openai_responses",
            false,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/responses?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_codex_responses_endpoint_to_chat_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_chat("/v1/responses?foo=bar");

        assert_eq!(endpoint, "/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_codex_responses_compact_endpoint_to_chat_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_chat("/v1/responses/compact?foo=bar");

        assert_eq!(endpoint, "/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_uses_copilot_path() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            "anthropic",
            true,
            &json!({ "model": "claude-sonnet-4-6" }),
        );

        assert_eq!(endpoint, "/chat/completions?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_uses_copilot_responses_path() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            "openai_responses",
            true,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/responses?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_maps_gemini_generate_content() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            "gemini_native",
            false,
            &json!({ "model": "gemini-2.5-pro" }),
        );

        assert_eq!(
            endpoint,
            "/v1beta/models/gemini-2.5-pro:generateContent?x-id=1"
        );
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    /// Regression: body.model arriving as the resource-name form
    /// `models/gemini-2.5-pro` must not produce a doubled
    /// `/v1beta/models/models/...` path.
    #[test]
    fn rewrite_claude_transform_endpoint_strips_gemini_model_resource_prefix() {
        let (endpoint, _) = rewrite_claude_transform_endpoint(
            "/v1/messages",
            "gemini_native",
            false,
            &json!({ "model": "models/gemini-2.5-pro" }),
        );

        assert_eq!(endpoint, "/v1beta/models/gemini-2.5-pro:generateContent");
    }

    #[test]
    fn rewrite_claude_transform_endpoint_maps_gemini_streaming() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true",
            "gemini_native",
            false,
            &json!({ "model": "gemini-2.5-flash", "stream": true }),
        );

        assert_eq!(
            endpoint,
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
        assert_eq!(passthrough_query.as_deref(), Some("alt=sse"));
    }

    #[test]
    fn append_query_to_full_url_preserves_existing_query_string() {
        let url = append_query_to_full_url("https://relay.example/api?foo=bar", Some("x-id=1"));

        assert_eq!(url, "https://relay.example/api?foo=bar&x-id=1");
    }

    #[test]
    fn build_gemini_native_url_uses_origin_when_base_ends_with_v1beta() {
        let url = crate::proxy::gemini_url::build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/models/gemini-2.5-pro:generateContent",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }

    #[test]
    fn build_gemini_native_url_uses_origin_when_base_already_contains_models_prefix() {
        let url = crate::proxy::gemini_url::build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta/models",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn resolve_gemini_native_url_keeps_opaque_full_url_as_is() {
        let url = crate::proxy::gemini_url::resolve_gemini_native_url(
            "https://relay.example/custom/generate-content",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/generate-content?alt=sse");
    }

    #[test]
    fn force_identity_for_stream_flag_requests() {
        let headers = HeaderMap::new();

        assert!(should_force_identity_encoding(
            "/v1/responses",
            &json!({ "stream": true }),
            &headers
        ));
    }

    #[test]
    fn force_identity_for_gemini_stream_endpoints() {
        let headers = HeaderMap::new();

        assert!(should_force_identity_encoding(
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
            &json!({ "model": "gemini-2.5-pro" }),
            &headers
        ));
    }

    #[test]
    fn streaming_request_detects_gemini_sse_without_body_stream_flag() {
        let headers = HeaderMap::new();

        assert!(is_streaming_request(
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
            &json!({ "model": "gemini-2.5-pro" }),
            &headers
        ));
    }

    #[test]
    fn force_identity_for_sse_accept_header() {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));

        assert!(should_force_identity_encoding(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }

    #[test]
    fn non_streaming_requests_allow_automatic_compression() {
        let headers = HeaderMap::new();

        assert!(!should_force_identity_encoding(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }

    // ==================== Copilot 鍔ㄦ€?endpoint 璺敱鐩稿叧娴嬭瘯 ====================

    /// 楠岃瘉 is_copilot 妫€娴嬮€昏緫锛氶€氳繃 provider_type 鍒ゆ柇
    #[test]
    fn copilot_detection_via_provider_type() {
        use crate::provider::{Provider, ProviderMeta};

        let provider = Provider {
            id: "test".to_string(),
            name: "Test Copilot".to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        let is_copilot = provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("github_copilot");

        assert!(is_copilot, "搴旇閫氳繃 provider_type 妫€娴嬩负 Copilot");
    }

    /// 楠岃瘉 is_copilot 妫€娴嬮€昏緫锛氶€氳繃 base_url 鍒ゆ柇
    #[test]
    fn copilot_detection_via_base_url() {
        let base_url = "https://api.githubcopilot.com";
        let is_copilot = base_url.contains("githubcopilot.com");
        assert!(is_copilot, "搴旇閫氳繃 base_url 妫€娴嬩负 Copilot");

        let non_copilot_url = "https://api.anthropic.com";
        let is_not_copilot = non_copilot_url.contains("githubcopilot.com");
        assert!(!is_not_copilot, "闈?Copilot URL 涓嶅簲琚娴嬩负 Copilot");
    }

    /// 楠岃瘉浼佷笟鐗?endpoint锛堜笉鍖呭惈 githubcopilot.com锛夊満鏅笅 is_copilot 浠嶇劧姝ｇ‘
    #[test]
    fn copilot_detection_for_enterprise_endpoint() {
        use crate::provider::{Provider, ProviderMeta};

        // 浼佷笟鐗堝満鏅細provider_type 鏄?github_copilot锛屼絾 base_url 鍙兘鏄紒涓氬唴閮ㄥ煙鍚?        let provider = Provider {
            id: "enterprise".to_string(),
            name: "Enterprise Copilot".to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        let enterprise_base_url = "https://copilot-api.corp.example.com";

        // is_copilot 搴旇閫氳繃 provider_type 妫€娴嬫垚鍔燂紝鍗充娇 base_url 涓嶅寘鍚?githubcopilot.com
        let is_copilot = provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("github_copilot")
            || enterprise_base_url.contains("githubcopilot.com");

        assert!(
            is_copilot,
            "浼佷笟鐗?Copilot 搴旇閫氳繃 provider_type 琚纭娴?
        );
    }

    /// 楠岃瘉鍔ㄦ€?endpoint 鏇挎崲鏉′欢
    #[test]
    fn dynamic_endpoint_replacement_conditions() {
        // 鏉′欢锛歩s_copilot && !is_full_url
        let test_cases = [
            (true, false, true, "Copilot + 闈?full_url 搴旇鏇挎崲"),
            (true, true, false, "Copilot + full_url 涓嶅簲鏇挎崲"),
            (false, false, false, "闈?Copilot 涓嶅簲鏇挎崲"),
            (false, true, false, "闈?Copilot + full_url 涓嶅簲鏇挎崲"),
        ];

        for (is_copilot, is_full_url, should_replace, desc) in test_cases {
            let will_replace = is_copilot && !is_full_url;
            assert_eq!(will_replace, should_replace, "{desc}");
        }
    }
}
