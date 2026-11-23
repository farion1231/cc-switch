//! Sending the prepared request upstream over the pooled reqwest client or
//! the header-case-preserving hyper path.

use super::*;

impl RequestForwarder {
    /// Send one prepared request. Streaming requests bound only the wait for
    /// response headers (first-byte timeout); the body is left to the stream
    /// processor's idle timeout.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn send_upstream(
        &self,
        url: &str,
        target_for_log: &str,
        method: &http::Method,
        headers: http::HeaderMap,
        extensions: &Extensions,
        body_bytes: Vec<u8>,
        request_is_streaming: bool,
        preserve_exact_header_case: bool,
    ) -> Result<ProxyResponse, ProxyError> {
        // 确定超时
        let timeout = if self.non_streaming_timeout.is_zero() {
            std::time::Duration::from_secs(600) // 默认 600 秒
        } else {
            self.non_streaming_timeout
        };

        // 流式请求只约束「等响应头」这一段，用首包超时；body 交给 response_processor
        // 的静默期超时。两条传输路径必须用同一个值，否则 Claude（hyper）会按非流式
        // 超时等 10 分钟，故障转移迟迟不触发。
        let header_timeout = if request_is_streaming {
            if self.streaming_first_byte_timeout.is_zero() {
                timeout
            } else {
                self.streaming_first_byte_timeout
            }
        } else {
            timeout
        };

        // 获取全局代理 URL（上游是本机地址时直连，不送进代理）
        let upstream_proxy_url: Option<String> = crate::proxy::http_client::proxy_url_for(url);

        // SOCKS5 代理不支持 CONNECT 隧道，需要用 reqwest
        let is_socks_proxy = upstream_proxy_url
            .as_deref()
            .map(|u| u.starts_with("socks5"))
            .unwrap_or(false);

        // 发送请求
        Ok(if is_socks_proxy || !preserve_exact_header_case {
            // OpenAI / Copilot / Codex 类后端不依赖原始 header 大小写；走 reqwest
            // 连接池，避免 raw TCP/TLS path 每次请求都重新握手。SOCKS5 也只能走 reqwest。
            log::debug!(
                "[Forwarder] Using pooled reqwest client (preserve_exact_header_case={preserve_exact_header_case}, socks_proxy={is_socks_proxy})"
            );
            let client = crate::proxy::http_client::get_for_url(url);
            let mut request = client.request(method.clone(), url);
            if request_is_streaming {
                // reqwest 的 timeout 是整请求超时；流式请求交给 response_processor
                // 的首包/静默期超时控制，避免长流被总时长误杀。
                request = request.timeout(std::time::Duration::from_secs(24 * 60 * 60));
            } else if !self.non_streaming_timeout.is_zero() {
                request = request.timeout(self.non_streaming_timeout);
            }
            for (key, value) in &headers {
                request = request.header(key, value);
            }
            let send = request.body(body_bytes).send();
            let send_result = if request_is_streaming {
                tokio::time::timeout(header_timeout, send)
                    .await
                    .map_err(|_| {
                        ProxyError::Timeout(format!(
                            "流式响应首包超时: {}s（上游未返回响应头）",
                            header_timeout.as_secs()
                        ))
                    })?
            } else {
                send.await
            };
            let reqwest_resp = send_result.map_err(map_reqwest_send_error)?;
            ProxyResponse::Reqwest(reqwest_resp)
        } else {
            // HTTP 代理或直连：走 hyper raw write（保持 header 大小写）
            // 如果有 HTTP 代理，hyper_client 会用 CONNECT 隧道穿过代理
            let uri: http::Uri = url.parse().map_err(|e| {
                ProxyError::ForwardFailed(format!("Invalid upstream URL ({target_for_log}): {e}"))
            })?;
            crate::proxy::hyper_client::send_request(
                uri,
                target_for_log,
                method.clone(),
                headers,
                extensions.clone(),
                body_bytes,
                header_timeout,
                upstream_proxy_url.as_deref(),
            )
            .await?
        })
    }
}
