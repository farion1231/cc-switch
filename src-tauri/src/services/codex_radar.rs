//! Native Codex degradation radar with 30-minute TTL cache.
//! Returns DTOs only — never remote HTML for React injection.

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default public source used by CodexElves-style radar pages.
pub const DEFAULT_RADAR_SOURCE_URL: &str = "https://artificialanalysis.ai/leaderboards/models";

const CACHE_REL: &str = "codex-workbench/cache/radar.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarModelIq {
    pub model: String,
    pub score: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarIqComparison {
    pub left_model: String,
    pub right_model: String,
    pub delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarSnapshot {
    pub fetched_at: i64,
    pub source_url: String,
    pub models: Vec<CodexRadarModelIq>,
    pub comparisons: Vec<CodexRadarIqComparison>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RadarResult {
    pub snapshot: Option<CodexRadarSnapshot>,
    pub stale: bool,
    pub from_cache: bool,
    pub error: Option<String>,
}

/// Optional test override for cache root and fetch body/error.
#[derive(Default)]
struct RadarTestHooks {
    cache_root: Option<PathBuf>,
    /// When set, fetch returns this body instead of network.
    fetch_body: Option<String>,
    /// When set, fetch fails with this error.
    fetch_error: Option<String>,
    /// Fixed "now" for TTL tests (unix secs).
    now_secs: Option<i64>,
}

static TEST_HOOKS: Mutex<RadarTestHooks> = Mutex::new(RadarTestHooks {
    cache_root: None,
    fetch_body: None,
    fetch_error: None,
    now_secs: None,
});

fn now_unix_secs() -> i64 {
    if let Ok(h) = TEST_HOOKS.lock() {
        if let Some(n) = h.now_secs {
            return n;
        }
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn cache_path() -> PathBuf {
    if let Ok(h) = TEST_HOOKS.lock() {
        if let Some(ref root) = h.cache_root {
            return root.join("radar.json");
        }
    }
    crate::config::get_app_config_dir().join(CACHE_REL)
}

fn ttl_secs() -> u64 {
    let mins = crate::settings::get_settings()
        .codex_workbench
        .radar_ttl_minutes
        .max(1) as u64;
    mins * 60
}

/// Parse a lightweight radar payload.
/// Accepts:
/// 1) JSON: `{ "models":[{"model":"gpt","score":80,"label":"gpt"}], "comparisons":[...] }`
/// 2) Line format: `MODEL|SCORE|LABEL` per line
/// 3) Simple HTML table rows: `<tr><td>model</td><td>12.3</td></tr>`
pub fn parse_radar_payload(
    body: &str,
    source_url: &str,
    fetched_at: i64,
) -> Result<CodexRadarSnapshot, AppError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(AppError::Message("radar payload empty".into()));
    }

    // JSON first
    if trimmed.starts_with('{') {
        #[derive(Deserialize)]
        struct Wire {
            models: Vec<CodexRadarModelIq>,
            #[serde(default)]
            comparisons: Vec<CodexRadarIqComparison>,
        }
        let wire: Wire = serde_json::from_str(trimmed)
            .map_err(|e| AppError::Message(format!("radar json parse: {e}")))?;
        if wire.models.is_empty() {
            return Err(AppError::Message("radar models empty".into()));
        }
        let comparisons = if wire.comparisons.is_empty() {
            build_comparisons(&wire.models)
        } else {
            wire.comparisons
        };
        return Ok(CodexRadarSnapshot {
            fetched_at,
            source_url: source_url.to_string(),
            models: wire.models,
            comparisons,
        });
    }

    // Line format MODEL|SCORE|LABEL
    let mut models = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains('|') {
            let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
            if parts.len() >= 2 {
                if let Ok(score) = parts[1].parse::<f64>() {
                    let model = parts[0].to_string();
                    let label = parts
                        .get(2)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| model.clone());
                    models.push(CodexRadarModelIq {
                        model,
                        score,
                        label,
                    });
                    continue;
                }
            }
        }
        // HTML-ish: <td>Model</td><td>12.3</td>
        if line.contains("<td") {
            let cells: Vec<String> = extract_td_texts(line);
            if cells.len() >= 2 {
                if let Ok(score) = cells[1].replace(',', "").parse::<f64>() {
                    let model = cells[0].clone();
                    models.push(CodexRadarModelIq {
                        label: model.clone(),
                        model,
                        score,
                    });
                }
            }
        }
    }

    // Fallback: scan whole body for table rows
    if models.is_empty() {
        for caps in extract_table_rows(trimmed) {
            models.push(caps);
        }
    }

    if models.is_empty() {
        return Err(AppError::Message("no radar models parsed".into()));
    }

    // de-dupe by model name keeping first
    let mut seen = std::collections::HashSet::new();
    models.retain(|m| seen.insert(m.model.clone()));

    let comparisons = build_comparisons(&models);
    Ok(CodexRadarSnapshot {
        fetched_at,
        source_url: source_url.to_string(),
        models,
        comparisons,
    })
}

