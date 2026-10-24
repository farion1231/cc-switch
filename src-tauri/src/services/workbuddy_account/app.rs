//! 「应用」页的 WorkBuddy 行：装没装、装的哪个版本、在不在运行，以及请它正常退出。
//!
//! WorkBuddy 是桌面应用（Electron），升级由它自己负责，这里不碰更新。关闭只做「请求退出」
//! （macOS 上经 Apple Events 让它自己 quit、保存状态），不 kill、不强制退出；它不肯退或
//! 超时仍在运行时交给用户手动退出。Windows / Linux 暂不支持关闭。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyAppStatus {
    pub installed: bool,
    /// 已安装的版本（macOS 读 Info.plist；其他平台读不到时为空）
    pub version: Option<String>,
    /// 安装位置（`.app` 目录或可执行文件）
    pub path: Option<String>,
    pub running: bool,
    /// 这个平台能不能在这里请 WorkBuddy 退出（目前只有 macOS）
    pub quit_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyQuitOutcome {
    /// 等完之后 WorkBuddy 还在运行（它拒绝了退出、弹了确认框，或还没退完）
    pub still_running: bool,
}

/// 探测本机的 WorkBuddy。会起子进程（`lsappinfo` / `ps` / `mdfind`），放阻塞线程里调。
pub fn app_status() -> WorkBuddyAppStatus {
    let running = super::workbuddy_running();
    let found = locate_install();
    let version = found.as_deref().and_then(installed_version);
    WorkBuddyAppStatus {
        // 在跑就一定装了，哪怕不在我们认识的位置
        installed: found.is_some() || running,
        version,
        path: found.map(|path| path.to_string_lossy().into_owned()),
        running,
        quit_supported: cfg!(target_os = "macos"),
    }
}

#[cfg(target_os = "macos")]
fn locate_install() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("/Applications/WorkBuddy.app")];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Applications").join("WorkBuddy.app"));
    }
    if let Some(found) = candidates
        .into_iter()
        .find(|path| path.join("Contents").join("Info.plist").is_file())
    {
        return Some(found);
    }
    // 装在别处时按 bundle id 让 Spotlight 找
    let output = std::process::Command::new("mdfind")
        .arg(format!(
            "kMDItemCFBundleIdentifier == '{}'",
            super::MAC_BUNDLE_ID
        ))
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| PathBuf::from(line.trim()))
        .find(|path| {
            path.extension().is_some_and(|ext| ext == "app")
                && path.join("Contents").join("Info.plist").is_file()
        })
}

/// 推断的安装位置（未实测）：`%LOCALAPPDATA%\Programs\WorkBuddy\WorkBuddy.exe`，
/// 或装给所有用户时的 `%ProgramFiles%\WorkBuddy\WorkBuddy.exe`。
#[cfg(windows)]
fn locate_install() -> Option<PathBuf> {
    ["LOCALAPPDATA", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .flat_map(|base| {
            [
                base.join("Programs")
                    .join("WorkBuddy")
                    .join("WorkBuddy.exe"),
                base.join("WorkBuddy").join("WorkBuddy.exe"),
            ]
        })
        .find(|path| path.is_file())
}

/// 推断的安装位置（未实测）：PATH 里的 `workbuddy`，或 deb / rpm 常见的 `/opt/WorkBuddy`。
#[cfg(not(any(target_os = "macos", windows)))]
fn locate_install() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join("workbuddy"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    on_path
        .into_iter()
        .chain([
            PathBuf::from("/opt/WorkBuddy/workbuddy"),
            PathBuf::from("/opt/workbuddy/workbuddy"),
        ])
        .find(|path| path.is_file())
}

/// 已安装的版本。macOS 读 `.app/Contents/Info.plist` 的 `CFBundleShortVersionString`；
/// 其他平台暂时读不到（只显示已安装）。
fn installed_version(path: &Path) -> Option<String> {
    if path.extension().is_some_and(|ext| ext == "app") {
        return read_bundle_version(&path.join("Contents").join("Info.plist"));
    }
    None
}

/// 读 Info.plist 里的版本号：先按 XML 解析，是二进制 plist 时（macOS 上）让 `plutil` 转一下。
pub(super) fn read_bundle_version(plist: &Path) -> Option<String> {
    let bytes = std::fs::read(plist).ok()?;
    if let Some(version) = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|xml| plist_string_value(xml, "CFBundleShortVersionString"))
    {
        return Some(version);
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("plutil")
            .args(["-convert", "xml1", "-o", "-"])
            .arg(plist)
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        if output.status.success() {
            return plist_string_value(
                &String::from_utf8_lossy(&output.stdout),
                "CFBundleShortVersionString",
            );
        }
    }
    None
}

