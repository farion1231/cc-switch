use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::models::{RemoteTargetConfig, RemoteTargetValidationError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTargetDocument {
    #[serde(default = "document_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_target_id: Option<String>,
    #[serde(default)]
    pub targets: Vec<RemoteTargetConfig>,
}

impl Default for RemoteTargetDocument {
    fn default() -> Self {
        Self {
            version: document_version(),
            active_target_id: None,
            targets: Vec::new(),
        }
    }
}

const fn document_version() -> u32 {
    1
}

#[derive(Debug, Clone)]
pub struct RemoteTargetStore {
    path: PathBuf,
}

impl RemoteTargetStore {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Self {
        Self::at(
            crate::config::get_home_dir()
                .join(".cc-switch")
                .join("remote-targets.json"),
        )
    }

    pub fn load(&self) -> Result<RemoteTargetDocument, TargetStoreError> {
        if !self.path.exists() {
            return Ok(RemoteTargetDocument::default());
        }
        let bytes = std::fs::read(&self.path)?;
        serde_json::from_slice(&bytes).map_err(TargetStoreError::InvalidData)
    }

    pub fn upsert(&self, target: RemoteTargetConfig) -> Result<(), TargetStoreError> {
        let target = target.normalize()?;
        let mut document = self.load()?;
        if let Some(existing) = document
            .targets
            .iter_mut()
            .find(|item| item.id == target.id)
        {
            *existing = target;
        } else {
            document.targets.push(target);
        }
        self.save(&document)
    }

    pub fn set_active_target(&self, target_id: Option<String>) -> Result<(), TargetStoreError> {
        let mut document = self.load()?;
        if let Some(target_id) = target_id {
            if !document.targets.iter().any(|item| item.id == target_id) {
                return Err(TargetStoreError::TargetNotFound(target_id));
            }
            document.active_target_id = Some(target_id);
        } else {
            document.active_target_id = None;
        }
        self.save(&document)
    }

    pub fn delete(&self, target_id: &str) -> Result<bool, TargetStoreError> {
        let mut document = self.load()?;
        let original_len = document.targets.len();
        document.targets.retain(|target| target.id != target_id);
        if document.targets.len() == original_len {
            return Ok(false);
        }
        if document.active_target_id.as_deref() == Some(target_id) {
            document.active_target_id = None;
        }
        self.save(&document)?;
        Ok(true)
    }

    /// 临时文件与目标文件位于同一目录，确保 rename 不跨文件系统；写入失败时保留旧文件。
    fn save(&self, document: &RemoteTargetDocument) -> Result<(), TargetStoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp_path = temp_path_for(&self.path);
        let bytes = serde_json::to_vec_pretty(document)?;
        std::fs::write(&temp_path, bytes)?;
        if let Err(error) = std::fs::rename(&temp_path, &self.path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(TargetStoreError::Io(error));
        }
        Ok(())
    }
}

fn temp_path_for(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| format!("{value}."))
            .unwrap_or_default()
    ))
}

#[derive(Debug, thiserror::Error)]
pub enum TargetStoreError {
    #[error("远程目标文件读写失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("远程目标文件 JSON 无效: {0}")]
    InvalidData(serde_json::Error),
    #[error("远程目标序列化失败: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("远程目标不存在: {0}")]
    TargetNotFound(String),
    #[error(transparent)]
    Validation(#[from] RemoteTargetValidationError),
}
