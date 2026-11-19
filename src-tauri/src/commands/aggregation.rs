//! 全局聚合模式（Aggregation）相关的 Tauri 命令。

use std::str::FromStr;

use tauri::{AppHandle, Emitter, State};

use crate::app_config::AppType;
use crate::provider::UniversalProvider;
use crate::services::provider::aggregation::{
    current_visible_apps, delete_aggregation_provider as service_delete_provider,
    get_aggregation_overview as service_get_overview, mask_universal_apps_to_visible,
    promote_app_provider_to_universal as service_promote_to_universal,
    set_aggregation_app_mode as service_set_app_mode,
    set_global_aggregation_mode as service_set_global_mode,
    set_provider_target_apps as service_set_provider_target_apps,
    sync_universal_to_compatible_apps as service_sync_universal, AggregationOverview,
    SyncAllResult,
};
use crate::services::ProviderService;
use crate::store::AppState;

use super::provider::universal_mutation;

/// 获取聚合页总览（全局开关、所有通用供应商、所有各应用供应商、各应用状态）。
#[tauri::command]
pub async fn get_aggregation_overview(
    state: State<'_, AppState>,
) -> Result<AggregationOverview, String> {
    service_get_overview(state.inner()).await
}

/// 切换全局聚合模式开关（开启时同步通用供应商；关闭时将 Stack 模式的应用退回直连）。
#[tauri::command]
pub async fn set_global_aggregation_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AggregationOverview, String> {
    let overview = service_set_global_mode(state.inner(), &app, enabled).await?;
    let _ = app.emit(
        "universal-provider-synced",
        serde_json::json!({ "action": "mode", "enabled": enabled }),
    );
    Ok(overview)
}

/// 保存通用供应商并同步到全部启用的兼容应用（同时注册进 Stack 成员）。
#[tauri::command]
pub async fn save_and_sync_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    mut provider: UniversalProvider,
) -> Result<SyncAllResult, String> {
    let visible = current_visible_apps();
    mask_universal_apps_to_visible(&mut provider.apps, &visible);
    let saved = provider.clone();
    universal_mutation(&state, move |state| {
        ProviderService::upsert_universal(state, saved)
    })
    .await?;

    let result = service_sync_universal(state.inner(), &provider).await?;
    crate::tray::refresh_tray_menu(&app);
    let _ = app.emit(
        "universal-provider-synced",
        serde_json::json!({ "action": "sync", "id": provider.id }),
    );
    Ok(result)
}

/// 为指定外部供应商精确指定适用应用列表（未选中的应用会自动取消同步）。
#[tauri::command]
pub async fn set_aggregation_provider_apps(
    app: AppHandle,
    state: State<'_, AppState>,
    source_app: Option<String>,
    provider_id: String,
    target_apps: Vec<String>,
) -> Result<SyncAllResult, String> {
    let parsed_source = match source_app
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(raw) => Some(AppType::from_str(raw).map_err(|e| e.to_string())?),
        None => None,
    };
    let mut parsed_targets = Vec::with_capacity(target_apps.len());
    for raw in target_apps {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            parsed_targets.push(AppType::from_str(trimmed).map_err(|e| e.to_string())?);
        }
    }
    let result = service_set_provider_target_apps(
        state.inner(),
        &app,
        parsed_source,
        &provider_id,
        parsed_targets,
    )
    .await?;
    let _ = app.emit(
        "universal-provider-synced",
        serde_json::json!({ "action": "assign", "id": provider_id }),
    );
    Ok(result)
}

/// 将遗留应用外部供应商提升保存为聚合外部供应商，并同步到指定应用。
#[tauri::command]
pub async fn promote_app_provider_to_universal(
    app: AppHandle,
    state: State<'_, AppState>,
    source_app: String,
    source_provider_id: String,
    provider: UniversalProvider,
) -> Result<SyncAllResult, String> {
    let parsed_source = AppType::from_str(&source_app).map_err(|e| e.to_string())?;
    let provider_id = provider.id.clone();
    let result = service_promote_to_universal(
        state.inner(),
        &app,
        parsed_source,
        &source_provider_id,
        provider,
    )
    .await?;
    let _ = app.emit(
        "universal-provider-synced",
        serde_json::json!({ "action": "promote", "id": provider_id }),
    );
    Ok(result)
}

/// 从聚合页安全删除外部供应商及其同步到各应用的全部配置。
#[tauri::command]
pub async fn delete_aggregation_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    source_app: Option<String>,
    provider_id: String,
) -> Result<SyncAllResult, String> {
    let parsed_source = match source_app
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(raw) => Some(AppType::from_str(raw).map_err(|e| e.to_string())?),
        None => None,
    };
    let result =
        service_delete_provider(state.inner(), &app, parsed_source, &provider_id).await?;
    let _ = app.emit(
        "universal-provider-synced",
        serde_json::json!({ "action": "delete", "id": provider_id }),
    );
    Ok(result)
}

/// 在全局聚合页内让某个应用接入或退出聚合（代理模式）。
#[tauri::command]
pub async fn set_aggregation_app_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    app_type: String,
    enabled: bool,
    route_provider_id: Option<String>,
) -> Result<(), String> {
    let target_app = AppType::from_str(&app_type).map_err(|e| e.to_string())?;
    service_set_app_mode(state.inner(), &app, target_app, enabled, route_provider_id).await?;
    let _ = app.emit(
        "universal-provider-synced",
        serde_json::json!({ "action": "appMode", "appType": app_type }),
    );
    Ok(())
}
