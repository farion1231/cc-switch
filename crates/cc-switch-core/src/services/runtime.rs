use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const APP_NAME: &str = "CC Switch";
const TOOL_NAMES: [&str; 4] = ["claude", "codex", "gemini", "opencode"];
const REPOSITORY_URL: &str = env!("CARGO_PKG_REPOSITORY");
const ONE_CLICK_INSTALL_COMMANDS: &str = "# Claude Code (Native install - recommended)\n\
curl -fsSL https://claude.ai/install.sh | bash\n\
# Codex\n\
npm i -g @openai/codex@latest\n\
# Gemini CLI\n\
npm i -g @google/gemini-cli@latest\n\
# OpenCode\n\
curl -fsSL https://opencode.ai/install | bash";
const TEST_CURRENT_EXE_ENV: &str = "CC_SWITCH_TEST_CURRENT_EXE";
const TEST_GITHUB_API_BASE_URL_ENV: &str = "CC_SWITCH_TEST_GITHUB_API_BASE_URL";
const TEST_NPM_REGISTRY_BASE_URL_ENV: &str = "CC_SWITCH_TEST_NPM_REGISTRY_BASE_URL";

static VERSION_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\d+\.\d+\.\d+(-[\w.]+)?").expect("valid version regex"));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub repository: String,
    pub releases_url: String,
    pub current_release_notes_url: String,
    pub latest_release_url: String,
    pub portable_mode: bool,
    pub install_commands: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolVersionInfo {
    pub name: String,
    pub version: Option<String>,
    pub latest_version: Option<String>,
    pub error: Option<String>,
    pub env_type: String,
    pub wsl_distro: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslShellPreference {
    pub wsl_shell: Option<String>,
    pub wsl_shell_flag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub has_update: bool,
    pub releases_url: String,
    pub release_notes_url: String,
    pub latest_release_url: String,
    pub error: Option<String>,
}

pub struct RuntimeService;

impl RuntimeService {
    pub fn about() -> Result<AppInfo, AppError> {
        Ok(AppInfo {
            name: APP_NAME.to_string(),
            version: current_version().to_string(),
            repository: REPOSITORY_URL.to_string(),
            releases_url: releases_url(),
            current_release_notes_url: release_notes_url(Some(current_version())),
            latest_release_url: latest_release_url(),
            portable_mode: Self::is_portable_mode()?,
            install_commands: ONE_CLICK_INSTALL_COMMANDS.to_string(),
        })
    }

    pub fn is_portable_mode() -> Result<bool, AppError> {
        let exe_path = current_exe_path()?;
        Ok(exe_path
            .parent()
            .is_some_and(|dir| dir.join("portable.ini").is_file()))
    }

    pub async fn get_tool_versions(
        tools: Option<Vec<String>>,
        wsl_shell_by_tool: Option<HashMap<String, WslShellPreference>>,
        include_latest: bool,
    ) -> Result<Vec<ToolVersionInfo>, AppError> {
        #[cfg(target_os = "windows")]
        {
            let _ = (tools, wsl_shell_by_tool, include_latest);
            return Ok(Vec::new());
        }

        #[cfg(not(target_os = "windows"))]
        {
            let requested = normalize_requested_tools(tools)?;
            let client = http_client()?;
            let mut results = Vec::new();

            for tool in requested {
                let pref = wsl_shell_by_tool.as_ref().and_then(|value| value.get(tool));
                let tool_wsl_shell = pref.and_then(|value| value.wsl_shell.as_deref());
                let tool_wsl_shell_flag = pref.and_then(|value| value.wsl_shell_flag.as_deref());

                results.push(
                    get_single_tool_version(tool, tool_wsl_shell, tool_wsl_shell_flag, include_latest, &client)
                        .await,
                );
            }

            Ok(results)
        }
    }

    pub async fn check_for_updates() -> Result<UpdateInfo, AppError> {
        let client = http_client()?;
        let current = current_version().to_string();
        let latest = fetch_latest_release_version(&client).await;
        let error = latest.as_ref().err().map(|value| value.to_string());
        let latest_version = latest.ok().flatten();
        let has_update = latest_version
            .as_deref()
            .is_some_and(|value| normalize_version_tag(value) != normalize_version_tag(&current));
        let release_notes_url = latest_version
            .as_deref()
            .map(|value| release_notes_url(Some(value)))
            .unwrap_or_else(|| release_notes_url(Some(&current)));

        Ok(UpdateInfo {
            current_version: current,
            latest_version,
            has_update,
            releases_url: releases_url(),
            release_notes_url,
            latest_release_url: latest_release_url(),
            error,
        })
    }
}

fn current_exe_path() -> Result<PathBuf, AppError> {
    if let Some(path) = std::env::var_os(TEST_CURRENT_EXE_ENV) {
        return Ok(PathBuf::from(path));
    }

    std::env::current_exe().map_err(|e| AppError::Message(format!("获取可执行路径失败: {e}")))
}

fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn releases_url() -> String {
    format!("{REPOSITORY_URL}/releases")
}

fn latest_release_url() -> String {
    format!("{}/latest", releases_url())
}

fn release_notes_url(version: Option<&str>) -> String {
    match version {
        Some(value) => format!("{}/tag/v{}", releases_url(), normalize_version_tag(value)),
        None => releases_url(),
    }
}

fn normalize_requested_tools(tools: Option<Vec<String>>) -> Result<Vec<&'static str>, AppError> {
    let Some(tools) = tools else {
        return Ok(TOOL_NAMES.to_vec());
    };

    let valid: HashSet<&str> = TOOL_NAMES.into_iter().collect();
    let mut normalized = Vec::new();

    for item in tools {
        let candidate = item.trim().to_lowercase();
        if !valid.contains(candidate.as_str()) {
            return Err(AppError::InvalidInput(format!(
                "unsupported tool '{candidate}', expected one of: {}",
                TOOL_NAMES.join(", ")
            )));
        }

        if let Some(&tool) = TOOL_NAMES.iter().find(|value| **value == candidate) {
            if !normalized.contains(&tool) {
                normalized.push(tool);
            }
        }
    }

    Ok(normalized)
}

