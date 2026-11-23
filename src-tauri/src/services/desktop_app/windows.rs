use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const APPX_PACKAGES_KEY: &str = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages";
const CODEX_APPX_PREFIXES: &[&str] = &["OpenAI.Codex", "OpenAI.CodexBeta", "OpenAI.ChatGPT"];
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub(super) enum WindowsDesktopAppTarget {
    Application(PathBuf),
    WindowsAppId(String),
}

struct AppxPackage {
    full_name: String,
    family_name: String,
    version: String,
    install_location: PathBuf,
}

struct StopTarget {
    executable: Option<PathBuf>,
    install_root: Option<PathBuf>,
    image_names: &'static [&'static str],
}

pub(super) fn find_target(
    app: &str,
    home: &Path,
) -> Result<Option<WindowsDesktopAppTarget>, String> {
    Ok(match app {
        "codex" => find_codex_appx_target()
            .or_else(|| find_codex_executable(home).map(WindowsDesktopAppTarget::Application)),
        "claude-desktop" => find_claude_executable(home)
            .map(WindowsDesktopAppTarget::Application)
            .or_else(|| {
                find_appx_app_id(&["Claude", "Anthropic.Claude"])
                    .map(WindowsDesktopAppTarget::WindowsAppId)
            }),
        "opencode" => find_opencode_application(home).map(WindowsDesktopAppTarget::Application),
        _ => None,
    })
}

pub(super) fn is_running(target: &WindowsDesktopAppTarget, app: &str) -> bool {
    let Ok(stop_target) = stop_target(target, app) else {
        return false;
    };
    !matching_process_ids(&stop_target).is_empty()
}

pub(super) fn stop(target: &WindowsDesktopAppTarget, app: &str, label: &str) -> Result<(), String> {
    let stop_target = stop_target(target, app)?;
    let mut remaining = matching_process_ids(&stop_target);
    if remaining.is_empty() {
        return Ok(());
    }

    request_windows_close(&remaining);
    let deadline = Instant::now() + GRACEFUL_STOP_TIMEOUT;
    loop {
        remaining = matching_process_ids(&stop_target);
        if remaining.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }
        sleep_until(deadline);
    }

    // Revalidate paths before opening the handle and through the handle itself,
    // so PID reuse cannot target an unrelated process.
    for process_id in remaining {
        terminate_matching_process(process_id, &stop_target);
    }

    let deadline = Instant::now() + GRACEFUL_STOP_TIMEOUT;
    loop {
        remaining = matching_process_ids(&stop_target);
        if remaining.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let process_ids = remaining
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "{label} did not exit completely; remaining process IDs: {process_ids}"
            ));
        }
        sleep_until(deadline);
    }
}

fn sleep_until(deadline: Instant) {
    std::thread::sleep(
        PROCESS_POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
    );
}

