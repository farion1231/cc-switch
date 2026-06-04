//! OpenCode Go workspace quota handler.
//!
//! Fetches the OpenCode Go dashboard HTML and extracts rolling / weekly / monthly
//! usage percentages via regex from SolidJS SSR hydration output.
//!
//! Credentials (workspace ID + auth cookie) are user-provided and stored in
//! `ProviderMeta`, never passed through the JS script engine.

use crate::provider::{UsageData, UsageResult};
use regex::Regex;
use std::sync::LazyLock;
use std::time::Duration;

// ── Constants ─────────────────────────────────────────────────

const DASHBOARD_URL: &str = "https://opencode.ai/workspace";
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Gecko/20100101 Firefox/148.0";
const SCRAPE_TIMEOUT_MS: u64 = 10_000;

// ── Window labels ────────────────────────────────────────────

const LABEL_ROLLING: &str = "5h Rolling";
const LABEL_WEEKLY: &str = "Weekly";
const LABEL_MONTHLY: &str = "Monthly";

// ── Regex patterns (ported from opencode-quota) ───────────────
//
// SolidJS SSR hydration emits entries like:
//   rollingUsage:$R[0]={usagePercent:65.5,resetInSec:3600}
// Fields may appear in either order. Patterns capture both orderings.

static RE_ROLLING_PCT_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"rollingUsage:\s*\$R\[\d+\]\s*=\s*\{[^}]*usagePercent\s*:\s*(-?\d+(?:\.\d+)?)[^}]*resetInSec\s*:\s*(-?\d+(?:\.\d+)?)[^}]*\}")
        .expect("RE_ROLLING_PCT_FIRST")
});

static RE_ROLLING_RESET_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"rollingUsage:\s*\$R\[\d+\]\s*=\s*\{[^}]*resetInSec\s*:\s*(-?\d+(?:\.\d+)?)[^}]*usagePercent\s*:\s*(-?\d+(?:\.\d+)?)[^}]*\}")
        .expect("RE_ROLLING_RESET_FIRST")
});

static RE_WEEKLY_PCT_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"weeklyUsage:\s*\$R\[\d+\]\s*=\s*\{[^}]*usagePercent\s*:\s*(-?\d+(?:\.\d+)?)[^}]*resetInSec\s*:\s*(-?\d+(?:\.\d+)?)[^}]*\}")
        .expect("RE_WEEKLY_PCT_FIRST")
});

static RE_WEEKLY_RESET_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"weeklyUsage:\s*\$R\[\d+\]\s*=\s*\{[^}]*resetInSec\s*:\s*(-?\d+(?:\.\d+)?)[^}]*usagePercent\s*:\s*(-?\d+(?:\.\d+)?)[^}]*\}")
        .expect("RE_WEEKLY_RESET_FIRST")
});

static RE_MONTHLY_PCT_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"monthlyUsage:\s*\$R\[\d+\]\s*=\s*\{[^}]*usagePercent\s*:\s*(-?\d+(?:\.\d+)?)[^}]*resetInSec\s*:\s*(-?\d+(?:\.\d+)?)[^}]*\}")
        .expect("RE_MONTHLY_PCT_FIRST")
});

static RE_MONTHLY_RESET_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"monthlyUsage:\s*\$R\[\d+\]\s*=\s*\{[^}]*resetInSec\s*:\s*(-?\d+(?:\.\d+)?)[^}]*usagePercent\s*:\s*(-?\d+(?:\.\d+)?)[^}]*\}")
        .expect("RE_MONTHLY_RESET_FIRST")
});

// ── Scraped data ──────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ScrapedWindowUsage {
    usage_percent: f64,
    reset_in_sec: i64,
}

fn build_cookie_header(auth_cookie: &str) -> String {
    let cookie = auth_cookie.trim();
    if cookie
        .split(';')
        .any(|part| part.trim_start().starts_with("auth="))
    {
        cookie.to_string()
    } else {
        format!("auth={cookie}")
    }
}

// ── Parsing helpers ───────────────────────────────────────────

