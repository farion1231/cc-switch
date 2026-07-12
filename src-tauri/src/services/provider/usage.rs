//! Usage script execution
//!
//! Handles executing and formatting usage query results.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{UsageData, UsageResult, UsageScript};
use crate::settings;
use crate::store::AppState;
use crate::usage_script;

// ───────────────────────────────────────────────────────────────────────────
// Special-template branches shared between the provider-level path
// (`commands::provider::query_provider_usage_inner`) and the per-key path
// (`query_usage_for_key` below). Each is a thin adapter that translates a
// provider's `template_type` + `(base_url, api_key)` into a `UsageResult`.
//
// `api_key` here is the **resolved** credential — for the per-key caller it
// comes from `provider_api_keys[row].api_key`; for the provider-level caller
// it comes from `usage_script.api_key` → `provider.settings_config`. So the
// per-key TOKEN_PLAN call uses *that key's* MiniMax/Kimi/Zhipu/… token
// against the live API, which is the whole point of letting a key pool
// reflect N independent MiniMax accounts. (When a pool is genuinely the same
// account with N keys for round-robin/limits, the backend will simply return
// the same 5h/7d numbers — that's still correct, just no per-key variety.)
// ───────────────────────────────────────────────────────────────────────────

const TEMPLATE_TYPE_TOKEN_PLAN: &str = "token_plan";
const TEMPLATE_TYPE_BALANCE: &str = "balance";
const TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION: &str = "official_subscription";

