#[cfg(target_os = "windows")]
use super::misc::{
    configure_windows_batch_env, decode_windows_where_output, effective_path_os,
    is_windows_app_execution_alias_dir, wait_child_output, windows_path_lookup_command,
    CommandDeadline,
};
#[cfg(any(target_os = "windows", test))]
use super::misc::{push_unique_path, WINDOWS_BATCH_PATH_COMMAND};
use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Shell family of Windows Terminal's default profile.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WtDefaultShell {
    Pwsh,
    PowerShell,
    Cmd,
}

/// Keep the selector with the shell whose flags we append. Without `--profile`,
/// WT can match the appended commandline against another profile instead of
/// using defaultProfile, even when `--appendCommandLine` is set.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct WtDefaultProfile {
    selector: String,
    shell: WtDefaultShell,
}

#[cfg(any(target_os = "windows", test))]
impl WtDefaultProfile {
    fn build_wt_args<'a>(&'a self, ps_cmd: &'a str) -> Vec<&'a str> {
        let mut args = vec![
            "new-tab",
            "--profile",
            self.selector.as_str(),
            "--appendCommandLine",
            "--",
        ];
        match self.shell {
            WtDefaultShell::Pwsh | WtDefaultShell::PowerShell => {
                args.extend(["-NoExit", "-Command", ps_cmd]);
            }
            WtDefaultShell::Cmd => {
                args.extend(["/D", "/V:OFF", "/K", WINDOWS_BATCH_PATH_COMMAND]);
            }
        }
        args
    }
}

/// When the default profile cannot be appended to safely, start cmd explicitly
/// instead of appending `/K` to an unknown shell.
#[cfg(any(target_os = "windows", test))]
fn build_wt_cmd_fallback_args() -> Vec<&'static str> {
    vec![
        "new-tab",
        "cmd",
        "/D",
        "/V:OFF",
        "/K",
        WINDOWS_BATCH_PATH_COMMAND,
    ]
}

/// `--appendCommandLine` exists from WT 1.19. Fall back to explicit cmd before spawn
/// so an unsupported flag is not discovered only after the async launch.
#[cfg(any(target_os = "windows", test))]
fn select_wt_launch_args<'a>(
    profile: Option<&'a WtDefaultProfile>,
    supports_append: bool,
    ps_cmd: &'a str,
) -> Vec<&'a str> {
    match (profile, supports_append) {
        (Some(profile), true) => profile.build_wt_args(ps_cmd),
        _ => build_wt_cmd_fallback_args(),
    }
}

/// Strip braces and lowercase GUID strings so they compare across configs.
#[cfg(any(target_os = "windows", test))]
fn normalize_wt_guid(guid: &str) -> String {
    guid.trim()
        .trim_matches(|c: char| c == '{' || c == '}')
        .to_ascii_lowercase()
}

#[cfg(any(target_os = "windows", test))]
fn known_wt_shell_from_normalized_guid(guid: &str) -> Option<WtDefaultShell> {
    match guid {
        "574e775e-4f2a-5b96-ac1e-a2962a402336" => Some(WtDefaultShell::Pwsh),
        "61c54bbd-c2c6-5271-96e7-009a87ff44bf" => Some(WtDefaultShell::PowerShell),
        "0caa0dad-35be-5f56-a8ff-afceeeaa6101" => Some(WtDefaultShell::Cmd),
        _ => None,
    }
}

/// Split the first executable from the remaining arguments. WT still owns the
/// original commandline quoting; this parser does not rebuild it.
#[cfg(any(target_os = "windows", test))]
fn split_wt_commandline(cmdline: &str) -> Option<(&str, &str)> {
    let trimmed = cmdline.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(stripped) = trimmed.strip_prefix('"') {
        let closing_quote = stripped.find('"')?;
        let executable = &stripped[..closing_quote];
        if executable.is_empty() {
            return None;
        }
        return Some((executable, stripped[closing_quote + 1..].trim_start()));
    }

    let split_at = trimmed
        .find(|character: char| character.is_whitespace())
        .unwrap_or(trimmed.len());
    Some((&trimmed[..split_at], trimmed[split_at..].trim_start()))
}

/// Tokenize arguments with Windows commandline quoting. Parse failure means
/// appending extra arguments cannot be proven safe.
#[cfg(any(target_os = "windows", test))]
fn split_wt_arguments(arguments: &str) -> Option<Vec<String>> {
    let characters: Vec<char> = arguments.chars().collect();
    let mut result = Vec::new();
    let mut index = 0usize;

    while index < characters.len() {
        while index < characters.len() && characters[index].is_whitespace() {
            index += 1;
        }
        if index == characters.len() {
            break;
        }

        let mut argument = String::new();
        let mut in_quotes = false;
        while index < characters.len() {
            if !in_quotes && characters[index].is_whitespace() {
                break;
            }

            if characters[index] == '\\' {
                let slash_start = index;
                while index < characters.len() && characters[index] == '\\' {
                    index += 1;
                }
                let slash_count = index - slash_start;
                if index < characters.len() && characters[index] == '"' {
                    argument.extend(std::iter::repeat_n('\\', slash_count / 2));
                    if slash_count % 2 == 0 {
                        in_quotes = !in_quotes;
                    } else {
                        argument.push('"');
                    }
                    index += 1;
                } else {
                    argument.extend(std::iter::repeat_n('\\', slash_count));
                }
                continue;
            }

            if characters[index] == '"' {
                in_quotes = !in_quotes;
                index += 1;
                continue;
            }

            argument.push(characters[index]);
            index += 1;
        }

        if in_quotes {
            return None;
        }
        result.push(argument);
    }

    Some(result)
}

/// PowerShell host CLI option arity. Unique prefixes match; mixed-kind
/// prefixes are treated as unknown so append is refused.
/// `pwsh` and Windows PowerShell 5.1 differ only in `-Version`'s kind and in
/// shell-specific parameters, so each shell keeps its own table.
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PsHostOption {
    Flag,
    Value,
    Terminal,
}

/// Host aliases and parameter names shared by `pwsh` and Windows PowerShell.
/// Shell-specific differences remain separate so changes cannot silently drift.
#[cfg(any(target_os = "windows", test))]
const COMMON_POWERSHELL_HOST_OPTIONS: &[(&str, PsHostOption)] = &[
    ("?", PsHostOption::Terminal),
    ("c", PsHostOption::Terminal),
    ("command", PsHostOption::Terminal),
    ("config", PsHostOption::Value),
    ("configurationname", PsHostOption::Value),
    ("e", PsHostOption::Terminal),
    ("ea", PsHostOption::Value),
    ("ec", PsHostOption::Terminal),
    ("encodedarguments", PsHostOption::Value),
    ("encodedcommand", PsHostOption::Terminal),
    ("ep", PsHostOption::Value),
    ("ex", PsHostOption::Value),
    ("executionpolicy", PsHostOption::Value),
    ("f", PsHostOption::Terminal),
    ("file", PsHostOption::Terminal),
    ("h", PsHostOption::Terminal),
    ("help", PsHostOption::Terminal),
    ("if", PsHostOption::Value),
    ("inp", PsHostOption::Value),
    ("inputformat", PsHostOption::Value),
    ("mta", PsHostOption::Flag),
    ("noe", PsHostOption::Flag),
    ("noexit", PsHostOption::Flag),
    ("nol", PsHostOption::Flag),
    ("nologo", PsHostOption::Flag),
    ("noni", PsHostOption::Flag),
    ("noninteractive", PsHostOption::Flag),
    ("nop", PsHostOption::Flag),
    ("noprofile", PsHostOption::Flag),
    ("o", PsHostOption::Value),
    ("of", PsHostOption::Value),
    ("outputformat", PsHostOption::Value),
    ("servermode", PsHostOption::Terminal),
    ("sta", PsHostOption::Flag),
    ("w", PsHostOption::Value),
    ("wd", PsHostOption::Value),
    ("windowstyle", PsHostOption::Value),
    ("workingdirectory", PsHostOption::Value),
];

/// `pwsh`-only options. `-Version` exits instead of accepting a value.
#[cfg(any(target_os = "windows", test))]
const PWSH_HOST_OPTIONS: &[(&str, PsHostOption)] = &[
    ("commandwithargs", PsHostOption::Terminal),
    ("configurationfile", PsHostOption::Value),
    ("custompipename", PsHostOption::Value),
    ("cwa", PsHostOption::Terminal),
    ("i", PsHostOption::Flag),
    ("interactive", PsHostOption::Flag),
    ("l", PsHostOption::Flag),
    ("login", PsHostOption::Flag),
    ("noprofileloadtime", PsHostOption::Flag),
    ("settings", PsHostOption::Value),
    ("settingsfile", PsHostOption::Value),
    ("sshs", PsHostOption::Terminal),
    ("sshservermode", PsHostOption::Terminal),
    ("v", PsHostOption::Terminal),
    ("version", PsHostOption::Terminal),
    ("wo", PsHostOption::Value),
];

/// Windows PowerShell 5.1-only options and classifications. `-Version`
/// accepts `2.0`/`3.0`, unlike `pwsh`.
#[cfg(any(target_os = "windows", test))]
const WINDOWS_POWERSHELL_HOST_OPTIONS: &[(&str, PsHostOption)] = &[
    ("psconsolefile", PsHostOption::Value),
    ("v", PsHostOption::Value),
    ("version", PsHostOption::Value),
];

/// PowerShell host CLI option dashes: ASCII hyphen plus the Unicode dashes
/// `pwsh`/`powershell.exe` accept (U+2013, U+2014, U+2015). Copied-from-docs
/// commandlines often use these instead of ASCII `-`.
#[cfg(any(target_os = "windows", test))]
const PS_OPTION_DASHES: &[char] = &['-', '\u{2013}', '\u{2014}', '\u{2015}'];

#[cfg(any(target_os = "windows", test))]
fn ps_host_option_name(argument: &str) -> Option<&str> {
    let mut chars = argument.chars();
    let first = chars.next()?;
    let rest = chars.as_str();
    if first == '/' {
        return (!rest.is_empty()).then_some(rest);
    }
    if !PS_OPTION_DASHES.contains(&first) {
        return None;
    }
    // `--NoLogo` and `––NoLogo` (same dash twice) are long-option spellings.
    // Mixed dashes (`-–NoLogo`) are not options; leave the second character.
    let mut inner = rest.chars();
    let rest = if inner.next() == Some(first) {
        inner.as_str()
    } else {
        rest
    };
    (!rest.is_empty()).then_some(rest)
}

#[cfg(any(target_os = "windows", test))]
fn classify_ps_host_option(shell: WtDefaultShell, name: &str) -> Option<PsHostOption> {
    let shell_options = match shell {
        WtDefaultShell::Pwsh => PWSH_HOST_OPTIONS,
        WtDefaultShell::PowerShell => WINDOWS_POWERSHELL_HOST_OPTIONS,
        WtDefaultShell::Cmd => return None,
    };
    let name = name.to_ascii_lowercase();

    let all_options = || {
        COMMON_POWERSHELL_HOST_OPTIONS
            .iter()
            .chain(shell_options.iter())
    };

    // Exact alias or full parameter match wins immediately.
    if let Some((_, option)) = all_options().find(|(alias, _)| *alias == name.as_str()) {
        return Some(*option);
    }

    // Otherwise accept only a unique full-parameter prefix. Multiple
    // candidates are ambiguous even when they have the same arity (`-no`
    // matches NoExit, NoLogo, NonInteractive, and NoProfile).
    let mut matches = all_options().filter(|(parameter, _)| parameter.starts_with(&name));
    let (_, option) = matches.next()?;
    matches.next().is_none().then_some(*option)
}

