use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use codex_plus_core::install::SILENT_BINARY;
use codex_plus_core::models::{DeleteResult, SessionRef};
use codex_plus_core::relay_environment::RelayEnvironmentReport;
use codex_plus_core::script_market::{self, MarketScript, ScriptMarketManifest};
use codex_plus_core::settings::{BackendSettings, RelayProfile, SettingsStore};
use codex_plus_core::status::{LaunchStatus, StatusStore};
use codex_plus_core::user_scripts::UserScriptManager;
use codex_plus_core::zed_remote::{ZedOpenStrategy, ZedRemoteProject};
use serde::Serialize;
use serde_json::{Value, json};

use crate::install::{self, InstallActionResult, InstallOptions};

#[derive(Debug, Clone, Serialize)]
pub struct CommandResult<T>
where
    T: Serialize,
{
    pub status: String,
    pub message: String,
    #[serde(flatten)]
    pub payload: T,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionPayload {
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathState {
    pub status: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverviewPayload {
    pub codex_app: PathState,
    pub codex_version: Option<String>,
    pub silent_shortcut: PathState,
    pub management_shortcut: PathState,
    pub latest_launch: Option<LaunchStatus>,
    pub current_version: String,
    pub update_status: String,
    pub settings_path: String,
    pub logs_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsPayload {
    pub settings: BackendSettings,
    pub settings_path: String,
    pub user_scripts: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceRepairPayload {
    pub codex_home: String,
    pub marketplace_root: Option<String>,
    pub initialized: bool,
    pub configured: bool,
    pub needs_repair: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceStatusPayload {
    pub codex_home: String,
    pub marketplace_root: Option<String>,
    pub config_registered: bool,
    pub needs_repair: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePluginMarketplacePayload {
    pub codex_home: String,
    pub marketplace_root: Option<String>,
    pub config_registered: bool,
    pub needs_repair: bool,
    pub plugin_count: usize,
    pub skill_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcsProvidersPayload {
    pub db_path: String,
    pub providers: Vec<codex_plus_core::ccs_import::CcsProviderImport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingProviderImportPayload {
    pub pending: Option<codex_plus_core::provider_import::ProviderImportRequest>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSessionsPayload {
    pub db_path: String,
    pub db_paths: Vec<String>,
    pub sessions: Vec<codex_plus_data::LocalSession>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZedRemoteProjectsPayload {
    pub projects: Vec<ZedRemoteProject>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZedRemoteOpenPayload {
    pub url: String,
    pub strategy: ZedOpenStrategy,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLocalSessionRequest {
    pub session_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub db_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayPayload {
    pub authenticated: bool,
    pub auth_source: String,
    pub account_label: Option<String>,
    pub config_path: String,
    pub configured: bool,
    pub requires_openai_auth: bool,
    pub has_bearer_token: bool,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayFilesPayload {
    pub config_path: String,
    pub auth_path: String,
    pub config_contents: String,
    pub auth_contents: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelaySwitchPayload {
    pub settings: BackendSettings,
    pub relay: RelayPayload,
    pub settings_path: String,
    pub user_scripts: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsBackfillPayload {
    pub settings: BackendSettings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEntriesPayload {
    pub settings: BackendSettings,
    pub entries: codex_plus_core::relay_config::CodexContextEntries,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveContextEntriesPayload {
    pub entries: codex_plus_core::relay_config::CodexContextEntries,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRelayCommonConfigPayload {
    pub common_config_contents: String,
    pub profile_config_contents: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayProfileTestPayload {
    pub http_status: u16,
    pub endpoint: String,
    pub response_preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayLatencyPayload {
    pub latency_ms: Option<u64>,
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepwiseTestPayload {
    pub item_count: usize,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayProfileModelsPayload {
    pub models: Vec<String>,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDoctorCheck {
    pub id: String,
    pub title: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDoctorPayload {
    pub profile_name: String,
    pub model: String,
    pub summary: String,
    pub recommendation: String,
    pub checks: Vec<ProviderDoctorCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvConflictsPayload {
    pub conflicts: Vec<codex_plus_core::env_conflicts::EnvConflict>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveEnvConflictsRequest {
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveEnvConflictsPayload {
    pub removed: Vec<codex_plus_core::env_conflicts::EnvConflictRemoval>,
    pub backup_path: Option<String>,
    pub remaining: Vec<codex_plus_core::env_conflicts::EnvConflict>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRelayFileRequest {
    pub kind: String,
    pub contents: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackfillRelayProfileRequest {
    pub settings: BackendSettings,
    pub profile_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSettingsRequest {
    pub settings: BackendSettings,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEntryRequest {
    pub settings: BackendSettings,
    pub kind: String,
    pub id: String,
    pub toml_body: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDeleteRequest {
    pub settings: BackendSettings,
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRelayCommonConfigRequest {
    pub config_contents: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRequest {
    #[serde(default)]
    pub app_path: String,
    #[serde(default = "default_debug_port")]
    pub debug_port: u16,
    #[serde(default = "default_helper_port")]
    pub helper_port: u16,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRequest {
    #[serde(default = "default_log_lines")]
    pub lines: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogsPayload {
    pub path: String,
    pub text: String,
    pub lines: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsPayload {
    pub report: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatcherPayload {
    pub enabled: bool,
    pub disabled_flag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdsPayload {
    pub version: u64,
    pub ads: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScriptMarketPayload {
    pub market: Value,
    pub user_scripts: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupPayload {
    pub show_update: bool,
}

#[tauri::command]
pub fn backend_version() -> CommandResult<VersionPayload> {
    ok(
        "后端版本已读取。",
        VersionPayload {
            version: codex_plus_core::version::VERSION.to_string(),
        },
    )
}

#[tauri::command]
pub fn startup_options() -> CommandResult<StartupPayload> {
    ok(
        "启动参数已读取。",
        StartupPayload {
            show_update: startup_should_show_update(),
        },
    )
}

pub fn startup_should_show_update() -> bool {
    should_show_update(
        std::env::args(),
        std::env::var("CODEX_PLUS_SHOW_UPDATE").ok().as_deref(),
    )
}

fn should_show_update<I, S>(args: I, env_value: Option<&str>) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| arg.as_ref() == "--show-update") || env_value == Some("1")
}

#[tauri::command]
pub async fn load_overview() -> CommandResult<OverviewPayload> {
    let payload = tauri::async_runtime::spawn_blocking(load_overview_payload).await;
    let Ok((codex_app_path, entrypoints, latest_launch)) = payload else {
        return failed(
            "概览后台任务失败。",
            OverviewPayload {
                codex_app: path_state(None),
                codex_version: None,
                silent_shortcut: path_state(None),
                management_shortcut: path_state(None),
                latest_launch: None,
                current_version: codex_plus_core::version::VERSION.to_string(),
                update_status: "not_checked".to_string(),
                settings_path: codex_plus_core::paths::default_settings_path()
                    .to_string_lossy()
                    .to_string(),
                logs_path: codex_plus_core::paths::default_diagnostic_log_path()
                    .to_string_lossy()
                    .to_string(),
            },
        );
    };
    ok(
        "概览已加载。",
        OverviewPayload {
            codex_version: codex_app_path
                .as_deref()
                .and_then(codex_plus_core::app_paths::codex_app_version),
            codex_app: path_state(codex_app_path),
            silent_shortcut: shortcut_state(entrypoints.silent_shortcut),
            management_shortcut: shortcut_state(entrypoints.management_shortcut),
            latest_launch,
            current_version: codex_plus_core::version::VERSION.to_string(),
            update_status: "not_checked".to_string(),
            settings_path: codex_plus_core::paths::default_settings_path()
                .to_string_lossy()
                .to_string(),
            logs_path: codex_plus_core::paths::default_diagnostic_log_path()
                .to_string_lossy()
                .to_string(),
        },
    )
}

#[tauri::command]
pub fn launch_codex_plus(request: LaunchRequest) -> CommandResult<Value> {
    spawn_codex_plus_launch(request, "启动任务已在后台开始，可稍后查看概览状态。")
}

#[tauri::command]
pub fn restart_codex_plus(request: LaunchRequest) -> CommandResult<Value> {
    codex_plus_core::watcher::stop_launcher_processes_and_wait();
    codex_plus_core::watcher::stop_codex_processes_and_wait();
    spawn_codex_plus_launch(request, "Codex 已请求重启，启动任务正在后台运行。")
}

fn spawn_codex_plus_launch(request: LaunchRequest, accepted_message: &str) -> CommandResult<Value> {
    let debug_port = request.debug_port;
    let helper_port = request.helper_port;
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "manager.launch_requested",
        json!({
            "debug_port": debug_port,
            "helper_port": helper_port,
            "app_path": request.app_path.trim()
        }),
    );
    match spawn_silent_launcher(&request) {
        Ok(()) => CommandResult {
            status: "accepted".to_string(),
            message: accepted_message.to_string(),
            payload: json!({
                "debugPort": debug_port,
                "helperPort": helper_port
            }),
        },
        Err(error) => failed(
            &format!("启动静默入口失败：{error}"),
            json!({
                "debugPort": debug_port,
                "helperPort": helper_port
            }),
        ),
    }
}

fn spawn_silent_launcher(request: &LaunchRequest) -> anyhow::Result<()> {
    let launcher = codex_plus_core::install::companion_binary_path(SILENT_BINARY);
    let mut command = std::process::Command::new(&launcher);
    if !request.app_path.trim().is_empty() {
        command.arg("--app-path").arg(request.app_path.trim());
    }
    command
        .arg("--debug-port")
        .arg(request.debug_port.to_string())
        .arg("--helper-port")
        .arg(request.helper_port.to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("无法启动 {}：{error}", launcher.to_string_lossy()))
}

#[tauri::command]
pub fn load_settings() -> CommandResult<SettingsPayload> {
    settings_payload("设置已加载。", "设置读取失败")
}

#[tauri::command]
pub fn save_settings(settings: BackendSettings) -> CommandResult<SettingsPayload> {
    let settings = normalize_settings_before_save(settings);
    match SettingsStore::default().save(&settings) {
        Ok(()) => settings_payload("设置已保存。", "设置保存后重新读取失败"),
        Err(error) => failed(
            &format!("保存设置失败：{error}"),
            SettingsPayload {
                settings,
                settings_path: codex_plus_core::paths::default_settings_path()
                    .to_string_lossy()
                    .to_string(),
                user_scripts: user_script_inventory(),
            },
        ),
    }
}

#[tauri::command]
pub fn load_ccs_providers() -> CommandResult<CcsProvidersPayload> {
    let db_path = codex_plus_core::ccs_import::default_ccs_db_path();
    match codex_plus_core::ccs_import::list_codex_providers_from_db(&db_path) {
        Ok(providers) => ok(
            &format!(
                "已读取 cc-switch Codex 供应商配置：{} 个。",
                providers.len()
            ),
            CcsProvidersPayload {
                db_path: db_path.to_string_lossy().to_string(),
                providers,
            },
        ),
        Err(error) => failed(
            &format!("读取 cc-switch 供应商配置失败：{error}"),
            CcsProvidersPayload {
                db_path: db_path.to_string_lossy().to_string(),
                providers: Vec::new(),
            },
        ),
    }
}

#[tauri::command]
pub fn import_ccs_providers() -> CommandResult<SettingsPayload> {
    let providers = match codex_plus_core::ccs_import::list_codex_providers_from_default_db() {
        Ok(providers) => providers,
        Err(error) => {
            let payload = settings_payload_value().unwrap_or_else(|(_, payload)| payload);
            return failed(&format!("读取 cc-switch 供应商配置失败：{error}"), payload);
        }
    };

    let store = SettingsStore::default();
    let mut settings = store.load().unwrap_or_default();
    let mut existing_keys: Vec<String> = settings
        .relay_profiles
        .iter()
        .map(codex_plus_core::ccs_import::imported_provider_identity)
        .collect();
    let mut existing_ids: Vec<String> = settings
        .relay_profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect();
    let mut imported = 0usize;

    for provider in providers {
        let key = codex_plus_core::ccs_import::provider_identity_from_ccs(&provider);
        if existing_keys.iter().any(|existing| existing == &key) {
            continue;
        }
        let profile = codex_plus_core::ccs_import::relay_profile_from_ccs(&provider, &existing_ids);
        existing_ids.push(profile.id.clone());
        existing_keys.push(key);
        settings.relay_profiles.push(profile);
        imported += 1;
    }

    if imported == 0 {
        return settings_payload("没有新的 cc-switch 供应商配置需要导入。", "设置读取失败");
    }

    settings = normalize_settings_before_save(settings);
    match store.save(&settings) {
        Ok(()) => settings_payload(
            &format!("已从 cc-switch 导入供应商配置：{imported} 个。"),
            "导入供应商配置后重新读取设置失败",
        ),
        Err(error) => failed(
            &format!("保存 cc-switch 供应商配置失败：{error}"),
            settings_payload_value().unwrap_or_else(|(_, payload)| payload),
        ),
    }
}

#[tauri::command]
pub fn load_pending_provider_import() -> CommandResult<PendingProviderImportPayload> {
    match codex_plus_core::provider_import::load_pending_provider_import() {
        Ok(pending) => ok(
            "待确认供应商导入已读取。",
            PendingProviderImportPayload { pending },
        ),
        Err(error) => failed(
            &format!("读取待确认供应商导入失败：{error}"),
            PendingProviderImportPayload { pending: None },
        ),
    }
}

#[tauri::command]
pub fn confirm_pending_provider_import() -> CommandResult<SettingsPayload> {
    match codex_plus_core::provider_import::confirm_pending_provider_import() {
        Ok(Some(result)) => {
            let message = if result.imported {
                format!("已导入供应商配置：{}。", result.profile_name)
            } else {
                format!("供应商配置已存在：{}。", result.profile_name)
            };
            settings_payload(&message, "供应商导入后重新读取设置失败")
        }
        Ok(None) => settings_payload("没有待确认的供应商导入。", "设置读取失败"),
        Err(error) => failed(
            &format!("导入供应商配置失败：{error}"),
            settings_payload_value().unwrap_or_else(|(_, payload)| payload),
        ),
    }
}

#[tauri::command]
pub fn dismiss_pending_provider_import() -> CommandResult<PendingProviderImportPayload> {
    match codex_plus_core::provider_import::clear_pending_provider_import() {
        Ok(()) => ok(
            "已取消供应商导入。",
            PendingProviderImportPayload { pending: None },
        ),
        Err(error) => failed(
            &format!("取消供应商导入失败：{error}"),
            PendingProviderImportPayload { pending: None },
        ),
    }
}

#[tauri::command]
pub fn list_local_sessions() -> CommandResult<LocalSessionsPayload> {
    let home = codex_plus_core::codex_sqlite::default_codex_home_dir();
    let db_paths = codex_plus_core::codex_sqlite::codex_session_db_paths_from_home(&home);
    let mut sessions = Vec::new();
    let mut errors = Vec::new();
    for db_path in &db_paths {
        let adapter = local_session_adapter(db_path);
        match adapter.list_local_sessions() {
            Ok(mut items) => sessions.append(&mut items),
            Err(error) if db_path.exists() => {
                errors.push(format!("{}: {error}", db_path.to_string_lossy()));
            }
            Err(_) => {}
        }
    }
    sessions.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    let mut seen_session_ids = std::collections::HashSet::new();
    sessions.retain(|session| seen_session_ids.insert(session.id.clone()));
    let payload = LocalSessionsPayload {
        db_path: db_paths
            .first()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default(),
        db_paths: db_paths
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
        sessions,
    };
    if errors.is_empty() {
        ok(
            &format!("已读取 {} 个本地会话。", payload.sessions.len()),
            payload,
        )
    } else {
        failed(
            &format!("读取部分本地会话失败：{}", errors.join("; ")),
            payload,
        )
    }
}

#[tauri::command]
pub fn list_zed_remote_projects() -> CommandResult<ZedRemoteProjectsPayload> {
    let result = codex_plus_core::zed_remote::list_zed_remote_projects_response(&json!({}));
    if result.get("status").and_then(Value::as_str) == Some("ok") {
        let projects = serde_json::from_value::<Vec<ZedRemoteProject>>(
            result
                .get("projects")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        )
        .unwrap_or_default();
        return ok(
            &format!("已读取 {} 个 Zed 远程项目。", projects.len()),
            ZedRemoteProjectsPayload { projects },
        );
    }
    failed(
        result
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("读取 Zed 远程项目失败。"),
        ZedRemoteProjectsPayload {
            projects: Vec::new(),
        },
    )
}

#[tauri::command]
pub fn open_zed_remote(payload: Value) -> CommandResult<ZedRemoteOpenPayload> {
    let result = codex_plus_core::zed_remote::open_zed_remote(&payload);
    let strategy = result
        .get("strategy")
        .cloned()
        .and_then(|value| serde_json::from_value::<ZedOpenStrategy>(value).ok())
        .unwrap_or_default();
    let url = result
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if result.get("status").and_then(Value::as_str) == Some("ok") {
        return ok(
            "已在 Zed Remote 打开项目。",
            ZedRemoteOpenPayload { url, strategy },
        );
    }
    failed(
        result
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("无法在 Zed Remote 打开项目。"),
        ZedRemoteOpenPayload { url, strategy },
    )
}

#[tauri::command]
pub fn forget_zed_remote_project(id: String) -> CommandResult<ZedRemoteProjectsPayload> {
    let result =
        codex_plus_core::zed_remote::forget_zed_remote_project_response(&json!({ "id": id }));
    if result.get("status").and_then(Value::as_str) != Some("ok") {
        return failed(
            result
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("移除 Zed 远程项目失败。"),
            ZedRemoteProjectsPayload {
                projects: Vec::new(),
            },
        );
    }
    list_zed_remote_projects()
}

#[tauri::command]
pub fn delete_local_session(request: DeleteLocalSessionRequest) -> CommandResult<DeleteResult> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return failed(
            "会话 ID 不能为空。",
            DeleteResult {
                status: codex_plus_core::models::DeleteStatus::Failed,
                session_id: String::new(),
                message: "会话 ID 不能为空。".to_string(),
                undo_token: None,
                backup_path: None,
            },
        );
    }
    let session = SessionRef {
        session_id: session_id.to_string(),
        title: request.title,
    };
    let mut candidate_paths = Vec::new();
    if let Some(path) = request.db_path.as_deref() {
        let path = PathBuf::from(path);
        if !candidate_paths.iter().any(|candidate| candidate == &path) {
            candidate_paths.push(path);
        }
    }
    for path in codex_plus_core::codex_sqlite::codex_session_db_paths_from_home(
        &codex_plus_core::codex_sqlite::default_codex_home_dir(),
    ) {
        if !candidate_paths.iter().any(|candidate| candidate == &path) {
            candidate_paths.push(path);
        }
    }
    log_manager_event(
        "manager.delete_local_session.start",
        json!({
            "session_id": session_id,
            "title": session.title,
            "requested_db_path": request.db_path,
            "candidate_paths": candidate_paths
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>(),
        }),
    );
    let result = codex_plus_data::delete_local_from_paths(
        candidate_paths.clone(),
        codex_plus_data::BackupStore::new(
            codex_plus_core::paths::default_app_state_dir().join("backups"),
        ),
        &session,
    );
    log_manager_event(
        "manager.delete_local_session.finish",
        json!({
            "session_id": session_id,
            "final_status": format!("{:?}", result.status),
            "final_message": result.message,
            "candidate_paths": candidate_paths
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>(),
        }),
    );
    let status = if matches!(
        result.status,
        codex_plus_core::models::DeleteStatus::LocalDeleted
    ) {
        "ok"
    } else {
        "failed"
    };
    CommandResult {
        status: status.to_string(),
        message: result.message.clone(),
        payload: result,
    }
}

fn local_session_adapter(db_path: &Path) -> codex_plus_data::SQLiteStorageAdapter {
    codex_plus_data::SQLiteStorageAdapter::new(
        db_path,
        codex_plus_data::BackupStore::new(
            codex_plus_core::paths::default_app_state_dir().join("backups"),
        ),
    )
}

fn normalize_settings_before_save(mut settings: BackendSettings) -> BackendSettings {
    if let Some(path) =
        codex_plus_core::app_paths::normalize_codex_app_path(Path::new(&settings.codex_app_path))
    {
        settings.codex_app_path = path.to_string_lossy().to_string();
    }
    settings.relay_common_config_contents =
        codex_plus_core::relay_config::sanitize_common_config_contents(
            &settings.relay_common_config_contents,
        );
    let (common_without_context, extracted_context) =
        split_relay_context_config_sections(&settings.relay_common_config_contents);
    settings.relay_common_config_contents = common_without_context;
    settings.relay_context_config_contents =
        relay_join_config_sections(&[&settings.relay_context_config_contents, &extracted_context]);
    settings.relay_context_config_contents =
        codex_plus_core::relay_config::sanitize_common_config_contents(
            &settings.relay_context_config_contents,
        );
    for profile in &mut settings.relay_profiles {
        if let Err(error) =
            codex_plus_core::relay_config::normalize_relay_profile_for_storage(profile)
        {
            log_manager_event(
                "manager.normalize_relay_profile_for_storage.failed",
                json!({
                    "profileId": profile.id,
                    "profileName": profile.name,
                    "error": error.to_string()
                }),
            );
        }
    }
    let common_config = relay_combined_common_config(&settings);
    if !common_config.trim().is_empty() {
        for profile in &mut settings.relay_profiles {
            if !profile.use_common_config || profile.config_contents.trim().is_empty() {
                continue;
            }
            match codex_plus_core::relay_config::strip_common_config_from_config(
                &profile.config_contents,
                &common_config,
            ) {
                Ok(stripped) => {
                    profile.config_contents =
                        strip_common_config_text_fallback(&stripped, &common_config);
                }
                Err(_) => {
                    profile.config_contents =
                        strip_common_config_text_fallback(&profile.config_contents, &common_config);
                }
            }
        }
    }
    settings.provider_sync_saved_providers =
        normalize_provider_sync_provider_list(settings.provider_sync_saved_providers);
    settings.provider_sync_manual_providers =
        normalize_provider_sync_provider_list(settings.provider_sync_manual_providers);
    settings.provider_sync_last_selected_provider = settings
        .provider_sync_last_selected_provider
        .trim()
        .to_string();
    settings
}

fn normalize_provider_sync_provider_list(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            result.push(trimmed.to_string());
        }
    }
    result.sort();
    result
}

fn relay_combined_common_config(settings: &BackendSettings) -> String {
    relay_join_config_sections(&[
        &settings.relay_common_config_contents,
        &settings.relay_context_config_contents,
    ])
}

fn relay_join_config_sections(sections: &[&str]) -> String {
    let sections = sections
        .iter()
        .map(|section| section.trim())
        .filter(|section| !section.is_empty())
        .collect::<Vec<_>>();
    if sections.is_empty() {
        String::new()
    } else {
        codex_plus_core::relay_config::normalize_config_text(&format!(
            "{}\n",
            sections.join("\n\n")
        ))
    }
}

fn split_relay_context_config_sections(config: &str) -> (String, String) {
    let mut common = Vec::new();
    let mut context = Vec::new();
    let mut in_context_table = false;

    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_context_table = trimmed.starts_with("[mcp_servers.")
                || trimmed.starts_with("[skills.")
                || trimmed.starts_with("[plugins.");
        }
        if in_context_table {
            context.push(line);
        } else {
            common.push(line);
        }
    }

    (
        relay_join_config_sections(&[&common.join("\n")]),
        relay_join_config_sections(&[&context.join("\n")]),
    )
}

fn strip_common_config_text_fallback(config_contents: &str, common_config: &str) -> String {
    let common = common_config_anchors(common_config);
    if common.root_keys.is_empty() && common.table_headers.is_empty() {
        return ensure_text_newline(config_contents.trim_end());
    }

    let mut kept = Vec::new();
    let mut skipping_table = false;
    let mut in_root_section = true;
    let mut removed_root_keys = std::collections::HashSet::new();
    let source_root_keys = toml_root_keys_before_first_table(config_contents);

    for line in config_contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_root_section = false;
            let header = trimmed.to_string();
            skipping_table = common.table_headers.contains(&header);
            if skipping_table {
                continue;
            }
        }

        if skipping_table {
            continue;
        }

        if in_root_section && let Some(key) = toml_key_from_line(trimmed) {
            if common.root_keys.contains(key) {
                let is_duplicate_common_key = removed_root_keys.contains(key)
                    || source_root_keys.contains(key)
                    || common.table_headers.contains("[features]")
                    || common
                        .table_headers
                        .contains("[marketplaces.openai-bundled]")
                    || common
                        .table_headers
                        .contains("[plugins.\"superpowers@openai-curated\"]");
                if is_duplicate_common_key {
                    removed_root_keys.insert(key.to_string());
                    continue;
                }
            }
        }

        kept.push(line);
    }

    ensure_text_newline(kept.join("\n").trim_end())
}

fn toml_root_keys_before_first_table(config_contents: &str) -> std::collections::HashSet<String> {
    let mut keys = std::collections::HashSet::new();
    for line in config_contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            break;
        }
        if let Some(key) = toml_key_from_line(trimmed) {
            keys.insert(key.to_string());
        }
    }
    keys
}

struct CommonConfigAnchors {
    root_keys: std::collections::HashSet<String>,
    table_headers: std::collections::HashSet<String>,
}

fn common_config_anchors(common_config: &str) -> CommonConfigAnchors {
    let mut root_keys = std::collections::HashSet::new();
    let mut table_headers = std::collections::HashSet::new();
    let mut in_table = false;

    for line in common_config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_table = true;
            table_headers.insert(trimmed.to_string());
            continue;
        }
        if !in_table {
            if let Some(key) = toml_key_from_line(trimmed) {
                root_keys.insert(key.to_string());
            }
        }
    }

    CommonConfigAnchors {
        root_keys,
        table_headers,
    }
}

fn toml_key_from_line(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty() { None } else { Some(key) }
}

fn ensure_text_newline(value: &str) -> String {
    if value.trim().is_empty() {
        String::new()
    } else {
        format!("{}\n", value.trim_end())
    }
}

#[tauri::command]
pub async fn load_provider_sync_targets() -> CommandResult<Value> {
    let settings = SettingsStore::default().load().unwrap_or_default();
    let result =
        tauri::async_runtime::spawn_blocking(|| codex_plus_data::load_provider_sync_targets(None))
            .await
            .map_err(|error| anyhow::anyhow!("provider target discovery task failed: {error}"));
    match result {
        Ok(mut targets) => {
            let manual = settings
                .provider_sync_manual_providers
                .iter()
                .chain(settings.provider_sync_saved_providers.iter())
                .filter_map(|value| {
                    let trimmed = value.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                })
                .collect::<Vec<_>>();
            merge_manual_provider_sync_targets(&mut targets, &manual, &settings);
            ok(
                "Provider 同步目标已加载。",
                serde_json::to_value(targets).unwrap_or_else(|_| json!({})),
            )
        }
        Err(error) => failed(&format!("Provider 同步目标加载失败：{error}"), json!({})),
    }
}

fn merge_manual_provider_sync_targets(
    targets: &mut codex_plus_data::ProviderSyncTargetList,
    manual: &[String],
    settings: &BackendSettings,
) {
    for id in manual {
        if let Some(existing) = targets.targets.iter_mut().find(|target| target.id == *id) {
            if !existing
                .sources
                .contains(&codex_plus_data::ProviderSyncTargetSource::Manual)
            {
                existing
                    .sources
                    .push(codex_plus_data::ProviderSyncTargetSource::Manual);
                existing.sources.sort();
            }
            existing.is_manual = settings.provider_sync_manual_providers.contains(id);
            existing.is_saved = settings.provider_sync_saved_providers.contains(id);
        } else {
            targets
                .targets
                .push(codex_plus_data::ProviderSyncTargetOption {
                    id: id.clone(),
                    sources: vec![codex_plus_data::ProviderSyncTargetSource::Manual],
                    is_current_provider: *id == targets.current_provider,
                    is_manual: settings.provider_sync_manual_providers.contains(id),
                    is_saved: settings.provider_sync_saved_providers.contains(id),
                });
        }
    }
    targets.targets.sort_by(|left, right| {
        right
            .is_current_provider
            .cmp(&left.is_current_provider)
            .then_with(|| left.id.cmp(&right.id))
    });
}

#[tauri::command]
pub async fn sync_providers_now(target_provider: Option<String>) -> CommandResult<Value> {
    let target_provider = target_provider
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let target_for_settings = target_provider.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        codex_plus_data::run_provider_sync_with_target(None, target_provider.as_deref())
    })
    .await
    .map_err(|error| anyhow::anyhow!("provider sync task failed: {error}"));
    match result {
        Ok(sync) => {
            if is_success_sync_status(&sync.status) {
                persist_provider_sync_selection(
                    target_for_settings
                        .as_deref()
                        .unwrap_or(&sync.target_provider),
                );
            }
            ok(
                &format!(
                    "供应商已同步一次：{} 个会话文件，{} 行索引，跳过 {} 个占用文件。",
                    sync.changed_session_files,
                    sync.sqlite_rows_updated,
                    sync.skipped_locked_rollout_files.len()
                ),
                json!({
                    "syncStatus": sync.status,
                    "targetProvider": sync.target_provider,
                    "changedSessionFiles": sync.changed_session_files,
                    "skippedLockedRolloutFiles": sync.skipped_locked_rollout_files,
                    "sqliteRowsUpdated": sync.sqlite_rows_updated,
                    "sqliteProviderRowsUpdated": sync.sqlite_provider_rows_updated,
                    "sqliteUserEventRowsUpdated": sync.sqlite_user_event_rows_updated,
                    "sqliteCwdRowsUpdated": sync.sqlite_cwd_rows_updated,
                    "updatedWorkspaceRoots": sync.updated_workspace_roots,
                    "encryptedContentWarning": sync.encrypted_content_warning,
                    "backupDir": sync.backup_dir,
                    "syncMessage": sync.message,
                }),
            )
        }
        Err(error) => failed(&format!("供应商同步失败：{error}"), json!({})),
    }
}

fn is_success_sync_status(status: &codex_plus_data::ProviderSyncStatus) -> bool {
    matches!(status, codex_plus_data::ProviderSyncStatus::Synced)
}

fn persist_provider_sync_selection(provider: &str) {
    let trimmed = provider.trim();
    if trimmed.is_empty() {
        return;
    }
    let store = SettingsStore::default();
    let mut settings = store.load().unwrap_or_default();
    settings.provider_sync_last_selected_provider = trimmed.to_string();
    if !settings
        .provider_sync_saved_providers
        .iter()
        .any(|item| item == trimmed)
    {
        settings
            .provider_sync_saved_providers
            .push(trimmed.to_string());
    }
    settings.provider_sync_saved_providers =
        normalize_provider_sync_provider_list(settings.provider_sync_saved_providers);
    let _ = store.save(&settings);
}

#[tauri::command]
pub async fn load_ads() -> CommandResult<AdsPayload> {
    match codex_plus_core::ads::fetch_ad_list().await {
        Ok(payload) => ok("推荐内容已加载。", ads_payload(payload)),
        Err(error) => failed(
            &format!("推荐内容加载失败：{error}"),
            AdsPayload {
                version: 1,
                ads: Vec::new(),
            },
        ),
    }
}

#[tauri::command]
pub async fn refresh_script_market() -> CommandResult<ScriptMarketPayload> {
    match script_market::fetch_market_manifest(script_market::DEFAULT_MARKET_INDEX_URL).await {
        Ok(manifest) => ok(
            "脚本市场已刷新。",
            script_market_payload_from_manifest(&manifest, "ok", "脚本市场已刷新。"),
        ),
        Err(error) => failed(
            &format!("脚本市场加载失败：{error}"),
            failed_script_market_payload(&format!("脚本市场加载失败：{error}")),
        ),
    }
}

#[tauri::command]
pub async fn install_market_script(id: String) -> CommandResult<ScriptMarketPayload> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return failed(
            "脚本 id 不能为空。",
            failed_script_market_payload("脚本 id 不能为空。"),
        );
    }
    let manifest =
        match script_market::fetch_market_manifest(script_market::DEFAULT_MARKET_INDEX_URL).await {
            Ok(manifest) => manifest,
            Err(error) => {
                return failed(
                    &format!("脚本市场加载失败：{error}"),
                    failed_script_market_payload(&format!("脚本市场加载失败：{error}")),
                );
            }
        };
    let Some(script) = manifest.scripts.iter().find(|script| script.id == trimmed) else {
        return failed(
            "市场清单中未找到该脚本。",
            script_market_payload_from_manifest(&manifest, "failed", "市场清单中未找到该脚本。"),
        );
    };
    let manager = default_user_script_manager();
    match script_market::install_market_script(&manager, script).await {
        Ok(()) => ok(
            "脚本已安装。",
            script_market_payload_from_manifest(&manifest, "ok", "脚本已安装。"),
        ),
        Err(error) => failed(
            &format!("安装脚本失败：{error}"),
            script_market_payload_from_manifest(
                &manifest,
                "failed",
                &format!("安装脚本失败：{error}"),
            ),
        ),
    }
}

#[tauri::command]
pub fn set_user_script_enabled(key: String, enabled: bool) -> CommandResult<SettingsPayload> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return failed("脚本 key 不能为空。", fallback_settings_payload());
    }
    let manager = default_user_script_manager();
    match manager.set_script_enabled(trimmed, enabled) {
        Ok(_) => settings_payload(
            if enabled {
                "脚本已启用。"
            } else {
                "脚本已禁用。"
            },
            "脚本启停失败",
        ),
        Err(error) => failed(
            &format!("脚本启停失败：{error}"),
            fallback_settings_payload(),
        ),
    }
}

#[tauri::command]
pub fn delete_user_script(key: String) -> CommandResult<SettingsPayload> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return failed("脚本 key 不能为空。", fallback_settings_payload());
    }
    let manager = default_user_script_manager();
    match manager.delete_user_script(trimmed) {
        Ok(_) => settings_payload("脚本已删除。", "脚本删除失败"),
        Err(error) => failed(
            &format!("脚本删除失败：{error}"),
            fallback_settings_payload(),
        ),
    }
}

#[tauri::command]
pub fn open_external_url(url: String) -> CommandResult<Value> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return failed("只允许打开 http 或 https 链接。", json!({}));
    }
    match open_url(trimmed) {
        Ok(()) => ok("已在系统浏览器打开链接。", json!({ "url": trimmed })),
        Err(error) => failed(&format!("打开链接失败：{error}"), json!({ "url": trimmed })),
    }
}

#[tauri::command]
pub async fn install_entrypoints() -> InstallActionResult {
    tauri::async_runtime::spawn_blocking(install::install_entrypoints)
        .await
        .unwrap_or_else(|error| install_background_failure("安装入口", error))
}

#[tauri::command]
pub async fn uninstall_entrypoints(options: InstallOptions) -> InstallActionResult {
    tauri::async_runtime::spawn_blocking(move || install::uninstall_entrypoints(options))
        .await
        .unwrap_or_else(|error| install_background_failure("卸载入口", error))
}

#[tauri::command]
pub async fn repair_shortcuts() -> InstallActionResult {
    tauri::async_runtime::spawn_blocking(install::repair_shortcuts)
        .await
        .unwrap_or_else(|error| install_background_failure("修复快捷方式", error))
}

#[tauri::command]
pub fn plugin_marketplace_status() -> CommandResult<PluginMarketplaceStatusPayload> {
    let home = codex_plus_core::codex_home::default_codex_home_dir();
    let status = codex_plus_core::plugin_marketplace::openai_curated_marketplace_status(&home);
    ok(
        if status.needs_repair() {
            "插件市场需要初始化或注册。"
        } else {
            "插件市场已可用。"
        },
        PluginMarketplaceStatusPayload {
            codex_home: home.to_string_lossy().to_string(),
            marketplace_root: status
                .marketplace_root
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            config_registered: status.config_registered,
            needs_repair: status.needs_repair(),
        },
    )
}

#[tauri::command]
pub async fn repair_plugin_marketplace() -> CommandResult<PluginMarketplaceRepairPayload> {
    let home = codex_plus_core::codex_home::default_codex_home_dir();
    match codex_plus_core::plugin_marketplace::initialize_openai_curated_marketplace_and_configure(
        &home,
    )
    .await
    {
        Ok(result) => ok(
            if result.initialized {
                "插件市场已从 openai/plugins 初始化并注册。"
            } else if result.configured {
                "已注册本地插件市场。"
            } else {
                "插件市场已可用，无需修复。"
            },
            PluginMarketplaceRepairPayload {
                codex_home: home.to_string_lossy().to_string(),
                marketplace_root:
                    codex_plus_core::plugin_marketplace::openai_curated_marketplace_status(&home)
                        .marketplace_root
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string()),
                initialized: result.initialized,
                configured: result.configured,
                needs_repair: false,
            },
        ),
        Err(error) => failed(
            &format!("插件市场修复失败：{error}"),
            PluginMarketplaceRepairPayload {
                codex_home: home.to_string_lossy().to_string(),
                marketplace_root:
                    codex_plus_core::plugin_marketplace::openai_curated_marketplace_status(&home)
                        .marketplace_root
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string()),
                initialized: false,
                configured: false,
                needs_repair: true,
            },
        ),
    }
}

#[tauri::command]
pub fn remote_plugin_marketplace_status() -> CommandResult<RemotePluginMarketplacePayload> {
    let home = codex_plus_core::codex_home::default_codex_home_dir();
    let status =
        codex_plus_core::plugin_marketplace::openai_curated_remote_marketplace_status(&home);
    let (plugin_count, skill_count) =
        remote_plugin_marketplace_counts(status.marketplace_root.as_deref());
    ok(
        if status.needs_repair() {
            "官方远端插件缓存需要释放或注册。"
        } else {
            "官方远端插件缓存已可用。"
        },
        RemotePluginMarketplacePayload {
            codex_home: home.to_string_lossy().to_string(),
            marketplace_root: status
                .marketplace_root
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            config_registered: status.config_registered,
            needs_repair: status.needs_repair(),
            plugin_count,
            skill_count,
        },
    )
}

#[tauri::command]
pub fn repair_remote_plugin_marketplace() -> CommandResult<RemotePluginMarketplacePayload> {
    let home = codex_plus_core::codex_home::default_codex_home_dir();
    match codex_plus_core::plugin_marketplace::ensure_openai_curated_remote_marketplace_available(
        &home,
    ) {
        Ok(result) => {
            let status =
                codex_plus_core::plugin_marketplace::openai_curated_remote_marketplace_status(
                    &home,
                );
            let (plugin_count, skill_count) =
                remote_plugin_marketplace_counts(status.marketplace_root.as_deref());
            ok(
                if result.initialized {
                    "已释放并注册内置官方远端插件缓存。"
                } else if result.configured {
                    "已注册官方远端插件缓存。"
                } else {
                    "官方远端插件缓存已可用，无需修复。"
                },
                RemotePluginMarketplacePayload {
                    codex_home: home.to_string_lossy().to_string(),
                    marketplace_root: status
                        .marketplace_root
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string()),
                    config_registered: status.config_registered,
                    needs_repair: status.needs_repair(),
                    plugin_count,
                    skill_count,
                },
            )
        }
        Err(error) => {
            let status =
                codex_plus_core::plugin_marketplace::openai_curated_remote_marketplace_status(
                    &home,
                );
            let (plugin_count, skill_count) =
                remote_plugin_marketplace_counts(status.marketplace_root.as_deref());
            failed(
                &format!("官方远端插件缓存修复失败：{error}"),
                RemotePluginMarketplacePayload {
                    codex_home: home.to_string_lossy().to_string(),
                    marketplace_root: status
                        .marketplace_root
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string()),
                    config_registered: status.config_registered,
                    needs_repair: status.needs_repair(),
                    plugin_count,
                    skill_count,
                },
            )
        }
    }
}

fn remote_plugin_marketplace_counts(root: Option<&Path>) -> (usize, usize) {
    let Some(root) = root else {
        return (0, 0);
    };
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    let plugin_count = std::fs::read_to_string(&marketplace_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|marketplace| {
            marketplace
                .get("plugins")
                .and_then(Value::as_array)
                .map(Vec::len)
        })
        .unwrap_or(0);
    let skill_count = count_skill_files(&root.join("plugins")).unwrap_or(0);
    (plugin_count, skill_count)
}

fn count_skill_files(root: &Path) -> std::io::Result<usize> {
    if !root.is_dir() {
        return Ok(0);
    }
    let mut total = 0;
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            total += count_skill_files(&path)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
            total += 1;
        }
    }
    Ok(total)
}

#[tauri::command]
pub async fn check_update() -> CommandResult<Value> {
    match codex_plus_core::update::check_for_update(codex_plus_core::version::VERSION).await {
        Ok(update) => {
            let status = if update.update_available {
                "ok"
            } else {
                "not_checked"
            };
            CommandResult {
                status: status.to_string(),
                message: if update.update_available {
                    "发现可用更新。".to_string()
                } else {
                    "当前已是最新版本。".to_string()
                },
                payload: json!({
                    "currentVersion": update.current_version,
                    "latestVersion": update.latest_version,
                    "releaseSummary": update.release_summary,
                    "assetName": update.asset_name,
                    "assetUrl": update.asset_url,
                    "updateAvailable": update.update_available,
                    "progress": 0
                }),
            }
        }
        Err(error) => failed(
            &format!("检查更新失败：{error}"),
            json!({
                "currentVersion": codex_plus_core::version::VERSION,
                "latestVersion": Value::Null,
                "releaseSummary": "",
                "assetName": Value::Null,
                "assetUrl": Value::Null,
                "updateAvailable": false,
                "progress": 0
            }),
        ),
    }
}

#[tauri::command]
pub async fn perform_update(
    release: Option<codex_plus_core::update::Release>,
) -> CommandResult<Value> {
    let Some(release) = release else {
        return failed(
            "请先检查更新并选择可下载的 Release asset。",
            json!({
                "currentVersion": codex_plus_core::version::VERSION,
                "progress": 0
            }),
        );
    };
    let download_dir = codex_plus_core::paths::default_app_state_dir().join("updates");
    match codex_plus_core::update::perform_update(&release, &download_dir).await {
        Ok(result) => ok(
            "安装包已下载并启动，请按安装向导完成更新。",
            json!({
                "currentVersion": codex_plus_core::version::VERSION,
                "latestVersion": result.release.version,
                "releaseSummary": result.release.body,
                "installedPath": result.installer_path.to_string_lossy(),
                "launched": result.launched,
                "progress": 100
            }),
        ),
        Err(error) => failed(
            &format!("安装更新失败：{error}"),
            json!({
                "currentVersion": codex_plus_core::version::VERSION,
                "latestVersion": release.version,
                "releaseSummary": release.body,
                "progress": 0
            }),
        ),
    }
}

#[tauri::command]
pub fn load_watcher_state() -> CommandResult<WatcherPayload> {
    ok("watcher 状态已加载。", watcher_payload())
}

#[tauri::command]
pub fn install_watcher() -> CommandResult<WatcherPayload> {
    let launcher_path =
        codex_plus_core::install::companion_binary_path(codex_plus_core::install::SILENT_BINARY);
    match codex_plus_core::watcher::install_watcher(&launcher_path, default_debug_port()) {
        Ok(()) => ok("watcher 已安装。", watcher_payload()),
        Err(error) => failed(&format!("安装 watcher 失败：{error}"), watcher_payload()),
    }
}

#[tauri::command]
pub fn uninstall_watcher() -> CommandResult<WatcherPayload> {
    match codex_plus_core::watcher::uninstall_watcher() {
        Ok(()) => ok("watcher 已移除。", watcher_payload()),
        Err(error) => failed(&format!("移除 watcher 失败：{error}"), watcher_payload()),
    }
}

#[tauri::command]
pub fn enable_watcher() -> CommandResult<WatcherPayload> {
    match codex_plus_core::watcher::enable_watcher() {
        Ok(()) => ok("watcher 已启用。", watcher_payload()),
        Err(error) => failed(&format!("启用 watcher 失败：{error}"), watcher_payload()),
    }
}

#[tauri::command]
pub fn disable_watcher() -> CommandResult<WatcherPayload> {
    match codex_plus_core::watcher::disable_watcher() {
        Ok(()) => ok("watcher 已禁用。", watcher_payload()),
        Err(error) => failed(&format!("禁用 watcher 失败：{error}"), watcher_payload()),
    }
}

#[tauri::command]
pub fn read_latest_logs(request: LogRequest) -> CommandResult<LogsPayload> {
    let path = codex_plus_core::paths::default_diagnostic_log_path();
    match read_tail(&path, request.lines) {
        Ok(text) => ok(
            "日志已读取。",
            LogsPayload {
                path: path.to_string_lossy().to_string(),
                text,
                lines: request.lines,
            },
        ),
        Err(error) => failed(
            &format!("读取日志失败：{error}"),
            LogsPayload {
                path: path.to_string_lossy().to_string(),
                text: String::new(),
                lines: request.lines,
            },
        ),
    }
}

#[tauri::command]
pub fn copy_diagnostics() -> CommandResult<DiagnosticsPayload> {
    ok(
        "诊断报告已生成。",
        DiagnosticsPayload {
            report: diagnostics_report(),
        },
    )
}

#[tauri::command]
pub fn reset_settings() -> CommandResult<SettingsPayload> {
    let settings = BackendSettings::default();
    match SettingsStore::default().save(&settings) {
        Ok(()) => settings_payload("设置已重置为默认值。", "设置重置后重新读取失败"),
        Err(error) => failed(
            &format!("重置设置失败：{error}"),
            SettingsPayload {
                settings,
                settings_path: codex_plus_core::paths::default_settings_path()
                    .to_string_lossy()
                    .to_string(),
                user_scripts: user_script_inventory(),
            },
        ),
    }
}

#[tauri::command]
pub fn reset_image_overlay_settings() -> CommandResult<SettingsPayload> {
    let store = SettingsStore::default();
    let mut settings = store.load().unwrap_or_default();
    let defaults = BackendSettings::default();
    settings.codex_app_image_overlay_enabled = defaults.codex_app_image_overlay_enabled;
    settings.codex_app_image_overlay_path = defaults.codex_app_image_overlay_path;
    settings.codex_app_image_overlay_opacity = defaults.codex_app_image_overlay_opacity;
    settings.codex_app_image_overlay_fit_mode = defaults.codex_app_image_overlay_fit_mode;
    let settings = normalize_settings_before_save(settings);
    match store.save(&settings) {
        Ok(()) => settings_payload("图片覆盖层设置已重置。", "图片覆盖层重置后重新读取失败"),
        Err(error) => failed(
            &format!("重置图片覆盖层失败：{error}"),
            SettingsPayload {
                settings,
                settings_path: codex_plus_core::paths::default_settings_path()
                    .to_string_lossy()
                    .to_string(),
                user_scripts: user_script_inventory(),
            },
        ),
    }
}

#[tauri::command]
pub fn relay_status() -> CommandResult<RelayPayload> {
    let status = codex_plus_core::relay_config::default_relay_status();
    let message = if status.authenticated {
        "已检测到 ChatGPT 登录状态。"
    } else {
        "未检测到 ChatGPT 登录状态，请先在 Codex/ChatGPT 中正常登录。"
    };
    ok(message, relay_payload(status, None))
}

#[tauri::command]
pub fn read_relay_files() -> CommandResult<RelayFilesPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    match relay_files_payload_from_home(&home) {
        Ok(payload) => ok("配置文件内容已读取。", payload),
        Err(error) => failed(
            &format!("读取配置文件失败：{error}"),
            RelayFilesPayload {
                config_path: home.join("config.toml").to_string_lossy().to_string(),
                auth_path: home.join("auth.json").to_string_lossy().to_string(),
                config_contents: String::new(),
                auth_contents: String::new(),
            },
        ),
    }
}

#[tauri::command]
pub fn check_env_conflicts() -> CommandResult<EnvConflictsPayload> {
    let conflicts = codex_plus_core::env_conflicts::detect_env_conflicts();
    let message = if conflicts.is_empty() {
        "未检测到会覆盖 Codex 供应商配置的 OPENAI 环境变量。"
    } else {
        "检测到可能覆盖 Codex 供应商配置的 OPENAI 环境变量。"
    };
    ok(message, EnvConflictsPayload { conflicts })
}

#[tauri::command]
pub fn check_relay_environment() -> CommandResult<RelayEnvironmentReport> {
    let report = codex_plus_core::relay_environment::inspect_relay_environment();
    let message = if report.all_passed() {
        "中转站环境配置检测全部通过。"
    } else {
        "检测到可能影响中转站配置的环境问题。"
    };
    ok(message, report)
}

#[tauri::command]
pub fn remove_env_conflicts(
    request: RemoveEnvConflictsRequest,
) -> CommandResult<RemoveEnvConflictsPayload> {
    let backup_dir = codex_plus_core::paths::default_app_state_dir().join("backups");
    match codex_plus_core::env_conflicts::remove_env_conflicts(&request.names, backup_dir) {
        Ok(result) => {
            let remaining = codex_plus_core::env_conflicts::detect_env_conflicts();
            ok(
                "环境变量已按确认项删除；重新启动 Codex 后生效。",
                RemoveEnvConflictsPayload {
                    removed: result.removed,
                    backup_path: result.backup_path,
                    remaining,
                },
            )
        }
        Err(error) => failed(
            &format!("删除环境变量失败：{error}"),
            RemoveEnvConflictsPayload {
                removed: Vec::new(),
                backup_path: None,
                remaining: codex_plus_core::env_conflicts::detect_env_conflicts(),
            },
        ),
    }
}

#[tauri::command]
pub fn save_relay_file(request: SaveRelayFileRequest) -> CommandResult<RelayFilesPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    match save_relay_file_in_home(&home, &request.kind, &request.contents)
        .and_then(|_| relay_files_payload_from_home(&home))
    {
        Ok(payload) => ok("配置文件已保存。", payload),
        Err(error) => failed(
            &format!("保存配置文件失败：{error}"),
            relay_files_payload_from_home(&home).unwrap_or_else(|_| RelayFilesPayload {
                config_path: home.join("config.toml").to_string_lossy().to_string(),
                auth_path: home.join("auth.json").to_string_lossy().to_string(),
                config_contents: String::new(),
                auth_contents: String::new(),
            }),
        ),
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayProfileSwitchRequest {
    pub settings: BackendSettings,
    #[serde(default)]
    pub previous_active_relay_id: String,
}

#[tauri::command]
pub fn switch_relay_profile(
    request: RelayProfileSwitchRequest,
) -> CommandResult<RelaySwitchPayload> {
    let Ok(_guard) = relay_switch_mutex().lock() else {
        let status = codex_plus_core::relay_config::default_relay_status();
        return failed(
            "供应商切换锁已损坏，请重启管理器后再试。",
            relay_switch_payload(
                SettingsStore::default().load().unwrap_or_default(),
                status,
                None,
            ),
        );
    };
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let store = SettingsStore::default();
    let previous_active_relay_id = request.previous_active_relay_id;
    let settings = normalize_settings_before_save(request.settings);
    log_manager_event(
        "manager.switch_relay_profile.start",
        json!({
            "previousActiveRelayId": previous_active_relay_id,
            "targetRelayId": settings.active_relay_id
        }),
    );
    match codex_plus_core::relay_switch::switch_relay_profile_in_home(
        &store,
        &home,
        settings,
        &previous_active_relay_id,
    ) {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_manager_event(
                "manager.switch_relay_profile.ok",
                json!({
                    "targetRelayId": result.settings.active_relay_id,
                    "configured": status.configured,
                    "backupPath": result.backup_path.as_ref()
                }),
            );
            ok(
                "供应商已切换。",
                relay_switch_payload(result.settings, status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            let settings = store.load().unwrap_or_default();
            log_manager_event(
                "manager.switch_relay_profile.failed",
                json!({
                    "previousActiveRelayId": previous_active_relay_id,
                    "activeRelayId": settings.active_relay_id,
                    "error": error.to_string()
                }),
            );
            failed(
                &format!("供应商切换失败：{error}"),
                relay_switch_payload(settings, status, None),
            )
        }
    }
}

#[tauri::command]
pub fn write_diagnostic_event(event: String, detail: Value) -> CommandResult<Value> {
    let event = sanitize_manager_event(&event);
    match codex_plus_core::diagnostic_log::append_diagnostic_log(&event, detail) {
        Ok(()) => ok("诊断日志已写入。", json!({})),
        Err(error) => failed(&format!("写入诊断日志失败：{error}"), json!({})),
    }
}

#[tauri::command]
pub fn backfill_relay_profile_from_live(
    request: BackfillRelayProfileRequest,
) -> CommandResult<SettingsBackfillPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let mut settings = request.settings;
    let requested_profile_id = request.profile_id.clone();
    log_manager_event(
        "manager.backfill_relay_profile_from_live.start",
        json!({
            "profileId": requested_profile_id,
            "activeRelayId": settings.active_relay_id
        }),
    );
    let Some(profile) = settings
        .relay_profiles
        .iter_mut()
        .find(|profile| profile.id == request.profile_id)
    else {
        log_manager_event(
            "manager.backfill_relay_profile_from_live.missing_profile",
            json!({
                "profileId": requested_profile_id
            }),
        );
        return failed(
            "当前供应商已不在配置列表中，已停止切换以避免覆盖用户改动。",
            SettingsBackfillPayload { settings },
        );
    };

    match codex_plus_core::relay_config::backfill_relay_profile_from_home_with_common(
        &home,
        profile,
        &mut settings.relay_context_config_contents,
    ) {
        Ok(()) => {
            log_manager_event(
                "manager.backfill_relay_profile_from_live.ok",
                json!({
                    "profileId": requested_profile_id
                }),
            );
            ok(
                "当前供应商配置已从 live 文件回填。",
                SettingsBackfillPayload { settings },
            )
        }
        Err(error) => {
            log_manager_event(
                "manager.backfill_relay_profile_from_live.failed",
                json!({
                    "profileId": requested_profile_id,
                    "error": error.to_string()
                }),
            );
            failed(
                &format!("回填当前供应商配置失败：{error}"),
                SettingsBackfillPayload { settings },
            )
        }
    }
}

#[tauri::command]
pub fn list_context_entries(
    request: ContextSettingsRequest,
) -> CommandResult<ContextEntriesPayload> {
    match codex_plus_core::relay_config::list_context_entries_from_common_config(
        &request.settings.relay_context_config_contents,
    ) {
        Ok(entries) => ok(
            "工具与插件列表已读取。",
            ContextEntriesPayload {
                settings: request.settings,
                entries,
            },
        ),
        Err(error) => failed(
            &format!("读取工具与插件列表失败：{error}"),
            ContextEntriesPayload {
                settings: request.settings,
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn read_live_context_entries() -> CommandResult<LiveContextEntriesPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let config_path = home.join("config.toml");
    let config = read_optional_text_file(&config_path).unwrap_or_default();
    match codex_plus_core::relay_config::list_context_entries_from_common_config(&config) {
        Ok(entries) => ok(
            "live 工具与插件已读取。",
            LiveContextEntriesPayload { entries },
        ),
        Err(error) => failed(
            &format!("读取 live 工具与插件失败：{error}"),
            LiveContextEntriesPayload {
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn upsert_context_entry(request: ContextEntryRequest) -> CommandResult<ContextEntriesPayload> {
    let mut settings = request.settings;
    match codex_plus_core::relay_config::upsert_context_entry_in_common_config(
        &settings.relay_context_config_contents,
        &request.kind,
        &request.id,
        &request.toml_body,
    ) {
        Ok(common) => {
            settings.relay_context_config_contents = common;
            list_context_entries(ContextSettingsRequest { settings })
        }
        Err(error) => failed(
            &format!("保存工具与插件失败：{error}"),
            ContextEntriesPayload {
                settings,
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn sync_live_context_entries(
    request: ContextSettingsRequest,
) -> CommandResult<LiveContextEntriesPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let config_path = home.join("config.toml");
    let current_config = match read_optional_text_file(&config_path) {
        Ok(config) => config,
        Err(error) => {
            return failed(
                &format!("读取 live config.toml 失败：{error}"),
                LiveContextEntriesPayload {
                    entries: empty_context_entries(),
                },
            );
        }
    };
    let updated_config = match codex_plus_core::relay_config::sync_live_config_context_entries(
        &current_config,
        &request.settings.relay_context_config_contents,
    ) {
        Ok(config) => config,
        Err(error) => {
            return failed(
                &format!("同步 live 工具与插件失败：{error}"),
                LiveContextEntriesPayload {
                    entries: empty_context_entries(),
                },
            );
        }
    };
    if let Some(parent) = config_path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return failed(
                &format!("创建 Codex 配置目录失败：{error}"),
                LiveContextEntriesPayload {
                    entries: empty_context_entries(),
                },
            );
        }
    }
    if let Err(error) = std::fs::write(&config_path, &updated_config) {
        return failed(
            &format!("写入 live config.toml 失败：{error}"),
            LiveContextEntriesPayload {
                entries: empty_context_entries(),
            },
        );
    }
    match codex_plus_core::relay_config::list_context_entries_from_common_config(&updated_config) {
        Ok(entries) => ok(
            "live 工具与插件已同步。",
            LiveContextEntriesPayload { entries },
        ),
        Err(error) => failed(
            &format!("读取同步后的 live 工具与插件失败：{error}"),
            LiveContextEntriesPayload {
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn delete_context_entry(request: ContextDeleteRequest) -> CommandResult<ContextEntriesPayload> {
    let mut settings = request.settings;
    match codex_plus_core::relay_config::delete_context_entry_from_common_config(
        &settings.relay_context_config_contents,
        &request.kind,
        &request.id,
    ) {
        Ok(common) => {
            settings.relay_context_config_contents = common;
            list_context_entries(ContextSettingsRequest { settings })
        }
        Err(error) => failed(
            &format!("删除工具与插件失败：{error}"),
            ContextEntriesPayload {
                settings,
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn extract_relay_common_config(
    request: ExtractRelayCommonConfigRequest,
) -> CommandResult<ExtractRelayCommonConfigPayload> {
    match codex_plus_core::relay_config::extract_common_config_from_config(&request.config_contents)
        .and_then(|common_config_contents| {
            let profile_config_contents =
                codex_plus_core::relay_config::strip_common_config_from_config(
                    &request.config_contents,
                    &common_config_contents,
                )?;
            Ok(ExtractRelayCommonConfigPayload {
                common_config_contents,
                profile_config_contents,
            })
        }) {
        Ok(payload) => ok("通用配置已按兼容切换规则提取。", payload),
        Err(error) => failed(
            &format!("提取通用配置失败：{error}"),
            ExtractRelayCommonConfigPayload {
                common_config_contents: String::new(),
                profile_config_contents: request.config_contents,
            },
        ),
    }
}

#[tauri::command]
pub async fn test_relay_profile(profile: RelayProfile) -> CommandResult<RelayProfileTestPayload> {
    let profile_name = if profile.name.trim().is_empty() {
        "未命名供应商"
    } else {
        profile.name.trim()
    };
    let settings = SettingsStore::default().load().unwrap_or_default();
    let test_model: String = if !profile.test_model.trim().is_empty() {
        // 1. 使用者在該供應商明確填的測試模型
        profile.test_model.trim().to_string()
    } else {
        // 2. 該供應商自己 config.toml 裡的 model（避免串味）
        let from_profile = codex_plus_core::relay_config::relay_profile_model(&profile);
        if from_profile.trim().is_empty() {
            // 3. 最後才用全域預設
            settings.relay_test_model.trim().to_string()
        } else {
            from_profile
        }
    };
    match codex_plus_core::relay_config::test_relay_profile(&profile, &test_model).await {
        Ok(result) => {
            let status = if result.http_status < 400 {
                "ok"
            } else {
                "failed"
            };
            let preview = result.response_preview.trim();
            let detail = if preview.is_empty() {
                "响应内容为空".to_string()
            } else {
                format!("响应：{preview}")
            };
            CommandResult {
                status: status.to_string(),
                message: format!(
                    "已向「{profile_name}」用模型「{test_model}」发送 hi，HTTP {}。{detail}",
                    result.http_status
                ),
                payload: RelayProfileTestPayload {
                    http_status: result.http_status,
                    endpoint: result.endpoint,
                    response_preview: result.response_preview,
                },
            }
        }
        Err(error) => failed(
            &format!("测试「{profile_name}」失败：{error}"),
            RelayProfileTestPayload {
                http_status: 0,
                endpoint: String::new(),
                response_preview: String::new(),
            },
        ),
    }
}

#[tauri::command]
pub async fn measure_relay_latency(url: String) -> CommandResult<RelayLatencyPayload> {
    match codex_plus_core::relay_latency::measure_relay_latency(&url).await {
        Ok(measurement) => ok(
            "目标 URL 延迟检测完成。",
            RelayLatencyPayload {
                latency_ms: Some(measurement.latency_ms),
                http_status: Some(measurement.http_status),
            },
        ),
        Err(error) => failed(
            &format!("目标 URL 延迟检测失败：{error}"),
            RelayLatencyPayload {
                latency_ms: None,
                http_status: None,
            },
        ),
    }
}

#[tauri::command]
pub async fn test_stepwise_settings(
    settings: BackendSettings,
) -> CommandResult<StepwiseTestPayload> {
    match codex_plus_core::stepwise::test_connection(&settings).await {
        Ok(result) => {
            let error = result
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let item_count = result
                .get("items")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            if error.is_empty() {
                ok(
                    &format!("Stepwise 连接正常，测试返回 {item_count} 条建议。"),
                    StepwiseTestPayload { item_count, error },
                )
            } else {
                failed(
                    &format!("Stepwise 测试失败：{error}"),
                    StepwiseTestPayload { item_count, error },
                )
            }
        }
        Err(error) => failed(
            &format!("Stepwise 测试失败：{error}"),
            StepwiseTestPayload {
                item_count: 0,
                error: error.to_string(),
            },
        ),
    }
}

#[tauri::command]
pub async fn fetch_relay_profile_models(
    profile: RelayProfile,
) -> CommandResult<RelayProfileModelsPayload> {
    let profile_name = if profile.name.trim().is_empty() {
        "未命名供应商"
    } else {
        profile.name.trim()
    };
    match codex_plus_core::model_catalog::fetch_relay_profile_model_ids(&profile).await {
        Ok((models, endpoint)) => ok(
            &format!("已从「{profile_name}」获取 {} 个模型。", models.len()),
            RelayProfileModelsPayload { models, endpoint },
        ),
        Err(error) => failed(
            &format!("从「{profile_name}」获取模型失败：{error}"),
            RelayProfileModelsPayload {
                models: Vec::new(),
                endpoint: String::new(),
            },
        ),
    }
}

#[tauri::command]
pub async fn diagnose_relay_profile(profile: RelayProfile) -> CommandResult<ProviderDoctorPayload> {
    let profile_name = if profile.name.trim().is_empty() {
        "未命名供应商".to_string()
    } else {
        profile.name.trim().to_string()
    };
    let settings = SettingsStore::default().load().unwrap_or_default();
    let test_model = if !profile.test_model.trim().is_empty() {
        profile.test_model.trim().to_string()
    } else {
        let from_profile = codex_plus_core::relay_config::relay_profile_model(&profile);
        if from_profile.trim().is_empty() {
            settings.relay_test_model.trim().to_string()
        } else {
            from_profile
        }
    };
    let mut checks = Vec::new();

    if profile.relay_mode == codex_plus_core::settings::RelayMode::Official
        && !profile.official_mix_api_key
    {
        checks.push(ProviderDoctorCheck {
            id: "config".to_string(),
            title: "配置完整性".to_string(),
            status: "ok".to_string(),
            detail: "官方登录供应商不需要 Base URL / API Key。".to_string(),
        });
        let payload = ProviderDoctorPayload {
            profile_name,
            model: test_model,
            summary: "官方登录供应商无需 API 诊断。".to_string(),
            recommendation: "如果 Codex 官方账号可用，直接使用官方登录模式即可。".to_string(),
            checks,
        };
        return ok("Provider Doctor：官方登录供应商无需 API 诊断。", payload);
    }

    if codex_plus_core::relay_config::relay_profile_base_url(&profile)
        .trim()
        .is_empty()
        || codex_plus_core::relay_config::relay_profile_api_key(&profile)
            .trim()
            .is_empty()
    {
        checks.push(ProviderDoctorCheck {
            id: "config".to_string(),
            title: "配置完整性".to_string(),
            status: "failed".to_string(),
            detail: "Base URL 或 API Key 为空。".to_string(),
        });
        let payload = ProviderDoctorPayload {
            profile_name,
            model: test_model,
            summary: "配置不完整，无法发起上游诊断。".to_string(),
            recommendation: "先填写 Base URL 和 API Key；如果是官方账号，请切换到官方登录模式。"
                .to_string(),
            checks,
        };
        return failed("Provider Doctor：配置不完整。", payload);
    }

    checks.push(ProviderDoctorCheck {
        id: "config".to_string(),
        title: "配置完整性".to_string(),
        status: "ok".to_string(),
        detail: format!(
            "{} / {}",
            codex_plus_core::relay_config::relay_profile_base_url(&profile),
            match profile.protocol {
                codex_plus_core::settings::RelayProtocol::Responses => "Responses API",
                codex_plus_core::settings::RelayProtocol::ChatCompletions => "Chat Completions",
            }
        ),
    });

    match codex_plus_core::model_catalog::fetch_relay_profile_model_ids(&profile).await {
        Ok((models, endpoint)) => {
            let contains_model = !test_model.trim().is_empty()
                && models.iter().any(|model| model == test_model.trim());
            let status = if models.is_empty() {
                "failed"
            } else if contains_model || test_model.trim().is_empty() {
                "ok"
            } else {
                "warning"
            };
            let detail = if models.is_empty() {
                format!("{endpoint} 返回 0 个模型。")
            } else if contains_model || test_model.trim().is_empty() {
                format!("{endpoint} 返回 {} 个模型。", models.len())
            } else {
                format!(
                    "{endpoint} 返回 {} 个模型，但未看到测试模型「{}」。",
                    models.len(),
                    test_model
                )
            };
            checks.push(ProviderDoctorCheck {
                id: "models".to_string(),
                title: "模型列表".to_string(),
                status: status.to_string(),
                detail,
            });
        }
        Err(error) => checks.push(ProviderDoctorCheck {
            id: "models".to_string(),
            title: "模型列表".to_string(),
            status: "failed".to_string(),
            detail: error.to_string(),
        }),
    }

    match codex_plus_core::relay_config::test_relay_profile(&profile, &test_model).await {
        Ok(result) => {
            let status = if result.http_status < 400 {
                "ok"
            } else {
                "failed"
            };
            let preview = result.response_preview.trim();
            checks.push(ProviderDoctorCheck {
                id: "request".to_string(),
                title: "真实请求".to_string(),
                status: status.to_string(),
                detail: if preview.is_empty() {
                    format!(
                        "{} 返回 HTTP {}，响应内容为空。",
                        result.endpoint, result.http_status
                    )
                } else {
                    format!(
                        "{} 返回 HTTP {}：{}",
                        result.endpoint, result.http_status, preview
                    )
                },
            });
        }
        Err(error) => checks.push(ProviderDoctorCheck {
            id: "request".to_string(),
            title: "真实请求".to_string(),
            status: "failed".to_string(),
            detail: error.to_string(),
        }),
    }

    let failed_count = checks
        .iter()
        .filter(|check| check.status == "failed")
        .count();
    let warning_count = checks
        .iter()
        .filter(|check| check.status == "warning")
        .count();
    let status = if failed_count > 0 {
        "failed"
    } else if warning_count > 0 {
        "ok"
    } else {
        "ok"
    };
    let summary = if failed_count > 0 {
        format!("发现 {failed_count} 项失败，Codex 可能无法使用该供应商。")
    } else if warning_count > 0 {
        format!("基础连接可用，但有 {warning_count} 项需要确认。")
    } else {
        "供应商基础诊断通过。".to_string()
    };
    let recommendation = provider_doctor_recommendation(&checks);
    let message = format!("Provider Doctor：{summary}");
    CommandResult {
        status: status.to_string(),
        message,
        payload: ProviderDoctorPayload {
            profile_name,
            model: test_model,
            summary,
            recommendation,
            checks,
        },
    }
}

fn provider_doctor_recommendation(checks: &[ProviderDoctorCheck]) -> String {
    if checks
        .iter()
        .any(|check| check.id == "config" && check.status == "failed")
    {
        return "先补齐 Base URL 和 API Key；如果使用官方账号，请切换到官方登录模式。".to_string();
    }
    if checks
        .iter()
        .any(|check| check.id == "models" && check.status == "failed")
    {
        return "优先检查 Base URL 是否包含正确的 /v1 前缀，以及供应商是否支持 /v1/models。"
            .to_string();
    }
    if checks
        .iter()
        .any(|check| check.id == "request" && check.status == "failed")
    {
        return "优先检查测试模型名称、上游协议选择和 Key 权限；如果 Chat Completions 可用，请切到对应协议。".to_string();
    }
    if checks.iter().any(|check| check.status == "warning") {
        return "连接可用，但测试模型没有出现在模型列表里；建议改用上游返回的模型名。".to_string();
    }
    "可以作为 Codex 供应商使用；如果真实对话仍失败，请查看协议代理日志里的上游响应。".to_string()
}

#[tauri::command]
pub fn apply_relay_injection() -> CommandResult<RelayPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let settings = SettingsStore::default().load().unwrap_or_default();
    if !settings.relay_profiles_enabled {
        let status = codex_plus_core::relay_config::relay_status_from_home(&home);
        return failed(
            "供应商配置总开关已关闭，未写入 config.toml / auth.json。",
            relay_payload(status, None),
        );
    }
    let relay = settings.active_relay_profile();
    log_relay_apply_request("manager.apply_relay_injection", &settings, &relay);
    if settings.active_aggregate_relay_profile().is_some() {
        return apply_aggregate_relay_injection_to_home(&home);
    }
    if relay_has_complete_files(&relay) {
        return match codex_plus_core::relay_config::apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
            &home,
            &relay,
            &relay_combined_common_config(&settings),
            settings.computer_use_guard_enabled,
        ) {
            Ok(result) => {
                let status = codex_plus_core::relay_config::relay_status_from_home(&home);
                log_relay_apply_result(
                    "manager.apply_relay_injection.ok",
                    &relay,
                    &status,
                    result.backup_path.as_ref(),
                    None,
                );
                ok(
                    "已按兼容切换规则切换供应商。",
                    relay_payload(status, result.backup_path),
                )
            }
            Err(error) => {
                let status = codex_plus_core::relay_config::relay_status_from_home(&home);
                log_relay_apply_result(
                    "manager.apply_relay_injection.failed",
                    &relay,
                    &status,
                    None,
                    Some(error.to_string()),
                );
                failed(
                    &format!("切换完整中转配置失败：{error}"),
                    relay_payload(status, None),
                )
            }
        };
    }

    let auth = codex_plus_core::relay_config::chatgpt_auth_status_from_home(&home);
    if !auth.authenticated {
        let status = codex_plus_core::relay_config::relay_status_from_home(&home);
        log_relay_apply_result(
            "manager.apply_relay_injection.failed",
            &relay,
            &status,
            None,
            Some("未检测到 ChatGPT 登录状态".to_string()),
        );
        return failed(
            "未检测到 ChatGPT 登录状态，已停止写入中转配置。",
            relay_payload(status, None),
        );
    }

    match codex_plus_core::relay_config::apply_relay_config_to_home_with_protocol(
        &home,
        &relay.base_url,
        &relay.api_key,
        relay.protocol,
        codex_plus_core::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    ) {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_relay_apply_result(
                "manager.apply_relay_injection.ok",
                &relay,
                &status,
                result.backup_path.as_ref(),
                None,
            );
            ok(
                "中转配置已写入，密钥未在界面明文显示。",
                relay_payload(status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_relay_apply_result(
                "manager.apply_relay_injection.failed",
                &relay,
                &status,
                None,
                Some(error.to_string()),
            );
            failed(
                &format!("写入中转配置失败：{error}"),
                relay_payload(status, None),
            )
        }
    }
}

fn apply_aggregate_relay_injection_to_home(home: &Path) -> CommandResult<RelayPayload> {
    match codex_plus_core::relay_config::apply_relay_config_to_home_with_protocol(
        home,
        &codex_plus_core::protocol_proxy::local_responses_proxy_base_url(
            codex_plus_core::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
        ),
        "codex-plus-aggregate",
        codex_plus_core::settings::RelayProtocol::Responses,
        codex_plus_core::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    ) {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(home);
            ok(
                "聚合供应商配置已写入，真实请求会由本地代理按策略轮转。",
                relay_payload(status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(home);
            failed(
                &format!("写入聚合供应商配置失败：{error}"),
                relay_payload(status, None),
            )
        }
    }
}

#[tauri::command]
pub fn apply_pure_api_injection() -> CommandResult<RelayPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let settings = SettingsStore::default().load().unwrap_or_default();
    if !settings.relay_profiles_enabled {
        let status = codex_plus_core::relay_config::relay_status_from_home(&home);
        return failed(
            "供应商配置总开关已关闭，未写入 config.toml / auth.json。",
            relay_payload(status, None),
        );
    }
    let relay = settings.active_relay_profile();
    log_relay_apply_request("manager.apply_pure_api_injection", &settings, &relay);
    if relay_has_complete_files(&relay) {
        return match codex_plus_core::relay_config::apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
            &home,
            &relay,
            &relay_combined_common_config(&settings),
            settings.computer_use_guard_enabled,
        ) {
            Ok(result) => {
                let status = codex_plus_core::relay_config::relay_status_from_home(&home);
                log_relay_apply_result(
                    "manager.apply_pure_api_injection.ok",
                    &relay,
                    &status,
                    result.backup_path.as_ref(),
                    None,
                );
                if !status.configured {
                    return failed(
                        "纯 API 配置写入后未检测到完整 custom provider，请检查 config.toml 和供应商 API Key。",
                        relay_payload(status, result.backup_path),
                    );
                }
                ok(
                    "已按兼容切换规则切换供应商。",
                    relay_payload(status, result.backup_path),
                )
            }
            Err(error) => {
                let status = codex_plus_core::relay_config::relay_status_from_home(&home);
                log_relay_apply_result(
                    "manager.apply_pure_api_injection.failed",
                    &relay,
                    &status,
                    None,
                    Some(error.to_string()),
                );
                failed(
                    &format!("切换纯 API 配置失败：{error}"),
                    relay_payload(status, None),
                )
            }
        };
    }

    match codex_plus_core::relay_config::apply_pure_api_config_to_home_with_protocol(
        &home,
        &relay.base_url,
        &relay.api_key,
        relay.protocol,
        codex_plus_core::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    ) {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_relay_apply_result(
                "manager.apply_pure_api_injection.ok",
                &relay,
                &status,
                result.backup_path.as_ref(),
                None,
            );
            if !status.configured {
                return failed(
                    "纯 API 配置写入后未检测到完整 custom provider，请检查 config.toml 和供应商 API Key。",
                    relay_payload(status, result.backup_path),
                );
            }
            ok(
                "纯 API 模式已写入：config.toml 已写入 custom provider，auth.json 已切换为当前供应商。",
                relay_payload(status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_relay_apply_result(
                "manager.apply_pure_api_injection.failed",
                &relay,
                &status,
                None,
                Some(error.to_string()),
            );
            failed(
                &format!("写入纯 API 模式失败：{error}"),
                relay_payload(status, None),
            )
        }
    }
}

#[tauri::command]
pub fn clear_relay_injection() -> CommandResult<RelayPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let settings = SettingsStore::default().load().unwrap_or_default();
    let relay = settings.active_relay_profile();
    log_manager_event("manager.clear_relay_injection.start", json!({}));
    let auth_contents = (relay.relay_mode == codex_plus_core::settings::RelayMode::Official
        && !relay.official_mix_api_key
        && !relay.auth_contents.trim().is_empty())
    .then_some(relay.auth_contents.as_str());
    match codex_plus_core::relay_config::clear_relay_config_to_home_with_auth(&home, auth_contents)
    {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_manager_event(
                "manager.clear_relay_injection.ok",
                json!({
                    "configured": status.configured,
                    "backupPath": result.backup_path.as_ref()
                }),
            );
            ok(
                "已清除 custom 中转 API 模式，并切换到官方 ChatGPT 登录模式。",
                relay_payload(status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_manager_event(
                "manager.clear_relay_injection.failed",
                json!({
                    "configured": status.configured,
                    "error": error.to_string()
                }),
            );
            failed(
                &format!("清除中转配置失败：{error}"),
                relay_payload(status, None),
            )
        }
    }
}

fn relay_has_complete_files(relay: &codex_plus_core::settings::RelayProfile) -> bool {
    if relay.relay_mode == codex_plus_core::settings::RelayMode::Official
        && relay.official_mix_api_key
    {
        return !relay.config_contents.trim().is_empty();
    }
    !relay.config_contents.trim().is_empty() && !relay.auth_contents.trim().is_empty()
}

fn log_relay_apply_request(
    event: &str,
    settings: &BackendSettings,
    relay: &codex_plus_core::settings::RelayProfile,
) {
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        event,
        json!({
            "activeRelayId": settings.active_relay_id,
            "relayId": relay.id,
            "relayName": relay.name,
            "relayMode": relay.relay_mode,
            "protocol": relay.protocol,
            "baseUrl": relay.base_url,
            "hasConfigContents": !relay.config_contents.trim().is_empty(),
            "hasAuthContents": !relay.auth_contents.trim().is_empty(),
            "configContainsProxy": relay.config_contents.contains("127.0.0.1:57321")
        }),
    );
}

fn log_relay_apply_result(
    event: &str,
    relay: &codex_plus_core::settings::RelayProfile,
    status: &codex_plus_core::relay_config::RelayStatus,
    backup_path: Option<&String>,
    error: Option<String>,
) {
    log_manager_event(
        event,
        json!({
            "relayId": relay.id,
            "relayName": relay.name,
            "relayMode": relay.relay_mode,
            "protocol": relay.protocol,
            "configured": status.configured,
            "requiresOpenaiAuth": status.requires_openai_auth,
            "hasBearerToken": status.has_bearer_token,
            "backupPath": backup_path,
            "error": error
        }),
    );
}

fn log_manager_event(event: &str, detail: Value) {
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(event, detail);
}

fn sanitize_manager_event(event: &str) -> String {
    let suffix = event
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let suffix = suffix.trim_matches(['.', '_', '-']).trim();
    if suffix.is_empty() {
        "manager.ui.event".to_string()
    } else if suffix.starts_with("manager.") {
        suffix.to_string()
    } else {
        format!("manager.ui.{suffix}")
    }
}

fn relay_payload(
    status: codex_plus_core::relay_config::RelayStatus,
    backup_path: Option<String>,
) -> RelayPayload {
    RelayPayload {
        authenticated: status.authenticated,
        auth_source: status.auth_source,
        account_label: status.account_label,
        config_path: status.config_path,
        configured: status.configured,
        requires_openai_auth: status.requires_openai_auth,
        has_bearer_token: status.has_bearer_token,
        backup_path,
    }
}

fn relay_switch_payload(
    settings: BackendSettings,
    status: codex_plus_core::relay_config::RelayStatus,
    backup_path: Option<String>,
) -> RelaySwitchPayload {
    RelaySwitchPayload {
        settings,
        relay: relay_payload(status, backup_path),
        settings_path: codex_plus_core::paths::default_settings_path()
            .to_string_lossy()
            .to_string(),
        user_scripts: user_script_inventory(),
    }
}

fn relay_switch_mutex() -> &'static Mutex<()> {
    static RELAY_SWITCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    RELAY_SWITCH_LOCK.get_or_init(|| Mutex::new(()))
}

fn empty_context_entries() -> codex_plus_core::relay_config::CodexContextEntries {
    codex_plus_core::relay_config::CodexContextEntries {
        mcp_servers: Vec::new(),
        skills: Vec::new(),
        plugins: Vec::new(),
    }
}

fn relay_files_payload_from_home(home: &std::path::Path) -> anyhow::Result<RelayFilesPayload> {
    let config_path = home.join("config.toml");
    let auth_path = home.join("auth.json");
    Ok(RelayFilesPayload {
        config_path: config_path.to_string_lossy().to_string(),
        auth_path: auth_path.to_string_lossy().to_string(),
        config_contents: read_optional_text_file(&config_path)?,
        auth_contents: read_optional_text_file(&auth_path)?,
    })
}

fn save_relay_file_in_home(
    home: &std::path::Path,
    kind: &str,
    contents: &str,
) -> anyhow::Result<()> {
    let path = match kind {
        "config" => home.join("config.toml"),
        "auth" => home.join("auth.json"),
        other => anyhow::bail!("未知配置文件类型：{other}"),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(())
}

fn read_optional_text_file(path: &std::path::Path) -> anyhow::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn ads_payload(payload: Value) -> AdsPayload {
    AdsPayload {
        version: payload.get("version").and_then(Value::as_u64).unwrap_or(1),
        ads: payload
            .get("ads")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    }
}

fn open_url(url: &str) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        codex_plus_core::windows_open_url(url)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("启动系统浏览器失败：{error}"))
    }
}

fn settings_payload(message: &str, failure_context: &str) -> CommandResult<SettingsPayload> {
    match settings_payload_value() {
        Ok(payload) => ok(message, payload),
        Err((error, payload)) => failed(&format!("{failure_context}：{error}"), payload),
    }
}

fn settings_payload_value() -> Result<SettingsPayload, (anyhow::Error, SettingsPayload)> {
    let store = SettingsStore::default();
    let settings_path = codex_plus_core::paths::default_settings_path()
        .to_string_lossy()
        .to_string();
    match store.load() {
        Ok(settings) => Ok(SettingsPayload {
            settings,
            settings_path,
            user_scripts: user_script_inventory(),
        }),
        Err(error) => Err((
            error,
            SettingsPayload {
                settings: BackendSettings::default(),
                settings_path,
                user_scripts: user_script_inventory(),
            },
        )),
    }
}

fn fallback_settings_payload() -> SettingsPayload {
    SettingsPayload {
        settings: SettingsStore::default().load().unwrap_or_default(),
        settings_path: codex_plus_core::paths::default_settings_path()
            .to_string_lossy()
            .to_string(),
        user_scripts: user_script_inventory(),
    }
}

fn user_script_inventory() -> Value {
    default_user_script_manager()
        .inventory()
        .unwrap_or_else(|error| {
            json!({
                "enabled": true,
                "scripts": [],
                "error": error.to_string()
            })
        })
}

fn failed_script_market_payload(message: &str) -> ScriptMarketPayload {
    ScriptMarketPayload {
        market: json!({
            "status": "failed",
            "message": message,
            "indexUrl": script_market::DEFAULT_MARKET_INDEX_URL,
            "updatedAt": "",
            "scripts": []
        }),
        user_scripts: user_script_inventory(),
    }
}

fn script_market_payload_from_manifest(
    manifest: &ScriptMarketManifest,
    status: &str,
    message: &str,
) -> ScriptMarketPayload {
    let user_scripts = user_script_inventory();
    let installed = installed_market_versions(&user_scripts);
    let scripts = manifest
        .scripts
        .iter()
        .map(|script| market_script_payload(script, &installed))
        .collect::<Vec<_>>();
    ScriptMarketPayload {
        market: json!({
            "status": status,
            "message": message,
            "indexUrl": script_market::DEFAULT_MARKET_INDEX_URL,
            "updatedAt": manifest.updated_at.clone().unwrap_or_default(),
            "scripts": scripts
        }),
        user_scripts,
    }
}

fn installed_market_versions(user_scripts: &Value) -> BTreeMap<String, String> {
    user_scripts
        .get("scripts")
        .and_then(Value::as_array)
        .map(|scripts| {
            scripts
                .iter()
                .filter_map(|script| {
                    let id = script.get("market_id").and_then(Value::as_str)?;
                    if id.is_empty() {
                        return None;
                    }
                    let version = script
                        .get("version")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    Some((id.to_string(), version))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn market_script_payload(script: &MarketScript, installed: &BTreeMap<String, String>) -> Value {
    let installed_version = installed.get(&script.id).cloned().unwrap_or_default();
    let is_installed = !installed_version.is_empty();
    json!({
        "id": script.id,
        "name": script.name,
        "description": script.description,
        "version": script.version,
        "author": script.author,
        "tags": script.tags,
        "homepage": script.homepage,
        "script_url": script.script_url,
        "sha256": script.sha256,
        "installed": is_installed,
        "installedVersion": installed_version,
        "updateAvailable": is_installed && installed.get(&script.id).map(|version| version != &script.version).unwrap_or(false)
    })
}

fn default_user_script_manager() -> UserScriptManager {
    let config_dir = user_scripts_config_dir();
    UserScriptManager::new(
        builtin_user_scripts_dir(),
        config_dir.join("user_scripts"),
        config_dir.join("user_scripts.json"),
    )
}

fn user_scripts_config_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(roaming) = std::env::var_os("APPDATA") {
            return PathBuf::from(roaming).join("Codex++");
        }
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("Codex++")
}

fn builtin_user_scripts_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|path| path.join("user_scripts"))
        .unwrap_or_else(|| PathBuf::from("user_scripts"))
}

fn diagnostics_report() -> String {
    let (codex_app_path, entrypoints, latest_launch) = load_overview_payload();
    let overview = ok(
        "概览已加载。",
        OverviewPayload {
            codex_version: codex_app_path
                .as_deref()
                .and_then(codex_plus_core::app_paths::codex_app_version),
            codex_app: path_state(codex_app_path),
            silent_shortcut: shortcut_state(entrypoints.silent_shortcut),
            management_shortcut: shortcut_state(entrypoints.management_shortcut),
            latest_launch,
            current_version: codex_plus_core::version::VERSION.to_string(),
            update_status: "not_checked".to_string(),
            settings_path: codex_plus_core::paths::default_settings_path()
                .to_string_lossy()
                .to_string(),
            logs_path: codex_plus_core::paths::default_diagnostic_log_path()
                .to_string_lossy()
                .to_string(),
        },
    );
    let settings = SettingsStore::default().load().unwrap_or_default();
    let generated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    serde_json::to_string_pretty(&json!({
        "generatedAtMs": generated_at_ms,
        "version": codex_plus_core::version::VERSION,
        "overview": overview.payload,
        "settings": settings,
        "logs": {
            "diagnosticLogPath": codex_plus_core::paths::default_diagnostic_log_path(),
            "latestStatusPath": codex_plus_core::paths::default_latest_status_path()
        },
        "platform": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH
        }
    }))
    .unwrap_or_else(|error| format!("诊断报告序列化失败：{error}"))
}

fn load_overview_payload() -> (
    Option<PathBuf>,
    install::EntryPointState,
    Option<LaunchStatus>,
) {
    let settings = SettingsStore::default().load().unwrap_or_default();
    (
        codex_plus_core::app_paths::resolve_codex_app_dir_with_saved(
            None,
            Some(settings.codex_app_path.as_str()),
        ),
        install::inspect_entrypoints(),
        StatusStore::default().load_latest().unwrap_or(None),
    )
}

fn install_background_failure(action: &str, error: impl std::fmt::Display) -> InstallActionResult {
    let state = install::inspect_entrypoints();
    InstallActionResult {
        status: "failed".to_string(),
        message: format!("{action}后台任务失败：{error}"),
        silent_shortcut: state.silent_shortcut,
        management_shortcut: state.management_shortcut,
    }
}

fn watcher_payload() -> WatcherPayload {
    let flag = codex_plus_core::watcher::default_watcher_disabled_flag();
    WatcherPayload {
        enabled: !flag.exists(),
        disabled_flag: flag.to_string_lossy().to_string(),
    }
}

fn read_tail(path: &Path, max_lines: usize) -> std::io::Result<String> {
    let contents = fs::read_to_string(path)?;
    let mut lines = contents.lines().rev().take(max_lines).collect::<Vec<_>>();
    lines.reverse();
    Ok(lines.join("\n"))
}

fn path_state(path: Option<PathBuf>) -> PathState {
    match path {
        Some(path) => PathState {
            status: "found".to_string(),
            path: Some(path.to_string_lossy().to_string()),
        },
        None => PathState {
            status: "missing".to_string(),
            path: None,
        },
    }
}

fn shortcut_state(shortcut: install::ShortcutState) -> PathState {
    PathState {
        status: if shortcut.installed {
            "installed".to_string()
        } else {
            "missing".to_string()
        },
        path: shortcut.path,
    }
}

fn ok<T: Serialize>(message: &str, payload: T) -> CommandResult<T> {
    CommandResult {
        status: "ok".to_string(),
        message: message.to_string(),
        payload,
    }
}

fn failed<T: Serialize>(message: &str, payload: T) -> CommandResult<T> {
    CommandResult {
        status: "failed".to_string(),
        message: message.to_string(),
        payload,
    }
}

fn default_debug_port() -> u16 {
    9229
}

fn default_helper_port() -> u16 {
    57321
}

fn default_log_lines() -> usize {
    200
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_version_returns_structured_payload() {
        let result = backend_version();

        assert_eq!(result.status, "ok");
        assert!(!result.payload.version.is_empty());
    }

    #[test]
    fn startup_options_returns_structured_payload() {
        let result = startup_options();

        assert_eq!(result.status, "ok");
    }

    #[test]
    fn startup_options_honors_show_update_environment() {
        unsafe {
            std::env::set_var("CODEX_PLUS_SHOW_UPDATE", "1");
        }

        let result = startup_options();

        unsafe {
            std::env::remove_var("CODEX_PLUS_SHOW_UPDATE");
        }

        assert_eq!(result.status, "ok");
        assert!(result.payload.show_update);
    }

    #[test]
    fn startup_options_honors_show_update_argument() {
        assert!(should_show_update(
            ["codex-plus-plus-manager.exe", "--show-update"],
            None
        ));
    }

    #[test]
    fn overview_contains_expected_operational_fields() {
        let result = tauri::async_runtime::block_on(load_overview());

        assert_eq!(result.status, "ok");
        assert!(!result.payload.current_version.is_empty());
        assert!(
            result.payload.codex_version.is_none()
                || result
                    .payload
                    .codex_version
                    .as_deref()
                    .is_some_and(|version| !version.is_empty())
        );
        assert!(matches!(
            result.payload.codex_app.status.as_str(),
            "found" | "missing"
        ));
        assert!(matches!(
            result.payload.silent_shortcut.status.as_str(),
            "installed" | "missing"
        ));
    }

    #[test]
    fn update_install_requires_release_payload() {
        let result = tauri::async_runtime::block_on(perform_update(None));

        assert_eq!(result.status, "failed");
        assert!(result.message.contains("请先检查更新"));
    }

    #[test]
    fn watcher_state_returns_disabled_flag_path() {
        let result = load_watcher_state();

        assert_eq!(result.status, "ok");
        assert!(result.payload.disabled_flag.contains("watcher.disabled"));
    }

    #[test]
    fn missing_logs_return_failed_status() {
        let result = read_latest_logs(LogRequest { lines: 25 });

        if result.payload.text.is_empty() {
            assert_eq!(result.status, "failed");
        }
    }

    #[test]
    fn relay_payload_does_not_expose_token_text() {
        let payload = relay_payload(
            codex_plus_core::relay_config::RelayStatus {
                authenticated: true,
                auth_source: "registry.json".to_string(),
                account_label: Some("user@example.test".to_string()),
                config_path: "config.toml".to_string(),
                configured: true,
                requires_openai_auth: true,
                has_bearer_token: true,
            },
            None,
        );
        let text = serde_json::to_string(&payload).unwrap();

        assert!(!text.contains("sk-"));
        assert!(text.contains("hasBearerToken"));
    }

    #[test]
    fn provider_doctor_recommendation_prioritizes_actionable_failures() {
        let recommendation = provider_doctor_recommendation(&[
            ProviderDoctorCheck {
                id: "models".to_string(),
                title: "模型列表".to_string(),
                status: "failed".to_string(),
                detail: "上游不支持 /v1/models".to_string(),
            },
            ProviderDoctorCheck {
                id: "request".to_string(),
                title: "真实请求".to_string(),
                status: "failed".to_string(),
                detail: "HTTP 404".to_string(),
            },
        ]);

        assert!(recommendation.contains("/v1/models"));
    }

    #[test]
    fn provider_doctor_recommendation_reports_model_warning() {
        let recommendation = provider_doctor_recommendation(&[
            ProviderDoctorCheck {
                id: "config".to_string(),
                title: "配置完整性".to_string(),
                status: "ok".to_string(),
                detail: "https://example.test/v1 / Responses API".to_string(),
            },
            ProviderDoctorCheck {
                id: "models".to_string(),
                title: "模型列表".to_string(),
                status: "warning".to_string(),
                detail: "未看到测试模型".to_string(),
            },
            ProviderDoctorCheck {
                id: "request".to_string(),
                title: "真实请求".to_string(),
                status: "ok".to_string(),
                detail: "HTTP 200".to_string(),
            },
        ]);

        assert!(recommendation.contains("测试模型"));
    }

    #[test]
    fn aggregate_relay_injection_writes_local_proxy_without_chatgpt_auth() {
        let temp = tempfile::tempdir().unwrap();

        let result = apply_aggregate_relay_injection_to_home(temp.path());
        let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

        assert_eq!(result.status, "ok");
        assert!(result.payload.configured);
        assert!(!result.payload.authenticated);
        assert!(config.contains(r#"base_url = "http://127.0.0.1:57321/v1""#));
        assert!(config.contains(r#"experimental_bearer_token = "codex-plus-aggregate""#));
    }

    #[test]
    fn relay_files_payload_reads_config_and_auth_contents() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("config.toml"),
            "model_provider = \"custom\"\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("auth.json"),
            "{\"OPENAI_API_KEY\":\"sk-test\"}\n",
        )
        .unwrap();

        let payload = relay_files_payload_from_home(temp.path()).unwrap();

        assert!(payload.config_path.ends_with("config.toml"));
        assert!(payload.auth_path.ends_with("auth.json"));
        assert_eq!(payload.config_contents, "model_provider = \"custom\"\n");
        assert_eq!(payload.auth_contents, "{\"OPENAI_API_KEY\":\"sk-test\"}\n");
    }

    #[test]
    fn env_conflict_commands_ignore_codex_home_and_remove_openai_vars() {
        let test_openai_name = "OPENAI_CODEX_PLUS_ENV_CONFLICT_TEST";
        let previous_openai = std::env::var_os(test_openai_name);
        let previous_codex_home = std::env::var_os("CODEX_HOME");
        let temp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var(test_openai_name, "sk-test");
            std::env::set_var("CODEX_HOME", temp.path());
        }

        let check = check_env_conflicts();
        assert_eq!(check.status, "ok");
        assert!(
            check
                .payload
                .conflicts
                .iter()
                .any(|item| item.name == test_openai_name)
        );
        assert!(
            !check
                .payload
                .conflicts
                .iter()
                .any(|item| item.name == "CODEX_HOME")
        );

        codex_plus_core::env_conflicts::remove_process_env_conflicts_for_tests(
            &[test_openai_name.to_string(), "CODEX_HOME".to_string()],
            codex_plus_core::paths::default_app_state_dir().join("test-backups"),
        )
        .unwrap();
        assert!(std::env::var_os(test_openai_name).is_none());
        assert_eq!(
            std::env::var_os("CODEX_HOME"),
            Some(temp.path().as_os_str().to_os_string())
        );

        unsafe {
            match previous_openai {
                Some(value) => std::env::set_var(test_openai_name, value),
                None => std::env::remove_var(test_openai_name),
            }
            match previous_codex_home {
                Some(value) => std::env::set_var("CODEX_HOME", value),
                None => std::env::remove_var("CODEX_HOME"),
            }
        }
    }

    #[test]
    fn delete_local_session_falls_back_when_requested_db_no_longer_contains_thread() {
        let temp = tempfile::tempdir().unwrap();
        let previous_codex_home = std::env::var_os("CODEX_HOME");
        let codex_home = temp.path().join("codex-home");
        let sqlite_dir = codex_home.join("sqlite");
        std::fs::create_dir_all(&sqlite_dir).unwrap();
        let stale_db = sqlite_dir.join("codex-dev.db");
        let active_db = sqlite_dir.join("state_5.sqlite");
        let rollout_path = temp.path().join("rollout.jsonl");
        std::fs::write(&rollout_path, "{\"type\":\"message\"}\n").unwrap();
        let stale = rusqlite::Connection::open(&stale_db).unwrap();
        stale
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT)",
                [],
            )
            .unwrap();
        drop(stale);
        let active = rusqlite::Connection::open(&active_db).unwrap();
        active
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT)",
                [],
            )
            .unwrap();
        active
            .execute(
                "INSERT INTO threads VALUES ('t1', ?1, 'Active Thread')",
                [rollout_path.to_string_lossy().to_string()],
            )
            .unwrap();
        drop(active);

        unsafe {
            std::env::set_var("CODEX_HOME", &codex_home);
        }
        let result = delete_local_session(DeleteLocalSessionRequest {
            session_id: "t1".to_string(),
            title: "Active Thread".to_string(),
            db_path: Some(stale_db.to_string_lossy().to_string()),
        });
        unsafe {
            if let Some(value) = previous_codex_home {
                std::env::set_var("CODEX_HOME", value);
            } else {
                std::env::remove_var("CODEX_HOME");
            }
        }

        assert_eq!(result.status, "ok");
        assert_eq!(
            result.payload.status,
            codex_plus_core::models::DeleteStatus::LocalDeleted
        );
        let active = rusqlite::Connection::open(&active_db).unwrap();
        assert_eq!(
            active
                .query_row("SELECT COUNT(*) FROM threads WHERE id = 't1'", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }

    #[test]
    fn list_local_sessions_deduplicates_threads_across_current_and_legacy_dbs() {
        let temp = tempfile::tempdir().unwrap();
        let previous_codex_home = std::env::var_os("CODEX_HOME");
        let codex_home = temp.path().join("codex-home");
        let sqlite_dir = codex_home.join("sqlite");
        std::fs::create_dir_all(&sqlite_dir).unwrap();
        let current_db = sqlite_dir.join("state_5.sqlite");
        let legacy_db = codex_home.join("state_5.sqlite");
        create_minimal_thread_db(&current_db, "t1", "Current Copy", 100);
        create_minimal_thread_db(&legacy_db, "t1", "Legacy Copy", 200);

        unsafe {
            std::env::set_var("CODEX_HOME", &codex_home);
        }
        let result = list_local_sessions();
        restore_codex_home(previous_codex_home);

        assert_eq!(result.status, "ok");
        assert_eq!(result.payload.sessions.len(), 1);
        assert_eq!(result.payload.sessions[0].id, "t1");
        assert_eq!(result.payload.sessions[0].title, "Legacy Copy");
        assert_eq!(
            result.payload.sessions[0].db_path,
            legacy_db.to_string_lossy()
        );
    }

    #[test]
    fn delete_local_session_removes_duplicate_threads_from_all_candidate_dbs() {
        let temp = tempfile::tempdir().unwrap();
        let previous_codex_home = std::env::var_os("CODEX_HOME");
        let codex_home = temp.path().join("codex-home");
        let sqlite_dir = codex_home.join("sqlite");
        std::fs::create_dir_all(&sqlite_dir).unwrap();
        let current_db = sqlite_dir.join("state_5.sqlite");
        let legacy_db = codex_home.join("state_5.sqlite");
        create_minimal_thread_db(&current_db, "t1", "Current Copy", 100);
        create_minimal_thread_db(&legacy_db, "t1", "Legacy Copy", 200);

        unsafe {
            std::env::set_var("CODEX_HOME", &codex_home);
        }
        let result = delete_local_session(DeleteLocalSessionRequest {
            session_id: "t1".to_string(),
            title: "Legacy Copy".to_string(),
            db_path: Some(legacy_db.to_string_lossy().to_string()),
        });
        restore_codex_home(previous_codex_home);

        assert_eq!(result.status, "ok");
        assert_eq!(thread_count(&current_db, "t1"), 0);
        assert_eq!(thread_count(&legacy_db, "t1"), 0);
    }

    fn create_minimal_thread_db(path: &Path, id: &str, title: &str, updated_at_ms: i64) {
        let db = rusqlite::Connection::open(path).unwrap();
        db.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT, updated_at_ms INTEGER)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO threads VALUES (?1, '', ?2, ?3)",
            (id, title, updated_at_ms),
        )
        .unwrap();
    }

    fn thread_count(path: &Path, id: &str) -> i64 {
        let db = rusqlite::Connection::open(path).unwrap();
        db.query_row("SELECT COUNT(*) FROM threads WHERE id = ?1", [id], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    }

    fn restore_codex_home(previous: Option<std::ffi::OsString>) {
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("CODEX_HOME", value);
            } else {
                std::env::remove_var("CODEX_HOME");
            }
        }
    }

    #[test]
    fn apply_relay_profile_to_home_with_switch_rules_preserves_custom_provider_id() {
        let temp = tempfile::tempdir().unwrap();
        let profile = RelayProfile {
            relay_mode: codex_plus_core::settings::RelayMode::PureApi,
            protocol: codex_plus_core::settings::RelayProtocol::Responses,
            config_contents: "model_provider = \"ai\"\nmodel = \"gpt-image-2\"\n\n[model_providers.ai]\nname = \"ai\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"https://ahg.codes\"\n"
                .to_string(),
            auth_contents: "{}\n".to_string(),
            ..RelayProfile::default()
        };

        codex_plus_core::relay_config::apply_relay_profile_to_home_with_switch_rules(
            temp.path(),
            &profile,
            "",
        )
        .unwrap();

        let applied = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
        assert!(applied.contains("model_provider = \"ai\""));
        assert!(applied.contains("[model_providers.ai]"));
        assert!(!applied.contains("[model_providers.custom]"));
    }

    #[test]
    fn save_relay_file_in_home_only_allows_known_files() {
        let temp = tempfile::tempdir().unwrap();

        save_relay_file_in_home(temp.path(), "config", "model = \"gpt-5\"\n").unwrap();
        save_relay_file_in_home(temp.path(), "auth", "{}\n").unwrap();

        assert_eq!(
            std::fs::read_to_string(temp.path().join("config.toml")).unwrap(),
            "model = \"gpt-5\"\n"
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("auth.json")).unwrap(),
            "{}\n"
        );
        assert!(save_relay_file_in_home(temp.path(), "../bad", "").is_err());
    }

    #[test]
    fn normalize_settings_before_save_preserves_profile_context_until_manual_extract() {
        let settings = BackendSettings {
            relay_common_config_contents: "[mcp_servers.context7]\ncommand = \"npx\"\n".to_string(),
            relay_profiles: vec![RelayProfile {
                use_common_config: false,
                config_contents: "model = \"gpt-5\"\n\n[mcp_servers.context7]\ncommand = \"npx\"\n"
                    .to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);

        assert!(
            normalized.relay_profiles[0]
                .config_contents
                .contains("model = \"gpt-5\"")
        );
        assert!(
            normalized.relay_profiles[0]
                .config_contents
                .contains("[mcp_servers.context7]")
        );
        assert!(
            normalized
                .relay_context_config_contents
                .contains("[mcp_servers.context7]")
        );
        assert!(
            !normalized
                .relay_common_config_contents
                .contains("[mcp_servers")
        );
    }

    #[test]
    fn normalize_settings_before_save_preserves_manual_relay_mode_for_pure_api_profile() {
        let settings = BackendSettings {
            active_relay_id: "api".to_string(),
            launch_mode: codex_plus_core::settings::LaunchMode::Relay,
            relay_profiles: vec![RelayProfile {
                id: "api".to_string(),
                relay_mode: codex_plus_core::settings::RelayMode::PureApi,
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);

        assert_eq!(
            normalized.launch_mode,
            codex_plus_core::settings::LaunchMode::Relay
        );
    }

    #[test]
    fn reset_image_overlay_settings_preserves_supplier_settings() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join("settings.json");
        let previous = codex_plus_core::paths::set_settings_path_for_tests(Some(settings_path));

        let settings = BackendSettings {
            codex_app_image_overlay_enabled: true,
            codex_app_image_overlay_path: "C:\\Users\\me\\Pictures\\overlay.png".to_string(),
            codex_app_image_overlay_opacity: 42,
            codex_app_image_overlay_fit_mode: "fill".to_string(),
            active_relay_id: "supplier-a".to_string(),
            relay_profiles: vec![RelayProfile {
                id: "supplier-a".to_string(),
                name: "供应商 A".to_string(),
                relay_mode: codex_plus_core::settings::RelayMode::PureApi,
                api_key: "sk-test".to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };
        SettingsStore::default().save(&settings).unwrap();

        let result = reset_image_overlay_settings();
        codex_plus_core::paths::set_settings_path_for_tests(previous);

        assert_eq!(result.status, "ok");
        assert!(!result.payload.settings.codex_app_image_overlay_enabled);
        assert_eq!(result.payload.settings.codex_app_image_overlay_path, "");
        assert_eq!(result.payload.settings.codex_app_image_overlay_opacity, 35);
        assert_eq!(
            result.payload.settings.codex_app_image_overlay_fit_mode,
            "fit"
        );
        assert_eq!(result.payload.settings.active_relay_id, "supplier-a");
        assert_eq!(result.payload.settings.relay_profiles.len(), 1);
        assert_eq!(result.payload.settings.relay_profiles[0].id, "supplier-a");
        assert_eq!(result.payload.settings.relay_profiles[0].api_key, "sk-test");
    }

    #[test]
    fn normalize_settings_before_save_preserves_official_profile_auth() {
        let settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                relay_mode: codex_plus_core::settings::RelayMode::Official,
                official_mix_api_key: false,
                auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"edited"}}"#
                    .to_string(),
                config_contents: "model_provider = \"custom\"\n".to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);

        let auth_json: serde_json::Value =
            serde_json::from_str(&normalized.relay_profiles[0].auth_contents).unwrap();
        assert_eq!(
            auth_json,
            serde_json::json!({
                "auth_mode": "chatgpt",
                "tokens": {
                    "access_token": "edited"
                }
            })
        );
        assert!(normalized.relay_profiles[0].config_contents.is_empty());
    }

    #[test]
    fn normalize_settings_before_save_strips_common_from_enabled_profile() {
        let settings = BackendSettings {
            relay_common_config_contents: r#"model_reasoning_effort = "high"

[features]
goals = true

[plugins."superpowers@openai-curated"]
enabled = true
"#
            .to_string(),
            relay_profiles: vec![RelayProfile {
                use_common_config: true,
                config_contents: r#"model = "gpt-5"
model_reasoning_effort = "high"

[features]
goals = true
model_reasoning_effort = "high"

[plugins."superpowers@openai-curated"]
enabled = true
"#
                .to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);
        let config = &normalized.relay_profiles[0].config_contents;

        assert!(config.contains("model = \"gpt-5\""));
        assert!(!config.contains("model_reasoning_effort"));
        assert!(!config.contains("[features]"));
        assert!(!config.contains("[plugins.\"superpowers@openai-curated\"]"));
    }

    #[test]
    fn normalize_settings_before_save_repairs_invalid_profile_common_duplication() {
        let settings = BackendSettings {
            relay_common_config_contents: r#"model_reasoning_effort = "high"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"
"#
            .to_string(),
            relay_profiles: vec![RelayProfile {
                use_common_config: true,
                config_contents: r#"model = "gpt-5"
model_reasoning_effort = "high"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"
"#
                .to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);
        let config = &normalized.relay_profiles[0].config_contents;

        assert!(config.contains("model = \"gpt-5\""));
        assert!(!config.contains("model_reasoning_effort"));
        assert!(!config.contains("[marketplaces.openai-bundled]"));
    }

    #[test]
    fn normalize_settings_before_save_removes_model_catalog_from_common_config() {
        let settings = BackendSettings {
            relay_common_config_contents: r#"model_catalog_json = "C:\\Users\\Administrator\\.codex\\model-catalogs\\relay-a.json"
model_catalog_json = 'C:\Users\Administrator\.codex\model-catalogs\relay-b.json'
model_reasoning_effort = "high"
"#
            .to_string(),
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);

        assert!(
            !normalized
                .relay_common_config_contents
                .contains("model_catalog_json")
        );
        assert!(
            normalized
                .relay_common_config_contents
                .contains("model_reasoning_effort = \"high\"")
        );
    }

    #[test]
    fn context_entry_commands_update_settings_payload() {
        let settings = BackendSettings::default();
        let upsert = upsert_context_entry(ContextEntryRequest {
            settings: settings.clone(),
            kind: "mcp".to_string(),
            id: "context7".to_string(),
            toml_body: "command = \"npx\"\n".to_string(),
        });

        assert_eq!(upsert.status, "ok");
        assert!(
            upsert
                .payload
                .settings
                .relay_context_config_contents
                .contains("[mcp_servers.context7]")
        );

        let listed = list_context_entries(ContextSettingsRequest {
            settings: upsert.payload.settings.clone(),
        });
        assert_eq!(listed.payload.entries.mcp_servers[0].id, "context7");

        let deleted = delete_context_entry(ContextDeleteRequest {
            settings: upsert.payload.settings,
            kind: "mcp".to_string(),
            id: "context7".to_string(),
        });
        assert_eq!(deleted.status, "ok");
        assert!(
            !deleted
                .payload
                .settings
                .relay_context_config_contents
                .contains("[mcp_servers.context7]")
        );
    }

    #[test]
    fn ads_payload_keeps_version_and_ad_items() {
        let payload = ads_payload(json!({
            "version": 1,
            "ads": [{"id": "ad-1", "type": "normal", "title": "Ad"}]
        }));

        assert_eq!(payload.version, 1);
        assert_eq!(payload.ads.len(), 1);
        assert_eq!(payload.ads[0]["id"], json!("ad-1"));
    }

    #[test]
    fn open_external_url_rejects_non_http_urls() {
        let result = open_external_url("file:///C:/Windows/win.ini".to_string());

        assert_eq!(result.status, "failed");
        assert!(result.message.contains("只允许打开 http 或 https 链接"));
    }
}
