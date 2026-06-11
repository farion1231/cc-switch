//! Kiro Tauri Commands

use crate::proxy::providers::kiro_auth::{KiroAuthManager, KiroAccountData};
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

    // 获取当前账号的有效 Token
    let token = if let Some(ref id) = account_id {
        manager.get_valid_token_for_account(id).await?
    } else {
        manager.get_valid_token().await?
    };

    // 获取当前账号的 Region
    let region = if let Some(ref id) = account_id {
        let local = manager.list_accounts().await;
        let acc = local.iter().find(|a| a.id == *id);
        if let Some(a) = acc {
            // Find in internal maps to get the actual region
            let mut reg = "us-east-1".to_string();
            if id.starts_with("kiro_cli_") {
                let method = if id.ends_with("social") { "desktop" } else { "idc" };
                // Call dynamic cli account loader
                if let Some(cli_acc) = manager.get_profile_arn_for_account(id) {
                    // profile arn is not region, let's keep region search
                }
            }
            // Let's just check the region from local_accounts
            let local_guard = manager.list_accounts().await;
            // A simpler way: we can implement a method on KiroAuthManager to get region by account id
            // Let's implement that in KiroAuthManager!
            "us-east-1".to_string() // fallback
        } else {
            "us-east-1".to_string()
        }
    } else {
        "us-east-1".to_string()
    };

    // We can implement `get_region_for_account` in KiroAuthManager, let's update KiroAuthManager.
    // Let's get the region dynamically.
    let resolved_region = manager.get_region_for_account(account_id.as_deref()).await.unwrap_or_else(|| "us-east-1".to_string());
    let profile_arn = manager.get_profile_arn_for_account(account_id.as_deref().unwrap_or("")).await;

    log::info!("[Kiro] Fetching models for region={resolved_region}");
    let management_url = format!("https://management.{resolved_region}.kiro.dev/");

    let client = reqwest::Client::new();
    let res = client.post(&management_url)
        .header("Content-Type", "application/x-amz-json-1.0")
        .header("Authorization", format!("Bearer {token}"))
        .header("X-Amz-Target", "AmazonCodeWhispererService.ListAvailableModels")
        .json(&serde_json::json!({
            "origin": "KIRO_CLI",
            "profileArn": profile_arn
        }))
        .send()
        .await
        .map_err(|e| format!("获取 Kiro 模型列表网络错误: {e}"))?;

    if !res.status().is_success() {
        return Err(format!("获取 Kiro 模型列表失败: {}", res.status()));
    }

    #[derive(serde::Deserialize)]
    struct KiroModel {
        #[serde(rename = "modelId")]
        model_id: String,
    }
    #[derive(serde::Deserialize)]
    struct ListModelsResponse {
        models: Option<Vec<KiroModel>>,
    }

    let data: ListModelsResponse = res.json()
        .await
        .map_err(|e| format!("解析 Kiro 模型列表响应失败: {e}"))?;

    let re = regex::Regex::new(r"(\d)\.(\d)").unwrap();
    let models = data.models.unwrap_or_default()
        .into_iter()
        .map(|m| {
            let mapped_id = re.replace_all(&m.model_id, "$1-$2").into_owned().replace('.', "-");
            FetchedModel {
                id: mapped_id,
                owned_by: Some("Kiro".to_string()),
            }
        })
        .collect();

    Ok(models)
}
