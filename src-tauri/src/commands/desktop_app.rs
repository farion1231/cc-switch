#[cfg(any(target_os = "windows", target_os = "macos", test))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(any(target_os = "windows", target_os = "macos"))]
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
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
enum DesktopAppTarget {
    Application(PathBuf),
    #[cfg(target_os = "windows")]
    WindowsApp {
        app_id: String,
        install_root: PathBuf,
    },
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

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let status = {
        let target = find_desktop_app_target(app)?;
        let running = target.as_ref().is_some_and(is_target_running);
        DesktopAppStatus {
            supported: true,
            installed: target.is_some(),
            running,
        }
    };

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
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
    stop_target(&target, label)?;
    launch_target(&target, label)
}

fn resolve_target(app: &str) -> Result<DesktopAppTarget, String> {
    if !is_supported_app(app) {
        return Err(format!("暂不支持启动 {} 桌面应用", app_label(app)));
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        find_desktop_app_target(app)?.ok_or_else(|| format!("未检测到 {} 桌面应用", app_label(app)))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(format!("当前系统暂不支持启动 {} 桌面应用", app_label(app)))
    }
}

#[cfg(target_os = "windows")]
fn find_desktop_app_target(app: &str) -> Result<Option<DesktopAppTarget>, String> {
    let prefixes: &[&str] = match app {
        "codex" => &["OpenAI.Codex", "OpenAI.CodexBeta", "OpenAI.ChatGPT"],
        "claude-desktop" => &["Claude", "Anthropic.Claude"],
        "opencode" => &["OpenCode", "OpenCode.OpenCode"],
        _ => return Ok(None),
    };

    if let Some((app_id, install_root)) = find_appx_target(prefixes) {
        return Ok(Some(DesktopAppTarget::WindowsApp {
            app_id,
            install_root,
        }));
    }

    Ok(find_application_path(app).map(DesktopAppTarget::Application))
}

#[cfg(target_os = "windows")]
fn application_candidates(app: &str) -> Vec<PathBuf> {
    let local = env_path("LOCALAPPDATA");
    let program_files = env_path("ProgramFiles");
    let mut candidates = Vec::new();

    let mut push = |base: Option<&Path>, relative: &str| {
        if let Some(base) = base {
            candidates.push(base.join(relative));
        }
    };

    match app {
        "codex" => {
            for relative in [
                "Programs\\OpenAI\\ChatGPT\\ChatGPT.exe",
                "Programs\\ChatGPT\\ChatGPT.exe",
                "OpenAI\\ChatGPT\\ChatGPT.exe",
                "Programs\\OpenAI\\Codex\\Codex.exe",
                "Programs\\Codex\\Codex.exe",
                "OpenAI\\Codex\\Codex.exe",
            ] {
                push(local.as_deref(), relative);
            }
            for relative in [
                "OpenAI\\ChatGPT\\ChatGPT.exe",
                "ChatGPT\\ChatGPT.exe",
                "OpenAI\\Codex\\Codex.exe",
                "Codex\\Codex.exe",
            ] {
                push(program_files.as_deref(), relative);
            }
        }
        "claude-desktop" => {
            for relative in [
                "Programs\\Claude\\Claude.exe",
                "Claude\\Claude.exe",
                "AnthropicClaude\\Claude.exe",
            ] {
                push(local.as_deref(), relative);
            }
            for relative in ["Claude\\Claude.exe", "AnthropicClaude\\Claude.exe"] {
                push(program_files.as_deref(), relative);
            }
        }
        "opencode" => {
            for relative in [
                "Programs\\@opencode-aidesktop\\OpenCode.exe",
                "Programs\\OpenCode\\OpenCode.exe",
                "Programs\\opencode\\OpenCode.exe",
                "OpenCode\\OpenCode.exe",
                "opencode\\OpenCode.exe",
                "@opencode-aidesktop\\OpenCode.exe",
            ] {
                push(local.as_deref(), relative);
            }
            for relative in [
                "@opencode-aidesktop\\OpenCode.exe",
                "OpenCode\\OpenCode.exe",
                "opencode\\OpenCode.exe",
            ] {
                push(program_files.as_deref(), relative);
            }
        }
        _ => {}
    }

    candidates
}

#[cfg(target_os = "windows")]
fn find_application_path(app: &str) -> Option<PathBuf> {
    application_candidates(app)
        .into_iter()
        .find(|path| path.is_file() && !is_cli_lookalike(path))
}

