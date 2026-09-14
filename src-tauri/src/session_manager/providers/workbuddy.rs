//! WorkBuddy（腾讯 WorkBuddy 桌面端）会话历史 Provider。
//!
//! 数据源：`~/.workbuddy/projects/**/*.jsonl`（与 CodeBuddy 完全同构的 flat
//! 事件格式），解析逻辑见 `tencent_core`。WorkBuddy 无 CLI，
//! 因此 `resume_command` 恒为 `None`（前端自动禁用恢复按钮）。

use std::path::{Path, PathBuf};

use crate::config::get_workbuddy_config_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::tencent_core::{self, collect_jsonl_files};

const PROVIDER_ID: &str = "workbuddy";

fn projects_root() -> PathBuf {
    get_workbuddy_config_dir().join("projects")
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = projects_root();
    let mut files = Vec::new();
    collect_jsonl_files(&root, &mut files);

    let mut sessions = Vec::new();
    for path in files {
        if let Some(meta) = tencent_core::scan_session(&path, PROVIDER_ID, None) {
            sessions.push(meta);
        }
    }
    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    tencent_core::load_messages(path)
}

pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    tencent_core::delete_session(path, session_id)
}

pub fn session_roots() -> Vec<PathBuf> {
    vec![projects_root()]
}
