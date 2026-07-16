//! Discover Codex executable and running processes (Windows-first).

use crate::error::AppError;
use std::path::PathBuf;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CodexProcessInfo {
    pub pid: u32,
    pub has_cdp: bool,
    pub cdp_port: Option<u16>,
    pub exe_path: Option<PathBuf>,
}

/// Prefer common Windows install locations for Codex Desktop.
pub fn discover_codex_executable() -> Result<PathBuf, AppError> {
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let candidates = [
                PathBuf::from(&local)
                    .join("Programs")
                    .join("Codex")
                    .join("Codex.exe"),
                PathBuf::from(&local)
                    .join("Programs")
                    .join("chatgpt")
                    .join("Codex.exe"),
                PathBuf::from(&local)
                    .join("Programs")
                    .join("ChatGPT")
                    .join("ChatGPT.exe"),
            ];
            for c in candidates {
                if c.is_file() {
                    return Ok(c);
                }
            }
        }
        // Microsoft Store / WindowsApps package (OpenAI.Codex_*)
        if let Some(path) = discover_windowsapps_codex() {
            return Ok(path);
        }
        if let Ok(path) = which_codex_on_path() {
            return Ok(path);
        }
        // Last resort: running process path (if Codex is already open)
        if let Some(path) = find_running_codex()
            .into_iter()
            .find_map(|p| p.exe_path)
        {
            return Ok(path);
        }
        Err(AppError::Config(
            "Codex Desktop executable not found (checked LOCALAPPDATA Programs, WindowsApps, PATH)"
                .into(),
        ))
    }
    #[cfg(not(windows))]
    {
        Err(AppError::Config(
            "Codex Desktop discovery is Windows-first in this build".into(),
        ))
    }
}

#[cfg(windows)]
fn discover_windowsapps_codex() -> Option<PathBuf> {
    // Prefer Get-AppxPackage InstallLocation: listing WindowsApps is often ACL-denied.
    if let Some(p) = discover_windowsapps_via_appx_package() {
        return Some(p);
    }
    let roots = [
        PathBuf::from(r"C:\Program Files\WindowsApps"),
        std::env::var("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\Program Files"))
            .join("WindowsApps"),
    ];
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        // Prefer newest OpenAI.Codex_* package by name (version-ish sort)
        let mut packages: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("OpenAI.Codex_"))
                    .unwrap_or(false)
            })
            .collect();
        packages.sort();
        packages.reverse();
        for pkg in packages {
            for name in ["ChatGPT.exe", "Codex.exe"] {
                let candidate = pkg.join("app").join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Resolve Codex via Appx package metadata (no WindowsApps directory listing).
#[cfg(windows)]
fn discover_windowsapps_via_appx_package() -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Emit full path to ChatGPT.exe under InstallLocation\app
    let script = concat!(
        "$p = Get-AppxPackage -Name 'OpenAI.Codex' -ErrorAction SilentlyContinue | ",
        "Select-Object -First 1 -ExpandProperty InstallLocation; ",
        "if ($p) { Join-Path $p 'app\\ChatGPT.exe' }"
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let path = PathBuf::from(line);
    if path.is_file() {
        return Some(path);
    }
    let alt = path
        .parent()
        .map(|d| d.join("Codex.exe"))
        .filter(|c| c.is_file());
    alt
}

fn which_codex_on_path() -> Result<PathBuf, AppError> {
    for name in ["Codex.exe", "ChatGPT.exe", "codex"] {
        if let Ok(output) = std::process::Command::new("where").arg(name).output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                if let Some(line) = text.lines().next() {
                    let p = PathBuf::from(line.trim());
                    if p.is_file() {
                        return Ok(p);
                    }
                }
            }
        }
    }
    Err(AppError::Config("codex not on PATH".into()))
}