fn extract_td_texts(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("<td") {
        let after = &rest[start..];
        if let Some(gt) = after.find('>') {
            let content_start = &after[gt + 1..];
            if let Some(end) = content_start.find("</td>") {
                let raw = content_start[..end].trim();
                // strip nested tags roughly
                let text = strip_tags(raw);
                if !text.is_empty() {
                    out.push(text);
                }
                rest = &content_start[end + 5..];
                continue;
            }
        }
        break;
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_table_rows(body: &str) -> Vec<CodexRadarModelIq> {
    let mut models = Vec::new();
    for segment in body.split("<tr").skip(1) {
        let row = if let Some(end) = segment.find("</tr>") {
            &segment[..end]
        } else {
            segment
        };
        let cells = extract_td_texts(row);
        if cells.len() >= 2 {
            // try last numeric cell as score, first non-empty as model
            let mut score_opt = None;
            for c in cells.iter().rev() {
                #[allow(clippy::collapsible_str_replace)]
                #[allow(clippy::collapsible_str_replace)]
                if let Ok(v) = c.replace(',', "").replace('%', "").parse::<f64>() {
                    score_opt = Some(v);
                    break;
                }
            }
            if let Some(score) = score_opt {
                let model = cells[0].clone();
                if !model.is_empty() && model.to_lowercase() != "model" {
                    models.push(CodexRadarModelIq {
                        label: model.clone(),
                        model,
                        score,
                    });
                }
            }
        }
    }
    models
}

fn build_comparisons(models: &[CodexRadarModelIq]) -> Vec<CodexRadarIqComparison> {
    if models.len() < 2 {
        return Vec::new();
    }
    let mut ranked = models.to_vec();
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut comps = Vec::new();
    for w in ranked.windows(2).take(8) {
        comps.push(CodexRadarIqComparison {
            left_model: w[0].model.clone(),
            right_model: w[1].model.clone(),
            delta: w[0].score - w[1].score,
        });
    }
    comps
}

fn read_cache(path: &Path) -> Option<CodexRadarSnapshot> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<CodexRadarSnapshot>(&text)
        .ok()
        .filter(|s| !s.models.is_empty())
}

fn write_cache(path: &Path, snap: &CodexRadarSnapshot) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let text = serde_json::to_string_pretty(snap)
        .map_err(|e| AppError::Message(format!("radar cache serialize: {e}")))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text.as_bytes()).map_err(|e| AppError::io(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| AppError::io(path, e))?;
    Ok(())
}

fn is_fresh(snap: &CodexRadarSnapshot, now: i64, ttl: u64) -> bool {
    let age = now.saturating_sub(snap.fetched_at);
    age >= 0 && (age as u64) < ttl
}