pub(super) fn launch(target: &WindowsDesktopAppTarget, label: &str) -> Result<(), String> {
    let mut command = match target {
        WindowsDesktopAppTarget::Application(path) => Command::new(path),
        WindowsDesktopAppTarget::WindowsAppId(app_id) => {
            let mut command = Command::new(explorer_executable());
            command.arg(format!("shell:AppsFolder\\{app_id}"));
            command
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Failed to launch {label}: {error}"))
}

fn find_codex_appx_target() -> Option<WindowsDesktopAppTarget> {
    find_codex_appx_target_with(|prefix| {
        find_appx_app_id(&[prefix]).map(WindowsDesktopAppTarget::WindowsAppId)
    })
}

fn find_codex_appx_target_with<F>(mut find_appx: F) -> Option<WindowsDesktopAppTarget>
where
    F: FnMut(&str) -> Option<WindowsDesktopAppTarget>,
{
    CODEX_APPX_PREFIXES
        .iter()
        .find_map(|prefix| find_appx(prefix))
}

fn find_codex_executable(home: &Path) -> Option<PathBuf> {
    find_codex_executable_in_roots(&local_app_data(home), &program_files_roots())
}

fn find_codex_executable_in_roots(
    local_app_data: &Path,
    program_files_roots: &[PathBuf],
) -> Option<PathBuf> {
    let mut candidates = vec![
        local_app_data.join("Programs/OpenAI/ChatGPT/ChatGPT.exe"),
        local_app_data.join("Programs/ChatGPT/ChatGPT.exe"),
        local_app_data.join("OpenAI/ChatGPT/ChatGPT.exe"),
        local_app_data.join("OpenAI/Codex/Codex.exe"),
        local_app_data.join("Programs/OpenAI/Codex/Codex.exe"),
        local_app_data.join("Programs/Codex/Codex.exe"),
    ];
    for root in program_files_roots {
        candidates.extend([
            root.join("OpenAI/ChatGPT/ChatGPT.exe"),
            root.join("ChatGPT/ChatGPT.exe"),
            root.join("OpenAI/Codex/Codex.exe"),
            root.join("Codex/Codex.exe"),
        ]);
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn find_claude_executable(home: &Path) -> Option<PathBuf> {
    let local = local_app_data(home);
    [
        local.join("Programs/Claude/Claude.exe"),
        local.join("Claude/Claude.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn find_opencode_application(home: &Path) -> Option<PathBuf> {
    find_opencode_application_in_roots(&local_app_data(home), &program_files_roots())
}

fn find_opencode_application_in_roots(
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

fn local_app_data(home: &Path) -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Local"))
}

fn program_files_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn find_appx_packages(prefixes: &[&str]) -> Vec<AppxPackage> {
    let mut packages = list_appx_packages()
        .into_iter()
        .filter(|package| {
            package_matches_prefixes(&package.full_name, &package.family_name, prefixes)
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| compare_dotted_versions(&right.version, &left.version));
    packages
}

fn find_appx_app_id(prefixes: &[&str]) -> Option<String> {
    let package = find_appx_packages(prefixes).into_iter().next()?;
    let application_id =
        read_appx_application_id(&package.install_location).unwrap_or_else(|| "App".to_string());
    Some(format!("{}!{application_id}", package.family_name))
}

fn find_appx_install_root(app_id: &str) -> Option<PathBuf> {
    let family = app_id.split('!').next().unwrap_or(app_id);
    find_appx_packages(&[family])
        .into_iter()
        .find(|package| package.family_name.eq_ignore_ascii_case(family))
        .map(|package| package.install_location)
}

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

fn read_appx_application_id(install_root: &Path) -> Option<String> {
    let content = std::fs::read_to_string(install_root.join("AppxManifest.xml")).ok()?;
    let tag = first_xml_tag(&content, "Application")?;
    let id = xml_attr(tag, "Id")?.trim().to_string();
    (!id.is_empty()).then_some(id)
}

fn family_name_from_full_name(full_name: &str) -> Option<String> {
    let name = full_name.split('_').next()?.trim();
    let publisher_id = full_name.rsplit('_').next()?.trim();
    if name.is_empty() || publisher_id.is_empty() || publisher_id == name {
        return None;
    }
    Some(format!("{name}_{publisher_id}"))
}

fn version_from_package_full_name(full_name: &str) -> Option<String> {
    let version = full_name.split('_').nth(1)?.trim();
    (!version.is_empty()).then(|| version.to_string())
}

fn package_matches_prefixes(full_name: &str, family_name: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| {
        full_name == *prefix
            || full_name.starts_with(&format!("{prefix}_"))
            || family_name == *prefix
            || family_name.starts_with(&format!("{prefix}_"))
    })
}

fn compare_dotted_versions(left: &str, right: &str) -> Ordering {
    let parse = |value: &str| {
        value
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let mut left = parse(left);
    let mut right = parse(right);
    let component_count = left.len().max(right.len());
    left.resize(component_count, 0);
    right.resize(component_count, 0);
    left.cmp(&right)
}

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

fn xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        if !bytes[position].is_ascii_whitespace() {
            position += 1;
            continue;
        }
        while position < bytes.len() && bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        let name_start = position;
        while position < bytes.len()
            && !bytes[position].is_ascii_whitespace()
            && !matches!(bytes[position], b'=' | b'/' | b'>')
        {
            position += 1;
        }
        if !tag[name_start..position].eq_ignore_ascii_case(name) {
            continue;
        }
        while position < bytes.len() && bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        if bytes.get(position) != Some(&b'=') {
            continue;
        }
        position += 1;
        while position < bytes.len() && bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        let quote = *bytes.get(position)?;
        if !matches!(quote, b'\'' | b'"') {
            continue;
        }
        let value_start = position + 1;
        let value_end = tag[value_start..].find(quote as char)? + value_start;
        return Some(&tag[value_start..value_end]);
    }
    None
}

fn stop_target(target: &WindowsDesktopAppTarget, app: &str) -> Result<StopTarget, String> {
    match target {
        WindowsDesktopAppTarget::Application(path) => Ok(StopTarget {
            executable: Some(path.clone()),
            install_root: None,
            image_names: image_names(app),
        }),
        WindowsDesktopAppTarget::WindowsAppId(app_id) => find_appx_install_root(app_id)
            .map(|install_root| StopTarget {
                executable: None,
                install_root: Some(install_root),
                image_names: image_names(app),
            })
            .ok_or_else(|| "Unable to locate the Appx installation directory".to_string()),
    }
}

fn image_names(app: &str) -> &'static [&'static str] {
    match app {
        "codex" => &["ChatGPT.exe", "Codex.exe"],
        "opencode" => &["OpenCode.exe"],
        _ => &[],
    }
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn process_matches_stop_target(process_path: &Path, target: &StopTarget) -> bool {
    if !target.image_names.is_empty() {
        let file_name = process_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !target
            .image_names
            .iter()
            .any(|name| file_name.eq_ignore_ascii_case(name))
        {
            return false;
        }
    }
    let process_key = path_key(process_path);
    if target
        .executable
        .as_deref()
        .is_some_and(|executable| process_key == path_key(executable))
    {
        return true;
    }
    target.install_root.as_deref().is_some_and(|root| {
        let root_key = path_key(root);
        !root_key.is_empty()
            && (process_key == root_key || process_key.starts_with(&format!("{root_key}\\")))
    })
}

fn matching_process_ids(target: &StopTarget) -> Vec<u32> {
    process_snapshot()
        .into_iter()
        .filter(|(_, path)| process_matches_stop_target(path, target))
        .map(|(process_id, _)| process_id)
        .collect()
}

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
    unsafe { CloseHandle(snapshot) };
    processes
}

fn process_executable_path(process_id: u32) -> Option<PathBuf> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }
    let path = process_executable_path_from_handle(handle);
    unsafe { CloseHandle(handle) };
    path
}

fn process_executable_path_from_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;

    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    let success =
        unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    if success == 0 {
        return None;
    }
    buffer.truncate(length as usize);
    Some(PathBuf::from(OsString::from_wide(&buffer)))
}

fn request_windows_close(process_ids: &[u32]) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };

    let process_ids = process_ids.iter().copied().collect::<HashSet<_>>();
    unsafe extern "system" fn collect_window(window: HWND, parameter: LPARAM) -> i32 {
        let (process_ids, windows) =
            unsafe { &mut *(parameter as *mut (&HashSet<u32>, Vec<HWND>)) };
        let mut process_id = 0;
        unsafe { GetWindowThreadProcessId(window, &mut process_id) };
        if process_ids.contains(&process_id) {
            windows.push(window);
        }
        1
    }
    let mut context = (process_ids, Vec::new());
    unsafe {
        EnumWindows(Some(collect_window), &mut context as *mut _ as LPARAM);
    }
    for window in context.1 {
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
    }
}

