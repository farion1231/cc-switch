//! 分时段（峰谷）模型定价规则
//!
//! 背景：DeepSeek 等供应商按「本地时段」收不同单价，只用一条全局单价算出来的
//! 成本会偏离真实账单。规则只回答两个问题：在什么本地时刻生效、生效时用什么价格。
//!
//! 时区用**固定 UTC 偏移**表达（不引 chrono-tz，因此不处理夏令时）：供应商的峰谷
//! 时段是按它自己的本地时钟切的，固定偏移已经够表达，且规则在本地时间里保持稳定。
//!
//! 计价语义与 issue 的约定一致：**按请求开始时刻取档**，跨档的请求整体归到开始
//! 时刻那一档（见 `logger::log_with_calculation`）。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::model_pricing::normalize_decimal;
use chrono::{Datelike, FixedOffset, NaiveDate, TimeZone, Timelike, Weekday};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// 周一至周日全选
const WEEKDAY_MASK_ALL: u32 = 0b0111_1111;
/// 一天的分钟数
const MINUTES_PER_DAY: u32 = 24 * 60;
/// 开始分钟上限：23:59
const MAX_START_MINUTE: u32 = MINUTES_PER_DAY - 1;
/// 结束分钟上限：1440 = 次日零点
const MAX_END_MINUTE: u32 = MINUTES_PER_DAY;
/// UTC 偏移绝对值上限：±23:59（chrono::FixedOffset 的硬上限是 ±23:59:59）
const MAX_UTC_OFFSET_MINUTES: u32 = MINUTES_PER_DAY - 1;

/// 诊断输出里的星期缩写，索引 0 = 周一（与 `weekday_mask` 的位序一致）
const WEEKDAY_ABBR: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// 单条分时段定价规则：本地时段命中时，用这里的四个价格**整体替换**基准价。
///
/// 规则是用户私有数据：`model-pricing.json` 是 SSOT，`model_pricing_time_rule`
/// 只是它的派生缓存（每次启动整表重建）。models.dev 同步只写 `model_pricing`，
/// 不碰本表。
///
/// 本表**会**进云同步快照——它不在 `backup::SYNC_SKIP_TABLES` 里，所以换机恢复
/// 能把另一台机器的规则带过来。但下一次启动时本地 JSON sidecar 会整表覆盖它：
/// 本地 JSON 里没有的规则会被清掉。与 `model_pricing` 相比后果更重——后者 JSON
/// 只存覆盖项、库存基准价，缺项是优雅降级；这里是整体丢失。这是「JSON 即 SSOT」
/// 这一设计的已知取舍，不是缺陷，但恢复快照后要留在本机就得把规则重新存一次。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingTimeRule {
    /// 规则标识；保存时由服务端补 uuid v4，前端可留空
    pub id: String,
    /// 只与该字段**完全相等**的计价模型才命中，不做前缀匹配
    /// （候选模型的生成规则见 `usage_stats::model_pricing_candidates`）
    pub model_id: String,
    /// 自由文本备注，可为空
    pub label: String,
    /// 固定 UTC 偏移（分钟），-1439..=1439
    pub utc_offset_minutes: i32,
    /// 位掩码，bit0 = 周一 … bit6 = 周日
    pub weekday_mask: u32,
    /// 本地零点的分钟数，0..=1439
    pub start_minute: u32,
    /// 本地零点的分钟数，0..=1440（1440 = 次日零点）
    pub end_minute: u32,
    /// 命中也作废的本地日期（`YYYY-MM-DD`），例如春节额外放假
    pub excluded_dates: Vec<String>,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
}

/// 周几对应的掩码位：周一 = bit0，周日 = bit6
pub const fn weekday_bit(weekday: Weekday) -> u32 {
    match weekday {
        Weekday::Mon => 1 << 0,
        Weekday::Tue => 1 << 1,
        Weekday::Wed => 1 << 2,
        Weekday::Thu => 1 << 3,
        Weekday::Fri => 1 << 4,
        Weekday::Sat => 1 << 5,
        Weekday::Sun => 1 << 6,
    }
}

