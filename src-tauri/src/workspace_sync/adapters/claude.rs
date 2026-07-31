//! Claude Code (`~/.claude`) workspace mappings.

use super::{FsAdapter, Mapping};
use crate::config::get_claude_config_dir;
use crate::workspace_sync::model::{DataKind, MergeCapability, WorkspaceProviderId};

/// `projects/**/*.jsonl` are append-only session transcripts; `plans/`,
/// `tasks/`, `todos/` are per-item files; `history.jsonl` is an index.
const MAPPINGS: &[Mapping] = &[
    Mapping::dir_ext(
        "projects",
        DataKind::Session,
        MergeCapability::AppendOnly,
        &["jsonl"],
    ),
    Mapping::dir("plans", DataKind::Plan, MergeCapability::Text),
    Mapping::dir("tasks", DataKind::Task, MergeCapability::Text),
    Mapping::dir("todos", DataKind::Todo, MergeCapability::AppendOnly),
    Mapping::file("history.jsonl", DataKind::Index, MergeCapability::Opaque),
];

pub fn adapter() -> FsAdapter {
    FsAdapter::new(
        WorkspaceProviderId::Claude,
        get_claude_config_dir(),
        MAPPINGS,
    )
}
