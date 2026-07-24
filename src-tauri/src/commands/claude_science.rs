use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use hkdf::Hkdf;
use rand::{rngs::OsRng, RngCore};
use serde::Serialize;
use serde_json::Value;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::store::AppState;

const CLAUDE_SCIENCE_BIN_ENV: &str = "CLAUDE_SCIENCE_BIN";
const MANAGED_PROFILE_DIR: &str = "claude-science-proxy";
const SCIENCE_PROXY_USER_ID: &str = "local-dev";
const SCIENCE_PROXY_EMAIL: &str = "cc-switch-proxy@local.invalid";
const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";
const OAUTH_TOKEN_DIR: &str = ".oauth-tokens";
const OAUTH_TOKEN_FILENAME: &str = "local-dev.enc";
const OAUTH_HKDF_INFO: &[u8] = b"operon:aes-256-gcm:oauth";
const OAUTH_AAD: &[u8] = b"v2:oauth";
const OAUTH_TOKEN_PREFIX: &str = "v2:";
const OAUTH_TOKEN_TTL_DAYS: i64 = 30;
const REQUIRED_OAUTH_KEY: &str = "OAUTH_ENCRYPTION_KEY";
const ENCRYPTION_KEY_FILENAME: &str = "encryption.key";
const LAUNCH_POLL_ATTEMPTS: usize = 50;
const LAUNCH_POLL_INTERVAL_MS: u64 = 100;
const CLAUDE_SCIENCE_BINARY_NAME: &str = "claude-science";
const SCIENCE_MODEL_ENV_KEYS_TO_CLEAR: &[&str] = &[
    "ANTHROPIC_MODEL",
    "ANTHROPIC_REASONING_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
    "ANTHROPIC_SMALL_FAST_MODEL",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeScienceStatus {
    pub installed: bool,
    pub running: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub binary_path: Option<String>,
    pub proxy_base_url: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeScienceLaunchResult {
    pub proxy_base_url: String,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub binary_path: String,
}

#[derive(Debug, Clone)]
struct ScienceLaunchOutcome {
    public_result: ClaudeScienceLaunchResult,
    url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ParsedScienceStatus {
    running: bool,
    pid: Option<u32>,
    port: Option<u16>,
    url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScienceProfilePaths {
    data_dir: PathBuf,
    auth_dir: PathBuf,
    config_path: PathBuf,
}

#[derive(Debug, Serialize)]
struct ScienceConfig<'a> {
    paths: ScienceConfigPaths<'a>,
}

#[derive(Debug, Serialize)]
struct ScienceConfigPaths<'a> {
    auth_dir: &'a str,
}

#[derive(Debug, Serialize)]
struct ScienceOAuthToken<'a> {
    access_token: &'a str,
    refresh_token: Option<String>,
    api_key: Option<String>,
    token_expires_at: String,
    provider: &'a str,
    scopes: &'a str,
    email: &'a str,
    account_uuid: &'a str,
    org_uuid: Option<String>,
    org_name: Option<String>,
    subscription_type: &'a str,
    rate_limit_tier: Option<String>,
    seat_tier: Option<String>,
    allow_safety_feedback: bool,
    billing_type: Option<String>,
    has_extra_usage_enabled: Option<bool>,
    tier_unmappable: bool,
    billing_resolved: bool,
}

#[tauri::command]
pub async fn get_claude_science_status() -> Result<ClaudeScienceStatus, String> {
    tokio::task::spawn_blocking(read_status)
        .await
        .map_err(|e| format!("Claude Science status task failed: {e}"))?
}

#[tauri::command]
pub async fn stop_claude_science() -> Result<(), String> {
    tokio::task::spawn_blocking(stop_science)
        .await
        .map_err(|e| format!("Claude Science stop task failed: {e}"))?
}

#[tauri::command]
pub async fn launch_claude_science_with_proxy(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ClaudeScienceLaunchResult, String> {
    let proxy_base_url = state
        .proxy_service
        .ensure_running_and_get_proxy_url()
        .await?;

    let launch_outcome = {
        let proxy_base_url = proxy_base_url.clone();
        tokio::task::spawn_blocking(move || launch_science(proxy_base_url))
            .await
            .map_err(|e| format!("Claude Science launch task failed: {e}"))??
    };

    if let Some(url) = launch_outcome.url.as_deref() {
        app.opener()
            .open_url(url, None::<String>)
            .map_err(|e| format!("Failed to open Claude Science URL: {e}"))?;
    }

    Ok(launch_outcome.public_result)
}

fn read_status() -> Result<ClaudeScienceStatus, String> {
    let Some(bin) = find_claude_science_binary() else {
        return Ok(ClaudeScienceStatus {
            installed: false,
            running: false,
            pid: None,
            port: None,
            binary_path: None,
            proxy_base_url: None,
            error: Some("Claude Science CLI was not found".to_string()),
        });
    };

    let profile = managed_profile_paths();
    if !profile.config_path.exists() {
        return Ok(ClaudeScienceStatus {
            installed: true,
            running: false,
            pid: None,
            port: None,
            binary_path: Some(bin.display().to_string()),
            proxy_base_url: None,
            error: None,
        });
    }

    match run_cli(&bin, &["status"], &[], Some(&profile)) {
        Ok(output) if output.status.success() => {
            let parsed = parse_status_output(&output).unwrap_or_default();
            Ok(ClaudeScienceStatus {
                installed: true,
                running: parsed.running,
                pid: parsed.pid,
                port: parsed.port,
                binary_path: Some(bin.display().to_string()),
                proxy_base_url: None,
                error: None,
            })
        }
        Ok(output) => Ok(ClaudeScienceStatus {
            installed: true,
            running: false,
            pid: None,
            port: None,
            binary_path: Some(bin.display().to_string()),
            proxy_base_url: None,
            error: Some(format_cli_failure("Claude Science status failed", &output)),
        }),
        Err(err) => Ok(ClaudeScienceStatus {
            installed: true,
            running: false,
            pid: None,
            port: None,
            binary_path: Some(bin.display().to_string()),
            proxy_base_url: None,
            error: Some(err),
        }),
    }
}

fn stop_science() -> Result<(), String> {
    let bin = find_claude_science_binary()
        .ok_or_else(|| "Claude Science CLI was not found".to_string())?;
    let profile = managed_profile_paths();
    if !profile.config_path.exists() {
        return Ok(());
    }

    stop_science_for_profile(&bin, &profile)
}

fn stop_science_for_profile(bin: &Path, profile: &ScienceProfilePaths) -> Result<(), String> {
    let output = run_cli(bin, &["stop"], &[], Some(profile))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format_cli_failure("Claude Science stop failed", &output))
    }
}

