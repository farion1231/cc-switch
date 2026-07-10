use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiModelsJson {
    pub value: Value,
    #[serde(rename = "fileHash")]
    pub file_hash: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct PiConfigError {
    message: String,
}

impl PiConfigError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PiConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PiConfigError {}

pub fn get_pi_agent_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pi")
        .join("agent")
}

pub fn get_pi_models_json_path() -> PathBuf {
    get_pi_agent_dir().join("models.json")
}

pub fn read_models_json() -> Result<PiModelsJson, PiConfigError> {
    read_models_json_at(&get_pi_models_json_path())
}

pub fn read_models_json_at(path: &Path) -> Result<PiModelsJson, PiConfigError> {
    if !path.exists() {
        return Ok(PiModelsJson {
            value: json!({ "providers": {} }),
            file_hash: String::new(),
            path: path.to_path_buf(),
        });
    }

    let raw = fs::read_to_string(path).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to read Pi models.json at {}: {e}",
            path.display()
        ))
    })?;
    let value: Value = serde_json::from_str(&raw).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to parse Pi models.json at {}: {e}",
            path.display()
        ))
    })?;

    Ok(PiModelsJson {
        value,
        file_hash: sha256_hex(raw.as_bytes()),
        path: path.to_path_buf(),
    })
}

pub fn write_models_json_at(path: &Path, value: &Value) -> Result<String, PiConfigError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| PiConfigError::new(format!("Failed to serialize Pi models.json: {e}")))?;
    let temp = prepare_private_temp_file(path, &bytes)?;
    persist_temp_file(temp, path)?;

    Ok(sha256_hex(&bytes))
}

fn prepare_private_temp_file(path: &Path, bytes: &[u8]) -> Result<NamedTempFile, PiConfigError> {
    let parent = path
        .parent()
        .ok_or_else(|| PiConfigError::new("Pi models.json path has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to create Pi config dir {}: {e}",
            parent.display()
        ))
    })?;

    let mut temp = NamedTempFile::new_in(parent).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to create private temp Pi models.json in {}: {e}",
            parent.display()
        ))
    })?;
    temp.write_all(bytes).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to write temp Pi models.json {}: {e}",
            temp.path().display()
        ))
    })?;
    temp.as_file().sync_all().map_err(|e| {
        PiConfigError::new(format!(
            "Failed to sync temp Pi models.json {}: {e}",
            temp.path().display()
        ))
    })?;
    Ok(temp)
}

fn persist_temp_file(temp: NamedTempFile, path: &Path) -> Result<(), PiConfigError> {
    temp.persist(path).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to replace Pi models.json {} with {}: {}",
            path.display(),
            e.file.path().display(),
            e.error
        ))
    })?;
    set_private_file_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), PiConfigError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to secure Pi config file {}: {e}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), PiConfigError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), PiConfigError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to secure Pi backup directory {}: {e}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), PiConfigError> {
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiModelsBackup {
    pub id: String,
    pub path: PathBuf,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

pub fn get_pi_models_backup_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cc-switch")
        .join("backups")
        .join("pi-models")
}

pub fn create_backup(path: &Path) -> Result<PiModelsBackup, PiConfigError> {
    create_backup_at(path, &get_pi_models_backup_dir())
}

pub fn create_backup_at(path: &Path, backup_dir: &Path) -> Result<PiModelsBackup, PiConfigError> {
    fs::create_dir_all(backup_dir).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to create Pi backup dir {}: {e}",
            backup_dir.display()
        ))
    })?;
    set_private_dir_permissions(backup_dir)?;
    let created_at = chrono::Utc::now().timestamp_millis();
    let id = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    let backup_path = backup_dir.join(format!("{id}-models.json"));

    if path.exists() {
        fs::copy(path, &backup_path).map_err(|e| {
            PiConfigError::new(format!(
                "Failed to backup Pi models.json from {} to {}: {e}",
                path.display(),
                backup_path.display()
            ))
        })?;
    } else {
        fs::write(&backup_path, b"{\n  \"providers\": {}\n}\n").map_err(|e| {
            PiConfigError::new(format!(
                "Failed to create empty Pi backup {}: {e}",
                backup_path.display()
            ))
        })?;
    }
    set_private_file_permissions(&backup_path)?;

    Ok(PiModelsBackup {
        id,
        path: backup_path,
        created_at,
    })
}

pub fn rollback_backup_at(models_path: &Path, backup_path: &Path) -> Result<String, PiConfigError> {
    let raw = fs::read_to_string(backup_path).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to read Pi backup {}: {e}",
            backup_path.display()
        ))
    })?;
    let value: Value = serde_json::from_str(&raw).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to parse Pi backup {}: {e}",
            backup_path.display()
        ))
    })?;
    write_models_json_at(models_path, &value)
}

pub fn write_models_json_with_expected_hash_at(
    path: &Path,
    value: &Value,
    expected_hash: &str,
) -> Result<String, PiConfigError> {
    write_models_json_with_expected_hash_at_impl(path, value, expected_hash, || {})
}

fn write_models_json_with_expected_hash_at_impl<F>(
    path: &Path,
    value: &Value,
    expected_hash: &str,
    before_final_check: F,
) -> Result<String, PiConfigError>
where
    F: FnOnce(),
{
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| PiConfigError::new(format!("Failed to serialize Pi models.json: {e}")))?;
    let temp = prepare_private_temp_file(path, &bytes)?;
    before_final_check();
    let current = read_models_json_at(path)?;
    if current.file_hash != expected_hash {
        return Err(PiConfigError::new(format!(
            "Pi models.json changed on disk; expected hash {expected_hash}, found {}",
            current.file_hash
        )));
    }
    persist_temp_file(temp, path)?;
    Ok(sha256_hex(&bytes))
}

#[cfg(test)]
pub fn write_models_json_with_expected_hash_at_test_hook<F>(
    path: &Path,
    value: &Value,
    expected_hash: &str,
    before_final_check: F,
) -> Result<String, PiConfigError>
where
    F: FnOnce(),
{
    write_models_json_with_expected_hash_at_impl(path, value, expected_hash, before_final_check)
}
