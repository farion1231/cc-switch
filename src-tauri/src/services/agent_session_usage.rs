//! Canonical Agent-session relationships, usage semantics, and write bridges.
//!
//! This module is intentionally a small domain contract.  Source adapters turn
//! their evidence into [`SessionRelationClaim`] and
//! [`NormalizedUsageRollupFact`] values for the shared canonical pipeline.
//! There is deliberately no task-total field or task-total write operation;
//! totals are derived later by the query layer from self and descendant rows.

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::services::sql_helpers::fresh_input_sql;
use crate::services::usage_stats::find_exact_model_pricing;
use chrono::{Local, TimeZone};
use rusqlite::{params, Connection, OptionalExtension, ToSql};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

/// Keep the database/API string contract for the three parsed usage enums in
/// one place. The enum definitions remain explicit because their serde names
/// and defaults are public response semantics.
macro_rules! impl_usage_string_contract {
    ($type:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        impl $type {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value,)+
                }
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $type {
            type Err = ();

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(()),
                }
            }
        }
    };
}

/// Precision of a usage measurement.  Serialized values are part of the
/// adapter/query contract and must not be renamed casually.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum UsagePrecision {
    RequestExact,
    SessionExact,
    SyncWindowDelta,
    Estimated,
    #[default]
    Unavailable,
}

impl_usage_string_contract!(UsagePrecision {
    RequestExact => "request_exact",
    SessionExact => "session_exact",
    SyncWindowDelta => "sync_window_delta",
    Estimated => "estimated",
    Unavailable => "unavailable",
});

impl UsagePrecision {
    fn rank(self) -> u8 {
        match self {
            Self::RequestExact => 0,
            Self::SessionExact => 1,
            Self::SyncWindowDelta => 2,
            Self::Estimated => 3,
            Self::Unavailable => 4,
        }
    }
}

/// Clock used by a usage bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TimeSemantics {
    EventTime,
    SessionTime,
    SyncWindowEnd,
    #[default]
    Unavailable,
}

impl_usage_string_contract!(TimeSemantics {
    EventTime => "event_time",
    SessionTime => "session_time",
    SyncWindowEnd => "sync_window_end",
    Unavailable => "unavailable",
});

/// Meaning of `request_count`; an agent call or assistant message is not an
/// HTTP request unless the source actually proves that semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum RequestCountSemantics {
    HttpRequest,
    AssistantMessage,
    AgentCall,
    UsageEvent,
    #[default]
    Unavailable,
}

impl_usage_string_contract!(RequestCountSemantics {
    HttpRequest => "http_request",
    AssistantMessage => "assistant_message",
    AgentCall => "agent_call",
    UsageEvent => "usage_event",
    Unavailable => "unavailable",
});

/// Normalized node kind persisted in `agent_session_nodes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionNodeKind {
    Root,
    Child,
    Standalone,
    Unknown,
    Conflict,
}

impl_usage_string_contract!(SessionNodeKind {
    Root => "root",
    Child => "child",
    Standalone => "standalone",
    Unknown => "unknown",
    Conflict => "conflict",
});

/// Confidence of the relationship stored on a normalized node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationConfidence {
    Explicit,
    Structural,
    Unavailable,
    Conflict,
}

impl_usage_string_contract!(RelationConfidence {
    Explicit => "explicit",
    Structural => "structural",
    Unavailable => "unavailable",
    Conflict => "conflict",
});

/// Capability availability is intentionally separate from boolean feature
/// flags: a source can be partially readable without being exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Supported,
    Partial,
    Unavailable,
}

impl_usage_string_contract!(CapabilityStatus {
    Supported => "supported",
    Partial => "partial",
    Unavailable => "unavailable",
});

/// Why a root with known descendants does not currently expose a descendant
/// measure.  A bounded task query can distinguish an empty range from a
/// source that failed to provide any usage, while the unbounded session query
/// preserves the latter as unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DescendantUsageStatus {
    Available,
    NoActivityInRange,
    Unavailable,
    NotApplicable,
}

/// Publication state for the Codex task generation.  A replay is written to
/// shadow tables and only becomes visible after a complete atomic publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AgentTaskUsageDataStatus {
    #[default]
    Ready,
    RebuildingWithSnapshot,
    Rebuilding,
}

impl_usage_string_contract!(DescendantUsageStatus {
    Available => "available",
    NoActivityInRange => "no_activity_in_range",
    Unavailable => "unavailable",
    NotApplicable => "not_applicable",
});

/// Backend capability contract consumed by future adapters and query/API
/// layers.  `token_status` and `cost_status` prevent unknown values from being
/// interpreted as zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageCapability {
    pub app_type: &'static str,
    pub session_enumeration: CapabilityStatus,
    pub usage_status: CapabilityStatus,
    pub supports_descendants: bool,
    pub token_status: CapabilityStatus,
    pub cost_status: CapabilityStatus,
    pub precision: UsagePrecision,
    pub time_semantics: TimeSemantics,
    pub request_count_semantics: RequestCountSemantics,
    pub notes: &'static str,
}

/// The common contract for native sources that prove one event-time usage
/// record at a time. Agent-specific capability deviations stay explicit in
/// the registry below.
const fn exact_event_capability(
    app_type: &'static str,
    supports_descendants: bool,
    request_count_semantics: RequestCountSemantics,
    notes: &'static str,
) -> AgentUsageCapability {
    AgentUsageCapability {
        app_type,
        session_enumeration: CapabilityStatus::Supported,
        usage_status: CapabilityStatus::Supported,
        supports_descendants,
        token_status: CapabilityStatus::Supported,
        cost_status: CapabilityStatus::Supported,
        precision: UsagePrecision::RequestExact,
        time_semantics: TimeSemantics::EventTime,
        request_count_semantics,
        notes,
    }
}

const CAPABILITY_REGISTRY: [AgentUsageCapability; 9] = [
    exact_event_capability(
        "claude",
        true,
        RequestCountSemantics::AssistantMessage,
        "Explicit SESSION_ID/subagents and workflow structure only.",
    ),
    AgentUsageCapability {
        session_enumeration: CapabilityStatus::Partial,
        usage_status: CapabilityStatus::Partial,
        token_status: CapabilityStatus::Partial,
        cost_status: CapabilityStatus::Partial,
        ..exact_event_capability(
            "claude-desktop",
            true,
            RequestCountSemantics::AssistantMessage,
            "Cowork local-agent-mode-sessions transcript path only; assistant rows require all token components and RFC3339 event time, cost may be unknown, and descendants require discovered structural sessionId/subagents parent evidence.",
        )
    },
    exact_event_capability(
        "codex",
        true,
        RequestCountSemantics::AgentCall,
        "Parentage requires agreeing explicit fork/thread_spawn evidence.",
    ),
    exact_event_capability(
        "gemini",
        false,
        RequestCountSemantics::AssistantMessage,
        "Self-only until an explicit parent field is proven.",
    ),
    exact_event_capability(
        "grokbuild",
        false,
        RequestCountSemantics::AgentCall,
        "turn_completed values are face-value agent calls; no HTTP inference.",
    ),
    exact_event_capability(
        "opencode",
        false,
        RequestCountSemantics::AssistantMessage,
        "JSON/SQLite duplicates are arbitrated; parentage is fixture-gated.",
    ),
    AgentUsageCapability {
        session_enumeration: CapabilityStatus::Partial,
        usage_status: CapabilityStatus::Unavailable,
        token_status: CapabilityStatus::Unavailable,
        cost_status: CapabilityStatus::Unavailable,
        precision: UsagePrecision::Unavailable,
        time_semantics: TimeSemantics::Unavailable,
        ..exact_event_capability(
            "openclaw",
            false,
            RequestCountSemantics::Unavailable,
            "Agent namespace is stable; display names are not parent evidence.",
        )
    },
    AgentUsageCapability {
        session_enumeration: CapabilityStatus::Partial,
        usage_status: CapabilityStatus::Partial,
        token_status: CapabilityStatus::Partial,
        cost_status: CapabilityStatus::Partial,
        precision: UsagePrecision::SyncWindowDelta,
        time_semantics: TimeSemantics::SyncWindowEnd,
        ..exact_event_capability(
            "hermes",
            false,
            RequestCountSemantics::Unavailable,
            "Readonly cumulative sync-window delta; task is not a parent relation and call count is not proven.",
        )
    },
    AgentUsageCapability {
        cost_status: CapabilityStatus::Partial,
        ..exact_event_capability(
            "pi",
            true,
            RequestCountSemantics::UsageEvent,
            "parentSession paths are matched only within the current successful scan; usage events include assistant, tool result, compaction, and branch summary entries.",
        )
    },
];

/// Return the immutable capability registry.  Future App additions must add a
/// matching entry and update the focused coverage test, otherwise CI fails.
pub fn agent_usage_capabilities() -> &'static [AgentUsageCapability] {
    &CAPABILITY_REGISTRY
}

/// Look up a capability by canonical or accepted `AppType` spelling.
pub fn agent_usage_capability(app_type: &str) -> Option<&'static AgentUsageCapability> {
    let canonical = AppType::from_str(app_type).ok()?.as_str().to_string();
    CAPABILITY_REGISTRY
        .iter()
        .find(|capability| capability.app_type == canonical)
}

/// Optional metadata carried by a node claim.  Missing metadata is not an
/// excuse to infer a parent relation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNodeMetadata {
    pub title: Option<String>,
    pub project_dir: Option<String>,
    pub source_path: Option<String>,
    pub created_at: Option<i64>,
    pub last_active_at: Option<i64>,
    #[serde(default)]
    pub last_synced_at: i64,
}

/// Evidence supplied by an adapter for one session node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RelationClaim {
    Root,
    Standalone,
    Parent {
        parent_session_id: String,
        confidence: RelationConfidence,
    },
    Unknown,
}

/// A relation claim is deliberately independent from a persisted node.  The
/// graph normalizer resolves all claims together so conflicts and cycles can be
/// failed closed before a DAO write occurs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRelationClaim {
    pub app_type: String,
    pub session_id: String,
    pub relation: RelationClaim,
    #[serde(default)]
    pub metadata: SessionNodeMetadata,
}

impl SessionRelationClaim {
    #[cfg(test)]
    pub fn root(app_type: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            app_type: app_type.into(),
            session_id: session_id.into(),
            relation: RelationClaim::Root,
            metadata: SessionNodeMetadata::default(),
        }
    }

    pub fn standalone(app_type: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            app_type: app_type.into(),
            session_id: session_id.into(),
            relation: RelationClaim::Standalone,
            metadata: SessionNodeMetadata::default(),
        }
    }

    pub fn child(
        app_type: impl Into<String>,
        session_id: impl Into<String>,
        parent_session_id: impl Into<String>,
        confidence: RelationConfidence,
    ) -> Self {
        Self {
            app_type: app_type.into(),
            session_id: session_id.into(),
            relation: RelationClaim::Parent {
                parent_session_id: parent_session_id.into(),
                confidence,
            },
            metadata: SessionNodeMetadata::default(),
        }
    }
}

/// A normalized, validated node ready for the canonical node write bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedSessionNode {
    pub app_type: String,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub root_session_id: String,
    pub node_kind: SessionNodeKind,
    pub relation_confidence: RelationConfidence,
    pub title: Option<String>,
    pub project_dir: Option<String>,
    pub source_path: Option<String>,
    pub created_at: Option<i64>,
    pub last_active_at: Option<i64>,
    pub last_synced_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NodeKey {
    app_type: String,
    session_id: String,
}

#[derive(Debug, Clone)]
struct ClaimRecord {
    claim: SessionRelationClaim,
    conflicting_claims: bool,
}

#[derive(Debug, Clone)]
struct ResolvedRelation {
    parent_session_id: Option<String>,
    root_session_id: String,
    node_kind: SessionNodeKind,
    relation_confidence: RelationConfidence,
}

fn canonical_app_type(value: &str) -> Result<String, AppError> {
    AppType::from_str(value)
        .map(|app| app.as_str().to_string())
        .map_err(|_| AppError::InvalidInput(format!("未知 Agent app_type: {value}")))
}

fn normalized_id(value: &str, field: &str) -> Result<String, AppError> {
    let id = value.trim();
    if id.is_empty() {
        return Err(AppError::InvalidInput(format!("{field} 不能为空")));
    }
    Ok(id.to_string())
}

fn normalized_relation(relation: &RelationClaim) -> RelationClaim {
    match relation {
        RelationClaim::Parent {
            parent_session_id,
            confidence,
        } => {
            let parent = parent_session_id.trim();
            if parent.is_empty() {
                RelationClaim::Unknown
            } else {
                RelationClaim::Parent {
                    parent_session_id: parent.to_string(),
                    confidence: *confidence,
                }
            }
        }
        other => other.clone(),
    }
}

fn relation_equivalent(left: &RelationClaim, right: &RelationClaim) -> bool {
    match (left, right) {
        (RelationClaim::Root, RelationClaim::Root)
        | (RelationClaim::Standalone, RelationClaim::Standalone)
        | (RelationClaim::Unknown, RelationClaim::Unknown) => true,
        (
            RelationClaim::Parent {
                parent_session_id: left_parent,
                ..
            },
            RelationClaim::Parent {
                parent_session_id: right_parent,
                ..
            },
        ) => left_parent == right_parent,
        _ => false,
    }
}

fn stronger_confidence(left: RelationConfidence, right: RelationConfidence) -> RelationConfidence {
    match (left, right) {
        (RelationConfidence::Conflict, _) | (_, RelationConfidence::Conflict) => {
            RelationConfidence::Conflict
        }
        (RelationConfidence::Explicit, _) | (_, RelationConfidence::Explicit) => {
            RelationConfidence::Explicit
        }
        (RelationConfidence::Structural, _) | (_, RelationConfidence::Structural) => {
            RelationConfidence::Structural
        }
        _ => RelationConfidence::Unavailable,
    }
}

