//! Localhost-only secure bridge with Bearer nonce auth.

use crate::error::AppError;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

#[derive(Debug)]
#[allow(dead_code)]
pub struct BridgeHandle {
    pub port: u16,
    pub nonce: String,
    pub instance_id: String,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl BridgeHandle {
    /// Signal accept-loop exit and wait until the listener task ends.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for BridgeHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        // Best-effort: do not block Drop; prefer explicit shutdown() on reinject.
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Start a minimal HTTP bridge bound to 127.0.0.1 only.
pub async fn start_bridge(instance_id: &str) -> Result<BridgeHandle, AppError> {
    let nonce = Uuid::new_v4().to_string();
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|e| AppError::Config(format!("bridge bind: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::Config(format!("bridge addr: {e}")))?
        .port();

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    let expected = Arc::new(format!("Bearer {nonce}"));
    let instance = instance_id.to_string();

    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                accept = listener.accept() => {
                    let Ok((mut socket, peer)) = accept else { continue };
                    if !peer.ip().is_loopback() {
                        continue;
                    }
                    let expected = expected.clone();
                    let instance = instance.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        let n = match socket.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        let req = String::from_utf8_lossy(&buf[..n]);
                        let first = req.lines().next().unwrap_or("");
                        let authorized = req
                            .lines()
                            .any(|l| l.eq_ignore_ascii_case(&format!("Authorization: {expected}")));

                        // CORS preflight for page fetch from app:// Codex.
                        if first.starts_with("OPTIONS ") {
                            let resp = "HTTP/1.1 204 No Content\r\n\
Access-Control-Allow-Origin: *\r\n\
Access-Control-Allow-Headers: Authorization, Content-Type\r\n\
Access-Control-Allow-Methods: GET, OPTIONS\r\n\
Content-Length: 0\r\n\
Connection: close\r\n\r\n";
                            let _ = socket.write_all(resp.as_bytes()).await;
                            return;
                        }

                        let (status, body) = if !authorized {
                            ("401 Unauthorized", "{\"error\":\"unauthorized\"}")
                        } else if first.starts_with("GET /health") {
                            ("200 OK", "{\"ok\":true}")
                        } else if first.starts_with("GET /instance") {
                            ("200 OK", "")
                        } else {
                            ("404 Not Found", "{\"error\":\"not_found\"}")
                        };
                        let body = if first.starts_with("GET /instance") && authorized {
                            format!("{{\"instanceId\":\"{instance}\"}}")
                        } else {
                            body.to_string()
                        };
                        let resp = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\nAccess-Control-Allow-Methods: GET, OPTIONS\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(resp.as_bytes()).await;
                    });
                }
            }
        }
    });

    Ok(BridgeHandle {
        port,
        nonce,
        instance_id: instance_id.to_string(),
        shutdown: Some(shutdown_tx),
        task: Some(task),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unauthorized_bridge_requests_fail() {
        let bridge = start_bridge("inst-1").await.expect("start");
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{}/health", bridge.port);
        let resp = client.get(&url).send().await.expect("req");
        assert_eq!(resp.status().as_u16(), 401);

        let resp_ok = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", bridge.nonce))
            .send()
            .await
            .expect("req2");
        assert!(resp_ok.status().is_success());
    }

    #[tokio::test]
    async fn shutdown_stops_accept_loop() {
        let bridge = start_bridge("inst-stop").await.expect("start");
        let port = bridge.port;
        bridge.shutdown().await;

        // Port should no longer accept after awaited shutdown.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(400))
            .build()
            .expect("client");
        let url = format!("http://127.0.0.1:{port}/health");
        let err = client.get(&url).send().await;
        assert!(err.is_err(), "expected connection failure after shutdown");
    }
}
