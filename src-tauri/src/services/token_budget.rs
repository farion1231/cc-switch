//! Token Budget 业务层
//!
//! 周期窗口计算 + 当前周期消耗聚合。三个职责：
//!
//! 1. **CRUD 包装**：调用 `Database` 的 DAO 方法，校验入参，补 id/timestamp。
//! 2. **周期窗口**：把 (period, period_start_day, now) 映射到 [period_start, period_end)，
//!    用本地时区（与 `usage_stats.rs` 保持一致）。DST 切换由 chrono 自动处理。
//! 3. **状态聚合**：在窗口内对 `proxy_request_logs` 做带 scope 过滤的求和，
//!    复用 `fresh_input_sql`（cache 归一化）+ `effective_usage_log_filter`（去重）。
//!
//! MVP 不读 `usage_daily_rollups`：单周期最多 ~31 天的 detail rows，SQLite 直接聚合足够快。
//! 若后续发现热路径慢，再合并 rollup（参考 `usage_stats::get_usage_summary` 写法）。
//!
//! **时间单位**：`proxy_request_logs.created_at` 存储为 Unix **秒**（与 usage_stats.rs 一致），
//! 所有窗口边界、SQL 条件均使用秒级时间戳。

use crate::database::Database;
use crate::error::AppError;
use crate::services::sql_helpers::fresh_input_sql;
use crate::services::usage_stats::effective_usage_log_filter;
use crate::store::AppState;
use crate::token_budget::{
    BudgetPeriod, BudgetScope, CreateTokenBudgetInput, TokenBudget, UpdateTokenBudgetInput,
};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Weekday};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

/// 周期窗口（unix **秒**，本地时区）。半开区间：[start, end)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetWindow {
    /// 窗口起始（含），unix 秒
    pub start_sec: i64,
    /// 窗口结束（不含），unix 秒
    pub end_sec: i64,
}

/// 一条预算的实时状态。前端 BudgetCard 直接消费。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetStatus {
    pub budget: TokenBudget,
    pub window: BudgetWindow,
    /// 当前窗口内已消费的 real_total_tokens（cache 归一化后）
    pub consumed_tokens: u64,
    /// 当前窗口内已消费的 USD（4 项成本之和），保留 6 位小数字符串（与 UsageSummary 对齐）
    pub consumed_usd: String,
    /// tokens 维度进度 (0.0 ~ )；超过 1.0 表示超额。None=未设置 tokens 上限。
    pub pct_tokens: Option<f64>,
    /// usd 维度进度。None=未设置 usd 上限。
    pub pct_usd: Option<f64>,
    pub remaining_tokens: Option<i64>, // 可能为负（已超）
    pub remaining_usd: Option<String>,
}

/// 入口服务结构体（与 PromptService 等保持一致风格）。
pub struct TokenBudgetService;

impl TokenBudgetService {
    // ── CRUD ────────────────────────────────────────────────────────

    pub fn list(state: &AppState) -> Result<Vec<TokenBudget>, AppError> {
        state.db.list_token_budgets()
    }

    pub fn create(
        state: &AppState,
        input: CreateTokenBudgetInput,
    ) -> Result<TokenBudget, AppError> {
        validate_input(&input)?;
        let id = Uuid::new_v4().to_string();
        let now = now_sec();
        state.db.insert_token_budget(&id, &input, now)
    }

    pub fn update(
        state: &AppState,
        id: &str,
        patch: UpdateTokenBudgetInput,
    ) -> Result<TokenBudget, AppError> {
        // 校验：若 patch 改了 limit/period/scope，需结合 DB 中现有值再校验最终合法性。
        let existing = state.db.get_token_budget(id)?.ok_or_else(|| {
            AppError::Message(format!("Token budget {id} 不存在"))
        })?;
        let merged = merge_for_validation(&existing, &patch);
        validate_merged(&merged)?;

        // touch updated_at
        if patch.name.is_none()
            && patch.scope.is_none()
            && patch.scope_value.is_none()
            && patch.period.is_none()
            && patch.period_start_day.is_none()
            && patch.limit_tokens.is_none()
            && patch.limit_usd.is_none()
            && patch.enabled.is_none()
        {
            // 完全空 patch：仍然 touch updated_at，给用户明确的"保存了"反馈
        }
        let now = now_sec();
        let updated = state.db.update_token_budget(id, &patch, now)?;
        // 校验副作用：上面借用结束才调用 DAO
        drop(merged);
        Ok(updated)
    }

