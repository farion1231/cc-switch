//! Shared canonical session-usage publication pipeline.
//!
//! Native adapters deliberately keep only source parsing, source-specific
//! de-duplication, and source-specific state machines.  Once an adapter has
//! admitted evidence, this module owns graph normalization, null-safe fact
//! aggregation, canonical coverage, and the durable write target.  Keeping
//! that boundary here prevents the eight native formats from gradually
//! growing eight subtly different definitions of a canonical usage bucket.

use crate::database::{lock_conn, AgentSessionCanonicalCoverageMarker, Database};
use crate::error::AppError;
use crate::services::agent_session_usage::{
    normalize_session_relations, NormalizedSessionNode, NormalizedUsageRollupFact,
    NormalizedUsageSnapshot, RequestCountSemantics, SessionRelationClaim, TimeSemantics,
    UsagePrecision,
};
use crate::services::session_usage::SessionSyncResult;
use rusqlite::{Connection, OptionalExtension};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

pub(crate) const CODEX_REPLAY_APP_TYPE: &str = "codex_replay";

/// Canonical destination for one admitted batch.  Replay is intentionally a
/// closed destination: it can only address the private Codex generation
/// tables, never a table name parsed from a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsagePublishTarget {
    Published,
    CodexReplay,
}

impl UsagePublishTarget {
    pub(crate) const fn node_table(self) -> &'static str {
        match self {
            Self::Published => "agent_session_nodes",
            Self::CodexReplay => "codex_replay_nodes",
        }
    }

    pub(crate) const fn rollup_table(self) -> &'static str {
        match self {
            Self::Published => "agent_session_usage_rollups",
            Self::CodexReplay => "codex_replay_rollups",
        }
    }

    pub(crate) const fn coverage_table(self) -> &'static str {
        match self {
            Self::Published => "agent_session_canonical_coverage",
            Self::CodexReplay => "codex_replay_coverage",
        }
    }

    pub(crate) const fn session_log_table(self) -> &'static str {
        match self {
            Self::Published => "proxy_request_logs",
            Self::CodexReplay => "codex_replay_session_logs",
        }
    }

    pub(crate) const fn cursor_table(self) -> &'static str {
        match self {
            Self::Published => "session_log_sync",
            Self::CodexReplay => "codex_replay_sync",
        }
    }

    fn stored_app_type(self, app_type: &str) -> String {
        match self {
            Self::Published => app_type.to_string(),
            // The only current replay adapter is Codex.  Do not preserve a
            // caller-provided app identity in the shadow generation.
            Self::CodexReplay => CODEX_REPLAY_APP_TYPE.to_string(),
        }
    }

    pub(crate) fn coverage_source(self, data_source: &str) -> String {
        match self {
            Self::Published => data_source.to_string(),
            Self::CodexReplay => match data_source {
                "codex_session" => "codex_session_replay".to_string(),
                "proxy" => "proxy_replay".to_string(),
                other => other.to_string(),
            },
        }
    }
}

/// Immutable source semantics shared by every observation emitted by one
/// parser path.  A source can still set model/session/window fields on each
/// fact; these are the dimensions that are truly source-wide.
#[derive(Debug, Clone, Default)]
pub(crate) struct UsageSourceSpec {
    pub app_type: String,
    pub provider_id: String,
    pub data_source: String,
    pub precision: UsagePrecision,
    pub time_semantics: TimeSemantics,
    pub request_count_semantics: RequestCountSemantics,
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
}

impl UsageSourceSpec {
    pub(crate) fn new(
        app_type: impl Into<String>,
        provider_id: impl Into<String>,
        data_source: impl Into<String>,
        precision: UsagePrecision,
        time_semantics: TimeSemantics,
        request_count_semantics: RequestCountSemantics,
    ) -> Self {
        Self {
            app_type: app_type.into(),
            provider_id: provider_id.into(),
            data_source: data_source.into(),
            precision,
            time_semantics,
            request_count_semantics,
            ..Self::default()
        }
    }

    /// Start a complete typed fact without recreating the schema's default
    /// dimensions in every native adapter.  The parser fills only observed
    /// identity/measurement fields afterwards.
    pub(crate) fn fact(
        &self,
        date: impl Into<String>,
        session_id: impl Into<String>,
        model: impl Into<String>,
        request_model: impl Into<String>,
        pricing_model: impl Into<String>,
    ) -> NormalizedUsageRollupFact {
        NormalizedUsageRollupFact {
            date: date.into(),
            app_type: self.app_type.clone(),
            session_id: session_id.into(),
            provider_id: self.provider_id.clone(),
            model: model.into(),
            request_model: request_model.into(),
            pricing_model: pricing_model.into(),
            data_source: self.data_source.clone(),
            precision: self.precision,
            time_semantics: self.time_semantics,
            request_count_semantics: self.request_count_semantics,
            input_token_semantics: self.input_token_semantics,
            source_identity: self.source_identity.clone(),
            profile_id: self.profile_id.clone(),
            database_identity: self.database_identity.clone(),
            base_url_digest: self.base_url_digest.clone(),
            billing_mode: self.billing_mode.clone(),
            task: self.task.clone(),
            source_version: self.source_version.clone(),
            sync_window_start: self.sync_window_start,
            sync_window_end: self.sync_window_end,
            ..NormalizedUsageRollupFact::default()
        }
    }
}

