use std::sync::{Arc, Mutex};

use super::capabilities::{CommandCapabilityRegistry, RemoteCapabilityError};
use super::client::RemoteClientError;
use super::models::{RemoteConnectionStatus, RemoteRuntimeSnapshot, RemoteTargetConfig};
use super::ssh::{preflight, OpenSshSession, RemotePlatform, RemoteSshError};
use super::target_store::{RemoteTargetStore, TargetStoreError};

trait RuntimeCommandSession: Send + Sync {
    fn invoke(
        &self,
        request_id: &str,
        command: &str,
        args: serde_json::Value,
        timeout_ms: u64,
    ) -> Result<serde_json::Value, RemoteSshError>;
}

impl RuntimeCommandSession for OpenSshSession {
    fn invoke(
        &self,
        request_id: &str,
        command: &str,
        args: serde_json::Value,
        timeout_ms: u64,
    ) -> Result<serde_json::Value, RemoteSshError> {
        OpenSshSession::invoke(self, request_id, command, args, timeout_ms)
    }
}

type SharedRuntimeSession = Arc<dyn RuntimeCommandSession>;

pub struct RemoteRuntimeState {
    store: RemoteTargetStore,
    snapshot: Mutex<RemoteRuntimeSnapshot>,
    session: Mutex<Option<SharedRuntimeSession>>,
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
                *self.lock_session()? = Some(Arc::new(session));
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
        expected_generation: u64,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, RemoteRuntimeError> {
        self.require_generation(expected_generation)?;
        let registry = CommandCapabilityRegistry::remote_supported();
        let capability = registry.require(command)?;
        // 克隆 Arc 后立即释放 runtime session 锁；目标切换可并发丢弃旧 session，
        // 正在执行的请求仍持有旧 Arc，返回时由第二次 generation 检查拒绝迟到结果。
        let session = self
            .lock_session()?
            .clone()
            .ok_or(RemoteRuntimeError::Offline)?;
        let result = session.invoke(
            &uuid::Uuid::new_v4().to_string(),
            command,
            args,
            capability.timeout_ms,
        );
        self.require_generation(expected_generation)?;
        result.map_err(RemoteRuntimeError::Ssh)
    }

    fn require_generation(&self, expected: u64) -> Result<(), RemoteRuntimeError> {
        let snapshot = self.lock_snapshot()?;
        if snapshot.generation != expected {
            return Err(RemoteRuntimeError::StaleRuntime {
                expected,
                actual: snapshot.generation,
            });
        }
        if snapshot.status != RemoteConnectionStatus::Online {
            return Err(RemoteRuntimeError::Offline);
        }
        Ok(())
    }

    #[cfg(test)]
    fn install_test_session(&self, generation: u64, session: Box<dyn RuntimeCommandSession>) {
        *self.session.lock().expect("锁定测试 session") = Some(Arc::from(session));
        *self.snapshot.lock().expect("锁定测试 snapshot") = RemoteRuntimeSnapshot {
            status: RemoteConnectionStatus::Online,
            generation,
            active_target_id: Some("test-target".to_string()),
            error_code: None,
            error_message: None,
        };
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
    ) -> Result<std::sync::MutexGuard<'_, Option<SharedRuntimeSession>>, RemoteRuntimeError> {
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
    #[error("远程运行时已切换: expected={expected}, actual={actual}")]
    StaleRuntime { expected: u64, actual: u64 },
    #[error(transparent)]
    Capability(#[from] RemoteCapabilityError),
    #[error(transparent)]
    Ssh(#[from] RemoteSshError),
    #[error(transparent)]
    Client(#[from] RemoteClientError),
}

impl RemoteRuntimeError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(_) => "REMOTE_TARGET_STORE_ERROR",
            Self::StatePoisoned(_) => "REMOTE_STATE_ERROR",
            Self::TargetNotFound(_) => "REMOTE_TARGET_NOT_FOUND",
            Self::Offline => "REMOTE_OFFLINE",
            Self::StaleRuntime { .. } => "STALE_RUNTIME",
            Self::Capability(_) => "COMMAND_NOT_EXPOSED",
            Self::Ssh(RemoteSshError::Validation(_)) => "REMOTE_TARGET_INVALID",
            Self::Ssh(error) => ssh_error_code(error),
            Self::Client(error) => error.code(),
        }
    }
}

fn ssh_error_code(error: &RemoteSshError) -> &str {
    error.code()
}

#[cfg(test)]
mod generation_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    struct BlockingSession {
        calls: Arc<AtomicUsize>,
        started: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl RuntimeCommandSession for BlockingSession {
        fn invoke(
            &self,
            _request_id: &str,
            _command: &str,
            _args: serde_json::Value,
            _timeout_ms: u64,
        ) -> Result<serde_json::Value, RemoteSshError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _ = self.started.send(());
            self.release
                .lock()
                .expect("锁定 fake release")
                .recv_timeout(Duration::from_secs(2))
                .expect("等待释放 fake 响应");
            Ok(json!({ "source": "old-generation" }))
        }
    }

    fn connected_runtime(session: BlockingSession) -> (Arc<RemoteRuntimeState>, Arc<AtomicUsize>) {
        let temp = tempfile::tempdir().expect("创建 runtime fixture");
        let store = RemoteTargetStore::at(temp.keep().join("remote-targets.json"));
        let runtime = Arc::new(RemoteRuntimeState::new(store).expect("创建 runtime"));
        let calls = Arc::clone(&session.calls);
        runtime.install_test_session(7, Box::new(session));
        (runtime, calls)
    }

    #[test]
    fn stale_generation_is_rejected_before_reaching_session() {
        let (started_sender, _started_receiver) = mpsc::channel();
        let (_release_sender, release_receiver) = mpsc::channel();
        let (runtime, calls) = connected_runtime(BlockingSession {
            calls: Arc::new(AtomicUsize::new(0)),
            started: started_sender,
            release: Mutex::new(release_receiver),
        });

        let error = runtime
            .invoke_remote(6, "usage.summary", json!({}))
            .expect_err("旧 generation 必须在发送前被拒绝");
        assert_eq!(error.code(), "STALE_RUNTIME");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn response_is_rejected_when_generation_changes_during_request() {
        let (started_sender, started_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let (runtime, _calls) = connected_runtime(BlockingSession {
            calls: Arc::new(AtomicUsize::new(0)),
            started: started_sender,
            release: Mutex::new(release_receiver),
        });
        let invoking = Arc::clone(&runtime);
        let request =
            std::thread::spawn(move || invoking.invoke_remote(7, "usage.summary", json!({})));
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("fake session 收到请求");
        let snapshot = runtime.use_local().expect("请求期间切回本地");
        assert_eq!(snapshot.generation, 8);
        release_sender.send(()).expect("释放旧响应");

        let error = request
            .join()
            .expect("等待旧请求")
            .expect_err("迟到响应必须被拒绝");
        assert_eq!(error.code(), "STALE_RUNTIME");
    }
}