/// Walk host arguments with arity. A leftover positional is implicit `-File`.
#[cfg(any(target_os = "windows", test))]
fn pwsh_host_args_block_append(shell: WtDefaultShell, arguments: &[String]) -> bool {
    let mut index = 0usize;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--" {
            return true;
        }
        let Some(name) = ps_host_option_name(argument) else {
            return true;
        };
        match classify_ps_host_option(shell, name) {
            Some(PsHostOption::Flag) => index += 1,
            Some(PsHostOption::Value) => {
                if index + 1 >= arguments.len() {
                    return true;
                }
                index += 2;
            }
            Some(PsHostOption::Terminal) | None => return true,
        }
    }
    false
}

/// Existing terminal-action flags consume or reject extra arguments, so
/// `--appendCommandLine` must not be used.
#[cfg(any(target_os = "windows", test))]
fn wt_commandline_has_terminal_action(shell: WtDefaultShell, arguments: &str) -> bool {
    let Some(arguments) = split_wt_arguments(arguments) else {
        return true;
    };

    match shell {
        WtDefaultShell::Pwsh | WtDefaultShell::PowerShell => {
            pwsh_host_args_block_append(shell, &arguments)
        }
        WtDefaultShell::Cmd => arguments.iter().any(|argument| {
            let option = argument.to_ascii_lowercase();
            // CMD allows glued switches such as /D/C and /D/K; `/R` is an
            // undocumented alias for terminating `/C`. An action is not always
            // the first whitespace-separated token.
            option.contains("/c") || option.contains("/k") || option.contains("/r")
        }),
    }
}

/// Classify a supported shell from the executable name. An explicit commandline
/// is accepted only when extra arguments can be appended safely.
#[cfg(any(target_os = "windows", test))]
fn classify_wt_commandline(cmdline: &str) -> Option<WtDefaultShell> {
    let (executable, arguments) = split_wt_commandline(cmdline)?;
    let file_name = executable
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(executable)
        .to_ascii_lowercase();
    let base_name = file_name.strip_suffix(".exe").unwrap_or(&file_name);

    let shell = match base_name {
        "pwsh" => WtDefaultShell::Pwsh,
        "powershell" => WtDefaultShell::PowerShell,
        "cmd" => WtDefaultShell::Cmd,
        _ => return None,
    };

    (!wt_commandline_has_terminal_action(shell, arguments)).then_some(shell)
}

/// Callers filter defaults by WT version first, then classify the shell from an
/// explicit command, inherited command, dynamic source, or known GUID.
/// If a provided commandline cannot be appended to safely, refuse the profile
/// instead of guessing from GUID or name.
#[cfg(any(target_os = "windows", test))]
fn classify_wt_profile(
    profile: &serde_json::Value,
    inherited_commandline: Option<&serde_json::Value>,
) -> Option<WtDefaultShell> {
    if let Some(commandline) = profile.get("commandline").or(inherited_commandline) {
        return commandline.as_str().and_then(classify_wt_commandline);
    }

    // Dynamic profiles may omit an inspectable commandline; only known sources
    // with a fixed meaning are accepted.
    if let Some(source) = profile.get("source").and_then(|v| v.as_str()) {
        if source == "Windows.Terminal.PowershellCore" {
            return Some(WtDefaultShell::Pwsh);
        }

        // An unknown generator may already include a terminating action; do not
        // fall back to GUID metadata.
        return None;
    }

    if let Some(shell) = profile
        .get("guid")
        .and_then(|value| value.as_str())
        .and_then(|guid| known_wt_shell_from_normalized_guid(&normalize_wt_guid(guid)))
    {
        return Some(shell);
    }

    // Without a commandline, a known generator source, or a built-in GUID,
    // the display name is not evidence of the shell. WT's built-in profile
    // commandline defaults to cmd.exe, while a sparse user override may still
    // inherit an unknown dynamic/fragment commandline. Refuse to guess so the
    // caller uses the explicit cmd fallback.
    None
}

// WT hashes Windows wide strings, not UTF-8: changing the encoding would select
// a different profile even though the displayed name is identical.
#[cfg(any(target_os = "windows", test))]
fn wt_uuid_v5(namespace: uuid::Uuid, value: &str) -> uuid::Uuid {
    let bytes: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
    uuid::Uuid::new_v5(&namespace, &bytes)
}

/// Mirror Profile::_GenerateGuidForProfile for entries in the local settings file.
/// WT assigns these IDs before layering defaults. Fragment/generator profiles not
/// present in this file cannot be reconstructed from names alone.
/// https://github.com/microsoft/terminal/blob/v1.24.11911.0/src/cascadia/TerminalSettingsModel/Profile.cpp#L301-L319
#[cfg(any(target_os = "windows", test))]
fn wt_profile_guid(profile: &serde_json::Value) -> Option<uuid::Uuid> {
    if let Some(guid) = profile.get("guid").filter(|value| !value.is_null()) {
        return uuid::Uuid::parse_str(guid.as_str()?).ok();
    }
    let name = profile.get("name")?.as_str()?;
    let source = match profile.get("source") {
        None | Some(serde_json::Value::Null) => "",
        Some(source) => source.as_str()?,
    };
    let namespace = uuid::Uuid::from_u128(0xf65ddb7e_706b_4499_8a50_40313caf510a);
    let namespace = if source.is_empty() {
        namespace
    } else {
        wt_uuid_v5(namespace, source)
    };
    Some(wt_uuid_v5(namespace, name))
}

/// Parse Windows Terminal settings.json and keep the default profile selector
/// alongside its shell, so launch uses the same profile that was classified.
#[cfg(any(target_os = "windows", test))]
fn parse_wt_default_profile(
    content: &str,
    wt_version: Option<(u16, u16, u16, u16)>,
) -> Option<WtDefaultProfile> {
    // Windows Terminal settings.json commonly uses JSONC comments and trailing commas.
    let json: serde_json::Value = match json5::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            log::debug!("Failed to parse Windows Terminal settings.json: {e}");
            return None;
        }
    };

    // WT profile names are case- and whitespace-sensitive; compare GUIDs as UUIDs below.
    let default_profile = json.get("defaultProfile").and_then(|v| v.as_str())?;
    if default_profile.is_empty() {
        return None;
    }

    // Support modern `profiles.list` and the legacy flat `profiles` array.
    let profiles = json.get("profiles");
    // WT #19225 clears this field from 1.24; first public 1.24.2372.0 includes
    // that rule, while 1.23 still inherits it.
    let inherited_commandline = profiles
        .and_then(|profiles| profiles.get("defaults"))
        .and_then(|defaults| defaults.get("commandline"))
        .filter(|_| wt_version.is_none_or(|version| version < (1, 24, 0, 0)));
    let profile_list = profiles.and_then(|profiles| {
        profiles
            .get("list")
            .and_then(|list| list.as_array())
            .or_else(|| profiles.as_array())
    });

    // WT only attempts GUID lookup for the canonical braced form. An unbraced
    // UUID-shaped value is a profile name and must stay on the exact-name path.
    let is_braced_guid = default_profile.len() == 38
        && default_profile.starts_with('{')
        && default_profile.ends_with('}');
    let default_guid = is_braced_guid
        .then(|| uuid::Uuid::parse_str(default_profile).ok())
        .flatten();
    let default_norm_guid = default_guid
        .as_ref()
        .map(uuid::Uuid::to_string)
        .unwrap_or_default();

    if let Some(list) = profile_list {
        // WT looks up GUID across the whole table first and only then searches
        // by name, so an earlier same-name profile must not steal the match.
        let matched = default_guid
            .and_then(|expected| {
                list.iter().find_map(|item| {
                    let guid = wt_profile_guid(item)?;
                    (guid == expected).then_some((item, guid))
                })
            })
            .or_else(|| {
                list.iter().find_map(|item| {
                    let guid = wt_profile_guid(item)?;
                    item.get("name")
                        .and_then(|name| name.as_str())
                        .is_some_and(|name| name == default_profile)
                        .then_some((item, guid))
                })
            });

        if let Some((item, guid)) = matched {
            // When the load rule is unknown, only accept an explicit commandline;
            // do not guess whether defaults would override the shell.
            if wt_version.is_none()
                && inherited_commandline.is_some()
                && item.get("commandline").is_none()
            {
                log::debug!("Unknown WT version with inherited commandline; using explicit cmd");
                return None;
            }
            return Some(WtDefaultProfile {
                selector: format!("{{{guid}}}"),
                shell: classify_wt_profile(item, inherited_commandline)?,
            });
        }
    }

    // Unlisted built-in profiles use the same version rule; an unknown version
    // must not skip the ambiguity via GUID fallback.
    let shell = if let Some(commandline) = inherited_commandline {
        if wt_version.is_none() {
            log::debug!("Unknown WT version with inherited commandline; using explicit cmd");
            return None;
        }
        commandline.as_str().and_then(classify_wt_commandline)
    } else {
        // If the profile is not listed, fall back to well-known built-in GUIDs.
        known_wt_shell_from_normalized_guid(&default_norm_guid)
    }?;
    Some(WtDefaultProfile {
        selector: format!("{{{}}}", default_guid?),
        shell,
    })
}

#[cfg(any(target_os = "windows", test))]
const APPEXECLINK_REPARSE_TAG: u32 = 0x8000_001b;

/// Parse an APPEXECLINK buffer from `FSCTL_GET_REPARSE_POINT`.
/// The payload starts with a string count; the third NUL-terminated UTF-16
/// string is the real executable.
#[cfg(any(target_os = "windows", test))]
fn parse_app_execution_alias_target(buffer: &[u8]) -> Option<Vec<u16>> {
    let tag = u32::from_le_bytes(buffer.get(0..4)?.try_into().ok()?);
    if tag != APPEXECLINK_REPARSE_TAG {
        return None;
    }

    let payload_length = u16::from_le_bytes(buffer.get(4..6)?.try_into().ok()?) as usize;
    let payload_end = 8usize.checked_add(payload_length)?;
    let payload = buffer.get(8..payload_end)?;
    let string_count = u32::from_le_bytes(payload.get(0..4)?.try_into().ok()?);
    if string_count < 3 || (payload.len() - 4) % 2 != 0 {
        return None;
    }

    let strings: Vec<u16> = payload[4..]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    let mut start = 0usize;
    for index in 0..3 {
        let end = start + strings.get(start..)?.iter().position(|value| *value == 0)?;
        if index == 2 {
            let target = strings.get(start..end)?.to_vec();
            let drive = *target.first()?;
            let is_ascii_drive = (drive >= b'A' as u16 && drive <= b'Z' as u16)
                || (drive >= b'a' as u16 && drive <= b'z' as u16);
            let is_absolute_drive_path = target.len() >= 3
                && is_ascii_drive
                && target[1] == b':' as u16
                && target[2] == b'\\' as u16;
            return is_absolute_drive_path.then_some(target);
        }
        start = end + 1;
    }
    None
}

#[cfg(target_os = "windows")]
fn resolve_app_execution_alias_target(alias: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    let wide_path: Vec<u16> = alias.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: pointer comes from a local NUL-terminated UTF-16 buffer; remaining
    // arguments follow the CreateFileW contract.
    // FSCTL_GET_REPARSE_POINT needs GENERIC_READ; access=0 can fail to return
    // payload data on App Execution Aliases.
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }

    let mut buffer = vec![0u8; 16 * 1024];
    let mut returned = 0u32;
    // SAFETY: handle is valid, the output buffer is writable for the call, and
    // `returned` does not overlap the buffer.
    let succeeded = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_REPARSE_POINT,
            std::ptr::null(),
            0,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    // SAFETY: handle was created by the CreateFileW call above and is closed once.
    let _ = unsafe { CloseHandle(handle) };
    if succeeded == 0 {
        return None;
    }

    let target = parse_app_execution_alias_target(buffer.get(..returned as usize)?)?;
    Some(PathBuf::from(std::ffi::OsString::from_wide(&target)))
}

