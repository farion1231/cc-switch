//! Per-app switch lock
//!
//! 确保同一应用同时只有一个供应商切换操作在执行，
//! 防止并发切换导致 is_current 与 Live 备份不一致。

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

static PROCESS_SWITCH_LOCKS: OnceLock<SwitchLockManager> = OnceLock::new();

/// 每个应用类型一把互斥锁，保证同一应用的切换操作串行执行。
///
/// 不同应用之间（如 Claude 和 Codex）可以并行切换。
#[derive(Clone, Default)]
pub struct SwitchLockManager {
    locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
}

impl SwitchLockManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the process-wide lock manager used by every [`ProxyService`].
    ///
    /// Background import and synchronization paths construct short-lived app
    /// state values. Sharing this manager ensures those paths cannot bypass a
    /// provider-switch confirmation that is using the primary app state.
    pub fn process_wide() -> Self {
        PROCESS_SWITCH_LOCKS.get_or_init(Self::new).clone()
    }

    /// 获取指定应用的切换锁。
    ///
    /// 返回 `OwnedMutexGuard`，持有期间同一 `app_type` 的其他切换会排队等待。
    pub async fn lock_for_app(&self, app_type: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let locks = self.locks.read().await;
            if let Some(lock) = locks.get(app_type) {
                lock.clone()
            } else {
                drop(locks);
                let mut locks = self.locks.write().await;
                locks
                    .entry(app_type.to_string())
                    .or_insert_with(|| Arc::new(Mutex::new(())))
                    .clone()
            }
        };
        lock.lock_owned().await
    }
}