    pub fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
        state.db.delete_token_budget(id)
    }

    // ── 状态聚合 ────────────────────────────────────────────────────

    pub fn get_status(
        state: &AppState,
        id: &str,
    ) -> Result<Option<BudgetStatus>, AppError> {
        let Some(budget) = state.db.get_token_budget(id)? else {
            return Ok(None);
        };
        Ok(Some(Self::status_for(state.db.clone(), &budget, now_sec())?))
    }

    pub fn get_all_statuses(state: &AppState) -> Result<Vec<BudgetStatus>, AppError> {
        let budgets = state.db.list_token_budgets()?;
        let now = now_sec();
        let db = state.db.clone();
        budgets
            .into_iter()
            .map(|b| Self::status_for(db.clone(), &b, now))
            .collect()
    }

    /// 内部：给定预算 + 时间点，计算窗口并对 `proxy_request_logs` 聚合。
    ///
    /// 暴露成 `pub(crate)` 是为了单测：测试可以用固定时间点驱动周期边界。
    pub(crate) fn status_for(
        db: Arc<Database>,
        budget: &TokenBudget,
        now_sec: i64,
    ) -> Result<BudgetStatus, AppError> {
        let window = compute_period_window(budget.period, budget.period_start_day, now_sec);
        let (consumed_tokens, consumed_usd) = aggregate_window(&db, budget, window)?;

        let pct_tokens = budget
            .limit_tokens
            .map(|lim| consumed_tokens as f64 / lim.max(1) as f64);
        let pct_usd = budget.limit_usd.as_deref().and_then(|lim_str| {
            Decimal::from_str(lim_str)
                .ok()
                .map(|lim| consumed_usd / lim)
        });

        let remaining_tokens = budget
            .limit_tokens
            .map(|lim| lim - consumed_tokens as i64);
        let remaining_usd = budget
            .limit_usd
            .as_deref()
            .and_then(|lim_str| Decimal::from_str(lim_str).ok())
            .map(|lim| lim - consumed_usd);

        Ok(BudgetStatus {
            budget: budget.clone(),
            window,
            consumed_tokens,
            consumed_usd: format!("{consumed_usd:.6}"),
            pct_tokens,
            pct_usd: pct_usd.map(|v| v.to_f64().unwrap_or(0.0)),
            remaining_tokens,
            remaining_usd: remaining_usd.map(|d| format!("{d:.6}")),
        })
    }
}

// ── 周期窗口 ────────────────────────────────────────────────────────

/// 计算当前时间点 `now_sec`（unix **秒**）落在哪个预算周期内，并返回该周期的边界。
///
/// * `daily` → 本地时区今日 00:00 ~ 明日 00:00；忽略 `start_day`。
/// * `weekly` → 本地本周 `start_day` 00:00 起的 7 天；`start_day=0` 表示周日。
/// * `monthly` → 本月 `start_day` 日 00:00 起到下月同日 00:00；
///   若 `start_day` 超过当月天数（如 2 月 + start_day=30），向后推迟到合法日期。
pub fn compute_period_window(
    period: BudgetPeriod,
    start_day: i32,
    now_sec: i64,
) -> BudgetWindow {
    let now_local = Local
        .timestamp_opt(now_sec, 0)
        .single()
        .unwrap_or_else(|| Local::now());
    match period {
        BudgetPeriod::Daily => daily_window(now_local),
        BudgetPeriod::Weekly => weekly_window(now_local, start_day),
        BudgetPeriod::Monthly => monthly_window(now_local, start_day),
    }
}

