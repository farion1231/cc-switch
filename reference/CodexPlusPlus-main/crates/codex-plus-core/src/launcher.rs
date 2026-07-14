use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::settings::{BackendSettings, SettingsStore, normalize_codex_extra_args};
use crate::status::{LaunchStatus, StatusStore};

#[cfg(windows)]
const POST_LAUNCH_COMPUTER_USE_GUARD_SECONDS: &[u64] = &[0, 5, 15, 30, 60, 120, 180, 240, 300];
#[cfg_attr(not(windows), allow(dead_code))]
const POST_LAUNCH_COMPUTER_USE_GUARD_STABLE_ATTEMPTS: usize = 3;
static PET_OVERLAY_SYNC_FAILED: AtomicBool = AtomicBool::new(false);
static PET_CURSOR_DRIVER_FAILED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexLaunch {
    Process {
        command: Vec<String>,
        wait_strategy: ProcessWaitStrategy,
        macos_cleanup_policy: Option<MacosCleanupPolicy>,
    },
    PackagedActivation {
        app_user_model_id: String,
        arguments: String,
        process_id: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessWaitStrategy {
    TrackedChild,
    ExternalWaitCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacosCleanupPolicy {
    QuitIfNotPreviouslyRunning,
    SkipQuitBecauseAlreadyRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsProcessControlStrategy {
    NativeWindowsApi,
}

#[cfg(windows)]
pub fn windows_process_control_strategy() -> WindowsProcessControlStrategy {
    WindowsProcessControlStrategy::NativeWindowsApi
}

impl CodexLaunch {
    pub fn process_id(&self) -> Option<u32> {
        match self {
            Self::PackagedActivation { process_id, .. } => *process_id,
            Self::Process { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub app_dir: Option<PathBuf>,
    pub debug_port: u16,
    pub helper_port: u16,
    pub status_store: StatusStore,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            app_dir: None,
            debug_port: 9229,
            helper_port: 57321,
            status_store: StatusStore::default(),
        }
    }
}

#[derive(Clone)]
pub struct LaunchHandle {
    pub debug_port: u16,
    pub helper_port: u16,
    pub app_dir: PathBuf,
    pub launch: CodexLaunch,
    pub status_store: StatusStore,
    helper_started: bool,
    hooks: Arc<dyn LaunchHooks>,
}

impl std::fmt::Debug for LaunchHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LaunchHandle")
            .field("debug_port", &self.debug_port)
            .field("helper_port", &self.helper_port)
            .field("app_dir", &self.app_dir)
            .field("launch", &self.launch)
            .field("status_store", &self.status_store)
            .finish_non_exhaustive()
    }
}

impl LaunchHandle {
    pub async fn wait_for_codex_exit(&self) -> anyhow::Result<()> {
        let result = self.hooks.wait_for_codex_exit(&self.launch).await;
        if self.helper_started {
            self.hooks.shutdown_helper(self.helper_port).await;
        }
        result
    }
}

#[async_trait(?Send)]
pub trait LaunchHooks: Send + Sync {
    fn resolve_app_dir(
        &self,
        app_dir: Option<&Path>,
        settings: &BackendSettings,
    ) -> anyhow::Result<PathBuf>;
    fn select_debug_port(&self, requested: u16) -> u16;
    fn select_helper_port(&self, requested: u16) -> u16;
    async fn load_settings(&self) -> anyhow::Result<BackendSettings>;
    async fn run_provider_sync(&self) -> anyhow::Result<()>;
    async fn apply_active_relay_profile(&self, _settings: &BackendSettings) -> anyhow::Result<()> {
        Ok(())
    }
    async fn ensure_computer_use_config(&self, _settings: &BackendSettings) -> anyhow::Result<()> {
        Ok(())
    }
    async fn ensure_plugin_marketplace_config(
        &self,
        _settings: &BackendSettings,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    async fn start_helper(&self, helper_port: u16) -> anyhow::Result<()>;
    async fn launch_codex(
        &self,
        app_dir: &Path,
        debug_port: u16,
        settings: &BackendSettings,
        extra_args: &[String],
    ) -> anyhow::Result<CodexLaunch>;
    async fn bridge_context(
        &self,
        _debug_port: u16,
        _app_dir: &Path,
    ) -> anyhow::Result<Option<crate::routes::BridgeContext>> {
        Ok(None)
    }
    async fn inject(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()>;
    async fn inject_bridge(
        &self,
        debug_port: u16,
        helper_port: u16,
        _ctx: crate::routes::BridgeContext,
    ) -> anyhow::Result<()> {
        self.inject(debug_port, helper_port).await
    }
    async fn ensure_injection(&self, debug_port: u16, helper_port: u16, app_dir: &Path) -> bool {
        for attempt in 1..=120 {
            let result = match self.bridge_context(debug_port, app_dir).await {
                Ok(Some(ctx)) => self.inject_bridge(debug_port, helper_port, ctx).await,
                Ok(None) => self.inject(debug_port, helper_port).await,
                Err(error) => Err(error),
            };
            match result {
                Ok(()) => return true,
                Err(error) => {
                    let _ = crate::diagnostic_log::append_diagnostic_log(
                        "launcher.ensure_injection_retry_failed",
                        serde_json::json!({
                            "debug_port": debug_port,
                            "helper_port": helper_port,
                            "attempt": attempt,
                            "message": error.to_string()
                        }),
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
        false
    }
    async fn start_bridge_watchdog(
        &self,
        _debug_port: u16,
        _helper_port: u16,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    async fn start_computer_use_guard_watchdog(
        &self,
        _settings: &BackendSettings,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    async fn write_status(&self, status: &str);
    async fn wait_for_codex_exit(&self, launch: &CodexLaunch) -> anyhow::Result<()>;
    async fn shutdown_helper(&self, helper_port: u16);
    async fn terminate_codex(&self, launch: &CodexLaunch);
}

#[derive(Default)]
pub struct DefaultLaunchHooks {
    child: Mutex<Option<Child>>,
    helper: Mutex<Option<HelperRuntime>>,
    bridge_watchdog: Mutex<Option<BridgeWatchdogRuntime>>,
    computer_use_guard_watchdog: Mutex<Option<ComputerUseGuardWatchdogRuntime>>,
    computer_use_guard_artifacts: Mutex<Option<crate::computer_use_guard::GuardArtifacts>>,
}

struct HelperRuntime {
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

struct BridgeWatchdogRuntime {
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

struct ComputerUseGuardWatchdogRuntime {
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

pub async fn launch_and_inject(options: LaunchOptions) -> anyhow::Result<LaunchHandle> {
    launch_and_inject_with_hooks(options, DefaultLaunchHooks::shared()).await
}

pub async fn launch_and_inject_with_hooks<H>(
    options: LaunchOptions,
    hooks: H,
) -> anyhow::Result<LaunchHandle>
where
    H: IntoLaunchHooks,
{
    let hooks = hooks.into_launch_hooks();
    let debug_port = hooks.select_debug_port(options.debug_port);
    let mut helper_port = hooks.select_helper_port(options.helper_port);
    let settings = hooks.load_settings().await?;
    let app_dir = hooks.resolve_app_dir(options.app_dir.as_deref(), &settings)?;
    let status_store = options.status_store.clone();
    let mut helper_started = false;
    let mut launched = None;
    let mut keep_launched_on_error = false;

    let result: anyhow::Result<LaunchHandle> = async {
        if settings.provider_sync_enabled {
            hooks.run_provider_sync().await?;
        }
        if let Err(error) = hooks.ensure_plugin_marketplace_config(&settings).await {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "launcher.plugin_marketplace_config_failed_nonfatal",
                serde_json::json!({
                    "message": error.to_string()
                }),
            );
        }
        if settings.computer_use_guard_enabled {
            hooks.ensure_computer_use_config(&settings).await?;
        }
        let home = crate::relay_config::default_codex_home_dir();
        match crate::codex_sqlite::sanitize_historical_model_suffixes(&home) {
            Ok(result) if result.updated > 0 => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "launcher.sanitize_historical_model_suffixes",
                    serde_json::json!({
                        "scanned": result.scanned,
                        "updated": result.updated
                    }),
                );
            }
            Ok(_) => {}
            Err(error) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "launcher.sanitize_historical_model_suffixes_failed",
                    serde_json::json!({
                        "error": error.to_string()
                    }),
                );
            }
        }
        let protocol_proxy_enabled = relay_protocol_proxy_enabled(&settings);
        if protocol_proxy_enabled {
            helper_port = crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT;
        }
        if settings.enhancements_enabled || protocol_proxy_enabled {
            hooks.start_helper(helper_port).await?;
            helper_started = true;
        }

        let launch = hooks
            .launch_codex(&app_dir, debug_port, &settings, &settings.codex_extra_args)
            .await?;
        launched = Some(launch.clone());
        keep_launched_on_error = true;
        if settings.computer_use_guard_enabled {
            hooks.start_computer_use_guard_watchdog(&settings).await?;
        }

        let mut injection_degraded = false;
        if settings.enhancements_enabled {
            let injection_ready = hooks
                .ensure_injection(debug_port, helper_port, &app_dir)
                .await;
            if injection_ready {
                keep_launched_on_error = false;
                // 注入成功后页面已加载，此时可以通过 CDP 清理 Electron Local Storage
                // 中残留的带后缀模型名，避免模型选择器继续显示废弃项。
                crate::codex_local_storage::sanitize_local_storage_model_suffixes_nonfatal(
                    debug_port,
                )
                .await;
                hooks.start_bridge_watchdog(debug_port, helper_port).await?;
            } else {
                let degraded = launch_status(
                    "running_degraded",
                    "Codex launched; Codex++ enhancements are still waiting for the page bridge.",
                    debug_port,
                    helper_port,
                    &app_dir,
                );
                options.status_store.save_latest(&degraded)?;
                hooks.write_status("running_degraded").await;
                injection_degraded = true;
            }
        }

        if !settings.enhancements_enabled || !injection_degraded {
            let status = launch_status(
                "running",
                "Codex++ launcher ready",
                debug_port,
                helper_port,
                &app_dir,
            );
            options.status_store.save_latest(&status)?;
            hooks.write_status("running").await;
        }

        Ok(LaunchHandle {
            debug_port,
            helper_port,
            app_dir: app_dir.clone(),
            launch,
            status_store: status_store.clone(),
            helper_started,
            hooks: Arc::clone(&hooks),
        })
    }
    .await;

    match result {
        Ok(handle) => Ok(handle),
        Err(error) => {
            if helper_started {
                hooks.shutdown_helper(helper_port).await;
            }
            if let Some(launch) = &launched {
                if !keep_launched_on_error {
                    hooks.terminate_codex(launch).await;
                }
            }
            let message = error.to_string();
            let failure = launch_status("failed", &message, debug_port, helper_port, &app_dir);
            let _ = status_store.save_latest(&failure);
            hooks.write_status("failed").await;
            Err(error)
        }
    }
}

fn relay_protocol_proxy_enabled(settings: &BackendSettings) -> bool {
    settings.active_relay_uses_protocol_proxy()
}

fn select_native_menu_inspector_port(debug_port: u16) -> u16 {
    let requested = debug_port.saturating_add(100);
    crate::ports::select_platform_loopback_port(requested)
}

fn start_native_menu_localizer(inspector_port: u16) {
    if inspector_port == 0 {
        return;
    }
    tokio::spawn(async move {
        if let Err(error) = crate::native_menu::install_native_menu_localizer(inspector_port).await
        {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "native_menu.localization_failed",
                serde_json::json!({
                    "inspector_port": inspector_port,
                    "message": error.to_string()
                }),
            );
        }
    });
}

#[cfg(windows)]
fn apply_codexplusplus_window_icon_after_launch(process_id: u32) {
    let icon_resource_path =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("codex-plus-plus.exe"));
    tokio::spawn(async move {
        for attempt in 1..=30 {
            if crate::windows_apply_codexplusplus_icon_to_process_window(
                process_id,
                icon_resource_path.clone(),
            ) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if attempt == 30 {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "launcher.window_icon.apply_failed",
                    serde_json::json!({
                        "process_id": process_id,
                        "icon_resource_path": icon_resource_path.to_string_lossy()
                    }),
                );
            }
        }
    });
}

#[cfg(not(windows))]
fn apply_codexplusplus_window_icon_after_launch(_process_id: u32) {}

pub trait IntoLaunchHooks {
    fn into_launch_hooks(self) -> Arc<dyn LaunchHooks>;
}

impl<T> IntoLaunchHooks for &T
where
    T: LaunchHooks + Clone + 'static,
{
    fn into_launch_hooks(self) -> Arc<dyn LaunchHooks> {
        Arc::new(self.clone())
    }
}

impl IntoLaunchHooks for Arc<dyn LaunchHooks> {
    fn into_launch_hooks(self) -> Arc<dyn LaunchHooks> {
        self
    }
}

impl IntoLaunchHooks for DefaultLaunchHooks {
    fn into_launch_hooks(self) -> Arc<dyn LaunchHooks> {
        Arc::new(self)
    }
}

impl DefaultLaunchHooks {
    pub fn shared() -> Arc<dyn LaunchHooks> {
        Arc::new(Self::default())
    }
}

fn helper_bind_host() -> String {
    std::env::var("CODEX_PLUS_HELPER_BIND")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

#[async_trait(?Send)]
impl LaunchHooks for DefaultLaunchHooks {
    fn resolve_app_dir(
        &self,
        app_dir: Option<&Path>,
        settings: &BackendSettings,
    ) -> anyhow::Result<PathBuf> {
        crate::app_paths::resolve_codex_app_dir_with_saved(
            app_dir,
            Some(settings.codex_app_path.as_str()),
        )
        .ok_or_else(|| anyhow::anyhow!("Codex App directory not found"))
    }

    fn select_debug_port(&self, requested: u16) -> u16 {
        crate::ports::select_packaged_codex_debug_port(requested)
    }

    fn select_helper_port(&self, requested: u16) -> u16 {
        crate::ports::select_platform_loopback_port(requested)
    }

    async fn load_settings(&self) -> anyhow::Result<BackendSettings> {
        SettingsStore::default().load()
    }

    async fn run_provider_sync(&self) -> anyhow::Result<()> {
        anyhow::bail!("provider sync requires launcher hooks with codex-plus-data integration")
    }

    async fn apply_active_relay_profile(&self, settings: &BackendSettings) -> anyhow::Result<()> {
        if !settings.relay_profiles_enabled {
            return Ok(());
        }
        let profile = settings.active_relay_profile();
        let home = crate::relay_config::default_codex_home_dir();
        let common_config = crate::relay_config::normalize_config_text(
            &[
                settings.relay_common_config_contents.as_str(),
                settings.relay_context_config_contents.as_str(),
            ]
            .into_iter()
            .map(str::trim)
            .filter(|section| !section.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        );
        if profile.relay_mode == crate::settings::RelayMode::Official
            && !profile.official_mix_api_key
        {
            let auth_contents = (!profile.auth_contents.trim().is_empty())
                .then_some(profile.auth_contents.as_str());
            crate::relay_config::clear_relay_config_to_home_with_auth_and_computer_use_guard(
                &home,
                auth_contents,
                settings.computer_use_guard_enabled,
            )?;
            return Ok(());
        }
        crate::relay_config::apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
            &home,
            &profile,
            &common_config,
            settings.computer_use_guard_enabled,
        )?;
        Ok(())
    }

    async fn ensure_computer_use_config(&self, settings: &BackendSettings) -> anyhow::Result<()> {
        if !settings.computer_use_guard_enabled {
            return Ok(());
        }
        let home = crate::relay_config::default_codex_home_dir();
        let artifacts = crate::computer_use_guard::resolve_computer_use_guard_artifacts(&home)?;
        crate::computer_use_guard::ensure_computer_use_config_with_artifacts(&home, &artifacts)?;
        *self.computer_use_guard_artifacts.lock().await = Some(artifacts);
        Ok(())
    }

    async fn ensure_plugin_marketplace_config(
        &self,
        settings: &BackendSettings,
    ) -> anyhow::Result<()> {
        if !settings.codex_app_plugin_marketplace_unlock {
            return Ok(());
        }
        let home = crate::relay_config::default_codex_home_dir();
        match crate::plugin_marketplace::ensure_openai_curated_marketplace_config(&home) {
            Ok(configured) => {
                if configured {
                    let _ = crate::diagnostic_log::append_diagnostic_log(
                        "launcher.openai_curated_marketplace_configured",
                        serde_json::json!({
                            "home": home,
                        }),
                    );
                }
            }
            Err(error) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "launcher.openai_curated_marketplace_config_failed",
                    serde_json::json!({
                        "home": home,
                        "message": error.to_string(),
                    }),
                );
            }
        }
        match crate::plugin_marketplace::ensure_role_specific_plugins_marketplace_config(&home) {
            Ok(configured) => {
                if configured {
                    let _ = crate::diagnostic_log::append_diagnostic_log(
                        "launcher.role_specific_plugins_marketplace_configured",
                        serde_json::json!({
                            "home": home,
                        }),
                    );
                }
            }
            Err(error) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "launcher.role_specific_plugins_marketplace_config_failed",
                    serde_json::json!({
                        "home": home,
                        "message": error.to_string(),
                    }),
                );
            }
        }
        Ok(())
    }

    async fn start_helper(&self, helper_port: u16) -> anyhow::Result<()> {
        let bind_host = helper_bind_host();
        let listener = tokio::net::TcpListener::bind((bind_host.as_str(), helper_port))
            .await
            .with_context(|| {
                format!("failed to bind helper runtime on {bind_host}:{helper_port}")
            })?;
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "helper.listening",
            serde_json::json!({
                "helper_port": helper_port,
                "bind_host": bind_host,
                "address": format!("http://{bind_host}:{helper_port}")
            }),
        );
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => {
                        if let Ok((stream, addr)) = accepted {
                            tokio::spawn(async move {
                                let _ = handle_helper_connection(stream, Some(addr)).await;
                            });
                        }
                    }
                }
            }
        });
        *self.helper.lock().await = Some(HelperRuntime {
            shutdown: shutdown_tx,
            task,
        });
        Ok(())
    }

    async fn launch_codex(
        &self,
        app_dir: &Path,
        debug_port: u16,
        settings: &BackendSettings,
        extra_args: &[String],
    ) -> anyhow::Result<CodexLaunch> {
        let native_menu_localization_enabled = settings.codex_app_native_menu_localization;
        let native_menu_inspector_port =
            native_menu_localization_enabled.then(|| select_native_menu_inspector_port(debug_port));
        let launch_extra_args = codex_extra_args_for_launch(settings, extra_args);
        if cfg!(windows) {
            let activation = if let Some(inspector_port) = native_menu_inspector_port {
                build_packaged_activation_with_native_menu_inspector(
                    app_dir,
                    debug_port,
                    inspector_port,
                    &launch_extra_args,
                )
            } else {
                build_packaged_activation(app_dir, debug_port, &launch_extra_args)
            };
            if let Some(activation) = activation {
                let CodexLaunch::PackagedActivation {
                    app_user_model_id,
                    arguments,
                    ..
                } = &activation
                else {
                    unreachable!();
                };
                let process_id = activate_packaged_app(app_user_model_id, arguments).await?;
                apply_codexplusplus_window_icon_after_launch(process_id);
                if let Some(inspector_port) = native_menu_inspector_port {
                    start_native_menu_localizer(inspector_port);
                }
                return Ok(match activation {
                    CodexLaunch::PackagedActivation {
                        app_user_model_id,
                        arguments,
                        ..
                    } => CodexLaunch::PackagedActivation {
                        app_user_model_id,
                        arguments,
                        process_id: Some(process_id),
                    },
                    CodexLaunch::Process { .. } => unreachable!(),
                });
            }
        }

        if app_dir.extension().and_then(|value| value.to_str()) == Some("app") {
            let cleanup_policy = if is_macos_app_running(app_dir).await {
                MacosCleanupPolicy::SkipQuitBecauseAlreadyRunning
            } else {
                MacosCleanupPolicy::QuitIfNotPreviouslyRunning
            };
            let command = if let Some(inspector_port) = native_menu_inspector_port {
                build_macos_open_command_with_native_menu_inspector(
                    app_dir,
                    debug_port,
                    inspector_port,
                    &launch_extra_args,
                )
            } else {
                build_macos_open_command(app_dir, debug_port, &launch_extra_args)
            };
            let executable = command
                .first()
                .ok_or_else(|| anyhow::anyhow!("macOS open command is empty"))?;
            let child = Command::new(executable)
                .args(&command[1..])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("failed to launch macOS Codex app")?;
            *self.child.lock().await = Some(child);
            if let Some(inspector_port) = native_menu_inspector_port {
                start_native_menu_localizer(inspector_port);
            }
            return Ok(CodexLaunch::Process {
                command,
                wait_strategy: ProcessWaitStrategy::ExternalWaitCommand,
                macos_cleanup_policy: Some(cleanup_policy),
            });
        }

        let command = if let Some(inspector_port) = native_menu_inspector_port {
            build_codex_command_with_native_menu_inspector(
                app_dir,
                debug_port,
                inspector_port,
                &launch_extra_args,
            )
        } else {
            build_codex_command(app_dir, debug_port, &launch_extra_args)
        };
        let executable = command
            .first()
            .ok_or_else(|| anyhow::anyhow!("Codex command is empty"))?;
        let mut child_command = Command::new(executable);
        child_command
            .args(&command[1..])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        child_command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
        let child = child_command
            .spawn()
            .with_context(|| format!("failed to launch Codex executable {executable}"))?;
        *self.child.lock().await = Some(child);
        if let Some(inspector_port) = native_menu_inspector_port {
            start_native_menu_localizer(inspector_port);
        }
        Ok(CodexLaunch::Process {
            command,
            wait_strategy: ProcessWaitStrategy::TrackedChild,
            macos_cleanup_policy: None,
        })
    }

    async fn inject(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        retry_injection(debug_port, helper_port).await
    }
    async fn start_bridge_watchdog(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            #[cfg(windows)]
            let pet_cursor_task = tokio::spawn(run_pet_real_mouse_cursor_driver(debug_port));
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    _ = interval.tick() => {
                        let (pet_result, _) = tokio::join!(
                            sync_pet_real_mouse_overlay(debug_port, helper_port),
                            check_and_reinject_bridge(debug_port, helper_port),
                        );
                        record_pet_overlay_sync_result(debug_port, helper_port, pet_result);
                    }
                }
            }
            #[cfg(windows)]
            {
                pet_cursor_task.abort();
                let _ = pet_cursor_task.await;
            }
        });
        if let Some(runtime) = self
            .bridge_watchdog
            .lock()
            .await
            .replace(BridgeWatchdogRuntime { shutdown, task })
        {
            let _ = runtime.shutdown.send(());
            let _ = runtime.task.await;
        }
        Ok(())
    }

    async fn start_computer_use_guard_watchdog(
        &self,
        settings: &BackendSettings,
    ) -> anyhow::Result<()> {
        #[cfg(windows)]
        {
            if !settings.computer_use_guard_enabled {
                return Ok(());
            }
            let home = crate::relay_config::default_codex_home_dir();
            let artifacts = self.computer_use_guard_artifacts.lock().await.clone();
            let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move {
                run_post_launch_computer_use_guard(home, artifacts, &mut shutdown_rx).await;
            });
            if let Some(runtime) = self
                .computer_use_guard_watchdog
                .lock()
                .await
                .replace(ComputerUseGuardWatchdogRuntime { shutdown, task })
            {
                let _ = runtime.shutdown.send(());
                let _ = runtime.task.await;
            }
        }
        #[cfg(target_os = "macos")]
        {
            let _ = &settings;
            let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = &mut shutdown_rx => break,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {
                            crate::computer_use_guard::kill_orphaned_computer_use_processes();
                        }
                    }
                }
            });
            if let Some(runtime) = self
                .computer_use_guard_watchdog
                .lock()
                .await
                .replace(ComputerUseGuardWatchdogRuntime { shutdown, task })
            {
                let _ = runtime.shutdown.send(());
                let _ = runtime.task.await;
            }
        }
        Ok(())
    }

    async fn write_status(&self, _status: &str) {}

    async fn wait_for_codex_exit(&self, launch: &CodexLaunch) -> anyhow::Result<()> {
        match launch {
            CodexLaunch::Process { .. } => {
                if let Some(mut child) = self.child.lock().await.take() {
                    let _ = child.wait().await;
                }
            }
            CodexLaunch::PackagedActivation { process_id, .. } => {
                if let Some(process_id) = process_id {
                    wait_for_windows_process_id(*process_id).await?;
                }
            }
        }
        let mut empty_streak = 0u32;
        loop {
            if crate::watcher::find_codex_processes().is_empty() {
                empty_streak = empty_streak.saturating_add(1);
                if empty_streak >= 3 {
                    break;
                }
            } else {
                empty_streak = 0;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        Ok(())
    }

    async fn shutdown_helper(&self, _helper_port: u16) {
        if let Some(runtime) = self.computer_use_guard_watchdog.lock().await.take() {
            let _ = runtime.shutdown.send(());
            let _ = runtime.task.await;
        }
        if let Some(runtime) = self.bridge_watchdog.lock().await.take() {
            let _ = runtime.shutdown.send(());
            let _ = runtime.task.await;
        }
        if let Some(runtime) = self.helper.lock().await.take() {
            let _ = runtime.shutdown.send(());
            let _ = runtime.task.await;
        }
    }

    async fn terminate_codex(&self, launch: &CodexLaunch) {
        match launch {
            CodexLaunch::Process {
                wait_strategy: ProcessWaitStrategy::ExternalWaitCommand,
                command,
                macos_cleanup_policy,
            } => {
                if let Some(mut child) = self.child.lock().await.take() {
                    let _ = child.kill().await;
                }
                if let (Some(app_dir), Some(cleanup_policy)) = (
                    macos_app_dir_from_open_command(command),
                    *macos_cleanup_policy,
                ) {
                    let _ = run_macos_cleanup_command(&app_dir, cleanup_policy).await;
                }
            }
            CodexLaunch::Process { .. } => {
                if let Some(mut child) = self.child.lock().await.take() {
                    let _ = child.kill().await;
                }
            }
            CodexLaunch::PackagedActivation {
                process_id: Some(process_id),
                ..
            } => {
                let _ = terminate_windows_process_id(*process_id).await;
            }
            CodexLaunch::PackagedActivation {
                process_id: None, ..
            } => {}
        }
    }
}

async fn handle_helper_connection(
    mut stream: tokio::net::TcpStream,
    remote_addr: Option<SocketAddr>,
) -> anyhow::Result<()> {
    let request_bytes = read_http_request(&mut stream).await?;
    let request = String::from_utf8_lossy(&request_bytes);
    let request_line = request.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let raw_path = parts.next().unwrap_or_default();
    let path = raw_path.split('?').next().unwrap_or(raw_path);
    let request_body = http_request_body(&request);
    let request_user_agent = header_value_from_request(&request, "user-agent");
    let remote_addr_text = remote_addr.map(|addr| addr.to_string());

    let _ = crate::diagnostic_log::append_diagnostic_log(
        "helper.request",
        serde_json::json!({
            "method": method,
            "path": path,
            "request_line": request_line,
            "remote_addr": remote_addr_text,
            "body_bytes": request_body.len()
        }),
    );

    if crate::protocol_proxy::is_responses_proxy_path(path) && method == "POST" {
        return handle_protocol_proxy_connection(
            &mut stream,
            request_body,
            request_user_agent.as_deref(),
            method,
            path,
            remote_addr_text,
        )
        .await;
    }
    if crate::protocol_proxy::is_chat_completions_proxy_path(path) && method == "POST" {
        return handle_chat_completions_proxy_connection(
            &mut stream,
            request_body,
            request_user_agent.as_deref(),
            method,
            path,
            remote_addr_text,
        )
        .await;
    }
    if crate::protocol_proxy::is_models_proxy_path(path) && matches!(method, "GET" | "OPTIONS") {
        return handle_models_proxy_connection(
            &mut stream,
            request_user_agent.as_deref(),
            method,
            path,
            remote_addr_text,
        )
        .await;
    }

    let (status, body, content_type, log_event) = if path == "/backend/status"
        && matches!(method, "GET" | "POST" | "OPTIONS")
    {
        (
            "200 OK".to_string(),
            serde_json::to_vec(&serde_json::json!({
                "status": "ok",
                "message": "后端已连接",
                "version": crate::version::VERSION,
                "transport": "http-helper"
            }))?,
            "application/json; charset=utf-8".to_string(),
            "helper.backend_status_ok",
        )
    } else if path == "/diagnostics/log" && matches!(method, "POST" | "OPTIONS") {
        if method == "POST" {
            let detail =
                serde_json::from_str::<serde_json::Value>(request_body).unwrap_or_else(|error| {
                    serde_json::json!({
                        "parse_error": error.to_string(),
                        "raw": request_body
                    })
                });
            let event = detail
                .get("event")
                .and_then(serde_json::Value::as_str)
                .map(sanitize_diagnostic_event)
                .unwrap_or_else(|| "event".to_string());
            let _ =
                crate::diagnostic_log::append_diagnostic_log(&format!("renderer.{event}"), detail);
        }
        (
            "200 OK".to_string(),
            serde_json::to_vec(&serde_json::json!({
                "status": "ok",
                "message": "日志已记录"
            }))?,
            "application/json; charset=utf-8".to_string(),
            "helper.diagnostics_log_ok",
        )
    } else if path == "/overlay/image" && matches!(method, "GET" | "OPTIONS") {
        if method == "OPTIONS" {
            (
                "200 OK".to_string(),
                Vec::new(),
                "application/octet-stream".to_string(),
                "helper.overlay_image_options",
            )
        } else {
            overlay_image_response()
        }
    } else {
        (
            "404 Not Found".to_string(),
            serde_json::to_vec(&serde_json::json!({
                "status": "failed",
                "message": "未知后端路径"
            }))?,
            "application/json; charset=utf-8".to_string(),
            "helper.unknown_path",
        )
    };
    let _ = crate::diagnostic_log::append_diagnostic_log(
        log_event,
        serde_json::json!({
            "method": method,
            "path": path,
            "status": status,
            "remote_addr": remote_addr_text
        }),
    );
    let response = if method == "OPTIONS" {
        format!(
            "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
    } else {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    };
    stream.write_all(response.as_bytes()).await?;
    if method != "OPTIONS" {
        stream.write_all(&body).await?;
    }
    stream.shutdown().await?;
    Ok(())
}

fn overlay_image_response() -> (String, Vec<u8>, String, &'static str) {
    let not_found = || {
        (
            "404 Not Found".to_string(),
            serde_json::to_vec(&serde_json::json!({
                "status": "failed",
                "message": "图片覆盖层未启用或图片不可用"
            }))
            .unwrap_or_default(),
            "application/json; charset=utf-8".to_string(),
            "helper.overlay_image_not_found",
        )
    };
    let settings = SettingsStore::default().load().unwrap_or_default();
    if !settings.codex_app_image_overlay_enabled {
        return not_found();
    }
    let image_path = PathBuf::from(settings.codex_app_image_overlay_path.trim());
    if image_path.as_os_str().is_empty() || !image_path.is_file() {
        return not_found();
    }
    let Some(content_type) = overlay_image_content_type(&image_path) else {
        return not_found();
    };
    match std::fs::read(&image_path) {
        Ok(bytes) => (
            "200 OK".to_string(),
            bytes,
            content_type.to_string(),
            "helper.overlay_image_ok",
        ),
        Err(_) => not_found(),
    }
}

#[cfg(windows)]
fn windows_logical_cursor_position() -> anyhow::Result<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED, SetThreadDpiAwarenessContext,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let previous = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED) };
    if previous.0.is_null() {
        anyhow::bail!("SetThreadDpiAwarenessContext failed");
    }
    let mut point = POINT::default();
    let result = unsafe { GetCursorPos(&mut point) };
    unsafe {
        SetThreadDpiAwarenessContext(previous);
    }
    result.ok().context("GetCursorPos failed")?;
    Ok((point.x, point.y))
}

