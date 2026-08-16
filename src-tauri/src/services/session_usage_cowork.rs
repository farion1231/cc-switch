//! Read-only Claude Desktop / Cowork transcript usage importer.
//!
//! Cowork stores Claude-compatible JSONL transcripts below a platform-specific
//! `local-agent-mode-sessions` directory.  This adapter deliberately parses
//! anonymous transcript fixtures through a small, evidence-backed boundary:
//! the real transcript `sessionId` is the session identity, `assistant` rows
//! are request-exact usage, and only a `sessionId/subagents/` path with a
//! discovered parent is accepted as structural child evidence.  Group files,
//! resident-agent labels, nearby timestamps, and workspace names never create
//! a parent relation.
//!
//! The adapter writes only the canonical session nodes and complete usage
//! buckets.  It does not write proxy request rows or perform central duplicate
//! arbitration.  [`CoworkSourceIdentity`] gives T12 a stable
//! `cowork:<session-id>:<message-id>` source ID and exact token/time metadata
//! for the final Desktop-gateway decision.

use crate::database::Database;
use crate::error::AppError;
use crate::services::agent_session_usage::{
    normalize_session_relations, write_agent_session_node, write_agent_session_usage_rollup,
    NormalizedUsageRollup, RelationClaim, RelationConfidence, RequestCountSemantics,
    SessionNodeMetadata, SessionRelationClaim, TimeSemantics, UsagePrecision,
};
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Canonical app entry used by the Desktop gateway and all Cowork nodes.
pub const COWORK_APP_TYPE: &str = "claude-desktop";

/// Durable source dimension.  This intentionally differs from Claude Code's
/// `session_log` so later source arbitration can reason about the actual app
/// entry instead of folding everything into `claude` too early.
pub const COWORK_DATA_SOURCE: &str = "cowork_session";

/// Platform selector used by deterministic path-discovery tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoworkPlatform {
    Windows,
    MacOs,
    Linux,
}

/// Exact source identity passed to the central T12 source arbitrator.
///
/// A transcript message ID is stable across repeated syncs and survives file
/// moves.  Token components are retained as matching metadata; no proxy row is
/// silently discarded by this adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoworkSourceIdentity {
    pub source_id: String,
    pub app_type: String,
    pub data_source: String,
    pub session_id: String,
    pub message_id: String,
    pub model: String,
    pub event_at: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub source_path: String,
}

/// Adapter result.  `imported` counts unique assistant usage rows; durable
/// buckets and source identities are reported separately so callers do not
/// mistake a bucket count for a request count.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoworkSyncResult {
    pub imported: u32,
    pub skipped: u32,
    pub files_scanned: u32,
    pub nodes_written: u32,
    pub buckets_written: u32,
    pub source_ids: Vec<String>,
    pub source_records: Vec<CoworkSourceIdentity>,
    pub errors: Vec<String>,
}

impl CoworkSyncResult {
    fn merge(&mut self, other: Self) {
        self.imported = self.imported.saturating_add(other.imported);
        self.skipped = self.skipped.saturating_add(other.skipped);
        self.files_scanned = self.files_scanned.saturating_add(other.files_scanned);
        self.nodes_written = self.nodes_written.saturating_add(other.nodes_written);
        self.buckets_written = self.buckets_written.saturating_add(other.buckets_written);
        self.source_ids.extend(other.source_ids);
        self.source_records.extend(other.source_records);
        self.errors.extend(other.errors);
    }
}

/// Stable source key used for T12's cross-source dedup decision.
pub fn cowork_source_id(session_id: &str, message_id: &str) -> String {
    format!("cowork:{session_id}:{message_id}")
}

