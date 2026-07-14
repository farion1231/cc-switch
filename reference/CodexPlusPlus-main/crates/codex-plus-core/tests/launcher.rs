use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use codex_plus_core::app_paths::{
    build_codex_executable, codex_app_version, find_latest_codex_app_dir,
    find_latest_codex_app_dir_from_roots, find_macos_codex_app,
    latest_appx_install_location_from_output, normalize_codex_app_path, packaged_app_user_model_id,
    resolve_codex_app_dir_with_saved, user_data_candidates_from,
};
use codex_plus_core::launcher::{
    CodexLaunch, DefaultLaunchHooks, LaunchHooks, LaunchOptions, MacosCleanupPolicy,
    build_codex_arguments, build_codex_arguments_for_settings,
    build_codex_arguments_with_native_menu_inspector, build_codex_command,
    build_codex_command_with_native_menu_inspector, build_macos_cleanup_command,
    build_macos_open_command, build_macos_open_command_with_native_menu_inspector,
    build_packaged_activation, build_packaged_activation_with_native_menu_inspector,
    launch_and_inject_with_hooks,
};
#[cfg(windows)]
use codex_plus_core::launcher::{WindowsProcessControlStrategy, windows_process_control_strategy};
use codex_plus_core::ports::{
    select_packaged_codex_debug_port_with, select_platform_loopback_port_with,
};
use codex_plus_core::settings::{BackendSettings, RelayProfile, RelayProtocol};
use codex_plus_core::status::StatusStore;

#[test]
fn app_paths_find_latest_windows_package_prefers_highest_version_app_dir() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("OpenAI.Codex_1.2.3.0_x64__abc/app")).unwrap();
    std::fs::create_dir_all(temp.path().join("OpenAI.Codex_26.429.8261.0_x64__abc/app")).unwrap();
    std::fs::create_dir_all(temp.path().join("OpenAI.Codex_not-a-version_x64__abc")).unwrap();

    let latest = find_latest_codex_app_dir(temp.path()).unwrap();

    assert_eq!(
        latest,
        temp.path().join("OpenAI.Codex_26.429.8261.0_x64__abc/app")
    );
}

#[test]
fn app_paths_find_latest_windows_package_ignores_chatgpt_desktop_package() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("OpenAI.Codex_26.707.3748.0_x64__abc/app")).unwrap();
    std::fs::create_dir_all(
        temp.path()
            .join("OpenAI.ChatGPT-Desktop_1.2026.133.0_x64__abc/app"),
    )
    .unwrap();
    std::fs::create_dir_all(
        temp.path()
            .join("OpenAI.ChatGPT-Desktop_2026.514.421.0_neutral_~_abc"),
    )
    .unwrap();

    let latest = find_latest_codex_app_dir(temp.path()).unwrap();

    assert_eq!(
        latest,
        temp.path().join("OpenAI.Codex_26.707.3748.0_x64__abc/app")
    );
    assert_eq!(codex_app_version(&latest).as_deref(), Some("26.707.3748.0"));
    assert_eq!(
        packaged_app_user_model_id(&latest).as_deref(),
        Some("OpenAI.Codex_abc!App")
    );
}

#[test]
fn app_paths_find_latest_windows_package_detects_beta_package() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(
        temp.path()
            .join("OpenAI.CodexBeta_26.527.7698.0_x64__2p2nqsd0c76g0/app"),
    )
    .unwrap();

    let latest = find_latest_codex_app_dir(temp.path()).unwrap();

    assert_eq!(
        latest,
        temp.path()
            .join("OpenAI.CodexBeta_26.527.7698.0_x64__2p2nqsd0c76g0/app")
    );
    assert_eq!(codex_app_version(&latest).as_deref(), Some("26.527.7698.0"));
    assert_eq!(
        packaged_app_user_model_id(&latest).as_deref(),
        Some("OpenAI.CodexBeta_2p2nqsd0c76g0!App")
    );
}

#[test]
fn app_paths_find_latest_windows_package_returns_package_when_app_dir_missing() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("OpenAI.Codex_26.429.8261.0_x64__abc");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("ChatGPT.exe"), "").unwrap();

    assert_eq!(find_latest_codex_app_dir(temp.path()).unwrap(), package);
}

#[test]
fn app_paths_find_latest_windows_package_checks_roots_before_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("WindowsApps");
    std::fs::create_dir_all(root.join("OpenAI.Codex_1.0.0.0_x64__abc/app")).unwrap();
    std::fs::create_dir_all(root.join("OpenAI.Codex_26.513.3673.0_x64__abc/app")).unwrap();

    let latest = find_latest_codex_app_dir_from_roots(&[root]).unwrap();

    assert!(latest.ends_with("OpenAI.Codex_26.513.3673.0_x64__abc/app"));
}

#[test]
fn app_paths_find_latest_windows_package_ignores_chatgpt_across_roots() {
    let temp = tempfile::tempdir().unwrap();
    let root_a = temp.path().join("WindowsAppsA");
    let root_b = temp.path().join("WindowsAppsB");
    std::fs::create_dir_all(root_a.join("OpenAI.Codex_26.999.0.0_x64__abc/app")).unwrap();
    std::fs::create_dir_all(root_b.join("OpenAI.ChatGPT-Desktop_1.2026.133.0_x64__abc/app"))
        .unwrap();

    let latest = find_latest_codex_app_dir_from_roots(&[root_a, root_b]).unwrap();

    assert!(latest.ends_with("OpenAI.Codex_26.999.0.0_x64__abc/app"));
}