/// Compatibility record for a source event that must remain visible to the
/// existing raw-log retention and global-usage paths. Canonical facts retain
/// nullable source semantics separately; this row only carries the legacy
/// non-null raw representation.
#[derive(Debug, Clone, Default)]
pub(crate) struct RawUsageLogRow {
    pub request_id: String,
    pub provider_id: String,
    pub app_type: String,
    pub model: String,
    pub request_model: String,
    pub pricing_model: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub input_token_semantics: i64,
    pub input_cost_usd: String,
    pub output_cost_usd: String,
    pub cache_read_cost_usd: String,
    pub cache_creation_cost_usd: String,
    pub total_cost_usd: String,
    pub latency_ms: i64,
    pub first_token_ms: Option<i64>,
    pub status_code: i64,
    pub error_message: Option<String>,
    pub session_id: Option<String>,
    pub provider_type: Option<String>,
    pub is_streaming: bool,
    pub cost_multiplier: String,
    pub created_at: i64,
    pub data_source: String,
}

impl RawUsageLogRow {
    pub(crate) fn native_session(
        request_id: impl Into<String>,
        provider_id: impl Into<String>,
        app_type: impl Into<String>,
        model: impl Into<String>,
        session_id: Option<&str>,
        data_source: impl Into<String>,
        created_at: i64,
    ) -> Self {
        let model = model.into();
        let data_source = data_source.into();
        Self {
            request_id: request_id.into(),
            provider_id: provider_id.into(),
            app_type: app_type.into(),
            request_model: model.clone(),
            model,
            input_cost_usd: "0".to_string(),
            output_cost_usd: "0".to_string(),
            cache_read_cost_usd: "0".to_string(),
            cache_creation_cost_usd: "0".to_string(),
            total_cost_usd: "0".to_string(),
            status_code: 200,
            session_id: session_id.map(str::to_owned),
            provider_type: Some(data_source.clone()),
            is_streaming: true,
            cost_multiplier: "1.0".to_string(),
            created_at,
            data_source,
            ..Self::default()
        }
    }

    /// Insert one raw compatibility row into a closed live/replay target.
    /// The source-specific caller decides whether an existing request should
    /// be refreshed before it reaches this helper; normal import paths use
    /// the same idempotent insert semantics everywhere.
    pub(crate) fn insert_or_ignore_on_conn(
        &self,
        conn: &Connection,
        target: UsagePublishTarget,
    ) -> Result<bool, AppError> {
        let sql = format!(
            "INSERT OR IGNORE INTO {} (
                request_id, provider_id, app_type, model, request_model, pricing_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_token_semantics, input_cost_usd, output_cost_usd,
                cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, status_code, error_message, session_id,
                provider_type, is_streaming, cost_multiplier, created_at, data_source
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
             )",
            target.session_log_table(),
        );
        conn.execute(
            &sql,
            rusqlite::params![
                &self.request_id,
                &self.provider_id,
                &self.app_type,
                &self.model,
                &self.request_model,
                &self.pricing_model,
                self.input_tokens,
                self.output_tokens,
                self.cache_read_tokens,
                self.cache_creation_tokens,
                self.input_token_semantics,
                &self.input_cost_usd,
                &self.output_cost_usd,
                &self.cache_read_cost_usd,
                &self.cache_creation_cost_usd,
                &self.total_cost_usd,
                self.latency_ms,
                self.first_token_ms,
                self.status_code,
                &self.error_message,
                &self.session_id,
                &self.provider_type,
                if self.is_streaming { 1i64 } else { 0i64 },
                &self.cost_multiplier,
                self.created_at,
                &self.data_source,
            ],
        )
        .map(|inserted| inserted > 0)
        .map_err(|error| AppError::Database(format!("写入会话 raw compatibility 行失败: {error}")))
    }
}

/// One source-admitted request/event.  `fact` deliberately keeps every
/// schema dimension typed, while the nested options distinguish absent source
/// components from explicitly reported zero values.
#[derive(Debug, Clone)]
pub(crate) struct UsageObservation {
    pub request_id: String,
    pub fact: NormalizedUsageRollupFact,
}

