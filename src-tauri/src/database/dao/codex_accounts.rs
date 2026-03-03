use crate::codex_account::{CodexAccount, CodexProviderBinding, CodexUsageState};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;

impl Database {
    pub fn list_codex_accounts(&self, active_only: bool) -> Result<Vec<CodexAccount>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut sql = String::from(
            "SELECT id,email,display_name,account_id,plan_type,auth_mode,access_token,refresh_token,id_token,last_refresh_at,last_used_at,source,is_active,created_at,updated_at
             FROM codex_accounts",
        );
        if active_only {
            sql.push_str(" WHERE is_active = 1");
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let iter = stmt
            .query_map([], |row| {
                Ok(CodexAccount {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    display_name: row.get(2)?,
                    account_id: row.get(3)?,
                    plan_type: row.get(4)?,
                    auth_mode: row.get(5)?,
                    access_token: row.get(6)?,
                    refresh_token: row.get(7)?,
                    id_token: row.get(8)?,
                    last_refresh_at: row.get(9)?,
                    last_used_at: row.get(10)?,
                    source: row.get(11)?,
                    is_active: row.get::<_, i64>(12)? == 1,
                    created_at: row.get(13)?,
                    updated_at: row.get(14)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn get_codex_account_by_id(&self, id: &str) -> Result<Option<CodexAccount>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id,email,display_name,account_id,plan_type,auth_mode,access_token,refresh_token,id_token,last_refresh_at,last_used_at,source,is_active,created_at,updated_at
                 FROM codex_accounts WHERE id = ?1 LIMIT 1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(CodexAccount {
                id: row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
                email: row.get(1).map_err(|e| AppError::Database(e.to_string()))?,
                display_name: row.get(2).map_err(|e| AppError::Database(e.to_string()))?,
                account_id: row.get(3).map_err(|e| AppError::Database(e.to_string()))?,
                plan_type: row.get(4).map_err(|e| AppError::Database(e.to_string()))?,
                auth_mode: row.get(5).map_err(|e| AppError::Database(e.to_string()))?,
                access_token: row.get(6).map_err(|e| AppError::Database(e.to_string()))?,
                refresh_token: row.get(7).map_err(|e| AppError::Database(e.to_string()))?,
                id_token: row.get(8).map_err(|e| AppError::Database(e.to_string()))?,
                last_refresh_at: row.get(9).map_err(|e| AppError::Database(e.to_string()))?,
                last_used_at: row.get(10).map_err(|e| AppError::Database(e.to_string()))?,
                source: row.get(11).map_err(|e| AppError::Database(e.to_string()))?,
                is_active: row
                    .get::<_, i64>(12)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    == 1,
                created_at: row.get(13).map_err(|e| AppError::Database(e.to_string()))?,
                updated_at: row.get(14).map_err(|e| AppError::Database(e.to_string()))?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_codex_account(&self, account: &CodexAccount) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO codex_accounts (
                id,email,display_name,account_id,plan_type,auth_mode,access_token,refresh_token,id_token,last_refresh_at,last_used_at,source,is_active,created_at,updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT(id) DO UPDATE SET
                email=excluded.email,
                display_name=excluded.display_name,
                account_id=excluded.account_id,
                plan_type=excluded.plan_type,
                auth_mode=excluded.auth_mode,
                access_token=excluded.access_token,
                refresh_token=excluded.refresh_token,
                id_token=excluded.id_token,
                last_refresh_at=excluded.last_refresh_at,
                last_used_at=excluded.last_used_at,
                source=excluded.source,
                is_active=excluded.is_active,
                updated_at=excluded.updated_at",
            params![
                account.id,
                account.email,
                account.display_name,
                account.account_id,
                account.plan_type,
                account.auth_mode,
                account.access_token,
                account.refresh_token,
                account.id_token,
                account.last_refresh_at,
                account.last_used_at,
                account.source,
                if account.is_active { 1 } else { 0 },
                account.created_at,
                account.updated_at
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn upsert_codex_provider_binding(
        &self,
        binding: &CodexProviderBinding,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO codex_provider_bindings (provider_id,account_id,auto_bound,updated_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(provider_id) DO UPDATE SET
                account_id=excluded.account_id,
                auto_bound=excluded.auto_bound,
                updated_at=excluded.updated_at",
            params![
                binding.provider_id,
                binding.account_id,
                if binding.auto_bound { 1 } else { 0 },
                binding.updated_at
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_codex_provider_binding(
        &self,
        provider_id: &str,
    ) -> Result<Option<CodexProviderBinding>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT provider_id,account_id,auto_bound,updated_at FROM codex_provider_bindings WHERE provider_id = ?1 LIMIT 1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![provider_id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(CodexProviderBinding {
                provider_id: row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
                account_id: row.get(1).map_err(|e| AppError::Database(e.to_string()))?,
                auto_bound: row
                    .get::<_, i64>(2)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    == 1,
                updated_at: row.get(3).map_err(|e| AppError::Database(e.to_string()))?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_codex_account_by_provider(
        &self,
        provider_id: &str,
    ) -> Result<Option<CodexAccount>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT a.id,a.email,a.display_name,a.account_id,a.plan_type,a.auth_mode,a.access_token,a.refresh_token,a.id_token,a.last_refresh_at,a.last_used_at,a.source,a.is_active,a.created_at,a.updated_at
             FROM codex_provider_bindings b
             JOIN codex_accounts a ON a.id = b.account_id
             WHERE b.provider_id = ?1 LIMIT 1",
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![provider_id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(CodexAccount {
                id: row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
                email: row.get(1).map_err(|e| AppError::Database(e.to_string()))?,
                display_name: row.get(2).map_err(|e| AppError::Database(e.to_string()))?,
                account_id: row.get(3).map_err(|e| AppError::Database(e.to_string()))?,
                plan_type: row.get(4).map_err(|e| AppError::Database(e.to_string()))?,
                auth_mode: row.get(5).map_err(|e| AppError::Database(e.to_string()))?,
                access_token: row.get(6).map_err(|e| AppError::Database(e.to_string()))?,
                refresh_token: row.get(7).map_err(|e| AppError::Database(e.to_string()))?,
                id_token: row.get(8).map_err(|e| AppError::Database(e.to_string()))?,
                last_refresh_at: row.get(9).map_err(|e| AppError::Database(e.to_string()))?,
                last_used_at: row.get(10).map_err(|e| AppError::Database(e.to_string()))?,
                source: row.get(11).map_err(|e| AppError::Database(e.to_string()))?,
                is_active: row
                    .get::<_, i64>(12)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    == 1,
                created_at: row.get(13).map_err(|e| AppError::Database(e.to_string()))?,
                updated_at: row.get(14).map_err(|e| AppError::Database(e.to_string()))?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_codex_usage_state(&self, usage: &CodexUsageState) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO codex_usage_state (
                account_id,allowed,limit_reached,primary_used_percent,primary_limit_window_seconds,primary_reset_at,primary_reset_after_seconds,
                secondary_used_percent,secondary_limit_window_seconds,secondary_reset_at,secondary_reset_after_seconds,
                credits_has_credits,credits_balance,credits_unlimited,last_refresh_at,last_error
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(account_id) DO UPDATE SET
                allowed=excluded.allowed,
                limit_reached=excluded.limit_reached,
                primary_used_percent=excluded.primary_used_percent,
                primary_limit_window_seconds=excluded.primary_limit_window_seconds,
                primary_reset_at=excluded.primary_reset_at,
                primary_reset_after_seconds=excluded.primary_reset_after_seconds,
                secondary_used_percent=excluded.secondary_used_percent,
                secondary_limit_window_seconds=excluded.secondary_limit_window_seconds,
                secondary_reset_at=excluded.secondary_reset_at,
                secondary_reset_after_seconds=excluded.secondary_reset_after_seconds,
                credits_has_credits=excluded.credits_has_credits,
                credits_balance=excluded.credits_balance,
                credits_unlimited=excluded.credits_unlimited,
                last_refresh_at=excluded.last_refresh_at,
                last_error=excluded.last_error",
            params![
                usage.account_id,
                usage.allowed.map(|v| if v { 1 } else { 0 }),
                usage.limit_reached.map(|v| if v { 1 } else { 0 }),
                usage.primary_used_percent,
                usage.primary_limit_window_seconds,
                usage.primary_reset_at,
                usage.primary_reset_after_seconds,
                usage.secondary_used_percent,
                usage.secondary_limit_window_seconds,
                usage.secondary_reset_at,
                usage.secondary_reset_after_seconds,
                usage.credits_has_credits.map(|v| if v { 1 } else { 0 }),
                usage.credits_balance,
                usage.credits_unlimited.map(|v| if v { 1 } else { 0 }),
                usage.last_refresh_at,
                usage.last_error
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_codex_usage_state(
        &self,
        account_id: &str,
    ) -> Result<Option<CodexUsageState>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT account_id,allowed,limit_reached,primary_used_percent,primary_limit_window_seconds,primary_reset_at,primary_reset_after_seconds,
                        secondary_used_percent,secondary_limit_window_seconds,secondary_reset_at,secondary_reset_after_seconds,
                        credits_has_credits,credits_balance,credits_unlimited,last_refresh_at,last_error
                 FROM codex_usage_state WHERE account_id = ?1 LIMIT 1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![account_id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(CodexUsageState {
                account_id: row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
                allowed: row
                    .get::<_, Option<i64>>(1)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    .map(|v| v == 1),
                limit_reached: row
                    .get::<_, Option<i64>>(2)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    .map(|v| v == 1),
                primary_used_percent: row.get(3).map_err(|e| AppError::Database(e.to_string()))?,
                primary_limit_window_seconds: row
                    .get(4)
                    .map_err(|e| AppError::Database(e.to_string()))?,
                primary_reset_at: row.get(5).map_err(|e| AppError::Database(e.to_string()))?,
                secondary_reset_after_seconds: row
                    .get(10)
                    .map_err(|e| AppError::Database(e.to_string()))?,
                primary_reset_after_seconds: row
                    .get(6)
                    .map_err(|e| AppError::Database(e.to_string()))?,
                secondary_used_percent: row
                    .get(7)
                    .map_err(|e| AppError::Database(e.to_string()))?,
                secondary_limit_window_seconds: row
                    .get(8)
                    .map_err(|e| AppError::Database(e.to_string()))?,
                secondary_reset_at: row.get(9).map_err(|e| AppError::Database(e.to_string()))?,
                credits_has_credits: row
                    .get::<_, Option<i64>>(11)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    .map(|v| v == 1),
                credits_balance: row.get(12).map_err(|e| AppError::Database(e.to_string()))?,
                credits_unlimited: row
                    .get::<_, Option<i64>>(13)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    .map(|v| v == 1),
                last_refresh_at: row.get(14).map_err(|e| AppError::Database(e.to_string()))?,
                last_error: row.get(15).map_err(|e| AppError::Database(e.to_string()))?,
            }))
        } else {
            Ok(None)
        }
    }
}
