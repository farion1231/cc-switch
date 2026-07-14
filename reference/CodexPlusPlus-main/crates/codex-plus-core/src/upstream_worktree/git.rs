use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use super::types::{
    GitOutput, UpstreamWorktreeCode, UpstreamWorktreeError, UpstreamWorktreeRequest,
    UpstreamWorktreeResult, UpstreamWorktreeSourceRequest,
};

pub fn validate_branch_name(branch: &str) -> UpstreamWorktreeResult<()> {
    let branch = branch.trim();
    if branch.is_empty() || branch.starts_with('-') || branch.contains('\\') {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::BranchInvalid,
            format!("Invalid branch name: {branch}"),
        ));
    }

    let output = Command::new("git")
        .args(["check-ref-format", "--branch", branch])
        .output()
        .map_err(|_| {
            UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, "Git is not available")
        })?;

    if output.status.success() {
        Ok(())
    } else {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::BranchInvalid,
            format!("Invalid branch name: {branch}"),
        ))
    }
}

pub fn validate_base_branch(base_branch: &str) -> UpstreamWorktreeResult<()> {
    validate_branch_name(base_branch).map_err(|error| {
        if error.code == UpstreamWorktreeCode::GitMissing {
            return error;
        }
        UpstreamWorktreeError::new(
            UpstreamWorktreeCode::BaseBranchMissing,
            format!("Invalid base branch: {base_branch}"),
        )
    })
}

pub fn source_ref(remote: &str, base_branch: &str) -> String {
    format!("{}/{}", remote.trim(), base_branch.trim())
}

pub(crate) fn qualified_remote_ref(remote: &str, base_branch: &str) -> String {
    format!("refs/remotes/{}/{}", remote.trim(), base_branch.trim())
}

pub(crate) fn string_field(payload: &Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub(crate) fn request_from_payload(
    payload: &Value,
) -> UpstreamWorktreeResult<UpstreamWorktreeRequest> {
    let repo_path = string_field(payload, "repoPath");
    let project_id = string_field(payload, "projectId");
    let branch_name = string_field(payload, "branchName");
    let worktree_path = string_field(payload, "worktreePath");
    let remote = string_field(payload, "remote");
    let base_branch = string_field(payload, "baseBranch");
    let fetch = payload
        .get("fetch")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    if repo_path.is_empty() && project_id.is_empty() {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::NotGitRepo,
            "Repository path is required",
        ));
    }
    if worktree_path.is_empty() {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::PathExists,
            "Worktree path is required",
        ));
    }

    validate_branch_name(&branch_name)?;
    validate_base_branch(&base_branch)?;

    if remote.is_empty() || remote.starts_with('-') || remote.contains('/') || remote.contains('\\')
    {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::RemoteMissing,
            "Remote is required",
        ));
    }

    Ok(UpstreamWorktreeRequest {
        repo_path: PathBuf::from(repo_path),
        project_id,
        branch_name,
        worktree_path: PathBuf::from(worktree_path),
        remote,
        base_branch,
        fetch,
    })
}

pub(crate) fn source_request_from_payload(
    payload: &Value,
) -> UpstreamWorktreeResult<UpstreamWorktreeSourceRequest> {
    let repo_path = string_field(payload, "repoPath");
    let project_id = string_field(payload, "projectId");
    let remote = string_field(payload, "remote");
    let base_branch = string_field(payload, "baseBranch");
    let fetch = payload
        .get("fetch")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    if repo_path.is_empty() && project_id.is_empty() {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::NotGitRepo,
            "Repository path is required",
        ));
    }

    validate_base_branch(&base_branch)?;

    if remote.is_empty() || remote.starts_with('-') || remote.contains('/') || remote.contains('\\')
    {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::RemoteMissing,
            "Remote is required",
        ));
    }

    Ok(UpstreamWorktreeSourceRequest {
        repo_path: PathBuf::from(repo_path),
        project_id,
        remote,
        base_branch,
        fetch,
    })
}

pub(crate) fn git_output(args: Vec<OsString>) -> Result<GitOutput, std::io::Error> {
    let output = Command::new("git").args(args).output()?;
    Ok(GitOutput {
        status_success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

pub(crate) fn git_in_repo(repo: &Path, args: &[&str]) -> Result<GitOutput, std::io::Error> {
    let mut command_args = vec![OsString::from("-C"), repo.as_os_str().to_os_string()];
    command_args.extend(args.iter().map(OsString::from));
    git_output(command_args)
}

pub(crate) fn failed_response(error: UpstreamWorktreeError) -> Value {
    error.to_value()
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn status_response() -> Value {
    let git_available = git_available();
    json!({
        "status": if git_available { "ok" } else { "failed" },
        "feature": "upstream-worktree",
        "gitAvailable": git_available,
        "platformSupported": true,
    })
}

pub(crate) fn repo_root(repo_path: &Path) -> UpstreamWorktreeResult<PathBuf> {
    let output = git_in_repo(repo_path, &["rev-parse", "--show-toplevel"]).map_err(|_| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, "Git is not available")
    })?;
    if !output.status_success || output.stdout.is_empty() {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::NotGitRepo,
            "Path is not inside a Git repository",
        ));
    }
    Ok(PathBuf::from(output.stdout))
}

pub(crate) fn current_branch(repo_root: &Path) -> String {
    git_in_repo(repo_root, &["branch", "--show-current"])
        .ok()
        .filter(|output| output.status_success)
        .map(|output| output.stdout)
        .filter(|branch| !branch.is_empty())
        .unwrap_or_default()
}

pub(crate) fn remote_names(repo_root: &Path) -> UpstreamWorktreeResult<Vec<String>> {
    let output = git_in_repo(repo_root, &["remote"]).map_err(|_| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, "Git is not available")
    })?;
    if !output.status_success {
        return Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::RemoteMissing,
            "Cannot read Git remotes",
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
