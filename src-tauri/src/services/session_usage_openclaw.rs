//! Read-only OpenClaw session discovery for the canonical session-usage layer.
//!
//! OpenClaw keeps one JSONL file per session below
//! `<openclaw-root>/agents/<agent-id>/sessions`.  The current local format does
//! not provide a fixture-proven token/cost contract, so this adapter deliberately
//! imports only the session node metadata and exposes an unavailable usage
//! measure.  It never writes OpenClaw data, reads message bodies, or manufactures
//! zero-valued usage buckets.

use crate::services::agent_session_usage::{
    SessionNodeMetadata, SessionRelationClaim, UsageMeasure,
};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Canonical AppType used by the normalized session-usage contract.
pub const OPENCLAW_APP_TYPE: &str = "openclaw";

/// A single discovered OpenClaw session.  `session_id` is the namespaced
/// canonical key; `bare_session_id` is retained for diagnostics only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawSessionRecord {
    pub agent_id: String,
    pub bare_session_id: String,
    pub session_id: String,
    pub source_path: PathBuf,
    pub claim: SessionRelationClaim,
    pub usage: UsageMeasure,
    pub used_filename_fallback: bool,
}

/// Result returned to the central sync orchestrator.  Usage is unavailable for
/// every current record; callers should normalize `claims` and persist nodes,
/// but must not persist a rollup for the unavailable measure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawSyncResult {
    pub sessions: Vec<OpenClawSessionRecord>,
    pub files_scanned: u32,
    pub files_imported: u32,
    pub files_skipped: u32,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct OpenClawNativeSessionMetadata {
    display_name: Option<String>,
    cwd: Option<String>,
}

impl OpenClawSyncResult {
    /// Claims for the normalized node bridge.  No usage rollup is returned:
    /// OpenClaw token/cost fields are not locally proven at this time.
    pub fn claims(&self) -> Vec<SessionRelationClaim> {
        self.sessions
            .iter()
            .map(|session| session.claim.clone())
            .collect()
    }
}

/// Build a stable, multi-agent session key.  OpenClaw session IDs are only
/// agent-local in the supported directory layout; including the agent ID is
/// therefore mandatory to avoid collisions between agents with the same bare
/// session ID.
pub fn namespaced_session_id(agent_id: &str, session_id: &str) -> Option<String> {
    let agent_id = agent_id.trim();
    let session_id = session_id.trim();
    if agent_id.is_empty() || session_id.is_empty() {
        return None;
    }

    // The ordinary OpenClaw IDs do not contain `:`, preserving the documented
    // `<agentId>:<sessionId>` shape.  Escaping the delimiter keeps the key
    // unambiguous even if a future version permits it in an ID component.
    Some(format!(
        "{}:{}",
        escape_namespace_component(agent_id),
        escape_namespace_component(session_id)
    ))
}

fn escape_namespace_component(value: &str) -> String {
    value.replace('%', "%25").replace(':', "%3A")
}

/// Scan an anonymous or configured OpenClaw root.  Missing roots and missing
/// `agents` directories are intentional no-ops; per-file failures are isolated
/// in `errors` and never abort unrelated sessions.
pub fn scan_openclaw_sessions(root: impl AsRef<Path>) -> OpenClawSyncResult {
    let root = root.as_ref();
    let agents_dir = root.join("agents");
    if !agents_dir.is_dir() {
        return OpenClawSyncResult::default();
    }

    let mut result = OpenClawSyncResult::default();
    let agent_paths = match sorted_child_directories(&agents_dir) {
        Ok(paths) => paths,
        Err(error) => {
            result.errors.push(format!(
                "OpenClaw agents directory {} is unreadable: {error}",
                agents_dir.display()
            ));
            return result;
        }
    };

    let mut seen_session_ids = HashSet::new();
    for agent_path in agent_paths {
        let Some(agent_id) = path_component(&agent_path) else {
            continue;
        };
        let sessions_dir = agent_path.join("sessions");
        if !sessions_dir.is_dir() {
            continue;
        }

        let session_paths = match sorted_jsonl_files(&sessions_dir) {
            Ok(paths) => paths,
            Err(error) => {
                result.errors.push(format!(
                    "OpenClaw sessions directory {} is unreadable: {error}",
                    sessions_dir.display()
                ));
                continue;
            }
        };
        let native_metadata = load_native_session_metadata(&sessions_dir);

        for path in session_paths {
            result.files_scanned = result.files_scanned.saturating_add(1);
            match parse_session_file(&agent_id, &path, &native_metadata) {
                Ok(record) => {
                    if !seen_session_ids.insert(record.session_id.clone()) {
                        result.files_skipped = result.files_skipped.saturating_add(1);
                        result.errors.push(format!(
                            "OpenClaw duplicate canonical session {} in {} (file skipped)",
                            record.session_id,
                            path.display()
                        ));
                        continue;
                    }
                    result.files_imported = result.files_imported.saturating_add(1);
                    result.sessions.push(record);
                }
                Err(FileImportError::Malformed(error)) => {
                    result.files_skipped = result.files_skipped.saturating_add(1);
                    result.errors.push(format!(
                        "OpenClaw malformed JSONL {}: {error} (file skipped)",
                        path.display()
                    ));
                }
                Err(FileImportError::Unreadable(error)) => {
                    result.files_skipped = result.files_skipped.saturating_add(1);
                    result.errors.push(format!(
                        "OpenClaw session {} is unreadable: {error} (file skipped)",
                        path.display()
                    ));
                }
            }
        }
    }

    result
}