#[cfg(target_os = "windows")]
fn is_cli_lookalike(path: &Path) -> bool {
    let is_codex = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("Codex.exe"));
    if !is_codex {
        return false;
    }
    let lowered = path_key(path);
    ["\\bin\\", "\\node_modules\\", "\\.vscode\\extensions\\"]
        .iter()
        .any(|marker| lowered.contains(marker))
}

#[cfg(target_os = "windows")]
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(target_os = "macos")]
fn find_desktop_app_target(app: &str) -> Result<Option<DesktopAppTarget>, String> {
    Ok(macos_bundle_candidates(app)
        .into_iter()
        .find(|path| path.is_dir())
        .map(DesktopAppTarget::Application))
}

#[cfg(any(target_os = "macos", test))]
fn macos_bundle_names(app: &str) -> &'static [&'static str] {
    match app {
        "codex" => &[
            "Codex.app",
            "ChatGPT.app",
            "OpenAI Codex.app",
            "OpenAI.Codex.app",
        ],
        "claude-desktop" => &["Claude.app", "Claude Desktop.app"],
        "opencode" => &["OpenCode.app", "opencode.app"],
        _ => &[],
    }
}

#[cfg(target_os = "macos")]
fn macos_bundle_candidates(app: &str) -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    roots
        .into_iter()
        .flat_map(|root| {
            macos_bundle_names(app)
                .iter()
                .map(move |name| root.join(name))
        })
        .collect()
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
fn find_appx_target(prefixes: &[&str]) -> Option<(String, PathBuf)> {
    let mut packages = list_appx_packages()
        .into_iter()
        .filter(|package| {
            package.install_location.is_dir()
                && package_matches_prefixes(&package.full_name, &package.family_name, prefixes)
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| compare_dotted_versions(&right.version, &left.version));

    let package = packages.into_iter().next()?;
    let application_id =
        read_appx_application_id(&package.install_location).unwrap_or_else(|| "App".to_string());
    Some((
        format!("{}!{application_id}", package.family_name),
        package.install_location,
    ))
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
fn process_matches_target(process_path: &Path, target: &DesktopAppTarget) -> bool {
    match target {
        DesktopAppTarget::Application(executable) => path_key(process_path) == path_key(executable),
        #[cfg(target_os = "windows")]
        DesktopAppTarget::WindowsApp { install_root, .. } => {
            let process_key = path_key(process_path);
            let root_key = path_key(install_root);
            !root_key.is_empty()
                && (process_key == root_key || process_key.starts_with(&format!("{root_key}\\")))
        }
    }
}

#[cfg(target_os = "windows")]
fn is_target_running(target: &DesktopAppTarget) -> bool {
    process_snapshot()
        .iter()
        .any(|(_, path)| process_matches_target(path, target))
}

#[cfg(target_os = "windows")]
fn matching_process_ids(target: &DesktopAppTarget) -> Vec<u32> {
    process_snapshot()
        .into_iter()
        .filter(|(_, path)| process_matches_target(path, target))
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
            if let Some(path) = process_executable_path(entry.th32ProcessID) {
                processes.push((entry.th32ProcessID, path));
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
fn stop_target(target: &DesktopAppTarget, label: &str) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

    loop {
        let remaining = matching_process_ids(target);
        if remaining.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(150));
            if matching_process_ids(target).is_empty() {
                return Ok(());
            }
        } else if std::time::Instant::now() >= deadline {
            let ids = remaining
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!("{label} 没有完全退出，剩余进程: {ids}"));
        } else {
            for process_id in remaining {
                terminate_process(process_id);
            }
        }

        if std::time::Instant::now() >= deadline {
            return Err(format!("{label} 没有完全退出"));
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
        DesktopAppTarget::WindowsApp { app_id, .. } => {
            let mut command = Command::new("explorer.exe");
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
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(any(target_os = "macos", test))]
fn macos_process_ids_from_ps(output: &str, bundle: &Path) -> Vec<i32> {
    let current_process = std::process::id() as i32;
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let split = line.find(char::is_whitespace)?;
            let process_id = line[..split].parse::<i32>().ok()?;
            if process_id <= 0 || process_id == current_process {
                return None;
            }
            let executable = Path::new(line[split..].trim());
            (executable == bundle || executable.starts_with(bundle)).then_some(process_id)
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_process_ids(bundle: &Path) -> Result<Vec<i32>, String> {
    let output = Command::new("ps")
        .args(["-axww", "-o", "pid=", "-o", "comm="])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("读取进程列表失败: {error}"))?;
    if !output.status.success() {
        return Err("读取进程列表失败".to_string());
    }
    Ok(macos_process_ids_from_ps(
        &String::from_utf8_lossy(&output.stdout),
        bundle,
    ))
}

#[cfg(target_os = "macos")]
fn macos_wait_for_exit(bundle: &Path, attempts: u32) -> Result<bool, String> {
    for _ in 0..attempts {
        if macos_process_ids(bundle)?.is_empty() {
            return Ok(true);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(macos_process_ids(bundle)?.is_empty())
}

#[cfg(target_os = "macos")]
fn macos_request_quit(bundle: &Path) {
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
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
fn is_target_running(target: &DesktopAppTarget) -> bool {
    let DesktopAppTarget::Application(bundle) = target;
    macos_process_ids(bundle)
        .map(|ids| !ids.is_empty())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn stop_target(target: &DesktopAppTarget, label: &str) -> Result<(), String> {
    let DesktopAppTarget::Application(bundle) = target;
    if macos_process_ids(bundle)?.is_empty() {
        return Ok(());
    }

    macos_request_quit(bundle);
    if macos_wait_for_exit(bundle, 50)? {
        return Ok(());
    }

    for process_id in macos_process_ids(bundle)? {
        macos_signal_process(process_id, libc::SIGTERM)?;
    }
    if macos_wait_for_exit(bundle, 20)? {
        return Ok(());
    }

    for process_id in macos_process_ids(bundle)? {
        macos_signal_process(process_id, libc::SIGKILL)?;
    }
    if macos_wait_for_exit(bundle, 20)? {
        return Ok(());
    }

    let ids = macos_process_ids(bundle)?
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!("{label} 没有完全退出，剩余进程: {ids}"))
}

#[cfg(target_os = "macos")]
fn launch_target(target: &DesktopAppTarget, label: &str) -> Result<(), String> {
    let DesktopAppTarget::Application(bundle) = target;
    Command::new("open")
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("启动 {label} 失败: {error}"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn stop_target(_target: &DesktopAppTarget, _label: &str) -> Result<(), String> {
    Err("当前系统暂不支持重启桌面应用".to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn launch_target(_target: &DesktopAppTarget, _label: &str) -> Result<(), String> {
    Err("当前系统暂不支持启动桌面应用".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODEX_FULL_NAME: &str = "OpenAI.Codex_26.1103.4239.0_x64__2p2nqsd0c76g0";

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
    fn application_target_matches_exact_path() {
        let target = DesktopAppTarget::Application(PathBuf::from(r"C:\Apps\Tool\Tool.exe"));
        assert!(process_matches_target(
            Path::new(r"c:/apps/tool/TOOL.EXE"),
            &target
        ));
        assert!(!process_matches_target(
            Path::new(r"C:\Apps\Tool\other.exe"),
            &target
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_app_target_matches_install_root() {
        let target = DesktopAppTarget::WindowsApp {
            app_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            install_root: PathBuf::from(r"C:\WA\OpenAI.Codex_26.1_x64__2p"),
        };
        assert!(process_matches_target(
            Path::new(r"C:\WA\OpenAI.Codex_26.1_x64__2p\ChatGPT.exe"),
            &target
        ));
        assert!(!process_matches_target(
            Path::new(r"C:\Other\Other.exe"),
            &target
        ));
    }
    #[test]
    fn macos_bundle_names_cover_desktop_apps() {
        assert!(macos_bundle_names("codex").contains(&"ChatGPT.app"));
        assert!(macos_bundle_names("claude-desktop").contains(&"Claude.app"));
        assert!(macos_bundle_names("opencode").contains(&"OpenCode.app"));
        assert!(macos_bundle_names("claude").is_empty());
    }

    #[test]
    fn macos_process_matching_is_scoped_to_the_bundle() {
        let bundle = Path::new("/Applications/Codex.app");
        let output = "10 /Applications/Codex.app/Contents/MacOS/Codex\n11 /Applications/Codex.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper\n12 /Applications/Codex.app.old/Contents/MacOS/Codex\n13 /Applications/ChatGPT.app/Contents/MacOS/ChatGPT\n14 /usr/local/bin/codex\n";
        assert_eq!(macos_process_ids_from_ps(output, bundle), vec![10, 11]);
        assert!(macos_process_ids_from_ps("bad input\n-1 /opt/desktop", bundle).is_empty());
    }
}
