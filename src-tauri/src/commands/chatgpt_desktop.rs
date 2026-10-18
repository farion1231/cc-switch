//! ChatGPT Desktop（OpenAI 的桌面应用）的安装检测。
//!
//! 它自带一份 Codex CLI，和 Codex CLI 共用 `~/.codex` 配置；随附的那份跟着桌面版更新。
//! 「应用」页把它列在 ChatGPT Desktop 那一行，Codex CLI 那一行只算单独安装的
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
}

#[tauri::command]
pub async fn get_chatgpt_desktop_status() -> Result<ChatGptDesktopStatus, String> {
    tokio::task::spawn_blocking(detect)
        .await
        .map_err(|e| format!("ChatGPT Desktop probe task join error: {e}"))?
}

#[cfg(windows)]
fn detect() -> Result<ChatGptDesktopStatus, String> {
    let mut status = ChatGptDesktopStatus {
        supported: true,
        ..Default::default()
    };
    let names = windows_package::full_names(WINDOWS_PACKAGE_FAMILY)?;
    let Some(full_name) = newest_main_package(&names) else {
        return Ok(status);
    };
    status.installed = true;
    status.app_version = parse_package_full_name(full_name).map(|parsed| parsed.version);

    let mut candidates = Vec::new();
    if let Some(root) = windows_package::path(full_name) {
        candidates.push(root.join("app").join("resources").join("codex.exe"));
    }
    // 包内那份跑不起来（例如组策略禁止直接执行 WindowsApps 里的程序）时，退到桌面版运行时解出的副本。
    candidates.extend(extracted_cli_candidates());
    probe_bundled_cli(&mut status, &candidates);
    Ok(status)
}

#[cfg(target_os = "macos")]
fn detect() -> Result<ChatGptDesktopStatus, String> {
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
        return Ok(status);
    };
    status.installed = true;
    status.app_version = macos_bundle_version(&app);
    probe_bundled_cli(&mut status, &macos_cli_candidates(&app));
    Ok(status)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn detect() -> Result<ChatGptDesktopStatus, String> {
    Ok(ChatGptDesktopStatus::default())
}

/// 找出 /Applications 和 ~/Applications 下已安装的 ChatGPT.app / Codex.app。
#[cfg(target_os = "macos")]
pub(crate) fn macos_installed_apps() -> Vec<PathBuf> {
    let home = crate::config::get_home_dir();
    let mut apps = Vec::new();
    for candidate in macos_app_candidates(&home) {
        let Some(app) = canonical_macos_app_path(&candidate) else {
            continue;
        };
        if is_chatgpt_desktop_bundle(&app) && !apps.contains(&app) {
            apps.push(app);
        }
    }
    apps
}

#[cfg(target_os = "macos")]
pub(crate) fn is_chatgpt_desktop_macos_cli_path(
    path: &std::path::Path,
    verified_apps: &[PathBuf],
) -> bool {
    is_macos_cli_path_with_verified_apps(path, verified_apps)
}

/// `apps` must come from `macos_installed_apps`, which only returns bundles with the expected
/// bundle identifier and a valid OpenAI signature.
#[cfg(any(target_os = "macos", test))]
fn is_macos_cli_path_with_verified_apps(path: &std::path::Path, apps: &[PathBuf]) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("codex") || name.eq_ignore_ascii_case("codex.exe")
        })
        && apps.iter().any(|app| macos_path_is_within_app(path, app))
}

#[cfg(any(target_os = "macos", test))]
fn canonical_macos_app_path(app: &std::path::Path) -> Option<PathBuf> {
    std::fs::canonicalize(app).ok()
}

#[cfg(any(target_os = "macos", test))]
fn macos_path_is_within_app(path: &std::path::Path, app: &std::path::Path) -> bool {
    let app = canonical_macos_app_path(app).unwrap_or_else(|| app.to_path_buf());
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.starts_with(app)
}

#[cfg(any(target_os = "macos", test))]
const MACOS_CHATGPT_DESKTOP_BUNDLE_IDENTIFIER: &str = "com.openai.codex";

