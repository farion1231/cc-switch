//! Durable Agent session-node and session-usage rollup primitives.
//!
//! This module deliberately exposes only node and bucket writes.  A task total
//! is derived by the query/service layer and is never stored as a third,
//! independently mutable measure.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};

/// Normalized session relationship metadata persisted for every discovered
/// session.  `root_session_id` is denormalized intentionally so read-side
/// aggregation does not need to recursively walk large child trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSessionNode {
    pub app_type: String,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub root_session_id: String,
    pub node_kind: String,
    pub relation_confidence: String,
    pub title: Option<String>,
    pub project_dir: Option<String>,
    pub source_path: Option<String>,
    pub created_at: Option<i64>,
    pub last_active_at: Option<i64>,
    pub last_synced_at: i64,
}

/// A single day/session/real-source-dimension usage bucket.
///
/// Compatibility shape used by the normalized service bridge.  Its upsert
/// writes stable empty/default values for the extended source dimensions;
/// sources that carry real profile/database/window semantics must use
/// [`AgentSessionUsageRollupFact`] instead.  `total_cost_usd` remains nullable
/// when the source cannot prove a cost value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSessionUsageRollup {
    pub date: String,
    pub app_type: String,
    pub session_id: String,
    pub provider_id: String,
    pub model: String,
    pub request_model: String,
    pub pricing_model: String,
    pub data_source: String,
    pub precision: String,
    pub time_semantics: String,
    pub request_count_semantics: String,
    pub request_count: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub total_cost_usd: Option<String>,
    pub first_event_at: Option<i64>,
    pub last_event_at: Option<i64>,
}

/// Full-fidelity durable session fact used by sources that expose dimensions
/// beyond the generic normalized bucket (notably Hermes).  This is still a
/// day/session usage bucket, never an independently stored task total.  The
/// source cumulative snapshot baseline remains a separate table; callers that
/// need an atomic snapshot+delta update should use the two `_on_conn` methods
/// inside one transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSessionUsageRollupFact {
    pub date: String,
    pub app_type: String,
    pub session_id: String,
    pub provider_id: String,
    pub model: String,
    pub request_model: String,
    pub pricing_model: String,
    pub data_source: String,
    pub precision: String,
    pub time_semantics: String,
    pub request_count_semantics: String,
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
    /// Hermes exposes cache-write separately from generic cache creation;
    /// preserve that source metric without mapping it into another component.
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub total_cost_usd: Option<String>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub cost_delta_kind: Option<String>,
    pub correction_state: Option<String>,
    pub first_event_at: Option<i64>,
    pub last_event_at: Option<i64>,
}

/// Per-request proof that a source row has already been represented by a
/// direct canonical session bucket.  Callers provide canonicalized
/// `app_type`, `data_source`, and `request_id`; this DAO never infers source
/// identity or stores request payload content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSessionCanonicalCoverageMarker {
    pub app_type: String,
    pub data_source: String,
    pub request_id: String,
    pub canonical_session_id: Option<String>,
    pub marked_at: i64,
}

/// Full identity of a source cumulative snapshot.
///
/// `source_identity` is a stable source/database namespace supplied by the
/// importer (for example a canonical provider/profile/database identity); it
/// must not be a bare filename or session id.  The remaining dimensions keep
/// profiles, database incarnations, models, endpoints and tasks isolated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSessionUsageSnapshotKey {
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
}

/// Durable source cumulative snapshot used as a synchronization baseline.
///
/// These counters are the latest non-negative values observed in the source
/// database, not user-facing usage totals.  Callers compute a sync-window
/// delta before writing regular session usage rollups.  Upsert replaces one
/// complete identity row; reset/replacement code should use the explicit
/// delete methods rather than smuggling reset semantics into this write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSessionUsageSnapshot {
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

impl AgentSessionUsageSnapshot {
    #[cfg(test)]
    pub(crate) fn key(&self) -> AgentSessionUsageSnapshotKey {
        AgentSessionUsageSnapshotKey {
            app_type: self.app_type.clone(),
            source_identity: self.source_identity.clone(),
            profile_id: self.profile_id.clone(),
            database_identity: self.database_identity.clone(),
            session_id: self.session_id.clone(),
            model: self.model.clone(),
            provider_id: self.provider_id.clone(),
            base_url_digest: self.base_url_digest.clone(),
            billing_mode: self.billing_mode.clone(),
            task: self.task.clone(),
            data_source: self.data_source.clone(),
            source_version: self.source_version.clone(),
        }
    }
}

impl Database {
    /// Insert or update a normalized session node.  Optional metadata is
    /// preserved when a later sync has no value for that field.
    pub(crate) fn upsert_agent_session_node(
        &self,
        node: &AgentSessionNode,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::upsert_agent_session_node_on_conn(&conn, node)
    }