/// 分钟是否落在窗口内。窗口左闭右开；`start == end` 表示全天。
fn minute_in_window(minute: u32, start: u32, end: u32) -> bool {
    match start.cmp(&end) {
        Ordering::Less => minute >= start && minute < end,
        // 跨午夜窗口（22:00 -> 08:00）：命中「不早于 22:00」或「早于 08:00」
        Ordering::Greater => minute >= start || minute < end,
        // 起止相同 = 全天
        Ordering::Equal => true,
    }
}

/// 本地时刻是否命中规则。纯函数：不读时钟、不碰 IO，便于测试。
fn matches(
    rule: &ModelPricingTimeRule,
    local_weekday: Weekday,
    local_minute: u32,
    local_date: NaiveDate,
) -> bool {
    if rule.weekday_mask & weekday_bit(local_weekday) == 0 {
        return false;
    }
    if !minute_in_window(local_minute, rule.start_minute, rule.end_minute) {
        return false;
    }
    // NaiveDate 的 Display 就是 %Y-%m-%d，与 normalize_excluded_dates 的归一化形式一致
    !rule.excluded_dates.contains(&local_date.to_string())
}

/// 把 unix 秒换算成规则本地时区里的「周几 / 当日分钟 / 本地日期」。
///
/// 偏移越界或时间戳溢出时返回 `None`；归一化已保证偏移在
/// `FixedOffset` 的接受范围内，因此正常输入不会走到 `None`。
pub fn local_parts(offset_minutes: i32, timestamp_secs: i64) -> Option<(Weekday, u32, NaiveDate)> {
    let offset = FixedOffset::east_opt(offset_minutes.saturating_mul(60))?;
    let local = offset.timestamp_opt(timestamp_secs, 0).single()?;
    Some((
        local.weekday(),
        local.hour() * 60 + local.minute(),
        local.date_naive(),
    ))
}

/// 选出 `timestamp_secs` 时刻生效的规则。
///
/// **调用方负责排序**：`rules` 必须已按优先级排好（数组顺序即 `sort_order`，
/// DB 读取时用 `sort_order ASC, rule_id ASC` 兜底），这里只返回第一个命中项。
/// 候选模型按 `candidates` 的顺序逐个尝试，前面的候选优先于后面的；
/// 同一候选内按 `rules` 的顺序取第一个命中项。
pub fn resolve_time_rule<'a>(
    rules: &'a [ModelPricingTimeRule],
    candidates: &[String],
    timestamp_secs: i64,
) -> Option<&'a ModelPricingTimeRule> {
    for candidate in candidates {
        for rule in rules {
            if rule.model_id != *candidate {
                continue;
            }
            let parts = local_parts(rule.utc_offset_minutes, timestamp_secs);
            let Some((weekday, minute, date)) = parts else {
                continue;
            };
            if matches(rule, weekday, minute, date) {
                return Some(rule);
            }
        }
    }
    None
}

/// 归一化排除日期：逐条解析成 `NaiveDate` 后按**规范化字符串**去重排序。
///
/// 存 `parsed.to_string()` 而不是原文：`2026-5-1` 也能被 chrono 解析，
/// 但那样存下来的字符串永远匹配不上 `NaiveDate` 的 Display 输出。
fn normalize_excluded_dates(values: &[String], index: usize) -> Result<Vec<String>, AppError> {
    let mut dates = BTreeSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").map_err(|error| {
            AppError::localized(
                "usage.timeRuleExcludedDateInvalid",
                format!(
                    "第 {} 条规则的排除日期格式必须为 YYYY-MM-DD: {trimmed} - {error}",
                    index + 1
                ),
                format!(
                    "Time rule {} excluded date must be YYYY-MM-DD: {trimmed}",
                    index + 1
                ),
            )
        })?;
        dates.insert(parsed.to_string());
    }
    Ok(dates.into_iter().collect())
}

