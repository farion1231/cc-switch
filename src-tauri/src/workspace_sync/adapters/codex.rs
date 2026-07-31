//! Codex (`~/.codex`) workspace mappings.
//!
//! `sessions/` + `archived_sessions/` are append-only transcripts and
//! `memories/` are text notes. The sqlite databases (`goals_*`, `state_*`,
//! `memories_*`) are treated as opaque whole-file artifacts — no row-level
//! merge — because they carry WAL/SHM sidecars and a naive merge would corrupt
//! them. They are excluded from the MVP scan (see notes) to avoid syncing a
//! half-written DB; whole-file DB sync can be layered on later with a
//! checkpoint step.

use super::{FsAdapter, Mapping};
use crate::codex_config::get_codex_config_dir;
use crate::workspace_sync::model::{DataKind, MergeCapability, WorkspaceProviderId};

const MAPPINGS: &[Mapping] = &[
    Mapping::dir("sessions", DataKind::Session, MergeCapability::AppendOnly),
    Mapping::dir(
        "archived_sessions",
        DataKind::Session,
        MergeCapability::AppendOnly,
    ),
    Mapping::dir("memories", DataKind::Memory, MergeCapability::Text),
    Mapping::file("history.jsonl", DataKind::Index, MergeCapability::Opaque),
    Mapping::file(
        "session_index.jsonl",
        DataKind::Index,
        MergeCapability::Opaque,
    ),
    Mapping::file("AGENTS.md", DataKind::Memory, MergeCapability::Text),
];

pub fn adapter() -> FsAdapter {
    FsAdapter::new(WorkspaceProviderId::Codex, get_codex_config_dir(), MAPPINGS)
}
