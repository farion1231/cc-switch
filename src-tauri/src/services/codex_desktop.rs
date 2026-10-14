//! Explicit desktop restart. No provider switch restarts clients implicitly.

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DesktopRestartOutcome {
    pub action: String,
    pub desktop_pid: u32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DesktopStatus {
    pub supported: bool,
    pub installed: bool,
    pub running: bool,
}

#[cfg(target_os = "windows")]
pub fn status() -> Result<DesktopStatus, String> {
    let output = run_powershell_script(include_bytes!("codex_desktop_windows.ps1"), "Status")?;
    serde_json::from_slice(&output.stdout).map_err(|e| format!("Invalid Codex desktop status: {e}"))
}

#[cfg(not(target_os = "windows"))]
pub fn status() -> Result<DesktopStatus, String> {
    Ok(DesktopStatus {
        supported: false,
        installed: false,
        running: false,
    })
}

#[cfg(target_os = "windows")]
pub fn restart() -> Result<DesktopRestartOutcome, String> {
    // Serialize requests so two button clicks cannot close the newly opened app.
    static RESTART_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = RESTART_LOCK
        .try_lock()
        .map_err(|_| "Codex desktop is already restarting".to_string())?;
    let output = run_powershell_script(include_bytes!("codex_desktop_windows.ps1"), "Restart")?;
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Invalid Codex desktop restart receipt: {e}"))
}

#[cfg(target_os = "windows")]
fn run_powershell_script(source: &[u8], mode: &str) -> Result<std::process::Output, String> {
    use std::{
        io::Write,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let mut script = tempfile::Builder::new()
        .suffix(".ps1")
        .tempfile()
        .map_err(|e| e.to_string())?;
    // Windows PowerShell 5.1 needs a BOM to decode UTF-8 scripts consistently.
    script
        .write_all(b"\xef\xbb\xbf")
        .map_err(|e| e.to_string())?;
    script.write_all(source).map_err(|e| e.to_string())?;
    script.flush().map_err(|e| e.to_string())?;
    // PowerShell cannot read a Windows delete-on-close file while its handle is
    // open. Keep the path's cleanup guard, but release the handle before launch.
    let script = script.into_temp_path();
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .args(["-Mode", mode])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| format!("Cannot start Codex desktop restart: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill(); // Only the helper started above, never a Codex client.
            let _ = child.wait();
            return Err("Codex desktop restart timed out. Check whether the desktop app has exited before retrying.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output)
}

#[cfg(not(target_os = "windows"))]
pub fn restart() -> Result<DesktopRestartOutcome, String> {
    Err("Desktop restart currently supports the Windows OpenAI.Codex package only".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unsupported_platform_does_not_offer_desktop_controls() {
        let state = status().unwrap();
        assert!(!state.supported);
        assert!(!state.installed);
        assert!(!state.running);
    }
    #[test]
    fn desktop_restart_receipt_requires_a_pid_and_action() {
        let receipt: DesktopRestartOutcome =
            serde_json::from_str(r#"{"action":"restarted","desktop_pid":42}"#).unwrap();
        assert_eq!(receipt.desktop_pid, 42);
        assert_eq!(receipt.action, "restarted");
        assert!(
            serde_json::from_str::<DesktopRestartOutcome>(r#"{"action":"restarted"}"#).is_err()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn materialized_script_runs_in_powershell_and_cleans_up() {
        // Exercise the same temp-file handoff as production, with no real app
        // close or activation. Directly running a checked-in script misses the
        // Windows delete-on-close sharing regression.
        let source = r#"param([string]$Mode)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
if ($Mode -ne 'Plan') { throw 'Unexpected execution mode' }
@{ action = 'restarted'; desktop_pid = 42; script_path = $PSCommandPath; label = '桌面' } | ConvertTo-Json -Compress
"#;
        let output = run_powershell_script(source.as_bytes(), "Plan").unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["action"], "restarted");
        assert_eq!(receipt["desktop_pid"], 42);
        assert_eq!(receipt["label"], "桌面");
        let path = receipt["script_path"].as_str().unwrap();
        assert!(!std::path::Path::new(path).exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn materialized_desktop_failure_reports_only_the_message() {
        // Run the production error handler without querying or changing a real
        // installation. A package lookup failure should not expose a PS stack.
        let (_, body) = include_str!("codex_desktop_windows.ps1")
            .split_once("$ErrorActionPreference")
            .unwrap();
        let source = format!(
            "param([string]$Mode)\nfunction Get-AppxPackage {{ param($Name) }}\n$ErrorActionPreference{body}"
        );
        let error = run_powershell_script(source.as_bytes(), "Plan").unwrap_err();
        assert_eq!(
            error,
            "Codex desktop (OpenAI.Codex) is not installed for this Windows user."
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn desktop_restart_lifecycle_fixtures() {
        use std::os::windows::process::CommandExt;
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../scripts/tests/codex-desktop-restart.Tests.ps1");
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
