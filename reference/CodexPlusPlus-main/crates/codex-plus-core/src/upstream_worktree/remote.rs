use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use super::defaults::{default_remote_name, refs_from_output, worktree_branches_from_output};
use super::git::{qualified_remote_ref, source_ref, string_field};
use super::types::{
    GitOutput, RemoteGitCommand, UpstreamRemoteProject, UpstreamWorktreeCode,
    UpstreamWorktreeError, UpstreamWorktreeRequest, UpstreamWorktreeResult,
    UpstreamWorktreeSourceRequest,
};
use crate::zed_remote::{SshTarget, resolve_ssh_target_for_host_id};

pub fn codex_global_state_path() -> PathBuf {
    crate::codex_home::default_codex_home_dir().join(".codex-global-state.json")
}

pub fn remote_project_from_state(state: &Value, project_id: &str) -> Option<UpstreamRemoteProject> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return None;
    }
    state
        .get("remote-projects")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_object)
        .find_map(|project| {
            let project_value = Value::Object(project.clone());
            if string_field(&project_value, "id") != project_id {
                return None;
            }
            let host_id = string_field(&project_value, "hostId");
            let remote_path = string_field(&project_value, "remotePath");
            if host_id.is_empty() || !remote_path.starts_with('/') {
                return None;
            }
            Some(UpstreamRemoteProject {
                project_id: project_id.to_string(),
                host_id,
                remote_path,
                label: string_field(&project_value, "label"),
            })
        })
}

pub fn remote_project_from_state_path(
    project_id: &str,
    state_path: &Path,
) -> Option<UpstreamRemoteProject> {
    let data = std::fs::read_to_string(state_path).ok()?;
    let state = serde_json::from_str::<Value>(&data).ok()?;
    remote_project_from_state(&state, project_id)
}

pub fn remote_project_for_id(project_id: &str) -> Option<UpstreamRemoteProject> {
    remote_project_from_state_path(project_id, &codex_global_state_path())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn ssh_destination(target: &SshTarget) -> String {
    if target.user.trim().is_empty() {
        target.host.trim().to_string()
    } else {
        format!("{}@{}", target.user.trim(), target.host.trim())
    }
}

fn remote_git_command(
    project: &UpstreamRemoteProject,
    target: &SshTarget,
    args: &[&str],
) -> RemoteGitCommand {
    let remote_command = std::iter::once("git".to_string())
        .chain(std::iter::once("-C".to_string()))
        .chain(std::iter::once(project.remote_path.clone()))
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .map(|arg| shell_quote(&arg))
        .collect::<Vec<_>>()
        .join(" ");
    RemoteGitCommand {
        destination: ssh_destination(target),
        port: target.port,
        command: remote_command,
    }
}

fn spawn_remote_git(command_spec: &RemoteGitCommand) -> Result<GitOutput, std::io::Error> {
    let mut command = Command::new("ssh");
    command.arg("-o").arg("BatchMode=yes");
    command.arg("-o").arg("ConnectTimeout=8");
    if let Some(port) = command_spec.port {
        command.arg("-p").arg(port.to_string());
    }
    command
        .arg(&command_spec.destination)
        .arg(&command_spec.command);
    let output = command.output().map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("Cannot run remote git over SSH: {error}"),
        )
    })?;
    Ok(GitOutput {
        status_success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn remote_git(project: &UpstreamRemoteProject, args: &[&str]) -> UpstreamWorktreeResult<GitOutput> {
    let target = resolve_ssh_target_for_host_id(&project.host_id, None).map_err(|error| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, error.to_string())
    })?;
    let command_spec = remote_git_command(project, &target, args);
    let output = spawn_remote_git(&command_spec).map_err(|error| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, error.to_string())
    })?;
    Ok(output)
}

fn remote_shell(
    project: &UpstreamRemoteProject,
    script: &str,
) -> UpstreamWorktreeResult<GitOutput> {
    let target = resolve_ssh_target_for_host_id(&project.host_id, None).map_err(|error| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, error.to_string())
    })?;
    let command_spec = RemoteGitCommand {
        destination: ssh_destination(&target),
        port: target.port,
        command: script.to_string(),
    };
    let output = spawn_remote_git(&command_spec).map_err(|error| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, error.to_string())
    })?;
    Ok(output)
}