/// Try to extract `(usage_percent, reset_in_sec)` from `html` using two regexes
/// (usagePercent-first and resetInSec-first orderings). Returns `None` if
/// neither pattern matches.
fn parse_window_usage(
    pct_first: &Regex,
    reset_first: &Regex,
    html: &str,
) -> Option<ScrapedWindowUsage> {
    if let Some(caps) = pct_first.captures(html) {
        return Some(ScrapedWindowUsage {
            usage_percent: caps[1].parse().unwrap_or(0.0),
            reset_in_sec: caps[2].parse().unwrap_or(0),
        });
    }
    if let Some(caps) = reset_first.captures(html) {
        return Some(ScrapedWindowUsage {
            usage_percent: caps[2].parse().unwrap_or(0.0),
            reset_in_sec: caps[1].parse().unwrap_or(0),
        });
    }
    None
}

/// Convert a scraped window to a `UsageData` entry.
///
/// Computes `percent_remaining = 100 - usage_percent` and the reset ISO 8601
/// time from `now` + `reset_in_sec`.
fn normalize_window_usage(
    label: &str,
    scraped: &ScrapedWindowUsage,
    now: chrono::DateTime<chrono::Utc>,
) -> UsageData {
    let remaining = (100.0 - scraped.usage_percent).max(0.0);
    let reset_time = now + chrono::Duration::seconds(scraped.reset_in_sec);
    UsageData {
        plan_name: Some(label.to_string()),
        remaining: Some(remaining),
        used: Some(scraped.usage_percent),
        total: Some(100.0),
        unit: Some("%".to_string()),
        is_valid: Some(true),
        invalid_message: None,
        extra: Some(reset_time.to_rfc3339()),
    }
}

fn build_dashboard_url(workspace_id: &str) -> Result<url::Url, String> {
    let ws = workspace_id.trim();
    if ws.is_empty() {
        return Err("Workspace ID is empty".to_string());
    }

    let mut url = url::Url::parse(DASHBOARD_URL)
        .map_err(|e| format!("Invalid OpenCode Go dashboard URL: {e}"))?;
    url.path_segments_mut()
        .map_err(|_| "Invalid OpenCode Go dashboard URL".to_string())?
        .push(ws)
        .push("go");
    Ok(url)
}

// ── Public API ────────────────────────────────────────────────

pub async fn get_quota(workspace_id: &str, auth_cookie: &str) -> Result<UsageResult, String> {
    let ws = workspace_id.trim();
    if ws.is_empty() {
        return Ok(make_error("Workspace ID is empty".to_string()));
    }
    let cookie = auth_cookie.trim();
    if cookie.is_empty() {
        return Ok(make_error("Auth cookie is empty".to_string()));
    }

    let url = match build_dashboard_url(ws) {
        Ok(url) => url,
        Err(e) => return Ok(make_error(e)),
    };
    let cookie_header = build_cookie_header(cookie);
    let client = crate::proxy::http_client::get();

    let resp = client
        .get(url)
        .header("Cookie", cookie_header)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "text/html, application/xhtml+xml")
        .timeout(Duration::from_millis(SCRAPE_TIMEOUT_MS))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Network error fetching dashboard: {e}");
            return Ok(make_http_error(&msg, Some("Check your network connection")));
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some(format!(
                "Authentication failed (HTTP {status}). Check your auth cookie — it may have expired."
            )),
        });
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some(format!(
                "Workspace not found (HTTP {status}). Verify your workspace ID."
            )),
        });
    }
    if !status.is_success() {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some(format!(
                "Dashboard returned HTTP {status}. Check your OpenCode Go workspace ID and auth cookie."
            )),
        });
    }

    let html = match resp.text().await {
        Ok(h) => h,
        Err(e) => return Ok(make_error(format!("Failed to read response body: {e}"))),
    };

    let now = chrono::Utc::now();
    let mut data: Vec<UsageData> = Vec::new();

    if let Some(rolling) = parse_window_usage(&RE_ROLLING_PCT_FIRST, &RE_ROLLING_RESET_FIRST, &html)
    {
        data.push(normalize_window_usage(LABEL_ROLLING, &rolling, now));
    }
    if let Some(weekly) = parse_window_usage(&RE_WEEKLY_PCT_FIRST, &RE_WEEKLY_RESET_FIRST, &html) {
        data.push(normalize_window_usage(LABEL_WEEKLY, &weekly, now));
    }
    if let Some(monthly) = parse_window_usage(&RE_MONTHLY_PCT_FIRST, &RE_MONTHLY_RESET_FIRST, &html)
    {
        data.push(normalize_window_usage(LABEL_MONTHLY, &monthly, now));
    }

    if data.is_empty() {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some(
                "Could not parse usage data from dashboard HTML. The dashboard format may have changed."
                    .to_string(),
            ),
        });
    }

    Ok(UsageResult {
        success: true,
        data: Some(data),
        error: None,
    })
}