fn overlay_image_content_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        _ => None,
    }
}

async fn handle_models_proxy_connection(
    stream: &mut tokio::net::TcpStream,
    request_user_agent: Option<&str>,
    method: &str,
    path: &str,
    remote_addr_text: Option<String>,
) -> anyhow::Result<()> {
    if method == "OPTIONS" {
        write_http_response(
            stream,
            "204 No Content",
            "application/json; charset=utf-8",
            &[],
        )
        .await?;
        stream.shutdown().await?;
        return Ok(());
    }
    let upstream = match crate::protocol_proxy::open_models_proxy_request(request_user_agent).await
    {
        Ok(upstream) => upstream,
        Err(error) => {
            let body = serde_json::to_vec(
                &serde_json::json!({                 "status": "failed",                 "message": error.to_string()             }),
            )?;
            write_http_response(
                stream,
                "502 Bad Gateway",
                "application/json; charset=utf-8",
                &body,
            )
            .await?;
            log_helper_response(
                "helper.models_proxy_failed",
                method,
                path,
                "502 Bad Gateway",
                remote_addr_text,
            );
            stream.shutdown().await?;
            return Ok(());
        }
    };
    let status = upstream.status();
    let is_success = upstream.is_success();
    let content_type = if upstream.content_type.is_empty() {
        "application/json; charset=utf-8".to_string()
    } else {
        upstream.content_type.clone()
    };
    let body = upstream.response.bytes().await?.to_vec();
    write_http_response(stream, &status, &content_type, &body).await?;
    log_helper_response(
        if is_success {
            "helper.models_proxy_ok"
        } else {
            "helper.models_proxy_upstream_error"
        },
        method,
        path,
        &status,
        remote_addr_text,
    );
    stream.shutdown().await?;
    Ok(())
}
async fn handle_protocol_proxy_connection(
    stream: &mut tokio::net::TcpStream,
    request_body: &str,
    request_user_agent: Option<&str>,
    method: &str,
    path: &str,
    remote_addr_text: Option<String>,
) -> anyhow::Result<()> {
    let request_json = serde_json::from_str::<serde_json::Value>(request_body).ok();
    let upstream = match crate::protocol_proxy::open_responses_proxy_request(
        request_body,
        request_user_agent,
    )
    .await
    {
        Ok(upstream) => upstream,
        Err(error) => {
            let body = serde_json::to_vec(
                &serde_json::json!({                     "status": "failed",                     "message": error.to_string()                 }),
            )?;
            write_http_response(
                stream,
                "502 Bad Gateway",
                "application/json; charset=utf-8",
                &body,
            )
            .await?;
            log_helper_response(
                "helper.protocol_proxy_failed",
                method,
                path,
                "502 Bad Gateway",
                remote_addr_text,
            );
            stream.shutdown().await?;
            return Ok(());
        }
    };
    if !upstream.is_success() {
        let status = upstream.status();
        let upstream_content_type = upstream.content_type.clone();
        let upstream_body = upstream.response.bytes().await?.to_vec();
        let error = crate::protocol_proxy::responses_error_from_upstream(
            upstream.status_code,
            &upstream_content_type,
            &upstream_body,
        );
        let body = serde_json::to_vec(&error)?;
        write_http_response(stream, &status, "application/json; charset=utf-8", &body).await?;
        log_helper_response(
            "helper.protocol_proxy_upstream_error",
            method,
            path,
            &status,
            remote_addr_text,
        );
        stream.shutdown().await?;
        return Ok(());
    }
    if upstream.is_stream {
        write_http_stream_headers(stream, "200 OK", "text/event-stream; charset=utf-8").await?;
        if upstream.wire_api == crate::protocol_proxy::UpstreamWireApi::Responses {
            let mut bytes_stream = upstream.response.bytes_stream();
            while let Some(chunk) = bytes_stream.next().await {
                if let Ok(bytes) = chunk {
                    stream.write_all(&bytes).await?;
                } else {
                    break;
                }
            }
            log_helper_response(
                "helper.protocol_proxy_stream_ok",
                method,
                path,
                "200 OK",
                remote_addr_text,
            );
            stream.shutdown().await?;
            return Ok(());
        }
        let mut converter = request_json
            .as_ref()
            .map(crate::protocol_proxy::ChatSseToResponsesConverter::with_request)
            .unwrap_or_default();
        let mut bytes_stream = upstream.response.bytes_stream();
        let mut stream_failed = false;
        while let Some(chunk) = bytes_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let converted = converter.push_bytes(&bytes);
                    if !converted.is_empty() {
                        stream.write_all(&converted).await?;
                    }
                }
                Err(error) => {
                    let failed = converter.fail(
                        format!("Stream error: {error}"),
                        Some("stream_error".to_string()),
                    );
                    if !failed.is_empty() {
                        stream.write_all(&failed).await?;
                    }
                    stream_failed = true;
                    break;
                }
            }
        }
        if !stream_failed {
            let tail = converter.finish();
            if !tail.is_empty() {
                stream.write_all(&tail).await?;
            }
        }
        log_helper_response(
            "helper.protocol_proxy_stream_ok",
            method,
            path,
            "200 OK",
            remote_addr_text,
        );
        stream.shutdown().await?;
        return Ok(());
    }
    let upstream_body = upstream.response.bytes().await?;
    if upstream.wire_api == crate::protocol_proxy::UpstreamWireApi::Responses {
        write_http_response(
            stream,
            "200 OK",
            if upstream.content_type.is_empty() {
                "application/json; charset=utf-8"
            } else {
                &upstream.content_type
            },
            &upstream_body,
        )
        .await?;
        log_helper_response(
            "helper.protocol_proxy_ok",
            method,
            path,
            "200 OK",
            remote_addr_text,
        );
        stream.shutdown().await?;
        return Ok(());
    }
    let chat_json: serde_json::Value = serde_json::from_slice(&upstream_body)?;
    let response_json = if let Some(request_json) = request_json.as_ref() {
        crate::protocol_proxy::chat_completion_to_response_with_request(chat_json, request_json)?
    } else {
        crate::protocol_proxy::chat_completion_to_response(chat_json)?
    };
    let body = serde_json::to_vec(&response_json)?;
    write_http_response(stream, "200 OK", "application/json; charset=utf-8", &body).await?;
    log_helper_response(
        "helper.protocol_proxy_ok",
        method,
        path,
        "200 OK",
        remote_addr_text,
    );
    stream.shutdown().await?;
    Ok(())
}
async fn handle_chat_completions_proxy_connection(
    stream: &mut tokio::net::TcpStream,
    request_body: &str,
    request_user_agent: Option<&str>,
    method: &str,
    path: &str,
    remote_addr_text: Option<String>,
) -> anyhow::Result<()> {
    let upstream = match crate::protocol_proxy::open_chat_completions_proxy_request(
        request_body,
        request_user_agent,
    )
    .await
    {
        Ok(upstream) => upstream,
        Err(error) => {
            let body = serde_json::to_vec(
                &serde_json::json!({                 "status": "failed",                 "message": error.to_string()             }),
            )?;
            write_http_response(
                stream,
                "502 Bad Gateway",
                "application/json; charset=utf-8",
                &body,
            )
            .await?;
            log_helper_response(
                "helper.chat_completions_proxy_failed",
                method,
                path,
                "502 Bad Gateway",
                remote_addr_text,
            );
            stream.shutdown().await?;
            return Ok(());
        }
    };
    let status = upstream.status();
    let is_success = upstream.is_success();
    let content_type = if upstream.content_type.is_empty() {
        "application/json; charset=utf-8".to_string()
    } else {
        upstream.content_type.clone()
    };
    if upstream.is_stream && is_success {
        write_http_stream_headers(stream, &status, &content_type).await?;
        let mut bytes_stream = upstream.response.bytes_stream();
        while let Some(chunk) = bytes_stream.next().await {
            stream.write_all(&chunk?).await?;
        }
        log_helper_response(
            "helper.chat_completions_proxy_stream_ok",
            method,
            path,
            &status,
            remote_addr_text,
        );
        stream.shutdown().await?;
        return Ok(());
    }
    let body = upstream.response.bytes().await?.to_vec();
    write_http_response(stream, &status, &content_type, &body).await?;
    log_helper_response(
        if is_success {
            "helper.chat_completions_proxy_ok"
        } else {
            "helper.chat_completions_proxy_upstream_error"
        },
        method,
        path,
        &status,
        remote_addr_text,
    );
    stream.shutdown().await?;
    Ok(())
}