#[test]
fn app_paths_extracts_codex_version_from_windows_package_app_dir() {
    let app_dir =
        PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex_26.513.3673.0_x64__abc\app");

    assert_eq!(
        codex_app_version(&app_dir).as_deref(),
        Some("26.513.3673.0")
    );
}

#[test]
fn app_paths_extracts_codex_version_from_macos_bundle_plist() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("OpenAI Codex.app");
    let contents = app.join("Contents");
    std::fs::create_dir_all(&contents).unwrap();
    std::fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleVersion</key>
  <string>26.500.0</string>
  <key>CFBundleShortVersionString</key>
  <string>26.513.3673</string>
</dict>
</plist>
"#,
    )
    .unwrap();

    assert_eq!(codex_app_version(&app).as_deref(), Some("26.513.3673"));
}

#[test]
fn app_paths_user_data_candidates_include_local_and_roaming_variants() {
    let local = PathBuf::from(r"C:\Users\me\AppData\Local");
    let roaming = PathBuf::from(r"C:\Users\me\AppData\Roaming");

    let candidates = user_data_candidates_from(Some(&local), Some(&roaming));

    assert_eq!(
        candidates,
        vec![
            local.join("OpenAI").join("ChatGPT"),
            local.join("OpenAI.ChatGPT-Desktop"),
            local.join("ChatGPT"),
            local.join("OpenAI").join("Codex"),
            local.join("OpenAI.Codex"),
            local.join("Codex"),
            roaming.join("OpenAI").join("ChatGPT"),
            roaming.join("OpenAI.ChatGPT-Desktop"),
            roaming.join("ChatGPT"),
            roaming.join("OpenAI").join("Codex"),
            roaming.join("OpenAI.Codex"),
            roaming.join("Codex"),
        ]
    );
}

#[test]
fn app_paths_find_macos_codex_app_prefers_first_search_root_and_known_names() {
    let temp = tempfile::tempdir().unwrap();
    let system_root = temp.path().join("Applications");
    let user_root = temp.path().join("Users/me/Applications");
    let system_app = system_root.join("OpenAI Codex.app");
    let user_app = user_root.join("Codex.app");
    std::fs::create_dir_all(&system_app).unwrap();
    std::fs::create_dir_all(&user_app).unwrap();

    assert_eq!(
        find_macos_codex_app(&[system_root, user_root]).unwrap(),
        system_app
    );
}

#[test]
fn app_paths_prefers_codex_app_over_chatgpt_app() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Applications");
    let codex = root.join("Codex.app");
    let chatgpt = root.join("ChatGPT.app");
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::create_dir_all(&chatgpt).unwrap();

    assert_eq!(
        find_macos_codex_app(&[root]).as_deref(),
        Some(codex.as_path())
    );
}

#[test]
fn app_paths_preserves_legacy_macos_candidates_before_chatgpt_app() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Applications");
    let legacy = root.join("OpenAI Codex.app");
    let chatgpt = root.join("ChatGPT.app");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::create_dir_all(&chatgpt).unwrap();

    assert_eq!(
        find_macos_codex_app(&[root]).as_deref(),
        Some(legacy.as_path())
    );
}

#[test]
fn app_paths_build_macos_bundle_executable() {
    let app = PathBuf::from("/Applications/OpenAI Codex.app");

    assert_eq!(
        build_codex_executable(&app),
        PathBuf::from("/Applications/OpenAI Codex.app/Contents/MacOS/Codex")
    );
}

#[test]
fn app_paths_finds_chatgpt_bundle_and_uses_its_declared_executable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Applications");
    let app = root.join("ChatGPT.app");
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    std::fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>com.openai.codex</string>
  <key>CFBundleExecutable</key>
  <string>ChatGPT</string>
</dict>
</plist>
"#,
    )
    .unwrap();
    std::fs::write(macos.join("ChatGPT"), "").unwrap();

    assert_eq!(
        find_macos_codex_app(&[root]).as_deref(),
        Some(app.as_path())
    );
    assert_eq!(build_codex_executable(&app), macos.join("ChatGPT"));
}

#[test]
fn app_paths_normalizes_executable_and_package_paths() {
    let temp = tempfile::tempdir().unwrap();
    let portable = temp.path().join("CodexPortable");
    let app = portable.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("Codex.exe"), "").unwrap();

    assert_eq!(
        normalize_codex_app_path(&app.join("Codex.exe")).as_deref(),
        Some(app.as_path())
    );
    assert_eq!(
        normalize_codex_app_path(&portable).as_deref(),
        Some(app.as_path())
    );
}

