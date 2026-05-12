#![allow(non_snake_case)]

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::codex_config;
use crate::config::{self, get_claude_settings_path, ConfigStatus};
use crate::settings;
use crate::store::AppState;
use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const DETECTABLE_CLI_APPS: [&str; 4] = ["claude", "codex", "gemini", "opencode"];

#[tauri::command]
pub async fn get_claude_config_status() -> Result<ConfigStatus, String> {
    Ok(config::get_claude_config_status())
}

use std::str::FromStr;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliDetectionSummary {
    wsl_installed: bool,
    wsl_distro: Option<String>,
    tools: Vec<CliToolDetection>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliToolDetection {
    app: String,
    native: CliLocationDetection,
    wsl: Option<WslCliLocationDetection>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliLocationDetection {
    env_type: String,
    executable_path: Option<String>,
    config_dir: String,
    config_exists: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslCliLocationDetection {
    env_type: String,
    distro: String,
    executable_path: Option<String>,
    config_dir: String,
    config_exists: bool,
}

fn invalid_json_format_error(error: serde_json::Error) -> String {
    let lang = settings::get_settings()
        .language
        .unwrap_or_else(|| "zh".to_string());

    match lang.as_str() {
        "en" => format!("Invalid JSON format: {error}"),
        "ja" => format!("JSON形式が無効です: {error}"),
        _ => format!("无效的 JSON 格式: {error}"),
    }
}

fn invalid_toml_format_error(error: toml_edit::TomlError) -> String {
    let lang = settings::get_settings()
        .language
        .unwrap_or_else(|| "zh".to_string());

    match lang.as_str() {
        "en" => format!("Invalid TOML format: {error}"),
        "ja" => format!("TOML形式が無効です: {error}"),
        _ => format!("无效的 TOML 格式: {error}"),
    }
}

fn native_env_type() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }

    #[cfg(target_os = "macos")]
    {
        "macos"
    }

    #[cfg(target_os = "linux")]
    {
        "linux"
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "unknown"
    }
}

fn default_config_dir_for_app(app: &str) -> PathBuf {
    let home = config::get_home_dir();
    match app {
        "claude" => home.join(".claude"),
        "codex" => home.join(".codex"),
        "gemini" => home.join(".gemini"),
        "opencode" => home.join(".config").join("opencode"),
        _ => home,
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

fn extend_from_path_env(paths: &mut Vec<PathBuf>) {
    if let Some(raw) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&raw) {
            push_unique_path(paths, entry);
        }
    }
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

fn opencode_extra_search_paths(home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for key in ["OPENCODE_INSTALL_DIR", "XDG_BIN_DIR"] {
        if let Some(value) = std::env::var_os(key) {
            push_unique_path(&mut paths, PathBuf::from(value));
        }
    }

    if !home.as_os_str().is_empty() {
        push_unique_path(&mut paths, home.join("bin"));
        push_unique_path(&mut paths, home.join(".opencode").join("bin"));
        push_unique_path(&mut paths, home.join(".bun").join("bin"));
        push_unique_path(&mut paths, home.join("go").join("bin"));
    }

    if let Some(gopath) = std::env::var_os("GOPATH") {
        for entry in std::env::split_paths(&gopath) {
            push_unique_path(&mut paths, entry.join("bin"));
        }
    }

    paths
}

fn native_search_paths(tool: &str) -> Vec<PathBuf> {
    let home = config::get_home_dir();
    let mut search_paths = Vec::new();

    extend_from_path_env(&mut search_paths);

    if !home.as_os_str().is_empty() {
        push_unique_path(&mut search_paths, home.join(".local").join("bin"));
        push_unique_path(&mut search_paths, home.join(".npm-global").join("bin"));
        push_unique_path(&mut search_paths, home.join("n").join("bin"));
        push_unique_path(&mut search_paths, home.join(".volta").join("bin"));
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
        if let Ok(program_files) = std::env::var("ProgramFiles") {
            push_unique_path(
                &mut search_paths,
                PathBuf::from(program_files).join("nodejs"),
            );
        }
        if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
            push_unique_path(
                &mut search_paths,
                PathBuf::from(program_files_x86).join("nodejs"),
            );
        }
    }

    let fnm_base = home.join(".local").join("state").join("fnm_multishells");
    if fnm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&fnm_base) {
            for entry in entries.flatten() {
                push_unique_path(&mut search_paths, entry.path().join("bin"));
            }
        }
    }

    let nvm_base = home.join(".nvm").join("versions").join("node");
    if nvm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                push_unique_path(&mut search_paths, entry.path().join("bin"));
            }
        }
    }

    if tool == "opencode" {
        for path in opencode_extra_search_paths(&home) {
            push_unique_path(&mut search_paths, path);
        }
    }

    search_paths
}

