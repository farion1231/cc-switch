//! Two-way merge of local and remote workspace snapshots.
//!
//! Policy (per project decision): **union by `logical_id`, keep-both on
//! conflict, never delete.** For each item keyed by `(kind, logical_id)`:
//!
//! - present on one side only → keep it (materialize if it came from remote);
//! - identical `content_hash` → no-op;
//! - differing content, by [`MergeCapability`]:
//!   - `AppendOnly` (sessions/todos): newer `updated_at` wins and is written
//!     locally; the losing side is recorded as a conflict for visibility.
//!   - `Text` (memory/plans): **keep both** — local file is left untouched and
//!     the remote version is written to a sibling `*.conflict-<device>.<ext>`.
//!   - `Opaque`/`RecordSet`/`Unsupported`: newer wins, and the losing local
//!     file is copied to a `*.conflict-<device>` path before being overwritten.
//!
//! Deletes are **not** propagated in this MVP (conservative: "keep rather than
//! risk deleting"). Tombstone-driven deletion can be layered on once the local
//! base snapshot is persisted in `state_db`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::workspace_sync::model::{DataItem, DataKind, MergeCapability, WorkspaceProviderId};

/// How to bring one item to its final local state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterializeAction {
    /// Fetch `content_hash` from the blob store and write it to `target_path`.
    FromRemoteBlob {
        content_hash: String,
        target_path: String,
    },
    /// Copy an existing local file to a conflict path (to preserve a losing side).
    CopyLocalFile { from_path: String, to_path: String },
}

/// A recorded conflict, surfaced to the UI so the user can reconcile manually.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRecord {
    pub provider: WorkspaceProviderId,
    pub kind: DataKind,
    pub logical_id: String,
    /// Human-readable resolution note (which side won / where the other went).
    pub resolution: String,
    /// The conflict-file path, when a keep-both copy was produced.
    pub conflict_path: Option<String>,
}

/// Result of merging one provider's items.
#[derive(Debug, Clone, Default)]
pub struct MergeOutcome {
    /// Filesystem actions the engine must perform to converge local state.
    pub actions: Vec<MaterializeAction>,
    /// Conflicts to record/report.
    pub conflicts: Vec<ConflictRecord>,
    /// The merged item set for the new snapshot (union, incl. conflict copies).
    pub merged_items: Vec<DataItem>,
}

fn key(item: &DataItem) -> (u8, String) {
    (kind_rank(item.kind), item.logical_id.clone())
}

fn kind_rank(kind: DataKind) -> u8 {
    match kind {
        DataKind::Session => 0,
        DataKind::Task => 1,
        DataKind::Todo => 2,
        DataKind::Plan => 3,
        DataKind::Goal => 4,
        DataKind::Memory => 5,
        DataKind::Index => 6,
        DataKind::Attachment => 7,
    }
}

/// Build a sibling conflict path: `dir/stem.conflict-<tag>.<ext>`
/// (or `dir/name.conflict-<tag>` when there is no extension).
pub fn conflict_path(native_path: &str, tag: &str) -> String {
    let path = Path::new(native_path);
    let parent = path.parent().and_then(|p| p.to_str()).unwrap_or("");
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(native_path);

    let sanitized_tag: String = tag
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    let new_name = match file_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            format!("{stem}.conflict-{sanitized_tag}.{ext}")
        }
        _ => format!("{file_name}.conflict-{sanitized_tag}"),
    };

    if parent.is_empty() {
        new_name
    } else {
        format!("{parent}/{new_name}")
    }
}

/// A conflict item copies the winning DataItem's shape but takes the conflict
/// path as its logical_id/native_path so it is a distinct entry in the snapshot.
fn conflict_item(base: &DataItem, path: String) -> DataItem {
    DataItem {
        provider: base.provider,
        kind: base.kind,
        logical_id: path.clone(),
        parent_id: base.parent_id.clone(),
        native_path: path,
        content_hash: base.content_hash.clone(),
        updated_at: base.updated_at,
        schema_fingerprint: base.schema_fingerprint.clone(),
        merge_capability: base.merge_capability,
        sensitivity: base.sensitivity,
        object_ids: base.object_ids.clone(),
    }
}

