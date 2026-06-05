use crate::app_config::{AppType, McpApps, McpServer, SkillApps};
use crate::database::Database;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::provider::{Provider, UniversalProvider};
use crate::proxy::circuit_breaker::CircuitBreakerConfig;
use crate::proxy::types::{
    AppProxyConfig, CopilotOptimizerConfig, GlobalProxyConfig, LogConfig, OptimizerConfig,
    ProxyConfig, ProxyServerInfo, RectifierConfig,
};
use crate::services::skill::{DiscoverableSkill, SkillRepo, SkillService};
use crate::services::usage_stats::LogFilters;
use crate::services::{McpService, ProviderService, ProxyService, UsageCache};
use crate::settings::ManagementApiSettings;
use crate::store::AppState;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tower_http::cors::{AllowOrigin, CorsLayer};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

const ALL_SCOPES: &[&str] = &[
    "providers:read",
    "providers:write",
    "providers:switch",
    "universal:read",
    "universal:write",
    "universal:sync",
    "mcp:read",
    "mcp:write",
    "mcp:sync",
    "prompts:read",
    "prompts:write",
    "skills:read",
    "skills:write",
    "skills:update",
    "proxy:read",
    "proxy:control",
    "proxy:config",
    "usage:read",
    "usage:write",
    "sessions:read",
    "sessions:delete",
    "workspace:read",
    "workspace:write",
    "settings:read",
    "settings:write",
    "events:read",
    "auth:admin",
    "api:read",
    "secrets:read",
];

const MAX_PAIRING_CLIENT_NAME_LEN: usize = 80;
const MAX_RECENT_PAIRING_REQUESTS: i64 = 20;
const PAIRING_RATE_LIMIT_WINDOW_MILLIS: i64 = 10 * 60 * 1000;

#[derive(Clone)]
pub struct ManagementApiService {
    db: Arc<Database>,
    proxy_service: ProxyService,
    token_secret: Arc<Vec<u8>>,
    running: Arc<RwLock<Option<RunningServer>>>,
}

