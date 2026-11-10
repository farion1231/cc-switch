#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux", test))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use std::process::{Command, Stdio};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppStatus {
    pub supported: bool,
    pub installed: bool,
    pub running: bool,
}

impl DesktopAppStatus {
    fn unsupported() -> Self {
        Self {
            supported: false,
            installed: false,
            running: false,
        }
    }
}

#[derive(Debug, Clone)]
enum DesktopAppTarget {
    Application(PathBuf),
    #[cfg(target_os = "windows")]
    WindowsAppId(String),
}

fn is_supported_app(app: &str) -> bool {
    matches!(app, "codex" | "claude-desktop" | "opencode")
}

fn app_label(app: &str) -> &'static str {
    match app {
        "codex" => "Codex",
        "claude-desktop" => "Claude Desktop",
        "opencode" => "OpenCode",
        _ => "Desktop App",
    }
}

#[tauri::command]
pub async fn get_desktop_app_status(app: String) -> Result<DesktopAppStatus, String> {
    match tauri::async_runtime::spawn_blocking(move || desktop_app_status(&app)).await {
        Ok(status) => status,
        Err(error) => Err(format!("检查桌面应用状态失败: {error}")),
    }
}

#[tauri::command]
pub async fn launch_desktop_app(app: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || launch_desktop_app_blocking(&app))
        .await
        .map_err(|error| format!("启动桌面应用失败: {error}"))?
}

#[tauri::command]
pub async fn restart_desktop_app(app: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || restart_desktop_app_blocking(&app))
        .await
        .map_err(|error| format!("重启桌面应用失败: {error}"))?
}

fn desktop_app_status(app: &str) -> Result<DesktopAppStatus, String> {
    if !is_supported_app(app) {
        return Ok(DesktopAppStatus::unsupported());
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    let status = {
        let target = find_desktop_app_target(app)?;
        let running = target
            .as_ref()
            .is_some_and(|target| is_target_running(target, app));
        DesktopAppStatus {
            supported: true,
            installed: target.is_some(),
            running,
        }
    };

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let status = DesktopAppStatus::unsupported();

    Ok(status)
}

fn launch_desktop_app_blocking(app: &str) -> Result<(), String> {
    let target = resolve_target(app)?;
    launch_target(&target, app_label(app))
}

fn restart_desktop_app_blocking(app: &str) -> Result<(), String> {
    let target = resolve_target(app)?;
    let label = app_label(app);
    stop_target(&target, app, label)?;
    launch_target(&target, label)
}

fn resolve_target(app: &str) -> Result<DesktopAppTarget, String> {
    if !is_supported_app(app) {
        return Err(format!("暂不支持启动 {} 桌面应用", app_label(app)));
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        find_desktop_app_target(app)?.ok_or_else(|| format!("未检测到 {} 桌面应用", app_label(app)))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err(format!("当前系统暂不支持启动 {} 桌面应用", app_label(app)))
    }
}

#[cfg(target_os = "windows")]
fn find_desktop_app_target(app: &str) -> Result<Option<DesktopAppTarget>, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取用户目录".to_string())?;
    Ok(match app {
        "codex" => find_windows_codex_app_installation(&home),
        "claude-desktop" => find_windows_claude_desktop_executable(&home)
            .map(DesktopAppTarget::Application)
            .or_else(|| {
                find_windows_appx_app_id(&["Claude", "Anthropic.Claude"])
                    .map(DesktopAppTarget::WindowsAppId)
            }),
        "opencode" => {
            find_windows_opencode_desktop_application(&home).map(DesktopAppTarget::Application)
        }
        _ => None,
    })
}

#[cfg(target_os = "windows")]
fn find_windows_codex_app_installation(home: &Path) -> Option<DesktopAppTarget> {
    find_windows_codex_app_executable(home)
        .map(DesktopAppTarget::Application)
        .or_else(find_windows_registered_codex_app_installation)
}

#[cfg(target_os = "windows")]
fn find_windows_registered_codex_app_installation() -> Option<DesktopAppTarget> {
    find_windows_appx_app_id(&["OpenAI.Codex", "OpenAI.CodexBeta", "OpenAI.ChatGPT"])
        .map(DesktopAppTarget::WindowsAppId)
        .or_else(|| {
            ["ChatGPT.exe", "Codex.exe"].into_iter().find_map(|name| {
                let path = windows_app_path_executable(name)?;
                if name.eq_ignore_ascii_case("Codex.exe") && is_codex_cli_lookalike(&path) {
                    None
                } else {
                    Some(DesktopAppTarget::Application(path))
                }
            })
        })
}

