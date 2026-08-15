use tauri::State;

use crate::store::AppState;
use crate::workbuddy_config;

// ============================================================================
// WorkBuddy Provider Commands
// ============================================================================

/// 从 WorkBuddy live models.json 导入 provider 到数据库。
///
/// WorkBuddy 使用 additive 模式 —— 用户可能已在 models.json 里配置了模型。
#[tauri::command]
pub fn import_workbuddy_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_workbuddy_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

/// 获取 WorkBuddy live 配置里的 provider id 列表（按网关聚合后的 id）。
#[tauri::command]
pub fn get_workbuddy_live_provider_ids() -> Result<Vec<String>, String> {
    workbuddy_config::get_typed_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}

/// 获取单个 WorkBuddy provider（按网关聚合）的配置片段。
#[tauri::command]
pub fn get_workbuddy_live_provider(
    #[allow(non_snake_case)] providerId: String,
) -> Result<Option<serde_json::Value>, String> {
    workbuddy_config::get_typed_providers()
        .map(|providers| {
            providers
                .get(&providerId)
                .and_then(|cfg| serde_json::to_value(cfg).ok())
        })
        .map_err(|e| e.to_string())
}

/// 扫描 models.json 的健康状况（是否合法 JSON 等）。
#[tauri::command]
pub fn scan_workbuddy_config_health() -> Result<Vec<String>, String> {
    Ok(workbuddy_config::scan_health())
}