async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> anyhow::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}

async fn write_http_stream_headers(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    content_type: &str,
) -> anyhow::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

fn log_helper_response(
    event: &str,
    method: &str,
    path: &str,
    status: &str,
    remote_addr_text: Option<String>,
) {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        event,
        serde_json::json!({
            "method": method,
            "path": path,
            "status": status,
            "remote_addr": remote_addr_text
        }),
    );
}

#[cfg(test)]
mod computer_use_tests {
    use super::{header_value_from_request, overlay_image_content_type};
    use std::path::Path;

    #[test]
    fn overlay_image_content_type_accepts_common_images_only() {
        assert_eq!(
            overlay_image_content_type(Path::new("overlay.PNG")),
            Some("image/png")
        );
        assert_eq!(
            overlay_image_content_type(Path::new("overlay.jpeg")),
            Some("image/jpeg")
        );
        assert_eq!(
            overlay_image_content_type(Path::new("overlay.webp")),
            Some("image/webp")
        );
        assert_eq!(overlay_image_content_type(Path::new("overlay.txt")), None);
    }

    #[test]
    fn header_value_from_request_reads_user_agent_case_insensitively() {
        let request = "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nUser-Agent: Codex/26.614\r\nContent-Length: 2\r\n\r\n{}";

        assert_eq!(
            header_value_from_request(request, "user-agent").as_deref(),
            Some("Codex/26.614")
        );
    }
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> anyhow::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut chunk = vec![0_u8; 4096];
    let mut header_end = None;
    let mut content_length = 0_usize;

    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if header_end.is_none() {
            header_end = find_header_end(&buffer);
            if let Some(end) = header_end {
                content_length = content_length_from_headers(&buffer[..end]).unwrap_or(0);
            }
        }
        if let Some(end) = header_end {
            if buffer.len() >= end + 4 + content_length {
                break;
            }
        }
        if buffer.len() > 32 * 1024 * 1024 {
            anyhow::bail!("HTTP 请求过大");
        }
    }

    Ok(buffer)
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length_from_headers(headers: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(headers);
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    })
}

