//! One-shot VPS command entry. No Tauri, database or GUI settings initialization.
use super::{ssh::exec, validate_id};
use crate::app_config::{AppType, SkillApps};
use serde::Serialize;
use std::ffi::OsString;
use std::path::{Component, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

const DEFAULT_TIMEOUT: u64 = 300;

#[derive(Debug, Clone)]
pub(super) struct ExecRequest {
    pub root: PathBuf,
    pub app: AppType,
    pub server_id: String,
    pub command: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CliError {
    InvalidArguments,
    DataInvalid,
    HostUnavailable,
    TrustRequired,
    TrustInvalid,
    TargetChanged,
    PasswordRequired,
    CredentialsUnavailable,
    PrepareFailed,
    SshNotFound,
    ExecutionFailed,
    Timeout,
    Cancelled,
}

impl CliError {
    fn key(self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalidArguments",
            Self::DataInvalid => "dataInvalid",
            Self::HostUnavailable => "hostUnavailable",
            Self::TrustRequired => "trustRequired",
            Self::TrustInvalid => "trustInvalid",
            Self::TargetChanged => "targetChanged",
            Self::PasswordRequired => "passwordRequired",
            Self::CredentialsUnavailable => "credentialsUnavailable",
            Self::PrepareFailed => "prepareFailed",
            Self::SshNotFound => "sshNotFound",
            Self::ExecutionFailed => "executionFailed",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
        }
    }

    fn exit_code(self) -> i32 {
        match self {
            Self::InvalidArguments => 2,
            Self::Timeout => 124,
            Self::SshNotFound => 127,
            Self::Cancelled => 130,
            _ => 125,
        }
    }
}

/// Options end at `--`; exactly one following string is interpreted by the REMOTE shell.
/// No SSH options or alternate targets can be forwarded through this entry.
fn parse(args: Vec<OsString>) -> Result<Option<ExecRequest>, CliError> {
    use CliError::InvalidArguments;
    if args == ["--help"] || args == ["exec", "--help"] {
        return Ok(None);
    }
    let mut args = args.into_iter();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("exec")) {
        return Err(InvalidArguments);
    }
    let mut root = None;
    let mut app = None;
    let mut server_id = None;
    let mut timeout = None;
    let mut command = None;
    while let Some(option) = args.next() {
        if option == "--" {
            command = Some(
                args.next()
                    .ok_or(InvalidArguments)?
                    .into_string()
                    .map_err(|_| InvalidArguments)?,
            );
            if args.next().is_some() {
                return Err(InvalidArguments);
            }
            break;
        }
        let value = args.next().ok_or(InvalidArguments)?;
        match option.to_str() {
            Some("--root") if root.is_none() => root = Some(PathBuf::from(value)),
            Some("--app") if app.is_none() => {
                let value = value.to_str().ok_or(InvalidArguments)?;
                let parsed = AppType::all()
                    .find(|app| app.as_str() == value)
                    .ok_or(InvalidArguments)?;
                if SkillApps::only(&parsed).is_empty() {
                    return Err(InvalidArguments);
                }
                app = Some(parsed);
            }
            Some("--server") if server_id.is_none() => {
                let value = value.into_string().map_err(|_| InvalidArguments)?;
                validate_id(&value).map_err(|_| InvalidArguments)?;
                server_id = Some(value);
            }
            Some("--timeout") if timeout.is_none() => {
                let value = value.to_str().ok_or(InvalidArguments)?;
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(InvalidArguments);
                }
                let seconds: u64 = value.parse().map_err(|_| InvalidArguments)?;
                if !(1..=86400).contains(&seconds) {
                    return Err(InvalidArguments);
                }
                timeout = Some(Duration::from_secs(seconds));
            }
            _ => return Err(InvalidArguments),
        }
    }
    let root = root.ok_or(InvalidArguments)?;
    let command = command.ok_or(InvalidArguments)?;
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || root
            .to_str()
            .is_none_or(|value| value.chars().any(char::is_control))
        || command.trim().is_empty()
        || command.len() > 65536
        || command.contains('\0')
    {
        return Err(InvalidArguments);
    }
    Ok(Some(ExecRequest {
        root,
        app: app.ok_or(InvalidArguments)?,
        server_id: server_id.ok_or(InvalidArguments)?,
        command,
        timeout: timeout.unwrap_or(Duration::from_secs(DEFAULT_TIMEOUT)),
    }))
}

// Read the same four i18next resources as the UI, without loading device settings or a webview.
fn message(key: &str, locale: &str) -> String {
    let normalized = locale.replace('_', "-").to_ascii_lowercase();
    let source = if normalized.starts_with("zh-tw")
        || normalized.starts_with("zh-hk")
        || normalized.starts_with("zh-hant")
    {
        include_str!("../../../../src/i18n/locales/zh-TW.json")
    } else if normalized.starts_with("zh") {
        include_str!("../../../../src/i18n/locales/zh.json")
    } else if normalized.starts_with("ja") {
        include_str!("../../../../src/i18n/locales/ja.json")
    } else {
        include_str!("../../../../src/i18n/locales/en.json")
    };
    let messages: serde_json::Value = serde_json::from_str(source).expect("bundled locale JSON");
    messages["vps"]["cli"][key]
        .as_str()
        .expect("bundled VPS CLI message")
        .to_owned()
}

