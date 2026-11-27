//! Lifecycle manager and Axum router for the opt-in LAN Mobile Web UI server.

use super::agent_remote::{sanitize_remote_control_apps, AgentRunManager};
use super::config::{
    generate_token, load_config, save_config, validate_port, LanWebConfig, LanWebMode,
};
use super::handlers::{self, LanWebRouterState};
use super::net::lan_ipv4_hosts;
use super::security::{origin_guard, security_headers};
use crate::store::AppState;
use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;

const MAX_LAN_WEB_BODY_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanWebStatus {
    pub mode: LanWebMode,
    pub port: u16,
    pub allow_control: bool,
    pub remote_control_apps: Vec<String>,
    pub installed_remote_apps: Vec<String>,
    pub running: bool,
    pub listen_address: Option<String>,
    pub local_url: Option<String>,
    pub lan_urls: Vec<String>,
    pub lan_hosts: Vec<String>,
    pub token: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanWebConfigUpdate {
    pub mode: LanWebMode,
    pub port: u16,
    pub allow_control: bool,
    #[serde(default)]
    pub remote_control_apps: Vec<String>,
}

struct RunningServer {
    listen_address: String,
    shutdown_tx: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

#[derive(Clone)]
pub struct LanWebService {
    config: Arc<RwLock<LanWebConfig>>,
    running: Arc<Mutex<Option<RunningServer>>>,
    config_op_lock: Arc<Mutex<()>>,
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    last_error: Arc<RwLock<Option<String>>>,
    remote_runs: Arc<AgentRunManager>,
}

impl Default for LanWebService {
    fn default() -> Self {
        Self::new()
    }
}

impl LanWebService {
    pub fn new() -> Self {
        let mut initial = load_config().unwrap_or_default();
        initial.remote_control_apps = sanitize_remote_control_apps(&initial.remote_control_apps);
        Self {
            config: Arc::new(RwLock::new(initial)),
            running: Arc::new(Mutex::new(None)),
            config_op_lock: Arc::new(Mutex::new(())),
            app_handle: Arc::new(RwLock::new(None)),
            last_error: Arc::new(RwLock::new(None)),
            remote_runs: Arc::new(AgentRunManager::new()),
        }
    }

    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        futures::executor::block_on(async {
            *self.app_handle.write().await = Some(handle);
        });
    }

    /// Start the LAN web server at app startup if the persisted mode is `local` or `lan`.
    pub async fn start_if_configured(&self, app_state: AppState) {
        let cfg = match load_config() {
            Ok(mut c) => {
                c.remote_control_apps = sanitize_remote_control_apps(&c.remote_control_apps);
                if c.mode != LanWebMode::Off && c.token.is_empty() {
                    c.token = generate_token();
                    let _ = save_config(&c);
                }
                c
            }
            Err(err) => {
                log::warn!("[LanWeb] failed to load lan-web.json: {err}");
                *self.last_error.write().await = Some(err);
                return;
            }
        };

        *self.config.write().await = cfg.clone();

        if cfg.mode == LanWebMode::Off {
            return;
        }

        if let Err(err) = self.start_listener(app_state, &cfg).await {
            log::warn!("[LanWeb] failed to start on startup: {err}");
            *self.last_error.write().await = Some(err);
        }
    }

    /// Update mode/port/allow_control, persist to `0600` `lan-web.json`, and
    /// reconcile the running listener cleanly.
    pub async fn update_config(
        &self,
        app_state: AppState,
        update: LanWebConfigUpdate,
    ) -> Result<LanWebStatus, String> {
        let _op_guard = self.config_op_lock.lock().await;

        // Skip proxy-port collision check when turning LAN Web off so the user can
        // always disable it even if proxy listen_port was changed to match.
        if update.mode != LanWebMode::Off {
            let proxy_port = app_state
                .db
                .get_global_proxy_config()
                .await
                .ok()
                .map(|c| c.listen_port);
            validate_port(update.port, proxy_port)?;
        }

        let prev = self.config.read().await.clone();
        let mut next = prev.clone();
        next.mode = update.mode;
        if update.mode != LanWebMode::Off || update.port >= 1024 {
            next.port = update.port;
        }
        next.allow_control = update.allow_control;
        next.remote_control_apps = sanitize_remote_control_apps(&update.remote_control_apps);
        if next.mode != LanWebMode::Off && next.token.is_empty() {
            next.token = generate_token();
        }

        save_config(&next)?;
        *self.config.write().await = next.clone();

        // Avoid tearing down and rebinding the TCP listener when only control flags changed.
        let listener_running = self.running.lock().await.is_some();
        let bind_unchanged =
            prev.mode == next.mode && prev.effective_port() == next.effective_port();
        if bind_unchanged && next.mode != LanWebMode::Off && listener_running {
            *self.last_error.write().await = None;
            return Ok(self.get_status().await);
        }

        self.stop().await;

        if next.mode == LanWebMode::Off {
            *self.last_error.write().await = None;
        } else if let Err(err) = self.start_listener(app_state, &next).await {
            *self.last_error.write().await = Some(err.clone());
            return Err(err);
        } else {
            *self.last_error.write().await = None;
        }

        Ok(self.get_status().await)
    }

    /// Rotate the 256-bit access token, persisting to `0600` `lan-web.json` and
    /// immediately invalidating all existing session cookies without dropping TCP.
    pub async fn regenerate_token(&self) -> Result<LanWebStatus, String> {
        let _op_guard = self.config_op_lock.lock().await;
        let mut next = self.config.read().await.clone();
        next.token = generate_token();
        save_config(&next)?;
        *self.config.write().await = next;
        Ok(self.get_status().await)
    }

    pub async fn get_status(&self) -> LanWebStatus {
        let cfg = self.config.read().await.clone();
        let running_guard = self.running.lock().await;
        let (running, listen_address) = match running_guard.as_ref() {
            Some(srv) => (true, Some(srv.listen_address.clone())),
            None => (false, None),
        };
        drop(running_guard);

        let error = self.last_error.read().await.clone();
        let port = cfg.effective_port();
        let lan_hosts = lan_ipv4_hosts();

        let (local_url, lan_urls) = if cfg.mode != LanWebMode::Off && !cfg.token.is_empty() {
            let frag = format!("/#t={}", cfg.token);
            let local = Some(format!("http://127.0.0.1:{port}{frag}"));
            let lans = if cfg.mode == LanWebMode::Lan {
                lan_hosts
                    .iter()
                    .map(|ip| format!("http://{ip}:{port}{frag}"))
                    .collect()
            } else {
                Vec::new()
            };
            (local, lans)
        } else {
            (None, Vec::new())
        };

        let installed_remote_apps = self.remote_runs.installed_agent_ids().await;

        LanWebStatus {
            mode: cfg.mode,
            port,
            allow_control: cfg.allow_control,
            remote_control_apps: cfg.remote_control_apps,
            installed_remote_apps,
            running,
            listen_address,
            local_url,
            lan_urls,
            lan_hosts,
            token: cfg.token,
            error,
        }
    }

    pub async fn stop(&self) {
        self.remote_runs.cancel_all().await;
        let active = self.running.lock().await.take();
        if let Some(srv) = active {
            let _ = srv.shutdown_tx.send(());
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), srv.handle).await;
            log::info!("[LanWeb] server stopped ({})", srv.listen_address);
        }
    }

    /// Graceful shutdown hook invoked on app exit: kills all active remote agent
    /// child processes and stops the HTTP listener.
    pub async fn shutdown(&self) {
        self.remote_runs.cancel_all().await;
        self.stop().await;
    }

    async fn start_listener(&self, app_state: AppState, cfg: &LanWebConfig) -> Result<(), String> {
        let Some(host) = cfg.mode.bind_host() else {
            return Ok(());
        };
        let port = cfg.effective_port();
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| format!("Invalid LAN web bind address {host}:{port}: {e}"))?;

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind LAN web server on {addr}: {e}"))?;
        let bound_addr = listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| addr.to_string());

        let handle_opt = self.app_handle.read().await.clone();
        let router_state = LanWebRouterState {
            app_state,
            config: self.config.clone(),
            app_handle: handle_opt,
            dist_dir_override: None,
            remote_runs: self.remote_runs.clone(),
        };
        let router = build_router(router_state);

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let join_handle = tokio::spawn(async move {
            let server = axum::serve(listener, router).with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            });
            if let Err(err) = server.await {
                log::warn!("[LanWeb] server exited with error: {err}");
            }
        });

        *self.running.lock().await = Some(RunningServer {
            listen_address: bound_addr.clone(),
            shutdown_tx,
            handle: join_handle,
        });
        log::info!("[LanWeb] listening on http://{bound_addr}");
        Ok(())
    }
}