struct RunningServer {
    info: ProxyServerInfo,
    settings: ManagementApiSettings,
    shutdown_tx: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagementApiStatus {
    pub enabled: bool,
    pub running: bool,
    pub address: String,
    pub port: u16,
    pub base_url: String,
    pub lan_enabled: bool,
    pub tls_enabled: bool,
    pub token_count: i64,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiTokenRequest {
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiTokenResponse {
    pub token: String,
    pub record: crate::database::ApiTokenRecord,
}

#[derive(Clone)]
struct HttpState {
    db: Arc<Database>,
    proxy_service: ProxyService,
    token_secret: Arc<Vec<u8>>,
    settings: ManagementApiSettings,
}

#[derive(Debug, Clone)]
struct AuthContext {
    token_id: String,
    scopes: Vec<String>,
    expires_at: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ApiEnvelope<T: Serialize> {
    data: T,
    meta: ApiMeta,
}

#[derive(Debug, Serialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
    meta: ApiMeta,
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiMeta {
    request_id: String,
}

impl ManagementApiService {
    pub fn new(db: Arc<Database>, proxy_service: ProxyService) -> Self {
        Self {
            db,
            proxy_service,
            token_secret: Arc::new(load_or_create_secret()),
            running: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn reconcile_with_settings(&self) -> Result<ManagementApiStatus, String> {
        let settings = crate::settings::get_settings().management_api;
        if settings.enabled {
            self.start(settings).await?;
        } else {
            let _ = self.stop().await;
        }
        self.status().await.map_err(|e| e.to_string())
    }

    pub async fn start(
        &self,
        mut settings: ManagementApiSettings,
    ) -> Result<ProxyServerInfo, String> {
        settings.normalize();
        settings.validate_for_start().map_err(|e| e.to_string())?;
        if settings.lan_enabled
            && self
                .db
                .active_api_token_count()
                .map_err(|e| e.to_string())?
                < 1
        {
            return Err("LAN Management API mode requires at least one active token".to_string());
        }
        if let Some(existing) = self.running.read().await.as_ref() {
            if existing.settings == settings {
                return Ok(existing.info.clone());
            }
        }
        let _ = self.stop().await;

        let addr: SocketAddr = format!("{}:{}", settings.listen_address, settings.port)
            .parse()
            .map_err(|e| format!("Invalid Management API listen address: {e}"))?;
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind Management API: {e}"))?;

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let state = HttpState {
            db: self.db.clone(),
            proxy_service: self.proxy_service.clone(),
            token_secret: self.token_secret.clone(),
            settings: settings.clone(),
        };
        let app = build_router(state, &settings);
        let handle = tokio::spawn(async move {
            let service = app.into_make_service_with_connect_info::<SocketAddr>();
            if let Err(e) = axum::serve(listener, service)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
            {
                log::error!("Management API server exited with error: {e}");
            }
        });

        let info = ProxyServerInfo {
            address: settings.listen_address.clone(),
            port: settings.port,
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        *self.running.write().await = Some(RunningServer {
            info: info.clone(),
            settings,
            shutdown_tx,
            handle,
        });
        log::info!("Management API started on {}:{}", info.address, info.port);
        Ok(info)
    }

    pub async fn stop(&self) -> Result<(), String> {
        if let Some(running) = self.running.write().await.take() {
            let _ = running.shutdown_tx.send(());
            match tokio::time::timeout(std::time::Duration::from_secs(5), running.handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(format!("Management API task failed: {e}")),
                Err(_) => return Err("Management API stop timed out".to_string()),
            }
            log::info!("Management API stopped");
        }
        Ok(())
    }

    pub async fn status(&self) -> Result<ManagementApiStatus, AppError> {
        let settings = crate::settings::get_settings().management_api;
        let running = self.running.read().await;
        let (is_running, started_at) = running
            .as_ref()
            .map(|server| (true, Some(server.info.started_at.clone())))
            .unwrap_or((false, None));
        Ok(ManagementApiStatus {
            enabled: settings.enabled,
            running: is_running,
            address: settings.listen_address.clone(),
            port: settings.port,
            base_url: format!("http://{}:{}/v1", settings.listen_address, settings.port),
            lan_enabled: settings.lan_enabled,
            tls_enabled: settings.tls_enabled,
            token_count: self.db.active_api_token_count()?,
            started_at,
        })
    }

    pub fn create_token(
        &self,
        name: &str,
        scopes: Vec<String>,
        expires_at: Option<i64>,
        source: Option<&str>,
    ) -> Result<CreateApiTokenResponse, AppError> {
        let normalized = normalize_scopes(scopes)?;
        let raw = new_raw_token();
        let id = Uuid::new_v4().to_string();
        let hash = hash_token(&self.token_secret, &raw);
        let record =
            self.db
                .create_api_token(&id, &hash, name.trim(), &normalized, expires_at, source)?;
        Ok(CreateApiTokenResponse { token: raw, record })
    }
}

fn build_router(state: HttpState, settings: &ManagementApiSettings) -> Router {
    let mut router = Router::new()
        .route("/health", get(health))
        .route("/v1/health", get(health))
        .route("/v1/openapi.json", get(openapi))
        .route("/v1/me", get(me))
        .route("/v1/auth/tokens", get(list_tokens).post(create_token_http))
        .route("/v1/auth/tokens/:id", delete(revoke_token))
        .route("/v1/auth/pairing/request", post(pairing_request))
        .route("/v1/auth/pairing/:id", get(pairing_poll))
        .route(
            "/v1/apps/:app/providers",
            get(list_providers).post(create_provider),
        )
        .route("/v1/apps/:app/providers/current", get(current_provider))
        .route(
            "/v1/apps/:app/providers/:id",
            get(get_provider)
                .put(upsert_provider)
                .delete(delete_provider),
        )
        .route("/v1/apps/:app/providers/:id/switch", post(switch_provider))
        .route(
            "/v1/apps/:app/providers/:id/custom-endpoints",
            post(add_custom_endpoint_http).delete(remove_custom_endpoint_http),
        )
        .route(
            "/v1/universal-providers",
            get(list_universal_providers).post(create_universal_provider_http),
        )
        .route(
            "/v1/universal-providers/:id",
            get(get_universal_provider_http)
                .put(update_universal_provider_http)
                .delete(delete_universal_provider_http),
        )
        .route(
            "/v1/universal-providers/:id/sync",
            post(sync_universal_provider_http),
        )
        .route(
            "/v1/mcp/servers",
            get(list_mcp_servers).post(create_mcp_server),
        )
        .route(
            "/v1/mcp/servers/:id",
            get(get_mcp_server)
                .put(update_mcp_server)
                .delete(delete_mcp_server_http),
        )
        .route("/v1/mcp/servers/:id/apps/:app", put(set_mcp_server_app))
        .route(
            "/v1/apps/:app/prompts",
            get(list_prompts).post(create_prompt),
        )
        .route(
            "/v1/apps/:app/prompts/:id",
            get(get_prompt)
                .put(update_prompt)
                .delete(delete_prompt_http),
        )
        .route("/v1/apps/:app/prompts/:id/enable", post(enable_prompt))
        .route("/v1/apps/:app/prompts/:id/disable", post(disable_prompt))
        .route("/v1/skills/installed", get(list_installed_skills))
        .route("/v1/skills/discover", get(discover_skills))
        .route("/v1/skills/updates", get(check_skill_updates_http))
        .route("/v1/skills/install", post(install_skill_http))
        .route("/v1/skills/install-zip", post(install_skills_from_zip_http))
        .route("/v1/skills/backups", get(list_skill_backups_http))
        .route(
            "/v1/skills/backups/:id",
            post(restore_skill_backup_http).delete(delete_skill_backup_http),
        )
        .route(
            "/v1/skills/repos",
            get(list_skill_repos).post(upsert_skill_repo),
        )
        .route(
            "/v1/skills/repos/:owner/:name",
            delete(delete_skill_repo_http),
        )
        .route(
            "/v1/skills/:id",
            get(get_installed_skill).delete(delete_skill_http),
        )
        .route("/v1/skills/:id/update", post(update_skill_http))
        .route("/v1/skills/:id/apps/:app", put(set_skill_app))
        .route("/v1/proxy/status", get(proxy_status))
        .route("/v1/proxy/start", post(proxy_start))
        .route("/v1/proxy/stop-with-restore", post(proxy_stop_with_restore))
        .route(
            "/v1/proxy/config",
            get(proxy_config).put(update_proxy_config_http),
        )
        .route(
            "/v1/proxy/global-config",
            get(global_proxy_config).put(update_global_proxy_config_http),
        )
        .route("/v1/proxy/takeover", get(proxy_takeover_status))
        .route("/v1/proxy/takeover/:app", put(set_proxy_takeover_http))
        .route(
            "/v1/proxy/apps/:app/config",
            get(app_proxy_config).put(update_app_proxy_config_http),
        )
        .route(
            "/v1/proxy/apps/:app/cost-multiplier",
            get(default_cost_multiplier_http).put(set_default_cost_multiplier_http),
        )
        .route(
            "/v1/proxy/apps/:app/pricing-source",
            get(pricing_model_source_http).put(set_pricing_model_source_http),
        )
        .route(
            "/v1/proxy/apps/:app/failover-queue",
            get(failover_queue_http).delete(clear_failover_queue_http),
        )
        .route(
            "/v1/proxy/apps/:app/failover-queue/:provider_id",
            post(add_failover_queue_http).delete(remove_failover_queue_http),
        )
        .route(
            "/v1/proxy/apps/:app/providers/:provider_id/health",
            get(provider_health_http),
        )
        .route(
            "/v1/proxy/apps/:app/providers/:provider_id/circuit-breaker/reset",
            post(reset_circuit_breaker_http),
        )
        .route(
            "/v1/proxy/circuit-breaker/config",
            get(circuit_breaker_config_http).put(update_circuit_breaker_config_http),
        )
        .route(
            "/v1/proxy/log-config",
            get(log_config_http).put(update_log_config_http),
        )
        .route(
            "/v1/proxy/optimizer-config",
            get(optimizer_config_http).put(update_optimizer_config_http),
        )
        .route(
            "/v1/proxy/copilot-optimizer-config",
            get(copilot_optimizer_config_http).put(update_copilot_optimizer_config_http),
        )
        .route(
            "/v1/proxy/rectifier-config",
            get(rectifier_config_http).put(update_rectifier_config_http),
        )
        .route("/v1/usage/summary", get(usage_summary))
        .route("/v1/usage/summary-by-app", get(usage_summary_by_app))
        .route("/v1/usage/trends", get(usage_trends))
        .route("/v1/usage/providers", get(usage_provider_stats))
        .route("/v1/usage/models", get(usage_model_stats))
        .route("/v1/usage/request-logs", get(usage_request_logs))
        .route("/v1/usage/request-logs/:id", get(usage_request_detail))
        .route("/v1/usage/pricing", get(usage_pricing))
        .route("/v1/usage/session-sync", post(sync_session_usage_http))
        .route("/v1/sessions", get(list_sessions_http))
        .route(
            "/v1/sessions/:provider/:id",
            get(get_session_messages_http).delete(delete_session_http),
        )
        .route(
            "/v1/workspace/files/:filename",
            get(read_workspace_file_http).put(write_workspace_file_http),
        )
        .route("/v1/workspace/memory", get(list_daily_memory_http))
        .route("/v1/workspace/memory/search", get(search_daily_memory_http))
        .route(
            "/v1/workspace/memory/:filename",
            get(read_daily_memory_http)
                .put(write_daily_memory_http)
                .delete(delete_daily_memory_http),
        )
        .route("/v1/settings", get(safe_settings).put(update_settings_http))
        .route(
            "/v1/settings/config-snippets/:app",
            get(config_snippet_http).put(update_config_snippet_http),
        )
        .route("/v1/mcp/import", post(import_mcp_http))
        .route("/v1/mcp/sync", post(sync_mcp_http))
        .route("/v1/events", get(events))
        .with_state(state);

    if !settings.cors_origins.is_empty() {
        let origins: Vec<HeaderValue> = settings
            .cors_origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok())
            .collect();
        if !origins.is_empty() {
            router = router.layer(
                CorsLayer::new()
                    .allow_origin(AllowOrigin::list(origins))
                    .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                    .allow_headers([
                        axum::http::header::AUTHORIZATION,
                        axum::http::header::CONTENT_TYPE,
                    ]),
            );
        }
    }
    router
}

async fn health(State(state): State<HttpState>) -> Response {
    ok(json!({
        "status": "ok",
        "service": "management-api",
        "lanEnabled": state.settings.lan_enabled,
        "tlsEnabled": state.settings.tls_enabled,
    }))
}

async fn openapi(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "api:read",
        "GET",
        "/v1/openapi.json",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let mut paths = Map::new();
    for (path, methods) in [
        ("/health", vec![("get", "Service health", true)]),
        ("/me", vec![("get", "Current token", false)]),
        (
            "/auth/tokens",
            vec![
                ("get", "List API tokens", false),
                ("post", "Create API token", false),
            ],
        ),
        (
            "/auth/tokens/{id}",
            vec![("delete", "Revoke API token", false)],
        ),
        (
            "/auth/pairing/request",
            vec![("post", "Request pairing", true)],
        ),
        (
            "/auth/pairing/{pairingId}",
            vec![("get", "Poll pairing", true)],
        ),
        (
            "/apps/{app}/providers",
            vec![
                ("get", "List providers", false),
                ("post", "Create provider", false),
            ],
        ),
        (
            "/apps/{app}/providers/current",
            vec![("get", "Current provider", false)],
        ),
        (
            "/apps/{app}/providers/{id}",
            vec![
                ("get", "Get provider", false),
                ("put", "Update provider", false),
                ("delete", "Delete provider", false),
            ],
        ),
        (
            "/apps/{app}/providers/{id}/switch",
            vec![("post", "Switch provider", false)],
        ),
        (
            "/apps/{app}/providers/{id}/custom-endpoints",
            vec![
                ("post", "Add custom endpoint", false),
                ("delete", "Remove custom endpoint", false),
            ],
        ),
        (
            "/universal-providers",
            vec![
                ("get", "List universal providers", false),
                ("post", "Create universal provider", false),
            ],
        ),
        (
            "/universal-providers/{id}",
            vec![
                ("get", "Get universal provider", false),
                ("put", "Update universal provider", false),
                ("delete", "Delete universal provider", false),
            ],
        ),
        (
            "/universal-providers/{id}/sync",
            vec![("post", "Sync universal provider to apps", false)],
        ),
        (
            "/mcp/servers",
            vec![
                ("get", "List MCP servers", false),
                ("post", "Create MCP server", false),
            ],
        ),
        (
            "/mcp/servers/{id}",
            vec![
                ("get", "Get MCP server", false),
                ("put", "Update MCP server", false),
                ("delete", "Delete MCP server", false),
            ],
        ),
        (
            "/mcp/servers/{id}/apps/{app}",
            vec![("put", "Set MCP app enablement", false)],
        ),
        (
            "/mcp/import",
            vec![("post", "Import MCP servers from app configs", false)],
        ),
        (
            "/mcp/sync",
            vec![("post", "Sync enabled MCP servers to app configs", false)],
        ),
        (
            "/apps/{app}/prompts",
            vec![
                ("get", "List prompts", false),
                ("post", "Create prompt", false),
            ],
        ),
        (
            "/apps/{app}/prompts/{id}",
            vec![
                ("get", "Get prompt", false),
                ("put", "Update prompt", false),
                ("delete", "Delete prompt", false),
            ],
        ),
        (
            "/apps/{app}/prompts/{id}/enable",
            vec![("post", "Enable prompt", false)],
        ),
        (
            "/apps/{app}/prompts/{id}/disable",
            vec![("post", "Disable prompt", false)],
        ),
        (
            "/skills/installed",
            vec![("get", "List installed skills", false)],
        ),
        (
            "/skills/discover",
            vec![("get", "Discover installable skills", false)],
        ),
        (
            "/skills/updates",
            vec![("get", "Check installed skill updates", false)],
        ),
        (
            "/skills/install",
            vec![("post", "Install a discovered skill", false)],
        ),
        (
            "/skills/install-zip",
            vec![("post", "Install skills from local ZIP path", false)],
        ),
        (
            "/skills/backups",
            vec![("get", "List skill backups", false)],
        ),
        (
            "/skills/backups/{id}",
            vec![
                ("post", "Restore a skill backup", false),
                ("delete", "Delete a skill backup", false),
            ],
        ),
        (
            "/skills/{id}",
            vec![
                ("get", "Get installed skill", false),
                ("delete", "Delete skill record", false),
            ],
        ),
        (
            "/skills/{id}/update",
            vec![("post", "Update installed skill", false)],
        ),
        (
            "/skills/{id}/apps/{app}",
            vec![("put", "Set skill app enablement", false)],
        ),
        (
            "/skills/repos",
            vec![
                ("get", "List skill repos", false),
                ("post", "Upsert skill repo", false),
            ],
        ),
        (
            "/skills/repos/{owner}/{name}",
            vec![("delete", "Delete skill repo", false)],
        ),
        ("/proxy/status", vec![("get", "Proxy status", false)]),
        ("/proxy/start", vec![("post", "Start proxy", false)]),
        (
            "/proxy/stop-with-restore",
            vec![("post", "Stop proxy and restore live config", false)],
        ),
        (
            "/proxy/config",
            vec![
                ("get", "Get proxy config", false),
                ("put", "Update proxy config", false),
            ],
        ),
        (
            "/proxy/global-config",
            vec![
                ("get", "Get global proxy config", false),
                ("put", "Update global proxy config", false),
            ],
        ),
        (
            "/proxy/takeover",
            vec![("get", "Get proxy takeover status", false)],
        ),
        (
            "/proxy/takeover/{app}",
            vec![("put", "Set proxy takeover for app", false)],
        ),
        (
            "/proxy/apps/{app}/config",
            vec![
                ("get", "Get app proxy config", false),
                ("put", "Update app proxy config", false),
            ],
        ),
        (
            "/proxy/apps/{app}/cost-multiplier",
            vec![
                ("get", "Get default cost multiplier", false),
                ("put", "Set default cost multiplier", false),
            ],
        ),
        (
            "/proxy/apps/{app}/pricing-source",
            vec![
                ("get", "Get pricing source", false),
                ("put", "Set pricing source", false),
            ],
        ),
        (
            "/proxy/apps/{app}/failover-queue",
            vec![
                ("get", "Get failover queue", false),
                ("delete", "Clear failover queue", false),
            ],
        ),
        (
            "/proxy/apps/{app}/failover-queue/{providerId}",
            vec![
                ("post", "Add provider to failover queue", false),
                ("delete", "Remove provider from failover queue", false),
            ],
        ),
        (
            "/proxy/apps/{app}/providers/{providerId}/health",
            vec![("get", "Get provider health", false)],
        ),
        (
            "/proxy/apps/{app}/providers/{providerId}/circuit-breaker/reset",
            vec![("post", "Reset provider circuit breaker", false)],
        ),
        (
            "/proxy/circuit-breaker/config",
            vec![
                ("get", "Get circuit breaker config", false),
                ("put", "Update circuit breaker config", false),
            ],
        ),
        (
            "/proxy/log-config",
            vec![
                ("get", "Get log config", false),
                ("put", "Update log config", false),
            ],
        ),
        (
            "/proxy/optimizer-config",
            vec![
                ("get", "Get optimizer config", false),
                ("put", "Update optimizer config", false),
            ],
        ),
        (
            "/proxy/copilot-optimizer-config",
            vec![
                ("get", "Get Copilot optimizer config", false),
                ("put", "Update Copilot optimizer config", false),
            ],
        ),
        (
            "/proxy/rectifier-config",
            vec![
                ("get", "Get rectifier config", false),
                ("put", "Update rectifier config", false),
            ],
        ),
        ("/usage/summary", vec![("get", "Usage summary", false)]),
        (
            "/usage/summary-by-app",
            vec![("get", "Usage summary by app", false)],
        ),
        ("/usage/trends", vec![("get", "Usage trends", false)]),
        (
            "/usage/providers",
            vec![("get", "Provider usage stats", false)],
        ),
        ("/usage/models", vec![("get", "Model usage stats", false)]),
        ("/usage/request-logs", vec![("get", "Request logs", false)]),
        (
            "/usage/request-logs/{id}",
            vec![("get", "Request log detail", false)],
        ),
        ("/usage/pricing", vec![("get", "Model pricing", false)]),
        (
            "/usage/session-sync",
            vec![("post", "Sync session usage", false)],
        ),
        ("/sessions", vec![("get", "List sessions", false)]),
        (
            "/sessions/{provider}/{id}",
            vec![
                ("get", "Read session messages", false),
                ("delete", "Delete session", false),
            ],
        ),
        (
            "/workspace/files/{filename}",
            vec![
                ("get", "Read workspace file", false),
                ("put", "Write workspace file", false),
            ],
        ),
        (
            "/workspace/memory",
            vec![("get", "List daily memory files", false)],
        ),
        (
            "/workspace/memory/search",
            vec![("get", "Search daily memory files", false)],
        ),
        (
            "/workspace/memory/{filename}",
            vec![
                ("get", "Read daily memory file", false),
                ("put", "Write daily memory file", false),
                ("delete", "Delete daily memory file", false),
            ],
        ),
        (
            "/settings",
            vec![
                ("get", "Safe settings", false),
                ("put", "Update settings", false),
            ],
        ),
        (
            "/settings/config-snippets/{app}",
            vec![
                ("get", "Get common config snippet", false),
                ("put", "Update common config snippet", false),
            ],
        ),
        ("/events", vec![("get", "Management events stream", false)]),
    ] {
        let mut path_item = Map::new();
        for (method, summary, no_auth) in methods {
            let mut operation = Map::new();
            operation.insert("summary".to_string(), Value::String(summary.to_string()));
            if no_auth {
                operation.insert("security".to_string(), Value::Array(Vec::new()));
            }
            path_item.insert(method.to_string(), Value::Object(operation));
        }
        paths.insert(path.to_string(), Value::Object(path_item));
    }
    let doc = json!({
        "openapi": "3.0.3",
        "info": { "title": "CC Switch Management API", "version": "v1" },
        "servers": [{ "url": "/v1" }],
        "security": [{ "bearerAuth": [] }],
        "paths": Value::Object(paths),
        "components": {
            "securitySchemes": {
                "bearerAuth": { "type": "http", "scheme": "bearer" }
            }
        }
    });
    audit(
        &state,
        Some(&auth),
        Some("api:read"),
        "GET",
        "/v1/openapi.json",
        200,
        None,
    );
    ok(doc)
}

async fn me(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(&state, &headers, addr.ip(), "api:read", "GET", "/v1/me") {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    audit(
        &state,
        Some(&auth),
        Some("api:read"),
        "GET",
        "/v1/me",
        200,
        None,
    );
    ok(json!({
        "tokenId": auth.token_id,
        "scopes": auth.scopes,
        "expiresAt": auth.expires_at,
    }))
}

async fn list_tokens(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "auth:admin",
        "GET",
        "/v1/auth/tokens",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.list_api_tokens() {
        Ok(tokens) => {
            audit(
                &state,
                Some(&auth),
                Some("auth:admin"),
                "GET",
                "/v1/auth/tokens",
                200,
                None,
            );
            ok(tokens)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn create_token_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<CreateApiTokenRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "auth:admin",
        "POST",
        "/v1/auth/tokens",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match (ManagementApiService {
        db: state.db.clone(),
        proxy_service: state.proxy_service.clone(),
        token_secret: state.token_secret.clone(),
        running: Arc::new(RwLock::new(None)),
    })
    .create_token(&req.name, req.scopes, req.expires_at, Some("api"))
    {
        Ok(created) => {
            audit(
                &state,
                Some(&auth),
                Some("auth:admin"),
                "POST",
                "/v1/auth/tokens",
                201,
                None,
            );
            (StatusCode::CREATED, Json(envelope(created))).into_response()
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_token_request",
            e.to_string(),
            None,
        ),
    }
}

async fn revoke_token(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "auth:admin",
        "DELETE",
        "/v1/auth/tokens/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.revoke_api_token(&id) {
        Ok(found) => {
            audit(
                &state,
                Some(&auth),
                Some("auth:admin"),
                "DELETE",
                "/v1/auth/tokens/:id",
                if found { 200 } else { 404 },
                None,
            );
            if found {
                ok(json!({ "revoked": true }))
            } else {
                api_error(StatusCode::NOT_FOUND, "not_found", "Token not found", None)
            }
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairingRequest {
    client_name: String,
    requested_scopes: Vec<String>,
}

async fn pairing_request(
    State(state): State<HttpState>,
    Json(req): Json<PairingRequest>,
) -> Response {
    if !state.settings.pairing_enabled {
        return api_error(
            StatusCode::FORBIDDEN,
            "pairing_disabled",
            "Pairing is disabled",
            None,
        );
    }
    let client_name = req.client_name.trim();
    if client_name.is_empty() || client_name.len() > MAX_PAIRING_CLIENT_NAME_LEN {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_client_name",
            format!("Client name must be 1-{MAX_PAIRING_CLIENT_NAME_LEN} bytes"),
            None,
        );
    }
    let scopes = match normalize_scopes(req.requested_scopes) {
        Ok(scopes) => scopes,
        Err(e) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "invalid_scope",
                e.to_string(),
                None,
            )
        }
    };
    let now = chrono::Utc::now().timestamp_millis();
    let _ = state.db.cleanup_expired_api_pairing_sessions(now);
    match state
        .db
        .count_recent_api_pairing_sessions(now - PAIRING_RATE_LIMIT_WINDOW_MILLIS)
    {
        Ok(count) if count >= MAX_RECENT_PAIRING_REQUESTS => {
            return api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "pairing_rate_limited",
                "Too many recent pairing requests",
                None,
            )
        }
        Ok(_) => {}
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            )
        }
    }
    let pairing_id = Uuid::new_v4().to_string();
    let poll_token = new_poll_token();
    let user_code = pairing_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_ascii_uppercase();
    let expires_at = now + 10 * 60 * 1000;
    let hash = hash_token(&state.token_secret, &poll_token);
    match state
        .db
        .create_api_pairing_session(&pairing_id, client_name, &hash, &scopes, expires_at)
    {
        Ok(_) => ok(json!({
            "pairingId": pairing_id,
            "userCode": user_code,
            "pollToken": poll_token,
            "expiresAt": expires_at,
        })),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PollQuery {
    poll_token: String,
}

async fn pairing_poll(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Query(query): Query<PollQuery>,
) -> Response {
    let session = match state.db.get_api_pairing_session(&id) {
        Ok(Some(session)) => session,
        Ok(None) => {
            return api_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "Pairing session not found",
                None,
            )
        }
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            )
        }
    };
    if session.poll_secret_hash != hash_token(&state.token_secret, &query.poll_token) {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Invalid poll token",
            None,
        );
    }
    let now = chrono::Utc::now().timestamp_millis();
    if session.record.expires_at <= now {
        return ok(json!({ "status": "expired" }));
    }
    if session.record.status != "approved" {
        return ok(json!({ "status": session.record.status }));
    }
    let consumed = match state.db.consume_approved_pairing_token(&id) {
        Ok(Some(consumed)) => consumed,
        Ok(None) => return ok(json!({ "status": "consumed" })),
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            )
        }
    };
    ok(json!({
        "status": "approved",
        "token": consumed.token,
        "scopes": consumed.approved_scopes,
    }))
}

async fn list_providers(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "providers:read",
        "GET",
        "/v1/apps/:app/providers",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match AppType::from_str(&app) {
        Ok(app) => app,
        Err(e) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "unsupported_app",
                e.to_string(),
                None,
            )
        }
    };
    match state.db.get_all_providers(app_type.as_str()) {
        Ok(providers) => {
            let include_secrets = query.get("includeSecrets").is_some_and(|v| v == "true")
                && has_scope(&auth.scopes, "secrets:read");
            let value = serde_json::to_value(providers).unwrap_or_else(|_| json!({}));
            let value = if include_secrets {
                value
            } else {
                redact_value(value)
            };
            audit(
                &state,
                Some(&auth),
                Some("providers:read"),
                "GET",
                "/v1/apps/:app/providers",
                200,
                None,
            );
            ok(value)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn current_provider(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "providers:read",
        "GET",
        "/v1/apps/:app/providers/current",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match AppType::from_str(&app) {
        Ok(app) => app,
        Err(e) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "unsupported_app",
                e.to_string(),
                None,
            )
        }
    };
    match crate::settings::get_effective_current_provider(&state.db, &app_type) {
        Ok(id) => {
            audit(
                &state,
                Some(&auth),
                Some("providers:read"),
                "GET",
                "/v1/apps/:app/providers/current",
                200,
                None,
            );
            ok(json!({ "id": id }))
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn switch_provider(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "providers:switch",
        "POST",
        "/v1/apps/:app/providers/:id/switch",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match AppType::from_str(&app) {
        Ok(app) => app,
        Err(e) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "unsupported_app",
                e.to_string(),
                None,
            )
        }
    };
    let app_state = crate::store::AppState {
        db: state.db.clone(),
        proxy_service: state.proxy_service.clone(),
        usage_cache: Arc::new(UsageCache::new()),
    };
    let result = match ProviderService::switch(&app_state, app_type, &id) {
        Ok(result) => result,
        Err(e) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "switch_failed",
                e.to_string(),
                None,
            )
        }
    };
    audit(
        &state,
        Some(&auth),
        Some("providers:switch"),
        "POST",
        "/v1/apps/:app/providers/:id/switch",
        200,
        None,
    );
    ok(json!({ "switched": true, "id": id, "result": result }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IncludeSecretsQuery {
    include_secrets: Option<bool>,
}

fn include_secrets_allowed(query: &IncludeSecretsQuery, auth: &AuthContext) -> bool {
    query.include_secrets.unwrap_or(false) && has_scope(&auth.scopes, "secrets:read")
}

fn api_app_state(state: &HttpState) -> AppState {
    AppState {
        db: state.db.clone(),
        proxy_service: state.proxy_service.clone(),
        usage_cache: Arc::new(UsageCache::new()),
    }
}

fn parse_app_param(app: &str) -> Result<AppType, Response> {
    AppType::from_str(app).map_err(|e| {
        api_error(
            StatusCode::BAD_REQUEST,
            "unsupported_app",
            e.to_string(),
            None,
        )
    })
}

fn safe_json<T: Serialize>(value: T, include_secrets: bool) -> Value {
    let value = serde_json::to_value(value).unwrap_or_else(|_| json!({}));
    if include_secrets {
        value
    } else {
        redact_value(value)
    }
}

async fn get_provider(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
    Query(query): Query<IncludeSecretsQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "providers:read",
        "GET",
        "/v1/apps/:app/providers/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.get_provider_by_id(&id, app_type.as_str()) {
        Ok(Some(provider)) => {
            audit(
                &state,
                Some(&auth),
                Some("providers:read"),
                "GET",
                "/v1/apps/:app/providers/:id",
                200,
                None,
            );
            ok(safe_json(provider, include_secrets_allowed(&query, &auth)))
        }
        Ok(None) => api_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "Provider not found",
            None,
        ),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn create_provider(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
    Json(provider): Json<Provider>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "providers:write",
        "POST",
        "/v1/apps/:app/providers",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    if provider.id.trim().is_empty() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_provider",
            "Provider id is required",
            None,
        );
    }
    match state.db.save_provider(app_type.as_str(), &provider) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("providers:write"),
                "POST",
                "/v1/apps/:app/providers",
                201,
                None,
            );
            (
                StatusCode::CREATED,
                Json(envelope(safe_json(provider, false))),
            )
                .into_response()
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "provider_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn upsert_provider(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
    Json(mut provider): Json<Provider>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "providers:write",
        "PUT",
        "/v1/apps/:app/providers/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    provider.id = id.clone();
    match state.db.save_provider(app_type.as_str(), &provider) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("providers:write"),
                "PUT",
                "/v1/apps/:app/providers/:id",
                200,
                None,
            );
            ok(safe_json(provider, false))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "provider_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn delete_provider(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "providers:write",
        "DELETE",
        "/v1/apps/:app/providers/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.delete_provider(app_type.as_str(), &id) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("providers:write"),
                "DELETE",
                "/v1/apps/:app/providers/:id",
                200,
                None,
            );
            ok(json!({ "deleted": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "provider_delete_failed",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndpointRequest {
    url: String,
}

async fn add_custom_endpoint_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
    Json(req): Json<EndpointRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "providers:write",
        "POST",
        "/v1/apps/:app/providers/:id/custom-endpoints",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match ProviderService::add_custom_endpoint(&api_app_state(&state), app_type, &id, req.url) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("providers:write"),
                "POST",
                "/v1/apps/:app/providers/:id/custom-endpoints",
                200,
                None,
            );
            ok(json!({ "updated": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "custom_endpoint_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn remove_custom_endpoint_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
    Json(req): Json<EndpointRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "providers:write",
        "DELETE",
        "/v1/apps/:app/providers/:id/custom-endpoints",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match ProviderService::remove_custom_endpoint(&api_app_state(&state), app_type, &id, req.url) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("providers:write"),
                "DELETE",
                "/v1/apps/:app/providers/:id/custom-endpoints",
                200,
                None,
            );
            ok(json!({ "updated": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "custom_endpoint_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn list_universal_providers(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<IncludeSecretsQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "universal:read",
        "GET",
        "/v1/universal-providers",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match ProviderService::list_universal(&api_app_state(&state)) {
        Ok(providers) => {
            audit(
                &state,
                Some(&auth),
                Some("universal:read"),
                "GET",
                "/v1/universal-providers",
                200,
                None,
            );
            ok(safe_json(providers, include_secrets_allowed(&query, &auth)))
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn get_universal_provider_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<IncludeSecretsQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "universal:read",
        "GET",
        "/v1/universal-providers/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match ProviderService::get_universal(&api_app_state(&state), &id) {
        Ok(Some(provider)) => {
            audit(
                &state,
                Some(&auth),
                Some("universal:read"),
                "GET",
                "/v1/universal-providers/:id",
                200,
                None,
            );
            ok(safe_json(provider, include_secrets_allowed(&query, &auth)))
        }
        Ok(None) => api_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "Universal provider not found",
            None,
        ),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn create_universal_provider_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(provider): Json<UniversalProvider>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "universal:write",
        "POST",
        "/v1/universal-providers",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match ProviderService::upsert_universal(&api_app_state(&state), provider.clone()) {
        Ok(_) => {
            audit(
                &state,
                Some(&auth),
                Some("universal:write"),
                "POST",
                "/v1/universal-providers",
                201,
                None,
            );
            (
                StatusCode::CREATED,
                Json(envelope(safe_json(provider, false))),
            )
                .into_response()
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "universal_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn update_universal_provider_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut provider): Json<UniversalProvider>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "universal:write",
        "PUT",
        "/v1/universal-providers/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    provider.id = id;
    match ProviderService::upsert_universal(&api_app_state(&state), provider.clone()) {
        Ok(_) => {
            audit(
                &state,
                Some(&auth),
                Some("universal:write"),
                "PUT",
                "/v1/universal-providers/:id",
                200,
                None,
            );
            ok(safe_json(provider, false))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "universal_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn delete_universal_provider_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "universal:write",
        "DELETE",
        "/v1/universal-providers/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match ProviderService::delete_universal(&api_app_state(&state), &id) {
        Ok(deleted) => {
            audit(
                &state,
                Some(&auth),
                Some("universal:write"),
                "DELETE",
                "/v1/universal-providers/:id",
                if deleted { 200 } else { 404 },
                None,
            );
            if deleted {
                ok(json!({ "deleted": true }))
            } else {
                api_error(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "Universal provider not found",
                    None,
                )
            }
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "universal_delete_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn sync_universal_provider_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "universal:sync",
        "POST",
        "/v1/universal-providers/:id/sync",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match ProviderService::sync_universal_to_apps(&api_app_state(&state), &id) {
        Ok(synced) => {
            audit(
                &state,
                Some(&auth),
                Some("universal:sync"),
                "POST",
                "/v1/universal-providers/:id/sync",
                200,
                None,
            );
            ok(json!({ "synced": synced }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "universal_sync_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn list_mcp_servers(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "mcp:read",
        "GET",
        "/v1/mcp/servers",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.get_all_mcp_servers() {
        Ok(servers) => {
            audit(
                &state,
                Some(&auth),
                Some("mcp:read"),
                "GET",
                "/v1/mcp/servers",
                200,
                None,
            );
            ok(servers)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn get_mcp_server(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "mcp:read",
        "GET",
        "/v1/mcp/servers/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.get_all_mcp_servers() {
        Ok(servers) => match servers.get(&id) {
            Some(server) => {
                audit(
                    &state,
                    Some(&auth),
                    Some("mcp:read"),
                    "GET",
                    "/v1/mcp/servers/:id",
                    200,
                    None,
                );
                ok(server)
            }
            None => api_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "MCP server not found",
                None,
            ),
        },
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn create_mcp_server(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(server): Json<McpServer>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "mcp:write",
        "POST",
        "/v1/mcp/servers",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    if server.id.trim().is_empty() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mcp_server",
            "MCP server id is required",
            None,
        );
    }
    match state.db.save_mcp_server(&server) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("mcp:write"),
                "POST",
                "/v1/mcp/servers",
                201,
                None,
            );
            (StatusCode::CREATED, Json(envelope(server))).into_response()
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "mcp_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn update_mcp_server(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut server): Json<McpServer>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "mcp:write",
        "PUT",
        "/v1/mcp/servers/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    server.id = id;
    match state.db.save_mcp_server(&server) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("mcp:write"),
                "PUT",
                "/v1/mcp/servers/:id",
                200,
                None,
            );
            ok(server)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "mcp_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn delete_mcp_server_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "mcp:write",
        "DELETE",
        "/v1/mcp/servers/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.delete_mcp_server(&id) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("mcp:write"),
                "DELETE",
                "/v1/mcp/servers/:id",
                200,
                None,
            );
            ok(json!({ "deleted": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "mcp_delete_failed",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
struct EnabledRequest {
    enabled: bool,
}

async fn set_mcp_server_app(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((id, app)): Path<(String, String)>,
    Json(req): Json<EnabledRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "mcp:write",
        "PUT",
        "/v1/mcp/servers/:id/apps/:app",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.get_all_mcp_servers() {
        Ok(mut servers) => match servers.swap_remove(&id) {
            Some(mut server) => {
                let mut apps: McpApps = server.apps.clone();
                apps.set_enabled_for(&app_type, req.enabled);
                server.apps = apps;
                if let Err(e) = state.db.save_mcp_server(&server) {
                    return api_error(
                        StatusCode::BAD_REQUEST,
                        "mcp_save_failed",
                        e.to_string(),
                        None,
                    );
                }
                audit(
                    &state,
                    Some(&auth),
                    Some("mcp:write"),
                    "PUT",
                    "/v1/mcp/servers/:id/apps/:app",
                    200,
                    None,
                );
                ok(server)
            }
            None => api_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "MCP server not found",
                None,
            ),
        },
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn list_prompts(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "prompts:read",
        "GET",
        "/v1/apps/:app/prompts",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.get_prompts(app_type.as_str()) {
        Ok(prompts) => {
            audit(
                &state,
                Some(&auth),
                Some("prompts:read"),
                "GET",
                "/v1/apps/:app/prompts",
                200,
                None,
            );
            ok(prompts)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn get_prompt(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "prompts:read",
        "GET",
        "/v1/apps/:app/prompts/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.get_prompts(app_type.as_str()) {
        Ok(prompts) => match prompts.get(&id) {
            Some(prompt) => {
                audit(
                    &state,
                    Some(&auth),
                    Some("prompts:read"),
                    "GET",
                    "/v1/apps/:app/prompts/:id",
                    200,
                    None,
                );
                ok(prompt)
            }
            None => api_error(StatusCode::NOT_FOUND, "not_found", "Prompt not found", None),
        },
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn create_prompt(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
    Json(mut prompt): Json<Prompt>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "prompts:write",
        "POST",
        "/v1/apps/:app/prompts",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    if prompt.id.trim().is_empty() {
        prompt.id = Uuid::new_v4().to_string();
    }
    let now = chrono::Utc::now().timestamp_millis();
    prompt.created_at.get_or_insert(now);
    prompt.updated_at = Some(now);
    match state.db.save_prompt(app_type.as_str(), &prompt) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("prompts:write"),
                "POST",
                "/v1/apps/:app/prompts",
                201,
                None,
            );
            (StatusCode::CREATED, Json(envelope(prompt))).into_response()
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "prompt_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn update_prompt(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
    Json(mut prompt): Json<Prompt>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "prompts:write",
        "PUT",
        "/v1/apps/:app/prompts/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    prompt.id = id;
    prompt.updated_at = Some(chrono::Utc::now().timestamp_millis());
    match state.db.save_prompt(app_type.as_str(), &prompt) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("prompts:write"),
                "PUT",
                "/v1/apps/:app/prompts/:id",
                200,
                None,
            );
            ok(prompt)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "prompt_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn delete_prompt_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "prompts:write",
        "DELETE",
        "/v1/apps/:app/prompts/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.delete_prompt(app_type.as_str(), &id) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("prompts:write"),
                "DELETE",
                "/v1/apps/:app/prompts/:id",
                200,
                None,
            );
            ok(json!({ "deleted": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "prompt_delete_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn set_prompt_enabled(
    state: HttpState,
    auth: AuthContext,
    app: String,
    id: String,
    enabled: bool,
) -> Response {
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    let mut prompt = match state.db.get_prompts(app_type.as_str()) {
        Ok(prompts) => match prompts.get(&id) {
            Some(prompt) => prompt.clone(),
            None => return api_error(StatusCode::NOT_FOUND, "not_found", "Prompt not found", None),
        },
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            )
        }
    };
    prompt.enabled = enabled;
    prompt.updated_at = Some(chrono::Utc::now().timestamp_millis());
    match state.db.save_prompt(app_type.as_str(), &prompt) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("prompts:write"),
                "POST",
                "/v1/apps/:app/prompts/:id/enable",
                200,
                None,
            );
            ok(prompt)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "prompt_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn enable_prompt(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "prompts:write",
        "POST",
        "/v1/apps/:app/prompts/:id/enable",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    set_prompt_enabled(state, auth, app, id, true).await
}

async fn disable_prompt(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "prompts:write",
        "POST",
        "/v1/apps/:app/prompts/:id/disable",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    set_prompt_enabled(state, auth, app, id, false).await
}

async fn list_installed_skills(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:read",
        "GET",
        "/v1/skills/installed",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.get_all_installed_skills() {
        Ok(skills) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:read"),
                "GET",
                "/v1/skills/installed",
                200,
                None,
            );
            ok(skills)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn get_installed_skill(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:read",
        "GET",
        "/v1/skills/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.get_installed_skill(&id) {
        Ok(Some(skill)) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:read"),
                "GET",
                "/v1/skills/:id",
                200,
                None,
            );
            ok(skill)
        }
        Ok(None) => api_error(StatusCode::NOT_FOUND, "not_found", "Skill not found", None),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn delete_skill_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:write",
        "DELETE",
        "/v1/skills/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.delete_skill(&id) {
        Ok(deleted) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:write"),
                "DELETE",
                "/v1/skills/:id",
                if deleted { 200 } else { 404 },
                None,
            );
            if deleted {
                ok(json!({ "deleted": true }))
            } else {
                api_error(StatusCode::NOT_FOUND, "not_found", "Skill not found", None)
            }
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "skill_delete_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn set_skill_app(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((id, app)): Path<(String, String)>,
    Json(req): Json<EnabledRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:write",
        "PUT",
        "/v1/skills/:id/apps/:app",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_type = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    let mut skill = match state.db.get_installed_skill(&id) {
        Ok(Some(skill)) => skill,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "not_found", "Skill not found", None),
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            )
        }
    };
    let mut apps: SkillApps = skill.apps.clone();
    apps.set_enabled_for(&app_type, req.enabled);
    skill.apps = apps;
    match state.db.update_skill_apps(&id, &skill.apps) {
        Ok(true) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:write"),
                "PUT",
                "/v1/skills/:id/apps/:app",
                200,
                None,
            );
            ok(skill)
        }
        Ok(false) => api_error(StatusCode::NOT_FOUND, "not_found", "Skill not found", None),
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "skill_update_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn list_skill_repos(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:read",
        "GET",
        "/v1/skills/repos",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.get_skill_repos() {
        Ok(repos) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:read"),
                "GET",
                "/v1/skills/repos",
                200,
                None,
            );
            ok(repos)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            e.to_string(),
            None,
        ),
    }
}

async fn upsert_skill_repo(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(repo): Json<SkillRepo>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:write",
        "POST",
        "/v1/skills/repos",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.save_skill_repo(&repo) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:write"),
                "POST",
                "/v1/skills/repos",
                200,
                None,
            );
            ok(repo)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "skill_repo_save_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn delete_skill_repo_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:write",
        "DELETE",
        "/v1/skills/repos/:owner/:name",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.delete_skill_repo(&owner, &name) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:write"),
                "DELETE",
                "/v1/skills/repos/:owner/:name",
                200,
                None,
            );
            ok(json!({ "deleted": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "skill_repo_delete_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn discover_skills(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:read",
        "GET",
        "/v1/skills/discover",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let repos = match state.db.get_skill_repos() {
        Ok(repos) => repos,
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            )
        }
    };
    match SkillService::new().discover_available(repos).await {
        Ok(skills) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:read"),
                "GET",
                "/v1/skills/discover",
                200,
                None,
            );
            ok(skills)
        }
        Err(e) => api_error(
            StatusCode::BAD_GATEWAY,
            "skill_discovery_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn check_skill_updates_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:update",
        "GET",
        "/v1/skills/updates",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match SkillService::new().check_updates(&state.db).await {
        Ok(updates) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:update"),
                "GET",
                "/v1/skills/updates",
                200,
                None,
            );
            ok(updates)
        }
        Err(e) => api_error(
            StatusCode::BAD_GATEWAY,
            "skill_update_check_failed",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallSkillRequest {
    skill: Option<DiscoverableSkill>,
    directory: Option<String>,
    current_app: String,
}

async fn install_skill_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<InstallSkillRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:write",
        "POST",
        "/v1/skills/install",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&req.current_app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    let service = SkillService::new();
    let skill = match req.skill {
        Some(skill) => skill,
        None => {
            let Some(directory) = req.directory else {
                return api_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_skill_install",
                    "skill or directory is required",
                    None,
                );
            };
            let repos = match state.db.get_skill_repos() {
                Ok(repos) => repos,
                Err(e) => {
                    return api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "database_error",
                        e.to_string(),
                        None,
                    )
                }
            };
            let discovered = match service.discover_available(repos).await {
                Ok(skills) => skills,
                Err(e) => {
                    return api_error(
                        StatusCode::BAD_GATEWAY,
                        "skill_discovery_failed",
                        e.to_string(),
                        None,
                    )
                }
            };
            match discovered.into_iter().find(|skill| {
                skill.directory.eq_ignore_ascii_case(&directory)
                    || std::path::Path::new(&skill.directory)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case(&directory))
            }) {
                Some(skill) => skill,
                None => {
                    return api_error(StatusCode::NOT_FOUND, "not_found", "Skill not found", None)
                }
            }
        }
    };
    match service.install(&state.db, &skill, &app).await {
        Ok(installed) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:write"),
                "POST",
                "/v1/skills/install",
                200,
                None,
            );
            ok(installed)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "skill_install_failed",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallZipRequest {
    file_path: String,
    current_app: String,
}

async fn install_skills_from_zip_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<InstallZipRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:write",
        "POST",
        "/v1/skills/install-zip",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&req.current_app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match SkillService::install_from_zip(&state.db, std::path::Path::new(&req.file_path), &app) {
        Ok(installed) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:write"),
                "POST",
                "/v1/skills/install-zip",
                200,
                None,
            );
            ok(installed)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "skill_zip_install_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn update_skill_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:update",
        "POST",
        "/v1/skills/:id/update",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match SkillService::new().update_skill(&state.db, &id).await {
        Ok(skill) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:update"),
                "POST",
                "/v1/skills/:id/update",
                200,
                None,
            );
            ok(skill)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "skill_update_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn list_skill_backups_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:read",
        "GET",
        "/v1/skills/backups",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match SkillService::list_backups() {
        Ok(backups) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:read"),
                "GET",
                "/v1/skills/backups",
                200,
                None,
            );
            ok(backups)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "skill_backup_error",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestoreSkillBackupRequest {
    current_app: String,
}

async fn restore_skill_backup_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<RestoreSkillBackupRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:write",
        "POST",
        "/v1/skills/backups/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&req.current_app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match SkillService::restore_from_backup(&state.db, &id, &app) {
        Ok(skill) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:write"),
                "POST",
                "/v1/skills/backups/:id",
                200,
                None,
            );
            ok(skill)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "skill_backup_restore_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn delete_skill_backup_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "skills:write",
        "DELETE",
        "/v1/skills/backups/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match SkillService::delete_backup(&id) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("skills:write"),
                "DELETE",
                "/v1/skills/backups/:id",
                200,
                None,
            );
            ok(json!({ "deleted": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "skill_backup_delete_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn proxy_status(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:read",
        "GET",
        "/v1/proxy/status",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.proxy_service.get_status().await {
        Ok(status) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:read"),
                "GET",
                "/v1/proxy/status",
                200,
                None,
            );
            ok(status)
        }
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "proxy_error", e, None),
    }
}

async fn proxy_start(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:control",
        "POST",
        "/v1/proxy/start",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.proxy_service.start().await {
        Ok(info) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:control"),
                "POST",
                "/v1/proxy/start",
                200,
                None,
            );
            ok(info)
        }
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "proxy_error", e, None),
    }
}

async fn proxy_stop_with_restore(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:control",
        "POST",
        "/v1/proxy/stop-with-restore",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.proxy_service.stop_with_restore().await {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:control"),
                "POST",
                "/v1/proxy/stop-with-restore",
                200,
                None,
            );
            ok(json!({ "stopped": true }))
        }
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "proxy_error", e, None),
    }
}

