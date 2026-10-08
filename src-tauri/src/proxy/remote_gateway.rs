//! Remote gateway: SSH hosts reach this proxy through a reverse tunnel.
//!
//! Tunnelled requests arrive on a separate loopback listener that requires a
//! per-host token, so the regular listener keeps working unauthenticated for
//! local CLIs. Each host can pin its own provider per app; the origin of a
//! request travels through a task-local so handlers need no signature changes.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use axum::body::Body;
use axum::Router;
use http::{HeaderMap, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rusqlite::OptionalExtension;
use tokio::sync::oneshot;

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;

pub const DATA_SOURCE_PREFIX: &str = "remote:";

#[derive(Debug, Clone)]
pub struct RemoteOrigin {
    pub host_key: String,
}

tokio::task_local! {
    static REMOTE_ORIGIN: RemoteOrigin;
}

pub fn current_origin() -> Option<RemoteOrigin> {
    REMOTE_ORIGIN.try_with(Clone::clone).ok()
}

pub fn data_source_for_host(host_key: &str) -> String {
    format!("{DATA_SOURCE_PREFIX}{host_key}")
}

/// Usage is logged after the response streams, outside the request task, so
/// the origin is remembered per session id (bounded, oldest evicted first).
struct SessionSources {
    map: HashMap<String, String>,
    order: VecDeque<String>,
}

const SESSION_SOURCES_CAP: usize = 4096;

fn session_sources() -> &'static Mutex<SessionSources> {
    static SOURCES: OnceLock<Mutex<SessionSources>> = OnceLock::new();
    SOURCES.get_or_init(|| {
        Mutex::new(SessionSources {
            map: HashMap::new(),
            order: VecDeque::new(),
        })
    })
}

pub fn remember_session_source(session_id: &str, data_source: String) {
    let mut sources = session_sources().lock().unwrap_or_else(|e| e.into_inner());
    if sources
        .map
        .insert(session_id.to_string(), data_source)
        .is_none()
    {
        sources.order.push_back(session_id.to_string());
        while sources.order.len() > SESSION_SOURCES_CAP {
            if let Some(oldest) = sources.order.pop_front() {
                sources.map.remove(&oldest);
            }
        }
    }
}

pub fn session_source(session_id: Option<&str>) -> Option<String> {
    let session_id = session_id?;
    let sources = session_sources().lock().unwrap_or_else(|e| e.into_inner());
    sources.map.get(session_id).cloned()
}

/// What a host may do with one app through the gateway.
#[derive(Debug)]
pub enum RemoteRoute {
    /// Follows the local current provider (and its failover queue).
    FollowLocal,
    Pinned(Box<Provider>),
    /// The app is not enabled for this host, or its pinned provider is gone.
    Denied(String),
}

/// The host token only proves which host is calling; each app needs its own
/// enabled route, so disabling one app on a shared tunnel revokes it.
pub fn remote_route(
    db: &Database,
    host_key: &str,
    app_type: &str,
) -> Result<RemoteRoute, AppError> {
    let route: Option<(bool, Option<String>)> = {
        let conn = crate::database::lock_conn!(db.conn);
        conn.query_row(
            "SELECT enabled, provider_id FROM remote_gateway_routes
             WHERE host_key = ?1 AND app_type = ?2",
            rusqlite::params![host_key, app_type],
            |row| Ok((row.get::<_, i64>(0)? != 0, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))?
    };
    let provider_id = match route {
        Some((true, provider_id)) => provider_id,
        _ => {
            return Ok(RemoteRoute::Denied(format!(
                "远端 {host_key} 未启用 {app_type} 的本地网关"
            )))
        }
    };
    let Some(provider_id) = provider_id else {
        return Ok(RemoteRoute::FollowLocal);
    };
    Ok(
        match db.get_all_providers(app_type)?.shift_remove(&provider_id) {
            Some(provider) => RemoteRoute::Pinned(Box::new(provider)),
            None => {
                log::warn!(
                    "[RemoteGateway] {host_key} pins missing provider {provider_id} for {app_type}"
                );
                RemoteRoute::Denied(format!(
                    "远端 {host_key} 固定的 {app_type} 供应商已不存在，请在 CC Switch 中重新选择"
                ))
            }
        },
    )
}

/// Provider pinned for this app on the request's host, if any. `None` means
/// the host follows the local current provider; a denied route is an error.
pub fn pinned_provider(
    db: &Database,
    origin: &RemoteOrigin,
    app_type: &str,
) -> Result<Option<Provider>, crate::proxy::ProxyError> {
    match remote_route(db, &origin.host_key, app_type)
        .map_err(|e| crate::proxy::ProxyError::DatabaseError(e.to_string()))?
    {
        RemoteRoute::FollowLocal => Ok(None),
        RemoteRoute::Pinned(provider) => Ok(Some(*provider)),
        RemoteRoute::Denied(message) => Err(crate::proxy::ProxyError::AuthError(message)),
    }
}

/// The app a tunnelled request belongs to. Only the client routes the remote
/// CLIs call are exposed; everything else (status, other apps) is refused.
fn remote_app_for(uri: &http::Uri, headers: &HeaderMap) -> Option<&'static str> {
    let path = uri.path();
    if ["/v1beta/", "/gemini/v1beta/", "/gemini/v1/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return Some("gemini");
    }
    if matches!(path, "/v1/messages" | "/claude/v1/messages") {
        return Some("claude");
    }
    if matches!(path, "/models" | "/v1/models") {
        return Some(
            if super::handlers::is_claude_model_discovery(uri, headers) {
                "claude"
            } else {
                "codex"
            },
        );
    }
    let rest = ["/codex/v1", "/v1/v1", "/v1"]
        .iter()
        .find_map(|prefix| path.strip_prefix(prefix))
        .unwrap_or(path);
    matches!(
        rest,
        "/responses"
            | "/responses/compact"
            | "/chat/completions"
            | "/alpha/search"
            | "/images/generations"
            | "/images/edits"
    )
    .then_some("codex")
}

