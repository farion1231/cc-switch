use codex_plus_core::zed_remote::{
    self, SshTarget, ZedOpenStrategy, ZedRemoteError, ZedRemoteProjectSource,
};
use serde_json::json;

#[test]
fn build_zed_remote_url_with_user_host_port_and_encoded_path() {
    let url = zed_remote::build_zed_remote_url(
        &SshTarget {
            user: "alice".to_string(),
            host: "example.com".to_string(),
            port: Some(2222),
        },
        "/home/alice/My Project/你好.py",
    )
    .unwrap();

    assert_eq!(
        url,
        "ssh://alice@example.com:2222/home/alice/My%20Project/%E4%BD%A0%E5%A5%BD.py"
    );
}

#[test]
fn build_zed_remote_url_allows_host_without_user() {
    let url = zed_remote::build_zed_remote_url(
        &SshTarget {
            user: String::new(),
            host: "box.internal".to_string(),
            port: None,
        },
        "/srv/app/main.py",
    )
    .unwrap();

    assert_eq!(url, "ssh://box.internal/srv/app/main.py");
}

#[test]
fn build_zed_remote_url_rejects_invalid_inputs() {
    let error = zed_remote::build_zed_remote_url(
        &SshTarget {
            user: "alice".to_string(),
            host: "bad host".to_string(),
            port: None,
        },
        "/a.py",
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ZedRemoteError::Validation("Invalid SSH host")
    ));
}

#[test]
fn build_zed_remote_url_allows_bracketed_ipv6_host() {
    let url = zed_remote::build_zed_remote_url(
        &SshTarget {
            user: "alice".to_string(),
            host: "[::1]".to_string(),
            port: Some(2222),
        },
        "/home/alice/a.py",
    )
    .unwrap();

    assert_eq!(url, "ssh://alice@[::1]:2222/home/alice/a.py");
}

#[test]
fn open_strategy_defaults_to_add_to_focused_workspace() {
    assert_eq!(
        zed_remote::zed_open_strategy_from_payload(&json!({})),
        ZedOpenStrategy::AddToFocusedWorkspace
    );
    assert_eq!(
        zed_remote::zed_open_strategy_from_payload(&json!({"strategy": "reuseWindow"})),
        ZedOpenStrategy::ReuseWindow
    );
    assert_eq!(
        zed_remote::zed_open_strategy_from_payload(&json!({"strategy": "unknown"})),
        ZedOpenStrategy::AddToFocusedWorkspace
    );
}

#[test]
fn launch_args_for_add_strategy_are_zed_dash_a_url() {
    assert_eq!(
        zed_remote::zed_cli_args_for_strategy(
            ZedOpenStrategy::AddToFocusedWorkspace,
            "ssh://example.com/home/app"
        ),
        vec!["-a".to_string(), "ssh://example.com/home/app".to_string()]
    );
}

#[test]
fn launch_args_for_reuse_strategy_are_zed_dash_r_url() {
    assert_eq!(
        zed_remote::zed_cli_args_for_strategy(
            ZedOpenStrategy::ReuseWindow,
            "ssh://example.com/home/app"
        ),
        vec!["-r".to_string(), "ssh://example.com/home/app".to_string()]
    );
}

#[test]
fn launch_args_for_new_window_strategy_are_zed_dash_n_url() {
    assert_eq!(
        zed_remote::zed_cli_args_for_strategy(
            ZedOpenStrategy::NewWindow,
            "ssh://example.com/home/app"
        ),
        vec!["-n".to_string(), "ssh://example.com/home/app".to_string()]
    );
}

#[test]
fn launch_args_for_default_strategy_are_plain_url() {
    assert_eq!(
        zed_remote::zed_cli_args_for_strategy(
            ZedOpenStrategy::Default,
            "ssh://example.com/home/app"
        ),
        vec!["ssh://example.com/home/app".to_string()]
    );
}

#[test]
fn target_from_payload_splits_codex_managed_authority() {
    let target =
        zed_remote::target_from_payload(&json!({"ssh": {"host": "longnv@192.168.100.31"}}))
            .unwrap();

    assert_eq!(
        target,
        SshTarget {
            user: "longnv".to_string(),
            host: "192.168.100.31".to_string(),
            port: None,
        }
    );
}

