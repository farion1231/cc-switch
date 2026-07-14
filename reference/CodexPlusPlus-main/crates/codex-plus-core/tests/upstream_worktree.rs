use codex_plus_core::upstream_worktree::{
    UpstreamWorktreeCode, default_remote_name, source_ref, validate_branch_name,
};

#[test]
fn branch_validation_accepts_normal_branch_names() {
    validate_branch_name("feature/upstream-worktree").expect("branch should be valid");
    validate_branch_name("carson/test-123").expect("branch should be valid");
}

#[test]
fn branch_validation_rejects_invalid_branch_names() {
    let error = validate_branch_name("bad branch").expect_err("spaces are invalid");
    assert_eq!(error.code, UpstreamWorktreeCode::BranchInvalid);

    let error = validate_branch_name("-bad").expect_err("dash prefix is invalid");
    assert_eq!(error.code, UpstreamWorktreeCode::BranchInvalid);
}

#[test]
fn source_ref_joins_remote_and_base_branch() {
    assert_eq!(source_ref("upstream", "main"), "upstream/main");
    assert_eq!(source_ref("origin", "feature/x"), "origin/feature/x");
}

#[test]
fn default_remote_prefers_upstream_then_origin_then_first_remote() {
    assert_eq!(
        default_remote_name(&["origin".into(), "upstream".into()]),
        "upstream"
    );
    assert_eq!(default_remote_name(&["origin".into()]), "origin");
    assert_eq!(default_remote_name(&["mirror".into()]), "mirror");
    assert_eq!(default_remote_name(&[]), "upstream");
}

use std::path::Path;
use std::process::Command;

use serde_json::json;

