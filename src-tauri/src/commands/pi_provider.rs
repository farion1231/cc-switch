use crate::pi_config;
use crate::services::pi_provider::{self, PiProviderDraft, PiProviderPatchPreview};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiProviderApplyResult {
    pub file_hash: String,
    pub models_json: Value,
    pub backup_path: String,
}

#[tauri::command]
pub fn list_pi_providers() -> Result<Value, String> {
    let loaded = pi_config::read_models_json().map_err(|e| e.to_string())?;
    Ok(loaded
        .value
        .get("providers")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({})))
}

#[tauri::command]
pub fn preview_pi_provider_patch(draft: PiProviderDraft) -> Result<PiProviderPatchPreview, String> {
    let loaded = pi_config::read_models_json().map_err(|e| e.to_string())?;
    pi_provider::build_upsert_preview(loaded, &draft).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn apply_pi_provider_patch(
    draft: PiProviderDraft,
    #[allow(non_snake_case)] expectedFileHash: String,
) -> Result<PiProviderApplyResult, String> {
    let models_path = pi_config::get_pi_models_json_path();
    let loaded = pi_config::read_models_json().map_err(|e| e.to_string())?;
    if loaded.file_hash != expectedFileHash {
        return Err(format!(
            "Pi models.json changed on disk; expected hash {}, found {}",
            expectedFileHash, loaded.file_hash
        ));
    }
    let backup = pi_config::create_backup(&models_path).map_err(|e| e.to_string())?;
    let next =
        pi_provider::upsert_provider_value(loaded.value, &draft).map_err(|e| e.to_string())?;
    let file_hash =
        pi_config::write_models_json_at(&models_path, &next).map_err(|e| e.to_string())?;

    Ok(PiProviderApplyResult {
        file_hash,
        models_json: next,
        backup_path: backup.path.display().to_string(),
    })
}

#[tauri::command]
pub fn delete_pi_provider(
    #[allow(non_snake_case)] providerId: String,
    #[allow(non_snake_case)] expectedFileHash: String,
) -> Result<PiProviderApplyResult, String> {
    let models_path = pi_config::get_pi_models_json_path();
    let loaded = pi_config::read_models_json().map_err(|e| e.to_string())?;
    if loaded.file_hash != expectedFileHash {
        return Err(format!(
            "Pi models.json changed on disk; expected hash {}, found {}",
            expectedFileHash, loaded.file_hash
        ));
    }

    let backup = pi_config::create_backup(&models_path).map_err(|e| e.to_string())?;
    let next =
        pi_provider::delete_provider_value(loaded.value, &providerId).map_err(|e| e.to_string())?;
    let file_hash =
        pi_config::write_models_json_at(&models_path, &next).map_err(|e| e.to_string())?;

    Ok(PiProviderApplyResult {
        file_hash,
        models_json: next,
        backup_path: backup.path.display().to_string(),
    })
}
