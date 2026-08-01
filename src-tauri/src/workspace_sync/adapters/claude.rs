//! Claude Code (`~/.claude`) workspace mappings.
//!
//! Session/plan/task/todo files sync via the generic [`FsAdapter`]. Two extra
//! concerns are handled by [`ClaudeAdapter`] so the Claude client can actually
//! *find* synced sessions on another machine:
//!
//! 1. **Project directory names encode an absolute path.** `~/.claude/projects/`
//!    subdir names are the project's absolute working dir with `/` → `-`
//!    (e.g. `/Users/alice/x` → `-Users-alice-x`). Across machines the home
//!    prefix differs, so we rewrite the encoded-home prefix to a portable
//!    `${HOME_ENC}` token on upload and back to the local home on download.
//! 2. **`~/.claude.json` holds the `projects` map** (keyed by absolute path) that
//!    the client uses to list projects. It lives *outside* `~/.claude`, and only
//!    its `projects` subtree is workspace data — the rest (`primaryApiKey`,
//!    `mcpServers`, onboarding) is config, owned elsewhere. We sync just that
//!    subtree (as a `RecordSet` deep-merge item), tokenizing its path keys.
//!
//! Limitation: only the home prefix is remapped. A repo cloned to a path outside
//! the home dir on the other machine won't line up.

use serde_json::Value;

use super::{FsAdapter, Mapping, ProviderAdapter};
use crate::config::{get_claude_config_dir, get_default_claude_mcp_path, get_home_dir};
use crate::error::AppError;
use crate::workspace_sync::model::{
    DataItem, DataKind, MergeCapability, Sensitivity, WorkspaceProviderId,
};

/// `projects/**/*.jsonl` are append-only session transcripts; `plans/`,
/// `tasks/` are per-item text files; `todos/` and `history.jsonl` are
/// line-append JSONL logs, so they merge by **line union** (`AppendOnly`).
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
    Mapping::file("history.jsonl", DataKind::Index, MergeCapability::AppendOnly),
];

/// Portable token standing in for the encoded home prefix in project dir names.
const HOME_ENC_TOKEN: &str = "${HOME_ENC}";
/// Portable token standing in for the real home prefix in `.claude.json` keys.
const HOME_TOKEN: &str = "${HOME}";
/// Sentinel native_path for the `.claude.json` projects subtree item.
const CLAUDE_JSON_PROJECTS_ID: &str = "__claude_json_projects__";

pub fn adapter() -> ClaudeAdapter {
    let root = get_claude_config_dir();
    ClaudeAdapter {
        inner: FsAdapter::new(WorkspaceProviderId::Claude, root.clone(), MAPPINGS),
    }
}

pub struct ClaudeAdapter {
    inner: FsAdapter,
}

/// Encode an absolute path the way Claude names project dirs: `/` → `-`.
fn encode_path(abs: &str) -> String {
    abs.replace(['/', '\\'], "-")
}

fn home_str() -> String {
    get_home_dir().to_string_lossy().replace('\\', "/")
}

/// Rewrite a scanned `projects/<encoded-abs>/…` path to a portable form by
/// replacing the encoded-home prefix with [`HOME_ENC_TOKEN`]. Non-project paths
/// pass through unchanged.
fn to_portable_project_path(native: &str) -> String {
    let Some(rest) = native.strip_prefix("projects/") else {
        return native.to_string();
    };
    let home_enc = encode_path(&home_str());
    if let Some(tail) = rest.strip_prefix(&home_enc) {
        format!("projects/{HOME_ENC_TOKEN}{tail}")
    } else {
        native.to_string()
    }
}

/// Inverse of [`to_portable_project_path`]: substitute the local encoded home.
fn from_portable_project_path(portable: &str) -> String {
    let Some(rest) = portable.strip_prefix("projects/") else {
        return portable.to_string();
    };
    if let Some(tail) = rest.strip_prefix(HOME_ENC_TOKEN) {
        let home_enc = encode_path(&home_str());
        format!("projects/{home_enc}{tail}")
    } else {
        portable.to_string()
    }
}

/// Tokenize the real-home prefix in a `.claude.json` project key.
fn to_portable_key(key: &str) -> String {
    let home = home_str();
    if let Some(tail) = key.strip_prefix(&home) {
        format!("{HOME_TOKEN}{tail}")
    } else {
        key.to_string()
    }
}

fn from_portable_key(key: &str) -> String {
    if let Some(tail) = key.strip_prefix(HOME_TOKEN) {
        format!("{}{tail}", home_str())
    } else {
        key.to_string()
    }
}

impl ClaudeAdapter {
    /// Read `.claude.json`'s `projects` subtree, tokenizing path keys, as a
    /// stable pretty-printed JSON object. `None` if absent or has no projects.
    fn read_claude_json_projects(&self) -> Option<Vec<u8>> {
        let path = get_default_claude_mcp_path();
        let bytes = std::fs::read(&path).ok()?;
        let value: Value = serde_json::from_slice(&bytes).ok()?;
        let projects = value.get("projects")?.as_object()?;
        if projects.is_empty() {
            return None;
        }
        let tokenized: serde_json::Map<String, Value> = projects
            .iter()
            .map(|(k, v)| (to_portable_key(k), v.clone()))
            .collect();
        serde_json::to_vec_pretty(&Value::Object(tokenized)).ok()
    }

