#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendMode {
    Desktop,
    WebUi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeOs {
    Windows,
    Macos,
    Linux,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub client: ClientRuntime,
    pub backend: BackendRuntime,
    pub relation: RuntimeRelation,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRuntime {
    pub shell: ClientShell,
    pub os: RuntimeOs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientShell {
    Desktop,
    Browser,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendRuntime {
    pub os: RuntimeOs,
    pub headless: bool,
    pub remote: bool,
    pub capabilities: BackendCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendCapabilities {
    pub read_config: bool,
    pub write_config: bool,
    pub open_local_folder: bool,
    pub pick_directory: bool,
    pub server_directory_browse: bool,
    pub app_config_dir_override: bool,
    pub save_file_dialog: bool,
    pub open_file_dialog: bool,
    pub launch_interactive_terminal: bool,
    pub launch_background_process: bool,
    pub auto_launch: bool,
    pub tool_version_check: bool,
    pub window_controls: bool,
    pub drag_region: bool,
    pub tray: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRelation {
    pub co_located: bool,
}

pub fn current_runtime_os() -> RuntimeOs {
    if cfg!(target_os = "windows") {
        RuntimeOs::Windows
    } else if cfg!(target_os = "macos") {
        RuntimeOs::Macos
    } else if cfg!(target_os = "linux") {
        RuntimeOs::Linux
    } else {
        RuntimeOs::Unknown
    }
}

pub fn backend_runtime_info(mode: BackendMode) -> RuntimeInfo {
    let os = current_runtime_os();
    let is_desktop = matches!(mode, BackendMode::Desktop);
    let is_webui = matches!(mode, BackendMode::WebUi);

    RuntimeInfo {
        client: ClientRuntime {
            shell: if is_desktop {
                ClientShell::Desktop
            } else {
                ClientShell::Browser
            },
            os: if is_desktop { os } else { RuntimeOs::Unknown },
        },
        backend: BackendRuntime {
            os,
            headless: is_webui,
            remote: is_webui,
            capabilities: BackendCapabilities {
                read_config: true,
                write_config: true,
                open_local_folder: is_desktop,
                pick_directory: is_desktop,
                server_directory_browse: true,
                app_config_dir_override: is_desktop,
                save_file_dialog: is_desktop,
                open_file_dialog: is_desktop,
                launch_interactive_terminal: is_desktop,
                launch_background_process: false,
                auto_launch: is_desktop,
                tool_version_check: is_desktop && !matches!(os, RuntimeOs::Windows),
                window_controls: is_desktop,
                drag_region: is_desktop && matches!(os, RuntimeOs::Macos),
                tray: is_desktop,
            },
        },
        relation: RuntimeRelation {
            co_located: is_desktop,
        },
    }
}