fn launch_science(proxy_base_url: String) -> Result<ScienceLaunchOutcome, String> {
    let bin = find_claude_science_binary()
        .ok_or_else(|| "Claude Science CLI was not found".to_string())?;
    let profile = prepare_managed_profile()?;

    stop_science_for_profile(&bin, &profile)?;

    let proxy_env = proxy_launch_env(&proxy_base_url);
    let output = run_cli_with_env(
        &bin,
        &[
            "serve",
            "--port",
            "0",
            "--detached",
            "--no-browser",
            "--no-auto-update",
        ],
        &proxy_env,
        Some(&profile),
    )?;
    if !output.status.success() {
        return Err(format_cli_failure("Claude Science launch failed", &output));
    }

    let parsed_status = poll_until_running(&bin, &profile)?;
    let url = read_science_url(&bin, &profile)
        .ok()
        .or(parsed_status.url.clone());

    Ok(ScienceLaunchOutcome {
        public_result: ClaudeScienceLaunchResult {
            proxy_base_url,
            pid: parsed_status.pid,
            port: parsed_status.port,
            binary_path: bin.display().to_string(),
        },
        url,
    })
}

fn poll_until_running(
    bin: &Path,
    profile: &ScienceProfilePaths,
) -> Result<ParsedScienceStatus, String> {
    let mut last_status = ParsedScienceStatus::default();
    let mut last_error = None;

    for _ in 0..LAUNCH_POLL_ATTEMPTS {
        match run_cli(bin, &["status"], &[], Some(profile)) {
            Ok(output) if output.status.success() => {
                if let Some(parsed) = parse_status_output(&output) {
                    if parsed.running {
                        return Ok(parsed);
                    }
                    last_status = parsed;
                }
            }
            Ok(output) => {
                last_error = Some(format_cli_failure("Claude Science status failed", &output));
            }
            Err(err) => {
                last_error = Some(err);
            }
        }

        std::thread::sleep(Duration::from_millis(LAUNCH_POLL_INTERVAL_MS));
    }

    if let Some(error) = last_error {
        Err(error)
    } else {
        Err(format!(
            "Claude Science did not report a running daemon within {} ms (last running={})",
            LAUNCH_POLL_ATTEMPTS as u64 * LAUNCH_POLL_INTERVAL_MS,
            last_status.running
        ))
    }
}