/// Remote routes also keep the shared proxy alive when local apps use direct mode.
pub(crate) fn has_enabled_routes(db: &Database) -> Result<bool, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM remote_gateway_routes WHERE enabled = 1)",
        [],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(error.to_string()))
}

/// Remote CLIs can't hand over a local ChatGPT / Claude subscription login.
pub fn provider_usable_remotely(provider: &Provider) -> bool {
    !crate::proxy::providers::is_codex_official_provider(provider)
        && provider.category.as_deref() != Some("official")
}

fn lookup_host_by_token(db: &Database, token: &str) -> Option<String> {
    let conn = db.conn.lock().ok()?;
    conn.query_row(
        "SELECT host_key FROM remote_gateways WHERE token = ?1",
        [token],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
}

fn extract_client_token(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    if let Some(value) = header("authorization") {
        let token = value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "))
            .unwrap_or(value)
            .trim();
        return Some(token.to_string());
    }
    if let Some(value) = header("x-api-key").or_else(|| header("x-goog-api-key")) {
        return Some(value.to_string());
    }
    query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("key="))
        .map(str::to_string)
}

/// Drops `key=` from the query so the gateway token never reaches upstream.
fn strip_key_query(uri: &http::Uri) -> Option<http::Uri> {
    let query = uri.query()?;
    if !query.split('&').any(|pair| pair.starts_with("key=")) {
        return None;
    }
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| !pair.starts_with("key="))
        .collect();
    let path_and_query = if kept.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), kept.join("&"))
    };
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = path_and_query.parse().ok();
    http::Uri::from_parts(parts).ok()
}

fn json_error(status: StatusCode, message: &str) -> Response<Body> {
    let error_type = match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::FORBIDDEN => "permission_error",
        StatusCode::NOT_FOUND => "not_found_error",
        _ => "api_error",
    };
    let body = serde_json::json!({
        "type": "error",
        "error": { "type": error_type, "message": message }
    });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

/// Supplies the router of the currently running proxy (None when stopped), so
/// tunnelled traffic always shares the live proxy state and stops with it.
pub type RouterSource =
    std::sync::Arc<dyn Fn() -> futures::future::BoxFuture<'static, Option<Router>> + Send + Sync>;

