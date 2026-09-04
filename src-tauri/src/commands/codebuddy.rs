use tauri::State;

use crate::services::provider;
use crate::store::AppState;

/// 读取 CodeBuddy 原生 settings.json 的当前模型 id（与供应商页展示、切换联动）。
#[tauri::command]
pub(crate) fn get_codebuddy_current_model() -> Result<Option<String>, String> {
    crate::codebuddy_config::current_model_id().map_err(|e| e.to_string())
}

/// 手动把 CodeBuddy 原生 models.json 中的条目同步进供应商列表（DB 镜像）。
#[tauri::command]
pub(crate) fn import_codebuddy_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    provider::import_codebuddy_providers_from_live(state.inner()).map_err(|e| e.to_string())
}
