use indexmap::IndexMap;
use std::str::FromStr;

use tauri::State;

use crate::app_config::AppType;
use crate::prompt::Prompt;
use crate::services::pi_prompt_files::{
    PiPromptFileKind, PiPromptFileService, PiPromptFileSnapshot, PiPromptTemplate,
    PiPromptTemplateService,
};
use crate::services::prompt::PromptService;
use crate::store::AppState;

#[tauri::command]
pub async fn get_prompts(
    app: String,
    state: State<'_, AppState>,
) -> Result<IndexMap<String, Prompt>, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    PromptService::get_prompts(&state, app_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_prompt(
    app: String,
    id: String,
    prompt: Prompt,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    PromptService::upsert_prompt(&state, app_type, &id, prompt).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_prompt(
    app: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    PromptService::delete_prompt(&state, app_type, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn enable_prompt(
    app: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    PromptService::enable_prompt(&state, app_type, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_prompt_from_file(
    app: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    PromptService::import_from_file(&state, app_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_current_prompt_file_content(app: String) -> Result<Option<String>, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    PromptService::get_current_file_content(app_type).map_err(|e| e.to_string())
}

/// 提示词目标文件的位置：`path` 是完整路径（复制用），`displayPath` 把主目录写成 `~`（显示用）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFileLocation {
    pub path: String,
    pub display_path: String,
}

#[tauri::command]
pub async fn get_prompt_file_location(app: String) -> Result<PromptFileLocation, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    let path = crate::prompt_files::prompt_file_path(&app_type).map_err(|e| e.to_string())?;
    Ok(PromptFileLocation {
        path: path.to_string_lossy().to_string(),
        display_path: crate::prompt_files::display_path(&path),
    })
}

/// Resolve the `app` argument of the Pi-family commands: only Pi and RPI own
/// this native layout, and each one reads and writes its own directory.
fn pi_family_app(app: &str) -> Result<AppType, String> {
    let app_type = AppType::from_str(app).map_err(|e| e.to_string())?;
    if matches!(app_type, AppType::Pi | AppType::Rpi) {
        Ok(app_type)
    } else {
        Err(format!("{app} does not use Pi-family prompt files"))
    }
}

#[tauri::command]
pub async fn get_pi_prompt_file(
    app: String,
    kind: PiPromptFileKind,
) -> Result<PiPromptFileSnapshot, String> {
    let app_type = pi_family_app(&app)?;
    PiPromptFileService::read(&app_type, kind).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn replace_pi_prompt_file(
    app: String,
    kind: PiPromptFileKind,
    #[allow(non_snake_case)] expectedRevision: String,
    content: String,
) -> Result<PiPromptFileSnapshot, String> {
    let app_type = pi_family_app(&app)?;
    PiPromptFileService::replace(&app_type, kind, &expectedRevision, &content)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_pi_prompt_file(
    app: String,
    kind: PiPromptFileKind,
    #[allow(non_snake_case)] expectedRevision: String,
) -> Result<bool, String> {
    let app_type = pi_family_app(&app)?;
    PiPromptFileService::delete(&app_type, kind, &expectedRevision)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_pi_prompt_templates(app: String) -> Result<Vec<PiPromptTemplate>, String> {
    let app_type = pi_family_app(&app)?;
    PiPromptTemplateService::list(&app_type).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn upsert_pi_prompt_template(
    app: String,
    slug: String,
    #[allow(non_snake_case)] originalSlug: Option<String>,
    #[allow(non_snake_case)] expectedRevision: String,
    content: String,
) -> Result<PiPromptTemplate, String> {
    let app_type = pi_family_app(&app)?;
    PiPromptTemplateService::upsert(
        &app_type,
        &slug,
        originalSlug.as_deref(),
        &expectedRevision,
        &content,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_pi_prompt_template(
    app: String,
    slug: String,
    #[allow(non_snake_case)] expectedRevision: String,
) -> Result<bool, String> {
    let app_type = pi_family_app(&app)?;
    PiPromptTemplateService::delete(&app_type, &slug, &expectedRevision)
        .map_err(|error| error.to_string())
}
