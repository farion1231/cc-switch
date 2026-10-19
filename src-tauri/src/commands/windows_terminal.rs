#[cfg(target_os = "windows")]
use super::misc::{
    detach_claude_parent_session_env, effective_path_os, is_windows_app_execution_alias_dir,
};
#[cfg(any(target_os = "windows", test))]
use super::misc::{
    powershell_encoded_command, quote_windows_batch_path_for_env,
    windows_powershell_run_and_delete_batch, PARENT_CLAUDE_SESSION_ENV_VARS,
    WINDOWS_BATCH_PATH_ENV, WINDOWS_CONFIG_PATH_ENV, WINDOWS_CWD_ENV,
};
#[cfg(target_os = "windows")]
use crate::error::AppError;
#[cfg(target_os = "windows")]
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

#[cfg(any(target_os = "windows", test))]
const IO_REPARSE_TAG_APPEXECLINK: u32 = 0x8000_001b;
#[cfg(any(target_os = "windows", test))]
const WINDOWS_LAUNCH_ACK_ENV: &str = "CC_SWITCH_INTERNAL_LAUNCH_ACK";
#[cfg(any(target_os = "windows", test))]
const WINDOWS_LAUNCH_CLAIM_ENV: &str = "CC_SWITCH_INTERNAL_LAUNCH_CLAIM";
#[cfg(any(target_os = "windows", test))]
const POWERSHELL_CORE_PROFILE_SOURCE: &str = "Windows.Terminal.PowershellCore";
#[cfg(any(target_os = "windows", test))]
const POWERSHELL_CORE_PROFILE_GUID: &str = "574e775e-4f2a-5b96-ac1e-a2962a402336";

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WtDefaultShell {
    Pwsh,
    PowerShell,
    Cmd,
}

// A nonempty commandline can select another profile, even with append enabled.
// Always bind the shell-specific arguments to the selected profile's GUID.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct WtDefaultProfile {
    selector: String,
    shell: WtDefaultShell,
}

#[cfg(any(target_os = "windows", test))]
impl WtDefaultProfile {
    fn build_wt_args<'a>(
        &'a self,
        encoded_command: &'a str,
        cmd_bootstrap_command: &'a str,
    ) -> Vec<&'a str> {
        let mut args = vec![
            "new-tab",
            "--profile",
            self.selector.as_str(),
            "--appendCommandLine",
            "--",
        ];
        match self.shell {
            WtDefaultShell::Pwsh | WtDefaultShell::PowerShell => {
                args.extend(["-NoExit", "-EncodedCommand", encoded_command]);
            }
            WtDefaultShell::Cmd => {
                args.extend(["/V:OFF", "/S", "/K", cmd_bootstrap_command]);
            }
        }
        args
    }
}

#[cfg(any(target_os = "windows", test))]
const WINDOWS_PRIVATE_LAUNCH_ENV_VARS: &[&str] = &[
    WINDOWS_BATCH_PATH_ENV,
    WINDOWS_CONFIG_PATH_ENV,
    WINDOWS_CWD_ENV,
    WINDOWS_LAUNCH_ACK_ENV,
    WINDOWS_LAUNCH_CLAIM_ENV,
];

#[cfg(any(target_os = "windows", test))]
fn wt_env_vars_to_clear() -> impl Iterator<Item = &'static &'static str> {
    PARENT_CLAUDE_SESSION_ENV_VARS
        .iter()
        .chain(WINDOWS_PRIVATE_LAUNCH_ENV_VARS.iter())
}

// WT expands its commandline using the resident's environment. Bind this run's
// Unicode paths only after decoding, and clear stale session/color variables.
#[cfg(any(target_os = "windows", test))]
fn wt_powershell_bootstrap(bat_path: &str, launch_env: &[(&str, &str)]) -> String {
    let clear_env = format!(
        "foreach($n in {}){{[Environment]::SetEnvironmentVariable($n,$null)}};",
        wt_env_vars_to_clear()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let quoted_bat_path = quote_windows_batch_path_for_env(bat_path);
    let bind_env: String = std::iter::once((WINDOWS_BATCH_PATH_ENV, quoted_bat_path.as_str()))
        .chain(launch_env.iter().copied())
        .map(|(name, value)| {
            // PowerShell also recognizes these Unicode apostrophes as delimiters.
            let value = value
                .replace('\'', "''")
                .replace('\u{2018}', "\u{2018}\u{2018}")
                .replace('\u{2019}', "\u{2019}\u{2019}")
                .replace('\u{201a}', "\u{201a}\u{201a}")
                .replace('\u{201b}', "\u{201b}\u{201b}");
            format!("$env:{name}='{value}';")
        })
        .collect();
    let clear_private_env = format!(
        "foreach($n in {}){{[Environment]::SetEnvironmentVariable($n,$null)}};",
        WINDOWS_PRIVATE_LAUNCH_ENV_VARS
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(",")
    );
    // The semaphore grants execution once; the file only observes the handoff.
    // OpenExisting cannot resurrect a cancelled launch after the parent closes it.
    let acknowledge = format!(
        "if ($env:{WINDOWS_LAUNCH_ACK_ENV}) {{ \
         $receipt = $env:{WINDOWS_LAUNCH_ACK_ENV}; \
         $claim = [Threading.Semaphore]::OpenExisting($env:{WINDOWS_LAUNCH_CLAIM_ENV}); \
         try {{ if (-not $claim.WaitOne(0)) {{ throw 'Terminal launch was cancelled' }} }} \
         finally {{ $claim.Dispose() }}; \
         try {{ $stream = [IO.File]::Open($receipt, 'Open', 'Write', 'ReadWrite, Delete'); \
         try {{ $stream.WriteByte(1) }} finally {{ $stream.Dispose() }} }} catch {{ }}; \
         $env:{WINDOWS_LAUNCH_ACK_ENV} = $null; \
         $env:{WINDOWS_LAUNCH_CLAIM_ENV} = $null; }}; "
    );
    format!(
        "& {{ $ErrorActionPreference = 'Stop'; {clear_env}{bind_env}try {{ \
         $batchFile = [IO.File]::Open($env:{WINDOWS_BATCH_PATH_ENV}.Trim([char]34), 'Open', 'Read', 'ReadWrite, Delete'); $batchFile.Dispose(); \
         {acknowledge}{run_batch} \
         }} finally {{ {clear_private_env}}} }}",
        run_batch = windows_powershell_run_and_delete_batch(),
    )
}

#[cfg(any(target_os = "windows", test))]
fn wt_cmd_bootstrap_command(powershell_path: &str, encoded_command: &str) -> String {
    // Clear the interactive CMD too: a child cannot change its parent's env.
    let clear_env: String = wt_env_vars_to_clear()
        .map(|name| format!("set \"{name}=\" & "))
        .collect();
    // One quoted argv plus CMD /S preserves the executable's inner quotes.
    format!(
        "{clear_env}\"{powershell_path}\" -NoLogo -NoProfile -NonInteractive -EncodedCommand {encoded_command}"
    )
}

#[cfg(any(target_os = "windows", test))]
fn known_wt_shell(guid: uuid::Uuid) -> Option<WtDefaultShell> {
    match guid.to_string().as_str() {
        POWERSHELL_CORE_PROFILE_GUID => Some(WtDefaultShell::Pwsh),
        "61c54bbd-c2c6-5271-96e7-009a87ff44bf" => Some(WtDefaultShell::PowerShell),
        "0caa0dad-35be-5f56-a8ff-afceeeaa6101" => Some(WtDefaultShell::Cmd),
        _ => None,
    }
}

// Recognize only executable-only profiles and simple flag-only options. WT
// retains their original commandline; we do not emulate host option parsing.
#[cfg(any(target_os = "windows", test))]
fn classify_wt_commandline(commandline: &str) -> Option<WtDefaultShell> {
    let commandline = commandline.trim();
    let (executable, arguments) = if let Some(rest) = commandline.strip_prefix('"') {
        let end = rest.find('"')?;
        (&rest[..end], rest[end + 1..].trim())
    } else {
        let end = commandline
            .find(char::is_whitespace)
            .unwrap_or(commandline.len());
        (&commandline[..end], commandline[end..].trim())
    };
    let file_name = executable.rsplit(['\\', '/']).next()?.to_ascii_lowercase();
    let shell = match file_name.strip_suffix(".exe").unwrap_or(&file_name) {
        "cmd" => WtDefaultShell::Cmd,
        "powershell" => WtDefaultShell::PowerShell,
        "pwsh" => WtDefaultShell::Pwsh,
        _ => return None,
    };
    let supported_options: &[&str] = match shell {
        WtDefaultShell::Cmd => &["/d", "/q", "/s", "/v:off"],
        WtDefaultShell::Pwsh | WtDefaultShell::PowerShell => &["-nologo", "-noprofile", "-noexit"],
    };
    arguments
        .split_whitespace()
        .all(|argument| {
            supported_options
                .iter()
                .any(|option| argument.eq_ignore_ascii_case(option))
        })
        .then_some(shell)
}

#[cfg(any(target_os = "windows", test))]
fn parse_wt_default_profile(content: &str) -> Option<WtDefaultProfile> {
    let settings: serde_json::Value = json5::from_str(content).ok()?;
    let selected = settings.get("defaultProfile")?.as_str()?;
    let selected_guid =
        (selected.len() == 38 && selected.starts_with('{') && selected.ends_with('}'))
            .then(|| uuid::Uuid::parse_str(selected).ok())
            .flatten();
    let profiles = settings.get("profiles");
    let list = profiles
        .and_then(|profiles| profiles.get("list").or(Some(profiles)))
        .and_then(serde_json::Value::as_array);
    let profile_guid = |profile: &serde_json::Value| {
        profile
            .get("guid")
            .and_then(serde_json::Value::as_str)
            .and_then(|guid| uuid::Uuid::parse_str(guid).ok())
    };
    let profile = list.and_then(|list| {
        selected_guid
            .and_then(|guid| {
                list.iter()
                    .find(|profile| profile_guid(profile) == Some(guid))
            })
            .or_else(|| {
                list.iter().find(|profile| {
                    profile.get("name").and_then(serde_json::Value::as_str) == Some(selected)
                })
            })
    });
    let disabled = |source: &str| {
        settings
            .get("disabledProfileSources")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|sources| sources.iter().any(|value| value.as_str() == Some(source)))
    };
    let inherited_commandline = profiles
        .and_then(|profiles| profiles.get("defaults"))
        .and_then(|defaults| defaults.get("commandline"));
    let (guid, shell) = if let Some(profile) = profile {
        // Name-only UUID generation and third-party sources belong to WT.
        let guid = profile_guid(profile)?;
        let source = profile.get("source").and_then(serde_json::Value::as_str);
        if source.is_some_and(|source| source != POWERSHELL_CORE_PROFILE_SOURCE || disabled(source))
        {
            return None;
        }
        let shell = if let Some(commandline) = profile.get("commandline") {
            classify_wt_commandline(commandline.as_str()?)?
        } else {
            // Do not reproduce version-dependent profiles.defaults inheritance.
            if inherited_commandline.is_some() {
                return None;
            }
            if source == Some(POWERSHELL_CORE_PROFILE_SOURCE) {
                WtDefaultShell::Pwsh
            } else {
                known_wt_shell(guid)?
            }
        };
        (guid, shell)
    } else {
        let guid = selected_guid?;
        if inherited_commandline.is_some()
            || (guid.to_string() == POWERSHELL_CORE_PROFILE_GUID
                && disabled(POWERSHELL_CORE_PROFILE_SOURCE))
        {
            return None;
        }
        (guid, known_wt_shell(guid)?)
    };
    Some(WtDefaultProfile {
        selector: format!("{{{guid}}}"),
        shell,
    })
}

