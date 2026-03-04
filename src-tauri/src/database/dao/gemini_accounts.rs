use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::gemini_account::{GeminiAccount, GeminiProviderBinding, GeminiUsageState};
use rusqlite::params;

impl Database {
    pub fn list_gemini_accounts(&self, active_only: bool) -> Result<Vec<GeminiAccount>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut sql = String::from(
            "SELECT id,email,display_name,google_account_id,access_token,refresh_token,token_type,expiry_date,source,is_active,created_at,updated_at
             FROM gemini_accounts",
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
                Ok(GeminiAccount {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    display_name: row.get(2)?,
                    google_account_id: row.get(3)?,
                    access_token: row.get(4)?,
                    refresh_token: row.get(5)?,
                    token_type: row.get(6)?,
                    expiry_date: row.get(7)?,
                    source: row.get(8)?,
                    is_active: row.get::<_, i64>(9)? == 1,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn get_gemini_account_by_id(&self, id: &str) -> Result<Option<GeminiAccount>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id,email,display_name,google_account_id,access_token,refresh_token,token_type,expiry_date,source,is_active,created_at,updated_at
                 FROM gemini_accounts WHERE id = ?1 LIMIT 1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(GeminiAccount {
                id: row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
                email: row.get(1).map_err(|e| AppError::Database(e.to_string()))?,
                display_name: row.get(2).map_err(|e| AppError::Database(e.to_string()))?,
                google_account_id: row.get(3).map_err(|e| AppError::Database(e.to_string()))?,
                access_token: row.get(4).map_err(|e| AppError::Database(e.to_string()))?,
                refresh_token: row.get(5).map_err(|e| AppError::Database(e.to_string()))?,
                token_type: row.get(6).map_err(|e| AppError::Database(e.to_string()))?,
                expiry_date: row.get(7).map_err(|e| AppError::Database(e.to_string()))?,
                source: row.get(8).map_err(|e| AppError::Database(e.to_string()))?,
                is_active: row
                    .get::<_, i64>(9)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    == 1,
                created_at: row.get(10).map_err(|e| AppError::Database(e.to_string()))?,
                updated_at: row.get(11).map_err(|e| AppError::Database(e.to_string()))?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_gemini_account(&self, account: &GeminiAccount) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO gemini_accounts (
                id,email,display_name,google_account_id,access_token,refresh_token,token_type,expiry_date,source,is_active,created_at,updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(id) DO UPDATE SET
                email=excluded.email,
                display_name=excluded.display_name,
                google_account_id=excluded.google_account_id,
                access_token=excluded.access_token,
                refresh_token=excluded.refresh_token,
                token_type=excluded.token_type,
                expiry_date=excluded.expiry_date,
                source=excluded.source,
                is_active=excluded.is_active,
                updated_at=excluded.updated_at",
            params![
                account.id,
                account.email,
                account.display_name,
                account.google_account_id,
                account.access_token,
                account.refresh_token,
                account.token_type,
                account.expiry_date,
                account.source,
                if account.is_active { 1 } else { 0 },
                account.created_at,
                account.updated_at
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn upsert_gemini_provider_binding(
        &self,
        binding: &GeminiProviderBinding,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO gemini_provider_bindings (provider_id,account_id,auto_bound,updated_at)
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

    pub fn get_gemini_provider_binding(
        &self,
        provider_id: &str,
    ) -> Result<Option<GeminiProviderBinding>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT provider_id,account_id,auto_bound,updated_at FROM gemini_provider_bindings WHERE provider_id = ?1 LIMIT 1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![provider_id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(GeminiProviderBinding {
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

    pub fn get_gemini_account_by_provider(
        &self,
        provider_id: &str,
    ) -> Result<Option<GeminiAccount>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT a.id,a.email,a.display_name,a.google_account_id,a.access_token,a.refresh_token,a.token_type,a.expiry_date,a.source,a.is_active,a.created_at,a.updated_at
             FROM gemini_provider_bindings b
             JOIN gemini_accounts a ON a.id = b.account_id
             WHERE b.provider_id = ?1 LIMIT 1",
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![provider_id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(GeminiAccount {
                id: row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
                email: row.get(1).map_err(|e| AppError::Database(e.to_string()))?,
                display_name: row.get(2).map_err(|e| AppError::Database(e.to_string()))?,
                google_account_id: row.get(3).map_err(|e| AppError::Database(e.to_string()))?,
                access_token: row.get(4).map_err(|e| AppError::Database(e.to_string()))?,
                refresh_token: row.get(5).map_err(|e| AppError::Database(e.to_string()))?,
                token_type: row.get(6).map_err(|e| AppError::Database(e.to_string()))?,
                expiry_date: row.get(7).map_err(|e| AppError::Database(e.to_string()))?,
                source: row.get(8).map_err(|e| AppError::Database(e.to_string()))?,
                is_active: row
                    .get::<_, i64>(9)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    == 1,
                created_at: row.get(10).map_err(|e| AppError::Database(e.to_string()))?,
                updated_at: row.get(11).map_err(|e| AppError::Database(e.to_string()))?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_gemini_usage_state(&self, usage: &GeminiUsageState) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO gemini_usage_state (account_id,cooldown_until,last_error,last_refresh_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(account_id) DO UPDATE SET
                cooldown_until=excluded.cooldown_until,
                last_error=excluded.last_error,
                last_refresh_at=excluded.last_refresh_at",
            params![
                usage.account_id,
                usage.cooldown_until,
                usage.last_error,
                usage.last_refresh_at
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_gemini_usage_state(
        &self,
        account_id: &str,
    ) -> Result<Option<GeminiUsageState>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT account_id,cooldown_until,last_error,last_refresh_at
                 FROM gemini_usage_state WHERE account_id = ?1 LIMIT 1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![account_id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(GeminiUsageState {
                account_id: row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
                cooldown_until: row.get(1).map_err(|e| AppError::Database(e.to_string()))?,
                last_error: row.get(2).map_err(|e| AppError::Database(e.to_string()))?,
                last_refresh_at: row.get(3).map_err(|e| AppError::Database(e.to_string()))?,
            }))
        } else {
            Ok(None)
        }
    }
}