fn http_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("cc-switch")
        .build()
        .map_err(|e| AppError::Message(format!("创建 HTTP 客户端失败: {e}")))
}

async fn get_single_tool_version(
    tool: &str,
    wsl_shell: Option<&str>,
    wsl_shell_flag: Option<&str>,
    include_latest: bool,
    client: &reqwest::Client,
) -> ToolVersionInfo {
    let (env_type, wsl_distro) = tool_env_type_and_wsl_distro(tool);
    let (local_version, local_error) = if let Some(distro) = wsl_distro.as_deref() {
        try_get_version_wsl(tool, distro, wsl_shell, wsl_shell_flag)
    } else {
        let direct = try_get_version(tool);
        if direct.0.is_some() {
            direct
        } else {
            scan_cli_version(tool)
        }
    };

    let latest_version = if include_latest {
        fetch_latest_tool_version(client, tool).await
    } else {
        None
    };

    ToolVersionInfo {
        name: tool.to_string(),
        version: local_version,
        latest_version,
        error: local_error,
        env_type,
        wsl_distro,
    }
}

async fn fetch_latest_tool_version(client: &reqwest::Client, tool: &str) -> Option<String> {
    match tool {
        "claude" => fetch_npm_latest_version(client, "@anthropic-ai/claude-code").await,
        "codex" => fetch_npm_latest_version(client, "@openai/codex").await,
        "gemini" => fetch_npm_latest_version(client, "@google/gemini-cli").await,
        "opencode" => fetch_github_latest_version(client, "anomalyco/opencode").await,
        _ => None,
    }
}

