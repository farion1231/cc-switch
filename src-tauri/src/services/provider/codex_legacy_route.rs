//! Existing official-proxy definitions are compatibility data, not the current route.
//! Keep their real opening state separate from the normalized editor preview.

use std::path::Path;

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item};

use crate::codex_config::get_codex_config_path;
use crate::error::AppError;
use crate::live::engine::{read_current, sha256_hex};
use crate::live::patch::toml::{parse, shape_error, TomlDocPatch};
use crate::live::patch::LiveWriteError;
use crate::live::project::codex::{profile_references, OFFICIAL_PROXY_ROUTE_ID};

use super::claude_editor::ConflictPolicy;
use super::editor_toml::{insert_at, remove_at, TomlEdits};

fn route_path() -> [String; 2] {
    ["model_providers".into(), OFFICIAL_PROXY_ROUTE_ID.into()]
}

fn legacy<'a>(path: &Path, doc: &'a DocumentMut) -> Result<Option<&'a Item>, LiveWriteError> {
    let Some(container) = doc.get("model_providers") else {
        return Ok(None);
    };
    let tables = container
        .as_table_like()
        .ok_or_else(|| shape_error(path, &route_path()[..1]))?;
    let item = tables.get(OFFICIAL_PROXY_ROUTE_ID);
    if item.is_some_and(|item| item.as_table_like().is_none()) {
        return Err(shape_error(path, &route_path()));
    }
    Ok(item)
}

// Sort parsed keys recursively, including nested tables inside arrays. TOML serialization
// preserves scalar types (including dates and non-finite floats) without leaking them.
fn ordered(value: toml::Value) -> toml::Value {
    match value {
        toml::Value::Table(table) => {
            let mut entries: Vec<_> = table.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            toml::Value::Table(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, ordered(value)))
                    .collect(),
            )
        }
        toml::Value::Array(values) => toml::Value::Array(values.into_iter().map(ordered).collect()),
        scalar => scalar,
    }
}

fn fingerprint(path: &Path, doc: &DocumentMut) -> Result<Option<String>, LiveWriteError> {
    let Some(item) = legacy(path, doc)? else {
        return Ok(None);
    };
    let mut fragment = DocumentMut::new();
    fragment.insert("route", item.clone());
    let normalize_error = || LiveWriteError::Parse {
        path: path.to_path_buf(),
        line: 1,
        column: 1,
        message: "Cannot normalize the legacy official proxy table".into(),
    };
    let parsed =
        toml::from_str::<toml::Value>(&fragment.to_string()).map_err(|_| normalize_error())?;
    let canonical = toml::to_string(&ordered(parsed)).map_err(|_| normalize_error())?;
    Ok(Some(sha256_hex(canonical.as_bytes())))
}

fn selected(doc: &DocumentMut) -> Option<&str> {
    doc.get("model_provider")
        .and_then(Item::as_str)
        .map(str::trim)
}

fn referenced(doc: &DocumentMut) -> bool {
    profile_references(doc.as_table(), OFFICIAL_PROXY_ROUTE_ID)
}

