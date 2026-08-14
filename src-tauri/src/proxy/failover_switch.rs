//! 故障转移切换模块
//!
//! 处理故障转移成功后的供应商切换逻辑，包括：
//! - 去重控制（避免多个请求同时触发）
//! - 托盘菜单更新
//! - 前端事件发射

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
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
    /// `expected_previous_provider_id` 是发起请求时快照的"当前供应商"。
    /// 若在请求执行期间用户已手动切换了供应商（当前供应商已变化），
    /// 则放弃自动切换，尊重用户选择——避免 failover 把用户的
    /// 手动切换覆盖回旧 provider（原 bug：旧请求完成后自动切回，
    /// 导致 UI 与路由"自己跳回旧供应商"）。
    ///
    /// # Returns
    /// - `Ok(true)` - 切换成功执行
    /// - `Ok(false)` - 切换已在进行中 / 当前供应商已变化，跳过
    /// - `Err(e)` - 切换过程中发生错误
    pub async fn try_switch(
        &self,
        app_handle: Option<&tauri::AppHandle>,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
        expected_previous_provider_id: Option<&str>,
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
            .do_switch(
                app_handle,
                app_type,
                provider_id,
                provider_name,
                expected_previous_provider_id,
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
        expected_previous_provider_id: Option<&str>,
    ) -> Result<bool, AppError> {
        // 检查该应用是否已被代理接管（enabled=true）
        // 只有被接管的应用才允许执行故障转移切换
        let app_enabled = match self.db.get_proxy_config_for_app(app_type).await {
            Ok(config) => config.enabled,
            Err(e) => {
                log::warn!("[FO-002] 无法读取 {app_type} 配置: {e}，跳过切换");
                return Ok(false);
            }
        };

        if !app_enabled {
            log::debug!("[Failover] {app_type} 未启用代理，跳过切换");
            return Ok(false);
        }

        // 关键竞态防护：请求执行期间用户可能已手动切换供应商。
        // 只有"当前供应商"仍等于请求开始时的快照，才允许自动切换；
        // 否则自动切换会覆盖用户刚做的选择。
        if let Some(expected_previous) = expected_previous_provider_id {
            if self.user_switched_during_request(app_type, expected_previous).await {
                let current = crate::settings::get_current_provider(
                    &AppType::from_str(app_type).unwrap_or(AppType::Claude),
                );
                log::info!(
                    "[Failover] 跳过自动切换 {app_type} → {provider_name}: 当前供应商已变为 {:?}（期望 {expected_previous}），尊重用户选择",
                    current
                );
                return Ok(false);
            }
        }

        log::info!("[FO-001] 切换: {app_type} → {provider_name}");

        let mut switched = false;

        if let Some(app) = app_handle {
            if let Some(app_state) = app.try_state::<crate::store::AppState>() {
                switched = app_state
                    .proxy_service
                    .hot_switch_provider(app_type, provider_id)
                    .await
                    .map_err(AppError::Message)?
                    .logical_target_changed;

                if !switched {
                    return Ok(false);
                }

                if let Ok(new_menu) = crate::tray::create_tray_menu(app, app_state.inner()) {
                    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
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
                log::error!("[Failover] 发射事件失败: {e}");
            }
        }

        Ok(switched)
    }

    /// 竞态防护判定：请求执行期间"当前供应商"是否已被（用户/其它路径）改变。
    ///
    /// 返回 `true` 表示应放弃自动切换：当前有效供应商不再等于请求开始时
    /// 快照的 `expected_previous_provider_id`。
    async fn user_switched_during_request(
        &self,
        app_type: &str,
        expected_previous_provider_id: &str,
    ) -> bool {
        let Ok(app_enum) = AppType::from_str(app_type) else {
            return true;
        };
        let current = crate::settings::get_effective_current_provider(&self.db, &app_enum)
            .ok()
            .flatten();
        current.as_deref() != Some(expected_previous_provider_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn setup_db() -> Arc<Database> {
        let db = Arc::new(Database::memory().unwrap());
        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);
        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();
        db
    }

    #[tokio::test]
    #[serial]
    async fn test_no_skip_when_current_matches_expected() {
        let _home = TempHome::new();
        let db = setup_db();
        crate::settings::set_current_provider(&AppType::Claude, Some("a")).unwrap();
        let manager = FailoverSwitchManager::new(db);

        assert!(!manager
            .user_switched_during_request("claude", "a")
            .await);
    }

    #[tokio::test]
    #[serial]
    async fn test_skip_when_user_switched_during_request() {
        let _home = TempHome::new();
        let db = setup_db();
        // Request bắt đầu khi current = "a"
        crate::settings::set_current_provider(&AppType::Claude, Some("a")).unwrap();
        let manager = FailoverSwitchManager::new(db);

        // Trong lúc request chạy, user switch tay sang "b" (settings device-level)
        crate::settings::set_current_provider(&AppType::Claude, Some("b")).unwrap();

        assert!(manager.user_switched_during_request("claude", "a").await);
    }

    #[tokio::test]
    #[serial]
    async fn test_no_skip_after_user_switch_to_expected_target() {
        let _home = TempHome::new();
        let db = setup_db();
        crate::settings::set_current_provider(&AppType::Claude, Some("a")).unwrap();
        let manager = FailoverSwitchManager::new(db);

        // User switch sang chính provider mà failover muốn chuyển tới
        crate::settings::set_current_provider(&AppType::Claude, Some("b")).unwrap();

        assert!(!manager
            .user_switched_during_request("claude", "b")
            .await);
    }

    #[tokio::test]
    #[serial]
    async fn test_skip_on_invalid_app_type() {
        let _home = TempHome::new();
        let db = setup_db();
        crate::settings::set_current_provider(&AppType::Claude, Some("a")).unwrap();
        let manager = FailoverSwitchManager::new(db);

        assert!(manager
            .user_switched_during_request("not-an-app", "a")
            .await);
    }
}