/// Compute the platform-specific Cowork root from explicit test parameters.
///
/// Windows uses `%APPDATA%`, macOS uses `~/Library/Application Support`, and
/// Linux follows Electron's `XDG_CONFIG_HOME` convention with a `~/.config`
/// fallback.  `None` means the corresponding environment value was unavailable
/// and therefore no root is returned.
pub fn cowork_root_for_platform(
    platform: CoworkPlatform,
    app_data: Option<&Path>,
    home: Option<&Path>,
    xdg_config_home: Option<&Path>,
) -> Option<PathBuf> {
    match platform {
        CoworkPlatform::Windows => {
            app_data.map(|base| base.join("Claude").join("local-agent-mode-sessions"))
        }
        CoworkPlatform::MacOs => home.map(|base| {
            base.join("Library")
                .join("Application Support")
                .join("Claude")
                .join("local-agent-mode-sessions")
        }),
        CoworkPlatform::Linux => {
            let config = xdg_config_home
                .map(PathBuf::from)
                .or_else(|| home.map(|base| base.join(".config")))?;
            Some(config.join("Claude").join("local-agent-mode-sessions"))
        }
    }
}

/// Discover the production root without touching it.  An explicit
/// `CC_SWITCH_COWORK_ROOT` override is useful for managed installs and keeps
/// tests independent from a developer's real Desktop data.
pub fn discover_cowork_roots() -> Vec<PathBuf> {
    if let Some(override_root) = env::var_os("CC_SWITCH_COWORK_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return vec![override_root];
    }

    let app_data = env::var_os("APPDATA").map(PathBuf::from);
    let home = env::var_os("HOME").map(PathBuf::from);
    let xdg_config_home = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let root = if cfg!(target_os = "windows") {
        cowork_root_for_platform(CoworkPlatform::Windows, app_data.as_deref(), None, None)
    } else if cfg!(target_os = "macos") {
        cowork_root_for_platform(CoworkPlatform::MacOs, None, home.as_deref(), None)
    } else if cfg!(target_os = "linux") {
        cowork_root_for_platform(
            CoworkPlatform::Linux,
            None,
            home.as_deref(),
            xdg_config_home.as_deref(),
        )
    } else {
        None
    };

    root.into_iter().collect()
}

/// Locate Cowork transcript JSONL files under the documented local-session
/// layout.  `audit.jsonl` is an audit/streaming copy and is always excluded.
pub fn discover_cowork_transcript_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_local_transcripts(root, &mut files);
    files.sort();
    files.dedup();
    files
}

fn collect_local_transcripts(path: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let child = entry.path();
        if !child.is_dir() {
            continue;
        }

        let is_local_group = child
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("local_"));
        let projects = child.join(".claude").join("projects");
        if is_local_group && projects.is_dir() {
            collect_jsonl_files(&projects, files);
        } else {
            collect_local_transcripts(&child, files);
        }
    }
}

fn collect_jsonl_files(path: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let child = entry.path();
        if child.is_dir() {
            collect_jsonl_files(&child, files);
            continue;
        }
        let is_jsonl = child
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"));
        let is_audit = child
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("audit.jsonl"));
        if is_jsonl && !is_audit {
            files.push(child);
        }
    }
}

#[derive(Debug, Clone)]
struct UsageEvent {
    message_id: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    event_at: i64,
    date: String,
}

impl UsageEvent {
    fn merge_snapshot(self, other: Self) -> Self {
        // Claude-style transcript streams may repeat one message ID while
        // output/cache counters grow.  Taking the component-wise maximum
        // counts that assistant row once without losing a later cache-only
        // snapshot.
        Self {
            message_id: self.message_id,
            model: if other.model == "unknown" {
                self.model
            } else {
                other.model
            },
            input_tokens: self.input_tokens.max(other.input_tokens),
            output_tokens: self.output_tokens.max(other.output_tokens),
            cache_read_tokens: self.cache_read_tokens.max(other.cache_read_tokens),
            cache_creation_tokens: self.cache_creation_tokens.max(other.cache_creation_tokens),
            event_at: self.event_at.max(other.event_at),
            date: if other.event_at >= self.event_at {
                other.date
            } else {
                self.date
            },
        }
    }
}

