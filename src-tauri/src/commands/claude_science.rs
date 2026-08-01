use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
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

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::store::AppState;

const CLAUDE_SCIENCE_BIN_ENV: &str = "CLAUDE_SCIENCE_BIN";
const MANAGED_PROFILE_DIR: &str = "claude-science-proxy";
const SCIENCE_PROXY_USER_ID: &str = "00000000-0000-4000-8000-000000157210";
const SCIENCE_PROXY_ORG_ID: &str = "00000000-0000-4000-8000-000000157211";
const SCIENCE_PROXY_EMAIL: &str = "cc-switch-proxy@localhost.invalid";
const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";
const OAUTH_TOKEN_DIR: &str = ".oauth-tokens";
const ACTIVE_ORG_FILENAME: &str = "active-org.json";
const OAUTH_HKDF_INFO: &[u8] = b"operon:aes-256-gcm:oauth";
const OAUTH_AAD: &[u8] = b"v2:oauth";
const OAUTH_TOKEN_PREFIX: &str = "v2:";
const OAUTH_TOKEN_EXPIRY: &str = "2099-01-01T00:00:00.000Z";
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

/// Runtime description for launching Claude Science.
/// On Windows, when the user points the config directory at a WSL path (or a
/// WSL distro is auto-detected), `wsl_distro` is set and the binary/command is
/// resolved inside WSL.
#[derive(Debug, Clone)]
struct ScienceRuntime {
    /// Human-readable binary path/command (used in status responses).
    bin_display: String,
    /// Native path used for non-WSL execution.
    bin_path: PathBuf,
    /// WSL distro name when running inside WSL.
    wsl_distro: Option<String>,
    /// Claude Science data/config home. For WSL runtimes this is the UNC path
    /// (`\\wsl$\Distro\<home>\.claude-science`); for native runtimes it is the
    /// local home (or the user's override).
    config_dir: PathBuf,
    /// WSL version (1 or 2) when running inside WSL. WSL1 shares the Windows
    /// loopback, so only WSL2 needs the proxy URL host rewritten to the
    /// Windows host IP visible from the distro.
    wsl_version: Option<u8>,
}

impl ScienceRuntime {
    fn is_wsl(&self) -> bool {
        self.wsl_distro.is_some()
    }

    fn binary_path_display(&self) -> String {
        self.bin_display.clone()
    }

    fn managed_profile_paths(&self) -> ScienceProfilePaths {
        managed_profile_paths_for_science_home(&self.config_dir)
    }
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

#[derive(Debug, Serialize)]
struct ScienceActiveOrg<'a> {
    org_uuid: &'a str,
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
    let runtime = match find_science_runtime() {
        Ok(runtime) => runtime,
        Err(err) => {
            return Ok(ClaudeScienceStatus {
                installed: false,
                running: false,
                pid: None,
                port: None,
                binary_path: None,
                proxy_base_url: None,
                error: Some(err),
            });
        }
    };

    let profile = runtime.managed_profile_paths();
    if !profile.config_path.exists() {
        return Ok(ClaudeScienceStatus {
            installed: true,
            running: false,
            pid: None,
            port: None,
            binary_path: Some(runtime.binary_path_display()),
            proxy_base_url: None,
            error: None,
        });
    }

    match run_science_cli(&runtime, &["status"], &[], Some(&profile)) {
        Ok(output) if output.status.success() => {
            let parsed = parse_status_output(&output).unwrap_or_default();
            Ok(ClaudeScienceStatus {
                installed: true,
                running: parsed.running,
                pid: parsed.pid,
                port: parsed.port,
                binary_path: Some(runtime.binary_path_display()),
                proxy_base_url: None,
                error: None,
            })
        }
        Ok(output) => Ok(ClaudeScienceStatus {
            installed: true,
            running: false,
            pid: None,
            port: None,
            binary_path: Some(runtime.binary_path_display()),
            proxy_base_url: None,
            error: Some(format_cli_failure("Claude Science status failed", &output)),
        }),
        Err(err) => Ok(ClaudeScienceStatus {
            installed: true,
            running: false,
            pid: None,
            port: None,
            binary_path: Some(runtime.binary_path_display()),
            proxy_base_url: None,
            error: Some(err),
        }),
    }
}

fn stop_science() -> Result<(), String> {
    let runtime = find_science_runtime()?;
    let profile = runtime.managed_profile_paths();
    if !profile.config_path.exists() {
        return Ok(());
    }

    stop_science_for_profile(&runtime, &profile)
}

fn stop_science_for_profile(
    runtime: &ScienceRuntime,
    profile: &ScienceProfilePaths,
) -> Result<(), String> {
    let output = run_science_cli(runtime, &["stop"], &[], Some(profile))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format_cli_failure("Claude Science stop failed", &output))
    }
}

