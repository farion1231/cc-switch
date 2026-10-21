//! Windows terminals for provider and command launches.
//!
//! Every launch writes its own ASCII batch. A CMD console runs it directly. A
//! PowerShell console or a Windows Terminal tab binds the launch's values in an
//! `-EncodedCommand` payload, which runs the batch in a helper CMD.

use super::misc::{
    effective_path_string, is_windows_app_execution_alias_dir, powershell_encoded_command,
    unique_temp_path, ProviderLaunch,
};
use super::windows_console::{spawn_new_console, system_cmd_path, system_powershell_path};
use std::ffi::OsString;
use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const BATCH_PATH_ENV: &str = "CC_SWITCH_INTERNAL_BATCH_PATH";
const CWD_ENV: &str = "CC_SWITCH_INTERNAL_CWD";
/// Carry a launch's arguments, cleanup paths and non-ASCII variable values.
const VALUE_ENV_VARS: [&str; 6] = [
    "CC_SWITCH_INTERNAL_VALUE_0",
    "CC_SWITCH_INTERNAL_VALUE_1",
    "CC_SWITCH_INTERNAL_VALUE_2",
    "CC_SWITCH_INTERNAL_VALUE_3",
    "CC_SWITCH_INTERNAL_VALUE_4",
    "CC_SWITCH_INTERNAL_VALUE_5",
];

fn private_env_vars() -> impl Iterator<Item = &'static str> {
    [BATCH_PATH_ENV, CWD_ENV].into_iter().chain(VALUE_ENV_VARS)
}

/// Terminals opened by CC Switch are top-level sessions. They must not inherit
/// the parent Claude session identity, IPC endpoints, or color-disable flags.
/// Only runtime metadata is stripped; auth, proxy, PATH, and user Claude config stay.
const PARENT_CLAUDE_SESSION_ENV_VARS: [&str; 11] = [
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_SSE_PORT",
    "CLAUDE_PID",
    // Claude Code injects these into tool subprocesses so transcripts stay ANSI-free.
    // A launched terminal is a real TTY; inheriting them greys out the new Claude TUI.
    "NO_COLOR",
    "NODE_DISABLE_COLORS",
];

/// Runs the batch whose quoted path is in `BATCH_PATH_ENV`, then deletes it.
/// CMD expands both references before the batch clears the variable.
pub(super) const RUN_AND_DELETE_BATCH: &str =
    "%CC_SWITCH_INTERNAL_BATCH_PATH% & del %CC_SWITCH_INTERNAL_BATCH_PATH% >nul 2>&1";

/// CMD ends a quoted word at `"` and a line at CR or LF, so no value carries them.
fn check_batch_value(value: &str, what: &str) -> Result<(), String> {
    if value.contains(|c: char| c == '"' || c.is_control()) {
        return Err(format!("{what} 含有引号或控制字符，无法交给 cmd"));
    }
    Ok(())
}

/// Returns the variable that carries `value`, sharing it with an equal value.
fn carry(values: &mut Vec<OsString>, value: OsString) -> Result<&'static str, String> {
    let index = match values.iter().position(|carried| *carried == value) {
        Some(index) => index,
        None => {
            values.push(value);
            values.len() - 1
        }
    };
    VALUE_ENV_VARS
        .get(index)
        .copied()
        .ok_or_else(|| "启动参数过多".to_string())
}

/// `echo` text: CMD would otherwise act on these characters.
fn escape_batch_echo(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '%' => escaped.push_str("%%"),
            '^' | '&' | '|' | '<' | '>' | '(' | ')' => {
                escaped.push('^');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }
    escaped
}

/// The batch for `launch`, and the values it reads from `VALUE_ENV_VARS`.
///
/// cmd.exe decodes a batch with the console code page, so non-ASCII text never
/// enters these bytes (#8034): arguments, cleanup paths and non-ASCII variable
/// values arrive in environment variables. ASCII variable values, such as API
/// keys, stay in the batch rather than on a command line. They are set inside
/// SETLOCAL, so the shell left after the program exits does not keep them;
/// SETLOCAL follows the directory change, which ENDLOCAL would otherwise undo.
/// The program runs in a child CMD so that npm's `.cmd` shims return here; the
/// doubled percent signs defer each argument to that child without CALL's
/// second expansion. The batch runs to its normal EOF, because aborting it with
/// `(goto)` skips CMD's title restore; the caller deletes it afterwards. A
/// failed directory change still deletes the cleanup files, which may hold keys.
fn provider_batch(launch: &ProviderLaunch, values: &mut Vec<OsString>) -> Result<String, String> {
    // The program and variable names are written into the batch as they are.
    let plain = |text: &str, extra: &[u8]| {
        !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || extra.contains(&b))
    };
    if !plain(launch.program, b"-.") {
        return Err(format!("无效的程序名: {}", launch.program));
    }
    let mut command = format!("\"%ComSpec%\" /D /E:ON /V:OFF /C {}", launch.program);
    for argument in &launch.args {
        check_batch_value(argument, "启动参数")?;
        if argument.is_empty() {
            command.push_str(" \"\"");
        } else {
            let name = carry(values, argument.into())?;
            command.push_str(&format!(" \"%%{name}%%\""));
        }
    }
    let mut set_lines = String::new();
    for (name, value) in &launch.env {
        if !plain(name, b"") {
            return Err(format!("无效的环境变量名: {name}"));
        }
        match value {
            None => set_lines.push_str(&format!("set \"{name}=\"\r\n")),
            Some(value) => {
                check_batch_value(value, &format!("环境变量 {name}"))?;
                let value = if value.is_ascii() {
                    value.replace('%', "%%")
                } else {
                    format!("%{}%", carry(values, value.into())?)
                };
                set_lines.push_str(&format!("set \"{name}={value}\"\r\n"));
            }
        }
    }
    let mut cleanup = String::new();
    for file in &launch.cleanup {
        let name = carry(values, file.clone().into_os_string())?;
        cleanup.push_str(&format!("del \"%{name}%\" >nul 2>&1\r\n"));
    }
    let carriers = &VALUE_ENV_VARS[..values.len()];
    let checks: String = carriers
        .iter()
        .map(|name| format!("if not defined {name} exit /b 1\r\n"))
        .collect();
    let clear: String = carriers
        .iter()
        .map(|name| format!("set \"{name}=\"\r\n"))
        .collect();
    let title = escape_batch_echo(launch.title);
    Ok(format!(
        "@echo off\r\n\
set \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\n\
{checks}\
if not defined CC_SWITCH_INTERNAL_CWD goto cc_switch_run\r\n\
if \"%CC_SWITCH_INTERNAL_CWD:~0,2%\"==\"\\\\\" goto cc_switch_unc\r\n\
cd /d \"%CC_SWITCH_INTERNAL_CWD%\" || goto cc_switch_cwd_failed\r\n\
goto cc_switch_run\r\n\
:cc_switch_unc\r\n\
pushd \"%CC_SWITCH_INTERNAL_CWD%\" || goto cc_switch_cwd_failed\r\n\
goto cc_switch_run\r\n\
:cc_switch_cwd_failed\r\n\
{cleanup}{clear}\
set \"CC_SWITCH_INTERNAL_CWD=\"\r\n\
exit /b 1\r\n\
:cc_switch_run\r\n\
set \"CC_SWITCH_INTERNAL_CWD=\"\r\n\
echo {title}\r\n\
setlocal\r\n\
{set_lines}\
{command}\r\n\
endlocal\r\n\
{cleanup}{clear}"
    ))
}

fn command_batch(label: &str, command_line: &str) -> String {
    format!(
        "@echo off\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\necho [cc-switch] Starting: {label}\r\necho.\r\n{command_line}\r\necho.\r\necho [cc-switch] Command exited. Press any key to close.\r\npause >nul\r\n"
    )
}