#[test]
fn app_paths_normalizes_chatgpt_desktop_executable_and_builds_it() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp
        .path()
        .join("OpenAI.Codex_1.2026.133.0_x64__abc")
        .join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("ChatGPT.exe"), "").unwrap();

    assert_eq!(
        normalize_codex_app_path(&app.join("ChatGPT.exe")).as_deref(),
        Some(app.as_path())
    );
    assert_eq!(build_codex_executable(&app), app.join("ChatGPT.exe"));
    assert_eq!(
        packaged_app_user_model_id(&app).as_deref(),
        Some("OpenAI.Codex_abc!App")
    );
}

#[test]
fn app_paths_saved_path_is_used_when_no_explicit_path_is_provided() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app).unwrap();

    assert_eq!(
        resolve_codex_app_dir_with_saved(None, Some(&app.to_string_lossy())).as_deref(),
        Some(app.as_path())
    );
}

#[test]
fn launcher_builds_debug_arguments_and_commands() {
    let app_dir = PathBuf::from(r"C:\Codex\app");

    assert_eq!(
        build_codex_arguments(9229, &[]),
        vec![
            "--remote-debugging-port=9229".to_string(),
            "--remote-allow-origins=http://127.0.0.1:9229".to_string(),
        ]
    );
    let command = build_codex_command(&app_dir, 9229, &[]);
    assert_eq!(command[1], "--remote-debugging-port=9229");
    assert_eq!(command[2], "--remote-allow-origins=http://127.0.0.1:9229");
}

#[test]
fn launcher_does_not_override_codex_app_environment() {
    let source = include_str!("../src/launcher.rs");

    assert!(!source.contains(".envs(codex_process_environment())"));
    assert!(!source.contains("activate_packaged_app_with_environment"));
    assert!(!source.contains("with_temporary_proxy_environment"));
}

#[test]
fn launcher_windows_process_wait_uses_platform_cfg_guards() {
    let source = include_str!("../src/launcher.rs").replace("\r\n", "\n");

    assert!(source.contains(
        "#[cfg(windows)]\nasync fn wait_for_windows_process_id(process_id: u32) -> anyhow::Result<()>"
    ));
    assert!(source.contains(
        "#[cfg(not(windows))]\nasync fn wait_for_windows_process_id(process_id: u32) -> anyhow::Result<()>"
    ));
    assert!(source.contains(
        "#[cfg(windows)]\nfn wait_for_windows_process_id_blocking(process_id: u32) -> anyhow::Result<()>"
    ));
}

#[test]
fn launcher_appends_extra_codex_arguments_after_debug_arguments() {
    let app_dir = PathBuf::from(r"C:\Codex\app");
    let extra_args = vec![
        "--force_high_performance_gpu".to_string(),
        "  ".to_string(),
        "--enable-features=UseOzonePlatform".to_string(),
    ];

    assert_eq!(
        build_codex_arguments(9229, &extra_args),
        vec![
            "--remote-debugging-port=9229".to_string(),
            "--remote-allow-origins=http://127.0.0.1:9229".to_string(),
            "--force_high_performance_gpu".to_string(),
            "--enable-features=UseOzonePlatform".to_string(),
        ]
    );
    let command = build_codex_command(&app_dir, 9229, &extra_args);
    assert_eq!(command[1], "--remote-debugging-port=9229");
    assert_eq!(command[2], "--remote-allow-origins=http://127.0.0.1:9229");
    assert_eq!(command[3], "--force_high_performance_gpu");
    assert_eq!(command[4], "--enable-features=UseOzonePlatform");
}

#[test]
fn launcher_fast_startup_adds_statsig_fast_fail_argument_when_enabled() {
    let settings = BackendSettings {
        codex_app_fast_startup: true,
        ..BackendSettings::default()
    };
    let args = build_codex_arguments_for_settings(9229, &settings);

    assert!(args.iter().any(|arg| {
        arg.starts_with("--host-resolver-rules=")
            && arg.contains("MAP ab.chatgpt.com 127.0.0.1")
            && arg.contains("MAP featureassets.org 127.0.0.1")
            && arg.contains("MAP cloudflare-dns.com 127.0.0.1")
    }));

    let settings = BackendSettings {
        codex_app_fast_startup: true,
        codex_extra_args: vec!["--host-resolver-rules=MAP example.test 127.0.0.1".to_string()],
        ..BackendSettings::default()
    };
    let args = build_codex_arguments_for_settings(9229, &settings);
    assert_eq!(
        args.iter()
            .filter(|arg| arg.starts_with("--host-resolver-rules="))
            .count(),
        1
    );

    let settings = BackendSettings {
        codex_app_fast_startup: false,
        ..BackendSettings::default()
    };
    let args = build_codex_arguments_for_settings(9229, &settings);
    assert!(
        !args
            .iter()
            .any(|arg| arg.starts_with("--host-resolver-rules="))
    );
}

#[test]
fn launcher_native_menu_inspector_arguments_are_added_before_extra_args() {
    let app_dir = PathBuf::from(r"C:\Codex\app");
    let extra_args = vec!["--force_high_performance_gpu".to_string()];

    assert_eq!(
        build_codex_arguments_with_native_menu_inspector(9229, 9329, &extra_args),
        vec![
            "--remote-debugging-port=9229".to_string(),
            "--remote-allow-origins=http://127.0.0.1:9229".to_string(),
            "--inspect=127.0.0.1:9329".to_string(),
            "--force_high_performance_gpu".to_string(),
        ]
    );
    let command = build_codex_command_with_native_menu_inspector(&app_dir, 9229, 9329, &extra_args);
    assert_eq!(command[1], "--remote-debugging-port=9229");
    assert_eq!(command[2], "--remote-allow-origins=http://127.0.0.1:9229");
    assert_eq!(command[3], "--inspect=127.0.0.1:9329");
    assert_eq!(command[4], "--force_high_performance_gpu");
}

