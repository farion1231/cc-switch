use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::{Command, Stdio};

#[cfg(any(target_os = "macos", test))]
use crate::codex_config::CODEX_DESKTOP_APP_BUNDLES;

#[cfg(any(target_os = "macos", test))]
const MACOS_QUIT_SCRIPT: &str = "on run argv\ntell application (item 1 of argv) to quit\nend run";

#[cfg(target_os = "macos")]
pub(super) fn find_target(app: &str, home: &Path) -> Result<Option<PathBuf>, String> {
    let system_applications = Path::new("/Applications");
    let user_applications = home.join("Applications");
    let target = match app {
        "codex" => CODEX_DESKTOP_APP_BUNDLES
            .iter()
            .flat_map(|bundle| {
                [system_applications, user_applications.as_path()]
                    .into_iter()
                    .map(move |root| root.join(bundle))
            })
            .find(|path| path.is_dir()),
        "claude-desktop" => crate::claude_desktop_config::macos_app_bundle_candidates()
            .unwrap_or_default()
            .into_iter()
            .find(|path| path.is_dir()),
        "opencode" => [system_applications, user_applications.as_path()]
            .into_iter()
            .map(|root| root.join("OpenCode.app"))
            .find(|path| path.is_dir()),
        _ => None,
    };
    Ok(target)
}

#[cfg(target_os = "linux")]
pub(super) fn find_target(app: &str, home: &Path) -> Result<Option<PathBuf>, String> {
    let executable_directories = linux_desktop_executable_directories(home);
    let data_directories = linux_desktop_data_directories(home);
    Ok(match app {
        "codex" => find_linux_codex_desktop_application_in_roots(
            home,
            &executable_directories,
            &data_directories,
            &[
                PathBuf::from("/usr/bin/chatgpt"),
                PathBuf::from("/usr/lib/chatgpt/ChatGPT"),
            ],
        ),
        "claude-desktop" => {
            find_linux_claude_desktop_application(home, &executable_directories, &data_directories)
        }
        "opencode" => find_linux_opencode_desktop_application_in_roots(
            home,
            &executable_directories,
            &data_directories,
            &linux_appimage_directories(home),
        ),
        _ => None,
    })
}

#[cfg(any(target_os = "macos", test))]
fn application_bundle(application: &Path) -> &Path {
    application
        .ancestors()
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("app"))
        .unwrap_or(application)
}

#[cfg(any(target_os = "linux", test))]
fn find_linux_codex_desktop_application_in_roots(
    home: &Path,
    executable_directories: &[PathBuf],
    data_directories: &[PathBuf],
    package_executables: &[PathBuf],
) -> Option<PathBuf> {
    find_linux_codex_application_from_desktop_entries(
        home,
        executable_directories,
        data_directories,
    )
    .or_else(|| {
        package_executables
            .iter()
            .find(|path| path.is_file())
            .cloned()
    })
}

#[cfg(any(target_os = "linux", test))]
fn find_linux_codex_application_from_desktop_entries(
    home: &Path,
    executable_directories: &[PathBuf],
    data_directories: &[PathBuf],
) -> Option<PathBuf> {
    for data_directory in data_directories {
        let applications = data_directory.join("applications");
        let mut entries = std::fs::read_dir(applications)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("desktop"))
            })
            .collect::<Vec<_>>();
        entries.sort();

        for entry in entries {
            let Ok(content) = std::fs::read_to_string(&entry) else {
                continue;
            };
            if !is_chatgpt_desktop_entry(&entry, &content) {
                continue;
            }
            let Some(command) = linux_desktop_entry_command(&content) else {
                continue;
            };
            if !is_chatgpt_desktop_command(&command) {
                continue;
            }
            if let Some(application) =
                resolve_linux_desktop_command(home, executable_directories, command.as_str())
            {
                return Some(application);
            }
        }
    }
    None
}

