//! 项目 Profile 管理命令

use serde::Serialize;
use tauri::{Emitter, State};

use super::run_db_task;
use crate::database::Profile;
use crate::services::profile::{ProfilePayload, ProfileScope, ProfileService};
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

/// 每个分组当前激活的项目 id（未使用项目时为 null）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentProfileIds {
    pub claude: Option<String>,
    pub claude_desktop: Option<String>,
    pub codex: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesResponse {
    pub profiles: Vec<ProfileDto>,
    pub current_ids: CurrentProfileIds,
}

/// Profile 应用完成后的统一收尾：发事件 + 重建托盘菜单
///
/// 只对项目所属分组内的应用发 provider-switched。UI 与托盘两个入口必须
/// 共用此函数，保证事件 payload 形状一致（前端 App.tsx 的
/// provider-switched 监听依赖该形状）。
pub fn emit_profile_apply_events(
    app: &tauri::AppHandle,
    state: &AppState,
    profile_id: &str,
    scope: ProfileScope,
) {
    for app_type in scope.apps().iter() {
        let app_str = app_type.as_str();
        let (_, auto_failover_enabled) = state.db.get_proxy_flags_sync(app_str);
        let proxy_enabled = crate::mode::current::is_proxy(app_type);
        let provider_id = crate::mode::current::provider_for(
            &state.db,
            app_type,
            crate::mode::current::Purpose::InUse,
        )
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
        serde_json::json!({ "profileId": profile_id, "scope": scope.as_str() }),
    ) {
        log::error!("发射 profile-applied 事件失败: {e}");
    }
    crate::tray::refresh_tray_menu(app);
}

#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<ProfilesResponse, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || list_profiles_blocking(&state))
        .await
        .map_err(|e| format!("读取项目列表任务失败: {e}"))?
}

fn list_profiles_blocking(state: &AppState) -> Result<ProfilesResponse, String> {
    let profiles = ProfileService::list(state).map_err(|e| e.to_string())?;
    let current_ids = CurrentProfileIds {
        claude: state
            .db
            .get_current_profile_id(ProfileScope::Claude.as_str())
            .map_err(|e| e.to_string())?,
        claude_desktop: state
            .db
            .get_current_profile_id(ProfileScope::ClaudeDesktop.as_str())
            .map_err(|e| e.to_string())?,
        codex: state
            .db
            .get_current_profile_id(ProfileScope::Codex.as_str())
            .map_err(|e| e.to_string())?,
    };
    Ok(ProfilesResponse {
        profiles: profiles.into_iter().map(ProfileDto::from).collect(),
        current_ids,
    })
}

/// 项目的写入口（这里的命令和托盘的两项）互相串行：主线程不再替它们排队。`apply` 收尾才写
/// 当前项目指针、自动保存会整行回写旧项目，期间的清除 / 重命名 / 删除会被盖掉；新建的快照会
/// 拍到切到一半的状态。
pub(crate) static PROFILE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn profile_mutation<T, F>(state: &State<'_, AppState>, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    let _serial = PROFILE_LOCK.lock().await;
    run_db_task(state, task).await
}

#[tauri::command]
pub async fn create_profile(
    state: State<'_, AppState>,
    name: String,
    scope: String,
) -> Result<ProfileDto, String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    profile_mutation(&state, move |state| {
        ProfileService::create(state, &name, scope)
            .map(ProfileDto::from)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn update_profile(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    resnapshot: Option<bool>,
    scope: Option<String>,
) -> Result<ProfileDto, String> {
    let scope = scope
        .map(|s| ProfileScope::parse(&s))
        .transpose()
        .map_err(|e| e.to_string())?;
    profile_mutation(&state, move |state| {
        ProfileService::update(state, &id, name, resnapshot.unwrap_or(false), scope)
            .map(ProfileDto::from)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    profile_mutation(&state, move |state| {
        ProfileService::delete(state, &id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn clear_current_profile(
    state: State<'_, AppState>,
    scope: String,
) -> Result<(), String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    profile_mutation(&state, move |state| {
        state
            .db
            .set_current_profile_id(scope.as_str(), None)
            .map_err(|e| e.to_string())
    })
    .await
}

/// 应用项目快照（只作用于发起页所属分组内的应用）。
///
/// `ProviderService::switch` 用 block_on 拿切换锁，整段只能在阻塞线程池里跑
/// （同托盘的「应用项目」入口）。
#[tauri::command]
pub async fn apply_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    scope: String,
) -> Result<Vec<String>, String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    profile_mutation(&state, move |state| {
        let warnings = ProfileService::apply(state, &id, scope).map_err(|e| e.to_string())?;
        emit_profile_apply_events(&app, state, &id, scope);
        Ok(warnings)
    })
    .await
}
