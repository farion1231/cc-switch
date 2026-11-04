//! Account-bound, same-protocol gateways for clients without a CC Switch routing mode.
use super::{
    providers::dim_oauth_auth::{apply_dim_effort, DimOAuthManager},
    server::ProxyState,
};
use crate::{
    app_config::AppType, database::Database, error::AppError, provider::Provider, store::AppState,
};
use axum::{
    body::{to_bytes, Body},
    extract::{ConnectInfo, Path, Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::Response,
    Json,
};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::{
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    str::FromStr,
    sync::Mutex,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tauri::Manager;
use url::Url;

const MASTER_KEY: &str = "dim_gateway_master_key";
static KEY_CREATION: Mutex<()> = Mutex::new(());
const MAX_BODY: usize = 200 * 1024 * 1024;

pub(crate) fn is_relay_client(app: &AppType) -> bool {
    app.is_additive_mode() || *app == AppType::Agy
}

fn api_format<'a>(app: &AppType, provider: &'a Provider) -> Result<&'a str, AppError> {
    let format = provider
        .meta
        .as_ref()
        .and_then(|m| m.api_format.as_deref())
        .ok_or_else(|| AppError::Config("DimAgent API format is required".into()))?;
    let allowed = match app {
        AppType::Mcode | AppType::Dsh | AppType::Hermes => {
            matches!(format, "openai_chat" | "openai_responses" | "anthropic")
        }
        AppType::Agy => format == "gemini_native",
        _ => matches!(
            format,
            "openai_chat" | "openai_responses" | "anthropic" | "gemini_native"
        ),
    };
    if !allowed {
        return Err(AppError::Config(
            "DimAgent protocol is not supported by this client".into(),
        ));
    }
    if provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("dim_oauth"))
        .is_none_or(|id| id.is_empty())
    {
        return Err(AppError::Config(
            "DimAgent account binding is required".into(),
        ));
    }
    Ok(format)
}

