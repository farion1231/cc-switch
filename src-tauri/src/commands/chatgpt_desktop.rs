//! ChatGPT Desktop（OpenAI 的桌面应用）的安装检测和卸载。
//!
//! 它自带一份 Codex CLI，和 Codex CLI 共用 `~/.codex` 配置；随附的那份跟着桌面版更新、
//! 随桌面版卸载。「应用」页把它列在 ChatGPT Desktop 那一行，Codex CLI 那一行只算单独安装的
//! （见 `is_chatgpt_desktop_codex_path`）。

use std::path::PathBuf;

/// Windows 上 ChatGPT Desktop 的 MSIX 包族名（包名 `OpenAI.Codex`，清单显示名「ChatGPT」）。
#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_PACKAGE_FAMILY: &str = "OpenAI.Codex_2p2nqsd0c76g0";

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptDesktopStatus {
    /// 当前系统有没有 ChatGPT Desktop（Linux 没有）。
    pub supported: bool,
    pub installed: bool,
    /// 桌面应用的版本：Windows 取 MSIX 包版本，macOS 取 `CFBundleShortVersionString`。
    pub app_version: Option<String>,
    /// 随附 Codex CLI 的版本（`codex --version`），只展示，从不和 npm 的最新版比较。
    pub bundled_cli_version: Option<String>,
    /// 读出版本的那份随附 CLI 的路径。
    pub bundled_cli_path: Option<String>,
    /// 卸载时执行的命令（多个主包时每行一条），给确认框展示；没装时为空。
    pub uninstall_command: Option<String>,
}

#[tauri::command]
pub async fn get_chatgpt_desktop_status() -> Result<ChatGptDesktopStatus, String> {
    tokio::task::spawn_blocking(detect)
        .await
        .map_err(|e| format!("ChatGPT Desktop probe task join error: {e}"))
}

/// 卸载 ChatGPT Desktop，它随附的 Codex CLI 一起没了。`~/.codex` 里的配置和会话不动，
/// 单独安装的 Codex CLI 也不动。
#[tauri::command]
pub async fn uninstall_chatgpt_desktop() -> Result<(), String> {
    tokio::task::spawn_blocking(uninstall)
        .await
        .map_err(|e| format!("ChatGPT Desktop uninstall task join error: {e}"))?
}

/// 卸载要等系统删完整个应用，给足时间；卡住就到点结束、如实报错。
#[cfg(any(windows, target_os = "macos"))]
const UNINSTALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

#[cfg(windows)]
fn detect() -> ChatGptDesktopStatus {
    let mut status = ChatGptDesktopStatus {
        supported: true,
        ..Default::default()
    };
    let names = windows_package::full_names(WINDOWS_PACKAGE_FAMILY);
    let Some(full_name) = newest_main_package(&names) else {
        return status;
    };
    status.installed = true;
    status.app_version = parse_package_full_name(full_name).map(|parsed| parsed.version);
    status.uninstall_command =
        remove_main_package_commands(&names).map(|commands| commands.join("\n"));

    let mut candidates = Vec::new();
    if let Some(root) = windows_package::path(full_name) {
        candidates.push(root.join("app").join("resources").join("codex.exe"));
    }
    // 包内那份跑不起来（例如组策略禁止直接执行 WindowsApps 里的程序）时，退到桌面版运行时解出的副本。
    candidates.extend(extracted_cli_candidates());
    probe_bundled_cli(&mut status, &candidates);
    status
}

#[cfg(target_os = "macos")]
fn detect() -> ChatGptDesktopStatus {
    let mut status = ChatGptDesktopStatus {
        supported: true,
        ..Default::default()
    };
    let apps = macos_installed_apps();
    let Some(app) = apps
        .iter()
        .find(|app| macos_cli_candidates(app).iter().any(|cli| cli.is_file()))
        .or_else(|| apps.first())
        .cloned()
    else {
        return status;
    };
    status.installed = true;
    status.app_version = macos_bundle_version(&app);
    status.uninstall_command = finder_trash_commands(&apps).map(|commands| commands.join("\n"));
    probe_bundled_cli(&mut status, &macos_cli_candidates(&app));
    status
}

#[cfg(not(any(windows, target_os = "macos")))]
fn detect() -> ChatGptDesktopStatus {
    ChatGptDesktopStatus::default()
}