#[derive(Debug, Clone)]
struct TranscriptRecord {
    path: PathBuf,
    session_id: String,
    parent_hint: Option<String>,
    project_dir: Option<String>,
    events: Vec<UsageEvent>,
    skipped_rows: u32,
}

#[derive(Debug, Clone)]
struct SessionAggregate {
    session_id: String,
    source_path: PathBuf,
    project_dir: Option<String>,
    parent_hints: HashSet<String>,
    events: HashMap<String, UsageEvent>,
}

#[derive(Debug, Clone, Default)]
struct BucketAccumulator {
    request_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    first_event_at: Option<i64>,
    last_event_at: Option<i64>,
}

impl BucketAccumulator {
    fn add(&mut self, event: &UsageEvent) {
        self.request_count = self.request_count.saturating_add(1);
        self.input_tokens = self.input_tokens.saturating_add(event.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(event.output_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(event.cache_read_tokens);
        self.cache_creation_tokens = self
            .cache_creation_tokens
            .saturating_add(event.cache_creation_tokens);
        self.first_event_at = Some(
            self.first_event_at
                .map_or(event.event_at, |first| first.min(event.event_at)),
        );
        self.last_event_at = Some(
            self.last_event_at
                .map_or(event.event_at, |last| last.max(event.event_at)),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct BucketKey {
    date: String,
    session_id: String,
    model: String,
}

/// Sync all currently discovered roots.  Missing roots are intentionally a
/// no-op; one unreadable root is reported without blocking other roots.
pub fn sync_cowork_usage(db: &Database) -> Result<CoworkSyncResult, AppError> {
    sync_cowork_usage_from_roots(db, &discover_cowork_roots())
}

/// Sync explicit roots.  This is the fixture seam used by tests and by callers
/// that have already resolved a managed profile directory.
pub fn sync_cowork_usage_from_roots(
    db: &Database,
    roots: &[PathBuf],
) -> Result<CoworkSyncResult, AppError> {
    let mut aggregate = CoworkSyncResult::default();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let result = sync_single_root(db, root)?;
        aggregate.merge(result);
    }
    aggregate.source_ids.sort();
    aggregate.source_ids.dedup();
    aggregate.source_records.sort_by(|left, right| {
        left.source_id
            .cmp(&right.source_id)
            .then_with(|| left.source_path.cmp(&right.source_path))
    });
    aggregate
        .source_records
        .dedup_by(|left, right| left.source_id == right.source_id);
    Ok(aggregate)
}

fn sync_single_root(db: &Database, root: &Path) -> Result<CoworkSyncResult, AppError> {
    let paths = discover_cowork_transcript_files(root);
    let mut result = CoworkSyncResult {
        files_scanned: paths.len() as u32,
        ..CoworkSyncResult::default()
    };
    if paths.is_empty() {
        return Ok(result);
    }

    let mut aggregates: HashMap<String, SessionAggregate> = HashMap::new();
    for path in paths {
        let record = match parse_transcript(&path) {
            Ok(Some(record)) => record,
            Ok(None) => {
                // Non-transcript JSONL such as a journal has no session ID
                // and no assistant usage; it must not become a fake session
                // named after the filename.
                result.skipped = result.skipped.saturating_add(1);
                continue;
            }
            Err(error) => {
                result.errors.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        result.skipped = result.skipped.saturating_add(record.skipped_rows);
        let entry = aggregates
            .entry(record.session_id.clone())
            .or_insert_with(|| SessionAggregate {
                session_id: record.session_id.clone(),
                source_path: record.path.clone(),
                project_dir: record.project_dir.clone(),
                parent_hints: HashSet::new(),
                events: HashMap::new(),
            });
        if entry.project_dir.is_none() {
            entry.project_dir = record.project_dir.clone();
        }
        // Prefer a root transcript path as the node source path when duplicate
        // copies of the same session are present.
        if entry.source_path.to_string_lossy().contains("subagents")
            && !record.path.to_string_lossy().contains("subagents")
        {
            entry.source_path = record.path.clone();
        }
        if let Some(parent) = record.parent_hint {
            entry.parent_hints.insert(parent);
        }
        for event in record.events {
            match entry.events.remove(&event.message_id) {
                Some(previous) => {
                    entry
                        .events
                        .insert(event.message_id.clone(), previous.merge_snapshot(event));
                    result.skipped = result.skipped.saturating_add(1);
                }
                None => {
                    entry.events.insert(event.message_id.clone(), event);
                }
            }
        }
    }

    let mut sessions: Vec<SessionAggregate> = aggregates.into_values().collect();
    sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    let known_sessions: HashSet<String> = sessions
        .iter()
        .map(|session| session.session_id.clone())
        .collect();
    let child_parent_ids: HashSet<String> = sessions
        .iter()
        .flat_map(|session| session.parent_hints.iter())
        .filter(|parent| known_sessions.contains(*parent))
        .cloned()
        .collect();

    let mut claims = Vec::with_capacity(sessions.len());
    for session in &sessions {
        let metadata = session_metadata(session);
        let mut known_parents: Vec<&String> = session
            .parent_hints
            .iter()
            .filter(|parent| known_sessions.contains(*parent))
            .collect();
        known_parents.sort();
        let has_unknown_parent_hint = session
            .parent_hints
            .iter()
            .any(|parent| !known_sessions.contains(parent));
        let claim = if has_unknown_parent_hint {
            // Mixed structural evidence (one path names a known parent while
            // another names a missing parent) must fail closed as unknown.
            SessionRelationClaim {
                app_type: COWORK_APP_TYPE.to_string(),
                session_id: session.session_id.clone(),
                relation: RelationClaim::Unknown,
                metadata,
            }
        } else if known_parents.len() > 1 {
            // Let the canonical normalizer mark conflicting structural claims
            // as conflict rather than choosing one parent by path order.
            let first = SessionRelationClaim::child(
                COWORK_APP_TYPE,
                session.session_id.clone(),
                known_parents[0].to_string(),
                RelationConfidence::Structural,
            );
            claims.push(with_metadata(first, metadata.clone()));
            SessionRelationClaim::child(
                COWORK_APP_TYPE,
                session.session_id.clone(),
                known_parents[1].to_string(),
                RelationConfidence::Structural,
            )
        } else if let Some(parent) = known_parents.first() {
            SessionRelationClaim::child(
                COWORK_APP_TYPE,
                session.session_id.clone(),
                parent.to_string(),
                RelationConfidence::Structural,
            )
        } else if !session.parent_hints.is_empty() {
            // A subagents path without a discovered parent is not enough to
            // assign ownership.  Keep the node self-rooted and unavailable.
            SessionRelationClaim {
                app_type: COWORK_APP_TYPE.to_string(),
                session_id: session.session_id.clone(),
                relation: RelationClaim::Unknown,
                metadata,
            }
        } else if child_parent_ids.contains(&session.session_id) {
            SessionRelationClaim {
                app_type: COWORK_APP_TYPE.to_string(),
                session_id: session.session_id.clone(),
                relation: RelationClaim::Root,
                metadata,
            }
        } else {
            SessionRelationClaim {
                app_type: COWORK_APP_TYPE.to_string(),
                session_id: session.session_id.clone(),
                relation: RelationClaim::Standalone,
                metadata,
            }
        };
        claims.push(with_metadata(claim, session_metadata(session)));
    }

    // Nodes are normalized as one graph before any bucket crosses the DAO
    // boundary.  This prevents a per-message write from temporarily claiming
    // child ownership or overwriting a root relation.
    let normalized_nodes = normalize_session_relations(&claims)?;
    for node in normalized_nodes {
        write_agent_session_node(db, &node)?;
        result.nodes_written = result.nodes_written.saturating_add(1);
    }

    let mut buckets: BTreeMap<BucketKey, BucketAccumulator> = BTreeMap::new();
    for session in &sessions {
        let mut events: Vec<&UsageEvent> = session.events.values().collect();
        events.sort_by(|left, right| left.message_id.cmp(&right.message_id));
        for event in events {
            result.imported = result.imported.saturating_add(1);
            let source_id = cowork_source_id(&session.session_id, &event.message_id);
            result.source_ids.push(source_id.clone());
            result.source_records.push(CoworkSourceIdentity {
                source_id,
                app_type: COWORK_APP_TYPE.to_string(),
                data_source: COWORK_DATA_SOURCE.to_string(),
                session_id: session.session_id.clone(),
                message_id: event.message_id.clone(),
                model: event.model.clone(),
                event_at: event.event_at,
                input_tokens: event.input_tokens,
                output_tokens: event.output_tokens,
                cache_read_tokens: event.cache_read_tokens,
                cache_creation_tokens: event.cache_creation_tokens,
                source_path: session.source_path.to_string_lossy().to_string(),
            });
            buckets
                .entry(BucketKey {
                    date: event.date.clone(),
                    session_id: session.session_id.clone(),
                    model: event.model.clone(),
                })
                .or_default()
                .add(event);
        }
    }

    // Every complete date/session/model key is aggregated first; only then do
    // we call the replacing DAO bridge.  Unknown cost stays NULL by design.
    for (key, bucket) in buckets {
        let rollup = NormalizedUsageRollup {
            date: key.date,
            app_type: COWORK_APP_TYPE.to_string(),
            session_id: key.session_id,
            provider_id: COWORK_DATA_SOURCE.to_string(),
            model: key.model.clone(),
            request_model: key.model.clone(),
            pricing_model: key.model,
            data_source: COWORK_DATA_SOURCE.to_string(),
            precision: UsagePrecision::RequestExact,
            time_semantics: TimeSemantics::EventTime,
            request_count_semantics: RequestCountSemantics::AssistantMessage,
            request_count: Some(bucket.request_count),
            input_tokens: Some(bucket.input_tokens),
            output_tokens: Some(bucket.output_tokens),
            cache_read_tokens: Some(bucket.cache_read_tokens),
            cache_creation_tokens: Some(bucket.cache_creation_tokens),
            total_cost_usd: None,
            first_event_at: bucket.first_event_at,
            last_event_at: bucket.last_event_at,
        };
        write_agent_session_usage_rollup(db, &rollup)?;
        result.buckets_written = result.buckets_written.saturating_add(1);
    }

    Ok(result)
}

fn with_metadata(
    mut claim: SessionRelationClaim,
    metadata: SessionNodeMetadata,
) -> SessionRelationClaim {
    claim.metadata = metadata;
    claim
}

fn session_metadata(session: &SessionAggregate) -> SessionNodeMetadata {
    let mut timestamps = session
        .events
        .values()
        .map(|event| event.event_at)
        .collect::<Vec<_>>();
    timestamps.sort_unstable();
    SessionNodeMetadata {
        title: None,
        project_dir: session.project_dir.clone(),
        source_path: Some(session.source_path.to_string_lossy().to_string()),
        created_at: timestamps.first().copied(),
        last_active_at: timestamps.last().copied(),
        // This is source event time, never the current wall clock.  The
        // adapter must not fabricate an event timestamp when a fixture omits
        // one.
        last_synced_at: timestamps.last().copied().unwrap_or(0),
    }
}

fn parse_transcript(path: &Path) -> Result<Option<TranscriptRecord>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let reader = BufReader::new(file);
    let parent_hint = parent_hint_from_path(path);
    let project_dir = project_dir_from_path(path);
    let mut session_id: Option<String> = None;
    let mut events: HashMap<String, UsageEvent> = HashMap::new();
    let mut skipped_rows = 0u32;

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if session_id.is_none() {
            session_id = value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToOwned::to_owned);
        }
        if session_id.is_none() {
            session_id = value
                .get("message")
                .and_then(|message| message.get("sessionId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToOwned::to_owned);
        }

        let is_assistant = value.get("type").and_then(Value::as_str) == Some("assistant");
        if !is_assistant {
            continue;
        }
        let message = match value.get("message") {
            Some(message) => message,
            None => {
                skipped_rows = skipped_rows.saturating_add(1);
                continue;
            }
        };
        let message_id = match message
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            Some(id) => id.to_string(),
            None => {
                skipped_rows = skipped_rows.saturating_add(1);
                continue;
            }
        };
        let usage = match message.get("usage") {
            Some(usage) => usage,
            None => {
                skipped_rows = skipped_rows.saturating_add(1);
                continue;
            }
        };
        let timestamp = match parse_event_timestamp(value.get("timestamp")) {
            Some(timestamp) => timestamp,
            None => {
                skipped_rows = skipped_rows.saturating_add(1);
                continue;
            }
        };
        let (input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens) =
            match token_components(usage) {
                Some(components) => components,
                // A missing/negative token component is unknown, not zero.
                // Keep it out of durable buckets until the source contract is
                // proven by a future fixture.
                None => {
                    skipped_rows = skipped_rows.saturating_add(1);
                    continue;
                }
            };
        let event = UsageEvent {
            message_id: message_id.clone(),
            model: message
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .unwrap_or("unknown")
                .to_string(),
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            event_at: timestamp.0,
            date: timestamp.1,
        };
        match events.remove(&message_id) {
            Some(previous) => {
                events.insert(message_id, previous.merge_snapshot(event));
            }
            None => {
                events.insert(message_id, event);
            }
        }
    }

    if session_id.is_none() && events.is_empty() {
        return Ok(None);
    }
    let session_id = session_id
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| "transcript has no sessionId or usable filename".to_string())?;
    let mut events: Vec<UsageEvent> = events.into_values().collect();
    events.sort_by(|left, right| left.message_id.cmp(&right.message_id));
    Ok(Some(TranscriptRecord {
        path: path.to_path_buf(),
        session_id,
        parent_hint,
        project_dir,
        events,
        skipped_rows,
    }))
}

fn token_components(usage: &Value) -> Option<(i64, i64, i64, i64)> {
    let component = |key: &str| {
        usage
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|value| i64::try_from(value).ok())
    };
    Some((
        component("input_tokens")?,
        component("output_tokens")?,
        component("cache_read_input_tokens")?,
        component("cache_creation_input_tokens")?,
    ))
}

fn parse_event_timestamp(value: Option<&Value>) -> Option<(i64, String)> {
    let value = value?.as_str()?.trim();
    let parsed: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(value).ok()?;
    let utc = parsed.with_timezone(&Utc);
    Some((parsed.timestamp(), utc.date_naive().to_string()))
}

fn parent_hint_from_path(path: &Path) -> Option<String> {
    let components: Vec<String> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(ToOwned::to_owned))
        .collect();
    components.windows(2).find_map(|window| {
        if window[1].eq_ignore_ascii_case("subagents") {
            let parent = window[0].trim();
            (!parent.is_empty()).then(|| parent.to_string())
        } else {
            None
        }
    })
}

fn project_dir_from_path(path: &Path) -> Option<String> {
    let components: Vec<String> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(ToOwned::to_owned))
        .collect();
    components.windows(2).find_map(|window| {
        if window[0].eq_ignore_ascii_case("projects") {
            (!window[1].is_empty()).then(|| window[1].clone())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;
    use tempfile::tempdir;

    fn write_fixture(root: &Path, include_child: bool) -> (PathBuf, PathBuf) {
        let group = root.join("workspace").join("group");
        let project = group
            .join("local_fixture")
            .join(".claude")
            .join("projects")
            .join("demo-project");
        fs::create_dir_all(&project).expect("create project");
        fs::write(
            group.join("local_fixture.json"),
            r#"{"residentAgent":"nearby-agent","parentSessionId":"must-not-infer"}"#,
        )
        .expect("write group metadata");

        let root_path = project.join("root-session.jsonl");
        fs::write(
            &root_path,
            concat!(
                r#"{"sessionId":"root-session","type":"user","timestamp":"2026-08-13T10:00:00Z"}"#,
                "\n",
                r#"{"sessionId":"root-session","type":"assistant","message":{"id":"msg-cache","model":"claude-sonnet","usage":{"input_tokens":4,"output_tokens":0,"cache_read_input_tokens":9,"cache_creation_input_tokens":0}},"timestamp":"2026-08-13T10:01:00Z"}"#,
                "\n",
                r#"{"sessionId":"root-session","type":"assistant","message":{"id":"msg-zero","model":"claude-sonnet","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-13T10:02:00Z"}"#,
                "\n",
                r#"{"sessionId":"root-session","type":"assistant","message":{"id":"msg-no-time","model":"claude-sonnet","usage":{"input_tokens":99,"output_tokens":1}},"timestamp":null}"#,
                "\n",
            ),
        )
        .expect("write root transcript");

        fs::write(
            project.join("audit.jsonl"),
            r#"{"sessionId":"root-session","type":"assistant","message":{"id":"audit-only","model":"claude-sonnet","usage":{"input_tokens":1000,"output_tokens":1000}},"timestamp":"2026-08-13T10:03:00Z"}"#,
        )
        .expect("write audit copy");

        let child_path = project
            .join("root-session")
            .join("subagents")
            .join("child-session.jsonl");
        if include_child {
            fs::create_dir_all(child_path.parent().unwrap()).expect("create child dir");
            fs::write(
                &child_path,
                r#"{"sessionId":"child-session","type":"assistant","message":{"id":"child-msg","model":"claude-haiku","usage":{"input_tokens":2,"output_tokens":3,"cache_read_input_tokens":0,"cache_creation_input_tokens":1}},"timestamp":"2026-08-13T10:04:00Z"}"#,
            )
            .expect("write child transcript");
        }
        (root_path, child_path)
    }

    #[test]
    fn platform_discovery_uses_parameterized_windows_macos_linux_paths() {
        let temp = tempdir().expect("tempdir");
        let app_data = temp.path().join("appdata");
        let home = temp.path().join("home");
        let xdg = temp.path().join("xdg");
        assert_eq!(
            cowork_root_for_platform(CoworkPlatform::Windows, Some(&app_data), None, None,),
            Some(app_data.join("Claude").join("local-agent-mode-sessions"))
        );
        assert_eq!(
            cowork_root_for_platform(CoworkPlatform::MacOs, None, Some(&home), None),
            Some(
                home.join("Library")
                    .join("Application Support")
                    .join("Claude")
                    .join("local-agent-mode-sessions")
            )
        );
        assert_eq!(
            cowork_root_for_platform(CoworkPlatform::Linux, None, Some(&home), Some(&xdg)),
            Some(xdg.join("Claude").join("local-agent-mode-sessions"))
        );
        assert_eq!(
            cowork_root_for_platform(CoworkPlatform::Linux, None, Some(&home), None),
            Some(
                home.join(".config")
                    .join("Claude")
                    .join("local-agent-mode-sessions")
            )
        );
    }

    #[test]
    fn missing_root_is_a_noop_and_audit_copy_is_excluded() -> Result<(), AppError> {
        let db = Database::memory()?;
        let missing = tempdir().expect("tempdir").path().join("does-not-exist");
        let empty = sync_cowork_usage_from_roots(&db, &[missing])?;
        assert_eq!(empty, CoworkSyncResult::default());

        let temp = tempdir().expect("tempdir");
        let (root_path, _) = write_fixture(temp.path(), false);
        let result = sync_cowork_usage_from_roots(&db, &[temp.path().to_path_buf()])?;
        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.imported, 2);
        assert_eq!(result.buckets_written, 1);
        assert!(result
            .source_ids
            .iter()
            .all(|id| !id.contains("audit-only")));
        assert!(root_path.exists());
        Ok(())
    }

    #[test]
    fn nested_child_is_structural_but_resident_metadata_stays_standalone() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let (_root_path, child_path) = write_fixture(temp.path(), true);
        let result = sync_cowork_usage_from_roots(&db, &[temp.path().to_path_buf()])?;
        assert_eq!(result.files_scanned, 2);
        assert_eq!(result.imported, 3);

        let conn = lock_conn!(db.conn);
        let root: (String, Option<String>, String, String) = conn.query_row(
            "SELECT node_kind, parent_session_id, root_session_id, app_type
             FROM agent_session_nodes WHERE session_id = 'root-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(
            root,
            (
                "root".into(),
                None,
                "root-session".into(),
                "claude-desktop".into()
            )
        );
        let child: (String, Option<String>, String) = conn.query_row(
            "SELECT node_kind, parent_session_id, root_session_id
             FROM agent_session_nodes WHERE session_id = 'child-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(
            child,
            (
                "child".into(),
                Some("root-session".into()),
                "root-session".into()
            )
        );

        let usage: (i64, i64, i64, i64, Option<String>, String, String) = conn.query_row(
            "SELECT request_count, input_tokens, output_tokens, cache_read_tokens,
                    total_cost_usd, data_source, request_count_semantics
             FROM agent_session_usage_rollups
             WHERE session_id = 'root-session'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )?;
        assert_eq!(
            usage,
            (
                2,
                4,
                0,
                9,
                None,
                "cowork_session".into(),
                "assistant_message".into()
            )
        );
        assert!(child_path.exists());
        Ok(())
    }

    #[test]
    fn no_child_evidence_is_self_only_and_nearby_group_fields_do_not_parent() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        write_fixture(temp.path(), false);
        let result = sync_cowork_usage_from_roots(&db, &[temp.path().to_path_buf()])?;
        assert_eq!(result.nodes_written, 1);
        let conn = lock_conn!(db.conn);
        let node: (String, Option<String>, String, String) = conn.query_row(
            "SELECT node_kind, parent_session_id, root_session_id, relation_confidence
             FROM agent_session_nodes WHERE session_id = 'root-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(
            node,
            (
                "standalone".into(),
                None,
                "root-session".into(),
                "unavailable".into()
            )
        );
        Ok(())
    }