async fn proxy_config(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "GET",
        "/v1/proxy/config",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.proxy_service.get_config().await {
        Ok(config) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "GET",
                "/v1/proxy/config",
                200,
                None,
            );
            ok(config)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "proxy_config_error",
            e,
            None,
        ),
    }
}

async fn update_proxy_config_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(config): Json<ProxyConfig>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "PUT",
        "/v1/proxy/config",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.proxy_service.update_config(&config).await {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "PUT",
                "/v1/proxy/config",
                200,
                None,
            );
            ok(config)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "proxy_config_update_failed",
            e,
            None,
        ),
    }
}

async fn global_proxy_config(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "GET",
        "/v1/proxy/global-config",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.get_global_proxy_config().await {
        Ok(config) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "GET",
                "/v1/proxy/global-config",
                200,
                None,
            );
            ok(config)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "proxy_config_error",
            e.to_string(),
            None,
        ),
    }
}

async fn update_global_proxy_config_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(config): Json<GlobalProxyConfig>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "PUT",
        "/v1/proxy/global-config",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.update_global_proxy_config(config.clone()).await {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "PUT",
                "/v1/proxy/global-config",
                200,
                None,
            );
            ok(config)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "proxy_config_update_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn proxy_takeover_status(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:read",
        "GET",
        "/v1/proxy/takeover",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.proxy_service.get_takeover_status().await {
        Ok(status) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:read"),
                "GET",
                "/v1/proxy/takeover",
                200,
                None,
            );
            ok(status)
        }
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "proxy_error", e, None),
    }
}

