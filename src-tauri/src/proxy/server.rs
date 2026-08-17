//! HTTP代理服务器
//!
//! 基于Axum的HTTP服务器，处理代理请求
//!
//! Uses a manual hyper HTTP/1.1 accept loop with `preserve_header_case(true)` so
//! that the original header-name casing from the CLI client is captured in a
//! `HeaderCaseMap` extension.  This map is later forwarded to the upstream via
//! the hyper-based HTTP client, producing wire-level header casing identical to
//! a direct (non-proxied) CLI request.

use super::{
    failover_switch::FailoverSwitchManager,
    handlers,
    log_codes::srv as log_srv,
    provider_router::ProviderRouter,
    providers::{codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore},
    types::*,
    ProxyError,
};
use crate::app_config::AppType;
use crate::database::Database;
use axum::{
    extract::{DefaultBodyLimit, State},
    routing::{any, get, post},
    Router,
};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;

/// 代理服务器状态（共享）
#[derive(Clone)]
pub struct ProxyState {
    pub db: Arc<Database>,
    pub config: Arc<RwLock<ProxyConfig>>,
    pub status: Arc<RwLock<ProxyStatus>>,
    pub start_time: Arc<RwLock<Option<std::time::Instant>>>,
    /// 每个应用类型当前使用的 provider (app_type -> (provider_id, provider_name))
    pub current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    /// 共享的 ProviderRouter（持有熔断器状态，跨请求保持）
    pub provider_router: Arc<ProviderRouter>,
    /// Gemini Native shadow state，用于 thoughtSignature / tool call 回放
    pub gemini_shadow: Arc<GeminiShadowStore>,
    /// Codex Chat bridge history，用于恢复 previous_response_id 指向的 tool call
    pub codex_chat_history: Arc<CodexChatHistoryStore>,
    /// AppHandle，用于发射事件和更新托盘菜单
    pub app_handle: Option<tauri::AppHandle>,
    /// 故障转移切换管理器
    pub failover_manager: Arc<FailoverSwitchManager>,
}