fn gateway_key(db: &Database, app: &AppType, id: &str) -> Result<String, AppError> {
    let _guard = KEY_CREATION
        .lock()
        .map_err(|_| AppError::Config("DimAgent gateway key lock failed".into()))?;
    let master = match db.get_setting(MASTER_KEY)? {
        Some(key) if key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()) => key,
        Some(_) => {
            return Err(AppError::Config(
                "Invalid DimAgent gateway master key".into(),
            ))
        }
        None => {
            let key = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            db.set_setting(MASTER_KEY, &key)?;
            key
        }
    };
    let mut mac =
        Hmac::<Sha256>::new_from_slice(master.as_bytes()).expect("HMAC accepts every key size");
    mac.update(app.as_str().as_bytes());
    mac.update(&[0]);
    mac.update(id.as_bytes());
    Ok(format!(
        "ccs-dim-{}",
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
}

fn loopback_host(address: &str, port: u16) -> Result<String, AppError> {
    let ip = match address.trim_matches(['[', ']']).parse::<IpAddr>() {
        Ok(ip) => ip,
        Err(_) if address == "localhost" => (address, port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut entries| entries.next())
            .map(|v| v.ip())
            .ok_or_else(|| AppError::Config("Cannot resolve DimAgent loopback listener".into()))?,
        Err(_) => {
            return Err(AppError::Config(
                "DimAgent requires a loopback or wildcard proxy listener".into(),
            ))
        }
    };
    let ip = if ip.is_unspecified() {
        if ip.is_ipv4() {
            "127.0.0.1".parse().unwrap()
        } else {
            "::1".parse().unwrap()
        }
    } else {
        ip
    };
    if !ip.to_canonical().is_loopback() {
        return Err(AppError::Config(
            "DimAgent requires a loopback or wildcard proxy listener".into(),
        ));
    }
    Ok(if ip.is_ipv6() {
        format!("[{ip}]")
    } else {
        ip.to_string()
    })
}

/// The stored row remains canonical. Only the native client projection has local credentials.
pub(crate) fn project_provider(
    db: &Database,
    app: &AppType,
    provider: &Provider,
) -> Result<Provider, AppError> {
    if !provider.is_dim_oauth() || !is_relay_client(app) {
        return Ok(provider.clone());
    }
    let format = api_format(app, provider)?;
    let config = futures::executor::block_on(db.get_proxy_config())?;
    let port = if config.listen_port == 0 {
        return Err(AppError::Config(
            "Start the DimAgent gateway before writing client configuration".into(),
        ));
    } else {
        config.listen_port
    };
    let host = loopback_host(&config.listen_address, port)?;
    let id = url::form_urlencoded::byte_serialize(provider.id.as_bytes())
        .collect::<String>()
        .replace('+', "%20");
    let suffix = match format {
        "openai_chat" | "openai_responses" => "/v1",
        "anthropic" if *app == AppType::OpenCode => "/v1",
        "gemini_native" if !matches!(app, AppType::Agy | AppType::Kimi) => "/v1beta",
        _ => "",
    };
    let base = format!(
        "http://{host}:{port}/managed/dim/{}/{id}{suffix}",
        app.as_str()
    );
    let key = gateway_key(db, app, &provider.id)?;
    let mut projected = provider.clone();
    let value = &mut projected.settings_config;
    match app {
        AppType::OpenCode => {
            let field = if provider.opencode_config_format()
                == Some(crate::provider::OpenCodeConfigFormat::V2)
            {
                "settings"
            } else {
                "options"
            };
            value[field]["baseURL"] = json!(base);
            value[field]["apiKey"] = json!(key);
        }
        AppType::Mcode | AppType::Dsh => {
            value["options"]["baseURL"] = json!(base);
            value["options"]["apiKey"] = json!(key);
        }
        AppType::OpenClaw | AppType::Pi | AppType::Omp => {
            value["baseUrl"] = json!(base);
            value["apiKey"] = json!(key);
        }
        AppType::Hermes => {
            value["base_url"] = json!(base);
            value["api_key"] = json!(key);
        }
        AppType::Kimi => {
            value["provider"]["base_url"] = json!(base);
            value["provider"]["api_key"] = json!(key);
        }
        AppType::Agy => {
            value["env"]["GOOGLE_GEMINI_BASE_URL"] = json!(base);
            value["env"]["GEMINI_API_KEY"] = json!(key);
        }
        _ => unreachable!("relay client was checked"),
    }
    Ok(projected)
}

pub(crate) async fn ensure_started(
    state: &AppState,
    app: &AppType,
    provider: &Provider,
) -> Result<(), String> {
    if !provider.is_dim_oauth() || !is_relay_client(app) {
        return Ok(());
    }
    api_format(app, provider).map_err(|e| e.to_string())?;
    let config = state
        .db
        .get_proxy_config()
        .await
        .map_err(|e| e.to_string())?;
    loopback_host(&config.listen_address, config.listen_port).map_err(|e| e.to_string())?;
    state.proxy_service.start().await?;
    Ok(())
}

pub(crate) fn client_exists(app: &AppType, id: &str) -> Result<bool, AppError> {
    Ok(match app {
        AppType::OpenCode => crate::opencode_config::get_providers()?.contains_key(id),
        AppType::OpenClaw => crate::openclaw_config::get_providers()?.contains_key(id),
        AppType::Hermes => crate::hermes_config::get_providers()?.contains_key(id),
        AppType::Pi => crate::pi_config::pi_provider_exists(id)?,
        AppType::Omp => crate::omp_config::get_providers()?.contains_key(id),
        AppType::Mcode => crate::mcode_config::get_providers()?.contains_key(id),
        AppType::Dsh => crate::dsh_config::get_providers()?.contains_key(id),
        AppType::Kimi => crate::kimi_config::get_providers()?.contains_key(id),
        AppType::Agy => crate::agy_config::key_profile_dir(id).exists(),
        _ => false,
    })
}

pub(crate) fn has_clients(db: &Database) -> Result<bool, AppError> {
    for app in AppType::all().filter(is_relay_client) {
        for provider in db
            .get_all_providers(app.as_str())?
            .values()
            .filter(|p| p.is_dim_oauth())
        {
            if client_exists(&app, &provider.id)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Reproject only configured native rows; no local credential is backfilled to the database.
pub(crate) fn refresh_clients(db: &Database) -> Result<(), AppError> {
    let mut clients = Vec::new();
    // Resolve and validate every source before touching any file.
    for app in AppType::all().filter(is_relay_client) {
        for provider in db
            .get_all_providers(app.as_str())?
            .into_values()
            .filter(Provider::is_dim_oauth)
        {
            if client_exists(&app, &provider.id)? {
                let projected = project_provider(db, &app, &provider)?;
                if app != AppType::Agy {
                    clients.push((app.clone(), projected));
                }
            }
        }
    }
    for (app, provider) in clients {
        let id = &provider.id;
        let opencode_format = provider
            .opencode_config_format()
            .unwrap_or(crate::provider::OpenCodeConfigFormat::V1);
        let config = provider.settings_config;
        match app {
            AppType::OpenCode => {
                crate::opencode_config::set_provider_with_format(id, config, opencode_format)?;
            }
            AppType::OpenClaw => {
                crate::openclaw_config::set_provider(id, config)?;
            }
            AppType::Hermes => {
                crate::hermes_config::set_provider(id, config)?;
            }
            AppType::Pi => {
                crate::pi_config::replace_pi_provider_if_present(id, &config)?;
            }
            AppType::Omp => {
                crate::omp_config::change_and_commit(
                    id,
                    Some(&config),
                    crate::omp_config::Change::ReplaceIfPresent,
                    |_| Ok(()),
                )?;
            }
            AppType::Mcode => crate::mcode_config::set_provider(id, config)?,
            AppType::Dsh => crate::dsh_config::set_provider(id, config)?,
            AppType::Kimi => crate::kimi_config::set_provider(id, config)?,
            _ => unreachable!("only additive clients have live gateway projections"),
        }
    }
    Ok(())
}

pub(crate) async fn restore_clients(state: &AppState) -> Result<(), String> {
    if has_clients(&state.db).map_err(|e| e.to_string())? {
        state.proxy_service.start().await?;
    }
    Ok(())
}

type RelayError = (StatusCode, Json<Value>);
fn error(status: StatusCode, message: &str) -> RelayError {
    (
        status,
        Json(json!({"error":{"type":"dim_gateway_error", "message":message}})),
    )
}

fn allowed_path(format: &str, method: &Method, path: &str) -> bool {
    if method == Method::GET {
        return if format == "gemini_native" {
            matches!(path, "/v1beta/models" | "/v1/models")
                || path.starts_with("/v1beta/models/")
                || path.starts_with("/v1/models/")
        } else {
            path == "/v1/models" || path.starts_with("/v1/models/")
        };
    }
    if method != Method::POST {
        return false;
    }
    match format {
        "openai_chat" => path == "/v1/chat/completions",
        "openai_responses" => matches!(path, "/v1/responses" | "/v1/responses/compact"),
        "anthropic" => matches!(path, "/v1/messages" | "/v1/messages/count_tokens"),
        "gemini_native" => {
            (path.starts_with("/v1beta/models/") || path.starts_with("/v1/models/"))
                && [":generateContent", ":streamGenerateContent", ":countTokens"]
                    .iter()
                    .any(|suffix| path.ends_with(suffix))
        }
        _ => false,
    }
}

fn authenticate(headers: &HeaderMap, query: Option<&str>, expected: &str) -> bool {
    let mut keys = Vec::new();
    for name in ["authorization", "x-api-key", "x-goog-api-key"] {
        for value in headers.get_all(name) {
            let Ok(value) = value.to_str() else {
                return false;
            };
            let value = if name == "authorization" {
                let Some(v) = value.strip_prefix("Bearer ") else {
                    return false;
                };
                v
            } else {
                value
            };
            keys.push(value.to_string());
        }
    }
    for (name, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        if name.eq_ignore_ascii_case("key") {
            keys.push(value.into_owned());
        }
    }
    !keys.is_empty()
        && keys
            .iter()
            .all(|value| value.as_bytes().ct_eq(expected.as_bytes()).into())
}

pub(crate) async fn handle(
    State(state): State<ProxyState>,
    Path((app, id, _tail)): Path<(String, String, String)>,
    request: Request,
) -> Result<Response, RelayError> {
    let (app, provider) = authorize_request(&state.db, &app, &id, &request)?;
    let handle = state.app_handle.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "DimAgent account manager is unavailable",
        )
    })?;
    let auth = handle.state::<crate::commands::DimOAuthState>();
    let manager = auth
        .manager()
        .map_err(|_| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "DimAgent account storage is unavailable",
            )
        })?
        .clone();
    relay(&manager, &app, &provider, request).await
}

