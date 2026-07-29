use cc_switch_lib::remote::models::{RemoteConnectionStatus, RemoteTargetConfig};
use cc_switch_lib::remote::runtime::RemoteRuntimeState;
use cc_switch_lib::remote::target_store::RemoteTargetStore;

fn target() -> RemoteTargetConfig {
    RemoteTargetConfig {
        id: "prod".to_string(),
        name: "Production".to_string(),
        host_alias: "prod-api".to_string(),
        username: None,
        port: None,
        identity_file: None,
    }
}

#[test]
fn runtime_starts_local_and_manages_saved_targets_without_connecting() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = RemoteTargetStore::at(temp.path().join("remote-targets.json"));
    let runtime = RemoteRuntimeState::new(store).expect("create runtime");

    let snapshot = runtime.snapshot().expect("local snapshot");
    assert_eq!(snapshot.status, RemoteConnectionStatus::Local);
    assert_eq!(snapshot.generation, 0);
    assert!(snapshot.active_target_id.is_none());

    runtime.upsert_target(target()).expect("save target");
    assert_eq!(runtime.list_targets().expect("list targets").len(), 1);
    runtime.delete_target("prod").expect("delete target");
    assert!(runtime
        .list_targets()
        .expect("list after delete")
        .is_empty());

    let error = runtime
        .invoke_remote("provider.list", serde_json::json!({ "app": "codex" }))
        .expect_err("offline runtime must reject remote calls");
    assert_eq!(error.code(), "REMOTE_OFFLINE");
}

#[test]
fn connection_test_validates_target_before_starting_ssh() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = RemoteTargetStore::at(temp.path().join("remote-targets.json"));
    let runtime = RemoteRuntimeState::new(store).expect("create runtime");
    let mut invalid = target();
    invalid.host_alias = "-unsafe-option".to_string();

    // 连接测试必须复用生产校验，避免用户输入被 OpenSSH 误解释为命令行选项。
    let error = runtime
        .test_target(&invalid)
        .expect_err("invalid target must fail before spawning ssh");
    assert_eq!(error.code(), "REMOTE_TARGET_INVALID");
}
