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
    plugins::PluginRegistry,
    provider_router::ProviderRouter,
    providers::{codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore},
    types::*,
    ProxyError,
};
use crate::database::Database;
use axum::{
    extract::DefaultBodyLimit,
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
    /// 插件注册表（由服务层构造并共享，热重载/重启代理时复用同一实例）
    pub plugins: Arc<PluginRegistry>,
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
        plugins: Arc<PluginRegistry>,
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
            plugins,
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
                        let (stream, _remote_addr) = match result {
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
                                async move {
                                    // 将 hyper::body::Incoming 转为 axum::body::Body，保留 extensions
                                    let (mut parts, body) = req.into_parts();

                                    // Insert our own header case map alongside hyper's internal one
                                    parts.extensions.insert(cases);

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
            // Codex standalone Alpha Search API. All local aliases normalize to
            // the selected provider's canonical sibling `/alpha/search` route.
            .route("/alpha/search", post(handlers::handle_alpha_search))
            .route("/v1/alpha/search", post(handlers::handle_alpha_search))
            .route("/v1/v1/alpha/search", post(handlers::handle_alpha_search))
            .route(
                "/codex/v1/alpha/search",
                post(handlers::handle_alpha_search),
            )
            // Codex built-in ImageGen still calls the legacy OpenAI Images API.
            .route(
                "/images/generations",
                post(handlers::handle_images_generations),
            )
            .route(
                "/v1/images/generations",
                post(handlers::handle_images_generations),
            )
            .route(
                "/v1/v1/images/generations",
                post(handlers::handle_images_generations),
            )
            .route(
                "/codex/v1/images/generations",
                post(handlers::handle_images_generations),
            )
            // Codex ImageGen posts to `/images/edits` when it references existing images.
            .route("/images/edits", post(handlers::handle_images_edits))
            .route("/v1/images/edits", post(handlers::handle_images_edits))
            .route("/v1/v1/images/edits", post(handlers::handle_images_edits))
            .route(
                "/codex/v1/images/edits",
                post(handlers::handle_images_edits),
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
            // 提高默认请求体大小限制（避免 413 Payload Too Large）
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};
    use crate::AppError;
    use axum::http::{header, HeaderMap, StatusCode};
    use serde_json::{json, Value};
    use tokio::sync::Mutex;

    #[derive(Debug)]
    struct CapturedRequest {
        path_and_query: String,
        authorization: Option<String>,
        body: Value,
    }

    /// A base URL pasted as a complete endpoint with the full-URL switch left off
    /// must derive the sibling standalone endpoint instead of having the
    /// standalone path appended to it.
    #[tokio::test]
    async fn codex_standalone_endpoints_derive_from_pasted_full_base_url() {
        let captured = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
        let capture_handler = {
            let captured = captured.clone();
            move |request: axum::extract::Request| {
                let captured = captured.clone();
                async move {
                    let (parts, _body) = request.into_parts();
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
                        body: Value::Null,
                    });

                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        r#"{"created":1,"data":[{"b64_json":"aW1hZ2U="}],"usage":{"input_tokens":7,"output_tokens":11,"total_tokens":18}}"#,
                    )
                }
            }
        };
        let mock_app = Router::new()
            .route("/v1/images/generations", post(capture_handler.clone()))
            .route("/v1/images/edits", post(capture_handler.clone()))
            .route("/Gateway/v1/images/edits", post(capture_handler.clone()))
            .route("/v1/alpha/search", post(capture_handler));
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
        let proxy = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                non_streaming_timeout: 10,
                ..ProxyConfig::default()
            },
            db.clone(),
            None,
            // 测试用空注册表：不加载真实插件目录，保证回归行为与主线一致
            Arc::new(PluginRegistry::new()),
        );
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();

        let cases = [
            (
                "pasted-mixed-case-chat-completions",
                format!("http://{mock_addr}/v1/Chat/Completions?api-version=CaseValue"),
                "/v1/images/edits",
                "/v1/images/edits?api-version=CaseValue&client_version=0.145.0",
            ),
            (
                "pasted-mixed-case-images-generations",
                format!("http://{mock_addr}/Gateway/v1/Images/Generations/?api-version=CaseValue#fragment"),
                "/v1/images/edits",
                "/Gateway/v1/images/edits?api-version=CaseValue&client_version=0.145.0",
            ),
            (
                "pasted-mixed-case-images-edits",
                format!("http://{mock_addr}/v1/Images/Edits/"),
                "/v1/images/generations",
                "/v1/images/generations?client_version=0.145.0",
            ),
            (
                "pasted-mixed-case-responses-compact",
                format!("http://{mock_addr}/v1/Responses/Compact/"),
                "/v1/alpha/search",
                "/v1/alpha/search?client_version=0.145.0",
            ),
            (
                "pasted-chat-completions",
                format!("http://{mock_addr}/v1/chat/completions"),
                "/v1/images/generations",
                "/v1/images/generations?client_version=0.145.0",
            ),
            (
                "pasted-chat-completions",
                format!("http://{mock_addr}/v1/chat/completions"),
                "/v1/images/edits",
                "/v1/images/edits?client_version=0.145.0",
            ),
            (
                "pasted-images-generations",
                format!("http://{mock_addr}/v1/images/generations?api-version=test"),
                "/v1/images/edits",
                "/v1/images/edits?api-version=test&client_version=0.145.0",
            ),
            (
                "pasted-responses",
                format!("http://{mock_addr}/v1/responses"),
                "/v1/alpha/search",
                "/v1/alpha/search?client_version=0.145.0",
            ),
        ];

        for (provider_id, base_url, local_path, expected_upstream) in cases {
            let provider = Provider::with_id(
                provider_id.to_string(),
                provider_id.to_string(),
                json!({
                    "base_url": base_url,
                    "auth": {"OPENAI_API_KEY": "upstream-secret"}
                }),
                None,
            );
            db.save_provider("codex", &provider)
                .expect("save pasted base URL provider");
            db.set_current_provider("codex", &provider.id)
                .expect("select pasted base URL provider");

            let response = client
                .post(format!(
                    "http://127.0.0.1:{}{local_path}?client_version=0.145.0",
                    proxy_info.port
                ))
                .header(header::AUTHORIZATION, "Bearer client-secret")
                .json(&json!({"model": "gpt-image-1", "prompt": "pasted base URL"}))
                .send()
                .await
                .expect("send images request");

            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{local_path} via {base_url}"
            );
            let request = captured
                .lock()
                .await
                .pop()
                .expect("upstream request captured");
            assert_eq!(
                request.path_and_query, expected_upstream,
                "{local_path} via {base_url}"
            );
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer upstream-secret")
            );
        }

        proxy.stop().await.expect("stop test proxy");
        mock_handle.abort();
    }

    #[tokio::test]
    async fn codex_images_generation_aliases_forward_and_record_usage() {
        let captured = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
        let mock_app = Router::new().route(
            "/v1/images/generations",
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

                        (
                            StatusCode::OK,
                            [(header::CONTENT_TYPE, "application/json")],
                            r#"{"created":1,"data":[{"b64_json":"aW1hZ2U="}],"usage":{"input_tokens":7,"output_tokens":11,"total_tokens":18}}"#,
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
            "images-api-upstream".to_string(),
            "Images API Upstream".to_string(),
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
                enable_logging: true,
                non_streaming_timeout: 10,
                ..ProxyConfig::default()
            },
            db.clone(),
            None,
            // 测试用空注册表：不加载真实插件目录，保证回归行为与主线一致
            Arc::new(PluginRegistry::new()),
        );
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();
        let aliases = [
            "/images/generations",
            "/v1/images/generations",
            "/v1/v1/images/generations",
            "/codex/v1/images/generations",
        ];

        for (index, path) in aliases.iter().enumerate() {
            let response = client
                .post(format!(
                    "http://127.0.0.1:{}{}?client_version=0.145.0",
                    proxy_info.port, path
                ))
                .header(header::AUTHORIZATION, "Bearer client-secret")
                .json(&json!({
                    "model": "gpt-image-1",
                    "prompt": format!("image generation alias {index}")
                }))
                .send()
                .await
                .expect("send images request");

            assert_eq!(response.status(), StatusCode::OK, "alias {path}");
            assert_eq!(
                response.text().await.expect("read proxy response"),
                r#"{"created":1,"data":[{"b64_json":"aW1hZ2U="}],"usage":{"input_tokens":7,"output_tokens":11,"total_tokens":18}}"#,
                "alias {path}"
            );
        }

        let (log_count, input_tokens, output_tokens): (i64, i64, i64) =
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    let totals = {
                        let conn = crate::database::lock_conn!(db.conn);
                        match conn.query_row(
                            "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
                             FROM proxy_request_logs WHERE provider_id = ?1",
                            ["images-api-upstream"],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                        ) {
                            Ok(totals) => totals,
                            Err(error) => panic!("query images usage logs: {error}"),
                        }
                    };

                    if totals.0 == aliases.len() as i64 {
                        break Ok::<_, AppError>(totals);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("images usage logs were not recorded")
            .expect("read images usage logs");
        assert_eq!(log_count, aliases.len() as i64);
        assert_eq!(input_tokens, 7 * aliases.len() as i64);
        assert_eq!(output_tokens, 11 * aliases.len() as i64);

        let mut full_url_provider = Provider::with_id(
            "images-api-full-url".to_string(),
            "Images API Full URL".to_string(),
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
            .expect("save full URL images provider");
        db.set_current_provider("codex", &full_url_provider.id)
            .expect("select full URL images provider");

        let response = client
            .post(format!(
                "http://127.0.0.1:{}/v1/images/generations?client_version=0.145.0",
                proxy_info.port
            ))
            .header(header::AUTHORIZATION, "Bearer client-secret")
            .json(&json!({
                "model": "gpt-image-1",
                "prompt": "image generation full URL"
            }))
            .send()
            .await
            .expect("send full URL images request");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.text().await.expect("read full URL image response"),
            r#"{"created":1,"data":[{"b64_json":"aW1hZ2U="}],"usage":{"input_tokens":7,"output_tokens":11,"total_tokens":18}}"#
        );

        let captured = captured.lock().await;
        assert_eq!(captured.len(), aliases.len() + 1);
        for (index, request) in captured.iter().take(aliases.len()).enumerate() {
            assert_eq!(
                request.path_and_query,
                "/v1/images/generations?client_version=0.145.0"
            );
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer upstream-secret")
            );
            assert_eq!(request.body["model"], "gpt-image-1");
            assert_eq!(
                request.body["prompt"],
                format!("image generation alias {index}")
            );
        }

        let full_url_request = captured.last().expect("full URL image request captured");
        assert_eq!(
            full_url_request.path_and_query,
            "/v1/images/generations?api-version=test&client_version=0.145.0"
        );
        assert_eq!(
            full_url_request.authorization.as_deref(),
            Some("Bearer full-url-secret")
        );
        assert_eq!(full_url_request.body["model"], "gpt-image-1");
        assert_eq!(full_url_request.body["prompt"], "image generation full URL");

        proxy.stop().await.expect("stop test proxy");
        mock_handle.abort();
    }

    #[tokio::test]
    async fn codex_images_edit_aliases_forward_and_record_usage() {
        // Real Images API responses carry `input_tokens_details` with text/image
        // splits; the shared Codex usage parser must tolerate them.
        const UPSTREAM_BODY: &str = r#"{"created":1,"data":[{"b64_json":"ZWRpdA=="}],"usage":{"input_tokens":13,"output_tokens":17,"total_tokens":30,"input_tokens_details":{"text_tokens":5,"image_tokens":8}}}"#;
        const IMAGE_DATA_URL: &str = "data:image/png;base64,Zm9v";

        let captured = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
        let mock_app = Router::new().route(
            "/v1/images/edits",
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

                        (
                            StatusCode::OK,
                            [(header::CONTENT_TYPE, "application/json")],
                            UPSTREAM_BODY,
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
            "images-edit-upstream".to_string(),
            "Images Edit Upstream".to_string(),
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
                enable_logging: true,
                non_streaming_timeout: 10,
                ..ProxyConfig::default()
            },
            db.clone(),
            None,
            // 测试用空注册表：不加载真实插件目录，保证回归行为与主线一致
            Arc::new(PluginRegistry::new()),
        );
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();
        let aliases = [
            "/images/edits",
            "/v1/images/edits",
            "/v1/v1/images/edits",
            "/codex/v1/images/edits",
        ];

        for (index, path) in aliases.iter().enumerate() {
            let response = client
                .post(format!(
                    "http://127.0.0.1:{}{}?client_version=0.145.0",
                    proxy_info.port, path
                ))
                .header(header::AUTHORIZATION, "Bearer client-secret")
                .json(&json!({
                    "model": "gpt-image-2",
                    "prompt": format!("image edit alias {index}"),
                    "images": [{"image_url": IMAGE_DATA_URL}]
                }))
                .send()
                .await
                .expect("send images edit request");

            assert_eq!(response.status(), StatusCode::OK, "alias {path}");
            assert_eq!(
                response.text().await.expect("read proxy response"),
                UPSTREAM_BODY,
                "alias {path}"
            );
        }

        let (log_count, input_tokens, output_tokens): (i64, i64, i64) =
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    let totals = {
                        let conn = crate::database::lock_conn!(db.conn);
                        match conn.query_row(
                            "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
                             FROM proxy_request_logs WHERE provider_id = ?1",
                            ["images-edit-upstream"],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                        ) {
                            Ok(totals) => totals,
                            Err(error) => panic!("query images edit usage logs: {error}"),
                        }
                    };

                    if totals.0 == aliases.len() as i64 {
                        break Ok::<_, AppError>(totals);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("images edit usage logs were not recorded")
            .expect("read images edit usage logs");
        assert_eq!(log_count, aliases.len() as i64);
        assert_eq!(input_tokens, 13 * aliases.len() as i64);
        assert_eq!(output_tokens, 17 * aliases.len() as i64);

        // A full URL pasted for the sibling generations route must derive
        // `/images/edits` instead of posting edit payloads to generations.
        let mut full_url_provider = Provider::with_id(
            "images-edit-full-url".to_string(),
            "Images Edit Full URL".to_string(),
            json!({
                "base_url": format!("http://{mock_addr}/v1/images/generations?api-version=test"),
                "auth": {"OPENAI_API_KEY": "full-url-secret"}
            }),
            None,
        );
        full_url_provider.meta = Some(ProviderMeta {
            is_full_url: Some(true),
            ..ProviderMeta::default()
        });
        db.save_provider("codex", &full_url_provider)
            .expect("save full URL images edit provider");
        db.set_current_provider("codex", &full_url_provider.id)
            .expect("select full URL images edit provider");

        let response = client
            .post(format!(
                "http://127.0.0.1:{}/v1/images/edits?client_version=0.145.0",
                proxy_info.port
            ))
            .header(header::AUTHORIZATION, "Bearer client-secret")
            .json(&json!({
                "model": "gpt-image-2",
                "prompt": "image edit full URL",
                "images": [{"image_url": IMAGE_DATA_URL}]
            }))
            .send()
            .await
            .expect("send full URL images edit request");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .text()
                .await
                .expect("read full URL image edit response"),
            UPSTREAM_BODY
        );

        let captured = captured.lock().await;
        assert_eq!(captured.len(), aliases.len() + 1);
        for (index, request) in captured.iter().take(aliases.len()).enumerate() {
            assert_eq!(
                request.path_and_query,
                "/v1/images/edits?client_version=0.145.0"
            );
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer upstream-secret")
            );
            assert_eq!(request.body["model"], "gpt-image-2");
            assert_eq!(request.body["prompt"], format!("image edit alias {index}"));
            assert_eq!(request.body["images"][0]["image_url"], IMAGE_DATA_URL);
        }

        let full_url_request = captured
            .last()
            .expect("full URL image edit request captured");
        assert_eq!(
            full_url_request.path_and_query,
            "/v1/images/edits?api-version=test&client_version=0.145.0"
        );
        assert_eq!(
            full_url_request.authorization.as_deref(),
            Some("Bearer full-url-secret")
        );
        assert_eq!(full_url_request.body["model"], "gpt-image-2");
        assert_eq!(full_url_request.body["prompt"], "image edit full URL");
        assert_eq!(
            full_url_request.body["images"][0]["image_url"],
            IMAGE_DATA_URL
        );

        proxy.stop().await.expect("stop test proxy");
        mock_handle.abort();
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
            // 测试用空注册表：不加载真实插件目录，保证回归行为与主线一致
            Arc::new(PluginRegistry::new()),
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

    /// 端到端冒烟（#[ignore]，需 node 在 PATH，手动运行：
    /// `cargo test --lib -- --ignored user_plugin_pre_request`）
    ///
    /// 验证完整链路：临时插件目录里的真实 Node 脚本插件（stdin/stdout 协议）
    /// 经 load_user_plugins 加载 → 真实 ProxyServer 转发 → PreRequest 挂点改写
    /// 请求体 → 上游收到的 body 带有插件标记。
    #[tokio::test]
    #[ignore = "spawn 真实 Node 进程，需 node 在 PATH；仅作手动冒烟"]
    async fn user_plugin_pre_request_rewrites_body_end_to_end() {
        use std::fs;

        // 1. 临时插件目录：Node 脚本读 stdin JSON，在 body 顶层打 plugin_touched 标记
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_dir = tmp.path().join("e2e-marker");
        fs::create_dir_all(&plugin_dir).expect("create plugin dir");
        fs::write(
            plugin_dir.join("plugin.json"),
            r#"{
                "id": "e2e-marker",
                "name": "E2E Marker",
                "stages": ["pre_request"],
                "command": ["node", "rewrite.js"],
                "timeout_ms": 10000
            }"#,
        )
        .expect("write plugin.json");
        fs::write(
            plugin_dir.join("rewrite.js"),
            r#"
                let raw = "";
                process.stdin.on("data", (chunk) => { raw += chunk; });
                process.stdin.on("end", () => {
                  const input = JSON.parse(raw);
                  input.body.metadata = { plugin_touched: "user:e2e-marker" };
                  process.stdout.write(JSON.stringify({ body: input.body }));
                });
            "#,
        )
        .expect("write rewrite.js");

        // 2. mock 上游：捕获 /chat/completions 请求体
        let captured = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
        let capture_handler = {
            let captured = captured.clone();
            move |request: axum::extract::Request| {
                let captured = captured.clone();
                async move {
                    let (parts, body) = request.into_parts();
                    let body_bytes = axum::body::to_bytes(body, usize::MAX)
                        .await
                        .expect("read upstream body");
                    captured.lock().await.push(CapturedRequest {
                        path_and_query: parts
                            .uri
                            .path_and_query()
                            .map(|value| value.as_str().to_string())
                            .unwrap_or_else(|| parts.uri.path().to_string()),
                        authorization: None,
                        body: serde_json::from_slice(&body_bytes)
                            .unwrap_or(Value::Null),
                    });
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        r#"{"id":"chatcmpl-1","object":"chat.completion","model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
                    )
                }
            }
        };
        // base_url 已含 /v1，转发最终路径为 /v1/chat/completions
        let mock_app = Router::new().route("/v1/chat/completions", post(capture_handler));
        let mock_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind mock upstream");
        let mock_addr = mock_listener.local_addr().expect("mock upstream address");
        let mock_handle = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("serve mock upstream");
        });

        // 3. codex 供应商指向 mock 上游
        let db = Arc::new(Database::memory().expect("memory database"));
        let provider = Provider::with_id(
            "e2e-upstream".to_string(),
            "E2E Upstream".to_string(),
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

        // 4. 注册表加载真实的外部插件
        let registry = Arc::new(PluginRegistry::new());
        let (plugins, errors) =
            crate::proxy::plugins::external::load_user_plugins(tmp.path());
        assert!(errors.is_empty(), "load errors: {errors:?}");
        assert_eq!(plugins.len(), 1, "one user plugin loaded");
        for plugin in plugins {
            registry.register(plugin);
        }

        // 5. 起真实代理并发请求
        let proxy = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                non_streaming_timeout: 10,
                ..ProxyConfig::default()
            },
            db.clone(),
            None,
            registry,
        );
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();
        let response = client
            .post(format!("http://127.0.0.1:{}/chat/completions", proxy_info.port))
            .header(header::AUTHORIZATION, "Bearer client-secret")
            .json(&json!({
                "model": "gpt-4o",
                "stream": false,
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .send()
            .await
            .expect("send chat request");
        assert_eq!(response.status(), StatusCode::OK);
        let _ = response.text().await;

        let requests = captured.lock().await;
        assert_eq!(requests.len(), 1, "exactly one upstream request");
        assert_eq!(
            requests[0].body["metadata"]["plugin_touched"],
            json!("user:e2e-marker"),
            "PreRequest 插件应已改写到达上游的请求体"
        );

        mock_handle.abort();
    }

    // ------------------------------------------------------------------
    // SseChunk 插件挂点集成测试
    // ------------------------------------------------------------------

    /// 测试用 SseChunk 插件：把 data 中出现的 needle 替换为 replacement
    struct SseStubPlugin {
        needle: &'static str,
        replacement: &'static str,
    }

    impl crate::proxy::plugins::ProxyPlugin for SseStubPlugin {
        fn id(&self) -> &str {
            "test:sse-stub"
        }
        fn display_name(&self) -> &str {
            "SSE Stub"
        }
        fn description(&self) -> &str {
            "测试用：SSE data 子串替换"
        }
        fn is_builtin(&self) -> bool {
            false
        }
        fn stages(&self) -> &'static [crate::proxy::plugins::PluginStage] {
            &[crate::proxy::plugins::PluginStage::SseChunk]
        }
        fn default_priority(&self) -> i32 {
            100
        }
        fn transform_sse_event(
            &self,
            _ctx: &crate::proxy::plugins::PluginRequestContext,
            _event_name: Option<&str>,
            data: &mut String,
            _state: &mut dyn std::any::Any,
        ) -> Result<bool, crate::proxy::plugins::PluginError> {
            if data.contains(self.needle) {
                *data = data.replace(self.needle, self.replacement);
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    /// mock 上游返回的 SSE 流：混合 LF/CRLF 分隔符、注释与 id 行，
    /// 且第一个事件的 data 在 chunk 边界处被切开
    fn sse_upstream_response() -> axum::response::Response {
        let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = vec![
            Ok(axum::body::Bytes::from_static(
                b"event: delta\ndata: {\"text\":\"hello SE",
            )),
            Ok(axum::body::Bytes::from_static(
                b"CRET tail\"}\n\nevent: ping\ndata: {\"n\":1}\n\n",
            )),
            Ok(axum::body::Bytes::from_static(
                b"id: 9\r\n: note\r\nevent: done\r\ndata: [DONE]\r\n\r\n",
            )),
        ];
        let body = axum::body::Body::from_stream(futures::stream::iter(chunks));
        axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(body)
            .expect("build mock SSE response")
    }

    /// 搭建「mock 上游 + codex 供应商 + 指定插件注册表」的完整代理环境
    async fn start_sse_proxy(
        registry: Arc<PluginRegistry>,
    ) -> (ProxyServer, u16, tokio::task::JoinHandle<()>) {
        let mock_app =
            Router::new().route("/v1/chat/completions", post(|| async { sse_upstream_response() }));
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
            "sse-upstream".to_string(),
            "SSE Upstream".to_string(),
            json!({
                "base_url": format!("http://{mock_addr}/v1"),
                "auth": {"OPENAI_API_KEY": "upstream-secret"}
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");
        db.set_current_provider("codex", &provider.id)
            .expect("select provider");

        let proxy = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                non_streaming_timeout: 10,
                ..ProxyConfig::default()
            },
            db,
            None,
            registry,
        );
        let proxy_info = proxy.start().await.expect("start test proxy");
        (proxy, proxy_info.port, mock_handle)
    }

    async fn send_sse_request(port: u16) -> String {
        let client = reqwest::Client::new();
        let response = client
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .header(header::AUTHORIZATION, "Bearer client-secret")
            .json(&json!({
                "model": "gpt-4o",
                "stream": true,
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .send()
            .await
            .expect("send streaming chat request");
        assert_eq!(response.status(), StatusCode::OK);
        response.text().await.expect("read streaming body")
    }

    #[tokio::test]
    async fn sse_chunk_plugin_transforms_streaming_response() {
        let registry = Arc::new(PluginRegistry::new());
        registry.register(Arc::new(SseStubPlugin {
            needle: "SECRET",
            replacement: "REPLACED",
        }));
        let (proxy, port, mock_handle) = start_sse_proxy(registry).await;

        let text = send_sse_request(port).await;

        // 被修改的块重 emitted（event 行保留、单行 data、沿用 \n\n 分隔符）；
        // 未修改的块（含 CRLF 块的 id/注释行）字节级原样透传
        let expected = "event: delta\ndata: {\"text\":\"hello REPLACED tail\"}\n\n\
                        event: ping\ndata: {\"n\":1}\n\n\
                        id: 9\r\n: note\r\nevent: done\r\ndata: [DONE]\r\n\r\n";
        assert_eq!(text, expected);

        proxy.stop().await.expect("stop test proxy");
        mock_handle.abort();
    }

    #[tokio::test]
    async fn sse_streaming_passthrough_without_sse_chunk_plugin_is_byte_identical() {
        // 回归红线：无 SseChunk 插件时代理行为与主线完全一致（字节级透传）
        let (proxy, port, mock_handle) = start_sse_proxy(Arc::new(PluginRegistry::new())).await;

        let text = send_sse_request(port).await;

        let expected = "event: delta\ndata: {\"text\":\"hello SECRET tail\"}\n\n\
                        event: ping\ndata: {\"n\":1}\n\n\
                        id: 9\r\n: note\r\nevent: done\r\ndata: [DONE]\r\n\r\n";
        assert_eq!(text, expected);

        proxy.stop().await.expect("stop test proxy");
        mock_handle.abort();
    }
}
