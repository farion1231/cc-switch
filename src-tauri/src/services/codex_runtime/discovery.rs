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
            ];
            for c in candidates {
                if c.is_file() {
                    return Ok(c);
                }
            }
        }
        if let Ok(path) = which_codex_on_path() {
            return Ok(path);
        }
        Err(AppError::Config(
            "未找到 Codex 可执行文件，请确认已安装 Codex Desktop".into(),
        ))
    }
    #[cfg(not(windows))]
    {
        Err(AppError::Config("增强启动目前仅支持 Windows".into()))
    }
}

fn which_codex_on_path() -> Result<PathBuf, AppError> {
    let path = std::env::var_os("PATH").ok_or_else(|| AppError::Config("PATH empty".into()))?;
    for dir in std::env::split_paths(&path) {
        for name in ["Codex.exe", "codex.exe", "Codex", "codex"] {
            let p = dir.join(name);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    Err(AppError::Config("codex not on PATH".into()))
}

/// Detect running Codex-like processes. Conservative: never assumes CDP unless port is known.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_running_codex_does_not_panic() {
        let _ = find_running_codex();
    }
}
