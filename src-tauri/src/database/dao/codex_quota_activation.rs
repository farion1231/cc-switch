use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaActivationPolicy {
    pub account_id: String,
    pub owner_provider_id: String,
    pub enabled: bool,
    pub updated_at: i64,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_window_type: Option<String>,
    pub last_model: Option<String>,
    pub last_attempt_at: Option<i64>,
    pub last_observed_limit_id: Option<String>,
    pub last_observed_window_seconds: Option<i64>,
    pub last_observed_reset_at: Option<i64>,
    pub last_observed_reset_after_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaActivationAttempt {
    pub account_id: String,
    pub limit_id: String,
    pub window_seconds: i64,
    pub window_generation: String,
    pub owner_provider_id: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub status: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

fn row_to_policy(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodexQuotaActivationPolicy> {
    Ok(CodexQuotaActivationPolicy {
        account_id: row.get(0)?,
        owner_provider_id: row.get(1)?,
        enabled: row.get::<_, i64>(2)? != 0,
        updated_at: row.get(3)?,
        last_status: row.get(4)?,
        last_error: row.get(5)?,
        last_window_type: row.get(6)?,
        last_model: row.get(7)?,
        last_attempt_at: row.get(8)?,
        last_observed_limit_id: row.get(9)?,
        last_observed_window_seconds: row.get(10)?,
        last_observed_reset_at: row.get(11)?,
        last_observed_reset_after_seconds: row.get(12)?,
    })
}

impl Database {
    pub fn get_codex_quota_activation_policy(
        &self,
        account_id: &str,
    ) -> Result<Option<CodexQuotaActivationPolicy>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT account_id, owner_provider_id, enabled, updated_at,
                    last_status, last_error, last_window_type, last_model,
                    last_attempt_at, last_observed_limit_id,
                    last_observed_window_seconds, last_observed_reset_at,
                    last_observed_reset_after_seconds
             FROM codex_quota_activation_policies WHERE account_id = ?1",
            params![account_id],
            row_to_policy,
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn upsert_codex_quota_activation_policy(
        &self,
        account_id: &str,
        owner_provider_id: &str,
        enabled: bool,
        now: i64,
    ) -> Result<CodexQuotaActivationPolicy, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let existing = tx
            .query_row(
                "SELECT account_id, owner_provider_id, enabled, updated_at,
                        last_status, last_error, last_window_type, last_model,
                        last_attempt_at, last_observed_limit_id,
                        last_observed_window_seconds, last_observed_reset_at,
                        last_observed_reset_after_seconds
                 FROM codex_quota_activation_policies WHERE account_id = ?1",
                params![account_id],
                row_to_policy,
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;

        if let Some(existing) = existing {
            if existing.owner_provider_id != owner_provider_id {
                return Err(AppError::InvalidInput(format!(
                    "Codex account is already owned by provider {}",
                    existing.owner_provider_id
                )));
            }
            tx.execute(
                "UPDATE codex_quota_activation_policies
                    SET enabled = ?1, updated_at = ?2 WHERE account_id = ?3",
                params![enabled, now, account_id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        } else {
            tx.execute(
                "INSERT INTO codex_quota_activation_policies
                    (account_id, owner_provider_id, enabled, updated_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![account_id, owner_provider_id, enabled, now],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        drop(conn);
        self.get_codex_quota_activation_policy(account_id)?
            .ok_or_else(|| AppError::Database("activation policy disappeared after write".into()))
    }

    pub fn claim_codex_quota_activation(
        &self,
        account_id: &str,
        limit_id: &str,
        window_seconds: i64,
        window_generation: &str,
        owner_provider_id: &str,
        model: Option<&str>,
        reasoning_effort: Option<&str>,
        now: i64,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO codex_quota_activation_attempts
                    (account_id, limit_id, window_seconds, window_generation,
                     owner_provider_id, model, reasoning_effort, status, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'reserved', ?8)",
                params![
                    account_id,
                    limit_id,
                    window_seconds,
                    window_generation,
                    owner_provider_id,
                    model,
                    reasoning_effort,
                    now
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(changed == 1)
    }

    pub fn update_codex_quota_activation_attempt(
        &self,
        account_id: &str,
        limit_id: &str,
        window_generation: &str,
        status: &str,
        ended_at: Option<i64>,
        exit_code: Option<i32>,
        error: Option<&str>,
        model: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE codex_quota_activation_attempts
                SET status = ?1, ended_at = ?2, exit_code = ?3, error = ?4,
                    model = COALESCE(?5, model)
              WHERE account_id = ?6 AND limit_id = ?7 AND window_generation = ?8",
            params![
                status,
                ended_at,
                exit_code,
                error,
                model,
                account_id,
                limit_id,
                window_generation
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn update_codex_quota_activation_summary(
        &self,
        account_id: &str,
        status: Option<&str>,
        error: Option<&str>,
        window_type: Option<&str>,
        model: Option<&str>,
        attempt_at: Option<i64>,
        observed_reset_at: Option<i64>,
        observed_reset_after_seconds: Option<i64>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE codex_quota_activation_policies
                SET last_status = ?1, last_error = ?2, last_window_type = ?3,
                    last_model = ?4, last_attempt_at = COALESCE(?5, last_attempt_at),
                    last_observed_reset_at = ?6,
                    last_observed_reset_after_seconds = ?7,
                    updated_at = ?8
              WHERE account_id = ?9",
            params![
                status,
                error,
                window_type,
                model,
                attempt_at,
                observed_reset_at,
                observed_reset_after_seconds,
                chrono::Utc::now().timestamp_millis(),
                account_id
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// Update the visible activation status without changing the observed
    /// quota generation. Used when no safe explicit model is configured: the
    /// next fresh quota observation must remain eligible to retry resolution.
    pub fn update_codex_quota_activation_status_summary(
        &self,
        account_id: &str,
        status: Option<&str>,
        error: Option<&str>,
        window_type: Option<&str>,
        model: Option<&str>,
        attempt_at: Option<i64>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE codex_quota_activation_policies
                SET last_status = ?1, last_error = ?2, last_window_type = ?3,
                    last_model = ?4, last_attempt_at = COALESCE(?5, last_attempt_at),
                    updated_at = ?6
              WHERE account_id = ?7",
            params![
                status,
                error,
                window_type,
                model,
                attempt_at,
                chrono::Utc::now().timestamp_millis(),
                account_id
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn record_codex_quota_observation(
        &self,
        account_id: &str,
        observed_limit_id: &str,
        observed_window_seconds: i64,
        observed_reset_at: Option<i64>,
        observed_reset_after_seconds: Option<i64>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE codex_quota_activation_policies
                SET last_observed_limit_id = ?1,
                    last_observed_window_seconds = ?2,
                    last_observed_reset_at = ?3,
                    last_observed_reset_after_seconds = ?4
              WHERE account_id = ?5",
            params![
                observed_limit_id,
                observed_window_seconds,
                observed_reset_at,
                observed_reset_after_seconds,
                account_id
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// Mark claims left in `reserved`/`running` after an interrupted process as
    /// indeterminate. They remain non-retryable for the same generation, which
    /// preserves the at-most-once guarantee while preventing a permanently
    /// misleading in-flight status.
    pub fn mark_stale_codex_quota_activation_attempts_unknown(
        &self,
        account_id: &str,
        now: i64,
        stale_after_ms: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE codex_quota_activation_attempts
                SET status = 'unknown', ended_at = ?1,
                    error = COALESCE(error, 'activation interrupted before completion')
              WHERE account_id = ?2
                AND status IN ('reserved', 'running')
                AND started_at < ?1 - ?3",
            params![now, account_id, stale_after_ms],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_latest_codex_quota_activation_attempt(
        &self,
        account_id: &str,
    ) -> Result<Option<CodexQuotaActivationAttempt>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT account_id, limit_id, window_seconds, window_generation,
                    owner_provider_id, model, reasoning_effort, status,
                    started_at, ended_at, exit_code, error
             FROM codex_quota_activation_attempts
             WHERE account_id = ?1 ORDER BY started_at DESC, id DESC LIMIT 1",
            params![account_id],
            |row| {
                Ok(CodexQuotaActivationAttempt {
                    account_id: row.get(0)?,
                    limit_id: row.get(1)?,
                    window_seconds: row.get(2)?,
                    window_generation: row.get(3)?,
                    owner_provider_id: row.get(4)?,
                    model: row.get(5)?,
                    reasoning_effort: row.get(6)?,
                    status: row.get(7)?,
                    started_at: row.get(8)?,
                    ended_at: row.get(9)?,
                    exit_code: row.get(10)?,
                    error: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_owner_is_stable_and_claim_is_idempotent() -> Result<(), AppError> {
        let db = Database::memory()?;
        let policy = db.upsert_codex_quota_activation_policy("account-a", "provider-a", true, 1)?;
        assert!(policy.enabled);
        assert!(db.claim_codex_quota_activation(
            "account-a",
            "five_hour",
            18_000,
            "generation-1",
            "provider-a",
            Some("gpt-test"),
            Some("low"),
            2,
        )?);
        assert!(!db.claim_codex_quota_activation(
            "account-a",
            "five_hour",
            18_000,
            "generation-1",
            "provider-a",
            Some("gpt-test"),
            Some("low"),
            3,
        )?);
        assert!(
            db.upsert_codex_quota_activation_policy("account-a", "provider-a", false, 4)?
                .enabled
                == false
        );
        assert!(db
            .upsert_codex_quota_activation_policy("account-a", "provider-b", true, 5)
            .is_err());
        Ok(())
    }

    #[test]
    fn latest_attempt_status_round_trips() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_codex_quota_activation_policy("account-a", "provider-a", true, 1)?;
        db.claim_codex_quota_activation(
            "account-a",
            "weekly",
            604_800,
            "generation-2",
            "provider-a",
            None,
            None,
            2,
        )?;
        db.update_codex_quota_activation_attempt(
            "account-a",
            "weekly",
            "generation-2",
            "unknown",
            Some(3),
            None,
            Some("timeout: activation outcome is unknown"),
            Some("gpt-actual"),
        )?;
        let attempt = db
            .get_latest_codex_quota_activation_attempt("account-a")?
            .unwrap();
        assert_eq!(attempt.status, "unknown");
        assert_eq!(attempt.model.as_deref(), Some("gpt-actual"));
        assert_eq!(
            attempt.error.as_deref(),
            Some("timeout: activation outcome is unknown")
        );
        Ok(())
    }

    #[test]
    fn stale_inflight_attempts_are_marked_unknown_without_being_retried() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_codex_quota_activation_policy("account-a", "provider-a", true, 1)?;
        db.claim_codex_quota_activation(
            "account-a",
            "weekly",
            604_800,
            "generation-3",
            "provider-a",
            None,
            None,
            1_000,
        )?;
        db.mark_stale_codex_quota_activation_attempts_unknown(
            "account-a",
            1_000 + 600_000,
            300_000,
        )?;
        let attempt = db
            .get_latest_codex_quota_activation_attempt("account-a")?
            .unwrap();
        assert_eq!(attempt.status, "unknown");
        assert_eq!(attempt.ended_at, Some(601_000));
        assert!(!db.claim_codex_quota_activation(
            "account-a",
            "weekly",
            604_800,
            "generation-3",
            "provider-a",
            None,
            None,
            601_001,
        )?);
        Ok(())
    }

    #[test]
    fn unresolved_model_status_preserves_observed_generation() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_codex_quota_activation_policy("account-a", "provider-a", true, 1)?;
        db.record_codex_quota_observation(
            "account-a",
            "five_hour",
            18_000,
            Some(20_000),
            Some(18_000),
        )?;
        db.update_codex_quota_activation_status_summary(
            "account-a",
            Some("model_unresolved"),
            Some("owner provider has no explicit model"),
            Some("five_hour"),
            None,
            None,
        )?;
        let policy = db
            .get_codex_quota_activation_policy("account-a")?
            .expect("policy should remain available");
        assert_eq!(policy.last_status.as_deref(), Some("model_unresolved"));
        assert_eq!(policy.last_observed_reset_at, Some(20_000));
        assert_eq!(policy.last_observed_reset_after_seconds, Some(18_000));
        Ok(())
    }
}
