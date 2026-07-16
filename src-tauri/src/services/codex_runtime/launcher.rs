//! Launch / attach orchestration for enhanced Codex. NEVER kills ordinary Codex.

use super::cdp;
use super::discovery::{self, CodexProcessInfo};
use super::state::{CodexRuntimeSnapshot, CodexRuntimeState};
use crate::error::AppError;
use crate::services::codex_injection::{self, BridgeHandle};
use crate::settings::get_settings;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

const DEFAULT_CDP_PORT: u16 = 9222;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchEnhancedCodexResult {
    pub state: CodexRuntimeState,
    pub pid: Option<u32>,
    pub cdp_port: Option<u16>,
    pub bridge_port: Option<u16>,
    pub instance_id: Option<String>,
    pub message: Option<String>,
}

/// In-process runtime handle shared via AppState (cloneable Arc core).
#[derive(Clone)]
pub struct CodexRuntimeHandle {
    inner: Arc<Mutex<RuntimeInner>>,
}

#[derive(Default)]
struct RuntimeInner {
    snapshot: CodexRuntimeSnapshot,
    bridge: Option<BridgeHandle>,
    child: Option<std::process::Child>,
    /// Last successfully injected bootstrap bundle (navigation reinject).
    last_bundle: Option<String>,
    /// Cancel background navigation reinject watcher.
    watcher_stop: Option<oneshot::Sender<()>>,
}

impl Default for CodexRuntimeHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexRuntimeHandle {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RuntimeInner::default())),
        }
    }

    pub async fn snapshot(&self) -> CodexRuntimeSnapshot {
        self.inner.lock().await.snapshot.clone()
    }

    pub async fn set_snapshot(&self, snap: CodexRuntimeSnapshot) {
        self.inner.lock().await.snapshot = snap;
    }
}

/// Hook trait for tests.
pub trait LaunchHooks: Send + Sync {
    fn find_running(&self) -> Vec<CodexProcessInfo>;
    fn spawn_with_cdp(&self, exe: &Path, cdp_port: u16) -> Result<u32, AppError>;
    #[allow(dead_code)]
    fn kill_calls(&self) -> u32;
    #[allow(dead_code)]
    fn spawn_calls(&self) -> u32;
}

#[derive(Default)]
pub struct FakeHooks {
    ordinary: bool,
    #[allow(dead_code)]
    kill: AtomicU32,
    spawn: AtomicU32,
}

impl FakeHooks {
    #[allow(dead_code)]
    pub fn ordinary_codex_without_cdp() -> Self {
        Self {
            ordinary: true,
            ..Default::default()
        }
    }
}

impl LaunchHooks for FakeHooks {
    fn find_running(&self) -> Vec<CodexProcessInfo> {
        if self.ordinary {
            vec![CodexProcessInfo {
                pid: 4242,
                has_cdp: false,
                cdp_port: None,
                exe_path: None,
            }]
        } else {
            vec![]
        }
    }

    fn spawn_with_cdp(&self, _exe: &Path, _cdp_port: u16) -> Result<u32, AppError> {
        self.spawn.fetch_add(1, Ordering::SeqCst);
        Ok(1001)
    }

    fn kill_calls(&self) -> u32 {
        self.kill.load(Ordering::SeqCst)
    }

    fn spawn_calls(&self) -> u32 {
        self.spawn.load(Ordering::SeqCst)
    }
}

/// Pure policy: if ordinary Codex is already running without CDP, never kill/relaunch.
#[allow(dead_code)]
pub fn launch_with_hooks(hooks: &dyn LaunchHooks) -> Result<LaunchEnhancedCodexResult, AppError> {
    let running = hooks.find_running();
    if let Some(proc) = running.first() {
        if !proc.has_cdp {
            return Ok(LaunchEnhancedCodexResult {
                state: CodexRuntimeState::OrdinaryRunning,
                pid: Some(proc.pid),
                cdp_port: None,
                bridge_port: None,
                instance_id: None,
                message: Some(
                    "检测到已运行的普通 Codex。请先手动关闭后再启动增强模式（不会强制结束进程）。"
                        .into(),
                ),
            });
        }
        return Ok(LaunchEnhancedCodexResult {
            state: CodexRuntimeState::Running,
            pid: Some(proc.pid),
            cdp_port: proc.cdp_port,
            bridge_port: None,
            instance_id: None,
            message: Some("已附加到带 CDP 的 Codex 进程".into()),
        });
    }

    #[cfg(windows)]
    let exe = discovery::discover_codex_executable().unwrap_or_else(|_| PathBuf::from("Codex.exe"));
    #[cfg(not(windows))]
    let exe = PathBuf::from("codex");

    let pid = hooks.spawn_with_cdp(&exe, DEFAULT_CDP_PORT)?;
    Ok(LaunchEnhancedCodexResult {
        state: CodexRuntimeState::Launching,
        pid: Some(pid),
        cdp_port: Some(DEFAULT_CDP_PORT),
        bridge_port: None,
        instance_id: None,
        message: Some("已请求启动增强 Codex".into()),
    })
}

