use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// One-shot marker passed when relaunching via the Windows desktop shell so
/// an OS/user-enforced RedirectionGuard policy (e.g. Exploit Protection)
/// cannot trigger a relaunch loop.
pub(crate) const RG_RELAUNCHED_ARG: &str = "--cc-switch-rg-relaunched";

const FALLBACK_STAMP_NAME: &str = "cc-switch-rg-relaunch.stamp";
const FALLBACK_GUARD_WINDOW: Duration = Duration::from_secs(15);

/// Escapes `msiexec`'s inherited RedirectionGuard (`ProcessRedirectionTrustPolicy`)
/// by delegating a fresh launch to the desktop `explorer.exe` shell and exiting
/// the restricted process before acquiring single-instance locks or starting Tauri.
pub(crate) fn maybe_relaunch_without_redirection_guard() {
    if std::env::var_os("CC_SWITCH_SKIP_RG_RELAUNCH").is_some() {
        return;
    }

    let raw_args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if has_rg_relaunch_arg(&raw_args) {
        return;
    }

    if !is_redirection_trust_enforced() {
        return;
    }

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let exe = crate::commands::windows_shell_compatible_path(&exe);
    let working_dir = std::env::current_dir()
        .ok()
        .map(|dir| crate::commands::windows_shell_compatible_path(&dir))
        .or_else(|| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let args_line = build_relaunch_args_line(&raw_args);

    if relaunch_via_desktop_shell(&exe, &args_line, &working_dir).is_ok() {
        std::process::exit(0);
    }

    // `explorer.exe <exe>` cannot forward arbitrary CLI arguments, so only use it
    // when no extra arguments need to be preserved and guard against loops with a
    // short-lived timestamp stamp.
    let has_extra_args = raw_args.iter().any(|arg| arg != RG_RELAUNCHED_ARG);
    if !has_extra_args && relaunch_via_explorer_fallback(&exe).is_ok() {
        std::process::exit(0);
    }
}

fn is_redirection_trust_enforced() -> bool {
    use windows::Win32::System::SystemServices::PROCESS_MITIGATION_REDIRECTION_TRUST_POLICY;
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessMitigationPolicy, ProcessRedirectionTrustPolicy,
    };

    let mut policy = PROCESS_MITIGATION_REDIRECTION_TRUST_POLICY::default();
    let ok = unsafe {
        GetProcessMitigationPolicy(
            GetCurrentProcess(),
            ProcessRedirectionTrustPolicy,
            &mut policy as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<PROCESS_MITIGATION_REDIRECTION_TRUST_POLICY>(),
        )
    };
    if ok.is_err() {
        return false;
    }
    // Bit 0 is `EnforceRedirectionTrustPolicy`.
    unsafe { (policy.Anonymous.Flags & 0x1) != 0 }
}

struct CoInitGuard(bool);

impl Drop for CoInitGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

fn relaunch_via_desktop_shell(
    exe: &Path,
    args_line: &str,
    working_dir: &Path,
) -> Result<(), String> {
    use windows::core::{Interface, BSTR};
    use windows::Win32::Foundation::S_FALSE;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED,
        COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Shell::{
        IShellDispatch2, IShellFolderViewDual, IShellWindows, IWebBrowserApp, ShellWindows,
        SWC_DESKTOP, SWFO_NEEDDISPATCH,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let _co_guard = if hr.is_ok() || hr == S_FALSE {
        CoInitGuard(true)
    } else {
        return Err(format!("CoInitializeEx failed: {hr:?}"));
    };

    let shell_windows: IShellWindows =
        unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|e| format!("CoCreateInstance(ShellWindows) failed: {e}"))?;

    let mut hwnd = 0i32;
    let empty = VARIANT::default();
    let desktop = unsafe {
        shell_windows.FindWindowSW(
            &VARIANT::from(0i32),
            &empty,
            SWC_DESKTOP,
            &mut hwnd,
            SWFO_NEEDDISPATCH,
        )
    }
    .map_err(|e| format!("FindWindowSW(SWC_DESKTOP) failed: {e}"))?;

    let browser: IWebBrowserApp = desktop
        .cast()
        .map_err(|e| format!("cast to IWebBrowserApp failed: {e}"))?;
    let document = unsafe { browser.Document() }
        .map_err(|e| format!("IWebBrowserApp::Document failed: {e}"))?;
    let view: IShellFolderViewDual = document
        .cast()
        .map_err(|e| format!("cast to IShellFolderViewDual failed: {e}"))?;
    let app = unsafe { view.Application() }
        .map_err(|e| format!("IShellFolderViewDual::Application failed: {e}"))?;
    let dispatch: IShellDispatch2 = app
        .cast()
        .map_err(|e| format!("cast to IShellDispatch2 failed: {e}"))?;

    let file = BSTR::from(exe.to_string_lossy().as_ref());
    let args = VARIANT::from(BSTR::from(args_line));
    let dir = VARIANT::from(BSTR::from(working_dir.to_string_lossy().as_ref()));
    let op = VARIANT::from(BSTR::from("open"));
    let show = VARIANT::from(SW_SHOWNORMAL.0);

    unsafe { dispatch.ShellExecute(&file, &args, &dir, &op, &show) }
        .map_err(|e| format!("IShellDispatch2::ShellExecute failed: {e}"))?;

    Ok(())
}