async fn fetch_latest_release_version(client: &reqwest::Client) -> Result<Option<String>, AppError> {
    let url = github_api_url(&format!("/repos/{}/releases/latest", github_repo_slug()));
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::Message(format!("检查更新失败: {e}")))?;

    let json = response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| AppError::Message(format!("解析更新响应失败: {e}")))?;

    Ok(json
        .get("tag_name")
        .and_then(|value| value.as_str())
        .map(normalize_version_tag))
}

fn github_repo_slug() -> &'static str {
    REPOSITORY_URL
        .strip_prefix("https://github.com/")
        .unwrap_or("farion1231/cc-switch")
}

async fn fetch_npm_latest_version(client: &reqwest::Client, package: &str) -> Option<String> {
    let url = format!("{}/{}", npm_registry_base_url(), package);
    let response = client.get(url).send().await.ok()?;
    let json = response.json::<serde_json::Value>().await.ok()?;
    json.get("dist-tags")
        .and_then(|tags| tags.get("latest"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

async fn fetch_github_latest_version(client: &reqwest::Client, repo: &str) -> Option<String> {
    let url = github_api_url(&format!("/repos/{repo}/releases/latest"));
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    let json = response.json::<serde_json::Value>().await.ok()?;
    json.get("tag_name")
        .and_then(|value| value.as_str())
        .map(normalize_version_tag)
}

fn npm_registry_base_url() -> String {
    std::env::var(TEST_NPM_REGISTRY_BASE_URL_ENV)
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn github_api_url(path: &str) -> String {
    let base = std::env::var(TEST_GITHUB_API_BASE_URL_ENV)
        .unwrap_or_else(|_| "https://api.github.com".to_string());
    format!("{}{}", base.trim_end_matches('/'), path)
}

fn normalize_version_tag(value: &str) -> String {
    value.trim().trim_start_matches('v').to_string()
}

fn extract_version(raw: &str) -> String {
    VERSION_RE
        .find(raw)
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| raw.trim().to_string())
}

#[cfg(target_os = "windows")]
fn tool_env_type_and_wsl_distro(tool: &str) -> (String, Option<String>) {
    if let Some(distro) = wsl_distro_for_tool(tool) {
        ("wsl".to_string(), Some(distro))
    } else {
        ("windows".to_string(), None)
    }
}

#[cfg(target_os = "macos")]
fn tool_env_type_and_wsl_distro(_tool: &str) -> (String, Option<String>) {
    ("macos".to_string(), None)
}

#[cfg(target_os = "linux")]
fn tool_env_type_and_wsl_distro(_tool: &str) -> (String, Option<String>) {
    ("linux".to_string(), None)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn tool_env_type_and_wsl_distro(_tool: &str) -> (String, Option<String>) {
    ("unknown".to_string(), None)
}

fn try_get_version(tool: &str) -> (Option<String>, Option<String>) {
    use std::process::Command;

    #[cfg(target_os = "windows")]
    let output = Command::new("cmd")
        .args(["/C", &format!("{tool} --version")])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    #[cfg(not(target_os = "windows"))]
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("{tool} --version"))
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if out.status.success() {
                let raw = if stdout.is_empty() { &stderr } else { &stdout };
                if raw.is_empty() {
                    (None, Some("not installed or not executable".to_string()))
                } else {
                    (Some(extract_version(raw)), None)
                }
            } else {
                let error = if stderr.is_empty() { stdout } else { stderr };
                (
                    None,
                    Some(if error.is_empty() {
                        "not installed or not executable".to_string()
                    } else {
                        error
                    }),
                )
            }
        }
        Err(err) => (None, Some(err.to_string())),
    }
}

#[cfg(target_os = "windows")]
fn try_get_version_wsl(
    tool: &str,
    distro: &str,
    force_shell: Option<&str>,
    force_shell_flag: Option<&str>,
) -> (Option<String>, Option<String>) {
    use std::process::Command;

    if !is_valid_wsl_distro_name(distro) {
        return (None, Some(format!("[WSL:{distro}] invalid distro name")));
    }

    let (shell, flag, cmd) = if let Some(shell) = force_shell {
        if !is_valid_shell(shell) {
            return (None, Some(format!("[WSL:{distro}] invalid shell: {shell}")));
        }
        let shell = shell.rsplit('/').next().unwrap_or(shell);
        let flag = if let Some(flag) = force_shell_flag {
            if !is_valid_shell_flag(flag) {
                return (
                    None,
                    Some(format!("[WSL:{distro}] invalid shell flag: {flag}")),
                );
            }
            flag
        } else {
            default_flag_for_shell(shell)
        };
        (shell.to_string(), flag, format!("{tool} --version"))
    } else {
        let cmd = if let Some(flag) = force_shell_flag {
            if !is_valid_shell_flag(flag) {
                return (
                    None,
                    Some(format!("[WSL:{distro}] invalid shell flag: {flag}")),
                );
            }
            format!("\"${{SHELL:-sh}}\" {flag} '{tool} --version'")
        } else {
            format!(
                "\"${{SHELL:-sh}}\" -lic '{tool} --version' 2>/dev/null || \"${{SHELL:-sh}}\" -lc '{tool} --version' 2>/dev/null || \"${{SHELL:-sh}}\" -c '{tool} --version'"
            )
        };
        ("sh".to_string(), "-c", cmd)
    };

    let output = Command::new("wsl.exe")
        .args(["-d", distro, "--", &shell, flag, &cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if out.status.success() {
                let raw = if stdout.is_empty() { &stderr } else { &stdout };
                if raw.is_empty() {
                    (
                        None,
                        Some(format!("[WSL:{distro}] not installed or not executable")),
                    )
                } else {
                    (Some(extract_version(raw)), None)
                }
            } else {
                let error = if stderr.is_empty() { stdout } else { stderr };
                (
                    None,
                    Some(format!(
                        "[WSL:{distro}] {}",
                        if error.is_empty() {
                            "not installed or not executable".to_string()
                        } else {
                            error
                        }
                    )),
                )
            }
        }
        Err(err) => (None, Some(format!("[WSL:{distro}] exec failed: {err}"))),
    }
}

#[cfg(not(target_os = "windows"))]
fn try_get_version_wsl(
    _tool: &str,
    _distro: &str,
    _force_shell: Option<&str>,
    _force_shell_flag: Option<&str>,
) -> (Option<String>, Option<String>) {
    (
        None,
        Some("WSL check not supported on this platform".to_string()),
    )
}

#[cfg(target_os = "windows")]
fn is_valid_wsl_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || value == '-' || value == '_' || value == '.')
}