fn remote_defaults_snapshot_script(remote_path: &str) -> String {
    let quoted_remote_path = shell_quote(remote_path);
    [
        "set -e",
        &format!("cd {quoted_remote_path}"),
        "printf '__ROOT__\\n'",
        "git rev-parse --show-toplevel",
        "printf '__BRANCH__\\n'",
        "git branch --show-current || true",
        "printf '__REMOTES__\\n'",
        "git remote",
        "printf '__REFS__\\n'",
        "git for-each-ref '--format=%(refname)' refs/remotes",
        "printf '__WORKTREES__\\n'",
        "git worktree list --porcelain",
    ]
    .join("\n")
}

#[derive(Debug, Default)]
struct RemoteDefaultsSnapshot {
    root: String,
    branch: String,
    remotes: Vec<String>,
    refs_output: String,
    worktrees_output: String,
}

fn parse_remote_defaults_snapshot(output: &str) -> RemoteDefaultsSnapshot {
    let mut snapshot = RemoteDefaultsSnapshot::default();
    let mut section = "";

    for line in output.lines() {
        match line {
            "__ROOT__" => {
                section = "root";
                continue;
            }
            "__BRANCH__" => {
                section = "branch";
                continue;
            }
            "__REMOTES__" => {
                section = "remotes";
                continue;
            }
            "__REFS__" => {
                section = "refs";
                continue;
            }
            "__WORKTREES__" => {
                section = "worktrees";
                continue;
            }
            _ => {}
        }

        match section {
            "root" if snapshot.root.is_empty() => snapshot.root = line.trim().to_string(),
            "branch" if snapshot.branch.is_empty() => snapshot.branch = line.trim().to_string(),
            "remotes" => {
                let remote = line.trim();
                if !remote.is_empty() {
                    snapshot.remotes.push(remote.to_string());
                }
            }
            "refs" => {
                snapshot.refs_output.push_str(line);
                snapshot.refs_output.push('\n');
            }
            "worktrees" => {
                snapshot.worktrees_output.push_str(line);
                snapshot.worktrees_output.push('\n');
            }
            _ => {}
        }
    }

    snapshot
}

fn remote_repo_root(project: &UpstreamRemoteProject) -> UpstreamWorktreeResult<String> {
    let output = remote_git(project, &["rev-parse", "--show-toplevel"])?;
    if output.status_success && !output.stdout.is_empty() {
        Ok(output.stdout)
    } else {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::NotGitRepo,
            if output.stderr.is_empty() {
                "Remote path is not inside a Git repository".to_string()
            } else {
                output.stderr
            },
        ))
    }
}

fn remote_names(project: &UpstreamRemoteProject) -> UpstreamWorktreeResult<Vec<String>> {
    let output = remote_git(project, &["remote"])?;
    if !output.status_success {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::RemoteMissing,
            if output.stderr.is_empty() {
                "Cannot read remote Git remotes".to_string()
            } else {
                output.stderr
            },
        ));
    }
    Ok(output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

pub fn defaults_for_remote_project(
    project: &UpstreamRemoteProject,
) -> UpstreamWorktreeResult<Value> {
    let output = remote_shell(
        project,
        &remote_defaults_snapshot_script(&project.remote_path),
    )?;
    if !output.status_success {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::NotGitRepo,
            if output.stderr.is_empty() {
                "Remote path is not inside a Git repository".to_string()
            } else {
                output.stderr
            },
        ));
    }
    let snapshot = parse_remote_defaults_snapshot(&output.stdout);
    let root = snapshot.root;
    if root.is_empty() {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::NotGitRepo,
            "Remote path is not inside a Git repository",
        ));
    }
    let branch = snapshot.branch;
    let remotes = snapshot.remotes;
    let default_base_branch = if branch.is_empty() {
        "main".to_string()
    } else {
        branch.clone()
    };
    let default_remote = default_remote_name(&remotes);
    Ok(json!({
        "status": "ok",
        "remoteProject": true,
        "projectId": project.project_id,
        "hostId": project.host_id,
        "remotePath": project.remote_path,
        "repoRoot": root,
        "currentBranch": branch,
        "defaultBaseBranch": default_base_branch,
        "remotes": remotes,
        "defaultRemote": default_remote,
        "upstreamRefs": refs_from_output(&snapshot.refs_output, &default_remote, &default_base_branch),
        "worktreeBranches": worktree_branches_from_output(&snapshot.worktrees_output),
    }))
}

fn remote_path_join(root: &str, path: &Path) -> String {
    let raw_path = path.to_string_lossy();
    if raw_path.starts_with('/') {
        raw_path.to_string()
    } else {
        let relative = raw_path
            .strip_prefix("./")
            .unwrap_or(raw_path.as_ref())
            .trim_start_matches('/');
        format!("{}/{}", root.trim_end_matches('/'), relative)
    }
}

