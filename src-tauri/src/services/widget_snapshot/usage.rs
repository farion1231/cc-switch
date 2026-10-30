//! 小组件「用量」段：今天、近 7 天、近 30 天逐日、今天按应用拆分。直接读本地数据库，
//! 金额按设置里的显示币种换算（同前端 `formatMoney`）。

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone};
use serde::Serialize;

use crate::database::Database;
use crate::error::AppError;
use crate::services::usage_stats::UsageSummary;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetUsage {
    pub today: UsageTotals,
    pub last7_days: UsageTotals,
    /// 最近 30 天每天一条（`YYYY-MM-DD`，本地），旧 → 新，没有用量的日子也有。
    pub days: Vec<UsageDay>,
    /// 今天按应用拆分，按花费从高到低，没用过的不列。
    pub by_app: Vec<UsageByApp>,
    /// 用量页的六个范围（键同前端 preset：today / 1d / 7d / 14d / 30d / all），
    /// 小组件按用户在「编辑小组件」里选的范围显示。
    pub ranges: BTreeMap<&'static str, UsageRange>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    /// 带币种符号，如 "$12.34"
    pub cost: String,
    pub cost_value: f64,
    pub tokens: u64,
    pub requests: u64,
    pub cache_hit_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    pub date: String,
    pub cost_value: f64,
    pub tokens: u64,
    pub requests: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageByApp {
    pub app: String,
    pub name: String,
    pub cost: String,
    pub cost_value: f64,
    pub tokens: u64,
}

/// 显示币种：符号、1 美元折多少、保留几位小数。
#[derive(Debug, Clone, PartialEq)]
pub struct Money {
    symbol: &'static str,
    rate: f64,
    digits: usize,
}

impl Money {
    /// 照前端 `resolveMoneyDisplay`：不认得的币种按美元，汇率没填或不合法用预置值；
    /// 日元少两位小数。
    pub fn from_settings(display: Option<&crate::settings::UsageDisplaySettings>) -> Self {
        let currency = display.and_then(|d| d.currency.as_deref()).unwrap_or("USD");
        let (symbol, default_rate, digits) = match currency {
            "CNY" => ("¥", 6.8, 2),
            "EUR" => ("€", 0.87, 2),
            "JPY" => ("¥", 158.0, 0),
            "TWD" => ("NT$", 32.0, 2),
            "TRY" => ("₺", 48.0, 2),
            _ => {
                return Self {
                    symbol: "$",
                    rate: 1.0,
                    digits: 2,
                }
            }
        };
        let rate = display
            .and_then(|d| d.exchange_rates.get(currency).copied())
            .filter(|rate| rate.is_finite() && *rate > 0.0)
            .unwrap_or(default_rate);
        Self {
            symbol,
            rate,
            digits,
        }
    }

    pub fn convert(&self, usd: f64) -> f64 {
        usd * self.rate
    }

    pub fn format(&self, usd: f64) -> String {
        let fixed = format!("{:.*}", self.digits, self.convert(usd));
        let (whole, frac) = match fixed.split_once('.') {
            Some((whole, frac)) => (whole, Some(frac)),
            None => (fixed.as_str(), None),
        };
        let (sign, digits) = whole.strip_prefix('-').map_or(("", whole), |d| ("-", d));
        let mut grouped = String::new();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(ch);
        }
        match frac {
            Some(frac) => format!("{sign}{}{grouped}.{frac}", self.symbol),
            None => format!("{sign}{}{grouped}", self.symbol),
        }
    }
}

fn cost_of(text: &str) -> f64 {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

fn totals(summary: &UsageSummary, money: &Money) -> UsageTotals {
    let usd = cost_of(&summary.total_cost);
    UsageTotals {
        cost: money.format(usd),
        cost_value: money.convert(usd),
        tokens: summary.real_total_tokens,
        requests: summary.total_requests,
        cache_hit_rate: summary.cache_hit_rate,
    }
}

/// 本地某天 0 点的时间戳。
fn local_midnight(date: NaiveDate) -> i64 {
    let naive = date.and_time(NaiveTime::MIN);
    // 当地没有 0 点（夏令时从 0 点跳过）：和前端 `new Date(y, m, d)` 一样取跳变后第一个有效时刻
    (0..=3)
        .find_map(|hours| {
            Local
                .from_local_datetime(&(naive + Duration::hours(hours)))
                .earliest()
        })
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| naive.and_utc().timestamp())
}