pub fn build_router(state: LanWebRouterState) -> Router {
    Router::new()
        // Static Mobile SPA + PWA files
        .route("/", get(handlers::serve_mobile_index))
        .route("/mobile.html", get(handlers::serve_mobile_index))
        .route("/assets/*path", get(handlers::serve_static_asset))
        .route("/manifest.json", get(handlers::serve_manifest))
        .route("/icons/apple-touch-icon.png", get(handlers::serve_app_icon))
        .route("/icons/icon-256.png", get(handlers::serve_app_icon))
        // Auth endpoints
        .route("/api/auth/status", get(handlers::get_auth_status))
        .route("/api/auth/session", post(handlers::create_session))
        .route("/api/auth/logout", post(handlers::logout_session))
        // Protected Monitoring & Optional Control API
        .route("/api/overview", get(handlers::get_overview))
        .route("/api/providers", get(handlers::get_providers))
        .route(
            "/api/providers/:app/:id/switch",
            post(handlers::switch_provider),
        )
        .route("/api/logs", get(handlers::get_logs))
        .route("/api/proxy/status", get(handlers::get_proxy_status))
        .route("/api/proxy/start", post(handlers::start_proxy))
        .route("/api/proxy/stop", post(handlers::stop_proxy))
        // Unified Multi-Agent Remote Control API
        .route("/api/remote/agents", get(handlers::get_remote_agents))
        .route("/api/remote/sessions", get(handlers::get_remote_sessions))
        .route(
            "/api/remote/sessions/:app/:session_id/messages",
            get(handlers::get_remote_session_messages),
        )
        .route(
            "/api/remote/runs",
            get(handlers::get_remote_runs).post(handlers::start_remote_run),
        )
        .route("/api/remote/runs/:run_id", get(handlers::get_remote_run))
        .route(
            "/api/remote/runs/:run_id/stream",
            get(handlers::stream_remote_run),
        )
        .route(
            "/api/remote/runs/:run_id/cancel",
            post(handlers::cancel_remote_run),
        )
        .layer(DefaultBodyLimit::max(MAX_LAN_WEB_BODY_BYTES))
        .layer(middleware::from_fn(origin_guard))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

#[cfg(test)]
pub(crate) fn build_test_router(
    app_state: AppState,
    config: Arc<RwLock<LanWebConfig>>,
    dist_dir_override: Option<PathBuf>,
) -> Router {
    build_test_router_with_remote(
        app_state,
        config,
        dist_dir_override,
        Arc::new(AgentRunManager::new()),
    )
}

#[cfg(test)]
pub(crate) fn build_test_router_with_remote(
    app_state: AppState,
    config: Arc<RwLock<LanWebConfig>>,
    dist_dir_override: Option<PathBuf>,
    remote_runs: Arc<AgentRunManager>,
) -> Router {
    build_router(LanWebRouterState {
        app_state,
        config,
        app_handle: None,
        dist_dir_override,
        remote_runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::lan_web::config::{derive_session_cookie, SESSION_COOKIE_NAME};
    use crate::provider::Provider;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request, StatusCode};
    use serde_json::{json, Value};
    use tempfile::tempdir;
    use tower::Service;

    async fn oneshot(mut router: Router, req: Request<Body>) -> axum::response::Response {
        std::future::poll_fn(|cx| Service::<Request<Body>>::poll_ready(&mut router, cx))
            .await
            .expect("router ready");
        Service::<Request<Body>>::call(&mut router, req)
            .await
            .expect("router call")
    }

    fn test_app_state_with_secret_provider() -> AppState {
        let db = Arc::new(Database::memory().expect("in-memory db"));
        let secret_provider = Provider::with_id(
            "prov-1".to_string(),
            "Anthropic Main".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-ant-super-secret-value-12345",
                    "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
                }
            }),
            Some("https://anthropic.com".to_string()),
        );
        db.save_provider(&AppType::Claude, &secret_provider)
            .expect("save provider");
        db.set_current_provider(&AppType::Claude, "prov-1")
            .expect("set current");
        AppState::new(db)
    }

    async fn body_json(resp: axum::response::Response) -> Value {
        let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn unauthenticated_protected_endpoints_return_401_and_security_headers() {
        let app_state = test_app_state_with_secret_provider();
        let token = generate_token();
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15722,
            allow_control: false,
            remote_control_apps: Vec::new(),
            token,
        }));
        let router = build_test_router(app_state, cfg, None);

        for path in [
            "/api/overview",
            "/api/providers",
            "/api/logs",
            "/api/proxy/status",
            "/api/remote/agents",
            "/api/remote/sessions",
            "/api/remote/runs",
        ] {
            let resp = oneshot(
                router.clone(),
                Request::builder()
                    .uri(path)
                    .header(header::HOST, "192.168.1.5:15722")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "path {path}");
            assert_eq!(
                resp.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
                "nosniff"
            );
            assert!(resp.headers().contains_key(header::CONTENT_SECURITY_POLICY));
            assert_eq!(
                resp.headers().get(header::REFERRER_POLICY).unwrap(),
                "no-referrer"
            );
        }
    }

    #[tokio::test]
    async fn session_exchange_sets_httponly_strict_cookie_and_strips_provider_secrets() {
        let app_state = test_app_state_with_secret_provider();
        let token = generate_token();
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15722,
            allow_control: false,
            remote_control_apps: Vec::new(),
            token: token.clone(),
        }));
        let router = build_test_router(app_state, cfg.clone(), None);

        // Wrong token -> 401
        let bad = oneshot(
            router.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/auth/session")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "token": "wrong" }).to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);

        // Valid token -> 200 + HttpOnly SameSite=Strict cookie
        let ok = oneshot(
            router.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/auth/session")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "token": token }).to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(ok.status(), StatusCode::OK);
        let set_cookie = ok
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
        let cookie_pair = set_cookie.split(';').next().unwrap().to_string();

        // Authenticated /api/providers must NEVER leak ANTHROPIC_AUTH_TOKEN or settingsConfig
        let prov_resp = oneshot(
            router.clone(),
            Request::builder()
                .uri("/api/providers")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(prov_resp.status(), StatusCode::OK);
        let raw_bytes = to_bytes(prov_resp.into_body(), 1024 * 1024).await.unwrap();
        let raw_text = String::from_utf8(raw_bytes.to_vec()).unwrap();
        assert!(!raw_text.contains("sk-ant-super-secret-value-12345"));
        assert!(!raw_text.contains("settingsConfig"));
        assert!(!raw_text.contains("ANTHROPIC_AUTH_TOKEN"));
        let parsed: Value = serde_json::from_str(&raw_text).unwrap();
        assert_eq!(parsed["apps"][0]["providers"][0]["name"], "Anthropic Main");
        assert_eq!(parsed["apps"][0]["providers"][0]["isCurrent"], true);

        // Rotating token immediately invalidates the old cookie
        cfg.write().await.token = generate_token();
        let stale = oneshot(
            router.clone(),
            Request::builder()
                .uri("/api/providers")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(stale.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn origin_and_sec_fetch_site_reject_cross_origin_requests() {
        let app_state = test_app_state_with_secret_provider();
        let token = generate_token();
        let cookie = format!("{SESSION_COOKIE_NAME}={}", derive_session_cookie(&token));
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15722,
            allow_control: true,
            remote_control_apps: Vec::new(),
            token,
        }));
        let router = build_test_router(app_state, cfg, None);

        let evil_origin = oneshot(
            router.clone(),
            Request::builder()
                .uri("/api/overview")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::ORIGIN, "https://attacker.test")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(evil_origin.status(), StatusCode::FORBIDDEN);

        let null_origin = oneshot(
            router.clone(),
            Request::builder()
                .uri("/api/overview")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::ORIGIN, "null")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(null_origin.status(), StatusCode::FORBIDDEN);

        let cross_site = oneshot(
            router.clone(),
            Request::builder()
                .uri("/api/overview")
                .header(header::HOST, "192.168.1.5:15722")
                .header("sec-fetch-site", "cross-site")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(cross_site.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn mutations_require_allow_control_flag() {
        let app_state = test_app_state_with_secret_provider();
        let token = generate_token();
        let cookie = format!("{SESSION_COOKIE_NAME}={}", derive_session_cookie(&token));
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15722,
            allow_control: false,
            remote_control_apps: Vec::new(),
            token,
        }));
        let router = build_test_router(app_state, cfg, None);

        for uri in [
            "/api/providers/claude/prov-1/switch",
            "/api/proxy/start",
            "/api/proxy/stop",
        ] {
            let resp = oneshot(
                router.clone(),
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(header::HOST, "192.168.1.5:15722")
                    .header(header::ORIGIN, "http://192.168.1.5:15722")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "read-only mode must reject {uri}"
            );
        }
    }

    #[tokio::test]
    async fn static_pwa_assets_and_dist_serving_work_safely() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mobile.html"), "<html>ok-mobile</html>").unwrap();
        std::fs::create_dir_all(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("assets/mobile.js"), "console.log(1);").unwrap();

        let app_state = test_app_state_with_secret_provider();
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Local,
            port: 15722,
            allow_control: false,
            remote_control_apps: Vec::new(),
            token: generate_token(),
        }));
        let router = build_test_router(app_state, cfg, Some(dir.path().to_path_buf()));

        // Manifest & icon bypass auth cleanly
        let manifest = oneshot(
            router.clone(),
            Request::builder()
                .uri("/manifest.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(manifest.status(), StatusCode::OK);
        let manifest_json = body_json(manifest).await;
        assert_eq!(manifest_json["display"], "standalone");

        let icon = oneshot(
            router.clone(),
            Request::builder()
                .uri("/icons/apple-touch-icon.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(icon.status(), StatusCode::OK);

        // Mobile HTML & JS chunk served
        let html = oneshot(
            router.clone(),
            Request::builder().uri("/").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(html.status(), StatusCode::OK);
        let html_bytes = to_bytes(html.into_body(), 1024 * 1024).await.unwrap();
        assert!(String::from_utf8_lossy(&html_bytes).contains("ok-mobile"));

        let js = oneshot(
            router.clone(),
            Request::builder()
                .uri("/assets/mobile.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(js.status(), StatusCode::OK);

        // Path traversal is rejected with 404
        let traversal = oneshot(
            router.clone(),
            Request::builder()
                .uri("/assets/..%2Fmobile.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(traversal.status(), StatusCode::NOT_FOUND);

        // Symlink escaping dist/ is also rejected with 404
        #[cfg(unix)]
        {
            let outside = tempdir().unwrap();
            let secret_file = outside.path().join("secret.txt");
            std::fs::write(&secret_file, "top-secret").unwrap();
            std::os::unix::fs::symlink(&secret_file, dir.path().join("assets/escape.txt")).unwrap();
            let escaped = oneshot(
                router.clone(),
                Request::builder()
                    .uri("/assets/escape.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(escaped.status(), StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn proxy_status_and_overview_serialize_camel_case_keys_and_no_store_header() {
        let app_state = test_app_state_with_secret_provider();
        let token = generate_token();
        let cookie = format!("{SESSION_COOKIE_NAME}={}", derive_session_cookie(&token));
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15722,
            allow_control: false,
            remote_control_apps: Vec::new(),
            token,
        }));
        let router = build_test_router(app_state, cfg, None);

        let proxy_resp = oneshot(
            router.clone(),
            Request::builder()
                .uri("/api/proxy/status")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(proxy_resp.status(), StatusCode::OK);
        assert_eq!(
            proxy_resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let proxy_json = body_json(proxy_resp).await;
        let status_obj = proxy_json["status"].as_object().expect("status object");
        for key in [
            "running",
            "uptimeSeconds",
            "totalRequests",
            "failoverCount",
            "activeTargets",
        ] {
            assert!(
                status_obj.contains_key(key),
                "expected camelCase key {key} in /api/proxy/status"
            );
        }
        assert!(status_obj["activeTargets"].is_array());
        assert!(!status_obj.contains_key("active_targets"));

        let ov_resp = oneshot(
            router.clone(),
            Request::builder()
                .uri("/api/overview?days=7")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(ov_resp.status(), StatusCode::OK);
        assert_eq!(
            ov_resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let ov_json = body_json(ov_resp).await;
        assert!(ov_json["proxyStatus"]["activeTargets"].is_array());
        assert!(ov_json["proxyStatus"].get("uptimeSeconds").is_some());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn stop_proxy_refuses_while_takeover_is_active() {
        let _home = crate::test_support::TestHome::new();
        let app_state = test_app_state_with_secret_provider();
        crate::mode::state::update(&crate::live::engine::DeviceStore::for_device(), |live| {
            live.apps
                .entry("claude".to_string())
                .or_default()
                .set_mode_state(crate::mode::state::ModeState {
                    mode: Some(crate::mode::state::Mode::Proxy),
                    ..Default::default()
                });
        })
        .expect("enable claude takeover mode");
        let token = generate_token();
        let cookie = format!("{SESSION_COOKIE_NAME}={}", derive_session_cookie(&token));
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15722,
            allow_control: true,
            remote_control_apps: Vec::new(),
            token,
        }));
        let router = build_test_router(app_state, cfg, None);

        let resp = oneshot(
            router.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/proxy/stop")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::ORIGIN, "http://192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert!(body["error"].as_str().unwrap_or("").contains("takeover"));
    }

    #[cfg(unix)]
    fn write_mock_script(dir: &std::path::Path, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, script).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_per_agent_gating_and_input_validation() {
        let dir = tempdir().unwrap();
        let mock_agy = write_mock_script(
            dir.path(),
            "mock-agy",
            "#!/bin/sh\necho '{\"init\":{\"conversation_id\":\"conv-agy-1\"}}'\necho '{\"step_update\":{\"text\":\"ok\"}}'\n",
        );
        let mock_claude = write_mock_script(
            dir.path(),
            "mock-claude",
            "#!/bin/sh\necho '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s1\"}'\n",
        );

        let mut overrides = std::collections::HashMap::new();
        overrides.insert("agy".to_string(), mock_agy);
        overrides.insert("claude".to_string(), mock_claude);
        let manager = Arc::new(AgentRunManager::with_bin_overrides(overrides));

        let app_state = test_app_state_with_secret_provider();
        let token = generate_token();
        let cookie = format!("{SESSION_COOKIE_NAME}={}", derive_session_cookie(&token));
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15722,
            allow_control: false,
            // Only `agy` is enabled; `claude` is disabled!
            remote_control_apps: vec!["agy".to_string()],
            token,
        }));
        let router = build_test_router_with_remote(app_state, cfg, None, manager);

        // GET /api/remote/agents reports installed & enabled flags accurately
        let agents_resp = oneshot(
            router.clone(),
            Request::builder()
                .uri("/api/remote/agents")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(agents_resp.status(), StatusCode::OK);
        assert_eq!(
            agents_resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let agents_json = body_json(agents_resp).await;
        let agents = agents_json["agents"].as_array().unwrap();
        assert_eq!(agents.len(), 10);
        let agy_info = agents.iter().find(|a| a["id"] == "agy").unwrap();
        assert_eq!(agy_info["installed"], true);
        assert_eq!(agy_info["enabled"], true);
        let claude_info = agents.iter().find(|a| a["id"] == "claude").unwrap();
        assert_eq!(claude_info["installed"], true);
        assert_eq!(claude_info["enabled"], false);

        // GET /api/remote/sessions succeeds even for disabled agents (read-only)
        let sessions_resp = oneshot(
            router.clone(),
            Request::builder()
                .uri("/api/remote/sessions?app=claude")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(sessions_resp.status(), StatusCode::OK);

        // POST /api/remote/runs for disabled `claude` -> 403 Forbidden
        let forbidden = oneshot(
            router.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/remote/runs")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::ORIGIN, "http://192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "app": "claude", "prompt": "hi", "cwd": dir.path() }).to_string(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        // Validation rejections -> 400 Bad Request
        for bad_payload in [
            json!({ "app": "unknown-agent", "prompt": "hi" }),
            json!({ "app": "agy", "prompt": "   " }),
            json!({ "app": "agy", "prompt": "hi", "mode": "dangerously-skip" }),
            json!({ "app": "agy", "prompt": "hi", "effort": "ultra" }),
            json!({ "app": "agy", "prompt": "hi", "model": "--dangerously-skip-permissions" }),
            json!({ "app": "agy", "prompt": "hi", "sessionId": "--gemini_dir=/tmp" }),
            json!({ "app": "agy", "prompt": "hi", "cwd": "/definitely/does/not/exist/ccswitch-test" }),
        ] {
            let bad = oneshot(
                router.clone(),
                Request::builder()
                    .method("POST")
                    .uri("/api/remote/runs")
                    .header(header::HOST, "192.168.1.5:15722")
                    .header(header::ORIGIN, "http://192.168.1.5:15722")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(bad_payload.to_string()))
                    .unwrap(),
            )
            .await;
            assert_eq!(
                bad.status(),
                StatusCode::BAD_REQUEST,
                "expected 400 for payload {bad_payload}"
            );
        }

        // POST /api/remote/runs for enabled `agy` -> 202 Accepted
        let ok = oneshot(
            router.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/remote/runs")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::ORIGIN, "http://192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "app": "agy",
                        "prompt": "hello",
                        "cwd": dir.path(),
                        "mode": "plan",
                        "effort": "high",
                        "sandbox": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(ok.status(), StatusCode::ACCEPTED);
        let run_json = body_json(ok).await;
        assert_eq!(run_json["app"], "agy");
        assert!(run_json["runId"].as_str().is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_streaming_sse_polling_session_capture_and_redaction() {
        let dir = tempdir().unwrap();
        let mock_agy = write_mock_script(
            dir.path(),
            "mock-agy",
            r#"#!/bin/sh
echo '{"init":{"conversation_id":"conv-live-42"}}'
echo '{"step_update":{"thought":"Checking repository"}}'
echo '{"step_update":{"tool_call":{"id":"tc1","name":"read_file","input":{"path":"README.md"}}}}'
echo '{"step_update":{"tool_result":{"call_id":"tc1","name":"read_file","output":"ok"}}}'
echo '{"step_update":{"text":"Done! GEMINI_API_KEY=sk-secret-leak-99999"}}'
echo 'stderr line GEMINI_API_KEY=sk-stderr-secret-88888' >&2
echo '{"result":{"conversation_id":"conv-live-42","text":"Done!"}}'
"#,
        );

        let mut overrides = std::collections::HashMap::new();
        overrides.insert("agy".to_string(), mock_agy);
        let manager = Arc::new(AgentRunManager::with_bin_overrides(overrides));

        let app_state = test_app_state_with_secret_provider();
        let token = generate_token();
        let cookie = format!("{SESSION_COOKIE_NAME}={}", derive_session_cookie(&token));
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15722,
            allow_control: false,
            remote_control_apps: vec!["agy".to_string()],
            token,
        }));
        let router = build_test_router_with_remote(app_state, cfg, None, manager);

        let start = oneshot(
            router.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/remote/runs")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::ORIGIN, "http://192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "app": "agy", "prompt": "check", "cwd": dir.path() }).to_string(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(start.status(), StatusCode::ACCEPTED);
        let run_id = body_json(start).await["runId"]
            .as_str()
            .unwrap()
            .to_string();

        // Read SSE stream to completion
        let sse_resp = oneshot(
            router.clone(),
            Request::builder()
                .uri(format!("/api/remote/runs/{run_id}/stream?after=0"))
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(sse_resp.status(), StatusCode::OK);
        assert!(sse_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("text/event-stream"));
        let sse_bytes = to_bytes(sse_resp.into_body(), 1024 * 1024).await.unwrap();
        let sse_text = String::from_utf8(sse_bytes.to_vec()).unwrap();
        assert!(sse_text.contains("event: init"));
        assert!(sse_text.contains("event: thought"));
        assert!(sse_text.contains("event: tool_call"));
        assert!(sse_text.contains("event: tool_result"));
        assert!(sse_text.contains("event: text"));
        assert!(sse_text.contains("event: completed"));
        assert!(sse_text.contains("conv-live-42"));
        assert!(!sse_text.contains("sk-secret-leak-99999"));
        assert!(!sse_text.contains("sk-stderr-secret-88888"));
        assert!(sse_text.contains("[REDACTED]"));

        // Polling fallback with `?after=<seq>` returns only subsequent events
        let poll_all = oneshot(
            router.clone(),
            Request::builder()
                .uri(format!("/api/remote/runs/{run_id}?after=0"))
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(poll_all.status(), StatusCode::OK);
        let detail = body_json(poll_all).await;
        assert_eq!(detail["run"]["status"], "completed");
        assert_eq!(detail["run"]["conversationId"], "conv-live-42");
        let events = detail["events"].as_array().unwrap();
        assert!(events.len() >= 6);
        let mid_seq = events[2]["seq"].as_u64().unwrap();

        let poll_after = oneshot(
            router.clone(),
            Request::builder()
                .uri(format!("/api/remote/runs/{run_id}?after={mid_seq}"))
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let detail_after = body_json(poll_after).await;
        let tail_events = detail_after["events"].as_array().unwrap();
        assert!(!tail_events.is_empty());
        assert!(tail_events
            .iter()
            .all(|e| e["seq"].as_u64().unwrap() > mid_seq));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_cancel_kills_running_process_and_emits_cancelled() {
        let dir = tempdir().unwrap();
        let mock_claude = write_mock_script(
            dir.path(),
            "mock-claude",
            "#!/bin/sh\necho '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s-cancel\"}'\nsleep 30\n",
        );

        let mut overrides = std::collections::HashMap::new();
        overrides.insert("claude".to_string(), mock_claude);
        let manager = Arc::new(AgentRunManager::with_bin_overrides(overrides));

        let app_state = test_app_state_with_secret_provider();
        let token = generate_token();
        let cookie = format!("{SESSION_COOKIE_NAME}={}", derive_session_cookie(&token));
        let cfg = Arc::new(RwLock::new(LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15722,
            allow_control: false,
            remote_control_apps: vec!["claude".to_string()],
            token,
        }));
        let router = build_test_router_with_remote(app_state, cfg, None, manager);

        let start = oneshot(
            router.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/remote/runs")
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::ORIGIN, "http://192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "app": "claude", "prompt": "long task", "cwd": dir.path() })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(start.status(), StatusCode::ACCEPTED);
        let run_id = body_json(start).await["runId"]
            .as_str()
            .unwrap()
            .to_string();

        tokio::time::sleep(std::time::Duration::from_millis(80)).await;

        let cancel = oneshot(
            router.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/remote/runs/{run_id}/cancel"))
                .header(header::HOST, "192.168.1.5:15722")
                .header(header::ORIGIN, "http://192.168.1.5:15722")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(cancel.status(), StatusCode::OK);
        let snap = body_json(cancel).await;
        assert_eq!(snap["status"], "cancelled");

        let detail = body_json(
            oneshot(
                router.clone(),
                Request::builder()
                    .uri(format!("/api/remote/runs/{run_id}?after=0"))
                    .header(header::HOST, "192.168.1.5:15722")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await,
        )
        .await;
        let kinds: Vec<&str> = detail["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["kind"].as_str())
            .collect();
        assert!(kinds.contains(&"cancelled"));
    }
}