#[cfg(target_os = "windows")]
fn is_valid_shell(shell: &str) -> bool {
    matches!(
        shell.rsplit('/').next().unwrap_or(shell),
        "sh" | "bash" | "zsh" | "fish" | "dash"
    )
}

#[cfg(target_os = "windows")]
fn is_valid_shell_flag(flag: &str) -> bool {
    matches!(flag, "-c" | "-lc" | "-lic")
}

#[cfg(target_os = "windows")]
fn default_flag_for_shell(shell: &str) -> &'static str {
    match shell.rsplit('/').next().unwrap_or(shell) {
        "dash" | "sh" => "-c",
        "fish" => "-lc",
        _ => "-lic",
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.as_os_str().is_empty() {
        return;
    }

    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn push_env_single_dir(paths: &mut Vec<PathBuf>, value: Option<std::ffi::OsString>) {
    if let Some(raw) = value {
        push_unique_path(paths, PathBuf::from(raw));
    }
}

fn extend_from_path_list(
    paths: &mut Vec<PathBuf>,
    value: Option<std::ffi::OsString>,
    suffix: Option<&str>,
) {
    if let Some(raw) = value {
        for path in std::env::split_paths(&raw) {
            let dir = match suffix {
                Some(suffix) => path.join(suffix),
                None => path,
            };
            push_unique_path(paths, dir);
        }
    }
}

fn opencode_extra_search_paths(
    home: &Path,
    opencode_install_dir: Option<std::ffi::OsString>,
    xdg_bin_dir: Option<std::ffi::OsString>,
    gopath: Option<std::ffi::OsString>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    push_env_single_dir(&mut paths, opencode_install_dir);
    push_env_single_dir(&mut paths, xdg_bin_dir);

    if !home.as_os_str().is_empty() {
        push_unique_path(&mut paths, home.join("bin"));
        push_unique_path(&mut paths, home.join(".opencode").join("bin"));
        push_unique_path(&mut paths, home.join("go").join("bin"));
    }

    extend_from_path_list(&mut paths, gopath, Some("bin"));
    paths
}

fn tool_executable_candidates(tool: &str, dir: &Path) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        vec![
            dir.join(format!("{tool}.cmd")),
            dir.join(format!("{tool}.exe")),
            dir.join(tool),
        ]
    }

    #[cfg(not(target_os = "windows"))]
    {
        vec![dir.join(tool)]
    }
}

