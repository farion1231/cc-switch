//! Launch / attach orchestration for enhanced Codex. NEVER kills ordinary Codex.

use super::cdp;
use super::discovery::{self, CodexProcessInfo};
use super::state::{CodexRuntimeSnapshot, CodexRuntimeState};
use crate::error::AppError;
use crate::services::codex_injection::{self, BridgeHandle};
use crate::settings::get_settings;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::Mutex;

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

/// In-process runtime handle shared via AppState.
pub struct CodexRuntimeHandle {
    inner: Mutex<RuntimeInner>,
}

#[derive(Default)]
struct RuntimeInner {
    snapshot: CodexRuntimeSnapshot,
    bridge: Option<BridgeHandle>,
    child: Option<std::process::Child>,
}

impl Default for CodexRuntimeHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexRuntimeHandle {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(RuntimeInner::default()),
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
    fn spawn_with_cdp(&self, exe: &PathBuf, cdp_port: u16) -> Result<u32, AppError>;
    fn kill_calls(&self) -> u32;
    fn spawn_calls(&self) -> u32;
}

#[derive(Default)]
pub struct FakeHooks {
    ordinary: bool,
    kill: AtomicU32,
    spawn: AtomicU32,
}

impl FakeHooks {
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

    fn spawn_with_cdp(&self, _exe: &PathBuf, _cdp_port: u16) -> Result<u32, AppError> {
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
            if let Some(port) = discovery::discover_open_cdp_port(DEFAULT_CDP_PORT, 20).await {
                return attach_and_inject(handle, Some(proc.pid), port).await;
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
        guard.snapshot = CodexRuntimeSnapshot {
            state: result.state.clone(),
            pid: result.pid,
            cdp_port: result.cdp_port,
            bridge_port: result.bridge_port,
            instance_id: Some(instance_id),
            message: result.message.clone(),
        };
    }
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

#[cfg(windows)]
fn spawn_codex_with_cdp(exe: &PathBuf, cdp_port: u16) -> Result<std::process::Child, AppError> {
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
}
