use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::git::{
    failed_response, git_in_repo, git_output, qualified_remote_ref, remote_names, repo_root,
    request_from_payload, source_ref, source_request_from_payload,
};
use super::remote::{create_for_remote_project, prepare_for_remote_project, remote_project_for_id};
use super::types::{UpstreamWorktreeCode, UpstreamWorktreeError, UpstreamWorktreeResult};

fn normalize_worktree_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
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

fn ensure_branch_is_available(repo_root: &Path, branch_name: &str) -> UpstreamWorktreeResult<()> {
    let output = git_in_repo(
        repo_root,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch_name}"),
        ],
    )
    .map_err(|_| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, "Git is not available")
    })?;
    if output.status_success {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::BranchExists,
            format!("Branch already exists: {branch_name}"),
        ))
    } else {
        Ok(())
    }
}

fn ensure_worktree_path_available(path: &Path) -> UpstreamWorktreeResult<()> {
    if path.exists() {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::PathExists,
            format!("Worktree path already exists: {}", path.display()),
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn fetch_remote_branch(
    repo_root: &Path,
    remote: &str,
    base_branch: &str,
) -> UpstreamWorktreeResult<()> {
    let refspec = format!("+refs/heads/{base_branch}:refs/remotes/{remote}/{base_branch}");
    let output = git_in_repo(repo_root, &["fetch", remote, &refspec]).map_err(|_| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, "Git is not available")
    })?;
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

pub(crate) fn ensure_source_ref_exists(
    repo_root: &Path,
    qualified_ref: &str,
) -> UpstreamWorktreeResult<String> {
    let commit_ref = format!("{qualified_ref}^{{commit}}");
    let output = git_in_repo(repo_root, &["rev-parse", "--verify", &commit_ref]).map_err(|_| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, "Git is not available")
    })?;
    if output.status_success && !output.stdout.is_empty() {
        Ok(output.stdout)
    } else {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::BaseBranchMissing,
            format!("Base branch does not exist: {qualified_ref}"),
        ))
    }
}

fn add_worktree(
    repo_root: &Path,
    branch_name: &str,
    worktree_path: &Path,
    qualified_ref: &str,
) -> UpstreamWorktreeResult<()> {
    let args = vec![
        OsString::from("-C"),
        repo_root.as_os_str().to_os_string(),
        OsString::from("worktree"),
        OsString::from("add"),
        OsString::from("-b"),
        OsString::from(branch_name),
        worktree_path.as_os_str().to_os_string(),
        OsString::from(qualified_ref),
    ];
    let output = git_output(args).map_err(|_| {
        UpstreamWorktreeError::new(UpstreamWorktreeCode::GitMissing, "Git is not available")
    })?;
    if output.status_success {
        Ok(())
    } else {
        Err(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::WorktreeCreateFailed,
            if output.stderr.is_empty() {
                "Failed to create worktree".to_string()
            } else {
                output.stderr
            },
        ))
    }
}

pub fn create_response(payload: &Value) -> Value {
    match create_worktree(payload) {
        Ok(value) => value,
        Err(error) => failed_response(error),
    }
}

pub fn prepare_response(payload: &Value) -> Value {
    match prepare_source_ref(payload) {
        Ok(value) => value,
        Err(error) => failed_response(error),
    }
}

fn prepare_source_ref(payload: &Value) -> UpstreamWorktreeResult<Value> {
    let request = source_request_from_payload(payload)?;
    if let Some(remote_project) = remote_project_for_id(&request.project_id) {
        return prepare_for_remote_project(&remote_project, &request);
    }
    let root = repo_root(&request.repo_path)?;
    let remotes = remote_names(&root)?;
    ensure_remote_exists(&remotes, &request.remote)?;
    if request.fetch {
        fetch_remote_branch(&root, &request.remote, &request.base_branch)?;
    }
    let display_source_ref = source_ref(&request.remote, &request.base_branch);
    let qualified_source_ref = qualified_remote_ref(&request.remote, &request.base_branch);
    let source_head = ensure_source_ref_exists(&root, &qualified_source_ref)?;
    Ok(json!({
        "status": "ok",
        "repoRoot": root.to_string_lossy(),
        "sourceRef": display_source_ref,
        "qualifiedSourceRef": qualified_source_ref,
        "sourceHead": source_head,
    }))
}

fn create_worktree(payload: &Value) -> UpstreamWorktreeResult<Value> {
    let request = request_from_payload(payload)?;
    if let Some(remote_project) = remote_project_for_id(&request.project_id) {
        return create_for_remote_project(&remote_project, &request);
    }
    let root = repo_root(&request.repo_path)?;
    let remotes = remote_names(&root)?;
    ensure_remote_exists(&remotes, &request.remote)?;
    ensure_branch_is_available(&root, &request.branch_name)?;
    let worktree_path = normalize_worktree_path(&root, &request.worktree_path);
    ensure_worktree_path_available(&worktree_path)?;
    if request.fetch {
        fetch_remote_branch(&root, &request.remote, &request.base_branch)?;
    }
    let display_source_ref = source_ref(&request.remote, &request.base_branch);
    let qualified_source_ref = qualified_remote_ref(&request.remote, &request.base_branch);
    let source_head = ensure_source_ref_exists(&root, &qualified_source_ref)?;
    add_worktree(
        &root,
        &request.branch_name,
        &worktree_path,
        &qualified_source_ref,
    )?;
    Ok(json!({
        "status": "ok",
        "repoRoot": root.to_string_lossy(),
        "worktreePath": worktree_path.to_string_lossy(),
        "branchName": request.branch_name,
        "sourceRef": display_source_ref,
        "sourceHead": source_head,
    }))
}
