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
            // Claude Router 网关（VS Code 模型选择器的确定性 provider/model 路由）
            .route(
                "/claude-router/v1/models",
                get(handlers::handle_claude_router_models),
            )
            .route(
                "/claude-router/v1/messages",
                post(handlers::handle_claude_router_messages),
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
    use crate::app_config::AppType;
    use crate::provider::{ClaudeRouterConfig, ClaudeRouterModel, Provider, ProviderMeta};
    use crate::proxy::claude_router::public_model_id;
    use crate::AppError;
    use axum::http::{header, HeaderMap, StatusCode};
    use serde_json::{json, Value};
    use serial_test::serial;
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

    // ========================================================================
    // Claude Router 网关（/claude-router）
    // ========================================================================

    #[derive(Debug)]
    struct CapturedClaudeRequest {
        authorization: Option<String>,
        x_api_key: Option<String>,
        model: Option<String>,
    }
    const CLAUDE_ROUTER_SSE_BODY: &str = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_sse\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"upstream-model-a\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    fn claude_router_provider(
        id: &str,
        name: &str,
        base_url: &str,
        token: &str,
        sort_index: Option<usize>,
        router: Option<ClaudeRouterConfig>,
    ) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            name.to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_AUTH_TOKEN": token,
                }
            }),
            None,
        );
        provider.sort_index = sort_index;
        if let Some(router) = router {
            provider.meta = Some(ProviderMeta {
                claude_router: Some(router),
                ..ProviderMeta::default()
            });
        }
        provider
    }

    fn enabled_router_config(models: Vec<ClaudeRouterModel>) -> ClaudeRouterConfig {
        ClaudeRouterConfig {
            enabled: true,
            models,
        }
    }

    fn router_model(alias: &str, upstream: &str, display: &str) -> ClaudeRouterModel {
        ClaudeRouterModel {
            alias: alias.to_string(),
            upstream_model: upstream.to_string(),
            display_name: display.to_string(),
        }
    }

    /// 启动一个捕获 authorization / x-api-key / model 的 Anthropic 风格 mock 上游
    async fn spawn_mock_anthropic_upstream(
        captured: Arc<Mutex<Vec<CapturedClaudeRequest>>>,
    ) -> (std::net::SocketAddr, JoinHandle<()>) {
        let mock_app = Router::new().route(
            "/v1/messages",
            post({
                let captured = captured.clone();
                move |request: axum::extract::Request| {
                    let captured = captured.clone();
                    async move {
                        let (parts, body) = request.into_parts();
                        let body = axum::body::to_bytes(body, 1024 * 1024)
                            .await
                            .expect("read mock request body");
                        let body: Value =
                            serde_json::from_slice(&body).expect("parse mock request body");
                        captured.lock().await.push(CapturedClaudeRequest {
                            authorization: parts
                                .headers
                                .get(header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .map(ToString::to_string),
                            x_api_key: parts
                                .headers
                                .get("x-api-key")
                                .and_then(|value| value.to_str().ok())
                                .map(ToString::to_string),
                            model: body
                                .get("model")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                        });
                        // The production exact-header path registers its response parser
                        // immediately after writing the request. A zero-latency in-process
                        // response can beat that registration and exercise its legacy
                        // fallback instead of the router contract under test.
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        axum::Json(json!({
                            "id": "msg_mock",
                            "type": "message",
                            "role": "assistant",
                            "model": body.get("model").cloned().unwrap_or(Value::Null),
                            "content": [{"type": "text", "text": "ok"}],
                            "stop_reason": "end_turn",
                            "usage": {"input_tokens": 1, "output_tokens": 1}
                        }))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind mock upstream");
        let addr = listener.local_addr().expect("mock upstream address");
        let handle = tokio::spawn(async move {
            axum::serve(listener, mock_app)
                .await
                .expect("serve mock upstream");
        });
        (addr, handle)
    }

    /// 启动一个返回有序 Anthropic SSE 事件流的 mock 上游
    async fn spawn_mock_anthropic_sse_upstream() -> (std::net::SocketAddr, JoinHandle<()>) {
        let mock_app = Router::new().route(
            "/v1/messages",
            post(|| async {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    CLAUDE_ROUTER_SSE_BODY,
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind mock sse upstream");
        let addr = listener.local_addr().expect("mock sse upstream address");
        let handle = tokio::spawn(async move {
            axum::serve(listener, mock_app)
                .await
                .expect("serve mock sse upstream");
        });
        (addr, handle)
    }

    fn test_proxy(db: Arc<Database>) -> ProxyServer {
        // In production Tauri installs Ring during app setup (`lib.rs`).
        // These in-process tests bypass that setup, so initialize the same
        // process-wide provider before reqwest creates concurrent connectors.
        let _ = rustls::crypto::ring::default_provider().install_default();
        ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                enable_logging: false,
                non_streaming_timeout: 10,
                ..ProxyConfig::default()
            },
            db,
            None,
        )
    }
    struct ClaudeCurrentProviderGuard {
        _home: tempfile::TempDir,
        previous_test_home: Option<std::ffi::OsString>,
        previous_settings: crate::settings::AppSettings,
    }

    impl ClaudeCurrentProviderGuard {
        fn new(provider_id: &str) -> Self {
            let previous_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            let previous_settings = crate::settings::get_settings();
            let home = tempfile::tempdir().expect("create isolated settings home");
            std::env::set_var("CC_SWITCH_TEST_HOME", home.path());
            crate::settings::update_settings(crate::settings::AppSettings::default())
                .expect("reset settings in isolated home");
            crate::settings::set_current_provider(&AppType::Claude, Some(provider_id))
                .expect("set isolated current Claude provider");
            Self {
                _home: home,
                previous_test_home,
                previous_settings,
            }
        }
    }

    impl Drop for ClaudeCurrentProviderGuard {
        fn drop(&mut self) {
            let _ = crate::settings::update_settings(self.previous_settings.clone());
            match &self.previous_test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn routed_messages_url(port: u16) -> String {
        format!("http://127.0.0.1:{port}/claude-router/v1/messages")
    }

    fn routed_request_body(model: &str, stream: bool) -> Value {
        json!({
            "model": model,
            "max_tokens": 16,
            "stream": stream,
            "messages": [{"role": "user", "content": "hi"}]
        })
    }

    #[tokio::test]
    async fn claude_router_models_catalog_exposes_only_enabled_routes() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let provider_a = claude_router_provider(
            "router-prov-a",
            "Router A",
            "https://upstream-a.example.com",
            "router-token-a",
            Some(1),
            Some(enabled_router_config(vec![
                router_model("fast", "upstream-fast-a", "Fast A"),
                router_model("pro", "upstream-pro-a", "Pro A"),
            ])),
        );
        let provider_b = claude_router_provider(
            "router-prov-b",
            "Router B",
            "https://upstream-b.example.com",
            "router-token-b",
            Some(0),
            Some(enabled_router_config(vec![router_model(
                "mini",
                "upstream-mini-b",
                "Mini B",
            )])),
        );
        let provider_c = claude_router_provider(
            "router-prov-c",
            "Router C",
            "https://upstream-c.example.com",
            "router-token-c",
            Some(2),
            Some(ClaudeRouterConfig {
                enabled: false,
                models: vec![router_model("off", "upstream-off-c", "Off C")],
            }),
        );
        // 有意乱序保存，验证目录仍按 sort_index 输出
        db.save_provider("claude", &provider_a).expect("save a");
        db.save_provider("claude", &provider_c).expect("save c");
        db.save_provider("claude", &provider_b).expect("save b");

        let proxy = test_proxy(db);
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();

        let response = client
            .get(format!(
                "http://127.0.0.1:{}/claude-router/v1/models",
                proxy_info.port
            ))
            .send()
            .await
            .expect("send models request");
        assert_eq!(response.status(), StatusCode::OK);
        let text = response.text().await.expect("read catalog body");
        let catalog: Value = serde_json::from_str(&text).expect("parse catalog");
        let entries = catalog["data"].as_array().expect("data array");
        let ids: Vec<&str> = entries
            .iter()
            .map(|entry| entry["id"].as_str().expect("entry id"))
            .collect();

        // 启用的 A/B 按 sort_index（B=0 在 A=1 前）再按模型配置顺序输出；禁用的 C 不出现
        assert_eq!(
            ids,
            vec![
                public_model_id("router-prov-b", "mini"),
                public_model_id("router-prov-a", "fast"),
                public_model_id("router-prov-a", "pro"),
            ]
        );
        assert!(
            ids.iter().all(|id| id.starts_with("ccswitch/claude/")),
            "公开 ID 必须带固定前缀（通过 Claude Code 的 claude|anthropic 过滤）"
        );
        assert_eq!(entries[0]["object"], json!("model"));
        assert_eq!(entries[0]["display_name"], json!("Router B / Mini B"));
        assert_eq!(entries[2]["display_name"], json!("Router A / Pro A"));

        // 目录不得泄露任何凭证 / 端点
        for sentinel in [
            "router-token-a",
            "router-token-b",
            "router-token-c",
            "upstream-a.example.com",
            "upstream-b.example.com",
            "upstream-c.example.com",
            "ANTHROPIC_AUTH_TOKEN",
        ] {
            assert!(
                !text.contains(sentinel),
                "catalog must not contain '{sentinel}': {text}"
            );
        }

        proxy.stop().await.expect("stop test proxy");
    }

    #[tokio::test]
    async fn claude_router_messages_reject_invalid_ids_with_anthropic_400() {
        let captured = Arc::new(Mutex::new(Vec::<CapturedClaudeRequest>::new()));
        let (mock_addr, mock_handle) = spawn_mock_anthropic_upstream(captured.clone()).await;

        let db = Arc::new(Database::memory().expect("memory database"));
        let disabled = claude_router_provider(
            "router-disabled",
            "Disabled",
            &format!("http://{mock_addr}"),
            "disabled-token",
            None,
            Some(ClaudeRouterConfig {
                enabled: false,
                models: vec![router_model("fast", "upstream-off", "Off")],
            }),
        );
        let enabled = claude_router_provider(
            "router-enabled",
            "Enabled",
            &format!("http://{mock_addr}"),
            "enabled-token",
            None,
            Some(enabled_router_config(vec![router_model(
                "fast",
                "upstream-fast-e",
                "Fast E",
            )])),
        );
        db.save_provider("claude", &disabled)
            .expect("save disabled");
        db.save_provider("claude", &enabled).expect("save enabled");

        let proxy = test_proxy(db);
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();
        let url = routed_messages_url(proxy_info.port);

        let cases = [
            // 缺少 ccswitch/claude/ 前缀
            "claude-3-5-sonnet".to_string(),
            // 非法 base64url 段
            "ccswitch/claude/!!!/QQ".to_string(),
            // 段数错误
            "ccswitch/claude/b25seQ".to_string(),
            // 未知供应商
            public_model_id("ghost-provider", "fast"),
            // 路由禁用
            public_model_id("router-disabled", "fast"),
            // 未知别名
            public_model_id("router-enabled", "slow"),
        ];
        for model in &cases {
            let response = client
                .post(&url)
                .json(&routed_request_body(model, false))
                .send()
                .await
                .expect("send invalid routed request");
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "model '{model}' 应返回 400"
            );
            let body: Value = response.json().await.expect("parse error body");
            assert_eq!(body["type"], json!("error"), "model '{model}'");
            assert_eq!(
                body["error"]["type"],
                json!("invalid_request_error"),
                "model '{model}'"
            );
            assert!(
                body["error"]["message"]
                    .as_str()
                    .map(|message| !message.is_empty())
                    .unwrap_or(false),
                "model '{model}' 应带非空 message"
            );
        }

        proxy.stop().await.expect("stop test proxy");
        mock_handle.abort();
        assert!(captured.lock().await.is_empty(), "非法路由绝不能触达上游");
    }

    #[tokio::test]
    async fn claude_router_bypasses_provider_default_model_mapping() {
        let captured = Arc::new(Mutex::new(Vec::<CapturedClaudeRequest>::new()));
        let (mock_addr, mock_handle) = spawn_mock_anthropic_upstream(captured.clone()).await;

        let db = Arc::new(Database::memory().expect("memory database"));
        let mut provider = claude_router_provider(
            "router-mapped",
            "Mapped",
            &format!("http://{mock_addr}"),
            "router-token",
            None,
            Some(enabled_router_config(vec![router_model(
                "fast",
                "upstream-fast",
                "Fast",
            )])),
        );
        provider.settings_config["env"]["ANTHROPIC_MODEL"] =
            Value::String("legacy-default".to_string());
        db.save_provider("claude", &provider)
            .expect("save mapped provider");

        let proxy = test_proxy(db);
        let proxy_info = proxy.start().await.expect("start test proxy");
        let response = reqwest::Client::new()
            .post(routed_messages_url(proxy_info.port))
            .json(&routed_request_body(
                &public_model_id("router-mapped", "fast"),
                false,
            ))
            .send()
            .await
            .expect("send routed request");
        assert_eq!(response.status(), StatusCode::OK);

        proxy.stop().await.expect("stop test proxy");
        mock_handle.abort();
        let captured = captured.lock().await;
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].model.as_deref(), Some("upstream-fast"));
    }

    #[tokio::test]
    #[serial]
    async fn claude_router_concurrent_requests_keep_routes_and_current_provider() {
        let captured_a = Arc::new(Mutex::new(Vec::<CapturedClaudeRequest>::new()));
        let captured_b = Arc::new(Mutex::new(Vec::<CapturedClaudeRequest>::new()));
        let (addr_a, handle_a) = spawn_mock_anthropic_upstream(captured_a.clone()).await;
        let (addr_b, handle_b) = spawn_mock_anthropic_upstream(captured_b.clone()).await;

        let db = Arc::new(Database::memory().expect("memory database"));
        let provider_a = claude_router_provider(
            "router-prov-a",
            "Router A",
            &format!("http://{addr_a}"),
            "router-token-a",
            Some(0),
            Some(enabled_router_config(vec![router_model(
                "fast",
                "upstream-model-a",
                "Fast A",
            )])),
        );
        let provider_b = claude_router_provider(
            "router-prov-b",
            "Router B",
            &format!("http://{addr_b}"),
            "router-token-b",
            Some(1),
            Some(enabled_router_config(vec![router_model(
                "pro",
                "upstream-model-b",
                "Pro B",
            )])),
        );
        // 哨兵：传统"当前供应商"，路由请求全程不得改变它
        let sentinel = claude_router_provider(
            "router-sentinel",
            "Sentinel C",
            "https://sentinel.example.com",
            "router-token-c",
            Some(2),
            None,
        );
        db.save_provider("claude", &provider_a).expect("save a");
        db.save_provider("claude", &provider_b).expect("save b");
        db.save_provider("claude", &sentinel)
            .expect("save sentinel");
        db.set_current_provider("claude", &sentinel.id)
            .expect("set sentinel current");
        let _current_guard = ClaudeCurrentProviderGuard::new(&sentinel.id);
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("router-sentinel")
        );

        let proxy = test_proxy(db.clone());
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();
        let url = routed_messages_url(proxy_info.port);
        let id_a = public_model_id("router-prov-a", "fast");
        let id_b = public_model_id("router-prov-b", "pro");

        // 100 个并发混合请求：交替 A/B 各 50
        let tasks: Vec<_> = (0..100)
            .map(|index| {
                let client = client.clone();
                let url = url.clone();
                let model = if index % 2 == 0 { &id_a } else { &id_b };
                let body = routed_request_body(model, false);
                async move { client.post(url).json(&body).send().await }
            })
            .collect();
        let responses = futures::future::join_all(tasks).await;
        for (index, response) in responses.iter().enumerate() {
            let response = response
                .as_ref()
                .unwrap_or_else(|_| panic!("request {index} failed to send"));
            assert_eq!(response.status(), StatusCode::OK, "request {index}");
        }

        proxy.stop().await.expect("stop test proxy");
        handle_a.abort();
        handle_b.abort();

        // 每个上游恰好收到自己的 50 个请求，只见自己的凭证与上游模型名
        let captured_a = captured_a.lock().await;
        let captured_b = captured_b.lock().await;
        let captured_total = captured_a.len() + captured_b.len();
        assert_eq!(captured_total, 100, "每个客户端请求必须且只能触达一个上游");
        assert_eq!(captured_a.len(), 50, "provider A 应收到 50 个请求");
        assert_eq!(captured_b.len(), 50, "provider B 应收到 50 个请求");
        for request in captured_a.iter() {
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer router-token-a")
            );
            assert_eq!(request.x_api_key, None);
            assert_eq!(request.model.as_deref(), Some("upstream-model-a"));
        }
        for request in captured_b.iter() {
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer router-token-b")
            );
            assert_eq!(request.x_api_key, None);
            assert_eq!(request.model.as_deref(), Some("upstream-model-b"));
        }

        // 数据库与本地 settings 的当前供应商均保持哨兵不变
        assert_eq!(
            db.get_current_provider("claude")
                .expect("db current provider")
                .as_deref(),
            Some("router-sentinel")
        );
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("router-sentinel"),
            "路由请求不得改写本地 settings 的当前供应商"
        );
    }

    #[tokio::test]
    #[serial]
    async fn claude_router_routes_ignore_legacy_active_provider_changes() {
        let captured_a = Arc::new(Mutex::new(Vec::<CapturedClaudeRequest>::new()));
        let captured_b = Arc::new(Mutex::new(Vec::<CapturedClaudeRequest>::new()));
        let (addr_a, handle_a) = spawn_mock_anthropic_upstream(captured_a.clone()).await;
        let (addr_b, handle_b) = spawn_mock_anthropic_upstream(captured_b.clone()).await;

        let db = Arc::new(Database::memory().expect("memory database"));
        let provider_a = claude_router_provider(
            "router-prov-a",
            "Router A",
            &format!("http://{addr_a}"),
            "router-token-a",
            Some(0),
            Some(enabled_router_config(vec![router_model(
                "fast",
                "upstream-model-a",
                "Fast A",
            )])),
        );
        let provider_b = claude_router_provider(
            "router-prov-b",
            "Router B",
            &format!("http://{addr_b}"),
            "router-token-b",
            Some(1),
            Some(enabled_router_config(vec![router_model(
                "pro",
                "upstream-model-b",
                "Pro B",
            )])),
        );
        db.save_provider("claude", &provider_a).expect("save a");
        db.save_provider("claude", &provider_b).expect("save b");
        db.set_current_provider("claude", &provider_a.id)
            .expect("set initial current");
        let _current_guard = ClaudeCurrentProviderGuard::new(&provider_a.id);

        let proxy = test_proxy(db.clone());
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();
        let url = routed_messages_url(proxy_info.port);
        let id_a = public_model_id("router-prov-a", "fast");
        let id_b = public_model_id("router-prov-b", "pro");

        let send_mixed_batch = |count: usize| {
            let client = client.clone();
            let url = url.clone();
            let id_a = id_a.clone();
            let id_b = id_b.clone();
            async move {
                let tasks: Vec<_> = (0..count)
                    .map(|index| {
                        let client = client.clone();
                        let url = url.clone();
                        let model = if index % 2 == 0 { &id_a } else { &id_b };
                        let body = routed_request_body(model, false);
                        async move { client.post(url).json(&body).send().await }
                    })
                    .collect();
                let responses = futures::future::join_all(tasks).await;
                for response in responses {
                    assert_eq!(
                        response.expect("send routed request").status(),
                        StatusCode::OK
                    );
                }
            }
        };

        // 批次 1：当前供应商为 A
        send_mixed_batch(20).await;
        // 模拟传统 Switch：把"当前供应商"切到 B
        db.set_current_provider("claude", &provider_b.id)
            .expect("legacy switch to b");
        crate::settings::set_current_provider(&AppType::Claude, Some(&provider_b.id))
            .expect("legacy local switch to b");
        // 批次 2：路由目的地不应随之改变
        send_mixed_batch(20).await;

        proxy.stop().await.expect("stop test proxy");
        handle_a.abort();
        handle_b.abort();

        let captured_a = captured_a.lock().await;
        let captured_b = captured_b.lock().await;
        assert_eq!(captured_a.len(), 20, "A 路由与传统当前供应商解耦");
        assert_eq!(captured_b.len(), 20, "B 路由与传统当前供应商解耦");
        assert!(
            captured_a.iter().all(
                |request| request.model.as_deref() == Some("upstream-model-a")
                    && request.authorization.as_deref() == Some("Bearer router-token-a")
            ),
            "A 的请求必须始终携带 A 的凭证与上游模型"
        );
        assert!(
            captured_b.iter().all(
                |request| request.model.as_deref() == Some("upstream-model-b")
                    && request.authorization.as_deref() == Some("Bearer router-token-b")
            ),
            "B 的请求必须始终携带 B 的凭证与上游模型"
        );
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("router-prov-b"),
            "传统当前供应商应已切换，但不影响确定性路由"
        );
    }

    #[tokio::test]
    async fn claude_router_streaming_passthrough_preserves_sse() {
        let (addr, mock_handle) = spawn_mock_anthropic_sse_upstream().await;

        let db = Arc::new(Database::memory().expect("memory database"));
        let provider = claude_router_provider(
            "router-prov-a",
            "Router A",
            &format!("http://{addr}"),
            "router-token-a",
            None,
            Some(enabled_router_config(vec![router_model(
                "fast",
                "upstream-model-a",
                "Fast A",
            )])),
        );
        db.save_provider("claude", &provider)
            .expect("save provider");

        let proxy = test_proxy(db);
        let proxy_info = proxy.start().await.expect("start test proxy");
        let client = reqwest::Client::new();

        let response = client
            .post(routed_messages_url(proxy_info.port))
            .json(&routed_request_body(
                &public_model_id("router-prov-a", "fast"),
                true,
            ))
            .send()
            .await
            .expect("send streaming routed request");
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            content_type.starts_with("text/event-stream"),
            "流式响应必须保留 SSE Content-Type，实际: {content_type}"
        );
        let body = response.text().await.expect("read sse body");

        assert_eq!(
            body, CLAUDE_ROUTER_SSE_BODY,
            "Anthropic SSE 事件字节必须按原顺序原样透传"
        );

        proxy.stop().await.expect("stop test proxy");
        mock_handle.abort();
    }
}
