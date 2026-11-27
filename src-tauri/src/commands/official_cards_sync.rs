use tauri::{Emitter, Manager};

use crate::app_config::AppType;
use crate::commands::agy_account::AgyAccountState;
use crate::commands::claude_account::ClaudeAccountState;
use crate::commands::codebuddy_account::CodebuddyAccountState;
use crate::commands::codex_oauth::CodexOAuthState;
use crate::commands::copilot::CopilotAuthState;
use crate::commands::workbuddy_account::{run_workbuddy, WorkBuddyAccountState};
use crate::commands::xai_oauth::XaiOAuthState;
use crate::services::provider::official_account_cards::{
    clear_dismissed_and_sync_locked, sync_app_cards_locked, OfficialAccountEntry,
    OFFICIAL_CARD_APPS,
};
use crate::store::AppState;

pub(crate) fn emit_official_cards_updated<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    app_type: &AppType,
) {
    let _ = app.emit(
        "official-account-cards-updated",
        serde_json::json!({ "appType": app_type.as_str() }),
    );
}

/// Syncs official account cards for `app_type` against `entries` and notifies the frontend when
/// cards were created or dismissal flags were explicitly cleared.
pub(crate) async fn sync_official_cards_and_emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    app_state: &AppState,
    app_type: &AppType,
    entries: &[OfficialAccountEntry],
    clear_dismissed: bool,
) {
    if !entries.is_empty() {
        let state = app_state.clone();
        let sync_app = app_type.clone();
        let entries_owned = entries.to_vec();
        let result = tauri::async_runtime::spawn_blocking(move || {
            if clear_dismissed {
                clear_dismissed_and_sync_locked(&state, &sync_app, &entries_owned)
            } else {
                sync_app_cards_locked(&state, &sync_app, &entries_owned)
            }
        })
        .await;
        match result {
            Ok(Ok(created)) => {
                if clear_dismissed || created > 0 {
                    emit_official_cards_updated(app, app_type);
                }
            }
            Ok(Err(error)) => {
                log::warn!(
                    "[OfficialAccountCards] 同步 {} 官方账号卡片失败: {error}",
                    app_type.as_str()
                );
            }
            Err(error) => {
                log::warn!(
                    "[OfficialAccountCards] 同步 {} 官方账号卡片任务失败: {error}",
                    app_type.as_str()
                );
            }
        }
    } else if clear_dismissed {
        emit_official_cards_updated(app, app_type);
    }
}

async fn collect_saved_official_accounts<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    app_type: &AppType,
) -> Vec<OfficialAccountEntry> {
    match app_type {
        AppType::Claude => {
            let Some(state) = app_handle.try_state::<ClaudeAccountState>() else {
                return Vec::new();
            };
            state
                .0
                .list()
                .await
                .into_iter()
                .map(|account| OfficialAccountEntry {
                    login: account.label(),
                    id: account.id,
                })
                .collect()
        }
        AppType::Codex => {
            let Some(state) = app_handle.try_state::<CodexOAuthState>() else {
                return Vec::new();
            };
            state
                .0
                .list_accounts()
                .await
                .into_iter()
                .map(|account| OfficialAccountEntry {
                    id: account.id,
                    login: account.login,
                })
                .collect()
        }
        AppType::GrokBuild => {
            let Some(state) = app_handle.try_state::<XaiOAuthState>() else {
                return Vec::new();
            };
            let manager = state.0.read().await;
            manager
                .list_accounts()
                .await
                .into_iter()
                .map(|account| OfficialAccountEntry {
                    id: account.id,
                    login: account.login,
                })
                .collect()
        }
        AppType::Agy => {
            let Some(state) = app_handle.try_state::<AgyAccountState>() else {
                return Vec::new();
            };
            let accounts = if crate::services::agy_account::keychain::supported() {
                match state.0.status().await {
                    Ok(status) => status.accounts,
                    Err(_) => state.0.list().await,
                }
            } else {
                state.0.list().await
            };
            accounts
                .into_iter()
                .map(|account| OfficialAccountEntry {
                    login: account.label(),
                    id: account.id,
                })
                .collect()
        }
        AppType::Copilot => {
            let Some(state) = app_handle.try_state::<CopilotAuthState>() else {
                return Vec::new();
            };
            let manager = state.0.read().await;
            manager
                .list_accounts()
                .await
                .into_iter()
                .map(|account| OfficialAccountEntry {
                    id: account.id,
                    login: account.login,
                })
                .collect()
        }
        AppType::WorkBuddy => {
            let Some(state) = app_handle.try_state::<WorkBuddyAccountState>() else {
                return Vec::new();
            };
            run_workbuddy(&state.0, |manager| manager.status())
                .await
                .map(|status| {
                    status
                        .accounts
                        .into_iter()
                        .map(|account| OfficialAccountEntry {
                            login: account.label(),
                            id: account.id,
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        AppType::Codebuddy => {
            let Some(state) = app_handle.try_state::<CodebuddyAccountState>() else {
                return Vec::new();
            };
            state
                .0
                .status()
                .await
                .map(|status| {
                    status
                        .accounts
                        .into_iter()
                        .map(|account| OfficialAccountEntry {
                            login: account.label(),
                            id: account.id,
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Syncs official account cards for all supported apps on startup without overriding local
/// dismissed markers.
pub(crate) async fn sync_all_official_account_cards_on_startup<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) {
    let Some(app_state) = app_handle.try_state::<AppState>() else {
        return;
    };
    for (app_type, _) in OFFICIAL_CARD_APPS {
        let entries = collect_saved_official_accounts(app_handle, app_type).await;
        if !entries.is_empty() {
            sync_official_cards_and_emit(app_handle, app_state.inner(), app_type, &entries, false)
                .await;
        }
    }
}
