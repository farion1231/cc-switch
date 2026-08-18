//! Durable Agent session-node and session-usage rollup primitives.
//!
//! This module deliberately exposes only node and bucket writes.  A task total
//! is derived by the query/service layer and is never stored as a third,
//! independently mutable measure.

#[cfg(test)]
use crate::database::lock_conn;
use crate::database::Database;
use crate::error::AppError;
use crate::services::agent_session_usage::{
    NormalizedSessionNode, NormalizedUsageRollupFact, NormalizedUsageSnapshot,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::str::FromStr;

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

fn optional_trimmed_text(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim)
}

impl Database {
    /// Insert or update a normalized session node.  Optional metadata is
    /// preserved when a later sync has no value for that field.
    #[cfg(test)]
    pub(crate) fn upsert_agent_session_node(
        &self,
        node: &NormalizedSessionNode,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let app_type = crate::app_config::AppType::from_str(&node.app_type)?;
        Self::upsert_agent_session_node_on_conn(&conn, node, app_type.as_str())
    }

    #[cfg(test)]
    pub(crate) fn upsert_agent_session_node_on_conn(
        conn: &Connection,
        node: &NormalizedSessionNode,
        stored_app_type: &str,
    ) -> Result<(), AppError> {
        Self::upsert_agent_session_node_on_conn_into(
            conn,
            node,
            "agent_session_nodes",
            stored_app_type,
        )
    }

    /// Internal generation-table variant used by canonical replay.  The table
    /// name comes from the closed [`UsagePublishTarget`](crate::services::session_usage_pipeline::UsagePublishTarget)
    /// set, never from source data.
    pub(crate) fn upsert_agent_session_node_on_conn_into(
        conn: &Connection,
        node: &NormalizedSessionNode,
        table: &str,
        stored_app_type: &str,
    ) -> Result<(), AppError> {
        node.validate_for_persistence()?;
        conn.execute(
            &format!(
                "INSERT INTO {table} (
                app_type, session_id, parent_session_id, root_session_id,
                node_kind, relation_confidence, title, project_dir, source_path,
                created_at, last_active_at, last_synced_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(app_type, session_id) DO UPDATE SET
                parent_session_id = excluded.parent_session_id,
                root_session_id = excluded.root_session_id,
                node_kind = excluded.node_kind,
                relation_confidence = excluded.relation_confidence,
                title = COALESCE(excluded.title, {table}.title),
                project_dir = COALESCE(excluded.project_dir, {table}.project_dir),
                source_path = COALESCE(excluded.source_path, {table}.source_path),
                created_at = COALESCE(excluded.created_at, {table}.created_at),
                last_active_at = COALESCE(excluded.last_active_at, {table}.last_active_at),
                last_synced_at = excluded.last_synced_at"
            ),
            params![
                stored_app_type,
                node.session_id.trim(),
                optional_trimmed_text(&node.parent_session_id),
                node.root_session_id.trim(),
                node.node_kind.as_str(),
                node.relation_confidence.as_str(),
                optional_trimmed_text(&node.title),
                optional_trimmed_text(&node.project_dir),
                optional_trimmed_text(&node.source_path),
                node.created_at,
                node.last_active_at,
                node.last_synced_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("写入 Agent 会话节点失败: {e}")))?;
        Ok(())
    }

    /// Replace a full-fidelity source usage fact.  The complete real
    /// dimension/window tuple is the conflict key; this operation never adds
    /// to an existing row and never stores a task total.
    #[cfg(test)]
    pub(crate) fn upsert_agent_session_usage_rollup_fact(
        &self,
        fact: &NormalizedUsageRollupFact,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let app_type = crate::app_config::AppType::from_str(&fact.app_type)?;
        Self::upsert_agent_session_usage_rollup_fact_on_conn_into(
            &conn,
            fact,
            "agent_session_usage_rollups",
            app_type.as_str(),
        )
    }

    /// Same upsert targeted at an internal generation table used by Codex
    /// shadow replay.  The table name is selected only by trusted callers.
    pub(crate) fn upsert_agent_session_usage_rollup_fact_on_conn_into(
        conn: &Connection,
        fact: &NormalizedUsageRollupFact,
        table: &str,
        stored_app_type: &str,
    ) -> Result<(), AppError> {
        fact.validate_for_persistence()?;
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
                fact.request_count,
                fact.api_call_count,
                fact.input_tokens,
                fact.output_tokens,
                fact.cache_read_tokens,
                fact.cache_creation_tokens,
                fact.cache_write_tokens,
                fact.reasoning_tokens,
                optional_trimmed_text(&fact.total_cost_usd),
                optional_trimmed_text(&fact.cost_status),
                optional_trimmed_text(&fact.cost_source),
                optional_trimmed_text(&fact.cost_delta_kind),
                optional_trimmed_text(&fact.correction_state),
                fact.first_event_at,
                fact.last_event_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("写入完整 Agent 会话用量桶失败: {e}")))?;
        Ok(())
    }

    pub(crate) fn get_agent_session_usage_snapshot_on_conn(
        conn: &Connection,
        key: &AgentSessionUsageSnapshotKey,
    ) -> Result<Option<NormalizedUsageSnapshot>, AppError> {
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
                    Ok(NormalizedUsageSnapshot {
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

    pub(crate) fn upsert_agent_session_usage_snapshot_on_conn(
        conn: &Connection,
        snapshot: &NormalizedUsageSnapshot,
    ) -> Result<(), AppError> {
        snapshot.validate_for_persistence()?;
        let app_type = crate::app_config::AppType::from_str(&snapshot.app_type)?;
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
                app_type.as_str(),
                snapshot.source_identity.trim(),
                snapshot.profile_id.trim(),
                snapshot.database_identity.trim(),
                snapshot.session_id.trim(),
                snapshot.model.trim(),
                snapshot.provider_id.trim(),
                snapshot.base_url_digest.trim(),
                snapshot.billing_mode.trim(),
                snapshot.task.trim(),
                snapshot.data_source.trim(),
                snapshot.source_version.trim(),
                snapshot.api_call_count,
                snapshot.input_tokens,
                snapshot.output_tokens,
                snapshot.cache_read_tokens,
                snapshot.cache_write_tokens,
                snapshot.reasoning_tokens,
                snapshot.first_seen,
                snapshot.last_seen,
                snapshot.last_synced_at,
                optional_trimmed_text(&snapshot.estimated_cost_usd),
                optional_trimmed_text(&snapshot.actual_cost_usd),
                optional_trimmed_text(&snapshot.cost_status),
                optional_trimmed_text(&snapshot.cost_source),
                optional_trimmed_text(&snapshot.correction_state),
            ],
        )
        .map_err(|e| AppError::Database(format!("写入来源累计快照失败: {e}")))?;
        Ok(())
    }

    /// Internal generation-table variant used by Codex canonical replay.
    /// Callers select only a trusted table name from a closed enum.
    pub(crate) fn upsert_agent_session_canonical_coverage_on_conn_into(
        conn: &Connection,
        marker: &AgentSessionCanonicalCoverageMarker,
        table: &str,
    ) -> Result<(), AppError> {
        conn.execute(
            &format!(
                "INSERT INTO {table} (
                app_type, data_source, request_id, canonical_session_id, marked_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(app_type, data_source, request_id) DO UPDATE SET
                canonical_session_id = excluded.canonical_session_id,
                marked_at = excluded.marked_at"
            ),
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
}