fn http_request_body(request: &str) -> &str {
    request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_default()
}

fn header_value_from_request(request: &str, header_name: &str) -> Option<String> {
    request
        .split_once("\r\n\r\n")
        .map(|(headers, _)| headers)
        .unwrap_or(request)
        .lines()
        .skip(1)
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case(header_name)
                .then(|| value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn sanitize_diagnostic_event(event: &str) -> String {
    let sanitized = event
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "event".to_string()
    } else {
        sanitized
    }
}

pub fn build_codex_arguments(debug_port: u16, extra_args: &[String]) -> Vec<String> {
    let mut args = vec![
        format!("--remote-debugging-port={debug_port}"),
        format!("--remote-allow-origins=http://127.0.0.1:{debug_port}"),
    ];
    args.extend(normalize_codex_extra_args(extra_args));
    args
}

pub fn build_codex_arguments_for_settings(
    debug_port: u16,
    settings: &BackendSettings,
) -> Vec<String> {
    build_codex_arguments(
        debug_port,
        &codex_extra_args_for_launch(settings, &settings.codex_extra_args),
    )
}

fn codex_extra_args_for_launch(settings: &BackendSettings, extra_args: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    if settings.codex_app_fast_startup && !has_host_resolver_rules(extra_args) {
        args.push(statsig_fast_fail_host_resolver_rule());
    }
    args.extend(normalize_codex_extra_args(extra_args));
    args
}

fn has_host_resolver_rules(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg.trim().starts_with("--host-resolver-rules"))
}

