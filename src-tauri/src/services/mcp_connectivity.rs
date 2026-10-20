//! Short-lived MCP handshakes requested by the MCP page, never by app startup.
use std::{collections::HashMap, process::Stdio, sync::Arc, time::Duration};

use futures::{future::AbortHandle, future::Abortable, stream, StreamExt};
use once_cell::sync::Lazy;
use process_wrap::tokio::{KillOnDrop, TokioChildWrapper, TokioCommandWrap};
use rmcp::{
    model::{ClientInfo, Implementation},
    transport::{
        async_rw::AsyncRwTransport,
        common::client_side_sse::{BoxedSseResponse, NeverRetry},
        sse_client::{SseClient, SseClientConfig, SseTransportError},
        streamable_http_client::StreamableHttpClientTransportConfig,
        SseClientTransport, StreamableHttpClientTransport, Transport,
    },
    RoleClient, ServiceExt,
};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::app_config::McpServer;

// Package launchers such as npx/uvx may need time to prepare a cold start.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONCURRENT_CHECKS: usize = 3;
static BATCHES: Lazy<std::sync::Mutex<HashMap<String, AbortHandle>>> = Lazy::new(Default::default);
// GUI launches can inherit only launchd's minimal PATH. Resolve the user's shell
// once, off the async executor, and share it across all page visits and probes.
#[cfg(not(target_os = "windows"))]
static LOGIN_SHELL_PATH: Lazy<Option<String>> = Lazy::new(crate::commands::login_shell_path);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityResult {
    pub server_id: String,
    pub connected: bool,
}

#[derive(Deserialize)]
struct ConnectionSpec {
    #[serde(rename = "type")]
    transport: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    cwd: Option<String>,
    url: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
}

pub fn start_checks(
    servers: Vec<McpServer>,
    on_result: impl Fn(ConnectivityResult) -> bool + Send + 'static,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let (abort, registration) = AbortHandle::new_pair();
    BATCHES.lock().unwrap().insert(id.clone(), abort);
    let batch_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let checks = async move {
            let mut results = stream::iter(servers.into_iter().map(|server| async move {
                ConnectivityResult {
                    server_id: server.id,
                    connected: check_server(server.server, CHECK_TIMEOUT).await,
                }
            }))
            .buffer_unordered(MAX_CONCURRENT_CHECKS);
            while let Some(result) = results.next().await {
                if !on_result(result) {
                    break; // The page/channel has gone away; drop queued checks too.
                }
            }
        };
        let _ = Abortable::new(checks, registration).await;
        BATCHES.lock().unwrap().remove(&batch_id);
    });
    id
}

pub fn cancel_checks(batch_id: &str) {
    if let Some(abort) = BATCHES.lock().unwrap().remove(batch_id) {
        abort.abort();
    }
}

async fn check_server(value: serde_json::Value, timeout: Duration) -> bool {
    // Do not return/log transport errors: URLs, headers and process arguments may
    // contain credentials. The UI only needs a red/green connectivity result.
    matches!(
        tokio::time::timeout(timeout, connect(value)).await,
        Ok(Ok(()))
    )
}