fn protect(path: &Path, doc: &DocumentMut) -> Result<(), LiveWriteError> {
    if selected(doc) == Some(OFFICIAL_PROXY_ROUTE_ID) || referenced(doc) {
        return Err(LiveWriteError::ProtectedRoute {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// Only a digest and reference facts cross the editor boundary, never the old table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEditorSnapshot {
    pub selector: Option<String>,
    pub legacy_route: bool,
    pub legacy_fingerprint: Option<String>,
    pub profile_referenced: bool,
}

impl CodexEditorSnapshot {
    pub(crate) fn of(path: &Path, doc: &DocumentMut) -> Result<Self, LiveWriteError> {
        let legacy_fingerprint = fingerprint(path, doc)?;
        Ok(Self {
            selector: selected(doc).map(str::to_string),
            legacy_route: legacy_fingerprint.is_some(),
            legacy_fingerprint,
            profile_referenced: referenced(doc),
        })
    }

    fn baseline(&self, base: &DocumentMut) -> Option<&str> {
        self.legacy_fingerprint.as_deref().filter(|hash| {
            self.legacy_route
                && hash.len() == 64
                && hash.bytes().all(|b| b.is_ascii_hexdigit())
                && self.selector.as_deref().map(str::trim) != Some(OFFICIAL_PROXY_ROUTE_ID)
                && !self.profile_referenced
                && !referenced(base)
        })
    }
}

#[derive(Debug, Clone)]
struct LegacyEdit {
    baseline: Option<String>,
    after: Option<Item>,
    after_fingerprint: Option<String>,
    on_conflict: ConflictPolicy,
}

/// Both editor write paths use this patch; every engine reread repeats the protection.
#[derive(Debug, Clone)]
pub(crate) struct CodexEditorEdits {
    global: TomlEdits,
    legacy: Option<LegacyEdit>,
}

impl CodexEditorEdits {
    pub(crate) fn new(
        global: TomlEdits,
        base: &DocumentMut,
        edited: &DocumentMut,
        snapshot: Option<&CodexEditorSnapshot>,
        on_conflict: ConflictPolicy,
    ) -> Result<Self, LiveWriteError> {
        let path = get_codex_config_path();
        let before = fingerprint(&path, base)?;
        let after_fingerprint = fingerprint(&path, edited)?;
        let is_provider_route =
            [selected(base), selected(edited)].contains(&Some(OFFICIAL_PROXY_ROUTE_ID));
        let legacy = if !is_provider_route && before != after_fingerprint {
            Some(LegacyEdit {
                baseline: snapshot
                    .filter(|_| before.is_some())
                    .and_then(|s| s.baseline(base))
                    .map(str::to_string),
                after: legacy(&path, edited)?.cloned(),
                after_fingerprint,
                on_conflict,
            })
        } else {
            None
        };
        Ok(Self { global, legacy })
    }

    /// A later contract rewrite must preserve the same legacy decision, without
    /// replaying unrelated global edits that have already been published.
    pub(crate) fn legacy_only(&self) -> Self {
        Self {
            global: TomlEdits::between(&[], &[], ConflictPolicy::Refuse),
            legacy: self.legacy.clone(),
        }
    }

    pub(crate) fn project<'a>(
        &'a self,
        projection: &'a dyn TomlDocPatch,
    ) -> impl TomlDocPatch + 'a {
        EditorProjection {
            edits: self,
            projection,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.global.is_empty() && self.legacy.is_none()
    }

    /// Called with the settled switch lock, before saving a row or starting an operation.
    pub(crate) fn check_live(&self) -> Result<(), AppError> {
        if self.legacy.is_none() {
            return Ok(());
        }
        let path = get_codex_config_path();
        let pre = read_current(&path)?;
        let mut doc = parse(&path, pre.as_deref())?;
        self.apply_to(&path, &mut doc)?;
        Ok(())
    }
}

impl TomlDocPatch for CodexEditorEdits {
    fn apply_to(&self, path: &Path, doc: &mut DocumentMut) -> Result<(), LiveWriteError> {
        let Some(edit) = &self.legacy else {
            return self.global.apply_to(path, doc);
        };
        protect(path, doc)?;
        let current = fingerprint(path, doc)?;
        let mut next = doc.clone();
        self.global.apply_to(path, &mut next)?;
        protect(path, &next)?;
        // An absent or inconsistent opening snapshot never authorizes modifying the table.
        if let Some(baseline) = &edit.baseline {
            let accepted = edit
                .on_conflict
                .resolve(path, std::slice::from_ref(edit), |_| {
                    (current.as_ref() != Some(baseline) && current != edit.after_fingerprint)
                        .then(|| route_path().join("."))
                })?;
            if !accepted.is_empty() && current != edit.after_fingerprint {
                match &edit.after {
                    Some(item) => insert_at(&mut next, &route_path(), item.clone()),
                    None => remove_at(&mut next, &route_path()),
                }
            }
        }
        *doc = next;
        Ok(())
    }
}

/// Resolve against the actual read before projection changes the selector or table.
/// Only an explicit legacy intent overrides routine maintenance for this write.
struct EditorProjection<'a> {
    edits: &'a CodexEditorEdits,
    projection: &'a dyn TomlDocPatch,
}

impl TomlDocPatch for EditorProjection<'_> {
    fn apply_to(&self, path: &Path, doc: &mut DocumentMut) -> Result<(), LiveWriteError> {
        let mut next = doc.clone();
        self.edits.apply_to(path, &mut next)?;
        let resolved = legacy(path, &next)?.cloned();
        self.projection.apply_to(path, &mut next)?;
        if self.edits.legacy.is_some() {
            match resolved {
                Some(item) => insert_at(&mut next, &route_path(), item),
                None => remove_at(&mut next, &route_path()),
            }
        }
        *doc = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::editor_toml::Entry;
    use super::*;

    const LIVE: &str = "model_provider = \"custom\"\napproval_policy = \"on-request\"\n[model_providers.cc-switch-official]\nname = \"Renamed\"\nbase_url = \"http://127.0.0.1:9999/v1\"\nhttp_headers = { Authorization = \"synthetic-secret\" }\n";
    const PREVIEW: &str = "model_provider = \"custom\"\napproval_policy = \"on-request\"\n[model_providers.cc-switch-official]\nname = \"OpenAI\"\nbase_url = \"http://127.0.0.1:15721/v1\"\nrequires_openai_auth = true\nsupports_websockets = false\nwire_api = \"responses\"\n";

    fn doc(text: &str) -> DocumentMut {
        text.parse().unwrap()
    }
    fn path() -> &'static Path {
        Path::new("config.toml")
    }
    fn snapshot(text: &str) -> CodexEditorSnapshot {
        CodexEditorSnapshot::of(path(), &doc(text)).unwrap()
    }
    fn edited(overwrite: bool) -> DocumentMut {
        let mut edited = doc(PREVIEW);
        edited["approval_policy"] = toml_edit::value("never");
        if overwrite {
            edited["model_providers"][OFFICIAL_PROXY_ROUTE_ID]["name"] = toml_edit::value("Edited");
        } else {
            remove_at(&mut edited, &route_path());
        }
        edited
    }
    fn patch(
        edited: &DocumentMut,
        snapshot: Option<&CodexEditorSnapshot>,
        policy: ConflictPolicy,
    ) -> CodexEditorEdits {
        let base = doc(PREVIEW);
        let globals = |doc: &DocumentMut| {
            ["approval_policy", "profiles"]
                .into_iter()
                .filter_map(|key| {
                    doc.get(key).map(|item| Entry {
                        path: vec![key.into()],
                        item: item.clone(),
                    })
                })
                .collect::<Vec<_>>()
        };
        CodexEditorEdits::new(
            TomlEdits::between(&globals(&base), &globals(edited), policy),
            &base,
            edited,
            snapshot,
            policy,
        )
        .unwrap()
    }
    const POLICIES: [ConflictPolicy; 3] = [
        ConflictPolicy::Refuse,
        ConflictPolicy::KeepMine,
        ConflictPolicy::KeepTheirs,
    ];

    #[test]
    fn opening_drift_is_not_an_external_conflict_and_edits_are_idempotent() {
        for overwrite in [false, true] {
            let edited = edited(overwrite);
            let patch = patch(&edited, Some(&snapshot(LIVE)), ConflictPolicy::Refuse);
            let mut live = doc(LIVE);
            patch.apply_to(path(), &mut live).unwrap();
            assert_eq!(
                fingerprint(path(), &live).unwrap(),
                fingerprint(path(), &edited).unwrap()
            );
            assert_eq!(live["approval_policy"].as_str(), Some("never"));
            let once = live.to_string();
            patch.apply_to(path(), &mut live).unwrap();
            assert_eq!(live.to_string(), once);
        }
    }

    #[test]
    fn external_changes_obey_each_policy_for_delete_and_overwrite() {
        for overwrite in [false, true] {
            for policy in POLICIES {
                let edited = edited(overwrite);
                let patch = patch(&edited, Some(&snapshot(LIVE)), policy);
                let mut live = doc(&LIVE.replace("9999", "8888"));
                let before = live.to_string();
                let result = patch.apply_to(path(), &mut live);
                match policy {
                    ConflictPolicy::Refuse => {
                        assert!(matches!(result, Err(LiveWriteError::EditConflict { .. })));
                        assert_eq!(live.to_string(), before);
                    }
                    ConflictPolicy::KeepMine => {
                        result.unwrap();
                        assert_eq!(
                            fingerprint(path(), &live).unwrap(),
                            fingerprint(path(), &edited).unwrap()
                        );
                    }
                    ConflictPolicy::KeepTheirs => {
                        result.unwrap();
                        assert_eq!(
                            fingerprint(path(), &live).unwrap(),
                            fingerprint(path(), &doc(&before)).unwrap()
                        );
                        assert_eq!(live["approval_policy"].as_str(), Some("never"));
                    }
                }
            }
        }
    }

    #[test]
    fn every_application_rechecks_current_and_new_references_before_conflict_policy() {
        for overwrite in [false, true] {
            for policy in POLICIES {
                let edited = edited(overwrite);
                let patch = patch(&edited, Some(&snapshot(LIVE)), policy);
                // A successful early dry run must not authorize a later active document.
                patch.apply_to(path(), &mut doc(LIVE)).unwrap();
                for text in [
                    LIVE.replace("model_provider = \"custom\"", "model_provider = \"cc-switch-official\""),
                    format!("{LIVE}\n[profiles.sleeping]\nmodel_provider = \"cc-switch-official\"\n"),
                    format!("profiles = {{ work = {{ model_provider = \" cc-switch-official \" }} }}\n{LIVE}"),
                ] {
                    let mut live = doc(&text);
                    let error = patch.apply_to(path(), &mut live).unwrap_err();
                    assert!(matches!(error, LiveWriteError::ProtectedRoute { .. }));
                    assert_eq!(live.to_string(), text);
                    let error: AppError = error.into();
                    assert!(!error.to_string().contains(crate::live::patch::EDIT_CONFLICT_CODE));
                }
                let mut new_reference = edited.clone();
                insert_at(
                    &mut new_reference,
                    &["profiles".into(), "added".into(), "model_provider".into()],
                    toml_edit::value(OFFICIAL_PROXY_ROUTE_ID),
                );
                let patch = self::patch(&new_reference, Some(&snapshot(LIVE)), policy);
                let mut live = doc(LIVE);
                assert!(matches!(
                    patch.apply_to(path(), &mut live),
                    Err(LiveWriteError::ProtectedRoute { .. })
                ));
                assert_eq!(live.to_string(), LIVE);
            }
        }
    }

    #[test]
    fn unsafe_snapshots_leave_the_table_alone_but_allow_global_edits() {
        let good = snapshot(LIVE);
        let mut inconsistent = good.clone();
        inconsistent.legacy_route = false;
        let mut invalid_hash = good.clone();
        invalid_hash.legacy_fingerprint = Some("bad".into());
        let mut missing_hash = good.clone();
        missing_hash.legacy_fingerprint = None;
        let mut selected = good.clone();
        selected.selector = Some(OFFICIAL_PROXY_ROUTE_ID.into());
        let mut referenced = good.clone();
        referenced.profile_referenced = true;
        for snapshot in [
            None,
            Some(inconsistent),
            Some(invalid_hash),
            Some(missing_hash),
            Some(selected),
            Some(referenced),
        ] {
            for policy in POLICIES {
                let patch = patch(&edited(false), snapshot.as_ref(), policy);
                let mut live = doc(LIVE);
                patch.apply_to(path(), &mut live).unwrap();
                assert_eq!(fingerprint(path(), &live).unwrap(), good.legacy_fingerprint);
                assert_eq!(live["approval_policy"].as_str(), Some("never"));
            }
        }
    }

    #[test]
    fn semantic_fingerprints_ignore_layout_and_do_not_expose_route_contents() {
        let standard = "[model_providers.cc-switch-official]\n# table comment\nname = 'OpenAI' # comment\nnumber = 1_000\nitems = [{ b = 2, a = 1 }]\n[model_providers.cc-switch-official.http_headers]\nz = 'last'\na = 'synthetic-secret'\n";
        let inline = "model_providers = { cc-switch-official = { items = [{a=1,b=2}], number = 1000, http_headers = { a = \"synthetic-secret\", z = \"last\" }, name = \"OpenAI\" } }\n";
        assert_eq!(
            snapshot(standard).legacy_fingerprint,
            snapshot(inline).legacy_fingerprint
        );
        assert_ne!(
            snapshot(standard).legacy_fingerprint,
            snapshot(&inline.replace("1000", "1001")).legacy_fingerprint
        );
        let serialized = serde_json::to_string(&snapshot(standard)).unwrap();
        assert!(!serialized.contains("synthetic-secret"));
        assert!(!serialized.contains("http_headers"));
        assert_eq!(snapshot(standard).legacy_fingerprint.unwrap().len(), 64);
        for malformed in [
            "model_providers = 1",
            "[model_providers]\ncc-switch-official = 'bad'",
        ] {
            assert!(matches!(
                CodexEditorSnapshot::of(path(), &doc(malformed)),
                Err(LiveWriteError::Shape { .. })
            ));
        }
    }

    #[test]
    fn untouched_legacy_and_missing_legacy_do_not_create_an_edit() {
        let base = doc(PREVIEW);
        let mut edited = base.clone();
        edited["approval_policy"] = toml_edit::value("never");
        let patch = patch(&edited, Some(&snapshot(LIVE)), ConflictPolicy::Refuse);
        let mut active = doc(&LIVE.replace("custom", OFFICIAL_PROXY_ROUTE_ID));
        patch.apply_to(path(), &mut active).unwrap();
        assert_eq!(active["approval_policy"].as_str(), Some("never"));
        assert_eq!(
            fingerprint(path(), &active).unwrap(),
            snapshot(LIVE).legacy_fingerprint
        );

        let empty = DocumentMut::new();
        let patch = CodexEditorEdits::new(
            TomlEdits::between(&[], &[], ConflictPolicy::Refuse),
            &empty,
            &empty,
            None,
            ConflictPolicy::Refuse,
        )
        .unwrap();
        assert!(patch.is_empty());
    }
}