/// True when cmdline looks like the Chromium/Electron *browser* main process.
/// Child processes (`--type=renderer|gpu-process|utility|...`) inherit
/// `--remote-debugging-port` on their command line but do **not** own the CDP
/// HTTP listener. Live Store Codex: main ChatGPT.exe has the flag alone;
/// renderer also shows `--remote-debugging-port=9229` plus `--type=renderer`.
pub fn is_browser_main_process(cmdline: &str) -> bool {
    let lower = cmdline.to_ascii_lowercase();
    // Any `--type=` is a helper process (renderer, gpu, utility, crashpad, etc.).
    if lower.contains("--type=") {
        return false;
    }
    true
}

/// Parse `--remote-debugging-port=NNNN` or `--remote-debugging-port NNNN` from a cmdline.
/// Returns `None` for Electron/Chromium child processes even if they inherit the flag.
pub fn parse_cdp_port_from_cmdline(cmdline: &str) -> Option<u16> {
    if !is_browser_main_process(cmdline) {
        return None;
    }
    let lower = cmdline.to_ascii_lowercase();
    const KEY: &str = "--remote-debugging-port";
    let idx = lower.find(KEY)?;
    // Keep leading whitespace so space-separated form
    // (--remote-debugging-port 9333) is still recognized after KEY.
    let after = &cmdline[idx + KEY.len()..];
    let digits = if let Some(rest) = after.strip_prefix('=') {
        rest
    } else if after.starts_with(|c: char| c.is_ascii_whitespace()) {
        after.trim_start()
    } else {
        return None;
    };
    let num: String = digits
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num.parse::<u16>().ok().filter(|p| *p > 0)
}

/// List running Codex / ChatGPT processes. On Windows, also fills `has_cdp` /
/// `cdp_port` / `exe_path` from process command lines when available.
pub fn find_running_codex() -> Vec<CodexProcessInfo> {
    #[cfg(windows)]
    {
        find_running_codex_windows()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn find_running_codex_windows() -> Vec<CodexProcessInfo> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // Prefer PowerShell CIM: gives CommandLine + ExecutablePath so we can
    // detect live `--remote-debugging-port` (Store Codex opens CDP on 9229).
    if let Some(from_ps) = find_running_codex_via_powershell() {
        if !from_ps.is_empty() {
            return from_ps;
        }
    }

    // Fallback: tasklist CSV (pid only, no CDP metadata)
    let output = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !(lower.contains("codex") || lower.contains("chatgpt")) {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 2 {
            continue;
        }
        let pid_raw = parts[1].trim().trim_matches('"');
        if let Ok(pid) = pid_raw.parse::<u32>() {
            out.push(CodexProcessInfo {
                pid,
                has_cdp: false,
                cdp_port: None,
                exe_path: None,
            });
        }
    }
    out
}

#[cfg(windows)]
fn find_running_codex_via_powershell() -> Option<Vec<CodexProcessInfo>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Compact one-liner: PID|EXE|CMDLINE for ChatGPT/Codex processes
    let script = r#"Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'ChatGPT|Codex' } | ForEach-Object { '{0}|{1}|{2}' -f $_.ProcessId, ($_.ExecutablePath -replace '\|','/'), ($_.CommandLine -replace '\|','/') }"#;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '|');
        let Some(pid_s) = parts.next() else { continue };
        let exe = parts.next().unwrap_or("");
        let cmdline = parts.next().unwrap_or("");
        let Ok(pid) = pid_s.trim().parse::<u32>() else {
            continue;
        };
        // Prefer browser main process that owns CDP (has --remote-debugging-port).
        // Child helpers also match name ChatGPT.exe but lack the flag.
        let port = parse_cdp_port_from_cmdline(cmdline);
        let exe_path = if exe.is_empty() {
            None
        } else {
            Some(PathBuf::from(exe))
        };
        out.push(CodexProcessInfo {
            pid,
            has_cdp: port.is_some(),
            cdp_port: port,
            exe_path,
        });
    }
    // Put CDP-owning main process first so launcher.attach uses the right pid.
    out.sort_by_key(|p| if p.has_cdp { 0u8 } else { 1u8 });
    Some(out)
}