fn local_date(ts: i64) -> NaiveDate {
    Local
        .timestamp_opt(ts, 0)
        .earliest()
        .map(|dt| dt.date_naive())
        .unwrap_or_default()
}

/// 用量页的时间范围（`src/lib/usageRange.ts` 的 preset，口径一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangePreset {
    Today,
    Last24h,
    Last7d,
    Last14d,
    Last30d,
    All,
}

impl RangePreset {
    pub const ALL: [RangePreset; 6] = [
        Self::Today,
        Self::Last24h,
        Self::Last7d,
        Self::Last14d,
        Self::Last30d,
        Self::All,
    ];

    /// 快照里的键，和前端 preset 同名。
    pub fn key(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Last24h => "1d",
            Self::Last7d => "7d",
            Self::Last14d => "14d",
            Self::Last30d => "30d",
            Self::All => "all",
        }
    }

    /// 起点（秒）。近 N 天按本地日历往前数（含今天），「全部」从最早一条记录那天算起。
    fn start(self, now_ts: i64, today: NaiveDate, first_day: Option<NaiveDate>) -> i64 {
        let days_back = |n: i64| local_midnight(today - Duration::days(n - 1));
        match self {
            Self::Today => local_midnight(today),
            Self::Last24h => now_ts - 24 * 60 * 60,
            Self::Last7d => days_back(7),
            Self::Last14d => days_back(14),
            Self::Last30d => days_back(30),
            Self::All => local_midnight(first_day.unwrap_or(today).min(today)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRange {
    pub totals: UsageTotals,
    /// 趋势每个点代表多长：`hour` / `day` / `week` / `month`
    pub granularity: &'static str,
    pub series: Vec<UsagePoint>,
    /// 按应用拆分，按花费从高到低，没用过的不列。
    pub by_app: Vec<UsageByApp>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePoint {
    /// 这一格的起点（Unix 秒，本地时间对齐）
    pub start: i64,
    pub cost_value: f64,
    pub tokens: u64,
    pub requests: u64,
}

/// 「全部」的天数多了，柱子太细：超过这么多天按周合并，再多按月。
const MAX_DAILY_POINTS: usize = 62;
const MAX_WEEKLY_POINTS: usize = 104;

/// 趋势一格的起点：按小时分桶时是 RFC 3339 时间，按天时是当地 0 点的 RFC 3339 或 `YYYY-MM-DD`。
fn bucket_start(date: &str) -> i64 {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date) {
        return dt.timestamp();
    }
    NaiveDate::parse_from_str(date.get(..10).unwrap_or(date), "%Y-%m-%d")
        .map(local_midnight)
        .unwrap_or_default()
}

/// 按天的点合并成按周（周一起）或按月。
fn regroup(points: Vec<UsagePoint>, granularity: &'static str) -> Vec<UsagePoint> {
    let key = |start: i64| {
        let date = local_date(start);
        match granularity {
            "week" => date - Duration::days(i64::from(date.weekday().num_days_from_monday())),
            _ => date.with_day(1).unwrap_or(date),
        }
    };
    let mut out: Vec<UsagePoint> = Vec::new();
    for point in points {
        let start = local_midnight(key(point.start));
        match out.last_mut() {
            Some(last) if last.start == start => {
                last.cost_value += point.cost_value;
                last.tokens += point.tokens;
                last.requests += point.requests;
            }
            _ => out.push(UsagePoint { start, ..point }),
        }
    }
    out
}

fn collect_range(
    db: &Database,
    money: &Money,
    preset: RangePreset,
    now_ts: i64,
    today: NaiveDate,
    first_day: Option<NaiveDate>,
) -> Result<UsageRange, AppError> {
    let start = preset.start(now_ts, today, first_day);
    let summary = db.get_usage_summary(Some(start), Some(now_ts), None, None, None)?;
    // 一天以内按小时分桶，更长按天（数据库那边按区间长短决定）
    let mut series: Vec<UsagePoint> = db
        .get_daily_trends(Some(start), Some(now_ts), None, None, None)?
        .into_iter()
        .map(|day| UsagePoint {
            start: bucket_start(&day.date),
            cost_value: money.convert(cost_of(&day.total_cost)),
            // 和合计同一口径：连缓存读写在内的全部 Token
            tokens: day.total_input_tokens
                + day.total_output_tokens
                + day.total_cache_creation_tokens
                + day.total_cache_read_tokens,
            requests: day.request_count,
        })
        .collect();
    let mut granularity = if now_ts - start <= 24 * 60 * 60 {
        "hour"
    } else {
        "day"
    };
    if granularity == "day" && series.len() > MAX_DAILY_POINTS {
        let weekly = regroup(series.clone(), "week");
        if weekly.len() > MAX_WEEKLY_POINTS {
            series = regroup(series, "month");
            granularity = "month";
        } else {
            series = weekly;
            granularity = "week";
        }
    }

    let mut by_app: Vec<UsageByApp> = db
        .get_usage_summary_by_app(Some(start), Some(now_ts), None, None)?
        .into_iter()
        .filter(|row| row.summary.total_requests > 0)
        .map(|row| {
            let usd = cost_of(&row.summary.total_cost);
            let name = row
                .app_type
                .parse::<crate::app_config::AppType>()
                .map(|app| crate::tray::app_display_name(&app).to_string())
                .unwrap_or_else(|_| row.app_type.clone());
            UsageByApp {
                app: row.app_type,
                name,
                cost: money.format(usd),
                cost_value: money.convert(usd),
                tokens: row.summary.real_total_tokens,
            }
        })
        .collect();
    by_app.sort_by(|a, b| b.cost_value.total_cmp(&a.cost_value));

    Ok(UsageRange {
        totals: totals(&summary, money),
        granularity,
        series,
        by_app,
    })
}

/// 同一份结果一分钟内复用：托盘状态一变就会写快照，用量不必每次都重查六个范围。
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(60);
static CACHE: std::sync::Mutex<Option<(std::time::Instant, Money, WidgetUsage)>> =
    std::sync::Mutex::new(None);

pub fn collect(db: &Database, money: &Money) -> Result<WidgetUsage, AppError> {
    {
        let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, cached_money, usage)) = cache.as_ref() {
            if at.elapsed() < CACHE_TTL && cached_money == money {
                return Ok(usage.clone());
            }
        }
    }
    let usage = collect_fresh(db, money)?;
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((std::time::Instant::now(), money.clone(), usage.clone()));
    Ok(usage)
}

fn collect_fresh(db: &Database, money: &Money) -> Result<WidgetUsage, AppError> {
    let now = Local::now();
    let now_ts = now.timestamp();
    let today = now.date_naive();
    let first_day = db
        .get_first_usage_date(None, None, None)?
        .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok());

    let mut ranges = BTreeMap::new();
    for preset in RangePreset::ALL {
        ranges.insert(
            preset.key(),
            collect_range(db, money, preset, now_ts, today, first_day)?,
        );
    }

    // 旧版小组件读的几段，从范围里取
    let today_range = &ranges[RangePreset::Today.key()];
    let month = &ranges[RangePreset::Last30d.key()];
    let days = month
        .series
        .iter()
        .map(|p| UsageDay {
            date: local_date(p.start).to_string(),
            cost_value: p.cost_value,
            tokens: p.tokens,
            requests: p.requests,
        })
        .collect();
    Ok(WidgetUsage {
        today: today_range.totals.clone(),
        last7_days: ranges[RangePreset::Last7d.key()].totals.clone(),
        days,
        by_app: today_range.by_app.clone(),
        ranges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::UsageDisplaySettings;

    #[test]
    fn money_follows_display_currency() {
        let usd = Money::from_settings(None);
        assert_eq!(usd.format(1234.5), "$1,234.50");
        assert_eq!(usd.format(0.0), "$0.00");

        let mut display = UsageDisplaySettings {
            currency: Some("CNY".to_string()),
            ..Default::default()
        };
        display.exchange_rates.insert("CNY".to_string(), 7.0);
        let cny = Money::from_settings(Some(&display));
        assert_eq!(cny.format(2.0), "¥14.00");
        assert_eq!(cny.convert(2.0), 14.0);

        display.currency = Some("JPY".to_string());
        let jpy = Money::from_settings(Some(&display));
        assert_eq!(jpy.format(10.0), "¥1,580");

        display.currency = Some("XYZ".to_string());
        assert_eq!(Money::from_settings(Some(&display)).format(1.0), "$1.00");
    }

    #[test]
    fn empty_database_still_yields_every_range() {
        let db = Database::memory().expect("memory db");
        let usage = collect_fresh(&db, &Money::from_settings(None)).expect("collect");
        assert_eq!(usage.days.len(), 30);
        assert_eq!(
            usage.days.last().map(|d| d.date.clone()),
            Some(Local::now().date_naive().to_string())
        );
        assert_eq!(usage.today.cost, "$0.00");
        assert!(usage.by_app.is_empty());

        let keys: Vec<_> = usage.ranges.keys().copied().collect();
        assert_eq!(keys, ["14d", "1d", "30d", "7d", "all", "today"]);
        assert_eq!(usage.ranges["today"].granularity, "hour");
        assert_eq!(usage.ranges["1d"].granularity, "hour");
        assert_eq!(usage.ranges["7d"].granularity, "day");
        assert_eq!(usage.ranges["7d"].series.len(), 7);
        assert_eq!(usage.ranges["30d"].series.len(), 30);
        // 没有记录时「全部」就是今天，和「当天」一样按小时
        assert_eq!(usage.ranges["all"].granularity, "hour");
        assert_eq!(
            usage.ranges["all"].series.len(),
            usage.ranges["today"].series.len()
        );
    }

    #[test]
    fn range_starts_follow_the_usage_page() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 30).unwrap();
        let now = local_midnight(today) + 15 * 3600;
        assert_eq!(
            RangePreset::Today.start(now, today, None),
            local_midnight(today)
        );
        assert_eq!(RangePreset::Last24h.start(now, today, None), now - 86_400);
        // 近 7 天含今天：从 6 天前的 0 点算起
        assert_eq!(
            RangePreset::Last7d.start(now, today, None),
            local_midnight(NaiveDate::from_ymd_opt(2026, 10, 24).unwrap())
        );
        let first = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        assert_eq!(
            RangePreset::All.start(now, today, Some(first)),
            local_midnight(first)
        );
    }

    #[test]
    fn long_history_regroups_by_week_and_month() {
        let day = |y, m, d, cost| UsagePoint {
            start: local_midnight(NaiveDate::from_ymd_opt(y, m, d).unwrap()),
            cost_value: cost,
            tokens: 1,
            requests: 1,
        };
        // 2026-10-26 是周一
        let weekly = regroup(
            vec![
                day(2026, 10, 25, 1.0),
                day(2026, 10, 26, 2.0),
                day(2026, 10, 27, 3.0),
            ],
            "week",
        );
        assert_eq!(weekly.len(), 2);
        assert_eq!(weekly[1].cost_value, 5.0);
        assert_eq!(weekly[1].requests, 2);
        let monthly = regroup(vec![day(2026, 9, 30, 1.0), day(2026, 10, 1, 2.0)], "month");
        assert_eq!(monthly.len(), 2);
        assert_eq!(
            monthly[1].start,
            local_midnight(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap())
        );
    }
}