async fn set_proxy_takeover_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
    Json(req): Json<EnabledRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:control",
        "PUT",
        "/v1/proxy/takeover/:app",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    if let Err(resp) = parse_app_param(&app) {
        return resp;
    }
    match state
        .proxy_service
        .set_takeover_for_app(&app, req.enabled)
        .await
    {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:control"),
                "PUT",
                "/v1/proxy/takeover/:app",
                200,
                None,
            );
            ok(json!({ "updated": true }))
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, "proxy_takeover_failed", e, None),
    }
}

async fn app_proxy_config(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "GET",
        "/v1/proxy/apps/:app/config",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.get_proxy_config_for_app(app.as_str()).await {
        Ok(config) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "GET",
                "/v1/proxy/apps/:app/config",
                200,
                None,
            );
            ok(config)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "proxy_config_error",
            e.to_string(),
            None,
        ),
    }
}

async fn update_app_proxy_config_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
    Json(mut config): Json<AppProxyConfig>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "PUT",
        "/v1/proxy/apps/:app/config",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    config.app_type = app.as_str().to_string();
    let circuit = CircuitBreakerConfig::from(&config);
    match state.db.update_proxy_config_for_app(config.clone()).await {
        Ok(()) => {
            let _ = state
                .proxy_service
                .update_circuit_breaker_config_for_app(app.as_str(), circuit)
                .await;
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "PUT",
                "/v1/proxy/apps/:app/config",
                200,
                None,
            );
            ok(config)
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "proxy_config_update_failed",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
struct StringValueRequest {
    value: String,
}

async fn default_cost_multiplier_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "GET",
        "/v1/proxy/apps/:app/cost-multiplier",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.get_default_cost_multiplier(app.as_str()).await {
        Ok(value) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "GET",
                "/v1/proxy/apps/:app/cost-multiplier",
                200,
                None,
            );
            ok(json!({ "value": value }))
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "proxy_config_error",
            e.to_string(),
            None,
        ),
    }
}