#[cfg(any(target_os = "linux", test))]
fn is_chatgpt_desktop_entry(path: &Path, content: &str) -> bool {
    let file_stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if is_chatgpt_desktop_identifier(&file_stem) {
        return true;
    }

    let mut in_desktop_entry = false;
    content.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line.eq_ignore_ascii_case("[Desktop Entry]");
            return false;
        }
        if !in_desktop_entry {
            return false;
        }
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        let key = key.trim().split('[').next().unwrap_or_default();
        let value = value.trim().to_ascii_lowercase();
        match key {
            "Name" => value == "chatgpt" || value.starts_with("chatgpt "),
            "Icon" | "StartupWMClass" => is_chatgpt_desktop_identifier(&value),
            _ => false,
        }
    })
}

#[cfg(any(target_os = "linux", test))]
fn is_chatgpt_desktop_identifier(value: &str) -> bool {
    matches!(
        value,
        "chatgpt" | "openai-chatgpt" | "openai.chatgpt" | "com.openai.chatgpt"
    ) || value.starts_with("chatgpt-")
        || value.starts_with("chatgpt.")
}

#[cfg(any(target_os = "linux", test))]
fn is_chatgpt_desktop_command(command: &str) -> bool {
    Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("chatgpt"))
}

#[cfg(any(target_os = "linux", test))]
fn find_linux_opencode_desktop_application_in_roots(
    home: &Path,
    executable_directories: &[PathBuf],
    data_directories: &[PathBuf],
    appimage_directories: &[PathBuf],
) -> Option<PathBuf> {
    const EXECUTABLE_NAMES: &[&str] = &[
        "ai.opencode.desktop",
        "ai.opencode.desktop.beta",
        "ai.opencode.desktop.dev",
        "opencode-desktop",
        "@opencode-aidesktop",
        "OpenCode",
        "OpenCode Desktop",
    ];

    for directory in executable_directories {
        for name in EXECUTABLE_NAMES {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    find_linux_opencode_application_from_desktop_entries(
        home,
        executable_directories,
        data_directories,
    )
    .or_else(|| {
        appimage_directories.iter().find_map(|directory| {
            let mut candidates = std::fs::read_dir(directory)
                .ok()?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file() && is_opencode_desktop_appimage(path))
                .collect::<Vec<_>>();
            candidates.sort();
            candidates.into_iter().next()
        })
    })
}

#[cfg(any(target_os = "linux", test))]
fn find_linux_opencode_application_from_desktop_entries(
    home: &Path,
    executable_directories: &[PathBuf],
    data_directories: &[PathBuf],
) -> Option<PathBuf> {
    for data_directory in data_directories {
        let applications = data_directory.join("applications");
        let mut entries = std::fs::read_dir(applications)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("desktop"))
            })
            .collect::<Vec<_>>();
        entries.sort();

        for entry in entries {
            let Ok(content) = std::fs::read_to_string(&entry) else {
                continue;
            };
            if !is_opencode_desktop_entry(&entry, &content) {
                continue;
            }
            let Some(command) = linux_desktop_entry_command(&content) else {
                continue;
            };
            if let Some(application) =
                resolve_linux_desktop_command(home, executable_directories, command.as_str())
            {
                return Some(application);
            }
        }
    }
    None
}

#[cfg(any(target_os = "linux", test))]
fn is_opencode_desktop_entry(path: &Path, content: &str) -> bool {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if file_name.contains("opencode") {
        return true;
    }

    let mut in_desktop_entry = false;
    content.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line.eq_ignore_ascii_case("[Desktop Entry]");
            return false;
        }
        if !in_desktop_entry {
            return false;
        }
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        let value = value.trim().to_ascii_lowercase();
        match key.trim() {
            "Name" => {
                value == "opencode"
                    || value.starts_with("opencode beta")
                    || value.starts_with("opencode dev")
            }
            "Icon" | "StartupWMClass" => {
                value.starts_with("ai.opencode.desktop")
                    || value == "@opencode-aidesktop"
                    || value == "opencode"
            }
            _ => false,
        }
    })
}

#[cfg(any(target_os = "linux", test))]
fn linux_desktop_entry_command(content: &str) -> Option<String> {
    let mut in_desktop_entry = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line.eq_ignore_ascii_case("[Desktop Entry]");
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("Exec") {
            continue;
        }
        let tokens = tokenize_linux_desktop_exec(value.trim());
        let mut index = 0;
        if tokens.first().is_some_and(|token| {
            Path::new(token)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "env")
        }) {
            index += 1;
            while tokens.get(index).is_some_and(|token| token.contains('=')) {
                index += 1;
            }
        }
        let command = tokens.get(index)?.trim();
        if command.is_empty()
            || command.starts_with('%')
            || ["flatpak", "snap", "gtk-launch", "sh", "bash"]
                .iter()
                .any(|wrapper| {
                    Path::new(command)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == *wrapper)
                })
        {
            return None;
        }
        return Some(command.to_string());
    }
    None
}

