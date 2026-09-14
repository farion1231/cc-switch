//! CodeBuddy（腾讯 CodeBuddy Code CLI）会话历史 Provider。
//!
//! 数据源：`~/.codebuddy/projects/**/*.jsonl`（flat 事件格式），
//! 解析逻辑见 `tencent_core`，本模块只做身份与根目录的绑定。

use std::path::{Path, PathBuf};

use crate::config::get_codebuddy_config_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::tencent_core::{self, collect_jsonl_files};

const PROVIDER_ID: &str = "codebuddy";

/// 恢复命令前缀（CodeBuddy CLI 存在，支持 `codebuddy --resume <id>`）。
const RESUME_PREFIX: &str = "codebuddy --resume";

fn projects_root() -> PathBuf {
    get_codebuddy_config_dir().join("projects")
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = projects_root();
    let mut files = Vec::new();
    collect_jsonl_files(&root, &mut files);

    let mut sessions = Vec::new();
    for path in files {
        if let Some(meta) = tencent_core::scan_session(&path, PROVIDER_ID, Some(RESUME_PREFIX)) {
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
