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

/// Resolve `(api_key, base_url)` for the JS-script path: explicit non-empty
/// script values win, otherwise fall back to the provider's stored config via
/// `Provider::resolve_usage_credentials` — the same per-app resolver the
/// native balance/coding-plan path and the frontend `getProviderCredentials`
/// use, so `{{apiKey}}`/`{{baseUrl}}` match what the UI shows for them.
fn resolve_script_credentials(
    app_type: &AppType,
    provider: &crate::provider::Provider,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> (String, String) {
    let (provider_base_url, provider_api_key) = provider.resolve_usage_credentials(app_type);

    let api_key = api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or(provider_api_key);

    let base_url = base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        // Trim like the provider path so `{{baseUrl}}/path` never doubles the slash.
        .map(|value| value.trim_end_matches('/').to_owned())
        .unwrap_or(provider_base_url);

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
        let (api_key, base_url) = resolve_script_credentials(
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

    // Resolve like the real query so testing matches what a saved script does:
    // explicit values win, empty ones fall back to the provider config.
    let (api_key, base_url) = resolve_script_credentials(&app_type, provider, api_key, base_url);

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

/// Resolve `(base_url, api_key)` for native (non-script) usage paths.
fn resolve_native_credentials(
    app_type: &AppType,
    provider: Option<&crate::provider::Provider>,
) -> (String, String) {
    provider
        .map(|p| p.resolve_usage_credentials(app_type))
        .unwrap_or_default()
}

/// Resolve coding-plan credentials: ZenMux uses the script's own base_url/api_key;
/// everything else falls back to the provider's stored credentials.
fn resolve_coding_plan_credentials(
    app_type: &AppType,
    provider: Option<&crate::provider::Provider>,
    usage_script: Option<&UsageScript>,
) -> (String, String) {
    let is_zenmux = usage_script
        .and_then(|s| s.coding_plan_provider.as_deref())
        .map(|p| p.eq_ignore_ascii_case("zenmux"))
        .unwrap_or(false);

    if !is_zenmux {
        return resolve_native_credentials(app_type, provider);
    }

    let script_base_url = usage_script
        .and_then(|s| s.base_url.as_deref())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    let script_api_key = usage_script
        .and_then(|s| s.api_key.as_deref())
        .unwrap_or("")
        .to_string();

    if !script_base_url.is_empty() && !script_api_key.is_empty() {
        return (script_base_url, script_api_key);
    }

    let native = resolve_native_credentials(app_type, provider);
    if !native.0.is_empty() && !native.1.is_empty() {
        native
    } else {
        (script_base_url, script_api_key)
    }
}

fn coding_plan_quota_to_usage_result(
    quota: crate::services::subscription::SubscriptionQuota,
) -> UsageResult {
    if !quota.success {
        return UsageResult {
            success: false,
            data: None,
            error: quota.error,
        };
    }

    let has_usd = quota
        .tiers
        .first()
        .map(|t| t.used_value_usd.is_some())
        .unwrap_or(false);
    let plan_label = quota
        .credential_message
        .as_deref()
        .and_then(|msg| msg.split(' ').next())
        .map(|tier| format!("ZenMux·{}", tier.to_uppercase()));
    let mut first_tier = true;

    let data: Vec<UsageData> = quota
        .tiers
        .iter()
        .map(|tier| {
            let total = 100.0;
            let used = tier.utilization;
            let remaining = total - used;
            let extra = if has_usd {
                let mut extra_json = serde_json::json!({ "resetsAt": tier.resets_at });
                if let Some(v) = tier.used_value_usd {
                    extra_json["usedValueUsd"] = serde_json::json!(v);
                }
                if let Some(v) = tier.max_value_usd {
                    extra_json["maxValueUsd"] = serde_json::json!(v);
                }
                if first_tier {
                    if let Some(ref label) = plan_label {
                        extra_json["planLabel"] = serde_json::json!(label);
                    }
                    first_tier = false;
                }
                Some(extra_json.to_string())
            } else {
                tier.resets_at.clone()
            };
            UsageData {
                plan_name: Some(tier.name.clone()),
                remaining: Some(remaining),
                total: Some(total),
                used: Some(used),
                unit: Some("%".to_string()),
                is_valid: Some(true),
                invalid_message: None,
                extra,
            }
        })
        .collect();

    UsageResult {
        success: true,
        data: if data.is_empty() { None } else { Some(data) },
        error: None,
    }
}

fn subscription_quota_to_usage_result(
    quota: crate::services::subscription::SubscriptionQuota,
) -> UsageResult {
    if !quota.success {
        return UsageResult {
            success: false,
            data: None,
            error: quota.error.or(quota.credential_message),
        };
    }
    let data: Vec<UsageData> = quota
        .tiers
        .iter()
        .map(|tier| UsageData {
            plan_name: Some(tier.name.clone()),
            remaining: Some(100.0 - tier.utilization),
            total: Some(100.0),
            used: Some(tier.utilization),
            unit: Some("%".to_string()),
            is_valid: Some(true),
            invalid_message: None,
            extra: tier.resets_at.clone(),
        })
        .collect();
    UsageResult {
        success: true,
        data: if data.is_empty() { None } else { Some(data) },
        error: None,
    }
}

/// Query provider usage, dispatching on the saved template type.
///
/// Handles the native (non-script) templates that the desktop main page
/// supports — `token_plan` (coding-plan quota), `balance`, and
/// `official_subscription` — plus the generic JS-script path. The
/// `github_copilot` template is intentionally NOT handled here because it
/// requires the desktop-only `CopilotAuthState`; callers that have it (the
/// Tauri command) handle Copilot before delegating here, and web callers get a
/// clear error.
pub async fn query_usage_with_templates(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
) -> Result<UsageResult, AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let provider = providers.get(provider_id);
    let usage_script = provider
        .and_then(|p| p.meta.as_ref())
        .and_then(|m| m.usage_script.as_ref());
    let template_type = usage_script
        .and_then(|s| s.template_type.as_deref())
        .unwrap_or("");

    match template_type {
        "github_copilot" => Err(AppError::localized(
            "provider.usage.copilot_web_unsupported",
            "GitHub Copilot 用量查询在 Web 模式下不可用",
            "GitHub Copilot usage query is not available in web mode",
        )),
        "token_plan" => {
            let (base_url, api_key) =
                resolve_coding_plan_credentials(&app_type, provider, usage_script);
            let quota = crate::services::coding_plan::get_coding_plan_quota(
                &base_url,
                &api_key,
                None,
                None,
            )
            .await
            .map_err(AppError::Config)?;
            Ok(coding_plan_quota_to_usage_result(quota))
        }
        "balance" => {
            let (base_url, api_key) = resolve_native_credentials(&app_type, provider);
            let result = crate::services::balance::get_balance(&base_url, &api_key)
                .await
                .map_err(AppError::Config)?;
            Ok(result)
        }
        "official_subscription" => {
            if !usage_script.map(|s| s.enabled).unwrap_or(false) {
                return Ok(UsageResult {
                    success: false,
                    data: None,
                    error: Some("Usage query is disabled".to_string()),
                });
            }
            let quota = crate::services::subscription::get_subscription_quota(app_type.as_str())
                .await
                .map_err(AppError::Config)?;
            Ok(subscription_quota_to_usage_result(quota))
        }
        _ => query_usage(state, app_type, provider_id).await,
    }
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
    use super::resolve_script_credentials;
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
    fn script_values_override_provider_credentials() {
        let provider = provider_with_settings(json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "provider-key",
                "ANTHROPIC_BASE_URL": "https://provider.example.com/"
            }
        }));

        let (api_key, base_url) = resolve_script_credentials(
            &AppType::Claude,
            &provider,
            Some(" script-key "),
            Some(" https://script.example.com/ "),
        );
        assert_eq!(api_key, "script-key");
        assert_eq!(base_url, "https://script.example.com");
    }

    #[test]
    fn empty_script_values_fall_back_to_provider_credentials() {
        let provider = provider_with_settings(json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "provider-key",
                "ANTHROPIC_BASE_URL": "https://provider.example.com/"
            }
        }));

        let (api_key, base_url) =
            resolve_script_credentials(&AppType::Claude, &provider, Some(""), None);
        assert_eq!(api_key, "provider-key");
        assert_eq!(base_url, "https://provider.example.com");
    }

    #[test]
    fn codex_fallback_reads_auth_and_config_toml() {
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

        let (api_key, base_url) =
            resolve_script_credentials(&AppType::Codex, &provider, None, None);
        assert_eq!(api_key, "openai-key");
        assert_eq!(base_url, "https://azure.example.com/v1");
    }
}