fn relaunch_via_explorer_fallback(exe: &Path) -> Result<(), String> {
    let stamp = std::env::temp_dir().join(FALLBACK_STAMP_NAME);
    let now = SystemTime::now();
    if should_skip_relaunch_by_stamp(&stamp, now, FALLBACK_GUARD_WINDOW) {
        return Err("recent fallback relaunch stamp exists".to_string());
    }
    write_fallback_stamp(&stamp, now);

    let explorer = PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows")),
    )
    .join("explorer.exe");

    std::process::Command::new(explorer)
        .arg(exe)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to spawn explorer.exe: {e}"))
}

fn has_rg_relaunch_arg<S: AsRef<OsStr>>(args: &[S]) -> bool {
    args.iter().any(|arg| arg.as_ref() == RG_RELAUNCHED_ARG)
}

fn build_relaunch_args_line<S: AsRef<OsStr>>(raw_args: &[S]) -> String {
    let mut parts: Vec<String> = raw_args
        .iter()
        .filter(|arg| arg.as_ref() != RG_RELAUNCHED_ARG)
        .map(|arg| quote_windows_command_arg(&arg.as_ref().to_string_lossy()))
        .collect();
    parts.push(RG_RELAUNCHED_ARG.to_string());
    parts.join(" ")
}

/// Quotes an argument following `CommandLineToArgvW` rules.
fn quote_windows_command_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| c == ' ' || c == '\t' || c == '"') {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                out.push(ch);
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

fn should_skip_relaunch_by_stamp(stamp: &Path, now: SystemTime, window: Duration) -> bool {
    let Ok(contents) = std::fs::read_to_string(stamp) else {
        return false;
    };
    let Ok(secs) = contents.trim().parse::<u64>() else {
        return false;
    };
    let recorded = UNIX_EPOCH + Duration::from_secs(secs);
    now.duration_since(recorded)
        .map(|elapsed| elapsed < window)
        .unwrap_or(false)
}

fn write_fallback_stamp(stamp: &Path, now: SystemTime) {
    if let Ok(elapsed) = now.duration_since(UNIX_EPOCH) {
        let _ = std::fs::write(stamp, elapsed.as_secs().to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_rg_relaunch_arg_detects_sentinel() {
        assert!(!has_rg_relaunch_arg(&["--silent", "ccswitch://open"]));
        assert!(has_rg_relaunch_arg(&[
            "--silent",
            RG_RELAUNCHED_ARG,
            "ccswitch://open"
        ]));
    }

    #[test]
    fn build_relaunch_args_line_preserves_and_quotes_args_without_duplicating_sentinel() {
        let line = build_relaunch_args_line(&[
            r"C:\Program Files\Test\",
            r#"say "hi""#,
            RG_RELAUNCHED_ARG,
            "ccswitch://import?name=foo",
        ]);
        assert_eq!(
            line,
            format!(
                r#""C:\Program Files\Test\\" "say \"hi\"" ccswitch://import?name=foo {RG_RELAUNCHED_ARG}"#
            )
        );
    }

    #[test]
    fn fallback_stamp_prevents_recent_relaunch_loop() {
        let dir = tempfile::tempdir().unwrap();
        let stamp = dir.path().join(FALLBACK_STAMP_NAME);
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);

        assert!(!should_skip_relaunch_by_stamp(
            &stamp,
            now,
            FALLBACK_GUARD_WINDOW
        ));

        write_fallback_stamp(&stamp, now);
        assert!(should_skip_relaunch_by_stamp(
            &stamp,
            now + Duration::from_secs(5),
            FALLBACK_GUARD_WINDOW
        ));
        assert!(!should_skip_relaunch_by_stamp(
            &stamp,
            now + Duration::from_secs(20),
            FALLBACK_GUARD_WINDOW
        ));
    }

    #[test]
    fn current_test_process_does_not_have_redirection_guard() {
        assert!(!is_redirection_trust_enforced());
    }

    #[test]
    fn is_redirection_trust_enforced_detects_enabled_policy_in_subprocess() {
        use windows::Win32::System::SystemServices::PROCESS_MITIGATION_REDIRECTION_TRUST_POLICY;
        use windows::Win32::System::Threading::{
            ProcessRedirectionTrustPolicy, SetProcessMitigationPolicy,
        };

        const CHILD: &str = "CC_SWITCH_TEST_RG_POLICY_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let mut policy = PROCESS_MITIGATION_REDIRECTION_TRUST_POLICY::default();
            policy.Anonymous.Flags = 0x1;
            unsafe {
                SetProcessMitigationPolicy(
                    ProcessRedirectionTrustPolicy,
                    &policy as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<PROCESS_MITIGATION_REDIRECTION_TRUST_POLICY>(),
                )
            }
            .expect("SetProcessMitigationPolicy(ProcessRedirectionTrustPolicy) should succeed");
            assert!(is_redirection_trust_enforced());
            return;
        }

        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "windows_launch_guard::tests::is_redirection_trust_enforced_detects_enabled_policy_in_subprocess",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}
