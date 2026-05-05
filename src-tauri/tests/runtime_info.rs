use cc_switch_lib::{backend_runtime_info, BackendMode, RuntimeOs};

#[test]
fn webui_runtime_reports_remote_headless_backend_capabilities() {
    let info = backend_runtime_info(BackendMode::WebUi);

    assert!(info.backend.headless);
    assert!(info.backend.remote);
    assert!(!info.relation.co_located);
    assert!(!info.backend.capabilities.open_local_folder);
    assert!(!info.backend.capabilities.pick_directory);
    assert!(info.backend.capabilities.server_directory_browse);
    assert!(!info.backend.capabilities.app_config_dir_override);
    assert!(!info.backend.capabilities.launch_interactive_terminal);
    assert!(!info.backend.capabilities.auto_launch);
    assert!(!info.backend.capabilities.tool_version_check);
}

#[test]
fn desktop_runtime_reports_colocated_backend_capabilities() {
    let info = backend_runtime_info(BackendMode::Desktop);

    assert!(!info.backend.headless);
    assert!(!info.backend.remote);
    assert!(info.relation.co_located);
    assert!(info.backend.capabilities.open_local_folder);
    assert!(info.backend.capabilities.pick_directory);
    assert!(info.backend.capabilities.server_directory_browse);
    assert!(info.backend.capabilities.app_config_dir_override);
    assert!(info.backend.capabilities.launch_interactive_terminal);
    assert!(info.backend.capabilities.auto_launch);
}

#[test]
fn runtime_os_matches_current_target() {
    let info = backend_runtime_info(BackendMode::Desktop);

    #[cfg(target_os = "windows")]
    assert_eq!(info.backend.os, RuntimeOs::Windows);
    #[cfg(target_os = "macos")]
    assert_eq!(info.backend.os, RuntimeOs::Macos);
    #[cfg(target_os = "linux")]
    assert_eq!(info.backend.os, RuntimeOs::Linux);
}