fn scan_cli_version(tool: &str) -> (Option<String>, Option<String>) {
    use std::process::Command;

    let home = dirs::home_dir().unwrap_or_default();
    let mut search_paths = Vec::new();

    if !home.as_os_str().is_empty() {
        push_unique_path(&mut search_paths, home.join(".local/bin"));
        push_unique_path(&mut search_paths, home.join(".npm-global/bin"));
        push_unique_path(&mut search_paths, home.join("n/bin"));
        push_unique_path(&mut search_paths, home.join(".volta/bin"));
    }

    #[cfg(target_os = "macos")]
    {
        push_unique_path(&mut search_paths, PathBuf::from("/opt/homebrew/bin"));
        push_unique_path(&mut search_paths, PathBuf::from("/usr/local/bin"));
    }

    #[cfg(target_os = "linux")]
    {
        push_unique_path(&mut search_paths, PathBuf::from("/usr/local/bin"));
        push_unique_path(&mut search_paths, PathBuf::from("/usr/bin"));
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::data_dir() {
            push_unique_path(&mut search_paths, appdata.join("npm"));
        }
        push_unique_path(&mut search_paths, PathBuf::from("C:\\Program Files\\nodejs"));
    }

    let fnm_base = home.join(".local/state/fnm_multishells");
    if fnm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&fnm_base) {
            for entry in entries.flatten() {
                let bin_path = entry.path().join("bin");
                if bin_path.exists() {
                    push_unique_path(&mut search_paths, bin_path);
                }
            }
        }
    }

    let nvm_base = home.join(".nvm/versions/node");
    if nvm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                let bin_path = entry.path().join("bin");
                if bin_path.exists() {
                    push_unique_path(&mut search_paths, bin_path);
                }
            }
        }
    }

    if tool == "opencode" {
        for path in opencode_extra_search_paths(
            &home,
            std::env::var_os("OPENCODE_INSTALL_DIR"),
            std::env::var_os("XDG_BIN_DIR"),
            std::env::var_os("GOPATH"),
        ) {
            push_unique_path(&mut search_paths, path);
        }
    }

    let current_path = std::env::var("PATH").unwrap_or_default();

    for path in &search_paths {
        #[cfg(target_os = "windows")]
        let new_path = format!("{};{current_path}", path.display());

        #[cfg(not(target_os = "windows"))]
        let new_path = format!("{}:{current_path}", path.display());

        for tool_path in tool_executable_candidates(tool, path) {
            if !tool_path.exists() {
                continue;
            }

            #[cfg(target_os = "windows")]
            let output = Command::new("cmd")
                .args(["/C", &format!("\"{}\" --version", tool_path.display())])
                .env("PATH", &new_path)
                .creation_flags(CREATE_NO_WINDOW)
                .output();

            #[cfg(not(target_os = "windows"))]
            let output = Command::new(&tool_path)
                .arg("--version")
                .env("PATH", &new_path)
                .output();

            if let Ok(out) = output {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if out.status.success() {
                    let raw = if stdout.is_empty() { &stderr } else { &stdout };
                    if !raw.is_empty() {
                        return (Some(extract_version(raw)), None);
                    }
                }
            }
        }
    }

    (None, Some("not installed or not executable".to_string()))
}