/// Integration-facing name for T12.  The root is explicit so tests never need
/// to touch the user's OpenClaw directory; T12 can pass its configured default
/// root when registering this source.
pub fn sync_openclaw_session_usage(root: impl AsRef<Path>) -> OpenClawSyncResult {
    scan_openclaw_sessions(root)
}

#[derive(Debug)]
enum FileImportError {
    Malformed(String),
    Unreadable(String),
}

fn parse_session_file(
    agent_id: &str,
    path: &Path,
    native_metadata: &HashMap<String, OpenClawNativeSessionMetadata>,
) -> Result<OpenClawSessionRecord, FileImportError> {
    let header = read_session_header(path)?;
    let (bare_session_id, cwd, created_at, used_filename_fallback, header_warning) = match header {
        HeaderRead::Session {
            id,
            cwd,
            created_at,
        } => {
            let fallback = id.is_none();
            let bare_id = id.or_else(|| filename_session_id(path)).ok_or_else(|| {
                FileImportError::Malformed("session id and filename are empty".into())
            })?;
            let warning = fallback.then_some(
                "session header id missing; anonymous filename fallback used".to_string(),
            );
            (bare_id, cwd, created_at, fallback, warning)
        }
        HeaderRead::Fallback { reason } => {
            let bare_id = filename_session_id(path).ok_or_else(|| {
                FileImportError::Malformed("session header is missing and filename is empty".into())
            })?;
            (bare_id, None, None, true, Some(reason))
        }
    };

    let session_id = namespaced_session_id(agent_id, &bare_session_id).ok_or_else(|| {
        FileImportError::Malformed("agent ID or session ID is empty after normalization".into())
    })?;

    let mut warnings =
        vec!["OpenClaw session JSONL has no locally validated token/cost usage fields".to_string()];
    if let Some(warning) = header_warning {
        warnings.push(warning);
    }

    let source_path = path.to_path_buf();
    let native = native_metadata.get(&bare_session_id);
    let metadata = SessionNodeMetadata {
        title: native.and_then(|value| value.display_name.clone()),
        project_dir: cwd.or_else(|| native.and_then(|value| value.cwd.clone())),
        source_path: Some(source_path.to_string_lossy().to_string()),
        created_at,
        last_active_at: None,
        last_synced_at: current_unix_ms(),
    };

    // Parent-looking fields (parentId, parentSessionId, etc.) are intentionally
    // ignored.  The native displayName is metadata only; no local fixture
    // proves a parent relation, so the only safe relation is standalone.
    let mut claim = SessionRelationClaim::standalone(OPENCLAW_APP_TYPE, session_id.clone());
    claim.metadata = metadata;

    let mut usage = UsageMeasure::unavailable(warnings[0].clone());
    usage.warnings = warnings;

    Ok(OpenClawSessionRecord {
        agent_id: agent_id.to_string(),
        bare_session_id,
        session_id,
        source_path,
        claim,
        usage,
        used_filename_fallback,
    })
}

/// Read only OpenClaw's native `sessions.json` display names and cwd. The
/// adapter deliberately does not fall back to message text or a directory
/// basename when the native index is absent.
fn load_native_session_metadata(
    sessions_dir: &Path,
) -> HashMap<String, OpenClawNativeSessionMetadata> {
    let content = match fs::read_to_string(sessions_dir.join("sessions.json")) {
        Ok(content) => content,
        Err(_) => return HashMap::new(),
    };
    let Ok(index) = serde_json::from_str::<serde_json::Map<String, Value>>(&content) else {
        return HashMap::new();
    };
    let mut metadata = HashMap::new();
    for entry in index.values() {
        let Some(session_id) = entry
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let display_name = entry
            .get("displayName")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let cwd = entry
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        if display_name.is_some() || cwd.is_some() {
            metadata.insert(
                session_id.to_string(),
                OpenClawNativeSessionMetadata { display_name, cwd },
            );
        }
    }
    metadata
}

