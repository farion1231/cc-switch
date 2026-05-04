use async_stream::stream;
use axum::{
    body::Body,
    extract::State as AxumState,
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::sse::{Event as SseEvent, KeepAlive, Sse},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use cc_switch_lib::{
    create_copilot_auth_state, AppSettings, AppState, AppType, CopilotAuthState, Database,
    ImportSkillSelection, McpServer, McpService, PromptService, Provider, ProviderService,
    ProviderSortUpdate, SkillService, UniversalProvider, WebUiAuthMode,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

mod webui_assets {
    include!(concat!(env!("OUT_DIR"), "/webui_assets.rs"));
}

const DEFAULT_WEBUI_HOST: &str = "127.0.0.1";
const DEFAULT_WEBUI_PORT: u16 = 9990;

fn main() {
    let args = std::env::args().skip(1).collect();
    std::process::exit(run_cli(args));
}

#[derive(Debug, PartialEq, Eq)]
enum CliCommand {
    Help,
    ProviderHelp,
    WebUi(WebUiOptions),
    Providers(ProviderCommand),
}

#[derive(Debug, PartialEq, Eq)]
struct WebUiOptions {
    host: String,
    port: u16,
    auth_mode: Option<WebUiAuthMode>,
    token: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum ProviderCommand {
    List { app_type: AppType },
    Current { app_type: AppType },
    Switch { app_type: AppType, id: String },
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedOptions {
    app: Option<String>,
    id: Option<String>,
    help: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct CliError {
    message: String,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub fn run_cli(args: Vec<String>) -> i32 {
    let command = match parse_cli_command(&args) {
        Ok(command) => command,
        Err(err) => {
            eprintln!("error: {}", err.message);
            eprintln!();
            eprintln!("{}", top_level_help());
            return 1;
        }
    };

    if let CliCommand::WebUi(options) = command {
        return match run_webui_blocking(options) {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("error: {}", err.message);
                1
            }
        };
    }

    match execute_command(command) {
        Ok(output) => {
            print!("{output}");
            0
        }
        Err(err) => {
            eprintln!("error: {}", err.message);
            eprintln!();
            eprintln!("{}", top_level_help());
            1
        }
    }
}

fn parse_cli_command(args: &[String]) -> Result<CliCommand, CliError> {
    match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") | Some("-h") => Ok(CliCommand::Help),
        Some("webui") => parse_webui_command(&args[1..]),
        Some("providers") | Some("provider") => parse_provider_command(&args[1..]),
        Some(command) => Err(CliError::new(format!("unknown cli command '{command}'"))),
    }
}

fn parse_webui_command(args: &[String]) -> Result<CliCommand, CliError> {
    let mut host = DEFAULT_WEBUI_HOST.to_string();
    let mut port = DEFAULT_WEBUI_PORT;
    let mut auth_mode: Option<WebUiAuthMode> = None;
    let mut token: Option<String> = None;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if matches!(arg.as_str(), "--help" | "-h") {
            return Ok(CliCommand::Help);
        }

        if let Some((key, value)) = arg.split_once('=') {
            match key {
                "--host" => host = parse_non_empty(value, "--host")?,
                "--port" => port = parse_port(value)?,
                "--auth" => auth_mode = Some(parse_webui_auth_mode(value)?),
                "--token" => token = Some(parse_non_empty(value, "--token")?),
                _ => return Err(CliError::new(format!("unknown option '{key}'"))),
            }
            index += 1;
            continue;
        }

        match arg.as_str() {
            "--host" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| CliError::new("missing value for --host"))?;
                host = parse_non_empty(value, "--host")?;
                index += 1;
            }
            "--port" | "-p" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| CliError::new("missing value for --port"))?;
                port = parse_port(value)?;
                index += 1;
            }
            "--auth" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| CliError::new("missing value for --auth"))?;
                auth_mode = Some(parse_webui_auth_mode(value)?);
                index += 1;
            }
            "--token" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| CliError::new("missing value for --token"))?;
                token = Some(parse_non_empty(value, "--token")?);
                index += 1;
            }
            _ if arg.starts_with('-') => {
                return Err(CliError::new(format!("unknown option '{arg}'")));
            }
            _ => {
                return Err(CliError::new(format!(
                    "unexpected positional argument '{arg}'"
                )));
            }
        }
    }

    if token.is_some() && matches!(auth_mode, Some(WebUiAuthMode::None)) {
        return Err(CliError::new("--token requires --auth token"));
    }
    if token.is_some() && auth_mode.is_none() {
        auth_mode = Some(WebUiAuthMode::Token);
    }

    Ok(CliCommand::WebUi(WebUiOptions {
        host,
        port,
        auth_mode,
        token,
    }))
}

fn parse_webui_auth_mode(value: &str) -> Result<WebUiAuthMode, CliError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "off" | "disabled" => Ok(WebUiAuthMode::None),
        "token" | "on" | "enabled" => Ok(WebUiAuthMode::Token),
        other => Err(CliError::new(format!(
            "invalid --auth '{other}', expected 'none' or 'token'"
        ))),
    }
}

fn parse_provider_command(args: &[String]) -> Result<CliCommand, CliError> {
    match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") | Some("-h") => Ok(CliCommand::ProviderHelp),
        Some("list") => {
            let options = parse_options(&args[1..])?;
            if options.help {
                return Ok(CliCommand::ProviderHelp);
            }
            reject_unused_id(&options, "providers list")?;
            Ok(CliCommand::Providers(ProviderCommand::List {
                app_type: parse_required_app(options.app)?,
            }))
        }
        Some("current") => {
            let options = parse_options(&args[1..])?;
            if options.help {
                return Ok(CliCommand::ProviderHelp);
            }
            reject_unused_id(&options, "providers current")?;
            Ok(CliCommand::Providers(ProviderCommand::Current {
                app_type: parse_required_app(options.app)?,
            }))
        }
        Some("switch") => {
            let options = parse_options(&args[1..])?;
            if options.help {
                return Ok(CliCommand::ProviderHelp);
            }
            Ok(CliCommand::Providers(ProviderCommand::Switch {
                app_type: parse_required_app(options.app)?,
                id: parse_required_option(options.id, "--id")?,
            }))
        }
        Some(command) => Err(CliError::new(format!(
            "unknown providers command '{command}'"
        ))),
    }
}

fn parse_options(args: &[String]) -> Result<ParsedOptions, CliError> {
    let mut options = ParsedOptions::default();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if matches!(arg.as_str(), "--help" | "-h") {
            options.help = true;
            index += 1;
            continue;
        }

        if let Some((key, value)) = arg.split_once('=') {
            match key {
                "--app" => set_option(&mut options.app, value, "--app")?,
                "--id" => set_option(&mut options.id, value, "--id")?,
                _ => return Err(CliError::new(format!("unknown option '{key}'"))),
            }
            index += 1;
            continue;
        }

        match arg.as_str() {
            "--app" | "-a" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| CliError::new("missing value for --app"))?;
                set_option(&mut options.app, value, "--app")?;
                index += 1;
            }
            "--id" | "-i" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| CliError::new("missing value for --id"))?;
                set_option(&mut options.id, value, "--id")?;
                index += 1;
            }
            _ if arg.starts_with('-') => {
                return Err(CliError::new(format!("unknown option '{arg}'")));
            }
            _ => {
                return Err(CliError::new(format!(
                    "unexpected positional argument '{arg}'"
                )));
            }
        }
    }

    Ok(options)
}

fn set_option(slot: &mut Option<String>, value: &str, name: &str) -> Result<(), CliError> {
    if value.trim().is_empty() {
        return Err(CliError::new(format!("missing value for {name}")));
    }
    if slot.is_some() {
        return Err(CliError::new(format!("duplicate option {name}")));
    }
    *slot = Some(value.to_string());
    Ok(())
}

fn parse_non_empty(value: &str, name: &str) -> Result<String, CliError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CliError::new(format!("missing value for {name}")));
    }
    Ok(trimmed.to_string())
}

fn parse_port(value: &str) -> Result<u16, CliError> {
    let port = value
        .parse::<u16>()
        .map_err(|_| CliError::new(format!("invalid port '{value}'")))?;
    if port == 0 {
        return Err(CliError::new("port must be greater than 0"));
    }
    Ok(port)
}

fn reject_unused_id(options: &ParsedOptions, command: &str) -> Result<(), CliError> {
    if options.id.is_some() {
        return Err(CliError::new(format!("{command} does not accept --id")));
    }
    Ok(())
}

fn parse_required_app(app: Option<String>) -> Result<AppType, CliError> {
    let app = parse_required_option(app, "--app")?;
    AppType::from_str(&app).map_err(|err| CliError::new(err.to_string()))
}

fn parse_required_option(value: Option<String>, name: &str) -> Result<String, CliError> {
    value.ok_or_else(|| CliError::new(format!("missing required option {name}")))
}

fn execute_command(command: CliCommand) -> Result<String, CliError> {
    match command {
        CliCommand::Help => Ok(top_level_help()),
        CliCommand::ProviderHelp => Ok(provider_help()),
        CliCommand::WebUi(_) => unreachable!("webui command is handled before execute_command"),
        CliCommand::Providers(command) => execute_provider_command(command),
    }
}

fn execute_provider_command(command: ProviderCommand) -> Result<String, CliError> {
    let state = build_state()?;
    init_provider_data(&state);

    match command {
        ProviderCommand::List { app_type } => {
            let providers = ProviderService::list(&state, app_type.clone())
                .map_err(|err| CliError::new(err.to_string()))?;
            let current = if app_type.is_additive_mode() {
                String::new()
            } else {
                ProviderService::current(&state, app_type.clone())
                    .map_err(|err| CliError::new(err.to_string()))?
            };
            Ok(format_provider_list(&app_type, &providers, &current))
        }
        ProviderCommand::Current { app_type } => {
            if app_type.is_additive_mode() {
                return Ok(format!(
                    "{} uses additive provider config and has no single current provider.\n",
                    app_type.as_str()
                ));
            }

            let current = ProviderService::current(&state, app_type.clone())
                .map_err(|err| CliError::new(err.to_string()))?;
            if current.is_empty() {
                Ok(format!(
                    "No current provider is set for {}.\n",
                    app_type.as_str()
                ))
            } else {
                Ok(format!("{current}\n"))
            }
        }
        ProviderCommand::Switch { app_type, id } => {
            let result = ProviderService::switch(&state, app_type.clone(), &id)
                .map_err(|err| CliError::new(err.to_string()))?;
            let mut output = format!("Switched {} to provider '{id}'.\n", app_type.as_str());

            for warning in result.warnings {
                output.push_str("warning: ");
                output.push_str(&warning);
                output.push('\n');
            }

            Ok(output)
        }
    }
}