#[cfg(windows)]
fn uninstall() -> Result<(), String> {
    // 执行时重新查：确认框开着的时候桌面版可能自己更新过，包全名会变
    let names = windows_package::full_names(WINDOWS_PACKAGE_FAMILY);
    let commands = remove_main_package_commands(&names)
        .ok_or_else(|| "ChatGPT Desktop is not installed".to_string())?;
    for command in commands {
        super::misc::run_hidden_command(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("$ErrorActionPreference = 'Stop'; {command}"),
            ],
            UNINSTALL_TIMEOUT,
        )?;
    }
    if newest_main_package(&windows_package::full_names(WINDOWS_PACKAGE_FAMILY)).is_some() {
        return Err("ChatGPT Desktop is still installed after Remove-AppxPackage".to_string());
    }
    if let Some(bin) = extracted_cli_bin() {
        remove_extracted_cli_copies_under(&bin)?;
    }
    if newest_main_package(&windows_package::full_names(WINDOWS_PACKAGE_FAMILY)).is_some() {
        return Err("ChatGPT Desktop is still installed after Remove-AppxPackage".to_string());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall() -> Result<(), String> {
    let apps = macos_installed_apps();
    if apps.is_empty() {
        return Err("ChatGPT Desktop is not installed".to_string());
    }
    let actions: Vec<(PathBuf, String)> = apps
        .iter()
        .map(|app| {
            finder_trash_script(app)
                .map(|script| (app.clone(), script))
                .ok_or_else(|| format!("Unexpected ChatGPT Desktop path: {}", app.display()))
        })
        .collect::<Result<_, _>>()?;

    for (app, script) in actions {
        if let Err(error) = super::misc::run_hidden_command(
            "/usr/bin/osascript",
            &["-e", &script],
            UNINSTALL_TIMEOUT,
        ) {
            log::warn!(
                "Could not move ChatGPT Desktop at {} to the Trash: {error}",
                app.display()
            );
        }
    }

    let mut remaining = macos_installed_apps();
    for app in &apps {
        if app.exists() && !remaining.contains(app) {
            remaining.push(app.clone());
        }
    }
    if remaining.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "ChatGPT Desktop uninstall was incomplete; still installed: {}",
            remaining
                .iter()
                .map(|app| app.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn uninstall() -> Result<(), String> {
    Err("ChatGPT Desktop is not available on this system".to_string())
}

/// 找出 /Applications 和 ~/Applications 下已安装的 ChatGPT.app / Codex.app。
#[cfg(target_os = "macos")]
fn macos_installed_apps() -> Vec<PathBuf> {
    let home = crate::config::get_home_dir();
    macos_app_candidates(&home)
        .into_iter()
        .filter(|app| app.join("Contents").join("Info.plist").is_file())
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn macos_app_candidates(home: &std::path::Path) -> [PathBuf; 4] {
    [
        PathBuf::from("/Applications/ChatGPT.app"),
        home.join("Applications").join("ChatGPT.app"),
        PathBuf::from("/Applications/Codex.app"),
        home.join("Applications").join("Codex.app"),
    ]
}

/// 卸载这个包的 PowerShell 命令。包全名只含字母、数字和 `._-`，放进单引号里不会被
/// PowerShell 解释；名字对不上就不给命令。
#[cfg_attr(not(windows), allow(dead_code))]
fn remove_package_command(full_name: &str) -> Option<String> {
    let plain = full_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    (plain && parse_package_full_name(full_name).is_some())
        .then(|| format!("Remove-AppxPackage -Package '{full_name}'"))
}

/// 让访达把应用移到废纸篓的 AppleScript，和手动拖进废纸篓一样可以放回。
/// 路径里有引号或反斜杠就不拼，免得改了脚本的意思。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn finder_trash_script(app: &std::path::Path) -> Option<String> {
    let path = app.to_str()?;
    if path.contains(['"', '\\']) || path.chars().any(char::is_control) || !path.ends_with(".app") {
        return None;
    }
    Some(format!(
        "tell application \"Finder\" to delete (POSIX file \"{path}\")"
    ))
}

/// 按顺序试候选，第一份读出版本的就是随附 CLI。
#[cfg(any(windows, target_os = "macos"))]
fn probe_bundled_cli(status: &mut ChatGptDesktopStatus, candidates: &[PathBuf]) {
    for candidate in candidates.iter().filter(|candidate| candidate.is_file()) {
        match super::misc::probe_executable_version(candidate) {
            Ok(version) => {
                status.bundled_cli_version = Some(version);
                status.bundled_cli_path = Some(candidate.display().to_string());
                return;
            }
            Err(error) => log::warn!(
                "ChatGPT Desktop bundled Codex CLI at {} did not report a version: {error}",
                candidate.display()
            ),
        }
    }
}

/// 桌面版运行时把随附 CLI 解到 `%LOCALAPPDATA%\OpenAI\Codex\bin\<hash>\`，新的优先。
#[cfg(windows)]
fn extracted_cli_candidates() -> Vec<PathBuf> {
    let Some(bin) = extracted_cli_bin() else {
        return Vec::new();
    };
    let Ok(paths) = extracted_cli_paths_under(&bin) else {
        return Vec::new();
    };
    newest_first(
        paths
            .into_iter()
            .filter_map(|exe| {
                let modified = exe.metadata().ok()?.modified().ok()?;
                Some((exe, modified))
            })
            .collect(),
    )
}

#[cfg(windows)]
fn extracted_cli_bin() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("OpenAI").join("Codex").join("bin"))
}

#[cfg(any(windows, test))]
fn extracted_cli_paths_under(bin: &std::path::Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(bin) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "Could not inspect ChatGPT Desktop CLI cache {}: {error}",
                bin.display()
            ));
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Could not inspect ChatGPT Desktop CLI cache {}: {error}",
                bin.display()
            )
        })?;
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "Could not inspect ChatGPT Desktop CLI cache {}: {error}",
                entry.path().display()
            )
        })?;
        if !file_type.is_dir() && !file_type.is_symlink() {
            continue;
        }
        let exe = entry.path().join("codex.exe");
        match std::fs::metadata(&exe) {
            Ok(metadata) if metadata.is_file() => paths.push(exe),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Could not inspect ChatGPT Desktop CLI cache {}: {error}",
                    exe.display()
                ));
            }
        }
    }
    Ok(paths)
}

