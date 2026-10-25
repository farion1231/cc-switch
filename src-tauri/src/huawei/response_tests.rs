use super::*;
use crate::{
    database::Database,
    provider::{Provider, ProviderMeta},
};
use std::sync::Arc;

async fn transform_huawei_sse(
    sse: &str,
    is_stream: bool,
    provider_type: &str,
) -> Result<axum::response::Response, ProxyError> {
    let state = ProxyState::for_test(Arc::new(Database::memory().unwrap()));
    let mut provider = Provider::with_id("huawei".into(), "Huawei".into(), json!({}), None);
    provider.meta = Some(ProviderMeta {
        provider_type: Some(provider_type.into()),
        ..Default::default()
    });
    let body = json!({"model":"claude-haiku-4-5", "stream":is_stream, "messages":[]});
    let ctx = RequestContext::new(
        &state,
        &body,
        &axum::http::HeaderMap::new(),
        AppType::ClaudeDesktop,
        "Claude Desktop",
        "claude-desktop",
        Some(crate::mode::stack::StackTarget {
            provider,
            upstream_model: "maas-glm-5.2-tencent-codeagent".into(),
            original_model: "claude-haiku-4-5".into(),
        }),
    )
    .await
    .unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("content-type", "text/event-stream".parse().unwrap());
    headers.insert("content-length", sse.len().to_string().parse().unwrap());
    let upstream = super::super::hyper_client::ProxyResponse::buffered(
        axum::http::StatusCode::OK,
        headers,
        Bytes::copy_from_slice(sse.as_bytes()),
    );
    handle_claude_transform(
        upstream,
        &ctx,
        &state,
        &body,
        is_stream,
        "openai_chat",
        None,
    )
    .await
}

const SSE: &str = concat!(
    "data:{\"id\":\"chat_1\",\"model\":\"maas-glm-5.2\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK.\"},\"finish_reason\":\"stop\"}]}\n\n",
    "data:{\"choices\":[],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":3,\"total_tokens\":11}}\n\n",
    "data:[DONE]\n\n"
);

#[tokio::test]
async fn huawei_non_streaming_probe_returns_complete_json() {
    let response = transform_huawei_sse(SSE, false, "huawei_codeagent_3")
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "application/json");
    assert!(!response.headers().contains_key("content-length"));
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["type"], "message");
    assert_eq!(body["content"][0]["text"], "OK.");
    assert_eq!(body["stop_reason"], "end_turn");
    assert_eq!(body["usage"]["input_tokens"], 8);
    assert_eq!(body["usage"]["output_tokens"], 3);
}

#[tokio::test]
async fn huawei_streaming_chat_still_returns_events() {
    let response = transform_huawei_sse(SSE, true, "huawei_codeagent_3")
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("message_stop"));
    assert!(body.contains("OK."));
}

#[tokio::test]
async fn huawei_non_streaming_probe_rejects_truncated_upstream() {
    let partial = "data:{\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n";
    assert!(transform_huawei_sse(partial, false, "huawei_codeagent_3")
        .await
        .is_err());
}

#[tokio::test]
async fn ordinary_chat_sse_keeps_existing_streaming_fallback() {
    let response = transform_huawei_sse(SSE, false, "ordinary").await.unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
}
