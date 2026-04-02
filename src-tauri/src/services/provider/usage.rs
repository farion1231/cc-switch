//! Usage script execution
//!
//! Handles executing and formatting usage query results.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, UsageData, UsageResult, UsageScript};
use crate::settings;
use crate::store::AppState;
use crate::usage_script;
use serde_json::Value;

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

fn value_as_non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn trim_trailing_slash(url: String) -> String {
    url.trim_end_matches('/').to_string()
}

fn extract_codex_base_url(config_toml: &str) -> Option<String> {
    let toml_value = toml::from_str::<toml::Value>(config_toml).ok()?;

    if let Some(active_provider) = toml_value.get("model_provider").and_then(|v| v.as_str()) {
        if let Some(base_url) = toml_value
            .get("model_providers")
            .and_then(|v| v.get(active_provider))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str())
        {
            return Some(base_url.to_string());
        }
    }

    if let Some(base_url) = toml_value.get("base_url").and_then(|v| v.as_str()) {
        return Some(base_url.to_string());
    }

    let provider_base_urls: Vec<&str> = toml_value
        .get("model_providers")
        .and_then(|v| v.as_table())
        .map(|providers| {
            providers
                .values()
                .filter_map(|provider| provider.get("base_url").and_then(|v| v.as_str()))
                .collect()
        })
        .unwrap_or_default();

    match provider_base_urls.as_slice() {
        [base_url] => Some((*base_url).to_string()),
        _ => None,
    }
}

/// Extract API key from provider configuration
fn extract_api_key_from_provider(provider: &Provider, app_type: &AppType) -> Option<String> {
    let config = &provider.settings_config;
    let env = config.get("env");

    match app_type {
        AppType::Claude => {
            value_as_non_empty_string(env.and_then(|v| v.get("ANTHROPIC_AUTH_TOKEN")))
                .or_else(|| value_as_non_empty_string(env.and_then(|v| v.get("ANTHROPIC_API_KEY"))))
                .or_else(|| {
                    value_as_non_empty_string(env.and_then(|v| v.get("OPENROUTER_API_KEY")))
                })
                .or_else(|| value_as_non_empty_string(env.and_then(|v| v.get("OPENAI_API_KEY"))))
                .or_else(|| value_as_non_empty_string(config.get("apiKey")))
                .or_else(|| value_as_non_empty_string(config.get("api_key")))
        }
        AppType::Codex => value_as_non_empty_string(config.pointer("/auth/OPENAI_API_KEY"))
            .or_else(|| value_as_non_empty_string(env.and_then(|v| v.get("OPENAI_API_KEY"))))
            .or_else(|| value_as_non_empty_string(env.and_then(|v| v.get("CODEX_API_KEY"))))
            .or_else(|| value_as_non_empty_string(config.get("apiKey")))
            .or_else(|| value_as_non_empty_string(config.get("api_key"))),
        AppType::Gemini => value_as_non_empty_string(env.and_then(|v| v.get("GEMINI_API_KEY")))
            .or_else(|| value_as_non_empty_string(config.get("GEMINI_API_KEY")))
            .or_else(|| value_as_non_empty_string(env.and_then(|v| v.get("GOOGLE_API_KEY"))))
            .or_else(|| value_as_non_empty_string(config.get("apiKey"))),
        AppType::OpenCode => value_as_non_empty_string(config.pointer("/options/apiKey"))
            .or_else(|| value_as_non_empty_string(config.get("apiKey")))
            .or_else(|| value_as_non_empty_string(config.get("api_key"))),
        AppType::OpenClaw => value_as_non_empty_string(config.get("apiKey"))
            .or_else(|| value_as_non_empty_string(config.get("api_key"))),
    }
}

fn extract_provider_base_url(provider: &Provider, app_type: &AppType) -> Option<String> {
    let config = &provider.settings_config;
    let env = config.get("env");

    let base_url = match app_type {
        AppType::Claude => value_as_non_empty_string(env.and_then(|v| v.get("ANTHROPIC_BASE_URL")))
            .or_else(|| value_as_non_empty_string(config.get("baseUrl")))
            .or_else(|| value_as_non_empty_string(config.get("baseURL"))),
        AppType::Codex => config
            .get("config")
            .and_then(|v| v.as_str())
            .and_then(extract_codex_base_url)
            .or_else(|| value_as_non_empty_string(config.get("baseUrl")))
            .or_else(|| value_as_non_empty_string(config.get("baseURL"))),
        AppType::Gemini => {
            value_as_non_empty_string(env.and_then(|v| v.get("GOOGLE_GEMINI_BASE_URL")))
                .or_else(|| value_as_non_empty_string(config.get("GOOGLE_GEMINI_BASE_URL")))
                .or_else(|| value_as_non_empty_string(config.get("GEMINI_BASE_URL")))
                .or_else(|| value_as_non_empty_string(config.get("baseUrl")))
        }
        AppType::OpenCode => value_as_non_empty_string(config.pointer("/options/baseURL"))
            .or_else(|| value_as_non_empty_string(config.get("baseUrl")))
            .or_else(|| value_as_non_empty_string(config.get("baseURL")))
            .or_else(|| value_as_non_empty_string(config.get("base_url"))),
        AppType::OpenClaw => value_as_non_empty_string(config.get("baseUrl"))
            .or_else(|| value_as_non_empty_string(config.get("baseURL")))
            .or_else(|| value_as_non_empty_string(config.get("base_url"))),
    };

    base_url.map(trim_trailing_slash)
}