#[test]
fn launcher_constructs_windows_packaged_activation_without_real_app() {
    let app_dir = PathBuf::from(
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.506.2212.0_x64__2p2nqsd0c76g0\app",
    );

    assert_eq!(
        packaged_app_user_model_id(&app_dir).unwrap(),
        "OpenAI.Codex_2p2nqsd0c76g0!App"
    );
    assert_eq!(
        build_packaged_activation(&app_dir, 9229, &[]).unwrap(),
        CodexLaunch::PackagedActivation {
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            arguments: "--remote-debugging-port=9229 --remote-allow-origins=http://127.0.0.1:9229"
                .to_string(),
            process_id: None,
        }
    );
}

#[test]
fn launcher_packaged_activation_appends_extra_codex_arguments() {
    let app_dir = PathBuf::from(
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.506.2212.0_x64__2p2nqsd0c76g0\app",
    );
    let extra_args = vec!["--force_high_performance_gpu".to_string()];

    assert_eq!(
        build_packaged_activation(&app_dir, 9229, &extra_args).unwrap(),
        CodexLaunch::PackagedActivation {
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            arguments:
                "--remote-debugging-port=9229 --remote-allow-origins=http://127.0.0.1:9229 --force_high_performance_gpu"
                    .to_string(),
            process_id: None,
        }
    );
}

#[test]
fn launcher_packaged_activation_adds_native_menu_inspector_argument() {
    let app_dir = PathBuf::from(
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.506.2212.0_x64__2p2nqsd0c76g0\app",
    );

    assert_eq!(
        build_packaged_activation_with_native_menu_inspector(&app_dir, 9229, 9329, &[]).unwrap(),
        CodexLaunch::PackagedActivation {
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            arguments:
                "--remote-debugging-port=9229 --remote-allow-origins=http://127.0.0.1:9229 --inspect=127.0.0.1:9329"
                    .to_string(),
            process_id: None,
        }
    );
}

#[test]
fn launcher_packaged_activation_can_preserve_process_id() {
    let launch = CodexLaunch::PackagedActivation {
        app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
        arguments: "--remote-debugging-port=9229".to_string(),
        process_id: Some(4242),
    };

    assert_eq!(launch.process_id(), Some(4242));
}

#[test]
fn launcher_applies_codexplusplus_window_icon_after_packaged_activation() {
    let source = include_str!("../src/launcher.rs");

    assert!(source.contains("apply_codexplusplus_window_icon_after_launch(process_id);"));
    assert!(source.contains("windows_apply_codexplusplus_icon_to_process_window"));
}

#[test]
fn launcher_no_longer_contains_mobile_control_runtime() {
    let launcher_source = include_str!("../src/launcher.rs");
    let settings_source = include_str!("../src/settings.rs");
    let workspace_toml = include_str!("../../../Cargo.toml");

    assert!(!workspace_toml.contains("apps/codex-plus-mobile-relay"));
    assert!(!launcher_source.contains("MobileRelay"));
    assert!(!launcher_source.contains("mobile_relay"));
    assert!(!launcher_source.contains("\"/mobile\""));
    assert!(!launcher_source.contains("CODEX_PLUS_MOBILE"));
    assert!(!settings_source.contains("mobileControl"));
}

#[test]
fn launcher_plugin_marketplace_unlock_repairs_role_specific_plugins() {
    let launcher_source = include_str!("../src/launcher.rs");

    assert!(launcher_source.contains("ensure_openai_curated_marketplace_config(&home)"));
    assert!(launcher_source.contains("ensure_role_specific_plugins_marketplace_config(&home)"));
}

#[test]
fn app_paths_parse_appx_install_location_from_powershell_output() {
    let output =
        "\r\nC:\\Program Files\\WindowsApps\\OpenAI.Codex_26.611.7849.0_x64__2p2nqsd0c76g0\r\n";

    assert_eq!(
        latest_appx_install_location_from_output(output).as_deref(),
        Some(r"C:\Program Files\WindowsApps\OpenAI.Codex_26.611.7849.0_x64__2p2nqsd0c76g0")
    );
}

#[test]
fn launcher_packaged_activation_does_not_directly_fallback_to_windowsapps_exe() {
    let source = include_str!("../src/launcher.rs");

    assert!(!source.contains("launcher.packaged_activation_cdp_unready_direct_fallback"));
    assert!(!source.contains("terminate_windows_process_id(process_id).await"));
}

#[cfg(windows)]
#[test]
fn launcher_windows_packaged_process_management_uses_native_api() {
    assert_eq!(
        windows_process_control_strategy(),
        WindowsProcessControlStrategy::NativeWindowsApi
    );
}