// ── Error constructors ────────────────────────────────────────

fn make_error(msg: String) -> UsageResult {
    UsageResult {
        success: false,
        data: None,
        error: Some(msg),
    }
}

fn make_http_error(detail: &str, hint: Option<&str>) -> UsageResult {
    let msg = match hint {
        Some(h) => format!("{detail}. {h}."),
        None => detail.to_string(),
    };
    UsageResult {
        success: false,
        data: None,
        error: Some(msg),
    }
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_html_all_windows_pct_first() -> String {
        format!(
            "<html><body>\
            <script>window.$R=[1,2,3];rollingUsage:$R[0]={{usagePercent:65.5,resetInSec:3600}};weeklyUsage:$R[1]={{usagePercent:42.0,resetInSec:14400}};monthlyUsage:$R[2]={{usagePercent:10.2,resetInSec:86400}};\
            </script></body></html>"
        )
    }

    fn test_html_all_windows_reset_first() -> String {
        format!(
            "<html><body>\
            <script>window.$R=[1,2,3];rollingUsage:$R[0]={{resetInSec:3600,usagePercent:65.5}};weeklyUsage:$R[1]={{resetInSec:14400,usagePercent:42.0}};monthlyUsage:$R[2]={{resetInSec:86400,usagePercent:10.2}};\
            </script></body></html>"
        )
    }

    fn test_html_mixed_orderings() -> String {
        format!(
            "<html><body>\
            <script>window.$R=[1,2,3];rollingUsage:$R[0]={{usagePercent:70.0,resetInSec:3600}};weeklyUsage:$R[1]={{resetInSec:7200,usagePercent:50.0}};\
            </script></body></html>"
        )
    }

    fn test_html_partial() -> String {
        format!(
            "<html><body>\
            <script>window.$R=[1,2];rollingUsage:$R[0]={{usagePercent:55.0,resetInSec:1800}};monthlyUsage:$R[1]={{usagePercent:88.0,resetInSec:86400}};\
            </script></body></html>"
        )
    }

    // ── parse_window_usage unit tests ─────────────────────────

    #[test]
    fn parse_rolling_pct_first() {
        let html = test_html_all_windows_pct_first();
        let result = parse_window_usage(&RE_ROLLING_PCT_FIRST, &RE_ROLLING_RESET_FIRST, &html);
        let w = result.expect("should parse rolling");
        assert!((w.usage_percent - 65.5).abs() < f64::EPSILON);
        assert_eq!(w.reset_in_sec, 3600);
    }

    #[test]
    fn parse_rolling_reset_first() {
        let html = test_html_all_windows_reset_first();
        let result = parse_window_usage(&RE_ROLLING_PCT_FIRST, &RE_ROLLING_RESET_FIRST, &html);
        let w = result.expect("should parse rolling");
        assert!((w.usage_percent - 65.5).abs() < f64::EPSILON);
        assert_eq!(w.reset_in_sec, 3600);
    }

    #[test]
    fn parse_weekly_pct_first() {
        let html = test_html_all_windows_pct_first();
        let result = parse_window_usage(&RE_WEEKLY_PCT_FIRST, &RE_WEEKLY_RESET_FIRST, &html);
        let w = result.expect("should parse weekly");
        assert!((w.usage_percent - 42.0).abs() < f64::EPSILON);
        assert_eq!(w.reset_in_sec, 14400);
    }

    #[test]
    fn parse_monthly_pct_first() {
        let html = test_html_all_windows_pct_first();
        let result = parse_window_usage(&RE_MONTHLY_PCT_FIRST, &RE_MONTHLY_RESET_FIRST, &html);
        let w = result.expect("should parse monthly");
        assert!((w.usage_percent - 10.2).abs() < f64::EPSILON);
        assert_eq!(w.reset_in_sec, 86400);
    }

    #[test]
    fn parse_empty_html_returns_none() {
        let html = "<html></html>";
        assert!(parse_window_usage(&RE_ROLLING_PCT_FIRST, &RE_ROLLING_RESET_FIRST, html).is_none());
    }

    #[test]
    fn parse_malformed_html_returns_none() {
        let html = "garbage text without any hydration data";
        assert!(parse_window_usage(&RE_ROLLING_PCT_FIRST, &RE_ROLLING_RESET_FIRST, html).is_none());
    }

    #[test]
    fn parse_mixed_orderings() {
        let html = test_html_mixed_orderings();
        let rolling = parse_window_usage(&RE_ROLLING_PCT_FIRST, &RE_ROLLING_RESET_FIRST, &html)
            .expect("rolling");
        let weekly = parse_window_usage(&RE_WEEKLY_PCT_FIRST, &RE_WEEKLY_RESET_FIRST, &html)
            .expect("weekly");
        assert!((rolling.usage_percent - 70.0).abs() < f64::EPSILON);
        assert!((weekly.usage_percent - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_partial_windows() {
        let html = test_html_partial();
        assert!(
            parse_window_usage(&RE_ROLLING_PCT_FIRST, &RE_ROLLING_RESET_FIRST, &html).is_some()
        );
        assert!(parse_window_usage(&RE_WEEKLY_PCT_FIRST, &RE_WEEKLY_RESET_FIRST, &html).is_none());
        assert!(
            parse_window_usage(&RE_MONTHLY_PCT_FIRST, &RE_MONTHLY_RESET_FIRST, &html).is_some()
        );
    }

    // ── normalize_window_usage unit tests ─────────────────────

    #[test]
    fn normalize_computes_remaining_and_reset_iso() {
        let scraped = ScrapedWindowUsage {
            usage_percent: 30.0,
            reset_in_sec: 7200,
        };
        let now = chrono::Utc::now();
        let usage = normalize_window_usage(LABEL_ROLLING, &scraped, now);
        assert_eq!(usage.plan_name.as_deref(), Some(LABEL_ROLLING));
        assert!((usage.remaining.unwrap() - 70.0).abs() < f64::EPSILON);
        assert!((usage.used.unwrap() - 30.0).abs() < f64::EPSILON);
        assert_eq!(usage.total, Some(100.0));
        assert_eq!(usage.unit.as_deref(), Some("%"));
        assert!(usage.is_valid.unwrap());
        // Check that reset time is approximately now + 7200s
        let expected_reset = now + chrono::Duration::seconds(7200);
        let expected_str = expected_reset.to_rfc3339();
        assert_eq!(usage.extra.as_deref(), Some(&expected_str[..]));
    }

    #[test]
    fn normalize_zero_usage() {
        let scraped = ScrapedWindowUsage {
            usage_percent: 0.0,
            reset_in_sec: 3600,
        };
        let now = chrono::Utc::now();
        let usage = normalize_window_usage(LABEL_ROLLING, &scraped, now);
        assert_eq!(usage.remaining, Some(100.0));
        assert_eq!(usage.used, Some(0.0));
    }

    #[test]
    fn normalize_full_usage_clamps_remaining_to_zero() {
        let scraped = ScrapedWindowUsage {
            usage_percent: 100.0,
            reset_in_sec: 3600,
        };
        let now = chrono::Utc::now();
        let usage = normalize_window_usage(LABEL_ROLLING, &scraped, now);
        assert_eq!(usage.remaining, Some(0.0));
        assert_eq!(usage.used, Some(100.0));
    }

    #[test]
    fn build_dashboard_url_uses_normal_workspace_path() {
        let url = build_dashboard_url("workspace_123").expect("url");
        assert_eq!(
            url.as_str(),
            "https://opencode.ai/workspace/workspace_123/go"
        );
    }

    #[test]
    fn build_dashboard_url_encodes_workspace_as_single_path_segment() {
        let url = build_dashboard_url("team/foo bar?x=1#frag").expect("url");
        assert_eq!(
            url.as_str(),
            "https://opencode.ai/workspace/team%2Ffoo%20bar%3Fx=1%23frag/go"
        );
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
    }

    #[test]
    fn build_dashboard_url_rejects_empty_workspace_id() {
        let err = build_dashboard_url("  ").expect_err("error");
        assert_eq!(err, "Workspace ID is empty");
    }

    #[tokio::test]
    async fn get_quota_requires_workspace_id() {
        let result = get_quota("", "session=secret").await.expect("result");
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("Workspace ID is empty"));
    }

    #[tokio::test]
    async fn get_quota_requires_auth_cookie() {
        let result = get_quota("workspace_123", "").await.expect("result");
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("Auth cookie is empty"));
    }
}
