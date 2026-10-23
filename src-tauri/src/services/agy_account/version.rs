//! Antigravity Hub 版本的自动探测与缓存。
//!
//! Google Cloud Code / Code Assist 服务端在下发模型和配额时，对客户端版本有最低要求
//! （低于 2.23.0 会被拒绝或过滤模型）。为避免写死版本号随官方发布而过时，
//! 参考 CLIProxyAPI (CPA) 的做法：从官方 Antigravity Hub 更新清单自动探测最新版本，
//! 缓存在内存中（TTL 6 小时）；若探测失败或超时，自动回退到安全的 fallback 版本（2.24.0）。

use once_cell::sync::Lazy;
use std::sync::RwLock;
use std::time::{Duration, Instant};

pub const FALLBACK_VERSION: &str = "2.24.0";
const HUB_MANIFEST_URL: &str =
    "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/manifest/latest-arm64-mac.yml";
const CACHE_TTL: Duration = Duration::from_secs(6 * 3600);
const FAILURE_RETRY_DELAY: Duration = Duration::from_secs(300);
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

struct VersionCache {
    version: String,
    fetched_at: Instant,
    is_fallback: bool,
}

static CACHED_VERSION: Lazy<RwLock<Option<VersionCache>>> = Lazy::new(|| RwLock::new(None));

/// 获取最新的 Antigravity Hub 版本号。优先从内存缓存获取；若过期则从官方源拉取；拉取失败则回退。
pub async fn get_latest_version() -> String {
    if let Ok(guard) = CACHED_VERSION.read() {
        if let Some(cache) = guard.as_ref() {
            let max_age = if cache.is_fallback {
                FAILURE_RETRY_DELAY
            } else {
                CACHE_TTL
            };
            if cache.fetched_at.elapsed() < max_age {
                return cache.version.clone();
            }
        }
    }

    match fetch_latest_version().await {
        Some(v) => {
            log::debug!("[AgyVersion] Detected latest Antigravity Hub version: {v}");
            if let Ok(mut guard) = CACHED_VERSION.write() {
                *guard = Some(VersionCache {
                    version: v.clone(),
                    fetched_at: Instant::now(),
                    is_fallback: false,
                });
            }
            v
        }
        None => {
            if let Ok(mut guard) = CACHED_VERSION.write() {
                let v = guard
                    .as_ref()
                    .map(|c| c.version.clone())
                    .unwrap_or_else(|| FALLBACK_VERSION.to_string());
                log::debug!(
                    "[AgyVersion] Falling back to Antigravity Hub version: {v} (retry in {FAILURE_RETRY_DELAY:?})"
                );
                *guard = Some(VersionCache {
                    version: v.clone(),
                    fetched_at: Instant::now(),
                    is_fallback: true,
                });
                v
            } else {
                FALLBACK_VERSION.to_string()
            }
        }
    }
}

/// 从官方 Hub 更新清单异步拉取版本字符串
async fn fetch_latest_version() -> Option<String> {
    let client = crate::proxy::http_client::get();
    let resp = match client
        .get(HUB_MANIFEST_URL)
        .header(reqwest::header::USER_AGENT, "electron-builder")
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            log::warn!("[AgyVersion] Failed to request Antigravity Hub manifest: {err}");
            return None;
        }
    };

    if !resp.status().is_success() {
        log::warn!(
            "[AgyVersion] Antigravity Hub manifest returned non-success status: {}",
            resp.status()
        );
        return None;
    }

    let text = match resp.text().await {
        Ok(t) => t,
        Err(err) => {
            log::warn!("[AgyVersion] Failed to read Antigravity Hub manifest text: {err}");
            return None;
        }
    };

    let version = parse_manifest_version(&text);
    if version.is_none() {
        log::warn!("[AgyVersion] Failed to parse version from Antigravity Hub manifest");
    }
    version
}

/// 从 manifest YAML 文本中解析 version 字段
pub(crate) fn parse_manifest_version(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version:") {
            let ver = rest.trim().trim_matches(|c| c == '\'' || c == '"');
            if is_valid_semver(ver) {
                return Some(ver.to_string());
            }
        }
    }
    None
}

/// 简单校验是否为标准的语义化版本格式（三段全数字，如 2.25.0）
fn is_valid_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_from_yaml_manifest() {
        let yaml = r#"
version: 2.25.0
files:
  - url: https://storage.googleapis.com/...
    sha512: abc...
    size: 183727794
"#;
        assert_eq!(parse_manifest_version(yaml).as_deref(), Some("2.25.0"));
    }

    #[test]
    fn parses_version_with_quotes() {
        assert_eq!(
            parse_manifest_version("version: '2.24.1'").as_deref(),
            Some("2.24.1")
        );
        assert_eq!(
            parse_manifest_version("version: \"2.26.0\"").as_deref(),
            Some("2.26.0")
        );
    }

    #[test]
    fn rejects_invalid_semver_strings() {
        assert_eq!(parse_manifest_version("version: 2.25"), None);
        assert_eq!(parse_manifest_version("version: 2.25.0-beta"), None);
        assert_eq!(parse_manifest_version("version: v2.25.0"), None);
        assert_eq!(parse_manifest_version("no_version: 2.25.0"), None);
    }
}