#[cfg(target_os = "windows")]
struct ResolvedWtInstallation {
    launcher: PathBuf,
    settings_executable: Option<PathBuf>,
}

#[cfg(target_os = "windows")]
const WT_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Locate the first `wt` that PATH would launch, and keep launcher plus settings target.
/// Uses the reconstructed GUI PATH; timeout falls through to the explicit cmd fallback.
#[cfg(target_os = "windows")]
fn resolve_wt_installation() -> Option<ResolvedWtInstallation> {
    let effective_path = effective_path_os().unwrap_or_default();
    let child = windows_path_lookup_command("wt", &effective_path)
        .spawn()
        .ok()?;
    let output = wait_child_output(
        child,
        CommandDeadline::from_timeout(Some(WT_LOOKUP_TIMEOUT)),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = decode_windows_where_output(&output.stdout);
    let launcher = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| PathBuf::from(line.trim_matches('"')))?;

    let is_store_alias = launcher
        .parent()
        .is_some_and(is_windows_app_execution_alias_dir);
    let settings_executable = if is_store_alias {
        resolve_app_execution_alias_target(&launcher)
    } else {
        Some(
            launcher
                .parent()
                .and_then(|parent| {
                    launcher
                        .file_stem()
                        .map(|stem| parent.join(format!("{}.shim", stem.to_string_lossy())))
                })
                .filter(|shim| shim.is_file())
                .and_then(|shim| parse_scoop_shim_target(&shim))
                .unwrap_or_else(|| launcher.clone()),
        )
    };

    Some(ResolvedWtInstallation {
        launcher,
        settings_executable,
    })
}

/// Walk `path` upward for the nearest Windows Terminal Store package under
/// `WindowsApps`; returns whether it is Preview and the lowercased package
/// directory name (which embeds the version). Directory-name substrings
/// elsewhere (GitHub zip extracts) must not match.
#[cfg(any(target_os = "windows", test))]
fn store_wt_package_dir(path: &Path) -> Option<(bool, String)> {
    path.ancestors().find_map(|directory| {
        let parent_name = directory.parent()?.file_name()?;
        if !parent_name
            .to_string_lossy()
            .eq_ignore_ascii_case("WindowsApps")
        {
            return None;
        }

        let package = directory
            .file_name()?
            .to_string_lossy()
            .to_ascii_lowercase();
        let preview = package.starts_with("microsoft.windowsterminalpreview_");
        if preview || package.starts_with("microsoft.windowsterminal_") {
            Some((preview, package))
        } else {
            None
        }
    })
}

/// Return the Scoop root and package directory name so `apps/<package>/current/wt.exe`
/// maps to the same install's `persist/<package>`, including custom Scoop roots.
#[cfg(any(target_os = "windows", test))]
fn scoop_wt_install_context(exe_path: &Path) -> Option<(PathBuf, String)> {
    let apps_dir = exe_path.ancestors().find(|ancestor| {
        ancestor
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("apps"))
    })?;
    let package = exe_path
        .strip_prefix(apps_dir)
        .ok()?
        .components()
        .next()?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    if !package.eq_ignore_ascii_case("windows-terminal")
        && !package.eq_ignore_ascii_case("windows-terminal-preview")
    {
        return None;
    }

    Some((apps_dir.parent()?.to_path_buf(), package))
}

/// Parse `major.minor.patch.revision` so Store package folders and Scoop
/// versioned app directories can be compared against WT 1.19.
#[cfg(any(target_os = "windows", test))]
fn parse_wt_dotted_version(value: &str) -> Option<(u16, u16, u16, u16)> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    let revision = parts.next()?.parse().ok()?;
    parts
        .next()
        .is_none()
        .then_some((major, minor, patch, revision))
}

#[cfg(any(target_os = "windows", test))]
fn wt_version_supports_append(version: (u16, u16, u16, u16)) -> bool {
    (version.0, version.1) >= (1, 19)
}

/// Store package folders and Scoop versioned directories embed the WT version.
/// `current` shims and unpackaged layouts have to use the PE version instead.
#[cfg(any(target_os = "windows", test))]
fn wt_version_from_path(path: &Path) -> Option<(u16, u16, u16, u16)> {
    if let Some((_, package)) = store_wt_package_dir(path) {
        let rest = package
            .strip_prefix("microsoft.windowsterminalpreview_")
            .or_else(|| package.strip_prefix("microsoft.windowsterminal_"))?;
        return parse_wt_dotted_version(rest.split('_').next()?);
    }

    // Scoop layout: `<root>/apps/<package>/<version>/…`; `current` is a shim
    // whose version lives in the PE, not the directory name.
    let (root, _) = scoop_wt_install_context(path)?;
    let mut segments = path.strip_prefix(root.join("apps")).ok()?.components();
    let version = segments.nth(1)?.as_os_str().to_string_lossy();
    parse_wt_dotted_version(&version)
}

#[cfg(any(target_os = "windows", test))]
fn append_scoop_wt_settings_paths(paths: &mut Vec<PathBuf>, root: &Path, package: &str) {
    let persist = root.join("persist").join(package);
    // Scoop persists a settings subdirectory; files at the persist root are not
    // the active WT config.
    push_unique_path(paths, persist.join("settings").join("settings.json"));
}

#[cfg(any(target_os = "windows", test))]
fn store_wt_settings_path(local: &Path, preview: bool) -> PathBuf {
    let package = if preview {
        "Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe"
    } else {
        "Microsoft.WindowsTerminal_8wekyb3d8bbwe"
    };
    local
        .join("Packages")
        .join(package)
        .join("LocalState")
        .join("settings.json")
}

#[cfg(any(target_os = "windows", test))]
fn unpackaged_wt_settings_path(local: &Path) -> PathBuf {
    local
        .join("Microsoft")
        .join("Windows Terminal")
        .join("settings.json")
}

/// When WT places `.portable` beside the executable, only use sibling `settings/`
/// and do not read LocalAppData.
#[cfg(any(target_os = "windows", test))]
fn is_wt_portable_install(exe_path: &Path) -> bool {
    exe_path
        .parent()
        .is_some_and(|dir| dir.join(".portable").is_file())
}

/// Return settings paths for the resolved executable so leftover configs from
/// other installs cannot participate in mtime ranking.
/// Sibling and Scoop persist paths are only live while `.portable` exists;
/// otherwise WT reads `%LOCALAPPDATA%\Microsoft\Windows Terminal`.
#[cfg(any(target_os = "windows", test))]
fn build_wt_settings_paths(
    settings_executable: &Path,
    local_appdata: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if is_wt_portable_install(settings_executable) {
        if let Some(dir) = settings_executable.parent() {
            // Portable mode only reads the settings subdirectory next to the
            // module; leftover files at the root must not enter the sort.
            push_unique_path(&mut paths, dir.join("settings").join("settings.json"));
        }
        if let Some((root, package)) = scoop_wt_install_context(settings_executable) {
            append_scoop_wt_settings_paths(&mut paths, &root, &package);
        }
        return paths;
    }

    if let Some((preview, _)) = store_wt_package_dir(settings_executable) {
        if let Some(local) = local_appdata {
            push_unique_path(&mut paths, store_wt_settings_path(local, preview));
        }
    } else if let Some(local) = local_appdata {
        push_unique_path(&mut paths, unpackaged_wt_settings_path(local));
    }

    paths
}

/// Parse the real executable path from a Scoop `.shim`.
#[cfg(any(target_os = "windows", test))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_scoop_shim_target(shim_path: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(shim_path).ok()?;
    parse_scoop_shim_content(&content)
}

#[cfg(any(target_os = "windows", test))]
fn parse_scoop_shim_content(content: &str) -> Option<PathBuf> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("path =") {
            let path_str = rest.trim().trim_matches('"');
            if !path_str.is_empty() {
                return Some(PathBuf::from(path_str));
            }
        }
    }
    None
}

/// Candidates all belong to the same resolved install, so mtime can pick the
/// active file. A parse failure continues to the next candidate of that install
/// and never crosses Store/Scoop/Preview installs.
#[cfg(any(target_os = "windows", test))]
fn detect_wt_default_profile_from_paths(
    paths: &[PathBuf],
    wt_version: Option<(u16, u16, u16, u16)>,
) -> Option<WtDefaultProfile> {
    let mut existing = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        let normalized = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(normalized) || !path.is_file() {
            continue;
        }
        let modified = std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        existing.push((path, modified));
    }
    existing.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));

    for (path, _) in existing {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Some(profile) = parse_wt_default_profile(&content, wt_version) {
                return Some(profile);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn detect_wt_default_profile(
    installation: &ResolvedWtInstallation,
    wt_version: Option<(u16, u16, u16, u16)>,
) -> Option<WtDefaultProfile> {
    let settings_executable = installation.settings_executable.as_deref()?;
    let local_appdata = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs::data_local_dir);
    let paths = build_wt_settings_paths(settings_executable, local_appdata.as_deref());
    let profile = detect_wt_default_profile_from_paths(&paths, wt_version);
    if let Some(profile) = &profile {
        log::debug!(
            "Detected Windows Terminal default profile: {:?} (candidates: {:?})",
            profile,
            paths
        );
    }
    profile
}

#[cfg(target_os = "windows")]
fn wt_file_version(path: &Path) -> Option<(u16, u16, u16, u16)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut dummy = 0u32;
    // SAFETY: `wide` is a local NUL-terminated path buffer.
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), &mut dummy) };
    if size == 0 {
        return None;
    }

    let mut buffer = vec![0u8; size as usize];
    // SAFETY: `buffer` is writable for `size` bytes returned above.
    let ok = unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, buffer.as_mut_ptr().cast()) };
    if ok == 0 {
        return None;
    }

    let mut info: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut info_len = 0u32;
    let subblock: Vec<u16> = "\\\0".encode_utf16().collect();
    // SAFETY: `buffer` still owns the version block; `info`/`info_len` are out params.
    let ok = unsafe {
        VerQueryValueW(
            buffer.as_ptr().cast(),
            subblock.as_ptr(),
            &mut info,
            &mut info_len,
        )
    };
    if ok == 0 || info.is_null() || (info_len as usize) < 16 {
        return None;
    }

    // VS_FIXEDFILEINFO: signature, struct version, then fileVersionMS/LS.
    let bytes = unsafe { std::slice::from_raw_parts(info as *const u8, info_len as usize) };
    let ms = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?);
    let ls = u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?);
    Some((
        (ms >> 16) as u16,
        (ms & 0xffff) as u16,
        (ls >> 16) as u16,
        (ls & 0xffff) as u16,
    ))
}

#[cfg(target_os = "windows")]
fn wt_version_from_executable(path: &Path) -> Option<(u16, u16, u16, u16)> {
    wt_version_from_path(path)
        .or_else(|| wt_file_version(path))
        .or_else(|| {
            let sibling = path.parent()?.join("WindowsTerminal.exe");
            if sibling == path || !sibling.is_file() {
                None
            } else {
                wt_file_version(&sibling)
            }
        })
}

