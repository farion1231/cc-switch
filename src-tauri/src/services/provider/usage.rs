//! Usage script execution
//!
//! Handles executing and formatting usage query results.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{UsageData, UsageResult, UsageScript};
use crate::settings;
use crate::store::AppState;
use crate::usage_script;
use toml_edit::DocumentMut;

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
fn extract_api_key_from_provider(
    app_type: &AppType,
    provider: &crate::provider::Provider,
) -> Option<String> {
    match app_type {
        AppType::Claude => provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
            .and_then(|env| {
                env.get("ANTHROPIC_AUTH_TOKEN")
                    .or_else(|| env.get("ANTHROPIC_API_KEY"))
            })
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        AppType::Codex => provider
            .settings_config
            .get("auth")
            .and_then(|v| v.as_object())
            .and_then(|auth| auth.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        AppType::Gemini => provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
            .and_then(|env| env.get("GEMINI_API_KEY"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        AppType::OpenCode | AppType::OpenClaw => None,
    }
}

/// Extract base URL from provider configuration
fn extract_base_url_from_provider(
    app_type: &AppType,
    provider: &crate::provider::Provider,
) -> Option<String> {
    match app_type {
        AppType::Claude => provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string()),
        AppType::Codex => provider
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .and_then(extract_codex_base_url_from_toml)
            .map(|s| s.trim_end_matches('/').to_string()),
        AppType::Gemini => provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
            .and_then(|env| env.get("GOOGLE_GEMINI_BASE_URL"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string()),
        AppType::OpenCode | AppType::OpenClaw => None,
    }
}

fn extract_codex_base_url_from_toml(config_toml: &str) -> Option<String> {
    let doc = config_toml.parse::<DocumentMut>().ok()?;

    if let Some(provider_key) = doc.get("model_provider").and_then(|item| item.as_str()) {
        if let Some(url) = doc
            .get("model_providers")
            .and_then(|item| item.as_table_like())
            .and_then(|providers| providers.get(provider_key))
            .and_then(|provider| provider.as_table_like())
            .and_then(|provider| provider.get("base_url"))
            .and_then(|item| item.as_str())
        {
            return Some(url.to_string());
        }
    }

    if let Some(url) = doc.get("base_url").and_then(|item| item.as_str()) {
        return Some(url.to_string());
    }

    let mut provider_urls = doc
        .get("model_providers")
        .and_then(|item| item.as_table_like())
        .into_iter()
        .flat_map(|providers| providers.iter())
        .filter_map(|(_, provider)| {
            provider
                .as_table_like()
                .and_then(|table| table.get("base_url"))
                .and_then(|item| item.as_str())
        });

    let first = provider_urls.next()?;
    if provider_urls.next().is_none() {
        Some(first.to_string())
    } else {
        None
    }
}

fn resolve_usage_credentials(
    app_type: &AppType,
    provider: &crate::provider::Provider,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> (String, String) {
    let api_key = api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| extract_api_key_from_provider(app_type, provider))
        .unwrap_or_default();

    let base_url = base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| extract_base_url_from_provider(app_type, provider))
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

        // Get credentials: prioritize UsageScript values, fallback to provider config
        let (api_key, base_url) = resolve_usage_credentials(
            &app_type,
            provider,
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
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let provider = providers.get(provider_id).ok_or_else(|| {
        AppError::localized(
            "provider.not_found",
            format!("供应商不存在: {provider_id}"),
            format!("Provider not found: {provider_id}"),
        )
    })?;

    let (api_key, base_url) = resolve_usage_credentials(&app_type, provider, api_key, base_url);

    execute_and_format_usage_result(
        script_code,
        &api_key,
        &base_url,
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
    use super::{
        extract_api_key_from_provider, extract_base_url_from_provider,
        extract_codex_base_url_from_toml, resolve_usage_credentials,
    };
    use crate::app_config::AppType;
    use crate::provider::Provider;
    use serde_json::json;

    fn provider_with_settings(settings_config: serde_json::Value) -> Provider {
        Provider::with_id(
            "provider-1".to_string(),
            "Provider".to_string(),
            settings_config,
            None,
        )
    }

    #[test]
    fn extracts_claude_credentials_from_env() {
        let provider = provider_with_settings(json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "auth-token",
                "ANTHROPIC_API_KEY": "api-key",
                "ANTHROPIC_BASE_URL": "https://claude.example.com/"
            }
        }));

        assert_eq!(
            extract_api_key_from_provider(&AppType::Claude, &provider).as_deref(),
            Some("auth-token")
        );
        assert_eq!(
            extract_base_url_from_provider(&AppType::Claude, &provider).as_deref(),
            Some("https://claude.example.com")
        );
    }

    #[test]
    fn extracts_codex_credentials_from_auth_and_active_provider_toml() {
        let provider = provider_with_settings(json!({
            "auth": {
                "OPENAI_API_KEY": "openai-key"
            },
            "config": r#"model_provider = "azure"

[model_providers.azure]
base_url = "https://azure.example.com/v1/"

[model_providers.other]
base_url = "https://other.example.com/v1"
"#
        }));

        assert_eq!(
            extract_api_key_from_provider(&AppType::Codex, &provider).as_deref(),
            Some("openai-key")
        );
        assert_eq!(
            extract_base_url_from_provider(&AppType::Codex, &provider).as_deref(),
            Some("https://azure.example.com/v1")
        );
    }

    #[test]
    fn codex_base_url_falls_back_to_top_level_or_single_provider() {
        let top_level = r#"base_url = "https://top-level.example.com/v1/""#;
        assert_eq!(
            extract_codex_base_url_from_toml(top_level).as_deref(),
            Some("https://top-level.example.com/v1/")
        );

        let single_provider = r#"[model_providers.any]
base_url = "https://single.example.com/v1"
"#;
        assert_eq!(
            extract_codex_base_url_from_toml(single_provider).as_deref(),
            Some("https://single.example.com/v1")
        );
    }

    #[test]
    fn extracts_gemini_credentials_from_env() {
        let provider = provider_with_settings(json!({
            "env": {
                "GEMINI_API_KEY": "gemini-key",
                "GOOGLE_GEMINI_BASE_URL": "https://gemini.example.com/"
            }
        }));

        assert_eq!(
            extract_api_key_from_provider(&AppType::Gemini, &provider).as_deref(),
            Some("gemini-key")
        );
        assert_eq!(
            extract_base_url_from_provider(&AppType::Gemini, &provider).as_deref(),
            Some("https://gemini.example.com")
        );
    }

    #[test]
    fn resolves_usage_credentials_with_script_override_then_provider_fallback() {
        let provider = provider_with_settings(json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "provider-key",
                "ANTHROPIC_BASE_URL": "https://provider.example.com/"
            }
        }));

        let (api_key, base_url) = resolve_usage_credentials(
            &AppType::Claude,
            &provider,
            Some(" script-key "),
            Some(" https://script.example.com "),
        );
        assert_eq!(api_key, "script-key");
        assert_eq!(base_url, "https://script.example.com");

        let (api_key, base_url) =
            resolve_usage_credentials(&AppType::Claude, &provider, Some(""), None);
        assert_eq!(api_key, "provider-key");
        assert_eq!(base_url, "https://provider.example.com");
    }
}
