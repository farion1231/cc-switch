//! 供应商连通性检查命令
//!
//! 注意：本检查只探测 base_url 是否可达，不发真实大模型请求，也不触碰故障转移
//! 熔断器（熔断器由真实转发流量驱动）。详见 `services::stream_check`。

use crate::app_config::AppType;
use crate::commands::copilot::CopilotAuthState;
use crate::commands::kiro::KiroAuthState;
use crate::error::AppError;
use crate::services::stream_check::{
    HealthStatus, StreamCheckConfig, StreamCheckResult, StreamCheckService,
};
use crate::store::AppState;
use std::collections::HashSet;
use tauri::State;

/// 连通性检查（单个供应商）
#[tauri::command]
pub async fn stream_check_provider(
    state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    kiro_state: State<'_, KiroAuthState>,
    app_type: AppType,
    provider_id: String,
) -> Result<StreamCheckResult, AppError> {
    let config = state.db.get_stream_check_config()?;

    let providers = state.db.get_all_providers(app_type.as_str())?;
    let provider = providers
        .get(&provider_id)
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

    // Kiro（托管 OAuth）走专用连通性探测
    if provider.is_kiro() {
        let result = check_kiro_provider(provider, &config, &kiro_state).await;
        let _ = state.db.save_stream_check_log(
            &provider_id,
            &provider.name,
            app_type.as_str(),
            &result,
        );
        return Ok(result);
    }

    // Copilot 端点是动态的（随 OAuth token 解析），需预先取出 host 再探测；
    // 其余供应商传 None，由服务层从 settings_config 提取 base_url。无需鉴权。
    let base_url_override = resolve_copilot_base_url_override(provider, &copilot_state).await?;
    let result =
        StreamCheckService::check_with_retry(&app_type, provider, &config, base_url_override)
            .await?;

    // 记录日志
    let _ =
        state
            .db
            .save_stream_check_log(&provider_id, &provider.name, app_type.as_str(), &result);

    Ok(result)
}

/// 批量连通性检查
#[tauri::command]
pub async fn stream_check_all_providers(
    state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    kiro_state: State<'_, KiroAuthState>,
    app_type: AppType,
    proxy_targets_only: bool,
) -> Result<Vec<(String, StreamCheckResult)>, AppError> {
    let config = state.db.get_stream_check_config()?;
    let providers = state.db.get_all_providers(app_type.as_str())?;

    let allowed_ids: Option<HashSet<String>> = if proxy_targets_only {
        let mut ids = HashSet::new();
        if let Ok(Some(current_id)) = state.db.get_current_provider(app_type.as_str()) {
            ids.insert(current_id);
        }
        if let Ok(queue) = state.db.get_failover_queue(app_type.as_str()) {
            for item in queue {
                ids.insert(item.provider_id);
            }
        }
        Some(ids)
    } else {
        None
    };

    let mut results = Vec::new();
    for (id, provider) in providers {
        if let Some(ids) = &allowed_ids {
            if !ids.contains(&id) {
                continue;
            }
        }

        // Kiro（托管 OAuth）走专用连通性探测
        if provider.is_kiro() {
            let result = check_kiro_provider(&provider, &config, &kiro_state).await;
            let _ = state
                .db
                .save_stream_check_log(&id, &provider.name, app_type.as_str(), &result);
            results.push((id, result));
            continue;
        }
        let base_url_override =
            resolve_copilot_base_url_override(&provider, &copilot_state).await?;
        let result =
            StreamCheckService::check_with_retry(&app_type, &provider, &config, base_url_override)
                .await
                .unwrap_or_else(|e| StreamCheckResult {
                    status: HealthStatus::Failed,
                    success: false,
                    message: e.to_string(),
                    response_time_ms: None,
                    http_status: None,
                    model_used: String::new(),
                    tested_at: chrono::Utc::now().timestamp(),
                    retry_count: 0,
                    error_category: None,
                });

        let _ = state
            .db
            .save_stream_check_log(&id, &provider.name, app_type.as_str(), &result);

        results.push((id, result));
    }

    Ok(results)
}