#[cfg(target_os = "windows")]
fn find_windows_codex_app_executable(home: &Path) -> Option<PathBuf> {
    let local = windows_local_app_data(home);
    let mut candidates = vec![
        local.join("Programs/OpenAI/ChatGPT/ChatGPT.exe"),
        local.join("Programs/ChatGPT/ChatGPT.exe"),
        local.join("OpenAI/ChatGPT/ChatGPT.exe"),
        local.join("OpenAI/Codex/Codex.exe"),
        local.join("Programs/OpenAI/Codex/Codex.exe"),
        local.join("Programs/Codex/Codex.exe"),
        local.join("Microsoft/WindowsApps/ChatGPT.exe"),
    ];
    for root in windows_program_files_roots() {
        candidates.extend([
            root.join("OpenAI/ChatGPT/ChatGPT.exe"),
            root.join("ChatGPT/ChatGPT.exe"),
            root.join("OpenAI/Codex/Codex.exe"),
            root.join("Codex/Codex.exe"),
        ]);
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
fn find_windows_claude_desktop_executable(home: &Path) -> Option<PathBuf> {
    let local = windows_local_app_data(home);
    [
        local.join("Programs/Claude/Claude.exe"),
        local.join("Claude/Claude.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
fn find_windows_opencode_desktop_application(home: &Path) -> Option<PathBuf> {
    find_windows_opencode_desktop_application_in_roots(
        &windows_local_app_data(home),
        &windows_program_files_roots(),
    )
}

#[cfg(any(target_os = "windows", test))]
fn find_windows_opencode_desktop_application_in_roots(
    local_app_data: &Path,
    program_files_roots: &[PathBuf],
) -> Option<PathBuf> {
    let mut candidates = vec![
        local_app_data.join("Programs/@opencode-aidesktop/OpenCode.exe"),
        local_app_data.join("Programs/OpenCode/OpenCode.exe"),
        local_app_data.join("Programs/opencode/OpenCode.exe"),
        local_app_data.join("OpenCode/OpenCode.exe"),
        local_app_data.join("opencode/OpenCode.exe"),
    ];
    for root in program_files_roots {
        candidates.extend([
            root.join("@opencode-aidesktop/OpenCode.exe"),
            root.join("OpenCode/OpenCode.exe"),
            root.join("opencode/OpenCode.exe"),
        ]);
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
fn windows_local_app_data(home: &Path) -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Local"))
}

#[cfg(target_os = "windows")]
fn windows_program_files_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(target_os = "windows")]
fn windows_app_path_executable(file_name: &str) -> Option<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    for (hive, prefix) in [
        (
            HKEY_CURRENT_USER,
            "Software\\Microsoft\\Windows\\CurrentVersion",
        ),
        (
            HKEY_CURRENT_USER,
            "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion",
        ),
        (
            HKEY_LOCAL_MACHINE,
            "Software\\Microsoft\\Windows\\CurrentVersion",
        ),
        (
            HKEY_LOCAL_MACHINE,
            "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion",
        ),
    ] {
        let root = RegKey::predef(hive);
        let Ok(key) = root.open_subkey(format!("{prefix}\\App Paths\\{file_name}")) else {
            continue;
        };
        let Ok(value) = key.get_value::<String, _>("") else {
            continue;
        };
        let expanded = expand_windows_environment_variables(&value);
        let path = PathBuf::from(expanded.trim().trim_matches('"'));
        if path.is_absolute() && path.is_file() {
            return Some(path);
        }
    }
    None
}

#[cfg(any(target_os = "windows", test))]
fn expand_windows_environment_variables(value: &str) -> String {
    let mut expanded = String::new();
    let mut remaining = value;
    while let Some(start) = remaining.find('%') {
        expanded.push_str(&remaining[..start]);
        let after_start = &remaining[start + 1..];
        let Some(end) = after_start.find('%') else {
            expanded.push_str(&remaining[start..]);
            return expanded;
        };
        let name = &after_start[..end];
        let replacement = std::env::var_os(name).or_else(|| {
            std::env::vars_os().find_map(|(key, value)| {
                key.to_string_lossy()
                    .eq_ignore_ascii_case(name)
                    .then_some(value)
            })
        });
        if let Some(replacement) = replacement {
            expanded.push_str(&replacement.to_string_lossy());
        } else {
            expanded.push('%');
            expanded.push_str(name);
            expanded.push('%');
        }
        remaining = &after_start[end + 1..];
    }
    expanded.push_str(remaining);
    expanded
}

#[cfg(any(target_os = "windows", test))]
fn is_codex_cli_lookalike(path: &Path) -> bool {
    let lowered = path_key(path);
    ["\\bin\\", "\\node_modules\\", "\\.vscode\\extensions\\"]
        .iter()
        .any(|marker| lowered.contains(marker))
}

#[cfg(target_os = "macos")]
fn find_desktop_app_target(app: &str) -> Result<Option<DesktopAppTarget>, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取用户目录".to_string())?;
    Ok(find_macos_desktop_app_target_in_roots(
        app,
        Path::new("/Applications"),
        &home.join("Applications"),
    ))
}

#[cfg(any(target_os = "macos", test))]
fn find_macos_desktop_app_target_in_roots(
    app: &str,
    system_applications: &Path,
    user_applications: &Path,
) -> Option<DesktopAppTarget> {
    let roots = [system_applications, user_applications];
    let path = match app {
        "codex" => roots
            .into_iter()
            .flat_map(|directory| {
                [
                    "ChatGPT.app",
                    "Codex.app",
                    "OpenAI Codex.app",
                    "OpenAI.Codex.app",
                ]
                .into_iter()
                .map(move |name| directory.join(name))
            })
            .find(|path| path.is_dir()),
        "claude-desktop" => roots
            .into_iter()
            .map(|directory| directory.join("Claude.app/Contents/MacOS/Claude"))
            .find(|path| path.is_file()),
        "opencode" => roots
            .into_iter()
            .map(|directory| directory.join("OpenCode.app"))
            .find(|path| path.is_dir()),
        _ => None,
    }?;
    Some(DesktopAppTarget::Application(path))
}

#[cfg(target_os = "linux")]
fn find_desktop_app_target(app: &str) -> Result<Option<DesktopAppTarget>, String> {
    if app == "codex" {
        return Ok(None);
    }
    let home = dirs::home_dir().ok_or_else(|| "无法获取用户目录".to_string())?;
    Ok(match app {
        "claude-desktop" => {
            find_linux_claude_desktop_executable(&home).map(DesktopAppTarget::Application)
        }
        "opencode" => {
            find_linux_opencode_desktop_application(&home).map(DesktopAppTarget::Application)
        }
        _ => None,
    })
}

#[cfg(target_os = "linux")]
fn find_linux_claude_desktop_executable(home: &Path) -> Option<PathBuf> {
    let mut candidates = linux_agent_executable_directories(home)
        .into_iter()
        .map(|directory| directory.join("claude-desktop"))
        .collect::<Vec<_>>();
    candidates.extend([
        PathBuf::from("/opt/Claude/claude-desktop"),
        PathBuf::from("/opt/Claude/claude"),
        PathBuf::from("/opt/claude-desktop/claude-desktop"),
    ]);
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(target_os = "linux")]
fn find_linux_opencode_desktop_application(home: &Path) -> Option<PathBuf> {
    let mut executable_directories = linux_agent_executable_directories(home);
    for directory in ["/opt/OpenCode", "/opt/opencode-desktop"] {
        let directory = PathBuf::from(directory);
        if !executable_directories.contains(&directory) {
            executable_directories.push(directory);
        }
    }

    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    let mut data_directories = vec![data_home];
    let system_data_directories = std::env::var_os("XDG_DATA_DIRS")
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .filter(|directories| !directories.is_empty())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    for directory in system_data_directories {
        if directory.is_absolute() && !data_directories.contains(&directory) {
            data_directories.push(directory);
        }
    }

    let appimage_directories = [
        home.join("Applications"),
        home.join(".local/Applications"),
        home.join("Downloads"),
        home.join("Desktop"),
    ];
    find_linux_opencode_desktop_application_in_roots(
        home,
        &executable_directories,
        &data_directories,
        &appimage_directories,
    )
}

#[cfg(target_os = "linux")]
fn linux_agent_executable_directories(home: &Path) -> Vec<PathBuf> {
    linux_agent_executable_directories_from_environment(home, |name| std::env::var_os(name))
}

#[cfg(any(target_os = "linux", test))]
fn linux_agent_executable_directories_from_environment(
    home: &Path,
    environment: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |path: PathBuf| {
        if !path.as_os_str().is_empty() && seen.insert(path.clone()) {
            directories.push(path);
        }
    };

    if let Some(path) = environment("PATH") {
        std::env::split_paths(&path).for_each(&mut push);
    }
    [
        home.join(".local/bin"),
        home.join(".npm-global/bin"),
        home.join(".bun/bin"),
        home.join(".cargo/bin"),
        home.join(".volta/bin"),
        home.join(".local/share/pnpm"),
        home.join(".pyenv/shims"),
        home.join("bin"),
    ]
    .into_iter()
    .for_each(&mut push);
    for variable in [
        "PNPM_HOME",
        "BUN_INSTALL",
        "NPM_CONFIG_PREFIX",
        "npm_config_prefix",
        "VOLTA_HOME",
        "UV_TOOL_BIN_DIR",
        "NVM_SYMLINK",
    ] {
        if let Some(path) = environment(variable).filter(|path| !path.is_empty()) {
            let path = PathBuf::from(path);
            push(
                if matches!(
                    variable,
                    "NPM_CONFIG_PREFIX" | "npm_config_prefix" | "BUN_INSTALL" | "VOLTA_HOME"
                ) {
                    path.join("bin")
                } else {
                    path
                },
            );
        }
    }
    for root in [
        home.join(".nvm/versions/node"),
        home.join(".local/state/fnm_multishells"),
    ] {
        if let Ok(entries) = std::fs::read_dir(root) {
            let mut entries = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>();
            entries.sort();
            for entry in entries.into_iter().rev() {
                push(entry.join("bin"));
            }
        }
    }
    for path in ["/usr/local/bin", "/usr/bin", "/bin"] {
        push(PathBuf::from(path));
    }
    directories
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

#[cfg(target_os = "windows")]
const APPX_PACKAGES_KEY: &str = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages";

#[cfg(target_os = "windows")]
struct AppxPackage {
    full_name: String,
    family_name: String,
    version: String,
    install_location: PathBuf,
}

#[cfg(target_os = "windows")]
fn find_windows_appx_packages(prefixes: &[&str]) -> Vec<AppxPackage> {
    let mut packages = list_appx_packages()
        .into_iter()
        .filter(|package| {
            package_matches_prefixes(&package.full_name, &package.family_name, prefixes)
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| compare_dotted_versions(&right.version, &left.version));
    packages
}

#[cfg(target_os = "windows")]
fn find_windows_appx_app_id(prefixes: &[&str]) -> Option<String> {
    let package = find_windows_appx_packages(prefixes).into_iter().next()?;
    let application_id =
        read_appx_application_id(&package.install_location).unwrap_or_else(|| "App".to_string());
    Some(format!("{}!{application_id}", package.family_name))
}

#[cfg(target_os = "windows")]
fn find_windows_appx_install_root(app_id: &str) -> Option<PathBuf> {
    let family = app_id.split('!').next().unwrap_or(app_id);
    find_windows_appx_packages(&[family])
        .into_iter()
        .find(|package| package.family_name.eq_ignore_ascii_case(family))
        .map(|package| package.install_location)
}

#[cfg(target_os = "windows")]
fn list_appx_packages() -> Vec<AppxPackage> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(APPX_PACKAGES_KEY) else {
        return Vec::new();
    };

    let mut packages = Vec::new();
    for full_name in root.enum_keys().flatten() {
        let Some(family_name) = family_name_from_full_name(&full_name) else {
            continue;
        };
        let Ok(key) = root.open_subkey(&full_name) else {
            continue;
        };
        let install_location = key
            .get_value::<String, _>("PackageRootFolder")
            .ok()
            .or_else(|| key.get_value::<String, _>("InstallLocation").ok())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        let Some(install_location) = install_location else {
            continue;
        };
        packages.push(AppxPackage {
            version: version_from_package_full_name(&full_name).unwrap_or_default(),
            full_name,
            family_name,
            install_location,
        });
    }
    packages
}

#[cfg(target_os = "windows")]
fn read_appx_application_id(install_root: &Path) -> Option<String> {
    let content = std::fs::read_to_string(install_root.join("AppxManifest.xml")).ok()?;
    let tag = first_xml_tag(&content, "Application")?;
    let id = xml_attr(tag, "Id")?.trim().to_string();
    (!id.is_empty()).then_some(id)
}

#[cfg(any(target_os = "windows", test))]
fn family_name_from_full_name(full_name: &str) -> Option<String> {
    let name = full_name.split('_').next()?.trim();
    let publisher_id = full_name.rsplit('_').next()?.trim();
    if name.is_empty() || publisher_id.is_empty() || publisher_id == name {
        return None;
    }
    Some(format!("{name}_{publisher_id}"))
}

#[cfg(any(target_os = "windows", test))]
fn version_from_package_full_name(full_name: &str) -> Option<String> {
    let version = full_name.split('_').nth(1)?.trim();
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}

#[cfg(any(target_os = "windows", test))]
fn package_matches_prefixes(full_name: &str, family_name: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| {
        full_name == *prefix
            || full_name.starts_with(&format!("{prefix}_"))
            || family_name == *prefix
            || family_name.starts_with(&format!("{prefix}_"))
    })
}

#[cfg(any(target_os = "windows", test))]
fn compare_dotted_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let parse = |value: &str| {
        value
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let left = parse(left);
    let right = parse(right);
    left.into_iter()
        .zip(right.iter().copied().chain(std::iter::repeat(0)))
        .find_map(|(left, right)| (left != right).then_some(left.cmp(&right)))
        .unwrap_or(std::cmp::Ordering::Equal)
}

#[cfg(target_os = "windows")]
fn first_xml_tag<'a>(content: &'a str, local_name: &str) -> Option<&'a str> {
    let mut search = content;
    while let Some(start) = search.find('<') {
        let rest = &search[start + 1..];
        if rest.starts_with('/') || rest.starts_with('!') || rest.starts_with('?') {
            search = rest;
            continue;
        }
        let name_end = rest
            .find(|character: char| {
                character.is_whitespace() || character == '>' || character == '/'
            })
            .unwrap_or(rest.len());
        let tag_name = &rest[..name_end];
        let local = tag_name
            .rsplit_once(':')
            .map(|(_, name)| name)
            .unwrap_or(tag_name);
        if local.eq_ignore_ascii_case(local_name) {
            let close = rest.find('>')?;
            return Some(&rest[..close]);
        }
        search = rest;
    }
    None
}

#[cfg(target_os = "windows")]
fn xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

#[cfg(any(target_os = "windows", test))]
fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_ascii_lowercase()
}