async fn set_default_cost_multiplier_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
    Json(req): Json<StringValueRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "PUT",
        "/v1/proxy/apps/:app/cost-multiplier",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state
        .db
        .set_default_cost_multiplier(app.as_str(), &req.value)
        .await
    {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "PUT",
                "/v1/proxy/apps/:app/cost-multiplier",
                200,
                None,
            );
            ok(json!({ "value": req.value }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "proxy_config_update_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn pricing_model_source_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "GET",
        "/v1/proxy/apps/:app/pricing-source",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.get_pricing_model_source(app.as_str()).await {
        Ok(value) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "GET",
                "/v1/proxy/apps/:app/pricing-source",
                200,
                None,
            );
            ok(json!({ "value": value }))
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "proxy_config_error",
            e.to_string(),
            None,
        ),
    }
}

async fn set_pricing_model_source_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
    Json(req): Json<StringValueRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "PUT",
        "/v1/proxy/apps/:app/pricing-source",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state
        .db
        .set_pricing_model_source(app.as_str(), &req.value)
        .await
    {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "PUT",
                "/v1/proxy/apps/:app/pricing-source",
                200,
                None,
            );
            ok(json!({ "value": req.value }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "proxy_config_update_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn failover_queue_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:read",
        "GET",
        "/v1/proxy/apps/:app/failover-queue",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.get_failover_queue(app.as_str()) {
        Ok(queue) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:read"),
                "GET",
                "/v1/proxy/apps/:app/failover-queue",
                200,
                None,
            );
            ok(queue)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failover_queue_error",
            e.to_string(),
            None,
        ),
    }
}

async fn add_failover_queue_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, provider_id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "POST",
        "/v1/proxy/apps/:app/failover-queue/:provider_id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.add_to_failover_queue(app.as_str(), &provider_id) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "POST",
                "/v1/proxy/apps/:app/failover-queue/:provider_id",
                200,
                None,
            );
            ok(json!({ "updated": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "failover_queue_update_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn remove_failover_queue_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, provider_id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "DELETE",
        "/v1/proxy/apps/:app/failover-queue/:provider_id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state
        .db
        .remove_from_failover_queue(app.as_str(), &provider_id)
    {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "DELETE",
                "/v1/proxy/apps/:app/failover-queue/:provider_id",
                200,
                None,
            );
            ok(json!({ "updated": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "failover_queue_update_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn clear_failover_queue_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "DELETE",
        "/v1/proxy/apps/:app/failover-queue",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state.db.clear_failover_queue(app.as_str()) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "DELETE",
                "/v1/proxy/apps/:app/failover-queue",
                200,
                None,
            );
            ok(json!({ "cleared": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "failover_queue_clear_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn provider_health_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, provider_id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:read",
        "GET",
        "/v1/proxy/apps/:app/providers/:provider_id/health",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    match state
        .db
        .get_provider_health(&provider_id, app.as_str())
        .await
    {
        Ok(health) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:read"),
                "GET",
                "/v1/proxy/apps/:app/providers/:provider_id/health",
                200,
                None,
            );
            ok(health)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "provider_health_error",
            e.to_string(),
            None,
        ),
    }
}

async fn reset_circuit_breaker_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((app, provider_id)): Path<(String, String)>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:control",
        "POST",
        "/v1/proxy/apps/:app/providers/:provider_id/circuit-breaker/reset",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    if let Err(e) = state
        .db
        .update_provider_health(&provider_id, app.as_str(), true, None)
        .await
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            "provider_health_update_failed",
            e.to_string(),
            None,
        );
    }
    match state
        .proxy_service
        .reset_provider_circuit_breaker(&provider_id, app.as_str())
        .await
    {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:control"),
                "POST",
                "/v1/proxy/apps/:app/providers/:provider_id/circuit-breaker/reset",
                200,
                None,
            );
            ok(json!({ "reset": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "circuit_breaker_reset_failed",
            e,
            None,
        ),
    }
}

async fn circuit_breaker_config_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "GET",
        "/v1/proxy/circuit-breaker/config",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.get_circuit_breaker_config().await {
        Ok(config) => {
            audit(
                &state,
                Some(&auth),
                Some("proxy:config"),
                "GET",
                "/v1/proxy/circuit-breaker/config",
                200,
                None,
            );
            ok(config)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "circuit_breaker_config_error",
            e.to_string(),
            None,
        ),
    }
}

async fn update_circuit_breaker_config_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(config): Json<CircuitBreakerConfig>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "proxy:config",
        "PUT",
        "/v1/proxy/circuit-breaker/config",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    if let Err(e) = state.db.update_circuit_breaker_config(&config).await {
        return api_error(
            StatusCode::BAD_REQUEST,
            "circuit_breaker_config_update_failed",
            e.to_string(),
            None,
        );
    }
    let _ = state
        .proxy_service
        .update_circuit_breaker_configs(config.clone())
        .await;
    audit(
        &state,
        Some(&auth),
        Some("proxy:config"),
        "PUT",
        "/v1/proxy/circuit-breaker/config",
        200,
        None,
    );
    ok(config)
}

macro_rules! simple_config_handlers {
    ($get_fn:ident, $put_fn:ident, $ty:ty, $scope:literal, $path:literal, $db_get:ident, $db_set:ident) => {
        async fn $get_fn(
            State(state): State<HttpState>,
            ConnectInfo(addr): ConnectInfo<SocketAddr>,
            headers: HeaderMap,
        ) -> Response {
            let auth = match require_auth(&state, &headers, addr.ip(), $scope, "GET", $path) {
                Ok(auth) => auth,
                Err(resp) => return resp,
            };
            match state.db.$db_get() {
                Ok(config) => {
                    audit(&state, Some(&auth), Some($scope), "GET", $path, 200, None);
                    ok(config)
                }
                Err(e) => api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "config_error",
                    e.to_string(),
                    None,
                ),
            }
        }

        async fn $put_fn(
            State(state): State<HttpState>,
            ConnectInfo(addr): ConnectInfo<SocketAddr>,
            headers: HeaderMap,
            Json(config): Json<$ty>,
        ) -> Response {
            let auth = match require_auth(&state, &headers, addr.ip(), $scope, "PUT", $path) {
                Ok(auth) => auth,
                Err(resp) => return resp,
            };
            match state.db.$db_set(&config) {
                Ok(()) => {
                    audit(&state, Some(&auth), Some($scope), "PUT", $path, 200, None);
                    ok(config)
                }
                Err(e) => api_error(
                    StatusCode::BAD_REQUEST,
                    "config_update_failed",
                    e.to_string(),
                    None,
                ),
            }
        }
    };
}