/// 归一化单条规则：裁剪空白、补 `id`、校验取值。
///
/// `index` 只用于错误信息定位（数组下标，从 0 开始）。
pub fn normalize_time_rule(
    rule: ModelPricingTimeRule,
    index: usize,
) -> Result<ModelPricingTimeRule, AppError> {
    let model_id = rule.model_id.trim().to_string();
    if model_id.is_empty() {
        return Err(AppError::localized(
            "usage.modelIdRequired",
            format!("第 {} 条分时段规则的模型 ID 不能为空", index + 1),
            format!("Model ID is required for time rule {}", index + 1),
        ));
    }

    // 偏移先按 ±23:59 收紧（用户可见的上限），再按 FixedOffset 的硬上限兜底：
    // 后者越界会让 local_parts 返回 None，规则被静默跳过。
    let offset_secs = rule.utc_offset_minutes.saturating_mul(60);
    if rule.utc_offset_minutes.unsigned_abs() > MAX_UTC_OFFSET_MINUTES
        || FixedOffset::east_opt(offset_secs).is_none()
    {
        return Err(AppError::localized(
            "usage.timeRuleOffsetInvalid",
            format!(
                "第 {} 条规则的时区偏移必须在 -23:59 ~ +23:59 之间: {}",
                index + 1,
                rule.utc_offset_minutes
            ),
            format!(
                "Time rule {} has a UTC offset outside -23:59 ~ +23:59: {}",
                index + 1,
                rule.utc_offset_minutes
            ),
        ));
    }

    if rule.weekday_mask == 0 || rule.weekday_mask > WEEKDAY_MASK_ALL {
        return Err(AppError::localized(
            "usage.timeRuleWeekdayRequired",
            format!(
                "第 {} 条规则至少要选择一天，且只能在周一至周日之间选择",
                index + 1
            ),
            format!("Time rule {} must select at least one weekday", index + 1),
        ));
    }

    if rule.start_minute > MAX_START_MINUTE || rule.end_minute > MAX_END_MINUTE {
        return Err(AppError::localized(
            "usage.timeRuleWindowInvalid",
            format!(
                "第 {} 条规则的开始/结束时间无效: {} -> {}",
                index + 1,
                rule.start_minute,
                rule.end_minute
            ),
            format!(
                "Time rule {} has an invalid start/end minute: {} -> {}",
                index + 1,
                rule.start_minute,
                rule.end_minute
            ),
        ));
    }

    let id = rule.id.trim();
    Ok(ModelPricingTimeRule {
        id: if id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            id.to_string()
        },
        model_id,
        label: rule.label.trim().to_string(),
        utc_offset_minutes: rule.utc_offset_minutes,
        weekday_mask: rule.weekday_mask,
        start_minute: rule.start_minute,
        end_minute: rule.end_minute,
        excluded_dates: normalize_excluded_dates(&rule.excluded_dates, index)?,
        input_cost_per_million: normalize_decimal("input_cost", &rule.input_cost_per_million)?,
        output_cost_per_million: normalize_decimal("output_cost", &rule.output_cost_per_million)?,
        cache_read_cost_per_million: normalize_decimal(
            "cache_read_cost",
            &rule.cache_read_cost_per_million,
        )?,
        cache_creation_cost_per_million: normalize_decimal(
            "cache_creation_cost",
            &rule.cache_creation_cost_per_million,
        )?,
    })
}