    pub(crate) fn upsert_agent_session_node_on_conn(
        conn: &Connection,
        node: &AgentSessionNode,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO agent_session_nodes (
                app_type, session_id, parent_session_id, root_session_id,
                node_kind, relation_confidence, title, project_dir, source_path,
                created_at, last_active_at, last_synced_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(app_type, session_id) DO UPDATE SET
                parent_session_id = excluded.parent_session_id,
                root_session_id = excluded.root_session_id,
                node_kind = excluded.node_kind,
                relation_confidence = excluded.relation_confidence,
                title = COALESCE(excluded.title, agent_session_nodes.title),
                project_dir = COALESCE(excluded.project_dir, agent_session_nodes.project_dir),
                source_path = COALESCE(excluded.source_path, agent_session_nodes.source_path),
                created_at = COALESCE(excluded.created_at, agent_session_nodes.created_at),
                last_active_at = COALESCE(excluded.last_active_at, agent_session_nodes.last_active_at),
                last_synced_at = excluded.last_synced_at",
            params![
                node.app_type,
                node.session_id,
                node.parent_session_id,
                node.root_session_id,
                node.node_kind,
                node.relation_confidence,
                node.title,
                node.project_dir,
                node.source_path,
                node.created_at,
                node.last_active_at,
                node.last_synced_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("写入 Agent 会话节点失败: {e}")))?;
        Ok(())
    }

    /// Replace a single durable bucket.  Callers that ingest deltas should
    /// compute the delta before calling this method; this API never accepts a
    /// task-level total.
    pub(crate) fn upsert_agent_session_usage_rollup(
        &self,
        rollup: &AgentSessionUsageRollup,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::upsert_agent_session_usage_rollup_on_conn(&conn, rollup)
    }

    pub(crate) fn upsert_agent_session_usage_rollup_on_conn(
        conn: &Connection,
        rollup: &AgentSessionUsageRollup,
    ) -> Result<(), AppError> {
        let fact = AgentSessionUsageRollupFact {
            date: rollup.date.clone(),
            app_type: rollup.app_type.clone(),
            session_id: rollup.session_id.clone(),
            provider_id: rollup.provider_id.clone(),
            model: rollup.model.clone(),
            request_model: rollup.request_model.clone(),
            pricing_model: rollup.pricing_model.clone(),
            data_source: rollup.data_source.clone(),
            precision: rollup.precision.clone(),
            time_semantics: rollup.time_semantics.clone(),
            request_count_semantics: rollup.request_count_semantics.clone(),
            input_token_semantics: 0,
            source_identity: String::new(),
            profile_id: String::new(),
            database_identity: String::new(),
            base_url_digest: String::new(),
            billing_mode: String::new(),
            task: String::new(),
            source_version: String::new(),
            sync_window_start: 0,
            sync_window_end: 0,
            request_count: rollup.request_count,
            api_call_count: None,
            input_tokens: rollup.input_tokens,
            output_tokens: rollup.output_tokens,
            cache_read_tokens: rollup.cache_read_tokens,
            cache_creation_tokens: rollup.cache_creation_tokens,
            cache_write_tokens: None,
            reasoning_tokens: None,
            total_cost_usd: rollup.total_cost_usd.clone(),
            cost_status: None,
            cost_source: None,
            cost_delta_kind: None,
            correction_state: None,
            first_event_at: rollup.first_event_at,
            last_event_at: rollup.last_event_at,
        };
        Self::upsert_agent_session_usage_rollup_fact_on_conn(conn, &fact)
    }

    /// Replace a full-fidelity source usage fact.  The complete real
    /// dimension/window tuple is the conflict key; this operation never adds
    /// to an existing row and never stores a task total.
    #[cfg(test)]
    pub(crate) fn upsert_agent_session_usage_rollup_fact(
        &self,
        fact: &AgentSessionUsageRollupFact,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::upsert_agent_session_usage_rollup_fact_on_conn(&conn, fact)
    }

    pub(crate) fn upsert_agent_session_usage_rollup_fact_on_conn(
        conn: &Connection,
        fact: &AgentSessionUsageRollupFact,
    ) -> Result<(), AppError> {
        Self::upsert_agent_session_usage_rollup_fact_on_conn_into(
            conn,
            fact,
            "agent_session_usage_rollups",
        )
    }

