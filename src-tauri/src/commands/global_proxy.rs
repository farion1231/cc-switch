//! 全局出站代理相关命令
//!
//! 提供获取、设置和测试全局代理的 Tauri 命令。

use crate::database::Database;
use crate::proxy::http_client;
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;

/// 获取全局代理 URL
///
/// 返回当前配置的代理 URL，null 表示直连。
#[tauri::command]
pub fn get_global_proxy_url(db: tauri::State<'_, Arc<Database>>) -> Result<Option<String>, String> {
    db.get_global_proxy_url().map_err(|e| e.to_string())
}

/// 设置全局代理 URL
///
/// - 传入非空字符串：启用代理
/// - 传入空字符串：清除代理（直连）
#[tauri::command]
pub fn set_global_proxy_url(
    db: tauri::State<'_, Arc<Database>>,
    url: String,
) -> Result<(), String> {
    let url_opt = if url.trim().is_empty() {
        None
    } else {
        Some(url.as_str())
    };

    // 1. 保存到数据库
    db.set_global_proxy_url(url_opt).map_err(|e| e.to_string())?;

    // 2. 热更新全局 HTTP 客户端
    http_client::update_proxy(url_opt)?;

    log::info!(
        "[GlobalProxy] Configuration updated: {}",
        url_opt.unwrap_or("direct connection")
    );

    Ok(())
}

/// 代理测试结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    /// 是否连接成功
    pub success: bool,
    /// 延迟（毫秒）
    pub latency_ms: u64,
    /// 错误信息
    pub error: Option<String>,
}

/// 测试代理连接
///
/// 通过指定的代理 URL 发送测试请求，返回连接结果和延迟。
#[tauri::command]
pub async fn test_proxy_url(url: String) -> Result<ProxyTestResult, String> {
    if url.trim().is_empty() {
        return Err("Proxy URL is empty".to_string());
    }

    let start = Instant::now();

    // 构建带代理的临时客户端
    let proxy = reqwest::Proxy::all(&url).map_err(|e| format!("Invalid proxy URL: {}", e))?;

    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build client: {}", e))?;

    // 测试连接到 api.anthropic.com
    // 使用 HEAD 请求，即使返回 401 也说明网络通了
    let test_url = "https://api.anthropic.com";

    match client.head(test_url).send().await {
        Ok(resp) => {
            let latency = start.elapsed().as_millis() as u64;
            log::debug!(
                "[GlobalProxy] Test successful: {} -> {} ({}ms)",
                url,
                resp.status(),
                latency
            );
            Ok(ProxyTestResult {
                success: true,
                latency_ms: latency,
                error: None,
            })
        }
        Err(e) => {
            let latency = start.elapsed().as_millis() as u64;
            log::debug!("[GlobalProxy] Test failed: {} -> {} ({}ms)", url, e, latency);
            Ok(ProxyTestResult {
                success: false,
                latency_ms: latency,
                error: Some(e.to_string()),
            })
        }
    }
}

/// 获取当前出站代理状态
///
/// 返回当前是否启用了出站代理以及代理 URL。
#[tauri::command]
pub fn get_upstream_proxy_status() -> UpstreamProxyStatus {
    let url = http_client::get_current_proxy_url();
    UpstreamProxyStatus {
        enabled: url.is_some(),
        proxy_url: url,
    }
}

/// 出站代理状态信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamProxyStatus {
    /// 是否启用代理
    pub enabled: bool,
    /// 代理 URL
    pub proxy_url: Option<String>,
}