#[test]
fn registry_lists_remote_projects_from_global_state() {
    let state = json!({
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
            "label": "sealos-skills",
        }],
        "project-order": ["main"],
    });
    let temp = tempfile::tempdir().unwrap();
    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({}),
        Some(&temp.path().join("recent.json")),
        None,
    )
    .unwrap();

    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].label, "sealos-skills");
    assert_eq!(
        projects[0].source,
        ZedRemoteProjectSource::CodexRemoteProject
    );
    assert_eq!(
        projects[0].url,
        "ssh://longnv@192.168.100.31/Users/longnv/bin/repo/sealos-skills"
    );
}

#[test]
fn registry_prefers_current_thread_workspace_hint() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        },
    });
    let temp = tempfile::tempdir().unwrap();
    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({"threadId": "019e39c1-worktree"}),
        Some(&temp.path().join("recent.json")),
        None,
    )
    .unwrap();
    let current = projects.iter().find(|project| project.is_current).unwrap();

    assert_eq!(
        current.path,
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
    assert_eq!(current.source, ZedRemoteProjectSource::CurrentThread);
}

#[test]
fn registry_dedupes_same_user_host_port_path() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills",
        },
    });
    let temp = tempfile::tempdir().unwrap();
    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({"threadId": "019e39c1-worktree"}),
        Some(&temp.path().join("recent.json")),
        None,
    )
    .unwrap();

    assert_eq!(
        projects
            .iter()
            .filter(|project| project.path == "/Users/longnv/bin/repo/sealos-skills")
            .count(),
        1
    );
}

#[test]
fn registry_marks_recent_opened_project() {
    let state = json!({
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
            "label": "sealos-skills",
        }],
    });
    let temp = tempfile::tempdir().unwrap();
    let registry_path = temp.path().join("recent.json");
    let mut projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({}),
        Some(&registry_path),
        None,
    )
    .unwrap();
    projects[0].source = ZedRemoteProjectSource::Recent;
    projects[0].last_opened_at_ms = Some(42);
    std::fs::write(
        &registry_path,
        serde_json::to_vec(&json!({ "projects": projects })).unwrap(),
    )
    .unwrap();

    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({}),
        Some(&registry_path),
        None,
    )
    .unwrap();

    assert_eq!(projects[0].last_opened_at_ms, Some(42));
}

#[test]
fn registry_lists_sqlite_thread_cwd_candidates() {
    let state = json!({
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
    });
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("state_5.sqlite");
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT NOT NULL)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads (id, cwd) VALUES (?1, ?2)",
        (
            "019e39c1-worktree",
            "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        ),
    )
    .unwrap();
    drop(db);

    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({}),
        Some(&temp.path().join("recent.json")),
        Some(&db_path),
    )
    .unwrap();

    assert!(projects.iter().any(|project| {
        project.source == ZedRemoteProjectSource::SqliteThreadCwd
            && project.path == "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    }));
}

#[test]
fn resolve_ssh_target_from_global_state_for_codex_managed_connection() {
    let state = json!({
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "displayName": "remote",
            "source": "codex-managed",
            "hostname": "longnv@192.168.100.31",
            "sshPort": null,
        }]
    });

    let target =
        zed_remote::resolve_ssh_target_from_global_state(&state, "remote-ssh-codex-managed:remote")
            .unwrap();

    assert_eq!(
        target,
        SshTarget {
            user: "longnv".to_string(),
            host: "192.168.100.31".to_string(),
            port: None,
        }
    );
}

#[test]
fn fallback_open_request_uses_selected_remote_project() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
            "sshPort": null,
        }],
        "remote-projects": [{
            "id": "032e652b-7956-4e6e-83bd-b29f456c6c3d",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
            "label": "sealos-skills",
        }],
        "project-order": ["032e652b-7956-4e6e-83bd-b29f456c6c3d"],
    });

    let request =
        zed_remote::fallback_open_request_from_global_state_with_context(&state, "", "", "", "")
            .unwrap();

    assert_eq!(
        request,
        json!({
            "hostId": "remote-ssh-codex-managed:remote",
            "ssh": {"user": "longnv", "host": "192.168.100.31", "port": null},
            "path": "/Users/longnv/bin/repo/sealos-skills",
        })
    );
}