/// 把 `weekday_mask` 渲染成诊断用摘要，连续的日子折叠成区间。
fn describe_weekdays(mask: u32) -> String {
    if mask == WEEKDAY_MASK_ALL {
        return "Every day".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut bit = 0usize;
    while bit < WEEKDAY_ABBR.len() {
        if mask & (1 << bit) == 0 {
            bit += 1;
            continue;
        }
        let start = bit;
        while bit + 1 < WEEKDAY_ABBR.len() && mask & (1 << (bit + 1)) != 0 {
            bit += 1;
        }
        parts.push(if start == bit {
            WEEKDAY_ABBR[start].to_string()
        } else {
            format!("{}-{}", WEEKDAY_ABBR[start], WEEKDAY_ABBR[bit])
        });
        bit += 1;
    }
    if parts.is_empty() {
        "Never".to_string()
    } else {
        parts.join(",")
    }
}

/// 分钟数渲染成 `HH:MM`；1440 渲染成 `24:00`（= 次日零点）。
fn describe_minute(minute: u32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// 偏移渲染成 `UTC+08:00` / `UTC-05:00`。
fn describe_offset(offset_minutes: i32) -> String {
    let sign = if offset_minutes < 0 { "-" } else { "+" };
    let absolute = offset_minutes.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", absolute / 60, absolute % 60)
}

/// 人类可读的时段摘要，用于日志与诊断，例如
/// `Mon-Fri 01:00-04:00 (UTC+00:00)`。
pub fn describe_window(rule: &ModelPricingTimeRule) -> String {
    // 起止相同写 "All day"：写成 00:00-00:00 会让人以为窗口是空的
    let window = if rule.start_minute == rule.end_minute {
        "All day".to_string()
    } else {
        format!(
            "{}-{}",
            describe_minute(rule.start_minute),
            describe_minute(rule.end_minute)
        )
    };
    format!(
        "{} {} ({})",
        describe_weekdays(rule.weekday_mask),
        window,
        describe_offset(rule.utc_offset_minutes)
    )
}

/// 用 `rules` 整体替换 `model_pricing_time_rule` 表。
///
/// 表由用户私有数据独占（只有 JSON sidecar 会写它），所以「先清空再逐条插入」
/// 比 model_pricing 那套 upsert + 墓碑的组合更简单也更正确。整批放在一个事务里，
/// 中途失败不会留下「换了半张表」的中间状态。
///
/// **不触发 `backfill_missing_usage_costs()`**：历史用量行记的是写入时刻实际生效的
/// 那一档价格，改规则是「以后怎么算」的决定，不应重算已经记过的账。
pub(crate) fn replace_time_rules_in_database(
    db: &Database,
    rules: &[ModelPricingTimeRule],
) -> Result<usize, AppError> {
    let mut conn = lock_conn!(db.conn);
    let transaction = conn.transaction()?;
    transaction.execute("DELETE FROM model_pricing_time_rule", [])?;
    for (sort_order, rule) in rules.iter().enumerate() {
        transaction.execute(
            "INSERT INTO model_pricing_time_rule (
                rule_id, model_id, label, utc_offset_minutes, weekday_mask,
                start_minute, end_minute, excluded_dates,
                input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million, sort_order
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                rule.id,
                rule.model_id,
                rule.label,
                rule.utc_offset_minutes,
                rule.weekday_mask,
                rule.start_minute,
                rule.end_minute,
                rule.excluded_dates.join(","),
                rule.input_cost_per_million,
                rule.output_cost_per_million,
                rule.cache_read_cost_per_million,
                rule.cache_creation_cost_per_million,
                sort_order as i64,
            ],
        )?;
    }
    transaction.commit()?;
    Ok(rules.len())
}

/// 读取全部规则，按 `sort_order ASC, rule_id ASC` 排序（即 UI 的展示顺序）。
pub(crate) fn read_model_pricing_time_rules(
    db: &Database,
) -> Result<Vec<ModelPricingTimeRule>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut stmt = conn.prepare(
        "SELECT rule_id, model_id, label, utc_offset_minutes, weekday_mask,
                start_minute, end_minute, excluded_dates,
                input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing_time_rule
         ORDER BY sort_order ASC, rule_id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ModelPricingTimeRule {
            id: row.get(0)?,
            model_id: row.get(1)?,
            label: row.get(2)?,
            utc_offset_minutes: row.get(3)?,
            // 与 dao/proxy.rs 一致：整数列按 i32 读出来再转，不依赖 u32 的 FromSql
            weekday_mask: row.get::<_, i32>(4)? as u32,
            start_minute: row.get::<_, i32>(5)? as u32,
            end_minute: row.get::<_, i32>(6)? as u32,
            excluded_dates: split_excluded_dates(&row.get::<_, String>(7)?),
            input_cost_per_million: row.get(8)?,
            output_cost_per_million: row.get(9)?,
            cache_read_cost_per_million: row.get(10)?,
            cache_creation_cost_per_million: row.get(11)?,
        })
    })?;
    let mut rules = Vec::new();
    for row in rows {
        rules.push(row?);
    }
    Ok(rules)
}