fn ensure_remote_exists(remotes: &[String], remote: &str) -> UpstreamWorktreeResult<()> {
    if remotes.iter().any(|candidate| candidate == remote) {
        Ok(())
    } else {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::RemoteMissing,
            format!("Remote does not exist: {remote}"),
        ))
    }
}

fn ensure_remote_branch_is_available(
    project: &UpstreamRemoteProject,
    branch_name: &str,
) -> UpstreamWorktreeResult<()> {
    let output = remote_git(
        project,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch_name}"),
        ],
    )?;
    if output.status_success {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::BranchExists,
            format!("Branch already exists: {branch_name}"),
        ))
    } else {
        Ok(())
    }
}

fn fetch_remote_branch(
    project: &UpstreamRemoteProject,
    remote: &str,
    base_branch: &str,
) -> UpstreamWorktreeResult<()> {
    let refspec = format!("+refs/heads/{base_branch}:refs/remotes/{remote}/{base_branch}");
    let output = remote_git(project, &["fetch", remote, &refspec])?;
    if output.status_success {
        Ok(())
    } else {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::FetchFailed,
            if output.stderr.is_empty() {
                format!("Failed to fetch {remote}/{base_branch}")
            } else {
                output.stderr
            },
        ))
    }
}

pub fn prepare_for_remote_project(
    project: &UpstreamRemoteProject,
    request: &UpstreamWorktreeSourceRequest,
) -> UpstreamWorktreeResult<Value> {
    let root = remote_repo_root(project)?;
    let remotes = remote_names(project)?;
    ensure_remote_exists(&remotes, &request.remote)?;
    if request.fetch {
        fetch_remote_branch(project, &request.remote, &request.base_branch)?;
    }
    let display_source_ref = source_ref(&request.remote, &request.base_branch);
    let qualified_source_ref = qualified_remote_ref(&request.remote, &request.base_branch);
    let source_head = ensure_source_ref_exists(project, &qualified_source_ref)?;
    Ok(json!({
        "status": "ok",
        "remoteProject": true,
        "projectId": project.project_id,
        "hostId": project.host_id,
        "repoRoot": root,
        "sourceRef": display_source_ref,
        "qualifiedSourceRef": qualified_source_ref,
        "sourceHead": source_head,
    }))
}

fn ensure_source_ref_exists(
    project: &UpstreamRemoteProject,
    qualified_ref: &str,
) -> UpstreamWorktreeResult<String> {
    let commit_ref = format!("{qualified_ref}^{{commit}}");
    let output = remote_git(project, &["rev-parse", "--verify", &commit_ref])?;
    if output.status_success && !output.stdout.is_empty() {
        Ok(output.stdout)
    } else {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::BaseBranchMissing,
            format!("Base branch does not exist: {qualified_ref}"),
        ))
    }
}

fn add_remote_worktree(
    project: &UpstreamRemoteProject,
    branch_name: &str,
    worktree_path: &str,
    qualified_ref: &str,
) -> UpstreamWorktreeResult<()> {
    let output = remote_git(
        project,
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            worktree_path,
            qualified_ref,
        ],
    )?;
    if output.status_success {
        Ok(())
    } else {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::WorktreeCreateFailed,
            if output.stderr.is_empty() {
                "Failed to create remote worktree".to_string()
            } else {
                output.stderr
            },
        ))
    }
}

pub fn create_for_remote_project(
    project: &UpstreamRemoteProject,
    request: &UpstreamWorktreeRequest,
) -> UpstreamWorktreeResult<Value> {
    let root = remote_repo_root(project)?;
    let remotes = remote_names(project)?;
    ensure_remote_exists(&remotes, &request.remote)?;
    ensure_remote_branch_is_available(project, &request.branch_name)?;
    let worktree_path = remote_path_join(&root, &request.worktree_path);
    if request.fetch {
        fetch_remote_branch(project, &request.remote, &request.base_branch)?;
    }
    let display_source_ref = source_ref(&request.remote, &request.base_branch);
    let qualified_source_ref = qualified_remote_ref(&request.remote, &request.base_branch);
    let source_head = ensure_source_ref_exists(project, &qualified_source_ref)?;
    add_remote_worktree(
        project,
        &request.branch_name,
        &worktree_path,
        &qualified_source_ref,
    )?;
    Ok(json!({
        "status": "ok",
        "remoteProject": true,
        "projectId": project.project_id,
        "hostId": project.host_id,
        "repoRoot": root,
        "worktreePath": worktree_path,
        "branchName": request.branch_name,
        "sourceRef": display_source_ref,
        "sourceHead": source_head,
    }))
}

#[cfg(test)]
mod tests;