fn launch_science(proxy_base_url: String) -> Result<ScienceLaunchOutcome, String> {
    let runtime = find_science_runtime()?;
    let profile = prepare_managed_profile_at(&runtime)?;

    stop_science_for_profile(&runtime, &profile)?;

    let proxy_env = if runtime.is_wsl() && runtime.wsl_version != Some(1) {
        // WSL2 has its own network namespace: the Windows loopback address is
        // not reachable from the distro, so rewrite the URL host to the
        // Windows host IP visible from WSL (its default gateway). WSL1 shares
        // the Windows loopback and needs no rewrite.
        let wsl_host = runtime
            .wsl_distro
            .as_deref()
            .and_then(wsl_host_ip)
            .ok_or_else(|| "Could not determine the Windows host IP from WSL".to_string())?;
        proxy_launch_env_with_host(&proxy_base_url, Some(&wsl_host))
    } else {
        proxy_launch_env(&proxy_base_url)
    };
    let output = run_science_cli_with_env(
        &runtime,
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

    let parsed_status = poll_until_running(&runtime, &profile)?;
    let url = read_science_url(&runtime, &profile)
        .ok()
        .or(parsed_status.url.clone());

    Ok(ScienceLaunchOutcome {
        public_result: ClaudeScienceLaunchResult {
            proxy_base_url,
            pid: parsed_status.pid,
            port: parsed_status.port,
            binary_path: runtime.binary_path_display(),
        },
        url,
    })
}

fn poll_until_running(
    runtime: &ScienceRuntime,
    profile: &ScienceProfilePaths,
) -> Result<ParsedScienceStatus, String> {
    let mut last_status = ParsedScienceStatus::default();
    let mut last_error = None;

    for _ in 0..LAUNCH_POLL_ATTEMPTS {
        match run_science_cli(runtime, &["status"], &[], Some(profile)) {
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

fn read_science_url(
    runtime: &ScienceRuntime,
    profile: &ScienceProfilePaths,
) -> Result<String, String> {
    let output = run_science_cli(runtime, &["url"], &[], Some(profile))?;
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

fn proxy_launch_env(proxy_base_url: &str) -> [(&'static str, String); 3] {
    proxy_launch_env_with_host(proxy_base_url, None)
}

fn proxy_launch_env_with_host(
    proxy_base_url: &str,
    host_override: Option<&str>,
) -> [(&'static str, String); 3] {
    // Claude Science does not currently document a stable config key for
    // Anthropic client routing. Keep the proxy handoff scoped to this managed
    // daemon launch instead of writing it into the user's default profile.
    //
    // Provider switching for Claude Science happens through the proxy's
    // independent route namespace: point ANTHROPIC_BASE_URL at
    // `{proxy}/claude-science` so the proxy routes `/claude-science/v1/messages`
    // to the claude-science provider namespace and failover queue (the CLI's
    // own config lives in an encrypted SQLite, so there is no live config
    // file for cc-switch to write).
    //
    // When the daemon runs inside WSL2, the Windows loopback address returned
    // by the proxy service is not reachable from WSL. In that case we rewrite
    // the URL host to the Windows host IP visible from WSL (the default
    // gateway in the WSL namespace). The proxy itself must be configured to
    // listen on an interface reachable from WSL (e.g., 0.0.0.0).
    let mut base_url = proxy_base_url.trim_end_matches('/').to_string();
    if let Some(host) = host_override {
        base_url = replace_url_host(&base_url, host)
            .trim_end_matches('/')
            .to_string();
    }
    let science_base_url = format!("{}/claude-science", base_url);
    [
        ("ANTHROPIC_BASE_URL", science_base_url),
        ("ANTHROPIC_AUTH_TOKEN", PROXY_TOKEN_PLACEHOLDER.to_string()),
        ("ANTHROPIC_API_KEY", PROXY_TOKEN_PLACEHOLDER.to_string()),
    ]
}

fn replace_url_host(url_str: &str, new_host: &str) -> String {
    let mut url = match url::Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return url_str.to_string(),
    };
    if url.set_host(Some(new_host)).is_err() {
        return url_str.to_string();
    }
    url.to_string()
}

/// Resolve the Windows host IP that is reachable from a WSL2 distro.
/// This is the default gateway in the WSL namespace.
#[cfg(target_os = "windows")]
fn wsl_host_ip(distro: &str) -> Option<String> {
    if !crate::commands::misc::is_valid_wsl_distro_name(distro) {
        return None;
    }

    let output = run_wsl_bash_script(distro, "ip route show default | awk '{print $3}'").ok()?;

    if !output.status.success() {
        return None;
    }

    let ip = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();

    if ip.is_empty() {
        return None;
    }

    Some(ip)
}

#[cfg(not(target_os = "windows"))]
fn wsl_host_ip(_distro: &str) -> Option<String> {
    None
}

fn run_cli_with_env<K, V>(
    bin: &Path,
    args: &[&str],
    envs: &[(K, V)],
    profile: Option<&ScienceProfilePaths>,
) -> Result<Output, String>
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    let mut command = Command::new(bin);
    command
        .args(args)
        .envs(envs.iter().map(|(k, v)| (k.as_ref(), v.as_ref())))
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
            // Must NOT be the data dir: since 0.1.25, sandboxed job spawns
            // (pip/env provisioning) are seatbelt-denied read access inside
            // the daemon's own data dir ("daemon territory"). Those spawns
            // inherit the daemon's cwd, and pip crashes at os.getcwd() when
            // it lands in denied territory — surfacing as "1 environment
            // failed". Launching from the user's home keeps the inherited
            // cwd readable, matching how the main profile is normally run.
            .current_dir(crate::config::get_home_dir());
    }

    command
        .output()
        .map_err(|e| format!("Failed to execute Claude Science CLI: {e}"))
}

fn run_science_cli(
    runtime: &ScienceRuntime,
    args: &[&str],
    envs: &[(&str, &str)],
    profile: Option<&ScienceProfilePaths>,
) -> Result<Output, String> {
    run_science_cli_with_env(runtime, args, envs, profile)
}

fn run_science_cli_with_env<K, V>(
    runtime: &ScienceRuntime,
    args: &[&str],
    envs: &[(K, V)],
    profile: Option<&ScienceProfilePaths>,
) -> Result<Output, String>
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    if let Some(distro) = &runtime.wsl_distro {
        #[cfg(target_os = "windows")]
        {
            let cmd = build_wsl_shell_command(runtime, args, envs, profile);
            let mut command = Command::new("wsl.exe");
            command
                .args(["-d", distro, "--", "sh", "-c", &cmd])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .creation_flags(CREATE_NO_WINDOW);
            return command
                .output()
                .map_err(|e| format!("Failed to execute Claude Science CLI in WSL: {e}"));
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (args, envs, profile);
            return Err("Claude Science WSL launch is only supported on Windows".to_string());
        }
    }

    run_cli_with_env(&runtime.bin_path, args, envs, profile)
}

/// Build the shell command executed inside WSL.
#[cfg(target_os = "windows")]
fn build_wsl_shell_command<K, V>(
    runtime: &ScienceRuntime,
    args: &[&str],
    envs: &[(K, V)],
    profile: Option<&ScienceProfilePaths>,
) -> String
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    let mut cmd = String::from("cd ~");

    if !SCIENCE_MODEL_ENV_KEYS_TO_CLEAR.is_empty() {
        cmd.push_str(" && unset");
        for key in SCIENCE_MODEL_ENV_KEYS_TO_CLEAR {
            cmd.push(' ');
            cmd.push_str(key);
        }
    }

    for (k, v) in envs {
        cmd.push_str(" && export ");
        cmd.push_str(&k.as_ref().to_string_lossy());
        cmd.push('=');
        cmd.push_str(&shell_quote(&v.as_ref().to_string_lossy()));
    }

    cmd.push_str(" && ");
    cmd.push_str(&shell_quote(&runtime.bin_display));

    for arg in args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(arg));
    }

    if let Some(profile) = profile {
        cmd.push_str(" --data-dir ");
        cmd.push_str(&shell_quote(
            &wsl_path_to_linux(&profile.data_dir)
                .unwrap_or_else(|| profile.data_dir.to_string_lossy().to_string()),
        ));
        cmd.push_str(" --config ");
        cmd.push_str(&shell_quote(
            &wsl_path_to_linux(&profile.config_path)
                .unwrap_or_else(|| profile.config_path.to_string_lossy().to_string()),
        ));
    }

    cmd
}

