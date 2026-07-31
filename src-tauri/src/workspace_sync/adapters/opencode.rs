//! OpenCode (`$XDG_DATA_HOME/opencode`, default `~/.local/share/opencode`)
//! workspace mappings.
//!
//! The base directory is resolved via the existing session-manager helper so we
//! honour `XDG_DATA_HOME`. `storage/` holds session/message JSON; `snapshot/`
//! holds file snapshots (treated as attachments).

use super::{FsAdapter, Mapping};
use crate::session_manager::providers::opencode::get_opencode_base_dir;
use crate::workspace_sync::model::{DataKind, MergeCapability, WorkspaceProviderId};

const MAPPINGS: &[Mapping] = &[
    Mapping::dir("storage", DataKind::Session, MergeCapability::AppendOnly),
    Mapping::dir("snapshot", DataKind::Attachment, MergeCapability::Opaque),
];

pub fn adapter() -> FsAdapter {
    FsAdapter::new(
        WorkspaceProviderId::OpenCode,
        get_opencode_base_dir(),
        MAPPINGS,
    )
}