#[cfg(any(target_os = "windows", test))]
fn windows_process_matches_stop_target(
    process_path: &Path,
    executable: Option<&Path>,
    install_root: Option<&Path>,
    image_names: &[&str],
) -> bool {
    if !image_names.is_empty() {
        let file_name = process_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !image_names
            .iter()
            .any(|name| file_name.eq_ignore_ascii_case(name))
        {
            return false;
        }
    }
    let process_key = path_key(process_path);
    if let Some(executable) = executable {
        if process_key == path_key(executable) {
            return true;
        }
    }
    if let Some(root) = install_root {
        let root_key = path_key(root);
        if !root_key.is_empty()
            && (process_key == root_key || process_key.starts_with(&format!("{root_key}\\")))
        {
            return true;
        }
    }
    false
}

#[cfg(target_os = "windows")]
fn windows_desktop_stop_target(
    target: &DesktopAppTarget,
) -> Result<(Option<PathBuf>, Option<PathBuf>), String> {
    match target {
        DesktopAppTarget::Application(path) => Ok((Some(path.clone()), None)),
        DesktopAppTarget::WindowsAppId(app_id) => find_windows_appx_install_root(app_id)
            .map(|install_root| (None, Some(install_root)))
            .ok_or_else(|| "未找到应用包目录".to_string()),
    }
}