/// Escape a value for safe use inside single quotes in a POSIX shell.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Convert a Windows UNC WSL path (`\\wsl$\Distro\home\user\.claude-science`)
/// to the Linux path seen inside WSL (`/home/user/.claude-science`).
fn wsl_path_to_linux(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    let prefix = components.next()?;
    let (server, _share) = match prefix {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                (server.to_string_lossy(), share.to_string_lossy())
            }
            _ => return None,
        },
        _ => return None,
    };

    if !server.eq_ignore_ascii_case("wsl$") && !server.eq_ignore_ascii_case("wsl.localhost") {
        return None;
    }

    let parts: Vec<String> = components
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();

    if parts.is_empty() {
        return None;
    }

    Some(format!("/{}", parts.join("/")))
}

fn managed_profile_paths_for_science_home(science_home: &Path) -> ScienceProfilePaths {
    let data_dir = science_home.join(MANAGED_PROFILE_DIR);
    let auth_dir = data_dir.clone();
    let config_path = data_dir.join("config.toml");

    ScienceProfilePaths {
        data_dir,
        auth_dir,
        config_path,
    }
}

fn legacy_managed_profile_dir() -> PathBuf {
    crate::config::get_app_config_dir().join(MANAGED_PROFILE_DIR)
}

/// Pre-relocation builds kept the managed profile under the cc-switch config
/// dir (`~/.cc-switch/claude-science-proxy`). Since Claude Science 0.1.25,
/// sandboxed job spawns (pip/env provisioning, MCP servers) are seatbelt-
/// restricted to Claude Science's own home (`~/.claude-science`); a data dir
/// outside that tree gets `deny(1) file-read-data`, which crashes pip at
/// `os.getcwd()` and surfaces as "1 environment failed". Move the profile
/// into Science's home, best-effort; on failure a fresh profile is created.
fn migrate_legacy_profile(profile: &ScienceProfilePaths) {
    migrate_legacy_profile_dir(&legacy_managed_profile_dir(), &profile.data_dir);
}

fn migrate_legacy_profile_dir(legacy: &Path, target: &Path) {
    if legacy == target || !legacy.exists() || target.exists() {
        return;
    }
    if let Some(parent) = target.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let _ = fs::rename(legacy, target);
}

fn prepare_managed_profile_at(runtime: &ScienceRuntime) -> Result<ScienceProfilePaths, String> {
    let profile = runtime.managed_profile_paths();
    migrate_legacy_profile(&profile);
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
    write_active_org(&profile.auth_dir)?;

    Ok(())
}

