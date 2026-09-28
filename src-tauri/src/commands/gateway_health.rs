//! 网关健康与启动命令层（薄包装）

use crate::gateway_health::{
    GatewayEndpointMeta, GatewayHealth, StartLocalGatewayResult,
};

/// 列出所有已注册的 gateway endpoint（含 label/url/role）。
#[tauri::command]
pub fn list_gateway_endpoints() -> Vec<GatewayEndpointMeta> {
    crate::gateway_health::known_endpoints()
}

/// 探测单个 endpoint（async）。
///
/// `url` 可选——不传就用 `endpoint_id` 找 known_endpoints 的 url。
#[tauri::command]
pub async fn probe_gateway(endpoint_id: String, url: Option<String>) -> GatewayHealth {
    let url = url.unwrap_or_else(|| {
        crate::gateway_health::known_endpoints()
            .into_iter()
            .find(|e| e.id == endpoint_id)
            .map(|e| e.url)
            .unwrap_or_else(|| "http://127.0.0.1:1".to_string())
    });
    crate::gateway_health::probe(&url, &endpoint_id).await
}

/// 探测所有已知 endpoint（async 并发）。
#[tauri::command]
pub async fn probe_all_gateways() -> Vec<GatewayHealth> {
    crate::gateway_health::probe_all().await
}

/// 启动本机 8782 网关。返回 success/command/stdout/stderr。
#[tauri::command]
pub async fn start_local_gateway() -> StartLocalGatewayResult {
    tauri::async_runtime::spawn_blocking(crate::gateway_health::start_local_gateway)
        .await
        .unwrap_or_else(|e| StartLocalGatewayResult {
            success: false,
            command: String::new(),
            stdout: String::new(),
            stderr: format!("join 失败: {e}"),
            exit_code: None,
            duration_ms: 0,
        })
}