use codex_plus_core::upstream_worktree::{
    create_response, defaults_response, prepare_response, remote_project_from_state,
    status_response,
};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_no_repo(args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_file(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("file should have parent")).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit_file(repo: &Path, name: &str, text: &str, message: &str) -> String {
    write_file(&repo.join(name), text);
    git(repo, &["add", name]);
    git(repo, &["commit", "-m", message]);
    git(repo, &["rev-parse", "HEAD"])
}

fn prepare_remote_repo(temp: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let remote = temp.join("remote.git");
    let seed = temp.join("seed");
    git_no_repo(&["init", "--bare", remote.to_str().unwrap()]);
    git_no_repo(&["init", seed.to_str().unwrap()]);
    git(&seed, &["config", "user.email", "test@example.com"]);
    git(&seed, &["config", "user.name", "Test User"]);
    commit_file(&seed, "README.md", "v1\n", "initial");
    git(&seed, &["branch", "-M", "main"]);
    git(
        &seed,
        &["remote", "add", "upstream", remote.to_str().unwrap()],
    );
    git(&seed, &["push", "-u", "upstream", "main"]);
    git_no_repo(&[
        "--git-dir",
        remote.to_str().unwrap(),
        "symbolic-ref",
        "HEAD",
        "refs/heads/main",
    ]);
    (remote, seed)
}

#[test]
fn status_response_reports_git_available() {
    let result = status_response();

    assert_eq!(result["status"], "ok");
    assert_eq!(result["feature"], "upstream-worktree");
    assert_eq!(result["gitAvailable"], true);
}

#[test]
fn defaults_response_detects_repo_branch_and_upstream_remote() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    git_no_repo(&["init", repo.to_str().unwrap()]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    commit_file(&repo, "README.md", "v1\n", "initial");
    git(&repo, &["checkout", "-b", "feature/local"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/origin.git",
        ],
    );
    git(
        &repo,
        &[
            "remote",
            "add",
            "upstream",
            "https://example.invalid/upstream.git",
        ],
    );

    let result = defaults_response(&json!({"repoPath": repo}));

    assert_eq!(result["status"], "ok");
    assert_eq!(result["currentBranch"], "feature/local");
    assert_eq!(result["defaultBaseBranch"], "feature/local");
    assert_eq!(result["defaultRemote"], "upstream");
    assert_eq!(result["remotes"].as_array().unwrap().len(), 2);
}

#[test]
fn defaults_response_lists_preferred_upstream_ref_for_current_branch() {
    let temp = tempfile::tempdir().unwrap();
    let remote = temp.path().join("remote.git");
    git_no_repo(&["init", "--bare", remote.to_str().unwrap()]);
    let repo = temp.path().join("repo");
    git_no_repo(&["clone", remote.to_str().unwrap(), repo.to_str().unwrap()]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    commit_file(&repo, "README.md", "hello\n", "initial");
    git(&repo, &["branch", "-M", "main"]);
    git(&repo, &["push", "origin", "main"]);
    git(&repo, &["remote", "rename", "origin", "upstream"]);
    git(&repo, &["fetch", "upstream", "main"]);

    let result = defaults_response(&json!({ "repoPath": repo }));

    assert_eq!(result["status"], "ok");
    assert_eq!(result["defaultRemote"], "upstream");
    assert_eq!(result["upstreamRefs"][0]["remote"], "upstream");
    assert_eq!(result["upstreamRefs"][0]["branch"], "main");
    assert_eq!(result["upstreamRefs"][0]["label"], "upstream/main");
    assert_eq!(
        result["upstreamRefs"][0]["sourceRef"],
        "refs/remotes/upstream/main"
    );
}

#[test]
fn defaults_response_lists_actual_upstream_refs_instead_of_local_current_branch() {
    let temp = tempfile::tempdir().unwrap();
    let (remote, seed) = prepare_remote_repo(temp.path());
    git(&seed, &["checkout", "-b", "release/alpha"]);
    commit_file(&seed, "release.txt", "alpha\n", "release branch");
    git(&seed, &["push", "upstream", "release/alpha"]);
    let repo = temp.path().join("repo");
    git_no_repo(&["clone", remote.to_str().unwrap(), repo.to_str().unwrap()]);
    git(&repo, &["remote", "rename", "origin", "upstream"]);
    git(&repo, &["fetch", "upstream"]);
    git(&repo, &["checkout", "-b", "codex-base"]);

    let result = defaults_response(&json!({ "repoPath": repo }));
    let labels = result["upstreamRefs"]
        .as_array()
        .expect("upstreamRefs should be an array")
        .iter()
        .filter_map(|entry| entry["label"].as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"upstream/main"));
    assert!(labels.contains(&"upstream/release/alpha"));
    assert!(!labels.contains(&"upstream/codex-base"));
    assert_eq!(
        result["upstreamRefs"][0]["sourceRef"],
        "refs/remotes/upstream/main"
    );
}

#[test]
fn remote_project_from_state_resolves_uuid_to_remote_workspace() {
    let state = json!({
        "remote-projects": [{
            "id": "032e652b-7956-4e6e-83bd-b29f456c6c3d",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
            "label": "sealos-skills"
        }]
    });

    let project =
        remote_project_from_state(&state, "032e652b-7956-4e6e-83bd-b29f456c6c3d").unwrap();

    assert_eq!(project.host_id, "remote-ssh-codex-managed:remote");
    assert_eq!(project.remote_path, "/Users/longnv/bin/repo/sealos-skills");
    assert_eq!(project.label, "sealos-skills");
}

#[test]
fn defaults_response_lists_branches_checked_out_by_worktrees() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    git_no_repo(&["init", repo.to_str().unwrap()]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    commit_file(&repo, "README.md", "v1\n", "initial");
    let linked_worktree = temp.path().join("linked worktree");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "docs/codex-plugin-integration",
            linked_worktree.to_str().unwrap(),
            "HEAD",
        ],
    );

    let result = defaults_response(&json!({ "repoPath": repo }));
    let worktree_branches = result["worktreeBranches"]
        .as_array()
        .expect("worktreeBranches should be an array");

    let linked_worktree_path = std::fs::canonicalize(&linked_worktree)
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(worktree_branches.iter().any(|entry| {
        entry["branch"] == "docs/codex-plugin-integration" && entry["path"] == linked_worktree_path
    }));
}

