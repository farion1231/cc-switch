//! 模型列表获取命令
//!
//! 提供 Tauri 命令，供前端在供应商表单中获取可用模型列表。

use crate::services::model_fetch::{self, FetchedModel};

/// 获取供应商的可用模型列表
///
/// 使用 OpenAI 兼容的 GET /v1/models 端点。
/// 主要面向第三方聚合站（硅基流动、OpenRouter 等）。
#[tauri::command(rename_all = "camelCase")]
pub async fn fetch_models_for_config(
    base_url: String,
    api_key: String,
    is_full_url: Option<bool>,
) -> Result<Vec<FetchedModel>, String> {
    model_fetch::fetch_models(&base_url, &api_key, is_full_url.unwrap_or(false)).await
}