#[cfg(any(target_os = "linux", test))]
fn tokenize_linux_desktop_exec(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;

    for character in value.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }
        if character == '"' || character == '\'' {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(any(target_os = "linux", test))]
fn resolve_linux_desktop_command(
    home: &Path,
    executable_directories: &[PathBuf],
    command: &str,
) -> Option<PathBuf> {
    let candidate = if let Some(relative) = command.strip_prefix("~/") {
        home.join(relative)
    } else {
        PathBuf::from(command)
    };
    if candidate.is_absolute() || command.contains('/') {
        return candidate.is_file().then_some(candidate);
    }
    executable_directories
        .iter()
        .map(|directory| directory.join(&candidate))
        .find(|path| path.is_file())
}

#[cfg(any(target_os = "linux", test))]
fn is_opencode_desktop_appimage(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let file_name = file_name.to_ascii_lowercase();
    file_name.ends_with(".appimage")
        && (file_name.starts_with("opencode-desktop") || file_name.starts_with("opencode_"))
}

#[cfg(any(target_os = "macos", test))]
fn desktop_process_ids_from_ps(output: &str, application: &Path) -> Vec<i32> {
    let bundle = application
        .ancestors()
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("app"));
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let split = line.find(char::is_whitespace)?;
            let process_id = line[..split].parse::<i32>().ok()?;
            if process_id <= 0 || process_id == std::process::id() as i32 {
                return None;
            }
            let executable = Path::new(line[split..].trim());
            (executable == application || bundle.is_some_and(|root| executable.starts_with(root)))
                .then_some(process_id)
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub(super) fn is_running(application: &Path) -> bool {
    macos_processes_for_application(application)
        .map(|ids| !ids.is_empty())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub(super) fn is_running(application: &Path) -> bool {
    let application =
        std::fs::canonicalize(application).unwrap_or_else(|_| application.to_path_buf());
    !linux_processes_for_application(&application).is_empty()
}

#[cfg(target_os = "macos")]
pub(super) fn stop(application: &Path, label: &str) -> Result<(), String> {
    if macos_processes_for_application(application)?.is_empty() {
        return Ok(());
    }
    macos_request_quit(application)?;
    if macos_wait_for_exit(application, 50)? {
        return Ok(());
    }
    for process_id in macos_processes_for_application(application)? {
        signal_process(process_id, libc::SIGKILL)?;
    }
    if macos_wait_for_exit(application, 10)? {
        Ok(())
    } else {
        Err(format!("{label} did not exit completely"))
    }
}

#[cfg(target_os = "linux")]
pub(super) fn stop(application: &Path, label: &str) -> Result<(), String> {
    let application =
        std::fs::canonicalize(application).unwrap_or_else(|_| application.to_path_buf());
    let mut signal_error = None;
    for process_id in linux_processes_for_application(&application) {
        if let Err(error) = signal_process(process_id, libc::SIGTERM) {
            signal_error.get_or_insert(error);
        }
    }
    for _ in 0..50 {
        if linux_processes_for_application(&application).is_empty() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    for process_id in linux_processes_for_application(&application) {
        if let Err(error) = signal_process(process_id, libc::SIGKILL) {
            signal_error.get_or_insert(error);
        }
    }
    for _ in 0..10 {
        if linux_processes_for_application(&application).is_empty() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let remaining = linux_processes_for_application(&application);
    if remaining.is_empty() {
        Ok(())
    } else if let Some(error) = signal_error {
        Err(error)
    } else {
        Err(format!(
            "{label} did not exit completely; remaining process IDs: {}",
            remaining
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

#[cfg(target_os = "macos")]
pub(super) fn launch(application: &Path, label: &str) -> Result<(), String> {
    let output = Command::new("open")
        .arg(application_bundle(application))
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Failed to launch {label}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to launch {label}: {}", detail.trim()))
    }
}

#[cfg(target_os = "linux")]
pub(super) fn launch(application: &Path, label: &str) -> Result<(), String> {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(application);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Keep the GUI in its own session and reap the direct child when it exits.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to launch {label}: {error}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_processes_for_application(application: &Path) -> Result<Vec<i32>, String> {
    let output = Command::new("ps")
        .args(["-axww", "-o", "pid=", "-o", "comm="])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Failed to read the process list: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Failed to read the process list: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(desktop_process_ids_from_ps(
        &String::from_utf8_lossy(&output.stdout),
        application,
    ))
}

#[cfg(target_os = "macos")]
fn macos_wait_for_exit(application: &Path, attempts: u32) -> Result<bool, String> {
    for _ in 0..attempts {
        if macos_processes_for_application(application)?.is_empty() {
            return Ok(true);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(macos_processes_for_application(application)?.is_empty())
}

#[cfg(target_os = "macos")]
fn macos_request_quit(application: &Path) -> Result<(), String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(MACOS_QUIT_SCRIPT)
        .arg(application_bundle(application))
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Failed to request application quit: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    if matches!(output.status.code(), Some(-1743 | 1))
        && (detail.contains("Not authorized")
            || detail.contains("not authorized")
            || detail.contains("not allowed"))
    {
        Err("Automation permission was denied. Allow cc-switch to control the desktop app in macOS Privacy & Security > Automation, then retry.".to_string())
    } else {
        Err(format!("Failed to request application quit: {detail}"))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn signal_process(process_id: i32, signal: i32) -> Result<(), String> {
    if unsafe { libc::kill(process_id, signal) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!("Failed to signal process {process_id}: {error}"))
    }
}

#[cfg(target_os = "linux")]
fn linux_processes_for_application(application: &Path) -> Vec<i32> {
    let current_process = std::process::id() as i32;
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut process_ids = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter(|process_id| *process_id != current_process)
        .filter(|process_id| linux_process_matches_application(*process_id, application))
        .collect::<Vec<_>>();
    process_ids.sort_unstable();
    process_ids
}

#[cfg(target_os = "linux")]
fn linux_process_matches_application(process_id: i32, application: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let process_directory = PathBuf::from(format!("/proc/{process_id}"));
    if std::fs::read_link(process_directory.join("exe"))
        .ok()
        .is_some_and(|path| linux_process_path_matches(&path, application))
    {
        return true;
    }
    if let Ok(command_line) = std::fs::read(process_directory.join("cmdline")) {
        let arguments = command_line
            .split(|byte| *byte == 0)
            .filter(|argument| !argument.is_empty())
            .map(std::ffi::OsStr::from_bytes)
            .collect::<Vec<_>>();
        if linux_nix_command_matches(&arguments, application, |path| {
            std::fs::canonicalize(path).ok()
        }) {
            return true;
        }
    }
    std::fs::read(process_directory.join("environ"))
        .ok()
        .is_some_and(|environment| {
            environment
                .split(|byte| *byte == 0)
                .filter_map(|entry| entry.strip_prefix(b"APPIMAGE="))
                .any(|path| {
                    !path.is_empty()
                        && linux_process_path_matches(
                            Path::new(std::ffi::OsStr::from_bytes(path)),
                            application,
                        )
                })
        })
}

#[cfg(any(target_os = "linux", test))]
fn linux_nix_command_matches(
    arguments: &[&std::ffi::OsStr],
    application: &Path,
    canonicalize: impl Fn(&Path) -> Option<PathBuf>,
) -> bool {
    let Some(root) = linux_opencode_nix_installation_root(application) else {
        return false;
    };
    let Some(executable) = arguments.first().and_then(|arg| Path::new(arg).file_stem()) else {
        return false;
    };
    if !matches!(executable.to_str(), Some("electron" | "node" | "nodejs")) {
        return false;
    }
    // The first non-option argument is the Electron/Node entry point.
    // Never match an arbitrary argument elsewhere in the command line.
    for argument in arguments.iter().skip(1) {
        let Some(argument) = argument.to_str() else {
            return false;
        };
        if argument.starts_with('-') {
            continue;
        }
        if argument.contains('=') && !argument.contains('/') {
            continue;
        }
        let path = Path::new(argument);
        return canonicalize(path)
            .unwrap_or_else(|| path.to_path_buf())
            .starts_with(root);
    }
    false
}

#[cfg(target_os = "linux")]
fn linux_process_path_matches(candidate: &Path, application: &Path) -> bool {
    candidate == application
        || std::fs::canonicalize(candidate)
            .ok()
            .is_some_and(|candidate| candidate == application)
}

#[cfg(any(target_os = "linux", test))]
fn linux_opencode_nix_installation_root(application: &Path) -> Option<&Path> {
    application.ancestors().find(|ancestor| {
        ancestor.parent() == Some(Path::new("/nix/store"))
            && ancestor
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_ascii_lowercase().contains("opencode"))
    })
}

#[cfg(target_os = "linux")]
fn linux_desktop_executable_directories(home: &Path) -> Vec<PathBuf> {
    let mut directories = vec![home.join(".nix-profile/bin")];
    directories.extend(
        [
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/opt/OpenCode",
            "/opt/opencode-desktop",
            "/opt/Claude",
            "/opt/claude-desktop",
            "/run/current-system/sw/bin",
            "/nix/var/nix/profiles/default/bin",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    directories
}

#[cfg(target_os = "linux")]
fn linux_desktop_data_directories(home: &Path) -> Vec<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    let mut directories = vec![data_home];
    if let Some(paths) = std::env::var_os("XDG_DATA_DIRS") {
        directories.extend(std::env::split_paths(&paths).filter(|path| path.is_absolute()));
    } else {
        directories.extend([
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]);
    }
    let mut seen = std::collections::HashSet::new();
    directories.retain(|path| seen.insert(path.clone()));
    directories
}

#[cfg(target_os = "linux")]
fn linux_appimage_directories(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("Applications"),
        home.join(".local/Applications"),
        home.join("Downloads"),
        home.join("Desktop"),
    ]
}

#[cfg(target_os = "linux")]
fn find_linux_claude_desktop_application(
    home: &Path,
    executable_directories: &[PathBuf],
    data_directories: &[PathBuf],
) -> Option<PathBuf> {
    for directory in executable_directories {
        let candidate = directory.join("claude-desktop");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    for candidate in [
        PathBuf::from("/opt/Claude/claude"),
        PathBuf::from("/opt/claude-desktop/claude"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    find_linux_desktop_entry_application(
        home,
        executable_directories,
        data_directories,
        |path, content| {
            is_named_desktop_entry(path, content, "claude", "Claude")
                && linux_desktop_entry_command(content).is_some_and(|command| {
                    let executable = Path::new(&command);
                    executable
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == "claude-desktop")
                        || executable.starts_with("/opt/Claude")
                        || executable.starts_with("/opt/claude-desktop")
                })
        },
    )
}

#[cfg(any(target_os = "linux", test))]
fn find_linux_desktop_entry_application(
    home: &Path,
    executable_directories: &[PathBuf],
    data_directories: &[PathBuf],
    matches: impl Fn(&Path, &str) -> bool,
) -> Option<PathBuf> {
    for data_directory in data_directories {
        let mut entries = std::fs::read_dir(data_directory.join("applications"))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("desktop"))
            })
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            let Ok(content) = std::fs::read_to_string(&entry) else {
                continue;
            };
            if !matches(&entry, &content) {
                continue;
            }
            let Some(command) = linux_desktop_entry_command(&content) else {
                continue;
            };
            if let Some(application) =
                resolve_linux_desktop_command(home, executable_directories, &command)
            {
                return Some(application);
            }
        }
    }
    None
}

#[cfg(any(target_os = "linux", test))]
fn is_named_desktop_entry(path: &Path, content: &str, identifier: &str, name: &str) -> bool {
    if path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.to_ascii_lowercase().contains(identifier))
    {
        return true;
    }
    content.lines().any(|line| {
        line.split_once('=').is_some_and(|(key, value)| {
            (key.trim() == "Name" && value.trim().eq_ignore_ascii_case(name))
                || (matches!(key.trim(), "Icon" | "StartupWMClass")
                    && value.trim().to_ascii_lowercase().contains(identifier))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_exec_preserves_windows_hosted_paths() {
        let application = "C:/tmp/cc-switch/home/.local/bin/chatgpt";
        let tokens = tokenize_linux_desktop_exec(&format!(
            "env CHATGPT_CHANNEL=stable \"{application}\" %U"
        ));
        assert_eq!(tokens[2], application);
    }

    #[test]
    fn nix_matching_requires_an_interpreter_and_main_entry() {
        let application = Path::new("/nix/store/hash-opencode-desktop-1.0/bin/opencode-desktop");
        let main = std::ffi::OsStr::new("/nix/store/hash-opencode-desktop-1.0/lib/app/main.js");
        let electron = std::ffi::OsStr::new("/nix/store/hash-electron/bin/electron");
        let node = std::ffi::OsStr::new("/nix/store/hash-node/bin/node");
        let other = std::ffi::OsStr::new("/tmp/unrelated.js");
        assert!(linux_nix_command_matches(
            &[electron, main],
            application,
            |_| None
        ));
        assert!(linux_nix_command_matches(
            &[node, main],
            application,
            |_| None
        ));
        assert!(!linux_nix_command_matches(
            &[electron, other],
            application,
            |_| None
        ));
        assert!(!linux_nix_command_matches(
            &[main, other],
            application,
            |_| None
        ));
        assert!(!linux_nix_command_matches(
            &[std::ffi::OsStr::new("/bin/sh"), other, main],
            application,
            |_| None
        ));
    }

    #[test]
    fn applescript_uses_positional_arguments() {
        assert!(MACOS_QUIT_SCRIPT.contains("item 1 of argv"));
        assert!(!MACOS_QUIT_SCRIPT.contains("tell application \""));
    }

    #[test]
    fn process_parser_does_not_match_arbitrary_arguments() {
        let ids = desktop_process_ids_from_ps(
            "12 /Applications/ChatGPT.app/Contents/MacOS/ChatGPT\n13 /Applications/Other.app/Contents/MacOS/Other\n",
            Path::new("/Applications/ChatGPT.app"),
        );
        assert_eq!(ids, vec![12]);
    }

    #[test]
    fn codex_linux_desktop_resolves_official_chatgpt_entry() {
        let root = portable_test_directory("codex-chatgpt");
        let home = root.join("home");
        let executable_directory = root.join("usr/bin");
        let data_directory = root.join("usr/share");
        let application = executable_directory.join("chatgpt");
        std::fs::create_dir_all(&executable_directory).unwrap();
        std::fs::create_dir_all(data_directory.join("applications")).unwrap();
        std::fs::write(&application, []).unwrap();
        std::fs::write(
            data_directory.join("applications/com.openai.chatgpt.desktop"),
            format!(
                "[Desktop Entry]\nName=ChatGPT\nIcon=com.openai.chatgpt\nExec=env CHATGPT_CHANNEL=stable \"{}\" %U\n",
                application.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        assert_eq!(
            find_linux_codex_desktop_application_in_roots(
                &home,
                &[executable_directory],
                &[data_directory],
                &[],
            ),
            Some(application)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_linux_desktop_rejects_the_codex_cli() {
        assert!(!is_chatgpt_desktop_command(
            "/tmp/home/.nvm/versions/node/bin/codex"
        ));
        assert!(!is_chatgpt_desktop_command("codex"));
        assert!(is_chatgpt_desktop_command("/usr/lib/chatgpt/ChatGPT"));
    }

    #[test]
    fn nix_matching_does_not_accept_a_later_arbitrary_entry() {
        let application = Path::new("/nix/store/hash-opencode-desktop-1.0/bin/opencode-desktop");
        let node = std::ffi::OsStr::new("/nix/store/hash-node/bin/node");
        let other = std::ffi::OsStr::new("/tmp/unrelated.js");
        let main = std::ffi::OsStr::new("/nix/store/hash-opencode-desktop-1.0/lib/app/main.js");
        assert!(!linux_nix_command_matches(
            &[node, other, main],
            application,
            |_| None
        ));
    }

    #[cfg(not(windows))]
    fn portable_test_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "cc-switch-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(windows)]
    fn portable_test_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("cc-switch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