/// One launch: its batch, plus the values the batch reads from its environment.
pub(super) struct BatchLaunch<'a> {
    pub(super) batch: &'a Path,
    pub(super) cwd: Option<&'a Path>,
    /// Carried in `VALUE_ENV_VARS`, in order.
    pub(super) values: Vec<OsString>,
}

impl BatchLaunch<'_> {
    /// The batch path is quoted, so `RUN_AND_DELETE_BATCH` survives spaces and `&`.
    pub(super) fn env(&self) -> Vec<(&'static str, OsString)> {
        let mut batch = OsString::from("\"");
        batch.push(self.batch);
        batch.push("\"");
        std::iter::once((BATCH_PATH_ENV, batch))
            .chain(self.cwd.map(|cwd| (CWD_ENV, cwd.into())))
            .chain(VALUE_ENV_VARS.into_iter().zip(self.values.iter().cloned()))
            .collect()
    }

    /// The directory an interactive shell may start in. CMD cannot use a UNC
    /// current directory, so the batch enters those with `pushd` instead.
    fn drive_cwd(&self) -> Option<&Path> {
        self.cwd
            .filter(|cwd| !cwd.as_os_str().to_string_lossy().starts_with(r"\\"))
    }
}

/// Creates a new batch in the temp directory; a file that already exists there
/// is left alone.
fn write_batch(stem: &str, content: &str) -> Result<PathBuf, String> {
    let batch = unique_temp_path(stem, "bat");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&batch)
        .map_err(|e| format!("写入批处理文件失败: {e}"))?;
    if let Err(error) = file.write_all(content.as_bytes()) {
        drop(file);
        let _ = std::fs::remove_file(&batch);
        return Err(format!("写入批处理文件失败: {error}"));
    }
    Ok(batch)
}

pub(super) fn launch_provider_terminal(
    launch: &ProviderLaunch,
    cwd: Option<&Path>,
) -> Result<(), String> {
    let mut values = Vec::new();
    let content = provider_batch(launch, &mut values)?;
    let batch = write_batch("cc_switch_launcher", &content)?;
    launch_batch(&BatchLaunch {
        batch: &batch,
        cwd,
        values,
    })
}

pub(super) fn launch_command_terminal(label: &str, command_line: &str) -> Result<(), String> {
    let batch = write_batch(
        &format!("cc_switch_{label}"),
        &command_batch(label, command_line),
    )?;
    launch_batch(&BatchLaunch {
        batch: &batch,
        cwd: None,
        values: Vec::new(),
    })
}

/// Opens the preferred terminal and, like upstream, falls back to a new CMD
/// console when it fails. A launched terminal deletes the batch after it returns.
fn launch_batch(launch: &BatchLaunch) -> Result<(), String> {
    let path = effective_path_string();
    let preferred = crate::settings::get_preferred_terminal();
    let terminal = preferred.as_deref().unwrap_or("cmd");
    let result = match terminal {
        "powershell" => spawn_powershell_console(launch, &path)
            .map_err(|e| format!("启动 PowerShell 失败: {e}")),
        "wt" => launch_wt_tab(launch, &path),
        _ => spawn_cmd_console(launch, &path),
    };
    let result = match result {
        Err(error) if terminal != "cmd" => {
            log::warn!("首选终端 {terminal} 启动失败，回退到 cmd: {error}");
            spawn_cmd_console(launch, &path)
        }
        result => result,
    };
    if result.is_err() {
        let _ = std::fs::remove_file(launch.batch);
    }
    result
}

fn inherited_env_to_clear() -> impl Iterator<Item = &'static str> {
    PARENT_CLAUDE_SESSION_ENV_VARS
        .into_iter()
        .chain(private_env_vars())
}

/// The terminal finds `claude` on the restored PATH: after an MSI restart, or when
/// the CLI was installed after CC Switch started, the process PATH lacks the user's
/// entries (#6061, #3877, #8446). Only the launched terminal sees this PATH.
pub(super) fn configure_shell_env(command: &mut Command, path: &str) {
    command.env("PATH", path);
    for name in inherited_env_to_clear() {
        command.env_remove(name);
    }
}

fn spawn_cmd_console(launch: &BatchLaunch, path: &str) -> Result<(), String> {
    let mut command = Command::new("cmd");
    configure_shell_env(&mut command, path);
    // No /D: the interactive CMD left after Claude exits keeps the user's AutoRun.
    command
        .args(["/E:ON", "/V:OFF", "/K", RUN_AND_DELETE_BATCH])
        .envs(launch.env());
    spawn_new_console(&mut command).map_err(|e| format!("启动 cmd 失败: {e}"))
}

fn spawn_powershell_console(launch: &BatchLaunch, path: &str) -> io::Result<()> {
    let payload = powershell_encoded_command(&powershell_payload(launch)?);
    let mut command = Command::new("powershell");
    configure_shell_env(&mut command, path);
    command.args(["-NoExit", "-EncodedCommand", &payload]);
    if let Some(cwd) = launch.drive_cwd() {
        command.current_dir(cwd);
    }
    spawn_new_console(&mut command)
}

/// PowerShell also treats U+2018..U+201B as single-quote delimiters.
fn powershell_literal(value: &str) -> String {
    let mut literal = String::with_capacity(value.len() + 2);
    literal.push('\'');
    for c in value.chars() {
        if matches!(c, '\'' | '\u{2018}'..='\u{201b}') {
            literal.push(c);
        }
        literal.push(c);
    }
    literal.push('\'');
    literal
}

// A profile can shadow cmdlets such as Remove-Item, but not .NET methods.
fn powershell_clear_env(names: impl Iterator<Item = &'static str>) -> String {
    let names: Vec<_> = names.map(|name| format!("'{name}'")).collect();
    format!(
        "foreach($n in {}){{[Environment]::SetEnvironmentVariable($n,$null)}}",
        names.join(",")
    )
}

/// Binds this launch's values inside the payload. WT expands `%VAR%` in its
/// commandline with the resident WindowsTerminal's environment (#7473), and a
/// tab may inherit that environment from an earlier launch. Base64 contains no
/// `%`, and no `;` for WT to split commands at.
pub(super) fn powershell_payload(launch: &BatchLaunch) -> io::Result<String> {
    let cmd = powershell_literal(&system_cmd_path()?.to_string_lossy());
    let bind: String = launch
        .env()
        .into_iter()
        .map(|(name, value)| {
            format!(
                "$env:{name}={}; ",
                powershell_literal(&value.to_string_lossy())
            )
        })
        .collect();
    Ok(format!(
        "& {{ $ErrorActionPreference='Stop'; {}; {bind}try {{ & {cmd} /D /E:ON /V:OFF /C '{RUN_AND_DELETE_BATCH}' }} finally {{ {} }} }}",
        powershell_clear_env(inherited_env_to_clear()),
        powershell_clear_env(private_env_vars()),
    ))
}