#[cfg(any(target_os = "windows", test))]
fn parse_app_execution_alias_target(buffer: &[u8]) -> Option<Vec<u16>> {
    if u32::from_le_bytes(buffer.get(..4)?.try_into().ok()?) != IO_REPARSE_TAG_APPEXECLINK {
        return None;
    }
    let length = u16::from_le_bytes(buffer.get(4..6)?.try_into().ok()?) as usize;
    let payload = buffer.get(8..8usize.checked_add(length)?)?;
    // Version 3+ stores NUL-terminated package, application and target strings.
    if u32::from_le_bytes(payload.get(..4)?.try_into().ok()?) < 3 || (payload.len() - 4) % 2 != 0 {
        return None;
    }
    let strings: Vec<u16> = payload[4..]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    let mut start = 0;
    for index in 0..3 {
        let end = start + strings.get(start..)?.iter().position(|value| *value == 0)?;
        if index == 2 {
            let target = strings.get(start..end)?;
            let drive = *target.first()?;
            let absolute = ((b'A' as u16..=b'Z' as u16).contains(&drive)
                || (b'a' as u16..=b'z' as u16).contains(&drive))
                && target.get(1) == Some(&(b':' as u16))
                && target.get(2) == Some(&(b'\\' as u16));
            return absolute.then(|| target.to_vec());
        }
        start = end + 1;
    }
    None
}

#[cfg(target_os = "windows")]
fn resolve_app_execution_alias_target(alias: &Path) -> Option<PathBuf> {
    use std::os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    use windows_sys::Win32::System::{Ioctl::FSCTL_GET_REPARSE_POINT, IO::DeviceIoControl};

    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(alias)
        .ok()?;
    let mut buffer = vec![0u8; 16 * 1024];
    let mut returned = 0;
    // SAFETY: the file owns the handle and the output buffer is writable.
    let succeeded = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_GET_REPARSE_POINT,
            std::ptr::null(),
            0,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if succeeded == 0 {
        return None;
    }
    let target = parse_app_execution_alias_target(buffer.get(..returned as usize)?)?;
    Some(PathBuf::from(std::ffi::OsString::from_wide(&target)))
}

#[cfg(target_os = "windows")]
struct ResolvedWtInstallation {
    launcher: PathBuf,
    executable: PathBuf,
}

#[cfg(target_os = "windows")]
fn resolve_wt_installation() -> Option<ResolvedWtInstallation> {
    // Read PATH as Unicode paths instead of changing global `where` decoding.
    let launcher = std::env::split_paths(&effective_path_os()?)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join("wt.exe"))
        .find(|path| path.is_file())?;
    let executable = if launcher
        .parent()
        .is_some_and(is_windows_app_execution_alias_dir)
    {
        resolve_app_execution_alias_target(&launcher)?
    } else {
        let shim = launcher.with_extension("shim");
        if shim.is_file() {
            let content = std::fs::read_to_string(shim).ok()?;
            let target = content.lines().find_map(|line| {
                line.trim()
                    .trim_start_matches('\u{feff}')
                    .strip_prefix("path =")
                    .map(|value| PathBuf::from(value.trim().trim_matches('"')))
            })?;
            if !target.is_absolute() {
                return None;
            }
            target
        } else {
            launcher.clone()
        }
    };
    Some(ResolvedWtInstallation {
        launcher,
        executable,
    })
}

#[cfg(target_os = "windows")]
fn wt_settings_path(executable: &Path, local_appdata: Option<&Path>) -> Option<PathBuf> {
    let directory = executable.parent()?;
    if directory.join(".portable").is_file() {
        // Scoop's sibling settings junction already resolves to persist/settings.
        return Some(directory.join("settings").join("settings.json"));
    }
    let local = local_appdata?;
    for ancestor in executable.ancestors() {
        if ancestor
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name.eq_ignore_ascii_case("WindowsApps"))
        {
            let package = ancestor.file_name()?.to_string_lossy().to_ascii_lowercase();
            let family = if package.starts_with("microsoft.windowsterminalpreview_") {
                "Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe"
            } else if package.starts_with("microsoft.windowsterminal_") {
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe"
            } else {
                return None;
            };
            return Some(
                local
                    .join("Packages")
                    .join(family)
                    .join("LocalState/settings.json"),
            );
        }
    }
    Some(local.join("Microsoft/Windows Terminal/settings.json"))
}

