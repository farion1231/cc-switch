//! 故障转移切换模块
//!
//! 处理故障转移成功后的供应商切换逻辑，包括：
//! - 去重控制（避免多个请求同时触发）
//! - 数据库更新
//! - 托盘菜单更新
//! - 前端事件发射
//! - Live 备份更新

use crate::database::Database;
use crate::error::AppError;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
#[cfg(feature = "tauri-app")]
use tauri::{Emitter, Manager};
use tokio::sync::RwLock;

/// 故障转移切换管理器
///
/// 负责处理故障转移成功后的供应商切换，确保 UI 能够直观反映当前使用的供应商。
#[derive(Clone)]
pub struct FailoverSwitchManager {
    /// 正在处理中的切换（key = "app_type:provider_id"）
    pending_switches: Arc<RwLock<HashSet<String>>>,
    db: Arc<Database>,
}

impl FailoverSwitchManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            pending_switches: Arc::new(RwLock::new(HashSet::new())),
            db,
        }
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
        #[cfg(feature = "tauri-app")] app_handle: Option<&tauri::AppHandle>,
        #[cfg(not(feature = "tauri-app"))] _app_handle: Option<&()>,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
    ) -> Result<bool, AppError> {
        let switch_key = format!("{app_type}:{provider_id}");

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
        #[cfg(feature = "tauri-app")]
        let result = self
            .do_switch(app_handle, app_type, provider_id, provider_name)
            .await;
        #[cfg(not(feature = "tauri-app"))]
        let result = self
            .do_switch(app_type, provider_id, provider_name)
            .await;

        // 清理 pending 标记
        {
            let mut pending = self.pending_switches.write().await;
            pending.remove(&switch_key);
        }

        result
    }

    #[cfg(feature = "tauri-app")]
    async fn do_switch(
        &self,
        app_handle: Option<&tauri::AppHandle>,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
    ) -> Result<bool, AppError> {
        if !self.check_app_enabled(app_type).await {
            return Ok(false);
        }
        self.do_switch_core(app_type, provider_id, provider_name)?;

        // 更新托盘菜单和发射事件
        if let Some(app) = app_handle {
            if let Some(app_state) = app.try_state::<crate::store::AppState>() {
                if let Ok(Some(provider)) = self.db.get_provider_by_id(provider_id, app_type) {
                    if let Err(e) = app_state
                        .proxy_service
                        .update_live_backup_from_provider(app_type, &provider)
                        .await
                    {
                        log::warn!("[FO-003] Live 备份更新失败: {e}");
                    }
                }
                if let Ok(new_menu) = crate::tray::create_tray_menu(app, app_state.inner()) {
                    if let Some(tray) = app.tray_by_id("main") {
                        if let Err(e) = tray.set_menu(Some(new_menu)) {
                            log::error!("[Failover] 更新托盘菜单失败: {e}");
                        }
                    }
                }
            }
            let event_data = serde_json::json!({
                "appType": app_type,
                "providerId": provider_id,
                "source": "failover"
            });
            if let Err(e) = app.emit("provider-switched", event_data) {
                log::error!("[Failover] 发射事件失败: {e}");
            }
        }
        Ok(true)
    }

    #[cfg(not(feature = "tauri-app"))]
    async fn do_switch(
        &self,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
    ) -> Result<bool, AppError> {
        if !self.check_app_enabled(app_type).await {
            return Ok(false);
        }
        self.do_switch_core(app_type, provider_id, provider_name)?;
        Ok(true)
    }

    async fn check_app_enabled(&self, app_type: &str) -> bool {
        match self.db.get_proxy_config_for_app(app_type).await {
            Ok(config) => config.enabled,
            Err(e) => {
                log::warn!("[FO-002] 无法读取 {app_type} 配置: {e}，跳过切换");
                false
            }
        }
    }

    fn do_switch_core(
        &self,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
    ) -> Result<(), AppError> {
        log::info!("[FO-001] 切换: {app_type} → {provider_name}");
        self.db.set_current_provider(app_type, provider_id)?;
        let app_type_enum = crate::app_config::AppType::from_str(app_type)
            .map_err(|_| AppError::Message(format!("无效的应用类型: {app_type}")))?;
        crate::settings::set_current_provider(&app_type_enum, Some(provider_id))?;
        Ok(())
    }
}