/// Probe whether a local CDP HTTP endpoint answers on port.
pub async fn probe_cdp_port(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/json/version");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(400))
        .build();
    let Ok(client) = client else {
        return false;
    };
    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Scan a small high port range for an open CDP endpoint.
pub async fn discover_open_cdp_port(start: u16, count: u16) -> Option<u16> {
    for port in start..start.saturating_add(count) {
        if probe_cdp_port(port).await {
            return Some(port);
        }
    }
    None
}

/// Prefer process-reported CDP port when present; else scan the default range.
pub async fn resolve_cdp_port(procs: &[CodexProcessInfo], scan_start: u16, scan_count: u16) -> Option<u16> {
    if let Some(port) = procs.iter().find_map(|p| p.cdp_port) {
        if probe_cdp_port(port).await {
            return Some(port);
        }
    }
    discover_open_cdp_port(scan_start, scan_count).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_running_codex_does_not_panic() {
        let _ = find_running_codex();
    }

    #[test]
    fn parse_cdp_port_equals_form() {
        assert_eq!(
            parse_cdp_port_from_cmdline(
                r#""C:\app\ChatGPT.exe" --remote-debugging-port=9229 --remote-allow-origins=http://127.0.0.1:9229"#
            ),
            Some(9229)
        );
    }

    #[test]
    fn parse_cdp_port_space_form() {
        assert_eq!(
            parse_cdp_port_from_cmdline("ChatGPT.exe --remote-debugging-port 9333"),
            Some(9333)
        );
    }

    #[test]
    fn parse_cdp_port_absent() {
        assert_eq!(
            parse_cdp_port_from_cmdline("ChatGPT.exe --type=renderer"),
            None
        );
    }

    #[test]
    fn parse_cdp_port_ignores_partial_flag() {
        // Must not treat --remote-debugging-port-file as the CDP flag.
        assert_eq!(
            parse_cdp_port_from_cmdline("ChatGPT.exe --remote-debugging-port-file=x"),
            None
        );
    }

    #[test]
    fn has_cdp_sort_puts_cdp_owner_first() {
        let mut procs = vec![
            CodexProcessInfo {
                pid: 1,
                has_cdp: false,
                cdp_port: None,
                exe_path: None,
            },
            CodexProcessInfo {
                pid: 2,
                has_cdp: true,
                cdp_port: Some(9229),
                exe_path: None,
            },
            CodexProcessInfo {
                pid: 3,
                has_cdp: false,
                cdp_port: None,
                exe_path: None,
            },
        ];
        procs.sort_by_key(|p| if p.has_cdp { 0u8 } else { 1u8 });
        assert_eq!(procs[0].pid, 2);
        assert_eq!(procs[0].cdp_port, Some(9229));
    }

    #[test]
    fn parse_cdp_port_ignores_renderer_that_inherits_flag() {
        // Live Store Codex renderer cmdline includes both --type=renderer and the port.
        let renderer = r#""C:\Program Files\WindowsApps\OpenAI.Codex\app\ChatGPT.exe" --type=renderer --remote-debugging-port=9229 --lang=zh-CN"#;
        assert_eq!(parse_cdp_port_from_cmdline(renderer), None);
        assert!(!is_browser_main_process(renderer));
    }

    #[test]
    fn parse_cdp_port_accepts_store_main_process() {
        let main = r#""C:\Program Files\WindowsApps\OpenAI.Codex\app\ChatGPT.exe" --remote-debugging-port=9229 --remote-allow-origins=http://127.0.0.1:9229 "#;
        assert_eq!(parse_cdp_port_from_cmdline(main), Some(9229));
        assert!(is_browser_main_process(main));
    }
}
