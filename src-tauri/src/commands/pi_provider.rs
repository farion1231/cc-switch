use crate::pi_config;
use crate::provider_runtime::ProviderRuntimeApp;
use crate::services::pi_provider::{PiProviderDraft, PiProviderPatchPreview};
use crate::services::{ProviderRuntimeProviders, ProviderRuntimeService};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiModelsMeta {
    pub file_hash: String,
}

#[tauri::command]
pub fn list_pi_providers() -> Result<Value, String> {
    match ProviderRuntimeService::list(None, ProviderRuntimeApp::Pi).map_err(|e| e.to_string())? {
        ProviderRuntimeProviders::Pi(providers) => Ok(Value::Object(providers)),
        ProviderRuntimeProviders::Db(_) => Err("Pi provider runtime returned DB providers".into()),
    }
}

/// Read-only metadata for Pi `models.json`. Returns the current file hash so the
/// frontend can optimistically lock a delete without running the upsert
/// validation path (which would reject the empty draft a delete uses).
#[tauri::command]
pub fn read_pi_models_meta() -> Result<PiModelsMeta, String> {
    Ok(PiModelsMeta {
        file_hash: ProviderRuntimeService::read_pi_models_meta().map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
pub fn preview_pi_provider_patch(draft: PiProviderDraft) -> Result<PiProviderPatchPreview, String> {
    ProviderRuntimeService::preview_pi_provider_patch(&draft).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn apply_pi_provider_patch(
    draft: PiProviderDraft,
    #[allow(non_snake_case)] expectedFileHash: String,
) -> Result<PiProviderApplyResult, String> {
    let result = ProviderRuntimeService::apply_pi_provider_patch(&draft, &expectedFileHash)
        .map_err(|e| e.to_string())?;
    Ok(PiProviderApplyResult {
        file_hash: result.file_hash,
        models_json: result.models_json,
        backup_path: result.backup_path,
    })
}

#[tauri::command]
pub fn delete_pi_provider(
    #[allow(non_snake_case)] providerId: String,
    #[allow(non_snake_case)] expectedFileHash: String,
) -> Result<PiProviderApplyResult, String> {
    let result = ProviderRuntimeService::delete_pi_provider(&providerId, &expectedFileHash)
        .map_err(|e| e.to_string())?;
    Ok(PiProviderApplyResult {
        file_hash: result.file_hash,
        models_json: result.models_json,
        backup_path: result.backup_path,
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

fn validate_connectivity_url(
    base_url: &str,
    has_api_credentials: bool,
) -> Result<url::Url, &'static str> {
    let parsed = url::Url::parse(base_url.trim()).map_err(|_| "invalidBaseUrl")?;
    if !matches!(parsed.scheme(), "http" | "https") || !parsed.has_host() {
        return Err("invalidBaseUrl");
    }
    let has_url_credentials = !parsed.username().is_empty() || parsed.password().is_some();
    if (has_api_credentials || has_url_credentials) && parsed.scheme() != "https" {
        return Err("insecureTransport");
    }
    Ok(parsed)
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

    let api_key_raw = provider
        .get("apiKey")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let resolved_key = resolve_api_key(api_key_raw);
    let parsed_base_url = match validate_connectivity_url(base_url, resolved_key.is_some()) {
        Ok(url) => url,
        Err(kind) => {
            return Ok(PiConnectivityResult {
                reachable: false,
                status_code: None,
                error_kind: Some(kind.to_string()),
                detail: None,
            });
        }
    };
    let normalized = parsed_base_url.as_str().trim_end_matches('/').to_string();

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

#[cfg(test)]
mod tests {
    use super::validate_connectivity_url;

    #[test]
    fn credentialed_connectivity_rejects_cleartext_http() {
        let err = validate_connectivity_url("http://api.example.com/v1", true)
            .expect_err("credentialed HTTP must be rejected");
        assert_eq!(err, "insecureTransport");
    }

    #[test]
    fn connectivity_rejects_cleartext_url_credentials() {
        let err = validate_connectivity_url("http://user:secret@api.example.com/v1", false)
            .expect_err("URL credentials over HTTP must be rejected");
        assert_eq!(err, "insecureTransport");
    }

    #[test]
    fn connectivity_accepts_https_with_credentials_and_http_without_them() {
        assert!(validate_connectivity_url("https://api.example.com/v1", true).is_ok());
        assert!(validate_connectivity_url("http://127.0.0.1:11434/v1", false).is_ok());
    }

    #[test]
    fn connectivity_rejects_non_http_schemes() {
        let err = validate_connectivity_url("file:///tmp/models", false)
            .expect_err("non-HTTP URL must be rejected");
        assert_eq!(err, "invalidBaseUrl");
    }
}