#[test]
fn launcher_macos_open_command_waits_for_app_exit() {
    let command = build_macos_open_command(Path::new("/Applications/Codex.app"), 9229, &[]);

    assert_eq!(command[0], "open");
    assert!(command.contains(&"-W".to_string()));
    assert!(command.contains(&"-a".to_string()));
    assert!(command.contains(&"--args".to_string()));
    assert!(command.contains(&"--remote-debugging-port=9229".to_string()));
}

#[test]
fn launcher_macos_open_command_appends_extra_codex_arguments_after_args() {
    let extra_args = vec!["--force_high_performance_gpu".to_string()];
    let command = build_macos_open_command(Path::new("/Applications/Codex.app"), 9229, &extra_args);
    let args_index = command
        .iter()
        .position(|part| part == "--args")
        .expect("macOS command should contain --args");

    assert_eq!(
        &command[args_index + 1..],
        &[
            "--remote-debugging-port=9229".to_string(),
            "--remote-allow-origins=http://127.0.0.1:9229".to_string(),
            "--force_high_performance_gpu".to_string(),
        ]
    );
}

#[test]
fn launcher_macos_open_command_adds_native_menu_inspector_argument() {
    let command = build_macos_open_command_with_native_menu_inspector(
        Path::new("/Applications/Codex.app"),
        9229,
        9329,
        &[],
    );
    let args_index = command
        .iter()
        .position(|part| part == "--args")
        .expect("macOS command should contain --args");

    assert_eq!(
        &command[args_index + 1..],
        &[
            "--remote-debugging-port=9229".to_string(),
            "--remote-allow-origins=http://127.0.0.1:9229".to_string(),
            "--inspect=127.0.0.1:9329".to_string(),
        ]
    );
}

#[test]
fn ports_windows_falls_back_to_ephemeral_when_requested_is_busy() {
    let selected = select_platform_loopback_port_with(9229, true, |_| false, || 43001);

    assert_eq!(selected, 43001);
}

#[test]
fn ports_windows_packaged_debug_falls_back_to_ephemeral_when_requested_is_busy() {
    let selected = select_packaged_codex_debug_port_with(9229, true, |_| false, || 43001);

    assert_eq!(selected, 43001);
}

#[test]
fn ports_non_windows_keeps_requested_even_when_busy() {
    let selected = select_platform_loopback_port_with(9229, false, |_| false, || 43001);

    assert_eq!(selected, 9229);
}

#[tokio::test]
async fn default_helper_serves_backend_status_over_http() {
    let hooks = DefaultLaunchHooks::default();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    hooks.start_helper(port).await.unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client
        .post(format!("http://127.0.0.1:{port}/backend/status"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let payload: serde_json::Value = response.json().await.unwrap();
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["transport"], "http-helper");

    let repair_response = client
        .post(format!("http://127.0.0.1:{port}/backend/repair"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert!(!repair_response.status().is_success());

    hooks.shutdown_helper(port).await;
}

#[tokio::test]
async fn default_helper_accepts_diagnostic_log_events_over_http() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("codex-plus.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let hooks = DefaultLaunchHooks::default();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    hooks.start_helper(port).await.unwrap();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/diagnostics/log"))
        .json(&serde_json::json!({
            "event": "backend_check_failed",
            "message": "fetch failed",
            "helperBase": format!("http://127.0.0.1:{port}")
        }))
        .send()
        .await
        .unwrap();

    assert!(response.status().is_success());
    let payload: serde_json::Value = response.json().await.unwrap();
    assert_eq!(payload["status"], "ok");
    hooks.shutdown_helper(port).await;

    let contents = std::fs::read_to_string(&log_path).unwrap();
    assert!(contents.contains("renderer.backend_check_failed"));
    assert!(contents.contains("fetch failed"));
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
}

#[tokio::test]
async fn launch_lifecycle_runs_enabled_maintenance_without_applying_relay_profile() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone())
        .with_settings(BackendSettings {
            provider_sync_enabled: true,
            relay_profiles_enabled: true,
            computer_use_guard_enabled: true,
            codex_app_plugin_marketplace_unlock: true,
            ..BackendSettings::default()
        })
        .with_launch_result(CodexLaunch::Process {
            command: vec!["codex".to_string()],
            wait_strategy: codex_plus_core::launcher::ProcessWaitStrategy::TrackedChild,
            macos_cleanup_policy: None,
        });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir.clone()),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:57321",
            "load-settings",
            "provider-sync",
            "computer-use-guard",
            "start-helper:57321",
            "launch:9229",
            "computer-use-guard-watchdog",
            "inject:9229:57321",
            "status:running",
            "wait-codex",
            "shutdown-helper:57321",
        ]
    );
    let events = events.lock().unwrap().clone();
    assert!(!events.contains(&"apply-relay".to_string()));
    assert!(events.contains(&"provider-sync".to_string()));
    assert!(events.contains(&"computer-use-guard".to_string()));
    assert!(events.contains(&"computer-use-guard-watchdog".to_string()));
    assert_eq!(
        handle
            .status_store
            .load_latest()
            .unwrap()
            .unwrap()
            .codex_app
            .as_deref(),
        Some(app_dir.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn launch_lifecycle_passes_configured_extra_args_to_codex_launch() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        codex_extra_args: vec!["--force_high_performance_gpu".to_string()],
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert!(
        events
            .lock()
            .unwrap()
            .contains(&"launch:9229:--force_high_performance_gpu".to_string())
    );
}

#[tokio::test]
async fn launch_lifecycle_passes_native_menu_localization_switch_to_codex_launch() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        codex_app_native_menu_localization: false,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert!(
        events
            .lock()
            .unwrap()
            .contains(&"launch:9229:native-menu-off".to_string())
    );
}

