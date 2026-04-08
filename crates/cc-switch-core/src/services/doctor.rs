use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::app_config::AppType;
use crate::config;
use crate::database::Database;
use crate::error::AppError;
use crate::prompt_files::prompt_file_path;
use crate::services::env_checker::EnvConflict;
use crate::services::global_proxy::GlobalProxyService;
use crate::services::host::HostService;
use crate::services::runtime::{RuntimeService, ToolVersionInfo, UpdateInfo};
use crate::settings::{self, VisibleApps};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorPathStatus {
    pub label: String,
    pub path: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSettingsSnapshot {
    pub launch_on_startup: bool,
    pub silent_startup: bool,
    pub preferred_terminal: Option<String>,
    pub visible_apps: VisibleApps,
    pub claude_plugin_integration: bool,
    pub skip_claude_onboarding: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorRuntimeSnapshot {
    pub version: String,
    pub executable_path: String,
    pub platform: String,
    pub arch: String,
    pub portable_mode: bool,
    pub config_dir: String,
    pub database_path: String,
    pub settings_path: String,
    pub global_proxy_url: Option<String>,
    pub global_proxy_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorAppSnapshot {
    pub app: String,
    pub additive_mode: bool,
    pub visible: bool,
    pub config_dir: String,
    pub override_dir: Option<String>,
    pub provider_count: usize,
    pub current_provider: Option<String>,
    pub live_paths: Vec<DoctorPathStatus>,
    pub env_conflicts: Vec<EnvConflict>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub generated_at: i64,
    pub runtime: DoctorRuntimeSnapshot,
    pub settings: DoctorSettingsSnapshot,
    pub tools: Vec<ToolVersionInfo>,
    pub apps: Vec<DoctorAppSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdateInfo>,
    pub warnings: Vec<String>,
}

pub struct DoctorService;

impl DoctorService {
    pub async fn inspect(
        db: &Arc<Database>,
        apps: Option<Vec<AppType>>,
        include_latest_tool_versions: bool,
        include_update_check: bool,
    ) -> Result<DoctorReport, AppError> {
        let selected_apps = normalize_apps(apps);
        let settings = settings::get_settings();
        let visible_apps = settings.visible_apps.clone().unwrap_or_default();
        let preferences = HostService::get_preferences()?;
        let executable_path = RuntimeService::current_executable_path()?;
        let mut warnings = Vec::new();
        let global_proxy_url = match GlobalProxyService::get_proxy_url(db) {
            Ok(url) => url,
            Err(err) => {
                warnings.push(format!(
                    "Global proxy status could not be loaded from database: {err}"
                ));
                None
            }
        };
        let runtime = DoctorRuntimeSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            executable_path: executable_path.display().to_string(),
            platform: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            portable_mode: RuntimeService::is_portable_mode()?,
            config_dir: config::config_dir().display().to_string(),
            database_path: config::database_path().display().to_string(),
            settings_path: config::settings_path().display().to_string(),
            global_proxy_active: global_proxy_url.is_some(),
            global_proxy_url,
        };
        let settings_snapshot = DoctorSettingsSnapshot {
            launch_on_startup: preferences.launch_on_startup,
            silent_startup: preferences.silent_startup,
            preferred_terminal: preferences.preferred_terminal,
            visible_apps,
            claude_plugin_integration: settings.enable_claude_plugin_integration,
            skip_claude_onboarding: settings.skip_claude_onboarding,
        };

        let tools =
            RuntimeService::get_tool_versions(None, None, include_latest_tool_versions).await?;
        let update = if include_update_check {
            Some(RuntimeService::check_for_updates().await?)
        } else {
            None
        };

        for tool in &tools {
            if tool.version.is_none() {
                warnings.push(format!(
                    "Tool '{}' is not available locally: {}",
                    tool.name,
                    tool.error
                        .as_deref()
                        .unwrap_or("not installed or not executable")
                ));
            }
        }
        if let Some(info) = &update {
            if let Some(error) = &info.error {
                warnings.push(format!(
                    "Update check could not fetch latest release metadata: {error}"
                ));
            }
        }

        let mut app_reports = Vec::new();
        for app in selected_apps {
            app_reports.push(Self::inspect_app(
                db,
                &app,
                &settings_snapshot.visible_apps,
            )?);
        }
        for app in &app_reports {
            warnings.extend(
                app.warnings
                    .iter()
                    .map(|warning| format!("[{}] {warning}", app.app)),
            );
        }

        Ok(DoctorReport {
            generated_at: chrono::Utc::now().timestamp(),
            runtime,
            settings: settings_snapshot,
            tools,
            apps: app_reports,
            update,
            warnings,
        })
    }

    fn inspect_app(
        db: &Arc<Database>,
        app: &AppType,
        visible_apps: &VisibleApps,
    ) -> Result<DoctorAppSnapshot, AppError> {
        let config_dir = app_config_dir(app);
        let live_paths = app_live_paths(app)?;
        let env_conflicts = env_conflicts_for(app);
        let mut warnings = Vec::new();
        let (provider_count, current_provider) = match db.get_all_providers(app.as_str()) {
            Ok(providers) => {
                let current_provider = resolve_current_provider(db, app, &providers, &mut warnings);
                (providers.len(), current_provider)
            }
            Err(err) => {
                warnings.push(format!(
                    "Provider inventory could not be loaded from database: {err}"
                ));
                (0, settings::get_current_provider(app))
            }
        };

        if !app.is_additive_mode() && provider_count == 0 {
            warnings.push("No providers are configured.".to_string());
        }
        if !app.is_additive_mode() && provider_count > 0 && current_provider.is_none() {
            warnings.push("No current provider is selected.".to_string());
        }
        if !env_conflicts.is_empty() {
            warnings.push(format!(
                "{} environment conflict(s) detected.",
                env_conflicts.len()
            ));
        } else if !env_conflict_supports(app) {
            warnings
                .push("Environment conflict scan is not specialized for this app yet.".to_string());
        }
        if live_paths.iter().all(|item| !item.exists) {
            warnings.push("No live config files were found yet.".to_string());
        }

        Ok(DoctorAppSnapshot {
            app: app.as_str().to_string(),
            additive_mode: app.is_additive_mode(),
            visible: visible_apps.is_visible(app),
            config_dir: config_dir.display().to_string(),
            override_dir: app_override_dir(app).map(|path| path.display().to_string()),
            provider_count,
            current_provider,
            live_paths,
            env_conflicts,
            warnings,
        })
    }
}

fn normalize_apps(apps: Option<Vec<AppType>>) -> Vec<AppType> {
    let Some(apps) = apps else {
        return AppType::all().collect();
    };

    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for app in apps {
        if seen.insert(app.as_str().to_string()) {
            ordered.push(app);
        }
    }
    ordered
}

fn resolve_current_provider(
    db: &Arc<Database>,
    app: &AppType,
    providers: &indexmap::IndexMap<String, crate::provider::Provider>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    if app.is_additive_mode() {
        return None;
    }

    if let Some(local_id) = settings::get_current_provider(app) {
        if providers.contains_key(&local_id) {
            return Some(local_id);
        }
        warnings.push(format!(
            "Configured current provider '{}' is no longer present.",
            local_id
        ));
        return None;
    }

    match db.get_current_provider(app.as_str()) {
        Ok(current) => current,
        Err(err) => {
            warnings.push(format!(
                "Current provider could not be resolved from database: {err}"
            ));
            None
        }
    }
}

fn app_override_dir(app: &AppType) -> Option<std::path::PathBuf> {
    match app {
        AppType::Claude => settings::get_claude_override_dir(),
        AppType::Codex => settings::get_codex_override_dir(),
        AppType::Gemini => settings::get_gemini_override_dir(),
        AppType::OpenCode => settings::get_opencode_override_dir(),
        AppType::OpenClaw => settings::get_openclaw_override_dir(),
    }
}

fn app_config_dir(app: &AppType) -> std::path::PathBuf {
    match app {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::Codex => config::get_codex_config_dir(),
        AppType::Gemini => config::get_gemini_config_dir(),
        AppType::OpenCode => config::get_opencode_config_dir(),
        AppType::OpenClaw => config::get_openclaw_config_dir(),
    }
}

fn app_live_paths(app: &AppType) -> Result<Vec<DoctorPathStatus>, AppError> {
    let mut paths = match app {
        AppType::Claude => vec![
            build_path("settings", config::get_claude_settings_path()),
            build_path("mcp", config::get_claude_mcp_path()),
        ],
        AppType::Codex => vec![
            build_path("auth", crate::codex_config::get_codex_auth_path()),
            build_path("config", crate::codex_config::get_codex_config_path()),
        ],
        AppType::Gemini => vec![
            build_path("env", crate::gemini_config::get_gemini_env_path()),
            build_path("settings", crate::gemini_config::get_gemini_settings_path()),
        ],
        AppType::OpenCode => vec![build_path(
            "config",
            crate::opencode_config::get_opencode_config_path(),
        )],
        AppType::OpenClaw => vec![build_path(
            "config",
            crate::openclaw_config::get_openclaw_config_path(),
        )],
    };
    paths.push(build_path("prompt", prompt_file_path(app)?));
    Ok(paths)
}

fn build_path(label: &str, path: std::path::PathBuf) -> DoctorPathStatus {
    DoctorPathStatus {
        label: label.to_string(),
        exists: path.exists(),
        path: path.display().to_string(),
    }
}

fn env_conflicts_for(app: &AppType) -> Vec<EnvConflict> {
    if !env_conflict_supports(app) {
        return Vec::new();
    }

    crate::services::env_checker::check_env_conflicts(app.as_str()).unwrap_or_default()
}

fn env_conflict_supports(app: &AppType) -> bool {
    matches!(app, AppType::Claude | AppType::Codex | AppType::Gemini)
}

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use tempfile::tempdir;

    use super::DoctorService;
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::settings::{self, update_settings, AppSettings, VisibleApps};

    #[tokio::test]
    #[serial]
    async fn doctor_report_includes_runtime_and_selected_app() -> Result<(), crate::error::AppError>
    {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        update_settings(AppSettings {
            visible_apps: Some(VisibleApps {
                claude: true,
                codex: false,
                gemini: true,
                opencode: true,
                openclaw: true,
            }),
            ..AppSettings::default()
        })?;

        let db = std::sync::Arc::new(Database::memory()?);
        db.save_provider(
            "claude",
            &Provider::with_id(
                "provider-a".to_string(),
                "Provider A".to_string(),
                serde_json::json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "token-a",
                        "ANTHROPIC_BASE_URL": "https://api.example.com"
                    }
                }),
                None,
            ),
        )?;
        settings::set_current_provider(&AppType::Claude, Some("provider-a"))?;

        let report = DoctorService::inspect(&db, Some(vec![AppType::Claude]), false, false).await?;

        assert_eq!(report.apps.len(), 1);
        assert_eq!(report.apps[0].app, "claude");
        assert_eq!(report.apps[0].provider_count, 1);
        assert_eq!(
            report.apps[0].current_provider.as_deref(),
            Some("provider-a")
        );
        assert!(
            report.runtime.database_path.ends_with("cc-switch.db"),
            "doctor should surface the database path"
        );

        Ok(())
    }
}