async fn fetch_remote_body(url: &str) -> Result<String, String> {
    // test hooks
    if let Ok(h) = TEST_HOOKS.lock() {
        if let Some(ref err) = h.fetch_error {
            return Err(err.clone());
        }
        if let Some(ref body) = h.fetch_body {
            return Ok(body.clone());
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("cc-switch-codex-radar/1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("http {}", resp.status()));
    }
    resp.text().await.map_err(|e| e.to_string())
}

/// Fetch radar snapshot. `refresh=false` honors TTL; `refresh=true` always network.
/// Failed fetch with valid cache returns snapshot + error (stale if past TTL).
pub async fn get_radar(refresh: bool) -> RadarResult {
    let path = cache_path();
    let now = now_unix_secs();
    let ttl = ttl_secs();
    let cached = read_cache(&path);

    if !refresh {
        if let Some(ref snap) = cached {
            if is_fresh(snap, now, ttl) {
                return RadarResult {
                    snapshot: Some(snap.clone()),
                    stale: false,
                    from_cache: true,
                    error: None,
                };
            }
        }
    }

    let source = DEFAULT_RADAR_SOURCE_URL.to_string();
    match fetch_remote_body(&source).await {
        Ok(body) => match parse_radar_payload(&body, &source, now) {
            Ok(snap) => {
                let _ = write_cache(&path, &snap);
                RadarResult {
                    snapshot: Some(snap),
                    stale: false,
                    from_cache: false,
                    error: None,
                }
            }
            Err(e) => fallback_cache(cached, now, ttl, Some(e.to_string())),
        },
        Err(e) => fallback_cache(cached, now, ttl, Some(e)),
    }
}

fn fallback_cache(
    cached: Option<CodexRadarSnapshot>,
    now: i64,
    ttl: u64,
    error: Option<String>,
) -> RadarResult {
    match cached {
        Some(snap) => {
            let stale = !is_fresh(&snap, now, ttl);
            RadarResult {
                snapshot: Some(snap),
                stale,
                from_cache: true,
                error,
            }
        }
        None => RadarResult {
            snapshot: None,
            stale: true,
            from_cache: false,
            error: error.or_else(|| Some("no radar data".into())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Ok(mut h) = TEST_HOOKS.lock() {
                *h = RadarTestHooks::default();
            }
        }
    }

    fn setup(tmp: &Path) -> Guard {
        let mut h = TEST_HOOKS.lock().unwrap();
        *h = RadarTestHooks {
            cache_root: Some(tmp.to_path_buf()),
            fetch_body: None,
            fetch_error: None,
            now_secs: Some(1_700_000_000),
        };
        Guard
    }

    #[test]
    fn parse_line_format_and_comparisons() {
        let body = "# comment\ngpt-4o|88.5|GPT-4o\nclaude-3.5|90.1|Claude 3.5\n";
        let snap = parse_radar_payload(body, "test://src", 100).unwrap();
        assert_eq!(snap.models.len(), 2);
        assert!(!snap.comparisons.is_empty());
        assert_eq!(snap.comparisons[0].left_model, "claude-3.5");
    }

    #[test]
    fn parse_json_fixture() {
        let body = r#"{"models":[{"model":"a","score":10,"label":"A"},{"model":"b","score":20,"label":"B"}]}"#;
        let snap = parse_radar_payload(body, "test://j", 1).unwrap();
        assert_eq!(snap.models.len(), 2);
        assert_eq!(snap.comparisons[0].delta, 10.0);
    }

    #[tokio::test]
    async fn failed_refresh_returns_old_cache_marked_stale() {
        let _g = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = setup(tmp.path());

        // seed cache 31 minutes old
        let old = CodexRadarSnapshot {
            fetched_at: 1_700_000_000 - 31 * 60,
            source_url: "test://src".into(),
            models: vec![
                CodexRadarModelIq {
                    model: "m1".into(),
                    score: 1.0,
                    label: "m1".into(),
                },
                CodexRadarModelIq {
                    model: "m2".into(),
                    score: 2.0,
                    label: "m2".into(),
                },
            ],
            comparisons: vec![],
        };
        write_cache(&cache_path(), &old).unwrap();

        {
            let mut h = TEST_HOOKS.lock().unwrap();
            h.fetch_error = Some("offline".into());
        }

        let result = get_radar(false).await;
        assert!(result.stale);
        assert!(result.from_cache);
        assert_eq!(result.snapshot.unwrap().models.len(), 2);
        assert_eq!(result.error.as_deref(), Some("offline"));
    }

    #[tokio::test]
    async fn fresh_cache_skips_network() {
        let _g = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = setup(tmp.path());

        let snap = CodexRadarSnapshot {
            fetched_at: 1_700_000_000 - 60, // 1 min ago
            source_url: "test://src".into(),
            models: vec![CodexRadarModelIq {
                model: "x".into(),
                score: 5.0,
                label: "x".into(),
            }],
            comparisons: vec![],
        };
        write_cache(&cache_path(), &snap).unwrap();
        {
            let mut h = TEST_HOOKS.lock().unwrap();
            h.fetch_error = Some("should-not-call".into());
        }
        let result = get_radar(false).await;
        assert!(!result.stale);
        assert!(result.from_cache);
        assert!(result.error.is_none());
        assert_eq!(result.snapshot.unwrap().models[0].model, "x");
    }

    #[tokio::test]
    async fn force_refresh_fetches_even_if_fresh() {
        let _g = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = setup(tmp.path());

        let snap = CodexRadarSnapshot {
            fetched_at: 1_700_000_000 - 10,
            source_url: "test://old".into(),
            models: vec![CodexRadarModelIq {
                model: "old".into(),
                score: 1.0,
                label: "old".into(),
            }],
            comparisons: vec![],
        };
        write_cache(&cache_path(), &snap).unwrap();
        {
            let mut h = TEST_HOOKS.lock().unwrap();
            h.fetch_body = Some("new|99|NewModel\n".into());
        }
        let result = get_radar(true).await;
        assert!(!result.from_cache);
        assert_eq!(result.snapshot.unwrap().models[0].model, "new");
    }
}