fn daily_window(now: DateTime<Local>) -> BudgetWindow {
    let start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight always valid");
    let start_local = Local
        .from_local_datetime(&start)
        .single()
        .expect("midnight always valid");
    let end_local = start_local + chrono::Duration::days(1);
    BudgetWindow {
        start_sec: start_local.timestamp(),
        end_sec: end_local.timestamp(),
    }
}

fn weekly_window(now: DateTime<Local>, start_day: i32) -> BudgetWindow {
    let today = now.weekday();
    let target = weekday_from_int(start_day).unwrap_or(Weekday::Mon);
    let delta = (today.num_days_from_monday() as i32 - target.num_days_from_monday() as i32)
        .rem_euclid(7);
    let monday_like = now.date_naive() - chrono::Duration::days(delta as i64);
    let start = monday_like.and_hms_opt(0, 0, 0).expect("midnight valid");
    let start_local = Local
        .from_local_datetime(&start)
        .single()
        .expect("midnight valid");
    let end_local = start_local + chrono::Duration::days(7);
    BudgetWindow {
        start_sec: start_local.timestamp(),
        end_sec: end_local.timestamp(),
    }
}

fn monthly_window(now: DateTime<Local>, start_day: i32) -> BudgetWindow {
    let day_clamped = start_day.clamp(1, 28);
    let year = now.year();
    let month = now.month();

    // 尝试当月 start_day；若今天日期 < start_day，则窗口实为上月 start_day 起。
    let (start_year, start_month) = if now.day() >= day_clamped as u32 {
        (year, month)
    } else {
        if month == 1 {
            (year - 1, 12u32)
        } else {
            (year, month - 1)
        }
    };

    let start_date = NaiveDate::from_ymd_opt(start_year, start_month, day_clamped as u32)
        .or_else(|| NaiveDate::from_ymd_opt(start_year, start_month, 1))
        .expect("valid date");
    let start_local = Local
        .from_local_datetime(&start_date.and_hms_opt(0, 0, 0).expect("midnight valid"))
        .single()
        .expect("midnight valid");

    // 下月同日；超月底时 chrono 自动归一化（如 1/31 + 1 月 → 2/28 或 3/3）。
    let end_date = start_date
        .checked_add_months(chrono::Months::new(1))
        .expect("adding one month does not overflow");
    let end_local = Local
        .from_local_datetime(&end_date.and_hms_opt(0, 0, 0).expect("midnight valid"))
        .single()
        .expect("midnight valid");

    BudgetWindow {
        start_sec: start_local.timestamp(),
        end_sec: end_local.timestamp(),
    }
}

fn weekday_from_int(d: i32) -> Option<Weekday> {
    match d {
        0 => Some(Weekday::Sun),
        1 => Some(Weekday::Mon),
        2 => Some(Weekday::Tue),
        3 => Some(Weekday::Wed),
        4 => Some(Weekday::Thu),
        5 => Some(Weekday::Fri),
        6 => Some(Weekday::Sat),
        _ => None,
    }
}

// ── 聚合查询 ────────────────────────────────────────────────────────