    #[test]
    fn source_ids_are_stable_and_unknown_cost_is_null() -> Result<(), AppError> {
        assert_eq!(
            cowork_source_id("session", "message"),
            "cowork:session:message"
        );
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        write_fixture(temp.path(), false);
        let result = sync_cowork_usage_from_roots(&db, &[temp.path().to_path_buf()])?;
        assert!(result
            .source_ids
            .contains(&"cowork:root-session:msg-cache".to_string()));
        assert!(result
            .source_records
            .iter()
            .all(|record| record.app_type == "claude-desktop"
                && record.data_source == "cowork_session"));
        let conn = lock_conn!(db.conn);
        let cost: Option<String> = conn.query_row(
            "SELECT total_cost_usd FROM agent_session_usage_rollups
             WHERE session_id = 'root-session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(cost, None);
        Ok(())
    }

    #[test]
    fn missing_token_component_is_not_fabricated_as_zero() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        fs::write(
            &path,
            r#"{"sessionId":"unknown-token","type":"assistant","message":{"id":"msg-missing-cache","model":"claude-sonnet","usage":{"input_tokens":4,"output_tokens":1}},"timestamp":"2026-08-13T10:00:00Z"}"#,
        )
        .expect("write");
        let parsed = parse_transcript(&path)
            .expect("fixture parses")
            .expect("usage fixture has a fallback session id");
        assert!(parsed.events.is_empty());
        assert_eq!(parsed.skipped_rows, 1);
        Ok(())
    }
}