fn statsig_fast_fail_host_resolver_rule() -> String {
    [
        "--host-resolver-rules=MAP ab.chatgpt.com 127.0.0.1",
        "MAP featureassets.org 127.0.0.1",
        "MAP prodregistryv2.org 127.0.0.1",
        "MAP api.statsigcdn.com 127.0.0.1",
        "MAP statsigapi.net 127.0.0.1",
        "MAP cloudflare-dns.com 127.0.0.1",
    ]
    .join(",")
}

pub fn build_codex_arguments_with_native_menu_inspector(
    debug_port: u16,
    inspector_port: u16,
    extra_args: &[String],
) -> Vec<String> {
    let mut args = build_codex_arguments(debug_port, &[]);
    if inspector_port != 0 {
        args.push(format!("--inspect=127.0.0.1:{inspector_port}"));
    }
    args.extend(normalize_codex_extra_args(extra_args));
    args
}

pub fn build_codex_command(app_dir: &Path, debug_port: u16, extra_args: &[String]) -> Vec<String> {
    let mut command = vec![
        crate::app_paths::build_codex_executable(app_dir)
            .to_string_lossy()
            .to_string(),
    ];
    command.extend(build_codex_arguments(debug_port, extra_args));
    command
}

pub fn build_codex_command_with_native_menu_inspector(
    app_dir: &Path,
    debug_port: u16,
    inspector_port: u16,
    extra_args: &[String],
) -> Vec<String> {
    let mut command = vec![
        crate::app_paths::build_codex_executable(app_dir)
            .to_string_lossy()
            .to_string(),
    ];
    command.extend(build_codex_arguments_with_native_menu_inspector(
        debug_port,
        inspector_port,
        extra_args,
    ));
    command
}

