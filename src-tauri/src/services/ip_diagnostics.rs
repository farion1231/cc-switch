//! On-demand exit IP observations. These samples do not prove traffic isolation.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::Instant;

const EXIT_URLS: [&str; 2] = ["https://ipv4.icanhazip.com/", "https://api.ipify.org/"];
const LOCATION_URL: &str = "https://ipwho.is/";
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticError {
    Network,
    Http,
    InvalidResponse,
    Unavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitSample {
    pub source: &'static str,
    pub ip: Option<IpAddr>,
    pub latency_ms: u64,
    pub error: Option<DiagnosticError>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Connection {
    pub asn: Option<u32>,
    pub isp: Option<String>,
    pub org: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Timezone {
    pub id: Option<String>,
    pub utc: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IpLocation {
    pub ip: IpAddr,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub connection: Option<Connection>,
    pub timezone: Option<Timezone>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpReport {
    pub samples: Vec<ExitSample>,
    pub location: Option<IpLocation>,
    pub location_error: Option<DiagnosticError>,
}

pub async fn diagnose(proxy_url: &str) -> Result<IpReport, String> {
    let client = crate::proxy::http_client::diagnostic_client(proxy_url)?;
    Ok(observe(&client, EXIT_URLS, LOCATION_URL).await)
}

async fn read_response(client: &Client, url: &str) -> Result<Vec<u8>, DiagnosticError> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| DiagnosticError::Network)?;
    if !response.status().is_success() {
        return Err(DiagnosticError::Http);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| DiagnosticError::Network)?
    {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(DiagnosticError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn sample(client: &Client, source: &'static str, url: &str) -> ExitSample {
    let start = Instant::now();
    let result = read_response(client, url)
        .await
        .and_then(|body| parse_ip(&body));
    ExitSample {
        source,
        ip: result.as_ref().ok().copied(),
        latency_ms: start.elapsed().as_millis() as u64,
        error: result.err(),
    }
}

fn parse_ip(body: &[u8]) -> Result<IpAddr, DiagnosticError> {
    std::str::from_utf8(body)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .ok_or(DiagnosticError::InvalidResponse)
}

fn parse_location(body: &[u8], expected_ip: IpAddr) -> Result<IpLocation, DiagnosticError> {
    #[derive(Deserialize)]
    struct Response {
        success: bool,
        #[serde(flatten)]
        data: serde_json::Value,
    }
    let response: Response =
        serde_json::from_slice(body).map_err(|_| DiagnosticError::InvalidResponse)?;
    if !response.success {
        return Err(DiagnosticError::Unavailable);
    }
    let location: IpLocation =
        serde_json::from_value(response.data).map_err(|_| DiagnosticError::InvalidResponse)?;
    if location.ip != expected_ip {
        return Err(DiagnosticError::InvalidResponse);
    }
    Ok(location)
}

async fn observe(client: &Client, exit_urls: [&str; 2], location_url: &str) -> IpReport {
    let (primary, secondary) = tokio::join!(
        sample(client, "icanhazip.com", exit_urls[0]),
        sample(client, "ipify.org", exit_urls[1]),
    );
    let mut report = IpReport {
        samples: vec![primary, secondary],
        location: None,
        location_error: None,
    };
    // Query the observed address explicitly so a rotating proxy cannot silently
    // attach another connection's location to this sample.
    if let Some(ip) = report.samples.iter().find_map(|sample| sample.ip) {
        let result = read_response(client, &format!("{location_url}{ip}"))
            .await
            .and_then(|body| parse_location(&body, ip));
        match result {
            Ok(location) => report.location = Some(location),
            Err(error) => report.location_error = Some(error),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, routing::get, Router};
    use std::time::Duration;

    const LOCATION: &str = r#"{"success":true,"ip":"203.0.113.10","country":"Example","region":"Region","city":"City","connection":{"asn":64500,"isp":"Example ISP","org":"Example Org"},"timezone":{"id":"Etc/UTC","utc":"+00:00"}}"#;

    struct TestServer {
        url: String,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn server() -> TestServer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/primary", get(|| async { "203.0.113.10\n" }))
            .route("/secondary", get(|| async { "203.0.113.11" }))
            .route("/invalid", get(|| async { "<html>not an IP</html>" }))
            .route("/limited", get(|| async { StatusCode::TOO_MANY_REQUESTS }))
            .route(
                "/large",
                get(|| async { "x".repeat(MAX_RESPONSE_BYTES + 1) }),
            )
            .route("/geo/203.0.113.10", get(|| async { LOCATION }))
            .route(
                "/invalid-geo/203.0.113.10",
                get(|| async { r#"{"success":true,"ip":"203.0.113.11"}"# }),
            )
            .route(
                "/redirect",
                get(|| async { axum::response::Redirect::temporary("/primary") }),
            )
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    "203.0.113.10"
                }),
            );
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        TestServer { url, task }
    }

    fn client() -> Client {
        Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(500))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    #[test]
    fn parses_ip_addresses_and_rejects_invalid_samples() {
        for value in [" 203.0.113.10\n", "2001:db8::1"] {
            assert_eq!(
                parse_ip(value.as_bytes()).unwrap(),
                value.trim().parse::<IpAddr>().unwrap()
            );
        }
        for value in [b"".as_slice(), b"not an IP", &[0xff], b"203.0.113.10 extra"] {
            assert_eq!(parse_ip(value), Err(DiagnosticError::InvalidResponse));
        }
    }

    #[test]
    fn location_is_bound_to_observed_ip_and_preserves_missing_fields() {
        let ip = "203.0.113.10".parse().unwrap();
        let location = parse_location(LOCATION.as_bytes(), ip).unwrap();
        assert_eq!(location.connection.unwrap().asn, Some(64500));
        assert_eq!(location.timezone.unwrap().id.as_deref(), Some("Etc/UTC"));
        let minimal = parse_location(br#"{"success":true,"ip":"203.0.113.10"}"#, ip).unwrap();
        assert!(minimal.country.is_none());
        assert!(minimal.connection.is_none());
        for body in [
            b"invalid".as_slice(),
            br#"{"ip":"203.0.113.10"}"#,
            br#"{"success":true,"ip":"bad"}"#,
            br#"{"success":true,"ip":"203.0.113.11"}"#,
            br#"{"success":true,"ip":"203.0.113.10","connection":{"asn":"bad"}}"#,
        ] {
            assert_eq!(
                parse_location(body, ip).unwrap_err(),
                DiagnosticError::InvalidResponse
            );
        }
        assert_eq!(
            parse_location(br#"{"success":false,"message":"reserved range"}"#, ip).unwrap_err(),
            DiagnosticError::Unavailable
        );
    }

    #[tokio::test]
    async fn reports_both_samples_and_looks_up_the_first_observed_ip() {
        let server = server().await;
        let report = observe(
            &client(),
            [
                &format!("{}/primary", server.url),
                &format!("{}/secondary", server.url),
            ],
            &format!("{}/geo/", server.url),
        )
        .await;
        assert_eq!(report.samples[0].ip.unwrap().to_string(), "203.0.113.10");
        assert_eq!(report.samples[1].ip.unwrap().to_string(), "203.0.113.11");
        assert!(report.samples.iter().all(|sample| sample.error.is_none()));
        assert_eq!(
            report.location.as_ref().unwrap().ip,
            report.samples[0].ip.unwrap()
        );
        assert!(report.location_error.is_none());
        let json = serde_json::to_value(&report).unwrap();
        assert!(json["samples"][0]["latencyMs"].is_number());
        assert!(json["locationError"].is_null());
    }

    #[tokio::test]
    async fn preserves_partial_success_and_skips_location_without_an_ip() {
        let server = server().await;
        let bad = format!("{}/invalid", server.url);
        let good = format!("{}/primary", server.url);
        let geo = format!("{}/geo/", server.url);
        let report = observe(&client(), [&bad, &good], &geo).await;
        assert_eq!(
            report.samples[0].error,
            Some(DiagnosticError::InvalidResponse)
        );
        assert!(report.location.is_some());
        let report = observe(&client(), [&bad, &bad], "invalid URL").await;
        assert!(report.samples.iter().all(|sample| sample.ip.is_none()));
        assert!(report.location.is_none());
        assert!(report.location_error.is_none());
        let report = observe(
            &client(),
            [&good, &good],
            &format!("{}/missing/", server.url),
        )
        .await;
        assert!(report.samples.iter().all(|sample| sample.ip.is_some()));
        assert_eq!(report.location_error, Some(DiagnosticError::Http));
        let report = observe(
            &client(),
            [&good, &good],
            &format!("{}/invalid-geo/", server.url),
        )
        .await;
        assert!(report.location.is_none());
        assert_eq!(
            report.location_error,
            Some(DiagnosticError::InvalidResponse)
        );
    }

    #[tokio::test]
    async fn handles_http_errors_redirects_timeout_and_oversized_bodies() {
        let server = server().await;
        for (path, expected) in [
            ("limited", DiagnosticError::Http),
            ("redirect", DiagnosticError::Http),
            ("large", DiagnosticError::InvalidResponse),
            ("slow", DiagnosticError::Network),
        ] {
            assert_eq!(
                read_response(&client(), &format!("{}/{path}", server.url))
                    .await
                    .unwrap_err(),
                expected
            );
        }
        assert_eq!(
            read_response(&client(), "invalid URL").await.unwrap_err(),
            DiagnosticError::Network
        );
    }

    #[tokio::test]
    async fn rejects_truncated_response_body() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nx")
                .await
                .unwrap();
        });
        assert_eq!(
            read_response(&client(), &url).await.unwrap_err(),
            DiagnosticError::Network
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_proxy_without_exposing_credentials() {
        assert_eq!(
            diagnose("ftp://user:secret@example.com").await.unwrap_err(),
            "invalidProxy"
        );
    }

    #[tokio::test]
    async fn failed_explicit_proxy_does_not_fall_back_to_direct_connection() {
        let server = server().await;
        let blocked = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = format!("http://user:secret@{}", blocked.local_addr().unwrap());
        drop(blocked);
        let client = crate::proxy::http_client::diagnostic_client(&proxy).unwrap();
        let sample = sample(&client, "test", &format!("{}/primary", server.url)).await;
        assert_eq!(sample.error, Some(DiagnosticError::Network));
        assert!(sample.ip.is_none());
        assert!(!serde_json::to_string(&sample).unwrap().contains("secret"));
    }
}
