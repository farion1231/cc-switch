use std::path::Path;
use std::process::Command;

use crate::zed_remote::SshTarget;

use super::{remote_defaults_snapshot_script, remote_git_command, remote_path_join, shell_quote};
use crate::upstream_worktree::UpstreamRemoteProject;

fn remote_project_fixture() -> UpstreamRemoteProject {
    UpstreamRemoteProject {
        project_id: "project-id".to_string(),
        host_id: "remote-ssh-codex-managed:remote".to_string(),
        remote_path: "/Users/longnv/bin/repo/project".to_string(),
        label: "project".to_string(),
    }
}

#[test]
fn shell_quote_escapes_single_quotes_for_remote_git_command() {
    assert_eq!(shell_quote("/tmp/repo"), "'/tmp/repo'");
    assert_eq!(shell_quote("/tmp/a'b"), "'/tmp/a'\\''b'");
}

#[test]
fn remote_path_join_keeps_absolute_paths_and_resolves_relative_paths() {
    assert_eq!(
        remote_path_join("/Users/longnv/bin/repo/project", Path::new("/tmp/wt")),
        "/tmp/wt"
    );
    assert_eq!(
        remote_path_join(
            "/Users/longnv/bin/repo/project",
            Path::new(".worktree/task")
        ),
        "/Users/longnv/bin/repo/project/.worktree/task"
    );
}

#[test]
fn remote_git_command_targets_project_remote_path_over_ssh() {
    let project = remote_project_fixture();
    let target = SshTarget {
        user: "longnv".to_string(),
        host: "100.125.101.8".to_string(),
        port: Some(2222),
    };

    let command = remote_git_command(
        &project,
        &target,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/remotes/upstream",
        ],
    );

    assert_eq!(command.destination, "longnv@100.125.101.8");
    assert_eq!(command.port, Some(2222));
    assert_eq!(
        command.command,
        "'git' '-C' '/Users/longnv/bin/repo/project' 'for-each-ref' '--format=%(refname)' 'refs/remotes/upstream'"
    );
}

#[test]
fn remote_defaults_snapshot_script_collects_defaults_with_one_ssh_command() {
    let script = remote_defaults_snapshot_script("/Users/longnv/bin/repo/project");

    assert!(script.contains("cd '/Users/longnv/bin/repo/project'"));
    assert!(script.contains("git rev-parse --show-toplevel"));
    assert!(script.contains("git branch --show-current"));
    assert!(script.contains("git remote"));
    assert!(script.contains("git for-each-ref '--format=%(refname)' refs/remotes"));
    assert!(script.contains("git worktree list --porcelain"));
}

#[test]
fn remote_defaults_snapshot_script_is_valid_posix_shell() {
    let script = remote_defaults_snapshot_script("/Users/longnv/bin/repo/project");
    let output = match Command::new("sh").arg("-n").arg("-c").arg(&script).output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("sh should parse snapshot script: {error}"),
    };

    assert!(
        output.status.success(),
        "snapshot script should be valid POSIX shell\nscript:\n{script}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