fn merge_metadata(existing: &mut SessionNodeMetadata, incoming: &SessionNodeMetadata) {
    if incoming.title.is_some() {
        existing.title = incoming.title.clone();
    }
    if incoming.project_dir.is_some() {
        existing.project_dir = incoming.project_dir.clone();
    }
    if incoming.source_path.is_some() {
        existing.source_path = incoming.source_path.clone();
    }
    if incoming.created_at.is_some() {
        existing.created_at = incoming.created_at;
    }
    if incoming.last_active_at.is_some() {
        existing.last_active_at = incoming.last_active_at;
    }
    existing.last_synced_at = existing.last_synced_at.max(incoming.last_synced_at);
}

fn invalid_resolution(session_id: String, node_kind: SessionNodeKind) -> ResolvedRelation {
    let relation_confidence = match node_kind {
        SessionNodeKind::Conflict => RelationConfidence::Conflict,
        _ => RelationConfidence::Unavailable,
    };
    ResolvedRelation {
        parent_session_id: None,
        root_session_id: session_id,
        node_kind,
        relation_confidence,
    }
}

fn resolve_relation(
    key: &NodeKey,
    records: &HashMap<NodeKey, ClaimRecord>,
    cache: &mut HashMap<NodeKey, ResolvedRelation>,
    visiting: &mut HashSet<NodeKey>,
) -> ResolvedRelation {
    if let Some(cached) = cache.get(key) {
        return cached.clone();
    }

    // Encountering a key already on the DFS stack proves a cycle.  The
    // current and all callers that depend on it are subsequently marked
    // conflict, and none receives a parent/root ownership edge.
    if !visiting.insert(key.clone()) {
        let resolution = invalid_resolution(key.session_id.clone(), SessionNodeKind::Conflict);
        cache.insert(key.clone(), resolution.clone());
        return resolution;
    }

    let resolution = match records.get(key) {
        None => invalid_resolution(key.session_id.clone(), SessionNodeKind::Unknown),
        Some(record) if record.conflicting_claims => {
            invalid_resolution(key.session_id.clone(), SessionNodeKind::Conflict)
        }
        Some(record) => match &record.claim.relation {
            RelationClaim::Root => ResolvedRelation {
                parent_session_id: None,
                root_session_id: key.session_id.clone(),
                node_kind: SessionNodeKind::Root,
                relation_confidence: RelationConfidence::Explicit,
            },
            RelationClaim::Standalone => {
                invalid_resolution(key.session_id.clone(), SessionNodeKind::Standalone)
            }
            RelationClaim::Unknown => {
                invalid_resolution(key.session_id.clone(), SessionNodeKind::Unknown)
            }
            RelationClaim::Parent {
                parent_session_id,
                confidence,
            } => {
                let parent_key = NodeKey {
                    app_type: key.app_type.clone(),
                    session_id: parent_session_id.clone(),
                };
                if parent_session_id == &key.session_id {
                    invalid_resolution(key.session_id.clone(), SessionNodeKind::Conflict)
                } else if !records.contains_key(&parent_key) {
                    invalid_resolution(key.session_id.clone(), SessionNodeKind::Unknown)
                } else if *confidence == RelationConfidence::Conflict {
                    invalid_resolution(key.session_id.clone(), SessionNodeKind::Conflict)
                } else if !matches!(
                    confidence,
                    RelationConfidence::Explicit | RelationConfidence::Structural
                ) {
                    invalid_resolution(key.session_id.clone(), SessionNodeKind::Unknown)
                } else {
                    let parent_resolution = resolve_relation(&parent_key, records, cache, visiting);
                    match parent_resolution.node_kind {
                        SessionNodeKind::Conflict => {
                            invalid_resolution(key.session_id.clone(), SessionNodeKind::Conflict)
                        }
                        SessionNodeKind::Unknown => {
                            invalid_resolution(key.session_id.clone(), SessionNodeKind::Unknown)
                        }
                        _ => ResolvedRelation {
                            parent_session_id: Some(parent_session_id.clone()),
                            root_session_id: parent_resolution.root_session_id,
                            node_kind: SessionNodeKind::Child,
                            relation_confidence: *confidence,
                        },
                    }
                }
            }
        },
    };

    visiting.remove(key);
    cache.insert(key.clone(), resolution.clone());
    resolution
}

/// Normalize all claims for one or more apps in one graph pass.
///
/// Missing parents, self-parent edges, cycles, and conflicting duplicate
/// claims are represented as `unknown`/`conflict` nodes rooted at themselves;
/// they never retain an unsafe parent or root ownership edge.  Repeated equal
/// claims are merged deterministically, with explicit confidence winning over
/// structural confidence.
pub fn normalize_session_relations(
    claims: &[SessionRelationClaim],
) -> Result<Vec<NormalizedSessionNode>, AppError> {
    let mut records: HashMap<NodeKey, ClaimRecord> = HashMap::new();
    let mut order = Vec::new();

    for claim in claims {
        let app_type = canonical_app_type(&claim.app_type)?;
        let session_id = normalized_id(&claim.session_id, "session_id")?;
        let mut normalized_claim = claim.clone();
        normalized_claim.app_type = app_type.clone();
        normalized_claim.session_id = session_id.clone();
        normalized_claim.relation = normalized_relation(&claim.relation);

        let key = NodeKey {
            app_type,
            session_id,
        };
        if let Some(existing) = records.get_mut(&key) {
            if relation_equivalent(&existing.claim.relation, &normalized_claim.relation) {
                if let (
                    RelationClaim::Parent {
                        confidence: existing_confidence,
                        ..
                    },
                    RelationClaim::Parent {
                        confidence: incoming_confidence,
                        ..
                    },
                ) = (&mut existing.claim.relation, &normalized_claim.relation)
                {
                    *existing_confidence =
                        stronger_confidence(*existing_confidence, *incoming_confidence);
                }
                merge_metadata(&mut existing.claim.metadata, &normalized_claim.metadata);
            } else {
                existing.conflicting_claims = true;
            }
        } else {
            order.push(key.clone());
            records.insert(
                key,
                ClaimRecord {
                    claim: normalized_claim,
                    conflicting_claims: false,
                },
            );
        }
    }

    let mut cache = HashMap::new();
    let mut normalized = Vec::with_capacity(order.len());
    for key in order {
        let resolution = resolve_relation(&key, &records, &mut cache, &mut HashSet::new());
        let record = records
            .get(&key)
            .expect("every ordered key has a claim record");
        normalized.push(NormalizedSessionNode {
            app_type: key.app_type,
            session_id: key.session_id,
            parent_session_id: resolution.parent_session_id,
            root_session_id: resolution.root_session_id,
            node_kind: resolution.node_kind,
            relation_confidence: resolution.relation_confidence,
            title: record.claim.metadata.title.clone(),
            project_dir: record.claim.metadata.project_dir.clone(),
            source_path: record.claim.metadata.source_path.clone(),
            created_at: record.claim.metadata.created_at,
            last_active_at: record.claim.metadata.last_active_at,
            last_synced_at: record.claim.metadata.last_synced_at,
        });
    }

    Ok(normalized)
}