/// XML plist 里 `<key>{key}</key>` 紧跟着的 `<string>` 值。
pub(super) fn plist_string_value(xml: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let rest = &xml[xml.find(&marker)? + marker.len()..];
    let value = rest.trim_start().strip_prefix("<string>")?;
    let value = value[..value.find("</string>")?].trim();
    (!value.is_empty()).then(|| unescape_xml(value))
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 请求退出的 AppleScript：按 bundle id 发 quit，WorkBuddy 自己走正常退出流程。
/// AppleScript 自己最多等 `timeout_secs` 秒回应。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn quit_script(bundle_id: &str, timeout_secs: u64) -> String {
    format!(
        "with timeout of {timeout_secs} seconds\n\
         tell application id \"{bundle_id}\" to quit\n\
         end timeout"
    )
}

/// 每隔 `interval` 看一次，最多 `attempts` 次，等 `running()` 变成 false。返回最后是否仍在运行。
pub(super) fn wait_until_stopped(
    running: impl Fn() -> bool,
    attempts: usize,
    interval: Duration,
) -> bool {
    for attempt in 0..attempts {
        if !running() {
            return false;
        }
        if attempt + 1 < attempts {
            std::thread::sleep(interval);
        }
    }
    running()
}

/// 请 WorkBuddy 正常退出并等它退完。没在运行就什么也不做（按 bundle id `tell` 一个没运行
/// 的应用会把它启动起来）。阻塞，放阻塞线程里调。
pub fn quit_app() -> Result<WorkBuddyQuitOutcome, AppError> {
    if !super::workbuddy_running() {
        return Ok(WorkBuddyQuitOutcome {
            still_running: false,
        });
    }
    request_quit()?;
    let still_running =
        wait_until_stopped(super::workbuddy_running, 20, Duration::from_millis(500));
    Ok(WorkBuddyQuitOutcome { still_running })
}

#[cfg(target_os = "macos")]
fn request_quit() -> Result<(), AppError> {
    use std::process::{Command, Stdio};
    use std::time::Instant;

    const APPLESCRIPT_TIMEOUT_SECS: u64 = 10;
    let mut child = Command::new("osascript")
        .arg("-e")
        .arg(quit_script(super::MAC_BUNDLE_ID, APPLESCRIPT_TIMEOUT_SECS))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| quit_failed(&error.to_string()))?;
    // osascript 自己有超时；再兜一层，卡住了只结束 osascript 这个子进程，不碰 WorkBuddy
    let deadline = Instant::now() + Duration::from_secs(APPLESCRIPT_TIMEOUT_SECS + 5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    use std::io::Read;
                    let _ = pipe.read_to_string(&mut stderr);
                }
                log::warn!("[WorkBuddy] 请求退出失败: {}", stderr.trim());
                return Err(quit_failed(stderr.trim()));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                // 没等到回应：交给后面的运行检测，仍在运行就提示手动退出
                return Ok(());
            }
            Err(error) => return Err(quit_failed(&error.to_string())),
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn request_quit() -> Result<(), AppError> {
    Err(AppError::localized(
        "workbuddy.app.quit_unsupported",
        "这个系统上不支持在这里关闭 WorkBuddy，请在 WorkBuddy 里手动退出。",
        "Closing WorkBuddy from here isn't supported on this system. Quit it from WorkBuddy.",
    ))
}