/// Read the version from the same install's real target or launcher so config
/// load rules and append support share one source.
/// GUI WT help text is unreliable, so use path and PE version info instead of
/// launching the terminal to probe.
#[cfg(target_os = "windows")]
fn wt_installation_version(installation: &ResolvedWtInstallation) -> Option<(u16, u16, u16, u16)> {
    installation
        .settings_executable
        .as_deref()
        .filter(|path| *path != installation.launcher.as_path())
        .and_then(wt_version_from_executable)
        .or_else(|| wt_version_from_executable(&installation.launcher))
}

/// The launcher path may contain cmd metacharacters, so spawn it directly
/// instead of going through `cmd /C start`.
/// WT may already be a resident GUI process, so report only spawn failure and
/// return without waiting for the window to close.
#[cfg(target_os = "windows")]
fn run_wt_command(launcher: &str, args: &[&str], bat_path: &str) -> Result<(), String> {
    let mut command = std::process::Command::new(launcher);
    configure_windows_batch_env(&mut command, bat_path);
    command
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动 Windows Terminal 失败: {e}"))
}

/// Launch the same resolved Windows Terminal, and fall back to an explicit cmd
/// tab when the default profile cannot be recognized safely or WT is too old
/// for `--appendCommandLine`.
#[cfg(target_os = "windows")]
pub(super) fn launch_wt_terminal(bat_path: &str, ps_cmd: &str) -> Result<(), String> {
    let installation = resolve_wt_installation();
    let wt_command = installation
        .as_ref()
        .map(|installation| installation.launcher.to_string_lossy().into_owned())
        .unwrap_or_else(|| "wt".to_string());
    let wt_version = installation.as_ref().and_then(wt_installation_version);
    let profile = installation
        .as_ref()
        .and_then(|installation| detect_wt_default_profile(installation, wt_version));
    // Keep the previous append policy for unknown versions; configs that depend
    // on ambiguous defaults are refused by the parser.
    let supports_append = profile.is_some() && wt_version.is_none_or(wt_version_supports_append);
    let args = select_wt_launch_args(profile.as_ref(), supports_append, ps_cmd);
    run_wt_command(&wt_command, &args, bat_path)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::super::misc::{
        configure_windows_batch_env, configure_windows_cmd_batch, quote_windows_batch_path_for_env,
        PARENT_CLAUDE_SESSION_ENV_VARS, WINDOWS_BATCH_PATH_ENV, WINDOWS_POWERSHELL_BATCH_COMMAND,
    };
    use super::super::misc::{decode_command_output, escape_windows_batch_value};
    use super::*;
    use std::path::{Path, PathBuf};

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

    fn extend_expected_cmd_batch_args(args: &mut Vec<&str>) {
        args.extend(["/D", "/V:OFF", "/K", WINDOWS_BATCH_PATH_COMMAND]);
    }

    /// Cover JSONC, well-known GUIDs, quoted commandlines, and name matches.
    #[test]
    fn parse_wt_default_profile_recognizes_supported_profiles() {
        // JSONC comments + PowerShell 7 well-known GUID
        let jsonc_pwsh = r#"{
            // Windows Terminal single line comment
            "defaultProfile": "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
            /* block comment */
            "profiles": {
                "list": [
                    {
                        "guid": "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
                        "name": "PowerShell",
                        "source": "Windows.Terminal.PowershellCore",
                    },
                ],
            },
        }"#;
        assert_eq!(
            parse_wt_default_profile(jsonc_pwsh, None).map(|profile| profile.shell),
            Some(WtDefaultShell::Pwsh)
        );

        // Absolute commandline path for Windows PowerShell
        let powershell_cmdline = r#"{
            "defaultProfile": "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}",
            "profiles": {
                "list": [
                    {
                        "guid": "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}",
                        "name": "Windows PowerShell",
                        "commandline": "%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
                    }
                ]
            }
        }"#;
        assert_eq!(
            parse_wt_default_profile(powershell_cmdline, None).map(|profile| profile.shell),
            Some(WtDefaultShell::PowerShell)
        );

        // Quoted custom pwsh path with arguments
        let quoted_pwsh = r#"{
            "defaultProfile": "{11111111-1111-4111-8111-111111111111}",
            "profiles": [
                {
                    "guid": "{11111111-1111-4111-8111-111111111111}",
                    "name": "Custom",
                    "commandline": "\"C:\\Program Files\\PowerShell\\7\\pwsh.Exe\" -NoLogo"
                }
            ]
        }"#;
        assert_eq!(
            parse_wt_default_profile(quoted_pwsh, None).map(|profile| profile.shell),
            Some(WtDefaultShell::Pwsh)
        );

        // cmd.exe
        let cmd_config = r#"{
            "defaultProfile": "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}",
            "profiles": {
                "list": [
                    {
                        "guid": "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}",
                        "name": "Command Prompt",
                        "commandline": "cmd.exe"
                    }
                ]
            }
        }"#;
        assert_eq!(
            parse_wt_default_profile(cmd_config, None).map(|profile| profile.shell),
            Some(WtDefaultShell::Cmd)
        );

        // Match by profile name instead of GUID
        let name_config = r#"{
            "defaultProfile": "PowerShell 7",
            "profiles": {
                "list": [
                    {
                        "guid": "{22222222-2222-4222-8222-222222222222}",
                        "name": "PowerShell 7",
                        "commandline": "pwsh.exe"
                    }
                ]
            }
        }"#;
        assert_eq!(
            parse_wt_default_profile(name_config, None).map(|profile| profile.shell),
            Some(WtDefaultShell::Pwsh)
        );
    }

    #[test]
    fn name_only_profiles_do_not_imply_a_shell() {
        for name in [
            "pwsh custom",
            "PowerShell 7 custom",
            "Windows PowerShell custom",
            "Command Prompt custom",
        ] {
            for version in [
                None,
                Some((1, 18, 0, 0)),
                Some((1, 19, 0, 0)),
                Some((1, 23, 0, 0)),
                Some((1, 24, 0, 0)),
            ] {
                let settings = serde_json::json!({
                    "defaultProfile": "{77777777-7777-4777-8777-777777777777}",
                    "profiles": {"list": [{
                        "guid": "{77777777-7777-4777-8777-777777777777}",
                        "name": name
                    }]}
                });
                let profile = parse_wt_default_profile(&settings.to_string(), version);
                assert_eq!(
                    profile, None,
                    "a display name is not shell evidence: {name:?}, version {version:?}"
                );
                assert_eq!(
                    select_wt_launch_args(
                        profile.as_ref(),
                        version.is_some_and(wt_version_supports_append),
                        "& 'probe.bat'",
                    ),
                    build_wt_cmd_fallback_args(),
                    "an ambiguous profile must use the explicit cmd fallback"
                );
            }
        }
    }

    #[test]
    fn unbraced_default_profile_uuid_is_a_name_not_a_guid() {
        let uuid_name = "574e775e-4f2a-5b96-ac1e-a2962a402336";
        let named_profile_guid = "{77777777-7777-4777-8777-777777777777}";
        let settings = serde_json::json!({
            "defaultProfile": uuid_name,
            "profiles": {"list": [
                {
                    "guid": "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
                    "name": "Actual PowerShell profile",
                    "source": "Windows.Terminal.PowershellCore"
                },
                {
                    "guid": named_profile_guid,
                    "name": uuid_name,
                    "commandline": "cmd.exe"
                }
            ]}
        });

        let profile = parse_wt_default_profile(&settings.to_string(), Some((1, 24, 0, 0)))
            .expect("the UUID-shaped profile name should be resolved");
        assert_eq!(profile.selector, named_profile_guid);
        assert_eq!(profile.shell, WtDefaultShell::Cmd);

        let unlisted = serde_json::json!({"defaultProfile": uuid_name});
        assert_eq!(
            parse_wt_default_profile(&unlisted.to_string(), Some((1, 24, 0, 0))),
            None,
            "an unbraced UUID-shaped name must not enter the built-in GUID fallback"
        );
    }

    #[test]
    fn parse_wt_default_profile_returns_none_for_unsupported_or_malformed() {
        let wsl = r#"{"defaultProfile": "{55555555-5555-4555-8555-555555555555}","profiles":{"list":[{"guid":"{55555555-5555-4555-8555-555555555555}","commandline":"wsl.exe"}]}}"#;
        assert_eq!(parse_wt_default_profile(wsl, None), None);

        let overridden_builtin = r#"{
            "defaultProfile": "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
            "profiles": {
                "list": [{
                    "guid": "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
                    "name": "PowerShell",
                    "source": "Windows.Terminal.PowershellCore",
                    "commandline": "wsl.exe"
                }]
            }
        }"#;
        assert_eq!(parse_wt_default_profile(overridden_builtin, None), None);
        assert_eq!(parse_wt_default_profile("not valid json", None), None);
        assert_eq!(
            parse_wt_default_profile(r#"{"defaultProfile": ""}"#, None),
            None
        );
    }

    #[test]
    fn third_party_powershellcore_source_uses_explicit_fallback() {
        let settings = serde_json::json!({
            "defaultProfile": "{55555555-5555-4555-8555-555555555555}",
            "profiles": {"list": [{
                "guid": "{55555555-5555-4555-8555-555555555555}",
                "source": "Contoso.PowerShellCore.Wsl"
            }]}
        });

        let profile = parse_wt_default_profile(&settings.to_string(), None);
        assert_eq!(
            profile, None,
            "third-party sources containing PowerShellCore must not be classified as pwsh"
        );
        assert_eq!(
            select_wt_launch_args(profile.as_ref(), true, "& 'probe.bat'"),
            build_wt_cmd_fallback_args(),
            "unknown dynamic profiles must use the explicit cmd fallback"
        );
    }

    #[test]
    fn visual_studio_dynamic_profiles_use_explicit_fallback() {
        for (guid, name) in [
            (
                "{11111111-1111-4111-8111-111111111111}",
                "Developer PowerShell for VS 2022",
            ),
            (
                "{22222222-2222-4222-8222-222222222222}",
                "Developer Command Prompt for VS 2022",
            ),
        ] {
            let settings = serde_json::json!({
                "defaultProfile": guid,
                "profiles": {"list": [{
                    "guid": guid,
                    "name": name,
                    "source": "Windows.Terminal.VisualStudio"
                }]}
            });

            let profile = parse_wt_default_profile(&settings.to_string(), None);
            assert_eq!(
                profile, None,
                "dynamic profile {name} must not be guessed by name"
            );
            assert_eq!(
                select_wt_launch_args(profile.as_ref(), true, "& 'probe.bat'"),
                build_wt_cmd_fallback_args(),
                "dynamic profile {name} must use the explicit cmd fallback"
            );
        }

        let explicit_commandline = serde_json::json!({
            "defaultProfile": "{33333333-3333-4333-8333-333333333333}",
            "profiles": {"list": [{
                "guid": "{33333333-3333-4333-8333-333333333333}",
                "name": "Developer PowerShell for VS 2022",
                "source": "Windows.Terminal.VisualStudio",
                "commandline": "pwsh.exe -NoLogo"
            }]}
        });
        assert_eq!(
            parse_wt_default_profile(&explicit_commandline.to_string(), None)
                .map(|profile| profile.shell),
            Some(WtDefaultShell::Pwsh),
            "an explicit safe commandline must take precedence over the dynamic source"
        );
    }

    /// Server mode runs the remoting protocol and ignores `-Command`; leftover
    /// positionals mean implicit `-File`. Append must be refused for all of
    /// these, including the ServerMode prefixes `-s`/`-se` that PowerShell
    /// resolves over `-Sta`/`-SettingsFile`.
    #[test]
    fn wt_commandline_classification_accepts_safe_and_refuses_unsafe() {
        let refused: &[&str] = &[
            r#"pwsh.exe -NoLogo -Fil "C:\init.ps1""#,
            r#"cmd.exe /D /K C:\init.cmd"#,
            r#"cmd.exe /R echo ORIGINAL"#,
            r#"cmd.exe /D/R echo ORIGINAL"#,
            r#"cmd.exe /cdir"#,
            r#"cmd.exe /D/C echo ORIGINAL"#,
            r#"cmd.exe /d/k echo ORIGINAL"#,
            r#"cmd.exe /Q/D/CECHO ORIGINAL"#,
            r#"cmd.exe /D/V:OFF/Kecho ORIGINAL"#,
            r#"pwsh.exe -no"#,
            r#"powershell.exe -no"#,
            r#"pwsh.exe -SSHServerMode"#,
            r#"pwsh.exe -s -NoProfile"#,
            r#"pwsh.exe -se -NoProfile"#,
            r#"pwsh.exe -ServerMode"#,
            r#"powershell.exe -s -NoProfile"#,
            r#"powershell.exe -se -NoProfile"#,
            r#"powershell.exe -ServerMode"#,
            r#"pwsh.exe C:\init.ps1"#,
            r#"pwsh.exe -f C:\init.ps1"#,
            r#"powershell.exe -c Get-Date"#,
            r#"pwsh.exe -Version"#,
            r#"powershell.exe -Version"#,
            r#"pwsh.exe -WorkingDirectory "C:\missing"#,
        ];
        for cmdline in refused {
            assert_eq!(classify_wt_commandline(cmdline), None, "{cmdline}");
        }

        let accepted: &[(&str, WtDefaultShell)] = &[
            (r#"cmd.exe /D /T:0C"#, WtDefaultShell::Cmd),
            (r#"cmd.exe /D/Q/V:OFF"#, WtDefaultShell::Cmd),
            (r#"pwsh.exe -EncodedArguments ABC="#, WtDefaultShell::Pwsh),
            (r#"pwsh.exe -ea ABC="#, WtDefaultShell::Pwsh),
            (r#"pwsh.exe -encodeda ABC="#, WtDefaultShell::Pwsh),
            (
                r#"powershell.exe -EncodedArguments ABC="#,
                WtDefaultShell::PowerShell,
            ),
            (r#"pwsh.exe -Login -NoLogo"#, WtDefaultShell::Pwsh),
            (
                r#"pwsh.exe -Interactive -NoProfileLoadTime"#,
                WtDefaultShell::Pwsh,
            ),
            (
                r#"pwsh.exe -SettingsFile C:\x.json -NoLogo"#,
                WtDefaultShell::Pwsh,
            ),
            (r#"pwsh.exe -wd C:\Work -nol"#, WtDefaultShell::Pwsh),
            (r#"pwsh.exe -i"#, WtDefaultShell::Pwsh),
            (r#"powershell.exe -ep Bypass"#, WtDefaultShell::PowerShell),
            (
                r#"powershell.exe -if Text -of Text"#,
                WtDefaultShell::PowerShell,
            ),
            (r#"powershell.exe -ea ABC="#, WtDefaultShell::PowerShell),
            (
                r#"pwsh.exe -ConfigurationFile C:\x.pssc"#,
                WtDefaultShell::Pwsh,
            ),
            (
                r#"powershell.exe -Version 2.0 -NoLogo"#,
                WtDefaultShell::PowerShell,
            ),
            (r#"pwsh.exe -NoLogo -NoExit"#, WtDefaultShell::Pwsh),
            (
                r#"pwsh.exe -WorkingDirectory "C:\Work\My -File Project" -NoLogo"#,
                WtDefaultShell::Pwsh,
            ),
            // Quoted executable with a space and no arguments appends safely.
            (r#""C:\missing quote\pwsh.exe""#, WtDefaultShell::Pwsh),
        ];
        for (cmdline, expected) in accepted {
            assert_eq!(
                classify_wt_commandline(cmdline),
                Some(*expected),
                "{cmdline}"
            );
        }

        // Copied-from-docs Unicode dashes (U+2013/2014/2015) and their
        // doubled long-option spelling parse like ASCII `-`.
        for dash in ['\u{2013}', '\u{2014}', '\u{2015}'] {
            let doubled = format!("pwsh.exe {dash}{dash}NoLogo");
            assert_eq!(
                classify_wt_commandline(&format!("pwsh.exe {dash}NoLogo")),
                Some(WtDefaultShell::Pwsh),
                "dash {dash:?}"
            );
            assert_eq!(
                classify_wt_commandline(&doubled),
                Some(WtDefaultShell::Pwsh),
                "doubled dash {dash:?}"
            );
            assert_eq!(
                classify_wt_commandline(&format!("powershell.exe {dash}NoLogo")),
                Some(WtDefaultShell::PowerShell),
                "dash {dash:?}"
            );
        }
    }

    #[test]
    fn wt_cmd_concatenated_actions_use_explicit_fallback() {
        let settings = serde_json::json!({
            "defaultProfile": "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}",
            "profiles": {"list": [{
                "guid": "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}",
                "commandline": "cmd.exe /D/C echo ORIGINAL"
            }]}
        });
        let profile = parse_wt_default_profile(&settings.to_string(), None);
        assert_eq!(
            select_wt_launch_args(profile.as_ref(), true, "& 'probe.bat'"),
            build_wt_cmd_fallback_args()
        );

        #[cfg(target_os = "windows")]
        {
            // Pin CMD dialect with a real shell: a glued /K after /C is text of
            // the original command and does not run a second batch.
            let output = std::process::Command::new("cmd.exe")
                .args(["/d/c", "echo", "ORIGINAL", "/K", "echo", "APPENDED"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .expect("CMD concatenated switch probe should start");
            assert!(output.status.success());
            assert_eq!(
                decode_command_output(&output.stdout).trim(),
                "ORIGINAL /K echo APPENDED"
            );
        }
    }

    #[test]
    fn parse_wt_default_profile_inherits_profiles_defaults_commandline() {
        let defaults_wsl = r#"{
            "defaultProfile": "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
            "profiles": {
                "defaults": { "commandline": "wsl.exe" },
                "list": [{
                    "guid": "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
                    "name": "PowerShell",
                    "source": "Windows.Terminal.PowershellCore"
                }]
            }
        }"#;
        assert_eq!(
            parse_wt_default_profile(defaults_wsl, Some((1, 23, 20211, 0))),
            None
        );

        let defaults_file = r#"{
            "defaultProfile": "{44444444-4444-4444-8444-444444444444}",
            "profiles": {
                "defaults": { "commandline": "pwsh.exe -File C:\\init.ps1" },
                "list": [{ "guid": "{44444444-4444-4444-8444-444444444444}", "name": "Custom" }]
            }
        }"#;
        assert_eq!(
            parse_wt_default_profile(defaults_file, Some((1, 23, 20211, 0))),
            None
        );

        let defaults_pwsh = r#"{
            "defaultProfile": "{44444444-4444-4444-8444-444444444444}",
            "profiles": {
                "defaults": { "commandline": "pwsh.exe -NoLogo" },
                "list": [{ "guid": "{44444444-4444-4444-8444-444444444444}", "name": "Custom" }]
            }
        }"#;
        assert_eq!(
            parse_wt_default_profile(defaults_pwsh, Some((1, 23, 20211, 0)))
                .map(|profile| profile.shell),
            Some(WtDefaultShell::Pwsh)
        );

        let profile_overrides_defaults = r#"{
            "defaultProfile": "{44444444-4444-4444-8444-444444444444}",
            "profiles": {
                "defaults": { "commandline": "wsl.exe" },
                "list": [{
                    "guid": "{44444444-4444-4444-8444-444444444444}",
                    "name": "Custom",
                    "commandline": "cmd.exe"
                }]
            }
        }"#;
        assert_eq!(
            parse_wt_default_profile(profile_overrides_defaults, Some((1, 23, 20211, 0)))
                .map(|profile| profile.shell),
            Some(WtDefaultShell::Cmd)
        );

        let unlisted_builtin_with_defaults = r#"{
            "defaultProfile": "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
            "profiles": {
                "defaults": { "commandline": "wsl.exe" },
                "list": []
            }
        }"#;
        assert_eq!(
            parse_wt_default_profile(unlisted_builtin_with_defaults, Some((1, 23, 20211, 0))),
            None
        );
    }

    #[test]
    fn wt_default_profile_uses_versioned_defaults_commandline() {
        let guid = "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}";
        let temp = tempfile::tempdir().expect("settings directory should be created");
        let path = temp.path().join("settings.json");
        for (version, expected_shell) in [
            (Some((1, 19, 10821, 0)), Some(WtDefaultShell::Pwsh)),
            (Some((1, 22, 10352, 0)), Some(WtDefaultShell::Pwsh)),
            (Some((1, 23, 20211, 0)), Some(WtDefaultShell::Pwsh)),
            (Some((1, 24, 2372, 0)), Some(WtDefaultShell::Cmd)),
            (Some((1, 24, 11911, 0)), Some(WtDefaultShell::Cmd)),
            (Some((1, 25, 1912, 0)), Some(WtDefaultShell::Cmd)),
            (None, None),
        ] {
            for listed in [true, false] {
                let mut profiles = Vec::new();
                if listed {
                    profiles.push(serde_json::json!({"guid": guid, "name": "Command Prompt"}));
                }
                let settings = serde_json::json!({
                    "defaultProfile": guid,
                    "profiles": {"defaults": {"commandline": "pwsh.exe"}, "list": profiles}
                });
                std::fs::write(&path, settings.to_string()).expect("settings should be written");
                let profile =
                    detect_wt_default_profile_from_paths(std::slice::from_ref(&path), version);
                assert_eq!(
                    profile,
                    expected_shell.map(|shell| WtDefaultProfile {
                        selector: guid.to_string(),
                        shell
                    }),
                    "version={version:?}, listed={listed}"
                );
                let mut expected_args = vec!["new-tab"];
                if let Some(shell) = expected_shell {
                    expected_args.extend(["--profile", guid, "--appendCommandLine", "--"]);
                    match shell {
                        WtDefaultShell::Cmd => extend_expected_cmd_batch_args(&mut expected_args),
                        _ => expected_args.extend(["-NoExit", "-Command", "& 'probe.bat'"]),
                    }
                } else {
                    expected_args.push("cmd");
                    extend_expected_cmd_batch_args(&mut expected_args);
                }
                assert_eq!(
                    select_wt_launch_args(profile.as_ref(), true, "& 'probe.bat'"),
                    expected_args,
                    "version={version:?}, listed={listed}"
                );
            }
        }
    }

    #[test]
    fn wt_defaults_version_does_not_override_explicit_commandline() {
        let guid = "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}";
        for version in [None, Some((1, 23, 20211, 0)), Some((1, 24, 11911, 0))] {
            for defaults in ["pwsh.exe", "wsl.exe"] {
                for explicit in ["cmd.exe", "wsl.exe"] {
                    let settings = serde_json::json!({
                        "defaultProfile": guid,
                        "profiles": {
                            "defaults": {"commandline": defaults},
                            "list": [{"guid": guid, "commandline": explicit}]
                        }
                    });
                    assert_eq!(
                        parse_wt_default_profile(&settings.to_string(), version),
                        (explicit == "cmd.exe").then(|| WtDefaultProfile {
                            selector: guid.to_string(),
                            shell: WtDefaultShell::Cmd,
                        }),
                        "version={version:?}, defaults={defaults}, explicit={explicit}"
                    );
                }
            }
        }
    }

    #[test]
    fn wt_profile_guids_follow_wt_utf16_namespace_rules() {
        // Fixed vector from WT's src/types/ut_types/UuidTests.cpp, not computed
        // with the implementation under test. UTF-8 gives a different UUID.
        let namespace = uuid::Uuid::parse_str("ad56de9e-5167-41b6-80eb-fb19f7927d1a").unwrap();
        assert_eq!(
            wt_uuid_v5(namespace, "testing").to_string(),
            "e04fb1f7-739d-5d63-bb18-e0ea00b19ee8"
        );
        // Profile vectors independently calculated from WT's published algorithm.
        for (profile, expected) in [
            (
                serde_json::json!({"name": "profile0"}),
                "690955e2-dbc2-509c-b36d-ac466fdda656",
            ),
            (
                serde_json::json!({"name": "profile0", "source": ""}),
                "690955e2-dbc2-509c-b36d-ac466fdda656",
            ),
            (
                serde_json::json!({"name": "profile0", "source": "Terminal.App.UnitTest.0"}),
                "52b9372a-c1b1-57eb-a9ce-e2cdc8d52ceb",
            ),
            (
                serde_json::json!({"name": "Profile0"}),
                "6a2f325d-dd64-5214-ac09-ceac61244422",
            ),
            (
                serde_json::json!({"name": "开发 🚀"}),
                "513625d0-fe16-526d-b8b5-494277269c46",
            ),
            (
                serde_json::json!({"name": "Dev&echo"}),
                "f7143a97-a88e-5523-94a1-19c0c687e353",
            ),
            (
                serde_json::json!({"name": "Dev%COMSPEC%"}),
                "46d028dc-f2b0-526e-9427-7bf1d8c7070d",
            ),
            (
                serde_json::json!({"name": "Dev;Test"}),
                "c0bfcc63-c933-5d17-9a24-892f27f2f34c",
            ),
            (
                serde_json::json!({"guid": "{574E775E-4F2A-5B96-AC1E-A2962A402336}", "name": "Dev&echo"}),
                "574e775e-4f2a-5b96-ac1e-a2962a402336",
            ),
        ] {
            assert_eq!(
                wt_profile_guid(&profile)
                    .map(|guid| guid.to_string())
                    .as_deref(),
                Some(expected),
                "{profile}"
            );
        }
        for profile in [
            serde_json::json!({}),
            serde_json::json!({"source": "Windows.Terminal.PowershellCore"}),
            serde_json::json!({"name": "Dev", "guid": "Dev&echo"}),
            serde_json::json!({"name": "Dev", "source": false}),
        ] {
            assert_eq!(wt_profile_guid(&profile), None, "{profile}");
        }
    }

    #[test]
    fn wt_default_profile_name_collisions_bind_the_exact_profile() {
        // This fixture inherits defaults; use the legacy rule so selector matching
        // is isolated from versioned default loading.
        // Explicit GUIDs isolate selector matching from GUID generation.
        let expected = "{22222222-2222-4222-8222-222222222222}";
        let mut failures = Vec::new();
        for (first_name, exact_name) in [
            ("dev", "Dev"),
            (" Dev", "Dev"),
            ("Dev ", "Dev"),
            ("Dev", " Dev"),
            ("Dev", "Dev "),
        ] {
            for include_decoy in [false, true] {
                let mut profiles = vec![serde_json::json!({"guid": expected, "name": exact_name})];
                if include_decoy {
                    profiles.insert(
                        0,
                        serde_json::json!({
                            "guid": "{11111111-1111-4111-8111-111111111111}",
                            "name": first_name
                        }),
                    );
                }
                let settings = serde_json::json!({
                    "defaultProfile": exact_name,
                    "profiles": {
                        "defaults": {"commandline": "cmd.exe"},
                        "list": profiles
                    }
                });
                let profile =
                    parse_wt_default_profile(&settings.to_string(), Some((1, 23, 20211, 0)));
                let args = select_wt_launch_args(profile.as_ref(), true, "& 'probe.bat'");
                let actual = args
                    .windows(2)
                    .find(|pair| pair[0] == "--profile")
                    .map(|pair| pair[1]);
                assert_eq!(
                    actual,
                    profile.as_ref().map(|profile| profile.selector.as_str()),
                    "argument generation must preserve the parsed selector"
                );
                if actual != Some(expected) {
                    failures.push(format!(
                        "names={first_name:?}/{exact_name:?}, default={exact_name:?}, decoy={include_decoy}: expected {expected}, got {actual:?}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn wt_default_profile_guid_precedes_name_collisions() {
        let cmd_guid = "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}";
        let pwsh_guid = "{574e775e-4f2a-5b96-ac1e-a2962a402336}";
        for selector in [cmd_guid.to_string(), cmd_guid.to_ascii_uppercase()] {
            for reverse in [false, true] {
                for commandline in ["cmd.exe", "wsl.exe"] {
                    let mut profiles = vec![
                        serde_json::json!({"guid": pwsh_guid, "name": selector, "commandline": "pwsh.exe"}),
                        serde_json::json!({"guid": cmd_guid, "name": "Command Prompt", "commandline": commandline}),
                    ];
                    if reverse {
                        profiles.reverse();
                    }
                    let settings = serde_json::json!({
                        "defaultProfile": selector,
                        "profiles": {"list": profiles}
                    });
                    let expected = (commandline == "cmd.exe").then(|| WtDefaultProfile {
                        selector: cmd_guid.to_string(),
                        shell: WtDefaultShell::Cmd,
                    });
                    assert_eq!(
                        parse_wt_default_profile(&settings.to_string(), None),
                        expected,
                        "selector={selector}, reverse={reverse}, commandline={commandline}"
                    );
                }
            }
        }

        // Only when no GUID in the table matches may a GUID-shaped string be
        // looked up as a name.
        let name = "{11111111-1111-4111-8111-111111111111}";
        let settings = serde_json::json!({
            "defaultProfile": name,
            "profiles": {"list": [{"guid": pwsh_guid, "name": name, "commandline": "pwsh.exe"}]}
        });
        assert_eq!(
            parse_wt_default_profile(&settings.to_string(), None)
                .unwrap()
                .selector,
            pwsh_guid
        );
    }

    #[test]
    fn parse_wt_default_profile_preserves_the_matched_selector() {
        // This fixture inherits defaults; use the legacy rule so selector matching
        // is isolated from versioned default loading.
        for (content, selector) in [
            (
                r#"{"defaultProfile":"{33333333-3333-4333-8333-333333333333}","profiles":{"list":[{"guid":"{33333333-3333-4333-8333-333333333333}","name":"Custom shell","commandline":"pwsh.exe -NoLogo"}]}}"#,
                "{33333333-3333-4333-8333-333333333333}",
            ),
            (
                r#"{"defaultProfile":"Custom shell","profiles":{"list":[{"guid":"{33333333-3333-4333-8333-333333333333}","name":"Custom shell","commandline":"pwsh.exe -NoLogo"}]}}"#,
                "{33333333-3333-4333-8333-333333333333}",
            ),
            (
                r#"{"defaultProfile":"Custom shell","profiles":[{"name":"Custom shell","commandline":"pwsh.exe -NoLogo"}]}"#,
                "{fa616ee5-b304-5217-8de0-4f1eb1e90d19}",
            ),
            (
                r#"{"defaultProfile":"Profile0","profiles":{"list":[{"name":"profile0","commandline":"pwsh.exe"},{"name":"Profile0","commandline":"pwsh.exe"}]}}"#,
                "{6a2f325d-dd64-5214-ac09-ceac61244422}",
            ),
            (
                r#"{"defaultProfile":"{FA616EE5-B304-5217-8DE0-4F1EB1E90D19}","profiles":{"list":[{"name":"Custom shell","commandline":"pwsh.exe -NoLogo"}]}}"#,
                "{fa616ee5-b304-5217-8de0-4f1eb1e90d19}",
            ),
            (
                r#"{"defaultProfile":"Custom shell","profiles":{"defaults":{"guid":"{33333333-3333-4333-8333-333333333333}","name":"Other shell","source":"Other source","commandline":"pwsh.exe -NoLogo"},"list":[{"name":"Custom shell"}]}}"#,
                "{fa616ee5-b304-5217-8de0-4f1eb1e90d19}",
            ),
            (
                r#"{"defaultProfile":"{574e775e-4f2a-5b96-ac1e-a2962a402336}"}"#,
                "{574e775e-4f2a-5b96-ac1e-a2962a402336}",
            ),
            (
                r#"{"defaultProfile":"{33333333-3333-4333-8333-333333333333}","profiles":{"defaults":{"commandline":"pwsh.exe"},"list":[]}}"#,
                "{33333333-3333-4333-8333-333333333333}",
            ),
        ] {
            assert_eq!(
                parse_wt_default_profile(content, Some((1, 23, 20211, 0))),
                Some(WtDefaultProfile {
                    selector: selector.to_string(),
                    shell: WtDefaultShell::Pwsh,
                }),
                "{content}"
            );
        }
    }

    #[test]
    fn parse_wt_default_profile_rejects_unresolvable_selectors() {
        for content in [
            r#"{"defaultProfile":"custom shell","profiles":{"list":[{"guid":"{33333333-3333-4333-8333-333333333333}","name":"Custom shell","commandline":"pwsh.exe -NoLogo"}]}}"#,
            r#"{"defaultProfile":"custom shell","profiles":[{"name":"Custom shell","commandline":"pwsh.exe -NoLogo"}]}"#,
            r#"{"defaultProfile":" Custom shell","profiles":[{"name":"Custom shell","commandline":"pwsh.exe"}]}"#,
            r#"{"defaultProfile":"Custom shell ","profiles":[{"name":"Custom shell","commandline":"pwsh.exe"}]}"#,
            r#"{"defaultProfile":"Dev&echo","profiles":{"defaults":{"commandline":"pwsh.exe"},"list":[]}}"#,
            r#"{"defaultProfile":"Dev&echo","profiles":{"list":[{"name":"Dev&echo","guid":"invalid","commandline":"pwsh.exe"}]}}"#,
        ] {
            assert_eq!(parse_wt_default_profile(content, None), None, "{content}");
        }
    }

    #[test]
    fn wt_launch_helpers_preserve_the_profile_and_use_explicit_fallback() {
        let ps = r"& 'C:\Temp\test.bat'";
        let expected_fallback = vec![
            "new-tab",
            "cmd",
            "/D",
            "/V:OFF",
            "/K",
            "%CC_SWITCH_INTERNAL_BATCH_PATH%",
        ];

        for shell in [
            WtDefaultShell::Pwsh,
            WtDefaultShell::PowerShell,
            WtDefaultShell::Cmd,
        ] {
            let profile = WtDefaultProfile {
                selector: "Custom profile with spaces".to_string(),
                shell,
            };
            let mut expected = vec![
                "new-tab",
                "--profile",
                "Custom profile with spaces",
                "--appendCommandLine",
                "--",
            ];
            if shell == WtDefaultShell::Cmd {
                expected.extend(["/D", "/V:OFF", "/K", "%CC_SWITCH_INTERNAL_BATCH_PATH%"]);
            } else {
                expected.extend(["-NoExit", "-Command", ps]);
            }
            assert_eq!(select_wt_launch_args(Some(&profile), true, ps), expected);
            assert_eq!(
                select_wt_launch_args(Some(&profile), false, ps),
                expected_fallback
            );
        }
        for supports_append in [false, true] {
            assert_eq!(
                select_wt_launch_args(None, supports_append, ps),
                expected_fallback
            );
        }

        let shim = "path = \"C:\\scoop\\apps\\windows-terminal\\current\\wt.exe\"\r\nargs = \r\n";
        assert_eq!(
            parse_scoop_shim_content(shim),
            Some(PathBuf::from(
                r"C:\scoop\apps\windows-terminal\current\wt.exe"
            ))
        );
        assert_eq!(parse_scoop_shim_content("bad content"), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cmd_batch_env_reference_executes_special_character_path() {
        let temp = tempfile::tempdir().expect("probe directory should be created");
        let hostile_dir = temp
            .path()
            .join("space & %CC_SWITCH_CMD_EXPAND% ^ ! (test)");
        std::fs::create_dir(&hostile_dir).expect("hostile directory should be created");
        let batch = hostile_dir.join("probe.bat");
        let marker = temp.path().join("executed.txt");
        std::fs::write(
            &batch,
            format!(
                "@echo off\r\nsetlocal DisableDelayedExpansion\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\n> \"{}\" echo ok\r\n",
                escape_windows_batch_value(&marker.to_string_lossy())
            ),
        )
        .expect("probe batch should be written");

        let mut command = std::process::Command::new("cmd");
        command.env("CC_SWITCH_CMD_EXPAND", "wrong");
        configure_windows_cmd_batch(&mut command, "/C", &batch.to_string_lossy());
        let output = command.output().expect("cmd probe should start");

        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            decode_command_output(&output.stdout),
            decode_command_output(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(marker).unwrap().trim(),
            "ok",
            "the literal hostile path must execute without variable expansion"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn powershell_batch_command_executes_special_character_path() {
        let temp = tempfile::tempdir().expect("probe directory should be created");
        let hostile_dir = temp
            .path()
            .join("space & %CC_SWITCH_CMD_EXPAND% ^ ! (test) and O'Brien");
        std::fs::create_dir(&hostile_dir).expect("hostile directory should be created");
        let batch = hostile_dir.join("probe.bat");
        let marker = hostile_dir.join("executed.txt");
        std::fs::write(
            &batch,
            format!(
                "@echo off\r\nsetlocal DisableDelayedExpansion\r\n> \"{}\" echo ok\r\n",
                escape_windows_batch_value(&marker.to_string_lossy())
            ),
        )
        .expect("probe batch should be written");

        let bat_path = batch.to_string_lossy();
        let output = std::process::Command::new("pwsh")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                WINDOWS_POWERSHELL_BATCH_COMMAND,
            ])
            .env(
                WINDOWS_BATCH_PATH_ENV,
                quote_windows_batch_path_for_env(&bat_path),
            )
            .env("CC_SWITCH_CMD_EXPAND", "wrong")
            .output()
            .expect("PowerShell probe should start");

        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            decode_command_output(&output.stdout),
            decode_command_output(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(marker).unwrap().trim(),
            "ok",
            "PowerShell must execute the literal hostile batch path"
        );
    }

    // Spawn a real argv probe so the launcher path and args are not re-parsed
    // by an extra shell.
    #[cfg(target_os = "windows")]
    #[test]
    fn wt_launch_preserves_special_character_paths_and_profile_args() {
        let temp = tempfile::tempdir().expect("probe directory should be created");
        let source = temp.path().join("argv_probe.rs");
        let executable = temp.path().join("argv_probe.exe");
        std::fs::write(
            &source,
            r#"#![windows_subsystem = "windows"]
fn main() {
    let path = std::env::current_exe().unwrap().with_extension("args");
    let staging = path.with_extension("staging");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let batch_path = std::env::var("CC_SWITCH_INTERNAL_BATCH_PATH").unwrap_or_default();
    std::fs::write(&staging, format!("{args:?}\n{batch_path}")).unwrap();
    std::fs::rename(staging, path).unwrap();
}
"#,
        )
        .expect("argv probe source should be written");
        let compiled =
            std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .output()
                .expect("rustc should compile the argv probe");
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let mut failures = Vec::new();
        for directory in [
            "Tools&echo",
            "Tools%COMSPEC%",
            "Tools^A",
            "Tools(A)",
            "Tools and O'Brien",
            "工具🚀",
        ] {
            let launcher_dir = temp.path().join(directory);
            std::fs::create_dir(&launcher_dir).expect("launcher directory should be created");
            let launcher_executable = launcher_dir.join("wt.exe");
            std::fs::copy(&executable, &launcher_executable)
                .expect("probe should be copied to the launcher path");
            let capture = launcher_executable.with_extension("args");
            let launcher = launcher_executable.to_string_lossy();
            for name in [
                "Custom profile",
                "Dev&echo",
                "Dev%COMSPEC%",
                "Dev;Test",
                r"Dev\;Test",
                r"Dev\\;Test",
                "开发 🚀",
                "Dev|echo",
                "Dev^echo",
            ] {
                let settings = serde_json::json!({
                    "defaultProfile": name,
                    "profiles": {"list": [{"name": name, "commandline": "pwsh.exe -NoLogo"}]}
                });
                let profile = parse_wt_default_profile(&settings.to_string(), None)
                    .expect("the profile should be recognized");
                assert!(
                    uuid::Uuid::parse_str(&profile.selector).is_ok(),
                    "names must not reach cmd or WT as selectors: {name:?}"
                );
                let bat = r"C:\Temp\batch & %COMSPEC% ^ O'Brien\probe.bat";
                let ps_cmd = format!("& '{}'", bat.replace('\'', "''"));
                let args = select_wt_launch_args(Some(&profile), true, &ps_cmd);
                let expected = format!("{args:?}\n{}", quote_windows_batch_path_for_env(bat));
                run_wt_command(&launcher, &args, bat)
                    .expect("WT launcher should directly start the probe");
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while !capture.is_file() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                let observed =
                    std::fs::read_to_string(&capture).expect("argv probe must write its result");
                if observed != expected {
                    failures.push(format!(
                        "{name:?}: expected {expected}, received {observed}"
                    ));
                }
                std::fs::remove_file(&capture)
                    .expect("the completed probe result should be removed");
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        let missing_launcher = temp.path().join("missing.exe");
        assert!(run_wt_command(&missing_launcher.to_string_lossy(), &[], "probe.bat").is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[serial_test::serial]
    fn windows_terminal_launchers_do_not_inherit_parent_claude_session() {
        fn wait_for_probe(capture: &Path) -> String {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !capture.is_file() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            std::fs::read_to_string(capture)
                .expect("env probe must write its inherited variable names")
        }

        let restore = EnvRestore(
            PARENT_CLAUDE_SESSION_ENV_VARS
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect(),
        );
        for name in PARENT_CLAUDE_SESSION_ENV_VARS {
            std::env::set_var(name, "cc-switch-parent-session");
        }

        let temp = tempfile::tempdir().expect("probe directory should be created");
        let source = temp.path().join("env_probe.rs");
        let executable = temp.path().join("env_probe.exe");
        std::fs::write(
            &source,
            r#"#![windows_subsystem = "windows"]
fn main() {
    let mut args = std::env::args_os().skip(1);
    let capture = args.next().unwrap();
    let inherited: Vec<String> = args
        .filter(|name| std::env::var_os(name).is_some())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    std::fs::write(capture, inherited.join("\n")).unwrap();
}
"#,
        )
        .expect("env probe source should be written");
        let compiled =
            std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .output()
                .expect("rustc should compile the env probe");
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );

        fn run_wt_probe(args: &[&str]) -> Result<(), String> {
            let (launcher, arguments) = args
                .split_first()
                .ok_or_else(|| "missing env probe executable".to_string())?;
            run_wt_command(launcher, arguments, r"C:\Temp\cc-switch-env-probe.bat")
        }

        fn run_configured_batch_probe(args: &[&str]) -> Result<(), String> {
            let (launcher, arguments) = args
                .split_first()
                .ok_or_else(|| "missing env probe executable".to_string())?;
            let mut command = std::process::Command::new(launcher);
            configure_windows_batch_env(&mut command, r"C:\Temp\cc-switch-env-probe.bat");
            command
                .args(arguments)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        let launcher = executable.to_string_lossy();
        for (label, capture, launch) in [
            (
                "Windows Terminal",
                temp.path().join("wt.result"),
                run_wt_probe as fn(&[&str]) -> Result<(), String>,
            ),
            (
                "configured batch process",
                temp.path().join("batch.result"),
                run_configured_batch_probe as fn(&[&str]) -> Result<(), String>,
            ),
        ] {
            let capture_arg = capture.to_string_lossy();
            let mut args = vec![launcher.as_ref(), capture_arg.as_ref()];
            args.extend(PARENT_CLAUDE_SESSION_ENV_VARS.iter().copied());
            launch(&args).unwrap_or_else(|error| panic!("{label} probe should launch: {error}"));
            let inherited = wait_for_probe(&capture);
            assert!(
                inherited.is_empty(),
                "{label} inherited parent Claude session variables:\n{inherited}"
            );
        }
        drop(restore);
    }

    // Exercise the real WT profile selection, not only the generated argv.
    // Opt in on a Windows desktop; this opens diagnostic tabs without changing settings.
    #[cfg(target_os = "windows")]
    #[test]
    #[serial_test::serial]
    #[ignore = "requires desktop WT, a supported default profile GUID, and built-in PowerShell/cmd profiles"]
    fn wt_launch_executes_batch_in_real_terminal() {
        const SENTINEL: &str = "CC_SWITCH_WT_ENV_SENTINEL";
        const CMD_EXPANSION_SENTINEL: &str = "CC_SWITCH_CMD_EXPAND";
        let restore = EnvRestore(
            PARENT_CLAUDE_SESSION_ENV_VARS
                .iter()
                .copied()
                .chain([SENTINEL, CMD_EXPANSION_SENTINEL])
                .map(|name| (name, std::env::var_os(name)))
                .collect(),
        );
        for name in PARENT_CLAUDE_SESSION_ENV_VARS {
            std::env::set_var(name, "cc-switch-parent-session");
        }
        std::env::set_var(SENTINEL, "preserved");
        std::env::set_var(CMD_EXPANSION_SENTINEL, "wrong");

        let installation = resolve_wt_installation().expect("Windows Terminal must be installed");
        let wt_version = wt_installation_version(&installation);
        let supports_append = wt_version.is_none_or(wt_version_supports_append);
        let default_profile = detect_wt_default_profile(&installation, wt_version)
            .expect("the default WT profile must support batch execution");
        let cases = [
            (default_profile.clone(), supports_append),
            (
                WtDefaultProfile {
                    selector: "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}".to_string(),
                    shell: WtDefaultShell::PowerShell,
                },
                supports_append,
            ),
            (
                WtDefaultProfile {
                    selector: "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}".to_string(),
                    shell: WtDefaultShell::Cmd,
                },
                supports_append,
            ),
            (default_profile, false),
        ];
        let temp = tempfile::Builder::new()
            .prefix("cc-switch-wt-smoke-")
            .tempdir()
            .expect("diagnostic directory should be created");
        let fixture_dir = temp
            .path()
            .join("space & %CC_SWITCH_CMD_EXPAND% ^ ! (test) and O'Brien");
        std::fs::create_dir(&fixture_dir).expect("quoted fixture directory should be created");
        for (index, (profile, supports_append)) in cases.iter().enumerate() {
            let marker = fixture_dir.join(format!("executed-{index}.txt"));
            let batch = fixture_dir.join(format!("probe-{index}.bat"));
            std::fs::write(
                &batch,
                format!(
                    "@echo off\r\nsetlocal DisableDelayedExpansion\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\n> \"{marker}\" echo profile=%WT_PROFILE_ID%\r\n>> \"{marker}\" echo NO_COLOR=%NO_COLOR%\r\n>> \"{marker}\" echo NODE_DISABLE_COLORS=%NODE_DISABLE_COLORS%\r\n>> \"{marker}\" echo CLAUDE_CODE_CHILD_SESSION=%CLAUDE_CODE_CHILD_SESSION%\r\n>> \"{marker}\" echo CLAUDE_CODE_SESSION_ID=%CLAUDE_CODE_SESSION_ID%\r\n>> \"{marker}\" echo sentinel=%CC_SWITCH_WT_ENV_SENTINEL%\r\nexit\r\n",
                    marker = escape_windows_batch_value(&marker.to_string_lossy())
                ),
            )
            .expect("diagnostic batch should be written");
            let bat_path = batch.to_string_lossy();
            let ps_cmd = format!("{WINDOWS_POWERSHELL_BATCH_COMMAND}; exit");
            if index == 0 {
                launch_wt_terminal(&bat_path, &ps_cmd).expect("WT launch should succeed");
            } else {
                let launcher = installation.launcher.to_string_lossy();
                let args = select_wt_launch_args(Some(profile), *supports_append, &ps_cmd);
                run_wt_command(&launcher, &args, &bat_path)
                    .expect("explicit-profile WT launch should succeed");
            }

            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            while !marker.is_file() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let captured = std::fs::read_to_string(&marker)
                .unwrap_or_else(|error| panic!("case {index}: WT must execute the batch: {error}"));
            let values: std::collections::HashMap<_, _> = captured
                .lines()
                .filter_map(|line| line.split_once('='))
                .collect();
            for name in [
                "NO_COLOR",
                "NODE_DISABLE_COLORS",
                "CLAUDE_CODE_CHILD_SESSION",
                "CLAUDE_CODE_SESSION_ID",
            ] {
                assert_eq!(
                    values.get(name),
                    Some(&""),
                    "case {index}: {name} must not cross the WT spawn boundary"
                );
            }
            assert_eq!(
                values.get("sentinel"),
                Some(&"preserved"),
                "case {index}: unrelated environment must be preserved"
            );
            if *supports_append {
                assert_eq!(
                    normalize_wt_guid(values.get("profile").copied().unwrap_or_default()),
                    normalize_wt_guid(&profile.selector),
                    "case {index}: the batch must run in the same profile that was classified"
                );
            }
        }
        drop(restore);
    }

    #[test]
    fn wt_settings_paths_stay_bound_to_the_resolved_installation() {
        let local = Path::new("C:/Users/test/AppData/Local");
        for exe in [
            "C:/Users/test/scoop/apps/windows-terminal/current/WindowsTerminal.exe",
            "C:/Program Files/Windows Terminal/wt.exe",
            "C:/Tools/terminal/wt.exe",
        ] {
            assert_eq!(
                build_wt_settings_paths(Path::new(exe), Some(local)),
                vec![unpackaged_wt_settings_path(local)],
                "{exe}"
            );
        }

        // Do not depend on the local install type; classify stable vs Preview
        // and bind each to its unique active settings path.
        for (exe, expected_preview) in [
            (
                "C:/Program Files/WindowsApps/Microsoft.WindowsTerminal_1.24.11911.0_x64__8wekyb3d8bbwe/wt.exe",
                false,
            ),
            (
                "C:/Program Files/WindowsApps/Microsoft.WindowsTerminalPreview_1.0.0.0_x64__8wekyb3d8bbwe/wt.exe",
                true,
            ),
        ] {
            let exe = Path::new(exe);
            let (preview, _) = store_wt_package_dir(exe)
                .expect("Store executable must belong to a WT package");
            assert_eq!(preview, expected_preview, "{exe:?}");
            assert_eq!(
                build_wt_settings_paths(exe, Some(local)),
                vec![store_wt_settings_path(local, expected_preview)],
                "{exe:?}"
            );
        }
    }

    #[test]
    fn wt_portable_install_uses_only_active_settings() {
        let local = Path::new("C:/Users/test/AppData/Local");
        for (layout, persist) in [
            ("tools-wt", None),
            (
                "scoop/apps/windows-terminal/current",
                Some("scoop/persist/windows-terminal"),
            ),
            (
                "scoop/apps/windows-terminal-preview/1.24.0",
                Some("scoop/persist/windows-terminal-preview"),
            ),
        ] {
            let temp = tempfile::tempdir().expect("temp dir should be created");
            let exe_dir = temp.path().join(layout);
            std::fs::create_dir_all(&exe_dir).expect("portable dir should be created");
            let exe = exe_dir.join("wt.exe");
            std::fs::write(&exe, b"").expect("portable exe placeholder should be written");
            std::fs::write(exe_dir.join(".portable"), b"")
                .expect("portable marker should be written");

            let active = r#"{
                "defaultProfile": "{574e775e-4f2a-5b96-ac1e-a2962a402336}"
            }"#;
            let stale = r#"{
                "defaultProfile": "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}"
            }"#;
            let mut expected = Vec::new();
            for root in std::iter::once(exe_dir.clone())
                .chain(persist.map(|persist| temp.path().join(persist)))
            {
                let settings_dir = root.join("settings");
                std::fs::create_dir_all(&settings_dir).expect("settings dir should be created");
                let active_path = settings_dir.join("settings.json");
                std::fs::write(&active_path, active).expect("active settings should be written");
                expected.push(active_path);
                let stale_path = root.join("settings.json");
                std::fs::write(&stale_path, stale).expect("stale settings should be written");
                // A file at the root is not WT settings, even if its timestamp is
                // newer than the active config.
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&stale_path)
                    .expect("stale settings should open")
                    .set_times(std::fs::FileTimes::new().set_modified(
                        std::time::SystemTime::now() + std::time::Duration::from_secs(60),
                    ))
                    .expect("stale settings should have a newer timestamp");
            }

            let paths = build_wt_settings_paths(&exe, Some(local));
            assert_eq!(
                detect_wt_default_profile_from_paths(&paths, None),
                parse_wt_default_profile(active, None),
                "{layout}: root settings must not override the active PowerShell profile"
            );
            assert_eq!(
                paths, expected,
                "{layout}: only active settings are candidates"
            );
        }
    }

    #[test]
    fn wt_version_and_path_detect_append_command_line_support() {
        assert!(!wt_version_supports_append((1, 18, 3181, 0)));
        assert!(wt_version_supports_append((1, 19, 0, 0)));
        assert!(wt_version_supports_append((1, 24, 11911, 0)));
        assert_eq!(
            wt_version_from_path(Path::new(
                "C:/Program Files/WindowsApps/Microsoft.WindowsTerminal_1.18.3181.0_x64__8wekyb3d8bbwe/wt.exe"
            )),
            Some((1, 18, 3181, 0))
        );
        assert_eq!(
            wt_version_from_path(Path::new(
                "C:/Program Files/WindowsApps/Microsoft.WindowsTerminal_1.24.11911.0_x64__8wekyb3d8bbwe/wt.exe"
            )),
            Some((1, 24, 11911, 0))
        );
        assert_eq!(
            wt_version_from_path(Path::new(
                "C:/Users/test/scoop/apps/windows-terminal/1.18.3181.0/wt.exe"
            )),
            Some((1, 18, 3181, 0))
        );
        assert_eq!(
            wt_version_from_path(Path::new(
                "C:/Users/test/scoop/apps/windows-terminal/current/wt.exe"
            )),
            None
        );
    }

    #[test]
    fn unpackaged_release_directory_is_not_misclassified_as_store() {
        let local = Path::new("C:/Users/test/AppData/Local");
        let exe = Path::new("C:/Tools/Microsoft.WindowsTerminal_1.23_x64/wt.exe");
        let paths = build_wt_settings_paths(exe, Some(local));
        assert!(paths.contains(&unpackaged_wt_settings_path(local)));
        assert!(!paths.contains(&store_wt_settings_path(local, false)));
        assert!(!paths.contains(&store_wt_settings_path(local, true)));
    }

    #[test]
    fn app_execution_alias_parser_extracts_the_package_target() {
        let build_buffer = |target: &str| {
            let values = ["Package_Family", "Package_Family!App", target, "0"];
            let mut payload = (values.len() as u32).to_le_bytes().to_vec();
            for value in values {
                for unit in value.encode_utf16().chain(Some(0)) {
                    payload.extend_from_slice(&unit.to_le_bytes());
                }
            }

            let mut buffer = APPEXECLINK_REPARSE_TAG.to_le_bytes().to_vec();
            buffer.extend_from_slice(&(payload.len() as u16).to_le_bytes());
            buffer.extend_from_slice(&0u16.to_le_bytes());
            buffer.extend_from_slice(&payload);
            buffer
        };

        let target = r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.0.0.0_x64__8wekyb3d8bbwe\wt.exe";
        let parsed = parse_app_execution_alias_target(&build_buffer(target))
            .expect("App Execution Alias target should parse");
        assert_eq!(String::from_utf16(&parsed).unwrap(), target);
        assert_eq!(
            parse_app_execution_alias_target(&build_buffer(r"relative\wt.exe")),
            None
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn store_wt_alias_resolves_to_package_settings_not_scoop() {
        let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
            return;
        };
        let alias = local.join("Microsoft").join("WindowsApps").join("wt.exe");
        if std::fs::symlink_metadata(&alias).is_err() {
            return;
        }

        let target = resolve_app_execution_alias_target(&alias)
            .expect("Store wt.exe App Execution Alias should resolve via APPEXECLINK");
        // The wt.exe alias may belong to stable or Preview; settings paths must
        // follow the actual target package.
        let (preview, _) = store_wt_package_dir(&target)
            .unwrap_or_else(|| panic!("Store alias must resolve to a WT package: {target:?}"));
        assert_eq!(
            build_wt_settings_paths(&target, Some(&local)),
            vec![store_wt_settings_path(&local, preview)],
            "Store settings should be bound to the resolved package: {target:?}"
        );

        // PATH lookup is a separate concern: GitHub-hosted Windows runners often
        // have the Store alias file but no WindowsApps directory on PATH.
        if let Some(installation) = resolve_wt_installation() {
            assert_eq!(
                installation
                    .launcher
                    .file_name()
                    .map(|name| name.to_string_lossy().to_ascii_lowercase()),
                Some("wt.exe".into()),
                "launcher should stay the resolved wt.exe, got {:?}",
                installation.launcher
            );
            if installation
                .launcher
                .parent()
                .is_some_and(is_windows_app_execution_alias_dir)
            {
                assert_eq!(
                    installation.settings_executable.as_ref(),
                    Some(&target),
                    "PATH-selected Store alias should bind settings to the same APPEXECLINK target"
                );
            }
        }
    }

    #[test]
    fn wt_settings_detection_skips_malformed_candidates() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let valid = temp.path().join("valid.json");
        let malformed = temp.path().join("malformed.json");
        let pwsh = r#"{
            "defaultProfile": "PowerShell 7",
            "profiles": [{"name": "PowerShell 7", "commandline": "pwsh.exe"}]
        }"#;
        std::fs::write(&valid, pwsh).expect("valid settings should be written");
        std::fs::write(&malformed, "not json").expect("malformed settings should be written");

        assert_eq!(
            detect_wt_default_profile_from_paths(&[malformed, valid], None),
            Some(WtDefaultProfile {
                selector: "{8f7fc2c9-b084-5bc6-89f4-82fc0f5d1b6c}".to_string(),
                shell: WtDefaultShell::Pwsh,
            })
        );
    }
}