fn report(error: CliError, locale: &str) -> i32 {
    // Stable machine-readable reason, localized guidance, no raw errors/commands/credentials.
    let _ = writeln_stderr(
        &serde_json::json!({
            "code": error.key(),
            "message": message(error.key(), locale),
        })
        .to_string(),
    );
    error.exit_code()
}

fn writeln_stderr(message: &str) -> std::io::Result<()> {
    use std::io::Write;
    writeln!(std::io::stderr().lock(), "{message}")
}

#[cfg(windows)]
fn attach_parent_console() -> Result<(), CliError> {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{GetFileType, FILE_TYPE_DISK, FILE_TYPE_PIPE},
        System::Console::{
            AttachConsole, GetConsoleCP, GetStdHandle, SetConsoleCtrlHandler, SetStdHandle,
            ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        },
    };
    // AttachConsole can replace standard handles. Preserve redirected pipes/files before
    // Rust stdio is first used, but let real console handles come from the attached console.
    let ids = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
    unsafe {
        let redirected = ids.map(|id| {
            let handle = GetStdHandle(id);
            if !handle.is_null()
                && handle != INVALID_HANDLE_VALUE
                && matches!(GetFileType(handle), FILE_TYPE_DISK | FILE_TYPE_PIPE)
            {
                Some(handle)
            } else {
                None
            }
        });
        // Already-attached and console-less parents are both normal; never allocate a window.
        AttachConsole(ATTACH_PARENT_PROCESS);
        for (id, handle) in ids.into_iter().zip(redirected) {
            if let Some(handle) = handle {
                if SetStdHandle(id, handle) == 0 {
                    return Err(CliError::ExecutionFailed);
                }
            }
        }
        if GetConsoleCP() != 0 && SetConsoleCtrlHandler(None, 0) == 0 {
            return Err(CliError::ExecutionFailed);
        }
    }
    Ok(())
}

async fn interrupted() -> std::io::Result<()> {
    #[cfg(windows)]
    if unsafe { windows_sys::Win32::System::Console::GetConsoleCP() } == 0 {
        // There is no console event source for detached, pipe-only callers. Their bounded
        // execution timeout still applies; do not open a console just to receive Ctrl-C.
        return std::future::pending().await;
    }
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}

/// Called after ASKPASS dispatch but before all GUI initialization. A malformed VPS invocation
/// must never fall through to the single-instance plugin or open the user's database.
pub fn dispatch() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("vps")) {
        return None;
    }
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en".into());
    #[cfg(windows)]
    if let Err(error) = attach_parent_console() {
        return Some(report(error, &locale));
    }
    let request = match parse(args.collect()) {
        Ok(Some(request)) => request,
        Ok(None) => {
            use std::io::Write;
            let _ = writeln!(std::io::stdout().lock(), "{}", message("usage", &locale));
            return Some(0);
        }
        Err(error) => return Some(report(error, &locale)),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return Some(report(CliError::ExecutionFailed, &locale)),
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let timeout = request.timeout;
    let result = runtime.block_on(async move {
        let mut worker = tokio::task::spawn_blocking(move || exec::execute(request, worker_cancelled));
        let error = tokio::select! {
            biased;
            signal = interrupted() => if signal.is_ok() { CliError::Cancelled } else { CliError::ExecutionFailed },
            _ = tokio::time::sleep(timeout) => CliError::Timeout,
            result = &mut worker => return result.unwrap_or(Err(CliError::ExecutionFailed)),
        };
        cancelled.store(true, Ordering::SeqCst);
        // Vault calls cannot be killed, but a late result must never launch SSH. The runner
        // normally observes cancellation within one poll and reaps its own child before returning.
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut worker).await;
        Err(error)
    });
    runtime.shutdown_background();
    Some(match result {
        Ok(code) => code,
        Err(error) => report(error, &locale),
    })
}

#[derive(Serialize)]
pub(super) struct ExecutionEntry {
    program: PathBuf,
    args: Vec<String>,
}

fn execution_program(executable: PathBuf, appimage: Option<OsString>) -> anyhow::Result<PathBuf> {
    let executable = appimage.map(PathBuf::from).unwrap_or(executable);
    if !executable.is_absolute()
        || executable
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || executable
            .to_str()
            .is_none_or(|value| value.chars().any(char::is_control))
    {
        anyhow::bail!(
            "The VPS execution entry must be an absolute Unicode path without control characters"
        );
    }
    Ok(executable)
}

pub(super) fn execution_entry(
    root: &std::path::Path,
    app: &AppType,
) -> anyhow::Result<ExecutionEntry> {
    let appimage = if cfg!(target_os = "linux") {
        std::env::var_os("APPIMAGE")
    } else {
        None
    };
    let executable = execution_program(std::env::current_exe()?, appimage)?;
    Ok(ExecutionEntry {
        program: executable,
        args: vec![
            "vps".into(),
            "exec".into(),
            "--root".into(),
            root.to_str()
                .ok_or_else(|| anyhow::anyhow!("VPS data directory must be Unicode"))?
                .into(),
            "--app".into(),
            app.as_str().into(),
        ],
    })
}

#[cfg(test)]
#[path = "cli/tests.rs"]
mod tests;