fn write_science_config(profile: &ScienceProfilePaths) -> Result<(), String> {
    // When the profile lives on a WSL UNC path, the daemon inside WSL needs a
    // Linux path rather than the Windows UNC representation.
    let auth_dir = wsl_path_to_linux(&profile.auth_dir)
        .unwrap_or_else(|| profile.auth_dir.to_string_lossy().to_string());
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

    let token_path = token_dir.join(format!("{SCIENCE_PROXY_USER_ID}.enc"));
    remove_stale_oauth_tokens(&token_dir, &token_path)?;

    let token = ScienceOAuthToken {
        access_token: PROXY_TOKEN_PLACEHOLDER,
        refresh_token: Some(String::new()),
        api_key: None,
        token_expires_at: OAUTH_TOKEN_EXPIRY.to_string(),
        // Deliberately NOT "claude_ai": since 0.1.25 the daemon uses the stored
        // OAuth session for claude.ai directory connectors, and when
        // provider == "claude_ai" it classifies the (expected) rejection of our
        // placeholder token as "session expired", surfacing an alarming
        // "Your claude.ai session has expired" banner. With any other provider
        // value the connector bootstrap degrades to a quiet "not signed in"
        // state (`no_org`) that the web UI does not banner on, while
        // inference keeps flowing through ANTHROPIC_BASE_URL unchanged.
        provider: "proxy_managed",
        scopes: "user:inference user:file_upload user:profile user:mcp_servers user:plugins",
        email: SCIENCE_PROXY_EMAIL,
        account_uuid: SCIENCE_PROXY_USER_ID,
        org_uuid: Some(SCIENCE_PROXY_ORG_ID.to_string()),
        org_name: None,
        subscription_type: "max",
        rate_limit_tier: None,
        seat_tier: None,
        allow_safety_feedback: false,
        billing_type: None,
        has_extra_usage_enabled: Some(false),
        tier_unmappable: false,
        billing_resolved: true,
    };
    let plaintext = serde_json::to_vec(&token)
        .map_err(|e| format!("Failed to serialize Claude Science OAuth token: {e}"))?;
    let encrypted = encrypt_oauth_payload(oauth_key, &plaintext)?;
    write_private_file(&token_path, encrypted.as_bytes())
}

fn remove_stale_oauth_tokens(token_dir: &Path, keep: &Path) -> Result<(), String> {
    let entries = fs::read_dir(token_dir).map_err(|e| {
        format!(
            "Failed to inspect Claude Science OAuth token dir {}: {e}",
            token_dir.display()
        )
    })?;

    for entry in entries {
        let path = entry
            .map_err(|e| format!("Failed to inspect Claude Science OAuth token entry: {e}"))?
            .path();
        if path == keep || path.extension().and_then(|ext| ext.to_str()) != Some("enc") {
            continue;
        }
        fs::remove_file(&path).map_err(|e| {
            format!(
                "Failed to remove stale Claude Science OAuth token {}: {e}",
                path.display()
            )
        })?;
    }

    Ok(())
}

fn write_active_org(auth_dir: &Path) -> Result<(), String> {
    let active_org = ScienceActiveOrg {
        org_uuid: SCIENCE_PROXY_ORG_ID,
    };
    let content = serde_json::to_vec_pretty(&active_org)
        .map_err(|e| format!("Failed to serialize Claude Science active organization: {e}"))?;
    write_private_file(&auth_dir.join(ACTIVE_ORG_FILENAME), &content)
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

/// Resolve the runtime used to launch/status/stop Claude Science.
///
/// Windows resolution order:
/// 1. Explicit WSL override: config dir points at a WSL UNC path.
/// 2. Native host binary.
/// 3. Auto-detect: scan registered WSL distros (default first) for the
///    claude-science binary and derive the data dir from the distro's $HOME.
///
/// Other platforms use the native binary search only.
fn find_science_runtime() -> Result<ScienceRuntime, String> {
    #[cfg(target_os = "windows")]
    {
        if let Some(distro) = crate::commands::misc::wsl_distro_for_science() {
            return find_wsl_override_runtime(&distro);
        }

        let candidates = native_binary_candidates();
        if let Some(path) = find_first_executable(candidates.clone()) {
            return Ok(ScienceRuntime {
                bin_display: path.display().to_string(),
                bin_path: path,
                wsl_distro: None,
                config_dir: crate::config::get_claude_science_config_dir(),
                wsl_version: None,
            });
        }

        if let Some(runtime) = detect_wsl_science_runtime() {
            return Ok(runtime);
        }

        let searched = candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let wsl_hint = match registered_wsl_distros() {
            Some(distros) if !distros.is_empty() => format!(
                "Registered WSL distros: {} (no claude-science binary found inside them).",
                distros.join(", ")
            ),
            _ => "No WSL distro with claude-science was detected.".to_string(),
        };
        return Err(format!(
            "Claude Science CLI was not found. Searched:\n{searched}\n\n{wsl_hint}\n\
             Install claude-science, point the config directory at a WSL path, or set the \
             {CLAUDE_SCIENCE_BIN_ENV} environment variable."
        ));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let candidates = native_binary_candidates();
        find_first_executable(candidates.clone())
            .map(|path| ScienceRuntime {
                bin_display: path.display().to_string(),
                bin_path: path,
                wsl_distro: None,
                config_dir: crate::config::get_claude_science_config_dir(),
                wsl_version: None,
            })
            .ok_or_else(|| {
                let searched = candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "Claude Science CLI was not found. Searched:\n{searched}\n\n\
                     Install claude-science or set the {CLAUDE_SCIENCE_BIN_ENV} environment variable."
                )
            })
    }
}