#[cfg(target_os = "windows")]
fn windows_image_names(app: &str) -> &'static [&'static str] {
    match app {
        "codex" => &["ChatGPT.exe", "Codex.exe"],
        "opencode" => &["OpenCode.exe"],
        _ => &[],
    }
}

#[cfg(target_os = "windows")]
fn is_target_running(target: &DesktopAppTarget, app: &str) -> bool {
    let Ok((executable, install_root)) = windows_desktop_stop_target(target) else {
        return false;
    };
    !matching_process_ids(
        executable.as_deref(),
        install_root.as_deref(),
        windows_image_names(app),
    )
    .is_empty()
}

#[cfg(target_os = "windows")]
fn matching_process_ids(
    executable: Option<&Path>,
    install_root: Option<&Path>,
    image_names: &[&str],
) -> Vec<u32> {
    process_snapshot()
        .into_iter()
        .filter(|(_, path)| {
            windows_process_matches_stop_target(path, executable, install_root, image_names)
        })
        .map(|(process_id, _)| process_id)
        .collect()
}

#[cfg(target_os = "windows")]
fn process_snapshot() -> Vec<(u32, PathBuf)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }

    let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut processes = Vec::new();
    if unsafe { Process32FirstW(snapshot, &mut entry) } != 0 {
        loop {
            if entry.th32ProcessID != 0 {
                if let Some(path) = process_executable_path(entry.th32ProcessID) {
                    processes.push((entry.th32ProcessID, path));
                }
            }
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }
    }

    unsafe {
        CloseHandle(snapshot);
    }
    processes
}