/// A pre-computed fact is used for sources such as Hermes whose evidence is a
/// cumulative snapshot/window delta rather than a list of independent
/// requests.  Its coverage markers are optional because no request identity
/// is fabricated for snapshots.
#[derive(Debug, Clone)]
pub(crate) struct CanonicalUsageFact {
    pub fact: NormalizedUsageRollupFact,
    pub request_ids: Vec<String>,
    pub mode: CanonicalFactMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanonicalFactMode {
    /// Add the newly admitted request evidence to the existing full-key row.
    Additive,
    /// Replace an exact source/window fact already recomputed by the adapter.
    Replace,
}

/// A source/session scoped replacement is used for rewrite-aware formats
/// such as Grok.  It never means a task-total replacement.
#[derive(Debug, Clone)]
pub(crate) struct CanonicalReplaceScope {
    pub app_type: String,
    pub session_id: String,
    pub data_source: String,
}

/// All canonical records admitted by one caller-owned transaction.  Raw
/// compatibility rows, de-dup ledgers, and cursors are written by the source
/// adapter in the same transaction immediately before this batch is
/// published; this keeps provider-specific formats private without splitting
/// canonical publication into a second commit.
#[derive(Debug, Default)]
pub(crate) struct CanonicalUsageBatch {
    pub relation_claims: Vec<SessionRelationClaim>,
    /// Adapters that must normalize a complete graph before a source-specific
    /// import order (notably Pi fork replay) may hand over the resulting
    /// nodes directly. They remain subject to the same target transaction.
    pub nodes: Vec<NormalizedSessionNode>,
    pub observations: Vec<UsageObservation>,
    /// Complete current source evidence for a rewrite-aware source/session.
    /// It is grouped exactly like additive observations, but replaces the
    /// resulting source fact after its explicit replacement scope is cleared.
    pub replacement_observations: Vec<UsageObservation>,
    pub facts: Vec<CanonicalUsageFact>,
    pub snapshots: Vec<NormalizedUsageSnapshot>,
    pub replace_scopes: Vec<CanonicalReplaceScope>,
}

impl CanonicalUsageBatch {
    pub(crate) fn observe(
        &mut self,
        request_id: impl Into<String>,
        fact: NormalizedUsageRollupFact,
    ) {
        self.observations.push(UsageObservation {
            request_id: request_id.into(),
            fact,
        });
    }

    pub(crate) fn replace_observe(
        &mut self,
        request_id: impl Into<String>,
        fact: NormalizedUsageRollupFact,
    ) {
        self.replacement_observations.push(UsageObservation {
            request_id: request_id.into(),
            fact,
        });
    }

    pub(crate) fn add_fact(&mut self, fact: NormalizedUsageRollupFact, request_ids: Vec<String>) {
        self.facts.push(CanonicalUsageFact {
            fact,
            request_ids,
            mode: CanonicalFactMode::Additive,
        });
    }
}

#[derive(Debug)]
struct AggregatedFact {
    fact: NormalizedUsageRollupFact,
    request_ids: Vec<String>,
}

/// Group by the durable fact dimensions while deliberately excluding every
/// nullable measurement and qualifier that the aggregation step reconciles.
/// Reusing the typed fact avoids a second mirror struct that can silently fall
/// out of sync with the v18 schema when a new dimension is added.
fn fact_dimension_key(fact: &NormalizedUsageRollupFact) -> NormalizedUsageRollupFact {
    let mut key = fact.clone();
    key.request_count = None;
    key.api_call_count = None;
    key.input_tokens = None;
    key.output_tokens = None;
    key.cache_read_tokens = None;
    key.cache_creation_tokens = None;
    key.cache_write_tokens = None;
    key.reasoning_tokens = None;
    key.total_cost_usd = None;
    key.cost_status = None;
    key.cost_source = None;
    key.cost_delta_kind = None;
    key.correction_state = None;
    key.first_event_at = None;
    key.last_event_at = None;
    key
}

/// Publish a fully admitted canonical batch on a caller-owned transaction.
/// No imported count or cursor should be advanced until the caller commits
/// that transaction successfully.
pub(crate) fn publish_canonical_batch_on_conn(
    conn: &Connection,
    target: UsagePublishTarget,
    batch: CanonicalUsageBatch,
) -> Result<(), AppError> {
    let replace_scopes = batch.replace_scopes.clone();
    clear_replaced_coverage_on_conn(conn, target, &replace_scopes)?;
    publish_canonical_batch_on_conn_after_coverage_clear(conn, target, batch)
}

/// Remove ownership markers that were created by the previous generation of a
/// rewrite-aware source/session.  Proxy markers are included because they are
/// keyed by the canonical session rather than the native source name.
pub(crate) fn clear_replaced_coverage_on_conn(
    conn: &Connection,
    target: UsagePublishTarget,
    replace_scopes: &[CanonicalReplaceScope],
) -> Result<(), AppError> {
    let sql = format!(
        "DELETE FROM {}\n         WHERE app_type = ?1\n           AND canonical_session_id = ?2\n           AND data_source IN (?3, ?4)",
        target.coverage_table()
    );
    for scope in replace_scopes {
        let stored_app_type = target.stored_app_type(&scope.app_type);
        let source = target.coverage_source(&scope.data_source);
        let proxy = target.coverage_source("proxy");
        conn.execute(
            &sql,
            rusqlite::params![stored_app_type, &scope.session_id, source, proxy],
        )
        .map_err(|error| AppError::Database(format!("清理 canonical 覆盖标记失败: {error}")))?;
    }
    Ok(())
}

/// Publish after the caller has explicitly cleared replacement coverage.  The
/// Cowork adapter uses this boundary because it must reserve matching proxy
/// rows between the clear and the canonical write in one transaction.
pub(crate) fn publish_canonical_batch_on_conn_after_coverage_clear(
    conn: &Connection,
    target: UsagePublishTarget,
    batch: CanonicalUsageBatch,
) -> Result<(), AppError> {
    if !batch.nodes.is_empty() && !batch.relation_claims.is_empty() {
        return Err(AppError::InvalidInput(
            "canonical batch cannot contain both normalized nodes and relation claims".into(),
        ));
    }
    let nodes = if batch.nodes.is_empty() {
        normalize_session_relations(&batch.relation_claims)?
    } else {
        batch.nodes.clone()
    };
    for node in &nodes {
        write_node_to_target_on_conn(conn, target, node)?;
    }

    for scope in &batch.replace_scopes {
        let stored_app_type = target.stored_app_type(&scope.app_type);
        let sql = format!(
            "DELETE FROM {} WHERE app_type = ?1 AND session_id = ?2 AND data_source = ?3",
            target.rollup_table()
        );
        conn.execute(
            &sql,
            rusqlite::params![stored_app_type, &scope.session_id, &scope.data_source],
        )
        .map_err(|error| AppError::Database(format!("清理 canonical 来源范围失败: {error}")))?;
    }

    publish_observations_on_conn(
        conn,
        target,
        batch.observations,
        CanonicalFactMode::Additive,
    )?;
    publish_observations_on_conn(
        conn,
        target,
        batch.replacement_observations,
        CanonicalFactMode::Replace,
    )?;

    for fact in batch.facts {
        write_canonical_fact_on_conn(conn, target, fact.fact, fact.request_ids, fact.mode)?;
    }

    if target == UsagePublishTarget::CodexReplay && !batch.snapshots.is_empty() {
        return Err(AppError::InvalidInput(
            "Codex replay generation cannot publish live Hermes snapshots".into(),
        ));
    }
    for snapshot in batch.snapshots {
        Database::upsert_agent_session_usage_snapshot_on_conn(conn, &snapshot)?;
    }

    Ok(())
}

/// Publish a self-contained canonical batch in one transaction. Adapters that
/// also own raw rows, de-dup ledgers, or cursors continue to use the
/// connection-level entry point so every part of their source transaction
/// commits together.
pub(crate) fn publish_canonical_batch(
    db: &Database,
    target: UsagePublishTarget,
    batch: CanonicalUsageBatch,
    label: &str,
) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("开启 {label} canonical 事务失败: {error}")))?;
    publish_canonical_batch_on_conn(&tx, target, batch)?;
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 {label} canonical 事务失败: {error}")))
}