fn authorize_request(
    db: &Database,
    app: &str,
    id: &str,
    request: &Request,
) -> Result<(AppType, Provider), RelayError> {
    let Some(peer) = request.extensions().get::<ConnectInfo<SocketAddr>>() else {
        return Err(error(
            StatusCode::FORBIDDEN,
            "DimAgent gateway requires a local client",
        ));
    };
    if !peer.0.ip().to_canonical().is_loopback() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "DimAgent gateway only accepts loopback clients",
        ));
    }
    let app =
        AppType::from_str(&app).map_err(|_| error(StatusCode::NOT_FOUND, "Unknown client"))?;
    if !is_relay_client(&app) {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Client does not use this gateway",
        ));
    }
    let provider = db
        .get_provider_by_id(&id, app.as_str())
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Cannot load DimAgent provider",
            )
        })?
        .filter(Provider::is_dim_oauth)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "DimAgent provider was removed"))?;
    let key = gateway_key(db, &app, &id).map_err(|_| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "DimAgent gateway key is unavailable",
        )
    })?;
    if !authenticate(request.headers(), request.uri().query(), &key) {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "Invalid DimAgent local gateway credential",
        ));
    }
    Ok((app, provider))
}

async fn relay(
    manager: &DimOAuthManager,
    app: &AppType,
    provider: &Provider,
    request: Request,
) -> Result<Response, RelayError> {
    let format = api_format(app, provider).map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "Invalid DimAgent protocol or account binding",
        )
    })?;
    let raw_path = request
        .uri()
        .path()
        .splitn(6, '/')
        .nth(5)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Invalid DimAgent endpoint"))?;
    let path = format!("/{raw_path}");
    if !allowed_path(format, request.method(), &path) {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Unsupported DimAgent model endpoint",
        ));
    }
    let account = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("dim_oauth"))
        .expect("binding checked");
    let auth = manager.get_request_auth(&account).await.map_err(|_| {
        error(
            StatusCode::UNAUTHORIZED,
            "DimAgent authentication failed; check Accounts in CC Switch",
        )
    })?;
    let mut url = Url::parse(&auth.issuer)
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Invalid DimAgent issuer"))?;
    url.set_path(&path);
    if !allowed_path(format, request.method(), url.path()) {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Unsupported DimAgent model endpoint",
        ));
    }
    let query: Vec<_> =
        url::form_urlencoded::parse(request.uri().query().unwrap_or_default().as_bytes())
            .filter(|(name, _)| {
                !matches!(
                    name.to_ascii_lowercase().as_str(),
                    "key" | "api_key" | "access_token" | "token"
                )
            })
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(
            query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
    }
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, MAX_BODY).await.map_err(|_| {
        error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "DimAgent request body is too large",
        )
    })?;
    let mut streaming = path.contains(":streamGenerateContent");
    let body = if parts.method == Method::POST {
        let mut payload: Value = serde_json::from_slice(&bytes)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid DimAgent JSON request"))?;
        if !payload.is_object() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "DimAgent model requests must be JSON objects",
            ));
        }
        streaming |= payload.get("stream").and_then(Value::as_bool) == Some(true);
        if matches!(
            path.as_str(),
            "/v1/chat/completions" | "/v1/responses" | "/v1/messages"
        ) {
            apply_dim_effort(
                payload
                    .as_object_mut()
                    .expect("request boundary checked object"),
                format,
                provider
                    .meta
                    .as_ref()
                    .and_then(|m| m.dim_oauth_effort.as_deref()),
            );
        }
        serde_json::to_vec(&payload)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid DimAgent request"))?
    } else {
        bytes.to_vec()
    };
    let client = super::http_client::no_redirect().map_err(|_| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "DimAgent HTTP client is unavailable",
        )
    })?;
    let mut outgoing = client
        .request(parts.method, url)
        .bearer_auth(auth.access_token)
        .body(body);
    if streaming {
        outgoing = outgoing.timeout(Duration::from_secs(24 * 60 * 60));
    }
    for name in [
        "content-type",
        "accept",
        "anthropic-version",
        "anthropic-beta",
        "user-agent",
        "x-goog-api-client",
        "x-session-id",
        "x-client-request-id",
        "x-opencode-session",
    ] {
        if let Some(value) = parts.headers.get(name) {
            outgoing = outgoing.header(name, value);
        }
    }
    if format == "anthropic" && !parts.headers.contains_key("anthropic-version") {
        outgoing = outgoing.header("anthropic-version", "2023-06-01");
    }
    let response = outgoing
        .send()
        .await
        .map_err(|_| error(StatusCode::BAD_GATEWAY, "DimAgent upstream request failed"))?;
    let status = response.status();
    if status.is_redirection() {
        return Err(error(
            StatusCode::BAD_GATEWAY,
            "DimAgent upstream redirects are not permitted",
        ));
    }
    let mut headers = response.headers().clone();
    for name in [
        "connection",
        "transfer-encoding",
        "authorization",
        "x-api-key",
        "x-goog-api-key",
        "set-cookie",
    ] {
        headers.remove(name);
    }
    let mut result = Response::new(Body::from_stream(response.bytes_stream()));
    *result.status_mut() = status;
    *result.headers_mut() = headers;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{response::IntoResponse, routing::any, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn provider(id: &str, format: &str) -> Provider {
        let mut p = Provider::with_id(
            id.into(),
            "DimAgent".into(),
            json!({"options":{"baseURL":"https://dimagent.com/v1","apiKey":"PROXY_MANAGED"}}),
            None,
        );
        p.meta = Some(serde_json::from_value(json!({
            "providerType":"dim_oauth", "apiFormat":format, "dimOauthEffort":"high",
            "authBinding":{"source":"managed_account","authProvider":"dim_oauth","accountId":"fixture"}
        })).unwrap());
        p
    }

    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    async fn serve(router: Router) -> (String, Server) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        (url, Server(task))
    }

    #[tokio::test]
    async fn dim_gateway_client_projection_authentication_and_streaming_flow() {
        let db = Arc::new(Database::memory().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let released = Arc::new(tokio::sync::Notify::new());
        let captured = Arc::new(Mutex::new(Vec::<(String, HeaderMap, Value)>::new()));
        let (upstream, _upstream) = serve(Router::new().fallback(any({
            let calls = calls.clone();
            let captured = captured.clone();
            let released = released.clone();
            move |request: Request| {
                let calls = calls.clone();
                let captured = captured.clone();
                let released = released.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let uri = request.uri().to_string();
                    let headers = request.headers().clone();
                    let body: Value = serde_json::from_slice(
                        &to_bytes(request.into_body(), MAX_BODY).await.unwrap(),
                    )
                    .unwrap_or(Value::Null);
                    captured.lock().unwrap().push((uri, headers, body.clone()));
                    if body["redirect"] == true {
                        return (
                            StatusCode::TEMPORARY_REDIRECT,
                            [("location", "http://127.0.0.1:1/should-never-follow")],
                        )
                            .into_response();
                    }
                    if body["stream"] == true {
                        let stream = futures::stream::unfold(0, move |step| {
                            let released = released.clone();
                            async move {
                                match step {
                                    0 => Some((Ok::<_, std::io::Error>("data: first\n\n"), 1)),
                                    1 => {
                                        released.notified().await;
                                        Some((Ok("data: [DONE]\n\n"), 2))
                                    }
                                    _ => None,
                                }
                            }
                        });
                        return Response::new(Body::from_stream(stream));
                    }
                    Json(json!({"ok":true})).into_response()
                }
            }
        })))
        .await;
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("dim_oauth_auth.json"), serde_json::to_vec(&json!({
            "accounts":{"fixture":{"account":{"id":"fixture","site":"global","login":"fixture","authenticated_at":1,"requires_reauth":false},"access_token":"private-fixture-token","refresh_token":"private-fixture-refresh","expires_at":u64::MAX}},
            "default_account_id":"fixture"
        })).unwrap()).unwrap();
        let mut manager = DimOAuthManager::new(directory.path().to_path_buf()).unwrap();
        manager.registration = Some((upstream, "http://127.0.0.1:1/oauth/callback".into()));
        let manager = Arc::new(manager);
        let (gateway, _gateway) = serve(Router::new().route(
            "/managed/dim/:app/:provider/*tail",
            any({
                let db = db.clone();
                let manager = manager.clone();
                move |Path((app, id, _)): Path<(String, String, String)>, request: Request| {
                    let db = db.clone();
                    let manager = manager.clone();
                    async move {
                        match authorize_request(&db, &app, &id, &request) {
                            Ok((app, provider)) => relay(&manager, &app, &provider, request)
                                .await
                                .into_response(),
                            Err(error) => error.into_response(),
                        }
                    }
                }
            }),
        ))
        .await;
        let client = reqwest::Client::new();
        let cases = [
            (
                AppType::Mcode,
                "openai_chat",
                "options",
                "baseURL",
                "/chat/completions",
                "/v1/chat/completions",
            ),
            (
                AppType::OpenCode,
                "anthropic",
                "options",
                "baseURL",
                "/messages",
                "/v1/messages",
            ),
            (
                AppType::Pi,
                "gemini_native",
                "",
                "baseUrl",
                "/models/vendor%2Ffull-id:generateContent",
                "/v1beta/models/vendor%2Ffull-id:generateContent",
            ),
            (
                AppType::Kimi,
                "gemini_native",
                "provider",
                "base_url",
                "/v1beta/models/vendor%2Ffull-id:generateContent",
                "/v1beta/models/vendor%2Ffull-id:generateContent",
            ),
            (
                AppType::Hermes,
                "openai_responses",
                "",
                "base_url",
                "/responses",
                "/v1/responses",
            ),
        ];
        for (index, (app, format, container, field, endpoint, expected)) in
            cases.into_iter().enumerate()
        {
            let p = provider(&format!("fixture-{index}"), format);
            db.save_provider(app.as_str(), &p).unwrap();
            let projected = project_provider(&db, &app, &p).unwrap();
            let config = if container.is_empty() {
                &projected.settings_config
            } else {
                &projected.settings_config[container]
            };
            let base = config[field].as_str().unwrap();
            let path = Url::parse(base).unwrap().path().to_string();
            let key = gateway_key(&db, &app, &p.id).unwrap();
            let response = client
                .post(format!(
                    "{gateway}{path}{endpoint}?key={key}&API_KEY=do-not-forward&alt=sse"
                ))
                .header("x-api-key", &key)
                .json(&json!({"model":"vendor/full-id","messages":[]}))
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{app:?}: {}",
                response.text().await.unwrap()
            );
            let capture = captured.lock().unwrap();
            let (uri, headers, body) = capture.last().unwrap();
            assert_eq!(uri, &format!("{expected}?alt=sse"));
            assert_eq!(headers["authorization"], "Bearer private-fixture-token");
            assert!(!headers.contains_key("x-api-key"));
            assert_eq!(body["model"], "vendor/full-id");
            assert_eq!(
                db.get_provider_by_id(&p.id, app.as_str())
                    .unwrap()
                    .unwrap()
                    .settings_config,
                p.settings_config
            );
        }
        let p = provider("stream", "openai_chat");
        db.save_provider("mcode", &p).unwrap();
        let key = gateway_key(&db, &AppType::Mcode, &p.id).unwrap();
        let url = format!("{gateway}/managed/dim/mcode/stream/v1/chat/completions");
        let before = calls.load(Ordering::SeqCst);
        for body in [json!([]), json!(42)] {
            assert_eq!(
                client
                    .post(&url)
                    .bearer_auth(&key)
                    .json(&body)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            client
                .post(&url)
                .bearer_auth("wrong")
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .post(url.replace("/v1/chat/completions", "/api/me/usage"))
                .bearer_auth(&key)
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(calls.load(Ordering::SeqCst), before);
        assert_eq!(
            client
                .post(&url)
                .bearer_auth(&key)
                .json(&json!({"redirect":true}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_GATEWAY
        );
        let mut response = client
            .post(&url)
            .bearer_auth(&key)
            .json(&json!({"stream":true}))
            .send()
            .await
            .unwrap();
        let first = tokio::time::timeout(Duration::from_secs(3), response.chunk())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(first, "data: first\n\n");
        released.notify_one();
        assert_eq!(response.chunk().await.unwrap().unwrap(), "data: [DONE]\n\n");
        assert_eq!(
            captured.lock().unwrap().last().unwrap().2["reasoning_effort"],
            "high"
        );
        manager.remove_account("fixture").await.unwrap();
        assert_eq!(
            client
                .post(&url)
                .bearer_auth(&key)
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    #[serial_test::serial]
    async fn dim_gateway_live_membership_and_port_restart_preserve_canonical_rows() {
        use crate::services::ProviderService;
        let _pi = crate::pi_config::test_support::TestAgentDir::new();
        let _omp = crate::omp_config::test_support::TestAgentDir::new();
        let db = Arc::new(Database::memory().unwrap());
        let state = AppState::new(db.clone());
        let mut config = db.get_proxy_config().await.unwrap();
        config.listen_address = "127.0.0.1".into();
        config.listen_port = 0;
        db.update_proxy_config(config).await.unwrap();
        let first = state.proxy_service.start().await.unwrap();
        for app in [AppType::Pi, AppType::Omp] {
            let mut p = provider("dim-live-fixture", "openai_chat");
            p.settings_config = json!({
                "name":"DimAgent", "baseUrl":"https://dimagent.com/v1", "apiKey":"PROXY_MANAGED",
                "api":"openai-completions", "models":[{"id":"vendor/full-id"}]
            });
            ProviderService::add(&state, app.clone(), p, false).unwrap();
            assert!(!client_exists(&app, "dim-live-fixture").unwrap());
            assert!(!has_clients(&db).unwrap());
        }
        let canonical: Vec<_> = [AppType::Pi, AppType::Omp]
            .iter()
            .map(|app| {
                db.get_provider_by_id("dim-live-fixture", app.as_str())
                    .unwrap()
                    .unwrap()
            })
            .collect();
        for app in [AppType::Pi, AppType::Omp] {
            ProviderService::switch(&state, app.clone(), "dim-live-fixture").unwrap();
            ProviderService::list(&state, app.clone()).unwrap();
        }
        assert!(has_clients(&db).unwrap());
        let native = || {
            [
                crate::pi_config::read_pi_native_provider("dim-live-fixture")
                    .unwrap()
                    .unwrap(),
                crate::omp_config::get_providers().unwrap()["dim-live-fixture"].clone(),
            ]
        };
        for row in native() {
            assert!(row["baseUrl"]
                .as_str()
                .unwrap()
                .contains(&format!(":{}/managed/dim/", first.port)));
            assert!(row["apiKey"].as_str().unwrap().starts_with("ccs-dim-"));
        }
        let mut changed = db.get_proxy_config().await.unwrap();
        let available = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        changed.listen_port = available.local_addr().unwrap().port();
        drop(available);
        assert!(state.proxy_service.update_config(&changed).await.unwrap());
        for row in native() {
            assert!(row["baseUrl"]
                .as_str()
                .unwrap()
                .contains(&format!(":{}/managed/dim/", changed.listen_port)));
        }
        let before_failure = native();
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut rejected = changed.clone();
        rejected.listen_port = occupied.local_addr().unwrap().port();
        assert!(state.proxy_service.update_config(&rejected).await.is_err());
        assert_eq!(native(), before_failure);
        assert_eq!(
            db.get_proxy_config().await.unwrap().listen_port,
            changed.listen_port
        );
        for (app, expected) in [AppType::Pi, AppType::Omp].into_iter().zip(canonical) {
            ProviderService::remove_from_live_config(&state, app.clone(), "dim-live-fixture")
                .unwrap();
            let saved = db
                .get_provider_by_id("dim-live-fixture", app.as_str())
                .unwrap()
                .unwrap();
            assert_eq!(saved.settings_config, expected.settings_config);
            assert_eq!(
                serde_json::to_value(saved.meta).unwrap(),
                serde_json::to_value(expected.meta).unwrap()
            );
            ProviderService::switch(&state, app.clone(), "dim-live-fixture").unwrap();
            ProviderService::list(&state, app.clone()).unwrap();
            let saved = db
                .get_provider_by_id("dim-live-fixture", app.as_str())
                .unwrap()
                .unwrap();
            assert_eq!(saved.settings_config, expected.settings_config);
            ProviderService::remove_from_live_config(&state, app.clone(), "dim-live-fixture")
                .unwrap();
        }
        assert!(!has_clients(&db).unwrap());
        state.proxy_service.stop().await.unwrap();
    }
}
