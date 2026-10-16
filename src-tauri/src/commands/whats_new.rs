use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WhatsNewSource {
    Mirror,
    Github,
}

fn summary_url(version: &str, source: WhatsNewSource) -> Result<String, String> {
    static VERSION: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$").unwrap());
    if !VERSION.is_match(version) {
        return Err("Invalid release summary version".to_string());
    }
    let base = match source {
        WhatsNewSource::Mirror => "https://dl.ccswitch.io",
        WhatsNewSource::Github => "https://github.com/farion1231/cc-switch/releases/download",
    };
    Ok(format!("{base}/v{version}/whats-new-{version}.json"))
}

async fn fetch_summary(request: reqwest::RequestBuilder) -> Result<Value, String> {
    request
        .timeout(Duration::from_secs(8))
        .header(reqwest::header::USER_AGENT, "cc-switch")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("Failed to fetch release summary: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Invalid release summary JSON: {e}"))
}

/// Fetch through the native HTTP client: release assets do not allow webview CORS.
/// Only versioned summaries from the two built-in release sources are accepted.
#[tauri::command]
pub async fn fetch_whats_new_summary(
    version: String,
    source: WhatsNewSource,
) -> Result<Value, String> {
    let url = summary_url(&version, source)?;
    fetch_summary(crate::proxy::http_client::get().get(url)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, response::Redirect, routing::get, Json, Router};
    use serde_json::json;

    #[test]
    fn restricts_summary_urls_to_versioned_release_assets() {
        assert_eq!(
            summary_url("4.0.13", WhatsNewSource::Mirror).unwrap(),
            "https://dl.ccswitch.io/v4.0.13/whats-new-4.0.13.json"
        );
        assert_eq!(
            summary_url("4.0.13-beta.1", WhatsNewSource::Github).unwrap(),
            "https://github.com/farion1231/cc-switch/releases/download/v4.0.13-beta.1/whats-new-4.0.13-beta.1.json"
        );
        for version in ["", "v4.0.13", "../4.0.13", "4.0.13/other", "4.0.13?x=1"] {
            assert!(summary_url(version, WhatsNewSource::Mirror).is_err());
        }
        assert!(serde_json::from_str::<WhatsNewSource>(r#""https://example.com""#).is_err());
    }

    #[tokio::test]
    async fn fetches_json_without_cors_headers_and_follows_redirects() {
        let entry = json!({ "version": "4.0.13", "items": [] });
        let app = Router::new()
            .route(
                "/redirect",
                get(|| async { Redirect::temporary("/summary") }),
            )
            .route("/summary", get(move || async { Json(entry) }))
            .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
            .route("/invalid", get(|| async { "not JSON" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let result = fetch_summary(client.get(format!("http://{address}/redirect"))).await;
        let missing = fetch_summary(client.get(format!("http://{address}/missing"))).await;
        let invalid = fetch_summary(client.get(format!("http://{address}/invalid"))).await;
        server.abort();

        assert_eq!(result.unwrap(), json!({ "version": "4.0.13", "items": [] }));
        assert!(missing
            .unwrap_err()
            .starts_with("Failed to fetch release summary:"));
        assert!(invalid
            .unwrap_err()
            .starts_with("Invalid release summary JSON:"));
    }
}