#[cfg(any(target_os = "macos", test))]
const MACOS_OPENAI_TEAM_IDENTIFIER: &str = "2DC432GLL2";

#[cfg(any(target_os = "macos", test))]
fn is_chatgpt_desktop_bundle_identifier(bundle_identifier: &str) -> bool {
    bundle_identifier == MACOS_CHATGPT_DESKTOP_BUNDLE_IDENTIFIER
}

#[cfg(target_os = "macos")]
fn is_chatgpt_desktop_bundle(app: &std::path::Path) -> bool {
    if !app.is_dir() || !app.join("Contents").join("Info.plist").is_file() {
        return false;
    }
    let Some(bundle_identifier) = macos_bundle_identifier(app) else {
        return false;
    };
    is_chatgpt_desktop_bundle_identifier(&bundle_identifier) && macos_openai_signature_is_valid(app)
}

#[cfg(target_os = "macos")]
fn macos_bundle_identifier(app: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(app.join("Contents").join("Info.plist"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let bundle_identifier = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!bundle_identifier.is_empty()).then_some(bundle_identifier)
}

#[cfg(any(target_os = "macos", test))]
fn macos_openai_signature_requirement() -> String {
    format!(
        "=identifier \"{MACOS_CHATGPT_DESKTOP_BUNDLE_IDENTIFIER}\" and anchor apple generic and certificate leaf[subject.OU] = \"{MACOS_OPENAI_TEAM_IDENTIFIER}\""
    )
}

#[cfg(target_os = "macos")]
fn macos_openai_signature_is_valid(app: &std::path::Path) -> bool {
    std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "-R"])
        .arg(macos_openai_signature_requirement())
        .arg(app)
        .output()
        .is_ok_and(|output| output.status.success())
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

/// 桌面版运行时把随附 CLI 解到 `%LOCALAPPDATA%\OpenAI\Codex\bin\` 下的哈希或旧版版本目录，新的优先。
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
        if !entry
            .file_name()
            .to_str()
            .is_some_and(is_extracted_cli_cache_dir_name)
        {
            continue;
        }
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
            Ok(metadata) if metadata.is_file() => {
                let real_path = std::fs::canonicalize(&exe).unwrap_or_else(|_| exe.clone());
                if is_extracted_cli_candidate_owned(&exe, &real_path) {
                    paths.push(exe);
                }
            }
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

/// 新版缓存目录是 16 位十六进制 ID；旧版用三段纯数字版本号。其它名字（如 `stable`）
/// 可能是单独安装的 CLI，不能归为桌面版随附程序。
#[cfg(any(windows, test))]
fn is_extracted_cli_cache_dir_name(name: &str) -> bool {
    let is_hash = name.len() == 16 && name.bytes().all(|byte| byte.is_ascii_hexdigit());
    let parts = name.split('.').collect::<Vec<_>>();
    let is_legacy_version = parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    is_hash || is_legacy_version
}

