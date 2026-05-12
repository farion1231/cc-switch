//! Usage script execution
//!
//! Handles executing and formatting usage query results.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{UsageData, UsageResult, UsageScript};
use crate::settings;
use crate::store::AppState;
use crate::usage_script;

/// Execute usage script and format result (private helper method)
pub(crate) async fn execute_and_format_usage_result(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    timeout: u64,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<UsageResult, AppError> {
    match usage_script::execute_usage_script(
        script_code,
        api_key,
        base_url,
        timeout,
        access_token,
        user_id,
        template_type,
    )
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
        if let Some(api_key) = env
            .get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| env.get("ANTHROPIC_API_KEY"))
            .or_else(|| env.get("OPENROUTER_API_KEY"))
            .or_else(|| env.get("GOOGLE_API_KEY"))
            .or_else(|| env.get("GEMINI_API_KEY"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            return Some(api_key.to_string());
        }
    }

    provider
        .settings_config
        .get("auth")
        .and_then(|auth| auth.get("OPENAI_API_KEY"))
        .or_else(|| {
            provider
                .settings_config
                .get("options")
                .and_then(|options| options.get("apiKey"))
        })
        .or_else(|| provider.settings_config.get("apiKey"))
        .or_else(|| provider.settings_config.get("api_key"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
}

/// Extract base URL from provider configuration
fn extract_base_url_from_provider(provider: &crate::provider::Provider) -> Option<String> {
    if let Some(env) = provider.settings_config.get("env") {
        // Try multiple possible base URL fields
        if let Some(base_url) = env
            .get("ANTHROPIC_BASE_URL")
            .or_else(|| env.get("GOOGLE_GEMINI_BASE_URL"))
            .or_else(|| env.get("OPENAI_BASE_URL"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            return Some(base_url.trim_end_matches('/').to_string());
        }
    }

    provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_codex_base_url_from_toml)
        .or_else(|| {
            provider
                .settings_config
                .get("options")
                .and_then(|options| options.get("baseURL").or_else(|| options.get("baseUrl")))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim_end_matches('/').to_string())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("baseUrl")
                .or_else(|| provider.settings_config.get("base_url"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim_end_matches('/').to_string())
        })
}

fn extract_codex_base_url_from_toml(config_text: &str) -> Option<String> {
    let parsed = config_text.parse::<toml::Value>().ok()?;

    if let Some(model_provider) = parsed.get("model_provider").and_then(|v| v.as_str()) {
        if let Some(base_url) = parsed
            .get("model_providers")
            .and_then(|providers| providers.get(model_provider))
            .and_then(|provider| provider.get("base_url"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            return Some(base_url.trim_end_matches('/').to_string());
        }
    }

    if let Some(base_url) = parsed
        .get("base_url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        return Some(base_url.trim_end_matches('/').to_string());
    }

    let base_urls: Vec<String> = parsed
        .get("model_providers")
        .and_then(|providers| providers.as_table())
        .map(|providers| {
            providers
                .values()
                .filter_map(|provider| {
                    provider
                        .get("base_url")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| s.trim_end_matches('/').to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    if base_urls.len() == 1 {
        base_urls.into_iter().next()
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
    let (script_code, timeout, api_key, base_url, access_token, user_id, template_type) = {
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
            usage_script.access_token.clone(),
            usage_script.user_id.clone(),
            usage_script.template_type.clone(),
        )
    };

    execute_and_format_usage_result(
        &script_code,
        &api_key,
        &base_url,
        timeout,
        access_token.as_deref(),
        user_id.as_deref(),
        template_type.as_deref(),
    )
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
    let provider_credentials = || -> Option<(Option<String>, Option<String>)> {
        let providers = state.db.get_all_providers(app_type.as_str()).ok()?;
        let provider = providers.get(provider_id)?;
        Some((
            extract_api_key_from_provider(provider),
            extract_base_url_from_provider(provider),
        ))
    };

    let (provider_api_key, provider_base_url) =
        if api_key.is_none_or(|s| s.is_empty()) || base_url.is_none_or(|s| s.is_empty()) {
            provider_credentials().unwrap_or((None, None))
        } else {
            (None, None)
        };

    let resolved_api_key = api_key
        .filter(|s| !s.is_empty())
        .or(provider_api_key.as_deref())
        .unwrap_or("");
    let resolved_base_url = base_url
        .filter(|s| !s.is_empty())
        .or(provider_base_url.as_deref())
        .unwrap_or("");

    execute_and_format_usage_result(
        script_code,
        resolved_api_key,
        resolved_base_url,
        timeout,
        access_token,
        user_id,
        template_type,
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;

    fn provider_with_settings(settings_config: serde_json::Value) -> Provider {
        Provider::with_id(
            "test-provider".to_string(),
            "Test Provider".to_string(),
            settings_config,
            None,
        )
    }

    #[test]
    fn extracts_codex_credentials_from_auth_and_active_model_provider() {
        let provider = provider_with_settings(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test"
            },
            "config": r#"
model_provider = "cubence"

[model_providers.cubence]
base_url = "https://api.cubence.me/v1"

[model_providers.other]
base_url = "https://api.other.example/v1"
"#
        }));

        assert_eq!(
            extract_api_key_from_provider(&provider),
            Some("sk-test".to_string())
        );
        assert_eq!(
            extract_base_url_from_provider(&provider),
            Some("https://api.cubence.me/v1".to_string())
        );
    }

    #[test]
    fn extracts_codex_base_url_from_single_provider_without_active_provider() {
        let base_url = extract_codex_base_url_from_toml(
            r#"
[model_providers.cubence]
base_url = "https://api.cubence.me/v1/"
"#,
        );

        assert_eq!(base_url, Some("https://api.cubence.me/v1".to_string()));
    }
}