/// Merge one provider's `local` and `remote` item lists.
///
/// `remote_device` labels remote-origin conflict files, `local_device` labels
/// preserved local-origin ones.
pub fn merge_provider(
    local: &[DataItem],
    remote: &[DataItem],
    remote_device: &str,
    local_device: &str,
) -> MergeOutcome {
    let mut outcome = MergeOutcome::default();

    let local_map: BTreeMap<(u8, String), &DataItem> =
        local.iter().map(|i| (key(i), i)).collect();
    let remote_map: BTreeMap<(u8, String), &DataItem> =
        remote.iter().map(|i| (key(i), i)).collect();

    // Union of keys, deterministic order.
    let mut all_keys: Vec<(u8, String)> =
        local_map.keys().chain(remote_map.keys()).cloned().collect();
    all_keys.sort();
    all_keys.dedup();

    for k in all_keys {
        match (local_map.get(&k), remote_map.get(&k)) {
            (Some(l), None) => {
                // Local only: already on disk, keep as-is.
                outcome.merged_items.push((*l).clone());
            }
            (None, Some(r)) => {
                // Remote only: pull it down.
                outcome.actions.push(MaterializeAction::FromRemoteBlob {
                    content_hash: r.content_hash.clone(),
                    target_path: r.native_path.clone(),
                });
                outcome.merged_items.push((*r).clone());
            }
            (Some(l), Some(r)) => {
                if l.content_hash == r.content_hash {
                    // Identical: nothing to do.
                    outcome.merged_items.push((*l).clone());
                } else {
                    merge_conflicting_pair(l, r, remote_device, local_device, &mut outcome);
                }
            }
            (None, None) => unreachable!("key came from one of the maps"),
        }
    }

    outcome
}

