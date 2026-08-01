//! Two-way merge of local and remote workspace snapshots.
//!
//! Policy (per project decision): **union by `logical_id`, never delete.** For
//! each item keyed by `(kind, logical_id)`:
//!
//! - present on one side only → keep it (materialize if it came from remote);
//! - identical `content_hash` → no-op;
//! - differing content, by [`MergeCapability`]:
//!   - `AppendOnly` (sessions/todos/`*.jsonl` logs): **line union** — the merged
//!     file is the union of both sides' lines (local order preserved, remote-only
//!     lines appended, exact duplicates de-duped). Lossless, no clobber.
//!   - `Text` (memory/plans/tasks): **newer `updated_at` wins** and overwrites.
//!     No `.conflict` sidecar (they re-propagate through the scan and multiply).
//!   - `Opaque`/`RecordSet`/`Unsupported` (e.g. `state_5.sqlite`): whole-file
//!     newer wins.
//!
//! Deletes are **not** propagated in this MVP (conservative: "keep rather than
//! risk deleting"). Tombstone-driven deletion can be layered on once the local
//! base snapshot is persisted in `state_db`.

use std::collections::BTreeMap;

use crate::workspace_sync::model::{DataItem, DataKind, MergeCapability, WorkspaceProviderId};

/// How to bring one item to its final local state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterializeAction {
    /// Fetch `content_hash` from the blob store and write it to `target_path`,
    /// stamping the file's mtime to `updated_at` (ms) when known so cross-device
    /// "newer wins" comparisons use logical edit time, not download time.
    FromRemoteBlob {
        content_hash: String,
        target_path: String,
        updated_at: Option<i64>,
    },
    /// Line-union merge: combine the local file at `native_path` with the remote
    /// blob `remote_content_hash` and write the union back to `native_path`. The
    /// engine computes the union bytes (their hash isn't known until then).
    /// `remote_newer` drives the fallback when the content isn't line-mergeable.
    UnionMergeJsonl {
        native_path: String,
        remote_content_hash: String,
        remote_newer: bool,
    },
    /// Deep JSON object union: merge the local JSON at `native_path` with the
    /// remote blob, recursively unioning objects (used for `.claude.json`'s
    /// `projects` map so neither device's projects are lost). Engine computes the
    /// merged bytes and patches the item's hash.
    MergeJsonUnion {
        native_path: String,
        remote_content_hash: String,
    },
}

/// A recorded conflict, surfaced to the UI so the user can reconcile manually.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRecord {
    pub provider: WorkspaceProviderId,
    pub kind: DataKind,
    pub logical_id: String,
    /// Human-readable resolution note (which side won / how it merged).
    pub resolution: String,
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

/// Line-union two JSONL/text blobs: keep every local line in order, then append
/// each remote line not already present. Exact-duplicate lines are de-duped.
/// A trailing newline is preserved when either input had one.
///
/// If either side is not valid UTF-8, fall back to whichever `updated_at` is
/// newer (the caller passes `remote_newer`) — a binary "log" can't be merged
/// line-wise. Returns the merged bytes.
pub fn jsonl_union(local: &[u8], remote: &[u8], remote_newer: bool) -> Vec<u8> {
    let (Ok(local_str), Ok(remote_str)) =
        (std::str::from_utf8(local), std::str::from_utf8(remote))
    else {
        return if remote_newer {
            remote.to_vec()
        } else {
            local.to_vec()
        };
    };

    let mut seen = std::collections::HashSet::new();
    let mut lines: Vec<&str> = Vec::new();
    for line in local_str.lines().chain(remote_str.lines()) {
        if seen.insert(line) {
            lines.push(line);
        }
    }

    let mut out = lines.join("\n");
    // Preserve a trailing newline if either original had one (JSONL convention).
    if (local_str.ends_with('\n') || remote_str.ends_with('\n')) && !out.is_empty() {
        out.push('\n');
    }
    out.into_bytes()
}

