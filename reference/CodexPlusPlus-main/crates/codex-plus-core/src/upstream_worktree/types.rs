use std::path::PathBuf;

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamWorktreeCode {
    GitMissing,
    NotGitRepo,
    RemoteMissing,
    BaseBranchMissing,
    FetchFailed,
    BranchInvalid,
    BranchExists,
    PathExists,
    WorktreeCreateFailed,
    AmbiguousNativeFlow,
}

impl UpstreamWorktreeCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitMissing => "git-missing",
            Self::NotGitRepo => "not-git-repo",
            Self::RemoteMissing => "remote-missing",
            Self::BaseBranchMissing => "base-branch-missing",
            Self::FetchFailed => "fetch-failed",
            Self::BranchInvalid => "branch-invalid",
            Self::BranchExists => "branch-exists",
            Self::PathExists => "path-exists",
            Self::WorktreeCreateFailed => "worktree-create-failed",
            Self::AmbiguousNativeFlow => "ambiguous-native-flow",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamWorktreeError {
    pub code: UpstreamWorktreeCode,
    pub message: String,
}

impl UpstreamWorktreeError {
    pub fn new(code: UpstreamWorktreeCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({
            "status": "failed",
            "code": self.code.as_str(),
            "message": self.message,
        })
    }
}

pub type UpstreamWorktreeResult<T> = Result<T, UpstreamWorktreeError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamWorktreeRequest {
    pub repo_path: PathBuf,
    pub project_id: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub remote: String,
    pub base_branch: String,
    pub fetch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamWorktreeSourceRequest {
    pub repo_path: PathBuf,
    pub project_id: String,
    pub remote: String,
    pub base_branch: String,
    pub fetch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamRemoteProject {
    pub project_id: String,
    pub host_id: String,
    pub remote_path: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteGitCommand {
    pub destination: String,
    pub port: Option<u16>,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOutput {
    pub status_success: bool,
    pub stdout: String,
    pub stderr: String,
}
