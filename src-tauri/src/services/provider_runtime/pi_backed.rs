use crate::pi_config;
use crate::services::pi_provider::{self, PiProviderDraft, PiProviderPatchPreview};
use crate::AppError;

use super::types::{PiProviderWriteResult, PiProvidersMap};

pub struct PiBackedProviderRuntime;

impl PiBackedProviderRuntime {
    pub fn list() -> Result<PiProvidersMap, AppError> {
        let loaded = pi_config::read_models_json().map_err(|e| AppError::Message(e.to_string()))?;
        Ok(loaded
            .value
            .get("providers")
            .and_then(|value| value.as_object())
            .cloned()
            .unwrap_or_default())
    }

    pub fn read_models_meta() -> Result<String, AppError> {
        let loaded = pi_config::read_models_json().map_err(|e| AppError::Message(e.to_string()))?;
        Ok(loaded.file_hash)
    }

    pub fn preview_upsert(draft: &PiProviderDraft) -> Result<PiProviderPatchPreview, AppError> {
        let loaded = pi_config::read_models_json().map_err(|e| AppError::Message(e.to_string()))?;
        pi_provider::build_upsert_preview(loaded, draft)
            .map_err(|e| AppError::Message(e.to_string()))
    }

    pub fn apply_upsert(
        draft: &PiProviderDraft,
        expected_file_hash: &str,
    ) -> Result<PiProviderWriteResult, AppError> {
        let models_path = pi_config::get_pi_models_json_path();
        let loaded = pi_config::read_models_json().map_err(|e| AppError::Message(e.to_string()))?;
        if loaded.file_hash != expected_file_hash {
            return Err(AppError::Message(format!(
                "Pi models.json changed on disk; expected hash {}, found {}",
                expected_file_hash, loaded.file_hash
            )));
        }

        let backup =
            pi_config::create_backup(&models_path).map_err(|e| AppError::Message(e.to_string()))?;
        let next = pi_provider::upsert_provider_value(loaded.value, draft)
            .map_err(|e| AppError::Message(e.to_string()))?;
        let file_hash = pi_config::write_models_json_with_expected_hash_at(
            &models_path,
            &next,
            expected_file_hash,
        )
        .map_err(|e| AppError::Message(e.to_string()))?;

        Ok(PiProviderWriteResult {
            file_hash,
            models_json: next,
            backup_path: backup.path.display().to_string(),
        })
    }

    pub fn delete(
        provider_id: &str,
        expected_file_hash: &str,
    ) -> Result<PiProviderWriteResult, AppError> {
        let models_path = pi_config::get_pi_models_json_path();
        let loaded = pi_config::read_models_json().map_err(|e| AppError::Message(e.to_string()))?;
        if loaded.file_hash != expected_file_hash {
            return Err(AppError::Message(format!(
                "Pi models.json changed on disk; expected hash {}, found {}",
                expected_file_hash, loaded.file_hash
            )));
        }

        let backup =
            pi_config::create_backup(&models_path).map_err(|e| AppError::Message(e.to_string()))?;
        let next = pi_provider::delete_provider_value(loaded.value, provider_id)
            .map_err(|e| AppError::Message(e.to_string()))?;
        let file_hash = pi_config::write_models_json_with_expected_hash_at(
            &models_path,
            &next,
            expected_file_hash,
        )
        .map_err(|e| AppError::Message(e.to_string()))?;

        Ok(PiProviderWriteResult {
            file_hash,
            models_json: next,
            backup_path: backup.path.display().to_string(),
        })
    }
}