fn native_binary_candidates() -> Vec<PathBuf> {
    claude_science_binary_candidates(
        std::env::var(CLAUDE_SCIENCE_BIN_ENV).ok(),
        Some(crate::config::get_home_dir()),
        std::env::var_os("PATH"),
    )
}

/// Runtime for an explicitly configured WSL config directory.
#[cfg(target_os = "windows")]
fn find_wsl_override_runtime(distro: &str) -> Result<ScienceRuntime, String> {
    // Try the actual binary search first; if it succeeds we don't care about
    // any list/probe quirks.
    match find_claude_science_binary_wsl(distro) {
        Ok(Some(cmd)) => {
            return Ok(ScienceRuntime {
                bin_display: cmd.clone(),
                bin_path: PathBuf::from(&cmd),
                wsl_distro: Some(distro.to_string()),
                config_dir: crate::config::get_claude_science_config_dir(),
                wsl_version: wsl_version_for(distro),
            });
        }
        Ok(None) => {}
        Err(err) => return Err(err),
    }

    if !wsl_distro_exists(distro) {
        let registered = registered_wsl_distros()
            .map(|list| list.join(", "))
            .unwrap_or_else(|| "could not enumerate".to_string());
        return Err(format!(
            "WSL distro '{distro}' does not exist. Registered distros: {registered}. \
             Update the Claude Science config directory to point to an existing distro, \
             e.g. \\\\wsl$\\<distro>\\home\\<user>\\.claude-science."
        ));
    }

    Err(format!(
        "Claude Science CLI was not found in WSL distro '{distro}'. \
         Searched: $PATH, $HOME/.claude-science/bin/claude-science, \
         $HOME/.local/bin/claude-science. \
         Install it inside WSL or set the {CLAUDE_SCIENCE_BIN_ENV} environment variable."
    ))
}

/// Auto-detect a WSL distro that has claude-science installed and build a
/// runtime whose data dir lives in that distro's home.
#[cfg(target_os = "windows")]
fn detect_wsl_science_runtime() -> Option<ScienceRuntime> {
    let mut distros = registered_wsl_distros_verbose()?;
    distros.sort_by_key(|d| !d.is_default);

    for distro in distros {
        let cmd = match find_claude_science_binary_wsl(&distro.name) {
            Ok(Some(cmd)) => cmd,
            _ => continue,
        };
        let home = match wsl_user_home(&distro.name) {
            Some(home) if home.starts_with('/') => home,
            _ => continue,
        };
        return Some(ScienceRuntime {
            bin_display: cmd.clone(),
            bin_path: PathBuf::from(&cmd),
            wsl_distro: Some(distro.name.clone()),
            config_dir: wsl_unc_science_home(&distro.name, &home),
            wsl_version: Some(distro.version),
        });
    }

    None
}

/// Query the default user's $HOME inside a WSL distro.
#[cfg(target_os = "windows")]
fn wsl_user_home(distro: &str) -> Option<String> {
    let output = run_wsl_bash_script(distro, r#"printf '%s' "$HOME""#).ok()?;
    if !output.status.success() {
        return None;
    }
    let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if home.is_empty() {
        None
    } else {
        Some(home)
    }
}

/// Build the Windows UNC path for the Claude Science home inside a distro,
/// e.g. `\\wsl$\Ubuntu-24.04\home\ciao\.claude-science`.
#[cfg(target_os = "windows")]
fn wsl_unc_science_home(distro: &str, linux_home: &str) -> PathBuf {
    let windows_home = linux_home.trim_end_matches('/').replace('/', "\\");
    PathBuf::from(format!(
        "\\\\wsl$\\{distro}{windows_home}\\.claude-science"
    ))
}

/// Run a bash script inside a WSL distro. The script is base64-encoded before
/// being handed to wsl.exe so quoting/variable-expansion quirks of the
/// Windows -> WSL command line cannot corrupt it.
#[cfg(target_os = "windows")]
fn run_wsl_bash_script(distro: &str, script: &str) -> Result<Output, String> {
    let encoded_script = BASE64_STANDARD.encode(script.as_bytes());
    let wrapped = format!("echo {encoded_script} | base64 -d | bash");
    Command::new("wsl.exe")
        .args(["-d", distro, "--", "sh", "-c", &wrapped])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to run wsl.exe: {e}"))
}

/// Locate the claude-science executable inside a WSL distro.
#[cfg(target_os = "windows")]
fn find_claude_science_binary_wsl(distro: &str) -> Result<Option<String>, String> {
    if !crate::commands::misc::is_valid_wsl_distro_name(distro) {
        return Err(format!("Invalid WSL distro name: '{distro}'"));
    }

    // Prepend the standard user-level bin directories to PATH before using
    // command -v, so binaries installed outside the login PATH are still found.
    let script = r#"PATH="$HOME/.claude-science/bin:$HOME/.local/bin:$PATH"; for p in "$(command -v claude-science 2>/dev/null)" "$HOME/.claude-science/bin/claude-science" "$HOME/.local/bin/claude-science"; do if [ -n "$p" ] && [ -x "$p" ]; then printf '%s\n' "$p"; break; fi; done"#;

    let output = run_wsl_bash_script(distro, script)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit status {}", output.status)
        };
        return Err(format!(
            "WSL command failed in distro '{distro}': {detail}"
        ));
    }

    let line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    if line.is_empty() {
        return Ok(None);
    }

    Ok(Some(line))
}