pub struct RemoteGatewayListener {
    pub port: u16,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for RemoteGatewayListener {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl RemoteGatewayListener {
    pub async fn start(
        db: std::sync::Arc<Database>,
        router_source: RouterSource,
    ) -> Result<Self, AppError> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| AppError::Message(format!("远端网关监听失败: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::Message(format!("远端网关监听失败: {e}")))?
            .port();
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        log::info!("[RemoteGateway] listening on 127.0.0.1:{port}");

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            continue;
                        };
                        tokio::spawn(serve_connection(stream, db.clone(), router_source.clone()));
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
            log::info!("[RemoteGateway] listener on port {port} stopped");
        });

        Ok(Self {
            port,
            shutdown: Some(shutdown_tx),
        })
    }
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    db: std::sync::Arc<Database>,
    router_source: RouterSource,
) {
    let original_cases = {
        let mut peek_buf = vec![0u8; 8192];
        match stream.peek(&mut peek_buf).await {
            Ok(n) => super::hyper_client::OriginalHeaderCases::from_raw_bytes(&peek_buf[..n]),
            Err(_) => super::hyper_client::OriginalHeaderCases::default(),
        }
    };

    let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
        let db = db.clone();
        let router_source = router_source.clone();
        let cases = original_cases.clone();
        async move {
            let (mut parts, body) = req.into_parts();
            let token = extract_client_token(&parts.headers, parts.uri.query());
            let Some(host_key) = token.and_then(|token| lookup_host_by_token(&db, &token)) else {
                return Ok::<_, std::convert::Infallible>(json_error(
                    StatusCode::UNAUTHORIZED,
                    "CC Switch 远端网关密钥无效，请在 CC Switch 中重新推送网关配置",
                ));
            };
            let Some(app_type) = remote_app_for(&parts.uri, &parts.headers) else {
                return Ok(json_error(
                    StatusCode::NOT_FOUND,
                    "CC Switch 远端网关不提供该路径",
                ));
            };
            match remote_route(&db, &host_key, app_type) {
                Ok(RemoteRoute::FollowLocal | RemoteRoute::Pinned(_)) => {}
                Ok(RemoteRoute::Denied(message)) => {
                    return Ok(json_error(StatusCode::FORBIDDEN, &message));
                }
                Err(e) => {
                    return Ok(json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &e.to_string(),
                    ));
                }
            }
            let Some(mut router) = router_source().await else {
                return Ok(json_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "本机 CC Switch 网关未运行",
                ));
            };

            if let Some(uri) = strip_key_query(&parts.uri) {
                parts.uri = uri;
            }
            parts.extensions.insert(cases);
            let request = http::Request::from_parts(parts, Body::new(body));
            let origin = RemoteOrigin { host_key };
            let response = REMOTE_ORIGIN
                .scope(origin, async move {
                    <Router as tower::Service<http::Request<Body>>>::call(&mut router, request)
                        .await
                })
                .await;
            Ok(response.unwrap_or_else(|never| match never {}))
        }
    });

    if let Err(e) = hyper::server::conn::http1::Builder::new()
        .preserve_header_case(true)
        .serve_connection(TokioIo::new(stream), service)
        .await
    {
        log::debug!("[RemoteGateway] connection error: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routed_state(provider: &Provider, pinned: bool) -> crate::proxy::server::ProxyState {
        let db = std::sync::Arc::new(Database::memory().unwrap());
        db.save_provider("codex", provider).unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO remote_gateway_routes VALUES ('remote-test-host', 'codex', 1, ?1)",
                [pinned.then_some(provider.id.as_str())],
            )
            .unwrap();
        }
        crate::proxy::server::ProxyState::for_test(db)
    }

    #[tokio::test]
    async fn remote_pinned_context_disables_failover_and_records_host_source() {
        use crate::app_config::AppType;
        use crate::proxy::handler_context::RequestContext;
        let provider = Provider::with_id(
            "remote-pinned".into(),
            "Pinned".into(),
            serde_json::json!({}),
            None,
        );
        let state = routed_state(&provider, true);
        let mut config = state.db.get_proxy_config_for_app("codex").await.unwrap();
        config.auto_failover_enabled = true;
        state.db.update_proxy_config_for_app(config).await.unwrap();
        let ctx = REMOTE_ORIGIN
            .scope(
                RemoteOrigin {
                    host_key: "remote-test-host".into(),
                },
                async {
                    RequestContext::new(
                        &state,
                        &serde_json::json!({"model": "old-model"}),
                        &HeaderMap::new(),
                        AppType::Codex,
                        "Codex",
                        "codex",
                        None,
                    )
                    .await
                    .unwrap()
                },
            )
            .await;
        assert_eq!(ctx.provider.id, provider.id);
        assert_eq!(ctx.current_provider_id, provider.id);
        assert_eq!(ctx.get_providers().len(), 1);
        assert!(!ctx.app_config.auto_failover_enabled);
        assert_eq!(
            session_source(Some(&ctx.session_id)),
            Some("remote:remote-test-host".into())
        );
    }

    #[tokio::test]
    async fn remote_following_stack_preserves_model_and_refuses_local_login() {
        use crate::app_config::AppType;
        use crate::mode::stack::StackTarget;
        use crate::proxy::handler_context::RequestContext;
        let provider = Provider::with_id(
            "stack-member".into(),
            "Stack member".into(),
            serde_json::json!({}),
            None,
        );
        let state = routed_state(&provider, false);
        for official in [false, true] {
            let mut provider = provider.clone();
            if official {
                provider.category = Some("official".into());
            }
            let target = StackTarget {
                provider,
                upstream_model: "member-model".into(),
                original_model: "ccs-member/member-model".into(),
            };
            let result = REMOTE_ORIGIN
                .scope(
                    RemoteOrigin {
                        host_key: "remote-test-host".into(),
                    },
                    async {
                        RequestContext::new(
                            &state,
                            &serde_json::json!({"model": "member-model"}),
                            &HeaderMap::new(),
                            AppType::Codex,
                            "Codex",
                            "codex",
                            Some(target),
                        )
                        .await
                    },
                )
                .await;
            if official {
                assert!(matches!(
                    result,
                    Err(crate::proxy::ProxyError::AuthError(_))
                ));
            } else {
                let ctx = result.unwrap();
                assert!(ctx.is_stack);
                assert_eq!(ctx.provider.id, "stack-member");
                assert_eq!(ctx.request_model, "ccs-member/member-model");
                assert!(!ctx.app_config.auto_failover_enabled);
            }
        }
    }

    #[tokio::test]
    async fn remote_pinned_host_cannot_bypass_route_with_stack_model() {
        use tower::Service;
        let provider = Provider::with_id(
            "pinned".into(),
            "Pinned".into(),
            serde_json::json!({}),
            None,
        );
        let state = routed_state(&provider, true);
        let server = crate::proxy::server::ProxyServer::new(
            crate::proxy::types::ProxyConfig::default(),
            state.db,
            None,
        );
        let request = http::Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"model":"ccs-other/other-model","input":[]}"#,
            ))
            .unwrap();
        let mut router = server.router();
        let response = REMOTE_ORIGIN
            .scope(
                RemoteOrigin {
                    host_key: "remote-test-host".into(),
                },
                router.call(request),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("固定供应商"));
    }

    /// Claude enabled, Codex disabled, Gemini never enabled, all on one host
    /// token: only Claude gets through the shared tunnel.
    #[tokio::test]
    async fn listener_refuses_apps_not_enabled_for_the_host() {
        let db = std::sync::Arc::new(Database::memory().unwrap());
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch(
                "INSERT INTO remote_gateways VALUES ('h', '{}', 23456, 'tok', 0);
                 INSERT INTO remote_gateway_routes VALUES ('h', 'claude', 1, NULL);
                 INSERT INTO remote_gateway_routes VALUES ('h', 'codex', 0, NULL);",
            )
            .unwrap();
        }
        let router = Router::new().fallback(|| async { "reached" });
        let source: RouterSource = std::sync::Arc::new(move || {
            let router = router.clone();
            Box::pin(async move { Some(router) })
        });
        let listener = RemoteGatewayListener::start(db, source).await.unwrap();
        let client = reqwest::Client::new();
        let send = |path: &str, token: &str| {
            client
                .post(format!("http://127.0.0.1:{}{path}", listener.port))
                .bearer_auth(token)
                .body("{}")
                .send()
        };

        assert_eq!(send("/v1/messages", "tok").await.unwrap().status(), 200);
        assert_eq!(send("/v1/messages", "bad").await.unwrap().status(), 401);
        assert_eq!(send("/v1/responses", "tok").await.unwrap().status(), 403);
        assert_eq!(
            send("/v1beta/models/g:generateContent", "tok")
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(send("/status", "tok").await.unwrap().status(), 404);
    }

    #[test]
    fn remote_route_denies_missing_disabled_and_dangling_pins() {
        let db = Database::memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch(
                "INSERT INTO remote_gateway_routes VALUES ('h', 'claude', 1, NULL);
                 INSERT INTO remote_gateway_routes VALUES ('h', 'codex', 0, NULL);
                 INSERT INTO remote_gateway_routes VALUES ('h', 'gemini', 1, 'gone');",
            )
            .unwrap();
        }
        assert!(matches!(
            remote_route(&db, "h", "claude").unwrap(),
            RemoteRoute::FollowLocal
        ));
        for app in ["codex", "gemini", "grokbuild"] {
            assert!(
                matches!(remote_route(&db, "h", app).unwrap(), RemoteRoute::Denied(_)),
                "{app}"
            );
        }
    }

    #[test]
    fn remote_app_for_maps_client_paths() {
        let app = |path: &str| remote_app_for(&path.parse().unwrap(), &HeaderMap::new());
        assert_eq!(app("/v1/messages"), Some("claude"));
        assert_eq!(app("/v1/responses"), Some("codex"));
        assert_eq!(app("/codex/v1/responses/compact"), Some("codex"));
        assert_eq!(app("/v1/models"), Some("codex"));
        assert_eq!(app("/v1/models?limit=100"), Some("claude"));
        assert_eq!(app("/v1beta/models/x:generateContent"), Some("gemini"));
        assert_eq!(app("/grokbuild/v1/responses"), None);
        assert_eq!(app("/claude-desktop/v1/messages"), None);
        assert_eq!(app("/status"), None);
    }

    /// The remote CLI keeps requesting the model pushed when gateway mode was
    /// enabled; after the route moves to a provider with another model, the
    /// outbound URL must name that provider's model (pinned or following local).
    #[tokio::test]
    async fn remote_gemini_request_uses_the_routed_providers_model() {
        use tower::Service;
        let seen = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let upstream = {
            let seen = seen.clone();
            Router::new().fallback(move |uri: http::Uri| {
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(uri.path().to_string());
                    axum::Json(serde_json::json!({"candidates": []}))
                }
            })
        };
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });

        let db = std::sync::Arc::new(Database::memory().unwrap());
        for (id, model) in [("a", "gemini-a"), ("b", "gemini-b")] {
            let provider = Provider::with_id(
                id.into(),
                id.into(),
                serde_json::json!({ "env": {
                    "GOOGLE_GEMINI_BASE_URL": format!("http://{addr}/{id}"),
                    "GEMINI_API_KEY": "upstream-key",
                    "GEMINI_MODEL": model,
                }}),
                None,
            );
            db.save_provider("gemini", &provider).unwrap();
        }
        db.set_current_provider("gemini", "b").unwrap();
        let server = crate::proxy::server::ProxyServer::new(
            crate::proxy::types::ProxyConfig::default(),
            db.clone(),
            None,
        );

        for pinned in [Some("b"), None] {
            {
                let conn = db.conn.lock().unwrap();
                conn.execute(
                    "INSERT OR REPLACE INTO remote_gateway_routes VALUES ('h', 'gemini', 1, ?1)",
                    [pinned],
                )
                .unwrap();
            }
            let request = http::Request::builder()
                .method("POST")
                .uri("/v1beta/models/gemini-a:generateContent")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"contents":[]}"#))
                .unwrap();
            let mut router = server.router();
            let response = REMOTE_ORIGIN
                .scope(
                    RemoteOrigin {
                        host_key: "h".into(),
                    },
                    router.call(request),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{pinned:?}");
            assert_eq!(
                seen.lock().unwrap().pop().as_deref(),
                Some("/b/v1beta/models/gemini-b:generateContent"),
                "{pinned:?}"
            );
        }
    }

    #[test]
    fn extract_client_token_reads_bearer_api_key_and_query() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer abc".parse().unwrap());
        assert_eq!(extract_client_token(&headers, None).as_deref(), Some("abc"));

        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "def".parse().unwrap());
        assert_eq!(extract_client_token(&headers, None).as_deref(), Some("def"));

        let headers = HeaderMap::new();
        assert_eq!(
            extract_client_token(&headers, Some("alt=sse&key=ghi")).as_deref(),
            Some("ghi")
        );
        assert_eq!(extract_client_token(&headers, Some("alt=sse")), None);
    }

    #[test]
    fn strip_key_query_removes_only_the_key() {
        let uri: http::Uri = "/v1beta/models/x:streamGenerateContent?alt=sse&key=secret"
            .parse()
            .unwrap();
        assert_eq!(
            strip_key_query(&uri).unwrap().to_string(),
            "/v1beta/models/x:streamGenerateContent?alt=sse"
        );
        let uri: http::Uri = "/v1beta/models?key=secret".parse().unwrap();
        assert_eq!(strip_key_query(&uri).unwrap().to_string(), "/v1beta/models");
        let uri: http::Uri = "/v1/messages".parse().unwrap();
        assert!(strip_key_query(&uri).is_none());
    }

    #[test]
    fn session_sources_evict_oldest_beyond_cap() {
        remember_session_source("remote-test-first", "remote:a".to_string());
        assert_eq!(
            session_source(Some("remote-test-first")).as_deref(),
            Some("remote:a")
        );
        for i in 0..SESSION_SOURCES_CAP {
            remember_session_source(&format!("remote-test-{i}"), "remote:b".to_string());
        }
        assert_eq!(session_source(Some("remote-test-first")), None);
    }
}