fn publish_observations_on_conn(
    conn: &Connection,
    target: UsagePublishTarget,
    observations: Vec<UsageObservation>,
    mode: CanonicalFactMode,
) -> Result<(), AppError> {
    let mut grouped: HashMap<NormalizedUsageRollupFact, AggregatedFact> = HashMap::new();
    for observation in observations {
        let key = fact_dimension_key(&observation.fact);
        if let Some(current) = grouped.get_mut(&key) {
            merge_fact_measurements(&mut current.fact, &observation.fact)?;
            current.request_ids.push(observation.request_id);
        } else {
            grouped.insert(
                key,
                AggregatedFact {
                    fact: observation.fact,
                    request_ids: vec![observation.request_id],
                },
            );
        }
    }

    let mut grouped: Vec<_> = grouped.into_values().collect();
    grouped.sort_by(|left, right| fact_sort_key(&left.fact).cmp(&fact_sort_key(&right.fact)));
    for aggregate in grouped {
        write_canonical_fact_on_conn(conn, target, aggregate.fact, aggregate.request_ids, mode)?;
    }
    Ok(())
}

/// Reserve native/proxy ownership in the same transaction as canonical facts.
/// A later failure rolls this marker back with the fact and cursor update.
pub(crate) fn reserve_canonical_coverage_on_conn(
    conn: &Connection,
    target: UsagePublishTarget,
    app_type: &str,
    data_source: &str,
    request_id: &str,
    canonical_session_id: Option<&str>,
    marked_at: i64,
) -> Result<(), AppError> {
    let marker = AgentSessionCanonicalCoverageMarker {
        app_type: target.stored_app_type(app_type),
        data_source: target.coverage_source(data_source),
        request_id: request_id.to_string(),
        canonical_session_id: canonical_session_id.map(str::to_string),
        marked_at,
    };
    Database::upsert_agent_session_canonical_coverage_on_conn_into(
        conn,
        &marker,
        target.coverage_table(),
    )
}

pub(crate) fn has_canonical_coverage_on_conn(
    conn: &Connection,
    target: UsagePublishTarget,
    app_type: &str,
    data_source: &str,
    request_id: &str,
) -> Result<bool, AppError> {
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {} WHERE app_type = ?1 AND data_source = ?2 AND request_id = ?3)",
        target.coverage_table()
    );
    conn.query_row(
        &sql,
        rusqlite::params![
            target.stored_app_type(app_type),
            target.coverage_source(data_source),
            request_id
        ],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
    .map_err(|error| AppError::Database(format!("读取 canonical 覆盖标记失败: {error}")))
}