fn canonicalize_for_display(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn find_native_executable_in_paths(tool: &str) -> Option<PathBuf> {
    for dir in native_search_paths(tool) {
        for candidate in tool_executable_candidates(tool, &dir) {
            if candidate.exists() {
                return Some(canonicalize_for_display(candidate));
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn query_windows_executable(tool: &str) -> Option<PathBuf> {
    let output = std::process::Command::new("where.exe")
        .arg(tool)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .find(|path| path.exists())
        .map(canonicalize_for_display)
}

fn detect_native_executable(tool: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        query_windows_executable(tool).or_else(|| find_native_executable_in_paths(tool))
    }

    #[cfg(not(target_os = "windows"))]
    {
        find_native_executable_in_paths(tool)
    }
}

#[cfg(target_os = "windows")]
fn run_wsl_output(args: &[&str]) -> Option<std::process::Output> {
    std::process::Command::new("wsl.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
}

#[cfg(target_os = "windows")]
fn first_nonempty_line(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.to_string())
}

#[cfg(target_os = "windows")]
fn is_valid_wsl_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

#[cfg(target_os = "windows")]
fn detect_wsl_installation() -> (bool, Option<String>) {
    let installed = query_windows_executable("wsl.exe").is_some();
    if !installed {
        return (false, None);
    }

    if let Some(output) = run_wsl_output(&["--", "sh", "-lc", "printf %s \"$WSL_DISTRO_NAME\""]) {
        if output.status.success() {
            if let Some(line) = first_nonempty_line(&String::from_utf8_lossy(&output.stdout)) {
                return (true, Some(line));
            }
        }
    }

    let distro = run_wsl_output(&["-l", "-q"])
        .and_then(|output| {
            if !output.status.success() {
                return None;
            }
            first_nonempty_line(&String::from_utf8_lossy(&output.stdout))
        })
        .filter(|name| is_valid_wsl_distro_name(name));

    (true, distro)
}

#[cfg(target_os = "windows")]
fn detect_wsl_home(distro: &str) -> Option<String> {
    if !is_valid_wsl_distro_name(distro) {
        return None;
    }

    let output = run_wsl_output(&["-d", distro, "--", "sh", "-lc", "printf %s \"$HOME\""])?;
    if !output.status.success() {
        return None;
    }

    first_nonempty_line(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "windows")]
fn detect_wsl_executable(tool: &str, distro: &str) -> Option<String> {
    if !is_valid_wsl_distro_name(distro) {
        return None;
    }

    let output = run_wsl_output(&[
        "-d",
        distro,
        "--",
        "sh",
        "-lc",
        &format!("command -v {tool} 2>/dev/null"),
    ])?;

    if !output.status.success() {
        return None;
    }

    first_nonempty_line(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "windows")]
fn detect_wsl_config_exists(app: &str, distro: &str) -> bool {
    if !is_valid_wsl_distro_name(distro) {
        return false;
    }

    let test_command = match app {
        "claude" => r#"test -d "$HOME/.claude""#,
        "codex" => r#"test -d "$HOME/.codex""#,
        "gemini" => r#"test -d "$HOME/.gemini""#,
        "opencode" => r#"test -d "$HOME/.config/opencode""#,
        _ => return false,
    };

    run_wsl_output(&["-d", distro, "--", "sh", "-lc", test_command])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn join_config_dir_for_home(home: &Path, app: &str) -> PathBuf {
    match app {
        "claude" => home.join(".claude"),
        "codex" => home.join(".codex"),
        "gemini" => home.join(".gemini"),
        "opencode" => home.join(".config").join("opencode"),
        _ => home.to_path_buf(),
    }
}

#[cfg(target_os = "windows")]
fn detect_wsl_location(app: &str, distro: &str) -> Option<WslCliLocationDetection> {
    let home = detect_wsl_home(distro)?;
    let config_dir = join_config_dir_for_home(Path::new(&home), app);

    Some(WslCliLocationDetection {
        env_type: "wsl".to_string(),
        distro: distro.to_string(),
        executable_path: detect_wsl_executable(app, distro),
        config_dir: config_dir.to_string_lossy().to_string(),
        config_exists: detect_wsl_config_exists(app, distro),
    })
}

#[allow(unused_variables)]
fn detect_cli_tool(app: &str, wsl_distro: Option<&str>) -> CliToolDetection {
    let config_dir = default_config_dir_for_app(app);

    CliToolDetection {
        app: app.to_string(),
        native: CliLocationDetection {
            env_type: native_env_type().to_string(),
            executable_path: detect_native_executable(app)
                .map(|path| path.to_string_lossy().to_string()),
            config_dir: config_dir.to_string_lossy().to_string(),
            config_exists: config_dir.exists(),
        },
        #[cfg(target_os = "windows")]
        wsl: wsl_distro.and_then(|distro| detect_wsl_location(app, distro)),
        #[cfg(not(target_os = "windows"))]
        wsl: None,
    }
}

fn validate_common_config_snippet(app_type: &str, snippet: &str) -> Result<(), String> {
    if snippet.trim().is_empty() {
        return Ok(());
    }

    match app_type {
        "claude" | "gemini" | "omo" | "omo-slim" => {
            serde_json::from_str::<serde_json::Value>(snippet)
                .map_err(invalid_json_format_error)?;
        }
        "codex" => {
            snippet
                .parse::<toml_edit::DocumentMut>()
                .map_err(invalid_toml_format_error)?;
        }
        _ => {}
    }

    Ok(())
}

#[tauri::command]
pub async fn get_config_status(
    state: State<'_, AppState>,
    app: String,
) -> Result<ConfigStatus, String> {
    match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => Ok(config::get_claude_config_status()),
        AppType::ClaudeDesktop => {
            let status = crate::claude_desktop_config::get_status(
                state.db.as_ref(),
                state.proxy_service.is_running().await,
            )
            .map_err(|e| e.to_string())?;
            Ok(ConfigStatus {
                exists: status.configured,
                path: status.config_library_path.unwrap_or_default(),
            })
        }
        AppType::Codex => {
            let auth_path = codex_config::get_codex_auth_path();
            let exists = auth_path.exists();
            let path = codex_config::get_codex_config_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::Gemini => {
            let env_path = crate::gemini_config::get_gemini_env_path();
            let exists = env_path.exists();
            let path = crate::gemini_config::get_gemini_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::OpenCode => {
            let config_path = crate::opencode_config::get_opencode_config_path();
            let exists = config_path.exists();
            let path = crate::opencode_config::get_opencode_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::OpenClaw => {
            let config_path = crate::openclaw_config::get_openclaw_config_path();
            let exists = config_path.exists();
            let path = crate::openclaw_config::get_openclaw_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::Hermes => {
            let config_path = crate::hermes_config::get_hermes_config_path();
            let exists = config_path.exists();
            let path = crate::hermes_config::get_hermes_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
    }
}

#[tauri::command]
pub async fn get_claude_code_config_path() -> Result<String, String> {
    Ok(get_claude_settings_path().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn get_config_dir(app: String) -> Result<String, String> {
    let dir = match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::ClaudeDesktop => {
            crate::claude_desktop_config::get_config_library_path().map_err(|e| e.to_string())?
        }
        AppType::Codex => codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
        AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
        AppType::OpenClaw => crate::openclaw_config::get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
    };

    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn detect_cli_tools() -> Result<CliDetectionSummary, String> {
    #[cfg(target_os = "windows")]
    let (wsl_installed, wsl_distro) = detect_wsl_installation();

    #[cfg(not(target_os = "windows"))]
    let (wsl_installed, wsl_distro) = (false, None);

    Ok(CliDetectionSummary {
        wsl_installed,
        wsl_distro: wsl_distro.clone(),
        tools: DETECTABLE_CLI_APPS
            .iter()
            .map(|app| detect_cli_tool(app, wsl_distro.as_deref()))
            .collect(),
    })
}

#[tauri::command]
pub async fn open_config_folder(handle: AppHandle, app: String) -> Result<bool, String> {
    let config_dir = match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::ClaudeDesktop => {
            crate::claude_desktop_config::get_config_library_path().map_err(|e| e.to_string())?
        }
        AppType::Codex => codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
        AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
        AppType::OpenClaw => crate::openclaw_config::get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
    };

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

#[tauri::command]
pub async fn pick_directory(
    app: AppHandle,
    #[allow(non_snake_case)] defaultPath: Option<String>,
) -> Result<Option<String>, String> {
    let initial = defaultPath
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());

    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = app.dialog().file();
        if let Some(path) = initial {
            builder = builder.set_directory(path);
        }
        builder.blocking_pick_folder()
    })
    .await
    .map_err(|e| format!("弹出目录选择器失败: {e}"))?;

    match result {
        Some(file_path) => {
            let resolved = file_path
                .simplified()
                .into_path()
                .map_err(|e| format!("解析选择的目录失败: {e}"))?;
            Ok(Some(resolved.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn get_app_config_path() -> Result<String, String> {
    let config_path = config::get_app_config_path();
    Ok(config_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_app_config_folder(handle: AppHandle) -> Result<bool, String> {
    let config_dir = config::get_app_config_dir();

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

#[tauri::command]
pub async fn get_claude_common_config_snippet(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, String> {
    state
        .db
        .get_config_snippet("claude")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_claude_common_config_snippet(
    snippet: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), String> {
    let is_cleared = snippet.trim().is_empty();

    if !snippet.trim().is_empty() {
        serde_json::from_str::<serde_json::Value>(&snippet).map_err(invalid_json_format_error)?;
    }

    let value = if is_cleared { None } else { Some(snippet) };

    state
        .db
        .set_config_snippet("claude", value)
        .map_err(|e| e.to_string())?;
    state
        .db
        .set_config_snippet_cleared("claude", is_cleared)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_common_config_snippet(
    app_type: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, String> {
    state
        .db
        .get_config_snippet(&app_type)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_common_config_snippet(
    app_type: String,
    snippet: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), String> {
    let is_cleared = snippet.trim().is_empty();
    let old_snippet = state
        .db
        .get_config_snippet(&app_type)
        .map_err(|e| e.to_string())?;

    validate_common_config_snippet(&app_type, &snippet)?;

    let value = if is_cleared { None } else { Some(snippet) };

    if matches!(app_type.as_str(), "claude" | "codex" | "gemini") {
        if let Some(legacy_snippet) = old_snippet
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let app = AppType::from_str(&app_type).map_err(|e| e.to_string())?;
            crate::services::provider::ProviderService::migrate_legacy_common_config_usage(
                state.inner(),
                app,
                legacy_snippet,
            )
            .map_err(|e| e.to_string())?;
        }
    }

    state
        .db
        .set_config_snippet(&app_type, value)
        .map_err(|e| e.to_string())?;
    state
        .db
        .set_config_snippet_cleared(&app_type, is_cleared)
        .map_err(|e| e.to_string())?;

    if matches!(app_type.as_str(), "claude" | "codex" | "gemini") {
        let app = AppType::from_str(&app_type).map_err(|e| e.to_string())?;
        crate::services::provider::ProviderService::sync_current_provider_for_app(
            state.inner(),
            app,
        )
        .map_err(|e| e.to_string())?;
    }

    if app_type == "omo"
        && state
            .db
            .get_current_omo_provider("opencode", "omo")
            .map_err(|e| e.to_string())?
            .is_some()
    {
        crate::services::OmoService::write_config_to_file(
            state.inner(),
            &crate::services::omo::STANDARD,
        )
        .map_err(|e| e.to_string())?;
    }
    if app_type == "omo-slim"
        && state
            .db
            .get_current_omo_provider("opencode", "omo-slim")
            .map_err(|e| e.to_string())?
            .is_some()
    {
        crate::services::OmoService::write_config_to_file(
            state.inner(),
            &crate::services::omo::SLIM,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_common_config_snippet;

    #[test]
    fn validate_common_config_snippet_accepts_comment_only_codex_snippet() {
        validate_common_config_snippet("codex", "# comment only\n")
            .expect("comment-only codex snippet should be valid");
    }

    #[test]
    fn validate_common_config_snippet_rejects_invalid_codex_snippet() {
        let err = validate_common_config_snippet("codex", "[broken")
            .expect_err("invalid codex snippet should be rejected");
        assert!(
            err.contains("TOML") || err.contains("toml") || err.contains("格式"),
            "expected TOML validation error, got {err}"
        );
    }
}

#[tauri::command]
pub async fn extract_common_config_snippet(
    appType: String,
    settingsConfig: Option<String>,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<String, String> {
    let app = AppType::from_str(&appType).map_err(|e| e.to_string())?;

    if let Some(settings_config) = settingsConfig.filter(|s| !s.trim().is_empty()) {
        let settings: serde_json::Value =
            serde_json::from_str(&settings_config).map_err(invalid_json_format_error)?;

        return crate::services::provider::ProviderService::extract_common_config_snippet_from_settings(
            app,
            &settings,
        )
        .map_err(|e| e.to_string());
    }

    crate::services::provider::ProviderService::extract_common_config_snippet(&state, app)
        .map_err(|e| e.to_string())
}

/// 获取指定应用的 WSL 覆盖目录（如果已配置）
#[tauri::command]
pub async fn get_wsl_config_dir(app: String) -> Result<Option<String>, String> {
    let dir = match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => crate::config::get_claude_wsl_config_dir(),
        AppType::Codex => crate::config::get_codex_wsl_config_dir(),
        AppType::Gemini => crate::config::get_gemini_wsl_config_dir(),
        AppType::OpenCode => crate::config::get_opencode_wsl_config_dir(),
        AppType::OpenClaw => crate::config::get_openclaw_wsl_config_dir(),
        _ => None,
    };
    Ok(dir.map(|p| p.to_string_lossy().to_string()))
}

/// 根据运行环境获取配置目录（use_wsl=true 且配置了 WSL 覆盖时返回 WSL 目录）
#[tauri::command]
pub async fn get_config_dir_for_environment(app: String, use_wsl: bool) -> Result<String, String> {
    let dir = match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => crate::config::get_claude_config_dir_for_environment(use_wsl),
        AppType::Codex => crate::config::get_codex_config_dir_for_environment(use_wsl),
        AppType::Gemini => crate::config::get_gemini_dir_for_environment(use_wsl),
        AppType::OpenCode => crate::config::get_opencode_dir_for_environment(use_wsl),
        AppType::OpenClaw => crate::config::get_openclaw_dir_for_environment(use_wsl),
        _ => return Err(format!("Unsupported app: {app}")),
    };
    Ok(dir.to_string_lossy().to_string())
}