#[tokio::test]
async fn launch_lifecycle_keeps_js_injection_in_relay_mode() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        launch_mode: codex_plus_core::settings::LaunchMode::Relay,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:57321",
            "load-settings",
            "start-helper:57321",
            "launch:9229",
            "inject:9229:57321",
            "status:running",
            "wait-codex",
            "shutdown-helper:57321",
        ]
    );
}

#[tokio::test]
async fn launch_lifecycle_skips_helper_and_injection_when_enhancements_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        enhancements_enabled: false,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:57321",
            "load-settings",
            "launch:9229",
            "status:running",
            "wait-codex",
        ]
    );
}

#[tokio::test]
async fn launch_lifecycle_runs_computer_use_guard_when_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        computer_use_guard_enabled: true,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:57321",
            "load-settings",
            "computer-use-guard",
            "start-helper:57321",
            "launch:9229",
            "computer-use-guard-watchdog",
            "inject:9229:57321",
            "status:running",
            "wait-codex",
            "shutdown-helper:57321",
        ]
    );
}

#[tokio::test]
async fn launch_lifecycle_skips_computer_use_guard_by_default() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone());

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    let events = events.lock().unwrap().clone();
    assert!(!events.contains(&"computer-use-guard".to_string()));
    assert!(!events.contains(&"computer-use-guard-watchdog".to_string()));
    assert!(events.contains(&"launch:9229".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_does_not_apply_relay_profile_before_launching_codex() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        relay_profiles_enabled: true,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    let events = events.lock().unwrap().clone();
    assert!(!events.contains(&"apply-relay".to_string()));
    assert!(events.contains(&"launch:9229".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_skips_active_relay_profile_when_supplier_config_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        relay_profiles_enabled: false,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    let events = events.lock().unwrap().clone();
    assert!(!events.contains(&"apply-relay".to_string()));
    assert!(!events.contains(&"computer-use-guard".to_string()));
    assert!(events.contains(&"launch:9229".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_tolerates_duplicate_context_parent_tables_without_applying_relay() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        relay_common_config_contents: "[mcp_servers]\n".to_string(),
        relay_context_config_contents: "[mcp_servers]\n\n[mcp_servers.ida]\ncommand = \"python\"\n"
            .to_string(),
        relay_profiles: vec![RelayProfile {
            id: "relay-a".to_string(),
            name: "Relay A".to_string(),
            relay_mode: codex_plus_core::settings::RelayMode::PureApi,
            config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-test"
"#
            .to_string(),
            auth_contents: r#"{"OPENAI_API_KEY":"sk-test"}"#.to_string(),
            ..RelayProfile::default()
        }],
        active_relay_id: "relay-a".to_string(),
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    let events = events.lock().unwrap().clone();
    assert!(!events.contains(&"apply-relay".to_string()));
    assert!(!events.contains(&"computer-use-guard".to_string()));
    assert!(events.contains(&"launch:9229".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_enters_degraded_mode_and_retries_when_injection_fails() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_inject_error("inject failed");

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store: status_store.clone(),
        },
        &hooks,
    )
    .await
    .unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:57321",
            "load-settings",
            "start-helper:57321",
            "launch:9229",
            "inject:9229:57321",
            "status:running_degraded",
        ]
    );
    let status = status_store.load_latest().unwrap().unwrap();
    assert_eq!(status.status, "running_degraded");
    assert!(status.message.contains("Codex launched"));

    handle.wait_for_codex_exit().await.unwrap();
    let events = events.lock().unwrap().clone();
    assert!(events.contains(&"wait-codex".to_string()));
    assert!(events.contains(&"shutdown-helper:57321".to_string()));
    assert!(!events.contains(&"terminate-codex".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_cleans_helper_when_launch_fails_after_helper_started() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_launch_error("launch failed");

    let error = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store: status_store.clone(),
        },
        &hooks,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("launch failed"));
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:57321",
            "load-settings",
            "start-helper:57321",
            "launch:9229",
            "shutdown-helper:57321",
            "status:failed",
        ]
    );
}

#[tokio::test]
async fn launch_starts_helper_when_chat_protocol_proxy_is_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let settings = BackendSettings {
        enhancements_enabled: false,
        relay_profiles: vec![RelayProfile {
            id: "relay-chat".to_string(),
            name: "Chat".to_string(),
            model: String::new(),
            base_url: "https://chat-only.example.test/v1".to_string(),
            upstream_base_url: "https://chat-only.example.test/v1".to_string(),
            api_key: "sk-test".to_string(),
            protocol: RelayProtocol::ChatCompletions,
            relay_mode: codex_plus_core::settings::RelayMode::MixedApi,
            official_mix_api_key: false,
            test_model: String::new(),
            config_contents: String::new(),
            auth_contents: String::new(),
            use_common_config: true,
            context_selection: codex_plus_core::settings::RelayContextSelection::default(),
            context_selection_initialized: false,
            context_window: String::new(),
            auto_compact_limit: String::new(),
            model_insert_mode: codex_plus_core::settings::RelayModelInsertMode::default(),
            model_list: String::new(),
            model_windows: String::new(),
            user_agent: String::new(),
        }],
        active_relay_id: "relay-chat".to_string(),
        ..BackendSettings::default()
    };
    let hooks = FakeHooks::new(events.clone()).with_settings(settings);

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 58000,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();

    let before_stop = events.lock().unwrap().clone();
    assert!(before_stop.contains(&"select-helper:58000".to_string()));
    assert!(before_stop.contains(&"start-helper:57321".to_string()));
    assert!(!before_stop.contains(&"inject:9229:57321".to_string()));

    handle.wait_for_codex_exit().await.unwrap();

    let after_stop = events.lock().unwrap().clone();
    assert!(after_stop.contains(&"wait-codex".to_string()));
    assert!(after_stop.contains(&"shutdown-helper:57321".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_cleans_helper_and_codex_when_status_save_fails() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    std::fs::write(temp.path().join("status-parent-file"), "not a directory").unwrap();
    let status_store = StatusStore::new(
        temp.path()
            .join("status-parent-file")
            .join("latest-status.json"),
    );
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks =
        FakeHooks::new(events.clone()).with_launch_result(CodexLaunch::PackagedActivation {
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            arguments: "--remote-debugging-port=9229".to_string(),
            process_id: Some(4242),
        });

    let error = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("failed to create directory"));
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:57321",
            "load-settings",
            "start-helper:57321",
            "launch:9229",
            "inject:9229:57321",
            "shutdown-helper:57321",
            "terminate-packaged:4242",
            "status:failed",
        ]
    );
}

