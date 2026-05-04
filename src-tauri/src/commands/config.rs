#![allow(non_snake_case)]

use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::codex_config;
use crate::config::{self, get_claude_settings_path, ConfigStatus};
use crate::settings;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[tauri::command]
pub async fn get_claude_config_status() -> Result<ConfigStatus, String> {
    Ok(config::get_claude_config_status())
}

use std::str::FromStr;

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
pub async fn get_config_status(app: String) -> Result<ConfigStatus, String> {
    match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => Ok(config::get_claude_config_status()),
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
        AppType::Codex => codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
        AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
        AppType::OpenClaw => crate::openclaw_config::get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
    };

    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_config_folder(handle: AppHandle, app: String) -> Result<bool, String> {
    let config_dir = match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => config::get_claude_config_dir(),
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDirectoryEntry {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDirectoryListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<ServerDirectoryEntry>,
}

fn expand_server_directory_path(path: Option<String>) -> Result<PathBuf, String> {
    let raw = path
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match raw {
        None => dirs::home_dir().ok_or_else(|| "无法解析服务端 home 目录".to_string()),
        Some(value) if value == "~" => {
            dirs::home_dir().ok_or_else(|| "无法解析服务端 home 目录".to_string())
        }
        Some(value) if value.starts_with("~/") || value.starts_with("~\\") => {
            let home = dirs::home_dir().ok_or_else(|| "无法解析服务端 home 目录".to_string())?;
            Ok(home.join(&value[2..]))
        }
        Some(value) => Ok(PathBuf::from(value)),
    }
}

fn path_to_display(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn server_directory_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    }
    roots.extend([
        config::get_app_config_dir(),
        config::get_claude_config_dir(),
        codex_config::get_codex_config_dir(),
        crate::gemini_config::get_gemini_dir(),
        crate::opencode_config::get_opencode_dir(),
        crate::openclaw_config::get_openclaw_dir(),
        crate::hermes_config::get_hermes_dir(),
    ]);

    let mut canonical_roots: Vec<PathBuf> = Vec::new();
    for root in roots {
        let canonical = std::fs::canonicalize(&root).unwrap_or(root);
        if !canonical_roots
            .iter()
            .any(|existing| existing == &canonical)
        {
            canonical_roots.push(canonical);
        }
    }
    canonical_roots
}

fn is_path_in_allowed_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| path == root || path.starts_with(root))
}

fn canonical_existing_dir(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("目录不存在: {}", path_to_display(path)));
    }
    if !path.is_dir() {
        return Err(format!("不是目录: {}", path_to_display(path)));
    }
    std::fs::canonicalize(path).map_err(|err| format!("解析目录失败: {err}"))
}

fn list_server_directory_in_roots(
    path: Option<String>,
    roots: &[PathBuf],
) -> Result<ServerDirectoryListing, String> {
    let expanded = expand_server_directory_path(path)?;
    let dir = canonical_existing_dir(&expanded)?;
    if !is_path_in_allowed_roots(&dir, roots) {
        return Err("目录不在允许浏览的服务端范围内".to_string());
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|err| format!("读取目录失败: {err}"))? {
        let entry = entry.map_err(|err| format!("读取目录项失败: {err}"))?;
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }
        let Ok(entry_canonical) = canonical_existing_dir(&entry_path) else {
            continue;
        };
        if !is_path_in_allowed_roots(&entry_canonical, roots) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        entries.push(ServerDirectoryEntry {
            name,
            path: path_to_display(&entry_canonical),
        });
    }

    entries.sort_by_key(|entry| entry.name.to_lowercase());

    Ok(ServerDirectoryListing {
        parent: dir
            .parent()
            .filter(|parent| is_path_in_allowed_roots(parent, roots))
            .map(path_to_display),
        path: path_to_display(&dir),
        entries,
    })
}

fn list_server_directory_impl(path: Option<String>) -> Result<ServerDirectoryListing, String> {
    list_server_directory_in_roots(path, &server_directory_roots())
}

#[tauri::command]
pub async fn list_server_directory(path: Option<String>) -> Result<ServerDirectoryListing, String> {
    list_server_directory_impl(path)
}

#[tauri::command]
pub async fn validate_server_directory(path: String) -> Result<bool, String> {
    let dir = expand_server_directory_path(Some(path))?;
    if !dir.is_dir() {
        return Ok(false);
    }
    let canonical = canonical_existing_dir(&dir)?;
    Ok(is_path_in_allowed_roots(
        &canonical,
        &server_directory_roots(),
    ))
}

#[tauri::command]
pub async fn get_app_config_path() -> Result<String, String> {
    let config_path = config::get_app_config_path();
    Ok(config_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn get_app_config_dir() -> Result<String, String> {
    let config_dir = config::get_app_config_dir();
    Ok(config_dir.to_string_lossy().to_string())
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
    use super::{list_server_directory_in_roots, validate_common_config_snippet};
    use std::path::PathBuf;

    #[test]
    fn server_directory_listing_returns_sorted_child_directories_only() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("zeta")).unwrap();
        std::fs::create_dir(temp.path().join("alpha")).unwrap();
        std::fs::write(temp.path().join("file.txt"), "ignored").unwrap();

        let roots = vec![PathBuf::from(temp.path())];
        let listing =
            list_server_directory_in_roots(Some(temp.path().to_string_lossy().to_string()), &roots)
                .unwrap();

        let names: Vec<_> = listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
        assert_eq!(listing.path, temp.path().to_string_lossy().to_string());
        assert!(listing.parent.is_none());
    }

    #[test]
    fn server_directory_listing_rejects_paths_outside_allowed_roots() {
        let allowed = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let roots = vec![PathBuf::from(allowed.path())];

        let err = list_server_directory_in_roots(
            Some(outside.path().to_string_lossy().to_string()),
            &roots,
        )
        .expect_err("outside root should be rejected");

        assert!(err.contains("允许浏览"));
    }

    #[test]
    fn server_directory_listing_hides_parent_outside_allowed_roots() {
        let root = tempfile::tempdir().unwrap();
        let roots = vec![PathBuf::from(root.path())];

        let listing =
            list_server_directory_in_roots(Some(root.path().to_string_lossy().to_string()), &roots)
                .unwrap();

        assert!(listing.parent.is_none());
    }

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