#[cfg(not(target_os = "windows"))]
fn find_claude_science_binary_wsl(_distro: &str) -> Result<Option<String>, String> {
    Ok(None)
}

/// A registered WSL distro with its version and default flag.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct WslDistroInfo {
    name: String,
    version: u8,
    is_default: bool,
}

/// Parse `wsl.exe --list --verbose` output. Lines look like
/// `* Ubuntu-24.04 Running 2` (localized header is skipped automatically
/// because its last token does not parse as a version number).
#[cfg(target_os = "windows")]
fn parse_wsl_verbose_output(text: &str) -> Vec<WslDistroInfo> {
    let mut distros = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (is_default, rest) = match line.strip_prefix('*') {
            Some(rest) => (true, rest.trim()),
            None => (false, line),
        };
        let mut tokens: Vec<&str> = rest.split_whitespace().collect();
        let Some(version_str) = tokens.last().copied() else {
            continue;
        };
        let Ok(version) = version_str.parse::<u8>() else {
            continue;
        };
        tokens.pop();
        if tokens.is_empty() {
            continue;
        }
        // Distro names cannot contain whitespace, so the first token is the
        // name; anything between the name and the version is the state column.
        let name = tokens[0];
        distros.push(WslDistroInfo {
            name: name.to_string(),
            version,
            is_default,
        });
    }
    distros
}

/// List registered WSL distros with version info (Windows only).
#[cfg(target_os = "windows")]
fn registered_wsl_distros_verbose() -> Option<Vec<WslDistroInfo>> {
    let output = Command::new("wsl.exe")
        .args(["--list", "--verbose"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = decode_wsl_text_output(&output.stdout);
    let distros = parse_wsl_verbose_output(&text);
    if distros.is_empty() {
        None
    } else {
        Some(distros)
    }
}

#[cfg(not(target_os = "windows"))]
fn registered_wsl_distros_verbose() -> Option<Vec<WslDistroInfo>> {
    None
}

/// Non-Windows placeholder type so the verbose stub compiles.
#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone)]
struct WslDistroInfo {
    name: String,
    version: u8,
    is_default: bool,
}

/// List registered WSL distro names (Windows only).
#[cfg(target_os = "windows")]
fn registered_wsl_distros() -> Option<Vec<String>> {
    registered_wsl_distros_verbose()
        .map(|list| list.into_iter().map(|distro| distro.name).collect())
}

#[cfg(not(target_os = "windows"))]
fn registered_wsl_distros() -> Option<Vec<String>> {
    None
}

/// Look up the WSL version (1 or 2) for a distro name.
#[cfg(target_os = "windows")]
fn wsl_version_for(distro: &str) -> Option<u8> {
    registered_wsl_distros_verbose().and_then(|list| {
        list.into_iter()
            .find(|d| d.name.eq_ignore_ascii_case(distro))
            .map(|d| d.version)
    })
}

/// Decode output from wsl.exe. It often returns UTF-16LE when stdout is
/// redirected — with or without a BOM — so detect the BOM first and otherwise
/// fall back to a NUL-ratio heuristic before assuming UTF-8.
#[cfg(target_os = "windows")]
fn decode_wsl_text_output(bytes: &[u8]) -> String {
    let bom = bytes.starts_with(&[0xFF, 0xFE]);
    let body = if bom { &bytes[2..] } else { bytes };

    // ASCII text encoded as UTF-16LE has a NUL in every second byte (~50%).
    // A >=25% NUL ratio is a safe signal that this is not UTF-8 text.
    let nul_count = body.iter().filter(|&&b| b == 0).count();
    let looks_utf16le = bom || (body.len() >= 4 && nul_count * 4 >= body.len());

    if looks_utf16le {
        let u16s: Vec<u16> = body
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&u16s)
            .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());
    }

    String::from_utf8_lossy(bytes).to_string()
}

#[cfg(not(target_os = "windows"))]
fn decode_wsl_text_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