#[derive(Debug)]
enum HeaderRead {
    Session {
        id: Option<String>,
        cwd: Option<String>,
        created_at: Option<i64>,
    },
    Fallback {
        reason: String,
    },
}

/// Read only enough of a JSONL stream to locate the first session header.
/// Once a message/body event is encountered, the remainder is not inspected.
/// This prevents assistant payloads that happen to contain `usage` or `cost`
/// keys from being mistaken for a proven usage schema.
fn read_session_header(path: &Path) -> Result<HeaderRead, FileImportError> {
    let file = File::open(path).map_err(|error| FileImportError::Unreadable(error.to_string()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut saw_non_empty = false;

    for _ in 0..64 {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .map_err(|error| FileImportError::Unreadable(error.to_string()))?;
        if bytes_read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        saw_non_empty = true;
        let value: Value = serde_json::from_str(trimmed)
            .map_err(|error| FileImportError::Malformed(error.to_string()))?;
        match value.get("type").and_then(Value::as_str) {
            Some("session") => {
                let id = value
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
                let cwd = value
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
                let created_at = value.get("timestamp").and_then(timestamp_to_ms);
                return Ok(HeaderRead::Session {
                    id,
                    cwd,
                    created_at,
                });
            }
            // Do not parse message bodies or search them for usage-like keys.
            Some("message") | Some("assistant") | Some("user") | Some("toolResult") => {
                return Ok(HeaderRead::Fallback {
                    reason: "session header not found before message body; filename fallback used"
                        .to_string(),
                });
            }
            _ => continue,
        }
    }

    Ok(HeaderRead::Fallback {
        reason: if saw_non_empty {
            "session header missing or unknown event format; filename fallback used".to_string()
        } else {
            "empty session file; filename fallback used".to_string()
        },
    })
}

fn filename_session_id(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .map(ToOwned::to_owned)
}

fn timestamp_to_ms(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        // OpenClaw has emitted both second and millisecond numeric timestamps
        // across versions; only apply the unambiguous magnitude conversion.
        return if number.unsigned_abs() < 100_000_000_000 {
            number.checked_mul(1_000)
        } else {
            Some(number)
        };
    }
    let text = value.as_str()?.trim();
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|timestamp| timestamp.timestamp_millis())
}

