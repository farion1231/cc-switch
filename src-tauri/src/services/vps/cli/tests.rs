use super::*;

fn arguments(root: &std::path::Path) -> Vec<OsString> {
    vec![
        "exec".into(),
        "--root".into(),
        root.into(),
        "--app".into(),
        "claude".into(),
        "--server".into(),
        "550e8400-e29b-41d4-a716-446655440000".into(),
        "--".into(),
        "printf '%s\\n' 'hello world'; exit 17".into(),
    ]
}

#[test]
fn appimage_entry_uses_the_persistent_image_instead_of_its_mount() {
    let home = tempfile::tempdir().unwrap();
    let mounted = home.path().join(".mount_CC-Switch/usr/bin/cc-switch");
    let image = home.path().join("Applications/CC Switch 日本語.AppImage");
    assert_eq!(execution_program(mounted.clone(), None).unwrap(), mounted);
    assert_eq!(
        execution_program(mounted.clone(), Some(image.clone().into_os_string())).unwrap(),
        image
    );
    assert!(execution_program(mounted.clone(), Some("relative.AppImage".into())).is_err());
    assert!(execution_program(
        mounted,
        Some(home.path().join("bad\nentry").into_os_string())
    )
    .is_err());
}

#[test]
fn parses_one_remote_command_without_splitting_or_local_shell_interpolation() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("VPS data 日本語");
    let args = arguments(&root);
    let request = parse(args.clone()).unwrap().unwrap();
    assert_eq!(request.root, root);
    assert_eq!(request.app, AppType::Claude);
    assert_eq!(request.command, args.last().unwrap().to_str().unwrap());
    assert_eq!(request.timeout, Duration::from_secs(300));
    let mut args = args;
    args.splice(1..1, ["--timeout".into(), "42".into()]);
    assert_eq!(
        parse(args).unwrap().unwrap().timeout,
        Duration::from_secs(42)
    );
}

#[test]
fn rejects_missing_duplicate_unknown_options_and_unsafe_target_overrides() {
    let home = tempfile::tempdir().unwrap();
    let valid = arguments(home.path());
    for (index, value) in [
        (2, "relative"),
        (4, "claude-desktop"),
        (4, "unknown"),
        (6, "../host"),
        (8, ""),
        (8, "bad\0command"),
    ] {
        let mut args = valid.clone();
        args[index] = value.into();
        assert_eq!(
            parse(args).unwrap_err(),
            CliError::InvalidArguments,
            "{index}: {value}"
        );
    }
    for prefix in [
        vec!["--root", "other"],
        vec!["--app", "codex"],
        vec!["--server", "other"],
        vec!["--timeout", "0"],
        vec!["--timeout", "86401"],
        vec!["--timeout", "-1"],
        vec!["--timeout", "+1"],
        vec!["--timeout", "1.5"],
        vec!["-o", "ProxyCommand=bad"],
        vec!["-F", "another-config"],
        vec!["--password", "never-accepted"],
    ] {
        let mut args = valid.clone();
        args.splice(1..1, prefix.iter().map(OsString::from));
        assert_eq!(
            parse(args).unwrap_err(),
            CliError::InvalidArguments,
            "{prefix:?}"
        );
    }
    for index in [1, 3, 5, 7] {
        let mut args = valid.clone();
        args.drain(index..=index + 1);
        assert!(parse(args).is_err());
    }
    let mut extra = valid.clone();
    extra.push("second remote argument".into());
    assert!(parse(extra).is_err());
    let mut parent = valid;
    parent[2] = home.path().join("child/../vps").into_os_string();
    assert!(parse(parent).is_err());
}

#[test]
fn supports_exact_help_forms_and_existing_skill_clients_only() {
    assert!(parse(vec!["--help".into()]).unwrap().is_none());
    assert!(parse(vec!["exec".into(), "--help".into()])
        .unwrap()
        .is_none());
    assert!(parse(vec![]).is_err());
    assert!(parse(vec!["get-password".into()]).is_err());
    let home = tempfile::tempdir().unwrap();
    for app in AppType::all() {
        let mut args = arguments(home.path());
        args[4] = app.as_str().into();
        assert_eq!(parse(args).is_ok(), !SkillApps::only(&app).is_empty());
    }
}

#[test]
fn cli_messages_use_all_four_locales_and_stable_error_codes() {
    for locale in ["en-US", "zh-CN", "zh-TW", "ja-JP"] {
        for error in [
            CliError::InvalidArguments,
            CliError::DataInvalid,
            CliError::HostUnavailable,
            CliError::TrustRequired,
            CliError::TrustInvalid,
            CliError::TargetChanged,
            CliError::PasswordRequired,
            CliError::CredentialsUnavailable,
            CliError::PrepareFailed,
            CliError::SshNotFound,
            CliError::ExecutionFailed,
            CliError::Timeout,
            CliError::Cancelled,
        ] {
            assert!(!message(error.key(), locale).trim().is_empty());
        }
        assert!(message("usage", locale).contains("--server"));
    }
    assert_eq!(
        message("cancelled", "zh-Hant"),
        message("cancelled", "zh-TW")
    );
    assert_ne!(message("cancelled", "zh-CN"), message("cancelled", "en-US"));
    assert_eq!(CliError::InvalidArguments.exit_code(), 2);
    assert_eq!(CliError::Timeout.exit_code(), 124);
    assert_eq!(CliError::SshNotFound.exit_code(), 127);
    assert_eq!(CliError::Cancelled.exit_code(), 130);
}
