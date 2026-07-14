use std::env;
use std::fs;
use std::net::Ipv6Addr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ZedRemoteError {
    #[error("{0}")]
    Validation(&'static str),
    #[error("Cannot read Codex remote connection state")]
    StateRead(#[source] std::io::Error),
    #[error("Cannot parse Codex remote connection state")]
    StateParse(#[source] serde_json::Error),
    #[error("Cannot read Codex++ Zed remote project registry")]
    RegistryRead(#[source] std::io::Error),
    #[error("Cannot parse Codex++ Zed remote project registry")]
    RegistryParse(#[source] serde_json::Error),
    #[error("Cannot write Codex++ Zed remote project registry")]
    RegistryWrite(#[source] std::io::Error),
    #[error("Failed to launch Zed: {0}")]
    Launch(std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshTarget {
    pub user: String,
    pub host: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ZedOpenStrategy {
    AddToFocusedWorkspace,
    ReuseWindow,
    NewWindow,
    Default,
}

impl Default for ZedOpenStrategy {
    fn default() -> Self {
        Self::AddToFocusedWorkspace
    }
}

pub fn candidate_zed_app_paths() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("/Applications/Zed.app"),
        PathBuf::from("/Applications/Zed Preview.app"),
        PathBuf::from("/Applications/Zed Nightly.app"),
    ];
    if let Some(home) = home_dir() {
        paths.push(home.join("Applications/Zed.app"));
        paths.push(home.join("Applications/Zed Preview.app"));
        paths.push(home.join("Applications/Zed Nightly.app"));
    }
    paths
}

pub fn find_zed_app_path() -> Option<PathBuf> {
    candidate_zed_app_paths()
        .into_iter()
        .find(|path| path.exists())
}

pub fn find_zed_cli_path() -> String {
    find_executable_on_path("zed")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn zed_remote_status() -> Value {
    let app_path = find_zed_app_path();
    let cli_path = find_zed_cli_path();
    let platform_supported =
        cfg!(target_os = "macos") || cfg!(target_os = "windows") || cfg!(target_os = "linux");
    json!({
        "status": if platform_supported { "ok" } else { "failed" },
        "platformSupported": platform_supported,
        "zedAppFound": app_path.is_some(),
        "zedCliFound": !cli_path.is_empty(),
        "zedAppPath": app_path.map(|path| path.to_string_lossy().into_owned()).unwrap_or_default(),
        "zedCliPath": cli_path,
    })
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    }
}

pub fn split_ssh_authority(value: &str) -> Result<(String, String, Option<u16>), ZedRemoteError> {
    let mut authority = value.trim();
    if authority.is_empty() {
        return Ok((String::new(), String::new(), None));
    }
    let mut user = "";
    if let Some(index) = authority.rfind('@') {
        user = &authority[..index];
        authority = &authority[index + 1..];
    }

    if authority.starts_with('[') {
        if let Some(close_index) = authority.find(']') {
            let host = authority[..=close_index].trim().to_string();
            let suffix = &authority[close_index + 1..];
            let port = if let Some(raw_port) = suffix.strip_prefix(':') {
                parse_port_str(raw_port)?
            } else {
                None
            };
            return Ok((user.trim().to_string(), host, port));
        }
        return Ok((user.trim().to_string(), authority.trim().to_string(), None));
    }

    if authority.matches(':').count() == 1 {
        let (host, raw_port) = authority.rsplit_once(':').unwrap_or((authority, ""));
        if raw_port.chars().all(|ch| ch.is_ascii_digit()) && !raw_port.is_empty() {
            return Ok((
                user.trim().to_string(),
                host.trim().to_string(),
                parse_port_str(raw_port)?,
            ));
        }
    }
    Ok((user.trim().to_string(), authority.trim().to_string(), None))
}

fn parse_port_value(value: Option<&Value>) -> Result<Option<u16>, ZedRemoteError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
        Some(Value::String(value)) => parse_port_str(value.trim()),
        Some(Value::Number(value)) => {
            let port = value
                .as_u64()
                .ok_or(ZedRemoteError::Validation("Invalid SSH port"))?;
            u16::try_from(port)
                .ok()
                .filter(|port| *port >= 1)
                .ok_or(ZedRemoteError::Validation("Invalid SSH port"))
                .map(Some)
        }
        _ => Err(ZedRemoteError::Validation("Invalid SSH port")),
    }
}

fn parse_port_str(value: &str) -> Result<Option<u16>, ZedRemoteError> {
    if value.is_empty() {
        return Ok(None);
    }
    if !value.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(ZedRemoteError::Validation("Invalid SSH port"));
    }
    let port: u16 = value
        .parse()
        .map_err(|_| ZedRemoteError::Validation("Invalid SSH port"))?;
    if port == 0 {
        return Err(ZedRemoteError::Validation("Invalid SSH port"));
    }
    Ok(Some(port))
}

pub fn validate_ssh_host(host: &str) -> Result<String, ZedRemoteError> {
    let host = host.trim();
    if host.is_empty() {
        return Err(ZedRemoteError::Validation(
            "Cannot determine remote SSH host for this file",
        ));
    }
    if host
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace() || matches!(ch, '/' | '?' | '#' | '@'))
    {
        return Err(ZedRemoteError::Validation("Invalid SSH host"));
    }
    if host.starts_with('[') || host.ends_with(']') {
        if !(host.starts_with('[') && host.ends_with(']')) {
            return Err(ZedRemoteError::Validation("Invalid SSH host"));
        }
        host[1..host.len() - 1]
            .parse::<Ipv6Addr>()
            .map_err(|_| ZedRemoteError::Validation("Invalid SSH host"))?;
        return Ok(host.to_string());
    }
    if host.contains('[') || host.contains(']') {
        return Err(ZedRemoteError::Validation("Invalid SSH host"));
    }
    Ok(host.to_string())
}

pub fn target_from_payload(payload: &Value) -> Result<SshTarget, ZedRemoteError> {
    let ssh = payload.get("ssh").and_then(Value::as_object);
    let raw_host = ssh
        .map(|ssh| {
            string_value(ssh.get("host"))
                .or_else_nonempty(|| string_value(ssh.get("hostname")))
                .or_else_nonempty(|| string_value(ssh.get("hostName")))
        })
        .unwrap_or_default();
    let (authority_user, authority_host, authority_port) = split_ssh_authority(&raw_host)?;
    let user = ssh
        .map(|ssh| {
            string_value(ssh.get("user")).or_else_nonempty(|| string_value(ssh.get("username")))
        })
        .unwrap_or_default()
        .or_else_nonempty(|| authority_user.clone());
    let host = validate_ssh_host(&authority_host)?;
    let port = match ssh.and_then(|ssh| ssh.get("port")) {
        Some(Value::Null) | None => authority_port,
        Some(Value::String(value)) if value.trim().is_empty() => authority_port,
        value => parse_port_value(value)?,
    };
    Ok(SshTarget { user, host, port })
}

pub fn encode_remote_path(path: &str) -> Result<String, ZedRemoteError> {
    if path.is_empty() {
        return Err(ZedRemoteError::Validation("Remote path is required"));
    }
    if !path.starts_with('/') {
        return Err(ZedRemoteError::Validation("Remote path must be absolute"));
    }
    Ok(path
        .split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/"))
}

pub fn build_zed_remote_url(target: &SshTarget, path: &str) -> Result<String, ZedRemoteError> {
    let host = validate_ssh_host(&target.host)?;
    let port = target
        .port
        .map(|port| {
            if port == 0 {
                Err(ZedRemoteError::Validation("Invalid SSH port"))
            } else {
                Ok(port)
            }
        })
        .transpose()?;
    let user_prefix = if target.user.trim().is_empty() {
        String::new()
    } else {
        format!("{}@", percent_encode_segment(target.user.trim()))
    };
    let port_suffix = port.map(|port| format!(":{port}")).unwrap_or_default();
    let encoded_path = encode_remote_path(path)?;
    Ok(format!(
        "ssh://{user_prefix}{host}{port_suffix}{encoded_path}"
    ))
}

fn percent_encode_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_' | '~') {
            encoded.push(ch);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub fn zed_cli_args_for_strategy(strategy: ZedOpenStrategy, url: &str) -> Vec<String> {
    let mut args = Vec::new();
    match strategy {
        ZedOpenStrategy::AddToFocusedWorkspace => args.push("-a".to_string()),
        ZedOpenStrategy::ReuseWindow => args.push("-r".to_string()),
        ZedOpenStrategy::NewWindow => args.push("-n".to_string()),
        ZedOpenStrategy::Default => {}
    }
    args.push(url.to_string());
    args
}

pub fn zed_open_strategy_from_payload(payload: &Value) -> ZedOpenStrategy {
    payload
        .get("strategy")
        .and_then(|value| serde_json::from_value::<ZedOpenStrategy>(value.clone()).ok())
        .unwrap_or_default()
}

pub fn launch_zed_url_with_strategy(
    url: &str,
    strategy: ZedOpenStrategy,
) -> Result<(), ZedRemoteError> {
    let cli_path = find_zed_cli_path();
    if !cli_path.is_empty() {
        Command::new(cli_path)
            .args(zed_cli_args_for_strategy(strategy, url))
            .spawn()
            .map_err(ZedRemoteError::Launch)?;
        return Ok(());
    }
    if cfg!(target_os = "macos") {
        if let Some(app_path) = find_zed_app_path() {
            Command::new("open")
                .arg("-a")
                .arg(app_path)
                .arg(url)
                .spawn()
                .map_err(ZedRemoteError::Launch)?;
            return Ok(());
        }
    }
    Err(ZedRemoteError::Validation(
        "Zed CLI is not installed or not available on PATH",
    ))
}

pub fn launch_zed_url(url: &str) -> Result<(), ZedRemoteError> {
    launch_zed_url_with_strategy(url, ZedOpenStrategy::Default)
}

pub fn codex_global_state_path() -> PathBuf {
    crate::codex_home::default_codex_home_dir().join(".codex-global-state.json")
}

pub fn target_from_managed_remote_connection(
    connection: &serde_json::Map<String, Value>,
) -> Result<SshTarget, ZedRemoteError> {
    let ssh_host = string_value(connection.get("sshHost"))
        .or_else_nonempty(|| string_value(connection.get("hostname")));
    let ssh_alias = string_value(connection.get("sshAlias"))
        .or_else_nonempty(|| string_value(connection.get("alias")));
    let (authority_user, authority_host, authority_port) = split_ssh_authority(&ssh_host)?;
    let host = authority_host.or_else_nonempty(|| ssh_alias.clone());
    let user = string_value(connection.get("sshUser"))
        .or_else_nonempty(|| string_value(connection.get("user")))
        .or_else_nonempty(|| authority_user.clone());
    let port = match connection.get("sshPort") {
        Some(Value::Null) | None => authority_port,
        Some(Value::String(value)) if value.trim().is_empty() => authority_port,
        value => parse_port_value(value)?,
    };
    Ok(SshTarget {
        user,
        host: validate_ssh_host(&host)?,
        port,
    })
}

pub fn resolve_ssh_target_from_global_state(
    state: &Value,
    host_id: &str,
) -> Result<SshTarget, ZedRemoteError> {
    if host_id.is_empty() {
        return Err(ZedRemoteError::Validation("Remote host id is required"));
    }
    let connections = state
        .get("codex-managed-remote-connections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for connection in connections {
        let Some(connection) = connection.as_object() else {
            continue;
        };
        if string_value(connection.get("hostId")) != host_id {
            continue;
        }
        return target_from_managed_remote_connection(connection);
    }
    Err(ZedRemoteError::Validation(
        "Cannot resolve remote SSH host for this file",
    ))
}

pub fn resolve_ssh_target_for_host_id(
    host_id: &str,
    state_path: Option<&Path>,
) -> Result<SshTarget, ZedRemoteError> {
    if host_id.is_empty() {
        return Err(ZedRemoteError::Validation("Remote host id is required"));
    }
    let path = state_path
        .map(Path::to_path_buf)
        .unwrap_or_else(codex_global_state_path);
    let data = fs::read_to_string(path).map_err(ZedRemoteError::StateRead)?;
    let state: Value = serde_json::from_str(&data).map_err(ZedRemoteError::StateParse)?;
    resolve_ssh_target_from_global_state(&state, host_id)
}

mod fallback;
mod registry;

pub use fallback::{
    fallback_open_request_from_global_state_with_context, fallback_open_request_response,
    workspace_root_from_sqlite,
};
pub use registry::{
    ZedRemoteProject, ZedRemoteProjectSource, forget_zed_remote_project_response,
    list_zed_remote_projects_from_state, list_zed_remote_projects_response,
    remember_zed_remote_project_response,
};

pub fn resolve_ssh_target_response(payload: &Value) -> Value {
    let host_id = string_value(payload.get("hostId"));
    match resolve_ssh_target_for_host_id(&host_id, None) {
        Ok(target) => json!({
            "status": "ok",
            "ssh": { "user": target.user, "host": target.host, "port": target.port },
        }),
        Err(error) => json!({"status": "failed", "message": error.to_string()}),
    }
}

pub fn open_zed_remote(payload: &Value) -> Value {
    let strategy = zed_open_strategy_from_payload(payload);
    let result = target_from_payload(payload).and_then(|target| {
        let path = string_value(payload.get("path"));
        let url = build_zed_remote_url(&target, &path)?;
        launch_zed_url_with_strategy(&url, strategy)?;
        if payload
            .get("remember")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            let _ = registry::remember_zed_remote_project(payload, Some(&target), Some(&url));
        }
        Ok(url)
    });
    match result {
        Ok(url) => json!({"status": "ok", "url": url, "strategy": strategy}),
        Err(error) => json!({"status": "failed", "message": error.to_string()}),
    }
}

trait NonEmptyStringExt {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String;
}

impl NonEmptyStringExt for String {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String,
    {
        if self.is_empty() { fallback() } else { self }
    }
}