/// 代理HTTP服务器
pub struct ProxyServer {
    config: ProxyConfig,
    state: ProxyState,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    /// 服务器任务句柄，用于等待服务器实际关闭
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl ProxyServer {
    pub fn new(
        config: ProxyConfig,
        db: Arc<Database>,
        app_handle: Option<tauri::AppHandle>,
    ) -> Self {
        // 创建共享的 ProviderRouter（熔断器状态将跨所有请求保持）
        let provider_router = Arc::new(ProviderRouter::new(db.clone()));
        // 创建故障转移切换管理器
        let failover_manager = Arc::new(FailoverSwitchManager::new(db.clone()));

        let state = ProxyState {
            db,
            config: Arc::new(RwLock::new(config.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            provider_router,
            gemini_shadow: Arc::new(GeminiShadowStore::default()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            app_handle,
            failover_manager,
        };

        Self {
            config,
            state,
            shutdown_tx: Arc::new(RwLock::new(None)),
            server_handle: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn start(&self) -> Result<ProxyServerInfo, ProxyError> {
        // 检查是否已在运行
        if self.shutdown_tx.read().await.is_some() {
            return Err(ProxyError::AlreadyRunning);
        }

        let addr: SocketAddr =
            format!("{}:{}", self.config.listen_address, self.config.listen_port)
                .parse()
                .map_err(|e| ProxyError::BindFailed(format!("无效的地址: {e}")))?;

        // 创建关闭通道
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // 构建路由
        let app = self.build_router();

        // 绑定监听器
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| ProxyError::BindFailed(e.to_string()))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| ProxyError::BindFailed(e.to_string()))?;
        let actual_port = local_addr.port();

        log::info!("[{}] 代理服务器启动于 {local_addr}", log_srv::STARTED);

        // 更新全局代理端口，用于系统代理检测
        crate::proxy::http_client::set_proxy_port(actual_port);

        // 保存关闭句柄
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        // 更新状态
        let mut status = self.state.status.write().await;
        status.running = true;
        status.address = self.config.listen_address.clone();
        status.port = actual_port;
        drop(status);

        // 记录启动时间
        *self.state.start_time.write().await = Some(std::time::Instant::now());

        // 启动服务器 — 使用手动 hyper HTTP/1.1 accept loop
        // 开启 preserve_header_case 以捕获客户端请求头的原始大小写
        let state = self.state.clone();
        let handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let (stream, remote_addr) = match result {
                            Ok(v) => v,
                            Err(e) => {
                                log::error!("[{SRV}] accept 失败: {e}", SRV = log_srv::ACCEPT_ERR);
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                continue;
                            }
                        };

                        let app = app.clone();
                        tokio::spawn(async move {
                            // Peek raw TCP bytes to capture original header casing
                            // before hyper parses (and lowercases) the header names.
                            let original_cases = {
                                let mut peek_buf = vec![0u8; 8192];
                                match stream.peek(&mut peek_buf).await {
                                    Ok(n) => {
                                        let cases = super::hyper_client::OriginalHeaderCases::from_raw_bytes(&peek_buf[..n]);
                                        log::debug!(
                                            "[ProxyServer] Peeked {} bytes, captured {} header casings",
                                            n, cases.cases.len()
                                        );
                                        cases
                                    }
                                    Err(e) => {
                                        log::debug!("[ProxyServer] peek failed (non-fatal): {e}");
                                        super::hyper_client::OriginalHeaderCases::default()
                                    }
                                }
                            };

                            // service_fn 将 axum Router（tower::Service）桥接到 hyper
                            let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                let mut router = app.clone();
                                let cases = original_cases.clone();
                                let remote_addr = remote_addr;
                                async move {
                                    // 将 hyper::body::Incoming 转为 axum::body::Body，保留 extensions
                                    let (mut parts, body) = req.into_parts();

                                    // Insert our own header case map alongside hyper's internal one
                                    parts.extensions.insert(cases);

                                    // 注入真实来源地址：公网路由鉴权中间件按来源 IP 判定本地/外部，
                                    // 绝不信任客户端可控的 Host 头（防 LAN 攻击者伪造 Host 绕过鉴权）。
                                    parts
                                        .extensions
                                        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                                            remote_addr,
                                        ));

                                    let body = axum::body::Body::new(body);
                                    let axum_req = http::Request::from_parts(parts, body);
                                    <Router as tower::Service<http::Request<axum::body::Body>>>::call(&mut router, axum_req).await
                                }
                            });

                            if let Err(e) = hyper::server::conn::http1::Builder::new()
                                .preserve_header_case(true)
                                .serve_connection(TokioIo::new(stream), service)
                                .await
                            {
                                // Connection reset / broken pipe 等在代理场景下很常见，debug 级别
                                log::debug!("[{SRV}] connection error: {e}", SRV = log_srv::CONN_ERR);
                            }
                        });
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }

            // 服务器停止后更新状态
            state.status.write().await.running = false;
            *state.start_time.write().await = None;
        });

        // 保存服务器任务句柄
        *self.server_handle.write().await = Some(handle);

        Ok(ProxyServerInfo {
            address: self.config.listen_address.clone(),
            port: actual_port,
            started_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    pub async fn stop(&self) -> Result<(), ProxyError> {
        // 1. 发送关闭信号
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        } else {
            return Err(ProxyError::NotRunning);
        }

        // 2. 等待服务器任务结束（带 5 秒超时保护）
        if let Some(handle) = self.server_handle.write().await.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                Ok(Ok(())) => {
                    log::info!("[{}] 代理服务器已完全停止", log_srv::STOPPED);
                    Ok(())
                }
                Ok(Err(e)) => {
                    log::warn!("[{}] 代理服务器任务异常终止: {e}", log_srv::TASK_ERROR);
                    Err(ProxyError::StopFailed(e.to_string()))
                }
                Err(_) => {
                    log::warn!(
                        "[{}] 代理服务器停止超时（5秒），强制继续",
                        log_srv::STOP_TIMEOUT
                    );
                    Err(ProxyError::StopTimeout)
                }
            }
        } else {
            Ok(())
        }
    }

    pub async fn get_status(&self) -> ProxyStatus {
        let mut status = self.state.status.read().await.clone();

        // 计算运行时间
        if let Some(start) = *self.state.start_time.read().await {
            status.uptime_seconds = start.elapsed().as_secs();
        }

        // 从 current_providers HashMap 获取每个应用类型当前正在使用的 provider
        let current_providers = self.state.current_providers.read().await;
        status.active_targets = current_providers
            .iter()
            .map(|(app_type, (provider_id, provider_name))| ActiveTarget {
                app_type: app_type.clone(),
                provider_id: provider_id.clone(),
                provider_name: provider_name.clone(),
            })
            .collect();

        status
    }

    /// 更新某个应用类型当前“目标供应商”（用于 UI 展示 active_targets）
    ///
    /// 注意：这不代表该供应商一定已经处理过请求，而是用于“热切换/启用故障转移立即切 P1”
    /// 等场景下，让 UI 能立刻反映最新目标。
    pub async fn set_active_target(&self, app_type: &str, provider_id: &str, provider_name: &str) {
        let mut current_providers = self.state.current_providers.write().await;
        current_providers.insert(
            app_type.to_string(),
            (provider_id.to_string(), provider_name.to_string()),
        );
    }

    fn build_router(&self) -> Router {
        Router::new()
            // 健康检查
            .route("/health", get(handlers::health_check))
            .route("/status", get(handlers::get_status))
            // Claude API (支持带前缀和不带前缀两种格式)
            .route("/v1/messages", post(handlers::handle_messages))
            .route("/claude/v1/messages", post(handlers::handle_messages))
            // Claude Desktop 3P 本地 gateway（独立 provider namespace）
            .route(
                "/claude-desktop/v1/models",
                get(handlers::handle_claude_desktop_models),
            )
            .route(
                "/claude-desktop/v1/messages",
                post(handlers::handle_claude_desktop_messages),
            )
            // OpenAI Chat Completions API (Codex CLI，支持带前缀和不带前缀)
            .route("/chat/completions", post(handlers::handle_chat_completions))
            .route(
                "/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            .route(
                "/v1/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            .route(
                "/codex/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            // OpenAI Models API (Codex CLI reachability check)
            .route("/models", get(handlers::handle_models))
            .route("/v1/models", get(handlers::handle_models))
            // OpenAI Responses API (Codex CLI，支持带前缀和不带前缀)
            .route("/responses", post(handlers::handle_responses))
            .route("/v1/responses", post(handlers::handle_responses))
            .route("/v1/v1/responses", post(handlers::handle_responses))
            .route("/codex/v1/responses", post(handlers::handle_responses))
            // Grok Build uses the Responses protocol but has an independent
            // provider namespace and failover queue.
            .route(
                "/grokbuild/v1/responses",
                post(handlers::handle_grokbuild_responses),
            )
            // OpenAI Responses Compact API (Codex CLI 远程压缩，透传)
            .route(
                "/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/v1/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/v1/v1/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/codex/v1/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/grokbuild/v1/responses/compact",
                post(handlers::handle_grokbuild_responses_compact),
            )
            // 公网路由（Cursor 经公网隧道接入）命名空间：经 cloudflared 公网隧道进入的 Cursor 请求
            // 固定路由到 Cursor 的当前供应商（与 Codex/GrokBuild 命名空间隔离）。
            // Cursor 侧 Override OpenAI Base URL 填 {public_url}/cursor/v1。
            .route(
                "/cursor/v1/chat/completions",
                post(handlers::handle_cursor_chat_completions),
            )
            .route(
                "/cursor/v1/responses",
                post(handlers::handle_cursor_responses),
            )
            .route("/cursor/v1/models", get(handlers::handle_cursor_models))
            // Codex standalone Alpha Search API. All local aliases normalize to
            // the selected provider's canonical sibling `/alpha/search` route.
            .route("/alpha/search", post(handlers::handle_alpha_search))
            .route("/v1/alpha/search", post(handlers::handle_alpha_search))
            .route("/v1/v1/alpha/search", post(handlers::handle_alpha_search))
            .route(
                "/codex/v1/alpha/search",
                post(handlers::handle_alpha_search),
            )
            // Gemini API (支持带前缀和不带前缀)
            //
            // 用 `any(..)` 覆盖所有 HTTP 方法：除了 POST `:generateContent` /
            // `:streamGenerateContent` / `:countTokens` 之外，Gemini SDK / CLI 还会发
            // GET `/models`、GET `/models/<id>` 等只读端点。如果只挂 POST，这些 GET
            // 请求会在路由层 404，绕过本地代理的统计、整流和故障转移。
            .route("/v1beta/*path", any(handlers::handle_gemini))
            .route("/gemini/v1beta/*path", any(handlers::handle_gemini))
            // Gemini 的 GA 版本也叫 /v1，给原 SDK 留一条出口
            .route("/gemini/v1/*path", any(handlers::handle_gemini))
            // Codex / Cursor 透传：未匹配的其他 /v1/* 路径（如用量查询、账户检查等）
            // 直接代理到上游 API，避免 404 导致客户端误报额度/授权错误。
            .route("/v1/*path", any(handlers::handle_codex_passthrough))
            // 提高默认请求体大小限制（避免 413 Payload Too Large）
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
            // 公网路由鉴权：axum 后注册的 layer 在外层（先执行），故把鉴权放在
            // DefaultBodyLimit 之后注册 = 最外层 = 最先执行，未授权请求在读 body 前就被拒绝。
            // 来源判定——本地（回环来源 IP）放行，外部（经隧道/局域网）必须携带 ccsk Bearer key，
            // 且目标应用必须已开启本地路由（接管）。
            .layer(axum::middleware::from_fn_with_state(
                self.state.clone(),
                public_route_auth_middleware,
            ))
            .with_state(self.state.clone())
    }

    /// 在不重启服务的情况下更新运行时配置
    pub async fn apply_runtime_config(&self, config: &ProxyConfig) {
        *self.state.config.write().await = config.clone();
    }

    /// 热更新熔断器配置
    ///
    /// 将新配置应用到所有已创建的熔断器实例
    pub async fn update_circuit_breaker_configs(
        &self,
        config: super::circuit_breaker::CircuitBreakerConfig,
    ) {
        self.state.provider_router.update_all_configs(config).await;
    }

    pub async fn update_circuit_breaker_config_for_app(
        &self,
        app_type: &str,
        config: super::circuit_breaker::CircuitBreakerConfig,
    ) {
        self.state
            .provider_router
            .update_app_configs(app_type, config)
            .await;
    }

    /// 重置指定 Provider 的熔断器
    pub async fn reset_provider_circuit_breaker(&self, provider_id: &str, app_type: &str) {
        self.state
            .provider_router
            .reset_provider_breaker(provider_id, app_type)
            .await;
    }
}

