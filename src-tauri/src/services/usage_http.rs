//! Shared reply handling for the balance and coding plan queries.

use reqwest::{RequestBuilder, StatusCode};

/// A definite answer from a usage endpoint.
pub(crate) enum JsonReply {
    /// A 2xx response with a JSON body.
    Body(serde_json::Value),
    /// HTTP 401 or 403.
    Denied(StatusCode),
    /// Any other status, or a complete body that is not JSON. The text is shown as is.
    Failed(String),
}

/// Sends `request` and reads its JSON reply.
///
/// `Err` means the request didn't complete (send failed or the body was cut off), which is
/// transient and gets retried. Everything the server decided comes back as a [`JsonReply`].
pub(crate) async fn send_for_json(request: RequestBuilder) -> Result<JsonReply, String> {
    let resp = request
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Ok(JsonReply::Denied(status));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(JsonReply::Failed(format!(
            "API error (HTTP {status}): {body}"
        )));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {e}"))?;
    Ok(match serde_json::from_slice(&raw) {
        Ok(body) => JsonReply::Body(body),
        Err(e) => JsonReply::Failed(format!("Failed to parse response: {e}")),
    })
}

/// 解析 JSON 值为 f64，兼容数字和字符串格式（如 `100` 和 `"100"`）
pub(crate) fn parse_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}