fn write_node_to_target_on_conn(
    conn: &Connection,
    target: UsagePublishTarget,
    node: &NormalizedSessionNode,
) -> Result<(), AppError> {
    node.validate_for_persistence()?;
    let canonical_app_type = crate::app_config::AppType::from_str(&node.app_type)?;
    let stored_app_type = target.stored_app_type(canonical_app_type.as_str());
    Database::upsert_agent_session_node_on_conn_into(
        conn,
        node,
        target.node_table(),
        &stored_app_type,
    )
}

fn write_canonical_fact_on_conn(
    conn: &Connection,
    target: UsagePublishTarget,
    mut incoming: NormalizedUsageRollupFact,
    request_ids: Vec<String>,
    mode: CanonicalFactMode,
) -> Result<(), AppError> {
    if mode == CanonicalFactMode::Additive {
        if let Some(existing) = read_existing_fact_on_conn(conn, target, &incoming)? {
            merge_fact_measurements(&mut incoming, &existing)?;
        }
    }

    let canonical_session_id = incoming.session_id.clone();
    let data_source = incoming.data_source.clone();
    let app_type = incoming.app_type.clone();
    let marked_at = incoming
        .last_event_at
        .or(incoming.first_event_at)
        .unwrap_or(0);
    incoming.validate_for_persistence()?;
    let canonical_app_type = crate::app_config::AppType::from_str(&incoming.app_type)?;
    let stored_app_type = target.stored_app_type(canonical_app_type.as_str());
    Database::upsert_agent_session_usage_rollup_fact_on_conn_into(
        conn,
        &incoming,
        target.rollup_table(),
        &stored_app_type,
    )?;
    for request_id in request_ids {
        reserve_canonical_coverage_on_conn(
            conn,
            target,
            &app_type,
            &data_source,
            &request_id,
            Some(&canonical_session_id),
            marked_at,
        )?;
    }
    Ok(())
}

fn read_existing_fact_on_conn(
    conn: &Connection,
    target: UsagePublishTarget,
    fact: &NormalizedUsageRollupFact,
) -> Result<Option<NormalizedUsageRollupFact>, AppError> {
    fact.validate_for_persistence()?;
    let canonical_app_type = crate::app_config::AppType::from_str(&fact.app_type)?;
    let stored_app_type = target.stored_app_type(canonical_app_type.as_str());
    let sql = format!(
        "SELECT request_count, api_call_count, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, cache_write_tokens,
                reasoning_tokens, total_cost_usd, cost_status, cost_source,
                cost_delta_kind, correction_state, first_event_at, last_event_at
         FROM {}
         WHERE date = ?1 AND app_type = ?2 AND session_id = ?3 AND provider_id = ?4
           AND model = ?5 AND request_model = ?6 AND pricing_model = ?7
           AND data_source = ?8 AND precision = ?9 AND time_semantics = ?10
           AND request_count_semantics = ?11 AND input_token_semantics = ?12
           AND source_identity = ?13 AND profile_id = ?14 AND database_identity = ?15
           AND base_url_digest = ?16 AND billing_mode = ?17 AND task = ?18
           AND source_version = ?19 AND sync_window_start = ?20 AND sync_window_end = ?21",
        target.rollup_table()
    );
    conn.query_row(
        &sql,
        rusqlite::params![
            fact.date.trim(),
            stored_app_type,
            fact.session_id.trim(),
            fact.provider_id.trim(),
            fact.model.trim(),
            fact.request_model.trim(),
            fact.pricing_model.trim(),
            fact.data_source.trim(),
            fact.precision.as_str(),
            fact.time_semantics.as_str(),
            fact.request_count_semantics.as_str(),
            fact.input_token_semantics,
            fact.source_identity.trim(),
            fact.profile_id.trim(),
            fact.database_identity.trim(),
            fact.base_url_digest.trim(),
            fact.billing_mode.trim(),
            fact.task.trim(),
            fact.source_version.trim(),
            fact.sync_window_start,
            fact.sync_window_end,
        ],
        |row| {
            let mut existing = fact.clone();
            existing.request_count = row.get(0)?;
            existing.api_call_count = row.get(1)?;
            existing.input_tokens = row.get(2)?;
            existing.output_tokens = row.get(3)?;
            existing.cache_read_tokens = row.get(4)?;
            existing.cache_creation_tokens = row.get(5)?;
            existing.cache_write_tokens = row.get(6)?;
            existing.reasoning_tokens = row.get(7)?;
            existing.total_cost_usd = row.get(8)?;
            existing.cost_status = row.get(9)?;
            existing.cost_source = row.get(10)?;
            existing.cost_delta_kind = row.get(11)?;
            existing.correction_state = row.get(12)?;
            existing.first_event_at = row.get(13)?;
            existing.last_event_at = row.get(14)?;
            Ok(existing)
        },
    )
    .optional()
    .map_err(|error| AppError::Database(format!("读取 canonical 来源事实失败: {error}")))
}

