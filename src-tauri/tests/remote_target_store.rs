use cc_switch_lib::remote::models::RemoteTargetConfig;
use cc_switch_lib::remote::ssh::{
    build_ssh_args, classify_ssh_failure, parse_remote_platform, RemoteSshError,
};
use cc_switch_lib::remote::target_store::{RemoteTargetStore, TargetStoreError};
use std::ffi::OsString;

fn target(id: &str, name: &str, host_alias: &str) -> RemoteTargetConfig {
    RemoteTargetConfig {
        id: id.to_string(),
        name: name.to_string(),
        host_alias: host_alias.to_string(),
        username: None,
        port: None,
        identity_file: None,
    }
}

#[test]
fn target_store_round_trips_and_updates_atomically() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("remote-targets.json");
    let store = RemoteTargetStore::at(path.clone());

    let first = target("target-a", "  Production  ", "prod-api");
    store.upsert(first).expect("save target");
    store
        .set_active_target(Some("target-a".to_string()))
        .expect("activate target");

    let document = store.load().expect("load targets");
    assert_eq!(document.targets.len(), 1);
    assert_eq!(document.targets[0].name, "Production");
    assert_eq!(document.active_target_id.as_deref(), Some("target-a"));
    assert!(!path.with_extension("json.tmp").exists());

    store
        .upsert(target("target-a", "Production API", "prod-api"))
        .expect("update target");
    let document = store.load().expect("reload targets");
    assert_eq!(document.targets.len(), 1);
    assert_eq!(document.targets[0].name, "Production API");
}

#[test]
fn target_store_rejects_corrupt_json_without_overwriting_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("remote-targets.json");
    std::fs::write(&path, "{ broken").expect("seed corrupt file");
    let store = RemoteTargetStore::at(path.clone());

    let error = store.load().expect_err("corrupt JSON must be surfaced");

    assert!(matches!(error, TargetStoreError::InvalidData(_)));
    assert_eq!(
        std::fs::read_to_string(path).expect("read original"),
        "{ broken"
    );
}

#[test]
fn ssh_args_are_separate_and_reject_option_injection() {
    let mut config = target("target-a", "Production", "prod-api");
    config.username = Some("deploy".to_string());
    config.port = Some(2222);
    config.identity_file = Some("C:\\Keys\\production key".to_string());

    let args = build_ssh_args(
        &config,
        &[
            "~/.cc-switch/agents/3.18.0/cc-switch-agent".to_string(),
            "--stdio".to_string(),
        ],
    )
    .expect("build SSH args");

    assert!(args
        .windows(2)
        .any(|pair| pair == [OsString::from("-l"), OsString::from("deploy")]));
    assert!(args
        .windows(2)
        .any(|pair| pair == [OsString::from("-p"), OsString::from("2222")]));
    assert!(args.windows(2).any(|pair| {
        pair == [
            OsString::from("-i"),
            OsString::from("C:\\Keys\\production key"),
        ]
    }));
    assert!(args.contains(&OsString::from("BatchMode=yes")));
    assert!(args.contains(&OsString::from("StrictHostKeyChecking=yes")));
    assert!(args.contains(&OsString::from("prod-api")));

    config.host_alias = "-oProxyCommand=bad".to_string();
    assert!(build_ssh_args(&config, &[]).is_err());
}

#[test]
fn ssh_errors_and_linux_platform_are_classified_stably() {
    assert!(matches!(
        classify_ssh_failure("Host key verification failed."),
        RemoteSshError::HostKeyNotTrusted
    ));
    assert!(matches!(
        classify_ssh_failure("Permission denied (publickey)."),
        RemoteSshError::AuthenticationFailed
    ));
    assert!(matches!(
        parse_remote_platform("Linux\nx86_64\n"),
        Ok(platform) if platform.os == "linux" && platform.architecture == "x86_64"
    ));
    assert!(matches!(
        parse_remote_platform("Darwin\narm64\n"),
        Err(RemoteSshError::PlatformUnsupported(platform)) if platform == "Darwin"
    ));
}