fn build_state() -> Result<AppState, CliError> {
    let db = Database::init()
        .map_err(|err| CliError::new(format!("failed to initialize database: {err}")))?;
    Ok(AppState::new(Arc::new(db)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartupProviderImport {
    Default(AppType),
    SeedOfficial,
    OpenCodeLive,
    OpenClawLive,
    HermesLive,
}

fn startup_provider_import_plan() -> Vec<StartupProviderImport> {
    let mut plan: Vec<_> = AppType::all()
        .filter(|app| !app.is_additive_mode())
        .map(StartupProviderImport::Default)
        .collect();
    plan.extend([
        StartupProviderImport::SeedOfficial,
        StartupProviderImport::OpenCodeLive,
        StartupProviderImport::OpenClawLive,
        StartupProviderImport::HermesLive,
    ]);
    plan
}

fn init_provider_data(state: &AppState) {
    for step in startup_provider_import_plan() {
        match step {
            StartupProviderImport::Default(app_type) => {
                let _ = ProviderService::import_default_config(state, app_type);
            }
            StartupProviderImport::SeedOfficial => {
                let _ = state.db.init_default_official_providers();
            }
            StartupProviderImport::OpenCodeLive => {
                let _ = ProviderService::import_opencode_providers_from_live(state);
            }
            StartupProviderImport::OpenClawLive => {
                let _ = ProviderService::import_openclaw_providers_from_live(state);
            }
            StartupProviderImport::HermesLive => {
                let _ = ProviderService::import_hermes_providers_from_live(state);
            }
        }
    }
}

#[derive(Clone)]
struct WebServerState {
    app_state: Arc<AppState>,
    copilot_state: Arc<CopilotAuthState>,
    auth: Arc<WebUiAuth>,
    events: Arc<WebEventBus>,
}

#[derive(Clone)]
struct WebEventBus {
    sender: broadcast::Sender<WebEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebEvent {
    event: String,
    payload: Value,
}

impl WebEventBus {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    fn subscribe(&self) -> broadcast::Receiver<WebEvent> {
        self.sender.subscribe()
    }

    fn publish(&self, event: impl Into<String>, payload: Value) {
        let _ = self.sender.send(WebEvent {
            event: event.into(),
            payload,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WebUiAuth {
    mode: WebUiAuthMode,
    token: Option<String>,
}

impl WebUiAuth {
    fn none() -> Self {
        Self {
            mode: WebUiAuthMode::None,
            token: None,
        }
    }

    fn token(token: String) -> Self {
        Self {
            mode: WebUiAuthMode::Token,
            token: Some(token),
        }
    }

    fn token_value(&self) -> Option<&str> {
        self.token.as_deref()
    }
}

#[derive(Debug, Deserialize)]
struct InvokeRequest {
    cmd: String,
    #[serde(default)]
    args: Value,
}

fn run_webui_blocking(options: WebUiOptions) -> Result<(), CliError> {
    if webui_assets::ASSET_COUNT == 0 {
        return Err(CliError::new(
            "webui assets are not embedded; run `pnpm run build:renderer` before building cc-switch-cli",
        ));
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| CliError::new(format!("failed to start async runtime: {err}")))?;

    runtime.block_on(run_webui_server(options))
}

async fn run_webui_server(options: WebUiOptions) -> Result<(), CliError> {
    let address = format!("{}:{}", options.host, options.port)
        .parse::<SocketAddr>()
        .map_err(|err| CliError::new(format!("invalid listen address: {err}")))?;
    let app_state = Arc::new(init_webui_state()?);
    let copilot_state = Arc::new(create_copilot_auth_state(
        cc_switch_lib::get_app_config_dir_path(),
    ));
    let auth = Arc::new(resolve_webui_auth(&options));
    let events = Arc::new(WebEventBus::new());
    cc_switch_lib::start_worker_with_status_emitter(
        app_state.db.clone(),
        webdav_status_emitter(events.clone()),
    );
    let web_state = WebServerState {
        app_state,
        copilot_state,
        auth: auth.clone(),
        events,
    };

    let app = Router::new()
        .route("/__cc_switch_webui__/invoke", post(web_invoke))
        .route("/__cc_switch_webui__/events", get(web_events))
        .layer(CorsLayer::permissive())
        .fallback(serve_webui_asset)
        .with_state(web_state);

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|err| CliError::new(format!("failed to bind {address}: {err}")))?;
    match auth.token_value() {
        Some(token) => println!("CC Switch WebUI listening on http://{address}/?token={token}"),
        None => println!("CC Switch WebUI listening on http://{address}/"),
    }
    println!("Press Ctrl+C to stop.");

    axum::serve(listener, app)
        .await
        .map_err(|err| CliError::new(format!("webui server failed: {err}")))
}

fn resolve_webui_auth(options: &WebUiOptions) -> WebUiAuth {
    let settings = cc_switch_lib::get_app_settings();
    let mode = options.auth_mode.unwrap_or(settings.webui_auth.mode);
    match mode {
        WebUiAuthMode::None => WebUiAuth::none(),
        WebUiAuthMode::Token => WebUiAuth::token(
            options
                .token
                .clone()
                .or(settings.webui_auth.token)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(generate_webui_auth_token),
        ),
    }
}

fn init_webui_state() -> Result<AppState, CliError> {
    let state = build_state()?;

    let _ = state.db.init_default_skill_repos();
    init_provider_data(&state);

    Ok(state)
}

async fn web_invoke(
    AxumState(state): AxumState<WebServerState>,
    headers: HeaderMap,
    Json(request): Json<InvokeRequest>,
) -> Response {
    if !is_authorized_webui_request(&headers, &state.auth) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "unauthorized WebUI request",
            })),
        )
            .into_response();
    }

    match invoke_web_command(
        &state.app_state,
        &state.copilot_state,
        &state.events,
        &request.cmd,
        request.args,
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
            })),
        )
            .into_response(),
    }
}

