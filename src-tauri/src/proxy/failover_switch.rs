//! 故障转移切换模块
//!
//! 处理故障转移成功后的供应商切换逻辑，包括：
//! - 去重控制（避免多个请求同时触发）
//! - 托盘菜单更新
//! - 前端事件发射

use crate::{error::AppError, store::AppState};
use std::collections::HashSet;
use std::sync::Arc;
use tauri::{Emitter, Manager};
#[cfg(test)]
use tokio::sync::mpsc;
use tokio::sync::RwLock;

/// 故障转移切换管理器
///
/// 负责处理故障转移成功后的供应商切换，确保 UI 能够直观反映当前使用的供应商。
#[derive(Clone, Default)]
pub struct FailoverSwitchManager {
    /// 正在处理中的切换（同一应用、供应商和请求路由版本）
    pending_switches: Arc<RwLock<HashSet<String>>>,
    #[cfg(test)]
    test_switch: Option<TestSwitch>,
}

#[cfg(test)]
#[derive(Clone)]
struct TestSwitch {
    state: AppState,
    completed: mpsc::UnboundedSender<Result<bool, String>>,
}

impl FailoverSwitchManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Supply the application state without a desktop runtime and observe the real writeback.
    #[cfg(test)]
    pub(crate) fn for_test(
        state: AppState,
    ) -> (Self, mpsc::UnboundedReceiver<Result<bool, String>>) {
        let (completed, receiver) = mpsc::unbounded_channel();
        let mut manager = Self::new();
        manager.test_switch = Some(TestSwitch { state, completed });
        (manager, receiver)
    }

    /// 尝试执行故障转移切换
    ///
    /// 如果相同的切换已在进行中，则跳过；否则执行切换逻辑。
    ///
    /// # Returns
    /// - `Ok(true)` - 切换成功执行
    /// - `Ok(false)` - 切换已在进行中，跳过
    /// - `Err(e)` - 切换过程中发生错误
    pub async fn try_switch(
        &self,
        app_handle: Option<&tauri::AppHandle>,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
        route_revision: u64,
    ) -> Result<bool, AppError> {
        let switch_key = format!("{app_type}:{provider_id}:{route_revision}");

        // 去重检查：如果相同切换已在进行中，跳过
        {
            let mut pending = self.pending_switches.write().await;
            if pending.contains(&switch_key) {
                log::debug!("[Failover] 切换已在进行中，跳过: {app_type} -> {provider_id}");
                return Ok(false);
            }
            pending.insert(switch_key.clone());
        }

        // 执行切换（确保最后清理 pending 标记）
        let result = self
            .do_switch(
                app_handle,
                app_type,
                provider_id,
                provider_name,
                route_revision,
            )
            .await;

        // 清理 pending 标记
        {
            let mut pending = self.pending_switches.write().await;
            pending.remove(&switch_key);
        }

        result
    }

    async fn do_switch(
        &self,
        app_handle: Option<&tauri::AppHandle>,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
        route_revision: u64,
    ) -> Result<bool, AppError> {
        // 只有处于代理模式的应用才允许执行故障转移切换
        let Ok(app_enum) = app_type.parse::<crate::app_config::AppType>() else {
            return Ok(false);
        };
        if !crate::mode::current::is_proxy(&app_enum) {
            log::debug!("[Failover] {app_type} 不在路由模式，跳过切换");
            return Ok(false);
        }

        let app_state = app_handle.and_then(|app| app.try_state::<AppState>());
        let state = app_state.as_ref().map(|state| state.inner());
        #[cfg(test)]
        let state = state.or_else(|| self.test_switch.as_ref().map(|test| &test.state));

        let mut switched = false;
        if let Some(state) = state {
            // 只换代理的路由，不写客户端文件。
            let result = crate::mode::controller::record_failover_route(
                state,
                &app_enum,
                provider_id,
                route_revision,
            )
            .await;
            #[cfg(test)]
            if let Some(test) = &self.test_switch {
                let _ = test.completed.send(result.clone());
            }
            switched = result.map_err(AppError::Message)?;
            if !switched {
                return Ok(false);
            }
            log::info!("[FO-001] 切换: {app_type} → {provider_name}");

            // Linux 重建原生菜单，其它平台通知托盘面板（不挂原生菜单）
            if let Some(app) = app_handle {
                crate::tray::refresh_tray_menu(app);
            }
        }

        if let Some(app) = app_handle {
            // 发射事件到前端
            let event_data = serde_json::json!({
                "appType": app_type,
                "providerId": provider_id,
                "source": "failover"  // 标识来源是故障转移
            });
            if let Err(e) = app.emit("provider-switched", event_data) {
                log::error!("[Failover] 发射事件失败: {e}");
            }
        }

        Ok(switched)
    }
}
