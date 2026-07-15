use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            PiConfigError::new(format!(
                "Failed to create Pi config dir {}: {e}",
                parent.display()
            ))
        })?;
    }

    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| PiConfigError::new(format!("Failed to serialize Pi models.json: {e}")))?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&tmp).map_err(|e| {
            PiConfigError::new(format!(
                "Failed to create temp Pi models.json {}: {e}",
                tmp.display()
            ))
        })?;
        file.write_all(&bytes).map_err(|e| {
            PiConfigError::new(format!(
                "Failed to write temp Pi models.json {}: {e}",
                tmp.display()
            ))
        })?;
        file.sync_all().map_err(|e| {
            PiConfigError::new(format!(
                "Failed to sync temp Pi models.json {}: {e}",
                tmp.display()
            ))
        })?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        PiConfigError::new(format!(
            "Failed to replace Pi models.json {} with {}: {e}",
            path.display(),
            tmp.display()
        ))
    })?;

    Ok(sha256_hex(&bytes))
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
    let current = read_models_json_at(path)?;
    if current.file_hash != expected_hash {
        return Err(PiConfigError::new(format!(
            "Pi models.json changed on disk; expected hash {expected_hash}, found {}",
            current.file_hash
        )));
    }
    write_models_json_at(path, value)
}