/// 判断请求是否来自本地可信来源。
///
/// 本地来源 = 真实来源 IP 为回环地址（server 层注入 `ConnectInfo`）且未经 Cloudflare 隧道。
/// - `cf-connecting-ip` 存在 → 必为经 cloudflared 隧道的请求（Cloudflare 边缘覆写该头，
///   客户端无法伪造），一律按外部鉴权；
/// - 否则以来源 IP 判定：仅回环（本机 app 直连）放行；LAN/远程来源或缺失来源信息一律外部。
///
/// **绝不信任客户端可控的 Host 头**：绑 0.0.0.0（局域网可达）时，攻击者伪造
/// `Host: 127.0.0.1` 无法绕过——来源 IP 仍是 LAN 地址（#review HIGH Host 头欺骗）。
pub(crate) fn request_is_local_origin(request: &axum::extract::Request) -> bool {
    if request.headers().contains_key("cf-connecting-ip") {
        return false;
    }
    match request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
    {
        Some(info) => info.0.ip().is_loopback(),
        None => false,
    }
}

/// 公网路由鉴权决策：本地来源放行；外部来源（经隧道/局域网直连）必须携带
/// 公网路由的 ccsk Bearer key，且目标应用必须已开启本地路由（接管）。
/// 纯函数便于单测，由中间件注入运行时 settings / db。
pub(crate) fn public_route_auth_decision(
    is_local_origin: bool,
    enabled: bool,
    api_key: Option<&str>,
    app_enabled: bool,
    headers: &axum::http::HeaderMap,
) -> Result<(), ProxyError> {
    if is_local_origin {
        return Ok(());
    }
    handlers::validate_public_route_auth(enabled, api_key, headers)?;
    // 限制：仅开启本地路由（接管）的应用才能经公网路由访问（用户需求 2026-08-05）。
    if !app_enabled {
        return Err(ProxyError::AuthError(
            "应用未开启本地路由，无法经公网路由访问".to_string(),
        ));
    }
    Ok(())
}

