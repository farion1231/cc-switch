use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::DateTime;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::subscription::SubscriptionQuota;

const HISTORY_RETENTION_DAYS: i64 = 35;
const MINIMUM_SAMPLE_COUNT: usize = 3;
const MAX_SAMPLE_COUNT: usize = 7;
const FIVE_HOURS_MS: f64 = 5.0 * 60.0 * 60.0 * 1000.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaForecast {
    pub sample_count: usize,
    pub minimum_sample_count: usize,
    pub weekly_percent_per_five_hour: Option<f64>,
    pub estimated_five_hour_windows_remaining: Option<f64>,
    pub available_five_hour_windows_until_reset: Option<f64>,
    pub will_exhaust_before_reset: Option<bool>,
    pub weekly_resets_at: Option<String>,
}

#[derive(Debug, Clone)]
struct Snapshot {
    captured_at: i64,
    five_hour_utilization: f64,
    five_hour_resets_at: i64,
    seven_day_utilization: f64,
    seven_day_resets_at: i64,
}

pub fn record_and_forecast<'a>(
    db: &Database,
    quotas: impl IntoIterator<Item = (&'a String, &'a SubscriptionQuota)>,
) -> Result<HashMap<String, CodexQuotaForecast>, AppError> {
    let quotas = quotas.into_iter().collect::<Vec<_>>();
    let now = now_millis();
    let conn = lock_conn!(db.conn);
    conn.execute(
        "DELETE FROM codex_quota_history WHERE captured_at < ?1",
        params![now - HISTORY_RETENTION_DAYS * 24 * 60 * 60 * 1000],
    )
    .map_err(|error| AppError::Database(format!("清理 Codex 额度历史失败: {error}")))?;

    for (account_key, quota) in &quotas {
        let Some(snapshot) = snapshot_from_quota(quota, now) else {
            continue;
        };
        conn.execute(
            "INSERT OR REPLACE INTO codex_quota_history (
                account_key, captured_at, five_hour_utilization, five_hour_resets_at,
                seven_day_utilization, seven_day_resets_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                *account_key,
                snapshot.captured_at,
                snapshot.five_hour_utilization,
                snapshot.five_hour_resets_at,
                snapshot.seven_day_utilization,
                snapshot.seven_day_resets_at,
            ],
        )
        .map_err(|error| AppError::Database(format!("保存 Codex 额度历史失败: {error}")))?;
    }

    let mut forecasts = HashMap::new();
    for (account_key, quota) in quotas {
        let Some(current) = snapshot_from_quota(quota, now) else {
            continue;
        };
        let mut statement = conn
            .prepare(
                "SELECT captured_at, five_hour_utilization, five_hour_resets_at,
                        seven_day_utilization, seven_day_resets_at
                 FROM codex_quota_history
                 WHERE account_key = ?1 AND seven_day_resets_at = ?2
                 ORDER BY captured_at ASC",
            )
            .map_err(|error| AppError::Database(format!("查询 Codex 额度历史失败: {error}")))?;
        let snapshots = statement
            .query_map(params![account_key, current.seven_day_resets_at], |row| {
                Ok(Snapshot {
                    captured_at: row.get(0)?,
                    five_hour_utilization: row.get(1)?,
                    five_hour_resets_at: row.get(2)?,
                    seven_day_utilization: row.get(3)?,
                    seven_day_resets_at: row.get(4)?,
                })
            })
            .map_err(|error| AppError::Database(format!("读取 Codex 额度历史失败: {error}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::Database(format!("解析 Codex 额度历史失败: {error}")))?;
        forecasts.insert(
            account_key.clone(),
            calculate_forecast(&snapshots, &current, now),
        );
    }
    Ok(forecasts)
}

pub fn load_forecasts(db: &Database) -> Result<HashMap<String, CodexQuotaForecast>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut statement = conn
        .prepare("SELECT DISTINCT account_key FROM codex_quota_history")
        .map_err(|error| AppError::Database(format!("查询 Codex 额度账号失败: {error}")))?;
    let account_keys = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| AppError::Database(format!("读取 Codex 额度账号失败: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Database(format!("解析 Codex 额度账号失败: {error}")))?;
    drop(statement);

    let now = now_millis();
    let mut forecasts = HashMap::new();
    for account_key in account_keys {
        let mut statement = conn
            .prepare(
                "SELECT captured_at, five_hour_utilization, five_hour_resets_at,
                        seven_day_utilization, seven_day_resets_at
                 FROM codex_quota_history WHERE account_key = ?1
                 ORDER BY captured_at ASC",
            )
            .map_err(|error| AppError::Database(format!("查询 Codex 额度历史失败: {error}")))?;
        let snapshots = statement
            .query_map(params![account_key], |row| {
                Ok(Snapshot {
                    captured_at: row.get(0)?,
                    five_hour_utilization: row.get(1)?,
                    five_hour_resets_at: row.get(2)?,
                    seven_day_utilization: row.get(3)?,
                    seven_day_resets_at: row.get(4)?,
                })
            })
            .map_err(|error| AppError::Database(format!("读取 Codex 额度历史失败: {error}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::Database(format!("解析 Codex 额度历史失败: {error}")))?;
        let Some(current) = snapshots.last().cloned() else {
            continue;
        };
        let current_week = snapshots
            .into_iter()
            .filter(|snapshot| snapshot.seven_day_resets_at == current.seven_day_resets_at)
            .collect::<Vec<_>>();
        forecasts.insert(
            account_key,
            calculate_forecast(&current_week, &current, now),
        );
    }
    Ok(forecasts)
}

fn snapshot_from_quota(quota: &SubscriptionQuota, captured_at: i64) -> Option<Snapshot> {
    if !quota.success {
        return None;
    }
    let five_hour = quota.tiers.iter().find(|tier| tier.name == "five_hour")?;
    let seven_day = quota.tiers.iter().find(|tier| tier.name == "seven_day")?;
    Some(Snapshot {
        captured_at,
        five_hour_utilization: five_hour.utilization,
        five_hour_resets_at: parse_timestamp_millis(five_hour.resets_at.as_deref()?)?,
        seven_day_utilization: seven_day.utilization,
        seven_day_resets_at: parse_timestamp_millis(seven_day.resets_at.as_deref()?)?,
    })
}

fn calculate_forecast(snapshots: &[Snapshot], current: &Snapshot, now: i64) -> CodexQuotaForecast {
    if current.seven_day_resets_at <= now {
        return CodexQuotaForecast {
            sample_count: 0,
            minimum_sample_count: MINIMUM_SAMPLE_COUNT,
            weekly_percent_per_five_hour: None,
            estimated_five_hour_windows_remaining: None,
            available_five_hour_windows_until_reset: Some(0.0),
            will_exhaust_before_reset: None,
            weekly_resets_at: DateTime::from_timestamp_millis(current.seven_day_resets_at)
                .map(|value| value.to_rfc3339()),
        };
    }
    let mut windows: BTreeMap<i64, Vec<&Snapshot>> = BTreeMap::new();
    for snapshot in snapshots {
        if snapshot.five_hour_resets_at <= now {
            windows
                .entry(snapshot.five_hour_resets_at)
                .or_default()
                .push(snapshot);
        }
    }

    let mut burns = windows
        .into_values()
        .filter_map(|window| {
            let first = window.first()?;
            let last = window.last()?;
            let session_delta = last.five_hour_utilization - first.five_hour_utilization;
            let weekly_delta = last.seven_day_utilization - first.seven_day_utilization;
            if session_delta < 5.0 || weekly_delta <= 0.0 {
                return None;
            }
            let normalized = 100.0 * weekly_delta / session_delta;
            normalized.is_finite().then_some(normalized)
        })
        .collect::<Vec<_>>();
    if burns.len() > MAX_SAMPLE_COUNT {
        burns.drain(..burns.len() - MAX_SAMPLE_COUNT);
    }

    let sample_count = burns.len();
    let available = ((current.seven_day_resets_at - now).max(0) as f64) / FIVE_HOURS_MS;
    let median = (sample_count >= MINIMUM_SAMPLE_COUNT).then(|| {
        burns.sort_by(f64::total_cmp);
        median(&burns)
    });
    let estimated = median.map(|burn| (100.0 - current.seven_day_utilization).max(0.0) / burn);

    CodexQuotaForecast {
        sample_count,
        minimum_sample_count: MINIMUM_SAMPLE_COUNT,
        weekly_percent_per_five_hour: median,
        estimated_five_hour_windows_remaining: estimated,
        available_five_hour_windows_until_reset: Some(available),
        will_exhaust_before_reset: estimated.map(|value| value < available),
        weekly_resets_at: DateTime::from_timestamp_millis(current.seven_day_resets_at)
            .map(|value| value.to_rfc3339()),
    }
}

fn median(values: &[f64]) -> f64 {
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn parse_timestamp_millis(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp_millis())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(captured_at: i64, reset: i64, five: f64, seven: f64) -> Snapshot {
        Snapshot {
            captured_at,
            five_hour_utilization: five,
            five_hour_resets_at: reset,
            seven_day_utilization: seven,
            seven_day_resets_at: 200_000_000,
        }
    }

    #[test]
    fn waits_for_three_completed_windows() {
        let rows = vec![
            snapshot(1, 10_000, 10.0, 10.0),
            snapshot(2, 10_000, 90.0, 14.0),
            snapshot(3, 20_000, 10.0, 14.0),
            snapshot(4, 20_000, 90.0, 18.0),
        ];
        let current = snapshot(30_000, 40_000, 10.0, 18.0);
        let forecast = calculate_forecast(&rows, &current, 30_000);
        assert_eq!(forecast.sample_count, 2);
        assert!(forecast.weekly_percent_per_five_hour.is_none());
    }

    #[test]
    fn normalizes_weekly_burn_to_full_five_hour_windows() {
        let mut rows = Vec::new();
        for (index, reset) in [10_000, 20_000, 30_000].into_iter().enumerate() {
            rows.push(snapshot(
                index as i64 * 2,
                reset,
                20.0,
                10.0 + index as f64 * 4.0,
            ));
            rows.push(snapshot(
                index as i64 * 2 + 1,
                reset,
                80.0,
                13.0 + index as f64 * 4.0,
            ));
        }
        let current = snapshot(40_000, 50_000, 5.0, 22.0);
        let forecast = calculate_forecast(&rows, &current, 40_000);
        assert_eq!(forecast.sample_count, 3);
        assert_eq!(forecast.weekly_percent_per_five_hour, Some(5.0));
        assert_eq!(forecast.estimated_five_hour_windows_remaining, Some(15.6));
    }

    #[test]
    fn expired_weekly_snapshot_never_produces_a_stale_forecast() {
        let current = snapshot(1, 10_000, 50.0, 50.0);
        let forecast = calculate_forecast(&[], &current, 200_000_001);
        assert_eq!(forecast.sample_count, 0);
        assert!(forecast.estimated_five_hour_windows_remaining.is_none());
        assert!(forecast.will_exhaust_before_reset.is_none());
    }
}