fn merge_fact_measurements(
    current: &mut NormalizedUsageRollupFact,
    incoming: &NormalizedUsageRollupFact,
) -> Result<(), AppError> {
    current.request_count = merge_optional_sum(current.request_count, incoming.request_count);
    current.api_call_count = merge_optional_sum(current.api_call_count, incoming.api_call_count);
    current.input_tokens = merge_optional_sum(current.input_tokens, incoming.input_tokens);
    current.output_tokens = merge_optional_sum(current.output_tokens, incoming.output_tokens);
    current.cache_read_tokens =
        merge_optional_sum(current.cache_read_tokens, incoming.cache_read_tokens);
    current.cache_creation_tokens = merge_optional_sum(
        current.cache_creation_tokens,
        incoming.cache_creation_tokens,
    );
    current.cache_write_tokens =
        merge_optional_sum(current.cache_write_tokens, incoming.cache_write_tokens);
    current.reasoning_tokens =
        merge_optional_sum(current.reasoning_tokens, incoming.reasoning_tokens);
    current.total_cost_usd = merge_optional_decimal(
        current.total_cost_usd.as_deref(),
        incoming.total_cost_usd.as_deref(),
    )?;
    let previous_status = current.cost_status.clone();
    let next_status =
        merge_cost_status(previous_status.as_deref(), incoming.cost_status.as_deref());
    current.cost_source = match next_status.as_deref() {
        Some(status) if incoming.cost_status.as_deref() == Some(status) => {
            incoming.cost_source.clone()
        }
        Some(status) if previous_status.as_deref() == Some(status) => current.cost_source.clone(),
        _ => merge_metadata(current.cost_source.take(), incoming.cost_source.clone()),
    };
    current.cost_status = next_status;
    current.cost_delta_kind = merge_metadata(
        current.cost_delta_kind.take(),
        incoming.cost_delta_kind.clone(),
    );
    current.correction_state = merge_metadata(
        current.correction_state.take(),
        incoming.correction_state.clone(),
    );
    current.first_event_at = merge_optional_min(current.first_event_at, incoming.first_event_at);
    current.last_event_at = merge_optional_max(current.last_event_at, incoming.last_event_at);
    Ok(())
}

fn merge_optional_sum(current: Option<i64>, incoming: Option<i64>) -> Option<i64> {
    match (current, incoming) {
        (Some(left), Some(right)) => left.checked_add(right),
        _ => None,
    }
}

fn merge_optional_min(current: Option<i64>, incoming: Option<i64>) -> Option<i64> {
    match (current, incoming) {
        (Some(left), Some(right)) => Some(left.min(right)),
        _ => None,
    }
}

fn merge_optional_max(current: Option<i64>, incoming: Option<i64>) -> Option<i64> {
    match (current, incoming) {
        (Some(left), Some(right)) => Some(left.max(right)),
        _ => None,
    }
}

fn merge_optional_decimal(
    current: Option<&str>,
    incoming: Option<&str>,
) -> Result<Option<String>, AppError> {
    let (Some(current), Some(incoming)) = (current, incoming) else {
        return Ok(None);
    };
    let current = Decimal::from_str(current)
        .map_err(|error| AppError::Database(format!("无法汇总已存 canonical 费用: {error}")))?;
    let incoming = Decimal::from_str(incoming)
        .map_err(|error| AppError::InvalidInput(format!("无法汇总来源 canonical 费用: {error}")))?;
    Ok(Some((current + incoming).normalize().to_string()))
}

fn merge_cost_status(current: Option<&str>, incoming: Option<&str>) -> Option<String> {
    match (current, incoming) {
        (Some("unavailable"), _) | (_, Some("unavailable")) => Some("unavailable".to_string()),
        (Some("partial"), _) | (_, Some("partial")) => Some("partial".to_string()),
        (Some("unknown"), _) | (_, Some("unknown")) => Some("unknown".to_string()),
        (Some("estimated"), _) | (_, Some("estimated")) => Some("estimated".to_string()),
        (Some("reported"), Some("reported")) => Some("reported".to_string()),
        (Some("complete"), Some("complete")) => Some("complete".to_string()),
        (Some(left), Some(right)) if left == right => Some(left.to_string()),
        (Some(left), None) => Some(left.to_string()),
        (None, Some(right)) => Some(right.to_string()),
        (None, None) => None,
        // A pair of incompatible source status labels is explicitly partial;
        // it must not be promoted to either input label.
        (Some(_), Some(_)) => Some("partial".to_string()),
    }
}