/// Real launch path used by commands.
pub async fn launch_enhanced_codex(
    handle: &CodexRuntimeHandle,
) -> Result<LaunchEnhancedCodexResult, AppError> {
    #[cfg(not(windows))]
    {
        let snap = CodexRuntimeSnapshot {
            state: CodexRuntimeState::Unsupported,
            message: Some("增强启动仅支持 Windows".into()),
            ..Default::default()
        };
        handle.set_snapshot(snap.clone()).await;
        return Ok(LaunchEnhancedCodexResult {
            state: snap.state,
            pid: None,
            cdp_port: None,
            bridge_port: None,
            instance_id: None,
            message: snap.message,
        });
    }

    #[cfg(windows)]
    {
        let running = discovery::find_running_codex();
        if let Some(proc) = running.first() {
            // Prefer CDP port parsed from process cmdline (Store Codex uses 9229);
            // fall back to scanning DEFAULT_CDP_PORT..+20.
            if let Some(port) =
                discovery::resolve_cdp_port(&running, DEFAULT_CDP_PORT, 20).await
            {
                let pid = running
                    .iter()
                    .find(|p| p.has_cdp)
                    .map(|p| p.pid)
                    .or(Some(proc.pid));
                return attach_and_inject(handle, pid, port).await;
            }
            let result = LaunchEnhancedCodexResult {
                state: CodexRuntimeState::OrdinaryRunning,
                pid: Some(proc.pid),
                cdp_port: None,
                bridge_port: None,
                instance_id: None,
                message: Some(
                    "检测到已运行的普通 Codex。请先手动关闭后再启动增强模式（不会强制结束进程）。"
                        .into(),
                ),
            };
            handle
                .set_snapshot(CodexRuntimeSnapshot {
                    state: result.state.clone(),
                    pid: result.pid,
                    cdp_port: None,
                    bridge_port: None,
                    instance_id: None,
                    message: result.message.clone(),
                })
                .await;
            return Ok(result);
        }

        handle
            .set_snapshot(CodexRuntimeSnapshot {
                state: CodexRuntimeState::Launching,
                message: Some("正在启动 Codex…".into()),
                ..Default::default()
            })
            .await;

        let exe = discovery::discover_codex_executable()?;
        let cdp_port = DEFAULT_CDP_PORT;
        let child = spawn_codex_with_cdp(&exe, cdp_port)?;
        let pid = child.id();
        {
            let mut guard = handle.inner.lock().await;
            guard.child = Some(child);
            guard.snapshot = CodexRuntimeSnapshot {
                state: CodexRuntimeState::Launching,
                pid: Some(pid),
                cdp_port: Some(cdp_port),
                bridge_port: None,
                instance_id: None,
                message: Some("等待 CDP 就绪…".into()),
            };
        }

        let mut ready = false;
        for _ in 0..40 {
            if discovery::probe_cdp_port(cdp_port).await {
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        if !ready {
            let result = LaunchEnhancedCodexResult {
                state: CodexRuntimeState::Degraded,
                pid: Some(pid),
                cdp_port: Some(cdp_port),
                bridge_port: None,
                instance_id: None,
                message: Some("Codex 已启动但 CDP 未就绪".into()),
            };
            handle
                .set_snapshot(CodexRuntimeSnapshot {
                    state: result.state.clone(),
                    pid: result.pid,
                    cdp_port: result.cdp_port,
                    bridge_port: None,
                    instance_id: None,
                    message: result.message.clone(),
                })
                .await;
            return Ok(result);
        }

        attach_and_inject(handle, Some(pid), cdp_port).await
    }
}

async fn attach_and_inject(
    handle: &CodexRuntimeHandle,
    pid: Option<u32>,
    cdp_port: u16,
) -> Result<LaunchEnhancedCodexResult, AppError> {
    handle
        .set_snapshot(CodexRuntimeSnapshot {
            state: CodexRuntimeState::Injecting,
            pid,
            cdp_port: Some(cdp_port),
            bridge_port: None,
            instance_id: None,
            message: Some("正在注入增强脚本…".into()),
        })
        .await;

    let settings = get_settings().codex_workbench;
    let instance_id = uuid::Uuid::new_v4().to_string();

    // Reinject/re-attach: await previous bridge shutdown so accept-loop exits
    // before we bind a new listener (no dual-listener window).
    let prev = {
        let mut guard = handle.inner.lock().await;
        guard.bridge.take()
    };
    if let Some(prev) = prev {
        prev.shutdown().await;
    }

    let bridge = codex_injection::start_bridge(&instance_id).await?;
    let bridge_port = bridge.port;
    let nonce = bridge.nonce.clone();

    let bundle =
        codex_injection::build_bootstrap_bundle(&settings, &instance_id, bridge_port, &nonce);

    if let Err(e) = cdp::inject_script(cdp_port, &bundle).await {
        {
            let mut guard = handle.inner.lock().await;
            guard.bridge = Some(bridge);
            guard.snapshot = CodexRuntimeSnapshot {
                state: CodexRuntimeState::Degraded,
                pid,
                cdp_port: Some(cdp_port),
                bridge_port: Some(bridge_port),
                instance_id: Some(instance_id.clone()),
                message: Some(format!("注入失败: {e}")),
            };
        }
        return Ok(LaunchEnhancedCodexResult {
            state: CodexRuntimeState::Degraded,
            pid,
            cdp_port: Some(cdp_port),
            bridge_port: Some(bridge_port),
            instance_id: Some(instance_id),
            message: Some(format!("注入失败: {e}")),
        });
    }

    let result = LaunchEnhancedCodexResult {
        state: CodexRuntimeState::Running,
        pid,
        cdp_port: Some(cdp_port),
        bridge_port: Some(bridge_port),
        instance_id: Some(instance_id.clone()),
        message: Some("增强 Codex 运行中".into()),
    };
    {
        let mut guard = handle.inner.lock().await;
        guard.bridge = Some(bridge);
        guard.last_bundle = Some(bundle);
        guard.snapshot = CodexRuntimeSnapshot {
            state: result.state.clone(),
            pid: result.pid,
            cdp_port: result.cdp_port,
            bridge_port: result.bridge_port,
            instance_id: Some(instance_id),
            message: result.message.clone(),
        };
    }
    // Keep enhancements alive across page reloads / soft navigations.
    start_nav_watcher(handle, cdp_port);
    Ok(result)
}

pub async fn reinject_enhancements(
    handle: &CodexRuntimeHandle,
) -> Result<LaunchEnhancedCodexResult, AppError> {
    let snap = handle.snapshot().await;
    let cdp_port = snap
        .cdp_port
        .ok_or_else(|| AppError::Config("无 CDP 端口，无法重新注入；请先启动增强 Codex".into()))?;
    attach_and_inject(handle, snap.pid, cdp_port).await
}



/// Pure decision for one nav-watcher poll tick.
/// Returns whether to attempt script-only reinject, plus next counters.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct NavWatcherTick {
    reinject: bool,
    consecutive_missing: u32,
    cooldown: bool,
}

fn nav_watcher_tick(
    consecutive_missing: u32,
    cooldown: bool,
    probe: Result<bool, ()>,
) -> NavWatcherTick {
    match probe {
        Ok(true) => NavWatcherTick {
            reinject: false,
            consecutive_missing: 0,
            cooldown: false,
        },
        Ok(false) => {
            let consecutive_missing = consecutive_missing.saturating_add(1);
            if consecutive_missing >= 2 && !cooldown {
                NavWatcherTick {
                    reinject: true,
                    consecutive_missing: 0,
                    cooldown: true,
                }
            } else {
                NavWatcherTick {
                    reinject: false,
                    consecutive_missing,
                    cooldown,
                }
            }
        }
        Err(()) => NavWatcherTick {
            reinject: false,
            consecutive_missing: 0,
            cooldown,
        },
    }
}

/// Poll CSP marker every 2s. After navigation wipes the page (marker false for
/// two consecutive polls), reinject `last_bundle` without rebuilding the bridge.
fn start_nav_watcher(handle: &CodexRuntimeHandle, cdp_port: u16) {
    let handle = handle.clone();
    let (tx, mut rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        // Replace previous watcher (if any) with this one.
        {
            let mut guard = handle.inner.lock().await;
            if let Some(prev) = guard.watcher_stop.take() {
                let _ = prev.send(());
            }
            guard.watcher_stop = Some(tx);
        }

        let mut consecutive_missing = 0u32;
        let mut cooldown = false;
        loop {
            tokio::select! {
                _ = &mut rx => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
            }

            let (state, bundle) = {
                let guard = handle.inner.lock().await;
                (guard.snapshot.state.clone(), guard.last_bundle.clone())
            };
            if state != CodexRuntimeState::Running {
                continue;
            }
            let Some(bundle) = bundle else { continue };

            let probe = cdp::probe_csp_marker(cdp_port)
                .await
                .map_err(|_| ());
            // Decision without side effects first; only inject when tick.reinject.
            // On inject failure keep previous counters so the next poll can retry.
            let tentative = nav_watcher_tick(consecutive_missing, cooldown, probe);
            if tentative.reinject {
                log::info!("codex nav watcher: marker missing — reinjecting bundle");
                match cdp::inject_script(cdp_port, &bundle).await {
                    Ok(()) => {
                        consecutive_missing = tentative.consecutive_missing;
                        cooldown = tentative.cooldown;
                    }
                    Err(e) => {
                        log::warn!("codex nav watcher reinject failed: {e}");
                        // Keep missing count so we keep trying; do not enter cooldown.
                        consecutive_missing = consecutive_missing.saturating_add(1);
                    }
                }
            } else {
                consecutive_missing = tentative.consecutive_missing;
                cooldown = tentative.cooldown;
            }
        }
    });
}

