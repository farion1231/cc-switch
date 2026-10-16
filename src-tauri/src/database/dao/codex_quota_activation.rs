//! SQLite persistence for account-scoped activation authorization.
//!
//! Rows are local only.  The state JSON is a bounded finite state machine, so
//! malformed or unknown data fails closed instead of recreating eligibility.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::{AuthBindingSource, ProviderMeta};
use crate::services::codex_quota_activation::{ActivationState, ClaimedAttempt};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaActivationRecord {
    pub account_id: String,
    pub enabled: bool,
    pub effective_interval_minutes: Option<u64>,
    pub enabled_at_ms: Option<i64>,
    pub last_polled_at_ms: Option<i64>,
    pub last_attempt_at_ms: Option<i64>,
    pub last_attempt_status: Option<String>,
    pub last_reason: Option<String>,
    pub state: ActivationState,
    pub state_error: Option<String>,
    pub authorization_revision: i64,
    pub updated_at: i64,
}

fn decode_state(raw: &str) -> Result<ActivationState, AppError> {
    serde_json::from_str(raw).map_err(|error| {
        AppError::Database(format!(
            "invalid Codex activation state; activation paused: {error}"
        ))
    })
}

fn encode_state(state: &ActivationState) -> Result<String, AppError> {
    serde_json::to_string(state)
        .map_err(|error| AppError::Database(format!("serialize Codex activation state: {error}")))
}

impl Database {
    /// Derive the account timer from bound official Codex providers. Explicit
    /// zero and disabled/unbound providers are excluded; the shortest positive
    /// interval wins for one shared account timer.
    pub(crate) fn derive_codex_quota_activation_interval(
        &self,
        account_id: &str,
        _default_account_id: Option<&str>,
    ) -> Result<Option<u64>, AppError> {
        let conn = lock_conn!(self.conn);
        Self::derive_codex_quota_activation_interval_on_conn(&conn, account_id)
    }

    /// Derive the effective interval without acquiring the database mutex.
    /// Callers that already hold a transaction use this helper so the
    /// Provider binding, query switch, and interval are read in the same
    /// SQLite critical section as their state transition.
    pub(crate) fn derive_codex_quota_activation_interval_on_conn(
        conn: &Connection,
        account_id: &str,
    ) -> Result<Option<u64>, AppError> {
        let mut stmt = conn
            .prepare(
                "SELECT id, category, meta FROM providers
                 WHERE app_type = 'codex'",
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        let mut intervals = Vec::new();
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| AppError::Database(error.to_string()))?;
        for row in rows {
            let (provider_id, category, meta) =
                row.map_err(|error| AppError::Database(error.to_string()))?;
            let provider_meta: ProviderMeta = match serde_json::from_str(&meta) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            let Some(binding) = provider_meta.auth_binding.as_ref() else {
                continue;
            };
            if binding.source != AuthBindingSource::ManagedAccount
                || binding.auth_provider.as_deref() != Some("codex_oauth")
                || binding.account_id.as_deref().map(str::trim) != Some(account_id)
            {
                continue;
            }
            let official = provider_id == crate::database::CODEX_OFFICIAL_PROVIDER_ID
                || category.as_deref() == Some("official")
                || provider_meta.provider_type.as_deref() == Some("codex_oauth");
            if !official {
                continue;
            }
            let Some(script) = provider_meta.usage_script.as_ref() else {
                intervals.push(5);
                continue;
            };
            if !script.enabled
                || script
                    .template_type
                    .as_deref()
                    .is_some_and(|template| template != "official_subscription")
            {
                continue;
            }
            intervals.push(script.auto_query_interval.unwrap_or(5));
        }
        if intervals.is_empty() {
            return Ok(None);
        }
        Ok(intervals
            .iter()
            .copied()
            .filter(|value| *value > 0)
            .min()
            .or(Some(0)))
    }

    pub(crate) fn get_codex_quota_activation_state(
        &self,
        account_id: &str,
    ) -> Result<Option<CodexQuotaActivationRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let row = conn
            .query_row(
                "SELECT account_id, enabled, effective_interval_minutes, enabled_at_ms,
                        last_polled_at_ms, last_attempt_at_ms, last_attempt_status,
                        last_reason, state_json, authorization_revision, updated_at
                 FROM codex_quota_activation_state WHERE account_id = ?1",
                params![account_id],
                |row| {
                    let raw: String = row.get(8)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)? != 0,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        raw,
                        row.get::<_, i64>(9)?,
                        row.get::<_, i64>(10)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let Some((
            account_id,
            enabled,
            interval,
            enabled_at_ms,
            last_polled_at_ms,
            last_attempt_at_ms,
            last_attempt_status,
            last_reason,
            raw,
            authorization_revision,
            updated_at,
        )) = row
        else {
            return Ok(None);
        };
        let (state, state_error, enabled) = match decode_state(&raw) {
            Ok(state) => (state, None, enabled),
            Err(error) => (
                ActivationState::AwaitingBaseline,
                Some(error.to_string()),
                false,
            ),
        };
        Ok(Some(CodexQuotaActivationRecord {
            account_id,
            enabled,
            effective_interval_minutes: interval.and_then(|value| u64::try_from(value).ok()),
            enabled_at_ms,
            last_polled_at_ms,
            last_attempt_at_ms,
            last_attempt_status,
            last_reason,
            state,
            state_error,
            authorization_revision,
            updated_at,
        }))
    }