fn read_science_url(bin: &Path, profile: &ScienceProfilePaths) -> Result<String, String> {
    let output = run_cli(bin, &["url"], &[], Some(profile))?;
    if !output.status.success() {
        return Err(format_cli_failure(
            "Claude Science URL lookup failed",
            &output,
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    extract_first_http_url(&stdout)
        .ok_or_else(|| "Claude Science URL lookup did not return a URL".to_string())
}

fn proxy_launch_env(proxy_base_url: &str) -> [(&'static str, &str); 3] {
    // Claude Science does not currently document a stable config key for
    // Anthropic client routing. Keep the proxy handoff scoped to this managed
    // daemon launch instead of writing it into the user's default profile.
    [
        ("ANTHROPIC_BASE_URL", proxy_base_url),
        ("ANTHROPIC_AUTH_TOKEN", PROXY_TOKEN_PLACEHOLDER),
        ("ANTHROPIC_API_KEY", PROXY_TOKEN_PLACEHOLDER),
    ]
}

fn run_cli(
    bin: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    profile: Option<&ScienceProfilePaths>,
) -> Result<Output, String> {
    run_cli_with_env(bin, args, envs, profile)
}

fn run_cli_with_env(
    bin: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    profile: Option<&ScienceProfilePaths>,
) -> Result<Output, String> {
    let mut command = Command::new(bin);
    command
        .args(args)
        .envs(envs.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in SCIENCE_MODEL_ENV_KEYS_TO_CLEAR {
        command.env_remove(key);
    }

    if let Some(profile) = profile {
        command
            .arg("--data-dir")
            .arg(&profile.data_dir)
            .arg("--config")
            .arg(&profile.config_path)
            .current_dir(&profile.data_dir);
    }

    command
        .output()
        .map_err(|e| format!("Failed to execute Claude Science CLI: {e}"))
}

fn managed_profile_paths() -> ScienceProfilePaths {
    managed_profile_paths_for_app_config_dir(&crate::config::get_app_config_dir())
}

fn managed_profile_paths_for_app_config_dir(app_config_dir: &Path) -> ScienceProfilePaths {
    let data_dir = app_config_dir.join(MANAGED_PROFILE_DIR);
    let auth_dir = data_dir.clone();
    let config_path = data_dir.join("config.toml");

    ScienceProfilePaths {
        data_dir,
        auth_dir,
        config_path,
    }
}

fn prepare_managed_profile() -> Result<ScienceProfilePaths, String> {
    let profile = managed_profile_paths();
    prepare_profile_at(&profile)?;
    Ok(profile)
}

fn prepare_profile_at(profile: &ScienceProfilePaths) -> Result<(), String> {
    fs::create_dir_all(&profile.data_dir).map_err(|e| {
        format!(
            "Failed to create Claude Science managed data dir {}: {e}",
            profile.data_dir.display()
        )
    })?;
    fs::create_dir_all(&profile.auth_dir).map_err(|e| {
        format!(
            "Failed to create Claude Science managed auth dir {}: {e}",
            profile.auth_dir.display()
        )
    })?;
    fs::create_dir_all(profile.auth_dir.join(OAUTH_TOKEN_DIR)).map_err(|e| {
        format!(
            "Failed to create Claude Science OAuth token dir under {}: {e}",
            profile.auth_dir.display()
        )
    })?;

    set_private_dir_permissions(&profile.data_dir)?;
    set_private_dir_permissions(&profile.auth_dir.join(OAUTH_TOKEN_DIR))?;
    write_science_config(profile)?;
    let oauth_key = ensure_encryption_key(&profile.auth_dir)?;
    write_proxy_managed_oauth_token(&profile.auth_dir, &oauth_key)?;

    Ok(())
}

fn write_science_config(profile: &ScienceProfilePaths) -> Result<(), String> {
    let auth_dir = profile.auth_dir.to_string_lossy().to_string();
    let config = ScienceConfig {
        paths: ScienceConfigPaths {
            auth_dir: auth_dir.as_str(),
        },
    };
    let content = toml::to_string(&config)
        .map_err(|e| format!("Failed to serialize Claude Science config: {e}"))?;
    write_private_file(&profile.config_path, content.as_bytes())
}

fn ensure_encryption_key(auth_dir: &Path) -> Result<String, String> {
    let path = auth_dir.join(ENCRYPTION_KEY_FILENAME);
    let mut keys = if path.exists() {
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read Claude Science encryption key file: {e}"))?;
        parse_key_file(&content)
    } else {
        BTreeMap::new()
    };

    for key_name in [
        "ANTHROPIC_API_KEY_ENCRYPTION_KEY",
        REQUIRED_OAUTH_KEY,
        "JWT_SIGNING_SECRET",
        "USER_SECRET_ENCRYPTION_KEY",
    ] {
        if !keys.contains_key(key_name) {
            keys.insert(key_name.to_string(), random_base64_key());
        }
    }

    let oauth_key = keys
        .get(REQUIRED_OAUTH_KEY)
        .cloned()
        .ok_or_else(|| "Claude Science OAuth encryption key is missing".to_string())?;
    validate_base64_key(&oauth_key)?;

    let content = render_key_file(&keys);
    write_private_file(&path, content.as_bytes())?;
    Ok(oauth_key)
}

fn parse_key_file(content: &str) -> BTreeMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn render_key_file(keys: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for key_name in [
        "ANTHROPIC_API_KEY_ENCRYPTION_KEY",
        REQUIRED_OAUTH_KEY,
        "JWT_SIGNING_SECRET",
        "USER_SECRET_ENCRYPTION_KEY",
    ] {
        if let Some(value) = keys.get(key_name) {
            out.push_str(key_name);
            out.push('=');
            out.push_str(value);
            out.push('\n');
        }
    }

    for (key, value) in keys {
        if matches!(
            key.as_str(),
            "ANTHROPIC_API_KEY_ENCRYPTION_KEY"
                | REQUIRED_OAUTH_KEY
                | "JWT_SIGNING_SECRET"
                | "USER_SECRET_ENCRYPTION_KEY"
        ) {
            continue;
        }
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push('\n');
    }

    out
}

fn random_base64_key() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    BASE64_STANDARD.encode(bytes)
}

fn validate_base64_key(value: &str) -> Result<(), String> {
    let decoded = BASE64_STANDARD
        .decode(value)
        .map_err(|e| format!("Claude Science encryption key is not valid base64: {e}"))?;
    if decoded.len() != 32 {
        return Err(format!(
            "Claude Science encryption key must decode to 32 bytes, got {}",
            decoded.len()
        ));
    }
    Ok(())
}

fn write_proxy_managed_oauth_token(auth_dir: &Path, oauth_key: &str) -> Result<(), String> {
    let token_dir = auth_dir.join(OAUTH_TOKEN_DIR);
    fs::create_dir_all(&token_dir).map_err(|e| {
        format!(
            "Failed to create Claude Science OAuth token dir {}: {e}",
            token_dir.display()
        )
    })?;

    let token = ScienceOAuthToken {
        access_token: PROXY_TOKEN_PLACEHOLDER,
        refresh_token: None,
        api_key: None,
        token_expires_at: proxy_token_expiry(),
        provider: "claude_ai",
        scopes: "openid profile email user:inference user:file_upload user:profile user:mcp_servers user:plugins",
        email: SCIENCE_PROXY_EMAIL,
        account_uuid: SCIENCE_PROXY_USER_ID,
        org_uuid: None,
        org_name: None,
        subscription_type: "pro",
        rate_limit_tier: None,
        seat_tier: None,
        allow_safety_feedback: false,
        billing_type: None,
        has_extra_usage_enabled: None,
        tier_unmappable: false,
        billing_resolved: true,
    };
    let plaintext = serde_json::to_vec(&token)
        .map_err(|e| format!("Failed to serialize Claude Science OAuth token: {e}"))?;
    let encrypted = encrypt_oauth_payload(oauth_key, &plaintext)?;
    write_private_file(&token_dir.join(OAUTH_TOKEN_FILENAME), encrypted.as_bytes())
}

fn proxy_token_expiry() -> String {
    (Utc::now() + ChronoDuration::days(OAUTH_TOKEN_TTL_DAYS))
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn encrypt_oauth_payload(oauth_key: &str, plaintext: &[u8]) -> Result<String, String> {
    let ikm = BASE64_STANDARD
        .decode(oauth_key)
        .map_err(|e| format!("Claude Science OAuth encryption key is not valid base64: {e}"))?;
    let hk = Hkdf::<Sha256>::new(None, &ikm);
    let mut key = [0_u8; 32];
    hk.expand(OAUTH_HKDF_INFO, &mut key)
        .map_err(|_| "Failed to derive Claude Science OAuth encryption key".to_string())?;

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Failed to initialize Claude Science token cipher: {e}"))?;
    let mut nonce = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: OAUTH_AAD,
            },
        )
        .map_err(|e| format!("Failed to encrypt Claude Science OAuth token: {e}"))?;

    let mut payload = Vec::with_capacity(nonce.len() + ciphertext.len());
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);

    Ok(format!(
        "{OAUTH_TOKEN_PREFIX}{}",
        BASE64_STANDARD.encode(payload)
    ))
}

fn write_private_file(path: &Path, content: &[u8]) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    set_private_file_permissions(path)
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| {
        format!(
            "Failed to set private permissions on {}: {e}",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| {
        format!(
            "Failed to set private permissions on {}: {e}",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn find_claude_science_binary() -> Option<PathBuf> {
    find_claude_science_binary_from(
        std::env::var(CLAUDE_SCIENCE_BIN_ENV).ok(),
        home_dir(),
        std::env::var_os("PATH"),
    )
}

fn find_claude_science_binary_from(
    override_path: Option<String>,
    home: Option<PathBuf>,
    path_var: Option<OsString>,
) -> Option<PathBuf> {
    find_first_executable(claude_science_binary_candidates(
        override_path,
        home,
        path_var,
    ))
}

fn claude_science_binary_candidates(
    override_path: Option<String>,
    home: Option<PathBuf>,
    path_var: Option<OsString>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = override_path {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            push_unique_path(&mut candidates, PathBuf::from(trimmed));
        }
    }

    if let Some(home) = home {
        push_unique_path(
            &mut candidates,
            home.join(".claude-science/bin")
                .join(CLAUDE_SCIENCE_BINARY_NAME),
        );
        push_unique_path(
            &mut candidates,
            home.join(".local/bin").join(CLAUDE_SCIENCE_BINARY_NAME),
        );
    }

    if let Some(path_var) = path_var {
        for dir in std::env::split_paths(&path_var) {
            push_unique_path(&mut candidates, dir.join(CLAUDE_SCIENCE_BINARY_NAME));
        }
    }

    push_unique_path(
        &mut candidates,
        PathBuf::from("/Applications/Claude Science.app/Contents/Resources/bin/claude-science"),
    );

    candidates
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn find_first_executable(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates
        .into_iter()
        .find(|path| path.is_file() && is_executable(path))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn parse_status_output(output: &Output) -> Option<ParsedScienceStatus> {
    parse_status_bytes(&output.stdout)
}

fn parse_status_bytes(stdout: &[u8]) -> Option<ParsedScienceStatus> {
    let value: Value = serde_json::from_slice(stdout).ok()?;
    let running = value
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let pid = value
        .get("pid")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    let port = value
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|n| u16::try_from(n).ok());
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
        .map(ToString::to_string);

    Some(ParsedScienceStatus {
        running,
        pid,
        port,
        url,
    })
}

fn extract_first_http_url(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("http://") || line.starts_with("https://"))
        .map(ToString::to_string)
}

fn format_cli_failure(context: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    format!("{context}: {detail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn fixed_base64_key(byte: u8) -> String {
        BASE64_STANDARD.encode([byte; 32])
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create executable parent");
        }
        fs::write(path, content).expect("write executable");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("mark executable");
    }

    fn decrypt_oauth_payload_for_test(oauth_key: &str, encrypted: &str) -> Vec<u8> {
        let payload = encrypted
            .strip_prefix(OAUTH_TOKEN_PREFIX)
            .expect("encrypted token prefix");
        let payload = BASE64_STANDARD
            .decode(payload)
            .expect("decode encrypted payload");
        assert!(payload.len() > 28, "nonce + ciphertext + tag");

        let nonce = &payload[..12];
        let ciphertext = &payload[12..];
        let ikm = BASE64_STANDARD.decode(oauth_key).expect("decode oauth key");
        let hk = Hkdf::<Sha256>::new(None, &ikm);
        let mut key = [0_u8; 32];
        hk.expand(OAUTH_HKDF_INFO, &mut key)
            .expect("derive oauth key");
        let cipher = Aes256Gcm::new_from_slice(&key).expect("cipher");

        cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: OAUTH_AAD,
                },
            )
            .expect("decrypt oauth token")
    }

    #[test]
    fn managed_profile_paths_stay_under_cc_switch_config_dir() {
        let root = PathBuf::from("/tmp/cc-switch-test-config");
        let paths = managed_profile_paths_for_app_config_dir(&root);

        assert_eq!(paths.data_dir, root.join(MANAGED_PROFILE_DIR));
        assert_eq!(paths.auth_dir, root.join(MANAGED_PROFILE_DIR));
        assert_eq!(
            paths.config_path,
            root.join(MANAGED_PROFILE_DIR).join("config.toml")
        );
    }

    #[test]
    fn ensure_encryption_key_preserves_existing_oauth_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let oauth_key = fixed_base64_key(7);
        let key_path = tmp.path().join(ENCRYPTION_KEY_FILENAME);
        fs::write(&key_path, format!("{REQUIRED_OAUTH_KEY}={oauth_key}\n"))
            .expect("seed encryption key");

        let returned = ensure_encryption_key(tmp.path()).expect("ensure encryption key");
        let rendered = fs::read_to_string(&key_path).expect("read encryption key");

        assert_eq!(returned, oauth_key);
        assert!(rendered.contains(&format!("{REQUIRED_OAUTH_KEY}={oauth_key}\n")));
        assert!(rendered.contains("ANTHROPIC_API_KEY_ENCRYPTION_KEY="));
        assert!(rendered.contains("JWT_SIGNING_SECRET="));
        assert!(rendered.contains("USER_SECRET_ENCRYPTION_KEY="));
    }

    #[test]
    fn prepare_profile_writes_config_and_proxy_managed_oauth_token() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profile = managed_profile_paths_for_app_config_dir(tmp.path());

        prepare_profile_at(&profile).expect("prepare profile");

        let config = fs::read_to_string(&profile.config_path).expect("read config");
        assert!(config.contains("[paths]"));
        assert!(config.contains("auth_dir"));
        assert!(config.contains(&profile.auth_dir.to_string_lossy().to_string()));

        let key_file =
            fs::read_to_string(profile.auth_dir.join(ENCRYPTION_KEY_FILENAME)).expect("key file");
        let keys = parse_key_file(&key_file);
        let oauth_key = keys.get(REQUIRED_OAUTH_KEY).expect("oauth key");
        validate_base64_key(oauth_key).expect("valid oauth key");

        let encrypted = fs::read_to_string(
            profile
                .auth_dir
                .join(OAUTH_TOKEN_DIR)
                .join(OAUTH_TOKEN_FILENAME),
        )
        .expect("encrypted oauth token");
        let plaintext = decrypt_oauth_payload_for_test(oauth_key, encrypted.trim());
        let token: Value = serde_json::from_slice(&plaintext).expect("token json");

        assert_eq!(token["access_token"], PROXY_TOKEN_PLACEHOLDER);
        assert_eq!(token["email"], SCIENCE_PROXY_EMAIL);
        assert_eq!(token["account_uuid"], SCIENCE_PROXY_USER_ID);
        assert_eq!(token["provider"], "claude_ai");
        let scopes = token["scopes"].as_str().expect("scopes");
        assert!(scopes.contains("user:inference"));
        assert!(scopes.contains("user:file_upload"));
        assert!(scopes.contains("user:mcp_servers"));
        assert!(scopes.contains("user:plugins"));
    }

    #[test]
    fn proxy_launch_env_points_science_at_local_proxy() {
        let env = proxy_launch_env("http://127.0.0.1:15721");

        assert_eq!(env[0], ("ANTHROPIC_BASE_URL", "http://127.0.0.1:15721"));
        assert_eq!(env[1], ("ANTHROPIC_AUTH_TOKEN", PROXY_TOKEN_PLACEHOLDER));
        assert_eq!(env[2], ("ANTHROPIC_API_KEY", PROXY_TOKEN_PLACEHOLDER));
    }

    #[test]
    fn binary_candidates_include_supported_locations() {
        let home = PathBuf::from("/home/science-user");
        let path_entries = [PathBuf::from("/opt/science/bin"), PathBuf::from("/usr/bin")];
        let path_var = std::env::join_paths(path_entries.iter()).expect("join PATH");

        let candidates = claude_science_binary_candidates(
            Some(" /custom/claude-science ".to_string()),
            Some(home.clone()),
            Some(path_var),
        );

        assert_eq!(candidates[0], PathBuf::from("/custom/claude-science"));
        assert!(candidates.contains(
            &home
                .join(".claude-science/bin")
                .join(CLAUDE_SCIENCE_BINARY_NAME)
        ));
        assert!(candidates.contains(&home.join(".local/bin").join(CLAUDE_SCIENCE_BINARY_NAME)));
        assert!(candidates
            .contains(&PathBuf::from("/opt/science/bin").join(CLAUDE_SCIENCE_BINARY_NAME)));
    }

    #[cfg(unix)]
    #[test]
    fn find_binary_checks_documented_linux_local_bin() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let bin = home.join(".local/bin").join(CLAUDE_SCIENCE_BINARY_NAME);
        write_executable(&bin, "#!/bin/sh\nexit 0\n");

        let found = find_claude_science_binary_from(None, Some(home), None);

        assert_eq!(found, Some(bin));
    }

    #[cfg(unix)]
    #[test]
    fn find_binary_falls_back_to_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path_dir = tmp.path().join("path-bin");
        let bin = path_dir.join(CLAUDE_SCIENCE_BINARY_NAME);
        write_executable(&bin, "#!/bin/sh\nexit 0\n");
        let path_var = std::env::join_paths([path_dir]).expect("join PATH");

        let found = find_claude_science_binary_from(None, None, Some(path_var));

        assert_eq!(found, Some(bin));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn run_cli_scopes_proxy_env_and_clears_model_env() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bin = tmp.path().join("dump-env.sh");
        write_executable(
            &bin,
            r#"#!/bin/sh
printf 'base=%s\n' "${ANTHROPIC_BASE_URL-}"
printf 'auth=%s\n' "${ANTHROPIC_AUTH_TOKEN-}"
printf 'api=%s\n' "${ANTHROPIC_API_KEY-}"
printf 'model=%s\n' "${ANTHROPIC_MODEL-}"
printf 'sonnet_name=%s\n' "${ANTHROPIC_DEFAULT_SONNET_MODEL_NAME-}"
"#,
        );
        let profile = managed_profile_paths_for_app_config_dir(&tmp.path().join("cc-switch"));
        fs::create_dir_all(&profile.data_dir).expect("profile data dir");

        std::env::set_var("ANTHROPIC_MODEL", "stale-model");
        std::env::set_var("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME", "Stale Sonnet");
        let output = run_cli_with_env(
            &bin,
            &[],
            &proxy_launch_env("http://127.0.0.1:15721"),
            Some(&profile),
        )
        .expect("run CLI");
        std::env::remove_var("ANTHROPIC_MODEL");
        std::env::remove_var("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
        assert!(stdout.contains("base=http://127.0.0.1:15721\n"));
        assert!(stdout.contains(&format!("auth={PROXY_TOKEN_PLACEHOLDER}\n")));
        assert!(stdout.contains(&format!("api={PROXY_TOKEN_PLACEHOLDER}\n")));
        assert!(stdout.contains("model=\n"));
        assert!(stdout.contains("sonnet_name=\n"));
    }

    #[test]
    fn parse_status_output_reads_running_fields() {
        let parsed = parse_status_bytes(
            br#"{"running":true,"pid":46657,"port":8011,"url":"http://localhost:8011/?nonce=redacted"}"#,
        )
        .expect("status should parse");

        assert!(parsed.running);
        assert_eq!(parsed.pid, Some(46657));
        assert_eq!(parsed.port, Some(8011));
        assert_eq!(
            parsed.url,
            Some("http://localhost:8011/?nonce=redacted".to_string())
        );
    }

    #[test]
    fn parse_status_output_accepts_minimal_not_running_status() {
        let parsed = parse_status_bytes(br#"{"running":false}"#).expect("status should parse");

        assert!(!parsed.running);
        assert_eq!(parsed.pid, None);
        assert_eq!(parsed.port, None);
        assert_eq!(parsed.url, None);
    }

    #[test]
    fn extract_first_http_url_skips_non_url_lines() {
        let output = "Claude Science\nhttp://localhost:8000/?nonce=redacted\n";

        assert_eq!(
            extract_first_http_url(output),
            Some("http://localhost:8000/?nonce=redacted".to_string())
        );
    }
}