fn merge_conflicting_pair(
    local: &DataItem,
    remote: &DataItem,
    remote_device: &str,
    local_device: &str,
    outcome: &mut MergeOutcome,
) {
    let remote_newer = remote.updated_at.unwrap_or(0) > local.updated_at.unwrap_or(0);

    match local.merge_capability {
        MergeCapability::Text => {
            // Keep both: local untouched, remote written to a conflict sibling.
            let cpath = conflict_path(&remote.native_path, remote_device);
            outcome.actions.push(MaterializeAction::FromRemoteBlob {
                content_hash: remote.content_hash.clone(),
                target_path: cpath.clone(),
            });
            outcome.merged_items.push(local.clone());
            outcome.merged_items.push(conflict_item(remote, cpath.clone()));
            outcome.conflicts.push(ConflictRecord {
                provider: local.provider,
                kind: local.kind,
                logical_id: local.logical_id.clone(),
                resolution: format!(
                    "text conflict: kept local, saved remote copy to {cpath}"
                ),
                conflict_path: Some(cpath),
            });
        }
        MergeCapability::AppendOnly => {
            // Newer wins; record the losing side (no destructive copy — the
            // append-only newer file is expected to be a superset).
            if remote_newer {
                outcome.actions.push(MaterializeAction::FromRemoteBlob {
                    content_hash: remote.content_hash.clone(),
                    target_path: remote.native_path.clone(),
                });
                outcome.merged_items.push(remote.clone());
            } else {
                outcome.merged_items.push(local.clone());
            }
            outcome.conflicts.push(ConflictRecord {
                provider: local.provider,
                kind: local.kind,
                logical_id: local.logical_id.clone(),
                resolution: format!(
                    "append-only conflict: {} version kept (newer)",
                    if remote_newer { "remote" } else { "local" }
                ),
                conflict_path: None,
            });
        }
        // Opaque / RecordSet / Unsupported: newer wins, losing local file
        // preserved to a conflict path so nothing is lost.
        _ => {
            if remote_newer {
                // Preserve the local file before overwriting it.
                let cpath = conflict_path(&local.native_path, local_device);
                outcome.actions.push(MaterializeAction::CopyLocalFile {
                    from_path: local.native_path.clone(),
                    to_path: cpath.clone(),
                });
                outcome.actions.push(MaterializeAction::FromRemoteBlob {
                    content_hash: remote.content_hash.clone(),
                    target_path: remote.native_path.clone(),
                });
                outcome.merged_items.push(remote.clone());
                outcome.merged_items.push(conflict_item(local, cpath.clone()));
                outcome.conflicts.push(ConflictRecord {
                    provider: local.provider,
                    kind: local.kind,
                    logical_id: local.logical_id.clone(),
                    resolution: format!(
                        "opaque conflict: took remote, preserved local copy to {cpath}"
                    ),
                    conflict_path: Some(cpath),
                });
            } else {
                // Local newer: keep local, no conflict file needed.
                outcome.merged_items.push(local.clone());
                outcome.conflicts.push(ConflictRecord {
                    provider: local.provider,
                    kind: local.kind,
                    logical_id: local.logical_id.clone(),
                    resolution: "opaque conflict: kept local (newer)".to_string(),
                    conflict_path: None,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_sync::model::Sensitivity;

    fn item(
        id: &str,
        kind: DataKind,
        cap: MergeCapability,
        hash: &str,
        updated_at: Option<i64>,
    ) -> DataItem {
        DataItem {
            provider: WorkspaceProviderId::Claude,
            kind,
            logical_id: id.to_string(),
            parent_id: None,
            native_path: id.to_string(),
            content_hash: hash.to_string(),
            updated_at,
            schema_fingerprint: None,
            merge_capability: cap,
            sensitivity: Sensitivity::WorkData,
            object_ids: vec![hash.to_string()],
        }
    }

    #[test]
    fn remote_only_item_is_pulled_down() {
        let out = merge_provider(
            &[],
            &[item("plans/a.md", DataKind::Plan, MergeCapability::Text, "h1", Some(1))],
            "dev-remote",
            "dev-local",
        );
        assert_eq!(out.actions.len(), 1);
        assert_eq!(
            out.actions[0],
            MaterializeAction::FromRemoteBlob {
                content_hash: "h1".into(),
                target_path: "plans/a.md".into()
            }
        );
        assert!(out.conflicts.is_empty());
        assert_eq!(out.merged_items.len(), 1);
    }

    #[test]
    fn local_only_item_is_kept_without_action() {
        let out = merge_provider(
            &[item("plans/a.md", DataKind::Plan, MergeCapability::Text, "h1", Some(1))],
            &[],
            "dev-remote",
            "dev-local",
        );
        assert!(out.actions.is_empty());
        assert_eq!(out.merged_items.len(), 1);
    }

    #[test]
    fn identical_hash_is_noop() {
        let l = item("plans/a.md", DataKind::Plan, MergeCapability::Text, "h1", Some(1));
        let r = item("plans/a.md", DataKind::Plan, MergeCapability::Text, "h1", Some(2));
        let out = merge_provider(&[l], &[r], "dev-remote", "dev-local");
        assert!(out.actions.is_empty());
        assert!(out.conflicts.is_empty());
        assert_eq!(out.merged_items.len(), 1);
    }

    #[test]
    fn text_conflict_keeps_both() {
        let l = item("memory/m.md", DataKind::Memory, MergeCapability::Text, "hL", Some(1));
        let r = item("memory/m.md", DataKind::Memory, MergeCapability::Text, "hR", Some(2));
        let out = merge_provider(&[l], &[r], "mac", "pc");

        // remote copy written to conflict sibling
        assert_eq!(
            out.actions,
            vec![MaterializeAction::FromRemoteBlob {
                content_hash: "hR".into(),
                target_path: "memory/m.conflict-mac.md".into(),
            }]
        );
        // local + conflict copy both in merged set
        assert_eq!(out.merged_items.len(), 2);
        assert_eq!(out.conflicts.len(), 1);
        assert_eq!(
            out.conflicts[0].conflict_path.as_deref(),
            Some("memory/m.conflict-mac.md")
        );
    }

    #[test]
    fn append_only_conflict_takes_newer() {
        let l = item("s/1.jsonl", DataKind::Session, MergeCapability::AppendOnly, "hL", Some(10));
        let r = item("s/1.jsonl", DataKind::Session, MergeCapability::AppendOnly, "hR", Some(20));
        let out = merge_provider(&[l], &[r], "mac", "pc");
        assert_eq!(
            out.actions,
            vec![MaterializeAction::FromRemoteBlob {
                content_hash: "hR".into(),
                target_path: "s/1.jsonl".into(),
            }]
        );
        assert_eq!(out.conflicts.len(), 1);
    }

    #[test]
    fn opaque_conflict_remote_newer_preserves_local() {
        let l = item("idx.json", DataKind::Index, MergeCapability::Opaque, "hL", Some(1));
        let r = item("idx.json", DataKind::Index, MergeCapability::Opaque, "hR", Some(2));
        let out = merge_provider(&[l], &[r], "mac", "pc");
        assert_eq!(
            out.actions,
            vec![
                MaterializeAction::CopyLocalFile {
                    from_path: "idx.json".into(),
                    to_path: "idx.conflict-pc.json".into(),
                },
                MaterializeAction::FromRemoteBlob {
                    content_hash: "hR".into(),
                    target_path: "idx.json".into(),
                },
            ]
        );
    }

    #[test]
    fn conflict_path_handles_extension_and_none() {
        assert_eq!(conflict_path("a/b/f.md", "mac"), "a/b/f.conflict-mac.md");
        assert_eq!(conflict_path("f.md", "mac"), "f.conflict-mac.md");
        assert_eq!(conflict_path("dir/noext", "mac"), "dir/noext.conflict-mac");
        assert_eq!(conflict_path("f.md", "a b/c"), "f.conflict-a-b-c.md");
    }
}
