//! codex-resets.com 的社区重置预测（`/api/v1/status`）。
//!
//! OpenAI 官方 rate_limits 接口给的 `resets_at` 常常不准，这个社区站点汇总
//! @thsottiaux 的重置公告和大家观测到的真实重置时刻。数据是全局的（不分账号、
//! 不分订阅档），所以只存一份内存缓存，TTL 内所有调用共用；上游有 429 限流，
//! 失败不缓存，等前端下次轮询再试。

use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

const STATUS_URL: &str = "https://codex-resets.com/api/v1/status";
/// 成功结果缓存 10 分钟：重置事件以小时计，没必要每次都打上游
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

// ===== API 响应（wire 格式，字段名原样反序列化）=====

#[derive(Debug, Deserialize)]
struct StatusResponse {
    data: StatusData,
}

#[derive(Debug, Deserialize)]
struct StatusData {
    latest_reset: Option<ResetWire>,
    scheduled_reset: Option<ScheduledResetWire>,
    active_watch: Option<WatchWire>,
    #[serde(default)]
    stats: StatsWire,
}

#[derive(Debug, Deserialize)]
struct ResetWire {
    id: String,
    reset_type: String,
    announced_at: String,
    #[serde(default)]
    text: Option<String>,
    source: SourceWire,
}

#[derive(Debug, Deserialize)]
struct ScheduledResetWire {
    id: String,
    reset_type: String,
    announced_at: String,
    #[serde(default)]
    scheduled_for: Option<String>,
    #[serde(default)]
    text: Option<String>,
    source: SourceWire,
}

#[derive(Debug, Deserialize)]
struct WatchWire {
    level: String,
    reset_chance_percent: Option<i32>,
    forecast_window: String,
    observed_at: String,
    expires_at: String,
    #[serde(default)]
    text: Option<String>,
    source: SourceWire,
}

#[derive(Debug, Deserialize)]
struct SourceWire {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct StatsWire {
    #[serde(default)]
    total: i64,
    last_reset_at: Option<String>,
    days_since_last: Option<f64>,
    avg_interval_days: Option<f64>,
}

// ===== 给前端的格式（camelCase，和 SubscriptionQuota 一致）=====

/// 已执行的一次重置（最近一次）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexResetEntry {
    pub id: String,
    /// "regular" | "banked"
    pub reset_type: String,
    /// 公告或首次观测到的时间（ISO 8601）
    pub announced_at: String,
    pub text: Option<String>,
    pub source: CodexResetSource,
}

/// 已公告、还没等到执行证据的一次重置；`scheduled_for` 过期不代表已执行
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexScheduledReset {
    pub id: String,
    /// "regular" | "banked"
    pub reset_type: String,
    pub announced_at: String,
    pub scheduled_for: Option<String>,
    pub text: Option<String>,
    pub source: CodexResetSource,
}