fn terminate_matching_process(process_id: u32, target: &StopTarget) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    if !process_executable_path(process_id)
        .is_some_and(|path| process_matches_stop_target(&path, target))
    {
        return;
    }
    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            process_id,
        )
    };
    if handle.is_null() {
        return;
    }
    let is_match = process_executable_path_from_handle(handle)
        .is_some_and(|path| process_matches_stop_target(&path, target));
    if is_match {
        unsafe { TerminateProcess(handle, 1) };
    }
    unsafe { CloseHandle(handle) };
}

fn explorer_executable() -> PathBuf {
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

fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODEX_FULL_NAME: &str = "OpenAI.Codex_26.1103.4239.0_x64__2p2nqsd0c76g0";

    #[test]
    fn parses_package_identity() {
        assert_eq!(
            family_name_from_full_name(CODEX_FULL_NAME).as_deref(),
            Some("OpenAI.Codex_2p2nqsd0c76g0")
        );
        assert_eq!(
            version_from_package_full_name(CODEX_FULL_NAME).as_deref(),
            Some("26.1103.4239.0")
        );
        assert_eq!(family_name_from_full_name("NoUnderscores"), None);
    }

    #[test]
    fn compares_versions_with_zero_padding_on_both_sides() {
        assert_eq!(
            compare_dotted_versions("1.10.0", "1.9.9"),
            Ordering::Greater
        );
        assert_eq!(compare_dotted_versions("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_dotted_versions("1.0", "1.0.1"), Ordering::Less);
        assert_eq!(compare_dotted_versions("1.0.1", "1.0"), Ordering::Greater);
        assert_eq!(compare_dotted_versions("0.9", "1.0"), Ordering::Less);
    }

    #[test]
    fn codex_appx_candidates_follow_strict_priority() {
        let mut visited = Vec::new();
        let target = find_codex_appx_target_with(|prefix| {
            visited.push(prefix.to_string());
            (prefix == "OpenAI.CodexBeta")
                .then(|| WindowsDesktopAppTarget::WindowsAppId("beta!App".to_string()))
        });
        assert_eq!(visited, ["OpenAI.Codex", "OpenAI.CodexBeta"]);
        assert!(
            matches!(target, Some(WindowsDesktopAppTarget::WindowsAppId(id)) if id == "beta!App")
        );

        let mut visited = Vec::new();
        assert!(find_codex_appx_target_with(|prefix| {
            visited.push(prefix.to_string());
            None
        })
        .is_none());
        assert_eq!(visited, CODEX_APPX_PREFIXES);
    }

    #[test]
    fn codex_traditional_candidates_exclude_execution_aliases() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("AppData/Local");
        let alias = local.join("Microsoft/WindowsApps/ChatGPT.exe");
        let application = local.join("Programs/ChatGPT/ChatGPT.exe");
        std::fs::create_dir_all(alias.parent().unwrap()).unwrap();
        std::fs::create_dir_all(application.parent().unwrap()).unwrap();
        std::fs::write(alias, []).unwrap();
        std::fs::write(&application, []).unwrap();
        assert_eq!(
            find_codex_executable_in_roots(&local, &[]),
            Some(application)
        );
    }

    #[test]
    fn matches_package_prefixes_without_codex_beta_collision() {
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
    }

    #[test]
    fn xml_attributes_require_a_complete_name() {
        let tag = r#"Application OtherId="wrong" Id = "OpenAI.Codex" Description='test'"#;
        assert_eq!(xml_attr(tag, "Id"), Some("OpenAI.Codex"));
        assert_eq!(xml_attr(r#"Application OtherId="wrong""#, "Id"), None);
        assert_eq!(xml_attr(r#"Application Identifier="wrong""#, "Id"), None);
    }

    #[test]
    fn process_matching_is_scoped_to_the_detected_installation() {
        let executable_target = StopTarget {
            executable: Some(PathBuf::from(r"C:\Apps\Tool\Tool.exe")),
            install_root: None,
            image_names: &[],
        };
        assert!(process_matches_stop_target(
            Path::new(r"c:/apps/tool/TOOL.EXE"),
            &executable_target
        ));
        assert!(!process_matches_stop_target(
            Path::new(r"C:\Apps\Tool.old\Tool.exe"),
            &executable_target
        ));

        let appx_target = StopTarget {
            executable: None,
            install_root: Some(PathBuf::from(r"C:\WA\OpenAI.Codex_26.1_x64__2p")),
            image_names: &["ChatGPT.exe", "Codex.exe"],
        };
        assert!(process_matches_stop_target(
            Path::new(r"C:\WA\OpenAI.Codex_26.1_x64__2p\ChatGPT.exe"),
            &appx_target
        ));
        assert!(!process_matches_stop_target(
            Path::new(r"C:\WA\OpenAI.Codex_26.1_x64__2p\helper.exe"),
            &appx_target
        ));
        assert!(!process_matches_stop_target(
            Path::new(r"C:\Other\ChatGPT.exe"),
            &appx_target
        ));
    }

    #[test]
    fn opencode_is_found_in_the_electron_installer_directory() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("AppData/Local");
        let application = local.join("Programs/@opencode-aidesktop/OpenCode.exe");
        std::fs::create_dir_all(application.parent().unwrap()).unwrap();
        std::fs::write(&application, []).unwrap();
        assert_eq!(
            find_opencode_application_in_roots(&local, &[]),
            Some(application)
        );
    }
}