async fn web_events(AxumState(state): AxumState<WebServerState>, headers: HeaderMap) -> Response {
    if !is_authorized_webui_request(&headers, &state.auth) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "unauthorized WebUI request",
            })),
        )
            .into_response();
    }

    let mut receiver = state.events.subscribe();
    let event_stream = stream! {
        loop {
            match receiver.recv().await {
                Ok(event) => match serde_json::to_string(&event) {
                    Ok(data) => yield Ok::<SseEvent, Infallible>(SseEvent::default().data(data)),
                    Err(err) => {
                        log::error!("failed to serialize WebUI event: {err}");
                    }
                },
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    log::warn!("WebUI event stream lagged, skipped {skipped} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(event_stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response()
}

async fn serve_webui_asset(uri: Uri) -> Response {
    let path = normalize_request_path(uri.path());
    let asset = webui_assets::get(&path)
        .or_else(|| should_fallback_to_index(&path).then(|| webui_assets::get("index.html"))?);

    match asset {
        Some(asset) if asset.path == "index.html" => html_response(asset.bytes),
        Some(asset) => asset_response(asset.mime, asset.bytes),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn normalize_request_path(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        "index.html".to_string()
    } else {
        trimmed.to_string()
    }
}

fn should_fallback_to_index(path: &str) -> bool {
    !path.contains('.')
}

fn html_response(bytes: &'static [u8]) -> Response {
    let html = String::from_utf8_lossy(bytes);
    let shim = format!("<script>{}</script>", webui_tauri_shim());
    let body = if html.contains("</head>") {
        html.replacen("</head>", &format!("{shim}</head>"), 1)
    } else {
        format!("{shim}{html}")
    };
    asset_response("text/html; charset=utf-8", body.into_bytes())
}

fn asset_response(mime: &'static str, body: impl Into<Vec<u8>>) -> Response {
    let bytes: Vec<u8> = body.into();
    let mut response = Response::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    response
}

fn webui_tauri_shim() -> &'static str {
    r#"
(() => {
  if (window.__TAURI_INTERNALS__) return;
  window.__CC_SWITCH_WEBUI__ = true;
  const params = new URLSearchParams(window.location.search);
  const token = params.get('token') || window.sessionStorage.getItem('cc-switch-webui-token') || '';
  if (token) window.sessionStorage.setItem('cc-switch-webui-token', token);
  if (params.has('token')) {
    params.delete('token');
    const search = params.toString();
    const nextUrl = window.location.pathname + (search ? `?${search}` : '') + window.location.hash;
    window.history.replaceState(window.history.state, '', nextUrl);
  }
  let nextCallbackId = 1;
  window.__TAURI_INTERNALS__ = {
    transformCallback(callback, once) {
      const id = nextCallbackId++;
      const name = `_${id}`;
      window[name] = (...args) => {
        try {
          return callback(...args);
        } finally {
          if (once) delete window[name];
        }
      };
      return id;
    },
    async invoke(cmd, args = {}, _options) {
      const headers = { 'content-type': 'application/json' };
      if (token) headers['x-cc-switch-webui-token'] = token;
      const response = await fetch('/__cc_switch_webui__/invoke', {
        method: 'POST',
        headers,
        body: JSON.stringify({ cmd, args })
      });
      const text = await response.text();
      const payload = text ? JSON.parse(text) : null;
      if (!response.ok) {
        throw payload && payload.error ? payload.error : payload;
      }
      return payload;
    }
  };
})();
"#
}

fn generate_webui_auth_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn is_authorized_webui_request(headers: &HeaderMap, auth: &WebUiAuth) -> bool {
    match auth.mode {
        WebUiAuthMode::None => is_same_origin_or_non_browser_request(headers),
        WebUiAuthMode::Token => {
            let Some(expected_token) = auth.token_value() else {
                return false;
            };
            headers
                .get("x-cc-switch-webui-token")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|token| token == expected_token)
        }
    }
}

fn is_same_origin_or_non_browser_request(headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };

    origin == format!("http://{host}") || origin == format!("https://{host}")
}

async fn invoke_web_command(
    state: &AppState,
    copilot_state: &CopilotAuthState,
    events: &WebEventBus,
    cmd: &str,
    args: Value,
) -> Result<Value, String> {
    if let Some(value) = invoke_web_command_without_state(cmd, args.clone())? {
        return Ok(value);
    }

    match cmd {
        "get_settings" => json_value(cc_switch_lib::get_settings().await?),
        "save_settings" => {
            let settings: AppSettings = deserialize_arg(&args, "settings")?;
            json_value(cc_switch_lib::save_settings(settings).await?)
        }
        "restart_app" => json_value(false),
        "check_for_updates" => json_value(false),
        "is_portable_mode" => json_value(cc_switch_lib::is_portable_mode().await?),
        "get_config_status" => {
            json_value(cc_switch_lib::get_config_status(string_arg(&args, "app")?).await?)
        }
        "get_config_dir" => {
            json_value(cc_switch_lib::get_config_dir(string_arg(&args, "app")?).await?)
        }
        "list_server_directory" => json_value(
            cc_switch_lib::list_server_directory(optional_string_arg(&args, "path")).await?,
        ),
        "validate_server_directory" => {
            json_value(cc_switch_lib::validate_server_directory(string_arg(&args, "path")?).await?)
        }
        "get_claude_config_status" => json_value(cc_switch_lib::get_claude_config_status().await?),
        "get_claude_code_config_path" => {
            json_value(cc_switch_lib::get_claude_code_config_path().await?)
        }
        "get_app_config_path" => json_value(cc_switch_lib::get_app_config_path().await?),
        "get_app_config_dir" => json_value(cc_switch_lib::get_app_config_dir().await?),
        "get_claude_common_config_snippet" => json_value(
            state
                .db
                .get_config_snippet("claude")
                .map_err(|err| err.to_string())?,
        ),
        "set_claude_common_config_snippet" => {
            let snippet = string_arg(&args, "snippet")?;
            set_common_config_snippet_value(state, "claude", snippet)?;
            Ok(Value::Null)
        }
        "get_common_config_snippet" => {
            let app_type = common_config_key(&args)?;
            json_value(
                state
                    .db
                    .get_config_snippet(&app_type)
                    .map_err(|err| err.to_string())?,
            )
        }
        "set_common_config_snippet" => {
            let app_type = common_config_key(&args)?;
            let snippet = string_arg(&args, "snippet")?;
            set_common_config_snippet_value(state, &app_type, snippet)?;
            Ok(Value::Null)
        }
        "extract_common_config_snippet" => {
            let app_type = app_type_arg(&args, "appType")?;
            if let Some(settings_config) = optional_string_arg(&args, "settingsConfig")
                .filter(|value| !value.trim().is_empty())
            {
                let settings: Value = serde_json::from_str(&settings_config)
                    .map_err(|err| format!("invalid settings config JSON: {err}"))?;
                json_value(
                    ProviderService::extract_common_config_snippet_from_settings(
                        app_type, &settings,
                    )
                    .map_err(|err| err.to_string())?,
                )
            } else {
                json_value(
                    ProviderService::extract_common_config_snippet(state, app_type)
                        .map_err(|err| err.to_string())?,
                )
            }
        }
        "get_app_config_dir_override" => json_value(Option::<String>::None),
        "set_app_config_dir_override" => {
            Err("app config directory override is not available in CLI WebUI".to_string())
        }
        "export_config_as_content" => {
            let db = state.db.clone();
            tokio::task::spawn_blocking(move || {
                let content = db.export_sql_string().map_err(|err| err.to_string())?;
                Ok::<_, String>(json!({
                    "success": true,
                    "message": "SQL exported successfully",
                    "filePath": "cc-switch-export.sql",
                    "content": content
                }))
            })
            .await
            .map_err(|err| err.to_string())?
        }
        "import_config_from_content" => {
            let content = string_arg(&args, "content")?;
            let db = state.db.clone();
            let db_for_sync = db.clone();
            tokio::task::spawn_blocking(move || {
                let backup_id = db
                    .import_sql_string(&content)
                    .map_err(|err| err.to_string())?;
                let app_state = AppState::new(db_for_sync);
                let warning = ProviderService::sync_current_to_live(&app_state)
                    .and_then(|_| cc_switch_lib::reload_settings())
                    .err()
                    .map(|err| format!("Post-operation synchronization failed: {err}"));
                Ok::<_, String>(json!({
                    "success": true,
                    "message": "SQL imported successfully",
                    "backupId": backup_id,
                    "warning": warning
                }))
            })
            .await
            .map_err(|err| err.to_string())?
        }
        "get_providers" => {
            let app_type = app_arg(&args)?;
            json_value(ProviderService::list(state, app_type).map_err(|err| err.to_string())?)
        }
        "get_current_provider" => {
            let app_type = app_arg(&args)?;
            json_value(ProviderService::current(state, app_type).map_err(|err| err.to_string())?)
        }
        "add_provider" => {
            let app_type = app_arg(&args)?;
            let provider: Provider = deserialize_arg(&args, "provider")?;
            let add_to_live = optional_bool_arg(&args, "addToLive").unwrap_or(true);
            json_value(
                ProviderService::add(state, app_type, provider, add_to_live)
                    .map_err(|err| err.to_string())?,
            )
        }
        "update_provider" => {
            let app_type = app_arg(&args)?;
            let provider: Provider = deserialize_arg(&args, "provider")?;
            let original_id = optional_string_arg(&args, "originalId");
            json_value(
                ProviderService::update(state, app_type, original_id.as_deref(), provider)
                    .map_err(|err| err.to_string())?,
            )
        }
        "delete_provider" => {
            let app_type = app_arg(&args)?;
            let id = string_arg(&args, "id")?;
            ProviderService::delete(state, app_type, &id).map_err(|err| err.to_string())?;
            json_value(true)
        }
        "remove_provider_from_live_config" => {
            let app_type = app_arg(&args)?;
            let id = string_arg(&args, "id")?;
            ProviderService::remove_from_live_config(state, app_type, &id)
                .map_err(|err| err.to_string())?;
            json_value(true)
        }
        "switch_provider" => {
            let app_type = app_arg(&args)?;
            let id = string_arg(&args, "id")?;
            let result = ProviderService::switch(state, app_type.clone(), &id)
                .map_err(|err| err.to_string())?;
            events.publish(
                "provider-switched",
                json!({
                    "appType": app_type.as_str(),
                    "providerId": id,
                }),
            );
            json_value(result)
        }
        "import_default_config" => {
            let app_type = app_arg(&args)?;
            json_value(
                ProviderService::import_default_config(state, app_type)
                    .map_err(|err| err.to_string())?,
            )
        }
        "import_opencode_providers_from_live" => json_value(
            ProviderService::import_opencode_providers_from_live(state)
                .map_err(|err| err.to_string())?,
        ),
        "import_openclaw_providers_from_live" => json_value(
            ProviderService::import_openclaw_providers_from_live(state)
                .map_err(|err| err.to_string())?,
        ),
        "import_hermes_providers_from_live" => json_value(
            ProviderService::import_hermes_providers_from_live(state)
                .map_err(|err| err.to_string())?,
        ),
        "get_opencode_live_provider_ids" => json_value(
            cc_switch_lib::opencode_config::get_providers()
                .map(|providers| providers.keys().cloned().collect::<Vec<_>>())
                .map_err(|err| err.to_string())?,
        ),
        "get_openclaw_live_provider_ids" => json_value(
            cc_switch_lib::openclaw_config::get_providers()
                .map(|providers| providers.keys().cloned().collect::<Vec<_>>())
                .map_err(|err| err.to_string())?,
        ),
        "get_hermes_live_provider_ids" => json_value(
            cc_switch_lib::hermes_config::get_providers()
                .map(|providers| providers.keys().cloned().collect::<Vec<_>>())
                .map_err(|err| err.to_string())?,
        ),
        "read_live_provider_settings" => {
            let app_type = app_arg(&args)?;
            json_value(
                ProviderService::read_live_settings(app_type).map_err(|err| err.to_string())?,
            )
        }
        "get_custom_endpoints" => {
            let app_type = app_arg(&args)?;
            let provider_id = string_arg(&args, "providerId")?;
            json_value(
                ProviderService::get_custom_endpoints(state, app_type, &provider_id)
                    .map_err(|err| err.to_string())?,
            )
        }
        "add_custom_endpoint" => {
            let app_type = app_arg(&args)?;
            let provider_id = string_arg(&args, "providerId")?;
            let url = string_arg(&args, "url")?;
            ProviderService::add_custom_endpoint(state, app_type, &provider_id, url)
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "remove_custom_endpoint" => {
            let app_type = app_arg(&args)?;
            let provider_id = string_arg(&args, "providerId")?;
            let url = string_arg(&args, "url")?;
            ProviderService::remove_custom_endpoint(state, app_type, &provider_id, url)
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "update_endpoint_last_used" => {
            let app_type = app_arg(&args)?;
            let provider_id = string_arg(&args, "providerId")?;
            let url = string_arg(&args, "url")?;
            ProviderService::update_endpoint_last_used(state, app_type, &provider_id, url)
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "update_providers_sort_order" => {
            let app_type = app_arg(&args)?;
            let updates: Vec<ProviderSortUpdate> = deserialize_arg(&args, "updates")?;
            json_value(
                ProviderService::update_sort_order(state, app_type, updates)
                    .map_err(|err| err.to_string())?,
            )
        }
        "get_universal_providers" => {
            json_value(ProviderService::list_universal(state).map_err(|err| err.to_string())?)
        }
        "get_universal_provider" => {
            let id = string_arg(&args, "id")?;
            json_value(ProviderService::get_universal(state, &id).map_err(|err| err.to_string())?)
        }
        "upsert_universal_provider" => {
            let provider: UniversalProvider = deserialize_arg(&args, "provider")?;
            let id = provider.id.clone();
            let result = ProviderService::upsert_universal(state, provider)
                .map_err(|err| err.to_string())?;
            events.publish(
                "universal-provider-synced",
                json!({
                    "action": "upsert",
                    "id": id,
                }),
            );
            json_value(result)
        }
        "delete_universal_provider" => {
            let id = string_arg(&args, "id")?;
            let result = ProviderService::delete_universal(state, &id)
                .map_err(|err| err.to_string())?;
            events.publish(
                "universal-provider-synced",
                json!({
                    "action": "delete",
                    "id": id,
                }),
            );
            json_value(result)
        }
        "sync_universal_provider" => {
            let id = string_arg(&args, "id")?;
            let result = ProviderService::sync_universal_to_apps(state, &id)
                .map_err(|err| err.to_string())?;
            events.publish(
                "universal-provider-synced",
                json!({
                    "action": "sync",
                    "id": id,
                }),
            );
            json_value(result)
        }
        "get_proxy_status" => json_value(state.proxy_service.get_status().await?),
        "get_proxy_config" => json_value(state.proxy_service.get_config().await?),
        "update_proxy_config" => {
            let config = deserialize_arg(&args, "config")?;
            state.proxy_service.update_config(&config).await?;
            Ok(Value::Null)
        }
        "start_proxy_server" => json_value(state.proxy_service.start().await?),
        "stop_proxy_with_restore" => {
            state.proxy_service.stop_with_restore().await?;
            Ok(Value::Null)
        }
        "get_proxy_takeover_status" => json_value(state.proxy_service.get_takeover_status().await?),
        "set_proxy_takeover_for_app" => {
            let app_type = string_arg(&args, "appType")?;
            let enabled = bool_arg(&args, "enabled")?;
            state
                .proxy_service
                .set_takeover_for_app(&app_type, enabled)
                .await?;
            if enabled {
                emit_proxy_official_warning(events, state, &app_type)?;
            }
            Ok(Value::Null)
        }
        "is_proxy_running" => json_value(state.proxy_service.is_running().await),
        "is_live_takeover_active" => json_value(state.proxy_service.is_takeover_active().await?),
        "get_global_proxy_url" => json_value(
            state
                .db
                .get_global_proxy_url()
                .map_err(|err| err.to_string())?,
        ),
        "get_upstream_proxy_status" => json_value(cc_switch_lib::get_upstream_proxy_status()),
        "scan_local_proxies" => json_value(cc_switch_lib::scan_local_proxies().await),
        "get_global_proxy_config" => json_value(
            state
                .db
                .get_global_proxy_config()
                .await
                .map_err(|err| err.to_string())?,
        ),
        "update_global_proxy_config" => {
            let config = deserialize_arg(&args, "config")?;
            state
                .db
                .update_global_proxy_config(config)
                .await
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "get_proxy_config_for_app" => json_value(
            state
                .db
                .get_proxy_config_for_app(&string_arg(&args, "appType")?)
                .await
                .map_err(|err| err.to_string())?,
        ),
        "update_proxy_config_for_app" => {
            let config = deserialize_arg(&args, "config")?;
            state
                .db
                .update_proxy_config_for_app(config)
                .await
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "get_default_cost_multiplier" => json_value(
            state
                .db
                .get_default_cost_multiplier(&string_arg(&args, "appType")?)
                .await
                .map_err(|err| err.to_string())?,
        ),
        "set_default_cost_multiplier" => {
            state
                .db
                .set_default_cost_multiplier(
                    &string_arg(&args, "appType")?,
                    &string_arg(&args, "value")?,
                )
                .await
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "get_pricing_model_source" => json_value(
            state
                .db
                .get_pricing_model_source(&string_arg(&args, "appType")?)
                .await
                .map_err(|err| err.to_string())?,
        ),
        "set_pricing_model_source" => {
            state
                .db
                .set_pricing_model_source(
                    &string_arg(&args, "appType")?,
                    &string_arg(&args, "value")?,
                )
                .await
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "switch_proxy_provider" => {
            let app_type = string_arg(&args, "appType")?;
            let provider_id = string_arg(&args, "providerId")?;
            let provider = state
                .db
                .get_provider_by_id(&provider_id, &app_type)
                .map_err(|err| format!("读取供应商失败: {err}"))?
                .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
            if provider.category.as_deref() == Some("official") {
                return Err(
                    "代理接管模式下不能切换到官方供应商 (Cannot switch to official provider during proxy takeover)"
                        .to_string(),
                );
            }
            state
                .proxy_service
                .switch_proxy_target(&app_type, &provider_id)
                .await?;
            Ok(Value::Null)
        }
        "get_provider_health" => json_value(
            state
                .db
                .get_provider_health(&string_arg(&args, "providerId")?, &string_arg(&args, "appType")?)
                .await
                .map_err(|err| err.to_string())?,
        ),
        "reset_circuit_breaker" => {
            let provider_id = string_arg(&args, "providerId")?;
            let app_type = string_arg(&args, "appType")?;
            state
                .db
                .update_provider_health(&provider_id, &app_type, true, None)
                .await
                .map_err(|err| err.to_string())?;
            state
                .proxy_service
                .reset_provider_circuit_breaker(&provider_id, &app_type)
                .await?;
            Ok(Value::Null)
        }
        "get_circuit_breaker_config" => json_value(
            state
                .db
                .get_circuit_breaker_config()
                .await
                .map_err(|err| err.to_string())?,
        ),
        "update_circuit_breaker_config" => {
            let config = deserialize_arg(&args, "config")?;
            state
                .db
                .update_circuit_breaker_config(&config)
                .await
                .map_err(|err| err.to_string())?;
            state.proxy_service.update_circuit_breaker_configs(config).await?;
            Ok(Value::Null)
        }
        "get_circuit_breaker_stats" => json_value(Option::<Value>::None),
        "get_failover_queue" => json_value(
            state
                .db
                .get_failover_queue(&string_arg(&args, "appType")?)
                .map_err(|err| err.to_string())?,
        ),
        "get_available_providers_for_failover" => json_value(
            state
                .db
                .get_available_providers_for_failover(&string_arg(&args, "appType")?)
                .map_err(|err| err.to_string())?,
        ),
        "add_to_failover_queue" => {
            state
                .db
                .add_to_failover_queue(
                    &string_arg(&args, "appType")?,
                    &string_arg(&args, "providerId")?,
                )
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "remove_from_failover_queue" => {
            state
                .db
                .remove_from_failover_queue(
                    &string_arg(&args, "appType")?,
                    &string_arg(&args, "providerId")?,
                )
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "get_auto_failover_enabled" => json_value(
            state
                .db
                .get_proxy_config_for_app(&string_arg(&args, "appType")?)
                .await
                .map(|config| config.auto_failover_enabled)
                .map_err(|err| err.to_string())?,
        ),
        "set_auto_failover_enabled" => {
            let app_type = string_arg(&args, "appType")?;
            let mut config = state
                .db
                .get_proxy_config_for_app(&app_type)
                .await
                .map_err(|err| err.to_string())?;
            config.auto_failover_enabled = bool_arg(&args, "enabled")?;
            state
                .db
                .update_proxy_config_for_app(config)
                .await
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "get_usage_summary" => json_value(
            state
                .db
                .get_usage_summary(opt_i64_arg(&args, "startDate"), opt_i64_arg(&args, "endDate"), optional_string_arg(&args, "appType").as_deref())
                .map_err(|err| err.to_string())?,
        ),
        "get_usage_trends" => json_value(
            state
                .db
                .get_daily_trends(opt_i64_arg(&args, "startDate"), opt_i64_arg(&args, "endDate"), optional_string_arg(&args, "appType").as_deref())
                .map_err(|err| err.to_string())?,
        ),
        "get_provider_stats" => json_value(
            state
                .db
                .get_provider_stats(opt_i64_arg(&args, "startDate"), opt_i64_arg(&args, "endDate"), optional_string_arg(&args, "appType").as_deref())
                .map_err(|err| err.to_string())?,
        ),
        "get_model_stats" => json_value(
            state
                .db
                .get_model_stats(opt_i64_arg(&args, "startDate"), opt_i64_arg(&args, "endDate"), optional_string_arg(&args, "appType").as_deref())
                .map_err(|err| err.to_string())?,
        ),
        "queryProviderUsage" => {
            let provider_id = string_arg(&args, "providerId")?;
            let app = string_arg(&args, "app")?;
            let (result, payload) = cc_switch_lib::query_provider_usage_for_backend(
                state,
                copilot_state,
                provider_id,
                app,
            )
            .await?;
            events.publish("usage-cache-updated", payload);
            json_value(result?)
        }
        "testUsageScript" => {
            let app_type = app_arg(&args)?;
            let provider_id = string_arg(&args, "providerId")?;
            let script_code = string_arg(&args, "scriptCode")?;
            json_value(
                ProviderService::test_usage_script(
                    state,
                    app_type,
                    &provider_id,
                    &script_code,
                    args.get("timeout").and_then(Value::as_u64).unwrap_or(10),
                    optional_string_arg(&args, "apiKey").as_deref(),
                    optional_string_arg(&args, "baseUrl").as_deref(),
                    optional_string_arg(&args, "accessToken").as_deref(),
                    optional_string_arg(&args, "userId").as_deref(),
                    optional_string_arg(&args, "templateType").as_deref(),
                )
                .await
                .map_err(|err| err.to_string())?,
            )
        }
        "get_request_detail" => json_value(
            state
                .db
                .get_request_detail(&string_arg(&args, "requestId")?)
                .map_err(|err| err.to_string())?,
        ),
        "get_model_pricing" => json_value(Vec::<Value>::new()),
        "check_provider_limits" => json_value(
            state
                .db
                .check_provider_limits(&string_arg(&args, "providerId")?, &string_arg(&args, "appType")?)
                .map_err(|err| err.to_string())?,
        ),
        "get_usage_data_sources" => json_value(Vec::<Value>::new()),
        "list_sessions" => json_value(cc_switch_lib::list_sessions().await?),
        "get_session_messages" => json_value(
            cc_switch_lib::get_session_messages(
                string_arg(&args, "providerId")?,
                string_arg(&args, "sourcePath")?,
            )
            .await?,
        ),
        "delete_session" => json_value(
            cc_switch_lib::delete_session(
                string_arg(&args, "providerId")?,
                string_arg(&args, "sessionId")?,
                string_arg(&args, "sourcePath")?,
            )
            .await?,
        ),
        "delete_sessions" => {
            let items = args
                .get("items")
                .cloned()
                .ok_or_else(|| "missing argument 'items'".to_string())?;
            let items = serde_json::from_value(items).map_err(|err| err.to_string())?;
            json_value(cc_switch_lib::delete_sessions(items).await?)
        }
        "get_installed_skills" => json_value(
            SkillService::get_all_installed(&state.db).map_err(|err| err.to_string())?,
        ),
        "get_skill_backups" => {
            json_value(SkillService::list_backups().map_err(|err| err.to_string())?)
        }
        "delete_skill_backup" => json_value(
            SkillService::delete_backup(&string_arg(&args, "backupId")?)
                .map(|_| true)
                .map_err(|err| err.to_string())?,
        ),
        "uninstall_skill_unified" => json_value(
            SkillService::uninstall(&state.db, &string_arg(&args, "id")?)
                .map_err(|err| err.to_string())?,
        ),
        "restore_skill_backup" => {
            let app_type = app_type_arg(&args, "currentApp")?;
            json_value(
                SkillService::restore_from_backup(
                    &state.db,
                    &string_arg(&args, "backupId")?,
                    &app_type,
                )
                .map_err(|err| err.to_string())?,
            )
        }
        "toggle_skill_app" => {
            let app_type = app_type_arg(&args, "app")?;
            SkillService::toggle_app(
                &state.db,
                &string_arg(&args, "id")?,
                &app_type,
                bool_arg(&args, "enabled")?,
            )
            .map_err(|err| err.to_string())?;
            json_value(true)
        }
        "scan_unmanaged_skills" => json_value(
            SkillService::scan_unmanaged(&state.db).map_err(|err| err.to_string())?,
        ),
        "import_skills_from_apps" => {
            let imports: Vec<ImportSkillSelection> = deserialize_arg(&args, "imports")?;
            json_value(
                SkillService::import_from_apps(&state.db, imports).map_err(|err| err.to_string())?,
            )
        }
        "migrate_skill_storage" => {
            let target = deserialize_arg(&args, "target")?;
            json_value(
                SkillService::migrate_storage(&state.db, target).map_err(|err| err.to_string())?,
            )
        }
        "uninstall_skill" => {
            let directory = string_arg(&args, "directory")?;
            let skills = SkillService::get_all_installed(&state.db).map_err(|err| err.to_string())?;
            let skill = skills
                .into_iter()
                .find(|skill| skill.directory.eq_ignore_ascii_case(&directory))
                .ok_or_else(|| format!("未找到已安装的 Skill: {directory}"))?;
            json_value(SkillService::uninstall(&state.db, &skill.id).map_err(|err| err.to_string())?)
        }
        "uninstall_skill_for_app" => {
            let _ = app_type_arg(&args, "app")?;
            let directory = string_arg(&args, "directory")?;
            let skills = SkillService::get_all_installed(&state.db).map_err(|err| err.to_string())?;
            let skill = skills
                .into_iter()
                .find(|skill| skill.directory.eq_ignore_ascii_case(&directory))
                .ok_or_else(|| format!("未找到已安装的 Skill: {directory}"))?;
            json_value(SkillService::uninstall(&state.db, &skill.id).map_err(|err| err.to_string())?)
        }
        "get_skill_repos" => json_value(
            state
                .db
                .get_skill_repos()
                .map_err(|err| err.to_string())?,
        ),
        "add_skill_repo" => {
            let repo = deserialize_arg(&args, "repo")?;
            state.db.save_skill_repo(&repo).map_err(|err| err.to_string())?;
            json_value(true)
        }
        "remove_skill_repo" => {
            state
                .db
                .delete_skill_repo(&string_arg(&args, "owner")?, &string_arg(&args, "name")?)
                .map_err(|err| err.to_string())?;
            json_value(true)
        }
        "install_skills_from_zip" => {
            let app_type = app_type_arg(&args, "currentApp")?;
            json_value(
                SkillService::install_from_zip(
                    &state.db,
                    std::path::Path::new(&string_arg(&args, "filePath")?),
                    &app_type,
                )
                .map_err(|err| err.to_string())?,
            )
        }
        "get_mcp_config" => {
            let config_path = cc_switch_lib::get_app_config_path()
                .await?
                .to_string();
            let app_type = app_type_arg(&args, "app")?;
            let servers: IndexMap<String, Value> = McpService::get_all_servers(state)
                .map_err(|err| err.to_string())?
                .into_iter()
                .filter_map(|(id, server)| {
                    server
                        .apps
                        .is_enabled_for(&app_type)
                        .then_some((id, server.server))
                })
                .collect();
            json_value(json!({
                "config_path": config_path,
                "servers": servers,
            }))
        }
        "upsert_mcp_server_in_config" => {
            let app_type = app_type_arg(&args, "app")?;
            let id = string_arg(&args, "id")?;
            let spec = args.get("spec").cloned().ok_or_else(|| "missing argument 'spec'".to_string())?;
            let mut server = if let Some(mut existing) = state
                .db
                .get_all_mcp_servers()
                .map_err(|err| err.to_string())?
                .get(&id)
                .cloned()
            {
                existing.server = spec.clone();
                existing.apps.set_enabled_for(&app_type, true);
                existing
            } else {
                let mut apps = cc_switch_lib::McpApps::default();
                apps.set_enabled_for(&app_type, true);
                McpServer {
                    id: id.clone(),
                    name: spec
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&id)
                        .to_string(),
                    server: spec,
                    apps,
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                }
            };
            if optional_bool_arg(&args, "syncOtherSide").unwrap_or(false) {
                server.apps.claude = true;
                server.apps.codex = true;
                server.apps.gemini = true;
                server.apps.opencode = true;
            }
            McpService::upsert_server(state, server).map_err(|err| err.to_string())?;
            json_value(true)
        }
        "delete_mcp_server_in_config" => json_value(
            McpService::delete_server(state, &string_arg(&args, "id")?)
                .map_err(|err| err.to_string())?,
        ),
        "set_mcp_enabled" => {
            let app_type = app_type_arg(&args, "app")?;
            McpService::toggle_app(
                state,
                &string_arg(&args, "id")?,
                app_type,
                bool_arg(&args, "enabled")?,
            )
            .map_err(|err| err.to_string())?;
            json_value(true)
        }
        "get_mcp_servers" => {
            json_value(McpService::get_all_servers(state).map_err(|err| err.to_string())?)
        }
        "upsert_mcp_server" => {
            let server: McpServer = deserialize_arg(&args, "server")?;
            McpService::upsert_server(state, server).map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "delete_mcp_server" => json_value(
            McpService::delete_server(state, &string_arg(&args, "id")?)
                .map_err(|err| err.to_string())?,
        ),
        "toggle_mcp_app" => {
            let app_type = app_type_arg(&args, "app")?;
            McpService::toggle_app(
                state,
                &string_arg(&args, "serverId")?,
                app_type,
                bool_arg(&args, "enabled")?,
            )
            .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "import_mcp_from_apps" => {
            let total = McpService::import_from_claude(state).unwrap_or(0)
                + McpService::import_from_codex(state).unwrap_or(0)
                + McpService::import_from_gemini(state).unwrap_or(0)
                + McpService::import_from_opencode(state).unwrap_or(0)
                + McpService::import_from_hermes(state).unwrap_or(0);
            json_value(total)
        }
        "get_prompts" => {
            let app_type = app_type_arg(&args, "app")?;
            json_value(PromptService::get_prompts(state, app_type).map_err(|err| err.to_string())?)
        }
        "delete_prompt" => {
            let app_type = app_type_arg(&args, "app")?;
            PromptService::delete_prompt(state, app_type, &string_arg(&args, "id")?)
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "enable_prompt" => {
            let app_type = app_type_arg(&args, "app")?;
            PromptService::enable_prompt(state, app_type, &string_arg(&args, "id")?)
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "get_current_prompt_file_content" => {
            let app_type = app_type_arg(&args, "app")?;
            json_value(PromptService::get_current_file_content(app_type).map_err(|err| err.to_string())?)
        }
        "get_openclaw_live_provider" => json_value(
            cc_switch_lib::openclaw_config::get_provider(&string_arg(&args, "providerId")?)
                .map_err(|err| err.to_string())?,
        ),
        "scan_openclaw_config_health" => json_value(
            cc_switch_lib::openclaw_config::scan_openclaw_config_health()
                .map_err(|err| err.to_string())?,
        ),
        "get_openclaw_default_model" => json_value(
            cc_switch_lib::openclaw_config::get_default_model().map_err(|err| err.to_string())?,
        ),
        "set_openclaw_default_model" => json_value(
            cc_switch_lib::openclaw_config::set_default_model(&deserialize_arg(&args, "model")?)
                .map_err(|err| err.to_string())?,
        ),
        "get_openclaw_model_catalog" => json_value(
            cc_switch_lib::openclaw_config::get_model_catalog().map_err(|err| err.to_string())?,
        ),
        "set_openclaw_model_catalog" => json_value(
            cc_switch_lib::openclaw_config::set_model_catalog(&deserialize_arg(&args, "catalog")?)
                .map_err(|err| err.to_string())?,
        ),
        "get_openclaw_agents_defaults" => json_value(
            cc_switch_lib::openclaw_config::get_agents_defaults().map_err(|err| err.to_string())?,
        ),
        "set_openclaw_agents_defaults" => json_value(
            cc_switch_lib::openclaw_config::set_agents_defaults(&deserialize_arg(&args, "defaults")?)
                .map_err(|err| err.to_string())?,
        ),
        "get_openclaw_env" => json_value(
            cc_switch_lib::openclaw_config::get_env_config().map_err(|err| err.to_string())?,
        ),
        "set_openclaw_env" => json_value(
            cc_switch_lib::openclaw_config::set_env_config(&deserialize_arg(&args, "env")?)
                .map_err(|err| err.to_string())?,
        ),
        "get_openclaw_tools" => json_value(
            cc_switch_lib::openclaw_config::get_tools_config().map_err(|err| err.to_string())?,
        ),
        "set_openclaw_tools" => json_value(
            cc_switch_lib::openclaw_config::set_tools_config(&deserialize_arg(&args, "tools")?)
                .map_err(|err| err.to_string())?,
        ),
        "get_hermes_live_provider" => json_value(
            cc_switch_lib::hermes_config::get_provider(&string_arg(&args, "providerId")?)
                .map_err(|err| err.to_string())?,
        ),
        "get_hermes_model_config" => json_value(
            cc_switch_lib::hermes_config::get_model_config().map_err(|err| err.to_string())?,
        ),
        "get_hermes_memory" => json_value(
            cc_switch_lib::hermes_config::read_memory(deserialize_arg(&args, "kind")?)
                .map_err(|err| err.to_string())?,
        ),
        "set_hermes_memory" => {
            cc_switch_lib::hermes_config::write_memory(
                deserialize_arg(&args, "kind")?,
                &string_arg(&args, "content")?,
            )
            .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "get_hermes_memory_limits" => json_value(
            cc_switch_lib::hermes_config::read_memory_limits().map_err(|err| err.to_string())?,
        ),
        "set_hermes_memory_enabled" => json_value(
            cc_switch_lib::hermes_config::set_memory_enabled(
                deserialize_arg(&args, "kind")?,
                bool_arg(&args, "enabled")?,
            )
            .map_err(|err| err.to_string())?,
        ),
        "read_workspace_file" => json_value(
            cc_switch_lib::read_workspace_file(string_arg(&args, "filename")?).await?,
        ),
        "write_workspace_file" => {
            cc_switch_lib::write_workspace_file(
                string_arg(&args, "filename")?,
                string_arg(&args, "content")?,
            )
            .await?;
            Ok(Value::Null)
        }
        "list_daily_memory_files" => json_value(cc_switch_lib::list_daily_memory_files().await?),
        "read_daily_memory_file" => json_value(
            cc_switch_lib::read_daily_memory_file(string_arg(&args, "filename")?).await?,
        ),
        "write_daily_memory_file" => {
            cc_switch_lib::write_daily_memory_file(
                string_arg(&args, "filename")?,
                string_arg(&args, "content")?,
            )
            .await?;
            Ok(Value::Null)
        }
        "delete_daily_memory_file" => {
            cc_switch_lib::delete_daily_memory_file(string_arg(&args, "filename")?).await?;
            Ok(Value::Null)
        }
        "search_daily_memory_files" => json_value(
            cc_switch_lib::search_daily_memory_files(string_arg(&args, "query")?).await?,
        ),
        "fetch_models_for_config" => json_value(
            cc_switch_lib::fetch_models_for_config(
                string_arg(&args, "baseUrl")?,
                string_arg(&args, "apiKey")?,
                optional_bool_arg(&args, "isFullUrl"),
                optional_string_arg(&args, "modelsUrl"),
            )
            .await?,
        ),
        "get_balance" => json_value(
            cc_switch_lib::get_balance(string_arg(&args, "baseUrl")?, string_arg(&args, "apiKey")?)
                .await?,
        ),
        "get_coding_plan_quota" => json_value(
            cc_switch_lib::get_coding_plan_quota(
                string_arg(&args, "baseUrl")?,
                string_arg(&args, "apiKey")?,
            )
            .await?,
        ),
        "get_stream_check_config" => json_value(
            state
                .db
                .get_stream_check_config()
                .map_err(|err| err.to_string())?,
        ),
        "save_stream_check_config" => {
            let config = args
                .get("config")
                .cloned()
                .ok_or_else(|| "missing argument 'config'".to_string())?;
            let config = serde_json::from_value(config).map_err(|err| err.to_string())?;
            state
                .db
                .save_stream_check_config(&config)
                .map_err(|err| err.to_string())?;
            Ok(Value::Null)
        }
        "webdav_test_connection" => {
            let settings: cc_switch_lib::WebDavSyncSettings = deserialize_arg(&args, "settings")?;
            let resolved = resolve_webdav_password_for_request(
                settings,
                cc_switch_lib::get_webdav_sync_settings(),
                optional_bool_arg(&args, "preserveEmptyPassword").unwrap_or(true),
            );
            cc_switch_lib::webdav_check_connection(&resolved)
                .await
                .map_err(|err| err.to_string())?;
            json_value(json!({
                "success": true,
                "message": "WebDAV connection ok"
            }))
        }
        "webdav_sync_upload" => {
            let mut settings = require_enabled_webdav_settings()?;
            let db = state.db.clone();
            let result =
                cc_switch_lib::run_with_webdav_sync_lock(cc_switch_lib::webdav_upload(
                    &db,
                    &mut settings,
                ))
                .await;
            map_webdav_sync_result(events, result, &mut settings, "manual")
        }
        "webdav_sync_download" => {
            let mut settings = require_enabled_webdav_settings()?;
            let db = state.db.clone();
            let result =
                cc_switch_lib::run_with_webdav_sync_lock(cc_switch_lib::webdav_download(
                    &db,
                    &mut settings,
                ))
                .await;
            map_webdav_sync_result(events, result, &mut settings, "manual")
        }
        "webdav_sync_save_settings" => {
            let incoming: cc_switch_lib::WebDavSyncSettings = deserialize_arg(&args, "settings")?;
            let existing = cc_switch_lib::get_webdav_sync_settings();
            let mut sync_settings = resolve_webdav_password_for_request(
                incoming,
                existing.clone(),
                !optional_bool_arg(&args, "passwordTouched").unwrap_or(false),
            );
            if let Some(existing_settings) = existing {
                sync_settings.status = existing_settings.status;
            }
            sync_settings.normalize();
            sync_settings.validate().map_err(|err| err.to_string())?;
            cc_switch_lib::set_webdav_sync_settings(Some(sync_settings))
                .map_err(|err| err.to_string())?;
            json_value(json!({ "success": true }))
        }
        "webdav_sync_fetch_remote_info" => {
            let settings = require_enabled_webdav_settings()?;
            let info = cc_switch_lib::webdav_fetch_remote_info(&settings)
                .await
                .map_err(|err| err.to_string())?;
            Ok(info.unwrap_or(json!({ "empty": true })))
        }
        "get_subscription_quota" => {
            let tool = string_arg(&args, "tool")?;
            let (result, payload) =
                cc_switch_lib::get_subscription_quota_for_backend(state, tool).await;
            if let Some(payload) = payload {
                events.publish("usage-cache-updated", payload);
            }
            json_value(result?)
        }
        "auth_get_status" => {
            json_value(auth_status_unavailable(&string_arg(&args, "authProvider")?))
        }
        "auth_list_accounts" => json_value(Vec::<Value>::new()),
        "auth_start_login" | "auth_poll_for_account" | "auth_remove_account"
        | "auth_set_default_account" | "auth_logout"
        | "get_codex_oauth_quota" | "stream_check_provider" | "stream_check_all_providers"
        | "sync_session_usage" | "update_model_pricing" | "delete_model_pricing"
        | "upsert_prompt" | "import_prompt_from_file" | "install_skill_unified"
        | "discover_available_skills" | "check_skill_updates" | "update_skill"
        | "search_skills_sh" | "get_skills" | "get_skills_for_app" | "install_skill"
        | "install_skill_for_app" | "open_zip_file_dialog"
        | "open_hermes_web_ui" | "launch_hermes_dashboard" | "open_workspace_directory"
        | "launch_session_terminal" | "open_provider_terminal" | "open_config_folder"
        | "open_app_config_folder" | "pick_directory" | "save_file_dialog" | "open_file_dialog"
        | "export_config_to_file" | "import_config_from_file" => {
            Err(format!("command '{cmd}' requires desktop state or user interaction and is unavailable in CLI WebUI"))
        }
        "sync_current_providers_live" => {
            ProviderService::sync_current_to_live(state).map_err(|err| err.to_string())?;
            json_value(json!({
                "success": true,
                "message": "Live configuration synchronized"
            }))
        }
        other => Err(format!(
            "command '{other}' is not available in CLI WebUI yet"
        )),
    }
}

fn emit_proxy_official_warning(
    events: &WebEventBus,
    state: &AppState,
    app_type: &str,
) -> Result<(), String> {
    let app_type = AppType::from_str(app_type).map_err(|err| err.to_string())?;
    let current_id =
        ProviderService::current(state, app_type.clone()).map_err(|err| err.to_string())?;
    if let Some(provider) = state
        .db
        .get_provider_by_id(&current_id, app_type.as_str())
        .map_err(|err| err.to_string())?
        .filter(|provider| provider.category.as_deref() == Some("official"))
    {
        events.publish(
            "proxy-official-warning",
            json!({
                "appType": app_type.as_str(),
                "providerName": provider.name,
            }),
        );
    }
    Ok(())
}

#[cfg(test)]
fn publish_usage_cache_updated_script<T: Serialize>(
    events: &WebEventBus,
    app_type: &AppType,
    provider_id: &str,
    result: &T,
) {
    events.publish(
        "usage-cache-updated",
        json!({
            "kind": "script",
            "appType": app_type.as_str(),
            "providerId": provider_id,
            "data": result,
        }),
    );
}

fn webdav_not_configured_error() -> String {
    cc_switch_lib::AppError::localized(
        "webdav.sync.not_configured",
        "未配置 WebDAV 同步",
        "WebDAV sync is not configured.",
    )
    .to_string()
}

fn webdav_sync_disabled_error() -> String {
    cc_switch_lib::AppError::localized(
        "webdav.sync.disabled",
        "WebDAV 同步未启用",
        "WebDAV sync is disabled.",
    )
    .to_string()
}

fn require_enabled_webdav_settings() -> Result<cc_switch_lib::WebDavSyncSettings, String> {
    let settings =
        cc_switch_lib::get_webdav_sync_settings().ok_or_else(webdav_not_configured_error)?;
    if !settings.enabled {
        return Err(webdav_sync_disabled_error());
    }
    Ok(settings)
}

fn resolve_webdav_password_for_request(
    mut incoming: cc_switch_lib::WebDavSyncSettings,
    existing: Option<cc_switch_lib::WebDavSyncSettings>,
    preserve_empty_password: bool,
) -> cc_switch_lib::WebDavSyncSettings {
    if let Some(existing_settings) = existing {
        if preserve_empty_password && incoming.password.is_empty() {
            incoming.password = existing_settings.password;
        }
    }
    incoming
}

fn map_webdav_sync_result(
    events: &WebEventBus,
    result: Result<Value, cc_switch_lib::AppError>,
    settings: &mut cc_switch_lib::WebDavSyncSettings,
    source: &str,
) -> Result<Value, String> {
    match result {
        Ok(value) => {
            publish_webdav_sync_status_updated(events, source, "success", None);
            Ok(value)
        }
        Err(err) => {
            settings.status.last_error = Some(err.to_string());
            settings.status.last_error_source = Some(source.to_string());
            let _ = cc_switch_lib::update_webdav_sync_status(settings.status.clone());
            publish_webdav_sync_status_updated(events, source, "error", Some(&err.to_string()));
            Err(err.to_string())
        }
    }
}

fn publish_webdav_sync_status_updated(
    events: &WebEventBus,
    source: &str,
    status: &str,
    error: Option<&str>,
) {
    events.publish(
        "webdav-sync-status-updated",
        json!({
            "source": source,
            "status": status,
            "error": error,
        }),
    );
}

type WebdavStatusEmitter = Arc<dyn Fn(&str, Option<&str>) + Send + Sync + 'static>;

fn webdav_status_emitter(events: Arc<WebEventBus>) -> WebdavStatusEmitter {
    Arc::new(move |status, error| {
        publish_webdav_sync_status_updated(&events, "auto", status, error);
    })
}

fn invoke_web_command_without_state(cmd: &str, args: Value) -> Result<Option<Value>, String> {
    let value = match cmd {
        "plugin:event|listen" => json_value(0_u64)?,
        "plugin:event|unlisten" => Value::Null,
        "plugin:app|version" | "plugin:app|get_version" => json_value(env!("CARGO_PKG_VERSION"))?,
        "plugin:path|resolve_directory" => resolve_directory_arg(&args)?,
        "plugin:path|join" => join_path_arg(&args)?,
        "plugin:dialog|message" | "plugin:dialog|ask" | "plugin:dialog|confirm" => Value::Null,
        "plugin:process|exit" | "plugin:process|relaunch" => Value::Null,
        "get_runtime_info" => json_value(cc_switch_lib::backend_runtime_info(
            cc_switch_lib::BackendMode::WebUi,
        ))?,
        "plugin:window|is_maximized"
        | "plugin:window|is_minimized"
        | "plugin:window|is_fullscreen"
        | "plugin:window|is_focused"
        | "plugin:window|is_decorated"
        | "plugin:window|is_resizable"
        | "plugin:window|is_visible" => Value::Bool(false),
        "plugin:window|set_decorations"
        | "plugin:window|minimize"
        | "plugin:window|toggle_maximize"
        | "plugin:window|maximize"
        | "plugin:window|unmaximize"
        | "plugin:window|close"
        | "plugin:window|start_dragging" => Value::Null,
        "set_window_theme" => Value::Null,
        "update_tray_menu" => Value::Bool(false),
        "check_env_conflicts" => json_value(Vec::<Value>::new())?,
        "get_init_error" => json_value(Option::<Value>::None)?,
        "get_migration_result" => Value::Bool(false),
        "get_skills_migration_result" => json_value(Option::<Value>::None)?,
        _ => return Ok(None),
    };

    Ok(Some(value))
}

fn json_value<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|err| err.to_string())
}

fn resolve_directory_arg(args: &Value) -> Result<Value, String> {
    let directory = args
        .get("directory")
        .ok_or_else(|| "missing directory".to_string())?;
    let path = match directory {
        Value::String(directory) if matches!(directory.as_str(), "Home" | "home") => {
            dirs::home_dir().ok_or_else(|| "failed to resolve home dir".to_string())?
        }
        Value::String(directory) if matches!(directory.as_str(), "AppConfig" | "appConfig") => {
            dirs::home_dir()
                .ok_or_else(|| "failed to resolve home dir".to_string())?
                .join(".cc-switch")
        }
        Value::Number(directory) if directory.as_u64() == Some(21) => {
            dirs::home_dir().ok_or_else(|| "failed to resolve home dir".to_string())?
        }
        Value::Number(directory) if directory.as_u64() == Some(13) => dirs::home_dir()
            .ok_or_else(|| "failed to resolve home dir".to_string())?
            .join(".cc-switch"),
        other => return Err(format!("unsupported path directory '{other}'")),
    };
    json_value(path.to_string_lossy().to_string())
}

fn join_path_arg(args: &Value) -> Result<Value, String> {
    let paths = args
        .get("paths")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing paths".to_string())?;
    let mut iter = paths.iter();
    let Some(first) = iter.next() else {
        return json_value(String::new());
    };
    let mut joined = std::path::PathBuf::from(
        first
            .as_str()
            .ok_or_else(|| "paths must contain strings".to_string())?,
    );
    for part in iter {
        joined.push(
            part.as_str()
                .ok_or_else(|| "paths must contain strings".to_string())?,
        );
    }
    json_value(joined.to_string_lossy().to_string())
}

fn app_arg(args: &Value) -> Result<AppType, String> {
    app_type_arg(args, "app")
}

fn app_type_arg(args: &Value, name: &str) -> Result<AppType, String> {
    AppType::from_str(&string_arg(args, name)?).map_err(|err| err.to_string())
}

fn common_config_key(args: &Value) -> Result<String, String> {
    let key = string_arg(args, "appType")?;
    if key == "omo_slim" {
        Ok("omo-slim".to_string())
    } else {
        Ok(key)
    }
}

fn string_arg(args: &Value, name: &str) -> Result<String, String> {
    args.get(name)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| format!("missing string argument '{name}'"))
}

fn bool_arg(args: &Value, name: &str) -> Result<bool, String> {
    args.get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing boolean argument '{name}'"))
}

fn optional_bool_arg(args: &Value, name: &str) -> Option<bool> {
    args.get(name).and_then(Value::as_bool)
}

fn optional_string_arg(args: &Value, name: &str) -> Option<String> {
    args.get(name)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn opt_i64_arg(args: &Value, name: &str) -> Option<i64> {
    args.get(name).and_then(Value::as_i64)
}

fn deserialize_arg<T: for<'de> Deserialize<'de>>(args: &Value, name: &str) -> Result<T, String> {
    let value = args
        .get(name)
        .ok_or_else(|| format!("missing argument '{name}'"))?;
    serde_json::from_value(value.clone()).map_err(|err| err.to_string())
}

fn auth_status_unavailable(auth_provider: &str) -> Value {
    json!({
        "provider": auth_provider,
        "authenticated": false,
        "default_account_id": Value::Null,
        "migration_error": "Auth device managers are unavailable in CLI WebUI",
        "accounts": [],
    })
}

fn set_common_config_snippet_value(
    state: &AppState,
    app_type: &str,
    snippet: String,
) -> Result<(), String> {
    let is_cleared = snippet.trim().is_empty();
    let old_snippet = state
        .db
        .get_config_snippet(app_type)
        .map_err(|err| err.to_string())?;

    validate_common_config_snippet(app_type, &snippet)?;

    if matches!(app_type, "claude" | "codex" | "gemini") {
        if let Some(legacy_snippet) = old_snippet
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let app = AppType::from_str(app_type).map_err(|err| err.to_string())?;
            ProviderService::migrate_legacy_common_config_usage(state, app, legacy_snippet)
                .map_err(|err| err.to_string())?;
        }
    }

    let value = if is_cleared { None } else { Some(snippet) };
    state
        .db
        .set_config_snippet(app_type, value)
        .map_err(|err| err.to_string())?;
    state
        .db
        .set_config_snippet_cleared(app_type, is_cleared)
        .map_err(|err| err.to_string())?;

    if matches!(app_type, "claude" | "codex" | "gemini") {
        let app = AppType::from_str(app_type).map_err(|err| err.to_string())?;
        ProviderService::sync_current_provider_for_app(state, app)
            .map_err(|err| err.to_string())?;
    }

    Ok(())
}

fn validate_common_config_snippet(app_type: &str, snippet: &str) -> Result<(), String> {
    if snippet.trim().is_empty() {
        return Ok(());
    }

    match app_type {
        "claude" | "gemini" | "omo" | "omo-slim" => {
            serde_json::from_str::<Value>(snippet)
                .map_err(|err| format!("invalid JSON format: {err}"))?;
        }
        "codex" => {
            snippet
                .parse::<toml_edit::DocumentMut>()
                .map_err(|err| format!("invalid TOML format: {err}"))?;
        }
        _ => {}
    }

    Ok(())
}

fn format_provider_list(
    app_type: &AppType,
    providers: &IndexMap<String, Provider>,
    current: &str,
) -> String {
    if providers.is_empty() {
        return format!("No providers found for {}.\n", app_type.as_str());
    }

    let mut output = String::from("CURRENT\tID\tNAME\tCATEGORY\n");
    for provider in providers.values() {
        let marker = if !current.is_empty() && provider.id == current {
            "*"
        } else {
            ""
        };
        let category = provider.category.as_deref().unwrap_or("");
        output.push_str(marker);
        output.push('\t');
        output.push_str(&clean_cell(&provider.id));
        output.push('\t');
        output.push_str(&clean_cell(&provider.name));
        output.push('\t');
        output.push_str(&clean_cell(category));
        output.push('\n');
    }

    output
}

fn clean_cell(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if matches!(ch, '\t' | '\n' | '\r') {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

fn top_level_help() -> String {
    [
        "CC Switch",
        "",
        "Usage:",
        "  cc-switch-cli <command> [options]",
        "  cc-switch-cli --help",
        "",
        "WebUI:",
        "  cc-switch-cli webui        Start embedded WebUI server on http://127.0.0.1:9990",
        "  cc-switch-cli webui --auth token --token <token>",
        "  cc-switch-cli webui --auth none",
        "",
        "Commands:",
        "  webui                         Start embedded WebUI server",
        "  providers                     Manage providers",
        "",
        "Examples:",
        "  cc-switch-cli webui",
        "  cc-switch-cli webui --port 9990",
        "  cc-switch-cli webui --host 0.0.0.0 --auth token --token secret",
        "  cc-switch-cli providers list --app claude",
        "  cc-switch-cli providers current --app codex",
        "  cc-switch-cli providers switch --app claude --id my-provider",
        "",
    ]
    .join("\n")
}

fn provider_help() -> String {
    [
        "CC Switch provider commands",
        "",
        "Usage:",
        "  cc-switch-cli providers list --app <app>",
        "  cc-switch-cli providers current --app <app>",
        "  cc-switch-cli providers switch --app <app> --id <provider-id>",
        "",
        "Apps:",
        "  claude, codex, gemini, opencode, openclaw, hermes",
        "",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use tempfile::TempDir;

    struct ScopedTestHome {
        previous_test_home: Option<std::ffi::OsString>,
        _temp: TempDir,
    }

    impl ScopedTestHome {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("tempdir");
            let previous_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
            Self {
                previous_test_home,
                _temp: temp,
            }
        }
    }

    impl Drop for ScopedTestHome {
        fn drop(&mut self) {
            match &self.previous_test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn test_web_state(auth: WebUiAuth, events: Arc<WebEventBus>) -> WebServerState {
        WebServerState {
            app_state: Arc::new(AppState::new(Arc::new(
                Database::memory().expect("in-memory database"),
            ))),
            copilot_state: Arc::new(create_copilot_auth_state(
                cc_switch_lib::get_app_config_dir_path(),
            )),
            auth: Arc::new(auth),
            events,
        }
    }

    fn provider_with_category(id: &str, name: &str, category: Option<&str>) -> Provider {
        let mut provider = Provider::with_id(id.to_string(), name.to_string(), json!({}), None);
        provider.category = category.map(ToString::to_string);
        provider
    }

    #[test]
    fn parses_provider_list_command() {
        assert_eq!(
            parse_cli_command(&strings(&["providers", "list", "--app=claude"])).unwrap(),
            CliCommand::Providers(ProviderCommand::List {
                app_type: AppType::Claude
            })
        );
    }

    #[test]
    fn parses_provider_switch_command() {
        assert_eq!(
            parse_cli_command(&strings(&[
                "providers",
                "switch",
                "-a",
                "codex",
                "-i",
                "my-provider"
            ]))
            .unwrap(),
            CliCommand::Providers(ProviderCommand::Switch {
                app_type: AppType::Codex,
                id: "my-provider".to_string()
            })
        );
    }

    #[test]
    fn parses_webui_command_with_default_port() {
        assert_eq!(
            parse_cli_command(&strings(&["webui"])).unwrap(),
            CliCommand::WebUi(WebUiOptions {
                host: DEFAULT_WEBUI_HOST.to_string(),
                port: DEFAULT_WEBUI_PORT,
                auth_mode: None,
                token: None
            })
        );
    }

    #[test]
    fn parses_webui_command_with_custom_port() {
        assert_eq!(
            parse_cli_command(&strings(&["webui", "--host", "0.0.0.0", "--port", "9991"])).unwrap(),
            CliCommand::WebUi(WebUiOptions {
                host: "0.0.0.0".to_string(),
                port: 9991,
                auth_mode: None,
                token: None
            })
        );
    }

    #[test]
    fn parses_webui_command_with_auth_none() {
        assert_eq!(
            parse_cli_command(&strings(&["webui", "--auth", "none"])).unwrap(),
            CliCommand::WebUi(WebUiOptions {
                host: DEFAULT_WEBUI_HOST.to_string(),
                port: DEFAULT_WEBUI_PORT,
                auth_mode: Some(WebUiAuthMode::None),
                token: None
            })
        );
    }

    #[test]
    fn parses_webui_command_with_token_auth_and_token() {
        assert_eq!(
            parse_cli_command(&strings(&[
                "webui",
                "--auth=token",
                "--token",
                "secret-token"
            ]))
            .unwrap(),
            CliCommand::WebUi(WebUiOptions {
                host: DEFAULT_WEBUI_HOST.to_string(),
                port: DEFAULT_WEBUI_PORT,
                auth_mode: Some(WebUiAuthMode::Token),
                token: Some("secret-token".to_string())
            })
        );
    }

    #[test]
    fn rejects_webui_token_when_auth_none() {
        let err =
            parse_cli_command(&strings(&["webui", "--auth", "none", "--token", "x"])).unwrap_err();
        assert_eq!(err.message, "--token requires --auth token");
    }

    #[test]
    fn rejects_invalid_webui_port() {
        let err = parse_cli_command(&strings(&["webui", "--port", "0"])).unwrap_err();
        assert_eq!(err.message, "port must be greater than 0");
    }

    #[test]
    fn missing_asset_with_extension_does_not_fallback_to_index() {
        assert!(!should_fallback_to_index("assets/missing.js"));
        assert!(!should_fallback_to_index("style.css"));
    }

    #[test]
    fn route_path_without_extension_falls_back_to_index() {
        assert!(should_fallback_to_index("settings"));
        assert!(should_fallback_to_index("providers/claude"));
    }

    #[test]
    fn webui_window_command_stubs_return_safe_values() {
        let result =
            invoke_web_command_without_state("plugin:window|is_maximized", Value::Null).unwrap();
        assert_eq!(result, Some(Value::Bool(false)));

        let result =
            invoke_web_command_without_state("plugin:window|set_decorations", Value::Null).unwrap();
        assert_eq!(result, Some(Value::Null));
    }

    #[test]
    fn webui_auth_requires_matching_token_header() {
        let mut headers = HeaderMap::new();
        assert!(is_authorized_webui_request(&headers, &WebUiAuth::none()));
        assert!(!is_authorized_webui_request(
            &headers,
            &WebUiAuth::token("secret".to_string())
        ));

        headers.insert("x-cc-switch-webui-token", HeaderValue::from_static("wrong"));
        assert!(!is_authorized_webui_request(
            &headers,
            &WebUiAuth::token("secret".to_string())
        ));

        headers.insert(
            "x-cc-switch-webui-token",
            HeaderValue::from_static("secret"),
        );
        assert!(is_authorized_webui_request(
            &headers,
            &WebUiAuth::token("secret".to_string())
        ));
    }

    #[test]
    fn webui_auth_none_rejects_cross_origin_browser_requests() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:9990"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://malicious.example"),
        );
        assert!(!is_authorized_webui_request(&headers, &WebUiAuth::none()));

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:9990"),
        );
        assert!(is_authorized_webui_request(&headers, &WebUiAuth::none()));
    }

    #[tokio::test]
    async fn web_event_bus_publishes_named_events_to_subscribers() {
        let bus = WebEventBus::new();
        let mut events = bus.subscribe();

        bus.publish(
            "provider-switched",
            json!({
                "appType": "claude",
                "providerId": "p1"
            }),
        );

        let event = events.recv().await.unwrap();
        assert_eq!(event.event, "provider-switched");
        assert_eq!(
            event.payload,
            json!({
                "appType": "claude",
                "providerId": "p1"
            })
        );
    }

    #[tokio::test]
    async fn webui_backend_supports_representative_app_commands() {
        let state = AppState::new(Arc::new(Database::memory().expect("in-memory database")));
        let events = WebEventBus::new();

        for (cmd, args) in [
            ("get_installed_skills", Value::Null),
            ("get_mcp_servers", Value::Null),
            ("get_usage_summary", json!({})),
            ("get_subscription_quota", json!({ "tool": "unknown" })),
            ("webdav_sync_upload", Value::Null),
        ] {
            let copilot_state = create_copilot_auth_state(cc_switch_lib::get_app_config_dir_path());
            let result = invoke_web_command(&state, &copilot_state, &events, cmd, args).await;
            assert!(
                !matches!(&result, Err(message) if message.contains("not available in CLI WebUI yet") || message.contains("requires desktop state")),
                "{cmd} should be implemented for CLI WebUI, got {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn web_events_rejects_missing_token_auth() {
        let state = test_web_state(
            WebUiAuth::token("secret".to_string()),
            Arc::new(WebEventBus::new()),
        );

        let response = web_events(AxumState(state), HeaderMap::new()).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn web_events_serializes_published_events_as_sse_data_frames() {
        let events = Arc::new(WebEventBus::new());
        let state = test_web_state(WebUiAuth::none(), events.clone());
        let mut response = web_events(AxumState(state), HeaderMap::new())
            .await
            .into_body();

        events.publish(
            "provider-switched",
            json!({
                "appType": "claude",
                "providerId": "p1"
            }),
        );

        let frame = tokio::time::timeout(Duration::from_secs(1), response.frame())
            .await
            .expect("SSE response should yield a frame")
            .expect("SSE body should not end")
            .expect("SSE frame should be successful");
        let data = frame.into_data().expect("SSE frame should contain data");
        let text = std::str::from_utf8(&data).expect("SSE frame should be UTF-8");

        assert!(text.starts_with("data: "));
        assert!(text.ends_with("\n\n"));
        let envelope: WebEvent = serde_json::from_str(
            text.trim()
                .strip_prefix("data: ")
                .expect("SSE data frame should contain a data prefix"),
        )
        .expect("SSE data should be a serialized WebEvent");
        assert_eq!(
            envelope,
            WebEvent {
                event: "provider-switched".to_string(),
                payload: json!({
                    "appType": "claude",
                    "providerId": "p1"
                }),
            }
        );
    }

    #[tokio::test]
    async fn emit_proxy_official_warning_publishes_cli_webui_event_for_current_official_provider() {
        let _home = ScopedTestHome::new();

        let db = Arc::new(Database::memory().expect("in-memory database"));
        let state = AppState::new(db.clone());
        let official = provider_with_category("official", "Official Provider", Some("official"));
        db.save_provider(AppType::Claude.as_str(), &official)
            .expect("save provider");
        db.set_current_provider(AppType::Claude.as_str(), &official.id)
            .expect("set current provider");

        let events = WebEventBus::new();
        let mut receiver = events.subscribe();

        emit_proxy_official_warning(&events, &state, AppType::Claude.as_str())
            .expect("emit warning");

        let event = receiver.recv().await.expect("warning event");
        assert_eq!(
            event,
            WebEvent {
                event: "proxy-official-warning".to_string(),
                payload: json!({
                    "appType": "claude",
                    "providerName": "Official Provider",
                }),
            }
        );
    }

    #[tokio::test]
    async fn publish_usage_cache_updated_sends_script_payload_to_webui_subscribers() {
        let events = WebEventBus::new();
        let mut receiver = events.subscribe();
        let result = json!({
            "success": true,
            "data": Value::Null,
            "error": Value::Null,
        });

        publish_usage_cache_updated_script(&events, &AppType::Claude, "provider-1", &result);

        let event = receiver.recv().await.expect("usage event");
        assert_eq!(
            event,
            WebEvent {
                event: "usage-cache-updated".to_string(),
                payload: json!({
                    "kind": "script",
                    "appType": "claude",
                    "providerId": "provider-1",
                    "data": result,
                }),
            }
        );
    }

    #[tokio::test]
    async fn publish_webdav_sync_status_updated_sends_status_to_webui_subscribers() {
        let events = WebEventBus::new();
        let mut receiver = events.subscribe();

        publish_webdav_sync_status_updated(&events, "manual", "error", Some("boom"));

        let event = receiver.recv().await.expect("webdav event");
        assert_eq!(
            event,
            WebEvent {
                event: "webdav-sync-status-updated".to_string(),
                payload: json!({
                    "source": "manual",
                    "status": "error",
                    "error": "boom",
                }),
            }
        );
    }

    #[test]
    fn generated_webui_auth_token_is_strong_random_token() {
        let first = generate_webui_auth_token();
        let second = generate_webui_auth_token();

        assert_ne!(first, second);
        assert!(
            first.len() >= 64,
            "token should contain at least 256 bits encoded as hex"
        );
        assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn rejects_unknown_options() {
        let err = parse_cli_command(&strings(&["providers", "list", "--app", "claude", "--bad"]))
            .unwrap_err();
        assert_eq!(err.message, "unknown option '--bad'");
    }

    #[test]
    fn rejects_duplicate_options() {
        let err = parse_cli_command(&strings(&[
            "providers",
            "current",
            "--app",
            "claude",
            "--app",
            "codex",
        ]))
        .unwrap_err();
        assert_eq!(err.message, "duplicate option --app");
    }

    #[test]
    fn formats_provider_rows_without_secret_fields() {
        let mut providers = IndexMap::new();
        let mut provider = Provider::with_id(
            "first".to_string(),
            "First Provider".to_string(),
            serde_json::json!({"apiKey": "secret"}),
            None,
        );
        provider.category = Some("custom".to_string());
        providers.insert(provider.id.clone(), provider);

        assert_eq!(
            format_provider_list(&AppType::Claude, &providers, "first"),
            "CURRENT\tID\tNAME\tCATEGORY\n*\tfirst\tFirst Provider\tcustom\n"
        );
    }

    #[test]
    fn startup_provider_import_plan_covers_additive_live_imports() {
        let plan = startup_provider_import_plan();

        assert!(plan.contains(&StartupProviderImport::Default(AppType::Claude)));
        assert!(plan.contains(&StartupProviderImport::SeedOfficial));
        assert!(plan.contains(&StartupProviderImport::OpenCodeLive));
        assert!(plan.contains(&StartupProviderImport::OpenClawLive));
        assert!(plan.contains(&StartupProviderImport::HermesLive));
    }
}