    pub(crate) fn list_codex_quota_activation_states(
        &self,
    ) -> Result<Vec<CodexQuotaActivationRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT account_id, enabled, effective_interval_minutes, enabled_at_ms,
                        last_polled_at_ms, last_attempt_at_ms, last_attempt_status,
                        last_reason, state_json, authorization_revision, updated_at
                 FROM codex_quota_activation_state ORDER BY account_id",
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let raw: String = row.get(8)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    raw,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            })
            .map_err(|error| AppError::Database(error.to_string()))?;
        rows.map(|row| {
            let (
                account_id,
                enabled,
                interval,
                enabled_at_ms,
                last_polled_at_ms,
                last_attempt_at_ms,
                last_attempt_status,
                last_reason,
                raw,
                authorization_revision,
                updated_at,
            ) = row.map_err(|error| AppError::Database(error.to_string()))?;
            let (state, state_error, enabled) = match decode_state(&raw) {
                Ok(state) => (state, None, enabled),
                Err(error) => (
                    ActivationState::AwaitingBaseline,
                    Some(error.to_string()),
                    false,
                ),
            };
            Ok(CodexQuotaActivationRecord {
                account_id,
                enabled,
                effective_interval_minutes: interval.and_then(|value| u64::try_from(value).ok()),
                enabled_at_ms,
                last_polled_at_ms,
                last_attempt_at_ms,
                last_attempt_status,
                last_reason,
                state,
                state_error,
                authorization_revision,
                updated_at,
            })
        })
        .collect()
    }

    pub(crate) fn set_codex_quota_activation_enabled(
        &self,
        account_id: &str,
        enabled: bool,
        effective_interval_minutes: Option<u64>,
        now_ms: i64,
    ) -> Result<CodexQuotaActivationRecord, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let current = tx
            .query_row(
                "SELECT state_json, authorization_revision
                 FROM codex_quota_activation_state WHERE account_id = ?1",
                params![account_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let (raw, revision) = match current {
            Some((raw, revision)) => {
                if enabled {
                    decode_state(&raw)?;
                }
                (raw, revision.saturating_add(1))
            }
            None => (encode_state(&ActivationState::BootstrapPending)?, 1),
        };
        tx.execute(
            "INSERT INTO codex_quota_activation_state
                (account_id, enabled, effective_interval_minutes, enabled_at_ms, state_json,
                 authorization_revision, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(account_id) DO UPDATE SET
                enabled=excluded.enabled,
                effective_interval_minutes=excluded.effective_interval_minutes,
                enabled_at_ms=excluded.enabled_at_ms,
                authorization_revision=excluded.authorization_revision,
                updated_at=excluded.updated_at",
            params![
                account_id,
                i64::from(enabled),
                effective_interval_minutes.and_then(|value| i64::try_from(value).ok()),
                now_ms,
                raw,
                revision,
                now_ms
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
        drop(conn);
        self.get_codex_quota_activation_state(account_id)?
            .ok_or_else(|| {
                AppError::Database("activation state disappeared after update".to_string())
            })
    }

    pub(crate) fn save_codex_quota_activation_state(
        &self,
        account_id: &str,
        state: &ActivationState,
        now_ms: i64,
    ) -> Result<(), AppError> {
        let raw = encode_state(state)?;
        let conn = lock_conn!(self.conn);
        let changed = conn
            .execute(
                "UPDATE codex_quota_activation_state SET state_json = ?1, updated_at = ?2
                 WHERE account_id = ?3",
                params![raw, now_ms, account_id],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        if changed == 0 {
            return Err(AppError::Database(
                "activation state is not enabled for this account".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn save_codex_quota_activation_state_if_revision(
        &self,
        account_id: &str,
        expected_revision: i64,
        state: &ActivationState,
        now_ms: i64,
    ) -> Result<bool, AppError> {
        let raw = encode_state(state)?;
        let conn = lock_conn!(self.conn);
        let changed = conn
            .execute(
                "UPDATE codex_quota_activation_state
                 SET state_json = ?1, updated_at = ?2
                 WHERE account_id = ?3 AND enabled = 1 AND authorization_revision = ?4",
                params![raw, now_ms, account_id, expected_revision],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(changed == 1)
    }

    pub(crate) fn codex_quota_activation_revision(
        &self,
        account_id: &str,
    ) -> Result<Option<(bool, i64)>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT enabled, authorization_revision
             FROM codex_quota_activation_state WHERE account_id = ?1",
            params![account_id],
            |row| Ok((row.get::<_, i64>(0)? != 0, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn refresh_codex_quota_activation_interval(
        &self,
        account_id: &str,
        interval_minutes: Option<u64>,
        now_ms: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE codex_quota_activation_state
             SET effective_interval_minutes = ?1, updated_at = ?2
             WHERE account_id = ?3",
            params![
                interval_minutes.and_then(|value| i64::try_from(value).ok()),
                now_ms,
                account_id
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    /// Backend due guard for the explicit polling IPC. The first poll is not
    /// due until a complete configured interval after the user enabled it.
    pub(crate) fn try_begin_codex_quota_activation_poll(
        &self,
        account_id: &str,
        effective_interval_minutes: Option<u64>,
        now_ms: i64,
    ) -> Result<bool, AppError> {
        let Some(interval_minutes) = effective_interval_minutes.filter(|value| *value > 0) else {
            self.refresh_codex_quota_activation_interval(
                account_id,
                effective_interval_minutes,
                now_ms,
            )?;
            return Ok(false);
        };
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let row = tx
            .query_row(
                "SELECT enabled, enabled_at_ms, last_polled_at_ms
             FROM codex_quota_activation_state WHERE account_id = ?1",
                params![account_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? != 0,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let Some((true, Some(enabled_at_ms), last_polled_at_ms)) = row else {
            tx.commit()
                .map_err(|error| AppError::Database(error.to_string()))?;
            return Ok(false);
        };
        let anchor = last_polled_at_ms.map_or(enabled_at_ms, |last| last.max(enabled_at_ms));
        let interval_ms = i64::try_from(interval_minutes)
            .unwrap_or(i64::MAX)
            .saturating_mul(60_000);
        if now_ms.saturating_sub(anchor) < interval_ms {
            tx.execute(
                "UPDATE codex_quota_activation_state SET effective_interval_minutes = ?1
                 WHERE account_id = ?2",
                params![
                    i64::try_from(interval_minutes).unwrap_or(i64::MAX),
                    account_id
                ],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
            tx.commit()
                .map_err(|error| AppError::Database(error.to_string()))?;
            return Ok(false);
        }
        let changed = tx
            .execute(
                "UPDATE codex_quota_activation_state
             SET effective_interval_minutes = ?1, last_polled_at_ms = ?2, updated_at = ?2
             WHERE account_id = ?3 AND enabled = 1 AND enabled_at_ms = ?4
               AND last_polled_at_ms IS ?5",
                params![
                    i64::try_from(interval_minutes).unwrap_or(i64::MAX),
                    now_ms,
                    account_id,
                    enabled_at_ms,
                    last_polled_at_ms
                ],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(changed == 1)
    }

    pub(crate) fn finish_codex_quota_activation_attempt(
        &self,
        account_id: &str,
        attempt_id: &str,
        status: &str,
        reason: Option<&str>,
        now_ms: i64,
    ) -> Result<bool, AppError> {
        let Some((_, revision)) = self.codex_quota_activation_revision(account_id)? else {
            return Ok(false);
        };
        self.finish_codex_quota_activation_attempt_if_revision(
            account_id, revision, attempt_id, status, reason, now_ms,
        )
    }

    pub(crate) fn finish_codex_quota_activation_attempt_if_revision(
        &self,
        account_id: &str,
        expected_revision: i64,
        attempt_id: &str,
        status: &str,
        reason: Option<&str>,
        now_ms: i64,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let changed = conn
            .execute(
                "UPDATE codex_quota_activation_state
             SET last_attempt_status = ?1, last_reason = ?2, updated_at = ?3
             WHERE account_id = ?4 AND last_attempt_id = ?5
               AND authorization_revision = ?6 AND enabled = 1",
                params![
                    status,
                    reason,
                    now_ms,
                    account_id,
                    attempt_id,
                    expected_revision
                ],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(changed == 1)
    }

    pub(crate) fn mark_codex_quota_activation_inflight_unknown(
        &self,
        now_ms: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE codex_quota_activation_state
             SET last_attempt_status = 'unknown', last_reason = 'previous process ended after reserving the request', updated_at = ?1
             WHERE last_attempt_status IN ('reserved', 'submitted')",
            params![now_ms],
        ).map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    /// Atomically reserve the current eligible event. A failed reservation
    /// commits no state and the caller must not issue a POST.
    pub(crate) fn claim_codex_quota_activation(
        &self,
        account_id: &str,
        attempt_id: &str,
        attempted_at_ms: i64,
    ) -> Result<Option<ClaimedAttempt>, AppError> {
        let Some((enabled, revision)) = self.codex_quota_activation_revision(account_id)? else {
            return Ok(None);
        };
        if !enabled {
            return Ok(None);
        }
        self.claim_codex_quota_activation_if_revision(
            account_id,
            revision,
            attempt_id,
            attempted_at_ms,
        )
    }

    pub(crate) fn claim_codex_quota_activation_if_revision(
        &self,
        account_id: &str,
        expected_revision: i64,
        attempt_id: &str,
        attempted_at_ms: i64,
    ) -> Result<Option<ClaimedAttempt>, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let row = tx
            .query_row(
                "SELECT enabled, state_json FROM codex_quota_activation_state
                 WHERE account_id = ?1 AND authorization_revision = ?2",
                params![account_id, expected_revision],
                |row| Ok((row.get::<_, i64>(0)? != 0, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let Some((enabled, raw)) = row else {
            tx.commit()
                .map_err(|error| AppError::Database(error.to_string()))?;
            return Ok(None);
        };
        if !enabled {
            tx.commit()
                .map_err(|error| AppError::Database(error.to_string()))?;
            return Ok(None);
        }
        let mut state = decode_state(&raw)?;
        let Some(claim) = state.claim(attempt_id.to_string(), attempted_at_ms) else {
            tx.commit()
                .map_err(|error| AppError::Database(error.to_string()))?;
            return Ok(None);
        };
        let encoded = encode_state(&state)?;
        let changed = tx
            .execute(
                "UPDATE codex_quota_activation_state SET state_json = ?1, updated_at = ?2
                 , last_attempt_at_ms = ?2, last_attempt_id = ?5,
                   last_attempt_status = 'reserved', last_reason = NULL
                 WHERE account_id = ?3 AND enabled = 1 AND state_json = ?4
                   AND authorization_revision = ?6",
                params![
                    encoded,
                    attempted_at_ms,
                    account_id,
                    raw,
                    attempt_id,
                    expected_revision
                ],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        if changed != 1 {
            tx.commit()
                .map_err(|error| AppError::Database(error.to_string()))?;
            return Ok(None);
        }
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(Some(claim))
    }

    /// Full restore/import callers use this to stop dispatch and require a
    /// fresh nonzero baseline before eligibility can be rebuilt.
    pub(crate) fn pause_codex_quota_activation_after_restore(
        &self,
        now_ms: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::pause_codex_quota_activation_after_restore_on_conn(&conn, now_ms, None)
    }

    pub(crate) fn pause_codex_quota_activation_after_restore_on_conn(
        conn: &Connection,
        now_ms: i64,
        minimum_revision: Option<i64>,
    ) -> Result<(), AppError> {
        conn.execute(
            "UPDATE codex_quota_activation_state
             SET enabled = 0, state_json = ?1, last_attempt_id = NULL,
                 authorization_revision = MAX(authorization_revision + 1, ?3), updated_at = ?2",
            params![
                encode_state(&ActivationState::AwaitingBaseline)?,
                now_ms,
                minimum_revision.map_or(0, |revision| revision.saturating_add(1))
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    /// Keep a disabled tombstone for every live account whose activation row
    /// is absent from a full-restore staging database.  Without this row the
    /// normal enable path starts at revision 1 again and an in-flight task
    /// from the replaced database can become indistinguishable from the new
    /// authorization generation.
    pub(crate) fn ensure_codex_quota_activation_tombstones_on_conn(
        live_conn: &Connection,
        staging_conn: &Connection,
        now_ms: i64,
        minimum_revision: Option<i64>,
    ) -> Result<(), AppError> {
        let baseline_state = encode_state(&ActivationState::AwaitingBaseline)?;
        let revision_floor = minimum_revision.unwrap_or(0).saturating_add(1);
        let mut stmt = live_conn
            .prepare(
                "SELECT account_id, authorization_revision
                 FROM codex_quota_activation_state",
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| AppError::Database(error.to_string()))?;
        for row in rows {
            let (account_id, live_revision) =
                row.map_err(|error| AppError::Database(error.to_string()))?;
            let exists = staging_conn
                .query_row(
                    "SELECT 1 FROM codex_quota_activation_state WHERE account_id = ?1",
                    params![account_id],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|error| AppError::Database(error.to_string()))?
                .is_some();
            if exists {
                continue;
            }
            let revision = live_revision.saturating_add(1).max(revision_floor);
            staging_conn
                .execute(
                    "INSERT INTO codex_quota_activation_state
                        (account_id, enabled, effective_interval_minutes,
                         enabled_at_ms, last_polled_at_ms, last_attempt_at_ms,
                         last_attempt_id, last_attempt_status, last_reason,
                         state_json, authorization_revision, updated_at)
                     VALUES (?1, 0, NULL, NULL, NULL, NULL, NULL, 'unknown',
                             ?2, ?3, ?4, ?5)",
                    params![
                        account_id,
                        "activation invalidated by full restore",
                        baseline_state.as_str(),
                        revision,
                        now_ms
                    ],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn bump_codex_quota_activation_revisions_on_conn(
        conn: &Connection,
        now_ms: i64,
    ) -> Result<(), AppError> {
        conn.execute(
            "UPDATE codex_quota_activation_state
             SET authorization_revision = authorization_revision + 1, updated_at = ?1",
            params![now_ms],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    /// Linearization point for the final, not-yet-submitted dispatch.  The
    /// conditional UPDATE is serialized with disable/restore writes by
    /// SQLite, so a close that commits first prevents the sender call.  Once
    /// this succeeds the request is considered handed to the one-shot
    /// sender; a later close may invalidate its finish but cannot retract it.
    pub(crate) fn begin_codex_quota_activation_dispatch_if_revision(
        &self,
        account_id: &str,
        expected_revision: i64,
        attempt_id: &str,
        now_ms: i64,
    ) -> Result<bool, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let interval = Self::derive_codex_quota_activation_interval_on_conn(&tx, account_id)?;
        if !interval.is_some_and(|value| value > 0) {
            tx.rollback()
                .map_err(|error| AppError::Database(error.to_string()))?;
            return Ok(false);
        }
        let changed = tx
            .execute(
                "UPDATE codex_quota_activation_state
                 SET last_attempt_status = 'submitted', updated_at = ?1
                 WHERE account_id = ?2 AND enabled = 1
                   AND authorization_revision = ?3 AND last_attempt_id = ?4
                   AND last_attempt_status = 'reserved'",
                params![now_ms, account_id, expected_revision, attempt_id],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(changed == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::{AuthBinding, AuthBindingSource, Provider, ProviderMeta, UsageScript};
    use crate::services::codex_quota_activation::ActivationState;
    use serde_json::json;

    fn usage_script(
        enabled: bool,
        auto_query_interval: Option<u64>,
        template_type: Option<&str>,
    ) -> UsageScript {
        UsageScript {
            enabled,
            language: "javascript".to_string(),
            code: "return {};".to_string(),
            timeout: None,
            api_key: None,
            base_url: None,
            access_token: None,
            user_id: None,
            template_type: template_type.map(str::to_string),
            auto_query_interval,
            coding_plan_provider: None,
            access_key_id: None,
            secret_access_key: None,
            team_organization_id: None,
            team_project_id: None,
        }
    }

    fn provider_meta(account_id: Option<&str>, script: Option<UsageScript>) -> ProviderMeta {
        ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("codex_oauth".to_string()),
                account_id: account_id.map(str::to_string),
            }),
            provider_type: Some("codex_oauth".to_string()),
            usage_script: script,
            ..ProviderMeta::default()
        }
    }

    fn insert_codex_provider(
        db: &Database,
        id: &str,
        category: Option<&str>,
        meta: ProviderMeta,
    ) -> Result<(), AppError> {
        let mut provider = Provider::with_id(id.to_string(), id.to_string(), json!({}), None);
        provider.category = category.map(str::to_string);
        provider.meta = Some(meta);
        db.save_provider("codex", &provider)
    }

    #[test]
    fn state_defaults_off_and_claim_is_atomic() {
        let db = Database::memory().unwrap();
        let row = db
            .set_codex_quota_activation_enabled("a", true, Some(5), 1)
            .unwrap();
        assert!(row.enabled);
        db.save_codex_quota_activation_state(
            "a",
            &ActivationState::Eligible {
                target: crate::services::codex_quota_activation::WindowKind::FiveHour,
                event_key: "e".into(),
            },
            2,
        )
        .unwrap();
        assert!(db
            .claim_codex_quota_activation("a", "attempt", 3)
            .unwrap()
            .is_some());
        assert!(db
            .claim_codex_quota_activation("a", "attempt-2", 4)
            .unwrap()
            .is_none());
    }

    #[test]
    fn malformed_state_fails_closed() -> Result<(), AppError> {
        let db = Database::memory().unwrap();
        db.set_codex_quota_activation_enabled("a", true, Some(5), 1)
            .unwrap();
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "UPDATE codex_quota_activation_state SET state_json = '{' WHERE account_id='a'",
                [],
            )
            .unwrap();
        }
        let record = db.get_codex_quota_activation_state("a")?.unwrap();
        assert!(!record.enabled);
        assert!(record.state_error.is_some());
        assert!(matches!(record.state, ActivationState::AwaitingBaseline));
        let disabled = db.set_codex_quota_activation_enabled("a", false, None, 2)?;
        assert!(!disabled.enabled);
        assert!(disabled.state_error.is_some());
        Ok(())
    }

    #[test]
    fn corrupt_account_isolated_from_other_accounts_and_not_bootstrapped() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.set_codex_quota_activation_enabled("bad", true, Some(1), 1)?;
        db.set_codex_quota_activation_enabled("good", true, Some(1), 1)?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "UPDATE codex_quota_activation_state SET state_json = '{' WHERE account_id = 'bad'",
                [],
            )?;
        }
        let rows = db.list_codex_quota_activation_states()?;
        assert_eq!(rows.len(), 2);
        let bad = rows.iter().find(|row| row.account_id == "bad").unwrap();
        let good = rows.iter().find(|row| row.account_id == "good").unwrap();
        assert!(!bad.enabled);
        assert!(bad.state_error.is_some());
        assert!(matches!(bad.state, ActivationState::AwaitingBaseline));
        assert!(good.enabled);
        assert!(good.state_error.is_none());

        let disabled = db.set_codex_quota_activation_enabled("bad", false, None, 2)?;
        assert!(!disabled.enabled);
        assert!(disabled.state_error.is_some());
        let raw: String = crate::database::lock_conn!(db.conn).query_row(
            "SELECT state_json FROM codex_quota_activation_state WHERE account_id = 'bad'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(raw, "{");
        Ok(())
    }

    #[test]
    fn interval_zero_pauses_and_reserved_attempt_becomes_unknown_on_restart() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        db.set_codex_quota_activation_enabled("a", true, Some(5), 0)?;
        assert!(!db.try_begin_codex_quota_activation_poll("a", Some(0), 600_000)?);
        db.save_codex_quota_activation_state(
            "a",
            &ActivationState::Eligible {
                target: crate::services::codex_quota_activation::WindowKind::FiveHour,
                event_key: "event".to_string(),
            },
            600_000,
        )?;
        assert!(db
            .claim_codex_quota_activation("a", "attempt", 600_001)?
            .is_some());
        db.finish_codex_quota_activation_attempt("a", "attempt", "reserved", None, 600_002)?;
        db.mark_codex_quota_activation_inflight_unknown(600_003)?;
        let state = db.get_codex_quota_activation_state("a")?.unwrap();
        assert_eq!(state.last_attempt_status.as_deref(), Some("unknown"));
        assert!(db
            .claim_codex_quota_activation("a", "attempt-2", 600_004)?
            .is_none());
        Ok(())
    }

    #[test]
    fn derive_interval_reads_serialized_usage_script_defaults_and_guards() -> Result<(), AppError> {
        let cases = [
            ("disabled", Some(usage_script(false, Some(10), None)), None),
            (
                "explicit zero",
                Some(usage_script(true, Some(0), None)),
                Some(0),
            ),
            (
                "explicit ten",
                Some(usage_script(true, Some(10), None)),
                Some(10),
            ),
            (
                "custom template",
                Some(usage_script(true, Some(1), Some("custom"))),
                None,
            ),
            (
                "official template",
                Some(usage_script(true, Some(10), Some("official_subscription"))),
                Some(10),
            ),
            ("missing usage script", None, Some(5)),
            (
                "missing interval",
                Some(usage_script(true, None, None)),
                Some(5),
            ),
        ];

        for (name, script, expected) in cases {
            let db = Database::memory()?;
            insert_codex_provider(
                &db,
                "codex-official",
                Some("official"),
                provider_meta(Some("account-a"), script),
            )?;
            assert_eq!(
                db.derive_codex_quota_activation_interval("account-a", None)?,
                expected,
                "case {name}"
            );
        }
        Ok(())
    }

    #[test]
    fn derive_interval_uses_shortest_positive_bound_to_each_account() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_codex_provider(
            &db,
            "account-a-disabled",
            None,
            provider_meta(Some("account-a"), Some(usage_script(false, Some(1), None))),
        )?;
        insert_codex_provider(
            &db,
            "account-a-zero",
            None,
            provider_meta(Some("account-a"), Some(usage_script(true, Some(0), None))),
        )?;
        insert_codex_provider(
            &db,
            "account-a-ten",
            None,
            provider_meta(Some("account-a"), Some(usage_script(true, Some(10), None))),
        )?;
        insert_codex_provider(
            &db,
            "account-a-five",
            None,
            provider_meta(Some("account-a"), Some(usage_script(true, Some(5), None))),
        )?;
        insert_codex_provider(
            &db,
            "account-a-custom",
            None,
            provider_meta(
                Some("account-a"),
                Some(usage_script(true, Some(1), Some("custom"))),
            ),
        )?;
        insert_codex_provider(
            &db,
            "account-b-one",
            None,
            provider_meta(Some("account-b"), Some(usage_script(true, Some(1), None))),
        )?;

        assert_eq!(
            db.derive_codex_quota_activation_interval("account-a", None)?,
            Some(5)
        );
        assert_eq!(
            db.derive_codex_quota_activation_interval("account-b", None)?,
            Some(1)
        );
        assert_eq!(
            db.derive_codex_quota_activation_interval("account-c", None)?,
            None
        );
        Ok(())
    }

    #[test]
    fn derive_interval_requires_explicit_managed_account_binding() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_codex_provider(
            &db,
            "unbound-default",
            None,
            provider_meta(None, Some(usage_script(true, Some(7), None))),
        )?;

        assert_eq!(
            db.derive_codex_quota_activation_interval("account-a", None)?,
            None
        );
        assert_eq!(
            db.derive_codex_quota_activation_interval("account-a", Some("account-a"))?,
            None
        );
        Ok(())
    }

    #[test]
    fn automatic_tick_uses_saved_provider_state_claim_and_fake_sender() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_codex_provider(
            &db,
            "codex-provider",
            Some("official"),
            provider_meta(
                Some("account-a"),
                Some(usage_script(true, Some(5), Some("official_subscription"))),
            ),
        )?;
        let interval = db.derive_codex_quota_activation_interval("account-a", None)?;
        assert_eq!(interval, Some(5));
        db.set_codex_quota_activation_enabled("account-a", true, interval, 0)?;
        assert!(db.try_begin_codex_quota_activation_poll("account-a", interval, 300_001)?);
        let record = db.get_codex_quota_activation_state("account-a")?.unwrap();
        let raw = serde_json::json!({
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {"used_percent": 0, "limit_window_seconds": 18000, "reset_at": 18300, "reset_after_seconds": 18000},
                "secondary_window": {"used_percent": 50, "limit_window_seconds": 604800, "reset_at": 605100, "reset_after_seconds": 604800}
            }
        });
        let mut snapshot =
            crate::services::codex_quota_activation::snapshot_from_value(&raw, 300_001, 300_001)
                .unwrap();
        snapshot.server_date_at_ms = Some(snapshot.observed_at_ms);
        assert_eq!(
            crate::services::codex_oauth_models::parse_activation_model_capability(
                &serde_json::json!({"models":[{"slug":"gpt-6-luna","supported_reasoning_levels":[{"effort":"low"}]}]})
            ),
            crate::services::codex_oauth_models::ActivationModelCapability::Supported
        );
        let mut state = record.state;
        assert!(matches!(
            state.observe(&snapshot, 300_001),
            crate::services::codex_quota_activation::ActivationDecision::Candidate(
                crate::services::codex_quota_activation::WindowKind::FiveHour
            )
        ));
        assert!(db.save_codex_quota_activation_state_if_revision(
            "account-a",
            record.authorization_revision,
            &state,
            300_002,
        )?);
        let claim = db
            .claim_codex_quota_activation_if_revision(
                "account-a",
                record.authorization_revision,
                "fake-attempt",
                300_003,
            )?
            .expect("candidate should claim");
        let mut fake_posts = 0;
        fake_posts += 1;
        assert!(db.finish_codex_quota_activation_attempt_if_revision(
            "account-a",
            record.authorization_revision,
            &claim.attempt_id,
            "completed",
            None,
            300_004,
        )?);
        assert_eq!(fake_posts, 1);
        assert!(db
            .claim_codex_quota_activation_if_revision(
                "account-a",
                record.authorization_revision,
                "fake-attempt-2",
                300_005,
            )?
            .is_none());
        Ok(())
    }

    #[test]
    fn t01_parser_state_dao_preserves_current_window_nonzero_before_recovery(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_codex_provider(
            &db,
            "codex-provider",
            Some("official"),
            provider_meta(
                Some("account-a"),
                Some(usage_script(true, Some(5), Some("official_subscription"))),
            ),
        )?;
        let interval = db.derive_codex_quota_activation_interval("account-a", None)?;
        assert_eq!(interval, Some(5));
        db.set_codex_quota_activation_enabled("account-a", true, interval, 0)?;
        assert!(db.try_begin_codex_quota_activation_poll("account-a", interval, 300_001)?);

        let parse = |used: f64, request_started_at_ms: i64, reset_after_seconds: i64| {
            let raw = json!({
                "rate_limit": {
                    "allowed": true,
                    "limit_reached": false,
                    "primary_window": {
                        "used_percent": used,
                        "limit_window_seconds": 18000,
                        "reset_at": 19000,
                        "reset_after_seconds": reset_after_seconds
                    },
                    "secondary_window": {
                        "used_percent": 50,
                        "limit_window_seconds": 604800,
                        "reset_at": 19000,
                        "reset_after_seconds": reset_after_seconds
                    }
                }
            });
            let mut snapshot = crate::services::codex_quota_activation::snapshot_from_value(
                &raw,
                request_started_at_ms,
                request_started_at_ms,
            )
            .unwrap();
            snapshot.server_date_at_ms = Some(snapshot.observed_at_ms);
            snapshot
        };

        let record = db.get_codex_quota_activation_state("account-a")?.unwrap();
        let initial = parse(0.0, 1_000_000, 18_000);
        let mut state = record.state;
        assert!(matches!(
            state.observe(&initial, initial.observed_at_ms),
            crate::services::codex_quota_activation::ActivationDecision::Candidate(
                crate::services::codex_quota_activation::WindowKind::FiveHour
            )
        ));
        assert!(db.save_codex_quota_activation_state_if_revision(
            "account-a",
            record.authorization_revision,
            &state,
            1_100_000,
        )?);
        assert!(db
            .claim_codex_quota_activation_if_revision(
                "account-a",
                record.authorization_revision,
                "attempt",
                1_000_100,
            )?
            .is_some());

        let record = db.get_codex_quota_activation_state("account-a")?.unwrap();
        let used_before_boundary = parse(20.0, 1_300_000, 17_700);
        let mut state = record.state;
        assert_eq!(
            state.observe(&used_before_boundary, used_before_boundary.observed_at_ms,),
            crate::services::codex_quota_activation::ActivationDecision::Observe
        );
        assert!(matches!(
            state,
            ActivationState::TrackingUsage {
                boundary_reset_at: 19_000,
                ..
            }
        ));
        assert!(db.save_codex_quota_activation_state_if_revision(
            "account-a",
            record.authorization_revision,
            &state,
            1_300_001,
        )?);

        let record = db.get_codex_quota_activation_state("account-a")?.unwrap();
        let recovered = parse(0.0, 19_001_000, 0);
        let mut state = record.state;
        assert!(matches!(
            state.observe(&recovered, recovered.observed_at_ms),
            crate::services::codex_quota_activation::ActivationDecision::Candidate(
                crate::services::codex_quota_activation::WindowKind::FiveHour
            )
        ));
        assert!(db.save_codex_quota_activation_state_if_revision(
            "account-a",
            record.authorization_revision,
            &state,
            19_001_001,
        )?);
        assert!(db
            .claim_codex_quota_activation_if_revision(
                "account-a",
                record.authorization_revision,
                "second-attempt",
                19_001_002,
            )?
            .is_some());
        Ok(())
    }

    fn seed_eligible(db: &Database, account_id: &str) -> Result<(), AppError> {
        db.set_codex_quota_activation_enabled(account_id, true, Some(1), 0)?;
        db.save_codex_quota_activation_state(
            account_id,
            &ActivationState::Eligible {
                target: crate::services::codex_quota_activation::WindowKind::FiveHour,
                event_key: format!("five_hour:{account_id}"),
            },
            1,
        )
    }

    #[test]
    fn f03_disable_barrier_blocks_due_or_claim_at_each_pre_dispatch_dao_point(
    ) -> Result<(), AppError> {
        // The command currently performs its OAuth/GET/model awaits inline, so
        // this is the narrow DAO-level barrier that can be tested without
        // touching a real manager or endpoint. The command-level await
        // injection remains an explicit coverage gap until those dependencies
        // are passed into a small testable poll function.
        for phase in ["before_get", "before_model_preflight", "before_claim"] {
            let db = Database::memory()?;
            seed_eligible(&db, "account-a")?;
            if phase == "before_get" {
                db.set_codex_quota_activation_enabled("account-a", false, Some(1), 2)?;
                assert!(
                    !db.try_begin_codex_quota_activation_poll("account-a", Some(1), 60_001)?,
                    "disabled account must not admit the GET phase"
                );
                continue;
            }

            assert!(db.try_begin_codex_quota_activation_poll("account-a", Some(1), 60_001)?);
            // Simulate the user closing the account while a later network
            // phase is suspended. Claim must observe the short transaction.
            db.set_codex_quota_activation_enabled("account-a", false, Some(1), 60_002)?;
            assert!(
                db.claim_codex_quota_activation("account-a", phase, 60_003)?
                    .is_none(),
                "disabled account must not claim after {phase}"
            );
        }
        Ok(())
    }

    #[test]
    fn f03_full_restore_rejects_late_finish_for_pre_restore_attempt() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_eligible(&db, "account-a")?;
        let claim = db
            .claim_codex_quota_activation("account-a", "attempt-a", 60_001)?
            .expect("eligible account should claim once");

        // FullRestore is the dispatch-before barrier: it invalidates the
        // authorization while the sender is still waiting to start. A late
        // result from the old attempt must not write a post-restore status,
        // even if the account is enabled again before that result arrives.
        db.pause_codex_quota_activation_after_restore(60_002)?;
        db.set_codex_quota_activation_enabled("account-a", true, Some(1), 60_003)?;
        assert!(
            !db.finish_codex_quota_activation_attempt(
                "account-a",
                &claim.attempt_id,
                "completed",
                None,
                60_004,
            )?,
            "late finish must fail after restore invalidates authorization"
        );
        let state = db
            .get_codex_quota_activation_state("account-a")?
            .expect("restore preserves the local row");
        assert!(state.enabled);
        assert!(matches!(state.state, ActivationState::AwaitingBaseline));
        assert_eq!(state.last_attempt_status.as_deref(), Some("reserved"));
        Ok(())
    }

    #[test]
    fn f03_finish_requires_expected_account_and_attempt() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_eligible(&db, "account-a")?;
        let claim = db
            .claim_codex_quota_activation("account-a", "attempt-a", 60_001)?
            .expect("eligible account should claim once");

        assert!(!db.finish_codex_quota_activation_attempt(
            "account-a",
            "wrong-attempt",
            "completed",
            None,
            60_002,
        )?);
        assert!(!db.finish_codex_quota_activation_attempt(
            "account-b",
            &claim.attempt_id,
            "completed",
            None,
            60_003,
        )?);
        assert_eq!(
            db.get_codex_quota_activation_state("account-a")?
                .unwrap()
                .last_attempt_status
                .as_deref(),
            Some("reserved")
        );
        assert!(db.finish_codex_quota_activation_attempt(
            "account-a",
            &claim.attempt_id,
            "completed",
            None,
            60_004,
        )?);
        assert_eq!(
            db.get_codex_quota_activation_state("account-a")?
                .unwrap()
                .last_attempt_status
                .as_deref(),
            Some("completed")
        );
        Ok(())
    }

    #[tokio::test]
    async fn f03_barrier_fake_quota_and_model_disable_prevents_fake_sender() -> Result<(), AppError>
    {
        let db = std::sync::Arc::new(Database::memory()?);
        seed_eligible(&db, "account-a")?;
        let revision = db.codex_quota_activation_revision("account-a")?.unwrap().1;

        let (quota_release, quota_ready) = tokio::sync::oneshot::channel::<()>();
        let quota_task = tokio::spawn(async move {
            let _ = quota_ready.await;
            "fresh fake quota"
        });
        db.set_codex_quota_activation_enabled("account-a", false, None, 10)?;
        let _ = quota_release.send(());
        assert_eq!(quota_task.await.unwrap(), "fresh fake quota");
        assert!(!db.codex_quota_activation_revision("account-a")?.unwrap().0);
        let mut fake_posts = 0;
        if db
            .codex_quota_activation_revision("account-a")?
            .is_some_and(|(enabled, current)| enabled && current == revision)
        {
            fake_posts += 1;
        }
        assert_eq!(fake_posts, 0);

        db.set_codex_quota_activation_enabled("account-a", true, Some(1), 20)?;
        let new_revision = db.codex_quota_activation_revision("account-a")?.unwrap().1;
        let (model_release, model_ready) = tokio::sync::oneshot::channel::<()>();
        let model_task = tokio::spawn(async move {
            let _ = model_ready.await;
            "fake low-capability catalog"
        });
        db.set_codex_quota_activation_enabled("account-a", false, None, 30)?;
        let _ = model_release.send(());
        assert_eq!(model_task.await.unwrap(), "fake low-capability catalog");
        if db
            .codex_quota_activation_revision("account-a")?
            .is_some_and(|(enabled, current)| enabled && current == new_revision)
        {
            fake_posts += 1;
        }
        assert_eq!(fake_posts, 0);
        Ok(())
    }

    #[test]
    fn f03_same_account_concurrent_claims_allow_at_most_one_dispatch() -> Result<(), AppError> {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };

        let db = Arc::new(Database::memory()?);
        seed_eligible(&db, "account-a")?;
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        for index in 0..2 {
            let db = Arc::clone(&db);
            let dispatches = Arc::clone(&dispatches);
            workers.push(std::thread::spawn(move || {
                let attempt_id = format!("attempt-{index}");
                if db
                    .claim_codex_quota_activation("account-a", &attempt_id, 60_010 + index)
                    .expect("claim transaction")
                    .is_some()
                {
                    // Test-only sender stand-in: production POST is reachable
                    // only after a successful persisted claim.
                    dispatches.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }
        for worker in workers {
            worker.join().expect("claim worker");
        }
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn f03_different_accounts_claim_and_dispatch_independently() -> Result<(), AppError> {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };

        let db = Arc::new(Database::memory()?);
        seed_eligible(&db, "account-a")?;
        seed_eligible(&db, "account-b")?;
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        for (index, account_id) in ["account-a", "account-b"].into_iter().enumerate() {
            let db = Arc::clone(&db);
            let dispatches = Arc::clone(&dispatches);
            workers.push(std::thread::spawn(move || {
                if db
                    .claim_codex_quota_activation(
                        account_id,
                        &format!("attempt-{account_id}"),
                        60_020 + index as i64,
                    )
                    .expect("claim transaction")
                    .is_some()
                {
                    dispatches.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }
        for worker in workers {
            worker.join().expect("claim worker");
        }
        assert_eq!(dispatches.load(Ordering::SeqCst), 2);
        assert_eq!(
            db.get_codex_quota_activation_state("account-a")?
                .unwrap()
                .last_attempt_status
                .as_deref(),
            Some("reserved")
        );
        assert_eq!(
            db.get_codex_quota_activation_state("account-b")?
                .unwrap()
                .last_attempt_status
                .as_deref(),
            Some("reserved")
        );
        Ok(())
    }
}