/// CMD has no encoded-command form, so a CMD tab runs the payload in a helper
/// PowerShell. The interactive CMD clears the inherited variables itself, as a
/// child cannot change its parent's environment.
fn wt_cmd_bootstrap(payload: &str) -> io::Result<String> {
    let clear: String = inherited_env_to_clear()
        .map(|name| format!("set \"{name}=\" & "))
        .collect();
    // One argument under CMD /S keeps the quotes around the absolute path.
    let bootstrap = format!(
        "{clear}\"{}\" -NoLogo -NoProfile -NonInteractive -EncodedCommand {payload}",
        system_powershell_path()?.display()
    );
    if bootstrap.encode_utf16().count() >= 8191 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "命令行超出 CMD 的 8191 字符上限",
        ));
    }
    Ok(bootstrap)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WtDefaultShell {
    Cmd,
    PowerShell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WtDefaultProfile {
    /// The profile's braced GUID. A commandline alone could select another profile.
    selector: String,
    shell: WtDefaultShell,
}

const POWERSHELL_CORE_PROFILE_SOURCE: &str = "Windows.Terminal.PowershellCore";
const POWERSHELL_CORE_PROFILE_GUID: &str = "574e775e-4f2a-5b96-ac1e-a2962a402336";

fn known_wt_shell(guid: uuid::Uuid) -> Option<WtDefaultShell> {
    match guid.to_string().as_str() {
        POWERSHELL_CORE_PROFILE_GUID | "61c54bbd-c2c6-5271-96e7-009a87ff44bf" => {
            Some(WtDefaultShell::PowerShell)
        }
        "0caa0dad-35be-5f56-a8ff-afceeeaa6101" => Some(WtDefaultShell::Cmd),
        _ => None,
    }
}

// Recognize only executable-only profiles and simple flag-only options. WT
// retains their original commandline; we do not emulate host option parsing.
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
        "powershell" | "pwsh" => WtDefaultShell::PowerShell,
        _ => return None,
    };
    let supported_options: &[&str] = match shell {
        WtDefaultShell::Cmd => &["/d", "/q", "/s", "/v:off"],
        WtDefaultShell::PowerShell => &["-nologo", "-noprofile", "-noexit"],
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

/// The default profile, if CC Switch can append to it. `None` means the caller
/// should open an explicit CMD tab.
fn parse_wt_default_profile(content: &str) -> Option<WtDefaultProfile> {
    let settings: serde_json::Value = json5::from_str(content).ok()?;
    let selected = settings.get("defaultProfile")?.as_str()?;
    // WT treats only a 38-character braced value as a GUID; anything else is a name.
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
                WtDefaultShell::PowerShell
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

fn wt_new_tab_args(
    profile: Option<&WtDefaultProfile>,
    launch: &BatchLaunch,
) -> io::Result<Vec<String>> {
    let payload = powershell_encoded_command(&powershell_payload(launch)?);
    let mut args = vec!["new-tab".to_string()];
    if launch.drive_cwd().is_some() {
        // WT resolves `.` against wt.exe's cwd. A literal path would go through the
        // resident's %VAR% expansion and WT's `;` command splitting.
        args.extend(["--startingDirectory".into(), ".".into()]);
    }
    let shell = match profile {
        Some(profile) => {
            args.extend([
                "--profile".into(),
                profile.selector.clone(),
                "--appendCommandLine".into(),
                "--".into(),
            ]);
            profile.shell
        }
        None => {
            // Replace an unsupported profile's commandline, rather than append CMD
            // arguments to WSL, Git Bash or another unrelated shell.
            args.extend([
                "--".into(),
                system_cmd_path()?.to_string_lossy().into_owned(),
            ]);
            WtDefaultShell::Cmd
        }
    };
    match shell {
        WtDefaultShell::Cmd => {
            let bootstrap = wt_cmd_bootstrap(&payload)?;
            args.extend(["/E:ON", "/V:OFF", "/S", "/K"].map(String::from));
            args.push(bootstrap);
        }
        WtDefaultShell::PowerShell => {
            args.extend(["-NoExit".into(), "-EncodedCommand".into(), payload]);
        }
    }
    Ok(args)
}

const IO_REPARSE_TAG_APPEXECLINK: u32 = 0x8000_001b;

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

struct WtInstallation {
    /// What CC Switch runs: `wt.exe` on PATH, possibly an app alias or a Scoop shim.
    launcher: PathBuf,
    /// The real `wt.exe`, whose location identifies the installation.
    executable: PathBuf,
}

fn resolve_wt_installation(path: &str) -> Option<WtInstallation> {
    let launcher = std::env::split_paths(path)
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
    Some(WtInstallation {
        launcher,
        executable,
    })
}

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

/// Opens a tab with the user's default profile, or an explicit CMD tab when CC
/// Switch cannot append to that profile. Errors mean that no tab will open.
fn launch_wt_tab(launch: &BatchLaunch, path: &str) -> Result<(), String> {
    let wt = resolve_wt_installation(path).ok_or("未找到 Windows Terminal")?;
    let module = wt.executable.with_file_name("WindowsTerminal.exe");
    let module = if module.is_file() {
        module
    } else {
        wt.executable.clone()
    };
    if wt_file_version(&module).is_none_or(|version| version < (1, 19)) {
        return Err("--appendCommandLine 需要 Windows Terminal 1.19 或更新版本".into());
    }
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs::data_local_dir);
    let settings = wt_settings_path(&wt.executable, local.as_deref())
        .ok_or("无法定位 Windows Terminal 的设置文件")?;
    let content = std::fs::read_to_string(&settings)
        .map_err(|e| format!("读取 {} 失败: {e}", settings.display()))?;
    let profile = parse_wt_default_profile(&content);
    if profile.is_none() {
        log::warn!("Windows Terminal 默认 profile 不受支持，改用显式 cmd 标签");
    }
    let args = wt_new_tab_args(profile.as_ref(), launch)
        .map_err(|e| format!("生成 Windows Terminal 参数失败: {e}"))?;
    let mut command = Command::new(&wt.launcher);
    // As with upstream's `start wt`, a tab that inherits wt.exe's environment gets this PATH.
    configure_shell_env(&mut command, path);
    if let Some(cwd) = launch.drive_cwd() {
        command.current_dir(cwd);
    }
    let child = command
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("启动 Windows Terminal 失败: {e}"))?;
    wait_for_wt_handoff(child)
}