    /// Same upsert targeted at an internal generation table used by Codex
    /// shadow replay.  The table name is selected only by trusted callers.
    pub(crate) fn upsert_agent_session_usage_rollup_fact_on_conn_into(
        conn: &Connection,
        fact: &AgentSessionUsageRollupFact,
        table: &str,
    ) -> Result<(), AppError> {
        conn.execute(
            &format!(
                "INSERT INTO {table} (
                date, app_type, session_id, provider_id, model,
                request_model, pricing_model, data_source, precision,
                time_semantics, request_count_semantics, input_token_semantics,
                source_identity, profile_id, database_identity, base_url_digest,
                billing_mode, task, source_version, sync_window_start,
                sync_window_end, request_count, api_call_count, input_tokens,
                output_tokens, cache_read_tokens, cache_creation_tokens,
                cache_write_tokens, reasoning_tokens, total_cost_usd, cost_status, cost_source,
                cost_delta_kind, correction_state, first_event_at, last_event_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23,
                ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36
             )
             ON CONFLICT(
                date, app_type, session_id, provider_id, model,
                request_model, pricing_model, data_source, precision,
                time_semantics, request_count_semantics, input_token_semantics,
                source_identity, profile_id, database_identity, base_url_digest,
                billing_mode, task, source_version, sync_window_start,
                sync_window_end
             ) DO UPDATE SET
                request_count = excluded.request_count,
                api_call_count = excluded.api_call_count,
                input_tokens = excluded.input_tokens,
                output_tokens = excluded.output_tokens,
                cache_read_tokens = excluded.cache_read_tokens,
                cache_creation_tokens = excluded.cache_creation_tokens,
                cache_write_tokens = excluded.cache_write_tokens,
                reasoning_tokens = excluded.reasoning_tokens,
                total_cost_usd = excluded.total_cost_usd,
                cost_status = excluded.cost_status,
                cost_source = excluded.cost_source,
                cost_delta_kind = excluded.cost_delta_kind,
                correction_state = excluded.correction_state,
                first_event_at = excluded.first_event_at,
             last_event_at = excluded.last_event_at"
            ),
            params![
                &fact.date,
                &fact.app_type,
                &fact.session_id,
                &fact.provider_id,
                &fact.model,
                &fact.request_model,
                &fact.pricing_model,
                &fact.data_source,
                &fact.precision,
                &fact.time_semantics,
                &fact.request_count_semantics,
                fact.input_token_semantics,
                &fact.source_identity,
                &fact.profile_id,
                &fact.database_identity,
                &fact.base_url_digest,
                &fact.billing_mode,
                &fact.task,
                &fact.source_version,
                fact.sync_window_start,
                fact.sync_window_end,
                fact.request_count,
                fact.api_call_count,
                fact.input_tokens,
                fact.output_tokens,
                fact.cache_read_tokens,
                fact.cache_creation_tokens,
                fact.cache_write_tokens,
                fact.reasoning_tokens,
                &fact.total_cost_usd,
                &fact.cost_status,
                &fact.cost_source,
                &fact.cost_delta_kind,
                &fact.correction_state,
                fact.first_event_at,
                fact.last_event_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("写入完整 Agent 会话用量桶失败: {e}")))?;
        Ok(())
    }

    /// Atomically persist one source delta fact and its cumulative baseline
    /// when the caller already owns a transaction.  The helper is deliberately
    /// explicit about both records; it does not provide a baseline-only or
    /// task-total shortcut.
    pub(crate) fn upsert_agent_session_usage_hermes_delta_on_conn(
        conn: &Connection,
        fact: &AgentSessionUsageRollupFact,
        snapshot: &AgentSessionUsageSnapshot,
    ) -> Result<(), AppError> {
        Self::upsert_agent_session_usage_rollup_fact_on_conn(conn, fact)?;
        Self::upsert_agent_session_usage_snapshot_on_conn(conn, snapshot)
    }

    /// Database-wrapper variant for callers that do not already own a
    /// connection transaction.  A savepoint keeps fact and baseline writes
    /// atomic without exposing an implicit baseline-only/reset behavior.
    #[cfg(test)]
    pub(crate) fn upsert_agent_session_usage_hermes_delta(
        &self,
        fact: &AgentSessionUsageRollupFact,
        snapshot: &AgentSessionUsageSnapshot,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("SAVEPOINT hermes_delta_upsert", [])
            .map_err(|e| AppError::Database(format!("开启 Hermes delta savepoint 失败: {e}")))?;
        let result = Self::upsert_agent_session_usage_hermes_delta_on_conn(&conn, fact, snapshot);
        match result {
            Ok(()) => {
                conn.execute("RELEASE hermes_delta_upsert", [])
                    .map_err(|e| {
                        AppError::Database(format!("提交 Hermes delta savepoint 失败: {e}"))
                    })?;
                Ok(())
            }
            Err(error) => {
                conn.execute("ROLLBACK TO hermes_delta_upsert", []).ok();
                conn.execute("RELEASE hermes_delta_upsert", []).ok();
                Err(error)
            }
        }
    }

    /// Read a source cumulative snapshot by its complete identity.
    #[cfg(test)]
    pub(crate) fn get_agent_session_usage_snapshot(
        &self,
        key: &AgentSessionUsageSnapshotKey,
    ) -> Result<Option<AgentSessionUsageSnapshot>, AppError> {
        let conn = lock_conn!(self.conn);
        Self::get_agent_session_usage_snapshot_on_conn(&conn, key)
    }

    pub(crate) fn get_agent_session_usage_snapshot_on_conn(
        conn: &Connection,
        key: &AgentSessionUsageSnapshotKey,
    ) -> Result<Option<AgentSessionUsageSnapshot>, AppError> {
        let mut stmt = conn
            .prepare(
                "SELECT app_type, source_identity, profile_id, database_identity,
                        session_id, model, provider_id, base_url_digest, billing_mode,
                        task, data_source, source_version,
                        api_call_count, input_tokens, output_tokens, cache_read_tokens,
                        cache_write_tokens, reasoning_tokens, first_seen, last_seen,
                        last_synced_at, estimated_cost_usd, actual_cost_usd,
                        cost_status, cost_source, correction_state
                 FROM agent_session_usage_snapshots
                 WHERE app_type = ?1 AND source_identity = ?2 AND profile_id = ?3
                   AND database_identity = ?4 AND session_id = ?5 AND model = ?6
                   AND provider_id = ?7 AND base_url_digest = ?8 AND billing_mode = ?9
                   AND task = ?10 AND data_source = ?11 AND source_version = ?12",
            )
            .map_err(|e| AppError::Database(format!("读取来源累计快照失败: {e}")))?;
        let snapshot = stmt
            .query_row(
                params![
                    &key.app_type,
                    &key.source_identity,
                    &key.profile_id,
                    &key.database_identity,
                    &key.session_id,
                    &key.model,
                    &key.provider_id,
                    &key.base_url_digest,
                    &key.billing_mode,
                    &key.task,
                    &key.data_source,
                    &key.source_version,
                ],
                |row| {
                    Ok(AgentSessionUsageSnapshot {
                        app_type: row.get(0)?,
                        source_identity: row.get(1)?,
                        profile_id: row.get(2)?,
                        database_identity: row.get(3)?,
                        session_id: row.get(4)?,
                        model: row.get(5)?,
                        provider_id: row.get(6)?,
                        base_url_digest: row.get(7)?,
                        billing_mode: row.get(8)?,
                        task: row.get(9)?,
                        data_source: row.get(10)?,
                        source_version: row.get(11)?,
                        api_call_count: row.get(12)?,
                        input_tokens: row.get(13)?,
                        output_tokens: row.get(14)?,
                        cache_read_tokens: row.get(15)?,
                        cache_write_tokens: row.get(16)?,
                        reasoning_tokens: row.get(17)?,
                        first_seen: row.get(18)?,
                        last_seen: row.get(19)?,
                        last_synced_at: row.get(20)?,
                        estimated_cost_usd: row.get(21)?,
                        actual_cost_usd: row.get(22)?,
                        cost_status: row.get(23)?,
                        cost_source: row.get(24)?,
                        correction_state: row.get(25)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AppError::Database(format!("读取来源累计快照失败: {e}")))?;
        Ok(snapshot)
    }

    /// Replace one source cumulative snapshot.  This method does not add to
    /// counters and has no implicit reset behavior; callers can wrap the
    /// `_on_conn` variant in their own transaction with rollup writes.
    #[cfg(test)]
    pub(crate) fn upsert_agent_session_usage_snapshot(
        &self,
        snapshot: &AgentSessionUsageSnapshot,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::upsert_agent_session_usage_snapshot_on_conn(&conn, snapshot)
    }

    pub(crate) fn upsert_agent_session_usage_snapshot_on_conn(
        conn: &Connection,
        snapshot: &AgentSessionUsageSnapshot,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO agent_session_usage_snapshots (
                app_type, source_identity, profile_id, database_identity,
                session_id, model, provider_id, base_url_digest, billing_mode,
                task, data_source, source_version, api_call_count, input_tokens,
                output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, first_seen, last_seen, last_synced_at,
                estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
                correction_state
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25,
                ?26
             )
             ON CONFLICT(
                app_type, source_identity, profile_id, database_identity,
                session_id, model, provider_id, base_url_digest, billing_mode,
                task, data_source, source_version
             ) DO UPDATE SET
                api_call_count = excluded.api_call_count,
                input_tokens = excluded.input_tokens,
                output_tokens = excluded.output_tokens,
                cache_read_tokens = excluded.cache_read_tokens,
                cache_write_tokens = excluded.cache_write_tokens,
                reasoning_tokens = excluded.reasoning_tokens,
                first_seen = excluded.first_seen,
                last_seen = excluded.last_seen,
                last_synced_at = excluded.last_synced_at,
                estimated_cost_usd = excluded.estimated_cost_usd,
                actual_cost_usd = excluded.actual_cost_usd,
                cost_status = excluded.cost_status,
                cost_source = excluded.cost_source,
                correction_state = excluded.correction_state",
            params![
                &snapshot.app_type,
                &snapshot.source_identity,
                &snapshot.profile_id,
                &snapshot.database_identity,
                &snapshot.session_id,
                &snapshot.model,
                &snapshot.provider_id,
                &snapshot.base_url_digest,
                &snapshot.billing_mode,
                &snapshot.task,
                &snapshot.data_source,
                &snapshot.source_version,
                snapshot.api_call_count,
                snapshot.input_tokens,
                snapshot.output_tokens,
                snapshot.cache_read_tokens,
                snapshot.cache_write_tokens,
                snapshot.reasoning_tokens,
                snapshot.first_seen,
                snapshot.last_seen,
                snapshot.last_synced_at,
                &snapshot.estimated_cost_usd,
                &snapshot.actual_cost_usd,
                &snapshot.cost_status,
                &snapshot.cost_source,
                &snapshot.correction_state,
            ],
        )
        .map_err(|e| AppError::Database(format!("写入来源累计快照失败: {e}")))?;
        Ok(())
    }

    /// Delete one snapshot for an explicit source reset/replacement.  Returns
    /// whether a matching baseline row existed.
    #[cfg(test)]
    pub(crate) fn delete_agent_session_usage_snapshot(
        &self,
        key: &AgentSessionUsageSnapshotKey,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        Self::delete_agent_session_usage_snapshot_on_conn(&conn, key)
    }

    #[cfg(test)]
    pub(crate) fn delete_agent_session_usage_snapshot_on_conn(
        conn: &Connection,
        key: &AgentSessionUsageSnapshotKey,
    ) -> Result<bool, AppError> {
        let changed = conn
            .execute(
                "DELETE FROM agent_session_usage_snapshots
                 WHERE app_type = ?1 AND source_identity = ?2 AND profile_id = ?3
                   AND database_identity = ?4 AND session_id = ?5 AND model = ?6
                   AND provider_id = ?7 AND base_url_digest = ?8 AND billing_mode = ?9
                   AND task = ?10 AND data_source = ?11 AND source_version = ?12",
                params![
                    &key.app_type,
                    &key.source_identity,
                    &key.profile_id,
                    &key.database_identity,
                    &key.session_id,
                    &key.model,
                    &key.provider_id,
                    &key.base_url_digest,
                    &key.billing_mode,
                    &key.task,
                    &key.data_source,
                    &key.source_version,
                ],
            )
            .map_err(|e| AppError::Database(format!("删除来源累计快照失败: {e}")))?;
        Ok(changed > 0)
    }

    /// Mark one raw request as covered by a successful direct canonical write.
    #[cfg(test)]
    pub(crate) fn upsert_agent_session_canonical_coverage(
        &self,
        marker: &AgentSessionCanonicalCoverageMarker,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::upsert_agent_session_canonical_coverage_on_conn(&conn, marker)
    }

    pub(crate) fn upsert_agent_session_canonical_coverage_on_conn(
        conn: &Connection,
        marker: &AgentSessionCanonicalCoverageMarker,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO agent_session_canonical_coverage (
                app_type, data_source, request_id, canonical_session_id, marked_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(app_type, data_source, request_id) DO UPDATE SET
                canonical_session_id = excluded.canonical_session_id,
                marked_at = excluded.marked_at",
            params![
                &marker.app_type,
                &marker.data_source,
                &marker.request_id,
                &marker.canonical_session_id,
                marker.marked_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("写入规范覆盖标记失败: {e}")))?;
        Ok(())
    }

    pub(crate) fn has_agent_session_canonical_coverage_on_conn(
        conn: &Connection,
        app_type: &str,
        data_source: &str,
        request_id: &str,
    ) -> Result<bool, AppError> {
        let covered: i64 = conn
            .query_row(
                "SELECT EXISTS(
                SELECT 1 FROM agent_session_canonical_coverage
                WHERE app_type = ?1 AND data_source = ?2 AND request_id = ?3
             )",
                params![app_type, data_source, request_id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(format!("读取规范覆盖标记失败: {e}")))?;
        Ok(covered != 0)
    }

    #[cfg(test)]
    pub(crate) fn delete_agent_session_canonical_coverage(
        &self,
        marker: &AgentSessionCanonicalCoverageMarker,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        Self::delete_agent_session_canonical_coverage_on_conn(&conn, marker)
    }

    #[cfg(test)]
    pub(crate) fn delete_agent_session_canonical_coverage_on_conn(
        conn: &Connection,
        marker: &AgentSessionCanonicalCoverageMarker,
    ) -> Result<bool, AppError> {
        let deleted = conn
            .execute(
                "DELETE FROM agent_session_canonical_coverage
             WHERE app_type = ?1 AND data_source = ?2 AND request_id = ?3",
                params![&marker.app_type, &marker.data_source, &marker.request_id],
            )
            .map_err(|e| AppError::Database(format!("删除规范覆盖标记失败: {e}")))?;
        Ok(deleted > 0)
    }

    #[cfg(test)]
    pub(crate) fn delete_agent_session_canonical_coverage_for_source(
        &self,
        app_type: &str,
        data_source: &str,
    ) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        Self::delete_agent_session_canonical_coverage_for_source_on_conn(
            &conn,
            app_type,
            data_source,
        )
    }

    pub(crate) fn delete_agent_session_canonical_coverage_for_source_on_conn(
        conn: &Connection,
        app_type: &str,
        data_source: &str,
    ) -> Result<usize, AppError> {
        let deleted = conn
            .execute(
                "DELETE FROM agent_session_canonical_coverage
             WHERE app_type = ?1 AND data_source = ?2",
                params![app_type, data_source],
            )
            .map_err(|e| AppError::Database(format!("删除来源规范覆盖标记失败: {e}")))?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_upsert_preserves_and_overwrites_optional_metadata() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut node = AgentSessionNode {
            app_type: "claude".into(),
            session_id: "root".into(),
            parent_session_id: None,
            root_session_id: "root".into(),
            node_kind: "root".into(),
            relation_confidence: "explicit".into(),
            title: Some("Build".into()),
            project_dir: Some("C:/project".into()),
            source_path: Some("C:/sessions/root.jsonl".into()),
            created_at: Some(10),
            last_active_at: Some(20),
            last_synced_at: 30,
        };
        db.upsert_agent_session_node(&node)?;
        node.title = Some("Renamed native title".into());
        node.project_dir = Some("C:/renamed-project".into());
        node.last_synced_at = 35;
        db.upsert_agent_session_node(&node)?;
        node.title = None;
        node.project_dir = None;
        node.source_path = None;
        node.created_at = None;
        node.last_active_at = None;
        node.last_synced_at = 40;
        db.upsert_agent_session_node(&node)?;

        let conn = crate::database::lock_conn!(db.conn);
        let values: (String, String, String, i64, i64, i64) = conn.query_row(
            "SELECT title, project_dir, source_path, created_at, last_active_at, last_synced_at
             FROM agent_session_nodes WHERE app_type = 'claude' AND session_id = 'root'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        assert_eq!(
            values,
            (
                "Renamed native title".into(),
                "C:/renamed-project".into(),
                "C:/sessions/root.jsonl".into(),
                10,
                20,
                40
            )
        );
        Ok(())
    }

    #[test]
    fn usage_rollup_keeps_unknown_cost_as_null() -> Result<(), AppError> {
        let db = Database::memory()?;
        let bucket = AgentSessionUsageRollup {
            date: "2026-08-01".into(),
            app_type: "openclaw".into(),
            session_id: "s-1".into(),
            provider_id: "p".into(),
            model: "model".into(),
            request_model: "model".into(),
            pricing_model: "model".into(),
            data_source: "openclaw_session".into(),
            precision: "unavailable".into(),
            time_semantics: "unavailable".into(),
            request_count_semantics: "unavailable".into(),
            request_count: None,
            input_tokens: Some(0),
            output_tokens: Some(0),
            cache_read_tokens: Some(0),
            cache_creation_tokens: Some(0),
            total_cost_usd: None,
            first_event_at: None,
            last_event_at: None,
        };
        db.upsert_agent_session_usage_rollup(&bucket)?;
        let conn = crate::database::lock_conn!(db.conn);
        let cost: Option<String> = conn.query_row(
            "SELECT total_cost_usd FROM agent_session_usage_rollups
             WHERE app_type = 'openclaw' AND session_id = 's-1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(cost, None);
        Ok(())
    }

    fn rollup_fact_fixture(database_identity: &str, task: &str) -> AgentSessionUsageRollupFact {
        AgentSessionUsageRollupFact {
            date: "2026-08-01".into(),
            app_type: "hermes".into(),
            session_id: "session-hermes".into(),
            provider_id: "provider-hermes".into(),
            model: "model-hermes".into(),
            request_model: "model-hermes".into(),
            pricing_model: "model-hermes".into(),
            data_source: "hermes_session_model_usage".into(),
            precision: "sync_window".into(),
            time_semantics: "source_event_time".into(),
            request_count_semantics: "agent_call_delta".into(),
            input_token_semantics: 1,
            source_identity: "hermes:fixture:v1".into(),
            profile_id: "profile-hermes".into(),
            database_identity: database_identity.into(),
            base_url_digest: "sha256:hermes-base".into(),
            billing_mode: "actual".into(),
            task: task.into(),
            source_version: "1".into(),
            sync_window_start: 100,
            sync_window_end: 200,
            request_count: Some(2),
            api_call_count: Some(2),
            input_tokens: Some(10),
            output_tokens: Some(20),
            cache_read_tokens: Some(3),
            // Hermes exposes cache-write separately; cache creation is not
            // source-proven and must remain NULL.
            cache_creation_tokens: None,
            cache_write_tokens: Some(6),
            reasoning_tokens: Some(4),
            total_cost_usd: None,
            cost_status: Some("unknown".into()),
            cost_source: Some("hermes".into()),
            cost_delta_kind: Some("normal".into()),
            correction_state: None,
            first_event_at: Some(100),
            last_event_at: Some(200),
        }
    }

    #[test]
    fn full_rollup_fact_identity_isolated_and_replacement_preserves_nullable_values(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let first = rollup_fact_fixture("db-a", "task-a");
        let second = rollup_fact_fixture("db-b", "task-a");
        db.upsert_agent_session_usage_rollup_fact(&first)?;
        db.upsert_agent_session_usage_rollup_fact(&second)?;

        let conn = crate::database::lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE app_type = 'hermes' AND session_id = 'session-hermes'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2, "database identity must isolate durable facts");
        let nullable: (Option<i64>, Option<i64>, Option<String>) = conn.query_row(
            "SELECT cache_creation_tokens, cache_write_tokens, total_cost_usd
             FROM agent_session_usage_rollups WHERE database_identity = 'db-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(nullable, (None, Some(6), None));
        let hermes_cache_creation: Option<i64> = conn.query_row(
            "SELECT cache_creation_tokens FROM agent_session_usage_rollups
             WHERE database_identity = 'db-b'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(hermes_cache_creation, None);
        drop(conn);

        let mut replacement = first.clone();
        replacement.input_tokens = Some(99);
        replacement.cache_creation_tokens = None;
        replacement.total_cost_usd = Some("0".into());
        db.upsert_agent_session_usage_rollup_fact(&replacement)?;
        let snapshot = snapshot_fixture("task-a", "db-a", Some("0"));
        db.upsert_agent_session_usage_hermes_delta(&replacement, &snapshot)?;
        let conn = crate::database::lock_conn!(db.conn);
        let replaced: (i64, Option<i64>, Option<String>) = conn.query_row(
            "SELECT input_tokens, cache_creation_tokens, total_cost_usd
             FROM agent_session_usage_rollups WHERE database_identity = 'db-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(
            replaced,
            (99, None, Some("0".into())),
            "Hermes cache creation remains unknown through replacement and snapshot upsert"
        );
        let snapshot_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_snapshots
             WHERE source_identity = 'hermes:fixture:v1' AND database_identity = 'db-a'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(snapshot_count, 1);
        drop(conn);

        // A source-proven zero remains distinguishable from Hermes' unknown
        // cache-creation component through the generic compatibility bridge.
        let proven_zero = AgentSessionUsageRollup {
            date: "2026-08-01".into(),
            app_type: "claude".into(),
            session_id: "claude-session".into(),
            provider_id: "provider-claude".into(),
            model: "claude-model".into(),
            request_model: "claude-model".into(),
            pricing_model: "claude-model".into(),
            data_source: "session_log".into(),
            precision: "request_exact".into(),
            time_semantics: "event_time".into(),
            request_count_semantics: "assistant_message".into(),
            request_count: Some(1),
            input_tokens: Some(1),
            output_tokens: Some(1),
            cache_read_tokens: Some(0),
            cache_creation_tokens: Some(0),
            total_cost_usd: None,
            first_event_at: Some(100),
            last_event_at: Some(100),
        };
        db.upsert_agent_session_usage_rollup(&proven_zero)?;
        let conn = crate::database::lock_conn!(db.conn);
        let proven_cache_creation: Option<i64> = conn.query_row(
            "SELECT cache_creation_tokens FROM agent_session_usage_rollups
             WHERE app_type = 'claude' AND session_id = 'claude-session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(proven_cache_creation, Some(0));
        Ok(())
    }

    fn snapshot_fixture(
        task: &str,
        database_identity: &str,
        actual_cost_usd: Option<&str>,
    ) -> AgentSessionUsageSnapshot {
        AgentSessionUsageSnapshot {
            app_type: "hermes".into(),
            source_identity: "hermes:fixture:v1".into(),
            profile_id: "profile-a".into(),
            database_identity: database_identity.into(),
            session_id: "session-a".into(),
            model: "model-a".into(),
            provider_id: "provider-a".into(),
            base_url_digest: "sha256:base-a".into(),
            billing_mode: "actual".into(),
            task: task.into(),
            data_source: "hermes_session_model_usage".into(),
            source_version: "1".into(),
            api_call_count: 3,
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 4,
            cache_write_tokens: 5,
            reasoning_tokens: 6,
            first_seen: Some(100),
            last_seen: Some(200),
            last_synced_at: 300,
            estimated_cost_usd: Some("0.25".into()),
            actual_cost_usd: actual_cost_usd.map(str::to_owned),
            cost_status: Some("actual".into()),
            cost_source: Some("hermes".into()),
            correction_state: None,
        }
    }

    #[test]
    fn snapshot_identity_isolated_and_upsert_replaces_without_accumulating() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let first = snapshot_fixture("task-a", "db-a", None);
        let mut second = snapshot_fixture("task-a", "db-b", Some("0"));
        let mut variant = first.clone();
        variant.profile_id = "profile-b".into();
        variant.model = "model-b".into();
        variant.base_url_digest = "sha256:base-b".into();
        variant.task = "task-b".into();
        db.upsert_agent_session_usage_snapshot(&first)?;
        db.upsert_agent_session_usage_snapshot(&second)?;
        db.upsert_agent_session_usage_snapshot(&variant)?;

        let first_key = first.key();
        let second_key = second.key();
        assert_eq!(
            db.get_agent_session_usage_snapshot(&first_key)?,
            Some(first.clone())
        );
        assert_eq!(
            db.get_agent_session_usage_snapshot(&second_key)?,
            Some(second.clone())
        );
        assert_eq!(
            db.get_agent_session_usage_snapshot(&variant.key())?,
            Some(variant.clone())
        );

        second.api_call_count = 9;
        second.input_tokens = 99;
        db.upsert_agent_session_usage_snapshot(&second)?;
        let replaced = db
            .get_agent_session_usage_snapshot(&second_key)?
            .expect("snapshot remains after replacement");
        assert_eq!(replaced.api_call_count, 9);
        assert_eq!(replaced.input_tokens, 99);
        assert_eq!(replaced.actual_cost_usd, Some("0".into()));

        let first_after = db
            .get_agent_session_usage_snapshot(&first_key)?
            .expect("independent database identity is preserved");
        assert_eq!(first_after.api_call_count, 3);
        assert_eq!(first_after.actual_cost_usd, None);

        assert!(db.delete_agent_session_usage_snapshot(&second_key)?);
        assert!(!db.delete_agent_session_usage_snapshot(&second_key)?);
        assert_eq!(db.get_agent_session_usage_snapshot(&second_key)?, None);
        Ok(())
    }

    #[test]
    fn snapshot_on_conn_supports_transactional_get_upsert_delete() -> Result<(), AppError> {
        let db = Database::memory()?;
        let snapshot = snapshot_fixture("task-transaction", "db-c", Some("0"));
        let key = snapshot.key();
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute("BEGIN", [])?;
        Database::upsert_agent_session_usage_snapshot_on_conn(&conn, &snapshot)?;
        assert_eq!(
            Database::get_agent_session_usage_snapshot_on_conn(&conn, &key)?,
            Some(snapshot.clone())
        );
        assert!(Database::delete_agent_session_usage_snapshot_on_conn(
            &conn, &key
        )?);
        conn.execute("COMMIT", [])?;
        assert_eq!(
            Database::get_agent_session_usage_snapshot_on_conn(&conn, &key)?,
            None
        );
        Ok(())
    }

    #[test]
    fn canonical_coverage_marker_is_idempotent_and_resettable() -> Result<(), AppError> {
        let db = Database::memory()?;
        let marker = AgentSessionCanonicalCoverageMarker {
            app_type: "claude".into(),
            data_source: "session_log".into(),
            request_id: "session:request-1".into(),
            canonical_session_id: Some("session-1".into()),
            marked_at: 100,
        };
        db.upsert_agent_session_canonical_coverage(&marker)?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            assert!(Database::has_agent_session_canonical_coverage_on_conn(
                &conn,
                "claude",
                "session_log",
                "session:request-1"
            )?);
        }
        let mut replacement = marker.clone();
        replacement.marked_at = 200;
        db.upsert_agent_session_canonical_coverage(&replacement)?;
        let marked_at: i64 = {
            let conn = crate::database::lock_conn!(db.conn);
            conn.query_row(
                "SELECT marked_at FROM agent_session_canonical_coverage
                 WHERE app_type = 'claude' AND data_source = 'session_log'
                   AND request_id = 'session:request-1'",
                [],
                |row| row.get(0),
            )?
        };
        assert_eq!(marked_at, 200);
        assert!(db.delete_agent_session_canonical_coverage(&replacement)?);
        assert!(!db.delete_agent_session_canonical_coverage(&replacement)?);
        Ok(())
    }
}