#[cfg(any(windows, test))]
fn remove_extracted_cli_copies_under(bin: &std::path::Path) -> Result<(), String> {
    let paths = extracted_cli_paths_under(bin)?;
    let mut failures = Vec::new();
    for exe in paths {
        let Some(version_dir) = exe.parent() else {
            failures.push(format!("Unexpected cache path: {}", exe.display()));
            continue;
        };
        let parent_is_directory = std::fs::symlink_metadata(version_dir).is_ok_and(|metadata| {
            metadata.file_type().is_dir() && !metadata.file_type().is_symlink()
        });
        if !parent_is_directory {
            failures.push(format!(
                "Refusing to remove a cache CLI outside a regular version directory: {}",
                exe.display()
            ));
            continue;
        }
        if let Err(error) = std::fs::remove_file(&exe) {
            if error.kind() != std::io::ErrorKind::NotFound {
                failures.push(format!("Could not remove {}: {error}", exe.display()));
            }
        }
    }

    let remaining = extracted_cli_paths_under(bin)?;
    if !failures.is_empty() || !remaining.is_empty() {
        let mut details = failures;
        if !remaining.is_empty() {
            details.push(format!(
                "CLI copies still remain: {}",
                remaining
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        return Err(format!(
            "ChatGPT Desktop CLI cache cleanup was incomplete: {}",
            details.join("; ")
        ));
    }
    Ok(())
}

/// 解出的副本可能有好几份（每个版本一个 hash 目录，旧的不一定清掉），按修改时间从新到旧。
#[cfg_attr(not(windows), allow(dead_code))]
fn newest_first(mut found: Vec<(PathBuf, std::time::SystemTime)>) -> Vec<PathBuf> {
    found.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    found.into_iter().map(|(path, _)| path).collect()
}

#[cfg(target_os = "macos")]
fn macos_cli_candidates(app: &std::path::Path) -> [PathBuf; 2] {
    let resources = app.join("Contents").join("Resources");
    [
        resources
            .join("codex-cli")
            .join("CodexCLI.app")
            .join("Contents")
            .join("MacOS")
            .join("codex"),
        resources.join("codex"),
    ]
}

/// `defaults read` 的域写 plist 路径但不带扩展名；二进制、XML 的 plist 都能读。
#[cfg(target_os = "macos")]
fn macos_bundle_version(app: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("/usr/bin/defaults")
        .arg("read")
        .arg(app.join("Contents").join("Info"))
        .arg("CFBundleShortVersionString")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!version.is_empty()).then_some(version)
}

/// MSIX 包全名 `Name_Version_Architecture_ResourceId_PublisherId` 里用得上的部分。
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageFullName {
    version: String,
    resource_id: String,
}

/// 解析包全名（包名本身不含 `_`）。不是五段、版本不是四段数字时返回 None。
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_package_full_name(full_name: &str) -> Option<PackageFullName> {
    let parts: Vec<&str> = full_name.split('_').collect();
    let [name, version, architecture, resource_id, publisher_id] = parts.as_slice() else {
        return None;
    };
    if name.is_empty() || architecture.is_empty() || publisher_id.is_empty() {
        return None;
    }
    package_version_key(version)?;
    Some(PackageFullName {
        version: (*version).to_string(),
        resource_id: (*resource_id).to_string(),
    })
}

/// 四段版本号 `major.minor.build.revision`，按数值比较。
#[cfg_attr(not(windows), allow(dead_code))]
fn package_version_key(version: &str) -> Option<[u32; 4]> {
    let mut key = [0u32; 4];
    let mut parts = version.split('.');
    for slot in &mut key {
        *slot = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(key)
}

/// 包族里的主包（跳过资源包）和各自的版本号。同一包族可能同时注册了几个包（升级过渡期、资源包）。
#[cfg_attr(not(windows), allow(dead_code))]
fn main_packages(full_names: &[String]) -> Vec<(&String, [u32; 4])> {
    full_names
        .iter()
        .filter_map(|name| {
            let parsed = parse_package_full_name(name)?;
            if !parsed.resource_id.is_empty() {
                return None;
            }
            Some((name, package_version_key(&parsed.version)?))
        })
        .collect()
}

/// 主包里版本最高的那个。
#[cfg_attr(not(windows), allow(dead_code))]
fn newest_main_package(full_names: &[String]) -> Option<&String> {
    main_packages(full_names)
        .into_iter()
        .max_by_key(|(_, key)| *key)
        .map(|(name, _)| name)
}

/// 每个已注册的主包都要卸载；返回的命令列表也会原样展示在确认框中。
#[cfg_attr(not(windows), allow(dead_code))]
fn remove_main_package_commands(full_names: &[String]) -> Option<Vec<String>> {
    let commands: Option<Vec<String>> = main_packages(full_names)
        .into_iter()
        .map(|(full_name, _)| remove_package_command(full_name))
        .collect();
    commands.filter(|commands| !commands.is_empty())
}

/// 给确认框展示可直接复制执行的 POSIX shell 参数。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn finder_trash_command(app: &std::path::Path) -> Option<String> {
    let script = finder_trash_script(app)?;
    Some(format!(
        "/usr/bin/osascript -e {}",
        shell_single_quote(&script)
    ))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn finder_trash_commands(apps: &[PathBuf]) -> Option<Vec<String>> {
    apps.iter().map(|app| finder_trash_command(app)).collect()
}

/// 这处 codex 是不是 ChatGPT Desktop 随附的那份。纯字符串判断、不碰文件系统，
/// 各平台的路径都能在任一平台单测。
///
/// - Windows：包内的 `WindowsApps\OpenAI.Codex_*\…\codex.exe`；
/// - macOS：`ChatGPT.app` / `Codex.app` 包内的 `codex`。
///
/// 官方独立安装器的 `Programs\OpenAI\Codex\bin\codex.exe`（入口直接在 bin 下）、
/// npm 的 `@openai/codex/bin/codex.js` 都不算。
pub(crate) fn is_chatgpt_desktop_codex_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file_name = normalized.rsplit('/').next().unwrap_or_default();
    if file_name != "codex" && file_name != "codex.exe" {
        return false;
    }
    if normalized.contains("/windowsapps/openai.codex_")
        || normalized.contains("/chatgpt.app/contents/")
        || normalized.contains("/codex.app/contents/")
    {
        return true;
    }
    false
}

/// 只在 Windows 当前用户的 LocalAppData 下识别桌面版解出的缓存 CLI。
pub(crate) fn is_chatgpt_desktop_extracted_cli_path(path: &str) -> bool {
    #[cfg(windows)]
    {
        dirs::data_local_dir().is_some_and(|local_data_dir| {
            is_extracted_cli_path_under_root(path, &local_data_dir.to_string_lossy())
        })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

#[cfg(any(windows, test))]
fn is_extracted_cli_path_under_root(path: &str, local_data_dir: &str) -> bool {
    fn normalize(path: &str) -> String {
        let path = path.replace('\\', "/").to_ascii_lowercase();
        path.strip_prefix("//?/").unwrap_or(&path).to_string()
    }

    let path = normalize(path);
    let root = normalize(local_data_dir);
    let prefix = format!("{}/openai/codex/bin/", root.trim_end_matches('/'));
    path.strip_prefix(&prefix).is_some_and(|rest| {
        matches!(rest.split('/').collect::<Vec<_>>().as_slice(), [version, exe] if !version.is_empty() && *exe == "codex.exe")
    })
}

#[cfg(windows)]
mod windows_package {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows_sys::Win32::Storage::Packaging::Appx::{
        GetPackagePathByFullName, GetPackagesByPackageFamily,
    };

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// 当前用户在这个包族下注册的所有包全名；没装时为空。
    pub(super) fn full_names(family: &str) -> Vec<String> {
        let family = wide(family);
        let mut count = 0u32;
        let mut buffer_len = 0u32;
        // SAFETY: `family` is a NUL-terminated UTF-16 buffer alive for the call; `count` and
        // `buffer_len` point to locals. Null output buffers only query the required sizes.
        let rc = unsafe {
            GetPackagesByPackageFamily(
                family.as_ptr(),
                &mut count,
                std::ptr::null_mut(),
                &mut buffer_len,
                std::ptr::null_mut(),
            )
        };
        // 没装：返回 ERROR_SUCCESS 且 count 为 0。
        if rc == ERROR_SUCCESS || count == 0 {
            return Vec::new();
        }
        if rc != ERROR_INSUFFICIENT_BUFFER {
            log::warn!("GetPackagesByPackageFamily failed with error {rc}");
            return Vec::new();
        }
        let mut names = vec![std::ptr::null_mut::<u16>(); count as usize];
        let mut buffer = vec![0u16; buffer_len as usize];
        // SAFETY: `names` holds `count` pointer slots and `buffer` holds `buffer_len` UTF-16
        // units, the sizes returned by the previous call. The API writes the names into
        // `buffer` and points each slot into it.
        let rc = unsafe {
            GetPackagesByPackageFamily(
                family.as_ptr(),
                &mut count,
                names.as_mut_ptr(),
                &mut buffer_len,
                buffer.as_mut_ptr(),
            )
        };
        if rc != ERROR_SUCCESS {
            log::warn!("GetPackagesByPackageFamily failed with error {rc}");
            return Vec::new();
        }
        // 指针都指向 `buffer` 内部：换算成下标在 `buffer` 里读，不解引用裸指针。
        let base = buffer.as_ptr() as usize;
        names
            .iter()
            .take(count as usize)
            .filter_map(|&ptr| {
                let start = (ptr as usize).checked_sub(base)? / std::mem::size_of::<u16>();
                let rest = buffer.get(start..)?;
                let end = rest.iter().position(|&unit| unit == 0)?;
                Some(String::from_utf16_lossy(&rest[..end]))
            })
            .collect()
    }

    /// 包的安装目录（`C:\Program Files\WindowsApps\<全名>`）。
    pub(super) fn path(full_name: &str) -> Option<PathBuf> {
        let name = wide(full_name);
        let mut len = 0u32;
        // SAFETY: `name` is a NUL-terminated UTF-16 buffer; a null output buffer only queries
        // the required length.
        let rc = unsafe { GetPackagePathByFullName(name.as_ptr(), &mut len, std::ptr::null_mut()) };
        if rc != ERROR_INSUFFICIENT_BUFFER || len == 0 {
            return None;
        }
        let mut buffer = vec![0u16; len as usize];
        // SAFETY: `buffer` holds exactly `len` UTF-16 units, the length returned above.
        let rc = unsafe { GetPackagePathByFullName(name.as_ptr(), &mut len, buffer.as_mut_ptr()) };
        if rc != ERROR_SUCCESS {
            return None;
        }
        let end = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        Some(PathBuf::from(OsString::from_wide(&buffer[..end])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    #[test]
    fn macos_app_candidates_cover_system_and_per_user_bundles() {
        let home = Path::new("/Users/me");
        assert_eq!(
            macos_app_candidates(home),
            [
                PathBuf::from("/Applications/ChatGPT.app"),
                PathBuf::from("/Users/me/Applications/ChatGPT.app"),
                PathBuf::from("/Applications/Codex.app"),
                PathBuf::from("/Users/me/Applications/Codex.app"),
            ]
        );
    }

    #[test]
    fn parses_main_package_full_names_of_any_architecture() {
        for (full_name, version) in [
            (
                "OpenAI.Codex_26.1013.3656.0_x64__2p2nqsd0c76g0",
                "26.1013.3656.0",
            ),
            (
                "OpenAI.Codex_26.1013.3656.0_arm64__2p2nqsd0c76g0",
                "26.1013.3656.0",
            ),
        ] {
            assert_eq!(
                parse_package_full_name(full_name),
                Some(PackageFullName {
                    version: version.to_string(),
                    resource_id: String::new(),
                }),
                "{full_name}"
            );
        }
        assert_eq!(
            parse_package_full_name(
                "OpenAI.Codex_26.1013.3656.0_neutral_split.scale-100_2p2nqsd0c76g0"
            )
            .map(|parsed| parsed.resource_id),
            Some("split.scale-100".to_string())
        );
    }

    #[test]
    fn rejects_malformed_package_full_names() {
        for full_name in [
            "",
            "OpenAI.Codex",
            "OpenAI.Codex_26.1013.3656_x64__2p2nqsd0c76g0",
            "OpenAI.Codex_26.1013.3656.0.1_x64__2p2nqsd0c76g0",
            "OpenAI.Codex_v26.1013.3656.0_x64__2p2nqsd0c76g0",
            "OpenAI.Codex_26.1013.3656.0_x64___2p2nqsd0c76g0",
            "OpenAI.Codex_26.1013.3656.0__2p2nqsd0c76g0",
        ] {
            assert_eq!(parse_package_full_name(full_name), None, "{full_name:?}");
        }
    }

    #[test]
    fn newest_main_package_skips_resource_packages_and_compares_numerically() {
        let names = vec![
            "OpenAI.Codex_26.917.9999.0_x64__2p2nqsd0c76g0".to_string(),
            "OpenAI.Codex_26.1013.3656.0_x64__2p2nqsd0c76g0".to_string(),
            "OpenAI.Codex_27.0.0.0_neutral_split.scale-100_2p2nqsd0c76g0".to_string(),
            "garbage".to_string(),
        ];
        assert_eq!(
            newest_main_package(&names).map(String::as_str),
            Some("OpenAI.Codex_26.1013.3656.0_x64__2p2nqsd0c76g0")
        );
        assert_eq!(newest_main_package(&[]), None);
    }

    #[test]
    fn extracted_copies_are_tried_newest_first() {
        let now = SystemTime::now();
        let old = PathBuf::from("bin/old/codex.exe");
        let new = PathBuf::from("bin/new/codex.exe");
        assert_eq!(
            newest_first(vec![
                (old.clone(), now - Duration::from_secs(3600)),
                (new.clone(), now),
            ]),
            vec![new, old]
        );
    }

    #[test]
    fn extracted_cli_cache_paths_are_anchored_to_localappdata() {
        let local_data_dir = r"C:\Users\me\AppData\Local";
        for path in [
            r"C:\Users\me\AppData\Local\OpenAI\Codex\bin\e78bad9887fa50cf\codex.exe",
            r"\\?\C:\Users\me\AppData\Local\OpenAI\Codex\bin\e78bad9887fa50cf\codex.exe",
        ] {
            assert!(
                is_extracted_cli_path_under_root(path, local_data_dir),
                "{path}"
            );
        }
        for path in [
            r"D:\Profiles\me\LocalAppData\OpenAI\Codex\bin\0123456789abcdef\codex.exe",
            r"C:\Users\me\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe",
            r"C:\Users\me\AppData\Local\OpenAI\Codex\bin\stable\current\codex.exe",
            "/opt/OpenAI/Codex/bin/stable/codex",
        ] {
            assert!(
                !is_extracted_cli_path_under_root(path, local_data_dir),
                "{path}"
            );
            assert!(!is_chatgpt_desktop_codex_path(path), "{path}");
        }
        assert!(!is_chatgpt_desktop_extracted_cli_path(
            "/opt/OpenAI/Codex/bin/stable/codex"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn extracted_cli_cache_path_uses_the_current_localappdata_root() {
        let local_data_dir = dirs::data_local_dir().unwrap();
        let path = local_data_dir
            .join("OpenAI")
            .join("Codex")
            .join("bin")
            .join("e78bad9887fa50cf")
            .join("codex.exe");
        assert!(is_chatgpt_desktop_extracted_cli_path(
            &path.to_string_lossy()
        ));
    }

    #[test]
    fn extracted_cli_cache_cleanup_removes_only_cli_executables() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("OpenAI/Codex/bin");
        let first = bin.join("first-hash");
        let second = bin.join("second-hash");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("codex.exe"), b"bundled cli").unwrap();
        std::fs::write(first.join("codex-command-runner.exe"), b"helper").unwrap();
        std::fs::write(second.join("codex.exe"), b"old bundled cli").unwrap();
        std::fs::write(bin.join("codex.exe"), b"not a versioned cache").unwrap();

        remove_extracted_cli_copies_under(&bin).unwrap();

        assert!(!first.join("codex.exe").exists());
        assert!(first.join("codex-command-runner.exe").is_file());
        assert!(!second.join("codex.exe").exists());
        assert!(bin.join("codex.exe").is_file());
    }

    #[test]
    fn recognizes_codex_bundled_with_chatgpt_desktop() {
        for path in [
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.1013.3656.0_x64__2p2nqsd0c76g0\app\resources\codex.exe",
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
            "/Users/me/Applications/ChatGPT.app/Contents/Resources/codex",
            "/Applications/Codex.app/Contents/Resources/codex",
        ] {
            assert!(is_chatgpt_desktop_codex_path(path), "{path}");
        }
    }

    #[test]
    fn leaves_standalone_npm_and_homebrew_codex_alone() {
        for path in [
            // 官方独立安装器：入口直接在 bin 下，真身在 CODEX_HOME 的发布包里。
            r"C:\Users\me\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe",
            r"C:\Users\me\.codex\packages\standalone\releases\0.150.0-x86_64-pc-windows-msvc\codex.exe",
            // npm 全局包：`@openai/codex/bin/` 里含 `openai/codex/bin/`，但文件名不是 codex。
            r"C:\Users\me\AppData\Roaming\npm\node_modules\@openai\codex\bin\codex.js",
            r"C:\Users\me\AppData\Roaming\npm\codex.cmd",
            "/Users/me/.nvm/versions/node/v22.0.0/lib/node_modules/@openai/codex/bin/codex.js",
            "/opt/homebrew/Caskroom/codex/0.146.0/bin/codex",
            "/usr/local/bin/codex",
            "/opt/OpenAI/Codex/bin/stable/codex",
            // 桌面版目录里的其它程序不是 codex 本体。
            r"C:\Users\me\AppData\Local\OpenAI\Codex\bin\e78bad9887fa50cf\codex-command-runner.exe",
            // 插件自带的 app-server 副本不在 ChatGPT.app 里。
            "/Users/me/.codex/plugins/.plugin-appserver/codex-cli/CodexCLI.app/Contents/MacOS/codex",
        ] {
            assert!(!is_chatgpt_desktop_codex_path(path), "{path}");
        }
    }

    #[test]
    fn uninstall_command_quotes_only_well_formed_package_names() {
        assert_eq!(
            remove_package_command("OpenAI.Codex_26.1013.4894.0_x64__2p2nqsd0c76g0").as_deref(),
            Some("Remove-AppxPackage -Package 'OpenAI.Codex_26.1013.4894.0_x64__2p2nqsd0c76g0'")
        );
        for full_name in [
            "OpenAI.Codex_26.1013.4894.0_x64__2p2nqsd0c76g0'; Remove-Item ~",
            "OpenAI.Codex_26.1013.4894.0_x64__2p2nqsd0c76g0 ",
            "OpenAI.Codex_26.1013.4894.0_x64__2p2nqsd0c76g0$x",
            "garbage",
        ] {
            assert_eq!(remove_package_command(full_name), None, "{full_name:?}");
        }
    }

    #[test]
    fn finder_moves_only_plain_app_bundles_to_the_trash() {
        assert_eq!(
            finder_trash_script(Path::new("/Applications/ChatGPT.app")).as_deref(),
            Some(r#"tell application "Finder" to delete (POSIX file "/Applications/ChatGPT.app")"#)
        );
        for path in [
            r#"/Applications/Chat"GPT.app"#,
            r"/Applications/Chat\GPT.app",
            "/Applications/ChatGPT",
        ] {
            assert_eq!(finder_trash_script(Path::new(path)), None, "{path}");
        }
        assert_eq!(
            finder_trash_command(Path::new("/Users/O'Brien/Applications/ChatGPT.app")).as_deref(),
            Some(
                r#"/usr/bin/osascript -e 'tell application "Finder" to delete (POSIX file "/Users/O'\''Brien/Applications/ChatGPT.app")'"#
            )
        );
    }

    #[test]
    fn finder_trash_commands_list_every_installed_bundle() {
        let apps = [
            PathBuf::from("/Applications/ChatGPT.app"),
            PathBuf::from("/Users/me/Applications/Codex.app"),
        ];
        assert_eq!(
            finder_trash_commands(&apps),
            Some(vec![
                r#"/usr/bin/osascript -e 'tell application "Finder" to delete (POSIX file "/Applications/ChatGPT.app")'"#.to_string(),
                r#"/usr/bin/osascript -e 'tell application "Finder" to delete (POSIX file "/Users/me/Applications/Codex.app")'"#.to_string(),
            ])
        );
        assert_eq!(
            finder_trash_commands(&[
                PathBuf::from("/Applications/ChatGPT.app"),
                PathBuf::from("/Applications/Unsafe\nPath.app"),
            ]),
            None
        );
    }

    #[test]
    fn uninstall_removes_every_main_package_and_no_resource_package() {
        let names = vec![
            "OpenAI.Codex_26.1013.3656.0_x64__2p2nqsd0c76g0".to_string(),
            "OpenAI.Codex_26.1013.4894.0_x64__2p2nqsd0c76g0".to_string(),
            "OpenAI.Codex_26.1013.4894.0_neutral_split.scale-100_2p2nqsd0c76g0".to_string(),
        ];
        let mains: Vec<&str> = main_packages(&names)
            .into_iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            mains,
            [
                "OpenAI.Codex_26.1013.3656.0_x64__2p2nqsd0c76g0",
                "OpenAI.Codex_26.1013.4894.0_x64__2p2nqsd0c76g0",
            ]
        );
    }

    #[test]
    fn uninstall_confirmation_lists_every_main_package_and_skips_resource_packages() {
        let names = vec![
            "OpenAI.Codex_26.1013.3656.0_x64__2p2nqsd0c76g0".to_string(),
            "OpenAI.Codex_26.1013.4894.0_x64__2p2nqsd0c76g0".to_string(),
            "OpenAI.Codex_26.1013.4894.0_neutral_split.scale-100_2p2nqsd0c76g0".to_string(),
        ];
        assert_eq!(
            remove_main_package_commands(&names).unwrap(),
            [
                "Remove-AppxPackage -Package 'OpenAI.Codex_26.1013.3656.0_x64__2p2nqsd0c76g0'",
                "Remove-AppxPackage -Package 'OpenAI.Codex_26.1013.4894.0_x64__2p2nqsd0c76g0'",
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_detection_reports_supported_without_panicking() {
        let status = detect();
        assert!(status.supported);
        if status.installed {
            assert!(status
                .uninstall_command
                .is_some_and(|command| command.starts_with("Remove-AppxPackage -Package '")));
        } else {
            assert_eq!(status.app_version, None);
            assert_eq!(status.bundled_cli_version, None);
            assert_eq!(status.uninstall_command, None);
        }
    }
}
