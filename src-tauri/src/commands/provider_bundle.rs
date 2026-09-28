//! Provider Bundle 命令层（薄包装，转发到 `crate::provider_bundle`）

use tauri::AppHandle;

use crate::provider_bundle::{self, InstallBundleRequest, InstallBundleResult, BundleSpecView};

/// 列出所有已注册的 provider bundle。
#[tauri::command]
pub fn list_provider_bundles() -> Vec<BundleSpecView> {
    provider_bundle::list_bundles()
}

/// 一键安装 bundle：原子化地把多个端点写入 providers 表 + 入故障转移
/// 队列 + 开启 auto_failover + 切到 P1。返回每个端点的 id、P1 id、
/// 是否真正启用了 auto_failover、以及缺失的环境变量（用于 UI 提示）。
#[tauri::command]
pub async fn install_provider_bundle(
    app_handle: AppHandle,
    request: InstallBundleRequest,
) -> Result<InstallBundleResult, String> {
    provider_bundle::install_bundle(app_handle, request).await
}