/// 按公网请求路径推断目标应用（与 build_router 的命名空间一致）。
/// 公共服务（/health /status）返回 None，不按应用门控，但外部请求仍须通过密钥校验。
fn app_for_public_path(path: &str) -> Option<AppType> {
    let p = path.trim_end_matches('/');
    if p == "/health" || p == "/status" {
        return None;
    }
    // 精确路由优先（/v1/messages 是 Claude 命名空间，/v1/* 是 Codex 透传）
    if p == "/v1/messages" || p == "/claude/v1/messages" {
        return Some(AppType::Claude);
    }
    if p == "/cursor/v1/models" || p.starts_with("/cursor/v1/") {
        return Some(AppType::Cursor);
    }
    if p.starts_with("/claude-desktop/") {
        return Some(AppType::ClaudeDesktop);
    }
    if p.starts_with("/claude/") {
        return Some(AppType::Claude);
    }
    if p.starts_with("/grokbuild/v1/") {
        return Some(AppType::GrokBuild);
    }
    if p.starts_with("/gemini/") || p.starts_with("/v1beta/") {
        return Some(AppType::Gemini);
    }
    if p.starts_with("/codex/v1/") {
        return Some(AppType::Codex);
    }
    // OpenAI 直通（Codex）命名空间
    if p == "/chat/completions"
        || p == "/models"
        || p == "/v1/models"
        || p == "/responses"
        || p.starts_with("/responses/compact")
        || p.starts_with("/v1/chat/completions")
        || p.starts_with("/v1/responses")
        || p == "/v1"
        || p.starts_with("/v1/")
    {
        return Some(AppType::Codex);
    }
    None
}

