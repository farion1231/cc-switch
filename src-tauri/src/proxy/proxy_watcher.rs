//! 系统代理监视器
//!
//! 周期性重新检测系统代理, 发生变化时热更新全局 HTTP 客户端,
//! 使转发链路跟随系统代理 / 环境变量变化, 无需重启应用。
//! 仅对「跟随系统代理」模式生效; 用户显式配置的全局代理优先且不受影响。

use std::time::Duration;

/// 启动系统代理监视任务。
pub fn spawn() {
    tauri::async_runtime::spawn(async move {
        let interval = Duration::from_secs(8);
        log::info!("[ProxyWatcher] started, interval={}s", interval.as_secs());

        loop {
            tokio::time::sleep(interval).await;
            let changed = crate::proxy::http_client::refresh_system_proxy();
            log::debug!("[ProxyWatcher] tick complete, changed={changed}");
        }
    });
}