/// 在窗口内对 `proxy_request_logs` 求 real_total_tokens 与 total_cost_usd。
///
/// 复用 `effective_usage_log_filter` 去除 session_log 与 proxy 重复行。
fn aggregate_window(
    db: &Database,
    budget: &TokenBudget,
    window: BudgetWindow,
) -> Result<(u64, Decimal), AppError> {
    use crate::database::lock_conn;
    let conn = lock_conn!(db.conn);

    let fresh_input = fresh_input_sql("l");
    let dedup = effective_usage_log_filter("l");

    let mut conditions = vec![
        format!("l.created_at >= {}", window.start_sec),
        format!("l.created_at < {}", window.end_sec),
        dedup,
    ];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    match budget.scope {
        BudgetScope::Global => { /* 无 scope 过滤 */ }
        BudgetScope::App => {
            if let Some(v) = &budget.scope_value {
                conditions.push("l.app_type = ?".to_string());
                params.push(Box::new(v.clone()));
            }
        }
        BudgetScope::Provider => {
            // scope_value 格式: "app_type:provider_id"（与前端 BudgetEditor 写入格式一致）
            if let Some(v) = &budget.scope_value {
                if let Some((app_type, provider_id)) = v.split_once(':') {
                    conditions.push("l.app_type = ?".to_string());
                    params.push(Box::new(app_type.to_string()));
                    conditions.push("l.provider_id = ?".to_string());
                    params.push(Box::new(provider_id.to_string()));
                } else {
                    // 回退：旧数据没有 app_type 前缀时，仅按 provider_id 过滤
                    conditions.push("l.provider_id = ?".to_string());
                    params.push(Box::new(v.clone()));
                }
            }
        }
        BudgetScope::Model => {
            if let Some(v) = &budget.scope_value {
                conditions.push("l.model = ?".to_string());
                params.push(Box::new(v.clone()));
            }
        }
    }

    let where_clause = format!("WHERE {}", conditions.join(" AND "));

    let sql = format!(
        "SELECT
            COALESCE(SUM({fresh_input}), 0) + COALESCE(SUM(l.output_tokens), 0)
              + COALESCE(SUM(l.cache_creation_tokens), 0) + COALESCE(SUM(l.cache_read_tokens), 0),
            COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0)
         FROM proxy_request_logs l
         {where_clause}"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let (tokens, cost): (i64, f64) = conn
        .query_row(&sql, param_refs.as_slice(), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
        })
        .map_err(|e| AppError::Database(format!("聚合 token_budget 状态失败: {e}")))?;

    // f64 → Decimal 安全转换：成本字段保留 6 位
    let cost_dec = Decimal::from_str(&format!("{cost:.6}"))
        .unwrap_or(Decimal::ZERO);
    Ok((tokens.max(0) as u64, cost_dec))
}

// ── 校验 ──────────────────────────────────────────────────────────

fn validate_input(input: &CreateTokenBudgetInput) -> Result<(), AppError> {
    if input.name.trim().is_empty() {
        return Err(AppError::Message("预算名称不能为空".into()));
    }
    if matches!(input.scope, BudgetScope::Global) && input.scope_value.is_some() {
        return Err(AppError::Message(
            "全局预算不应指定 scope_value".into(),
        ));
    }
    // provider scope 的 scope_value 应为 "app_type:provider_id" 格式
    if matches!(input.scope, BudgetScope::Provider) {
        if let Some(ref v) = input.scope_value {
            if !v.contains(':') {
                return Err(AppError::Message(
                    "provider 预算的 scope_value 格式必须为 \"app_type:provider_id\"（如 \"claude:my-provider\"）".into(),
                ));
            }
        }
    }
    if !matches!(input.scope, BudgetScope::Global) && input.scope_value.as_deref().map_or(true, |s| s.trim().is_empty()) {
        return Err(AppError::Message(
            "非全局预算必须指定 scope_value".into(),
        ));
    }
    validate_period_start_day(input.period, input.period_start_day)?;
    validate_limits(input.limit_tokens, input.limit_usd.as_deref())?;
    Ok(())
}

fn validate_merged(merged: &TokenBudget) -> Result<(), AppError> {
    if merged.name.trim().is_empty() {
        return Err(AppError::Message("预算名称不能为空".into()));
    }
    if matches!(merged.scope, BudgetScope::Global) && merged.scope_value.is_some() {
        return Err(AppError::Message(
            "全局预算不应指定 scope_value".into(),
        ));
    }
    if matches!(merged.scope, BudgetScope::Provider) {
        if let Some(ref v) = merged.scope_value {
            if !v.contains(':') {
                return Err(AppError::Message(
                    "provider 预算的 scope_value 格式必须为 \"app_type:provider_id\"".into(),
                ));
            }
        }
    }
    if !matches!(merged.scope, BudgetScope::Global)
        && merged
            .scope_value
            .as_deref()
            .map_or(true, |s| s.trim().is_empty())
    {
        return Err(AppError::Message(
            "非全局预算必须指定 scope_value".into(),
        ));
    }
    validate_period_start_day(merged.period, merged.period_start_day)?;
    validate_limits(merged.limit_tokens, merged.limit_usd.as_deref())?;
    Ok(())
}

