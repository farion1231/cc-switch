use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod macos;
pub mod windows;

pub const SILENT_NAME: &str = "Codex++";
pub const MANAGER_NAME: &str = "Codex++ 管理工具";
pub const SILENT_BINARY: &str = "codex-plus-plus";
pub const MANAGER_BINARY: &str = "codex-plus-plus-manager";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstallOptions {
    #[serde(default)]
    pub install_root: Option<PathBuf>,
    #[serde(default)]
    pub launcher_path: Option<PathBuf>,
    #[serde(default)]
    pub manager_path: Option<PathBuf>,
    #[serde(default)]
    pub remove_owned_data: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShortcutState {
    pub installed: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryPointState {
    pub silent_shortcut: ShortcutState,
    pub management_shortcut: ShortcutState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallActionResult {
    pub status: String,
    pub message: String,
    pub silent_shortcut: ShortcutState,
    pub management_shortcut: ShortcutState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacosAppBundle {
    pub app_path: PathBuf,
    pub info_plist: String,
    pub launch_script: String,
    pub binary_source: Option<PathBuf>,
    pub binary_target_name: Option<String>,
}

impl ShortcutState {
    pub fn missing(path: Option<PathBuf>) -> Self {
        Self {
            installed: false,
            path: path.map(|path| path.to_string_lossy().to_string()),
        }
    }

    pub fn from_candidates(candidates: Vec<PathBuf>) -> Self {
        if let Some(path) = candidates.iter().find(|path| path.exists()) {
            return Self {
                installed: true,
                path: Some(path.to_string_lossy().to_string()),
            };
        }
        Self::missing(candidates.into_iter().next())
    }
}

pub fn shortcut_names() -> (&'static str, &'static str) {
    ("Codex++.lnk", "Codex++ 管理工具.lnk")
}

pub fn app_bundle_names() -> (&'static str, &'static str) {
    ("Codex++.app", "Codex++ 管理工具.app")
}

pub fn inspect_entrypoints() -> EntryPointState {
    let root = default_install_root();
    EntryPointState {
        silent_shortcut: ShortcutState::from_candidates(entrypoint_candidates(&root, false)),
        management_shortcut: ShortcutState::from_candidates(entrypoint_candidates(&root, true)),
    }
}

pub fn install_entrypoints(options: &InstallOptions) -> InstallActionResult {
    let result = platform_install(options);
    action_result(result, "入口已安装。")
}

pub fn uninstall_entrypoints(options: &InstallOptions) -> InstallActionResult {
    let result = platform_uninstall(options);
    if result.is_ok() && options.remove_owned_data {
        let _ = remove_owned_data();
    }
    action_result(result, "入口已卸载。")
}

pub fn repair_entrypoints(options: &InstallOptions) -> InstallActionResult {
    let result = platform_install(options);
    action_result(result, "入口已修复。")
}

pub fn build_windows_entrypoint_plan(options: &InstallOptions) -> windows::WindowsEntrypointPlan {
    windows::build_windows_entrypoint_plan(options)
}

pub fn build_macos_app_bundle(options: &InstallOptions, manager: bool) -> MacosAppBundle {
    macos::build_app_bundle(options, manager)
}

pub fn remove_owned_data() -> std::io::Result<()> {
    let dir = crate::paths::default_app_state_dir();
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

pub fn default_install_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        return crate::windows_integration::desktop_dir().or_else(|| {
            directories::UserDirs::new().and_then(|dirs| dirs.desktop_dir().map(PathBuf::from))
        });
    }

    #[cfg(target_os = "macos")]
    {
        let sys_apps = PathBuf::from("/Applications");
        if sys_apps.join(format!("{SILENT_NAME}.app")).exists()
            || sys_apps.join(format!("{MANAGER_NAME}.app")).exists()
        {
            return Some(sys_apps);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = macos_applications_dir_from_exe(&exe) {
                if is_macos_applications_dir(&dir) {
                    return Some(dir);
                }
            }
        }
        return Some(sys_apps);
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        directories::UserDirs::new().and_then(|dirs| dirs.desktop_dir().map(PathBuf::from))
    }
}

pub fn default_install_root_strategy() -> &'static str {
    if cfg!(windows) {
        "windows-known-folder"
    } else if cfg!(target_os = "macos") {
        "macos-applications"
    } else {
        "user-dirs-desktop"
    }
}

fn platform_install(options: &InstallOptions) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        windows::install_shortcuts(options)
    }

    #[cfg(target_os = "macos")]
    {
        macos::install_app_bundles(options)
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = options;
        anyhow::bail!("当前平台暂不支持安装 Codex++ 入口")
    }
}

fn platform_uninstall(options: &InstallOptions) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        windows::uninstall_shortcuts(options)
    }

    #[cfg(target_os = "macos")]
    {
        macos::uninstall_app_bundles(options)
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = options;
        anyhow::bail!("当前平台暂不支持卸载 Codex++ 入口")
    }
}