pub fn build_packaged_activation(
    app_dir: &Path,
    debug_port: u16,
    extra_args: &[String],
) -> Option<CodexLaunch> {
    Some(CodexLaunch::PackagedActivation {
        app_user_model_id: crate::app_paths::packaged_app_user_model_id(app_dir)?,
        arguments: command_line_arguments(&build_codex_arguments(debug_port, extra_args)),
        process_id: None,
    })
}

pub fn build_packaged_activation_with_native_menu_inspector(
    app_dir: &Path,
    debug_port: u16,
    inspector_port: u16,
    extra_args: &[String],
) -> Option<CodexLaunch> {
    Some(CodexLaunch::PackagedActivation {
        app_user_model_id: crate::app_paths::packaged_app_user_model_id(app_dir)?,
        arguments: command_line_arguments(&build_codex_arguments_with_native_menu_inspector(
            debug_port,
            inspector_port,
            extra_args,
        )),
        process_id: None,
    })
}

async fn retry_injection(debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
    let mut last_error = None;
    for _ in 0..20 {
        match try_inject(debug_port, helper_port).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Codex injection failed")))
}

pub async fn check_and_reinject_bridge(debug_port: u16, helper_port: u16) -> bool {
    let healthy = match bridge_health_ok(debug_port).await {
        Ok(healthy) => healthy,
        Err(error) => {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "bridge.health_check_failed",
                serde_json::json!({
                    "debug_port": debug_port,
                    "helper_port": helper_port,
                    "message": error.to_string()
                }),
            );
            false
        }
    };
    if healthy {
        return false;
    }

    let _ = crate::diagnostic_log::append_diagnostic_log(
        "bridge.reinject_start",
        serde_json::json!({
            "debug_port": debug_port,
            "helper_port": helper_port
        }),
    );
    match retry_injection(debug_port, helper_port).await {
        Ok(()) => {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "bridge.reinject_ok",
                serde_json::json!({
                    "debug_port": debug_port,
                    "helper_port": helper_port
                }),
            );
            true
        }
        Err(error) => {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "bridge.reinject_failed",
                serde_json::json!({
                    "debug_port": debug_port,
                    "helper_port": helper_port,
                    "message": error.to_string()
                }),
            );
            false
        }
    }
}

async fn bridge_health_ok(debug_port: u16) -> anyhow::Result<bool> {
    let targets = crate::cdp::list_targets(debug_port).await?;
    let target = crate::cdp::pick_injectable_codex_page_target(&targets)?;
    let websocket_url = target
        .web_socket_debugger_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("selected CDP target has no websocket URL"))?;
    let result = crate::bridge::evaluate_script_with_await_promise(
        websocket_url,
        crate::bridge::bridge_health_check_script(),
        true,
    )
    .await?;
    Ok(runtime_evaluate_result_is_true(&result))
}

