use crate::error::AppError;
use crate::services::model_verify::{ModelVerifyRequest, ModelVerifyResult, ModelVerifyService};

#[tauri::command]
pub async fn verify_model_authenticity(
    request: ModelVerifyRequest,
) -> Result<ModelVerifyResult, AppError> {
    ModelVerifyService::verify(request).await
}