/// 获取连通性检查配置
#[tauri::command]
pub fn get_stream_check_config(state: State<'_, AppState>) -> Result<StreamCheckConfig, AppError> {
    state.db.get_stream_check_config()
}

/// 保存连通性检查配置
#[tauri::command]
pub fn save_stream_check_config(
    state: State<'_, AppState>,
    config: StreamCheckConfig,
) -> Result<(), AppError> {
    state.db.save_stream_check_config(&config)
}

/// Kiro（AWS CodeWhisperer，托管 OAuth）专用连通性探测。
///
/// 与代理转发路径一致：解析 OAuth token 与区域，向
/// runtime.{api_region}.kiro.dev/ POST 一个最小 Kiro 运行时请求（带 X-Amz-Target
/// 等 AWS 头），HTTP 200 视为健康。避免走通用 Anthropic 预检路径造成假阴性。
async fn check_kiro_provider(
    provider: &crate::provider::Provider,
    config: &StreamCheckConfig,
    kiro_state: &State<'_, KiroAuthState>,
) -> StreamCheckResult {
    let model = provider
        .settings_config
        .pointer("/env/ANTHROPIC_MODEL")
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .to_string();
    let now = chrono::Utc::now().timestamp();

    let failed = |message: String, http_status: Option<u16>| StreamCheckResult {
        status: HealthStatus::Failed,
        success: false,
        message,
        response_time_ms: None,
        http_status,
        model_used: model.clone(),
        tested_at: now,
        retry_count: 0,
        error_category: None,
    };

    let account_id = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("kiro"));

    // 1) 解析 token / 区域 / profileArn
    let (token, sso_region, profile_arn) = {
        let auth_manager = kiro_state.0.read().await;
        let token = match account_id.as_deref() {
            Some(id) => auth_manager.get_valid_token_for_account(id).await,
            None => auth_manager.get_valid_token().await,
        };
        let token = match token {
            Ok(t) => t,
            Err(e) => return failed(format!("Kiro 认证失败: {e}"), None),
        };
        let sso_region = auth_manager
            .get_region_for_account(account_id.as_deref())
            .await;
        let profile_arn = auth_manager
            .get_profile_arn_for_account(account_id.as_deref())
            .await;
        (token, sso_region, profile_arn)
    };

    let api_region = crate::proxy::providers::kiro_auth::resolve_api_region(sso_region.as_deref());
    let url = format!("https://runtime.{api_region}.kiro.dev/");

    // 2) 构造最小 Kiro 运行时请求体
    let kiro_model_id = crate::proxy::providers::transform_kiro::map_model_to_kiro(&model);
    let current_message = serde_json::json!({
        "userInputMessage": {
            "content": "ping",
            "modelId": kiro_model_id,
            "origin": "KIRO_CLI"
        }
    });
    let mut body = serde_json::json!({
        "conversationState": {
            "chatTriggerType": "MANUAL",
            "agentTaskType": "vibe",
            "conversationId": uuid::Uuid::new_v4().to_string(),
            "currentMessage": current_message
        },
        "agentMode": "vibe"
    });
    if let Some(arn) = profile_arn.as_deref() {
        body["profileArn"] = serde_json::Value::String(arn.to_string());
    }

    // 3) 发送请求
    let timeout = std::time::Duration::from_secs(config.timeout_secs.max(1));
    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(c) => c,
        Err(e) => return failed(format!("创建 HTTP 客户端失败: {e}"), None),
    };
    let started = std::time::Instant::now();
    // 运行面要求 user-agent 包含 app/AmazonQ-For-CLI；API key 还需 tokentype 头
    let ua = "aws-sdk-rust/1.3.15 ua/2.1 api/codewhispererstreaming/0.1.16551 os/macos lang/rust/1.92.0 md/appVersion-2.7.0 app/AmazonQ-For-CLI";
    let amz_ua = "aws-sdk-rust/1.3.15 ua/2.1 api/codewhispererstreaming/0.1.16551 os/macos lang/rust/1.92.0 m/F app/AmazonQ-For-CLI";
    let mut req = client
        .post(&url)
        .header("content-type", "application/x-amz-json-1.0")
        .header("accept", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header(
            "x-amz-target",
            "AmazonCodeWhispererStreamingService.GenerateAssistantResponse",
        )
        .header("x-amzn-codewhisperer-optout", "true")
        .header("x-amzn-kiro-agent-mode", "vibe")
        .header("amz-sdk-invocation-id", uuid::Uuid::new_v4().to_string())
        .header("amz-sdk-request", "attempt=1; max=1")
        .header("user-agent", ua)
        .header("x-amz-user-agent", amz_ua);
    if crate::proxy::providers::kiro_auth::is_api_key(&token) {
        req = req.header("tokentype", "API_KEY");
    }
    let resp = req
        .body(serde_json::to_vec(&body).unwrap_or_default())
        .send()
        .await;

    let response = match resp {
        Ok(r) => r,
        Err(e) => return failed(format!("请求失败: {e}"), None),
    };
    let status = response.status();
    let elapsed_ms = started.elapsed().as_millis() as u64;

    if status.is_success() {
        let degraded = elapsed_ms > config.degraded_threshold_ms;
        StreamCheckResult {
            status: if degraded {
                HealthStatus::Degraded
            } else {
                HealthStatus::Operational
            },
            success: true,
            message: if degraded {
                format!("响应较慢 ({elapsed_ms}ms)")
            } else {
                "OK".to_string()
            },
            response_time_ms: Some(elapsed_ms),
            http_status: Some(status.as_u16()),
            model_used: model,
            tested_at: now,
            retry_count: 0,
            error_category: None,
        }
    } else {
        let code = status.as_u16();
        let body_text = response.text().await.unwrap_or_default();
        let snippet: String = body_text.chars().take(300).collect();
        failed(format!("Kiro 返回 HTTP {code}: {snippet}"), Some(code))
    }
}
/// Copilot 供应商的 base_url 需要从 OAuth 管理器动态解析（按账号或默认端点）。
/// `is_full_url` 的供应商已是完整地址，无需解析。
async fn resolve_copilot_base_url_override(
    provider: &crate::provider::Provider,
    copilot_state: &State<'_, CopilotAuthState>,
) -> Result<Option<String>, AppError> {
    let is_copilot = is_copilot_provider(provider);
    let is_full_url = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.is_full_url)
        .unwrap_or(false);

    if !is_copilot || is_full_url {
        return Ok(None);
    }

    let auth_manager = copilot_state.0.read().await;
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("github_copilot"));

    let endpoint = match account_id.as_deref() {
        Some(id) => auth_manager.get_api_endpoint(id).await,
        None => auth_manager.get_default_api_endpoint().await,
    };

    Ok(Some(endpoint))
}

fn is_copilot_provider(provider: &crate::provider::Provider) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("github_copilot")
        || provider
            .settings_config
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|value| value.as_str())
            .map(|url| url.contains("githubcopilot.com"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_copilot_provider;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    #[test]
    fn copilot_provider_detection_accepts_provider_type_or_base_url() {
        let typed_provider = Provider {
            id: "p1".to_string(),
            name: "typed".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        assert!(is_copilot_provider(&typed_provider));

        let url_provider = Provider {
            id: "p2".to_string(),
            name: "url".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        assert!(is_copilot_provider(&url_provider));
    }

    #[test]
    fn copilot_full_url_metadata_is_available_for_override_guard() {
        let provider = Provider {
            id: "p3".to_string(),
            name: "relay".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                is_full_url: Some(true),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        assert!(is_copilot_provider(&provider));
        assert_eq!(
            provider.meta.as_ref().and_then(|meta| meta.is_full_url),
            Some(true)
        );
    }
}