fn runtime_evaluate_result_is_true(result: &Value) -> bool {
    result
        .get("result")
        .and_then(|result| result.get("result"))
        .and_then(|result| result.get("value"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

async fn try_inject(debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
    let targets = crate::cdp::list_targets(debug_port).await?;
    let target = crate::cdp::pick_injectable_codex_page_target(&targets)?;
    let websocket_url = target
        .web_socket_debugger_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("selected CDP target has no websocket URL"))?;
    let settings = SettingsStore::default().load().unwrap_or_default();
    let script = crate::assets::injection_script_with_settings(helper_port, &settings);
    let ctx = crate::routes::BridgeContext::core(Arc::new(crate::routes::CoreRuntimeService::new(
        debug_port,
        StatusStore::default(),
    )));
    crate::bridge::install_bridge(
        websocket_url,
        crate::bridge::BRIDGE_BINDING_NAME,
        Arc::new(move |path, payload| {
            let ctx = ctx.clone();
            Box::pin(
                async move { Ok(crate::routes::handle_bridge_request(ctx, &path, payload).await) },
            )
        }),
        &[script],
    )
    .await
}

async fn confirmed_pet_overlay_targets(
    debug_port: u16,
) -> anyhow::Result<Vec<crate::cdp::CdpTarget>> {
    let targets = crate::cdp::list_targets(debug_port).await?;
    let mut confirmed = Vec::new();
    for target in targets
        .into_iter()
        .filter(crate::cdp::is_avatar_overlay_page_target)
    {
        let Some(websocket_url) = target.web_socket_debugger_url.as_deref() else {
            continue;
        };
        if pet_overlay_supports_v2_cursor(websocket_url).await {
            confirmed.push(target);
        }
    }
    Ok(confirmed)
}

async fn pet_overlay_supports_v2_cursor(websocket_url: &str) -> bool {
    crate::bridge::evaluate_script_with_await_promise(
        websocket_url,
        &crate::assets::pet_real_mouse_capability_probe_script(),
        true,
    )
    .await
    .as_ref()
    .is_ok_and(runtime_evaluate_result_is_true)
}

async fn sync_pet_real_mouse_overlay(debug_port: u16, _helper_port: u16) -> anyhow::Result<()> {
    let settings = SettingsStore::default().load().unwrap_or_default();
    let enabled = settings.enhancements_enabled && settings.codex_app_pet_real_mouse_look;
    let targets = crate::cdp::list_targets(debug_port).await?;
    for target in targets
        .iter()
        .filter(|target| crate::cdp::is_avatar_overlay_page_target(target))
    {
        let Some(websocket_url) = target.web_socket_debugger_url.as_deref() else {
            continue;
        };
        let supports_v2 = enabled && pet_overlay_supports_v2_cursor(websocket_url).await;
        let script = if supports_v2 {
            crate::assets::pet_real_mouse_script()
        } else {
            crate::assets::pet_real_mouse_stop_script()
        };
        crate::bridge::evaluate_script(websocket_url, script)
            .await
            .with_context(|| {
                format!(
                    "failed to evaluate pet overlay script in target {} ({})",
                    target.id, target.url
                )
            })?;
    }
    Ok(())
}

#[cfg(windows)]
async fn run_pet_real_mouse_cursor_driver(debug_port: u16) {
    loop {
        let settings = SettingsStore::default().load().unwrap_or_default();
        if !settings.enhancements_enabled || !settings.codex_app_pet_real_mouse_look {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            continue;
        }

        let targets = confirmed_pet_overlay_targets(debug_port)
            .await
            .unwrap_or_default();
        if targets.is_empty() {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }
        let mut drivers = tokio::task::JoinSet::new();
        for target in targets.iter().cloned() {
            drivers.spawn(run_pet_real_mouse_target_driver(debug_port, target));
        }
        if let Some(result) = drivers.join_next().await {
            if let Err(error) = result {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "pet.real_mouse_cursor_driver_join_failed",
                    serde_json::json!({
                        "debug_port": debug_port,
                        "message": error.to_string()
                    }),
                );
            }
        }
        for target in &targets {
            if let Some(websocket_url) = target.web_socket_debugger_url.as_deref() {
                let _ = crate::bridge::evaluate_script(
                    websocket_url,
                    crate::assets::pet_real_mouse_stop_script(),
                )
                .await;
            }
        }
        drivers.abort_all();
        while drivers.join_next().await.is_some() {}
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

#[cfg(windows)]
async fn run_pet_real_mouse_target_driver(debug_port: u16, target: crate::cdp::CdpTarget) {
    let Some(websocket_url) = target.web_socket_debugger_url.as_deref() else {
        return;
    };
    if let Err(error) =
        crate::bridge::evaluate_script(websocket_url, crate::assets::pet_real_mouse_script()).await
    {
        record_pet_cursor_driver_failure(debug_port, &target, error);
        return;
    }

    let mut ticks_until_settings_check = 10_u8;
    let result = crate::bridge::run_periodic_evaluations(
        websocket_url,
        std::time::Duration::from_millis(100),
        || {
            if ticks_until_settings_check == 0 {
                let settings = SettingsStore::default().load().unwrap_or_default();
                if !settings.enhancements_enabled || !settings.codex_app_pet_real_mouse_look {
                    return Ok(None);
                }
                ticks_until_settings_check = 10;
            }
            ticks_until_settings_check -= 1;
            let (x, y) = windows_logical_cursor_position()?;
            Ok(Some(crate::assets::pet_real_mouse_update_script(x, y)))
        },
    )
    .await;

    let _ =
        crate::bridge::evaluate_script(websocket_url, crate::assets::pet_real_mouse_stop_script())
            .await;
    match result {
        Ok(()) => {
            PET_CURSOR_DRIVER_FAILED.store(false, Ordering::Relaxed);
        }
        Err(error) => record_pet_cursor_driver_failure(debug_port, &target, error),
    }
}

#[cfg(windows)]
fn record_pet_cursor_driver_failure(
    debug_port: u16,
    target: &crate::cdp::CdpTarget,
    error: anyhow::Error,
) {
    if !PET_CURSOR_DRIVER_FAILED.swap(true, Ordering::Relaxed) {
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "pet.real_mouse_cursor_driver_disconnected",
            serde_json::json!({
                "debug_port": debug_port,
                "target_id": target.id,
                "target_url": target.url,
                "message": format!("{error:#}")
            }),
        );
    }
}

fn record_pet_overlay_sync_result(debug_port: u16, helper_port: u16, result: anyhow::Result<()>) {
    match result {
        Ok(()) => {
            if PET_OVERLAY_SYNC_FAILED.swap(false, Ordering::Relaxed) {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "pet.real_mouse_overlay_sync_recovered",
                    serde_json::json!({
                        "debug_port": debug_port,
                        "helper_port": helper_port
                    }),
                );
            }
        }
        Err(error) => {
            if !PET_OVERLAY_SYNC_FAILED.swap(true, Ordering::Relaxed) {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "pet.real_mouse_overlay_sync_failed",
                    serde_json::json!({
                        "debug_port": debug_port,
                        "helper_port": helper_port,
                        "message": format!("{error:#}")
                    }),
                );
            }
        }
    }
}

pub fn build_macos_open_command(
    app_dir: &Path,
    debug_port: u16,
    extra_args: &[String],
) -> Vec<String> {
    let mut command = vec![
        "open".to_string(),
        "-W".to_string(),
        "-a".to_string(),
        app_dir.to_string_lossy().to_string(),
        "--args".to_string(),
    ];
    command.extend(build_codex_arguments(debug_port, extra_args));
    command
}

pub fn build_macos_open_command_with_native_menu_inspector(
    app_dir: &Path,
    debug_port: u16,
    inspector_port: u16,
    extra_args: &[String],
) -> Vec<String> {
    let mut command = vec![
        "open".to_string(),
        "-W".to_string(),
        "-a".to_string(),
        app_dir.to_string_lossy().to_string(),
        "--args".to_string(),
    ];
    command.extend(build_codex_arguments_with_native_menu_inspector(
        debug_port,
        inspector_port,
        extra_args,
    ));
    command
}

pub fn build_macos_cleanup_command(
    app_dir: &Path,
    policy: MacosCleanupPolicy,
) -> Option<Vec<String>> {
    if policy == MacosCleanupPolicy::SkipQuitBecauseAlreadyRunning {
        return None;
    }
    let app_name = app_dir
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Codex");
    Some(vec![
        "osascript".to_string(),
        "-e".to_string(),
        format!(
            r#"tell application "{}" to quit"#,
            app_name.replace('"', "\\\"")
        ),
    ])
}

async fn run_macos_cleanup_command(
    app_dir: &Path,
    policy: MacosCleanupPolicy,
) -> anyhow::Result<()> {
    let Some(command) = build_macos_cleanup_command(app_dir, policy) else {
        return Ok(());
    };
    let Some(executable) = command.first() else {
        return Ok(());
    };
    let _ = Command::new(executable)
        .args(&command[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .with_context(|| format!("failed to request macOS app quit for {}", app_dir.display()))?;
    Ok(())
}

fn macos_app_dir_from_open_command(command: &[String]) -> Option<PathBuf> {
    let app_index = command.iter().position(|part| part == "-a")?;
    command.get(app_index + 1).map(PathBuf::from)
}

async fn is_macos_app_running(app_dir: &Path) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let app_name = app_dir
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Codex");
    let script = format!(
        r#"application "{}" is running"#,
        app_name.replace('"', "\\\"")
    );
    let Ok(output) = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
    else {
        return false;
    };
    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .trim()
            .eq_ignore_ascii_case("true")
}

#[cfg_attr(not(windows), allow(dead_code))]
fn post_launch_guard_artifacts_ready(
    artifacts: &crate::computer_use_guard::GuardArtifacts,
) -> bool {
    artifacts.notify_exe.is_some()
        && artifacts.marketplace_path.is_some()
        && (!artifacts.runtime_exports_needed || artifacts.sky_package_json.is_some())
}

#[cfg_attr(not(windows), allow(dead_code))]
fn should_stop_post_launch_computer_use_guard(
    stable_unchanged_attempts: usize,
    artifacts: &crate::computer_use_guard::GuardArtifacts,
) -> bool {
    stable_unchanged_attempts >= POST_LAUNCH_COMPUTER_USE_GUARD_STABLE_ATTEMPTS
        && post_launch_guard_artifacts_ready(artifacts)
}

#[cfg(windows)]
async fn run_post_launch_computer_use_guard(
    home: PathBuf,
    mut artifacts: Option<crate::computer_use_guard::GuardArtifacts>,
    shutdown_rx: &mut tokio::sync::oneshot::Receiver<()>,
) {
    let mut previous_delay = 0_u64;
    let mut stable_unchanged_attempts = 0_usize;
    for (index, delay) in POST_LAUNCH_COMPUTER_USE_GUARD_SECONDS
        .iter()
        .copied()
        .enumerate()
    {
        let wait_seconds = delay.saturating_sub(previous_delay);
        previous_delay = delay;
        if wait_seconds > 0 {
            tokio::select! {
                _ = &mut *shutdown_rx => return,
                _ = tokio::time::sleep(std::time::Duration::from_secs(wait_seconds)) => {}
            }
        }
        let attempt = index + 1;
        let resolved_artifacts = match artifacts.take() {
            Some(artifacts) => artifacts,
            None => match crate::computer_use_guard::resolve_computer_use_guard_artifacts(&home) {
                Ok(resolved) => resolved,
                Err(error) => {
                    stable_unchanged_attempts = 0;
                    let _ = crate::diagnostic_log::append_diagnostic_log(
                        "computer_use_guard.post_launch_failed",
                        serde_json::json!({
                            "attempt": attempt,
                            "delay_seconds": delay,
                            "phase": "resolve_artifacts",
                            "message": error.to_string()
                        }),
                    );
                    continue;
                }
            },
        };
        let artifacts_ready = post_launch_guard_artifacts_ready(&resolved_artifacts);
        artifacts = artifacts_ready.then_some(resolved_artifacts.clone());
        match crate::computer_use_guard::ensure_computer_use_config_with_artifacts(
            &home,
            &resolved_artifacts,
        ) {
            Ok(result) => {
                if !result.changed && artifacts_ready {
                    stable_unchanged_attempts += 1;
                } else {
                    stable_unchanged_attempts = 0;
                }
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "computer_use_guard.post_launch_ok",
                    serde_json::json!({
                        "attempt": attempt,
                        "delay_seconds": delay,
                        "changed": result.changed,
                        "stable_unchanged_attempts": stable_unchanged_attempts,
                        "notify_exe": result
                            .notify_exe
                            .map(|path| path.to_string_lossy().to_string())
                    }),
                );
                if should_stop_post_launch_computer_use_guard(
                    stable_unchanged_attempts,
                    &resolved_artifacts,
                ) {
                    let _ = crate::diagnostic_log::append_diagnostic_log(
                        "computer_use_guard.post_launch_stable_stop",
                        serde_json::json!({
                            "attempt": attempt,
                            "delay_seconds": delay,
                            "stable_unchanged_attempts": stable_unchanged_attempts
                        }),
                    );
                    return;
                }
            }
            Err(error) => {
                stable_unchanged_attempts = 0;
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "computer_use_guard.post_launch_failed",
                    serde_json::json!({
                        "attempt": attempt,
                        "delay_seconds": delay,
                        "message": error.to_string()
                    }),
                );
            }
        }
    }
}