#[cfg(target_os = "windows")]
fn process_executable_path(process_id: u32) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }

    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    let success =
        unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    unsafe {
        CloseHandle(handle);
    }
    if success == 0 {
        return None;
    }

    buffer.truncate(length as usize);
    Some(PathBuf::from(OsString::from_wide(&buffer)))
}

#[cfg(target_os = "windows")]
fn terminate_process(process_id: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, process_id) };
    if handle.is_null() {
        return;
    }
    unsafe {
        TerminateProcess(handle, 1);
        CloseHandle(handle);
    }
}

#[cfg(target_os = "windows")]
fn stop_target(target: &DesktopAppTarget, app: &str, label: &str) -> Result<(), String> {
    let (executable, install_root) = windows_desktop_stop_target(target)?;
    let image_names = windows_image_names(app);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

    loop {
        let remaining =
            matching_process_ids(executable.as_deref(), install_root.as_deref(), image_names);
        if remaining.is_empty() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            let ids = remaining
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!("{label} 没有完全退出，剩余进程: {ids}"));
        }
        for process_id in remaining {
            terminate_process(process_id);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(target_os = "windows")]
fn launch_target(target: &DesktopAppTarget, label: &str) -> Result<(), String> {
    match target {
        DesktopAppTarget::Application(path) => {
            let mut command = Command::new(path);
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            hide_console(&mut command);
            command
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("启动 {label} 失败: {error}"))
        }
        DesktopAppTarget::WindowsAppId(app_id) => {
            let mut command = Command::new(windows_explorer_executable());
            command
                .arg(format!("shell:AppsFolder\\{app_id}"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            hide_console(&mut command);
            command
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("启动 {label} 失败: {error}"))
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_explorer_executable() -> PathBuf {
    let system_root = std::env::var_os("SystemRoot")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let executable = system_root.join("explorer.exe");
    if executable.is_file() {
        executable
    } else {
        PathBuf::from("explorer.exe")
    }
}

#[cfg(target_os = "windows")]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
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
fn macos_processes_for_application(application: &Path) -> Result<Vec<i32>, String> {
    let output = Command::new("ps")
        .args(["-axww", "-o", "pid=", "-o", "comm="])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("读取进程列表失败: {error}"))?;
    if !output.status.success() {
        return Err("读取进程列表失败".to_string());
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
fn macos_request_quit(application: &Path) {
    let bundle = application
        .ancestors()
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("app"))
        .unwrap_or(application);
    let escaped = bundle
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let Ok(mut child) = Command::new("osascript")
        .args(["-e", &format!("tell application \"{escaped}\" to quit")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(target_os = "macos")]
fn macos_signal_process(process_id: i32, signal: i32) -> Result<(), String> {
    if unsafe { libc::kill(process_id, signal) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!("结束进程 {process_id} 失败: {error}"))
    }
}

#[cfg(target_os = "macos")]
fn is_target_running(target: &DesktopAppTarget, _app: &str) -> bool {
    let DesktopAppTarget::Application(application) = target;
    macos_processes_for_application(application)
        .map(|ids| !ids.is_empty())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn stop_target(target: &DesktopAppTarget, _app: &str, label: &str) -> Result<(), String> {
    let DesktopAppTarget::Application(application) = target;
    if macos_processes_for_application(application)?.is_empty() {
        return Ok(());
    }

    macos_request_quit(application);
    if macos_wait_for_exit(application, 50)? {
        return Ok(());
    }

    for process_id in macos_processes_for_application(application)? {
        macos_signal_process(process_id, libc::SIGKILL)?;
    }
    if macos_wait_for_exit(application, 10)? {
        return Ok(());
    }

    Err(format!("{label} 没有完全退出"))
}

#[cfg(target_os = "macos")]
fn launch_target(target: &DesktopAppTarget, label: &str) -> Result<(), String> {
    let DesktopAppTarget::Application(application) = target;
    let bundle = application
        .ancestors()
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("app"))
        .unwrap_or(application);
    Command::new("open")
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("启动 {label} 失败: {error}"))
}

#[cfg(target_os = "linux")]
fn is_target_running(target: &DesktopAppTarget, _app: &str) -> bool {
    let DesktopAppTarget::Application(application) = target;
    let application =
        std::fs::canonicalize(application).unwrap_or_else(|_| application.to_path_buf());
    !linux_processes_for_application(&application).is_empty()
}

#[cfg(target_os = "linux")]
fn stop_target(target: &DesktopAppTarget, _app: &str, label: &str) -> Result<(), String> {
    let DesktopAppTarget::Application(application) = target;
    stop_linux_desktop_application(application, label)
}

#[cfg(target_os = "linux")]
fn stop_linux_desktop_application(application: &Path, label: &str) -> Result<(), String> {
    let application =
        std::fs::canonicalize(application).unwrap_or_else(|_| application.to_path_buf());
    let processes = linux_processes_for_application(&application);
    let mut signal_error = None;
    for process_id in processes {
        if let Err(error) = signal_linux_process(process_id, libc::SIGTERM) {
            signal_error.get_or_insert(error);
        }
    }

    for _ in 0..50 {
        if linux_processes_for_application(&application).is_empty() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let remaining = linux_processes_for_application(&application);
    for process_id in &remaining {
        if let Err(error) = signal_linux_process(*process_id, libc::SIGKILL) {
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
            "{label} 没有完全退出，剩余进程: {}",
            remaining
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))
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
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let process_directory = PathBuf::from(format!("/proc/{process_id}"));
    if std::fs::read_link(process_directory.join("exe"))
        .ok()
        .is_some_and(|path| linux_process_path_matches(&path, application))
    {
        return true;
    }

    if let Ok(command_line) = std::fs::read(process_directory.join("cmdline")) {
        let installation_root = linux_opencode_nix_installation_root(application);
        if command_line
            .split(|byte| *byte == 0)
            .filter(|argument| !argument.is_empty())
            .map(|argument| Path::new(OsStr::from_bytes(argument)))
            .enumerate()
            .any(|(index, argument)| {
                (index == 0 && linux_process_path_matches(argument, application))
                    || installation_root.is_some_and(|root| {
                        std::fs::canonicalize(argument)
                            .ok()
                            .is_some_and(|argument| argument.starts_with(root))
                    })
            })
        {
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
                            Path::new(OsStr::from_bytes(path)),
                            application,
                        )
                })
        })
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
fn signal_linux_process(process_id: i32, signal: i32) -> Result<(), String> {
    if unsafe { libc::kill(process_id, signal) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!("结束桌面应用进程 {process_id} 失败: {error}"))
    }
}

#[cfg(target_os = "linux")]
fn launch_target(target: &DesktopAppTarget, label: &str) -> Result<(), String> {
    let DesktopAppTarget::Application(application) = target;
    Command::new(application)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("启动 {label} 失败: {error}"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn stop_target(_target: &DesktopAppTarget, _app: &str, _label: &str) -> Result<(), String> {
    Err("当前系统暂不支持重启桌面应用".to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn launch_target(_target: &DesktopAppTarget, _label: &str) -> Result<(), String> {
    Err("当前系统暂不支持启动桌面应用".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODEX_FULL_NAME: &str = "OpenAI.Codex_26.1103.4239.0_x64__2p2nqsd0c76g0";

    fn application_path(target: Option<DesktopAppTarget>) -> Option<PathBuf> {
        match target {
            Some(DesktopAppTarget::Application(path)) => Some(path),
            #[cfg(target_os = "windows")]
            Some(DesktopAppTarget::WindowsAppId(_)) => None,
            None => None,
        }
    }

    #[test]
    fn parses_package_family_name() {
        assert_eq!(
            family_name_from_full_name(CODEX_FULL_NAME).as_deref(),
            Some("OpenAI.Codex_2p2nqsd0c76g0")
        );
        assert_eq!(
            family_name_from_full_name("Claude_1.46388.1.0_x64__pzs8sxrjxfjjc").as_deref(),
            Some("Claude_pzs8sxrjxfjjc")
        );
        assert_eq!(family_name_from_full_name("NoUnderscores"), None);
    }

    #[test]
    fn parses_package_version() {
        assert_eq!(
            version_from_package_full_name(CODEX_FULL_NAME).as_deref(),
            Some("26.1103.4239.0")
        );
    }

    #[test]
    fn compares_versions_numerically() {
        use std::cmp::Ordering;
        assert_eq!(
            compare_dotted_versions("1.10.0", "1.9.9"),
            Ordering::Greater
        );
        assert_eq!(compare_dotted_versions("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_dotted_versions("0.9", "1.0"), Ordering::Less);
    }

    #[test]
    fn matches_package_prefixes() {
        assert!(package_matches_prefixes(
            CODEX_FULL_NAME,
            "OpenAI.Codex_2p2nqsd0c76g0",
            &["OpenAI.Codex"]
        ));
        assert!(!package_matches_prefixes(
            "OpenAI.CodexBeta_1.0.0.0_x64__abc",
            "OpenAI.CodexBeta_abc",
            &["OpenAI.Codex"]
        ));
        assert!(package_matches_prefixes(
            "Other.Package_1.0.0.0_x64__abc",
            "OpenAI.CodexBeta_abc",
            &["OpenAI.CodexBeta"]
        ));
    }

    #[test]
    fn windows_process_matching_is_scoped_to_the_detected_installation() {
        let executable = PathBuf::from(r"C:\Apps\Tool\Tool.exe");
        assert!(windows_process_matches_stop_target(
            Path::new(r"c:/apps/tool/TOOL.EXE"),
            Some(&executable),
            None,
            &[],
        ));
        assert!(!windows_process_matches_stop_target(
            Path::new(r"C:\Apps\Tool.old\Tool.exe"),
            Some(&executable),
            None,
            &[],
        ));
        let install_root = PathBuf::from(r"C:\WA\OpenAI.Codex_26.1_x64__2p");
        assert!(windows_process_matches_stop_target(
            Path::new(r"C:\WA\OpenAI.Codex_26.1_x64__2p\ChatGPT.exe"),
            None,
            Some(&install_root),
            &["ChatGPT.exe", "Codex.exe"],
        ));
        assert!(!windows_process_matches_stop_target(
            Path::new(r"C:\WA\OpenAI.Codex_26.1_x64__2p\helper.exe"),
            None,
            Some(&install_root),
            &["ChatGPT.exe", "Codex.exe"],
        ));
    }

    #[test]
    fn codex_cli_lookalikes_are_rejected() {
        assert!(is_codex_cli_lookalike(Path::new(
            r"C:\Users\tester\AppData\Roaming\npm\node_modules\codex\Codex.exe"
        )));
        assert!(is_codex_cli_lookalike(Path::new(
            r"C:\Users\tester\.vscode\extensions\openai.codex\bin\Codex.exe"
        )));
        assert!(!is_codex_cli_lookalike(Path::new(
            r"C:\Users\tester\AppData\Local\Programs\Codex\Codex.exe"
        )));
    }

    #[test]
    fn opencode_windows_desktop_is_found_in_the_electron_installer_directory() {
        let root = tempfile::tempdir().unwrap();
        let local_app_data = root.path().join("AppData/Local");
        let application = local_app_data.join("Programs/@opencode-aidesktop/OpenCode.exe");
        std::fs::create_dir_all(application.parent().unwrap()).unwrap();
        std::fs::write(&application, []).unwrap();

        assert_eq!(
            find_windows_opencode_desktop_application_in_roots(&local_app_data, &[]),
            Some(application)
        );
    }

    #[test]
    fn macos_targets_follow_easy_detection_priority() {
        let root = tempfile::tempdir().unwrap();
        let system = root.path().join("system");
        let user = root.path().join("user");
        let system_chatgpt = system.join("ChatGPT.app");
        let system_codex = system.join("Codex.app");
        let user_opencode = user.join("OpenCode.app");
        let user_claude = user.join("Claude.app/Contents/MacOS/Claude");
        std::fs::create_dir_all(&system_chatgpt).unwrap();
        std::fs::create_dir_all(&system_codex).unwrap();
        std::fs::create_dir_all(&user_opencode).unwrap();
        std::fs::create_dir_all(user_claude.parent().unwrap()).unwrap();
        std::fs::write(&user_claude, []).unwrap();

        assert_eq!(
            application_path(find_macos_desktop_app_target_in_roots(
                "codex", &system, &user
            )),
            Some(system_chatgpt)
        );
        assert_eq!(
            application_path(find_macos_desktop_app_target_in_roots(
                "claude-desktop",
                &system,
                &user,
            )),
            Some(user_claude)
        );
        assert_eq!(
            application_path(find_macos_desktop_app_target_in_roots(
                "opencode", &system, &user
            )),
            Some(user_opencode)
        );
    }

    #[test]
    fn macos_process_matching_is_scoped_to_the_bundle() {
        let application = Path::new("/Applications/Claude.app/Contents/MacOS/Claude");
        let output = "10 /Applications/Claude.app/Contents/MacOS/Claude\n11 /Applications/Claude.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper\n12 /Applications/Claude.app.old/Contents/MacOS/Claude\n13 /Users/test/Claude.app/Contents/MacOS/Claude\n14 /usr/local/bin/claude\n";
        assert_eq!(
            desktop_process_ids_from_ps(output, application),
            vec![10, 11]
        );
        assert!(desktop_process_ids_from_ps("bad input\n-1 /opt/desktop", application).is_empty());
    }

    #[test]
    fn opencode_linux_desktop_finds_the_current_package_binary() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let executable_directory = root.path().join("opt/OpenCode");
        let application = executable_directory.join("ai.opencode.desktop");
        std::fs::create_dir_all(&executable_directory).unwrap();
        std::fs::write(&application, []).unwrap();

        assert_eq!(
            find_linux_opencode_desktop_application_in_roots(
                &home,
                &[executable_directory],
                &[],
                &[],
            ),
            Some(application)
        );
    }

    #[test]
    fn linux_agent_executable_directories_follow_easy_order() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let first = root.path().join("first");
        let second = root.path().join("second");
        let prefix = root.path().join("prefix");
        let bun = root.path().join("bun");
        let path = std::env::join_paths([&first, &second]).unwrap();
        let environment = std::collections::HashMap::from([
            ("PATH", path),
            ("NPM_CONFIG_PREFIX", prefix.clone().into_os_string()),
            ("BUN_INSTALL", bun.clone().into_os_string()),
        ]);

        let directories = linux_agent_executable_directories_from_environment(&home, |name| {
            environment.get(name).cloned()
        });

        assert_eq!(&directories[..2], &[first, second]);
        assert!(directories.contains(&home.join(".local/bin")));
        assert!(directories.contains(&prefix.join("bin")));
        assert!(directories.contains(&bun.join("bin")));
    }

    #[test]
    fn opencode_linux_desktop_resolves_an_appimage_desktop_entry() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let data_directory = root.path().join("share");
        let applications = data_directory.join("applications");
        let application = home.join("Applications/opencode-desktop-linux-amd64.AppImage");
        std::fs::create_dir_all(&applications).unwrap();
        std::fs::create_dir_all(application.parent().unwrap()).unwrap();
        std::fs::write(&application, []).unwrap();
        let application = PathBuf::from(application.to_string_lossy().replace('\\', "/"));
        std::fs::write(
            applications.join("ai.opencode.desktop.desktop"),
            format!(
                "[Desktop Entry]\nName=OpenCode\nExec=env OPEN_CODE=1 \"{}\" %U\nType=Application\n",
                application.to_string_lossy()
            ),
        )
        .unwrap();

        assert_eq!(
            find_linux_opencode_desktop_application_in_roots(&home, &[], &[data_directory], &[],),
            Some(application)
        );
    }

    #[test]
    fn opencode_linux_desktop_finds_an_unregistered_appimage() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let applications = home.join("Applications");
        let ignored = applications.join("OpenCode.AppImage");
        let application = applications.join("opencode-desktop-linux-aarch64.AppImage");
        std::fs::create_dir_all(&applications).unwrap();
        std::fs::write(&ignored, []).unwrap();
        std::fs::write(&application, []).unwrap();

        assert_eq!(
            find_linux_opencode_desktop_application_in_roots(&home, &[], &[], &[applications],),
            Some(application)
        );
    }

    #[test]
    fn linux_desktop_entry_excludes_unsupported_wrappers() {
        for command in [
            "flatpak run ai.opencode.desktop",
            "snap run opencode",
            "gtk-launch ai.opencode.desktop",
            "sh -c opencode-desktop",
            "bash -lc opencode-desktop",
        ] {
            assert_eq!(
                linux_desktop_entry_command(&format!(
                    "[Desktop Entry]\nName=OpenCode\nExec={command}\n"
                )),
                None
            );
        }
    }

    #[test]
    fn linux_desktop_entry_parser_handles_quotes_escapes_and_sections() {
        let content = "[Desktop Action New]\nExec=flatpak run wrong\n[Desktop Entry]\nName=OpenCode\nExec=env A=1 \"/opt/OpenCode/OpenCode Desktop\" --label escaped\\ value %U\n";
        assert_eq!(
            linux_desktop_entry_command(content).as_deref(),
            Some("/opt/OpenCode/OpenCode Desktop")
        );
        assert_eq!(
            tokenize_linux_desktop_exec("one \"two three\" four\\ five"),
            vec!["one", "two three", "four five"]
        );
    }

    #[test]
    fn opencode_desktop_entry_detection_uses_identity_fields() {
        assert!(is_opencode_desktop_entry(
            Path::new("generic.desktop"),
            "[Desktop Entry]\nName=OpenCode Beta\nExec=/opt/OpenCode/app\n"
        ));
        assert!(is_opencode_desktop_entry(
            Path::new("generic.desktop"),
            "[Desktop Entry]\nName=Other\nStartupWMClass=ai.opencode.desktop\n"
        ));
        assert!(!is_opencode_desktop_entry(
            Path::new("generic.desktop"),
            "[Desktop Entry]\nName=Other\nIcon=other\n"
        ));
    }

    #[test]
    fn linux_opencode_nix_root_is_scoped_to_opencode_store_entries() {
        assert_eq!(
            linux_opencode_nix_installation_root(Path::new(
                "/nix/store/hash-opencode-desktop-1.18.16/bin/opencode-desktop"
            )),
            Some(Path::new("/nix/store/hash-opencode-desktop-1.18.16"))
        );
        assert_eq!(
            linux_opencode_nix_installation_root(Path::new(
                "/nix/store/hash-other-desktop/bin/opencode-desktop"
            )),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_codex_is_supported_but_not_installed() {
        let status = desktop_app_status("codex").unwrap();
        assert!(status.supported);
        assert!(!status.installed);
        assert!(!status.running);
    }
}
