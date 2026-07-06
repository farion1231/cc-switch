use crate::pi_config;
use crate::services::pi_provider::{self, PiProviderDraft, PiProviderPatchPreview};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiConnectivityResult {
    pub reachable: bool,
    pub status_code: Option<u16>,
    pub error_kind: Option<String>,
    pub detail: Option<String>,
}

/// Resolve a Pi models.json apiKey value into a usable key.
/// - `$VAR` -> environment variable
/// - `!command` -> shell command output (cross-platform)
/// - literal -> as-is
fn resolve_api_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(var) = trimmed.strip_prefix('$') {
        return std::env::var(var).ok().filter(|v| !v.is_empty());
    }
    if let Some(cmd) = trimmed.strip_prefix('!') {
        return run_shell_command(cmd).filter(|v| !v.is_empty());
    }
    Some(trimmed.to_string())
}

#[cfg(unix)]
fn run_shell_command(cmd: &str) -> Option<String> {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

#[cfg(windows)]
fn run_shell_command(cmd: &str) -> Option<String> {
    std::process::Command::new("cmd")
        .arg("/C")
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Test reachability of a Pi provider's endpoint by issuing GET {baseUrl}/models
/// from the backend (no browser CORS). Any HTTP response means the server is
/// reachable; only network errors (timeout, DNS, connection refused) mean not.
#[tauri::command]
pub async fn test_pi_connectivity(
    #[allow(non_snake_case)] providerId: String,
) -> Result<PiConnectivityResult, String> {
    let loaded = pi_config::read_models_json().map_err(|e| e.to_string())?;
    let provider = loaded
        .value
        .get("providers")
        .and_then(|v| v.as_object())
        .and_then(|p| p.get(&providerId))
        .ok_or_else(|| format!("Pi provider \"{providerId}\" not found"))?;

    let base_url = provider
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if base_url.trim().is_empty() {
        return Ok(PiConnectivityResult {
            reachable: false,
            status_code: None,
            error_kind: Some("noBaseUrl".to_string()),
            detail: None,
        });
    }

    let normalized = base_url.trim().trim_end_matches('/').to_string();
    let api_key_raw = provider
        .get("apiKey")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let resolved_key = resolve_api_key(api_key_raw);

    let client = crate::proxy::http_client::get();
    let timeout = Duration::from_secs(10);
    let mut request = client.get(format!("{normalized}/models")).timeout(timeout);
    if let Some(key) = &resolved_key {
        request = request.header("Authorization", format!("Bearer {key}"));
    }

    match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            Ok(PiConnectivityResult {
                reachable: true,
                status_code: Some(status),
                error_kind: None,
                detail: Some(format!("{normalized}/models -> HTTP {status}")),
            })
        }
        Err(err) => {
            let kind = if err.is_timeout() {
                "timeout"
            } else {
                "network"
            };
            Ok(PiConnectivityResult {
                reachable: false,
                status_code: None,
                error_kind: Some(kind.to_string()),
                detail: Some(err.to_string()),
            })
        }
    }
}