/// Deep-union two JSON objects: recursively merge object keys (local wins on
/// scalar/array conflicts, since the local file was just scanned and reflects
/// this device's latest state). Used for `.claude.json`'s `projects` map so
/// projects from both devices survive. Falls back to local bytes on parse error.
pub fn json_deep_union(local: &[u8], remote: &[u8]) -> Vec<u8> {
    let (Ok(mut local_val), Ok(remote_val)) = (
        serde_json::from_slice::<serde_json::Value>(local),
        serde_json::from_slice::<serde_json::Value>(remote),
    ) else {
        return local.to_vec();
    };
    deep_union(&mut local_val, &remote_val);
    serde_json::to_vec_pretty(&local_val).unwrap_or_else(|_| local.to_vec())
}

/// Merge `source` into `target`: for objects, recurse key-by-key adding
/// remote-only keys; for anything else, keep `target` (local wins).
fn deep_union(target: &mut serde_json::Value, source: &serde_json::Value) {
    if let (Some(t), Some(s)) = (target.as_object_mut(), source.as_object()) {
        for (k, sv) in s {
            match t.get_mut(k) {
                Some(tv) => deep_union(tv, sv),
                None => {
                    t.insert(k.clone(), sv.clone());
                }
            }
        }
    }
    // Non-object or type mismatch: local value is kept as-is.
}

/// Merge one provider's `local` and `remote` item lists (union by key).
pub fn merge_provider(local: &[DataItem], remote: &[DataItem]) -> MergeOutcome {
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
                    updated_at: r.updated_at,
                });
                outcome.merged_items.push((*r).clone());
            }
            (Some(l), Some(r)) => {
                if l.content_hash == r.content_hash {
                    // Identical: nothing to do.
                    outcome.merged_items.push((*l).clone());
                } else {
                    merge_conflicting_pair(l, r, &mut outcome);
                }
            }
            (None, None) => unreachable!("key came from one of the maps"),
        }
    }

    outcome
}