#[cfg(target_os = "macos")]
fn quit_failed(detail: &str) -> AppError {
    // -1743：用户没允许 CC Switch 控制 WorkBuddy（系统设置 › 隐私与安全性 › 自动化）
    let (zh, en) = if detail.contains("-1743") {
        (
            "没有控制 WorkBuddy 的权限。请在「系统设置 › 隐私与安全性 › 自动化」里允许 CC Switch 控制 WorkBuddy，或在 WorkBuddy 里手动退出。",
            "CC Switch isn't allowed to control WorkBuddy. Allow it in System Settings › Privacy & Security › Automation, or quit WorkBuddy manually.",
        )
    } else {
        (
            "WorkBuddy 没有响应退出请求，请在 WorkBuddy 里手动退出。",
            "WorkBuddy didn't respond to the quit request. Quit it from WorkBuddy.",
        )
    };
    AppError::localized("workbuddy.app.quit_failed", zh, en)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>com.tencent.workbuddy.mac</string>
	<key>CFBundleShortVersionString</key>
	<string>5.8.3</string>
	<key>CFBundleVersion</key>
	<string>5.8.3</string>
</dict>
</plist>"#;

    #[test]
    fn reads_short_version_from_xml_plist() {
        assert_eq!(
            plist_string_value(PLIST, "CFBundleShortVersionString").as_deref(),
            Some("5.8.3")
        );
        assert_eq!(
            plist_string_value(PLIST, "CFBundleIdentifier").as_deref(),
            Some("com.tencent.workbuddy.mac")
        );
        assert_eq!(plist_string_value(PLIST, "CFBundleName"), None);
        // 键后面不是 <string> 就不认
        assert_eq!(plist_string_value("<key>A</key><true/>", "A"), None);
        assert_eq!(
            plist_string_value("<key>A</key><string>a &amp; b</string>", "A").as_deref(),
            Some("a & b")
        );
    }

    #[test]
    fn reads_bundle_version_from_info_plist_file() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("WorkBuddy.app");
        let contents = app.join("Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(contents.join("Info.plist"), PLIST).unwrap();
        assert_eq!(installed_version(&app).as_deref(), Some("5.8.3"));
        assert_eq!(
            read_bundle_version(&contents.join("Info.plist")).as_deref(),
            Some("5.8.3")
        );
        // 没有 Info.plist、或不是 .app
        assert_eq!(installed_version(&dir.path().join("Missing.app")), None);
        assert_eq!(installed_version(&dir.path().join("WorkBuddy.exe")), None);
    }

    #[test]
    fn quit_script_asks_by_bundle_id_with_a_timeout() {
        let script = quit_script("com.tencent.workbuddy.mac", 10);
        assert!(script.contains("tell application id \"com.tencent.workbuddy.mac\" to quit"));
        assert!(script.starts_with("with timeout of 10 seconds"));
        assert!(script.trim_end().ends_with("end timeout"));
        // 只请求退出，不用任何强制手段
        assert!(!script.contains("kill") && !script.contains("force"));
    }

    #[test]
    fn waits_until_workbuddy_stops() {
        let checks = Cell::new(0);
        let still_running = wait_until_stopped(
            || {
                checks.set(checks.get() + 1);
                checks.get() < 3
            },
            10,
            Duration::ZERO,
        );
        assert!(!still_running);
        assert_eq!(checks.get(), 3);
    }

    #[test]
    fn reports_still_running_when_workbuddy_refuses() {
        let checks = Cell::new(0);
        let still_running = wait_until_stopped(
            || {
                checks.set(checks.get() + 1);
                true
            },
            4,
            Duration::ZERO,
        );
        assert!(still_running);
        assert_eq!(checks.get(), 5);
    }
}
