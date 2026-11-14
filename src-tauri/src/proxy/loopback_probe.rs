//! 本地路由环回自检：按客户端读系统代理的方式访问一次本地路由，看请求能不能回到自己
//!
//! Codex、Grok Build 用 reqwest 发请求，会跟随系统代理：macOS 不读「不使用代理的主机」，
//! Windows 读绕过列表但不认 `127.*` 这类写法。发往 `127.0.0.1` 的请求因此可能被送进
//! Clash 一类的代理，规则把它分给远端节点时就回一个空正文 502，CC Switch 这边一行日志
//! 都没有（#4789、#8611、#9221）。CC Switch 用的是同一个网络库，照同样的规则走一遍，
//! 回不到自己就说明客户端的请求也会被接走。

use std::time::Duration;

/// 自检请求带的随机值，`/health` 原样写回响应头；对得上才算回到了自己
pub const PROBE_HEADER: &str = "x-cc-switch-loopback-probe";

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// 跟随系统代理访问 `<origin>/health`，`origin` 是写进客户端的本地路由地址。回到了自己
/// 返回 `None`，没回来返回原因（只给日志用）；客户端建不起来时判断不了，也返回 `None`。
pub async fn hijack_reason(origin: &str) -> Option<String> {
    // 不能用 http_client 里的客户端：那些对本机地址一律直连，也会套上 CC Switch 自己的出站代理，
    // 和客户端看到的系统代理不是一回事。
    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => {
            log::debug!("[LOOPBACK] 建自检客户端失败，跳过: {error}");
            return None;
        }
    };
    check(&client, origin).await
}

async fn check(client: &reqwest::Client, origin: &str) -> Option<String> {
    let nonce = uuid::Uuid::new_v4().to_string();
    let response = match client
        .get(format!("{origin}/health"))
        .header(PROBE_HEADER, &nonce)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return Some(format!("请求失败: {error}")),
    };
    let echoed = response
        .headers()
        .get(PROBE_HEADER)
        .and_then(|value| value.to_str().ok());
    if response.status().is_success() && echoed == Some(nonce.as_str()) {
        None
    } else {
        Some(format!(
            "返回的不是本地路由的响应（HTTP {}）",
            response.status()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::proxy::server::ProxyServer;
    use crate::proxy::ProxyConfig;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// 每个连接读一次请求，回一份写死的响应（扮演把请求接走的系统代理或别的程序）
    async fn canned_server(response: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        port
    }

    // 测试一律显式指定代理或不用代理，不受开发机系统代理影响。
    fn direct() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    fn via(proxy_port: u16) -> reqwest::Client {
        reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{proxy_port}")).unwrap())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn reaching_the_local_route_passes() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let proxy = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                ..ProxyConfig::default()
            },
            db,
            None,
        );
        let info = proxy.start().await.expect("start test proxy");
        let origin = format!("http://127.0.0.1:{}", info.port);

        assert_eq!(check(&direct(), &origin).await, None);

        proxy.stop().await.expect("stop test proxy");
    }

    /// #9221：Clash 规则把环回地址分给远端节点时回空正文 502
    #[tokio::test]
    async fn empty_502_from_system_proxy_is_reported() {
        let clash = canned_server(
            "HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        )
        .await;
        let reason = check(&via(clash), "http://127.0.0.1:9").await;
        assert!(reason.is_some_and(|reason| reason.contains("502")));
    }

    /// 端口上回 200 的不是本地路由（没把随机值写回来），也算没回到自己
    #[tokio::test]
    async fn success_without_echoed_nonce_is_reported() {
        let other =
            canned_server("HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .await;
        let origin = format!("http://127.0.0.1:{other}");
        assert!(check(&direct(), &origin).await.is_some());
    }

    #[tokio::test]
    async fn unreachable_system_proxy_is_reported() {
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let reason = check(&via(port), "http://127.0.0.1:9").await;
        assert!(reason.is_some_and(|reason| reason.contains("请求失败")));
    }
}
