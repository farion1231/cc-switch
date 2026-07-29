use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTargetConfig {
    pub id: String,
    pub name: String,
    pub host_alias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteConnectionStatus {
    Local,
    Connecting,
    Online,
    Reconnecting,
    Offline,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRuntimeSnapshot {
    pub status: RemoteConnectionStatus,
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl RemoteRuntimeSnapshot {
    pub fn local(generation: u64) -> Self {
        Self {
            status: RemoteConnectionStatus::Local,
            generation,
            active_target_id: None,
            error_code: None,
            error_message: None,
        }
    }
}

impl RemoteTargetConfig {
    pub fn normalize(mut self) -> Result<Self, RemoteTargetValidationError> {
        self.id = normalize_token("id", &self.id)?;
        self.host_alias = normalize_token("hostAlias", &self.host_alias)?;
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            return Err(RemoteTargetValidationError::EmptyField("name"));
        }
        if self.name.chars().any(char::is_control) {
            return Err(RemoteTargetValidationError::InvalidField("name"));
        }
        self.username = normalize_optional_token("username", self.username)?;
        self.identity_file = normalize_optional_path(self.identity_file)?;
        Ok(self)
    }
}

fn normalize_token(
    field: &'static str,
    value: &str,
) -> Result<String, RemoteTargetValidationError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(RemoteTargetValidationError::EmptyField(field));
    }
    if value.starts_with('-')
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(RemoteTargetValidationError::InvalidField(field));
    }
    Ok(value.to_string())
}

fn normalize_optional_token(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, RemoteTargetValidationError> {
    value
        .map(|value| normalize_token(field, &value))
        .transpose()
}

fn normalize_optional_path(
    value: Option<String>,
) -> Result<Option<String>, RemoteTargetValidationError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().any(char::is_control) {
        return Err(RemoteTargetValidationError::InvalidField("identityFile"));
    }
    Ok(Some(value.to_string()))
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RemoteTargetValidationError {
    #[error("远程目标字段不能为空: {0}")]
    EmptyField(&'static str),
    #[error("远程目标字段无效: {0}")]
    InvalidField(&'static str),
}