fn validate_period_start_day(period: BudgetPeriod, start_day: i32) -> Result<(), AppError> {
    match period {
        BudgetPeriod::Daily => Ok(()),
        BudgetPeriod::Weekly => {
            if !(0..=6).contains(&start_day) {
                Err(AppError::Message(
                    "weekly period_start_day 必须在 0..=6（周日=0）".into(),
                ))
            } else {
                Ok(())
            }
        }
        BudgetPeriod::Monthly => {
            if !(1..=28).contains(&start_day) {
                Err(AppError::Message(
                    "monthly period_start_day 必须在 1..=28（避免月末漂移）".into(),
                ))
            } else {
                Ok(())
            }
        }
    }
}

fn validate_limits(
    limit_tokens: Option<i64>,
    limit_usd: Option<&str>,
) -> Result<(), AppError> {
    if limit_tokens.is_none() && limit_usd.is_none() {
        return Err(AppError::Message(
            "必须至少设置 token 上限或 USD 上限之一".into(),
        ));
    }
    if let Some(t) = limit_tokens {
        if t <= 0 {
            return Err(AppError::Message("token 上限必须 > 0".into()));
        }
    }
    if let Some(u) = limit_usd {
        let d = Decimal::from_str(u)
            .map_err(|_| AppError::Message(format!("无法解析 USD 上限: {u}")))?;
        if d <= Decimal::ZERO {
            return Err(AppError::Message("USD 上限必须 > 0".into()));
        }
    }
    Ok(())
}

/// 把 patch 应用到 existing（仅用于校验），不动数据库。
fn merge_for_validation(
    existing: &TokenBudget,
    patch: &UpdateTokenBudgetInput,
) -> TokenBudget {
    let mut merged = existing.clone();
    if let Some(n) = &patch.name {
        merged.name = n.clone();
    }
    if let Some(s) = patch.scope {
        merged.scope = s;
    }
    if let Some(sv) = &patch.scope_value {
        merged.scope_value = sv.clone();
    }
    if let Some(p) = patch.period {
        merged.period = p;
    }
    if let Some(d) = patch.period_start_day {
        merged.period_start_day = d;
    }
    if let Some(t) = patch.limit_tokens {
        merged.limit_tokens = t;
    }
    if let Some(u) = &patch.limit_usd {
        merged.limit_usd = u.clone();
    }
    if let Some(e) = patch.enabled {
        merged.enabled = e;
    }
    merged
}