#[cfg(windows)]
async fn wait_for_windows_process_id(process_id: u32) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || wait_for_windows_process_id_blocking(process_id))
        .await
        .context("Windows process wait task failed")?
}

#[cfg(windows)]
async fn terminate_windows_process_id(process_id: u32) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || terminate_windows_process_id_blocking(process_id))
        .await
        .context("Windows process termination task failed")?
}

#[cfg(windows)]
fn wait_for_windows_process_id_blocking(process_id: u32) -> anyhow::Result<()> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_FAILED};
    use windows::Win32::System::Threading::{
        INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        WaitForSingleObject,
    };

    unsafe {
        let handle = OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            process_id,
        )
        .with_context(|| format!("failed to open Windows process id {process_id}"))?;
        let wait_result = WaitForSingleObject(handle, INFINITE);
        let _ = CloseHandle(handle);
        if wait_result == WAIT_FAILED {
            anyhow::bail!("failed to wait for Windows process id {process_id}");
        }
    }
    Ok(())
}

#[cfg(windows)]
fn terminate_windows_process_id_blocking(process_id: u32) -> anyhow::Result<()> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
    };

    unsafe {
        let handle = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            process_id,
        )
        .with_context(|| format!("failed to open Windows process id {process_id}"))?;
        let terminate_result = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
        terminate_result
            .with_context(|| format!("failed to terminate Windows process id {process_id}"))?;
    }
    Ok(())
}

#[cfg(not(windows))]
async fn wait_for_windows_process_id(process_id: u32) -> anyhow::Result<()> {
    anyhow::bail!("cannot wait for Windows process id {process_id} on this platform")
}

#[cfg(not(windows))]
async fn terminate_windows_process_id(process_id: u32) -> anyhow::Result<()> {
    anyhow::bail!("cannot terminate Windows process id {process_id} on this platform")
}

fn launch_status(
    status: &str,
    message: &str,
    debug_port: u16,
    helper_port: u16,
    app_dir: &Path,
) -> LaunchStatus {
    LaunchStatus {
        status: status.to_string(),
        message: message.to_string(),
        started_at_ms: now_ms(),
        debug_port: Some(debug_port),
        helper_port: Some(helper_port),
        codex_app: Some(app_dir.to_string_lossy().to_string()),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn command_line_arguments(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote_windows_argument(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_windows_argument(arg: &str) -> String {
    if !arg.is_empty() && !arg.bytes().any(|byte| matches!(byte, b' ' | b'\t' | b'"')) {
        return arg.to_string();
    }
    let mut output = String::from("\"");
    let mut backslashes = 0;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                output.push_str(&"\\".repeat(backslashes * 2 + 1));
                output.push('"');
                backslashes = 0;
            }
            _ => {
                output.push_str(&"\\".repeat(backslashes));
                output.push(ch);
                backslashes = 0;
            }
        }
    }
    output.push_str(&"\\".repeat(backslashes * 2));
    output.push('"');
    output
}

#[cfg(not(windows))]
pub async fn activate_packaged_app(
    _app_user_model_id: &str,
    _arguments: &str,
) -> anyhow::Result<u32> {
    anyhow::bail!("Packaged app activation is only supported on Windows")
}

#[cfg(windows)]
pub async fn activate_packaged_app(
    app_user_model_id: &str,
    arguments: &str,
) -> anyhow::Result<u32> {
    let app_user_model_id = app_user_model_id.to_string();
    let arguments = arguments.to_string();
    tokio::task::spawn_blocking(move || {
        activate_packaged_app_blocking(&app_user_model_id, &arguments)
    })
    .await
    .context("packaged app activation task failed")?
}

#[cfg(windows)]
fn activate_packaged_app_blocking(app_user_model_id: &str, arguments: &str) -> anyhow::Result<u32> {
    use windows::Win32::System::Com::{
        CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Shell::{ApplicationActivationManager, IApplicationActivationManager};
    use windows::core::HSTRING;

    unsafe {
        let coinit = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let should_uninitialize = coinit.is_ok();
        coinit.ok().or_else(|error| {
            const RPC_E_CHANGED_MODE: i32 = -2147417850;
            if error.code().0 == RPC_E_CHANGED_MODE {
                Ok(())
            } else {
                Err(error)
            }
        })?;

        let result: windows::core::Result<u32> = (|| {
            let manager: IApplicationActivationManager =
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)?;
            let process_id = manager.ActivateApplication(
                &HSTRING::from(app_user_model_id),
                &HSTRING::from(arguments),
                windows::Win32::UI::Shell::ACTIVATEOPTIONS(0),
            )?;
            Ok(process_id)
        })();

        if should_uninitialize {
            CoUninitialize();
        }
        result.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_launch_guard_stops_after_stable_ready_artifacts() {
        let artifacts = crate::computer_use_guard::GuardArtifacts {
            notify_exe: Some(PathBuf::from("codex-computer-use.exe")),
            marketplace_path: Some(PathBuf::from("openai-bundled")),
            sky_package_json: None,
            runtime_exports_needed: false,
        };

        assert!(!should_stop_post_launch_computer_use_guard(2, &artifacts));
        assert!(should_stop_post_launch_computer_use_guard(3, &artifacts));
    }

    #[test]
    fn post_launch_guard_keeps_retrying_until_artifacts_are_ready() {
        let missing_notify = crate::computer_use_guard::GuardArtifacts {
            notify_exe: None,
            marketplace_path: Some(PathBuf::from("openai-bundled")),
            sky_package_json: None,
            runtime_exports_needed: false,
        };
        let missing_marketplace = crate::computer_use_guard::GuardArtifacts {
            notify_exe: Some(PathBuf::from("codex-computer-use.exe")),
            marketplace_path: None,
            sky_package_json: None,
            runtime_exports_needed: false,
        };
        let missing_runtime_package = crate::computer_use_guard::GuardArtifacts {
            notify_exe: Some(PathBuf::from("codex-computer-use.exe")),
            marketplace_path: Some(PathBuf::from("openai-bundled")),
            sky_package_json: None,
            runtime_exports_needed: true,
        };

        assert!(!should_stop_post_launch_computer_use_guard(
            3,
            &missing_notify
        ));
        assert!(!should_stop_post_launch_computer_use_guard(
            3,
            &missing_marketplace
        ));
        assert!(!should_stop_post_launch_computer_use_guard(
            3,
            &missing_runtime_package
        ));
    }
}