fn merge_conflicting_pair(local: &DataItem, remote: &DataItem, outcome: &mut MergeOutcome) {
    let remote_newer = remote.updated_at.unwrap_or(0) > local.updated_at.unwrap_or(0);

    match local.merge_capability {
        MergeCapability::AppendOnly => {
            // Line union: combine both sides losslessly. The engine reads both
            // blobs, computes the union, writes it back, and patches the merged
            // item's content_hash to the union result.
            outcome.actions.push(MaterializeAction::UnionMergeJsonl {
                native_path: local.native_path.clone(),
                remote_content_hash: remote.content_hash.clone(),
                remote_newer,
            });
            // Provisionally keep local; hash is patched post-union in the engine.
            outcome.merged_items.push(local.clone());
            outcome.conflicts.push(ConflictRecord {
                provider: local.provider,
                kind: local.kind,
                logical_id: local.logical_id.clone(),
                resolution: "append-only: line-union merged both versions".to_string(),
            });
        }
        MergeCapability::RecordSet => {
            // Deep JSON object union (e.g. `.claude.json` projects map).
            outcome.actions.push(MaterializeAction::MergeJsonUnion {
                native_path: local.native_path.clone(),
                remote_content_hash: remote.content_hash.clone(),
            });
            outcome.merged_items.push(local.clone());
            outcome.conflicts.push(ConflictRecord {
                provider: local.provider,
                kind: local.kind,
                logical_id: local.logical_id.clone(),
                resolution: "record-set: deep-merged both versions".to_string(),
            });
        }
        // Text (memory/plans/tasks) and opaque whole-file (state_5.sqlite):
        // newer updated_at wins and overwrites. No .conflict sidecar.
        _ => {
            if remote_newer {
                outcome.actions.push(MaterializeAction::FromRemoteBlob {
                    content_hash: remote.content_hash.clone(),
                    target_path: remote.native_path.clone(),
                    updated_at: remote.updated_at,
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
                    "conflict: {} version kept (newer)",
                    if remote_newer { "remote" } else { "local" }
                ),
            });
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
        );
        assert_eq!(out.actions.len(), 1);
        assert_eq!(
            out.actions[0],
            MaterializeAction::FromRemoteBlob {
                content_hash: "h1".into(),
                target_path: "plans/a.md".into(),
                updated_at: Some(1),
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
        );
        assert!(out.actions.is_empty());
        assert_eq!(out.merged_items.len(), 1);
    }

    #[test]
    fn identical_hash_is_noop() {
        let l = item("plans/a.md", DataKind::Plan, MergeCapability::Text, "h1", Some(1));
        let r = item("plans/a.md", DataKind::Plan, MergeCapability::Text, "h1", Some(2));
        let out = merge_provider(&[l], &[r]);
        assert!(out.actions.is_empty());
        assert!(out.conflicts.is_empty());
        assert_eq!(out.merged_items.len(), 1);
    }

    #[test]
    fn text_conflict_newer_wins_no_sidecar() {
        // Remote newer → overwrite local; no .conflict file produced.
        let l = item("memory/m.md", DataKind::Memory, MergeCapability::Text, "hL", Some(1));
        let r = item("memory/m.md", DataKind::Memory, MergeCapability::Text, "hR", Some(2));
        let out = merge_provider(&[l], &[r]);

        assert_eq!(
            out.actions,
            vec![MaterializeAction::FromRemoteBlob {
                content_hash: "hR".into(),
                target_path: "memory/m.md".into(),
                updated_at: Some(2),
            }]
        );
        // Only the winning item in the merged set — no conflict copy.
        assert_eq!(out.merged_items.len(), 1);
        assert_eq!(out.merged_items[0].content_hash, "hR");
        assert_eq!(out.conflicts.len(), 1);
    }

    #[test]
    fn text_conflict_local_newer_keeps_local() {
        let l = item("memory/m.md", DataKind::Memory, MergeCapability::Text, "hL", Some(5));
        let r = item("memory/m.md", DataKind::Memory, MergeCapability::Text, "hR", Some(2));
        let out = merge_provider(&[l], &[r]);
        assert!(out.actions.is_empty());
        assert_eq!(out.merged_items.len(), 1);
        assert_eq!(out.merged_items[0].content_hash, "hL");
    }

    #[test]
    fn append_only_conflict_line_unions() {
        let l = item("s/1.jsonl", DataKind::Session, MergeCapability::AppendOnly, "hL", Some(10));
        let r = item("s/1.jsonl", DataKind::Session, MergeCapability::AppendOnly, "hR", Some(20));
        let out = merge_provider(&[l], &[r]);
        assert_eq!(
            out.actions,
            vec![MaterializeAction::UnionMergeJsonl {
                native_path: "s/1.jsonl".into(),
                remote_content_hash: "hR".into(),
                remote_newer: true,
            }]
        );
        assert_eq!(out.conflicts.len(), 1);
        assert_eq!(out.merged_items.len(), 1);
    }

    #[test]
    fn opaque_conflict_remote_newer_overwrites_no_sidecar() {
        let l = item("state.sqlite", DataKind::Index, MergeCapability::Opaque, "hL", Some(1));
        let r = item("state.sqlite", DataKind::Index, MergeCapability::Opaque, "hR", Some(2));
        let out = merge_provider(&[l], &[r]);
        assert_eq!(
            out.actions,
            vec![MaterializeAction::FromRemoteBlob {
                content_hash: "hR".into(),
                target_path: "state.sqlite".into(),
                updated_at: Some(2),
            }]
        );
        assert_eq!(out.merged_items.len(), 1);
        assert_eq!(out.merged_items[0].content_hash, "hR");
    }

    #[test]
    fn jsonl_union_dedups_and_preserves_order() {
        let local = b"a\nb\nc\n";
        let remote = b"b\nd\ne\n";
        let merged = jsonl_union(local, remote, true);
        assert_eq!(String::from_utf8(merged).unwrap(), "a\nb\nc\nd\ne\n");
    }

    #[test]
    fn jsonl_union_disjoint() {
        let merged = jsonl_union(b"x\ny", b"z", false);
        // Neither input had a trailing newline (last line "y"/"z" had none).
        assert_eq!(String::from_utf8(merged).unwrap(), "x\ny\nz");
    }

    #[test]
    fn jsonl_union_non_utf8_falls_back_to_newer() {
        let local = b"local\n";
        let remote = &[0xff, 0xfe, 0x00][..]; // invalid UTF-8
        assert_eq!(jsonl_union(local, remote, true), remote.to_vec());
        assert_eq!(jsonl_union(local, remote, false), local.to_vec());
    }
}
