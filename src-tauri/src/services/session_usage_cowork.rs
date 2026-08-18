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
//! arbitration. The batch carries a stable `cowork:<session-id>:<message-id>`
//! source ID and exact token/time metadata for the final Desktop-gateway
//! decision.

use crate::database::Database;
use crate::error::AppError;
use crate::services::agent_session_usage::{
    local_usage_date, RelationClaim, RelationConfidence, RequestCountSemantics,
    SessionNodeMetadata, SessionRelationClaim, TimeSemantics, UsagePrecision,
};
use crate::services::session_usage::{get_sync_state, metadata_modified_nanos, SessionSyncResult};
#[cfg(test)]
use crate::services::session_usage_pipeline::{publish_canonical_batch, UsagePublishTarget};
use crate::services::session_usage_pipeline::{
    CanonicalReplaceScope, CanonicalUsageBatch, UsageSourceSpec,
};
use chrono::{DateTime, FixedOffset};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
#[cfg(test)]
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Canonical app entry used by the Desktop gateway and all Cowork nodes.
pub(crate) const COWORK_APP_TYPE: &str = "claude-desktop";

/// Durable source dimension.  This intentionally differs from Claude Code's
/// `session_log` so later source arbitration can reason about the actual app
/// entry instead of folding everything into `claude` too early.
pub(crate) const COWORK_DATA_SOURCE: &str = "cowork_session";

/// Platform selector used by deterministic path-discovery tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoworkPlatform {
    Windows,
    MacOs,
    Linux,
}

/// Parser output for the central usage pipeline. Cowork itself has no raw-row
/// lifecycle, so the integration layer decides proxy ownership before this
/// batch is published. Keeping the batch crate-private prevents source-format
/// helpers from becoming a public storage API.
#[derive(Debug, Default)]
pub(crate) struct CoworkUsageBatch {
    pub result: SessionSyncResult,
    pub canonical_batch: CanonicalUsageBatch,
    pub source_revisions: Vec<CoworkSourceRevision>,
}

/// A transcript revision is recorded only after that source was parsed
/// successfully.  The central publisher persists it in the same transaction
/// as canonical facts, so an interrupted or failed publish is retried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoworkSourceRevision {
    pub file_path: String,
    pub last_modified: i64,
    pub last_offset: i64,
}