/// Check whether a WSL distro is registered (Windows only).
/// This uses a direct `wsl.exe -d <distro> -- echo ok` probe instead of parsing
/// `wsl.exe --list`, so it is immune to list-output encoding quirks.
#[cfg(target_os = "windows")]
fn wsl_distro_exists(distro: &str) -> bool {
    if !crate::commands::misc::is_valid_wsl_distro_name(distro) {
        return false;
    }

    Command::new("wsl.exe")
        .args(["-d", distro, "--", "echo", "ok"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn wsl_distro_exists(_distro: &str) -> bool {
    false
}

#[allow(dead_code)]
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
    fn managed_profile_paths_stay_under_claude_science_home() {
        let root = PathBuf::from("/tmp/claude-science-test-home");
        let paths = managed_profile_paths_for_science_home(&root);

        assert_eq!(paths.data_dir, root.join(MANAGED_PROFILE_DIR));
        assert_eq!(paths.auth_dir, root.join(MANAGED_PROFILE_DIR));
        assert_eq!(
            paths.config_path,
            root.join(MANAGED_PROFILE_DIR).join("config.toml")
        );
    }

    #[test]
    fn migrate_legacy_profile_moves_dir_when_target_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let legacy = tmp.path().join("cc-switch").join(MANAGED_PROFILE_DIR);
        let target = tmp.path().join("science-home").join(MANAGED_PROFILE_DIR);
        fs::create_dir_all(&legacy).expect("legacy dir");
        fs::write(legacy.join("marker.txt"), b"state").expect("legacy marker");

        migrate_legacy_profile_dir(&legacy, &target);

        assert!(!legacy.exists());
        assert_eq!(
            fs::read(target.join("marker.txt")).expect("migrated marker"),
            b"state"
        );
    }

    #[test]
    fn migrate_legacy_profile_keeps_existing_target() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let legacy = tmp.path().join("cc-switch").join(MANAGED_PROFILE_DIR);
        let target = tmp.path().join("science-home").join(MANAGED_PROFILE_DIR);
        fs::create_dir_all(&legacy).expect("legacy dir");
        fs::create_dir_all(&target).expect("target dir");
        fs::write(target.join("marker.txt"), b"current").expect("target marker");

        migrate_legacy_profile_dir(&legacy, &target);

        assert!(legacy.exists());
        assert_eq!(
            fs::read(target.join("marker.txt")).expect("target marker"),
            b"current"
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
        let profile = managed_profile_paths_for_science_home(tmp.path());

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
                .join(format!("{SCIENCE_PROXY_USER_ID}.enc")),
        )
        .expect("encrypted oauth token");
        let plaintext = decrypt_oauth_payload_for_test(oauth_key, encrypted.trim());
        let token: Value = serde_json::from_slice(&plaintext).expect("token json");

        assert_eq!(token["access_token"], PROXY_TOKEN_PLACEHOLDER);
        assert_eq!(token["email"], SCIENCE_PROXY_EMAIL);
        assert_eq!(token["account_uuid"], SCIENCE_PROXY_USER_ID);
        assert_eq!(token["org_uuid"], SCIENCE_PROXY_ORG_ID);
        // Must stay a non-"claude_ai" value so 0.1.25+ treats the placeholder
        // session as "not signed in" instead of "session expired" (see the
        // comment at the construction site).
        assert_eq!(token["provider"], "proxy_managed");
        assert_eq!(token["subscription_type"], "max");
        assert_eq!(token["token_expires_at"], OAUTH_TOKEN_EXPIRY);
        let scopes = token["scopes"].as_str().expect("scopes");
        assert!(scopes.contains("user:inference"));
        assert!(scopes.contains("user:file_upload"));
        assert!(scopes.contains("user:mcp_servers"));
        assert!(scopes.contains("user:plugins"));

        let active_org: Value = serde_json::from_slice(
            &fs::read(profile.auth_dir.join(ACTIVE_ORG_FILENAME)).expect("active org file"),
        )
        .expect("active org json");
        assert_eq!(active_org["org_uuid"], SCIENCE_PROXY_ORG_ID);
    }

    #[test]
    fn prepare_profile_removes_stale_oauth_tokens() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profile = managed_profile_paths_for_science_home(tmp.path());
        let token_dir = profile.auth_dir.join(OAUTH_TOKEN_DIR);
        fs::create_dir_all(&token_dir).expect("token dir");
        fs::write(token_dir.join("stale.enc"), "stale").expect("stale token");
        fs::write(token_dir.join("keep.txt"), "keep").expect("non-token file");

        prepare_profile_at(&profile).expect("prepare profile");

        assert!(!token_dir.join("stale.enc").exists());
        assert!(token_dir.join("keep.txt").exists());
        assert!(token_dir
            .join(format!("{SCIENCE_PROXY_USER_ID}.enc"))
            .exists());
    }

    #[test]
    fn proxy_launch_env_points_science_at_local_proxy() {
        let env = proxy_launch_env("http://127.0.0.1:15721");

        assert_eq!(
            env[0],
            (
                "ANTHROPIC_BASE_URL",
                "http://127.0.0.1:15721/claude-science".to_string()
            )
        );
        assert_eq!(
            env[1],
            ("ANTHROPIC_AUTH_TOKEN", PROXY_TOKEN_PLACEHOLDER.to_string())
        );
        assert_eq!(
            env[2],
            ("ANTHROPIC_API_KEY", PROXY_TOKEN_PLACEHOLDER.to_string())
        );
    }

    #[test]
    fn proxy_launch_env_trims_trailing_slash_before_namespace() {
        let env = proxy_launch_env("http://127.0.0.1:15721/");

        assert_eq!(
            env[0],
            (
                "ANTHROPIC_BASE_URL",
                "http://127.0.0.1:15721/claude-science".to_string()
            )
        );
    }

    #[test]
    fn proxy_launch_env_with_host_override_rewrites_loopback_for_wsl() {
        let env = proxy_launch_env_with_host("http://127.0.0.1:15721", Some("172.24.128.1"));

        assert_eq!(
            env[0],
            (
                "ANTHROPIC_BASE_URL",
                "http://172.24.128.1:15721/claude-science".to_string()
            )
        );
    }

    #[test]
    fn replace_url_host_swaps_host_and_preserves_port_and_path() {
        assert_eq!(
            replace_url_host("http://127.0.0.1:15721", "172.24.128.1"),
            "http://172.24.128.1:15721/"
        );
        assert_eq!(
            replace_url_host("http://localhost:15721/foo", "172.24.128.1"),
            "http://172.24.128.1:15721/foo"
        );
        assert_eq!(replace_url_host("not-a-url", "172.24.128.1"), "not-a-url");
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
        let profile = managed_profile_paths_for_science_home(&tmp.path().join("cc-switch"));
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
        assert!(stdout.contains("base=http://127.0.0.1:15721/claude-science\n"));
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

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("hello"), "'hello'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_path_to_linux_converts_unc_paths() {
        assert_eq!(
            wsl_path_to_linux(&PathBuf::from(r"\\wsl$\Ubuntu\home\alice\.claude-science")),
            Some("/home/alice/.claude-science".to_string())
        );
        assert_eq!(
            wsl_path_to_linux(&PathBuf::from(
                r"\\wsl.localhost\Ubuntu\root\.claude-science"
            )),
            Some("/root/.claude-science".to_string())
        );
        assert_eq!(
            wsl_path_to_linux(&PathBuf::from(r"C:\Users\alice\.claude-science")),
            None
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn build_wsl_shell_command_includes_env_profile_and_unset() {
        let runtime = ScienceRuntime {
            bin_display: "claude-science".to_string(),
            bin_path: PathBuf::from("claude-science"),
            wsl_distro: Some("Ubuntu".to_string()),
            config_dir: PathBuf::from(r"\\wsl$\Ubuntu\home\alice\.claude-science"),
            wsl_version: Some(2),
        };
        let profile = managed_profile_paths_for_science_home(&PathBuf::from(
            r"\\wsl$\Ubuntu\home\alice\.claude-science",
        ));
        let envs = proxy_launch_env("http://127.0.0.1:15721");

        let cmd = build_wsl_shell_command(&runtime, &["status"], &envs, Some(&profile));

        assert!(cmd.starts_with("cd ~"), "{cmd}");
        assert!(cmd.contains(" && unset ANTHROPIC_MODEL"), "{cmd}");
        assert!(
            cmd.contains("export ANTHROPIC_BASE_URL='http://127.0.0.1:15721/claude-science'"),
            "{cmd}"
        );
        assert!(
            cmd.contains("export ANTHROPIC_AUTH_TOKEN='PROXY_MANAGED'"),
            "{cmd}"
        );
        assert!(
            cmd.contains("export ANTHROPIC_API_KEY='PROXY_MANAGED'"),
            "{cmd}"
        );
        assert!(cmd.contains(" && 'claude-science' 'status'"), "{cmd}");
        assert!(
            cmd.contains("--data-dir '/home/alice/.claude-science/claude-science-proxy'"),
            "{cmd}"
        );
        assert!(
            cmd.contains("--config '/home/alice/.claude-science/claude-science-proxy/config.toml'"),
            "{cmd}"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_unc_science_home_builds_unc_path() {
        assert_eq!(
            wsl_unc_science_home("Ubuntu-24.04", "/home/ciao"),
            PathBuf::from(r"\\wsl$\Ubuntu-24.04\home\ciao\.claude-science")
        );
        assert_eq!(
            wsl_unc_science_home("Ubuntu-24.04", "/root"),
            PathBuf::from(r"\\wsl$\Ubuntu-24.04\root\.claude-science")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_unc_science_home_roundtrips_through_linux_conversion() {
        let unc = wsl_unc_science_home("Ubuntu-24.04", "/home/ciao");
        assert_eq!(
            wsl_path_to_linux(&unc),
            Some("/home/ciao/.claude-science".to_string())
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_wsl_verbose_output_reads_name_version_and_default() {
        let text = "  NAME            STATE           VERSION\r\n* Ubuntu-24.04    Running         2\r\n  Debian          Stopped         1\r\n";
        let distros = parse_wsl_verbose_output(text);

        assert_eq!(distros.len(), 2);
        assert_eq!(distros[0].name, "Ubuntu-24.04");
        assert_eq!(distros[0].version, 2);
        assert!(distros[0].is_default);
        assert_eq!(distros[1].name, "Debian");
        assert_eq!(distros[1].version, 1);
        assert!(!distros[1].is_default);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_wsl_verbose_output_skips_localized_header() {
        let text = "  名称            状态            版本\n  Ubuntu-24.04    正在运行        2\n";
        let distros = parse_wsl_verbose_output(text);

        assert_eq!(distros.len(), 1);
        assert_eq!(distros[0].name, "Ubuntu-24.04");
        assert_eq!(distros[0].version, 2);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decode_wsl_text_output_handles_utf16le_with_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("Ubuntu-24.04".encode_utf16().flat_map(|u| u.to_le_bytes()));
        assert_eq!(decode_wsl_text_output(&bytes), "Ubuntu-24.04");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decode_wsl_text_output_handles_utf16le_without_bom() {
        let bytes: Vec<u8> = "Ubuntu-24.04\r\n"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(decode_wsl_text_output(&bytes), "Ubuntu-24.04\r\n");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decode_wsl_text_output_passes_utf8_through() {
        let bytes = "适用于 Linux 的 Windows 子系统".as_bytes();
        assert_eq!(
            decode_wsl_text_output(bytes),
            "适用于 Linux 的 Windows 子系统"
        );
    }
}