fn merge_metadata(current: Option<String>, incoming: Option<String>) -> Option<String> {
    match (current, incoming) {
        (Some(current), Some(incoming)) if current == incoming => Some(current),
        (Some(_), Some(_)) => None,
        (Some(current), None) => Some(current),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn fact_sort_key(fact: &NormalizedUsageRollupFact) -> (&str, &str, &str, &str, &str) {
    (
        &fact.app_type,
        &fact.session_id,
        &fact.date,
        &fact.model,
        &fact.data_source,
    )
}

/// Internal provider interface.  The registry retains the established sync
/// order but makes normal sync and provider rebuild share one source of truth
/// for provider ownership and entry points.
pub(crate) trait SessionUsageAdapter: Sync {
    fn app_type(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn sync(&self, db: &Database) -> Result<SessionSyncResult, AppError>;
    fn sync_for_rebuild(&self, db: &Database) -> Option<Result<SessionSyncResult, AppError>>;
    fn preflight(&self) -> Option<Result<(), AppError>>;
}

type UsageAdapterSync = fn(&Database) -> Result<SessionSyncResult, AppError>;

#[derive(Clone, Copy)]
pub(crate) struct StaticSessionUsageAdapter {
    pub app_type: &'static str,
    pub display_name: &'static str,
    pub sync: UsageAdapterSync,
    pub rebuild_sync: Option<UsageAdapterSync>,
    pub preflight: Option<fn() -> Result<(), AppError>>,
}

impl SessionUsageAdapter for StaticSessionUsageAdapter {
    fn app_type(&self) -> &'static str {
        self.app_type
    }

    fn display_name(&self) -> &'static str {
        self.display_name
    }

    fn sync(&self, db: &Database) -> Result<SessionSyncResult, AppError> {
        (self.sync)(db)
    }

    fn sync_for_rebuild(&self, db: &Database) -> Option<Result<SessionSyncResult, AppError>> {
        self.rebuild_sync.map(|sync| sync(db))
    }

    fn preflight(&self) -> Option<Result<(), AppError>> {
        self.preflight.map(|preflight| preflight())
    }
}

/// The static registry is declared in `session_usage` because its Claude,
/// Cowork, OpenClaw, and Hermes wrappers are intentionally private to the
/// session-usage integration layer.  Keeping the lookup here avoids a second
/// provider match tree in rebuild code.
pub(crate) fn adapter_for(app_type: &str) -> Option<&'static dyn SessionUsageAdapter> {
    crate::services::session_usage::registered_usage_adapters()
        .iter()
        .find(|adapter| SessionUsageAdapter::app_type(*adapter) == app_type)
        .map(|adapter| adapter as &dyn SessionUsageAdapter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::services::agent_session_usage::{
        RelationClaim, SessionNodeMetadata, SessionRelationClaim,
    };

    fn spec() -> UsageSourceSpec {
        UsageSourceSpec::new(
            "claude",
            "_session",
            "session_log",
            UsagePrecision::RequestExact,
            TimeSemantics::EventTime,
            RequestCountSemantics::AgentCall,
        )
    }

    fn fact(
        session_id: &str,
        request_count: Option<i64>,
        input: Option<i64>,
    ) -> NormalizedUsageRollupFact {
        let mut fact = spec().fact("2026-08-16", session_id, "model", "model", "model");
        fact.request_count = request_count;
        fact.input_tokens = input;
        fact.output_tokens = Some(0);
        fact.first_event_at = Some(10);
        fact.last_event_at = Some(10);
        fact
    }

    fn table_count(db: &Database, table: &str) -> Result<i64, AppError> {
        crate::database::lock_conn!(db.conn)
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(AppError::from)
    }

    #[test]
    fn aggregates_nullable_components_without_collapsing_full_dimensions() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut batch = CanonicalUsageBatch::default();
        let mut explicit_zero = fact("session", Some(1), Some(0));
        explicit_zero.cache_creation_tokens = Some(0);
        batch.observe("zero", explicit_zero);
        batch.observe("unknown", fact("session", Some(1), None));

        let mut separate = fact("session", Some(1), Some(0));
        separate.database_identity = "separate-source".into();
        separate.cache_creation_tokens = Some(0);
        batch.observe("separate", separate);
        publish_canonical_batch(
            &db,
            UsagePublishTarget::Published,
            batch,
            "pipeline fixture",
        )?;

        let conn = crate::database::lock_conn!(db.conn);
        let rows = conn
            .prepare(
                "SELECT database_identity, request_count, input_tokens, cache_creation_tokens
                 FROM agent_session_usage_rollups ORDER BY database_identity",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                ("".into(), Some(2), None, None),
                ("separate-source".into(), Some(1), Some(0), Some(0)),
            ]
        );
        Ok(())
    }

    #[test]
    fn writes_nodes_facts_and_coverage_as_one_transaction() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut batch = CanonicalUsageBatch::default();
        batch.relation_claims.push(SessionRelationClaim {
            app_type: "claude".into(),
            session_id: "session".into(),
            relation: RelationClaim::Standalone,
            metadata: SessionNodeMetadata::default(),
        });
        batch.observe("request", fact("session", Some(1), Some(1)));
        publish_canonical_batch(
            &db,
            UsagePublishTarget::Published,
            batch,
            "pipeline fixture",
        )?;
        let rows = [
            table_count(&db, "agent_session_nodes")?,
            table_count(&db, "agent_session_usage_rollups")?,
            table_count(&db, "agent_session_canonical_coverage")?,
        ]
        .into_iter()
        .sum::<i64>();
        assert_eq!(rows, 3);
        Ok(())
    }

    #[test]
    fn replacement_scope_keeps_other_sources_and_nullable_fact_fields() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut initial = CanonicalUsageBatch::default();
        let mut native = fact("session", Some(1), Some(10));
        native.cache_write_tokens = Some(6);
        native.data_source = "native".into();
        initial.observe("native-old", native);
        let mut proxy = fact("session", Some(1), Some(5));
        proxy.data_source = "proxy".into();
        initial.observe("proxy", proxy);
        publish_canonical_batch(
            &db,
            UsagePublishTarget::Published,
            initial,
            "pipeline fixture",
        )?;

        let mut replacement = fact("session", Some(1), Some(99));
        replacement.data_source = "native".into();
        replacement.cache_creation_tokens = None;
        replacement.cache_write_tokens = Some(6);
        let mut batch = CanonicalUsageBatch::default();
        batch.replace_scopes.push(CanonicalReplaceScope {
            app_type: "claude".into(),
            session_id: "session".into(),
            data_source: "native".into(),
        });
        batch.replace_observe("native-new", replacement);
        publish_canonical_batch(
            &db,
            UsagePublishTarget::Published,
            batch,
            "pipeline fixture",
        )?;

        let conn = crate::database::lock_conn!(db.conn);
        let rows = conn
            .prepare(
                "SELECT data_source, input_tokens, cache_creation_tokens, cache_write_tokens
                 FROM agent_session_usage_rollups ORDER BY data_source",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                ("native".into(), Some(99), None, Some(6)),
                ("proxy".into(), Some(5), None, None),
            ]
        );
        Ok(())
    }

    #[test]
    fn replacement_scope_clears_stale_source_and_proxy_coverage() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut initial = CanonicalUsageBatch::default();
        let mut old_fact = fact("session", Some(1), Some(10));
        old_fact.data_source = "native".into();
        initial.observe("native-old", old_fact);
        publish_canonical_batch(
            &db,
            UsagePublishTarget::Published,
            initial,
            "pipeline fixture",
        )?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            reserve_canonical_coverage_on_conn(
                &conn,
                UsagePublishTarget::Published,
                "claude",
                "proxy",
                "proxy-old",
                Some("session"),
                10,
            )?;
            reserve_canonical_coverage_on_conn(
                &conn,
                UsagePublishTarget::Published,
                "claude",
                "other-source",
                "other-source-marker",
                Some("session"),
                10,
            )?;
        }

        let mut replacement = fact("session", Some(1), Some(99));
        replacement.data_source = "native".into();
        let mut batch = CanonicalUsageBatch::default();
        batch.replace_scopes.push(CanonicalReplaceScope {
            app_type: "claude".into(),
            session_id: "session".into(),
            data_source: "native".into(),
        });
        batch.replace_observe("native-new", replacement);
        publish_canonical_batch(
            &db,
            UsagePublishTarget::Published,
            batch,
            "pipeline fixture",
        )?;

        let conn = crate::database::lock_conn!(db.conn);
        let coverage = conn
            .prepare(
                "SELECT data_source, request_id
                 FROM agent_session_canonical_coverage
                 WHERE app_type = 'claude' AND canonical_session_id = 'session'
                 ORDER BY data_source, request_id",
            )?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            coverage,
            vec![
                ("native".into(), "native-new".into()),
                ("other-source".into(), "other-source-marker".into()),
            ]
        );
        Ok(())
    }

    #[test]
    fn caller_rollback_keeps_nodes_facts_coverage_and_cursor_together() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = crate::database::lock_conn!(db.conn);
        {
            let tx = conn.unchecked_transaction()?;
            let mut batch = CanonicalUsageBatch::default();
            batch
                .relation_claims
                .push(SessionRelationClaim::standalone("claude", "session"));
            batch.observe("request", fact("session", Some(1), Some(1)));
            publish_canonical_batch_on_conn(&tx, UsagePublishTarget::Published, batch)?;
            crate::services::session_usage::update_sync_state_on_conn(&tx, "fixture.jsonl", 10, 2)?;
        }
        drop(conn);

        let rows = [
            table_count(&db, "agent_session_nodes")?,
            table_count(&db, "agent_session_usage_rollups")?,
            table_count(&db, "agent_session_canonical_coverage")?,
            table_count(&db, "session_log_sync")?,
        ]
        .into_iter()
        .sum::<i64>();
        assert_eq!(rows, 0);
        Ok(())
    }

    #[test]
    fn replay_target_never_writes_live_canonical_tables() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut batch = CanonicalUsageBatch::default();
        batch.observe("request", fact("session", Some(1), Some(1)));
        publish_canonical_batch(
            &db,
            UsagePublishTarget::CodexReplay,
            batch,
            "pipeline fixture",
        )?;
        let live = table_count(&db, "agent_session_usage_rollups")?;
        let replay = table_count(&db, "codex_replay_rollups")?;
        assert_eq!(live, 0);
        assert_eq!(replay, 1);
        Ok(())
    }
}
