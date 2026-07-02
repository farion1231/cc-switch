//! Kiro Tauri Commands

use crate::proxy::providers::kiro_auth::KiroAuthManager;
use crate::services::model_fetch::FetchedModel;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Kiro 认证状态
pub struct KiroAuthState(pub Arc<RwLock<KiroAuthManager>>);

/// 获取 Kiro 可用模型列表
#[tauri::command(rename_all = "camelCase")]
pub async fn get_kiro_models(
    account_id: Option<String>,
    state: State<'_, KiroAuthState>,
) -> Result<Vec<FetchedModel>, String> {
    let manager = state.0.read().await;
    fetch_kiro_models(&manager, account_id.as_deref()).await
}

/// 拉取 Kiro 模型列表的底层实现（不依赖 Tauri State，便于 forwarder 预热调用）。
/// 作为副作用，会解析每个模型的 additionalModelRequestFieldsSchema 并写入
/// transform_kiro 的全局能力缓存。
pub(crate) async fn fetch_kiro_models(
    manager: &KiroAuthManager,
    account_id: Option<&str>,
) -> Result<Vec<FetchedModel>, String> {
    // 获取当前账号的有效 Token
    let token = if let Some(id) = account_id {
        manager.get_valid_token_for_account(id).await?
    } else {
        manager.get_valid_token().await?
    };

    // 动态解析当前账号的 Region，并映射为 Kiro Q API 实际部署的 Region
    // （Kiro Q API 仅部署在 us-east-1 / eu-central-1，其他 region 需映射）
    let sso_region = manager.get_region_for_account(account_id).await;
    let resolved_region =
        crate::proxy::providers::kiro_auth::resolve_api_region(sso_region.as_deref());
    let profile_arn = manager.get_profile_arn_for_account(account_id).await;

    log::info!(
        "[Kiro] Fetching models: sso_region={sso_region:?} api_region={resolved_region} has_profile_arn={}",
        profile_arn.is_some()
    );
    let management_url = format!("https://management.{resolved_region}.kiro.dev/");

    // 与参考实现一致：profileArn 为空时不下发该字段（避免发送 profileArn: null 被拒）
    let mut req_body = serde_json::json!({ "origin": "KIRO_CLI" });
    if let Some(arn) = profile_arn.as_ref() {
        req_body["profileArn"] = serde_json::Value::String(arn.clone());
    }

    let client = reqwest::Client::new();
    let mut req = client
        .post(&management_url)
        .header("Content-Type", "application/x-amz-json-1.0")
        .header("Authorization", format!("Bearer {token}"))
        .header(
            "X-Amz-Target",
            "AmazonCodeWhispererService.ListAvailableModels",
        );
    // API key (ksk_) 调用管理面需额外的 tokentype 头，否则被拒绝 Invalid token
    if crate::proxy::providers::kiro_auth::is_api_key(&token) {
        req = req.header("tokentype", "API_KEY");
    }
    let res = req
        .json(&req_body)
        .send()
        .await
        .map_err(|e| format!("获取 Kiro 模型列表网络错误: {e}"))?;

    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        log::warn!("[Kiro] ListAvailableModels 失败 status={status} body={body}");
        return Err(format!("获取 Kiro 模型列表失败: {status} {body}"));
    }

    #[derive(serde::Deserialize)]
    struct EffortSchema {
        #[serde(default)]
        effort: Option<serde_json::Value>,
    }
    #[derive(serde::Deserialize)]
    struct OutputConfigSchema {
        #[serde(default)]
        properties: Option<EffortSchema>,
    }
    #[derive(serde::Deserialize)]
    struct SchemaProperties {
        #[serde(default)]
        thinking: Option<serde_json::Value>,
        #[serde(default)]
        output_config: Option<OutputConfigSchema>,
    }
    #[derive(serde::Deserialize)]
    struct AdditionalSchema {
        #[serde(default)]
        properties: Option<SchemaProperties>,
    }
    #[derive(serde::Deserialize)]
    struct KiroModel {
        #[serde(rename = "modelId")]
        model_id: String,
        #[serde(rename = "additionalModelRequestFieldsSchema", default)]
        additional_schema: Option<AdditionalSchema>,
    }
    #[derive(serde::Deserialize)]
    struct ListModelsResponse {
        models: Option<Vec<KiroModel>>,
    }

    let data: ListModelsResponse = res
        .json()
        .await
        .map_err(|e| format!("解析 Kiro 模型列表响应失败: {e}"))?;

    let re = regex::Regex::new(r"(\d)\.(\d)").unwrap();
    let models = data
        .models
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            // 能力驱动：解析 additionalModelRequestFieldsSchema 并写入全局缓存，
            // 键为 Kiro 侧 modelId（与 anthropic_to_kiro 中 map_model_to_kiro 产出一致）。
            let props = m
                .additional_schema
                .as_ref()
                .and_then(|s| s.properties.as_ref());
            let supports_thinking = props.map(|p| p.thinking.is_some()).unwrap_or(false);
            let supports_effort = props
                .and_then(|p| p.output_config.as_ref())
                .and_then(|o| o.properties.as_ref())
                .map(|e| e.effort.is_some())
                .unwrap_or(false);
            crate::proxy::providers::transform_kiro::set_model_caps(
                &m.model_id,
                crate::proxy::providers::transform_kiro::KiroModelCaps {
                    supports_thinking,
                    supports_effort,
                },
            );

            let mapped_id = re
                .replace_all(&m.model_id, "$1-$2")
                .into_owned()
                .replace('.', "-");
            FetchedModel {
                id: mapped_id,
                owned_by: Some("Kiro".to_string()),
            }
        })
        .collect();

    Ok(models)
}