/// 读取候选模型名命中的规则，按 `sort_order ASC, rule_id ASC` 排序返回。
///
/// 表是用户私有的小表（通常为空或几条），所以一次读全表再在内存里按
/// `candidates` 过滤，比拼动态 `IN (...)` 更简单；顺序由 SQL 的 ORDER BY 决定。
pub(crate) fn load_time_rules_for_candidates(
    db: &Database,
    candidates: &[String],
) -> Result<Vec<ModelPricingTimeRule>, AppError> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let rules = read_model_pricing_time_rules(db)?;
    Ok(rules
        .into_iter()
        .filter(|rule| candidates.contains(&rule.model_id))
        .collect())
}

/// 逗号分隔的排除日期还原成 Vec；库里的形式由 `replace_time_rules_in_database` 写入。
fn split_excluded_dates(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-03-01T00:00:00Z：周日，UTC 当天第 0 分钟
    const SUNDAY_MIDNIGHT_UTC: i64 = 1_772_323_200;

    fn date(raw: &str) -> NaiveDate {
        NaiveDate::parse_from_str(raw, "%Y-%m-%d").expect("valid date")
    }

    fn sample_rule() -> ModelPricingTimeRule {
        ModelPricingTimeRule {
            id: "rule-1".to_string(),
            model_id: "deepseek-chat".to_string(),
            label: "DeepSeek 错峰".to_string(),
            utc_offset_minutes: 0,
            weekday_mask: WEEKDAY_MASK_ALL,
            // DeepSeek 的错峰优惠窗口：00:30 - 08:30 UTC
            start_minute: 30,
            end_minute: 510,
            excluded_dates: Vec::new(),
            input_cost_per_million: "0.2".to_string(),
            output_cost_per_million: "0.8".to_string(),
            cache_read_cost_per_million: "0.02".to_string(),
            cache_creation_cost_per_million: "0".to_string(),
        }
    }

    #[test]
    fn window_matches_inside_and_excludes_the_end_boundary() {
        let rule = sample_rule();
        let day = date("2026-03-01");

        assert!(matches(&rule, Weekday::Mon, 30, day));
        assert!(matches(&rule, Weekday::Mon, 509, day));
        // 左闭右开：整点命中开始，结束分钟本身不命中
        assert!(!matches(&rule, Weekday::Mon, 510, day));
        assert!(!matches(&rule, Weekday::Mon, 29, day));
    }

    #[test]
    fn window_wraps_past_midnight() {
        let mut rule = sample_rule();
        rule.start_minute = 22 * 60;
        rule.end_minute = 8 * 60;
        let day = date("2026-03-01");

        assert!(matches(&rule, Weekday::Mon, 0, day));
        assert!(matches(&rule, Weekday::Mon, 8 * 60 - 1, day));
        assert!(!matches(&rule, Weekday::Mon, 8 * 60, day));
        assert!(matches(&rule, Weekday::Mon, 22 * 60, day));
        assert!(matches(&rule, Weekday::Mon, 23 * 60 + 59, day));
        assert!(!matches(&rule, Weekday::Mon, 12 * 60, day));
    }

    #[test]
    fn window_with_equal_start_and_end_covers_the_whole_day() {
        let mut rule = sample_rule();
        rule.start_minute = 300;
        rule.end_minute = 300;

        for minute in [0, 299, 300, 1439] {
            assert!(matches(&rule, Weekday::Mon, minute, date("2026-03-01")));
        }
    }

    #[test]
    fn single_minute_window_matches_exactly_one_minute() {
        let mut rule = sample_rule();
        rule.start_minute = 100;
        rule.end_minute = 101;

        assert!(!matches(&rule, Weekday::Mon, 99, date("2026-03-01")));
        assert!(matches(&rule, Weekday::Mon, 100, date("2026-03-01")));
        assert!(!matches(&rule, Weekday::Mon, 101, date("2026-03-01")));
    }

    #[test]
    fn weekday_mask_selects_a_single_day() {
        let mut rule = sample_rule();
        rule.start_minute = 0;
        rule.end_minute = 1440;
        rule.weekday_mask = weekday_bit(Weekday::Wed);

        assert!(!matches(&rule, Weekday::Tue, 720, date("2026-03-01")));
        assert!(matches(&rule, Weekday::Wed, 720, date("2026-03-01")));
        assert!(!matches(&rule, Weekday::Thu, 720, date("2026-03-01")));
    }

    #[test]
    fn weekday_mask_mon_to_fri_excludes_the_weekend() {
        let mut rule = sample_rule();
        rule.start_minute = 0;
        rule.end_minute = 1440;
        // bit0 = 周一 … bit4 = 周五
        rule.weekday_mask = 0b0001_1111;

        for weekday in [Weekday::Mon, Weekday::Fri] {
            assert!(matches(&rule, weekday, 720, date("2026-03-01")));
        }
        for weekday in [Weekday::Sat, Weekday::Sun] {
            assert!(!matches(&rule, weekday, 720, date("2026-03-01")));
        }
    }

    #[test]
    fn weekday_mask_all_matches_every_day() {
        let mut rule = sample_rule();
        rule.start_minute = 0;
        rule.end_minute = 1440;

        for weekday in [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ] {
            assert!(matches(&rule, weekday, 720, date("2026-03-01")));
        }
    }

    #[test]
    fn local_parts_shifts_weekday_and_date_with_the_offset() {
        // UTC：2026-03-01 是周日，当天第 0 分钟
        assert_eq!(
            local_parts(0, SUNDAY_MIDNIGHT_UTC),
            Some((Weekday::Sun, 0, date("2026-03-01")))
        );
        // UTC+08:00：同一天推到 08:00，日期和周几都不变
        assert_eq!(
            local_parts(8 * 60, SUNDAY_MIDNIGHT_UTC),
            Some((Weekday::Sun, 8 * 60, date("2026-03-01")))
        );
        // UTC-05:00：回退到 2 月 28 日 19:00——本地日期和周几都异于 UTC，
        // 这是只有靠偏移才存在的档位
        assert_eq!(
            local_parts(-5 * 60, SUNDAY_MIDNIGHT_UTC),
            Some((Weekday::Sat, 19 * 60, date("2026-02-28")))
        );
    }

    #[test]
    fn local_parts_rejects_an_offset_fixedoffset_cannot_represent() {
        // 远超出 FixedOffset 的 ±24h 上限，返回 None 而不是panic
        assert_eq!(local_parts(1_000_000, 0), None);
    }

    #[test]
    fn excluded_date_wins_over_weekday_and_window() {
        let mut rule = sample_rule();
        rule.start_minute = 0;
        rule.end_minute = 1440;
        rule.excluded_dates = vec!["2026-03-01".to_string()];

        assert!(!matches(&rule, Weekday::Sun, 600, date("2026-03-01")));
        assert!(matches(&rule, Weekday::Sun, 600, date("2026-03-02")));
    }

    #[test]
    fn excluded_date_only_exists_because_of_the_offset() {
        let mut rule = sample_rule();
        rule.start_minute = 0;
        rule.end_minute = 1440;
        rule.utc_offset_minutes = -5 * 60;
        // UTC 时刻是 3 月 1 日（周日），本地却落在 2 月 28 日（周六）
        rule.excluded_dates = vec!["2026-02-28".to_string()];

        assert!(!matches(&rule, Weekday::Sat, 19 * 60, date("2026-02-28")));

        rule.excluded_dates = Vec::new();
        assert!(matches(&rule, Weekday::Sat, 19 * 60, date("2026-02-28")));
    }

    #[test]
    fn resolve_time_rule_skips_a_non_matching_rule_and_uses_the_next_one() {
        let mut second = sample_rule();
        second.id = "rule-2".to_string();
        second.start_minute = 0;
        second.end_minute = 1440;
        let rules = vec![sample_rule(), second];
        let candidates = vec!["deepseek-chat".to_string()];

        // sample_rule 的窗口是 00:30-08:30，UTC 00:00 不命中，落到第二条
        let resolved = resolve_time_rule(&rules, &candidates, SUNDAY_MIDNIGHT_UTC);
        assert_eq!(resolved.map(|rule| rule.id.as_str()), Some("rule-2"));
    }

    #[test]
    fn resolve_time_rule_lets_the_first_matching_rule_win() {
        let mut overlapping = sample_rule();
        overlapping.id = "rule-2".to_string();
        let rules = vec![sample_rule(), overlapping];
        let candidates = vec!["deepseek-chat".to_string()];

        // 两条都命中时，数组里靠前的那条赢
        let resolved = resolve_time_rule(&rules, &candidates, SUNDAY_MIDNIGHT_UTC + 3600);
        assert_eq!(resolved.map(|rule| rule.id.as_str()), Some("rule-1"));
    }

    #[test]
    fn resolve_time_rule_prefers_an_earlier_candidate() {
        let mut other = sample_rule();
        other.id = "rule-other".to_string();
        other.model_id = "deepseek-reasoner".to_string();
        other.start_minute = 0;
        other.end_minute = 1440;
        let rules = vec![sample_rule(), other];
        // 两条都命中：靠前的候选赢，靠后的候选不能把它顶掉
        let at = SUNDAY_MIDNIGHT_UTC + 3600;

        let mut candidates = vec!["deepseek-chat".to_string()];
        candidates.push("deepseek-reasoner".to_string());
        let resolved = resolve_time_rule(&rules, &candidates, at);
        assert_eq!(resolved.map(|rule| rule.id.as_str()), Some("rule-1"));

        // 候选顺序反转，命中的就换成 deepseek-reasoner 那条
        candidates.reverse();
        let resolved = resolve_time_rule(&rules, &candidates, at);
        assert_eq!(resolved.map(|rule| rule.id.as_str()), Some("rule-other"));
    }

    #[test]
    fn resolve_time_rule_returns_none_without_a_matching_candidate() {
        let rules = vec![sample_rule()];
        let candidates = vec!["gpt-5".to_string()];
        let resolved = resolve_time_rule(&rules, &candidates, SUNDAY_MIDNIGHT_UTC);
        assert_eq!(resolved.map(|rule| rule.id.as_str()), None);
    }

    #[test]
    fn weekday_bit_numbers_bits_from_monday() {
        assert_eq!(weekday_bit(Weekday::Mon), 1);
        assert_eq!(weekday_bit(Weekday::Fri), 1 << 4);
        assert_eq!(weekday_bit(Weekday::Sun), 1 << 6);
    }

    #[test]
    fn normalize_time_rule_rejects_an_empty_model_id() {
        let mut rule = sample_rule();
        rule.model_id = "  ".to_string();
        let error = normalize_time_rule(rule, 2).expect_err("empty model id");
        assert!(error.to_string().contains("模型 ID"));
    }

    #[test]
    fn normalize_time_rule_rejects_an_out_of_range_offset() {
        let mut rule = sample_rule();
        rule.utc_offset_minutes = 1440;
        assert!(normalize_time_rule(rule, 0).is_err());
    }

    #[test]
    fn normalize_time_rule_rejects_an_empty_weekday_mask() {
        let mut rule = sample_rule();
        rule.weekday_mask = 0;
        assert!(normalize_time_rule(rule, 0).is_err());
    }

    #[test]
    fn normalize_time_rule_rejects_minutes_past_midnight() {
        let mut rule = sample_rule();
        rule.start_minute = 1440;
        assert!(normalize_time_rule(rule, 0).is_err());

        let mut rule = sample_rule();
        rule.end_minute = 1441;
        assert!(normalize_time_rule(rule, 0).is_err());
    }

    #[test]
    fn normalize_time_rule_rejects_a_negative_price() {
        let mut rule = sample_rule();
        rule.input_cost_per_million = "-1".to_string();
        assert!(normalize_time_rule(rule, 0).is_err());
    }

    #[test]
    fn normalize_time_rule_rejects_a_malformed_excluded_date() {
        let mut rule = sample_rule();
        rule.excluded_dates = vec!["2026/03/01".to_string()];
        assert!(normalize_time_rule(rule, 0).is_err());
    }

    #[test]
    fn normalize_time_rule_accepts_the_deepseek_off_peak_example() {
        let normalized = normalize_time_rule(sample_rule(), 0).expect("normalize");

        assert_eq!(normalized.model_id, "deepseek-chat");
        assert_eq!(normalized.label, "DeepSeek 错峰");
        assert!(!normalized.id.is_empty());
        assert_eq!(normalized.utc_offset_minutes, 0);
        assert_eq!(normalized.weekday_mask, WEEKDAY_MASK_ALL);
        assert_eq!(normalized.start_minute, 30);
        assert_eq!(normalized.end_minute, 510);
        assert_eq!(normalized.input_cost_per_million, "0.2");
        assert_eq!(normalized.output_cost_per_million, "0.8");
    }

    #[test]
    fn normalize_time_rule_is_idempotent() {
        let once = normalize_time_rule(sample_rule(), 0).expect("normalize once");
        let twice = normalize_time_rule(once.clone(), 0).expect("normalize twice");
        assert_eq!(once, twice);
    }

    #[test]
    fn normalize_time_rule_dedupes_and_canonicalizes_excluded_dates() {
        let mut rule = sample_rule();
        rule.excluded_dates = vec![
            "2026-03-01".to_string(),
            " 2026-02-28 ".to_string(),
            // chrono 能解析个位月日，但必须归一成 YYYY-MM-DD 才能匹配上
            "2026-3-2".to_string(),
            "".to_string(),
        ];

        let normalized = normalize_time_rule(rule, 0).expect("normalize");
        assert_eq!(
            normalized.excluded_dates,
            vec![
                "2026-02-28".to_string(),
                "2026-03-01".to_string(),
                "2026-03-02".to_string(),
            ]
        );
    }

    #[test]
    fn describe_window_renders_weekdays_window_and_offset() {
        let mut rule = sample_rule();
        rule.weekday_mask = 0b0001_1111; // Mon-Fri
        rule.start_minute = 60;
        rule.end_minute = 240;
        rule.utc_offset_minutes = 0;
        assert_eq!(describe_window(&rule), "Mon-Fri 01:00-04:00 (UTC+00:00)");
    }

    #[test]
    fn describe_window_renders_full_day_and_negative_offset() {
        let mut rule = sample_rule();
        rule.start_minute = 22 * 60;
        rule.end_minute = 22 * 60;
        rule.utc_offset_minutes = -5 * 60;
        assert_eq!(describe_window(&rule), "Every day All day (UTC-05:00)");

        let mut rule = sample_rule();
        rule.start_minute = 22 * 60;
        rule.end_minute = 8 * 60;
        rule.utc_offset_minutes = -5 * 60;
        assert_eq!(describe_window(&rule), "Every day 22:00-08:00 (UTC-05:00)");
    }

    #[test]
    fn describe_window_renders_split_weekday_runs() {
        let mut rule = sample_rule();
        // 周一和周五被中间的日子断开，不能折叠成区间
        rule.weekday_mask = weekday_bit(Weekday::Mon) | weekday_bit(Weekday::Fri);
        rule.start_minute = 0;
        rule.end_minute = 1440;
        assert_eq!(describe_window(&rule), "Mon,Fri 00:00-24:00 (UTC+00:00)");
    }
}