/// Query the special account-level templates (TOKEN_PLAN / BALANCE /
/// OFFICIAL_SUBSCRIPTION) and return a `UsageResult` shaped like the JS-script
/// path so callers can pipe it straight into the existing per-key cache and
/// the same UI bar.
///
/// `usage_script` is read here only for the optional `access_key_id`,
/// `secret_access_key`, `group_id` fields used by Volcengine / MiniMax.
/// Pass `None` from paths that don't have a usage_script reference (none
/// today — both call-sites have one in scope).
pub(crate) async fn query_special_template(
    app_type: &AppType,
    template_type: &str,
    base_url: &str,
    api_key: &str,
    usage_script: Option<&UsageScript>,
) -> Result<UsageResult, AppError> {
    if template_type == TEMPLATE_TYPE_TOKEN_PLAN {
        // 火山方舟用账号 AK/SK 签名（与数据面 api_key 分离）；其他供应商 None。
        let access_key_id = usage_script.and_then(|s| s.access_key_id.clone());
        let secret_access_key = usage_script.and_then(|s| s.secret_access_key.clone());
        // 智谱团队版：base_url 与个人版相同，靠显式 provider 路由。
        let coding_plan_provider = usage_script.and_then(|s| s.coding_plan_provider.clone());
        let team_organization_id = usage_script.and_then(|s| s.team_organization_id.clone());
        let team_project_id = usage_script.and_then(|s| s.team_project_id.clone());
        // MiniMax Coding Plan 缺 GroupId 时接口返回占位零值（误显示 0%）。
        let group_id = usage_script.and_then(|s| s.group_id.clone());

        let quota = crate::services::coding_plan::get_coding_plan_quota(
            base_url,
            api_key,
            access_key_id.as_deref(),
            secret_access_key.as_deref(),
            coding_plan_provider.as_deref(),
            team_organization_id.as_deref(),
            team_project_id.as_deref(),
            group_id.as_deref(),
        )
        .await
        .map_err(|e| {
            AppError::localized(
                "provider.usage.coding_plan_failed",
                format!("查询 Coding Plan 失败: {e}"),
                format!("Failed to query coding plan: {e}"),
            )
        })?;

        if !quota.success {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: quota.error,
            });
        }

        // ZenMux 的 tier 携带 USD 额度信息，需要编码为 JSON extra
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
                // 统一走 JSON extra 路径：把 resetsAt 与所有可选结构化字段塞进去，
                // 让前端 toQuotaTier / parseExtraObject 解析。
                // - usedCount/totalCount/countUnit：MiniMax / Zhipu 等提供绝对次数的 provider
                // - usedValueUsd / maxValueUsd / planLabel：ZenMux
                let mut extra_json = serde_json::json!({
                    "resetsAt": tier.resets_at,
                });
                if let (Some(used_count), Some(total_count), Some(unit)) =
                    (tier.used_count, tier.total_count, tier.count_unit.as_ref())
                {
                    extra_json["usedCount"] = serde_json::json!(used_count);
                    extra_json["totalCount"] = serde_json::json!(total_count);
                    extra_json["countUnit"] = serde_json::json!(unit);
                }
                if let Some(remains_ms) = tier.remains_time_ms {
                    // 服务端精确剩余毫秒数,优先于前端 end_time - Date.now() 计算
                    // （避免本地时钟漂移 + 拿到毫秒精度而非秒级）
                    extra_json["remainsTimeMs"] = serde_json::json!(remains_ms);
                }
                if has_usd {
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
                }
                UsageData {
                    plan_name: Some(tier.name.clone()),
                    remaining: Some(remaining),
                    total: Some(total),
                    used: Some(used),
                    unit: Some("%".to_string()),
                    is_valid: Some(true),
                    invalid_message: None,
                    extra: Some(extra_json.to_string()),
                }
            })
            .collect();

        return Ok(UsageResult {
            success: true,
            data: if data.is_empty() { None } else { Some(data) },
            error: None,
        });
    }

    if template_type == TEMPLATE_TYPE_BALANCE {
        return crate::services::balance::get_balance(base_url, api_key)
            .await
            .map_err(|e| {
                AppError::localized(
                    "provider.usage.balance_failed",
                    format!("查询余额失败: {e}"),
                    format!("Failed to query balance: {e}"),
                )
            });
    }

    if template_type == TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION {
        if !usage_script.map(|s| s.enabled).unwrap_or(false) {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some("Usage query is disabled".to_string()),
            });
        }

        let quota = crate::services::subscription::get_subscription_quota(app_type.as_str())
            .await
            .map_err(|e| {
                AppError::localized(
                    "provider.usage.subscription_failed",
                    format!("查询订阅额度失败: {e}"),
                    format!("Failed to query subscription quota: {e}"),
                )
            })?;

        if !quota.success {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: quota.error.or(quota.credential_message),
            });
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

        return Ok(UsageResult {
            success: true,
            data: if data.is_empty() { None } else { Some(data) },
            error: None,
        });
    }

    // 未识别的 template_type → 让调用方退回 JS 脚本路径。
    Err(AppError::localized(
        "provider.usage.template_unsupported",
        format!("模板类型 {template_type} 不支持 special 路径"),
        format!("Template type {template_type} is not supported by special path"),
    ))
}

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
            // 瞬时传输失败（send 失败/超时、读体中断）以 Err 传播，让前端 invoke
            // reject → react-query retry 并保留上次成功值；按错误 key 判定而非
            // 文案匹配。其余脚本/配置/HTTP 业务错误折叠成 success:false 展示文案。
            if let AppError::Localized { key, .. } = &err {
                if matches!(
                    *key,
                    "usage_script.request_failed" | "usage_script.read_response_failed"
                ) {
                    return Err(err);
                }
            }

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

