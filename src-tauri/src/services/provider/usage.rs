//! Usage script execution
//!
//! Handles executing and formatting usage query results.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{UsageData, UsageResult, UsageScript};
use crate::services::provider::{
    extract_provider_api_key, extract_provider_base_url, non_empty_trimmed,
};
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

fn resolve_usage_credentials(
    provider: &crate::provider::Provider,
    app_type: &AppType,
    api_key_override: Option<&str>,
    base_url_override: Option<&str>,
) -> (String, String) {
    let api_key = non_empty_trimmed(api_key_override)
        .or_else(|| extract_provider_api_key(provider, app_type))
        .unwrap_or_default();

    let base_url = non_empty_trimmed(base_url_override)
        .or_else(|| extract_provider_base_url(provider, app_type))
        .unwrap_or_default();

    (api_key, base_url)
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

        let (api_key, base_url) = resolve_usage_credentials(
            provider,
            &app_type,
            usage_script.api_key.as_deref(),
            usage_script.base_url.as_deref(),
        );

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
    let provider = state
        .db
        .get_provider_by_id(provider_id, app_type.as_str())?
        .ok_or_else(|| {
            AppError::localized(
                "provider.not_found",
                format!("供应商不存在: {provider_id}"),
                format!("Provider not found: {provider_id}"),
            )
        })?;
    let (resolved_api_key, resolved_base_url) =
        resolve_usage_credentials(&provider, &app_type, api_key, base_url);

    execute_and_format_usage_result(
        script_code,
        &resolved_api_key,
        &resolved_base_url,
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
        Provider {
            id: "provider-1".to_string(),
            name: "provider".to_string(),
            settings_config,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn resolve_usage_credentials_falls_back_when_overrides_are_blank() {
        let provider = provider_with_settings(json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-provider",
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic/"
            }
        }));

        let (api_key, base_url) =
            resolve_usage_credentials(&provider, &AppType::Claude, Some(""), Some("   "));

        assert_eq!(api_key, "sk-provider");
        assert_eq!(base_url, "https://api.deepseek.com/anthropic/");
    }

    #[test]
    fn resolve_usage_credentials_reads_top_level_and_options_values() {
        let provider = provider_with_settings(json!({
            "options": {
                "apiKey": "sk-options",
                "baseURL": "https://api.deepseek.com/v1/"
            }
        }));

        let (api_key, base_url) =
            resolve_usage_credentials(&provider, &AppType::OpenCode, None, None);

        assert_eq!(api_key, "sk-options");
        assert_eq!(base_url, "https://api.deepseek.com/v1/");
    }

    #[test]
    fn resolve_usage_credentials_prefers_non_empty_overrides() {
        let provider = provider_with_settings(json!({
            "apiKey": "sk-provider",
            "baseUrl": "https://provider.example.com/",
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-env",
                "ANTHROPIC_BASE_URL": "https://env.example.com/"
            }
        }));

        let (api_key, base_url) = resolve_usage_credentials(
            &provider,
            &AppType::OpenClaw,
            Some("sk-override"),
            Some("https://override.example.com/"),
        );

        assert_eq!(api_key, "sk-override");
        assert_eq!(base_url, "https://override.example.com/");
    }

    #[test]
    fn resolve_usage_credentials_ignores_blank_provider_fields_and_uses_env() {
        let provider = provider_with_settings(json!({
            "apiKey": "   ",
            "api_key": "",
            "base_url": "",
            "options": {
                "api_key": "",
                "base_url": "   "
            },
            "env": {
                "OPENAI_API_KEY": "sk-env",
                "OPENAI_BASE_URL": "https://env.example.com/v1/"
            }
        }));

        let (api_key, base_url) = resolve_usage_credentials(&provider, &AppType::Codex, None, None);

        assert_eq!(api_key, "sk-env");
        assert_eq!(base_url, "https://env.example.com/v1/");
    }

    #[test]
    fn resolve_usage_credentials_reads_codex_auth_and_config_toml() {
        let provider = provider_with_settings(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-codex"
            },
            "config": "model = \"gpt-5\"\nbase_url = \"https://api.openai.com/v1\"\n"
        }));

        let (api_key, base_url) = resolve_usage_credentials(&provider, &AppType::Codex, None, None);

        assert_eq!(api_key, "sk-codex");
        assert_eq!(base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn resolve_usage_credentials_prefers_active_codex_model_provider_base_url() {
        let provider = provider_with_settings(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-codex"
            },
            "config": r#"model_provider = "azure"
base_url = "https://top-level.example/v1"

[model_providers.azure]
base_url = "https://azure.example/v1"

[model_providers.openai]
base_url = "https://openai.example/v1"

[mcp_servers.local]
base_url = "http://localhost:8080"
"#
        }));

        let (api_key, base_url) = resolve_usage_credentials(&provider, &AppType::Codex, None, None);

        assert_eq!(api_key, "sk-codex");
        assert_eq!(base_url, "https://azure.example/v1");
    }

    #[test]
    fn resolve_usage_credentials_supports_legacy_alias_fields() {
        let provider = provider_with_settings(json!({
            "api_key": "sk-legacy",
            "base_url": "https://legacy.example.com/v1",
        }));

        let (api_key, base_url) =
            resolve_usage_credentials(&provider, &AppType::OpenClaw, None, None);

        assert_eq!(api_key, "sk-legacy");
        assert_eq!(base_url, "https://legacy.example.com/v1");
    }
}
