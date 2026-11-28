#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod unix;
#[cfg(target_os = "linux")]
mod unix;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppStatus {
    pub supported: bool,
    pub installed: bool,
    pub running: bool,
}

impl DesktopAppStatus {
    fn unsupported() -> Self {
        Self {
            supported: false,
            installed: false,
            running: false,
        }
    }
}

fn is_supported_app(app: &str) -> bool {
    matches!(app, "codex" | "claude-desktop" | "opencode")
}

fn app_label(app: &str) -> &'static str {
    match app {
        "codex" => "Codex",
        "claude-desktop" => "Claude Desktop",
        "opencode" => "OpenCode",
        _ => "Desktop App",
    }
}

pub fn status(app: &str) -> Result<DesktopAppStatus, String> {
    if !is_supported_app(app) {
        return Ok(DesktopAppStatus::unsupported());
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        let home =
            dirs::home_dir().ok_or_else(|| "Unable to locate the home directory".to_string())?;
        #[cfg(target_os = "windows")]
        let target = windows::find_target(app, &home)?;
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let target = unix::find_target(app, &home)?;
        let running = target.as_ref().is_some_and(|target| {
            #[cfg(target_os = "windows")]
            {
                windows::is_running(target, app)
            }
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                unix::is_running(target)
            }
        });
        return Ok(DesktopAppStatus {
            supported: true,
            installed: target.is_some(),
            running,
        });
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    Ok(DesktopAppStatus::unsupported())
}

pub fn launch(app: &str) -> Result<(), String> {
    let target = resolve_target(app)?;
    launch_resolved(&target, app_label(app))
}

pub fn restart(app: &str) -> Result<(), String> {
    let target = resolve_target(app)?;
    let label = app_label(app);
    stop_resolved(&target, app, label)?;
    launch_resolved(&target, label)
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
type DesktopAppTarget = PlatformDesktopAppTarget;
#[cfg(target_os = "windows")]
type PlatformDesktopAppTarget = windows::WindowsDesktopAppTarget;
#[cfg(any(target_os = "macos", target_os = "linux"))]
type PlatformDesktopAppTarget = PathBuf;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
type DesktopAppTarget = PathBuf;

fn resolve_target(app: &str) -> Result<DesktopAppTarget, String> {
    if !is_supported_app(app) {
        return Err(format!(
            "{} desktop app controls are not supported",
            app_label(app)
        ));
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        let home =
            dirs::home_dir().ok_or_else(|| "Unable to locate the home directory".to_string())?;
        #[cfg(target_os = "windows")]
        let target = windows::find_target(app, &home)?;
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let target = unix::find_target(app, &home)?;
        target.ok_or_else(|| format!("{} desktop app is not installed", app_label(app)))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err(format!(
            "{} desktop app controls are not supported on this platform",
            app_label(app)
        ))
    }
}

#[cfg(target_os = "windows")]
fn stop_resolved(target: &DesktopAppTarget, app: &str, label: &str) -> Result<(), String> {
    windows::stop(target, app, label)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn stop_resolved(target: &DesktopAppTarget, _app: &str, label: &str) -> Result<(), String> {
    unix::stop(target, label)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn stop_resolved(_target: &DesktopAppTarget, _app: &str, _label: &str) -> Result<(), String> {
    Err("Desktop app controls are not supported on this platform".to_string())
}

#[cfg(target_os = "windows")]
fn launch_resolved(target: &DesktopAppTarget, label: &str) -> Result<(), String> {
    windows::launch(target, label)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn launch_resolved(target: &DesktopAppTarget, label: &str) -> Result<(), String> {
    unix::launch(target, label)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn launch_resolved(_target: &DesktopAppTarget, _label: &str) -> Result<(), String> {
    Err("Desktop app controls are not supported on this platform".to_string())
}