/// Compute the platform-specific Cowork root from explicit test parameters.
///
/// Windows uses `%APPDATA%`, macOS uses `~/Library/Application Support`, and
/// Linux follows Electron's `XDG_CONFIG_HOME` convention with a `~/.config`
/// fallback.  `None` means the corresponding environment value was unavailable
/// and therefore no root is returned.
pub(crate) fn cowork_root_for_platform(
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
pub(crate) fn discover_cowork_roots() -> Vec<PathBuf> {
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
pub(crate) fn discover_cowork_transcript_files(root: &Path) -> Vec<PathBuf> {
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
    changed: bool,
}

/// Parse all currently discovered roots that changed since their successful
/// canonical publication.  Metadata is intentionally checked before JSONL
/// parsing so the background sync does not reimport historical Cowork usage
/// every minute when no transcript changed.
pub(crate) fn collect_cowork_usage(db: &Database) -> Result<CoworkUsageBatch, AppError> {
    collect_cowork_usage_from_roots_with_sync_state(db, &discover_cowork_roots())
}

/// Production collector with an explicit root seam for tests.  `last_offset`
/// stores the file length for Cowork because transcripts are rewrite-aware;
/// comparing it alongside mtime catches appends even on filesystems with a
/// coarse modification clock.
pub(crate) fn collect_cowork_usage_from_roots_with_sync_state(
    db: &Database,
    roots: &[PathBuf],
) -> Result<CoworkUsageBatch, AppError> {
    let mut paths = roots
        .iter()
        .filter(|root| root.is_dir())
        .flat_map(|root| discover_cowork_transcript_files(root))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();

    let mut changed_paths = HashSet::new();
    let mut revision_candidates = HashMap::new();
    let mut metadata_errors = Vec::new();
    for path in &paths {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                metadata_errors.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let file_path = path.to_string_lossy().to_string();
        let last_modified = metadata_modified_nanos(&metadata);
        let last_offset = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
        let (saved_modified, saved_offset) = get_sync_state(db, &file_path)?;
        if last_modified != saved_modified || last_offset != saved_offset {
            changed_paths.insert(path.clone());
            revision_candidates.insert(
                file_path.clone(),
                CoworkSourceRevision {
                    file_path,
                    last_modified,
                    last_offset,
                },
            );
        }
    }

    if changed_paths.is_empty() {
        return Ok(CoworkUsageBatch {
            result: SessionSyncResult {
                files_scanned: paths.len() as u32,
                errors: metadata_errors,
                ..SessionSyncResult::default()
            },
            ..CoworkUsageBatch::default()
        });
    }

    let mut batch = collect_cowork_usage_from_roots_with_revisions(
        roots,
        Some(&changed_paths),
        Some(&revision_candidates),
    )?;
    batch.result.errors.extend(metadata_errors);
    defer_incomplete_cowork_batch(&mut batch);
    Ok(batch)
}

/// A rewrite is only safe when every discovered source file was readable.
/// Duplicate transcript copies can contribute to one session, so publishing a
/// partial aggregate would delete usage that exists only in the failed copy.
fn defer_incomplete_cowork_batch(batch: &mut CoworkUsageBatch) {
    if batch.result.errors.is_empty() && !batch.source_revisions.is_empty() {
        return;
    }
    batch.result.imported = 0;
    batch.canonical_batch = CanonicalUsageBatch::default();
    batch.source_revisions.clear();
}

/// Parse explicit roots for isolated parser fixtures.
#[cfg(test)]
pub(crate) fn collect_cowork_usage_from_roots(
    roots: &[PathBuf],
) -> Result<CoworkUsageBatch, AppError> {
    collect_cowork_usage_from_roots_with_revisions(roots, None, None)
}

fn collect_cowork_usage_from_roots_with_revisions(
    roots: &[PathBuf],
    changed_paths: Option<&HashSet<PathBuf>>,
    revision_candidates: Option<&HashMap<String, CoworkSourceRevision>>,
) -> Result<CoworkUsageBatch, AppError> {
    let mut aggregate = CoworkUsageBatch::default();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let batch = collect_single_root(root, changed_paths, revision_candidates)?;
        aggregate.result.merge(batch.result);
        aggregate
            .canonical_batch
            .relation_claims
            .extend(batch.canonical_batch.relation_claims);
        aggregate
            .canonical_batch
            .replacement_observations
            .extend(batch.canonical_batch.replacement_observations);
        aggregate
            .canonical_batch
            .replace_scopes
            .extend(batch.canonical_batch.replace_scopes);
        aggregate.source_revisions.extend(batch.source_revisions);
    }
    aggregate
        .source_revisions
        .sort_by(|left, right| left.file_path.cmp(&right.file_path));
    aggregate
        .source_revisions
        .dedup_by(|left, right| left.file_path == right.file_path);
    Ok(aggregate)
}

/// Compatibility entry point for isolated parser tests. Production sync uses
/// [`collect_cowork_usage`] and lets the central pipeline arbitrate matching
/// Desktop gateway rows before publishing.
#[cfg(test)]
fn sync_cowork_usage_from_roots(
    db: &Database,
    roots: &[PathBuf],
) -> Result<SessionSyncResult, AppError> {
    let batch = collect_cowork_usage_from_roots(roots)?;
    publish_canonical_batch(
        db,
        UsagePublishTarget::Published,
        batch.canonical_batch,
        "Cowork fixture",
    )?;
    Ok(batch.result)
}

#[cfg(test)]
fn sync_cowork_usage_from_roots_with_state(
    db: &Database,
    roots: &[PathBuf],
) -> Result<SessionSyncResult, AppError> {
    let mut batch = collect_cowork_usage_from_roots_with_sync_state(db, roots)?;
    let source_revisions = batch.source_revisions.clone();
    if !source_revisions.is_empty() {
        crate::services::session_usage::arbitrate_cowork_proxy_rows(
            db,
            &mut batch.canonical_batch,
            &source_revisions,
        )?;
    }
    Ok(batch.result)
}

fn collect_single_root(
    root: &Path,
    changed_paths: Option<&HashSet<PathBuf>>,
    revision_candidates: Option<&HashMap<String, CoworkSourceRevision>>,
) -> Result<CoworkUsageBatch, AppError> {
    let paths = discover_cowork_transcript_files(root);
    let mut result = SessionSyncResult {
        files_scanned: paths.len() as u32,
        ..SessionSyncResult::default()
    };
    if paths.is_empty() {
        return Ok(CoworkUsageBatch {
            result,
            ..CoworkUsageBatch::default()
        });
    }

    let mut aggregates: HashMap<String, SessionAggregate> = HashMap::new();
    let mut source_revisions = Vec::new();
    for path in paths {
        let changed = changed_paths.is_none_or(|paths| paths.contains(&path));
        let revision = revision_candidates
            .and_then(|candidates| candidates.get(&path.to_string_lossy().to_string()).cloned());
        let record = match parse_transcript(&path) {
            Ok(Some(record)) => {
                if changed {
                    if let Some(revision) = revision {
                        source_revisions.push(revision);
                    }
                }
                record
            }
            Ok(None) => {
                // Non-transcript JSONL such as a journal has no session ID
                // and no assistant usage; it must not become a fake session
                // named after the filename.
                if changed {
                    if let Some(revision) = revision {
                        source_revisions.push(revision);
                    }
                }
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
                changed,
            });
        entry.changed |= changed;
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

    let source = UsageSourceSpec::new(
        COWORK_APP_TYPE,
        COWORK_DATA_SOURCE,
        COWORK_DATA_SOURCE,
        UsagePrecision::RequestExact,
        TimeSemantics::EventTime,
        RequestCountSemantics::AssistantMessage,
    );
    let has_changed_session = sessions.iter().any(|session| session.changed);
    let mut canonical_batch = CanonicalUsageBatch {
        relation_claims: if has_changed_session {
            claims
        } else {
            Vec::new()
        },
        ..CanonicalUsageBatch::default()
    };
    for session in sessions.iter().filter(|session| session.changed) {
        // Each changed session is a complete current transcript. Replacing its
        // whole Cowork source scope also removes facts written under the former
        // UTC calendar date before local-date publication.
        canonical_batch.replace_scopes.push(CanonicalReplaceScope {
            app_type: COWORK_APP_TYPE.to_string(),
            session_id: session.session_id.clone(),
            data_source: COWORK_DATA_SOURCE.to_string(),
        });
        let mut events: Vec<&UsageEvent> = session.events.values().collect();
        events.sort_by(|left, right| left.message_id.cmp(&right.message_id));
        for event in events {
            result.imported = result.imported.saturating_add(1);
            let mut fact = source.fact(
                event.date.clone(),
                session.session_id.clone(),
                event.model.clone(),
                event.model.clone(),
                event.model.clone(),
            );
            fact.request_count = Some(1);
            fact.input_tokens = Some(event.input_tokens);
            fact.output_tokens = Some(event.output_tokens);
            fact.cache_read_tokens = Some(event.cache_read_tokens);
            fact.cache_creation_tokens = Some(event.cache_creation_tokens);
            fact.first_event_at = Some(event.event_at);
            fact.last_event_at = Some(event.event_at);
            canonical_batch.replace_observe(
                format!("cowork:{}:{}", session.session_id, event.message_id),
                fact,
            );
        }
    }

    Ok(CoworkUsageBatch {
        result,
        canonical_batch,
        source_revisions,
    })
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
    let event_at = parsed.timestamp();
    Some((event_at, local_usage_date(event_at)?))
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
        sync_cowork_usage_from_roots(&db, &[temp.path().to_path_buf()])?;
        let conn = lock_conn!(db.conn);
        let node_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM agent_session_nodes", [], |row| {
                row.get(0)
            })?;
        assert_eq!(node_count, 1);
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

    #[test]
    fn event_timestamp_uses_the_shared_local_usage_date() {
        let value = Value::String("2026-08-18T00:30:00+08:00".into());
        let (event_at, date) = parse_event_timestamp(Some(&value)).expect("valid timestamp");
        assert_eq!(
            date,
            local_usage_date(event_at).expect("timestamp has a local calendar date")
        );
    }

    #[test]
    fn full_session_replace_removes_a_previous_calendar_bucket() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        write_fixture(temp.path(), false);
        sync_cowork_usage_from_roots(&db, &[temp.path().to_path_buf()])?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE agent_session_usage_rollups
                 SET date = '1900-01-01'
                 WHERE app_type = 'claude-desktop' AND session_id = 'root-session'",
                [],
            )?;
        }

        sync_cowork_usage_from_roots(&db, &[temp.path().to_path_buf()])?;
        let conn = lock_conn!(db.conn);
        let (fact_count, stale_count): (i64, i64) = conn.query_row(
            "SELECT COUNT(*),
                    SUM(CASE WHEN date = '1900-01-01' THEN 1 ELSE 0 END)
             FROM agent_session_usage_rollups
             WHERE app_type = 'claude-desktop' AND session_id = 'root-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!((fact_count, stale_count), (1, 0));
        Ok(())
    }

    #[test]
    fn unchanged_transcripts_do_not_republish_or_increment_imported() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let (root_path, _child_path) = write_fixture(temp.path(), true);
        let roots = [temp.path().to_path_buf()];

        let first = sync_cowork_usage_from_roots_with_state(&db, &roots)?;
        assert_eq!(first.imported, 3);
        let before = {
            let conn = lock_conn!(db.conn);
            let counts: (i64, i64, i64, i64) = conn.query_row(
                "SELECT
                    (SELECT COUNT(*) FROM agent_session_usage_rollups
                     WHERE app_type = 'claude-desktop'),
                    (SELECT COALESCE(SUM(request_count), 0)
                     FROM agent_session_usage_rollups
                     WHERE app_type = 'claude-desktop'),
                    (SELECT COUNT(*) FROM agent_session_canonical_coverage
                     WHERE app_type = 'claude-desktop'),
                    (SELECT COUNT(*) FROM session_log_sync)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            let cursors = conn
                .prepare(
                    "SELECT file_path, last_modified, last_line_offset
                     FROM session_log_sync ORDER BY file_path",
                )?
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            (counts, cursors)
        };

        let second = sync_cowork_usage_from_roots_with_state(&db, &roots)?;
        assert_eq!(second.imported, 0);
        assert_eq!(second.files_scanned, 2);
        let after = {
            let conn = lock_conn!(db.conn);
            let counts: (i64, i64, i64, i64) = conn.query_row(
                "SELECT
                    (SELECT COUNT(*) FROM agent_session_usage_rollups
                     WHERE app_type = 'claude-desktop'),
                    (SELECT COALESCE(SUM(request_count), 0)
                     FROM agent_session_usage_rollups
                     WHERE app_type = 'claude-desktop'),
                    (SELECT COUNT(*) FROM agent_session_canonical_coverage
                     WHERE app_type = 'claude-desktop'),
                    (SELECT COUNT(*) FROM session_log_sync)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            let cursors = conn
                .prepare(
                    "SELECT file_path, last_modified, last_line_offset
                     FROM session_log_sync ORDER BY file_path",
                )?
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            (counts, cursors)
        };
        assert_eq!(after, before);

        fs::OpenOptions::new()
            .append(true)
            .open(&root_path)
            .expect("open root transcript")
            .write_all(
                concat!(
                    r#"{"sessionId":"root-session","type":"assistant","message":{"id":"msg-new","model":"claude-sonnet","usage":{"input_tokens":1,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-13T10:05:00Z"}"#,
                    "\n",
                )
                .as_bytes(),
            )
            .expect("append root transcript");

        let third = sync_cowork_usage_from_roots_with_state(&db, &roots)?;
        assert_eq!(third.imported, 3);
        let conn = lock_conn!(db.conn);
        let request_count: i64 = conn.query_row(
            "SELECT COALESCE(SUM(request_count), 0)
             FROM agent_session_usage_rollups
             WHERE app_type = 'claude-desktop'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(request_count, 4);
        Ok(())
    }

    #[test]
    fn transcript_error_defers_session_replacement_and_cursor_publish() {
        let mut batch = CoworkUsageBatch {
            result: SessionSyncResult {
                imported: 2,
                errors: vec!["duplicate transcript could not be read".to_string()],
                ..SessionSyncResult::default()
            },
            canonical_batch: CanonicalUsageBatch {
                replace_scopes: vec![CanonicalReplaceScope {
                    app_type: COWORK_APP_TYPE.to_string(),
                    session_id: "shared-session".to_string(),
                    data_source: COWORK_DATA_SOURCE.to_string(),
                }],
                ..CanonicalUsageBatch::default()
            },
            source_revisions: vec![CoworkSourceRevision {
                file_path: "C:/fixtures/shared-session.jsonl".to_string(),
                last_modified: 1,
                last_offset: 2,
            }],
        };

        defer_incomplete_cowork_batch(&mut batch);

        assert_eq!(batch.result.imported, 0);
        assert_eq!(batch.result.errors.len(), 1);
        assert!(batch.source_revisions.is_empty());
        assert!(batch.canonical_batch.replace_scopes.is_empty());
        assert!(batch.canonical_batch.relation_claims.is_empty());
        assert!(batch.canonical_batch.replacement_observations.is_empty());
    }
}
