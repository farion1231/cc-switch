use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex};

use cc_switch_lib::remote::embedded_agent::{
    embedded_agent_catalog, AgentArchitecture, EphemeralAgentSpec,
};
use cc_switch_lib::remote::ephemeral_deploy::{
    build_cleanup_command, build_launch_command, build_preflight_command, build_scp_args,
    CleanupScheduler, EphemeralCleanupGuard,
};
use cc_switch_lib::remote::models::RemoteTargetConfig;
use cc_switch_lib::remote::ssh::RemoteSshError;

const SAFE_REMOTE_COMMAND_PREFIX: &str =
    r#"PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin${PATH:+:$PATH}"; export PATH; "#;

fn target() -> RemoteTargetConfig {
    RemoteTargetConfig {
        id: "remote-test".to_string(),
        name: "Remote Test".to_string(),
        host_alias: "build-host".to_string(),
        username: Some("deploy".to_string()),
        port: Some(2222),
        identity_file: Some("C:\\Keys With Spaces\\agent key".to_string()),
    }
}

#[test]
fn agent_spec_normalizes_architecture_hashes_bytes_and_randomizes_path() {
    let first = EphemeralAgentSpec::for_architecture("amd64", b"abc").expect("x86_64 架构");
    let second = EphemeralAgentSpec::for_architecture("x86_64", b"abc").expect("x86_64 别名");

    assert_eq!(first.architecture, AgentArchitecture::X86_64);
    assert_eq!(first.length, 3);
    assert_eq!(
        first.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert!(first.remote_path.starts_with("/tmp/cc-switch-agent-"));
    assert!(first
        .remote_path
        .trim_start_matches("/tmp/cc-switch-agent-")
        .chars()
        .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()));
    assert_ne!(first.remote_path, second.remote_path);
    assert!(EphemeralAgentSpec::for_architecture("riscv64", b"abc").is_err());
}

#[test]
fn scp_and_remote_commands_keep_transport_arguments_isolated() {
    let spec = EphemeralAgentSpec::for_architecture("aarch64", b"agent").expect("aarch64");
    let args = build_scp_args(
        &target(),
        Path::new("C:\\Temp Files\\agent bin"),
        &spec.remote_path,
    )
    .expect("构造 scp 参数");

    assert!(args.contains(&OsString::from("C:\\Temp Files\\agent bin")));
    assert!(args.contains(&OsString::from("C:\\Keys With Spaces\\agent key")));
    assert_eq!(
        args.last(),
        Some(&OsString::from(format!(
            "deploy@build-host:{}",
            spec.remote_path
        )))
    );
    assert!(!args
        .iter()
        .any(|arg| arg.to_string_lossy().contains("~/.cc-switch/agents")));

    let launch = build_launch_command(&spec);
    assert!(launch.contains("trap cleanup EXIT HUP INT TERM"));
    assert!(launch.contains(&spec.sha256));
    assert!(launch.contains(&spec.length.to_string()));
    assert!(launch.contains("--stdio"));
    assert!(!launch.contains("CC_SWITCH_AGENT_ARTIFACT"));

    // 非交互式 shell 可能不继承标准 PATH；启动与清理必须共享同一前缀，避免只修一条退出路径。
    assert!(launch.starts_with(SAFE_REMOTE_COMMAND_PREFIX));
    for executable in ["wc", "tr", "sha256sum", "awk", "chmod", "rm"] {
        assert!(
            launch.contains(&format!("command {executable}")),
            "启动命令必须绕过 {executable} 的 alias 或同名函数"
        );
    }

    let cleanup = build_cleanup_command(&spec.remote_path);
    assert!(cleanup.starts_with(SAFE_REMOTE_COMMAND_PREFIX));
    assert!(cleanup.contains(&format!(
        "command rm -f -- '{}'",
        spec.remote_path
    )));
}

#[test]
fn preflight_uses_the_same_safe_remote_environment() {
    let command = build_preflight_command();

    // 预检和 Agent 生命周期命令必须保持同一环境契约，否则 PATH 异常会在不同阶段产生不一致错误。
    assert!(command.starts_with(SAFE_REMOTE_COMMAND_PREFIX));
    assert!(command.contains("command uname -s; command uname -m"));
}

#[derive(Default)]
struct RecordingScheduler {
    paths: Mutex<Vec<String>>,
}

impl CleanupScheduler for RecordingScheduler {
    fn schedule(&self, _target: &RemoteTargetConfig, remote_path: &str) {
        self.paths
            .lock()
            .expect("记录清理路径")
            .push(remote_path.to_string());
    }
}

#[test]
fn every_session_exit_path_schedules_idempotent_cleanup() {
    // 四种场景分别模拟上传错误、握手错误、正常 EOF 和主动 kill；守卫必须在上传前创建，
    // 才能覆盖“远端可能已经收到部分文件但 scp 返回失败”的边界。
    for exit_path in [
        "upload_failure",
        "handshake_failure",
        "normal_eof",
        "child_kill",
    ] {
        let scheduler = Arc::new(RecordingScheduler::default());
        let spec = EphemeralAgentSpec::for_architecture("x86_64", exit_path.as_bytes())
            .expect("创建临时 Agent 规范");
        let expected_path = spec.remote_path.clone();
        {
            let mut guard =
                EphemeralCleanupGuard::new(target(), expected_path.clone(), scheduler.clone());
            if exit_path == "normal_eof" {
                guard.schedule_cleanup();
                guard.schedule_cleanup();
            }
        }
        assert_eq!(
            scheduler.paths.lock().expect("读取清理记录").as_slice(),
            [expected_path]
        );
    }
}

#[test]
fn active_ssh_connection_contains_no_persistent_install_or_external_artifact_lookup() {
    let source = include_str!("../src/remote/ssh.rs");
    assert!(!source.contains("~/.cc-switch/agents"));
    assert!(!source.contains("CC_SWITCH_AGENT_ARTIFACT"));
    assert!(!source.contains("fn ensure_agent"));
    assert!(!source.contains("fn resolve_agent_artifact"));
}

#[test]
fn ephemeral_agent_failures_expose_stable_error_codes() {
    assert_eq!(
        RemoteSshError::AgentEmbeddedArtifactMissing {
            architecture: "x86_64".to_string(),
        }
        .code(),
        "AGENT_EMBEDDED_ARTIFACT_MISSING"
    );
    assert_eq!(
        RemoteSshError::AgentUploadFailed("scp failed".to_string()).code(),
        "AGENT_UPLOAD_FAILED"
    );
    assert_eq!(
        RemoteSshError::AgentIntegrityFailed("sha mismatch".to_string()).code(),
        "AGENT_INTEGRITY_FAILED"
    );
    assert_eq!(
        RemoteSshError::AgentStartFailed("noexec".to_string()).code(),
        "AGENT_START_FAILED"
    );
    assert_eq!(
        RemoteSshError::AgentIncompatible("protocol".to_string()).code(),
        "AGENT_INCOMPATIBLE"
    );
}

#[test]
fn desktop_catalog_exposes_both_linux_agent_architectures() {
    let catalog = embedded_agent_catalog();
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[0].architecture, AgentArchitecture::X86_64);
    assert_eq!(catalog[1].architecture, AgentArchitecture::Aarch64);
    // 本地开发允许条目为空并返回明确构建缺陷；发布工作流必须在 build.rs 前提供非空产物。
    assert_eq!(catalog[0].length, catalog[0].bytes.len());
    assert_eq!(catalog[1].length, catalog[1].bytes.len());
}