fn resolve_minimax_usage_url(provider: &Provider, app_type: &AppType) -> String {
    let provider_base_url = extract_provider_base_url(provider, app_type);
    let is_global = [
        provider_base_url.as_deref(),
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref()),
        Some(provider.name.as_str()),
        provider.website_url.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_ascii_lowercase().contains("minimax.io"));

    if is_global {
        "https://www.minimax.io/v1/api/openplatform/coding_plan/remains".to_string()
    } else {
        "https://www.minimaxi.com/v1/api/openplatform/coding_plan/remains".to_string()
    }
}

/// Extract base URL from provider configuration
fn extract_base_url_from_provider(
    provider: &Provider,
    app_type: &AppType,
    template_type: Option<&str>,
) -> Option<String> {
    if template_type == Some("minimax") {
        return Some(resolve_minimax_usage_url(provider, app_type));
    }

    extract_provider_base_url(provider, app_type)
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
            .or_else(|| extract_api_key_from_provider(provider, &app_type))
            .unwrap_or_default();

        let base_url = usage_script
            .base_url
            .clone()
            .filter(|u| !u.is_empty())
            .or_else(|| {
                extract_base_url_from_provider(
                    provider,
                    &app_type,
                    usage_script.template_type.as_deref(),
                )
            })
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
    _state: &AppState,
    _app_type: AppType,
    _provider_id: &str,
    script_code: &str,
    timeout: u64,
    api_key: Option<&str>,
    base_url: Option<&str>,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<UsageResult, AppError> {
    // Use provided credential parameters directly for testing
    execute_and_format_usage_result(
        script_code,
        api_key.unwrap_or(""),
        base_url.unwrap_or(""),
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
        extract_api_key_from_provider, extract_base_url_from_provider, extract_codex_base_url,
    };
    use crate::app_config::AppType;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    fn test_provider(settings_config: serde_json::Value) -> Provider {
        Provider {
            id: "test-provider".to_string(),
            name: "MiniMax".to_string(),
            settings_config,
            website_url: Some("https://www.minimaxi.com".to_string()),
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("minimax".to_string()),
                ..Default::default()
            }),
            icon: Some("minimax".to_string()),
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn extract_api_key_from_provider_supports_codex_auth() {
        let provider = test_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-codex"
            },
            "config": r#"
model_provider = "newapi"

[model_providers.newapi]
base_url = "https://api.minimaxi.com/v1"
"#
        }));

        assert_eq!(
            extract_api_key_from_provider(&provider, &AppType::Codex),
            Some("sk-codex".to_string())
        );
    }

    #[test]
    fn extract_api_key_from_provider_supports_opencode_and_openclaw() {
        let opencode_provider = test_provider(json!({
            "options": {
                "apiKey": "sk-opencode",
                "baseURL": "https://api.minimaxi.com/v1"
            }
        }));
        let openclaw_provider = test_provider(json!({
            "apiKey": "sk-openclaw",
            "baseUrl": "https://api.minimax.io/v1"
        }));

        assert_eq!(
            extract_api_key_from_provider(&opencode_provider, &AppType::OpenCode),
            Some("sk-opencode".to_string())
        );
        assert_eq!(
            extract_api_key_from_provider(&openclaw_provider, &AppType::OpenClaw),
            Some("sk-openclaw".to_string())
        );
    }

    #[test]
    fn extract_api_key_from_provider_supports_gemini_env_key() {
        let provider = test_provider(json!({
            "env": {
                "GEMINI_API_KEY": "sk-gemini",
                "GOOGLE_GEMINI_BASE_URL": "https://api.minimax.io/v1"
            }
        }));

        assert_eq!(
            extract_api_key_from_provider(&provider, &AppType::Gemini),
            Some("sk-gemini".to_string())
        );
    }

    #[test]
    fn extract_codex_base_url_reads_model_provider_section() {
        let config_toml = r#"
model_provider = "newapi"

[model_providers.openai]
base_url = "https://api.example.com/v1"

[model_providers.newapi]
base_url = "https://api.minimax.io/v1"
"#;

        assert_eq!(
            extract_codex_base_url(config_toml),
            Some("https://api.minimax.io/v1".to_string())
        );
    }

    #[test]
    fn extract_codex_base_url_uses_top_level_before_unselected_provider_sections() {
        let config_toml = r#"
model = "gpt-5"
base_url = "https://fallback.example.com/v1"

[model_providers.openai]
base_url = "https://api.example.com/v1"

[model_providers.minimax]
base_url = "https://api.minimax.io/v1"
"#;

        assert_eq!(
            extract_codex_base_url(config_toml),
            Some("https://fallback.example.com/v1".to_string())
        );
    }

    #[test]
    fn extract_base_url_from_provider_resolves_minimax_usage_url() {
        let provider = test_provider(json!({
            "options": {
                "apiKey": "sk-opencode",
                "baseURL": "https://api.minimax.io/v1"
            }
        }));

        assert_eq!(
            extract_base_url_from_provider(&provider, &AppType::OpenCode, Some("minimax")),
            Some("https://www.minimax.io/v1/api/openplatform/coding_plan/remains".to_string())
        );
    }
}