fn action_result(result: anyhow::Result<()>, success_message: &str) -> InstallActionResult {
    let state = inspect_entrypoints();
    match result {
        Ok(()) => InstallActionResult {
            status: "ok".to_string(),
            message: success_message.to_string(),
            silent_shortcut: state.silent_shortcut,
            management_shortcut: state.management_shortcut,
        },
        Err(error) => InstallActionResult {
            status: "failed".to_string(),
            message: error.to_string(),
            silent_shortcut: state.silent_shortcut,
            management_shortcut: state.management_shortcut,
        },
    }
}

fn entrypoint_candidates(root: &Option<PathBuf>, manager: bool) -> Vec<PathBuf> {
    let Some(root) = root else {
        return Vec::new();
    };
    let name = if manager { MANAGER_NAME } else { SILENT_NAME };
    if cfg!(windows) {
        vec![root.join(format!("{name}.lnk"))]
    } else if cfg!(target_os = "macos") {
        vec![root.join(format!("{name}.app"))]
    } else {
        vec![root.join(format!("{name}.desktop"))]
    }
}

pub fn option_or_current_exe(value: &Option<PathBuf>, binary: &str) -> PathBuf {
    if let Some(value) = value {
        return value.clone();
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    companion_binary_path_from_exe(&exe, binary)
}

pub fn companion_binary_path(binary: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    companion_binary_path_from_exe(&exe, binary)
}

pub fn companion_binary_path_from_exe(exe: &Path, binary: &str) -> PathBuf {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    if let Some(bundle_binary) = macos_companion_binary_from_exe(exe, binary) {
        return bundle_binary;
    }
    let same_bundle = dir.join(binary);
    if same_bundle.exists() {
        return same_bundle;
    }
    dir.join(format!("{binary}{suffix}"))
}

fn macos_companion_binary_from_exe(exe: &Path, binary: &str) -> Option<PathBuf> {
    let (applications_dir, app_name) = macos_applications_dir_and_app_name_from_exe(exe)?;
    if binary == SILENT_BINARY {
        if app_name == format!("{SILENT_NAME}.app") {
            return Some(macos_preferred_bundle_binary(
                exe,
                SILENT_BINARY,
                "CodexPlusPlus",
            ));
        }
        let macos = applications_dir
            .join(format!("{SILENT_NAME}.app"))
            .join("Contents")
            .join("MacOS");
        return Some(
            macos
                .join(SILENT_BINARY)
                .exists()
                .then(|| macos.join(SILENT_BINARY))
                .unwrap_or_else(|| macos.join("CodexPlusPlus")),
        );
    }
    if binary == MANAGER_BINARY {
        if app_name == format!("{MANAGER_NAME}.app") {
            return Some(macos_preferred_bundle_binary(
                exe,
                MANAGER_BINARY,
                "CodexPlusPlusManager",
            ));
        }
        let macos = applications_dir
            .join(format!("{MANAGER_NAME}.app"))
            .join("Contents")
            .join("MacOS");
        return Some(
            macos
                .join(MANAGER_BINARY)
                .exists()
                .then(|| macos.join(MANAGER_BINARY))
                .unwrap_or_else(|| macos.join("CodexPlusPlusManager")),
        );
    }
    None
}

fn macos_preferred_bundle_binary(
    exe: &Path,
    sidecar_name: &str,
    bundle_executable_name: &str,
) -> PathBuf {
    let macos = exe.parent().unwrap_or_else(|| Path::new("."));
    let sidecar = macos.join(sidecar_name);
    if sidecar.exists() {
        return sidecar;
    }
    let bundle_executable = macos.join(bundle_executable_name);
    if bundle_executable.exists() {
        return bundle_executable;
    }
    exe.to_path_buf()
}

#[cfg(target_os = "macos")]
fn macos_applications_dir_from_exe(exe: &Path) -> Option<PathBuf> {
    macos_applications_dir_and_app_name_from_exe(exe).map(|(dir, _)| dir)
}

fn macos_applications_dir_and_app_name_from_exe(exe: &Path) -> Option<(PathBuf, String)> {
    let mut path = exe;
    while let Some(parent) = path.parent() {
        if path.extension().and_then(|extension| extension.to_str()) == Some("app") {
            let app_name = path.file_name()?.to_string_lossy().to_string();
            return Some((parent.to_path_buf(), app_name));
        }
        path = parent;
    }
    None
}

#[cfg(target_os = "macos")]
fn is_macos_applications_dir(path: &Path) -> bool {
    if path == Path::new("/Applications") {
        return true;
    }
    directories::BaseDirs::new()
        .map(|dirs| path == dirs.home_dir().join("Applications"))
        .unwrap_or(false)
}

pub(crate) fn install_root_or_default(options: &InstallOptions) -> PathBuf {
    options
        .install_root
        .clone()
        .or_else(default_install_root)
        .unwrap_or_else(|| PathBuf::from("."))
}