/// wt.exe hands the request to WindowsTerminal.exe and normally exits at once.
/// Only an early non-zero exit proves that no tab opens. A launcher that is still
/// running may yet open the tab, so it counts as launched: falling back then
/// could run the batch twice.
fn wait_for_wt_handoff(mut child: Child) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                return Err(format!("Windows Terminal 启动失败: {status}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            _ => return Ok(()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::misc::decode_command_output;
    use super::*;
    use std::process::{Output, Stdio};

    struct EnvRestore(Vec<(String, Option<OsString>)>);

    impl EnvRestore {
        fn capture<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
            Self(
                names
                    .into_iter()
                    .map(|name| (name.to_string(), std::env::var_os(name)))
                    .collect(),
            )
        }
    }

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

    fn wait_until(what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn compile(source: &str, executable: &Path) {
        let source_file = executable.with_extension("rs");
        std::fs::write(&source_file, source).unwrap();
        let compiled = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg(&source_file)
            .arg("-o")
            .arg(executable)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
    }

    fn prepend_path(directory: &Path) -> String {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::join_paths(
            std::iter::once(directory.to_path_buf()).chain(std::env::split_paths(&path)),
        )
        .unwrap()
        .into_string()
        .unwrap()
    }

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
        assert_eq!(
            parse_wt_default_profile(content),
            Some(WtDefaultProfile {
                selector: "{22222222-2222-4222-8222-222222222222}".into(),
                shell: WtDefaultShell::PowerShell,
            })
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
                WtDefaultShell::PowerShell,
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
        for (guid, shell) in [
            (
                "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
                WtDefaultShell::PowerShell,
            ),
            (
                "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}",
                WtDefaultShell::Cmd,
            ),
        ] {
            let builtin = format!(r#"{{"defaultProfile":"{guid}"}}"#);
            assert_eq!(parse_wt_default_profile(&builtin).unwrap().shell, shell);
        }
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
            r#"{"defaultProfile":"{11111111-1111-4111-8111-111111111111}","profiles":{"list":[{"guid":"{11111111-1111-4111-8111-111111111111}","source":"Windows.Terminal.Wsl"}]}}"#,
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

    /// A stand-in `claude`: copies its settings, marks its cwd and exits with
    /// `CC_SWITCH_TEST_EXIT_CODE`; `--capture-parent-cwd` records the caller's cwd.
    const CLAUDE_PROBE: &str = r#"#[link(name = "kernel32")]
extern "system" {
    fn SetConsoleTitleW(title: *const u16) -> i32;
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().and_then(|value| value.to_str()) == Some("--capture-parent-cwd") {
        std::fs::write(
            std::env::var_os("CC_SWITCH_TEST_PARENT_MARKER").unwrap(),
            std::env::current_dir().unwrap().to_str().unwrap(),
        )
        .unwrap();
        return;
    }
    assert_eq!(args.first().and_then(|value| value.to_str()), Some("--settings"));
    let config = args.get(1).expect("missing settings path");
    std::fs::copy(config, std::env::var_os("CC_SWITCH_TEST_CONFIG_COPY").unwrap()).unwrap();
    std::fs::write(
        std::env::current_dir().unwrap().join("cc-switch-provider-marker.txt"),
        "ok\n",
    )
    .unwrap();
    if std::env::var_os("CC_SWITCH_TEST_CLEAR_TITLE").is_some() {
        assert_ne!(unsafe { SetConsoleTitleW([0u16].as_ptr()) }, 0);
    }
    let exit_code: i32 = std::env::var("CC_SWITCH_TEST_EXIT_CODE").unwrap().parse().unwrap();
    if exit_code != 0 {
        eprintln!("cc-switch-test-provider-error");
    }
    std::process::exit(exit_code);
}
"#;

    /// Directories holding `claude.exe`, and npm's `claude.cmd` shim around it.
    struct ClaudeProbes {
        _root: tempfile::TempDir,
        exe: PathBuf,
        cmd_shim: PathBuf,
    }

    impl ClaudeProbes {
        fn compile() -> Self {
            let root = tempfile::tempdir().unwrap();
            let exe = root.path().join("exe");
            let cmd_shim = root.path().join("cmd-shim");
            std::fs::create_dir(&exe).unwrap();
            std::fs::create_dir(&cmd_shim).unwrap();
            compile(CLAUDE_PROBE, &exe.join("claude.exe"));
            std::fs::copy(exe.join("claude.exe"), cmd_shim.join("claude_probe.exe")).unwrap();
            std::fs::write(
                cmd_shim.join("claude.cmd"),
                "@echo off\r\n\"%~dp0claude_probe.exe\" %*\r\n",
            )
            .unwrap();
            Self {
                _root: root,
                exe,
                cmd_shim,
            }
        }

        fn bins(&self) -> [&Path; 2] {
            [&self.exe, &self.cmd_shim]
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Runner {
        Cmd,
        CmdKeepOpen,
        CmdKeepOpenEmptyTitle,
        PowerShell,
    }

    /// Runs `launch` as its terminal would, with a hidden console. A /K CMD then
    /// records its own cwd and exits; PowerShell records that it resumed.
    fn run_batch(
        runner: Runner,
        launch: &BatchLaunch,
        bin: &Path,
        extra: &[(&str, &OsString)],
    ) -> Output {
        let mut command = match runner {
            Runner::PowerShell => {
                let script = format!(
                    "{}; $code = $LASTEXITCODE; [IO.File]::WriteAllText($env:CC_SWITCH_TEST_PARENT_MARKER, 'parent-resumed'); exit $code",
                    powershell_payload(launch).unwrap()
                );
                let mut command = Command::new(system_powershell_path().unwrap());
                command.args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-EncodedCommand",
                    &powershell_encoded_command(&script),
                ]);
                configure_shell_env(&mut command, &prepend_path(bin));
                command
            }
            _ => {
                let action = if runner == Runner::Cmd { "/C" } else { "/K" };
                let mut command = Command::new(system_cmd_path().unwrap());
                configure_shell_env(&mut command, &prepend_path(bin));
                command
                    .args(["/E:ON", "/V:OFF", action, RUN_AND_DELETE_BATCH])
                    .envs(launch.env());
                command
            }
        };
        let probe = if bin.join("claude.exe").is_file() {
            bin.join("claude.exe")
        } else {
            bin.join("claude_probe.exe")
        };
        command
            .env("PATHEXT", ".COM;.EXE;.BAT;.CMD")
            .env("CC_SWITCH_CMD_EXPAND", "wrong")
            .env("CC_SWITCH_TEST_PROBE_EXECUTABLE", probe)
            .envs(extra.iter().copied())
            .creation_flags(CREATE_NO_WINDOW);
        if matches!(runner, Runner::CmdKeepOpen | Runner::CmdKeepOpenEmptyTitle) {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            // `popd` releases a drive that `pushd` mapped for a UNC project.
            child
                .stdin
                .take()
                .unwrap()
                .write_all(
                    b"@\"%CC_SWITCH_TEST_PROBE_EXECUTABLE%\" --capture-parent-cwd\r\n@popd 2>nul\r\n@exit 0\r\n",
                )
                .unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        }
    }

    /// Writes the batch for a claude launch, as `claude_terminal_launch` builds it,
    /// and returns the values it reads.
    fn write_claude_batch(batch: &Path, config: &Path) -> Vec<OsString> {
        let launch = ProviderLaunch {
            title: "Using provider-specific claude config:",
            detail: None,
            program: "claude",
            args: vec!["--settings".into(), config.to_string_lossy().into_owned()],
            env: Vec::new(),
            cleanup: vec![config.to_path_buf()],
        };
        let mut values = Vec::new();
        let content = provider_batch(&launch, &mut values).unwrap();
        assert!(content.is_ascii(), "{content}");
        std::fs::write(batch, content).unwrap();
        values
    }

    fn assert_provider_batch_cleanup(
        runner: Runner,
        bin: &Path,
        exit_code: i32,
        project: Option<&Path>,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let hostile = "space & %CC_SWITCH_CMD_EXPAND% ^ ! (test) ; O'Brien ‘ ’ ‚ ‛";
        let cwd = match project {
            Some(project) => project.to_path_buf(),
            None => temp.path().join(format!("迅雷下载 {hostile}")),
        };
        let config_dir = temp.path().join(format!("配置 {hostile}"));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir(&config_dir).unwrap();
        let config = config_dir.join("设置.json");
        let batch = config_dir.join(format!("启动 {hostile}.bat"));
        let config_copy = temp.path().join("settings-copy.json");
        let parent_marker = temp.path().join("parent.txt");
        let marker = cwd.join("cc-switch-provider-marker.txt");
        std::fs::write(&config, "{}\n").unwrap();
        let launch = BatchLaunch {
            batch: &batch,
            cwd: Some(&cwd),
            values: write_claude_batch(&batch, &config),
        };
        let exit = OsString::from(exit_code.to_string());
        let config_copy_value = config_copy.clone().into_os_string();
        let parent_value = parent_marker.clone().into_os_string();
        let clear_title = OsString::from("1");
        let mut extra = vec![
            ("CC_SWITCH_TEST_EXIT_CODE", &exit),
            ("CC_SWITCH_TEST_CONFIG_COPY", &config_copy_value),
            ("CC_SWITCH_TEST_PARENT_MARKER", &parent_value),
        ];
        if runner == Runner::CmdKeepOpenEmptyTitle {
            extra.push(("CC_SWITCH_TEST_CLEAR_TITLE", &clear_title));
        }
        let output = run_batch(runner, &launch, bin, &extra);
        let diagnostics = format!(
            "{runner:?}, bin: {}, exit: {exit_code}\nstatus: {}\nstdout: {}\nstderr: {}",
            bin.display(),
            output.status,
            decode_command_output(&output.stdout),
            decode_command_output(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(&marker).ok().as_deref(),
            Some("ok\n"),
            "claude must run in the project: {diagnostics}"
        );
        assert_eq!(
            std::fs::read_to_string(&config_copy).ok().as_deref(),
            Some("{}\n"),
            "{diagnostics}"
        );
        assert!(!config.exists(), "config must be deleted: {diagnostics}");
        assert!(!batch.exists(), "batch must be deleted: {diagnostics}");
        assert!(output.status.success(), "{diagnostics}");
        assert_eq!(
            decode_command_output(&output.stderr).trim(),
            if exit_code == 0 {
                ""
            } else {
                "cc-switch-test-provider-error"
            },
            "{diagnostics}"
        );
        let parent = std::fs::read_to_string(&parent_marker).unwrap_or_default();
        match runner {
            Runner::Cmd => {}
            Runner::PowerShell => assert_eq!(parent, "parent-resumed", "{diagnostics}"),
            // The interactive CMD stays in the project after Claude exits; a UNC
            // project is entered through a mapped drive.
            _ => assert_eq!(
                Path::new(&parent).file_name(),
                cwd.file_name(),
                "{diagnostics}"
            ),
        }
        let _ = std::fs::remove_file(marker);
    }

    #[test]
    fn provider_batch_keeps_unicode_paths_out_of_batch_bytes() {
        let probes = ClaudeProbes::compile();
        for runner in [Runner::Cmd, Runner::CmdKeepOpen, Runner::PowerShell] {
            for exit_code in [0, 7] {
                assert_provider_batch_cleanup(runner, &probes.exe, exit_code, None);
            }
        }
    }

    #[test]
    fn provider_batch_returns_after_a_cmd_shim_exits() {
        let probes = ClaudeProbes::compile();
        for runner in [Runner::Cmd, Runner::CmdKeepOpen, Runner::PowerShell] {
            for exit_code in [0, 7] {
                assert_provider_batch_cleanup(runner, &probes.cmd_shim, exit_code, None);
            }
        }
    }

    #[test]
    fn provider_batch_survives_claude_clearing_the_console_title() {
        let probes = ClaudeProbes::compile();
        for bin in probes.bins() {
            for exit_code in [0, 7] {
                assert_provider_batch_cleanup(Runner::CmdKeepOpenEmptyTitle, bin, exit_code, None);
            }
        }
    }

    #[test]
    fn provider_batch_enters_a_unc_project_with_pushd() {
        let temp = tempfile::tempdir().unwrap();
        let local = temp.path().join("项目 & O'Brien");
        std::fs::create_dir(&local).unwrap();
        let local = local.to_string_lossy().into_owned();
        // The administrative share stands in for \\wsl$ or a network share.
        let unc = PathBuf::from(format!(r"\\localhost\{}${}", &local[..1], &local[2..]));
        if !unc.is_dir() {
            eprintln!("skipped: {} is not reachable", unc.display());
            return;
        }
        let probes = ClaudeProbes::compile();
        for runner in [Runner::Cmd, Runner::CmdKeepOpen, Runner::PowerShell] {
            assert_provider_batch_cleanup(runner, &probes.exe, 0, Some(&unc));
        }
    }

    #[test]
    fn provider_batch_deletes_config_when_project_directory_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("设置.json");
        let batch = temp.path().join("provider.bat");
        let missing = temp.path().join("已删除的项目");
        std::fs::write(&config, "{}\n").unwrap();
        let launch = BatchLaunch {
            batch: &batch,
            cwd: Some(&missing),
            values: write_claude_batch(&batch, &config),
        };
        let mut command = Command::new(system_cmd_path().unwrap());
        configure_shell_env(&mut command, &std::env::var("PATH").unwrap());
        let output = command
            .args(["/D", "/E:ON", "/V:OFF", "/C", RUN_AND_DELETE_BATCH])
            .envs(launch.env())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap();
        assert!(
            !config.exists(),
            "the provider config must not outlive a failed launch: {output:?}"
        );
        assert!(!batch.exists(), "caller must delete the provider batch");
    }

    /// A stand-in `agy`: records its arguments and the variables a launch sets.
    const AGY_PROBE: &str = r#"fn main() {
    let mut record = String::new();
    for argument in std::env::args().skip(1) {
        record.push_str(&format!("arg={argument}\n"));
    }
    for name in ["GEMINI_API_KEY", "GOOGLE_GEMINI_BASE_URL", "CC_SWITCH_TEST_UNICODE"] {
        record.push_str(&format!("{name}={:?}\n", std::env::var(name).ok()));
    }
    std::fs::write(std::env::var_os("CC_SWITCH_TEST_RECORD").unwrap(), record).unwrap();
}
"#;

    #[test]
    fn provider_batch_scopes_variables_to_the_program() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        compile(AGY_PROBE, &bin.join("agy.exe"));
        let hostile = "space & %CC_SWITCH_CMD_EXPAND% ^ ! (test) ; O'Brien ‘ ’";
        let profile = temp.path().join(format!("私有目录 {hostile}"));
        let key = "sk-test %CC_SWITCH_CMD_EXPAND% ^ & | < > ( ) ! ' ;";
        let unicode = format!("代理 {hostile}");
        let launch = ProviderLaunch {
            title: "Using a private profile (test) & 100%:",
            detail: None,
            program: "agy",
            args: vec![
                format!("--gemini_dir={}", profile.display()),
                String::new(),
                "--conversation=abc".into(),
            ],
            env: vec![
                ("GEMINI_API_KEY", Some(key.into())),
                ("GOOGLE_GEMINI_BASE_URL", None),
                ("CC_SWITCH_TEST_UNICODE", Some(unicode.clone())),
            ],
            cleanup: Vec::new(),
        };
        let mut values = Vec::new();
        let content = provider_batch(&launch, &mut values).unwrap();
        assert!(content.is_ascii(), "{content}");
        // An ASCII key stays in the batch, off every command line.
        assert!(!values
            .iter()
            .any(|value| value.to_string_lossy().contains("sk-test")));
        let batch = temp.path().join("provider.bat");
        std::fs::write(&batch, content).unwrap();
        let record = temp.path().join("record.txt");
        let mut command = Command::new(system_cmd_path().unwrap());
        configure_shell_env(&mut command, &prepend_path(&bin));
        let mut child = command
            .args(["/E:ON", "/V:OFF", "/K", RUN_AND_DELETE_BATCH])
            .envs(
                BatchLaunch {
                    batch: &batch,
                    cwd: None,
                    values,
                }
                .env(),
            )
            .env("PATHEXT", ".COM;.EXE;.BAT;.CMD")
            .env("CC_SWITCH_CMD_EXPAND", "wrong")
            .env("CC_SWITCH_TEST_RECORD", &record)
            .env("GEMINI_API_KEY", "parent-key")
            .env("GOOGLE_GEMINI_BASE_URL", "parent-url")
            .env_remove("CC_SWITCH_TEST_UNICODE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        // The shell left after the program keeps its own values and no carriers.
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"@echo key=%GEMINI_API_KEY% url=%GOOGLE_GEMINI_BASE_URL%\r\n@if defined CC_SWITCH_INTERNAL_VALUE_0 (echo carriers=kept) else echo carriers=cleared\r\n@exit 0\r\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = decode_command_output(&output.stdout);
        assert_eq!(
            std::fs::read_to_string(&record).unwrap_or_default(),
            format!(
                "arg=--gemini_dir={}\narg=\narg=--conversation=abc\nGEMINI_API_KEY={:?}\nGOOGLE_GEMINI_BASE_URL=None\nCC_SWITCH_TEST_UNICODE={:?}\n",
                profile.display(),
                Some(key),
                Some(&unicode)
            ),
            "{stdout}"
        );
        assert!(
            stdout.contains("Using a private profile (test) & 100%:"),
            "{stdout}"
        );
        // The interactive CMD echoes each command after its prompt, so match whole lines.
        for line in ["key=parent-key url=parent-url", "carriers=cleared"] {
            assert!(stdout.lines().any(|output| output == line), "{stdout}");
        }
        assert_eq!(decode_command_output(&output.stderr).trim(), "");
        assert!(!batch.exists(), "caller must delete the provider batch");
    }

    #[test]
    fn provider_batch_refuses_values_cmd_cannot_carry() {
        let launch = |args: Vec<String>, env: Vec<(&'static str, Option<String>)>| ProviderLaunch {
            title: "test",
            detail: None,
            program: "agy",
            args,
            env,
            cleanup: Vec::new(),
        };
        for launch in [
            launch(vec!["--model=a\"b".into()], Vec::new()),
            launch(vec!["--model=a\r\nb".into()], Vec::new()),
            launch(
                Vec::new(),
                vec![("GEMINI_API_KEY", Some("a\"&calc".into()))],
            ),
            launch(Vec::new(), vec![("GEMINI_API_KEY", Some("密钥\"".into()))]),
            launch(
                (0..=VALUE_ENV_VARS.len()).map(|i| i.to_string()).collect(),
                Vec::new(),
            ),
        ] {
            assert!(provider_batch(&launch, &mut Vec::new()).is_err());
        }
    }

    #[test]
    fn command_batch_returns_to_its_caller() {
        let temp = tempfile::tempdir().unwrap();
        let batch = temp.path().join("hermes_dashboard.bat");
        let content = command_batch("hermes dashboard", "echo cc-switch-command-ran");
        assert!(content.is_ascii());
        std::fs::write(&batch, content).unwrap();
        let launch = BatchLaunch {
            batch: &batch,
            cwd: None,
            values: Vec::new(),
        };
        let mut command = Command::new(system_cmd_path().unwrap());
        configure_shell_env(&mut command, &std::env::var("PATH").unwrap());
        // `pause` reads the closed stdin and returns at once.
        let output = command
            .args(["/D", "/E:ON", "/V:OFF", "/C", RUN_AND_DELETE_BATCH])
            .envs(launch.env())
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap();
        let stdout = decode_command_output(&output.stdout);
        assert!(stdout.contains("cc-switch-command-ran"), "{stdout}");
        assert!(stdout.contains("Command exited"), "{stdout}");
        assert_eq!(decode_command_output(&output.stderr).trim(), "");
        assert!(output.status.success());
        assert!(!batch.exists(), "caller must delete the command batch");
    }

    #[test]
    fn cmd_console_receives_restored_user_path() {
        let root = tempfile::tempdir().unwrap();
        let tool_dir = root.path().join("claude 用户 & bin");
        std::fs::create_dir_all(&tool_dir).unwrap();
        std::fs::write(
            tool_dir.join("cc_switch_start_test_tool.cmd"),
            "@echo restored-terminal-path\r\n@exit /b 0\r\n",
        )
        .unwrap();
        let batch = root.path().join("provider.bat");
        let run = |path: &str| {
            std::fs::write(
                &batch,
                "@echo off\r\necho cc-switch-provider-started\r\ncc_switch_start_test_tool\r\n",
            )
            .unwrap();
            let launch = BatchLaunch {
                batch: &batch,
                cwd: None,
                values: Vec::new(),
            };
            let mut command = Command::new(system_cmd_path().unwrap());
            configure_shell_env(&mut command, path);
            command
                .args(["/D", "/E:ON", "/V:OFF", "/C", RUN_AND_DELETE_BATCH])
                .envs(launch.env())
                .current_dir(root.path())
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .unwrap()
        };
        let system = system_cmd_path().unwrap();
        let inherited = system.parent().unwrap().to_string_lossy().into_owned();
        let parent_path = std::env::var_os("PATH");

        let missing = run(&inherited);
        assert!(decode_command_output(&missing.stdout).contains("cc-switch-provider-started"));
        assert!(
            !decode_command_output(&missing.stdout).contains("restored-terminal-path"),
            "the tool must be absent on the narrow PATH: {missing:?}"
        );
        let restored = run(&format!("{inherited};{}", tool_dir.display()));
        assert!(
            decode_command_output(&restored.stdout).contains("restored-terminal-path"),
            "{restored:?}"
        );
        assert!(!batch.exists(), "caller must delete the provider batch");
        assert_eq!(std::env::var_os("PATH"), parent_path);
    }

    /// The arguments WT appends after `--`: joined with spaces, and quoted when they
    /// contain one, without escaping inner quotes. Tests swap the keep-open flags.
    fn wt_appended_command_line(args: &[String]) -> String {
        args.iter()
            .filter(|argument| *argument != "-NoExit")
            .map(|argument| {
                let argument = if argument == "/K" { "/C" } else { argument };
                if argument.contains(' ') {
                    format!("\"{argument}\"")
                } else {
                    argument.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    // A resident WT may hold another launch's values, and its tabs may inherit
    // them. Run each tab command in exactly that environment, with real shells.
    #[test]
    #[serial_test::serial]
    fn wt_tab_binds_this_launch_despite_the_resident_environment() {
        let temp = tempfile::tempdir().unwrap();
        let hostile = temp.path().join(
            "迅雷下载 space & %CC_SWITCH_CMD_EXPAND% ^ !CC_SWITCH_BANG! (test) ; O'Brien ‘ ’ ‚ ‛",
        );
        let starting = temp.path().join("starting-directory");
        std::fs::create_dir(&hostile).unwrap();
        std::fs::create_dir(&starting).unwrap();
        // A cwd lookup would fail instead of running the system bootstrap.
        std::fs::write(starting.join("powershell.exe"), b"not a Windows executable").unwrap();
        let stale_batch = temp.path().join("stale.bat");
        let stale_config = temp.path().join("stale.txt");
        let batch = hostile.join("provider.bat");
        let marker = hostile.join("配置结果.txt");
        let _restore = EnvRestore::capture(
            inherited_env_to_clear().chain(["CC_SWITCH_CMD_EXPAND", "CC_SWITCH_BANG"]),
        );
        for name in PARENT_CLAUDE_SESSION_ENV_VARS {
            std::env::set_var(name, "stale-parent-session");
        }
        std::env::set_var(BATCH_PATH_ENV, format!("\"{}\"", stale_batch.display()));
        for name in VALUE_ENV_VARS {
            std::env::set_var(name, &stale_config);
        }
        std::env::set_var(CWD_ENV, temp.path());
        std::env::set_var("CC_SWITCH_CMD_EXPAND", "wrong");
        std::env::set_var("CC_SWITCH_BANG", "wrong");
        let probe: String = PARENT_CLAUDE_SESSION_ENV_VARS
            .iter()
            .map(|name| format!("if defined {name} exit /b 2\r\n"))
            .collect();
        let probe = format!(
            "@echo off\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\n{probe}cd /d \"%CC_SWITCH_INTERNAL_CWD%\" || exit /b 5\r\n> \"%CC_SWITCH_INTERNAL_VALUE_0%\" echo ran\r\n> cwd.txt echo ok\r\n"
        );
        let leak_check: String = inherited_env_to_clear()
            .map(|name| format!(" & if defined {name} echo cc-switch-leak-{name}"))
            .collect();
        let cmd = system_cmd_path().unwrap();
        let powershell = system_powershell_path().unwrap();
        let pwsh = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|directory| directory.join("pwsh.exe"))
            .find(|path| path.is_file());
        let profile = |shell| WtDefaultProfile {
            selector: "{22222222-2222-4222-8222-222222222222}".into(),
            shell,
        };
        let mut cases = vec![
            (Some(profile(WtDefaultShell::Cmd)), Some(cmd.clone())),
            (Some(profile(WtDefaultShell::PowerShell)), Some(powershell)),
            (None, None),
        ];
        if let Some(pwsh) = pwsh {
            cases.push((Some(profile(WtDefaultShell::PowerShell)), Some(pwsh)));
        }
        let mut failures = Vec::new();
        for (profile, executable) in cases {
            std::fs::write(&batch, &probe).unwrap();
            std::fs::write(&stale_batch, &probe).unwrap();
            let launch = BatchLaunch {
                batch: &batch,
                cwd: Some(&hostile),
                values: vec![marker.clone().into_os_string()],
            };
            let args = wt_new_tab_args(profile.as_ref(), &launch).unwrap();
            assert!(
                args.iter().all(|argument| !argument.contains(['%', ';'])),
                "WT would expand or split {args:?}"
            );
            let appended = &args[args.iter().position(|argument| argument == "--").unwrap() + 1..];
            let (program, appended) = match &executable {
                Some(executable) => (executable.clone(), appended),
                None => (PathBuf::from(&appended[0]), &appended[1..]),
            };
            let mut line = wt_appended_command_line(appended);
            let cmd_tab = profile
                .as_ref()
                .is_none_or(|profile| profile.shell == WtDefaultShell::Cmd);
            if cmd_tab {
                line.push_str(&leak_check);
            }
            let output = Command::new(&program)
                .raw_arg(&line)
                .current_dir(&starting)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .unwrap();
            let stdout = decode_command_output(&output.stdout);
            let observed = std::fs::read_to_string(&marker).unwrap_or_default();
            if !output.status.success()
                || observed.trim() != "ran"
                || !hostile.join("cwd.txt").is_file()
                || stdout.contains("cc-switch-leak-")
                || batch.exists()
                || !stale_batch.exists()
                || stale_config.exists()
            {
                failures.push(format!(
                    "{}: {observed:?}; stdout={stdout}; stderr={}",
                    program.display(),
                    decode_command_output(&output.stderr)
                ));
            }
            let _ = std::fs::remove_file(&marker);
            let _ = std::fs::remove_file(hostile.join("cwd.txt"));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A WT stand-in: records its mode and the parent-session variables it saw,
    /// then runs the appended CMD tab command with `/C` and exits with its status.
    fn fake_wt_installation(directory: &Path, settings: Option<&str>) {
        let source = r#"use std::os::windows::process::CommandExt;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let names: &[&str] = &NAMES;
    let leaked: Vec<_> = names.iter().filter(|name| std::env::var_os(name).is_some()).collect();
    let mut start = args.iter().position(|argument| argument == "--").unwrap() + 1;
    let explicit = !args[start].starts_with('/');
    let mut program = std::path::Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32\\cmd.exe");
    if explicit {
        assert!(!args.iter().any(|argument| argument == "--profile" || argument == "--appendCommandLine"));
        program = args[start].clone().into();
        start += 1;
    }
    let mode = if explicit { "explicit-cmd" } else { "profile" };
    std::fs::write(std::env::current_exe().unwrap().with_extension("log"), format!("{mode}\n{leaked:?}")).unwrap();
    let line = args[start..]
        .iter()
        .map(|argument| match argument.as_str() {
            "/K" => "/C".to_string(),
            argument if argument.contains(' ') => format!("\"{argument}\""),
            argument => argument.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let status = std::process::Command::new(program).raw_arg(line).status().unwrap();
    std::process::exit(status.code().unwrap_or(1));
}
"#
        .replace("NAMES", &format!("{PARENT_CLAUDE_SESSION_ENV_VARS:?}"));
        compile(&source, &directory.join("wt.exe"));
        // A versioned system PE supplies the version probe for this fake WT.
        std::fs::copy(
            system_cmd_path().unwrap(),
            directory.join("WindowsTerminal.exe"),
        )
        .unwrap();
        std::fs::write(directory.join(".portable"), "").unwrap();
        if let Some(settings) = settings {
            std::fs::create_dir_all(directory.join("settings")).unwrap();
            std::fs::write(directory.join("settings/settings.json"), settings).unwrap();
        }
    }

    const CMD_PROBE: &str = "@echo off\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\ncd /d \"%CC_SWITCH_INTERNAL_CWD%\" || exit /b 5\r\n> \"%CC_SWITCH_INTERNAL_VALUE_0%\" echo ok\r\n";

    #[test]
    fn wt_unsupported_default_profile_opens_an_explicit_cmd_tab() {
        let temp = tempfile::tempdir().unwrap();
        let wt = temp.path().join("wt");
        std::fs::create_dir(&wt).unwrap();
        fake_wt_installation(&wt, None);
        let project = temp
            .path()
            .join("项目 O'Brien ‘ ’ & %CC_SWITCH_TEST_PROFILE% ; !");
        std::fs::create_dir(&project).unwrap();
        let system = system_cmd_path().unwrap();
        let path = format!("{};{}", wt.display(), system.parent().unwrap().display());
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
            std::fs::create_dir_all(wt.join("settings")).unwrap();
            std::fs::write(wt.join("settings/settings.json"), settings).unwrap();
            let batch = temp.path().join(format!("provider-{index}.bat"));
            let marker = project.join(format!("配置结果-{index}.txt"));
            std::fs::write(&batch, CMD_PROBE).unwrap();
            launch_wt_tab(
                &BatchLaunch {
                    batch: &batch,
                    cwd: Some(&project),
                    values: vec![marker.clone().into_os_string()],
                },
                &path,
            )
            .unwrap();
            wait_until("explicit CMD tab runs and deletes its batch", || {
                !batch.exists() && marker.exists()
            });
            assert_eq!(std::fs::read_to_string(marker).unwrap().trim(), "ok");
            let log = std::fs::read_to_string(wt.join("wt.log")).unwrap();
            assert!(log.starts_with("explicit-cmd\n"), "{settings}: {log}");
        }
    }

    // Model WT's CMD profile: the tab inherits wt.exe's environment and runs the
    // appended command. A typical user PATH must not push CMD past 8191 characters.
    #[test]
    #[serial_test::serial]
    fn wt_cmd_tab_inherits_restored_path_but_not_the_parent_session() {
        let _restore = EnvRestore::capture(PARENT_CLAUDE_SESSION_ENV_VARS);
        for name in PARENT_CLAUDE_SESSION_ENV_VARS {
            std::env::set_var(name, "stale-parent-session");
        }
        let temp = tempfile::tempdir().unwrap();
        let wt = temp.path().join("wt");
        std::fs::create_dir(&wt).unwrap();
        fake_wt_installation(
            &wt,
            Some(r#"{"defaultProfile":"{0caa0dad-35be-5f56-a8ff-afceeeaa6101}"}"#),
        );
        let project = temp.path().join("项目");
        std::fs::create_dir(&project).unwrap();
        let marker = project.join("配置结果.txt");
        let batch = temp.path().join("provider.bat");
        std::fs::write(
            &batch,
            "@echo off\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\n> \"%CC_SWITCH_INTERNAL_VALUE_0%\" echo %PATH%\r\n> cwd.txt echo ok\r\n",
        )
        .unwrap();
        let system = system_cmd_path().unwrap();
        let path = [
            wt.display().to_string(),
            system.parent().unwrap().display().to_string(),
        ]
        .into_iter()
        .chain((0..120).map(|index| format!(r"C:\cc-switch-restored-path\tool-{index:03}")))
        .collect::<Vec<_>>()
        .join(";");
        assert!(path.len() > 3000);
        launch_wt_tab(
            &BatchLaunch {
                batch: &batch,
                cwd: Some(&project),
                values: vec![marker.clone().into_os_string()],
            },
            &path,
        )
        .unwrap();
        wait_until("CMD profile tab runs and deletes its batch", || {
            !batch.exists() && marker.exists()
        });
        assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), path);
        assert!(
            project.join("cwd.txt").is_file(),
            "--startingDirectory . must open the tab in the project"
        );
        assert_eq!(
            std::fs::read_to_string(wt.join("wt.log")).unwrap(),
            "profile\n[]"
        );
    }

    #[test]
    #[serial_test::serial]
    fn wt_failures_fall_back_to_a_new_cmd_console() {
        struct RestoreSettings(crate::settings::AppSettings);
        impl Drop for RestoreSettings {
            fn drop(&mut self) {
                let _ = crate::settings::update_settings(self.0.clone());
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let _env = EnvRestore::capture(["CC_SWITCH_TEST_HOME", "PATH"]);
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path().join("home"));
        let _settings = RestoreSettings(crate::settings::get_settings());
        let mut settings = crate::settings::get_settings();
        settings.preferred_terminal = Some("wt".to_string());
        crate::settings::update_settings(settings).unwrap();
        let system = system_cmd_path().unwrap();
        let project = temp
            .path()
            .join("项目 O'Brien ‘ ’ & %CC_SWITCH_TEST_PROFILE% ; !");
        std::fs::create_dir(&project).unwrap();
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        for failure in [
            "old-version",
            "missing-settings",
            "launcher-failed",
            "spawn-failed",
        ] {
            let bin = temp.path().join(failure);
            std::fs::create_dir(&bin).unwrap();
            std::fs::write(bin.join(".portable"), "").unwrap();
            if failure == "launcher-failed" {
                // where.exe rejects WT's arguments and exits non-zero at once.
                std::fs::copy(system.with_file_name("where.exe"), bin.join("wt.exe")).unwrap();
            } else {
                std::fs::write(bin.join("wt.exe"), b"not a Windows executable").unwrap();
            }
            if failure != "old-version" {
                std::fs::copy(&system, bin.join("WindowsTerminal.exe")).unwrap();
            }
            if matches!(failure, "launcher-failed" | "spawn-failed") {
                std::fs::create_dir(bin.join("settings")).unwrap();
                std::fs::write(
                    bin.join("settings/settings.json"),
                    r#"{"defaultProfile":"{0caa0dad-35be-5f56-a8ff-afceeeaa6101}"}"#,
                )
                .unwrap();
            }
            std::env::set_var(
                "PATH",
                std::env::join_paths(
                    std::iter::once(bin.clone()).chain(std::env::split_paths(&inherited)),
                )
                .unwrap(),
            );
            let batch = temp.path().join(format!("{failure}.bat"));
            let marker = project.join(format!("{failure}.txt"));
            // Exit on the same parsed line as the marker: this route-only probe
            // must not leave an interactive test console open.
            std::fs::write(&batch, "@echo off\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\ncd /d \"%CC_SWITCH_INTERNAL_CWD%\" || exit 2\r\n> \"%CC_SWITCH_INTERNAL_VALUE_0%\" echo ok & exit 0\r\n").unwrap();
            launch_batch(&BatchLaunch {
                batch: &batch,
                cwd: Some(&project),
                values: vec![marker.clone().into_os_string()],
            })
            .unwrap_or_else(|error| panic!("{failure} must fall back to CMD: {error}"));
            // The redirect creates the marker before echo writes to it.
            wait_until(failure, || {
                std::fs::metadata(&marker).is_ok_and(|marker| marker.len() > 0)
            });
            assert_eq!(std::fs::read_to_string(marker).unwrap().trim(), "ok");
        }
    }

    // Only an isolated portable copy may opt in: WT hands new tabs to a resident
    // process of the same installation, so never point this at the user's WT.
    // The caller owns that portable WT and closes it afterwards.
    #[test]
    #[serial_test::serial]
    #[ignore = "requires CC_SWITCH_WT_TEST_EXE pointing to an isolated portable WT copy in temp"]
    fn real_wt_runs_each_launch_with_its_own_values() {
        let launcher =
            PathBuf::from(std::env::var_os("CC_SWITCH_WT_TEST_EXE").expect("isolated WT path"));
        assert!(launcher.starts_with(std::env::temp_dir()));
        let directory = launcher.parent().unwrap();
        assert!(directory.join(".portable").is_file());
        let _restore = EnvRestore::capture(PARENT_CLAUDE_SESSION_ENV_VARS);
        for name in PARENT_CLAUDE_SESSION_ENV_VARS {
            std::env::set_var(name, "stale-parent-session");
        }
        let path = format!("{};{}", directory.display(), effective_path_string());
        let temp = tempfile::tempdir().unwrap();
        let hostile = temp
            .path()
            .join("项目 space & %CC_SWITCH_CMD_EXPAND% ^ ; (test) O'Brien ‘ ’");
        let powershell = system_powershell_path().unwrap();
        let profiles = serde_json::json!([
            {"guid":"{22222222-2222-4222-8222-222222222222}","name":"CMD","commandline":system_cmd_path().unwrap()},
            {"guid":"{33333333-3333-4333-8333-333333333333}","name":"WinPS","commandline":format!("\"{}\" -NoProfile", powershell.display())},
            {"guid":"{44444444-4444-4444-8444-444444444444}","name":"PS7","commandline":"pwsh.exe -NoProfile"}
        ]);
        let leak_probe: String = PARENT_CLAUDE_SESSION_ENV_VARS
            .iter()
            .map(|name| format!("if defined {name} >> marker.txt echo leaked={name}\r\n"))
            .collect();
        for guid in [
            "{22222222-2222-4222-8222-222222222222}",
            "{33333333-3333-4333-8333-333333333333}",
            "{44444444-4444-4444-8444-444444444444}",
        ] {
            std::fs::write(
                directory.join("settings/settings.json"),
                serde_json::json!({"defaultProfile":guid,"profiles":{"list":profiles}}).to_string(),
            )
            .unwrap();
            // The first launch may start WT; the second reuses the resident.
            for label in ["A", "B"] {
                let cwd = hostile.join(label);
                std::fs::create_dir_all(&cwd).unwrap();
                let config = hostile.join(format!("config-{label}.json"));
                let batch = hostile.join(format!("provider-{label}.bat"));
                std::fs::write(&config, format!("{label}\r\n")).unwrap();
                std::fs::write(&batch, format!("@echo off\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\ncd /d \"%CC_SWITCH_INTERNAL_CWD%\" || exit /b 5\r\n> marker.txt echo profile=%WT_PROFILE_ID%\r\n>> marker.txt type \"%CC_SWITCH_INTERNAL_VALUE_0%\"\r\n{leak_probe}>> marker.txt echo done\r\n")).unwrap();
                launch_wt_tab(
                    &BatchLaunch {
                        batch: &batch,
                        cwd: Some(&cwd),
                        values: vec![config.clone().into_os_string()],
                    },
                    &path,
                )
                .unwrap();
                let marker = cwd.join("marker.txt");
                let deadline = Instant::now() + Duration::from_secs(15);
                let observed = loop {
                    let value = std::fs::read_to_string(&marker).unwrap_or_default();
                    if value.lines().last() == Some("done") {
                        break value;
                    }
                    assert!(Instant::now() < deadline, "WT must run batch {label}");
                    std::thread::sleep(Duration::from_millis(50));
                };
                assert!(observed.contains(&format!("profile={guid}")), "{observed}");
                assert_eq!(observed.lines().nth(1), Some(label), "{observed}");
                assert!(!observed.contains("leaked="), "{observed}");
                wait_until("caller deletes the batch", || !batch.exists());
                std::fs::remove_file(marker).unwrap();
            }
        }
    }
}
