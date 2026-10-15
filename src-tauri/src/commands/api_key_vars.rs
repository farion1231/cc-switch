//! API Key 变量命令（设置页「API Key 变量」与供应商表单的引用校验）

use std::str::FromStr;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::app_config::AppType;
use crate::services::api_key_vars::{
    self, ApiKeyVarInput, ApiKeyVarView, RefCheck, RefreshOutcome, RefreshReason, ResolvedScript,
    TestOutcome,
};
use crate::store::AppState;

#[tauri::command]
pub fn list_api_key_vars(state: State<'_, AppState>) -> Result<Vec<ApiKeyVarView>, String> {
    api_key_vars::list_views(&state.db).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn upsert_api_key_var(
    state: State<'_, AppState>,
    input: ApiKeyVarInput,
    original_name: Option<String>,
) -> Result<ApiKeyVarView, String> {
    api_key_vars::upsert(state.inner(), &input, original_name.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_api_key_var(state: State<'_, AppState>, name: String) -> Result<(), String> {
    api_key_vars::delete(state.inner(), &name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resolve_api_key_var_script(dir: String) -> Result<ResolvedScript, String> {
    api_key_vars::runner::resolve_script(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_api_key_var(input: ApiKeyVarInput) -> Result<TestOutcome, String> {
    api_key_vars::test_script(&input)
        .await
        .map_err(|e| e.to_string())
}

/// 返回内置脚本模板，用于设置页的完整代码预览。
#[tauri::command]
pub fn get_api_key_var_template() -> Vec<api_key_vars::template::ApiKeyVarTemplateFile> {
    api_key_vars::template::files()
}

/// 由后端弹出保存对话框并下载内置模板。前端不能传入任意写入路径。
/// 用户取消时返回 `None`，成功时返回选择的绝对路径。
#[tauri::command]
pub async fn download_api_key_var_template<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let bytes = api_key_vars::template::zip_bytes()?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("ZIP archive", &["zip"])
            .set_file_name("cc-switch-api-key-template.zip")
            .blocking_save_file()
    })
    .await
    .map_err(|error| format!("打开模板保存对话框失败: {error}"))?;

    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|error| format!("无效的模板保存路径: {error}"))?;
    api_key_vars::template::write_zip(&path, &bytes)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn refresh_api_key_var(app: AppHandle, name: String) -> Result<RefreshOutcome, String> {
    if app.try_state::<AppState>().is_none() {
        return Err("应用状态不可用".to_string());
    }
    api_key_vars::refresh_var(&app, &name, RefreshReason::Manual)
        .await
        .map_err(|e| e.to_string())
}

/// 检查一个 Key 输入值（`{{NAME}}`）的解析状态；不是引用时返回 None
#[tauri::command]
pub fn check_api_key_ref(
    state: State<'_, AppState>,
    value: String,
) -> Result<Option<RefCheck>, String> {
    api_key_vars::check_value(&state.db, &value).map_err(|e| e.to_string())
}

/// 检查一份供应商配置中所有 Key 字段的引用
#[tauri::command(rename_all = "camelCase")]
pub fn check_api_key_refs(
    state: State<'_, AppState>,
    app: String,
    settings_config: serde_json::Value,
) -> Result<Vec<RefCheck>, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    api_key_vars::check_refs(&state.db, &app_type, &settings_config).map_err(|e| e.to_string())
}
