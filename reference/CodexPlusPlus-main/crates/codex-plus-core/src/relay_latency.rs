use std::time::{Duration, Instant};

const RELAY_LATENCY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RELAY_LATENCY_TOTAL_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayLatencyMeasurement {
    pub latency_ms: u64,
    pub http_status: u16,
}

pub async fn measure_relay_latency(target_url: &str) -> anyhow::Result<RelayLatencyMeasurement> {
    let url = reqwest::Url::parse(target_url.trim())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        anyhow::bail!("目标 URL 只支持有效的 HTTP 或 HTTPS 地址");
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(RELAY_LATENCY_CONNECT_TIMEOUT)
        .timeout(RELAY_LATENCY_TOTAL_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(format!("CodexPlusPlus/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let started = Instant::now();
    let response = client.head(url).send().await?;
    let latency_ms = started.elapsed().as_millis().max(1) as u64;

    Ok(RelayLatencyMeasurement {
        latency_ms,
        http_status: response.status().as_u16(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[tokio::test]
    async fn measures_http_response_latency_without_requiring_success_status() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });

        let result = measure_relay_latency(&format!("http://{address}/v1"))
            .await
            .unwrap();
        server.join().unwrap();

        assert_eq!(result.http_status, 401);
        assert!(result.latency_ms >= 1);
    }

    #[tokio::test]
    async fn rejects_non_http_urls() {
        let error = measure_relay_latency("file:///tmp/config.toml")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("HTTP"));
    }
}