/// Query usage for a *specific* key in a provider's pool, using that key's
/// own `api_key` value (read from `provider_api_keys`) instead of the
/// provider-level fallback in [`query_usage`].
///
/// `base_url` / `access_token` / `user_id` / `template_type` are still
/// sourced from the provider's `usage_script` config — those are typically
/// shared across the pool. If a future schema adds a per-key `base_url`,
/// fall through to that field here.
///
/// **Returns Ok(UsageResult { success: false, ... }) for:**
///   - `enabled = false` on the key row (don't even try the network call)
///   - `key_id` not found
///
/// **Returns Err for:** DB / transport failures (consistent with `query_usage`).
pub async fn query_usage_for_key(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    key_id: &str,
) -> Result<UsageResult, AppError> {
    // 1) 拉取目标 key 行——api_key 在这里，base_url 仍走 provider 级。
    let key_row = state.db.get_api_key(key_id)?.ok_or_else(|| {
        AppError::localized(
            "provider.api_key.not_found",
            format!("key {key_id} 不存在"),
            format!("key {key_id} not found"),
        )
    })?;
    // 跨 (provider_id, app_type) 校验：key 必须属于该 provider 的同一个 app_type pool，
    // 防止前端误传"p2 的 key id + p1 的 provider_id"导致数据混淆。
    if key_row.provider_id != provider_id || key_row.app_type != app_type.as_str() {
        return Err(AppError::localized(
            "provider.api_key.mismatch",
            format!(
                "key {key_id} 不属于 ({provider_id}, {})",
                app_type.as_str()
            ),
            format!(
                "key {key_id} does not belong to ({provider_id}, {})",
                app_type.as_str()
            ),
        ));
    }
    if !key_row.enabled {
        // Disabled key 直接判业务失败，不消耗一次 API 调用。
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some(format!("key {} 已禁用", key_row.label)),
        });
    }

    // 2) 拉 provider 的 usage_script 配置（base_url + access_token + user_id + script）。
    //    同时保留 usage_script 的引用——special-template 路径（TOKEN_PLAN / 余额 /
    //    官方订阅）需要读取 access_key_id / secret_access_key / group_id 等字段。
    let (script_code, timeout, base_url, access_token, user_id, template_type, usage_script_owned) = {
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
        // base_url 仍走 provider 级——per-key base_url 不在当前 schema 里。
        // resolve_script_credentials 内部已经会把 usage_script.base_url 优先、
        // provider.settings_config 兜底。
        let (_unused_api_key, base_url) = resolve_script_credentials(
            &app_type,
            provider,
            None, // 关键：忽略 usage_script.api_key，永远用 key 行的 api_key
            usage_script.base_url.as_deref(),
        );
        (
            usage_script.code.clone(),
            usage_script.timeout.unwrap_or(10),
            base_url,
            usage_script.access_token.clone(),
            usage_script.user_id.clone(),
            usage_script.template_type.clone(),
            usage_script.clone(),
        )
    };

    // 3) 账户级模板（TOKEN_PLAN / BALANCE / OFFICIAL_SUBSCRIPTION）→ 走专用
    //    后端路径，**用这把 key 自己的 api_key 跑**。MiniMax/Kimi/Zhipu 这些
    //    Coding Plan 把 api_key 当 Bearer token 用，所以一个 key pool 里的 N 把
    //    key 各自拿到对应账号的 5h/7d 配额——这是 per-key 路径真正想要的语义。
    //
    //    注意：TOKEN_PLAN 注入时 `usage_script.code` 是空串（见
    //    `config/codingPlanProviders.ts::injectCodingPlanUsageScript`），如果
    //    走 JS 路径 `ctx.eval("")` 直接抛错，所以这一段 short-circuit 是必须
    //    的——它修了「api count > 1 + TOKEN_PLAN 时 5h/7d 进度条永久消失」的
    //    Bug 1。
    let template_str = template_type.as_deref().unwrap_or("");
    if matches!(
        template_str,
        TEMPLATE_TYPE_TOKEN_PLAN
            | TEMPLATE_TYPE_BALANCE
            | TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION
    ) {
        return query_special_template(
            &app_type,
            template_str,
            &base_url,
            key_row.api_key.as_str(),
            Some(&usage_script_owned),
        )
        .await;
    }

    // 4) 自定义 / JS 脚本路径：用 key 自己的 api_key + provider 的 base_url 跑脚本。
    execute_and_format_usage_result(
        &script_code,
        key_row.api_key.as_str(),
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