    /// Write the (already deep-merged) tokenized projects subtree back into
    /// `.claude.json`, detokenizing keys and preserving all other top-level keys.
    fn write_claude_json_projects(&self, tokenized_bytes: &[u8]) -> Result<(), AppError> {
        let path = get_default_claude_mcp_path();
        let incoming: Value = serde_json::from_slice(tokenized_bytes)
            .map_err(|e| AppError::Message(format!("invalid .claude.json projects: {e}")))?;
        let Some(incoming_obj) = incoming.as_object() else {
            return Ok(());
        };

        // Load existing file (or start empty) and merge only the projects map.
        let mut root: Value = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| Value::Object(Default::default()));
        if !root.is_object() {
            root = Value::Object(Default::default());
        }
        let root_obj = root.as_object_mut().unwrap();
        let projects = root_obj
            .entry("projects")
            .or_insert_with(|| Value::Object(Default::default()));
        if !projects.is_object() {
            *projects = Value::Object(Default::default());
        }
        let projects_obj = projects.as_object_mut().unwrap();
        for (k, v) in incoming_obj {
            projects_obj
                .entry(from_portable_key(k))
                .or_insert_with(|| v.clone());
        }

        let out = serde_json::to_vec_pretty(&root)
            .map_err(|e| AppError::Message(format!("serialize .claude.json: {e}")))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        let tmp = path.with_extension("json.cc-switch-tmp");
        std::fs::write(&tmp, &out).map_err(|e| AppError::io(&tmp, e))?;
        std::fs::rename(&tmp, &path).map_err(|e| AppError::io(&path, e))?;
        Ok(())
    }
}

impl ProviderAdapter for ClaudeAdapter {
    fn provider(&self) -> WorkspaceProviderId {
        self.inner.provider()
    }

    fn is_installed(&self) -> bool {
        self.inner.is_installed()
    }

    fn scan(&self) -> Result<Vec<DataItem>, AppError> {
        let mut items = self.inner.scan()?;
        // Rewrite project file paths to a portable, home-agnostic form.
        for item in items.iter_mut() {
            if item.native_path.starts_with("projects/") {
                item.native_path = to_portable_project_path(&item.native_path);
                item.logical_id = item.native_path.clone();
            }
        }

        // Add the `.claude.json` projects subtree as a deep-merge item.
        if let Some(bytes) = self.read_claude_json_projects() {
            let content_hash = crate::services::sync_protocol::sha256_hex(&bytes);
            let updated_at = std::fs::metadata(get_default_claude_mcp_path())
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64);
            items.push(DataItem {
                provider: WorkspaceProviderId::Claude,
                kind: DataKind::Index,
                logical_id: CLAUDE_JSON_PROJECTS_ID.to_string(),
                parent_id: None,
                native_path: CLAUDE_JSON_PROJECTS_ID.to_string(),
                content_hash: content_hash.clone(),
                updated_at,
                schema_fingerprint: None,
                merge_capability: MergeCapability::RecordSet,
                sensitivity: Sensitivity::WorkData,
                object_ids: vec![content_hash],
            });
        }

        Ok(items)
    }

    fn materialize(&self, item: &DataItem, bytes: &[u8]) -> Result<(), AppError> {
        if item.native_path == CLAUDE_JSON_PROJECTS_ID {
            return self.write_claude_json_projects(bytes);
        }
        if item.native_path.starts_with("projects/") {
            let mut local = item.clone();
            local.native_path = from_portable_project_path(&item.native_path);
            return self.inner.materialize(&local, bytes);
        }
        self.inner.materialize(item, bytes)
    }

    fn read_blob(&self, item: &DataItem) -> Result<Vec<u8>, AppError> {
        if item.native_path == CLAUDE_JSON_PROJECTS_ID {
            return self
                .read_claude_json_projects()
                .ok_or_else(|| AppError::Message("claude.json projects vanished".into()));
        }
        if item.native_path.starts_with("projects/") {
            let mut local = item.clone();
            local.native_path = from_portable_project_path(&item.native_path);
            return self.inner.read_blob(&local);
        }
        self.inner.read_blob(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_path_portable_roundtrip() {
        let home = home_str();
        let enc = encode_path(&home);
        let native = format!("projects/{enc}-Documents-Work-cc-switch/abc.jsonl");
        let portable = to_portable_project_path(&native);
        assert!(portable.starts_with(&format!("projects/{HOME_ENC_TOKEN}")));
        assert!(!portable.contains(&enc));
        // Round-trips back to the local encoded home.
        assert_eq!(from_portable_project_path(&portable), native);
    }

    #[test]
    fn project_path_outside_home_passes_through() {
        let native = "projects/-opt-shared-repo/x.jsonl";
        // Not under home → unchanged both ways (documented limitation).
        assert_eq!(to_portable_project_path(native), native);
    }

    #[test]
    fn claude_json_key_portable_roundtrip() {
        let home = home_str();
        let key = format!("{home}/Documents/Work/cc-switch");
        let portable = to_portable_key(&key);
        assert_eq!(portable, format!("{HOME_TOKEN}/Documents/Work/cc-switch"));
        assert_eq!(from_portable_key(&portable), key);
    }
}