#[cfg(target_os = "windows")]
fn wt_file_version(path: &Path) -> Option<(u16, u16)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut dummy = 0;
    // SAFETY: the path is NUL terminated and the version buffer is writable.
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), &mut dummy) };
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    if unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, buffer.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let mut info = std::ptr::null_mut();
    let mut length = 0;
    let root: Vec<u16> = "\\\0".encode_utf16().collect();
    // SAFETY: `buffer` owns the version block; both output pointers remain valid.
    if unsafe {
        VerQueryValueW(
            buffer.as_ptr().cast(),
            root.as_ptr(),
            &mut info,
            &mut length,
        )
    } == 0
        || info.is_null()
        || length < 16
    {
        return None;
    }
    // VS_FIXEDFILEINFO stores fileVersionMS after its signature and struct version.
    let bytes = unsafe { std::slice::from_raw_parts(info.cast::<u8>(), length as usize) };
    let version = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?);
    Some(((version >> 16) as u16, version as u16))
}

#[cfg(target_os = "windows")]
struct WtLaunchReceipt {
    file: tempfile::NamedTempFile,
    semaphore: std::os::windows::io::OwnedHandle,
    semaphore_name: String,
}

#[cfg(target_os = "windows")]
impl WtLaunchReceipt {
    fn new(file: tempfile::NamedTempFile) -> std::io::Result<Self> {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::{
            Foundation::{SetLastError, ERROR_ALREADY_EXISTS},
            System::Threading::CreateSemaphoreW,
        };

        let semaphore_name = format!("Local\\CCSwitchLaunch-{}", uuid::Uuid::new_v4());
        let name: Vec<u16> = semaphore_name.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe {
            SetLastError(0);
            CreateSemaphoreW(std::ptr::null(), 1, 1, name.as_ptr())
        };
        let error = std::io::Error::last_os_error();
        if handle.is_null() {
            return Err(error);
        }
        let semaphore = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) };
        if error.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) {
            return Err(error);
        }
        Ok(Self {
            file,
            semaphore,
            semaphore_name,
        })
    }

    fn path(&self) -> &Path {
        self.file.path()
    }

    fn cancel_or_preserve(&self, error: String) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::Threading::WaitForSingleObject,
        };

        // Neither side releases the count. Even an already-open late WT handle
        // cannot execute after cancellation; an unopened one cannot recreate it.
        match unsafe { WaitForSingleObject(self.semaphore.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Err(error),
            WAIT_TIMEOUT => Ok(()),
            status => {
                log::error!("无法取消 WT 启动，保留其执行权以防重复启动: {status:#x}; {error}");
                Ok(())
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn run_wt_command(
    command: &mut Command,
    receipt: &WtLaunchReceipt,
    timeout: Duration,
) -> Result<(), String> {
    detach_claude_parent_session_env(command);
    for name in WINDOWS_PRIVATE_LAUNCH_ENV_VARS {
        command.env_remove(name);
    }
    let result = (|| {
        let mut child = command
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("启动 Windows Terminal 失败: {error}"))?;
        let deadline = Instant::now() + timeout;
        loop {
            if std::fs::read(receipt.path()).is_ok_and(|value| value == [1]) {
                return Ok(());
            }
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("等待 Windows Terminal 失败: {error}"))?
            {
                if !status.success() {
                    return Err(format!("Windows Terminal 启动失败: {status}"));
                }
            }
            if Instant::now() >= deadline {
                return Err(AppError::localized(
                    "terminal.wt.startup_timeout",
                    "Windows Terminal 未确认执行启动脚本",
                    "Windows Terminal did not acknowledge the startup script",
                )
                .to_string());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })();
    match result {
        Err(error) => receipt.cancel_or_preserve(error),
        result => result,
    }
}

#[cfg(target_os = "windows")]
pub(super) fn launch_wt_terminal_with_env(
    bat_path: &str,
    launch_env: &[(&str, &str)],
    effective_path: &str,
) -> Result<(), String> {
    let installation = resolve_wt_installation().ok_or_else(|| {
        AppError::localized(
            "terminal.wt.not_found",
            "未找到 Windows Terminal 可执行文件",
            "Windows Terminal executable not found",
        )
        .to_string()
    })?;
    let sibling = installation
        .executable
        .parent()
        .unwrap()
        .join("WindowsTerminal.exe");
    let module = if sibling.is_file() {
        &sibling
    } else {
        &installation.executable
    };
    if wt_file_version(module).is_none_or(|version| version < (1, 19)) {
        return Err(AppError::localized(
            "terminal.wt.append_unsupported",
            "此启动方式需要 Windows Terminal 1.19 或更新版本",
            "Commandline append requires Windows Terminal 1.19 or later",
        )
        .to_string());
    }
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs::data_local_dir);
    let settings =
        wt_settings_path(&installation.executable, local.as_deref()).ok_or_else(|| {
            AppError::localized(
                "terminal.wt.settings_unavailable",
                "无法定位当前 Windows Terminal 的设置文件",
                "Cannot locate this Windows Terminal installation's settings file",
            )
            .to_string()
        })?;
    let content = std::fs::read_to_string(&settings)
        .map_err(|error| AppError::io(&settings, error).to_string())?;
    let profile = parse_wt_default_profile(&content);
    if profile.is_none() {
        log::warn!("Windows Terminal 默认 profile 不受支持，改用显式 CMD 标签");
    }
    launch_wt_tab(
        &installation.launcher,
        profile.as_ref(),
        bat_path,
        launch_env,
        effective_path,
    )
}

#[cfg(target_os = "windows")]
fn launch_wt_tab(
    launcher: &Path,
    profile: Option<&WtDefaultProfile>,
    bat_path: &str,
    launch_env: &[(&str, &str)],
    effective_path: &str,
) -> Result<(), String> {
    let receipt = tempfile::NamedTempFile::new()
        .and_then(WtLaunchReceipt::new)
        .map_err(|error| format!("创建终端启动回执失败: {error}"))?;
    let receipt_path = receipt.path().to_string_lossy();
    let cmd_path = super::windows_console::system_cmd_path()
        .map_err(|error| format!("定位系统 cmd 失败: {error}"))?;
    let cmd_path = cmd_path.to_string_lossy();
    let mut environment = launch_env.to_vec();
    environment.extend([
        (WINDOWS_LAUNCH_ACK_ENV, receipt_path.as_ref()),
        (WINDOWS_LAUNCH_CLAIM_ENV, receipt.semaphore_name.as_str()),
        ("ComSpec", cmd_path.as_ref()),
    ]);
    let encoded = powershell_encoded_command(&wt_powershell_bootstrap(bat_path, &environment));
    let powershell = super::windows_console::system_powershell_path()
        .map_err(|error| format!("定位系统 PowerShell 失败: {error}"))?;
    let bootstrap = wt_cmd_bootstrap_command(&powershell.to_string_lossy(), &encoded);
    let mut args = if let Some(profile) = profile {
        profile.build_wt_args(&encoded, &bootstrap)
    } else {
        // Replace the unsupported profile's commandline, rather than appending CMD
        // arguments to a WSL, Git Bash or other unrelated shell.
        vec![
            "new-tab",
            "--",
            cmd_path.as_ref(),
            "/V:OFF",
            "/S",
            "/K",
            &bootstrap,
        ]
    };
    let cwd = launch_env
        .iter()
        .find_map(|(name, value)| (*name == WINDOWS_CWD_ENV).then_some(*value));
    let mut command = Command::new(launcher);
    // Like the original `start wt`, the new tab inherits PATH from wt.exe. Inside the
    // encoded payload it would push CMD past 8191 characters, and PowerShell would
    // overwrite PATH changes made by the user's profile.
    command.env("PATH", effective_path);
    if let Some(cwd) = cwd {
        // Resolve '.' against this invocation's cwd, without exposing literal
        // %VAR% path components to WT's environment expansion.
        args.splice(1..1, ["--startingDirectory", "."]);
        command.current_dir(cwd);
    }
    command.args(args);
    run_wt_command(&mut command, &receipt, Duration::from_secs(15))
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::super::misc::{configure_windows_batch_env, decode_command_output};
    use super::*;

    #[test]
    fn default_profile_identity_is_preserved() {
        let content = r#"{
            // The default is not the first matching shell.
            "defaultProfile": "Windows PowerShell",
            "profiles": {"list": [
                {"guid": "{11111111-1111-4111-8111-111111111111}", "name": "Other", "commandline": "powershell.exe"},
                {"guid": "{22222222-2222-4222-8222-222222222222}", "name": "Windows PowerShell", "commandline": "powershell.exe -NoLogo"},
            ]},
        }"#;
        let profile = parse_wt_default_profile(content).unwrap();
        assert_eq!(profile.selector, "{22222222-2222-4222-8222-222222222222}");
        assert_eq!(profile.shell, WtDefaultShell::PowerShell);
        let args = profile.build_wt_args("encoded", "bootstrap");
        assert_eq!(
            args,
            [
                "new-tab",
                "--profile",
                profile.selector.as_str(),
                "--appendCommandLine",
                "--",
                "-NoExit",
                "-EncodedCommand",
                "encoded"
            ]
        );
    }

    #[test]
    fn default_profile_supports_the_three_shell_families() {
        for (commandline, shell) in [
            ("cmd.exe /D /Q", WtDefaultShell::Cmd),
            (
                r#""C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile"#,
                WtDefaultShell::PowerShell,
            ),
            (
                r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#,
                WtDefaultShell::Pwsh,
            ),
        ] {
            let settings = serde_json::json!({
                "defaultProfile": "{11111111-1111-4111-8111-111111111111}",
                "profiles": {"list": [{"guid": "{11111111-1111-4111-8111-111111111111}", "commandline": commandline}]},
            });
            assert_eq!(
                parse_wt_default_profile(&settings.to_string())
                    .unwrap()
                    .shell,
                shell
            );
        }
        let builtin = r#"{"defaultProfile":"{574e775e-4f2a-5b96-ac1e-a2962a402336}"}"#;
        assert_eq!(
            parse_wt_default_profile(builtin).unwrap().shell,
            WtDefaultShell::Pwsh
        );
    }

    #[test]
    fn parse_wt_default_profile_refuses_complex_profile_commands() {
        for commandline in [
            "powershell.exe -ExecutionPolicy Bypass",
            "pwsh.exe -WorkingDirectory C:/projects",
            "cmd.exe /E:OFF /V:ON",
            "wrapper.exe powershell.exe",
            "wsl.exe",
        ] {
            let settings = serde_json::json!({
                "defaultProfile": "{11111111-1111-4111-8111-111111111111}",
                "profiles": {"list": [{"guid": "{11111111-1111-4111-8111-111111111111}", "commandline": commandline}]},
            });
            assert!(
                parse_wt_default_profile(&settings.to_string()).is_none(),
                "{commandline}"
            );
        }
        for settings in [
            r#"{"defaultProfile":"Custom","profiles":{"list":[{"name":"Custom","commandline":"pwsh.exe"}]}}"#,
            r#"{"defaultProfile":"{574e775e-4f2a-5b96-ac1e-a2962a402336}","disabledProfileSources":["Windows.Terminal.PowershellCore"]}"#,
            r#"{"defaultProfile":"{61c54bbd-c2c6-5271-96e7-009a87ff44bf}","profiles":{"defaults":{"commandline":"cmd.exe"}}}"#,
        ] {
            assert!(parse_wt_default_profile(settings).is_none(), "{settings}");
        }
    }

    #[test]
    fn app_execution_alias_target_is_bounded() {
        let target =
            r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.25\WindowsTerminal.exe";
        let strings: Vec<u8> = format!("package\0application\0{target}\0")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let length = 4 + strings.len();
        let mut buffer = Vec::new();
        buffer.extend(IO_REPARSE_TAG_APPEXECLINK.to_le_bytes());
        buffer.extend((length as u16).to_le_bytes());
        buffer.extend([0, 0]);
        buffer.extend(3u32.to_le_bytes());
        buffer.extend(strings);
        assert_eq!(
            parse_app_execution_alias_target(&buffer),
            Some(target.encode_utf16().collect())
        );
        for end in [0, 4, 8, buffer.len() - 1] {
            assert_eq!(parse_app_execution_alias_target(&buffer[..end]), None);
        }
        buffer[0] = 0;
        assert_eq!(parse_app_execution_alias_target(&buffer), None);
    }

    #[cfg(target_os = "windows")]
    struct EnvRestore(Vec<(&'static str, Option<std::ffi::OsString>)>);

    #[cfg(target_os = "windows")]
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (name, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn wt_child_command_line(args: &[&str]) -> String {
        // Match WT's quoting of one complete CMD bootstrap argv.
        let start = args.iter().position(|argument| *argument == "--").unwrap() + 1;
        args[start..]
            .iter()
            .map(|argument| {
                let argument = if *argument == "/K" { "/C" } else { argument };
                if argument.contains(' ') {
                    format!("\"{argument}\"")
                } else {
                    argument.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wt_launch_does_not_report_spawn_as_script_execution() {
        let receipt = WtLaunchReceipt::new(tempfile::NamedTempFile::new().unwrap()).unwrap();
        // Rerunning this test binary with a filter that matches nothing exits 0 without
        // running a script, like WT opening an empty tab.
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "cc_switch_fake_wt_without_a_script"]);
        let error = run_wt_command(&mut command, &receipt, Duration::from_millis(100)).unwrap_err();
        assert!(error.contains("未确认执行启动脚本"), "{error}");
        assert!(std::fs::read(receipt.path()).unwrap().is_empty());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wt_timeout_and_cmd_fallback_execute_the_batch_only_once() {
        use super::super::misc::WINDOWS_RUN_AND_DELETE_BATCH;
        use std::process::{Child, Stdio};

        struct Shells {
            children: Vec<Child>,
            gates: Vec<PathBuf>,
        }
        impl Drop for Shells {
            fn drop(&mut self) {
                for gate in &self.gates {
                    let _ = std::fs::write(gate, b"go");
                }
                for child in &mut self.children {
                    let _ = child.wait();
                }
            }
        }
        let wait_until = |condition: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !condition() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            condition()
        };
        for case in [
            "cancel-before-open",
            "cancel-after-open",
            "wt-first",
            "missing-ack",
            "launcher-failed",
        ] {
            let cancel_first = !matches!(case, "wt-first" | "missing-ack");
            let (variable, mode) = match case {
                "cancel-after-open" => ("claim", "Read"),
                "missing-ack" => ("receipt", "Read"),
                _ => ("receipt", "Write"),
            };
            let temp = tempfile::tempdir().unwrap();
            let batch = temp.path().join("provider.bat");
            let counter = temp.path().join("executions.txt");
            let ready = temp.path().join("ready");
            let acknowledge = temp.path().join("acknowledge");
            let finish = temp.path().join("finish");
            let receipt =
                WtLaunchReceipt::new(tempfile::NamedTempFile::new_in(temp.path()).unwrap())
                    .unwrap();
            let receipt_path = receipt.path().to_path_buf();
            let semaphore_name = receipt.semaphore_name.clone();
            let mut shells = Shells {
                children: Vec::new(),
                gates: vec![acknowledge.clone(), finish.clone()],
            };
            std::fs::write(
                &batch,
                "@echo off\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\n>> \"%CC_SWITCH_INTERNAL_CONFIG_PATH%\" echo executed\r\n\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoProfile -NonInteractive -Command \"$limit = [DateTime]::UtcNow.AddSeconds(10); while (-not [IO.File]::Exists($env:CC_SWITCH_TEST_FINISH)) { if ([DateTime]::UtcNow -ge $limit) { exit 7 }; [Threading.Thread]::Sleep(10) }\"\r\n",
            )
            .unwrap();
            let receipt_value = receipt_path.to_string_lossy();
            let counter_value = counter.to_string_lossy();
            let finish_value = finish.to_string_lossy();
            let env = [
                (WINDOWS_LAUNCH_ACK_ENV, receipt_value.as_ref()),
                (WINDOWS_LAUNCH_CLAIM_ENV, semaphore_name.as_str()),
                (WINDOWS_CONFIG_PATH_ENV, counter_value.as_ref()),
                ("CC_SWITCH_TEST_FINISH", finish_value.as_ref()),
            ];
            // Pause the real bootstrap at its receipt handoff. In the old protocol
            // the receipt stream is already open, so deleting its path cannot cancel it.
            let script = format!(
                "$null = Set-PSBreakpoint -Variable {variable} -Mode {mode} -Action {{ \
                 [IO.File]::WriteAllText($env:CC_SWITCH_TEST_READY, 'ready'); \
                 $limit = [DateTime]::UtcNow.AddSeconds(10); \
                 while (-not [IO.File]::Exists($env:CC_SWITCH_TEST_ACKNOWLEDGE)) {{ \
                 if ([DateTime]::UtcNow -ge $limit) {{ throw 'receipt gate timed out' }}; \
                 [Threading.Thread]::Sleep(10) }} }}; {}",
                wt_powershell_bootstrap(&batch.to_string_lossy(), &env)
            );
            let encoded = powershell_encoded_command(&script);
            shells.children.push(
                Command::new(super::super::windows_console::system_powershell_path().unwrap())
                    .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
                    .env("CC_SWITCH_TEST_READY", &ready)
                    .env("CC_SWITCH_TEST_ACKNOWLEDGE", &acknowledge)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
                    .unwrap(),
            );
            assert!(
                wait_until(&|| ready.is_file()),
                "bootstrap did not reach the receipt gate"
            );
            let count = || {
                std::fs::read_to_string(&counter)
                    .unwrap_or_default()
                    .lines()
                    .count()
            };
            if case == "wt-first" {
                std::fs::write(&acknowledge, b"go").unwrap();
                assert!(wait_until(&|| count() == 1), "WT did not start the batch");
            }
            // Model a resident tab independent of the short-lived WT launcher.
            let mut launcher = if case == "launcher-failed" {
                let mut command =
                    Command::new(super::super::windows_console::system_cmd_path().unwrap());
                command.args(["/D", "/C", "exit 37"]);
                command
            } else {
                let mut command = Command::new(std::env::current_exe().unwrap());
                command.args(["--exact", "cc_switch_fake_wt_without_a_script"]);
                command
            };
            let timeout = if case == "launcher-failed" {
                Duration::from_secs(5)
            } else {
                Duration::ZERO
            };
            let result = run_wt_command(&mut launcher, &receipt, timeout);
            if case == "launcher-failed" {
                assert!(result
                    .as_ref()
                    .unwrap_err()
                    .contains("Windows Terminal 启动失败"));
            }
            drop(receipt);
            if result.is_err() {
                let mut fallback =
                    Command::new(super::super::windows_console::system_cmd_path().unwrap());
                configure_windows_batch_env(&mut fallback, &batch.to_string_lossy(), &env);
                fallback.args(["/D", "/V:OFF", "/C", WINDOWS_RUN_AND_DELETE_BATCH]);
                fallback.creation_flags(CREATE_NO_WINDOW);
                shells.children.push(fallback.spawn().unwrap());
                assert!(
                    wait_until(&|| count() >= 1),
                    "CMD fallback did not start the batch"
                );
            }
            std::fs::write(&acknowledge, b"go").unwrap();
            if case == "missing-ack" {
                assert!(
                    wait_until(&|| count() == 1),
                    "missing ACK blocked the claimed batch"
                );
            }
            // Keep the BAT alive in the first shell until the late WT bootstrap
            // has either attempted it or exited. This is a still-running provider.
            let deadline = Instant::now() + Duration::from_secs(10);
            while cancel_first
                && count() < 2
                && shells.children[0].try_wait().unwrap().is_none()
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::fs::write(&finish, b"go").unwrap();
            for child in &mut shells.children {
                child.wait().unwrap();
            }
            assert_eq!(
                count(),
                1,
                "cancel_first={cancel_first}: batch executed more than once"
            );
            assert_eq!(
                result.is_err(),
                cancel_first,
                "wrong shell won the handoff: {result:?}"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wt_bootstrap_does_not_acknowledge_an_unreadable_batch() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let batch = temp.path().join("locked.bat");
        std::fs::write(&batch, b"@echo off\r\nexit /b 0\r\n").unwrap();
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&batch)
            .unwrap();
        let receipt =
            WtLaunchReceipt::new(tempfile::NamedTempFile::new_in(temp.path()).unwrap()).unwrap();
        let receipt_path = receipt.path().to_string_lossy();
        let script = wt_powershell_bootstrap(
            &batch.to_string_lossy(),
            &[
                (WINDOWS_LAUNCH_ACK_ENV, receipt_path.as_ref()),
                (WINDOWS_LAUNCH_CLAIM_ENV, receipt.semaphore_name.as_str()),
            ],
        );
        let encoded = powershell_encoded_command(&script);
        let output = Command::new(super::super::windows_console::system_powershell_path().unwrap())
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap();
        assert!(
            std::fs::read(receipt.path()).unwrap().is_empty(),
            "unreadable batch was acknowledged; stderr={}",
            decode_command_output(&output.stderr)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn settings_path_belongs_to_the_launched_installation() {
        let temp = tempfile::tempdir().unwrap();
        let portable = temp.path().join("scoop/apps/windows-terminal/current");
        std::fs::create_dir_all(&portable).unwrap();
        std::fs::write(portable.join(".portable"), "").unwrap();
        let local = temp.path().join("LocalAppData");
        assert_eq!(
            wt_settings_path(&portable.join("wt.exe"), Some(&local)),
            Some(portable.join("settings/settings.json"))
        );
        for (package, family) in [
            (
                "Microsoft.WindowsTerminal_1.25",
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
            ),
            (
                "Microsoft.WindowsTerminalPreview_1.25",
                "Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe",
            ),
        ] {
            let executable = temp
                .path()
                .join("WindowsApps")
                .join(package)
                .join("WindowsTerminal.exe");
            assert_eq!(
                wt_settings_path(&executable, Some(&local)),
                Some(
                    local
                        .join("Packages")
                        .join(family)
                        .join("LocalState/settings.json")
                )
            );
        }
        assert_eq!(
            wt_settings_path(&temp.path().join("unpackaged/wt.exe"), Some(&local)),
            Some(local.join("Microsoft/Windows Terminal/settings.json"))
        );
    }

    // Use the actual Win32 expansion boundary and real shells, not a mocked parser.
    #[cfg(target_os = "windows")]
    #[test]
    #[serial_test::serial]
    fn wt_batch_path_is_expanded_only_in_the_new_shell() {
        fn expand_in_resident_environment(value: &str) -> String {
            use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
            let source: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
            let size =
                unsafe { ExpandEnvironmentStringsW(source.as_ptr(), std::ptr::null_mut(), 0) };
            assert!(size > 0);
            let mut target = vec![0; size as usize];
            let written =
                unsafe { ExpandEnvironmentStringsW(source.as_ptr(), target.as_mut_ptr(), size) };
            assert!(written > 0 && written <= size);
            String::from_utf16(&target[..written as usize - 1]).unwrap()
        }
        let names = [
            WINDOWS_BATCH_PATH_ENV,
            WINDOWS_CONFIG_PATH_ENV,
            WINDOWS_CWD_ENV,
            WINDOWS_LAUNCH_ACK_ENV,
            WINDOWS_LAUNCH_CLAIM_ENV,
            "CC_SWITCH_CMD_EXPAND",
            "CC_SWITCH_BANG",
        ];
        let _restore = EnvRestore(
            names
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect(),
        );
        std::env::set_var("CC_SWITCH_CMD_EXPAND", "wrong");
        std::env::set_var("CC_SWITCH_BANG", "wrong");
        let temp = tempfile::tempdir().unwrap();
        let hostile = temp.path().join(
            "迅雷下载 space & %CC_SWITCH_CMD_EXPAND% ^ !CC_SWITCH_BANG! (test) O'Brien ‘ ’ ‚ ‛",
        );
        let starting = temp.path().join("starting-directory");
        std::fs::create_dir(&hostile).unwrap();
        std::fs::create_dir(&starting).unwrap();
        // A cwd lookup would fail instead of running the system bootstrap.
        std::fs::write(starting.join("powershell.exe"), b"not a Windows executable").unwrap();
        let marker = hostile.join("配置结果.txt");
        let marker_path = marker.to_string_lossy();
        let cwd_path = hostile.to_string_lossy();
        let cmd = super::super::windows_console::system_cmd_path().unwrap();
        let powershell = super::super::windows_console::system_powershell_path().unwrap();
        let cmd_path = cmd.to_string_lossy();
        let check_env: String = PARENT_CLAUDE_SESSION_ENV_VARS
            .iter()
            .map(|name| format!("if defined {name} exit /b 2\r\n"))
            .collect();
        let mut batches = Vec::new();
        for (label, directory) in [("A", temp.path()), ("B", hostile.as_path())] {
            let batch = tempfile::Builder::new()
                .prefix(label)
                .suffix(".bat")
                .tempfile_in(directory)
                .unwrap();
            let content = format!("@echo off\r\nsetlocal DisableDelayedExpansion\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\n{check_env}if defined CC_SWITCH_INTERNAL_LAUNCH_ACK exit /b 6\r\nif defined CC_SWITCH_INTERNAL_LAUNCH_CLAIM exit /b 7\r\ncd /d \"%CC_SWITCH_INTERNAL_CWD%\" || exit /b 5\r\n> \"%CC_SWITCH_INTERNAL_CONFIG_PATH%\" echo {label}\r\n");
            assert!(content.is_ascii());
            std::fs::write(batch.path(), content).unwrap();
            batches.push(batch);
        }
        let batch_contents: Vec<_> = batches
            .iter()
            .map(|batch| std::fs::read(batch.path()).unwrap())
            .collect();
        let parent_env_check: String = PARENT_CLAUDE_SESSION_ENV_VARS
            .iter()
            .map(|name| format!(" & if defined {name} echo cc-switch-parent-env-leak-{name}"))
            .collect();
        let mut failures = Vec::new();
        for stale in [false, true] {
            for name in WINDOWS_PRIVATE_LAUNCH_ENV_VARS {
                if stale {
                    std::env::set_var(name, "stale-value");
                } else {
                    std::env::remove_var(name);
                }
            }
            for forwarded in &batches {
                for shell in [
                    WtDefaultShell::Cmd,
                    WtDefaultShell::PowerShell,
                    WtDefaultShell::Pwsh,
                ] {
                    for (batch, content) in batches.iter().zip(&batch_contents) {
                        std::fs::write(batch.path(), content).unwrap();
                    }
                    let receipt =
                        WtLaunchReceipt::new(tempfile::NamedTempFile::new_in(&hostile).unwrap())
                            .unwrap();
                    let receipt_path = receipt.path().to_string_lossy();
                    let launch_env = [
                        (WINDOWS_CONFIG_PATH_ENV, marker_path.as_ref()),
                        (WINDOWS_CWD_ENV, cwd_path.as_ref()),
                        (WINDOWS_LAUNCH_ACK_ENV, receipt_path.as_ref()),
                        (WINDOWS_LAUNCH_CLAIM_ENV, receipt.semaphore_name.as_str()),
                        ("ComSpec", cmd_path.as_ref()),
                    ];
                    let script =
                        wt_powershell_bootstrap(&batches[1].path().to_string_lossy(), &launch_env);
                    let profile_override = if shell == WtDefaultShell::Cmd {
                        ""
                    } else {
                        "function Remove-Item { throw 'profile override' }; "
                    };
                    let encoded = powershell_encoded_command(&format!(
                        "{profile_override}{script}; exit $LASTEXITCODE"
                    ));
                    let bootstrap = format!(
                        "{}{parent_env_check}",
                        wt_cmd_bootstrap_command(&powershell.to_string_lossy(), &encoded)
                    );
                    let profile = WtDefaultProfile {
                        selector: "probe".into(),
                        shell,
                    };
                    let args = profile.build_wt_args(&encoded, &bootstrap);
                    let child_args = expand_in_resident_environment(&wt_child_command_line(&args));
                    let cmdline_units = child_args.encode_utf16().count();
                    let executable = match shell {
                        WtDefaultShell::Cmd => cmd.as_os_str(),
                        WtDefaultShell::PowerShell => powershell.as_os_str(),
                        WtDefaultShell::Pwsh => std::ffi::OsStr::new("pwsh"),
                    };
                    let mut command = Command::new(executable);
                    configure_windows_batch_env(
                        &mut command,
                        &forwarded.path().to_string_lossy(),
                        &[],
                    );
                    let output = command
                        .raw_arg(child_args)
                        .current_dir(&starting)
                        .envs(
                            PARENT_CLAUDE_SESSION_ENV_VARS
                                .iter()
                                .map(|name| (*name, "stale-parent-session")),
                        )
                        .creation_flags(CREATE_NO_WINDOW)
                        .output()
                        .unwrap();
                    let observed = std::fs::read_to_string(&marker).unwrap_or_default();
                    if !output.status.success()
                        || observed.trim() != "B"
                        || decode_command_output(&output.stdout)
                            .contains("cc-switch-parent-env-leak-")
                        || std::fs::read(receipt.path()).unwrap() != [1]
                        || batches[1].path().exists()
                        || !batches[0].path().exists()
                    {
                        failures.push(format!("{shell:?}, stale={stale}: expected B and receipt, got {observed:?}; cmdline_units={cmdline_units}; stdout={}; stderr={}",
                            decode_command_output(&output.stdout), decode_command_output(&output.stderr)));
                    }
                    let _ = std::fs::remove_file(&marker);
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[serial_test::serial]
    fn wt_launcher_preserves_literal_arguments_and_detaches_parent_environment() {
        let names: Vec<_> = PARENT_CLAUDE_SESSION_ENV_VARS
            .iter()
            .chain(WINDOWS_PRIVATE_LAUNCH_ENV_VARS.iter())
            .copied()
            .collect();
        let _restore = EnvRestore(
            names
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect(),
        );
        for name in &names {
            std::env::set_var(name, "stale-parent-session");
        }
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("launcher_probe.rs");
        let code = r#"#![windows_subsystem = "windows"]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let names: &[&str] = &ENV_NAMES;
    let leaked: Vec<_> = names.iter().filter(|name| std::env::var_os(name).is_some()).collect();
    std::fs::write(&args[1], format!("{:?}\n{:?}", &args[2..], leaked)).unwrap();
    std::fs::write(&args[0], [1]).unwrap();
}
"#
        .replace("ENV_NAMES", &format!("{names:?}"));
        std::fs::write(&source, code).unwrap();
        let directory = temp
            .path()
            .join("工具🚀 & %COMSPEC% ^ ! (test) O'Brien ‘ ’ ‚ ‛");
        std::fs::create_dir(&directory).unwrap();
        let executable = directory.join("wt.exe");
        let compiled = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg(source)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let receipt =
            WtLaunchReceipt::new(tempfile::NamedTempFile::new_in(&directory).unwrap()).unwrap();
        let capture = directory.join("arguments.txt");
        let receipt_path = receipt.path().to_string_lossy();
        let capture_path = capture.to_string_lossy();
        let profile = WtDefaultProfile {
            selector: "{22222222-2222-4222-8222-222222222222}".into(),
            shell: WtDefaultShell::Pwsh,
        };
        let args = profile.build_wt_args("encoded-payload", "bootstrap");
        let expected = format!("{args:?}\n[]");
        let mut command = Command::new(executable);
        command
            .args([receipt_path.as_ref(), capture_path.as_ref()])
            .args(args);
        run_wt_command(&mut command, &receipt, Duration::from_secs(5)).unwrap();
        assert_eq!(std::fs::read_to_string(capture).unwrap(), expected);
    }

    #[cfg(target_os = "windows")]
    fn fake_wt_cmd_launcher(directory: &Path) -> PathBuf {
        let source = directory.join("fake_wt.rs");
        std::fs::write(
            &source,
            r#"use std::os::windows::process::CommandExt;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut start = args.iter().position(|argument| argument == "--").unwrap() + 1;
    let explicit_cmd = !args[start].starts_with('/');
    if explicit_cmd {
        assert!(args[start].to_ascii_lowercase().ends_with("cmd.exe"));
        assert!(!args.iter().any(|argument| argument == "--appendCommandLine"));
        start += 1;
    }
    std::fs::write(std::env::current_exe().unwrap().with_extension("mode"),
        if explicit_cmd { "explicit-cmd" } else { "profile" }).unwrap();
    let line = args[start..]
        .iter()
        .map(|argument| match argument.as_str() {
            "/K" => "/C".to_string(),
            argument if argument.contains(' ') => format!("\"{argument}\""),
            argument => argument.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let cmd = std::path::Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32\\cmd.exe");
    let status = std::process::Command::new(cmd).raw_arg(line).status().unwrap();
    std::process::exit(status.code().unwrap_or(1));
}
"#,
        )
        .unwrap();
        let launcher = directory.join("wt.exe");
        let compiled = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg(&source)
            .arg("-o")
            .arg(&launcher)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        launcher
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[serial_test::serial]
    fn wt_unsupported_default_profile_opens_an_explicit_cmd_tab() {
        let temp = tempfile::tempdir().unwrap();
        let launcher = fake_wt_cmd_launcher(temp.path());
        // A versioned system PE supplies the version probe for this fake WT.
        std::fs::copy(
            super::super::windows_console::system_cmd_path().unwrap(),
            temp.path().join("WindowsTerminal.exe"),
        )
        .unwrap();
        std::fs::write(temp.path().join(".portable"), "").unwrap();
        let settings_dir = temp.path().join("settings");
        std::fs::create_dir(&settings_dir).unwrap();
        let project = temp
            .path()
            .join("项目 O'Brien ‘ ’ & %CC_SWITCH_TEST_PROFILE% !");
        std::fs::create_dir(&project).unwrap();
        let path = std::env::var_os("PATH");
        let _restore = EnvRestore(vec![("PATH", path.clone())]);
        std::env::set_var(
            "PATH",
            std::env::join_paths(
                std::iter::once(temp.path().to_path_buf())
                    .chain(std::env::split_paths(&path.unwrap_or_default())),
            )
            .unwrap(),
        );
        let effective = super::super::windows_console::system_cmd_path().unwrap();
        let effective = effective.parent().unwrap().to_string_lossy();
        for (index, settings) in [
            r#"{"defaultProfile":"{11111111-1111-4111-8111-111111111111}","profiles":{"list":[{"guid":"{11111111-1111-4111-8111-111111111111}","source":"Windows.Terminal.Wsl"}]}}"#,
            r#"{"defaultProfile":"Git Bash","profiles":{"list":[{"guid":"{11111111-1111-4111-8111-111111111111}","name":"Git Bash","commandline":"bash.exe"}]}}"#,
            r#"{"defaultProfile":"{11111111-1111-4111-8111-111111111111}","profiles":{"list":[{"guid":"{11111111-1111-4111-8111-111111111111}","commandline":"powershell.exe -NoExit -Command Enter-VsDevShell"}]}}"#,
            r#"{"defaultProfile":"{11111111-1111-4111-8111-111111111111}","profiles":{"list":[{"guid":"{11111111-1111-4111-8111-111111111111}","commandline":"pwsh.exe -WorkingDirectory ~"}]}}"#,
            r#"{"defaultProfile":"{61c54bbd-c2c6-5271-96e7-009a87ff44bf}","profiles":{"defaults":{"commandline":"cmd.exe"}}}"#,
        ]
        .iter()
        .enumerate()
        {
            std::fs::write(settings_dir.join("settings.json"), settings).unwrap();
            let batch = temp.path().join(format!("provider-{index}.bat"));
            let marker = project.join(format!("配置结果-{index}.txt"));
            std::fs::write(&batch, "@echo off\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\n> \"%CC_SWITCH_INTERNAL_CONFIG_PATH%\" echo ok\r\n").unwrap();
            launch_wt_terminal_with_env(
                &batch.to_string_lossy(),
                &[
                    (WINDOWS_CONFIG_PATH_ENV, marker.to_string_lossy().as_ref()),
                    (WINDOWS_CWD_ENV, project.to_string_lossy().as_ref()),
                ],
                &effective,
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while batch.exists() || !marker.exists() {
                assert!(Instant::now() < deadline, "fallback CMD must run and clean its batch");
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(std::fs::read_to_string(marker).unwrap().trim(), "ok");
            assert_eq!(std::fs::read_to_string(launcher.with_extension("mode")).unwrap(), "explicit-cmd");
        }
    }

    // Model WT's CMD profile: the tab inherits wt.exe's environment and runs the
    // appended command. A typical user PATH must not push CMD past 8191 characters.
    #[cfg(target_os = "windows")]
    #[test]
    #[serial_test::serial]
    fn wt_cmd_profile_tab_inherits_a_long_restored_path() {
        let temp = tempfile::tempdir().unwrap();
        let launcher = fake_wt_cmd_launcher(temp.path());
        let project = temp.path().join("项目");
        std::fs::create_dir(&project).unwrap();
        let marker = project.join("配置结果.txt");
        let batch = temp.path().join("provider.bat");
        std::fs::write(
            &batch,
            "@echo off\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\n> \"%CC_SWITCH_INTERNAL_CONFIG_PATH%\" echo %PATH%\r\n",
        )
        .unwrap();
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let restored = std::iter::once(system.to_string_lossy().into_owned())
            .chain((0..120).map(|index| format!(r"C:\cc-switch-restored-path\tool-{index:03}")))
            .collect::<Vec<_>>()
            .join(";");
        assert!(restored.len() > 3000);
        let profile = WtDefaultProfile {
            selector: "{22222222-2222-4222-8222-222222222222}".into(),
            shell: WtDefaultShell::Cmd,
        };
        let marker_path = marker.to_string_lossy();
        let project_path = project.to_string_lossy();
        launch_wt_tab(
            &launcher,
            Some(&profile),
            &batch.to_string_lossy(),
            &[
                (WINDOWS_CONFIG_PATH_ENV, marker_path.as_ref()),
                (WINDOWS_CWD_ENV, project_path.as_ref()),
            ],
            &restored,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while batch.exists() || !marker.exists() {
            assert!(
                Instant::now() < deadline,
                "the tab must see the restored PATH, run the batch and delete it"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), restored);
    }

    // Only an isolated portable copy may opt into desktop validation. The caller
    // owns and removes its WT process tree; never point this at a live user install.
    #[cfg(target_os = "windows")]
    #[test]
    #[serial_test::serial]
    #[ignore = "requires CC_SWITCH_WT_TEST_EXE pointing to an isolated portable WT copy in temp"]
    fn wt_resident_launches_each_unique_batch() {
        let launcher =
            PathBuf::from(std::env::var_os("CC_SWITCH_WT_TEST_EXE").expect("isolated WT path"));
        assert!(launcher.starts_with(std::env::temp_dir()));
        let directory = launcher.parent().unwrap();
        assert!(directory.join(".portable").is_file());
        let settings_directory = directory.join("settings");
        let settings = settings_directory.join("settings.json");
        let expected_settings = settings_directory.to_string_lossy();
        let names: Vec<_> = [
            "PATH",
            "CC_SWITCH_CMD_EXPAND",
            "CC_SWITCH_BANG",
            "WT_SETTINGS_DIR",
            "WT_PROFILE_ID",
            "WT_SESSION",
            "CC_SWITCH_WT_TEST_PROFILE_NONCE",
            "CC_SWITCH_WT_TEST_SETTINGS_DIRECTORY",
        ]
        .into_iter()
        .chain(PARENT_CLAUDE_SESSION_ENV_VARS.iter().copied())
        .chain(WINDOWS_PRIVATE_LAUNCH_ENV_VARS.iter().copied())
        .collect();
        let _restore = EnvRestore(
            names
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect(),
        );
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            std::env::join_paths(
                std::iter::once(directory.to_path_buf()).chain(std::env::split_paths(&path)),
            )
            .unwrap(),
        );
        std::env::set_var("CC_SWITCH_CMD_EXPAND", "wrong");
        std::env::set_var("CC_SWITCH_BANG", "wrong");
        for name in [
            "WT_SETTINGS_DIR",
            "WT_PROFILE_ID",
            "WT_SESSION",
            "CC_SWITCH_WT_TEST_PROFILE_NONCE",
            "CC_SWITCH_WT_TEST_SETTINGS_DIRECTORY",
        ] {
            std::env::remove_var(name);
        }
        for name in PARENT_CLAUDE_SESSION_ENV_VARS {
            std::env::set_var(name, "stale-parent-session");
        }
        let temp = tempfile::tempdir().unwrap();
        let hostile = temp
            .path()
            .join("项目 space & %CC_SWITCH_CMD_EXPAND% ^ !CC_SWITCH_BANG! (test) O'Brien ‘ ’ ‚ ‛");
        std::fs::create_dir(&hostile).unwrap();
        let marker = temp.path().join("executed.txt");
        let powershell = super::super::windows_console::system_powershell_path().unwrap();
        let cmd = super::super::windows_console::system_cmd_path().unwrap();
        // Custom GUIDs and profile-only values rule out built-in fallback.
        // A prewarmed resident must load the same fixture nonce before this test.
        let nonce = std::env::var("CC_SWITCH_WT_TEST_EXPECTED_NONCE")
            .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
        assert!(
            uuid::Uuid::parse_str(&nonce).is_ok(),
            "fixture nonce must be a UUID"
        );
        let mut profiles = serde_json::json!([
            {"guid":"{11111111-1111-4111-8111-111111111111}","name":"WinPS alternate","commandline":format!("\"{}\" -NoProfile", powershell.display())},
            {"guid":"{22222222-2222-4222-8222-222222222222}","name":"CMD","commandline":cmd},
            {"guid":"{33333333-3333-4333-8333-333333333333}","name":"WinPS default","commandline":format!("\"{}\" -NoProfile", powershell.display())},
            {"guid":"{44444444-4444-4444-8444-444444444444}","name":"PS7","commandline":"pwsh.exe -NoProfile"}
        ]);
        for profile in profiles.as_array_mut().unwrap() {
            profile["environment"] = serde_json::json!({
                "CC_SWITCH_WT_TEST_PROFILE_NONCE":nonce,
                "CC_SWITCH_WT_TEST_SETTINGS_DIRECTORY":expected_settings,
            });
        }
        let wait_for_marker = || {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if let Ok(value) = std::fs::read_to_string(&marker) {
                    if value.lines().last() == Some("done") {
                        return value;
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "WT must execute the selected batch"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        // The BAT quotes this path; only percent expansion still applies there.
        let marker_path = marker.to_string_lossy().replace('%', "%%");
        let env_probe: String = PARENT_CLAUDE_SESSION_ENV_VARS
            .iter()
            .map(|name| format!("if defined {name} >> \"{marker_path}\" echo leaked={name}\r\n"))
            .collect();
        for guid in [
            "{33333333-3333-4333-8333-333333333333}",
            "{22222222-2222-4222-8222-222222222222}",
            "{44444444-4444-4444-8444-444444444444}",
        ] {
            std::fs::write(
                &settings,
                serde_json::json!({"defaultProfile":guid,"profiles":{"list":profiles}}).to_string(),
            )
            .unwrap();
            for label in ["A", "B"] {
                let cwd = hostile.join(label);
                std::fs::create_dir_all(&cwd).unwrap();
                let config = hostile.join(format!("config-{label}.json"));
                let batch = hostile.join(format!("provider-{label}.bat"));
                std::fs::write(&config, format!("{label}\r\n")).unwrap();
                // Do not cd in the BAT: startingDirectory='.' must use this
                // invocation's cwd, including literal percent and Unicode paths.
                // A resident may omit WT_SETTINGS_DIR; its profile-only nonce
                // still proves that it applied this fixture's settings.
                std::fs::write(&batch, format!("@echo off\r\nsetlocal DisableDelayedExpansion\r\nif defined WT_SETTINGS_DIR if /i not \"%WT_SETTINGS_DIR%\"==\"%CC_SWITCH_WT_TEST_SETTINGS_DIRECTORY%\" exit /b 9\r\n> \"{marker_path}\" echo profile=%WT_PROFILE_ID%\r\n>> \"{marker_path}\" type \"%CC_SWITCH_INTERNAL_CONFIG_PATH%\"\r\n>> \"{marker_path}\" echo nonce=%CC_SWITCH_WT_TEST_PROFILE_NONCE%\r\n{env_probe}> cwd-{label}.txt echo ok\r\n>> \"{marker_path}\" echo done\r\n")).unwrap();
                std::env::set_var(WINDOWS_BATCH_PATH_ENV, "missing-stale-batch");
                std::env::set_var(WINDOWS_CONFIG_PATH_ENV, "stale-config");
                std::env::set_var(WINDOWS_CWD_ENV, "stale-cwd");
                let config_path = config.to_string_lossy();
                let cwd_path = cwd.to_string_lossy();
                launch_wt_terminal_with_env(
                    &batch.to_string_lossy(),
                    &[
                        (WINDOWS_CONFIG_PATH_ENV, config_path.as_ref()),
                        (WINDOWS_CWD_ENV, cwd_path.as_ref()),
                    ],
                    &std::env::var("PATH").unwrap(),
                )
                .unwrap();
                let observed = wait_for_marker();
                assert!(observed.contains(&format!("profile={guid}")), "{observed}");
                assert_eq!(observed.lines().nth(1), Some(label), "{observed}");
                assert_eq!(
                    observed.lines().nth(2),
                    Some(format!("nonce={nonce}").as_str()),
                    "{observed}"
                );
                assert!(!observed.contains("leaked="), "{observed}");
                let cwd_marker = cwd.join(format!("cwd-{label}.txt"));
                assert_eq!(std::fs::read_to_string(&cwd_marker).unwrap().trim(), "ok");
                std::fs::remove_file(cwd_marker).unwrap();
                std::fs::remove_file(&marker).unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                while batch.exists() {
                    assert!(Instant::now() < deadline, "caller must delete the batch");
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }
}