#[test]
fn create_response_creates_new_worktree_from_fetched_upstream_ref() {
    let temp = tempfile::tempdir().unwrap();
    let (remote, seed) = prepare_remote_repo(temp.path());
    let repo = temp.path().join("repo");
    git_no_repo(&["clone", remote.to_str().unwrap(), repo.to_str().unwrap()]);
    git(&repo, &["remote", "rename", "origin", "upstream"]);
    git(&repo, &["checkout", "-b", "local-stale"]);
    let remote_head = commit_file(&seed, "README.md", "v2\n", "remote update");
    git(&seed, &["push", "upstream", "main"]);
    let worktree_path = temp.path().join("created worktree");

    let result = create_response(&json!({
        "repoPath": repo,
        "branchName": "feature/from-upstream",
        "worktreePath": worktree_path,
        "remote": "upstream",
        "baseBranch": "main",
        "fetch": true
    }));

    assert_eq!(result["status"], "ok");
    assert_eq!(result["sourceRef"], "upstream/main");
    let created_head = git(
        Path::new(result["worktreePath"].as_str().unwrap()),
        &["rev-parse", "HEAD"],
    );
    assert_eq!(created_head, remote_head);
}

#[test]
fn prepare_response_fetches_and_returns_qualified_upstream_ref() {
    let temp = tempfile::tempdir().unwrap();
    let (remote, seed) = prepare_remote_repo(temp.path());
    let repo = temp.path().join("repo");
    git_no_repo(&["clone", remote.to_str().unwrap(), repo.to_str().unwrap()]);
    git(&repo, &["remote", "rename", "origin", "upstream"]);
    let remote_head = commit_file(&seed, "README.md", "v2\n", "remote update");
    git(&seed, &["push", "upstream", "main"]);

    let result = prepare_response(&json!({
        "repoPath": repo,
        "remote": "upstream",
        "baseBranch": "main",
        "fetch": true
    }));

    assert_eq!(result["status"], "ok");
    assert_eq!(result["sourceRef"], "upstream/main");
    assert_eq!(result["qualifiedSourceRef"], "refs/remotes/upstream/main");
    assert_eq!(result["sourceHead"], remote_head);
}

#[test]
fn create_response_uses_remote_tracking_ref_when_local_branch_shadows_display_ref() {
    let temp = tempfile::tempdir().unwrap();
    let (remote, seed) = prepare_remote_repo(temp.path());
    let repo = temp.path().join("repo");
    git_no_repo(&["clone", remote.to_str().unwrap(), repo.to_str().unwrap()]);
    git(&repo, &["remote", "rename", "origin", "upstream"]);
    let stale_head = git(&repo, &["rev-parse", "HEAD"]);
    let remote_head = commit_file(&seed, "README.md", "v2\n", "remote update");
    git(&seed, &["push", "upstream", "main"]);
    git(&repo, &["fetch", "upstream", "main"]);
    git(&repo, &["branch", "upstream/main", &stale_head]);
    let tracking_head = git(&repo, &["rev-parse", "refs/remotes/upstream/main"]);
    assert_eq!(tracking_head, remote_head);
    assert_ne!(stale_head, tracking_head);
    let worktree_path = temp.path().join("created from tracking ref");

    let result = create_response(&json!({
        "repoPath": repo,
        "branchName": "feature/from-qualified-upstream",
        "worktreePath": worktree_path,
        "remote": "upstream",
        "baseBranch": "main",
        "fetch": false
    }));

    assert_eq!(result["status"], "ok");
    assert_eq!(result["sourceRef"], "upstream/main");
    assert_eq!(result["sourceHead"], tracking_head);
    let created_head = git(
        Path::new(result["worktreePath"].as_str().unwrap()),
        &["rev-parse", "HEAD"],
    );
    assert_eq!(created_head, tracking_head);
}

#[test]
fn create_response_does_not_create_worktree_when_fetch_fails() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    git_no_repo(&["init", repo.to_str().unwrap()]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    commit_file(&repo, "README.md", "v1\n", "initial");
    git(
        &repo,
        &[
            "remote",
            "add",
            "upstream",
            temp.path().join("missing.git").to_str().unwrap(),
        ],
    );
    let worktree_path = temp.path().join("should-not-exist");

    let result = create_response(&json!({
        "repoPath": repo,
        "branchName": "feature/no-fetch",
        "worktreePath": worktree_path,
        "remote": "upstream",
        "baseBranch": "main",
        "fetch": true
    }));

    assert_eq!(result["status"], "failed");
    assert_eq!(result["code"], "fetch-failed");
    assert!(!temp.path().join("should-not-exist").exists());
}
