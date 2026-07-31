//! Cursor (`~/.cursor`) workspace mappings.
//!
//! Cursor stores background-agent transcripts under `agents/`, plans under
//! `plans/`, and per-project metadata under `projects/`. `worktrees/` and
//! `ai-tracking/` are opaque supporting data.

use super::{FsAdapter, Mapping};
use crate::config::get_cursor_config_dir;
use crate::workspace_sync::model::{DataKind, MergeCapability, WorkspaceProviderId};

const MAPPINGS: &[Mapping] = &[
    Mapping::dir("agents", DataKind::Session, MergeCapability::AppendOnly),
    Mapping::dir("plans", DataKind::Plan, MergeCapability::Text),
    Mapping::dir("projects", DataKind::Index, MergeCapability::Opaque),
    Mapping::dir("worktrees", DataKind::Attachment, MergeCapability::Opaque),
    Mapping::dir("ai-tracking", DataKind::Index, MergeCapability::Opaque),
];

pub fn adapter() -> FsAdapter {
    FsAdapter::new(
        WorkspaceProviderId::Cursor,
        get_cursor_config_dir(),
        MAPPINGS,
    )
}