#[cfg(target_os = "windows")]
fn wsl_distro_for_tool(tool: &str) -> Option<String> {
    let override_dir = match tool {
        "claude" => crate::settings::get_claude_override_dir(),
        "codex" => crate::settings::get_codex_override_dir(),
        "gemini" => crate::settings::get_gemini_override_dir(),
        "opencode" => crate::settings::get_opencode_override_dir(),
        _ => None,
    }?;

    wsl_distro_from_path(&override_dir)
}

#[cfg(target_os = "windows")]
fn wsl_distro_from_path(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };

    match prefix.kind() {
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            let server_name = server.to_string_lossy();
            if server_name.eq_ignore_ascii_case("wsl$")
                || server_name.eq_ignore_ascii_case("wsl.localhost")
            {
                let distro = share.to_string_lossy().to_string();
                if !distro.is_empty() {
                    return Some(distro);
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::thread;

    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        current_version, normalize_version_tag, AppInfo, RuntimeService, WslShellPreference,
        TEST_CURRENT_EXE_ENV, TEST_GITHUB_API_BASE_URL_ENV, TEST_NPM_REGISTRY_BASE_URL_ENV,
    };

    #[test]
    fn normalize_version_strips_v_prefix() {
        assert_eq!(normalize_version_tag("v1.2.3"), "1.2.3");
        assert_eq!(normalize_version_tag("1.2.3"), "1.2.3");
    }

    #[test]
    fn about_contains_release_links() -> Result<(), crate::error::AppError> {
        let info: AppInfo = RuntimeService::about()?;
        assert_eq!(info.name, "CC Switch");
        assert_eq!(info.version, current_version());
        assert!(info.repository.ends_with("/cc-switch"));
        assert!(info.current_release_notes_url.contains("/releases/tag/v"));
        Ok(())
    }

    #[test]
    #[serial]
    fn portable_mode_uses_test_executable_override() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        let bundle = temp.path().join("portable-app");
        fs::create_dir_all(&bundle).expect("create bundle");
        fs::write(bundle.join("portable.ini"), "").expect("write marker");
        fs::write(bundle.join("cc-switch"), "").expect("write fake exe");
        std::env::set_var(TEST_CURRENT_EXE_ENV, bundle.join("cc-switch"));

        assert!(RuntimeService::is_portable_mode()?);

        std::env::remove_var(TEST_CURRENT_EXE_ENV);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn tool_versions_detect_local_binaries() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).expect("create bin dir");
        write_version_script(&bin.join("claude"), "claude 1.2.3")?;

        let original_path = std::env::var_os("PATH");
        std::env::set_var("PATH", join_path(&bin, original_path.as_deref()));

        let versions =
            RuntimeService::get_tool_versions(Some(vec!["claude".to_string()]), None, false)
                .await?;
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version.as_deref(), Some("1.2.3"));
        assert_eq!(versions[0].latest_version, None);

        restore_env("PATH", original_path);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn tool_versions_can_fetch_latest_metadata() -> Result<(), crate::error::AppError> {
        let server = spawn_server(vec![(
            "/@anthropic-ai/claude-code",
            r#"{"dist-tags":{"latest":"9.9.9"}}"#.to_string(),
        )]);
        let temp = tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).expect("create bin dir");
        write_version_script(&bin.join("claude"), "claude 1.2.3")?;

        let original_path = std::env::var_os("PATH");
        let original_npm = std::env::var_os(TEST_NPM_REGISTRY_BASE_URL_ENV);
        std::env::set_var("PATH", join_path(&bin, original_path.as_deref()));
        std::env::set_var(TEST_NPM_REGISTRY_BASE_URL_ENV, server);

        let versions =
            RuntimeService::get_tool_versions(Some(vec!["claude".to_string()]), None, true)
                .await?;
        assert_eq!(versions[0].version.as_deref(), Some("1.2.3"));
        assert_eq!(versions[0].latest_version.as_deref(), Some("9.9.9"));

        restore_env("PATH", original_path);
        restore_env(TEST_NPM_REGISTRY_BASE_URL_ENV, original_npm);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn update_check_uses_github_override() -> Result<(), crate::error::AppError> {
        let server = spawn_server(vec![(
            "/repos/farion1231/cc-switch/releases/latest",
            r#"{"tag_name":"v99.1.0"}"#.to_string(),
        )]);
        let original = std::env::var_os(TEST_GITHUB_API_BASE_URL_ENV);
        std::env::set_var(TEST_GITHUB_API_BASE_URL_ENV, server);

        let info = RuntimeService::check_for_updates().await?;
        assert_eq!(info.latest_version.as_deref(), Some("99.1.0"));
        assert!(info.has_update);
        assert!(info.release_notes_url.ends_with("/releases/tag/v99.1.0"));

        restore_env(TEST_GITHUB_API_BASE_URL_ENV, original);
        Ok(())
    }

    #[tokio::test]
    async fn invalid_tool_name_is_rejected() {
        let error = RuntimeService::get_tool_versions(Some(vec!["bad".to_string()]), None, false)
            .await
            .expect_err("expected invalid input");
        assert!(error.to_string().contains("unsupported tool"));
    }

    #[tokio::test]
    #[serial]
    async fn tool_versions_accept_wsl_override_map() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).expect("create bin dir");
        write_version_script(&bin.join("codex"), "codex 2.3.4")?;

        let original_path = std::env::var_os("PATH");
        std::env::set_var("PATH", join_path(&bin, original_path.as_deref()));

        let versions = RuntimeService::get_tool_versions(
            Some(vec!["codex".to_string()]),
            Some(HashMap::from([(
                "codex".to_string(),
                WslShellPreference {
                    wsl_shell: Some("bash".to_string()),
                    wsl_shell_flag: Some("-lc".to_string()),
                },
            )])),
            false,
        )
        .await?;
        assert_eq!(versions[0].version.as_deref(), Some("2.3.4"));

        restore_env("PATH", original_path);
        Ok(())
    }

    fn write_version_script(path: &Path, stdout: &str) -> Result<(), crate::error::AppError> {
        fs::write(path, format!("#!/bin/sh\necho '{stdout}'\n")).map_err(|source| {
            crate::error::AppError::Io {
                path: path.display().to_string(),
                source,
            }
        })?;
        let mut perms = fs::metadata(path)
            .map_err(|source| crate::error::AppError::Io {
                path: path.display().to_string(),
                source,
            })?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).map_err(|source| crate::error::AppError::Io {
            path: path.display().to_string(),
            source,
        })
    }

    fn join_path(bin: &Path, original: Option<&std::ffi::OsStr>) -> String {
        match original {
            Some(value) if !value.is_empty() => format!("{}:{}", bin.display(), value.to_string_lossy()),
            _ => bin.display().to_string(),
        }
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn spawn_server(routes: Vec<(&'static str, String)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind server");
        let addr = listener.local_addr().expect("server addr");

        thread::spawn(move || {
            for stream in listener.incoming().take(routes.len()) {
                let Ok(mut stream) = stream else {
                    continue;
                };
                let mut buffer = [0_u8; 4096];
                let _ = stream.read(&mut buffer);
                let request = String::from_utf8_lossy(&buffer);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                let body = routes
                    .iter()
                    .find(|(candidate, _)| *candidate == path)
                    .map(|(_, body)| body.clone())
                    .unwrap_or_else(|| "{}".to_string());
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        format!("http://{addr}")
    }
}