#[tokio::test]
async fn launch_lifecycle_keeps_packaged_process_id_running_and_retries_when_injection_fails() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone())
        .with_launch_result(CodexLaunch::PackagedActivation {
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            arguments: "--remote-debugging-port=9229".to_string(),
            process_id: Some(4242),
        })
        .with_inject_error("inject failed");

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();

    assert!(
        !events
            .lock()
            .unwrap()
            .contains(&"terminate-packaged:4242".to_string())
    );
    handle.wait_for_codex_exit().await.unwrap();
}

#[tokio::test]
async fn default_provider_sync_enabled_fails_instead_of_silently_skipping() {
    let hooks = FakeHooks::new(Arc::new(Mutex::new(Vec::new()))).with_provider_sync_unsupported();

    let error = hooks
        .run_provider_sync()
        .await
        .expect_err("default-style provider sync should be explicit");

    assert!(
        error
            .to_string()
            .contains("provider sync requires launcher hooks")
    );
}

#[tokio::test]
async fn launch_continues_when_plugin_marketplace_config_fails() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let hooks = FakeHooks::new(events.clone())
        .with_plugin_marketplace_error("config.toml TOML parse failed");

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(PathBuf::from("/Applications/Codex.app")),
            debug_port: 9229,
            helper_port: 57321,
            status_store: StatusStore::new(tempfile::tempdir().unwrap().path().join("status.json")),
        },
        &hooks,
    )
    .await
    .unwrap();

    assert_eq!(handle.debug_port, 9229);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "select-debug:9229",
            "select-helper:57321",
            "load-settings",
            "plugin-marketplace",
            "start-helper:57321",
            "launch:9229",
            "inject:9229:57321",
            "status:running"
        ]
    );
}

#[test]
fn launcher_macos_cleanup_command_targets_specific_app_bundle() {
    let command = build_macos_cleanup_command(
        Path::new("/Applications/OpenAI Codex.app"),
        MacosCleanupPolicy::QuitIfNotPreviouslyRunning,
    )
    .expect("cleanup command should be allowed");

    assert_eq!(command[0], "osascript");
    assert!(command.iter().any(|part| part.contains("OpenAI Codex")));
    assert!(!command.iter().any(|part| part == "Codex"));
}

#[test]
fn launcher_macos_cleanup_is_skipped_when_app_was_already_running() {
    let command = build_macos_cleanup_command(
        Path::new("/Applications/OpenAI Codex.app"),
        MacosCleanupPolicy::SkipQuitBecauseAlreadyRunning,
    );

    assert_eq!(command, None);
}

#[tokio::test]
async fn default_launch_hooks_provider_sync_enabled_returns_explicit_error() {
    let error = DefaultLaunchHooks::default()
        .run_provider_sync()
        .await
        .expect_err("default provider sync should not silently skip");

    assert!(
        error
            .to_string()
            .contains("provider sync requires launcher hooks")
    );
}

#[derive(Clone)]
struct FakeHooks {
    events: Arc<Mutex<Vec<String>>>,
    settings: BackendSettings,
    launch_result: CodexLaunch,
    launch_error: Option<String>,
    inject_error: Option<String>,
    provider_sync_unsupported: bool,
    plugin_marketplace_error: Option<String>,
}

