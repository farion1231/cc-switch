use std::str::FromStr;
use tauri::{Emitter, State};

use crate::app_config::AppType;
use crate::services::subscription::{CredentialStatus, SubscriptionQuota};
use crate::store::AppState;

/// 查询官方订阅额度
///
/// 读取 CLI 工具已有的 OAuth 凭据并调用官方 API 获取使用额度。
/// 结果（无论业务失败还是 transport 层 Err）都会写入 `UsageCache`、通知托盘
/// 刷新，并 emit `usage-cache-updated`，让前端 React Query 与托盘共享同一份
/// 最新数据。失败快照写入后 `format_subscription_summary` 会通过 `success=false`
/// 守卫返回 `None`，托盘 suffix 自然消失，避免长期滞留旧配额数字。
/// Err 原样向前端返回，React Query 的 onError 不会被吞掉。
#[tauri::command]
pub async fn get_subscription_quota(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    tool: String,
) -> Result<SubscriptionQuota, String> {
    let inner = crate::services::subscription::get_subscription_quota(&tool).await;
    let snapshot = match &inner {
        Ok(q) => q.clone(),
        // transport 层 Err —— 凭据状态不明，用 Valid 表达"凭据没问题，是通信/parse 出错"。
        Err(err_msg) => SubscriptionQuota::error(&tool, CredentialStatus::Valid, err_msg.clone()),
    };
    if let Ok(app_type) = AppType::from_str(&tool) {
        let payload = serde_json::json!({
            "kind": "subscription",
            "appType": app_type.as_str(),
            "data": &snapshot,
        });
        if let Err(e) = app.emit("usage-cache-updated", payload) {
            log::error!("emit usage-cache-updated (subscription) 失败: {e}");
        }
        state.usage_cache.put_subscription(app_type, snapshot);
        crate::tray::schedule_tray_refresh(&app);
    }
    inner
}

/// 查询指定 Codex provider 自带 OAuth 凭据的订阅额度。
///
/// 普通 `get_subscription_quota("codex")` 只能读取当前写入 `~/.codex/auth.json`
/// 的 live 凭据；provider 列表里同时展示多个官方 Codex 账号时，需要读取每张
/// 卡片自己的 `settings_config.auth`。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_codex_provider_quota(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<SubscriptionQuota, String> {
    let app_type = AppType::Codex;
    let app_type_str = app_type.as_str();

    let mut provider = state
        .db
        .get_provider_by_id(&provider_id, app_type_str)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Codex provider not found: {provider_id}"))?;
    let before_settings = provider.settings_config.clone();
    let current_provider_id = state
        .db
        .get_current_provider(app_type_str)
        .map_err(|e| e.to_string())?;
    let is_current_provider = current_provider_id.as_deref() == Some(provider_id.as_str());

    let inner =
        crate::services::subscription::get_codex_provider_subscription_quota(
            &mut provider.settings_config,
        )
        .await;

    if provider.settings_config != before_settings {
        state
            .db
            .save_provider(app_type_str, &provider)
            .map_err(|e| e.to_string())?;

        if is_current_provider {
            if let Some(auth) = provider.settings_config.get("auth") {
                let cfg_text = provider
                    .settings_config
                    .get("config")
                    .and_then(serde_json::Value::as_str);
                if let Err(e) = crate::codex_config::write_codex_live_atomic(auth, cfg_text) {
                    log::warn!(
                        "[CodexQuota] refreshed current provider token but failed to sync live auth: {e}"
                    );
                }
            }
        }
    }

    let snapshot = match &inner {
        Ok(q) => q.clone(),
        Err(err_msg) => SubscriptionQuota::error("codex", CredentialStatus::Valid, err_msg.clone()),
    };

    state.usage_cache.put_provider_subscription(
        app_type.clone(),
        provider_id.clone(),
        snapshot.clone(),
    );

    if is_current_provider {
        let payload = serde_json::json!({
            "kind": "subscription",
            "appType": app_type.as_str(),
            "data": &snapshot,
        });
        if let Err(e) = app.emit("usage-cache-updated", payload) {
            log::error!("emit usage-cache-updated (codex provider subscription) 失败: {e}");
        }
        state.usage_cache.put_subscription(app_type, snapshot);
        crate::tray::schedule_tray_refresh(&app);
    }

    inner
}
