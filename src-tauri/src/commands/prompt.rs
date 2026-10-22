use cc_command_api::command_api;
use indexmap::IndexMap;
use std::str::FromStr;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::services::pi_prompt_files::{
    PiPromptFileKind, PiPromptFileService, PiPromptFileSnapshot, PiPromptTemplate,
    PiPromptTemplateService,
};
use crate::services::prompt::PromptService;
use crate::store::AppState;

#[command_api]
pub async fn get_prompts(
    app: String,
    state: &AppState,
) -> Result<IndexMap<String, Prompt>, AppError> {
    let app_type = AppType::from_str(&app).map_err(|e| AppError::Config(e.to_string()))?;
    PromptService::get_prompts(&state, app_type).map_err(|e| AppError::Config(e.to_string()))
}

#[command_api]
pub async fn upsert_prompt(
    app: String,
    id: String,
    prompt: Prompt,
    state: &AppState,
) -> Result<(), AppError> {
    let app_type = AppType::from_str(&app).map_err(|e| AppError::Config(e.to_string()))?;
    PromptService::upsert_prompt(&state, app_type, &id, prompt)
        .map_err(|e| AppError::Config(e.to_string()))
}

#[command_api]
pub async fn delete_prompt(app: String, id: String, state: &AppState) -> Result<(), AppError> {
    let app_type = AppType::from_str(&app).map_err(|e| AppError::Config(e.to_string()))?;
    PromptService::delete_prompt(&state, app_type, &id).map_err(|e| AppError::Config(e.to_string()))
}

#[command_api]
pub async fn enable_prompt(app: String, id: String, state: &AppState) -> Result<(), AppError> {
    let app_type = AppType::from_str(&app).map_err(|e| AppError::Config(e.to_string()))?;
    PromptService::enable_prompt(&state, app_type, &id).map_err(|e| AppError::Config(e.to_string()))
}

#[command_api]
pub async fn import_prompt_from_file(app: String, state: &AppState) -> Result<String, AppError> {
    let app_type = AppType::from_str(&app).map_err(|e| AppError::Config(e.to_string()))?;
    PromptService::import_from_file(&state, app_type).map_err(|e| AppError::Config(e.to_string()))
}

#[command_api]
pub async fn get_current_prompt_file_content(app: String) -> Result<Option<String>, AppError> {
    let app_type = AppType::from_str(&app).map_err(|e| AppError::Config(e.to_string()))?;
    PromptService::get_current_file_content(app_type).map_err(|e| AppError::Config(e.to_string()))
}

/// 提示词目标文件的位置：`path` 是完整路径（复制用），`displayPath` 把主目录写成 `~`（显示用）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFileLocation {
    pub path: String,
    pub display_path: String,
}

#[command_api]
pub async fn get_prompt_file_location(app: String) -> Result<PromptFileLocation, AppError> {
    let app_type = AppType::from_str(&app).map_err(|e| AppError::Config(e.to_string()))?;
    let path = crate::prompt_files::prompt_file_path(&app_type)
        .map_err(|e| AppError::Config(e.to_string()))?;
    Ok(PromptFileLocation {
        path: path.to_string_lossy().to_string(),
        display_path: crate::prompt_files::display_path(&path),
    })
}

#[command_api]
pub async fn get_pi_prompt_file(kind: PiPromptFileKind) -> Result<PiPromptFileSnapshot, AppError> {
    PiPromptFileService::read(kind).map_err(|error| AppError::Config(error.to_string()))
}

#[command_api]
pub async fn replace_pi_prompt_file(
    kind: PiPromptFileKind,
    #[allow(non_snake_case)] expectedRevision: String,
    content: String,
) -> Result<PiPromptFileSnapshot, AppError> {
    PiPromptFileService::replace(kind, &expectedRevision, &content)
        .map_err(|error| AppError::Config(error.to_string()))
}

#[command_api]
pub async fn delete_pi_prompt_file(
    kind: PiPromptFileKind,
    #[allow(non_snake_case)] expectedRevision: String,
) -> Result<bool, AppError> {
    PiPromptFileService::delete(kind, &expectedRevision)
        .map_err(|error| AppError::Config(error.to_string()))
}

#[command_api]
pub async fn list_pi_prompt_templates() -> Result<Vec<PiPromptTemplate>, AppError> {
    PiPromptTemplateService::list().map_err(|error| AppError::Config(error.to_string()))
}

#[command_api]
pub async fn upsert_pi_prompt_template(
    slug: String,
    #[allow(non_snake_case)] originalSlug: Option<String>,
    #[allow(non_snake_case)] expectedRevision: String,
    content: String,
) -> Result<PiPromptTemplate, AppError> {
    PiPromptTemplateService::upsert(&slug, originalSlug.as_deref(), &expectedRevision, &content)
        .map_err(|error| AppError::Config(error.to_string()))
}

#[command_api]
pub async fn delete_pi_prompt_template(
    slug: String,
    #[allow(non_snake_case)] expectedRevision: String,
) -> Result<bool, AppError> {
    PiPromptTemplateService::delete(&slug, &expectedRevision)
        .map_err(|error| AppError::Config(error.to_string()))
}