impl FakeHooks {
    fn new(events: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            events,
            settings: BackendSettings::default(),
            launch_result: CodexLaunch::Process {
                command: vec!["codex".to_string()],
                wait_strategy: codex_plus_core::launcher::ProcessWaitStrategy::TrackedChild,
                macos_cleanup_policy: None,
            },
            launch_error: None,
            inject_error: None,
            provider_sync_unsupported: false,
            plugin_marketplace_error: None,
        }
    }

    fn with_settings(mut self, settings: BackendSettings) -> Self {
        self.settings = settings;
        self
    }

    fn with_launch_result(mut self, launch_result: CodexLaunch) -> Self {
        self.launch_result = launch_result;
        self
    }

    fn with_inject_error(mut self, message: &str) -> Self {
        self.inject_error = Some(message.to_string());
        self
    }

    fn with_launch_error(mut self, message: &str) -> Self {
        self.launch_error = Some(message.to_string());
        self
    }

    fn with_provider_sync_unsupported(mut self) -> Self {
        self.provider_sync_unsupported = true;
        self
    }

    fn with_plugin_marketplace_error(mut self, message: &str) -> Self {
        self.plugin_marketplace_error = Some(message.to_string());
        self
    }

    fn event(&self, event: impl Into<String>) {
        self.events.lock().unwrap().push(event.into());
    }
}

#[async_trait::async_trait(?Send)]
impl LaunchHooks for FakeHooks {
    fn resolve_app_dir(
        &self,
        app_dir: Option<&Path>,
        _settings: &BackendSettings,
    ) -> anyhow::Result<PathBuf> {
        app_dir
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("missing app dir"))
    }

    fn select_debug_port(&self, requested: u16) -> u16 {
        self.event(format!("select-debug:{requested}"));
        requested
    }

    fn select_helper_port(&self, requested: u16) -> u16 {
        self.event(format!("select-helper:{requested}"));
        requested
    }

    async fn load_settings(&self) -> anyhow::Result<BackendSettings> {
        self.event("load-settings");
        Ok(self.settings.clone())
    }

    async fn run_provider_sync(&self) -> anyhow::Result<()> {
        self.event("provider-sync");
        if self.provider_sync_unsupported {
            anyhow::bail!("provider sync requires launcher hooks");
        }
        Ok(())
    }

    async fn apply_active_relay_profile(&self, settings: &BackendSettings) -> anyhow::Result<()> {
        if !settings.relay_profiles_enabled {
            return Ok(());
        }
        self.event("apply-relay");
        Ok(())
    }

    async fn ensure_computer_use_config(&self, _settings: &BackendSettings) -> anyhow::Result<()> {
        self.event("computer-use-guard");
        Ok(())
    }

    async fn ensure_plugin_marketplace_config(
        &self,
        _settings: &BackendSettings,
    ) -> anyhow::Result<()> {
        if let Some(message) = &self.plugin_marketplace_error {
            self.event("plugin-marketplace");
            anyhow::bail!(message.clone());
        }
        Ok(())
    }

    async fn start_helper(&self, helper_port: u16) -> anyhow::Result<()> {
        self.event(format!("start-helper:{helper_port}"));
        Ok(())
    }

    async fn launch_codex(
        &self,
        app_dir: &Path,
        debug_port: u16,
        settings: &BackendSettings,
        extra_args: &[String],
    ) -> anyhow::Result<CodexLaunch> {
        assert!(app_dir.ends_with("Codex.app"));
        let launch_detail = if extra_args.is_empty() {
            format!("launch:{debug_port}")
        } else {
            format!("launch:{debug_port}:{}", extra_args.join(","))
        };
        if settings.codex_app_native_menu_localization {
            self.event(launch_detail);
        } else {
            self.event(format!("{launch_detail}:native-menu-off"));
        }
        if let Some(message) = &self.launch_error {
            anyhow::bail!(message.clone());
        }
        Ok(self.launch_result.clone())
    }

    async fn inject(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        self.event(format!("inject:{debug_port}:{helper_port}"));
        if let Some(message) = &self.inject_error {
            anyhow::bail!(message.clone());
        }
        Ok(())
    }

    async fn ensure_injection(&self, debug_port: u16, helper_port: u16, _app_dir: &Path) -> bool {
        self.event(format!("inject:{debug_port}:{helper_port}"));
        self.inject_error.is_none()
    }

    async fn start_bridge_watchdog(
        &self,
        _debug_port: u16,
        _helper_port: u16,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_computer_use_guard_watchdog(
        &self,
        _settings: &BackendSettings,
    ) -> anyhow::Result<()> {
        self.event("computer-use-guard-watchdog");
        Ok(())
    }

    async fn write_status(&self, status: &str) {
        self.event(format!("status:{status}"));
    }

    async fn wait_for_codex_exit(&self, _launch: &CodexLaunch) -> anyhow::Result<()> {
        self.event("wait-codex");
        Ok(())
    }

    async fn shutdown_helper(&self, helper_port: u16) {
        self.event(format!("shutdown-helper:{helper_port}"));
    }

    async fn terminate_codex(&self, launch: &CodexLaunch) {
        if let Some(process_id) = launch.process_id() {
            self.event(format!("terminate-packaged:{process_id}"));
        } else {
            self.event("terminate-codex");
        }
    }
}
