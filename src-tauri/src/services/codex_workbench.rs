//! Codex 工作台服务 — 状态聚合与设置读写（shell 阶段）
//!
//! 本模块仅提供只读状态与设置更新；启动/桥接/注入在后续任务实现。

use crate::app_config::AppType;
use crate::error::AppError;
use crate::settings::{
    get_settings, set_codex_workbench_settings, CodexWorkbenchSettings,
};
use crate::store::AppState;
use serde::Serialize;
use std::sync::Arc;

/// 工作台运行时状态快照
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

/// 读取工作台状态（shell：runtime 固定 stopped，无启动副作用）
pub async fn get_status(state: Arc<AppState>) -> Result<CodexWorkbenchStatus, AppError> {
    let platform_supported = cfg!(target_os = "windows");
    let install_state = if platform_supported {
        "unknown".to_string()
    } else {
        "unsupported".to_string()
    };

    let proxy_running = state.proxy_service.is_running().await;
    let current_provider_id = {
        let settings = get_settings();
        settings.current_provider_codex.clone()
    };

    Ok(CodexWorkbenchStatus {
        platform_supported,
        install_state,
        runtime_state: "stopped".to_string(),
        cdp_port: None,
        bridge_state: "idle".to_string(),
        current_provider_id,
        proxy_running,
        last_error: None,
    })
}

/// 读取工作台设置
pub fn get_workbench_settings() -> CodexWorkbenchSettings {
    get_settings().codex_workbench
}

/// 更新工作台设置
pub fn update_workbench_settings(
    settings: CodexWorkbenchSettings,
) -> Result<(), AppError> {
    set_codex_workbench_settings(settings)
}

// Silence unused import in shell stage (AppType reserved for later install probe)
#[allow(dead_code)]
fn _reserved_app_type() -> AppType {
    AppType::Codex
}


#[cfg(test)]
mod tests {
    use crate::settings::{
        CodexEnhancementSettings, CodexWorkbenchSettings,
    };

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
        assert!(s.auto_launch);
        assert!(s.auto_start_proxy);
        assert!(s.script_market_url.contains("CodexPlusPlusScriptMarket"));
    }

    #[test]
    fn workbench_settings_camel_case_roundtrip() {
        let s = CodexWorkbenchSettings::default();
        let json = serde_json::to_string(&s).expect("serialize");
        assert!(json.contains("scriptMarketUrl"));
        assert!(json.contains("radarTtlMinutes"));
        assert!(json.contains("pluginUnlock"));
        let back: CodexWorkbenchSettings =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.radar_ttl_minutes, 30);
        assert!(back.enhancements.plugin_unlock);
        assert!(!back.enhancements.reasoning_token);
    }
}
