//! 分类器队列命令
//!
//! 管理代理模式下的分类器队列（基于 providers 表的 in_classifier_queue 字段）。
//!
//! 目标场景：Claude Code Auto Mode 在执行 Bash 命令前发出的「安全分类器」请求
//! 有客户端硬超时，把它分流到响应快的供应商并关掉思考可避免超时。

use crate::database::ClassifierQueueItem;
use crate::provider::Provider;
use crate::store::AppState;
use serde::{Deserialize, Serialize};

/// 分类器队列的两个开关（成对读写）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifierConfig {
    /// 分类器队列总开关
    pub enabled: bool,
    /// 分类器请求强制关闭思考
    pub force_thinking_off: bool,
}

/// 分类器队列目前只对 Claude 开放
///
/// 特征签名来自 Claude Code 的 Bash 安全分类请求，只有 Anthropic Messages
/// 入站格式才认得出来；Codex / Gemini / GrokBuild 永远不会命中。
fn require_classifier_app(app_type: &str) -> Result<(), String> {
    if app_type != crate::app_config::AppType::Claude.as_str() {
        return Err(format!("{app_type} 不支持分类器队列"));
    }
    Ok(())
}

/// 获取分类器队列
#[tauri::command]
pub async fn get_classifier_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<ClassifierQueueItem>, String> {
    require_classifier_app(&app_type)?;
    state
        .db
        .get_classifier_queue(&app_type)
        .map_err(|e| e.to_string())
}

/// 获取可添加到分类器队列的供应商（不在队列中的）
#[tauri::command]
pub async fn get_available_providers_for_classifier(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<Provider>, String> {
    require_classifier_app(&app_type)?;
    state
        .db
        .get_available_providers_for_classifier(&app_type)
        .map_err(|e| e.to_string())
}

/// 添加供应商到分类器队列
#[tauri::command]
pub async fn add_to_classifier_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    require_classifier_app(&app_type)?;
    state
        .db
        .get_provider_by_id(&provider_id, &app_type)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
    state
        .db
        .add_to_classifier_queue(&app_type, &provider_id)
        .map_err(|e| e.to_string())
}

/// 从分类器队列移除供应商
#[tauri::command]
pub async fn remove_from_classifier_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    require_classifier_app(&app_type)?;
    state
        .db
        .remove_from_classifier_queue(&app_type, &provider_id)
        .map_err(|e| e.to_string())
}

/// 读取分类器队列的两个开关
#[tauri::command]
pub async fn get_classifier_config(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<ClassifierConfig, String> {
    require_classifier_app(&app_type)?;
    let config = state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ClassifierConfig {
        enabled: config.classifier_queue_enabled,
        force_thinking_off: config.classifier_force_thinking_off,
    })
}

/// 写入分类器队列的两个开关
///
/// 刻意比 `set_auto_failover_enabled` 简单：分类器队列不切换当前供应商、
/// 不刷托盘、不发 `provider-switched`，因为它不改变任何用户可见状态。
/// 队列为空也是合法状态 —— 此时分类器请求回落到常规路由链。
#[tauri::command]
pub async fn set_classifier_config(
    state: tauri::State<'_, AppState>,
    app_type: String,
    config: ClassifierConfig,
) -> Result<(), String> {
    require_classifier_app(&app_type)?;

    let mut proxy_config = state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())?;

    // 仅在开启时要求代理已接管（与故障转移一致）
    if config.enabled && !proxy_config.enabled {
        return Err("需要先启用 Claude 的代理接管，再开启分类器队列".to_string());
    }

    proxy_config.classifier_queue_enabled = config.enabled;
    proxy_config.classifier_force_thinking_off = config.force_thinking_off;

    state
        .db
        .update_proxy_config_for_app(proxy_config)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::require_classifier_app;

    #[test]
    fn classifier_is_claude_only() {
        assert!(require_classifier_app("claude").is_ok());
        assert!(require_classifier_app("codex").is_err());
        assert!(require_classifier_app("gemini").is_err());
        assert!(require_classifier_app("grokbuild").is_err());
        assert!(require_classifier_app("pi").is_err());
    }
}
