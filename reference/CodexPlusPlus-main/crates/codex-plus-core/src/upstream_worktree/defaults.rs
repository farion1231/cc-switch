use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::git::{
    current_branch, failed_response, git_in_repo, remote_names, repo_root, string_field,
};
use super::remote::{defaults_for_remote_project, remote_project_for_id};
use super::types::{UpstreamWorktreeCode, UpstreamWorktreeError, UpstreamWorktreeResult};

pub fn default_remote_name(remotes: &[String]) -> String {
    if remotes.iter().any(|remote| remote == "upstream") {
        "upstream".to_string()
    } else if remotes.iter().any(|remote| remote == "origin") {
        "origin".to_string()
    } else {
        remotes
            .first()
            .cloned()
            .unwrap_or_else(|| "upstream".to_string())
    }
}

pub(crate) fn fallback_upstream_refs(remote: &str, base_branch: &str) -> Vec<Value> {
    let remote = remote.trim();
    let base_branch = base_branch.trim();
    if remote.is_empty() || base_branch.is_empty() {
        return Vec::new();
    }
    vec![json!({
        "remote": remote,
        "branch": base_branch,
        "label": format!("{remote}/{base_branch}"),
        "sourceRef": format!("refs/remotes/{remote}/{base_branch}"),
    })]
}

pub(crate) fn refs_from_output(output: &str, remote: &str, fallback_branch: &str) -> Vec<Value> {
    let remote = remote.trim();
    if remote.is_empty() {
        return Vec::new();
    }
    let prefix = format!("refs/remotes/{remote}/");
    let mut refs = output
        .lines()
        .map(str::trim)
        .filter(|ref_name| ref_name.starts_with(&prefix))
        .filter_map(|ref_name| ref_name.strip_prefix(&prefix))
        .filter(|branch| !branch.is_empty() && *branch != "HEAD")
        .map(|branch| {
            json!({
                "remote": remote,
                "branch": branch,
                "label": format!("{remote}/{branch}"),
                "sourceRef": format!("refs/remotes/{remote}/{branch}"),
            })
        })
        .collect::<Vec<_>>();
    refs.sort_by(|left, right| {
        left["label"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["label"].as_str().unwrap_or_default())
    });
    if refs.is_empty() {
        fallback_upstream_refs(remote, fallback_branch)
    } else {
        refs
    }
}

fn upstream_refs(repo_root: &Path, remote: &str, fallback_branch: &str) -> Vec<Value> {
    let output = git_in_repo(
        repo_root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            &format!("refs/remotes/{remote}"),
        ],
    );
    output
        .ok()
        .filter(|output| output.status_success)
        .map(|output| refs_from_output(&output.stdout, remote, fallback_branch))
        .unwrap_or_else(|| refs_from_output("", remote, fallback_branch))
}

pub(crate) fn worktree_branches_from_output(output: &str) -> Vec<Value> {
    let mut branches = Vec::new();
    let mut worktree_path = String::new();
    let mut branch_name = String::new();

    for line in output.lines().chain(std::iter::once("")) {
        let line = line.trim();
        if line.is_empty() {
            if !worktree_path.is_empty() && !branch_name.is_empty() {
                let path = PathBuf::from(&worktree_path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&worktree_path));
                branches.push(json!({
                    "path": path.to_string_lossy(),
                    "branch": branch_name,
                }));
            }
            worktree_path.clear();
            branch_name.clear();
            continue;
        }

        if let Some(path) = line.strip_prefix("worktree ") {
            worktree_path = path.to_string();
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            branch_name = branch.to_string();
        }
    }

    branches
}

fn worktree_branches(repo_root: &Path) -> Vec<Value> {
    git_in_repo(repo_root, &["worktree", "list", "--porcelain"])
        .ok()
        .filter(|output| output.status_success)
        .map(|output| worktree_branches_from_output(&output.stdout))
        .unwrap_or_default()
}

pub fn defaults_response(payload: &Value) -> Value {
    let project_id = string_field(payload, "projectId");
    if let Some(remote_project) = remote_project_for_id(&project_id) {
        return match defaults_for_remote_project(&remote_project) {
            Ok(value) => value,
            Err(error) => failed_response(error),
        };
    }
    let repo_path = string_field(payload, "repoPath");
    if repo_path.is_empty() {
        return failed_response(UpstreamWorktreeError::new(
            UpstreamWorktreeCode::NotGitRepo,
            "Repository path is required",
        ));
    }
    match defaults_for_repo(Path::new(&repo_path)) {
        Ok(value) => value,
        Err(error) => failed_response(error),
    }
}

fn defaults_for_repo(repo_path: &Path) -> UpstreamWorktreeResult<Value> {
    let root = repo_root(repo_path)?;
    let branch = current_branch(&root);
    let remotes = remote_names(&root)?;
    let default_base_branch = if branch.is_empty() {
        "main".to_string()
    } else {
        branch.clone()
    };
    let default_remote = default_remote_name(&remotes);
    Ok(json!({
        "status": "ok",
        "repoRoot": root.to_string_lossy(),
        "currentBranch": branch,
        "defaultBaseBranch": default_base_branch,
        "remotes": remotes,
        "defaultRemote": default_remote,
        "upstreamRefs": upstream_refs(&root, &default_remote, &default_base_branch),
        "worktreeBranches": worktree_branches(&root),
    }))
}