#[test]
fn fallback_open_request_prefers_project_order_for_selected_host() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [
            {"id": "old", "hostId": "remote-ssh-codex-managed:remote", "remotePath": "/Users/longnv/bin/repo/old"},
            {"id": "current", "hostId": "remote-ssh-codex-managed:remote", "remotePath": "/Users/longnv/bin/repo/current"},
            {"id": "other-host", "hostId": "remote-ssh-codex-managed:other", "remotePath": "/srv/other"}
        ],
        "project-order": ["other-host", "current", "old"],
    });

    let request =
        zed_remote::fallback_open_request_from_global_state_with_context(&state, "", "", "", "")
            .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(request["path"], "/Users/longnv/bin/repo/current");
}

#[test]
fn fallback_open_request_prefers_remote_project_id_context() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [
            {
                "id": "032e652b-7956-4e6e-83bd-b29f456c6c3d",
                "hostId": "remote-ssh-codex-managed:remote",
                "remotePath": "/Users/longnv/bin/repo/sealos-skills",
            },
            {
                "id": "a21be7c9-a917-433a-bfc7-f422a34c2185",
                "hostId": "remote-ssh-codex-managed:remote",
                "remotePath": "/Users/longnv/bin/repo/Vocabloom",
            },
        ],
        "project-order": ["032e652b-7956-4e6e-83bd-b29f456c6c3d", "a21be7c9-a917-433a-bfc7-f422a34c2185"],
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "remote-ssh-codex-managed:remote",
        "",
        "",
        "a21be7c9-a917-433a-bfc7-f422a34c2185",
    )
    .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(request["path"], "/Users/longnv/bin/repo/Vocabloom");
}

#[test]
fn fallback_open_request_treats_remote_project_id_as_path() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "032e652b-7956-4e6e-83bd-b29f456c6c3d",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "project-order": ["032e652b-7956-4e6e-83bd-b29f456c6c3d"],
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "remote-ssh-codex-managed:remote",
        "",
        "",
        "/Users/longnv/bin/repo/Vocabloom",
    )
    .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(request["path"], "/Users/longnv/bin/repo/Vocabloom");
}

#[test]
fn fallback_open_request_prefers_thread_workspace_hint() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "project-order": ["main"],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        },
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "",
        "019e39c1-worktree",
        "",
        "",
    )
    .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(
        request["path"],
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
}

#[test]
fn fallback_open_request_accepts_local_prefixed_thread_workspace_hint() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "project-order": ["main"],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        },
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "",
        "local:019e39c1-worktree",
        "",
        "",
    )
    .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(
        request["path"],
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
}

#[test]
fn fallback_open_request_response_passes_thread_workspace_hint() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        },
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "",
        "019e39c1-worktree",
        "",
        "",
    )
    .unwrap();

    assert_eq!(
        request["path"],
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
}

#[test]
fn workspace_root_from_sqlite_reads_thread_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT NOT NULL)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads (id, cwd) VALUES (?1, ?2)",
        (
            "019e39c1-worktree",
            "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        ),
    )
    .unwrap();
    drop(db);

    let cwd = zed_remote::workspace_root_from_sqlite("local:019e39c1-worktree", Some(&db_path));

    assert_eq!(
        cwd,
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
}

#[test]
fn fallback_open_request_reports_missing_remote_project() {
    let state = json!({"selected-remote-host-id": "remote-ssh-codex-managed:remote"});

    let error =
        zed_remote::fallback_open_request_from_global_state_with_context(&state, "", "", "", "")
            .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Cannot determine remote workspace or file for Zed"
    );
}

#[test]
fn resolve_ssh_target_response_reports_missing_host_id() {
    let result = zed_remote::resolve_ssh_target_response(&json!({"hostId": ""}));

    assert_eq!(
        result,
        json!({"status": "failed", "message": "Remote host id is required"})
    );
}

#[test]
fn open_zed_remote_returns_failed_response_for_validation_error() {
    let result = zed_remote::open_zed_remote(&json!({"ssh": {"host": ""}, "path": "/a.py"}));

    assert_eq!(
        result,
        json!({"status": "failed", "message": "Cannot determine remote SSH host for this file"})
    );
}