fn current_unix_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn path_component(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

fn sorted_child_directories(parent: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(parent)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn sorted_jsonl_files(parent: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(parent)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_session_usage::RelationClaim;
    use std::fs;
    use tempfile::tempdir;

    fn write_session(root: &Path, agent: &str, file_name: &str, content: &str) -> PathBuf {
        let sessions_dir = root.join("agents").join(agent).join("sessions");
        fs::create_dir_all(&sessions_dir).expect("create anonymous sessions directory");
        let path = sessions_dir.join(file_name);
        fs::write(&path, content).expect("write anonymous session fixture");
        path
    }

    fn session_header(id: &str, cwd: &str) -> String {
        format!(
            "{{\"type\":\"session\",\"id\":\"{id}\",\"cwd\":\"{cwd}\",\"timestamp\":\"2026-08-13T10:00:00Z\"}}\n"
        )
    }

    #[test]
    fn two_agents_with_same_bare_id_get_distinct_namespaced_nodes() {
        let temp = tempdir().expect("tempdir");
        write_session(
            temp.path(),
            "alpha",
            "shared.jsonl",
            &session_header("shared", "/a"),
        );
        write_session(
            temp.path(),
            "beta",
            "shared.jsonl",
            &session_header("shared", "/b"),
        );

        let result = scan_openclaw_sessions(temp.path());
        let mut ids = result
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        assert_eq!(ids, vec!["alpha:shared", "beta:shared"]);
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors
        );
    }

    #[test]
    fn sessions_index_display_names_never_become_parent_evidence() {
        let temp = tempdir().expect("tempdir");
        let path = write_session(
            temp.path(),
            "main",
            "child.jsonl",
            &format!(
                "{{\"type\":\"session\",\"id\":\"child\",\"timestamp\":\"2026-08-13T10:00:00Z\",\"parentSessionId\":\"root\"}}\n"
            ),
        );
        fs::write(
            path.parent()
                .expect("sessions parent")
                .join("sessions.json"),
            r#"{
                "root": {"sessionId":"root", "displayName":"Root task"},
                "child": {"sessionId":"child", "displayName":"Child task", "cwd":"/index-project"}
            }"#,
        )
        .expect("write display-name index");

        let result = scan_openclaw_sessions(temp.path());
        let session = result.sessions.first().expect("session fixture");
        assert!(matches!(session.claim.relation, RelationClaim::Standalone));
        assert_eq!(session.claim.app_type, OPENCLAW_APP_TYPE);
        assert_eq!(session.claim.session_id, "main:child");
        assert_eq!(session.claim.metadata.title.as_deref(), Some("Child task"));
        assert_eq!(
            session.claim.metadata.project_dir.as_deref(),
            Some("/index-project")
        );
    }

    #[test]
    fn missing_root_is_a_noop() {
        let temp = tempdir().expect("tempdir");
        let missing = temp.path().join("does-not-exist");
        let result = scan_openclaw_sessions(&missing);
        assert!(result.sessions.is_empty());
        assert_eq!(result.files_scanned, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn header_id_is_preferred_and_filename_is_safe_fixture_fallback() {
        let temp = tempdir().expect("tempdir");
        write_session(
            temp.path(),
            "agent",
            "filename-id.jsonl",
            &session_header("header-id", "/project"),
        );
        write_session(
            temp.path(),
            "agent",
            "fallback-id.jsonl",
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"anonymous fixture\"}}\n",
        );

        let result = scan_openclaw_sessions(temp.path());
        let header = result
            .sessions
            .iter()
            .find(|session| session.bare_session_id == "header-id")
            .expect("header session");
        assert_eq!(header.session_id, "agent:header-id");
        assert!(!header.used_filename_fallback);

        let fallback = result
            .sessions
            .iter()
            .find(|session| session.bare_session_id == "fallback-id")
            .expect("filename fallback session");
        assert_eq!(fallback.session_id, "agent:fallback-id");
        assert!(fallback.used_filename_fallback);
    }

    #[test]
    fn assistant_usage_and_cost_are_unavailable_and_do_not_create_a_bucket() {
        let temp = tempdir().expect("tempdir");
        write_session(
            temp.path(),
            "agent",
            "usage.jsonl",
            &format!(
                "{}{{\"type\":\"message\",\"message\":{{\"role\":\"assistant\",\"usage\":{{\"input_tokens\":12,\"output_tokens\":7,\"cost\":\"0.42\"}}}}}}\n",
                session_header("usage", "/project")
            ),
        );

        let result = scan_openclaw_sessions(temp.path());
        let session = result.sessions.first().expect("usage fixture");
        assert_eq!(
            session.usage.precision,
            crate::services::agent_session_usage::UsagePrecision::Unavailable
        );
        assert_eq!(session.usage.total_tokens(), None);
        assert_eq!(session.usage.total_cost_usd, None);
        assert!(session.usage.partial);
        assert!(session
            .usage
            .warnings
            .iter()
            .any(|warning| warning.contains("token/cost")));
    }

    #[test]
    fn optional_parent_fields_are_ignored_without_fixture_proof() {
        let temp = tempdir().expect("tempdir");
        write_session(
            temp.path(),
            "agent",
            "session.jsonl",
            "{\"type\":\"session\",\"id\":\"session\",\"parentId\":\"root\",\"parentSessionId\":\"root\",\"cwd\":\"/project\"}\n",
        );
        let result = scan_openclaw_sessions(temp.path());
        let session = result.sessions.first().expect("parent-looking fixture");
        assert!(matches!(session.claim.relation, RelationClaim::Standalone));
    }

    #[test]
    fn malformed_file_is_isolated_from_another_valid_session() {
        let temp = tempdir().expect("tempdir");
        write_session(temp.path(), "agent", "broken.jsonl", "{not-json\n");
        write_session(
            temp.path(),
            "agent",
            "valid.jsonl",
            &session_header("valid", "/project"),
        );

        let result = scan_openclaw_sessions(temp.path());
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.sessions[0].session_id, "agent:valid");
        assert_eq!(result.files_scanned, 2);
        assert_eq!(result.files_skipped, 1);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("broken.jsonl")));
    }

    #[test]
    fn unknown_version_header_is_read_without_assuming_usage_or_parentage() {
        let temp = tempdir().expect("tempdir");
        write_session(
            temp.path(),
            "agent",
            "future.jsonl",
            "{\"type\":\"session\",\"version\":9999,\"id\":\"future\",\"cwd\":\"/project\"}\n",
        );
        let result = scan_openclaw_sessions(temp.path());
        let session = result.sessions.first().expect("future version fixture");
        assert_eq!(session.session_id, "agent:future");
        assert!(matches!(session.claim.relation, RelationClaim::Standalone));
        assert_eq!(session.usage.total_tokens(), None);
    }
}