/// 全局公网路由鉴权中间件：在进入任意路由前先做来源判定。
/// 未启用公网路由时，外部请求一律拒绝（隧道不应运行）。
async fn public_route_auth_middleware(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, ProxyError> {
    let settings = crate::settings::get_settings();
    // 外部请求按路径推断目标应用，检查其本地路由（接管）是否开启
    let app_enabled = match app_for_public_path(request.uri().path()) {
        Some(app) => state
            .db
            .get_proxy_config_for_app(app.as_str())
            .await
            .map(|c| c.enabled)
            .unwrap_or(false),
        None => true, // 公共服务或未映射路径：仅走密钥校验
    };
    public_route_auth_decision(
        request_is_local_origin(&request),
        settings.public_route_enabled,
        settings.public_route_api_key.as_deref(),
        app_enabled,
        request.headers(),
    )?;
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};
    use axum::body::Body;
    use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
    use serde_json::{json, Value};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn request_with(source_ip: Option<IpAddr>, cf_connecting_ip: Option<&str>) -> Request<Body> {
        let mut b = Request::builder();
        if let Some(ip) = cf_connecting_ip {
            b = b.header("cf-connecting-ip", ip);
        }
        let mut req = b.body(Body::empty()).unwrap();
        if let Some(ip) = source_ip {
            req.extensions_mut()
                .insert(axum::extract::ConnectInfo::<SocketAddr>(SocketAddr::new(
                    ip, 15721,
                )));
        }
        req
    }

    #[test]
    fn loopback_source_is_trusted() {
        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ] {
            assert!(
                request_is_local_origin(&request_with(Some(ip), None)),
                "{ip} should be local"
            );
        }
    }

    #[test]
    fn non_loopback_source_is_external() {
        // 绑 0.0.0.0 时，LAN 攻击者从非回环来源连入 → 即使伪造 Host 头也是外部，必须鉴权
        for ip in [
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        ] {
            assert!(
                !request_is_local_origin(&request_with(Some(ip), None)),
                "{ip} should be external"
            );
        }
    }

    #[test]
    fn cf_connecting_ip_forces_external_even_with_loopback_source() {
        // 隧道请求：来源 IP 是回环（cloudflared 本地转发），但带 cf-connecting-ip → 外部鉴权
        assert!(!request_is_local_origin(&request_with(
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            Some("203.0.113.5")
        )));
    }

    #[test]
    fn missing_source_info_is_external() {
        // 无法确认来源 → 按外部处理，要求鉴权（fail-safe）
        assert!(!request_is_local_origin(&request_with(None, None)));
    }

    #[test]
    fn host_header_spoofing_cannot_bypass() {
        // 关键回归：攻击者伪造 Host: 127.0.0.1，但来源 IP 是 LAN 地址 → 必须按外部鉴权
        let mut req = Request::builder()
            .header(header::HOST, "127.0.0.1:15721")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo::<SocketAddr>(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
                9999,
            )));
        assert!(
            !request_is_local_origin(&req),
            "LAN attacker forging Host: 127.0.0.1 must still be treated as external"
        );
    }

    #[test]
    fn local_origin_skips_auth() {
        let headers = HeaderMap::new();
        assert!(public_route_auth_decision(true, true, Some("ccsk-x"), false, &headers).is_ok());
        assert!(public_route_auth_decision(true, false, None, false, &headers).is_ok());
    }

    #[test]
    fn external_origin_requires_matching_key() {
        let mut ok = HeaderMap::new();
        ok.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str("Bearer ccsk-x").unwrap(),
        );
        assert!(public_route_auth_decision(false, true, Some("ccsk-x"), true, &ok).is_ok());

        let empty = HeaderMap::new();
        assert!(public_route_auth_decision(false, true, Some("ccsk-x"), true, &empty).is_err());

        let mut wrong = HeaderMap::new();
        wrong.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str("Bearer ccsk-y").unwrap(),
        );
        assert!(public_route_auth_decision(false, true, Some("ccsk-x"), true, &wrong).is_err());
    }

    #[test]
    fn external_origin_rejected_when_public_route_disabled() {
        let mut ok = HeaderMap::new();
        ok.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str("Bearer ccsk-x").unwrap(),
        );
        assert!(public_route_auth_decision(false, false, Some("ccsk-x"), true, &ok).is_err());
    }

    #[test]
    fn external_origin_rejected_when_app_routing_disabled() {
        // 限制：外部请求目标应用未开启本地路由 → 即使密钥正确也拒绝（用户需求）
        let mut ok = HeaderMap::new();
        ok.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str("Bearer ccsk-x").unwrap(),
        );
        assert!(
            public_route_auth_decision(false, true, Some("ccsk-x"), false, &ok).is_err(),
            "external request to an app without local routing must be rejected"
        );
    }

    #[test]
    fn local_origin_bypasses_app_routing_gate() {
        // 本地直连不受"应用需开启本地路由"限制（本地可信）
        let headers = HeaderMap::new();
        assert!(public_route_auth_decision(true, true, Some("ccsk-x"), false, &headers).is_ok());
    }

    #[test]
    fn app_for_public_path_maps_namespaces() {
        assert_eq!(
            app_for_public_path("/cursor/v1/chat/completions"),
            Some(AppType::Cursor)
        );
        assert_eq!(
            app_for_public_path("/cursor/v1/models"),
            Some(AppType::Cursor)
        );
        assert_eq!(
            app_for_public_path("/claude/v1/messages"),
            Some(AppType::Claude)
        );
        assert_eq!(app_for_public_path("/v1/messages"), Some(AppType::Claude));
        assert_eq!(
            app_for_public_path("/claude-desktop/v1/messages"),
            Some(AppType::ClaudeDesktop)
        );
        assert_eq!(
            app_for_public_path("/codex/v1/responses"),
            Some(AppType::Codex)
        );
        assert_eq!(
            app_for_public_path("/grokbuild/v1/responses"),
            Some(AppType::GrokBuild)
        );
        assert_eq!(
            app_for_public_path("/gemini/v1beta/models"),
            Some(AppType::Gemini)
        );
        assert_eq!(app_for_public_path("/v1beta/models"), Some(AppType::Gemini));
        assert_eq!(
            app_for_public_path("/v1/chat/completions"),
            Some(AppType::Codex)
        );
        assert_eq!(
            app_for_public_path("/chat/completions"),
            Some(AppType::Codex)
        );
        assert_eq!(app_for_public_path("/models"), Some(AppType::Codex));
        // 公共服务不按应用门控
        assert_eq!(app_for_public_path("/health"), None);
        assert_eq!(app_for_public_path("/status"), None);
    }

    #[derive(Debug)]
    struct CapturedRequest {
        path_and_query: String,
        authorization: Option<String>,
        body: Value,
    }

    #[tokio::test]
    async fn alpha_search_routes_forward_to_canonical_upstream() {
        let captured = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
        let mock_app = Router::new().route(
            "/v1/alpha/search",
            post({
                let captured = captured.clone();
                move |request: axum::extract::Request| {
                    let captured = captured.clone();
                    async move {
                        let (parts, body) = request.into_parts();
                        let body = axum::body::to_bytes(body, 1024 * 1024)
                            .await
                            .expect("read mock request body");
                        captured.lock().await.push(CapturedRequest {
                            path_and_query: parts
                                .uri
                                .path_and_query()
                                .map(|value| value.as_str().to_string())
                                .unwrap_or_else(|| parts.uri.path().to_string()),
                            authorization: parts
                                .headers
                                .get(header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .map(ToString::to_string),
                            body: serde_json::from_slice(&body).expect("parse mock request body"),
                        });

                        let mut headers = HeaderMap::new();
                        headers.insert(
                            header::CONTENT_TYPE,
                            "application/json".parse().expect("content type"),
                        );
                        headers.insert(
                            "x-upstream-request-id",
                            "search-1".parse().expect("request id"),
                        );
                        (
                            StatusCode::ACCEPTED,
                            headers,
                            r#"{"encrypted_output":"ciphertext"}"#,
                        )
                    }
                }
            }),
        );
        let mock_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind mock upstream");
        let mock_addr = mock_listener.local_addr().expect("mock upstream address");
        let mock_handle = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("serve mock upstream");
        });

        let db = Arc::new(Database::memory().expect("memory database"));
        let provider = Provider::with_id(
            "alpha-search-upstream".to_string(),
            "Alpha Search Upstream".to_string(),
            json!({
                "base_url": format!("http://{mock_addr}/v1"),
                "auth": {"OPENAI_API_KEY": "upstream-secret"}
            }),
            None,
        );
        db.save_provider("codex", &provider)
            .expect("save test provider");
        db.set_current_provider("codex", &provider.id)
            .expect("select test provider");

        let proxy = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                enable_logging: false,
                non_streaming_timeout: 10,
                ..ProxyConfig::default()
            },
            db.clone(),
            None,
        );
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();
        let aliases = [
            "/alpha/search",
            "/v1/alpha/search",
            "/v1/v1/alpha/search",
            "/codex/v1/alpha/search",
        ];

        for (index, path) in aliases.iter().enumerate() {
            let response = client
                .post(format!(
                    "http://127.0.0.1:{}{}?client_version=0.144.6",
                    proxy_info.port, path
                ))
                .header(header::AUTHORIZATION, "Bearer client-secret")
                .json(&json!({
                    "id": format!("search-{index}"),
                    "model": "gpt-5.6-sol",
                    "commands": {"search_query": [{"q": "test"}]}
                }))
                .send()
                .await
                .expect("send alpha search request");

            assert_eq!(response.status(), StatusCode::ACCEPTED, "alias {path}");
            assert_eq!(
                response
                    .headers()
                    .get("x-upstream-request-id")
                    .and_then(|value| value.to_str().ok()),
                Some("search-1"),
                "alias {path}"
            );
            assert_eq!(
                response.text().await.expect("read proxy response"),
                r#"{"encrypted_output":"ciphertext"}"#,
                "alias {path}"
            );
        }

        // Full-URL providers were the known flaw in the original PR: without a
        // sibling-endpoint rewrite, this request would be posted back to
        // `/v1/responses` instead of `/v1/alpha/search`.
        let mut full_url_provider = Provider::with_id(
            "alpha-search-full-url".to_string(),
            "Alpha Search Full URL".to_string(),
            json!({
                "base_url": format!("http://{mock_addr}/v1/responses?api-version=test"),
                "auth": {"OPENAI_API_KEY": "full-url-secret"}
            }),
            None,
        );
        full_url_provider.meta = Some(ProviderMeta {
            is_full_url: Some(true),
            ..ProviderMeta::default()
        });
        db.save_provider("codex", &full_url_provider)
            .expect("save full URL provider");
        db.set_current_provider("codex", &full_url_provider.id)
            .expect("select full URL provider");

        let response = client
            .post(format!(
                "http://127.0.0.1:{}/v1/alpha/search?client_version=0.144.6",
                proxy_info.port
            ))
            .header(header::AUTHORIZATION, "Bearer client-secret")
            .json(&json!({
                "id": "search-full-url",
                "model": "gpt-5.6-sol",
                "commands": {"search_query": [{"q": "full URL"}]}
            }))
            .send()
            .await
            .expect("send full URL alpha search request");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            response.text().await.expect("read full URL response"),
            r#"{"encrypted_output":"ciphertext"}"#
        );

        proxy.stop().await.expect("stop test proxy");
        mock_handle.abort();

        let captured = captured.lock().await;
        assert_eq!(captured.len(), aliases.len() + 1);
        for (index, request) in captured.iter().take(aliases.len()).enumerate() {
            assert_eq!(
                request.path_and_query,
                "/v1/alpha/search?client_version=0.144.6"
            );
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer upstream-secret")
            );
            assert_eq!(request.body["id"], format!("search-{index}"));
            assert_eq!(request.body["model"], "gpt-5.6-sol");
            assert_eq!(request.body["commands"]["search_query"][0]["q"], "test");
        }

        let full_url_request = captured.last().expect("full URL request captured");
        assert_eq!(
            full_url_request.path_and_query,
            "/v1/alpha/search?api-version=test&client_version=0.144.6"
        );
        assert_eq!(
            full_url_request.authorization.as_deref(),
            Some("Bearer full-url-secret")
        );
        assert_eq!(full_url_request.body["id"], "search-full-url");
        assert_eq!(
            full_url_request.body["commands"]["search_query"][0]["q"],
            "full URL"
        );
    }
}