impl NormalizedSessionNode {
    /// Validate persistence invariants before crossing the DAO boundary.
    pub fn validate_for_persistence(&self) -> Result<(), AppError> {
        canonical_app_type(&self.app_type)?;
        let session_id = normalized_id(&self.session_id, "session_id")?;
        let root_session_id = normalized_id(&self.root_session_id, "root_session_id")?;
        match self.node_kind {
            SessionNodeKind::Child => {
                let parent = self
                    .parent_session_id
                    .as_deref()
                    .ok_or_else(|| AppError::InvalidInput("child 缺少 parent_session_id".into()))?;
                let parent = normalized_id(parent, "parent_session_id")?;
                if parent == session_id || root_session_id == session_id {
                    return Err(AppError::InvalidInput(
                        "child 不能自父或把自身作为 root_session_id".into(),
                    ));
                }
                if !matches!(
                    self.relation_confidence,
                    RelationConfidence::Explicit | RelationConfidence::Structural
                ) {
                    return Err(AppError::InvalidInput(
                        "child 的 relation_confidence 必须是 explicit 或 structural".into(),
                    ));
                }
            }
            SessionNodeKind::Root
            | SessionNodeKind::Standalone
            | SessionNodeKind::Unknown
            | SessionNodeKind::Conflict => {
                if self.parent_session_id.is_some() || root_session_id != session_id {
                    return Err(AppError::InvalidInput(
                        "非 child 节点必须 self-root 且不保留 parent".into(),
                    ));
                }
                let expected_confidence = match self.node_kind {
                    SessionNodeKind::Root => RelationConfidence::Explicit,
                    SessionNodeKind::Conflict => RelationConfidence::Conflict,
                    SessionNodeKind::Standalone | SessionNodeKind::Unknown => {
                        RelationConfidence::Unavailable
                    }
                    SessionNodeKind::Child => unreachable!(),
                };
                if self.relation_confidence != expected_confidence {
                    return Err(AppError::InvalidInput(
                        "node_kind 与 relation_confidence 不匹配".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Full-fidelity source fact for a single day/session/source/window bucket.
///
/// This single contract retains every real source identity dimension and
/// source-provided token/cost qualifier needed by the final v18 schema.
/// It is still one usage bucket: no task-level total or independently mutable
/// aggregate is represented here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedUsageRollupFact {
    pub date: String,
    pub app_type: String,
    pub session_id: String,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub request_model: String,
    #[serde(default)]
    pub pricing_model: String,
    pub data_source: String,
    pub precision: UsagePrecision,
    pub time_semantics: TimeSemantics,
    pub request_count_semantics: RequestCountSemantics,
    /// Source-provided input-token accounting semantic. Do not infer this from
    /// `app_type`; callers should pass the source's proven value directly.
    pub input_token_semantics: i64,
    pub source_identity: String,
    pub profile_id: String,
    pub database_identity: String,
    pub base_url_digest: String,
    pub billing_mode: String,
    pub task: String,
    pub source_version: String,
    pub sync_window_start: i64,
    pub sync_window_end: i64,
    pub request_count: Option<i64>,
    pub api_call_count: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    /// Hermes cache-write is a distinct source dimension; it must never be
    /// copied into `cache_creation_tokens`.
    pub cache_write_tokens: Option<i64>,
    /// Grok reasoning is retained independently and is already a subset of
    /// output; it must never be added to output or derived totals.
    pub reasoning_tokens: Option<i64>,
    pub total_cost_usd: Option<String>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub cost_delta_kind: Option<String>,
    pub correction_state: Option<String>,
    pub first_event_at: Option<i64>,
    pub last_event_at: Option<i64>,
}

impl NormalizedUsageRollupFact {
    pub fn validate_for_persistence(&self) -> Result<(), AppError> {
        canonical_app_type(&self.app_type)?;
        normalized_id(&self.session_id, "session_id")?;
        if self.date.trim().is_empty() {
            return Err(AppError::InvalidInput("usage rollup date 不能为空".into()));
        }
        if self.data_source.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "usage rollup data_source 不能为空".into(),
            ));
        }
        for (field, value) in [
            ("request_count", self.request_count),
            ("api_call_count", self.api_call_count),
            ("input_tokens", self.input_tokens),
            ("output_tokens", self.output_tokens),
            ("cache_read_tokens", self.cache_read_tokens),
            ("cache_creation_tokens", self.cache_creation_tokens),
            ("cache_write_tokens", self.cache_write_tokens),
            ("reasoning_tokens", self.reasoning_tokens),
        ] {
            if value.is_some_and(|value| value < 0) {
                return Err(AppError::InvalidInput(format!("{field} 不能为负数")));
            }
        }
        let has_usage_dimension = self.request_count.is_some()
            || self.api_call_count.is_some()
            || self.input_tokens.is_some()
            || self.output_tokens.is_some()
            || self.cache_read_tokens.is_some()
            || self.cache_creation_tokens.is_some()
            || self.cache_write_tokens.is_some()
            || self.reasoning_tokens.is_some();
        if !has_usage_dimension {
            return Err(AppError::InvalidInput(
                "完整 usage fact 至少需要一个已知的 count/token component".into(),
            ));
        }
        if self.precision == UsagePrecision::Unavailable {
            return Err(AppError::InvalidInput(
                "precision=unavailable 的 usage 不应写入 durable bucket".into(),
            ));
        }
        if let Some(cost) = self.total_cost_usd.as_deref() {
            if cost.trim().is_empty() {
                return Err(AppError::InvalidInput(
                    "total_cost_usd 必须是非空字符串或 None".into(),
                ));
            }
            let parsed = Decimal::from_str(cost).map_err(|_| {
                AppError::InvalidInput("total_cost_usd 必须是可解析的十进制字符串".into())
            })?;
            if parsed < Decimal::ZERO
                && self.cost_delta_kind.as_deref() != Some("reconciliation")
                && self.correction_state.is_none()
            {
                return Err(AppError::InvalidInput(
                    "负费用必须带有 cost_delta_kind=reconciliation 或 correction_state 元数据"
                        .into(),
                ));
            }
        }
        Ok(())
    }
}

fn measure_cost(value: Option<&String>) -> (Option<String>, bool) {
    match value {
        None => (None, false),
        Some(value) => match Decimal::from_str(value.trim()) {
            Ok(cost) if cost >= Decimal::ZERO => (Some(value.trim().to_string()), false),
            _ => (None, true),
        },
    }
}

const COST_VALUE_SEPARATOR: char = '\u{1f}';

/// Correction/reconciliation metadata describes a source adjustment rather
/// than a billable user expense. Hermes also stores a JSON baseline/emitted
/// state in `correction_state` for ordinary increases, so only explicit
/// correction-like values are treated as an adjustment marker here.
fn has_cost_correction_metadata(
    cost_delta_kind: Option<&str>,
    correction_state: Option<&str>,
) -> bool {
    let kind_is_correction = cost_delta_kind.is_some_and(|value| {
        let value = value.trim().to_ascii_lowercase();
        value == "reconciliation"
            || value.contains("correction")
            || value.contains("reconcil")
            || value.contains("adjust")
            || value.contains("rollback")
    });
    let state_is_correction = correction_state.is_some_and(|value| {
        let value = value.trim().to_ascii_lowercase();
        value.contains("correction")
            || value.contains("reconcil")
            || value.contains("adjust")
            || value.contains("rollback")
    });
    kind_is_correction || state_is_correction
}

/// Aggregate persisted/raw cost text without delegating decimal arithmetic to
/// SQLite's REAL conversion. Invalid/negative values and explicit correction
/// groups are excluded from reported spend, but still make the result partial
/// and carry a warning. `known_cost_count` is the SQL COUNT of non-NULL cost
/// values; a missing value therefore remains observable even when another row
/// in the group has a valid positive cost.
fn aggregate_reported_cost(
    values: Option<&str>,
    row_count: i64,
    known_cost_count: i64,
    cost_delta_kind: Option<&str>,
    correction_state: Option<&str>,
) -> (Option<String>, bool, Vec<String>) {
    let mut partial = known_cost_count < row_count;
    let mut warnings = Vec::new();
    let correction_metadata = has_cost_correction_metadata(cost_delta_kind, correction_state);

    let mut total = Decimal::ZERO;
    let mut has_valid_cost = false;
    let mut has_invalid_cost = false;
    if let Some(values) = values {
        for raw_value in values.split(COST_VALUE_SEPARATOR) {
            let value = raw_value.trim().to_string();
            let (cost, cost_correction) = measure_cost(Some(&value));
            if cost_correction {
                has_invalid_cost = true;
                partial = true;
                continue;
            }
            if let Some(cost) = cost {
                // `measure_cost` has already rejected negative/invalid text;
                // this parse is infallible for the same trimmed decimal.
                if let Ok(cost) = Decimal::from_str(&cost) {
                    total += cost;
                    has_valid_cost = true;
                } else {
                    has_invalid_cost = true;
                    partial = true;
                }
            }
        }
    }
    if has_invalid_cost {
        warnings.push("negative or invalid cost was excluded from reported spend".to_string());
    }
    if correction_metadata {
        warnings
            .push("correction or reconciliation cost was excluded from reported spend".to_string());
        return (None, true, warnings);
    }
    if has_valid_cost {
        (Some(canonical_decimal_text(total)), partial, warnings)
    } else {
        (None, true, warnings)
    }
}

fn canonical_decimal_text(value: Decimal) -> String {
    let mut text = value.to_string();
    if let Some((whole, fraction)) = text.split_once('.') {
        let trimmed_fraction = fraction.trim_end_matches('0');
        text = if trimmed_fraction.is_empty() {
            whole.to_string()
        } else {
            format!("{whole}.{trimmed_fraction}")
        };
    }
    text
}

/// Canonical cumulative-source snapshot used with the Hermes fact+snapshot
/// atomic bridge. Snapshot counters are source totals, not a user-facing task
/// total; the adapter computes the sync-window delta before constructing a
/// [`NormalizedUsageRollupFact`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedUsageSnapshot {
    pub app_type: String,
    pub source_identity: String,
    pub profile_id: String,
    pub database_identity: String,
    pub session_id: String,
    pub model: String,
    pub provider_id: String,
    pub base_url_digest: String,
    pub billing_mode: String,
    pub task: String,
    pub data_source: String,
    pub source_version: String,
    pub api_call_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub first_seen: Option<i64>,
    pub last_seen: Option<i64>,
    pub last_synced_at: i64,
    pub estimated_cost_usd: Option<String>,
    pub actual_cost_usd: Option<String>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub correction_state: Option<String>,
}

impl NormalizedUsageSnapshot {
    pub fn validate_for_persistence(&self) -> Result<(), AppError> {
        canonical_app_type(&self.app_type)?;
        normalized_id(&self.session_id, "session_id")?;
        if self.data_source.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "snapshot data_source 不能为空".into(),
            ));
        }
        for (field, value) in [
            ("api_call_count", self.api_call_count),
            ("input_tokens", self.input_tokens),
            ("output_tokens", self.output_tokens),
            ("cache_read_tokens", self.cache_read_tokens),
            ("cache_write_tokens", self.cache_write_tokens),
            ("reasoning_tokens", self.reasoning_tokens),
        ] {
            if value < 0 {
                return Err(AppError::InvalidInput(format!(
                    "snapshot {field} 不能为负数"
                )));
            }
        }
        for (field, value) in [
            ("estimated_cost_usd", self.estimated_cost_usd.as_deref()),
            ("actual_cost_usd", self.actual_cost_usd.as_deref()),
        ] {
            if let Some(value) = value {
                if value.trim().is_empty() {
                    return Err(AppError::InvalidInput(format!(
                        "{field} 必须是非空字符串或 None"
                    )));
                }
                let parsed = Decimal::from_str(value).map_err(|_| {
                    AppError::InvalidInput(format!("{field} 必须是可解析的十进制字符串"))
                })?;
                if parsed < Decimal::ZERO {
                    return Err(AppError::InvalidInput(format!(
                        "snapshot {field} 不能为负数"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Query-layer usage measure.  Components and cost remain optional to make an
/// unavailable source observable as `null`, while `total_tokens` is computed
/// on demand and is never a persisted field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMeasure {
    pub data_source: Option<String>,
    pub request_count: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub total_cost_usd: Option<String>,
    pub precision: UsagePrecision,
    pub time_semantics: TimeSemantics,
    pub request_count_semantics: RequestCountSemantics,
    pub partial: bool,
    pub warnings: Vec<String>,
}

impl UsageMeasure {
    pub fn unavailable(warning: impl Into<String>) -> Self {
        Self {
            data_source: None,
            request_count: None,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            total_cost_usd: None,
            precision: UsagePrecision::Unavailable,
            time_semantics: TimeSemantics::Unavailable,
            request_count_semantics: RequestCountSemantics::Unavailable,
            partial: true,
            warnings: vec![warning.into()],
        }
    }

    /// Compute the canonical four token components without persisting a second
    /// total. Source-specific reasoning/cache-write dimensions are not folded
    /// into this derived value.
    pub fn total_tokens(&self) -> Option<i64> {
        Some(
            self.input_tokens?
                .saturating_add(self.output_tokens?)
                .saturating_add(self.cache_read_tokens?)
                .saturating_add(self.cache_creation_tokens?),
        )
    }

    /// Add self/descendant measures while preserving the weakest semantics.
    /// Incompatible request-count semantics remain `None` rather than being
    /// mislabeled as HTTP requests.
    pub fn combine(&self, other: &Self) -> Self {
        let same_count_semantics = self.request_count_semantics == other.request_count_semantics;
        let request_count = if same_count_semantics {
            match (self.request_count, other.request_count) {
                (Some(left), Some(right)) => left.checked_add(right),
                _ => None,
            }
        } else {
            None
        };
        let precision = if self.precision.rank() >= other.precision.rank() {
            self.precision
        } else {
            other.precision
        };
        let time_semantics = if self.time_semantics == other.time_semantics {
            self.time_semantics
        } else {
            TimeSemantics::Unavailable
        };
        let request_count_semantics = if same_count_semantics {
            self.request_count_semantics
        } else {
            RequestCountSemantics::Unavailable
        };
        let input_tokens = add_optional(self.input_tokens, other.input_tokens);
        let output_tokens = add_optional(self.output_tokens, other.output_tokens);
        let cache_read_tokens = add_optional(self.cache_read_tokens, other.cache_read_tokens);
        let cache_creation_tokens =
            add_optional(self.cache_creation_tokens, other.cache_creation_tokens);
        let total_cost_usd = match (
            self.total_cost_usd.as_deref(),
            other.total_cost_usd.as_deref(),
        ) {
            (Some(left), Some(right)) => add_cost(Some(left), Some(right)),
            (Some(left), None) if other.cost_was_excluded() => Some(left.to_string()),
            (None, Some(right)) if self.cost_was_excluded() => Some(right.to_string()),
            _ => None,
        };
        let total_cost_known = total_cost_usd.is_some();
        let mut warnings = self.warnings.clone();
        warnings.extend(other.warnings.iter().cloned());
        if !same_count_semantics {
            warnings.push("request_count semantics differ; count omitted".to_string());
        }
        if time_semantics == TimeSemantics::Unavailable
            && self.time_semantics != other.time_semantics
        {
            warnings.push("time semantics differ; aggregate time unavailable".to_string());
        }
        let complete_standard_components =
            self.has_complete_standard_components() && other.has_complete_standard_components();
        let combined_components_complete = request_count.is_some()
            && input_tokens.is_some()
            && output_tokens.is_some()
            && cache_read_tokens.is_some()
            && cache_creation_tokens.is_some();
        Self {
            data_source: if self.data_source == other.data_source {
                self.data_source.clone()
            } else {
                None
            },
            request_count,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            total_cost_usd,
            precision,
            time_semantics,
            request_count_semantics,
            partial: self.partial
                || other.partial
                || precision != UsagePrecision::RequestExact
                || !same_count_semantics
                || !complete_standard_components
                || !combined_components_complete
                || !total_cost_known,
            warnings,
        }
    }

    fn has_complete_standard_components(&self) -> bool {
        self.request_count.is_some()
            && self.input_tokens.is_some()
            && self.output_tokens.is_some()
            && self.cache_read_tokens.is_some()
            && self.cache_creation_tokens.is_some()
    }

    fn cost_was_excluded(&self) -> bool {
        self.warnings.iter().any(|warning| {
            warning.contains("excluded from reported spend")
                || warning.contains("retained only as source correction metadata")
        })
    }
}

fn add_optional(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    left?.checked_add(right?)
}

fn add_cost(left: Option<&str>, right: Option<&str>) -> Option<String> {
    let left = Decimal::from_str(left?).ok()?;
    let right = Decimal::from_str(right?).ok()?;
    if left < Decimal::ZERO || right < Decimal::ZERO {
        return None;
    }
    Some((left + right).to_string())
}

// -------------------------------------------------------------------------
// Public read-side session/task usage API
// -------------------------------------------------------------------------

/// An inclusive Unix-second range used by the session and task queries.
/// Durable buckets are selected by their local calendar date and event-span;
/// raw rows are selected by their event timestamp.  A bucket whose event span
/// crosses either boundary is retained and marked partial because a daily
/// bucket cannot be losslessly sliced into individual events.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageRange {
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
}

impl AgentUsageRange {
    fn validate(&self) -> Result<(), AppError> {
        if let (Some(start), Some(end)) = (self.start_at, self.end_at) {
            if start > end {
                return Err(AppError::InvalidInput(
                    "usage range start_at 不能晚于 end_at".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Durable usage is day-bucketed.  Any local calendar day intersecting the
/// requested range is retained as a whole bucket; the query marks ranged
/// durable contributions partial because a daily bucket cannot be losslessly
/// sliced into individual events.  Raw events are still filtered by their
/// inclusive event timestamp, so the two sources cover the same requested
/// interval without dropping a boundary day's durable facts.
fn rollup_date_bounds_for_range(
    range: &AgentUsageRange,
) -> Result<(Option<String>, Option<String>), AppError> {
    range.validate()?;
    let start = range.start_at.map(local_rollup_date).transpose()?;
    let end = range.end_at.map(local_rollup_date).transpose()?;
    Ok((start, end))
}

fn local_rollup_date(timestamp: i64) -> Result<String, AppError> {
    local_usage_date(timestamp)
        .ok_or_else(|| AppError::InvalidInput(format!("无法解析 usage range 时间戳: {timestamp}")))
}

/// Convert proven Unix-second event time to the local durable-rollup date.
/// Source adapters use the same calendar convention as task range queries.
pub(crate) fn local_usage_date(timestamp: i64) -> Option<String> {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|local| local.date_naive().to_string())
}

/// Request for a root/standalone session summary.  If `session_id` names a
/// normalized child node, the returned `root_session_id` is the normalized
/// root and `requested_session_id` preserves what the caller supplied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionUsageRequest {
    pub app_type: String,
    pub session_id: String,
    #[serde(default)]
    pub range: Option<AgentUsageRange>,
}

/// Root task list filters.  `limit` and `offset` intentionally live in this
/// DTO so the command has one extensible payload and remains forwards
/// compatible when another filter is added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskUsageFilter {
    pub app_type: Option<String>,
    pub title: Option<String>,
    pub project: Option<String>,
    pub project_dir: Option<String>,
    /// Exact native title selected by the task-statistics combobox.
    #[serde(default)]
    pub title_exact: Option<String>,
    /// Exact native project directory selected by the task-statistics combobox.
    #[serde(default)]
    pub project_dir_exact: Option<String>,
    pub range: Option<AgentUsageRange>,
    #[serde(default = "default_task_page_size")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

impl Default for AgentTaskUsageFilter {
    fn default() -> Self {
        Self {
            app_type: None,
            title: None,
            project: None,
            project_dir: None,
            title_exact: None,
            project_dir_exact: None,
            range: None,
            limit: default_task_page_size(),
            offset: 0,
        }
    }
}

/// Scope used to build the complete task-title/project candidate lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskUsageFilterOptionsRequest {
    pub app_type: Option<String>,
    #[serde(default)]
    pub range: Option<AgentUsageRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskUsageProjectOption {
    pub project_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskUsageFilterOptions {
    pub titles: Vec<String>,
    pub projects: Vec<AgentTaskUsageProjectOption>,
}

fn default_task_page_size() -> u32 {
    50
}

impl AgentTaskUsageFilter {
    fn normalized_page(&self) -> (u32, u32) {
        // A zero limit is useful to callers wanting only the total count, but
        // SQLite LIMIT 0 naturally returns no rows.  Clamp oversized pages to
        // keep a malformed UI payload from materializing an unbounded result.
        (self.limit.min(500), self.offset)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageSourceDimension {
    /// Whether this source contribution belongs to a normalized descendant
    /// rather than the selected root session itself.
    pub is_descendant: bool,
    pub provider_id: String,
    pub model: String,
    pub request_model: String,
    pub pricing_model: String,
    pub data_source: String,
    pub input_token_semantics: i64,
    pub source_identity: String,
    pub profile_id: String,
    pub database_identity: String,
    pub base_url_digest: String,
    pub billing_mode: String,
    pub task: String,
    pub source_version: String,
    pub sync_window_start: i64,
    pub sync_window_end: i64,
    pub api_call_count: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub cost_delta_kind: Option<String>,
    pub correction_state: Option<String>,
    pub range_partial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionNodeView {
    pub app_type: String,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub root_session_id: String,
    pub node_kind: SessionNodeKind,
    pub relation_confidence: RelationConfidence,
    pub title: Option<String>,
    pub project_dir: Option<String>,
    pub source_path: Option<String>,
    pub created_at: Option<i64>,
    pub last_active_at: Option<i64>,
    pub last_synced_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionUsageSummary {
    pub app_type: String,
    /// The session ID supplied by the caller.  This is distinct from
    /// `session_id` when a child resolves to a normalized root.
    pub requested_session_id: String,
    /// Canonical root ID used for all aggregation and returned to the UI as
    /// the selected task/session identity.
    pub session_id: String,
    pub root_session_id: String,
    pub root_resolved: bool,
    pub root: Option<AgentSessionNodeView>,
    pub supports_descendants: bool,
    pub self_usage: Option<UsageMeasure>,
    pub descendant_usage: Option<UsageMeasure>,
    pub descendant_usage_status: DescendantUsageStatus,
    pub total_usage: Option<UsageMeasure>,
    pub descendant_session_count: u32,
    pub precision: UsagePrecision,
    pub partial: bool,
    pub warnings: Vec<String>,
    pub source_dimensions: Vec<AgentUsageSourceDimension>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskUsageRow {
    pub app_type: String,
    pub session_id: String,
    pub root_session_id: String,
    pub root: Option<AgentSessionNodeView>,
    pub self_usage: Option<UsageMeasure>,
    pub descendant_usage: Option<UsageMeasure>,
    pub descendant_usage_status: DescendantUsageStatus,
    pub total_usage: Option<UsageMeasure>,
    pub descendant_session_count: u32,
    pub precision: UsagePrecision,
    pub partial: bool,
    pub warnings: Vec<String>,
    pub source_dimensions: Vec<AgentUsageSourceDimension>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskUsagePage {
    pub items: Vec<AgentTaskUsageRow>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
    pub has_more: bool,
    /// Codex proxy requests which are not covered by a verifiable native
    /// session event. This is a summary, not an additional task row.
    #[serde(default)]
    pub unattributed_usage: Option<UsageMeasure>,
    #[serde(default)]
    pub data_status: AgentTaskUsageDataStatus,
}

#[derive(Debug, Clone)]
struct QueryRoot {
    app_type: String,
    root_session_id: String,
    node: Option<AgentSessionNodeView>,
    descendant_session_count: u32,
}

#[derive(Debug, Clone)]
struct UsageGroup {
    root_session_id: String,
    is_descendant: bool,
    measure: UsageMeasure,
    source_dimension: AgentUsageSourceDimension,
}

/// Strip measurement values before comparing source dimensions. The response
/// model itself remains the identity so the grouping contract cannot drift
/// from fields added to the API later.
fn source_dimension_identity(dimension: &AgentUsageSourceDimension) -> AgentUsageSourceDimension {
    let mut identity = dimension.clone();
    identity.is_descendant = false;
    identity.api_call_count = None;
    identity.cache_write_tokens = None;
    identity.reasoning_tokens = None;
    identity.range_partial = false;
    identity
}

fn canonical_query_app_type(value: &str) -> Result<String, AppError> {
    canonical_app_type(value)
}

fn parse_node_kind(value: &str) -> SessionNodeKind {
    SessionNodeKind::from_str(value).unwrap_or(SessionNodeKind::Unknown)
}

fn parse_relation_confidence(value: &str) -> RelationConfidence {
    RelationConfidence::from_str(value).unwrap_or(RelationConfidence::Unavailable)
}

fn node_view_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentSessionNodeView> {
    let app_type: String = row.get(0)?;
    let session_id: String = row.get(1)?;
    let root_session_id: String = row.get(3)?;
    let node_kind: String = row.get(4)?;
    let relation_confidence: String = row.get(5)?;
    Ok(AgentSessionNodeView {
        app_type,
        session_id,
        parent_session_id: row.get(2)?,
        root_session_id,
        node_kind: parse_node_kind(&node_kind),
        relation_confidence: parse_relation_confidence(&relation_confidence),
        title: row.get(6)?,
        project_dir: row.get(7)?,
        source_path: row.get(8)?,
        created_at: row.get(9)?,
        last_active_at: row.get(10)?,
        last_synced_at: row.get(11)?,
    })
}

fn load_node(
    conn: &Connection,
    app_type: &str,
    session_id: &str,
) -> Result<Option<AgentSessionNodeView>, AppError> {
    conn.query_row(
        "SELECT app_type, session_id, parent_session_id, root_session_id,
                node_kind, relation_confidence, title, project_dir, source_path,
                created_at, last_active_at, last_synced_at
         FROM agent_session_nodes WHERE app_type = ?1 AND session_id = ?2",
        params![app_type, session_id],
        node_view_from_row,
    )
    .optional()
    .map_err(AppError::from)
}

fn resolve_query_root(
    conn: &Connection,
    app_type: &str,
    requested_session_id: &str,
) -> Result<QueryRoot, AppError> {
    if let Some(node) = load_node(conn, app_type, requested_session_id)? {
        let root_id = if node.node_kind == SessionNodeKind::Child {
            node.root_session_id.clone()
        } else {
            // Normalized unknown/conflict/self-only nodes are deliberately
            // self-rooted.  Never trust a non-child node's root field to adopt
            // it into another task.
            node.session_id.clone()
        };
        let root_node = load_node(conn, app_type, &root_id)?;
        let count = descendant_count(conn, app_type, &root_id)?;
        return Ok(QueryRoot {
            app_type: app_type.to_string(),
            root_session_id: root_id,
            node: if root_node.is_some() || node.node_kind != SessionNodeKind::Child {
                root_node.or(Some(node))
            } else {
                // A child with a missing root metadata row is still resolved
                // safely, but its child metadata must not be presented as the
                // root task metadata.
                None
            },
            descendant_session_count: count,
        });
    }

    // A source row may predate node discovery.  It is a safe standalone root
    // rather than an inferred child of a nearby title/time.
    Ok(QueryRoot {
        app_type: app_type.to_string(),
        root_session_id: requested_session_id.to_string(),
        node: None,
        descendant_session_count: 0,
    })
}

fn descendant_count(
    conn: &Connection,
    app_type: &str,
    root_session_id: &str,
) -> Result<u32, AppError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT session_id)
         FROM agent_session_nodes
         WHERE app_type = ?1 AND root_session_id = ?2
           AND session_id <> ?2 AND node_kind = 'child'",
        params![app_type, root_session_id],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as u32)
}

fn usage_measure_from_group_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageGroup> {
    let root_session_id: String = row.get(0)?;
    let is_descendant = row.get::<_, i64>(1)? != 0;
    let provider_id: String = row.get(2)?;
    let model: String = row.get(3)?;
    let request_model: String = row.get(4)?;
    let pricing_model: String = row.get(5)?;
    let data_source: String = row.get(6)?;
    let precision_text: String = row.get(7)?;
    let time_text: String = row.get(8)?;
    let count_text: String = row.get(9)?;
    let input_token_semantics: i64 = row.get(10)?;
    let source_identity: String = row.get(11)?;
    let profile_id: String = row.get(12)?;
    let database_identity: String = row.get(13)?;
    let base_url_digest: String = row.get(14)?;
    let billing_mode: String = row.get(15)?;
    let task: String = row.get(16)?;
    let source_version: String = row.get(17)?;
    let sync_window_start: i64 = row.get(18)?;
    let sync_window_end: i64 = row.get(19)?;
    let request_count: Option<i64> = row.get(20)?;
    let api_call_count: Option<i64> = row.get(21)?;
    let input_tokens: Option<i64> = row.get(22)?;
    let output_tokens: Option<i64> = row.get(23)?;
    let cache_read_tokens: Option<i64> = row.get(24)?;
    let cache_creation_tokens: Option<i64> = row.get(25)?;
    let cache_write_tokens: Option<i64> = row.get(26)?;
    let reasoning_tokens: Option<i64> = row.get(27)?;
    let total_cost_values: Option<String> = row.get(28)?;
    let cost_status: Option<String> = row.get(29)?;
    let cost_source: Option<String> = row.get(30)?;
    let cost_delta_kind: Option<String> = row.get(31)?;
    let correction_state: Option<String> = row.get(32)?;
    let _first_event_at: Option<i64> = row.get(33)?;
    let _last_event_at: Option<i64> = row.get(34)?;
    let range_partial: i64 = row.get(35)?;
    let cost_row_count: i64 = row.get(36)?;
    let cost_known_count: i64 = row.get(37)?;

    let precision = UsagePrecision::from_str(&precision_text).unwrap_or_default();
    let time_semantics = TimeSemantics::from_str(&time_text).unwrap_or_default();
    let request_count_semantics = RequestCountSemantics::from_str(&count_text).unwrap_or_default();
    let (total_cost_usd, cost_partial, cost_warnings) = aggregate_reported_cost(
        total_cost_values.as_deref(),
        cost_row_count,
        cost_known_count,
        cost_delta_kind.as_deref(),
        correction_state.as_deref(),
    );
    let mut warnings = cost_warnings;
    if precision == UsagePrecision::Unavailable {
        warnings.push("usage fact precision is unavailable".to_string());
    }
    if range_partial != 0 {
        warnings.push(
            "daily usage bucket intersects the requested range and cannot be sliced exactly"
                .to_string(),
        );
    }
    if data_source.ends_with("_session") && total_cost_usd.is_none() {
        warnings.push("source cost is unavailable or ambiguous".to_string());
    }
    let cost_missing = total_cost_usd.is_none();
    let measure = UsageMeasure {
        data_source: Some(data_source.clone()),
        request_count,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        total_cost_usd,
        precision,
        time_semantics,
        request_count_semantics,
        partial: range_partial != 0
            || request_count.is_none()
            || input_tokens.is_none()
            || output_tokens.is_none()
            || cache_read_tokens.is_none()
            || cache_creation_tokens.is_none()
            || cost_partial
            || cost_missing,
        warnings,
    };
    Ok(UsageGroup {
        root_session_id,
        is_descendant,
        measure,
        source_dimension: AgentUsageSourceDimension {
            is_descendant,
            provider_id,
            model,
            request_model,
            pricing_model,
            data_source,
            input_token_semantics,
            source_identity,
            profile_id,
            database_identity,
            base_url_digest,
            billing_mode,
            task,
            source_version,
            sync_window_start,
            sync_window_end,
            api_call_count,
            cache_write_tokens,
            reasoning_tokens,
            cost_status,
            cost_source,
            cost_delta_kind,
            correction_state,
            range_partial: range_partial != 0,
        },
    })
}

fn combine_measures(
    groups: impl Iterator<Item = UsageGroup>,
) -> (Option<UsageMeasure>, Vec<AgentUsageSourceDimension>) {
    let mut measure: Option<UsageMeasure> = None;
    let mut dimensions = Vec::new();
    for group in groups {
        dimensions.push(group.source_dimension);
        measure = Some(match measure {
            Some(existing) => existing.combine(&group.measure),
            None => group.measure,
        });
    }
    if let Some(measure) = measure.as_mut() {
        let distinct_sources = dimensions
            .iter()
            .map(source_dimension_identity)
            .collect::<HashSet<_>>();
        if distinct_sources.len() > 1 {
            measure.partial = true;
            measure.warnings.push(
                "multiple source dimensions were combined; inspect sourceDimensions for detail"
                    .into(),
            );
        }
    }
    (measure, dimensions)
}

fn weakest_precision(
    self_usage: Option<&UsageMeasure>,
    descendant_usage: Option<&UsageMeasure>,
    total_usage: Option<&UsageMeasure>,
) -> UsagePrecision {
    [self_usage, descendant_usage, total_usage]
        .into_iter()
        .flatten()
        .map(|usage| usage.precision)
        .max_by_key(|precision| precision.rank())
        .unwrap_or(UsagePrecision::Unavailable)
}

fn summary_for_root(
    root: &QueryRoot,
    requested_session_id: &str,
    groups: Vec<UsageGroup>,
    supports_descendants: bool,
    range: Option<&AgentUsageRange>,
) -> AgentSessionUsageSummary {
    let (self_usage, mut source_dimensions) =
        combine_measures(groups.iter().filter(|group| !group.is_descendant).cloned());
    let (descendant_usage, descendant_dimensions) = if supports_descendants {
        combine_measures(groups.iter().filter(|group| group.is_descendant).cloned())
    } else {
        (None, Vec::new())
    };
    let self_dimension_identities = groups
        .iter()
        .filter(|group| !group.is_descendant)
        .map(|group| source_dimension_identity(&group.source_dimension))
        .collect::<HashSet<_>>();
    let descendant_dimension_identities = groups
        .iter()
        .filter(|group| group.is_descendant)
        .map(|group| source_dimension_identity(&group.source_dimension))
        .collect::<HashSet<_>>();
    let self_descendant_dimensions_differ = !self_dimension_identities.is_empty()
        && !descendant_dimension_identities.is_empty()
        && self_dimension_identities != descendant_dimension_identities;
    source_dimensions.extend(descendant_dimensions);
    let visible_descendant_usage = if supports_descendants {
        descendant_usage.as_ref()
    } else {
        None
    };
    let descendant_usage_status = if !supports_descendants || root.descendant_session_count == 0 {
        DescendantUsageStatus::NotApplicable
    } else if visible_descendant_usage.is_some() {
        DescendantUsageStatus::Available
    } else if range.is_some() {
        DescendantUsageStatus::NoActivityInRange
    } else {
        DescendantUsageStatus::Unavailable
    };
    let mut total_usage = match (&self_usage, visible_descendant_usage) {
        (Some(self_usage), Some(descendant_usage)) => Some(self_usage.combine(descendant_usage)),
        (Some(self_usage), None) => Some(self_usage.clone()),
        (None, Some(descendant_usage)) => Some(descendant_usage.clone()),
        (None, None) => None,
    };
    if let (Some(_self_usage), Some(_descendant_usage), Some(total_usage)) =
        (&self_usage, visible_descendant_usage, total_usage.as_mut())
    {
        if self_descendant_dimensions_differ {
            total_usage.partial = true;
            total_usage.warnings.push(
                "self and descendant usage use different source dimensions; inspect sourceDimensions"
                    .into(),
            );
        }
    }
    if descendant_usage_status == DescendantUsageStatus::Unavailable {
        if let Some(total_usage) = total_usage.as_mut() {
            total_usage.partial = true;
            total_usage.total_cost_usd = None;
            total_usage
                .warnings
                .push("descendant usage is unavailable; total is a known lower bound".into());
        }
    }
    let mut warnings = Vec::new();
    let root_resolved = requested_session_id != root.root_session_id;
    if root_resolved {
        warnings.push(format!(
            "requested child session resolved to normalized root '{}'",
            root.root_session_id
        ));
    }
    if !supports_descendants && root.descendant_session_count > 0 {
        warnings.push("source capability is self-only; descendants are not included".into());
    }
    if descendant_usage_status == DescendantUsageStatus::Unavailable {
        warnings.push("descendant usage is unavailable in the selected range".into());
    }
    if total_usage.is_none() {
        warnings.push("usage is unavailable for this session in the selected range".into());
    }
    if let Some(usage) = &total_usage {
        warnings.extend(usage.warnings.iter().cloned());
    }
    let partial = total_usage.is_none()
        || total_usage.as_ref().is_some_and(|usage| usage.partial)
        || root.node.as_ref().is_some_and(|node| {
            matches!(
                node.node_kind,
                SessionNodeKind::Conflict | SessionNodeKind::Unknown
            )
        });
    let precision = weakest_precision(
        self_usage.as_ref(),
        descendant_usage.as_ref(),
        total_usage.as_ref(),
    );
    AgentSessionUsageSummary {
        app_type: root.app_type.clone(),
        requested_session_id: requested_session_id.to_string(),
        session_id: root.root_session_id.clone(),
        root_session_id: root.root_session_id.clone(),
        root_resolved,
        root: root.node.clone(),
        supports_descendants,
        self_usage,
        descendant_usage: if supports_descendants {
            descendant_usage
        } else {
            None
        },
        descendant_usage_status,
        total_usage,
        descendant_session_count: if supports_descendants {
            root.descendant_session_count
        } else {
            0
        },
        precision,
        partial,
        warnings,
        source_dimensions,
    }
}

fn query_usage_groups(
    conn: &Connection,
    app_type: &str,
    root_ids: &[String],
    range: Option<&AgentUsageRange>,
) -> Result<Vec<UsageGroup>, AppError> {
    if root_ids.is_empty() {
        return Ok(Vec::new());
    }
    let range = range.cloned().unwrap_or_default();
    range.validate()?;
    let (start_day, end_day) = rollup_date_bounds_for_range(&range)?;
    let mut params_vec: Vec<Box<dyn ToSql>> = Vec::new();
    let mut rollup_conditions = vec!["r.app_type = ?".to_string()];
    params_vec.push(Box::new(app_type.to_string()));
    if let Some(start_day) = start_day {
        rollup_conditions.push("r.date >= ?".to_string());
        params_vec.push(Box::new(start_day));
    }
    if let Some(end_day) = end_day {
        rollup_conditions.push("r.date <= ?".to_string());
        params_vec.push(Box::new(end_day));
    }
    // A daily bucket is retained when its known event span intersects the
    // requested range.  Unknown spans remain visible but are marked partial.
    if let Some(start_at) = range.start_at {
        rollup_conditions.push("(r.last_event_at IS NULL OR r.last_event_at >= ?)".into());
        params_vec.push(Box::new(start_at));
    }
    if let Some(end_at) = range.end_at {
        rollup_conditions.push("(r.first_event_at IS NULL OR r.first_event_at <= ?)".into());
        params_vec.push(Box::new(end_at));
    }
    let raw_filter = crate::services::usage_stats::effective_session_usage_log_filter("l");
    let mut raw_conditions = vec![
        "l.app_type = ?".to_string(),
        "l.session_id IS NOT NULL".to_string(),
        "TRIM(l.session_id) <> ''".to_string(),
        raw_filter,
    ];
    params_vec.push(Box::new(app_type.to_string()));
    if let Some(start_at) = range.start_at {
        raw_conditions.push("l.created_at >= ?".into());
        params_vec.push(Box::new(start_at));
    }
    if let Some(end_at) = range.end_at {
        raw_conditions.push("l.created_at <= ?".into());
        params_vec.push(Box::new(end_at));
    }
    let root_placeholders = std::iter::repeat_n("?", root_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut root_filter_params: Vec<Box<dyn ToSql>> = root_ids
        .iter()
        .map(|root| Box::new(root.clone()) as Box<dyn ToSql>)
        .collect();
    let rollup_partial = if range.start_at.is_some() || range.end_at.is_some() {
        // A durable row is a daily/session bucket rather than a set of
        // individually sliceable events.  Even when its span is known, a
        // caller-supplied timestamp range can cut through that bucket.  Mark
        // ranged durable contributions partial so the UI never presents a
        // day-level total as timestamp-exact.
        "1"
    } else {
        "0"
    };
    let rollup_input_sql = fresh_input_sql("r");
    let raw_input_sql = fresh_input_sql("l");
    let sql = format!(
        "WITH node_map AS (
             SELECT app_type, session_id,
                    CASE WHEN node_kind = 'child' AND root_session_id <> session_id
                         THEN root_session_id ELSE session_id END AS root_session_id,
                    CASE WHEN node_kind = 'child' AND root_session_id <> session_id
                         THEN 1 ELSE 0 END AS is_descendant
             FROM agent_session_nodes
         ), contributions AS (
             SELECT COALESCE(n.root_session_id, r.session_id) AS root_session_id,
                    COALESCE(n.is_descendant, 0) AS is_descendant,
                    r.provider_id, r.model, r.request_model, r.pricing_model,
                    r.data_source, r.precision, r.time_semantics,
                    r.request_count_semantics, r.input_token_semantics,
                    r.source_identity, r.profile_id, r.database_identity,
                    r.base_url_digest, r.billing_mode, r.task, r.source_version,
                    r.sync_window_start, r.sync_window_end,
                    r.request_count, r.api_call_count, {rollup_input_sql} AS input_tokens,
                    r.output_tokens, r.cache_read_tokens, r.cache_creation_tokens,
                    r.cache_write_tokens, r.reasoning_tokens, r.total_cost_usd,
                    r.cost_status, r.cost_source, r.cost_delta_kind, r.correction_state,
                    r.first_event_at, r.last_event_at, {rollup_partial} AS range_partial
             FROM agent_session_usage_rollups r
             LEFT JOIN node_map n ON n.app_type = r.app_type AND n.session_id = r.session_id
             WHERE {rollup_where}
             UNION ALL
             SELECT COALESCE(n.root_session_id, l.session_id) AS root_session_id,
                    COALESCE(n.is_descendant, 0) AS is_descendant,
                    l.provider_id, l.model, COALESCE(l.request_model, ''),
                    COALESCE(l.pricing_model, ''), COALESCE(l.data_source, 'proxy'),
                    CASE WHEN l.app_type IN ('openclaw') THEN 'unavailable'
                         ELSE 'request_exact' END,
                    CASE WHEN l.app_type IN ('openclaw') THEN 'unavailable'
                         ELSE 'event_time' END,
                    CASE WHEN COALESCE(l.data_source, 'proxy') = 'proxy' THEN 'http_request'
                         WHEN l.app_type IN ('codex', 'grokbuild') THEN 'agent_call'
                         WHEN l.app_type IN ('pi') THEN 'usage_event'
                         WHEN l.app_type IN ('openclaw') THEN 'unavailable'
                         ELSE 'assistant_message' END,
                    l.input_token_semantics, '', '', '', '', '', '', '', 0, 0,
                    1, NULL,
                    CASE WHEN COALESCE(l.data_source, 'proxy') IN
                                   ('session_log', 'codex_session', 'gemini_session')
                         AND l.input_tokens = 0
                         THEN NULL ELSE ({raw_input_sql}) END,
                    CASE WHEN COALESCE(l.data_source, 'proxy') IN
                                   ('session_log', 'codex_session', 'gemini_session')
                                   AND l.output_tokens = 0
                         THEN NULL ELSE l.output_tokens END,
                    CASE WHEN COALESCE(l.data_source, 'proxy') IN
                                   ('session_log', 'codex_session', 'gemini_session')
                                   AND l.cache_read_tokens = 0
                         THEN NULL ELSE l.cache_read_tokens END,
                    CASE WHEN COALESCE(l.data_source, 'proxy') IN
                                   ('codex_session', 'gemini_session', 'grok_session')
                         THEN NULL
                         WHEN COALESCE(l.data_source, 'proxy') = 'session_log'
                                   AND l.cache_creation_tokens = 0
                         THEN NULL
                         ELSE l.cache_creation_tokens END,
                    NULL, NULL,
                    CASE WHEN COALESCE(l.data_source, 'proxy') IN
                                   ('session_log', 'codex_session', 'gemini_session',
                                    'grok_session', 'opencode_session')
                         THEN NULL ELSE l.total_cost_usd END,
                    NULL, NULL, NULL, NULL,
                    l.created_at, l.created_at, 0 AS range_partial
             FROM proxy_request_logs l
             LEFT JOIN node_map n ON n.app_type = l.app_type AND n.session_id = l.session_id
             WHERE {raw_where}
         ), filtered AS (
             SELECT * FROM contributions
             WHERE root_session_id IN ({root_placeholders})
         )
         SELECT root_session_id, is_descendant,
                provider_id, model, request_model, pricing_model, data_source,
                precision, time_semantics, request_count_semantics,
                input_token_semantics, source_identity, profile_id, database_identity, base_url_digest,
                billing_mode, task, source_version, sync_window_start,
                sync_window_end,
                CASE WHEN COUNT(request_count) = COUNT(*) THEN SUM(request_count) END,
                CASE WHEN COUNT(api_call_count) = COUNT(*) THEN SUM(api_call_count) END,
                CASE WHEN COUNT(input_tokens) = COUNT(*) THEN SUM(input_tokens) END,
                CASE WHEN COUNT(output_tokens) = COUNT(*) THEN SUM(output_tokens) END,
                CASE WHEN COUNT(cache_read_tokens) = COUNT(*) THEN SUM(cache_read_tokens) END,
                CASE WHEN COUNT(cache_creation_tokens) = COUNT(*) THEN SUM(cache_creation_tokens) END,
                CASE WHEN COUNT(cache_write_tokens) = COUNT(*) THEN SUM(cache_write_tokens) END,
                CASE WHEN COUNT(reasoning_tokens) = COUNT(*) THEN SUM(reasoning_tokens) END,
                CASE WHEN COUNT(total_cost_usd) > 0
                     THEN GROUP_CONCAT(CAST(total_cost_usd AS TEXT), char(31)) END,
                cost_status, cost_source, cost_delta_kind, correction_state,
                MIN(first_event_at), MAX(last_event_at), MAX(range_partial),
                COUNT(*), COUNT(total_cost_usd)
         FROM filtered
         GROUP BY root_session_id, is_descendant, provider_id, model,
                  request_model, pricing_model, data_source, precision,
                  time_semantics, request_count_semantics, input_token_semantics,
                  source_identity,
                  profile_id, database_identity, base_url_digest, billing_mode,
                  task, source_version, sync_window_start, sync_window_end,
                  cost_status, cost_source, cost_delta_kind, correction_state
         ORDER BY root_session_id, is_descendant, data_source, model",
        rollup_where = rollup_conditions.join(" AND "),
        raw_where = raw_conditions.join(" AND "),
    );
    params_vec.append(&mut root_filter_params);
    let refs: Vec<&dyn ToSql> = params_vec.iter().map(|value| value.as_ref()).collect();
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(refs.as_slice(), usage_measure_from_group_row)?;
    let mut groups = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)?;
    enrich_codex_session_costs(conn, app_type, &mut groups);
    Ok(groups)
}

/// Fill a query-time API-equivalent estimate for Codex session facts when the
/// source did not report a billable dollar amount.  This intentionally does
/// not write back to durable usage rows: changing the model-pricing table must
/// affect the next read without a migration or a stale historical cache.
fn enrich_codex_session_costs(conn: &Connection, app_type: &str, groups: &mut [UsageGroup]) {
    if app_type != "codex" {
        return;
    }

    for group in groups {
        if group.measure.total_cost_usd.is_some()
            || group.source_dimension.data_source != "codex_session"
            || has_cost_correction_metadata(
                group.source_dimension.cost_delta_kind.as_deref(),
                group.source_dimension.correction_state.as_deref(),
            )
        {
            continue;
        }

        let candidates = [
            group.source_dimension.pricing_model.trim(),
            group.source_dimension.model.trim(),
            group.source_dimension.request_model.trim(),
        ];
        let pricing = candidates
            .into_iter()
            .filter(|model| !model.is_empty() && *model != "unknown")
            .find_map(|model| find_exact_model_pricing(conn, model));

        let Some((pricing_model, pricing)) = pricing else {
            group.source_dimension.cost_status = Some("unavailable".into());
            group.source_dimension.cost_source = Some("model_pricing".into());
            continue;
        };

        let (Some(input), Some(output), Some(cache_read)) = (
            group.measure.input_tokens,
            group.measure.output_tokens,
            group.measure.cache_read_tokens,
        ) else {
            group.source_dimension.cost_status = Some("unavailable".into());
            group.source_dimension.cost_source = Some("model_pricing".into());
            continue;
        };

        // Legacy and total Codex facts normalize input as source input minus
        // cache reads, so the remainder is priced as regular input exactly
        // once. Cache creation is an independent optional source component;
        // when present it is priced separately without changing normalized
        // input, and when absent it remains unknown rather than becoming zero.
        let cache_creation = group.measure.cache_creation_tokens;

        let million = Decimal::from(1_000_000i64);
        let mut total = Decimal::from(input) * pricing.input_cost_per_million / million
            + Decimal::from(output) * pricing.output_cost_per_million / million
            + Decimal::from(cache_read) * pricing.cache_read_cost_per_million / million;
        if let Some(cache_creation) = cache_creation {
            total +=
                Decimal::from(cache_creation) * pricing.cache_creation_cost_per_million / million;
        }

        group.measure.total_cost_usd = Some(total.to_string());
        group.source_dimension.pricing_model = pricing_model.to_string();
        group.source_dimension.cost_status = Some("estimated".into());
        group.source_dimension.cost_source = Some("model_pricing".into());
    }
}

fn append_root_metadata_filter(
    conditions: &mut Vec<String>,
    params_vec: &mut Vec<Box<dyn ToSql>>,
    title: Option<&str>,
    project: Option<&str>,
    title_exact: Option<&str>,
    project_dir_exact: Option<&str>,
) {
    if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
        conditions.push("LOWER(COALESCE(root_node.title, '')) LIKE '%' || LOWER(?) || '%'".into());
        params_vec.push(Box::new(title.trim().to_string()));
    }
    if let Some(project) = project.filter(|value| !value.trim().is_empty()) {
        conditions
            .push("LOWER(COALESCE(root_node.project_dir, '')) LIKE '%' || LOWER(?) || '%'".into());
        params_vec.push(Box::new(project.trim().to_string()));
    }
    if let Some(title) = title_exact.filter(|value| !value.trim().is_empty()) {
        conditions.push("LOWER(COALESCE(root_node.title, '')) = LOWER(?)".into());
        params_vec.push(Box::new(title.trim().to_string()));
    }
    if let Some(project_dir) = project_dir_exact.filter(|value| !value.trim().is_empty()) {
        conditions.push("LOWER(COALESCE(root_node.project_dir, '')) = LOWER(?)".into());
        params_vec.push(Box::new(project_dir.trim().to_string()));
    }
}

fn has_task_metadata_filter(filter: &AgentTaskUsageFilter) -> bool {
    [
        filter.title.as_deref(),
        filter.project.as_deref(),
        filter.project_dir.as_deref(),
        filter.title_exact.as_deref(),
        filter.project_dir_exact.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| !value.trim().is_empty())
}

fn codex_published_snapshot_on_conn(conn: &Connection) -> Result<bool, AppError> {
    let (nodes, rollups): (i64, i64) = conn.query_row(
        "SELECT
            (SELECT COUNT(*) FROM agent_session_nodes WHERE app_type = 'codex'),
            (SELECT COUNT(*) FROM agent_session_usage_rollups WHERE app_type = 'codex')",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(nodes > 0 && rollups > 0)
}

fn codex_task_data_status_on_conn(conn: &Connection) -> Result<AgentTaskUsageDataStatus, AppError> {
    if !crate::services::session_usage_codex::codex_replay_in_progress_on_conn(conn) {
        return Ok(AgentTaskUsageDataStatus::Ready);
    }
    if codex_published_snapshot_on_conn(conn)? {
        Ok(AgentTaskUsageDataStatus::RebuildingWithSnapshot)
    } else {
        Ok(AgentTaskUsageDataStatus::Rebuilding)
    }
}

fn task_data_status_on_conn(
    conn: &Connection,
    app_type: Option<&str>,
) -> Result<AgentTaskUsageDataStatus, AppError> {
    let app_type = app_type.map(canonical_query_app_type).transpose()?;
    if app_type
        .as_deref()
        .is_some_and(|app_type| app_type != "codex")
    {
        return Ok(AgentTaskUsageDataStatus::Ready);
    }
    codex_task_data_status_on_conn(conn)
}

/// Shared source scope for both the paged task list and its unpaged metadata
/// filter options. Keeping the candidate CTE and its positional parameters in
/// one place prevents the two views from drifting on range, raw ownership, or
/// the pending-Codex replay boundary.
struct TaskSourceScope {
    app_filter: Option<String>,
    has_time_bound: bool,
    hide_pending_codex: bool,
    rollup_conditions: Vec<String>,
    raw_conditions: Vec<String>,
    params: Vec<Box<dyn ToSql>>,
}

impl TaskSourceScope {
    fn new(
        conn: &Connection,
        app_type: Option<&str>,
        requested_range: Option<&AgentUsageRange>,
    ) -> Result<Self, AppError> {
        let range = requested_range.cloned().unwrap_or_default();
        range.validate()?;
        let (start_day, end_day) = rollup_date_bounds_for_range(&range)?;
        let app_filter = app_type.map(canonical_query_app_type).transpose()?;
        let codex_replay_in_progress =
            crate::services::session_usage_codex::codex_replay_in_progress_on_conn(conn);
        let hide_pending_codex =
            codex_replay_in_progress && !codex_published_snapshot_on_conn(conn)?;
        let mut rollup_conditions = vec!["1 = 1".to_string()];
        let mut raw_conditions = vec![
            "l.session_id IS NOT NULL".to_string(),
            "TRIM(l.session_id) <> ''".to_string(),
            crate::services::usage_stats::effective_session_usage_log_filter("l"),
        ];
        if hide_pending_codex && app_filter.is_none() {
            rollup_conditions.push("r.app_type <> 'codex'".into());
            raw_conditions.push("l.app_type <> 'codex'".into());
        }

        // SQL placeholder order always follows the CTE: durable facts first,
        // then raw compatibility rows. Query-specific metadata filters are
        // appended by the list view after this complete source scope.
        let mut rollup_params: Vec<Box<dyn ToSql>> = Vec::new();
        let mut raw_params: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(app_type) = &app_filter {
            rollup_conditions.push("r.app_type = ?".into());
            rollup_params.push(Box::new(app_type.clone()));
            raw_conditions.push("l.app_type = ?".into());
            raw_params.push(Box::new(app_type.clone()));
        }
        if let Some(start_day) = start_day {
            rollup_conditions.push("r.date >= ?".into());
            rollup_params.push(Box::new(start_day));
        }
        if let Some(end_day) = end_day {
            rollup_conditions.push("r.date <= ?".into());
            rollup_params.push(Box::new(end_day));
        }
        if let Some(start_at) = range.start_at {
            rollup_conditions.push("(r.last_event_at IS NULL OR r.last_event_at >= ?)".into());
            rollup_params.push(Box::new(start_at));
            raw_conditions.push("l.created_at >= ?".into());
            raw_params.push(Box::new(start_at));
        }
        if let Some(end_at) = range.end_at {
            rollup_conditions.push("(r.first_event_at IS NULL OR r.first_event_at <= ?)".into());
            rollup_params.push(Box::new(end_at));
            raw_conditions.push("l.created_at <= ?".into());
            raw_params.push(Box::new(end_at));
        }
        rollup_params.extend(raw_params);

        Ok(Self {
            app_filter,
            has_time_bound: range.start_at.is_some() || range.end_at.is_some(),
            hide_pending_codex,
            rollup_conditions,
            raw_conditions,
            params: rollup_params,
        })
    }

    fn blocks_requested_codex(&self) -> bool {
        self.hide_pending_codex && self.app_filter.as_deref() == Some("codex")
    }

    fn candidate_ctes(&self) -> String {
        let exclude_pending_codex = if self.hide_pending_codex && self.app_filter.is_none() {
            "AND n.app_type <> 'codex'"
        } else {
            ""
        };
        let candidates = if self.has_time_bound {
            // A ranged task view is usage-driven: node metadata alone cannot
            // prove a contribution inside the requested event interval.
            "SELECT app_type, root_session_id, source_activity_at FROM source_roots"
        } else {
            "SELECT app_type, root_session_id, source_activity_at FROM node_roots
             UNION
             SELECT app_type, root_session_id, source_activity_at FROM source_roots"
        };
        format!(
            "WITH node_roots AS (
                 SELECT n.app_type, n.session_id AS root_session_id,
                        CAST(NULL AS INTEGER) AS source_activity_at
                 FROM agent_session_nodes n
                 WHERE n.node_kind <> 'child' {exclude_pending_codex}
                 UNION
                 SELECT n.app_type, n.root_session_id,
                        CAST(NULL AS INTEGER) AS source_activity_at
                 FROM agent_session_nodes n
                 WHERE n.node_kind = 'child' {exclude_pending_codex}
             ), source_roots AS (
                 SELECT r.app_type,
                        CASE WHEN n.node_kind = 'child' AND n.root_session_id <> r.session_id
                             THEN n.root_session_id ELSE r.session_id END AS root_session_id,
                        COALESCE(r.last_event_at, r.first_event_at, 0) AS source_activity_at
                 FROM agent_session_usage_rollups r
                 LEFT JOIN agent_session_nodes n
                   ON n.app_type = r.app_type AND n.session_id = r.session_id
                 WHERE {rollup_where}
                 UNION
                 SELECT l.app_type,
                        CASE WHEN n.node_kind = 'child' AND n.root_session_id <> l.session_id
                             THEN n.root_session_id ELSE l.session_id END AS root_session_id,
                        l.created_at AS source_activity_at
                 FROM proxy_request_logs l
                 LEFT JOIN agent_session_nodes n
                   ON n.app_type = l.app_type AND n.session_id = l.session_id
                 WHERE {raw_where}
                   AND (l.app_type <> 'codex' OR n.session_id IS NOT NULL)
             ), candidates AS (
                 {candidates}
             )",
            rollup_where = self.rollup_conditions.join(" AND "),
            raw_where = self.raw_conditions.join(" AND "),
        )
    }

    fn into_params(self) -> Vec<Box<dyn ToSql>> {
        self.params
    }
}

fn query_task_roots(
    conn: &Connection,
    filter: &AgentTaskUsageFilter,
) -> Result<(Vec<QueryRoot>, u64), AppError> {
    let (limit, offset) = filter.normalized_page();
    let source_scope =
        TaskSourceScope::new(conn, filter.app_type.as_deref(), filter.range.as_ref())?;
    if source_scope.blocks_requested_codex() {
        return Ok((Vec::new(), 0));
    }
    let app_filter = source_scope.app_filter.clone();
    let source_ctes = source_scope.candidate_ctes();
    let mut params_vec = source_scope.into_params();
    let mut node_conditions = vec!["1 = 1".to_string()];
    let mut node_params: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(app_type) = &app_filter {
        node_conditions.push("root_node.app_type = ?".into());
        node_params.push(Box::new(app_type.clone()));
    }
    append_root_metadata_filter(
        &mut node_conditions,
        &mut node_params,
        filter.title.as_deref(),
        filter
            .project
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(filter
                .project_dir
                .as_deref()
                .filter(|value| !value.trim().is_empty())),
        filter.title_exact.as_deref(),
        filter.project_dir_exact.as_deref(),
    );
    let has_text_filter = has_task_metadata_filter(filter);
    params_vec.extend(node_params);
    let sql = format!(
        "{source_ctes}, filtered AS (
             SELECT c.app_type, c.root_session_id,
                    root_node.node_kind, root_node.relation_confidence,
                    root_node.title, root_node.project_dir,
                    root_node.created_at, root_node.last_active_at,
                    CASE
                        WHEN COALESCE(root_node.last_active_at, root_node.created_at,
                                      MAX(c.source_activity_at), 0)
                             > 100000000000
                        THEN COALESCE(root_node.last_active_at, root_node.created_at,
                                      MAX(c.source_activity_at), 0) / 1000
                        ELSE COALESCE(root_node.last_active_at, root_node.created_at,
                                      MAX(c.source_activity_at), 0)
                    END AS sort_timestamp,
                    COUNT(DISTINCT CASE
                        WHEN child.node_kind = 'child'
                         AND child.root_session_id = c.root_session_id
                         AND child.session_id <> c.root_session_id
                        THEN child.session_id END) AS descendant_count
             FROM candidates c
             LEFT JOIN agent_session_nodes root_node
               ON root_node.app_type = c.app_type
              AND root_node.session_id = c.root_session_id
             LEFT JOIN agent_session_nodes child
               ON child.app_type = c.app_type
              AND child.root_session_id = c.root_session_id
             WHERE {node_filter}
             GROUP BY c.app_type, c.root_session_id, root_node.node_kind,
                      root_node.relation_confidence, root_node.title,
                      root_node.project_dir, root_node.created_at,
                      root_node.last_active_at
         )
         SELECT app_type, root_session_id, descendant_count,
                sort_timestamp,
                COUNT(*) OVER () AS total_count
         FROM filtered
         ORDER BY sort_timestamp DESC,
                  app_type ASC, root_session_id ASC
         LIMIT ? OFFSET ?",
        node_filter = if has_text_filter {
            format!("({})", node_conditions.join(" AND "))
        } else {
            format!(
                "({} OR root_node.session_id IS NULL)",
                node_conditions.join(" AND ")
            )
        },
    );
    params_vec.push(Box::new(limit as i64));
    params_vec.push(Box::new(offset as i64));
    let refs: Vec<&dyn ToSql> = params_vec.iter().map(|value| value.as_ref()).collect();
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query(refs.as_slice())?;
    let mut raw_roots = Vec::new();
    while let Some(row) = rows.next()? {
        let app_type: String = row.get(0)?;
        let root_session_id: String = row.get(1)?;
        let descendant_count: i64 = row.get(2)?;
        raw_roots.push((app_type, root_session_id, descendant_count));
    }
    drop(rows);
    drop(statement);
    drop(refs);
    let count_sql = format!(
        "SELECT COUNT(*) FROM ({})",
        sql.strip_suffix("LIMIT ? OFFSET ?").unwrap_or(&sql)
    );
    let count_refs: Vec<&dyn ToSql> = params_vec[..params_vec.len() - 2]
        .iter()
        .map(|value| value.as_ref())
        .collect();
    let total_count: i64 = conn.query_row(&count_sql, count_refs.as_slice(), |row| row.get(0))?;
    let mut roots = Vec::with_capacity(raw_roots.len());
    for (app_type, root_session_id, descendant_count) in raw_roots {
        let node = load_node(conn, &app_type, &root_session_id)?;
        roots.push(QueryRoot {
            app_type,
            root_session_id,
            node,
            descendant_session_count: descendant_count.max(0) as u32,
        });
    }
    Ok((roots, total_count.max(0) as u64))
}

type UnattributedCodexProxyAggregate = (
    i64,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    i64,
);

fn query_unattributed_codex_usage(
    conn: &Connection,
    filter: &AgentTaskUsageFilter,
) -> Result<Option<UsageMeasure>, AppError> {
    let app_filter = filter
        .app_type
        .as_deref()
        .map(canonical_query_app_type)
        .transpose()?;
    if crate::services::session_usage_codex::codex_replay_in_progress_on_conn(conn) {
        return Ok(None);
    }
    if app_filter.as_deref().is_some_and(|app| app != "codex") {
        return Ok(None);
    }
    let has_text_filter = has_task_metadata_filter(filter);
    if has_text_filter {
        return Ok(None);
    }

    let range = filter.range.clone().unwrap_or_default();
    range.validate()?;
    let mut conditions = vec![
        "COALESCE(l.data_source, 'proxy') = 'proxy'".to_string(),
        "l.app_type = 'codex'".to_string(),
        "NOT EXISTS (
            SELECT 1 FROM agent_session_canonical_coverage coverage
            WHERE coverage.app_type = 'codex'
              AND coverage.data_source = 'proxy'
              AND coverage.request_id = l.request_id
        )"
        .to_string(),
        "NOT EXISTS (
            SELECT 1 FROM agent_session_nodes mapped_node
            WHERE mapped_node.app_type = 'codex'
              AND mapped_node.session_id = l.session_id
        )"
        .to_string(),
    ];
    let mut params_vec: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(start_at) = range.start_at {
        conditions.push("l.created_at >= ?".into());
        params_vec.push(Box::new(start_at));
    }
    if let Some(end_at) = range.end_at {
        conditions.push("l.created_at <= ?".into());
        params_vec.push(Box::new(end_at));
    }
    let fresh_input = fresh_input_sql("l");
    let sql = format!(
        "SELECT COUNT(*) AS request_count,
                CASE WHEN COUNT(l.input_tokens) = COUNT(*) THEN SUM({fresh_input}) END,
                CASE WHEN COUNT(l.output_tokens) = COUNT(*) THEN SUM(l.output_tokens) END,
                CASE WHEN COUNT(l.cache_read_tokens) = COUNT(*) THEN SUM(l.cache_read_tokens) END,
                CASE WHEN COUNT(l.cache_creation_tokens) = COUNT(*)
                     THEN SUM(l.cache_creation_tokens) END,
                CASE WHEN COUNT(l.total_cost_usd) > 0
                     THEN GROUP_CONCAT(CAST(l.total_cost_usd AS TEXT), char(31)) END,
                COUNT(l.total_cost_usd)
         FROM proxy_request_logs l
         WHERE {}",
        conditions.join(" AND ")
    );
    let refs: Vec<&dyn ToSql> = params_vec.iter().map(|value| value.as_ref()).collect();
    let (
        request_count,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        cost_values,
        cost_known_count,
    ): UnattributedCodexProxyAggregate = conn.query_row(&sql, refs.as_slice(), |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    })?;
    if request_count == 0 {
        return Ok(None);
    }
    let (cost, cost_partial, cost_warnings) = aggregate_reported_cost(
        cost_values.as_deref(),
        request_count,
        cost_known_count,
        None,
        None,
    );
    let mut warnings = cost_warnings;
    let partial = input_tokens.is_none()
        || output_tokens.is_none()
        || cache_read_tokens.is_none()
        || cache_creation_tokens.is_none()
        || cost_partial
        || cost.is_none();
    if partial {
        warnings.push("unattributed Codex proxy usage has missing fields".to_string());
    }
    Ok(Some(UsageMeasure {
        data_source: Some("proxy".to_string()),
        request_count: Some(request_count),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        total_cost_usd: cost,
        precision: UsagePrecision::RequestExact,
        time_semantics: TimeSemantics::EventTime,
        request_count_semantics: RequestCountSemantics::HttpRequest,
        partial,
        warnings,
    }))
}

/// Query every root/standalone task in the selected Agent/date scope and
/// return the native metadata dimensions used by the task-statistics
/// comboboxes. This deliberately has no pagination: deriving options from a
/// page of task rows would make valid older titles/projects disappear.
fn query_task_filter_options(
    conn: &Connection,
    request: &AgentTaskUsageFilterOptionsRequest,
) -> Result<AgentTaskUsageFilterOptions, AppError> {
    let source_scope =
        TaskSourceScope::new(conn, request.app_type.as_deref(), request.range.as_ref())?;
    if source_scope.blocks_requested_codex() {
        return Ok(AgentTaskUsageFilterOptions {
            titles: Vec::new(),
            projects: Vec::new(),
        });
    }
    let source_ctes = source_scope.candidate_ctes();
    let params_vec = source_scope.into_params();
    let sql = format!(
        "{source_ctes}
         SELECT TRIM(root_node.title), TRIM(root_node.project_dir)
         FROM candidates c
         JOIN agent_session_nodes root_node
           ON root_node.app_type = c.app_type
          AND root_node.session_id = c.root_session_id
         WHERE (TRIM(COALESCE(root_node.title, '')) <> ''
             OR TRIM(COALESCE(root_node.project_dir, '')) <> '')",
    );
    let refs: Vec<&dyn ToSql> = params_vec.iter().map(|value| value.as_ref()).collect();
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query(refs.as_slice())?;
    let mut titles = BTreeMap::<String, String>::new();
    let mut projects = BTreeMap::<String, String>::new();
    while let Some(row) = rows.next()? {
        let title: Option<String> = row.get(0)?;
        let project_dir: Option<String> = row.get(1)?;
        if let Some(title) = title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            let key = title.to_lowercase();
            titles
                .entry(key)
                .and_modify(|existing| {
                    if title.as_str() < existing.as_str() {
                        *existing = title.clone();
                    }
                })
                .or_insert(title);
        }
        if let Some(project_dir) = project_dir
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            let key = project_dir.to_lowercase();
            projects
                .entry(key)
                .and_modify(|existing| {
                    if project_dir.as_str() < existing.as_str() {
                        *existing = project_dir.clone();
                    }
                })
                .or_insert(project_dir);
        }
    }

    Ok(AgentTaskUsageFilterOptions {
        titles: titles.into_values().collect(),
        projects: projects
            .into_values()
            .map(|project_dir| AgentTaskUsageProjectOption { project_dir })
            .collect(),
    })
}

/// List complete title/project candidates for the task-statistics filters.
pub fn get_agent_task_usage_filter_options(
    db: &Database,
    request: &AgentTaskUsageFilterOptionsRequest,
) -> Result<AgentTaskUsageFilterOptions, AppError> {
    if let Some(range) = &request.range {
        range.validate()?;
    }
    let conn = crate::database::lock_conn!(db.conn);
    query_task_filter_options(&conn, request)
}

/// Query one normalized root task/session.  The returned total is derived
/// from self and descendant measures in memory; no total bucket is written.
pub fn get_agent_session_usage(
    db: &Database,
    request: &AgentSessionUsageRequest,
) -> Result<AgentSessionUsageSummary, AppError> {
    let app_type = canonical_query_app_type(&request.app_type)?;
    if request.session_id.trim().is_empty() {
        return Err(AppError::InvalidInput("session_id 不能为空".into()));
    }
    if let Some(range) = &request.range {
        range.validate()?;
    }
    let conn = crate::database::lock_conn!(db.conn);
    let root = resolve_query_root(&conn, &app_type, request.session_id.trim())?;
    let groups = query_usage_groups(
        &conn,
        &app_type,
        std::slice::from_ref(&root.root_session_id),
        request.range.as_ref(),
    )?;
    let supports_descendants = agent_usage_capability(&app_type)
        .map(|capability| capability.supports_descendants)
        .unwrap_or(false);
    Ok(summary_for_root(
        &root,
        request.session_id.trim(),
        groups,
        supports_descendants,
        request.range.as_ref(),
    ))
}

/// List root/standalone task rows.  Child nodes are joined into their root
/// aggregate and are never emitted as default list rows.
pub fn list_agent_task_usage(
    db: &Database,
    filter: &AgentTaskUsageFilter,
) -> Result<AgentTaskUsagePage, AppError> {
    if let Some(range) = &filter.range {
        range.validate()?;
    }
    let (limit, offset) = filter.normalized_page();
    let conn = crate::database::lock_conn!(db.conn);
    let data_status = task_data_status_on_conn(&conn, filter.app_type.as_deref())?;
    let (roots, total) = query_task_roots(&conn, filter)?;
    let unattributed_usage = query_unattributed_codex_usage(&conn, filter)?;
    let root_ids: Vec<String> = roots
        .iter()
        .map(|root| root.root_session_id.clone())
        .collect();
    let mut groups_by_root: HashMap<(String, String), Vec<UsageGroup>> = HashMap::new();
    if !root_ids.is_empty() {
        let app_type = filter
            .app_type
            .as_deref()
            .map(canonical_query_app_type)
            .transpose()?;
        // A mixed-app page is allowed; query groups once per selected app so
        // identical bare session IDs from different App namespaces cannot
        // merge.  The normal UI sends app_type in this path; mixed pages use
        // one aggregate query per app.
        if let Some(app_type) = app_type {
            for group in query_usage_groups(&conn, &app_type, &root_ids, filter.range.as_ref())? {
                groups_by_root
                    .entry((app_type.clone(), group.root_session_id.clone()))
                    .or_default()
                    .push(group);
            }
        } else {
            let mut ids_by_app: HashMap<String, Vec<String>> = HashMap::new();
            for root in &roots {
                ids_by_app
                    .entry(root.app_type.clone())
                    .or_default()
                    .push(root.root_session_id.clone());
            }
            for (app_type, ids) in ids_by_app {
                for group in query_usage_groups(&conn, &app_type, &ids, filter.range.as_ref())? {
                    groups_by_root
                        .entry((app_type.clone(), group.root_session_id.clone()))
                        .or_default()
                        .push(group);
                }
            }
        }
    }
    let mut items = Vec::with_capacity(roots.len());
    for root in roots {
        let supports_descendants = agent_usage_capability(&root.app_type)
            .map(|capability| capability.supports_descendants)
            .unwrap_or(false);
        let summary = summary_for_root(
            &root,
            &root.root_session_id,
            groups_by_root
                .remove(&(root.app_type.clone(), root.root_session_id.clone()))
                .unwrap_or_default(),
            supports_descendants,
            filter.range.as_ref(),
        );
        items.push(AgentTaskUsageRow {
            app_type: summary.app_type,
            session_id: summary.session_id,
            root_session_id: summary.root_session_id,
            root: summary.root,
            self_usage: summary.self_usage,
            descendant_usage: summary.descendant_usage,
            descendant_usage_status: summary.descendant_usage_status,
            total_usage: summary.total_usage,
            descendant_session_count: summary.descendant_session_count,
            precision: summary.precision,
            partial: summary.partial,
            warnings: summary.warnings,
            source_dimensions: summary.source_dimensions,
        });
    }
    Ok(AgentTaskUsagePage {
        items,
        total,
        limit,
        offset,
        has_more: (offset as u64).saturating_add(limit as u64) < total,
        unattributed_usage,
        data_status,
    })
}

/// Return the immutable nine-App registry from the normalized contract.  No
/// second hardcoded capability list is maintained by the query layer.
pub fn get_agent_usage_capabilities() -> Vec<AgentUsageCapability> {
    agent_usage_capabilities().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::session_usage_pipeline::UsageSourceSpec;

    fn normalize(claims: &[SessionRelationClaim]) -> Vec<NormalizedSessionNode> {
        normalize_session_relations(claims).expect("anonymous relation fixture should normalize")
    }

    #[test]
    fn relation_graph_normalizes_root_child_and_grandchild() {
        let nodes = normalize(&[
            SessionRelationClaim::root("claude", "root"),
            SessionRelationClaim::child("claude", "child", "root", RelationConfidence::Explicit),
            SessionRelationClaim::child(
                "claude",
                "grandchild",
                "child",
                RelationConfidence::Structural,
            ),
        ]);
        assert_eq!(nodes[0].node_kind, SessionNodeKind::Root);
        assert_eq!(nodes[0].root_session_id, "root");
        assert_eq!(nodes[1].node_kind, SessionNodeKind::Child);
        assert_eq!(nodes[1].parent_session_id.as_deref(), Some("root"));
        assert_eq!(nodes[1].root_session_id, "root");
        assert_eq!(nodes[2].node_kind, SessionNodeKind::Child);
        assert_eq!(nodes[2].parent_session_id.as_deref(), Some("child"));
        assert_eq!(nodes[2].root_session_id, "root");
    }

    #[test]
    fn relation_graph_keeps_standalone_and_missing_parent_safe() {
        let nodes = normalize(&[
            SessionRelationClaim::standalone("gemini", "independent"),
            SessionRelationClaim::child(
                "gemini",
                "orphan",
                "missing",
                RelationConfidence::Explicit,
            ),
        ]);
        assert_eq!(nodes[0].node_kind, SessionNodeKind::Standalone);
        assert_eq!(nodes[0].root_session_id, "independent");
        assert_eq!(nodes[1].node_kind, SessionNodeKind::Unknown);
        assert_eq!(nodes[1].parent_session_id, None);
        assert_eq!(nodes[1].root_session_id, "orphan");
        assert_eq!(
            nodes[1].relation_confidence,
            RelationConfidence::Unavailable
        );
    }

    #[test]
    fn relation_graph_rejects_self_parent_cycle_and_conflicting_parent() {
        let nodes = normalize(&[
            SessionRelationClaim::child("codex", "self", "self", RelationConfidence::Explicit),
            SessionRelationClaim::child("codex", "a", "b", RelationConfidence::Explicit),
            SessionRelationClaim::child("codex", "b", "a", RelationConfidence::Explicit),
            SessionRelationClaim::child("codex", "conflict", "p1", RelationConfidence::Explicit),
            SessionRelationClaim::child("codex", "conflict", "p2", RelationConfidence::Explicit),
            SessionRelationClaim::child(
                "codex",
                "conflict-confidence",
                "p1",
                RelationConfidence::Conflict,
            ),
            SessionRelationClaim::child(
                "codex",
                "conflict-confidence",
                "p1",
                RelationConfidence::Explicit,
            ),
            SessionRelationClaim::root("codex", "p1"),
            SessionRelationClaim::root("codex", "p2"),
        ]);
        for id in ["self", "a", "b", "conflict", "conflict-confidence"] {
            let node = nodes.iter().find(|node| node.session_id == id).unwrap();
            assert_eq!(node.node_kind, SessionNodeKind::Conflict, "{id}");
            assert_eq!(node.parent_session_id, None, "{id}");
            assert_eq!(node.root_session_id, id, "{id}");
            assert_eq!(
                node.relation_confidence,
                RelationConfidence::Conflict,
                "{id}"
            );
        }
    }

    #[test]
    fn relation_graph_does_not_infer_time_only_parentage() {
        let mut root = SessionRelationClaim::standalone("claude", "root");
        root.metadata.last_active_at = Some(100);
        let mut nearby = SessionRelationClaim::standalone("claude", "nearby");
        nearby.metadata.last_active_at = Some(101);
        let nodes = normalize(&[root, nearby]);
        assert!(nodes
            .iter()
            .all(|node| node.node_kind == SessionNodeKind::Standalone));
    }

    #[test]
    fn capability_registry_covers_exactly_all_managed_app_types() {
        let registry = agent_usage_capabilities();
        let app_types: Vec<AppType> = AppType::all().collect();
        assert_eq!(registry.len(), app_types.len());
        for app_type in &app_types {
            let app_type = app_type.as_str();
            assert!(
                agent_usage_capability(app_type).is_some(),
                "missing {app_type}"
            );
        }
        let mut ids: Vec<&str> = registry.iter().map(|entry| entry.app_type).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), registry.len(), "capability IDs must be unique");
    }

    #[test]
    fn pi_capability_reports_descendant_usage_events() {
        let capability = agent_usage_capability("pi")
            .expect("pi must remain in the canonical capability registry");
        assert!(capability.supports_descendants);
        assert_eq!(capability.precision, UsagePrecision::RequestExact);
        assert_eq!(capability.time_semantics, TimeSemantics::EventTime);
        assert_eq!(
            capability.request_count_semantics,
            RequestCountSemantics::UsageEvent
        );
        assert!(capability.notes.contains("parentSession paths"));
    }

    fn query_node(
        app_type: &str,
        session_id: &str,
        parent_session_id: Option<&str>,
    ) -> NormalizedSessionNode {
        let root = parent_session_id.unwrap_or(session_id);
        let mut claim = match parent_session_id {
            Some(parent) => SessionRelationClaim::child(
                app_type,
                session_id,
                parent,
                RelationConfidence::Explicit,
            ),
            None => SessionRelationClaim::root(app_type, session_id),
        };
        claim.metadata = SessionNodeMetadata {
            title: Some(format!("Task {root}")),
            project_dir: Some(format!("/tmp/{root}")),
            source_path: None,
            created_at: Some(1_000),
            last_active_at: Some(2_000),
            last_synced_at: 2_000,
        };
        normalize(&[claim]).pop().expect("one query fixture node")
    }

    fn query_rollup(
        session_id: &str,
        date: &str,
        input: i64,
        output: i64,
    ) -> NormalizedUsageRollupFact {
        query_rollup_for_app("claude", session_id, date, input, output)
    }

    fn query_rollup_for_app(
        app_type: &str,
        session_id: &str,
        date: &str,
        input: i64,
        output: i64,
    ) -> NormalizedUsageRollupFact {
        let mut fact = UsageSourceSpec::new(
            app_type,
            "provider",
            "query_fixture",
            UsagePrecision::RequestExact,
            TimeSemantics::EventTime,
            RequestCountSemantics::AssistantMessage,
        )
        .fact(date, session_id, "model", "model", "model");
        fact.request_count = Some(1);
        fact.input_tokens = Some(input);
        fact.output_tokens = Some(output);
        fact.cache_read_tokens = Some(0);
        fact.cache_creation_tokens = Some(0);
        fact.total_cost_usd = Some("0.01".into());
        fact.first_event_at = Some(1_000);
        fact.last_event_at = Some(2_000);
        fact
    }

    #[test]
    fn task_roots_sort_mixed_second_and_millisecond_timestamps() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut seconds = query_node("claude", "seconds-root", None);
        seconds.created_at = Some(1_700_000_000);
        seconds.last_active_at = Some(1_800_000_000);
        let mut millis = query_node("hermes", "millis-root", None);
        millis.created_at = Some(1_700_000_000_000);
        millis.last_active_at = Some(1_700_000_000_000);
        db.upsert_agent_session_node(&seconds)?;
        db.upsert_agent_session_node(&millis)?;
        db.upsert_agent_session_usage_rollup_fact(&query_rollup_for_app(
            "claude",
            "seconds-root",
            "2026-08-13",
            10,
            1,
        ))?;
        db.upsert_agent_session_usage_rollup_fact(&query_rollup_for_app(
            "hermes",
            "millis-root",
            "2026-08-13",
            20,
            2,
        ))?;

        let first = list_agent_task_usage(&db, &AgentTaskUsageFilter::default())?;
        assert_eq!(first.items[0].app_type, "claude");
        assert_eq!(first.items[0].session_id, "seconds-root");

        let repeated = list_agent_task_usage(&db, &AgentTaskUsageFilter::default())?;
        let first_ids: Vec<_> = first
            .items
            .iter()
            .map(|item| (&item.app_type, &item.session_id))
            .collect();
        let repeated_ids: Vec<_> = repeated
            .items
            .iter()
            .map(|item| (&item.app_type, &item.session_id))
            .collect();
        assert_eq!(first_ids, repeated_ids);
        Ok(())
    }

    #[test]
    fn node_less_proxy_task_uses_source_activity_for_pagination() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut node = query_node("claude", "older-node", None);
        node.created_at = Some(1_000);
        node.last_active_at = Some(2_000);
        db.upsert_agent_session_node(&node)?;
        db.upsert_agent_session_usage_rollup_fact(&query_rollup(
            "older-node",
            "1970-01-01",
            10,
            1,
        ))?;
        let mut rollup_only = query_rollup("rollup-only-session", "1970-01-01", 5, 2);
        rollup_only.first_event_at = Some(2_500);
        rollup_only.last_event_at = Some(2_500);
        db.upsert_agent_session_usage_rollup_fact(&rollup_only)?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, latency_ms, status_code,
                    session_id, created_at, data_source
                 ) VALUES (
                    'proxy-only-request', 'gateway', 'claude', 'claude-model',
                    3, 4, 0, 200, 'proxy-only-session', 3000, 'proxy'
                 )",
                [],
            )?;
        }

        let page = list_agent_task_usage(
            &db,
            &AgentTaskUsageFilter {
                app_type: Some("claude".into()),
                limit: 2,
                ..Default::default()
            },
        )?;
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].session_id, "proxy-only-session");
        assert!(page.items[0].root.is_none());
        assert_eq!(page.items[1].session_id, "rollup-only-session");
        assert!(page.items[1].root.is_none());
        Ok(())
    }

    #[test]
    fn query_root_child_grandchild_derives_self_descendant_and_total() -> Result<(), AppError> {
        let db = Database::memory()?;
        for node in normalize(&[
            SessionRelationClaim::root("claude", "root"),
            SessionRelationClaim::child("claude", "child", "root", RelationConfidence::Explicit),
            SessionRelationClaim::child(
                "claude",
                "grandchild",
                "child",
                RelationConfidence::Explicit,
            ),
        ]) {
            db.upsert_agent_session_node(&node)?;
        }
        db.upsert_agent_session_usage_rollup_fact(&query_rollup("root", "2026-08-13", 10, 20))?;
        db.upsert_agent_session_usage_rollup_fact(&query_rollup("child", "2026-08-13", 3, 4))?;
        db.upsert_agent_session_usage_rollup_fact(&query_rollup("grandchild", "2026-08-13", 5, 6))?;

        let summary = get_agent_session_usage(
            &db,
            &AgentSessionUsageRequest {
                app_type: "claude".into(),
                session_id: "root".into(),
                range: None,
            },
        )?;
        assert_eq!(summary.root_session_id, "root");
        assert_eq!(summary.descendant_session_count, 2);
        assert_eq!(
            summary.self_usage.as_ref().unwrap().total_tokens(),
            Some(30)
        );
        assert_eq!(
            summary.descendant_usage.as_ref().unwrap().total_tokens(),
            Some(18)
        );
        assert_eq!(
            summary.total_usage.as_ref().unwrap().total_tokens(),
            Some(48)
        );
        assert!(summary
            .source_dimensions
            .iter()
            .any(|dimension| !dimension.is_descendant));
        assert!(summary
            .source_dimensions
            .iter()
            .any(|dimension| dimension.is_descendant));
        let child_summary = get_agent_session_usage(
            &db,
            &AgentSessionUsageRequest {
                app_type: "claude".into(),
                session_id: "child".into(),
                range: None,
            },
        )?;
        assert_eq!(child_summary.root_session_id, "root");
        assert!(child_summary.root_resolved);
        assert!(child_summary
            .warnings
            .iter()
            .any(|warning| warning.contains("resolved to normalized root")));
        Ok(())
    }

    #[test]
    fn codex_replay_status_only_applies_to_codex_inclusive_task_queries() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value)
                 VALUES ('codex_usage_canonical_replay_v1', 'replaying')",
                [],
            )?;
        }

        let claude = list_agent_task_usage(
            &db,
            &AgentTaskUsageFilter {
                app_type: Some("claude".into()),
                ..Default::default()
            },
        )?;
        assert_eq!(claude.data_status, AgentTaskUsageDataStatus::Ready);

        let codex = list_agent_task_usage(
            &db,
            &AgentTaskUsageFilter {
                app_type: Some("codex".into()),
                ..Default::default()
            },
        )?;
        assert_eq!(codex.data_status, AgentTaskUsageDataStatus::Rebuilding);

        let mixed = list_agent_task_usage(&db, &AgentTaskUsageFilter::default())?;
        assert_eq!(mixed.data_status, AgentTaskUsageDataStatus::Rebuilding);
        Ok(())
    }

    #[test]
    fn query_date_range_applies_to_raw_and_durable_buckets() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_agent_session_node(&query_node("claude", "range-root", None))?;
        db.upsert_agent_session_usage_rollup_fact(&query_rollup(
            "range-root",
            "2026-08-01",
            10,
            1,
        ))?;
        db.upsert_agent_session_usage_rollup_fact(&query_rollup(
            "range-root",
            "2026-08-13",
            20,
            2,
        ))?;
        let summary = get_agent_session_usage(
            &db,
            &AgentSessionUsageRequest {
                app_type: "claude".into(),
                session_id: "range-root".into(),
                range: Some(AgentUsageRange {
                    start_at: Some(1_700_000_000),
                    end_at: Some(2_000_000_000),
                }),
            },
        )?;
        // The fixture timestamps are outside this range; no source may leak
        // an unbounded rollup row into the selected interval.
        assert!(summary.total_usage.is_none());
        assert!(summary.partial);
        Ok(())
    }

    #[test]
    fn task_filter_options_are_scoped_deduplicated_and_exact_filters_stay_compatible(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        for (session_id, title, project_dir) in [
            ("option-a", Some("Build"), Some("/tmp/a")),
            ("option-b", Some("build"), Some("/tmp/A")),
            ("option-c", Some("Build Extra"), Some("/tmp/extra")),
            ("option-empty", None, Some("/tmp/empty")),
        ] {
            let mut node = query_node("claude", session_id, None);
            node.title = title.map(str::to_owned);
            node.project_dir = project_dir.map(str::to_owned);
            db.upsert_agent_session_node(&node)?;
        }
        let start_at = Local
            .with_ymd_and_hms(2026, 8, 13, 0, 0, 0)
            .single()
            .ok_or_else(|| AppError::InvalidInput("invalid options range start".into()))?
            .timestamp();
        let end_at = Local
            .with_ymd_and_hms(2026, 8, 13, 23, 59, 59)
            .single()
            .ok_or_else(|| AppError::InvalidInput("invalid options range end".into()))?
            .timestamp();
        for session_id in ["option-a", "option-b", "option-c"] {
            let mut rollup = query_rollup(session_id, "2026-08-13", 1, 1);
            rollup.first_event_at = Some(start_at);
            rollup.last_event_at = Some(end_at);
            db.upsert_agent_session_usage_rollup_fact(&rollup)?;
        }

        let range = AgentUsageRange {
            start_at: Some(start_at),
            end_at: Some(end_at),
        };
        let options = get_agent_task_usage_filter_options(
            &db,
            &AgentTaskUsageFilterOptionsRequest {
                app_type: Some("claude".into()),
                range: Some(range.clone()),
            },
        )?;
        assert_eq!(options.titles, vec!["Build", "Build Extra"]);
        assert_eq!(options.projects.len(), 2);
        assert!(options
            .projects
            .iter()
            .any(|option| option.project_dir.eq_ignore_ascii_case("/tmp/a")));
        assert!(options
            .projects
            .iter()
            .any(|option| option.project_dir == "/tmp/extra"));
        assert!(!options.titles.iter().any(|title| title.is_empty()));

        let exact = list_agent_task_usage(
            &db,
            &AgentTaskUsageFilter {
                app_type: Some("claude".into()),
                title_exact: Some("bUiLd".into()),
                range: Some(range.clone()),
                ..Default::default()
            },
        )?;
        assert_eq!(exact.total, 2);
        let fuzzy = list_agent_task_usage(
            &db,
            &AgentTaskUsageFilter {
                app_type: Some("claude".into()),
                title: Some("Build".into()),
                range: Some(range),
                ..Default::default()
            },
        )?;
        assert_eq!(fuzzy.total, 3);
        Ok(())
    }
}