#[cfg(any(windows, test))]
fn is_extracted_cli_candidate_owned(path: &std::path::Path, real_path: &std::path::Path) -> bool {
    let is_cache_directory = path
        .parent()
        .and_then(std::path::Path::file_name)
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(is_extracted_cli_cache_dir_name);
    is_cache_directory && !super::misc::is_codex_standalone_install_at(path, real_path)
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

/// 这处 codex 是不是 ChatGPT Desktop 随附的那份。纯字符串判断、不碰文件系统，
/// 各平台的路径都能在任一平台单测。
///
/// - Windows：包内的 `WindowsApps\OpenAI.Codex_*\…\codex.exe`；
/// - macOS：`ChatGPT.app` / `Codex.app` 包内的 `codex`。
///
/// 官方独立安装器的 `Programs\OpenAI\Codex\bin\codex.exe`（入口直接在 bin 下）、
/// npm 的 `@openai/codex/bin/codex.js` 都不算。
#[cfg_attr(not(windows), allow(dead_code))]
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
#[cfg_attr(not(windows), allow(dead_code))]
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
        if let Some(unc) = path.strip_prefix("//?/unc/") {
            format!("//{unc}")
        } else {
            path.strip_prefix("//?/").unwrap_or(&path).to_string()
        }
    }

    let path = normalize(path);
    let root = normalize(local_data_dir);
    let prefix = format!("{}/openai/codex/bin/", root.trim_end_matches('/'));
    path.strip_prefix(&prefix).is_some_and(|rest| {
        matches!(rest.split('/').collect::<Vec<_>>().as_slice(), [version, exe] if is_extracted_cli_cache_dir_name(version) && *exe == "codex.exe")
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

    /// 当前用户在这个包族下注册的所有包全名；没装时为空，查询失败时返回错误。
    pub(super) fn full_names(family: &str) -> Result<Vec<String>, String> {
        let family = wide(family);
        for _ in 0..3 {
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
            // 没装：返回 ERROR_SUCCESS 且 count 为 0。其它错误不能当成“没装”。
            if rc == ERROR_SUCCESS {
                if count == 0 {
                    return Ok(Vec::new());
                }
                return Err(format!(
                    "GetPackagesByPackageFamily returned {count} packages without an output buffer"
                ));
            }
            if rc != ERROR_INSUFFICIENT_BUFFER {
                return Err(format!("GetPackagesByPackageFamily failed with error {rc}"));
            }
            if count == 0 || buffer_len == 0 {
                return Err("GetPackagesByPackageFamily reported an empty required buffer".into());
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
            if rc == ERROR_INSUFFICIENT_BUFFER {
                // 包注册状态可能在两次查询间变化；重新计算所需缓冲区。
                continue;
            }
            if rc != ERROR_SUCCESS {
                return Err(format!("GetPackagesByPackageFamily failed with error {rc}"));
            }
            if count == 0 {
                return Ok(Vec::new());
            }

            // 指针都指向 `buffer` 内部：换算成下标在 `buffer` 里读，不解引用裸指针。
            let base = buffer.as_ptr() as usize;
            return Ok(names
                .iter()
                .take(count as usize)
                .filter_map(|&ptr| {
                    let start = (ptr as usize).checked_sub(base)? / std::mem::size_of::<u16>();
                    let rest = buffer.get(start..)?;
                    let end = rest.iter().position(|&unit| unit == 0)?;
                    Some(String::from_utf16_lossy(&rest[..end]))
                })
                .collect());
        }
        Err("GetPackagesByPackageFamily kept changing while reading packages".into())
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
    fn macos_cli_filter_requires_a_verified_app_root() {
        let dir = tempfile::tempdir().unwrap();
        let verified_app = dir.path().join("ChatGPT.app");
        let unverified_app = dir.path().join("Other.app");
        std::fs::create_dir_all(&verified_app).unwrap();
        std::fs::create_dir_all(&unverified_app).unwrap();
        let bundled_cli = verified_app.join("Contents/Resources/codex");
        let unrelated_executable = verified_app.join("Contents/Resources/codex-command-runner");
        let unrelated_cli = unverified_app.join("Contents/Resources/codex");
        std::fs::create_dir_all(bundled_cli.parent().unwrap()).unwrap();
        std::fs::write(&bundled_cli, b"placeholder").unwrap();

        assert!(is_macos_cli_path_with_verified_apps(
            &bundled_cli,
            std::slice::from_ref(&verified_app)
        ));
        assert!(!is_macos_cli_path_with_verified_apps(
            &unrelated_cli,
            std::slice::from_ref(&verified_app)
        ));
        assert!(!is_macos_cli_path_with_verified_apps(
            &unrelated_executable,
            std::slice::from_ref(&verified_app)
        ));
        #[cfg(unix)]
        {
            let link = dir.path().join("bin/codex");
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(&bundled_cli, &link).unwrap();
            assert!(is_macos_cli_path_with_verified_apps(
                &link,
                std::slice::from_ref(&verified_app)
            ));
            assert!(is_macos_cli_path_with_verified_apps(
                &std::fs::canonicalize(&link).unwrap(),
                std::slice::from_ref(&verified_app)
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn macos_app_symlinks_are_resolved_before_detection() {
        let dir = tempfile::tempdir().unwrap();
        let real_app = dir.path().join("real/ChatGPT.app");
        let app_alias = dir.path().join("Applications/ChatGPT.app");
        let bundled_cli = real_app.join("Contents/Resources/codex");
        std::fs::create_dir_all(bundled_cli.parent().unwrap()).unwrap();
        std::fs::write(&bundled_cli, b"placeholder").unwrap();
        std::fs::create_dir_all(app_alias.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real_app, &app_alias).unwrap();

        let resolved_app = canonical_macos_app_path(&app_alias).unwrap();
        assert_eq!(resolved_app, std::fs::canonicalize(&real_app).unwrap());
        assert!(macos_path_is_within_app(
            &app_alias.join("Contents/Resources/codex"),
            &resolved_app
        ));
    }

    #[test]
    fn codesign_requirement_uses_inline_expression_syntax() {
        assert!(macos_openai_signature_requirement().starts_with('='));
    }

    #[test]
    fn only_the_unified_openai_codex_bundle_identifier_is_accepted() {
        assert!(is_chatgpt_desktop_bundle_identifier("com.openai.codex"));
        for bundle_identifier in [
            "com.openai.chat",
            "com.example.codex",
            "com.openai.codex.helper",
        ] {
            assert!(
                !is_chatgpt_desktop_bundle_identifier(bundle_identifier),
                "{bundle_identifier}"
            );
        }
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
            r"C:\Users\me\AppData\Local\OpenAI\Codex\bin\0.162.0\codex.exe",
            r"\\?\C:\Users\me\AppData\Local\OpenAI\Codex\bin\e78bad9887fa50cf\codex.exe",
        ] {
            assert!(
                is_extracted_cli_path_under_root(path, local_data_dir),
                "{path}"
            );
        }
        assert!(is_extracted_cli_path_under_root(
            r"\\?\UNC\server\share\Users\me\AppData\Local\OpenAI\Codex\bin\e78bad9887fa50cf\codex.exe",
            r"\\server\share\Users\me\AppData\Local"
        ));
        for path in [
            r"D:\Profiles\me\LocalAppData\OpenAI\Codex\bin\0123456789abcdef\codex.exe",
            r"C:\Users\me\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe",
            r"C:\Users\me\AppData\Local\OpenAI\Codex\bin\stable\codex.exe",
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

    #[test]
    fn extracted_cli_cache_directory_names_are_specific() {
        assert!(is_extracted_cli_cache_dir_name("e78bad9887fa50cf"));
        assert!(is_extracted_cli_cache_dir_name("0.162.0"));
        assert!(!is_extracted_cli_cache_dir_name("26.1013.3656.0"));
        assert!(!is_extracted_cli_cache_dir_name("stable"));
        assert!(!is_extracted_cli_cache_dir_name(
            ".staging-247581e40ee272fb-random"
        ));
        assert!(!is_extracted_cli_cache_dir_name("0.162.0-alpha.1"));
    }

    #[test]
    fn numeric_custom_standalone_install_is_not_owned_by_desktop_cache() {
        let custom_install =
            std::path::Path::new("/Users/me/AppData/Local/OpenAI/Codex/bin/1.2.3/codex.exe");
        let standalone_target = std::path::Path::new(
            "/Users/me/.codex/packages/standalone/releases/1.2.3-x86_64-pc-windows-msvc/bin/codex.exe",
        );
        assert!(!is_extracted_cli_candidate_owned(
            custom_install,
            standalone_target
        ));

        let legacy_desktop_cache =
            std::path::Path::new("/Users/me/AppData/Local/OpenAI/Codex/bin/0.162.0/codex.exe");
        assert!(is_extracted_cli_candidate_owned(
            legacy_desktop_cache,
            legacy_desktop_cache
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

    #[cfg(windows)]
    #[test]
    fn windows_detection_reports_supported_without_panicking() {
        let status = detect().unwrap();
        assert!(status.supported);
        if !status.installed {
            assert_eq!(status.app_version, None);
            assert_eq!(status.bundled_cli_version, None);
        }
    }
}