simple_config_handlers!(
    log_config_http,
    update_log_config_http,
    LogConfig,
    "proxy:config",
    "/v1/proxy/log-config",
    get_log_config,
    set_log_config
);
simple_config_handlers!(
    optimizer_config_http,
    update_optimizer_config_http,
    OptimizerConfig,
    "proxy:config",
    "/v1/proxy/optimizer-config",
    get_optimizer_config,
    set_optimizer_config
);
simple_config_handlers!(
    copilot_optimizer_config_http,
    update_copilot_optimizer_config_http,
    CopilotOptimizerConfig,
    "proxy:config",
    "/v1/proxy/copilot-optimizer-config",
    get_copilot_optimizer_config,
    set_copilot_optimizer_config
);
simple_config_handlers!(
    rectifier_config_http,
    update_rectifier_config_http,
    RectifierConfig,
    "proxy:config",
    "/v1/proxy/rectifier-config",
    get_rectifier_config,
    set_rectifier_config
);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageRangeQuery {
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
}

async fn usage_summary(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<UsageRangeQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "usage:read",
        "GET",
        "/v1/usage/summary",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state
        .db
        .get_usage_summary(query.start_date, query.end_date, query.app_type.as_deref())
    {
        Ok(summary) => {
            audit(
                &state,
                Some(&auth),
                Some("usage:read"),
                "GET",
                "/v1/usage/summary",
                200,
                None,
            );
            ok(summary)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_error",
            e.to_string(),
            None,
        ),
    }
}

async fn usage_summary_by_app(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<UsageRangeQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "usage:read",
        "GET",
        "/v1/usage/summary-by-app",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state
        .db
        .get_usage_summary_by_app(query.start_date, query.end_date)
    {
        Ok(summary) => {
            audit(
                &state,
                Some(&auth),
                Some("usage:read"),
                "GET",
                "/v1/usage/summary-by-app",
                200,
                None,
            );
            ok(summary)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_error",
            e.to_string(),
            None,
        ),
    }
}

async fn usage_trends(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<UsageRangeQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "usage:read",
        "GET",
        "/v1/usage/trends",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state
        .db
        .get_daily_trends(query.start_date, query.end_date, query.app_type.as_deref())
    {
        Ok(trends) => {
            audit(
                &state,
                Some(&auth),
                Some("usage:read"),
                "GET",
                "/v1/usage/trends",
                200,
                None,
            );
            ok(trends)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_error",
            e.to_string(),
            None,
        ),
    }
}

async fn usage_provider_stats(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<UsageRangeQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "usage:read",
        "GET",
        "/v1/usage/providers",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state
        .db
        .get_provider_stats(query.start_date, query.end_date, query.app_type.as_deref())
    {
        Ok(stats) => {
            audit(
                &state,
                Some(&auth),
                Some("usage:read"),
                "GET",
                "/v1/usage/providers",
                200,
                None,
            );
            ok(stats)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_error",
            e.to_string(),
            None,
        ),
    }
}

async fn usage_model_stats(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<UsageRangeQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "usage:read",
        "GET",
        "/v1/usage/models",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state
        .db
        .get_model_stats(query.start_date, query.end_date, query.app_type.as_deref())
    {
        Ok(stats) => {
            audit(
                &state,
                Some(&auth),
                Some("usage:read"),
                "GET",
                "/v1/usage/models",
                200,
                None,
            );
            ok(stats)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_error",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestLogsQuery {
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
    status_code: Option<u16>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    page: Option<u32>,
    page_size: Option<u32>,
}

async fn usage_request_logs(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<RequestLogsQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "usage:read",
        "GET",
        "/v1/usage/request-logs",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let filters = LogFilters {
        app_type: query.app_type,
        provider_name: query.provider_name,
        model: query.model,
        status_code: query.status_code,
        start_date: query.start_date,
        end_date: query.end_date,
    };
    match state.db.get_request_logs(
        &filters,
        query.page.unwrap_or(0),
        query.page_size.unwrap_or(50).min(500),
    ) {
        Ok(logs) => {
            audit(
                &state,
                Some(&auth),
                Some("usage:read"),
                "GET",
                "/v1/usage/request-logs",
                200,
                None,
            );
            ok(logs)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_error",
            e.to_string(),
            None,
        ),
    }
}

async fn usage_request_detail(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "usage:read",
        "GET",
        "/v1/usage/request-logs/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match state.db.get_request_detail(&id) {
        Ok(Some(detail)) => {
            audit(
                &state,
                Some(&auth),
                Some("usage:read"),
                "GET",
                "/v1/usage/request-logs/:id",
                200,
                None,
            );
            ok(detail)
        }
        Ok(None) => api_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "Request log not found",
            None,
        ),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_error",
            e.to_string(),
            None,
        ),
    }
}

async fn usage_pricing(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "usage:read",
        "GET",
        "/v1/usage/pricing",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    if let Err(e) = state.db.ensure_model_pricing_seeded() {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_error",
            e.to_string(),
            None,
        );
    }
    let conn = match state.db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            )
        }
    };
    let mut stmt = match conn.prepare(
        "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         ORDER BY display_name",
    ) {
        Ok(stmt) => stmt,
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            )
        }
    };
    let rows = match stmt.query_map([], |row| {
        Ok(json!({
            "modelId": row.get::<_, String>(0)?,
            "displayName": row.get::<_, String>(1)?,
            "inputCostPerMillion": row.get::<_, String>(2)?,
            "outputCostPerMillion": row.get::<_, String>(3)?,
            "cacheReadCostPerMillion": row.get::<_, String>(4)?,
            "cacheCreationCostPerMillion": row.get::<_, String>(5)?,
        }))
    }) {
        Ok(rows) => rows,
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            )
        }
    };
    let mut pricing = Vec::new();
    for row in rows {
        match row {
            Ok(value) => pricing.push(value),
            Err(e) => {
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    e.to_string(),
                    None,
                )
            }
        }
    }
    audit(
        &state,
        Some(&auth),
        Some("usage:read"),
        "GET",
        "/v1/usage/pricing",
        200,
        None,
    );
    ok(pricing)
}

async fn sync_session_usage_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "usage:write",
        "POST",
        "/v1/usage/session-sync",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        let mut result = crate::services::session_usage::sync_claude_session_logs(&db)?;
        if let Ok(codex) = crate::services::session_usage_codex::sync_codex_usage(&db) {
            result.imported += codex.imported;
            result.skipped += codex.skipped;
            result.files_scanned += codex.files_scanned;
            result.errors.extend(codex.errors);
        }
        if let Ok(gemini) = crate::services::session_usage_gemini::sync_gemini_usage(&db) {
            result.imported += gemini.imported;
            result.skipped += gemini.skipped;
            result.files_scanned += gemini.files_scanned;
            result.errors.extend(gemini.errors);
        }
        if let Ok(opencode) = crate::services::session_usage_opencode::sync_opencode_usage(&db) {
            result.imported += opencode.imported;
            result.skipped += opencode.skipped;
            result.files_scanned += opencode.files_scanned;
            result.errors.extend(opencode.errors);
        }
        Ok::<_, AppError>(result)
    })
    .await
    {
        Ok(Ok(result)) => {
            audit(
                &state,
                Some(&auth),
                Some("usage:write"),
                "POST",
                "/v1/usage/session-sync",
                200,
                None,
            );
            ok(result)
        }
        Ok(Err(e)) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_sync_failed",
            e.to_string(),
            None,
        ),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_sync_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn list_sessions_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "sessions:read",
        "GET",
        "/v1/sessions",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match tokio::task::spawn_blocking(crate::session_manager::scan_sessions).await {
        Ok(sessions) => {
            audit(
                &state,
                Some(&auth),
                Some("sessions:read"),
                "GET",
                "/v1/sessions",
                200,
                None,
            );
            ok(sessions)
        }
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_error",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionSourceQuery {
    source_path: String,
}

async fn get_session_messages_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((provider, id)): Path<(String, String)>,
    Query(query): Query<SessionSourceQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "sessions:read",
        "GET",
        "/v1/sessions/:provider/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match tokio::task::spawn_blocking(move || {
        crate::session_manager::load_messages(&provider, &query.source_path)
    })
    .await
    {
        Ok(Ok(messages)) => {
            audit(
                &state,
                Some(&auth),
                Some("sessions:read"),
                "GET",
                "/v1/sessions/:provider/:id",
                200,
                None,
            );
            ok(json!({ "sessionId": id, "messages": messages }))
        }
        Ok(Err(e)) => api_error(StatusCode::BAD_REQUEST, "session_read_failed", e, None),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_read_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn delete_session_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((provider, id)): Path<(String, String)>,
    Query(query): Query<SessionSourceQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "sessions:delete",
        "DELETE",
        "/v1/sessions/:provider/:id",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match tokio::task::spawn_blocking(move || {
        crate::session_manager::delete_session(&provider, &id, &query.source_path)
    })
    .await
    {
        Ok(Ok(deleted)) => {
            audit(
                &state,
                Some(&auth),
                Some("sessions:delete"),
                "DELETE",
                "/v1/sessions/:provider/:id",
                200,
                None,
            );
            ok(json!({ "deleted": deleted }))
        }
        Ok(Err(e)) => api_error(StatusCode::BAD_REQUEST, "session_delete_failed", e, None),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_delete_failed",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
struct ContentRequest {
    content: String,
}

async fn read_workspace_file_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(filename): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "workspace:read",
        "GET",
        "/v1/workspace/files/:filename",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match crate::commands::read_workspace_file(filename).await {
        Ok(content) => {
            audit(
                &state,
                Some(&auth),
                Some("workspace:read"),
                "GET",
                "/v1/workspace/files/:filename",
                200,
                None,
            );
            ok(json!({ "content": content }))
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, "workspace_read_failed", e, None),
    }
}

async fn write_workspace_file_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(filename): Path<String>,
    Json(req): Json<ContentRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "workspace:write",
        "PUT",
        "/v1/workspace/files/:filename",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match crate::commands::write_workspace_file(filename, req.content).await {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("workspace:write"),
                "PUT",
                "/v1/workspace/files/:filename",
                200,
                None,
            );
            ok(json!({ "written": true }))
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, "workspace_write_failed", e, None),
    }
}

