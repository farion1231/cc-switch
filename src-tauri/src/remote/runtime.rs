use std::sync::Mutex;

use super::capabilities::{CommandCapabilityRegistry, RemoteCapabilityError};
use super::client::RemoteClientError;
use super::models::{RemoteConnectionStatus, RemoteRuntimeSnapshot, RemoteTargetConfig};
use super::ssh::{preflight, OpenSshSession, RemotePlatform, RemoteSshError};
use super::target_store::{RemoteTargetStore, TargetStoreError};

pub struct RemoteRuntimeState {
    store: RemoteTargetStore,
    snapshot: Mutex<RemoteRuntimeSnapshot>,
    session: Mutex<Option<OpenSshSession>>,
}

impl RemoteRuntimeState {
    pub fn new(store: RemoteTargetStore) -> Result<Self, RemoteRuntimeError> {
        let document = store.load()?;
        let snapshot = match document.active_target_id {
            Some(target_id) => RemoteRuntimeSnapshot {
                status: RemoteConnectionStatus::Offline,
                generation: 0,
                active_target_id: Some(target_id),
                error_code: Some("NOT_CONNECTED".to_string()),
                error_message: Some("远程目标尚未连接".to_string()),
            },
            None => RemoteRuntimeSnapshot::local(0),
        };
        Ok(Self {
            store,
            snapshot: Mutex::new(snapshot),
            session: Mutex::new(None),
        })
    }

    pub fn default_store() -> Result<Self, RemoteRuntimeError> {
        Self::new(RemoteTargetStore::default_path())
    }

    pub fn snapshot(&self) -> Result<RemoteRuntimeSnapshot, RemoteRuntimeError> {
        Ok(self.lock_snapshot()?.clone())
    }

    pub fn list_targets(&self) -> Result<Vec<RemoteTargetConfig>, RemoteRuntimeError> {
        Ok(self.store.load()?.targets)
    }

    pub fn upsert_target(&self, target: RemoteTargetConfig) -> Result<(), RemoteRuntimeError> {
        self.store.upsert(target)?;
        Ok(())
    }

    /// 仅执行平台与认证预检，不上传 Agent，也不替换当前会话。
    /// 设置页可据此验证尚未保存的连接参数，避免“测试”意外改变用户当前环境。
    pub fn test_target(
        &self,
        target: &RemoteTargetConfig,
    ) -> Result<RemotePlatform, RemoteRuntimeError> {
        preflight(target).map_err(RemoteRuntimeError::Ssh)
    }

    pub fn delete_target(&self, target_id: &str) -> Result<bool, RemoteRuntimeError> {
        let deleted = self.store.delete(target_id)?;
        if deleted {
            let generation = {
                let snapshot = self.lock_snapshot()?;
                (snapshot.active_target_id.as_deref() == Some(target_id))
                    .then_some(snapshot.generation + 1)
            };
            if let Some(generation) = generation {
                // 删除活动目标等价于切回本机，必须同步终止旧 SSH 子进程，不能只改 UI 快照。
                *self.lock_session()? = None;
                *self.lock_snapshot()? = RemoteRuntimeSnapshot::local(generation);
            }
        }
        Ok(deleted)
    }

    pub fn connect_target(
        &self,
        target_id: &str,
    ) -> Result<RemoteRuntimeSnapshot, RemoteRuntimeError> {
        let target = self
            .list_targets()?
            .into_iter()
            .find(|target| target.id == target_id)
            .ok_or_else(|| RemoteRuntimeError::TargetNotFound(target_id.to_string()))?;
        let generation = {
            let mut snapshot = self.lock_snapshot()?;
            let generation = snapshot.generation + 1;
            *snapshot = RemoteRuntimeSnapshot {
                status: RemoteConnectionStatus::Connecting,
                generation,
                active_target_id: Some(target_id.to_string()),
                error_code: None,
                error_message: None,
            };
            generation
        };
        // 新目标连接前先销毁旧会话，确保失败时不会保留一个与快照不一致的远端进程。
        *self.lock_session()? = None;
        self.store.set_active_target(Some(target_id.to_string()))?;

        match OpenSshSession::connect(&target) {
            Ok(session) => {
                *self.lock_session()? = Some(session);
                let mut snapshot = self.lock_snapshot()?;
                *snapshot = RemoteRuntimeSnapshot {
                    status: RemoteConnectionStatus::Online,
                    generation,
                    active_target_id: Some(target_id.to_string()),
                    error_code: None,
                    error_message: None,
                };
                Ok(snapshot.clone())
            }
            Err(error) => {
                let mut snapshot = self.lock_snapshot()?;
                *snapshot = RemoteRuntimeSnapshot {
                    status: RemoteConnectionStatus::Offline,
                    generation,
                    active_target_id: Some(target_id.to_string()),
                    error_code: Some(ssh_error_code(&error).to_string()),
                    error_message: Some(error.to_string()),
                };
                Err(RemoteRuntimeError::Ssh(error))
            }
        }
    }

    pub fn use_local(&self) -> Result<RemoteRuntimeSnapshot, RemoteRuntimeError> {
        *self.lock_session()? = None;
        self.store.set_active_target(None)?;
        let mut snapshot = self.lock_snapshot()?;
        *snapshot = RemoteRuntimeSnapshot::local(snapshot.generation + 1);
        Ok(snapshot.clone())
    }

    pub fn invoke_remote(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, RemoteRuntimeError> {
        let registry = CommandCapabilityRegistry::provider_phase();
        let capability = registry.require(command)?;
        let mut session = self.lock_session()?;
        let session = session.as_mut().ok_or(RemoteRuntimeError::Offline)?;
        session
            .invoke(
                &uuid::Uuid::new_v4().to_string(),
                command,
                args,
                capability.timeout_ms,
            )
            .map_err(RemoteRuntimeError::Ssh)
    }

    fn lock_snapshot(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, RemoteRuntimeSnapshot>, RemoteRuntimeError> {
        self.snapshot
            .lock()
            .map_err(|error| RemoteRuntimeError::StatePoisoned(error.to_string()))
    }

    fn lock_session(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<OpenSshSession>>, RemoteRuntimeError> {
        self.session
            .lock()
            .map_err(|error| RemoteRuntimeError::StatePoisoned(error.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteRuntimeError {
    #[error(transparent)]
    Store(#[from] TargetStoreError),
    #[error("远程运行时状态锁损坏: {0}")]
    StatePoisoned(String),
    #[error("远程目标不存在: {0}")]
    TargetNotFound(String),
    #[error("远程连接未建立")]
    Offline,
    #[error(transparent)]
    Capability(#[from] RemoteCapabilityError),
    #[error(transparent)]
    Ssh(#[from] RemoteSshError),
    #[error(transparent)]
    Client(#[from] RemoteClientError),
}

impl RemoteRuntimeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Store(_) => "REMOTE_TARGET_STORE_ERROR",
            Self::StatePoisoned(_) => "REMOTE_STATE_ERROR",
            Self::TargetNotFound(_) => "REMOTE_TARGET_NOT_FOUND",
            Self::Offline => "REMOTE_OFFLINE",
            Self::Capability(_) => "COMMAND_NOT_EXPOSED",
            Self::Ssh(RemoteSshError::Validation(_)) => "REMOTE_TARGET_INVALID",
            Self::Ssh(error) => ssh_error_code(error),
            Self::Client(_) => "REMOTE_PROTOCOL_ERROR",
        }
    }
}

fn ssh_error_code(error: &RemoteSshError) -> &'static str {
    error.code()
}