async fn connect(value: serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let spec: ConnectionSpec = serde_json::from_value(value)?;
    let transport = spec.transport.as_deref().unwrap_or_else(|| {
        if spec.url.is_some() && spec.command.is_none() {
            "http"
        } else {
            "stdio"
        }
    });
    match transport {
        "stdio" => {
            let program = spec
                .command
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or("Missing MCP command")?;
            #[cfg(not(windows))]
            let mut command = Command::new(program);
            #[cfg(windows)]
            let mut command = Command::new(resolve_windows_command(program, &spec));
            #[cfg(not(target_os = "windows"))]
            if !spec.env.contains_key("PATH") {
                let login_path = tokio::task::spawn_blocking(|| LOGIN_SHELL_PATH.clone())
                    .await
                    .ok()
                    .flatten();
                let inherited = std::env::var("PATH").unwrap_or_default();
                apply_login_shell_path(&mut command, login_path.as_deref(), &inherited);
            }
            command
                .args(&spec.args)
                .envs(&spec.env)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            if let Some(cwd) = spec.cwd.as_deref().filter(|s| !s.is_empty()) {
                command.current_dir(cwd);
            }
            let mut command = TokioCommandWrap::from(command);
            command.wrap(KillOnDrop);
            // Kill the entire process tree, including children of npx/uvx.
            #[cfg(unix)]
            command.wrap(process_wrap::tokio::ProcessGroup::leader());
            #[cfg(windows)]
            {
                let mut flags = process_wrap::tokio::CreationFlags(Default::default());
                flags.0 .0 = 0x08000000; // CREATE_NO_WINDOW
                command.wrap(flags).wrap(process_wrap::tokio::JobObject);
            }
            let mut process = ProbeProcess(Some(command.spawn()?));
            let child = process.0.as_mut().unwrap().inner_mut();
            let stdout = child.stdout().take().ok_or("Missing MCP stdout")?;
            let stdin = child.stdin().take().ok_or("Missing MCP stdin")?;
            let result = handshake(AsyncRwTransport::new_client(stdout, stdin)).await;
            drop(process);
            result
        }
        "http" | "sse" => {
            let url = spec.url.as_deref().ok_or("Missing MCP URL")?;
            let mut headers = reqwest::header::HeaderMap::new();
            for (name, value) in &spec.headers {
                headers.insert(
                    reqwest::header::HeaderName::from_bytes(name.as_bytes())?,
                    reqwest::header::HeaderValue::from_str(value)?,
                );
            }
            let client = reqwest::Client::builder()
                .default_headers(headers)
                .connect_timeout(CHECK_TIMEOUT)
                .timeout(CHECK_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            if transport == "sse" {
                let transport = SseClientTransport::start_with_client(
                    SameOriginSseClient {
                        client,
                        origin: url.parse()?,
                    },
                    SseClientConfig {
                        sse_endpoint: url.into(),
                        retry_policy: Arc::new(NeverRetry),
                        ..Default::default()
                    },
                )
                .await?;
                handshake(transport).await
            } else {
                let transport = StreamableHttpClientTransport::with_client(
                    client,
                    StreamableHttpClientTransportConfig {
                        retry_config: Arc::new(NeverRetry),
                        ..StreamableHttpClientTransportConfig::with_uri(url)
                    },
                );
                handshake(transport).await
            }
        }
        _ => Err("Unsupported MCP transport".into()),
    }
}

/// Inject before configured env so a server's explicit PATH always wins.
#[cfg(not(target_os = "windows"))]
fn apply_login_shell_path(command: &mut Command, login_path: Option<&str>, inherited: &str) {
    if let Some(login_path) = login_path {
        command.env(
            "PATH",
            crate::commands::merge_path_segments(login_path, inherited),
        );
    }
}

/// Always terminate the process group/job, even after a successful handshake.
/// A launcher can exit cleanly while its server child is still running.
struct ProbeProcess(Option<Box<dyn TokioChildWrapper>>);

impl Drop for ProbeProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.start_kill();
            tokio::spawn(async move {
                let _ = Box::into_pin(child.wait()).await;
            });
        }
    }
}

async fn handshake<T: Transport<RoleClient> + 'static>(
    transport: T,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = ClientInfo {
        client_info: Implementation {
            name: "cc-switch-connectivity".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            ..Default::default()
        },
        ..Default::default()
    };
    // serve performs initialize + notifications/initialized. No tools are called.
    let service = client.serve(transport).await?;
    // A drop guard also closes the transport if the page leaves or we time out.
    let _ = service.cancel().await;
    Ok(())
}

/// A legacy SSE endpoint must not forward configured credentials to another host.
#[derive(Clone)]
struct SameOriginSseClient {
    client: reqwest::Client,
    origin: http::Uri,
}

impl SseClient for SameOriginSseClient {
    type Error = reqwest::Error;

    async fn post_message(
        &self,
        uri: http::Uri,
        message: rmcp::model::ClientJsonRpcMessage,
        auth_token: Option<String>,
    ) -> Result<(), SseTransportError<Self::Error>> {
        if uri.scheme() != self.origin.scheme() || uri.authority() != self.origin.authority() {
            return Err(SseTransportError::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Cross-origin MCP message endpoint",
            )));
        }
        self.client.post_message(uri, message, auth_token).await
    }

    async fn get_stream(
        &self,
        uri: http::Uri,
        last_event_id: Option<String>,
        auth_token: Option<String>,
    ) -> Result<BoxedSseResponse, SseTransportError<Self::Error>> {
        self.client.get_stream(uri, last_event_id, auth_token).await
    }
}

#[cfg(windows)]
fn resolve_windows_command(program: &str, spec: &ConnectionSpec) -> std::path::PathBuf {
    use std::path::PathBuf;
    let path = PathBuf::from(program);
    if path.extension().is_some() {
        return path;
    }
    let path_env = spec
        .env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| std::ffi::OsString::from(value))
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    let directories = if path.components().count() > 1 {
        vec![PathBuf::new()]
    } else {
        std::env::split_paths(&path_env).collect()
    };
    for directory in directories {
        for extension in ["exe", "cmd", "bat", "com"] {
            let candidate = directory.join(&path).with_extension(extension);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    path
}

#[cfg(test)]
mod tests;