#[cfg(windows)]
fn spawn_codex_with_cdp(exe: &Path, cdp_port: u16) -> Result<std::process::Child, AppError> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let remote = format!("--remote-debugging-port={cdp_port}");
    Command::new(exe)
        .arg(&remote)
        .arg("--remote-allow-origins=*")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| AppError::Config(format!("启动 Codex 失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_running_is_never_killed_or_relaunched() {
        let hooks = FakeHooks::ordinary_codex_without_cdp();
        let result = launch_with_hooks(&hooks).unwrap();
        assert_eq!(result.state, CodexRuntimeState::OrdinaryRunning);
        assert_eq!(hooks.kill_calls(), 0);
        assert_eq!(hooks.spawn_calls(), 0);
    }

    #[test]
    fn nav_tick_marker_present_clears_missing_and_cooldown() {
        let t = nav_watcher_tick(3, true, Ok(true));
        assert_eq!(
            t,
            NavWatcherTick {
                reinject: false,
                consecutive_missing: 0,
                cooldown: false,
            }
        );
    }

    #[test]
    fn nav_tick_single_missing_does_not_reinject() {
        let t = nav_watcher_tick(0, false, Ok(false));
        assert_eq!(
            t,
            NavWatcherTick {
                reinject: false,
                consecutive_missing: 1,
                cooldown: false,
            }
        );
    }

    #[test]
    fn nav_tick_two_missing_triggers_reinject_and_cooldown() {
        let t = nav_watcher_tick(1, false, Ok(false));
        assert_eq!(
            t,
            NavWatcherTick {
                reinject: true,
                consecutive_missing: 0,
                cooldown: true,
            }
        );
    }

    #[test]
    fn nav_tick_cooldown_suppresses_repeat_reinject() {
        let t = nav_watcher_tick(5, true, Ok(false));
        assert_eq!(
            t,
            NavWatcherTick {
                reinject: false,
                consecutive_missing: 6,
                cooldown: true,
            }
        );
    }

    #[test]
    fn nav_tick_probe_error_resets_missing_keeps_cooldown() {
        let t = nav_watcher_tick(2, true, Err(()));
        assert_eq!(
            t,
            NavWatcherTick {
                reinject: false,
                consecutive_missing: 0,
                cooldown: true,
            }
        );
    }

    /// Live path closest to attach_and_inject without Tauri UI / settings DB:
    /// start_bridge → build_bootstrap_bundle → inject_script → probe_csp_marker → bridge /health.
    ///
    /// Run:
    /// `cargo test --manifest-path src-tauri/Cargo.toml --lib services::codex_runtime::launcher::tests::live_bridge_bootstrap_inject_store_codex -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires live Store Codex with --remote-debugging-port=9229"]
    async fn live_bridge_bootstrap_inject_store_codex() {
        let port: u16 = std::env::var("CC_SWITCH_LIVE_CDP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(9229);

        assert!(
            discovery::probe_cdp_port(port).await,
            "CDP not reachable on {port}; start Store Codex with remote debugging"
        );

        let instance_id = uuid::Uuid::new_v4().to_string();
        let bridge = codex_injection::start_bridge(&instance_id)
            .await
            .expect("start_bridge");
        let bridge_port = bridge.port;
        let nonce = bridge.nonce.clone();
        let settings = crate::settings::CodexWorkbenchSettings::default();
        let bundle =
            codex_injection::build_bootstrap_bundle(&settings, &instance_id, bridge_port, &nonce);

        cdp::inject_script(port, &bundle)
            .await
            .unwrap_or_else(|e| panic!("inject bootstrap failed on port {port}: {e}"));

        let marked = cdp::probe_csp_marker(port)
            .await
            .unwrap_or_else(|e| panic!("probe_csp_marker failed: {e}"));
        assert!(marked, "CSP marker should be true after bootstrap inject");

        // Bridge health with bearer nonce (same contract page runtime uses).
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("reqwest client");
        let health_url = format!("http://127.0.0.1:{bridge_port}/health");
        let resp = client
            .get(&health_url)
            .header("Authorization", format!("Bearer {nonce}"))
            .send()
            .await
            .expect("bridge health request");
        assert!(
            resp.status().is_success(),
            "bridge /health expected 200, got {}",
            resp.status()
        );

        bridge.shutdown().await;
    }

    /// Live smoke: production `launch_enhanced_codex` against Store Codex already
    /// listening on CDP (discovery → attach_and_inject → nav_watcher).
    ///
    /// Run:
    /// `cargo test --manifest-path src-tauri/Cargo.toml --lib services::codex_runtime::launcher::tests::live_launch_enhanced_codex_attach_store -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires live Store Codex with CDP (discovery finds has_cdp main)"]
    async fn live_launch_enhanced_codex_attach_store() {
        let running = discovery::find_running_codex();
        let with_cdp: Vec<_> = running.iter().filter(|p| p.has_cdp).collect();
        assert!(
            !with_cdp.is_empty(),
            "no has_cdp Codex process; start Store Codex with --remote-debugging-port"
        );

        let handle = CodexRuntimeHandle::new();
        let result = launch_enhanced_codex(&handle)
            .await
            .unwrap_or_else(|e| panic!("launch_enhanced_codex failed: {e}"));

        eprintln!(
            "launch result: state={:?} pid={:?} cdp={:?} bridge={:?} msg={:?}",
            result.state, result.pid, result.cdp_port, result.bridge_port, result.message
        );

        assert_eq!(
            result.state,
            CodexRuntimeState::Running,
            "expected Running, got {:?} msg={:?}",
            result.state,
            result.message
        );
        let cdp_port = result.cdp_port.expect("cdp_port");
        assert!(result.bridge_port.is_some(), "bridge_port should be set");
        assert!(result.instance_id.is_some(), "instance_id should be set");

        let marked = cdp::probe_csp_marker(cdp_port)
            .await
            .unwrap_or_else(|e| panic!("probe_csp_marker failed: {e}"));
        assert!(marked, "CSP marker should be true after launch_enhanced_codex");

        // Cleanup bridge + nav watcher so the test process exits cleanly.
        let (bridge, stop) = {
            let mut guard = handle.inner.lock().await;
            (guard.bridge.take(), guard.watcher_stop.take())
        };
        if let Some(tx) = stop {
            let _ = tx.send(());
        }
        if let Some(bridge) = bridge {
            bridge.shutdown().await;
        }
    }
}
