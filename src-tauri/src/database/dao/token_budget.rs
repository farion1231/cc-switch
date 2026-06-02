//! Token Budget 数据访问对象
//!
//! 提供 token_budgets 表的 CRUD 操作。所有方法都是 `Database` 的 impl，
//! 与其它 DAO 风格一致。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::token_budget::{
    BudgetPeriod, BudgetScope, CreateTokenBudgetInput, TokenBudget, UpdateTokenBudgetInput,
};
use rusqlite::params;

impl Database {
    /// 拉取所有预算，按 created_at 升序（与 providers 等表排序策略一致）。
    pub fn list_token_budgets(&self) -> Result<Vec<TokenBudget>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, scope, scope_value, period, period_start_day,
                        limit_tokens, limit_usd, enabled, created_at, updated_at
                 FROM token_budgets
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let iter = stmt
            .query_map([], row_to_budget)
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut out = Vec::new();
        for r in iter {
            out.push(r.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(out)
    }

    /// 按 id 拉单条。不存在时返回 None。
    pub fn get_token_budget(&self, id: &str) -> Result<Option<TokenBudget>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, scope, scope_value, period, period_start_day,
                        limit_tokens, limit_usd, enabled, created_at, updated_at
                 FROM token_budgets
                 WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut iter = stmt
            .query_map(params![id], row_to_budget)
            .map_err(|e| AppError::Database(e.to_string()))?;

        if let Some(r) = iter.next() {
            Ok(Some(r.map_err(|e| AppError::Database(e.to_string()))?))
        } else {
            Ok(None)
        }
    }

    /// 创建预算。`now_ms` 由调用方传入（service 层取 `chrono::Local::now()`），
    /// 便于测试和事务一致性。
    pub fn insert_token_budget(
        &self,
        id: &str,
        input: &CreateTokenBudgetInput,
        now_ms: i64,
    ) -> Result<TokenBudget, AppError> {
        let limit_usd = input.limit_usd.as_deref();
        let scope_str = input.scope.as_str();
        let period_str = input.period.as_str();
        let scope_value = input.scope_value.as_deref();

        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO token_budgets
                (id, name, scope, scope_value, period, period_start_day,
                 limit_tokens, limit_usd, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                input.name,
                scope_str,
                scope_value,
                period_str,
                input.period_start_day,
                input.limit_tokens,
                limit_usd,
                input.enabled,
                now_ms,
                now_ms,
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 token_budget 失败: {e}")))?;

        // INSERT 后立即回读，避免前后端字段漂移（DB 是真相）。
        drop(conn); // 释放借用以走 get_token_budget 路径
        self.get_token_budget(id)?
            .ok_or_else(|| AppError::Database("刚插入的 token_budget 找不到".into()))
    }

    /// 部分更新。`UpdateTokenBudgetInput` 内层 Option 语义：外层 None=不改；
    /// 外层 Some(None)=清空；外层 Some(Some(v))=设为 v。
    pub fn update_token_budget(
        &self,
        id: &str,
        patch: &UpdateTokenBudgetInput,
        now_ms: i64,
    ) -> Result<TokenBudget, AppError> {
        // 用 COALESCE 一次性合并，避免多次往返。
        // 注意：scope_value / limit_tokens / limit_usd 是"可空字段"，需要单独处理 NULLIFY。
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE token_budgets SET
                name = COALESCE(?2, name),
                scope = COALESCE(?3, scope),
                scope_value = CASE WHEN ?4 THEN NULL ELSE COALESCE(?5, scope_value) END,
                period = COALESCE(?6, period),
                period_start_day = COALESCE(?7, period_start_day),
                limit_tokens = CASE WHEN ?8 THEN NULL ELSE COALESCE(?9, limit_tokens) END,
                limit_usd = CASE WHEN ?10 THEN NULL ELSE COALESCE(?11, limit_usd) END,
                enabled = COALESCE(?12, enabled),
                updated_at = ?13
             WHERE id = ?1",
            params![
                id,
                patch.name,
                patch.scope.map(|s| s.as_str()),
                patch.scope_value.is_some() && patch.scope_value.as_ref().unwrap().is_none(), // 1=清空
                patch.scope_value.as_ref().and_then(|v| v.as_deref()),
                patch.period.map(|p| p.as_str()),
                patch.period_start_day,
                patch.limit_tokens.is_some() && patch.limit_tokens.as_ref().unwrap().is_none(),
                patch.limit_tokens.and_then(|v| v),
                patch.limit_usd.is_some() && patch.limit_usd.as_ref().unwrap().is_none(),
                patch.limit_usd.as_ref().and_then(|v| v.as_deref()),
                patch.enabled,
                now_ms,
            ],
        )
        .map_err(|e| AppError::Database(format!("更新 token_budget 失败: {e}")))?;

        let affected = conn.changes();
        drop(conn);
        if affected == 0 {
            return Err(AppError::Database(format!("token_budget id={id} 不存在")));
        }
        self.get_token_budget(id)?
            .ok_or_else(|| AppError::Database("更新后 token_budget 找不到".into()))
    }

    /// 按 id 删除。不存在不算错（幂等）。
    pub fn delete_token_budget(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM token_budgets WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(format!("删除 token_budget 失败: {e}")))?;
        Ok(())
    }
}

/// 行映射闭包：rusqlite 行 → TokenBudget。
///
/// 单点处理 scope/period 字符串 → 枚举的转换；遇到脏数据返回 rusqlite::Error，
/// 让上层包成 AppError::Database。
fn row_to_budget(row: &rusqlite::Row<'_>) -> rusqlite::Result<TokenBudget> {
    let id: String = row.get(0)?;
    let name: String = row.get(1)?;
    let scope_str: String = row.get(2)?;
    let scope_value: Option<String> = row.get(3)?;
    let period_str: String = row.get(4)?;
    let period_start_day: i32 = row.get(5)?;
    let limit_tokens: Option<i64> = row.get(6)?;
    let limit_usd: Option<String> = row.get(7)?;
    let enabled: bool = row.get(8)?;
    let created_at: Option<i64> = row.get(9)?;
    let updated_at: Option<i64> = row.get(10)?;

    let scope = BudgetScope::from_str(&scope_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown budget scope: {scope_str}").into(),
        )
    })?;
    let period = BudgetPeriod::from_str(&period_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown budget period: {period_str}").into(),
        )
    })?;

    Ok(TokenBudget {
        id,
        name,
        scope,
        scope_value,
        period,
        period_start_day,
        limit_tokens,
        limit_usd,
        enabled,
        created_at,
        updated_at,
    })
}
