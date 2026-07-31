//! Grok Build (`~/.grok`) workspace mappings.

use super::{FsAdapter, Mapping};
use crate::grok_config::get_grok_config_dir;
use crate::workspace_sync::model::{DataKind, MergeCapability, WorkspaceProviderId};

const MAPPINGS: &[Mapping] = &[
    Mapping::dir("sessions", DataKind::Session, MergeCapability::AppendOnly),
    Mapping::dir(
        "archived_sessions",
        DataKind::Session,
        MergeCapability::AppendOnly,
    ),
];

pub fn adapter() -> FsAdapter {
    FsAdapter::new(
        WorkspaceProviderId::GrokBuild,
        get_grok_config_dir(),
        MAPPINGS,
    )
}