fn now_sec() -> i64 {
    chrono::Local::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(year: i32, month: u32, day: u32, hour: u32) -> i64 {
        Local
            .with_ymd_and_hms(year, month, day, hour, 0, 0)
            .unwrap()
            .timestamp()
    }

    #[test]
    fn daily_window_same_day() {
        let now = ts(2026, 6, 2, 14);
        let w = compute_period_window(BudgetPeriod::Daily, 1, now);
        let expected_start = ts(2026, 6, 2, 0);
        let expected_end = ts(2026, 6, 3, 0);
        assert_eq!(w.start_sec, expected_start);
        assert_eq!(w.end_sec, expected_end);
    }

    #[test]
    fn weekly_window_monday_start() {
        // 2026-06-03 is Wednesday; start_day=1 (Mon) → window starts 2026-06-01.
        let now = ts(2026, 6, 3, 10);
        let w = compute_period_window(BudgetPeriod::Weekly, 1, now);
        assert_eq!(w.start_sec, ts(2026, 6, 1, 0));
        assert_eq!(w.end_sec, ts(2026, 6, 8, 0));
    }

    #[test]
    fn weekly_window_sunday_start() {
        // 2026-06-03 Wed; start_day=0 (Sun) → window starts 2026-05-31.
        let now = ts(2026, 6, 3, 10);
        let w = compute_period_window(BudgetPeriod::Weekly, 0, now);
        assert_eq!(w.start_sec, ts(2026, 5, 31, 0));
        assert_eq!(w.end_sec, ts(2026, 6, 7, 0));
    }

    #[test]
    fn monthly_window_mid_month() {
        // start_day=1 → window = current month 1st ~ next month 1st.
        let now = ts(2026, 6, 15, 12);
        let w = compute_period_window(BudgetPeriod::Monthly, 1, now);
        assert_eq!(w.start_sec, ts(2026, 6, 1, 0));
        assert_eq!(w.end_sec, ts(2026, 7, 1, 0));
    }

    #[test]
    fn monthly_window_before_start_day_falls_back_to_prev_month() {
        // start_day=15; on June 5 → window = May 15 ~ June 15.
        let now = ts(2026, 6, 5, 12);
        let w = compute_period_window(BudgetPeriod::Monthly, 15, now);
        assert_eq!(w.start_sec, ts(2026, 5, 15, 0));
        assert_eq!(w.end_sec, ts(2026, 6, 15, 0));
    }

    #[test]
    fn monthly_window_clamps_to_28_to_avoid_month_end_drift() {
        // 31 is clamped to 28; on March 1 → window = Feb 28 ~ Mar 28.
        let now = ts(2026, 3, 1, 0);
        let w = compute_period_window(BudgetPeriod::Monthly, 31, now);
        assert_eq!(w.start_sec, ts(2026, 2, 28, 0));
        assert_eq!(w.end_sec, ts(2026, 3, 28, 0));
    }

    #[test]
    fn monthly_window_year_boundary() {
        // start_day=1; on 2027-01-15 → window = 2027-01-01 ~ 2027-02-01.
        let now = ts(2027, 1, 15, 0);
        let w = compute_period_window(BudgetPeriod::Monthly, 1, now);
        assert_eq!(w.start_sec, ts(2027, 1, 1, 0));
        assert_eq!(w.end_sec, ts(2027, 2, 1, 0));
    }

    #[test]
    fn monthly_window_prev_year_when_before_start_day_in_january() {
        // start_day=15; on 2026-01-05 → window = 2025-12-15 ~ 2026-01-15.
        let now = ts(2026, 1, 5, 0);
        let w = compute_period_window(BudgetPeriod::Monthly, 15, now);
        assert_eq!(w.start_sec, ts(2025, 12, 15, 0));
        assert_eq!(w.end_sec, ts(2026, 1, 15, 0));
    }

    #[test]
    fn validate_limits_requires_at_least_one() {
        assert!(validate_limits(None, None).is_err());
        assert!(validate_limits(Some(100), None).is_ok());
        assert!(validate_limits(None, Some("0.5")).is_ok());
        assert!(validate_limits(Some(100), Some("0.5")).is_ok());
    }

    #[test]
    fn validate_limits_rejects_non_positive() {
        assert!(validate_limits(Some(0), None).is_err());
        assert!(validate_limits(Some(-1), None).is_err());
        assert!(validate_limits(None, Some("0")).is_err());
        assert!(validate_limits(None, Some("-0.1")).is_err());
    }

    #[test]
    fn validate_period_rejects_out_of_range() {
        assert!(validate_period_start_day(BudgetPeriod::Weekly, 7).is_err());
        assert!(validate_period_start_day(BudgetPeriod::Weekly, -1).is_err());
        assert!(validate_period_start_day(BudgetPeriod::Monthly, 0).is_err());
        assert!(validate_period_start_day(BudgetPeriod::Monthly, 29).is_err());
        assert!(validate_period_start_day(BudgetPeriod::Weekly, 0).is_ok());
        assert!(validate_period_start_day(BudgetPeriod::Monthly, 28).is_ok());
    }
}
