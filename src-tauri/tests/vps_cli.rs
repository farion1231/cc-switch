//! Exercise the real binary's early dispatch, without a GUI, native credentials or network.
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn invoke(home: &Path, arguments: &[&str], malformed_askpass: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cc-switch"));
    command
        .args(arguments)
        .current_dir(home)
        .env("CC_SWITCH_TEST_HOME", home)
        .env("APPDATA", home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.join("AppData/Local"))
        .env("TEMP", home)
        .env("TMP", home)
        .env_remove("CC_SWITCH_VPS_ASKPASS")
        .env_remove("CC_SWITCH_VPS_ASKPASS_ENDPOINT")
        .env_remove("CC_SWITCH_VPS_ASKPASS_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if malformed_askpass {
        command.env("CC_SWITCH_VPS_ASKPASS", "malformed");
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("VPS CLI must return without entering the desktop event loop");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn native_help_and_invalid_parameters_do_not_initialize_the_desktop() {
    let home = tempfile::tempdir().unwrap();
    let output = invoke(home.path(), &["vps", "--help"], false);
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("--server"));
    assert!(output.stderr.is_empty());
    for args in [
        vec!["vps", "get-password"],
        vec!["vps", "exec", "--password", "must-not-be-echoed"],
    ] {
        let output = invoke(home.path(), &args, false);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let diagnostic: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(diagnostic["code"], "invalidArguments");
        assert!(!String::from_utf8(output.stderr)
            .unwrap()
            .contains("must-not-be-echoed"));
    }
    assert!(!home.path().join(".cc-switch").exists());
    assert!(!home.path().join("AppData").exists());
}

#[test]
fn native_askpass_dispatch_remains_before_the_cli() {
    let home = tempfile::tempdir().unwrap();
    let output = invoke(home.path(), &["vps", "--help"], true);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(home.path().read_dir().unwrap().next().is_none());
}

#[test]
fn native_cli_rejects_unbound_and_unconfirmed_targets_locally() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("VPS data 日本語");
    std::fs::create_dir(&root).unwrap();
    let id = "550e8400-e29b-41d4-a716-446655440000";
    let mut document = serde_json::json!({
        "version": 1,
        "servers": [{"id": id, "name": "Local-only fixture", "host": "127.0.0.1",
            "port": 1, "user": "fixture", "authMethod": "password", "apps": {"claude": false}}]
    });
    for (enabled, expected) in [(false, "hostUnavailable"), (true, "trustRequired")] {
        document["servers"][0]["apps"]["claude"] = enabled.into();
        let bytes = serde_json::to_vec(&document).unwrap();
        std::fs::write(root.join("servers.json"), &bytes).unwrap();
        let output = invoke(
            home.path(),
            &[
                "vps",
                "exec",
                "--root",
                root.to_str().unwrap(),
                "--app",
                "claude",
                "--server",
                id,
                "--",
                "never executed",
            ],
            false,
        );
        assert_eq!(output.status.code(), Some(125));
        assert!(output.stdout.is_empty());
        let diagnostic: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(diagnostic["code"], expected);
        assert_eq!(std::fs::read(root.join("servers.json")).unwrap(), bytes);
        assert_eq!(root.read_dir().unwrap().count(), 1);
    }
    assert!(!home.path().join(".cc-switch").exists());
}
