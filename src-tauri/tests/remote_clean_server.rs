use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use cc_switch_lib::remote::models::RemoteTargetConfig;
use cc_switch_lib::remote::ssh::{build_ssh_args, OpenSshSession};
use serde_json::json;

fn clean_server_target() -> Option<RemoteTargetConfig> {
    let host_alias = std::env::var("CC_SWITCH_CLEAN_SSH_HOST").ok()?;
    let username = std::env::var("CC_SWITCH_CLEAN_SSH_USER").ok()?;
    let port = std::env::var("CC_SWITCH_CLEAN_SSH_PORT")
        .ok()?
        .parse()
        .ok()?;
    let identity_file = std::env::var("CC_SWITCH_CLEAN_SSH_KEY").ok()?;
    Some(RemoteTargetConfig {
        id: "clean-server".to_string(),
        name: "Clean Server".to_string(),
        host_alias,
        username: Some(username),
        port: Some(port),
        identity_file: Some(identity_file),
    })
}

fn remote_output(target: &RemoteTargetConfig, command: &str) -> String {
    let args = build_ssh_args(target, &[command.to_string()]).expect("构造检查命令");
    let output = Command::new("ssh")
        .args(args)
        .output()
        .expect("执行远端检查命令");
    assert!(
        output.status.success(),
        "远端检查失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn provider(id: &str, name: &str) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "settingsConfig": {
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_AUTH_TOKEN": format!("sk-{id}")
            }
        },
        "category": "custom"
    })
}

#[test]
fn clean_linux_server_runs_ephemeral_provider_slice_and_leaves_no_agent() {
    let Some(target) = clean_server_target() else {
        // 本测试只在 Linux CI 创建 sshd 后启用；普通本地测试不要求 Docker 或 SSH 凭据。
        return;
    };

    assert_eq!(
        remote_output(&target, "command -v rustc || true; command -v node || true"),
        "",
        "干净服务器不应预装 Rust 或 Node"
    );
    let listeners_before = remote_output(
        &target,
        "awk '$4 == \"0A\" { count++ } END { print count + 0 }' /proc/net/tcp /proc/net/tcp6",
    );

    {
        let mut session = OpenSshSession::connect(&target).expect("连接干净 Linux 服务器");
        for (index, (id, name)) in [("clean-a", "Clean A"), ("clean-b", "Clean B")]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                session
                    .invoke(
                        &format!("add-{index}"),
                        "provider.add",
                        json!({ "app": "claude", "provider": provider(id, name), "addToLive": false }),
                        30_000,
                    )
                    .expect("远端新增供应商"),
                json!(true)
            );
        }
        session
            .invoke(
                "switch",
                "provider.switch",
                json!({ "app": "claude", "id": "clean-b" }),
                30_000,
            )
            .expect("远端切换供应商");
        let providers = session
            .invoke("list", "provider.list", json!({ "app": "claude" }), 30_000)
            .expect("远端列出供应商");
        assert_eq!(providers["clean-a"]["name"], "Clean A");
        assert_eq!(providers["clean-b"]["name"], "Clean B");
    }

    // launch trap 是第一道清理，Drop 守卫还会异步发起独立 SSH 删除；允许短暂调度延迟。
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let residue = remote_output(
            &target,
            "find \"$HOME\" /tmp /dev/shm -maxdepth 3 -name 'cc-switch-agent*' -print -quit 2>/dev/null",
        );
        if residue.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "远端残留 Agent 文件: {residue}");
        std::thread::sleep(Duration::from_millis(200));
    }

    let listeners_after = remote_output(
        &target,
        "awk '$4 == \"0A\" { count++ } END { print count + 0 }' /proc/net/tcp /proc/net/tcp6",
    );
    assert_eq!(listeners_after, listeners_before, "Agent 不应新增监听端口");
    assert!(PathBuf::from(target.identity_file.expect("identity file")).is_file());
}
