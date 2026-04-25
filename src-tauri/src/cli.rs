use axum::{
    body::Body,
    extract::State as AxumState,
    http::{header, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use cc_switch_lib::{
    AppSettings, AppState, AppType, Database, Provider, ProviderService, ProviderSortUpdate,
    UniversalProvider,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

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

    Ok(CliCommand::WebUi(WebUiOptions { host, port }))
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

#[derive(Clone)]
struct WebServerState {
    app_state: Arc<AppState>,
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
    let web_state = WebServerState { app_state };

    let app = Router::new()
        .route("/__cc_switch_webui__/invoke", post(web_invoke))
        .fallback(serve_webui_asset)
        .with_state(web_state);

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|err| CliError::new(format!("failed to bind {address}: {err}")))?;
    println!("CC Switch WebUI listening on http://{address}");
    println!("Press Ctrl+C to stop.");

    axum::serve(listener, app)
        .await
        .map_err(|err| CliError::new(format!("webui server failed: {err}")))
}

fn init_webui_state() -> Result<AppState, CliError> {
    let state = build_state()?;

    let _ = state.db.init_default_skill_repos();

    for app_type in AppType::all().filter(|app| !app.is_additive_mode()) {
        let _ = ProviderService::import_default_config(&state, app_type);
    }

    let _ = state.db.init_default_official_providers();

    Ok(state)
}

async fn web_invoke(
    AxumState(state): AxumState<WebServerState>,
    Json(request): Json<InvokeRequest>,
) -> Response {
    match invoke_web_command(&state.app_state, &request.cmd, request.args).await {
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

async fn serve_webui_asset(uri: Uri) -> Response {
    let path = normalize_request_path(uri.path());
    let asset = webui_assets::get(&path)
        .or_else(|| {
            if path.contains('.') {
                None
            } else {
                webui_assets::get("index.html")
            }
        })
        .or_else(|| webui_assets::get("index.html"));

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
      const response = await fetch('/__cc_switch_webui__/invoke', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
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

async fn invoke_web_command(state: &AppState, cmd: &str, args: Value) -> Result<Value, String> {
    match cmd {
        "plugin:event|listen" | "plugin:event|unlisten" => Ok(Value::Null),
        "plugin:app|version" | "plugin:app|get_version" => json_value(env!("CARGO_PKG_VERSION")),
        "plugin:path|resolve_directory" => resolve_directory_arg(&args),
        "plugin:path|join" => join_path_arg(&args),
        "plugin:dialog|message" | "plugin:dialog|ask" | "plugin:dialog|confirm" => Ok(Value::Null),
        "plugin:process|exit" | "plugin:process|relaunch" => Ok(Value::Null),
        "set_window_theme" => Ok(Value::Null),
        "update_tray_menu" => json_value(false),
        "get_init_error" => json_value(Option::<Value>::None),
        "get_migration_result" => json_value(false),
        "get_skills_migration_result" => json_value(Option::<Value>::None),
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
        "get_claude_config_status" => json_value(cc_switch_lib::get_claude_config_status().await?),
        "get_claude_code_config_path" => {
            json_value(cc_switch_lib::get_claude_code_config_path().await?)
        }
        "get_app_config_path" => json_value(cc_switch_lib::get_app_config_path().await?),
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
            json_value(
                ProviderService::switch(state, app_type, &id).map_err(|err| err.to_string())?,
            )
        }
        "import_default_config" => {
            let app_type = app_arg(&args)?;
            json_value(
                ProviderService::import_default_config(state, app_type)
                    .map_err(|err| err.to_string())?,
            )
        }
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
            json_value(
                ProviderService::upsert_universal(state, provider)
                    .map_err(|err| err.to_string())?,
            )
        }
        "delete_universal_provider" => {
            let id = string_arg(&args, "id")?;
            json_value(
                ProviderService::delete_universal(state, &id).map_err(|err| err.to_string())?,
            )
        }
        "sync_universal_provider" => {
            let id = string_arg(&args, "id")?;
            json_value(
                ProviderService::sync_universal_to_apps(state, &id)
                    .map_err(|err| err.to_string())?,
            )
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
        other => Err(format!(
            "command '{other}' is not available in CLI WebUI yet"
        )),
    }
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

fn deserialize_arg<T: for<'de> Deserialize<'de>>(args: &Value, name: &str) -> Result<T, String> {
    let value = args
        .get(name)
        .ok_or_else(|| format!("missing argument '{name}'"))?;
    serde_json::from_value(value.clone()).map_err(|err| err.to_string())
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
        "",
        "Commands:",
        "  webui                         Start embedded WebUI server",
        "  providers                     Manage providers",
        "",
        "Examples:",
        "  cc-switch-cli webui",
        "  cc-switch-cli webui --port 9990",
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

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
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
                port: DEFAULT_WEBUI_PORT
            })
        );
    }

    #[test]
    fn parses_webui_command_with_custom_port() {
        assert_eq!(
            parse_cli_command(&strings(&["webui", "--host", "0.0.0.0", "--port", "9991"])).unwrap(),
            CliCommand::WebUi(WebUiOptions {
                host: "0.0.0.0".to_string(),
                port: 9991
            })
        );
    }

    #[test]
    fn rejects_invalid_webui_port() {
        let err = parse_cli_command(&strings(&["webui", "--port", "0"])).unwrap_err();
        assert_eq!(err.message, "port must be greater than 0");
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
}
