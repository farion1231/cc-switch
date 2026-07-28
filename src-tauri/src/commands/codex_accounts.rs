#![allow(non_snake_case)]

use std::collections::HashMap;

use crate::codex_accounts::{CodexAccountSummary, CodexAccountSwitchResult, CodexAppRestartResult};
use crate::services::quota_forecast::CodexQuotaForecast;
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;
use tauri::{Emitter, State};

#[tauri::command]
pub fn codex_list_account_snapshots() -> Result<Vec<CodexAccountSummary>, String> {
    crate::codex_accounts::list_accounts().map_err(Into::into)
}

#[tauri::command]
pub async fn get_all_codex_quotas(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<HashMap<String, SubscriptionQuota>, String> {
    let quotas: HashMap<String, SubscriptionQuota> =
        crate::codex_accounts::get_all_account_quotas()
            .await
            .map_err(Into::<String>::into)?;

    let forecasts = crate::services::quota_forecast::record_and_forecast(&state.db, quotas.iter())
        .unwrap_or_else(|error| {
            log::warn!("保存 Codex 额度历史失败: {error}");
            HashMap::new()
        });

    for (account_key, quota) in &quotas {
        state
            .usage_cache
            .put_codex_account(account_key.clone(), quota.clone());
    }

    let payload = serde_json::json!({
        "kind": "codex-all",
        "accounts": quotas.iter().map(|(account_key, quota)| {
            serde_json::json!({ "accountKey": account_key, "quota": quota })
        }).collect::<Vec<_>>(),
        "forecasts": forecasts,
    });
    if let Err(e) = app.emit("codex-account-quotas-updated", payload) {
        log::error!("emit codex-account-quotas-updated 失败: {e}");
    }
    crate::tray::schedule_tray_refresh(&app);

    Ok(quotas)
}

#[tauri::command]
pub fn get_codex_quota_forecasts(
    state: State<'_, AppState>,
) -> Result<HashMap<String, CodexQuotaForecast>, String> {
    crate::services::quota_forecast::load_forecasts(&state.db).map_err(Into::into)
}

#[tauri::command]
pub fn codex_capture_current_account(label: Option<String>) -> Result<CodexAccountSummary, String> {
    crate::codex_accounts::capture_current(label).map_err(Into::into)
}

#[tauri::command]
pub fn codex_rename_account_snapshot(
    accountKey: String,
    profileName: String,
) -> Result<CodexAccountSummary, String> {
    crate::codex_accounts::rename_account(accountKey, profileName).map_err(Into::into)
}

#[tauri::command]
pub fn codex_switch_account(accountKey: String) -> Result<CodexAccountSwitchResult, String> {
    crate::codex_accounts::switch_account(accountKey).map_err(Into::into)
}

#[tauri::command]
pub fn codex_rollback_last_account_switch() -> Result<CodexAccountSwitchResult, String> {
    crate::codex_accounts::rollback_last_switch().map_err(Into::into)
}

#[tauri::command]
pub fn codex_restart_app() -> Result<CodexAppRestartResult, String> {
    crate::codex_accounts::restart_codex_app().map_err(Into::into)
}
