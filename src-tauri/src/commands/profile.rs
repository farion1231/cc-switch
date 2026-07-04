//! 项目 Profile 管理命令

use serde::Serialize;
use tauri::{Emitter, State};

use crate::database::Profile;
use crate::services::profile::{ProfilePayload, ProfileService, PROFILE_APPS};
use crate::store::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDto {
    pub id: String,
    pub name: String,
    pub payload: ProfilePayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

impl From<Profile> for ProfileDto {
    fn from(profile: Profile) -> Self {
        // 单条 payload 损坏不应拖垮整个列表：降级为默认值并记日志
        let payload = serde_json::from_str(&profile.payload).unwrap_or_else(|e| {
            log::warn!(
                "解析 profile '{}' payload 失败，使用默认值: {e}",
                profile.id
            );
            ProfilePayload::default()
        });
        Self {
            id: profile.id,
            name: profile.name,
            payload,
            created_at: profile.created_at,
            updated_at: profile.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesResponse {
    pub profiles: Vec<ProfileDto>,
    pub current_id: Option<String>,
}

/// Profile 应用完成后的统一收尾：发事件 + 重建托盘菜单
///
/// UI 与托盘两个入口必须共用此函数，保证事件 payload 形状一致
/// （前端 App.tsx 的 provider-switched 监听依赖该形状）。
pub fn emit_profile_apply_events(app: &tauri::AppHandle, state: &AppState, profile_id: &str) {
    for app_type in PROFILE_APPS.iter() {
        let app_str = app_type.as_str();
        let (proxy_enabled, auto_failover_enabled) = state.db.get_proxy_flags_sync(app_str);
        let provider_id = crate::settings::get_effective_current_provider(&state.db, app_type)
            .ok()
            .flatten()
            .unwrap_or_default();
        let event_data = serde_json::json!({
            "appType": app_str,
            "proxyEnabled": proxy_enabled,
            "autoFailoverEnabled": auto_failover_enabled,
            "providerId": provider_id,
        });
        if let Err(e) = app.emit("provider-switched", event_data) {
            log::error!("发射 provider-switched 事件失败: {e}");
        }
    }
    if let Err(e) = app.emit(
        "profile-applied",
        serde_json::json!({ "profileId": profile_id }),
    ) {
        log::error!("发射 profile-applied 事件失败: {e}");
    }
    crate::tray::refresh_tray_menu(app);
}

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Result<ProfilesResponse, String> {
    let (profiles, current_id) = ProfileService::list(&state).map_err(|e| e.to_string())?;
    Ok(ProfilesResponse {
        profiles: profiles.into_iter().map(ProfileDto::from).collect(),
        current_id,
    })
}

#[tauri::command]
pub fn create_profile(state: State<'_, AppState>, name: String) -> Result<ProfileDto, String> {
    ProfileService::create(&state, &name)
        .map(ProfileDto::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_profile(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    resnapshot: Option<bool>,
) -> Result<ProfileDto, String> {
    ProfileService::update(&state, &id, name, resnapshot.unwrap_or(false))
        .map(ProfileDto::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    ProfileService::delete(&state, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_current_profile(state: State<'_, AppState>) -> Result<(), String> {
    state
        .db
        .set_current_profile_id(None)
        .map_err(|e| e.to_string())
}

/// 应用项目快照。
///
/// 注意：必须保持同步命令（跑在 Tauri 线程池）——`ProviderService::switch`
/// 内部使用 block_on 获取切换锁，放进 async 命令会在运行时线程上 panic。
#[tauri::command]
pub fn apply_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<String>, String> {
    let warnings = ProfileService::apply(&state, &id).map_err(|e| e.to_string())?;
    emit_profile_apply_events(&app, &state, &id);
    Ok(warnings)
}