async fn list_daily_memory_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "workspace:read",
        "GET",
        "/v1/workspace/memory",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match crate::commands::list_daily_memory_files().await {
        Ok(files) => {
            audit(
                &state,
                Some(&auth),
                Some("workspace:read"),
                "GET",
                "/v1/workspace/memory",
                200,
                None,
            );
            ok(files)
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, "workspace_read_failed", e, None),
    }
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    query: String,
}

async fn search_daily_memory_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "workspace:read",
        "GET",
        "/v1/workspace/memory/search",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match crate::commands::search_daily_memory_files(query.query).await {
        Ok(results) => {
            audit(
                &state,
                Some(&auth),
                Some("workspace:read"),
                "GET",
                "/v1/workspace/memory/search",
                200,
                None,
            );
            ok(results)
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, "workspace_search_failed", e, None),
    }
}

async fn read_daily_memory_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(filename): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "workspace:read",
        "GET",
        "/v1/workspace/memory/:filename",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match crate::commands::read_daily_memory_file(filename).await {
        Ok(content) => {
            audit(
                &state,
                Some(&auth),
                Some("workspace:read"),
                "GET",
                "/v1/workspace/memory/:filename",
                200,
                None,
            );
            ok(json!({ "content": content }))
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, "workspace_read_failed", e, None),
    }
}

async fn write_daily_memory_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(filename): Path<String>,
    Json(req): Json<ContentRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "workspace:write",
        "PUT",
        "/v1/workspace/memory/:filename",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match crate::commands::write_daily_memory_file(filename, req.content).await {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("workspace:write"),
                "PUT",
                "/v1/workspace/memory/:filename",
                200,
                None,
            );
            ok(json!({ "written": true }))
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, "workspace_write_failed", e, None),
    }
}

async fn delete_daily_memory_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(filename): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "workspace:write",
        "DELETE",
        "/v1/workspace/memory/:filename",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match crate::commands::delete_daily_memory_file(filename).await {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("workspace:write"),
                "DELETE",
                "/v1/workspace/memory/:filename",
                200,
                None,
            );
            ok(json!({ "deleted": true }))
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, "workspace_delete_failed", e, None),
    }
}

async fn safe_settings(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "settings:read",
        "GET",
        "/v1/settings",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let settings = crate::settings::get_settings_for_frontend();
    audit(
        &state,
        Some(&auth),
        Some("settings:read"),
        "GET",
        "/v1/settings",
        200,
        None,
    );
    ok(redact_value(
        serde_json::to_value(settings).unwrap_or_else(|_| json!({})),
    ))
}

async fn update_settings_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(settings): Json<crate::settings::AppSettings>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "settings:write",
        "PUT",
        "/v1/settings",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let existing = crate::settings::get_settings();
    let merged = crate::commands::merge_settings_for_save(settings, &existing);
    match crate::settings::update_settings(merged) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("settings:write"),
                "PUT",
                "/v1/settings",
                200,
                None,
            );
            ok(redact_value(
                serde_json::to_value(crate::settings::get_settings_for_frontend())
                    .unwrap_or_else(|_| json!({})),
            ))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "settings_update_failed",
            e.to_string(),
            None,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigSnippetRequest {
    value: Option<String>,
    cleared: Option<bool>,
}

async fn config_snippet_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "settings:read",
        "GET",
        "/v1/settings/config-snippets/:app",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    let snippet = match state.db.get_config_snippet(app.as_str()) {
        Ok(snippet) => snippet,
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings_read_failed",
                e.to_string(),
                None,
            )
        }
    };
    let cleared = match state.db.is_config_snippet_cleared(app.as_str()) {
        Ok(cleared) => cleared,
        Err(e) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings_read_failed",
                e.to_string(),
                None,
            )
        }
    };
    audit(
        &state,
        Some(&auth),
        Some("settings:read"),
        "GET",
        "/v1/settings/config-snippets/:app",
        200,
        None,
    );
    ok(json!({ "app": app.as_str(), "value": snippet, "cleared": cleared }))
}

async fn update_config_snippet_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(app): Path<String>,
    Json(req): Json<ConfigSnippetRequest>,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "settings:write",
        "PUT",
        "/v1/settings/config-snippets/:app",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app = match parse_app_param(&app) {
        Ok(app) => app,
        Err(resp) => return resp,
    };
    if let Err(e) = state.db.set_config_snippet(app.as_str(), req.value.clone()) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "settings_update_failed",
            e.to_string(),
            None,
        );
    }
    if let Some(cleared) = req.cleared {
        if let Err(e) = state.db.set_config_snippet_cleared(app.as_str(), cleared) {
            return api_error(
                StatusCode::BAD_REQUEST,
                "settings_update_failed",
                e.to_string(),
                None,
            );
        }
    }
    audit(
        &state,
        Some(&auth),
        Some("settings:write"),
        "PUT",
        "/v1/settings/config-snippets/:app",
        200,
        None,
    );
    ok(json!({ "updated": true }))
}

async fn import_mcp_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "mcp:sync",
        "POST",
        "/v1/mcp/import",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    let app_state = api_app_state(&state);
    let total = [
        McpService::import_from_claude(&app_state),
        McpService::import_from_codex(&app_state),
        McpService::import_from_gemini(&app_state),
        McpService::import_from_opencode(&app_state),
        McpService::import_from_hermes(&app_state),
    ]
    .into_iter()
    .filter_map(Result::ok)
    .sum::<usize>();
    audit(
        &state,
        Some(&auth),
        Some("mcp:sync"),
        "POST",
        "/v1/mcp/import",
        200,
        None,
    );
    ok(json!({ "imported": total }))
}

async fn sync_mcp_http(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "mcp:sync",
        "POST",
        "/v1/mcp/sync",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    match McpService::sync_all_enabled(&api_app_state(&state)) {
        Ok(()) => {
            audit(
                &state,
                Some(&auth),
                Some("mcp:sync"),
                "POST",
                "/v1/mcp/sync",
                200,
                None,
            );
            ok(json!({ "synced": true }))
        }
        Err(e) => api_error(
            StatusCode::BAD_REQUEST,
            "mcp_sync_failed",
            e.to_string(),
            None,
        ),
    }
}

async fn events(
    State(state): State<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match require_auth(
        &state,
        &headers,
        addr.ip(),
        "events:read",
        "GET",
        "/v1/events",
    ) {
        Ok(auth) => auth,
        Err(resp) => return resp,
    };
    audit(
        &state,
        Some(&auth),
        Some("events:read"),
        "GET",
        "/v1/events",
        200,
        None,
    );
    let stream = async_stream::stream! {
        yield Ok::<Event, Infallible>(
            Event::default()
                .event("ready")
                .json_data(json!({ "service": "management-api" }))
                .unwrap_or_else(|_| Event::default().event("ready"))
        );
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("heartbeat")
                    .json_data(json!({ "ts": chrono::Utc::now().to_rfc3339() }))
                    .unwrap_or_else(|_| Event::default().event("heartbeat"))
            );
        }
    };
    Sse::new(stream).into_response()
}

fn require_auth(
    state: &HttpState,
    headers: &HeaderMap,
    remote_ip: IpAddr,
    scope: &str,
    method: &str,
    path: &str,
) -> Result<AuthContext, Response> {
    if let Err(message) = check_remote_allowed(&state.settings, remote_ip) {
        audit(state, None, Some(scope), method, path, 403, Some(remote_ip));
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "ip_not_allowed",
            message,
            None,
        ));
    }
    let Some(header) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    else {
        audit(state, None, Some(scope), method, path, 401, Some(remote_ip));
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Missing bearer token",
            None,
        ));
    };
    let Some(raw) = header
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        audit(state, None, Some(scope), method, path, 401, Some(remote_ip));
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Invalid Authorization header",
            None,
        ));
    };
    let hash = hash_token(&state.token_secret, raw);
    let lookup = match state.db.get_api_token_by_hash(&hash) {
        Ok(Some(token)) => token,
        Ok(None) => {
            audit(state, None, Some(scope), method, path, 401, Some(remote_ip));
            return Err(api_error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Invalid token",
                None,
            ));
        }
        Err(e) => {
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
                None,
            ))
        }
    };
    let now = chrono::Utc::now().timestamp_millis();
    if lookup.record.revoked_at.is_some()
        || lookup
            .record
            .expires_at
            .is_some_and(|expires| expires <= now)
    {
        audit(
            state,
            Some(&AuthContext {
                token_id: lookup.record.id.clone(),
                scopes: lookup.record.scopes.clone(),
                expires_at: lookup.record.expires_at,
            }),
            Some(scope),
            method,
            path,
            401,
            Some(remote_ip),
        );
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Token expired or revoked",
            None,
        ));
    }
    if !has_scope(&lookup.record.scopes, scope) {
        audit(
            state,
            Some(&AuthContext {
                token_id: lookup.record.id.clone(),
                scopes: lookup.record.scopes.clone(),
                expires_at: lookup.record.expires_at,
            }),
            Some(scope),
            method,
            path,
            403,
            Some(remote_ip),
        );
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Token does not include the required scope",
            Some(json!({ "requiredScope": scope })),
        ));
    }
    let _ = state.db.touch_api_token(&lookup.record.id);
    Ok(AuthContext {
        token_id: lookup.record.id,
        scopes: lookup.record.scopes,
        expires_at: lookup.record.expires_at,
    })
}

fn has_scope(scopes: &[String], required: &str) -> bool {
    scopes.iter().any(|scope| scope == required || scope == "*")
}

fn normalize_scopes(scopes: Vec<String>) -> Result<Vec<String>, AppError> {
    let mut normalized: Vec<String> = scopes
        .into_iter()
        .map(|scope| scope.trim().to_string())
        .filter(|scope| !scope.is_empty())
        .collect();
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        return Err(AppError::InvalidInput(
            "At least one scope is required".to_string(),
        ));
    }
    if let Some(invalid) = normalized
        .iter()
        .find(|scope| scope.as_str() != "*" && !ALL_SCOPES.contains(&scope.as_str()))
    {
        return Err(AppError::InvalidInput(format!("Unknown scope: {invalid}")));
    }
    Ok(normalized)
}

fn check_remote_allowed(settings: &ManagementApiSettings, ip: IpAddr) -> Result<(), String> {
    if ip.is_loopback() {
        return Ok(());
    }
    if !settings.lan_enabled {
        return Err("Only loopback clients are allowed".to_string());
    }
    if settings
        .allowed_cidrs
        .iter()
        .any(|cidr| ip_matches_cidr(ip, cidr))
    {
        return Ok(());
    }
    Err("Remote IP is not in the Management API allow-list".to_string())
}

fn ip_matches_cidr(ip: IpAddr, cidr: &str) -> bool {
    if let Ok(exact) = cidr.parse::<IpAddr>() {
        return exact == ip;
    }
    let Some((base, prefix)) = cidr.split_once('/') else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    match (ip, base.parse::<IpAddr>()) {
        (IpAddr::V4(ip), Ok(IpAddr::V4(base))) if prefix <= 32 => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            (u32::from(ip) & mask) == (u32::from(base) & mask)
        }
        (IpAddr::V6(ip), Ok(IpAddr::V6(base))) if prefix <= 128 => {
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            (u128::from(ip) & mask) == (u128::from(base) & mask)
        }
        _ => false,
    }
}

