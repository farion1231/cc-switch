//! Codex 工作台服务 — 状态聚合、设置读写与启动入口

use crate::app_config::AppType;
use crate::error::AppError;
use crate::services::codex_runtime::{
    self, CodexRuntimeHandle, CodexRuntimeState, LaunchEnhancedCodexResult,
};
use crate::settings::{
    get_settings, set_codex_workbench_settings, CodexWorkbenchSettings,
};
use crate::store::AppState;
use serde::Serialize;

/// 工作台聚合状态（前端轮询）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexWorkbenchStatus {
    pub platform_supported: bool,
    pub install_state: String,
    pub runtime_state: String,
    pub cdp_port: Option<u16>,
    pub bridge_state: String,
    pub current_provider_id: Option<String>,
    pub proxy_running: bool,
    pub last_error: Option<String>,
}

fn runtime_state_str(state: &CodexRuntimeState) -> String {
    match state {
        CodexRuntimeState::Stopped => "stopped",
        CodexRuntimeState::Launching => "launching",
        CodexRuntimeState::Injecting => "injecting",
        CodexRuntimeState::Running => "running",
        CodexRuntimeState::OrdinaryRunning => "ordinary_running",
        CodexRuntimeState::Degraded => "degraded",
        CodexRuntimeState::StaleLock => "stale_lock",
        CodexRuntimeState::Unsupported => "unsupported",
    }
    .to_string()
}

/// 读取工作台状态
pub async fn get_status(state: &AppState) -> Result<CodexWorkbenchStatus, AppError> {
    let platform_supported = cfg!(target_os = "windows");
    let install_state = if platform_supported {
        match codex_runtime::discovery::discover_codex_executable() {
            Ok(_) => "installed".to_string(),
            Err(_) => "missing".to_string(),
        }
    } else {
        "unsupported".to_string()
    };

    let proxy_running = state.proxy_service.is_running().await;
    let current_provider_id = get_settings().current_provider_codex.clone();
    let snap = state.codex_runtime.snapshot().await;
    let bridge_state = if snap.bridge_port.is_some() {
        "listening"
    } else {
        "idle"
    }
    .to_string();

    let runtime_state = if !platform_supported {
        "unsupported".to_string()
    } else {
        runtime_state_str(&snap.state)
    };

    Ok(CodexWorkbenchStatus {
        platform_supported,
        install_state,
        runtime_state,
        cdp_port: snap.cdp_port,
        bridge_state,
        current_provider_id,
        proxy_running,
        last_error: snap.message,
    })
}

/// 启动增强 Codex（Windows）
pub async fn launch_enhanced(
    handle: &CodexRuntimeHandle,
) -> Result<LaunchEnhancedCodexResult, AppError> {
    codex_runtime::launch_enhanced_codex(handle).await
}

/// 重新注入增强脚本，并返回最新状态（需完整 AppState 填 proxy 等）
pub async fn reinject(handle: &CodexRuntimeHandle) -> Result<LaunchEnhancedCodexResult, AppError> {
    codex_runtime::reinject_enhancements(handle).await
}

/// 读取工作台设置
pub fn get_workbench_settings() -> CodexWorkbenchSettings {
    get_settings().codex_workbench
}

/// 更新工作台设置
pub fn update_workbench_settings(settings: CodexWorkbenchSettings) -> Result<(), AppError> {
    set_codex_workbench_settings(settings)
}

#[allow(dead_code)]
fn _reserved_app_type() -> AppType {
    AppType::Codex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{CodexEnhancementSettings, CodexWorkbenchSettings};

    #[test]
    fn enhancement_defaults_first_six_true_last_five_false() {
        let e = CodexEnhancementSettings::default();
        assert!(e.plugin_unlock);
        assert!(e.auto_expand);
        assert!(e.session_delete);
        assert!(e.wide_conversation);
        assert!(e.native_menu);
        assert!(e.user_script_runtime);
        assert!(!e.markdown_export);
        assert!(!e.model_switcher);
        assert!(!e.system_prompt);
        assert!(!e.reasoning_resume);
        assert!(!e.reasoning_token);
    }

    #[test]
    fn workbench_defaults_radar_ttl_and_market_url() {
        let s = CodexWorkbenchSettings::default();
        assert_eq!(s.radar_ttl_minutes, 30);
        assert!(s.script_market_url.contains("CodexPlusPlusScriptMarket"));
        assert!(s.auto_launch);
        assert!(s.auto_start_proxy);
    }

    #[test]
    fn status_serializes_camel_case() {
        let status = CodexWorkbenchStatus {
            platform_supported: true,
            install_state: "unknown".into(),
            runtime_state: "stopped".into(),
            cdp_port: None,
            bridge_state: "idle".into(),
            current_provider_id: None,
            proxy_running: false,
            last_error: None,
        };
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(json.contains("platformSupported"));
        assert!(json.contains("runtimeState"));
        assert!(json.contains("bridgeState"));
        assert!(json.contains("proxyRunning"));
    }

    #[test]
    fn settings_roundtrip_camel_case() {
        let s = CodexWorkbenchSettings {
            radar_ttl_minutes: 30,
            enhancements: CodexEnhancementSettings {
                plugin_unlock: true,
                reasoning_token: false,
                ..CodexEnhancementSettings::default()
            },
            ..CodexWorkbenchSettings::default()
        };
        let json = serde_json::to_string(&s).expect("serialize");
        assert!(json.contains("radarTtlMinutes"));
        assert!(json.contains("pluginUnlock"));
        let back: CodexWorkbenchSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.radar_ttl_minutes, 30);
        assert!(back.enhancements.plugin_unlock);
        assert!(!back.enhancements.reasoning_token);
    }
}
