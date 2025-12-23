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
        app_handle: Option<&tauri::AppHandle>,
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
        let result = self
            .do_switch(app_handle, app_type, provider_id, provider_name)
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
    ) -> Result<bool, AppError> {
        log::info!("[Failover] 开始切换供应商: {app_type} -> {provider_name} ({provider_id})");

        // 1. 更新数据库 is_current
        self.db.set_current_provider(app_type, provider_id)?;

        // 2. 更新本地 settings（设备级）
        let app_type_enum = crate::app_config::AppType::from_str(app_type)
            .map_err(|_| AppError::Message(format!("无效的应用类型: {app_type}")))?;
        crate::settings::set_current_provider(&app_type_enum, Some(provider_id))?;

        // 3. 更新托盘菜单和发射事件
        if let Some(app) = app_handle {
            // 更新托盘菜单
            if let Some(app_state) = app.try_state::<crate::store::AppState>() {
                // 更新 Live 备份（确保代理停止时恢复正确配置）
                if let Ok(Some(provider)) = self.db.get_provider_by_id(provider_id, app_type) {
                    if let Err(e) = app_state
                        .proxy_service
                        .update_live_backup_from_provider(app_type, &provider)
                        .await
                    {
                        log::warn!("[Failover] 更新 Live 备份失败: {e}");
                    }
                }

                // 重建托盘菜单
                if let Ok(new_menu) = crate::tray::create_tray_menu(app, app_state.inner()) {
                    if let Some(tray) = app.tray_by_id("main") {
                        if let Err(e) = tray.set_menu(Some(new_menu)) {
                            log::error!("[Failover] 更新托盘菜单失败: {e}");
                        }
                    }
                }
            }

            // 发射事件到前端
            let event_data = serde_json::json!({
                "appType": app_type,
                "providerId": provider_id,
                "source": "failover"  // 标识来源是故障转移
            });
            if let Err(e) = app.emit("provider-switched", event_data) {
                log::error!("[Failover] 发射供应商切换事件失败: {e}");
            }
        }

        log::info!("[Failover] 供应商切换完成: {app_type} -> {provider_name} ({provider_id})");

        Ok(true)
    }
}
