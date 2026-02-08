//! 远程模型列表获取命令
//!
//! 通过 /v1/models 接口获取 NewAPI 兼容供应商的可用模型列表。

use crate::proxy::http_client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelInfo {
    pub id: String,
    pub owned_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelEntry {
    id: String,
    owned_by: Option<String>,
}

/// 获取远程模型列表
///
/// 调用 `{baseUrl}/v1/models`（若 baseUrl 已以 `/v1` 结尾则用 `{baseUrl}/models`），
/// 携带 `Authorization: Bearer {apiKey}` 头，解析 OpenAI 标准响应格式。
#[tauri::command]
pub async fn fetch_remote_models(
    base_url: String,
    api_key: String,
) -> Result<Vec<RemoteModelInfo>, String> {
    let base = base_url.trim().trim_end_matches('/');
    let url = if base.ends_with("/v1") {
        format!("{base}/models")
    } else {
        format!("{base}/v1/models")
    };

    log::info!("[RemoteModels] Fetching models from: {url}");

    let client = http_client::get();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        log::warn!("[RemoteModels] API returned {status}: {body}");
        return Err(format!("API returned {status}: {body}"));
    }

    let body: OpenAiModelsResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    let models: Vec<RemoteModelInfo> = body
        .data
        .into_iter()
        .map(|m| RemoteModelInfo {
            id: m.id,
            owned_by: m.owned_by,
        })
        .collect();

    log::info!("[RemoteModels] Fetched {} model(s)", models.len());
    Ok(models)
}
