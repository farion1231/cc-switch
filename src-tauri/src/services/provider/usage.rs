//! Usage script execution
//!
//! Handles executing and formatting usage query results.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{UsageData, UsageResult, UsageScript};
use crate::settings;
use crate::store::AppState;
use crate::usage_script;

struct UsageExecutionInput<'a> {
    script_code: &'a str,
    api_key: &'a str,
    base_url: &'a str,
    timeout: u64,
    connection_override: Option<&'a str>,
    access_token: Option<&'a str>,
    user_id: Option<&'a str>,
    template_type: Option<&'a str>,
}

/// Execute usage script and format result (private helper method)
async fn execute_and_format_usage_result(
    input: UsageExecutionInput<'_>,
) -> Result<UsageResult, AppError> {
    match usage_script::execute_usage_script(usage_script::ExecuteUsageScriptInput {
        script_code: input.script_code,
        api_key: input.api_key,
        base_url: input.base_url,
        timeout_secs: input.timeout,
        connection_override: input.connection_override,
        access_token: input.access_token,
        user_id: input.user_id,
        template_type: input.template_type,
    })
    .await
    {
        Ok(data) => {
            let usage_list: Vec<UsageData> = if data.is_array() {
                serde_json::from_value(data).map_err(|e| {
                    AppError::localized(
                        "usage_script.data_format_error",
                        format!("数据格式错误: {e}"),
                        format!("Data format error: {e}"),
                    )
                })?
            } else {
                let single: UsageData = serde_json::from_value(data).map_err(|e| {
                    AppError::localized(
                        "usage_script.data_format_error",
                        format!("数据格式错误: {e}"),
                        format!("Data format error: {e}"),
                    )
                })?;
                vec![single]
            };

            Ok(UsageResult {
                success: true,
                data: Some(usage_list),
                error: None,
            })
        }
        Err(err) => {
            let lang = settings::get_settings()
                .language
                .unwrap_or_else(|| "zh".to_string());

            let msg = match err {
                AppError::Localized { zh, en, .. } => {
                    if lang == "en" {
                        en
                    } else {
                        zh
                    }
                }
                other => other.to_string(),
            };

            Ok(UsageResult {
                success: false,
                data: None,
                error: Some(msg),
            })
        }
    }
}

/// Extract API key from provider configuration
fn extract_api_key_from_provider(provider: &crate::provider::Provider) -> Option<String> {
    if let Some(env) = provider.settings_config.get("env") {
        // Try multiple possible API key fields
        env.get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| env.get("ANTHROPIC_API_KEY"))
            .or_else(|| env.get("OPENROUTER_API_KEY"))
            .or_else(|| env.get("GOOGLE_API_KEY"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    }
}

/// Extract base URL from provider configuration
fn extract_base_url_from_provider(provider: &crate::provider::Provider) -> Option<String> {
    if let Some(env) = provider.settings_config.get("env") {
        // Try multiple possible base URL fields
        env.get("ANTHROPIC_BASE_URL")
            .or_else(|| env.get("GOOGLE_GEMINI_BASE_URL"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string())
    } else {
        None
    }
}

/// Query provider usage (using saved script configuration)
pub async fn query_usage(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
) -> Result<UsageResult, AppError> {
    let (
        script_code,
        timeout,
        api_key,
        base_url,
        connection_override,
        access_token,
        user_id,
        template_type,
    ) = {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers.get(provider_id).ok_or_else(|| {
            AppError::localized(
                "provider.not_found",
                format!("供应商不存在: {provider_id}"),
                format!("Provider not found: {provider_id}"),
            )
        })?;

        let usage_script = provider
            .meta
            .as_ref()
            .and_then(|m| m.usage_script.as_ref())
            .ok_or_else(|| {
                AppError::localized(
                    "provider.usage.script.missing",
                    "未配置用量查询脚本",
                    "Usage script is not configured",
                )
            })?;
        if !usage_script.enabled {
            return Err(AppError::localized(
                "provider.usage.disabled",
                "用量查询未启用",
                "Usage query is disabled",
            ));
        }

        // Get credentials: prioritize UsageScript values, fallback to provider config
        let api_key = usage_script
            .api_key
            .clone()
            .filter(|k| !k.is_empty())
            .or_else(|| extract_api_key_from_provider(provider))
            .unwrap_or_default();

        let base_url = usage_script
            .base_url
            .clone()
            .filter(|u| !u.is_empty())
            .or_else(|| extract_base_url_from_provider(provider))
            .unwrap_or_default();

        (
            usage_script.code.clone(),
            usage_script.timeout.unwrap_or(10),
            api_key,
            base_url,
            provider
                .meta
                .as_ref()
                .and_then(|m| m.connection_override.clone()),
            usage_script.access_token.clone(),
            usage_script.user_id.clone(),
            usage_script.template_type.clone(),
        )
    };

    execute_and_format_usage_result(UsageExecutionInput {
        script_code: &script_code,
        api_key: &api_key,
        base_url: &base_url,
        timeout,
        connection_override: connection_override.as_deref(),
        access_token: access_token.as_deref(),
        user_id: user_id.as_deref(),
        template_type: template_type.as_deref(),
    })
    .await
}

/// Test usage script (using temporary script content, not saved)
#[allow(clippy::too_many_arguments)]
pub async fn test_usage_script(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    script_code: &str,
    timeout: u64,
    api_key: Option<&str>,
    base_url: Option<&str>,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<UsageResult, AppError> {
    let connection_override = state
        .db
        .get_all_providers(app_type.as_str())?
        .get(provider_id)
        .and_then(|p| p.meta.as_ref())
        .and_then(|m| m.connection_override.as_deref())
        .map(str::to_string);

    // Use provided credential parameters directly for testing
    execute_and_format_usage_result(UsageExecutionInput {
        script_code,
        api_key: api_key.unwrap_or(""),
        base_url: base_url.unwrap_or(""),
        timeout,
        connection_override: connection_override.as_deref(),
        access_token,
        user_id,
        template_type,
    })
    .await
}

/// Validate UsageScript configuration (boundary checks)
pub(crate) fn validate_usage_script(script: &UsageScript) -> Result<(), AppError> {
    // Validate auto query interval (0-1440 minutes, max 24 hours)
    if let Some(interval) = script.auto_query_interval {
        if interval > 1440 {
            return Err(AppError::localized(
                "usage_script.interval_too_large",
                format!("自动查询间隔不能超过 1440 分钟（24小时），当前值: {interval}"),
                format!(
                    "Auto query interval cannot exceed 1440 minutes (24 hours), current: {interval}"
                ),
            ));
        }
    }

    Ok(())
}