fn new_raw_token() -> String {
    format!(
        "ccs_{}_{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn new_poll_token() -> String {
    format!(
        "poll_{}_{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn hash_token(secret: &[u8], raw: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(raw.as_bytes());
    let bytes = mac.finalize().into_bytes();
    hex_lower(&bytes)
}

fn load_or_create_secret() -> Vec<u8> {
    let path = management_secret_path();
    if let Ok(bytes) = std::fs::read(&path) {
        if bytes.len() >= 32 {
            return bytes;
        }
    }
    let seed = format!(
        "{}{}{}",
        Uuid::new_v4(),
        Uuid::new_v4(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let secret = Sha256::digest(seed.as_bytes()).to_vec();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, &secret);
    secret
}

fn management_secret_path() -> PathBuf {
    crate::config::get_app_config_dir().join("management-api.secret")
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn redact_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    if lower.contains("apikey")
                        || lower.contains("api_key")
                        || lower.contains("token")
                        || lower.contains("secret")
                        || lower.contains("password")
                        || lower.contains("authorization")
                        || lower.contains("auth_token")
                    {
                        (key, Value::String("[REDACTED]".to_string()))
                    } else {
                        (key, redact_value(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_value).collect()),
        other => other,
    }
}

fn envelope<T: Serialize>(data: T) -> ApiEnvelope<T> {
    ApiEnvelope {
        data,
        meta: ApiMeta {
            request_id: Uuid::new_v4().to_string(),
        },
    }
}

fn ok<T: Serialize>(data: T) -> Response {
    Json(envelope(data)).into_response()
}

fn api_error(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    details: Option<Value>,
) -> Response {
    (
        status,
        Json(ApiErrorEnvelope {
            error: ApiErrorBody {
                code: code.to_string(),
                message: message.into(),
                details,
            },
            meta: ApiMeta {
                request_id: Uuid::new_v4().to_string(),
            },
        }),
    )
        .into_response()
}

fn audit(
    state: &HttpState,
    auth: Option<&AuthContext>,
    scope: Option<&str>,
    method: &str,
    path: &str,
    status: u16,
    remote_ip: Option<IpAddr>,
) {
    let request_id = Uuid::new_v4().to_string();
    let _ = state.db.insert_api_audit_log(
        auth.map(|auth| auth.token_id.as_str()),
        scope,
        method,
        path,
        status,
        &request_id,
        remote_ip.map(|ip| ip.to_string()).as_deref(),
    );
}

pub fn approve_pairing(
    db: &Database,
    service: &ManagementApiService,
    pairing_id: &str,
    name: &str,
    scopes: Vec<String>,
    expires_at: Option<i64>,
) -> Result<CreateApiTokenResponse, AppError> {
    let created = service.create_token(name, scopes.clone(), expires_at, Some("pairing"))?;
    let approved = db.approve_api_pairing_session(
        pairing_id,
        &created.record.scopes,
        &created.record.id,
        &created.token,
    )?;
    if !approved {
        let _ = db.revoke_api_token(&created.record.id);
        return Err(AppError::InvalidInput(
            "Pairing session is not pending or does not exist".to_string(),
        ));
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::header::AUTHORIZATION;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn test_state(
        db: Arc<Database>,
        token_secret: Arc<Vec<u8>>,
        settings: ManagementApiSettings,
    ) -> HttpState {
        HttpState {
            db: db.clone(),
            proxy_service: ProxyService::new(db),
            token_secret,
            settings,
        }
    }

    fn bearer(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).expect("valid bearer header"),
        );
        headers
    }

    #[test]
    fn normalize_scopes_sorts_deduplicates_and_rejects_unknown_scope() {
        let scopes = normalize_scopes(vec![
            " providers:read ".to_string(),
            "api:read".to_string(),
            "providers:read".to_string(),
            "".to_string(),
        ])
        .expect("valid scopes");

        assert_eq!(
            scopes,
            vec!["api:read".to_string(), "providers:read".to_string()]
        );
        assert!(has_scope(&scopes, "api:read"));
        assert!(!has_scope(&scopes, "proxy:read"));
        assert!(has_scope(&["*".to_string()], "proxy:control"));
        assert!(normalize_scopes(vec!["nope:read".to_string()]).is_err());
    }

    #[test]
    fn check_remote_allowed_enforces_loopback_and_cidr_allow_list() {
        let mut settings = ManagementApiSettings::default();
        assert!(check_remote_allowed(&settings, IpAddr::V4(Ipv4Addr::LOCALHOST)).is_ok());
        assert!(
            check_remote_allowed(&settings, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))).is_err()
        );

        settings.lan_enabled = true;
        settings.allowed_cidrs = vec![
            "192.168.1.0/24".to_string(),
            "10.0.0.5".to_string(),
            "fd00::/8".to_string(),
        ];
        assert!(
            check_remote_allowed(&settings, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))).is_ok()
        );
        assert!(check_remote_allowed(&settings, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))).is_ok());
        assert!(check_remote_allowed(
            &settings,
            IpAddr::V6(Ipv6Addr::from_str("fd00::1").unwrap())
        )
        .is_ok());
        assert!(
            check_remote_allowed(&settings, IpAddr::V4(Ipv4Addr::new(192, 168, 2, 10))).is_err()
        );
    }

    #[test]
    fn redact_value_masks_nested_secret_like_fields() {
        let redacted = redact_value(json!({
            "apiKey": "sk-live",
            "nested": {
                "authorization": "Bearer abc",
                "safe": "visible",
                "items": [{ "refreshToken": "secret" }]
            }
        }));

        assert_eq!(redacted["apiKey"], "[REDACTED]");
        assert_eq!(redacted["nested"]["authorization"], "[REDACTED]");
        assert_eq!(redacted["nested"]["safe"], "visible");
        assert_eq!(redacted["nested"]["items"][0]["refreshToken"], "[REDACTED]");
    }

    #[test]
    fn require_auth_accepts_valid_token_and_updates_last_used() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let secret = Arc::new(b"test-management-secret".to_vec());
        let raw = "ccs_test_token";
        let hash = hash_token(&secret, raw);
        let record = db
            .create_api_token(
                "token-1",
                &hash,
                "test token",
                &["api:read".to_string(), "providers:read".to_string()],
                None,
                Some("test"),
            )
            .expect("create token");
        let state = test_state(db.clone(), secret, ManagementApiSettings::default());

        let auth = require_auth(
            &state,
            &bearer(raw),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            "api:read",
            "GET",
            "/v1/me",
        )
        .expect("valid auth");

        assert_eq!(auth.token_id, record.id);
        assert_eq!(
            auth.scopes,
            vec!["api:read".to_string(), "providers:read".to_string()]
        );
        let lookup = db
            .get_api_token_by_hash(&hash)
            .expect("lookup")
            .expect("token");
        assert!(lookup.record.last_used_at.is_some());
    }

    #[test]
    fn require_auth_rejects_wrong_scope_revoked_and_expired_tokens() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let secret = Arc::new(b"test-management-secret".to_vec());
        let settings = ManagementApiSettings::default();
        let state = test_state(db.clone(), secret.clone(), settings);

        let raw = "ccs_wrong_scope";
        db.create_api_token(
            "wrong-scope",
            &hash_token(&secret, raw),
            "wrong scope",
            &["api:read".to_string()],
            None,
            Some("test"),
        )
        .expect("create token");
        assert!(require_auth(
            &state,
            &bearer(raw),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            "proxy:read",
            "GET",
            "/v1/proxy/status",
        )
        .is_err());

        let revoked_raw = "ccs_revoked";
        db.create_api_token(
            "revoked",
            &hash_token(&secret, revoked_raw),
            "revoked",
            &["api:read".to_string()],
            None,
            Some("test"),
        )
        .expect("create token");
        db.revoke_api_token("revoked").expect("revoke token");
        assert!(require_auth(
            &state,
            &bearer(revoked_raw),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            "api:read",
            "GET",
            "/v1/me",
        )
        .is_err());

        let expired_raw = "ccs_expired";
        db.create_api_token(
            "expired",
            &hash_token(&secret, expired_raw),
            "expired",
            &["api:read".to_string()],
            Some(chrono::Utc::now().timestamp_millis() - 1),
            Some("test"),
        )
        .expect("create token");
        assert!(require_auth(
            &state,
            &bearer(expired_raw),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            "api:read",
            "GET",
            "/v1/me",
        )
        .is_err());
    }

    #[tokio::test]
    async fn openapi_includes_expanded_http_management_routes() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let secret = Arc::new(b"test-management-secret".to_vec());
        let raw = "ccs_openapi";
        db.create_api_token(
            "openapi",
            &hash_token(&secret, raw),
            "openapi",
            &["api:read".to_string()],
            None,
            Some("test"),
        )
        .expect("create token");
        let state = test_state(db, secret, ManagementApiSettings::default());

        let response = openapi(
            State(state),
            ConnectInfo(SocketAddr::from((Ipv4Addr::LOCALHOST, 12345))),
            bearer(raw),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&bytes).expect("openapi json");
        let paths = &body["data"]["paths"];

        for path in [
            "/apps/{app}/providers",
            "/universal-providers/{id}/sync",
            "/mcp/import",
            "/mcp/servers/{id}/apps/{app}",
            "/apps/{app}/prompts/{id}/enable",
            "/skills/discover",
            "/skills/install-zip",
            "/skills/{id}/update",
            "/skills/repos/{owner}/{name}",
            "/proxy/takeover/{app}",
            "/proxy/apps/{app}/config",
            "/proxy/apps/{app}/failover-queue/{providerId}",
            "/proxy/circuit-breaker/config",
            "/usage/request-logs/{id}",
            "/sessions/{provider}/{id}",
            "/workspace/memory/{filename}",
            "/settings/config-snippets/{app}",
            "/events",
        ] {
            assert!(paths.get(path).is_some(), "missing OpenAPI path {path}");
        }
    }

    #[tokio::test]
    async fn start_rebuilds_when_security_settings_change_on_same_address() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let service = ManagementApiService::new(db.clone(), ProxyService::new(db));
        let listener =
            std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let mut settings = ManagementApiSettings {
            enabled: true,
            port,
            ..ManagementApiSettings::default()
        };
        service.start(settings.clone()).await.expect("start api");

        settings.pairing_enabled = false;
        service
            .start(settings.clone())
            .await
            .expect("restart with changed settings");

        let running = service.running.read().await;
        assert_eq!(
            running
                .as_ref()
                .expect("running server")
                .settings
                .pairing_enabled,
            false
        );
        drop(running);
        service.stop().await.expect("stop api");
    }

    #[tokio::test]
    async fn pairing_request_validates_client_name_and_rate_limits() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let secret = Arc::new(b"test-management-secret".to_vec());
        let state = test_state(db, secret, ManagementApiSettings::default());

        let invalid = pairing_request(
            State(state.clone()),
            Json(PairingRequest {
                client_name: "   ".to_string(),
                requested_scopes: vec!["api:read".to_string()],
            }),
        )
        .await;
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        for i in 0..MAX_RECENT_PAIRING_REQUESTS {
            let response = pairing_request(
                State(state.clone()),
                Json(PairingRequest {
                    client_name: format!("client-{i}"),
                    requested_scopes: vec!["api:read".to_string()],
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }

        let limited = pairing_request(
            State(state),
            Json(PairingRequest {
                client_name: "client-limited".to_string(),
                requested_scopes: vec!["api:read".to_string()],
            }),
        )
        .await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn pairing_poll_delivers_approved_token_once() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let secret = Arc::new(b"test-management-secret".to_vec());
        let state = test_state(db.clone(), secret.clone(), ManagementApiSettings::default());
        let poll_token = "poll_test_token";
        let pairing_id = "pairing-once";
        db.create_api_pairing_session(
            pairing_id,
            "client",
            &hash_token(&secret, poll_token),
            &["api:read".to_string()],
            chrono::Utc::now().timestamp_millis() + 60_000,
        )
        .expect("create pairing");
        db.approve_api_pairing_session(
            pairing_id,
            &["api:read".to_string()],
            "token-1",
            "raw-token",
        )
        .expect("approve pairing");

        let first = pairing_poll(
            State(state.clone()),
            Path(pairing_id.to_string()),
            Query(PollQuery {
                poll_token: poll_token.to_string(),
            }),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let bytes = to_bytes(first.into_body(), 1024 * 1024)
            .await
            .expect("first body");
        let body: Value = serde_json::from_slice(&bytes).expect("first json");
        assert_eq!(body["data"]["status"], "approved");
        assert_eq!(body["data"]["token"], "raw-token");

        let second = pairing_poll(
            State(state),
            Path(pairing_id.to_string()),
            Query(PollQuery {
                poll_token: poll_token.to_string(),
            }),
        )
        .await;
        let bytes = to_bytes(second.into_body(), 1024 * 1024)
            .await
            .expect("second body");
        let body: Value = serde_json::from_slice(&bytes).expect("second json");
        assert_eq!(body["data"]["status"], "consumed");
    }
}