/// 进行中的重置观测（AI 分类的预测，非 OpenAI 官方承诺）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexResetWatch {
    /// "elevated" | "strong"
    pub level: String,
    pub reset_chance_percent: Option<i32>,
    pub forecast_window: String,
    pub observed_at: String,
    pub expires_at: String,
    pub text: Option<String>,
    pub source: CodexResetSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexResetSource {
    /// "x_post" | "observed"
    pub kind: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexResetsStats {
    pub total: i64,
    pub last_reset_at: Option<String>,
    pub days_since_last: Option<f64>,
    pub avg_interval_days: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexResetsStatus {
    pub latest_reset: Option<CodexResetEntry>,
    pub scheduled_reset: Option<CodexScheduledReset>,
    pub active_watch: Option<CodexResetWatch>,
    pub stats: CodexResetsStats,
    /// 本地抓取时间（毫秒时间戳）
    pub fetched_at: i64,
}

impl CodexResetsStatus {
    fn from_wire(data: StatusData) -> Self {
        Self {
            latest_reset: data.latest_reset.map(|r| CodexResetEntry {
                id: r.id,
                reset_type: r.reset_type,
                announced_at: r.announced_at,
                text: r.text,
                source: r.source.into_frontend(),
            }),
            scheduled_reset: data.scheduled_reset.map(|r| CodexScheduledReset {
                id: r.id,
                reset_type: r.reset_type,
                announced_at: r.announced_at,
                scheduled_for: r.scheduled_for,
                text: r.text,
                source: r.source.into_frontend(),
            }),
            active_watch: data.active_watch.map(|w| CodexResetWatch {
                level: w.level,
                reset_chance_percent: w.reset_chance_percent,
                forecast_window: w.forecast_window,
                observed_at: w.observed_at,
                expires_at: w.expires_at,
                text: w.text,
                source: w.source.into_frontend(),
            }),
            stats: CodexResetsStats {
                total: data.stats.total,
                last_reset_at: data.stats.last_reset_at,
                days_since_last: data.stats.days_since_last,
                avg_interval_days: data.stats.avg_interval_days,
            },
            fetched_at: chrono::Utc::now().timestamp_millis(),
        }
    }
}

impl SourceWire {
    fn into_frontend(self) -> CodexResetSource {
        CodexResetSource {
            kind: self.kind,
            url: self.url,
        }
    }
}

static CACHE: OnceLock<RwLock<Option<(Instant, CodexResetsStatus)>>> = OnceLock::new();

fn cache() -> &'static RwLock<Option<(Instant, CodexResetsStatus)>> {
    CACHE.get_or_init(|| RwLock::new(None))
}

/// 拉取重置预测状态；TTL 内直接返回上次成功的结果。
/// `force` 跳过缓存直接打上游（用户点额度刷新按钮时联动用），成功后照常写缓存。
pub async fn get_status(force: bool) -> Result<CodexResetsStatus, String> {
    if !force {
        let cached = cache().read().ok().and_then(|guard| guard.clone());
        if let Some((at, status)) = cached {
            if at.elapsed() < CACHE_TTL {
                return Ok(status);
            }
        }
    }

    let client = crate::proxy::http_client::get();
    let resp = client
        .get(STATUS_URL)
        .header("User-Agent", "cc-switch")
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    let status_code = resp.status();
    if !status_code.is_success() {
        return Err(format!("codex-resets.com API error (HTTP {status_code})"));
    }
    let body: StatusResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse codex-resets.com response: {e}"))?;

    let status = CodexResetsStatus::from_wire(body.data);
    if let Ok(mut guard) = cache().write() {
        *guard = Some((Instant::now(), status.clone()));
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-24 实际抓到的 /api/v1/status 响应（scheduled_reset / active_watch 为 null）
    #[test]
    fn parses_real_status_response() {
        let raw = r#"{
            "data": {
                "latest_reset": {
                    "id": "observed-2113492502355562875",
                    "reset_type": "regular",
                    "announced_at": "2026-10-23T17:00:00.000Z",
                    "text": "Codex usage limits have been reset. Check your limits.",
                    "source": {"type": "observed", "url": "https://x.com/thsottiaux/status/2113492502355562875"}
                },
                "scheduled_reset": null,
                "active_watch": null,
                "stats": {
                    "total": 66,
                    "last_reset_at": "2026-10-23T17:00:00.000Z",
                    "days_since_last": 0.7,
                    "avg_interval_days": 6.2
                }
            },
            "meta": {"api_version": "v1", "generated_at": "2026-10-24T10:22:46.044Z"}
        }"#;
        let parsed: StatusResponse = serde_json::from_str(raw).expect("parse status");
        let status = CodexResetsStatus::from_wire(parsed.data);

        let latest = status.latest_reset.clone().expect("latest reset");
        assert_eq!(latest.reset_type, "regular");
        assert_eq!(latest.announced_at, "2026-10-23T17:00:00.000Z");
        assert_eq!(latest.source.kind, "observed");
        assert!(status.scheduled_reset.is_none());
        assert!(status.active_watch.is_none());
        assert_eq!(status.stats.total, 66);
        assert_eq!(status.stats.days_since_last, Some(0.7));

        // 给前端的字段是 camelCase
        let json = serde_json::to_value(&status).expect("serialize");
        assert_eq!(json["latestReset"]["announcedAt"], "2026-10-23T17:00:00.000Z");
        assert_eq!(json["stats"]["avgIntervalDays"], 6.2);
    }

    /// OpenAPI 里 scheduled_reset / active_watch 的完整形状，字段齐时也要能解析
    #[test]
    fn parses_scheduled_reset_and_watch() {
        let raw = r#"{
            "data": {
                "latest_reset": null,
                "scheduled_reset": {
                    "id": "x-123",
                    "status": "scheduled",
                    "reset_type": "banked",
                    "announced_at": "2026-10-24T09:00:00.000Z",
                    "scheduled_for": "2026-10-25T01:00:00.000Z",
                    "text": "Banked reset incoming",
                    "source": {"type": "x_post", "author": "thsottiaux", "url": "https://x.com/thsottiaux/status/123"}
                },
                "active_watch": {
                    "level": "strong",
                    "reset_chance_percent": 60,
                    "forecast_window": "next 6 hours",
                    "observed_at": "2026-10-24T08:00:00.000Z",
                    "expires_at": "2026-10-24T14:00:00.000Z",
                    "text": "Elevated reset activity",
                    "source": {"type": "observed"}
                },
                "stats": {"total": 1, "last_reset_at": null, "days_since_last": null, "avg_interval_days": null}
            },
            "meta": {"api_version": "v1", "generated_at": "2026-10-24T10:00:00.000Z"}
        }"#;
        let parsed: StatusResponse = serde_json::from_str(raw).expect("parse status");
        let status = CodexResetsStatus::from_wire(parsed.data);

        let scheduled = status.scheduled_reset.expect("scheduled reset");
        assert_eq!(scheduled.scheduled_for.as_deref(), Some("2026-10-25T01:00:00.000Z"));
        assert_eq!(scheduled.source.kind, "x_post");
        let watch = status.active_watch.expect("watch");
        assert_eq!(watch.reset_chance_percent, Some(60));
        assert_eq!(watch.level, "strong");
    }
}
