//! 代理服务相关的 Tauri 命令
//!
//! 提供前端调用的 API 接口

use crate::error::AppError;
use crate::proxy::types::*;
use crate::proxy::{CircuitBreakerConfig, CircuitBreakerStats};
use crate::store::AppState;
use std::str::FromStr;

fn require_proxy_app(app_type: &str) -> Result<crate::app_config::AppType, String> {
    let app = crate::app_config::AppType::from_str(app_type)
        .map_err(|error| format!("无效的应用类型: {error}"))?;
    if !app.supports_local_proxy() {
        return Err(format!("{} 不支持本地路由", app.as_str()));
    }
    Ok(app)
}

/// 启动代理服务器（仅启动服务，不接管 Live 配置）
#[tauri::command]
pub async fn start_proxy_server(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyServerInfo, String> {
    state.proxy_service.start().await
}

/// 停止代理服务器（仅停止服务，不恢复/清理 Live 接管状态）
#[tauri::command]
pub async fn stop_proxy_server(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let takeover = state.proxy_service.get_takeover_status().await?;
    if takeover.claude
        || takeover.codex
        || takeover.gemini
        || takeover.grokbuild
        || takeover.opencode
        || takeover.openclaw
    {
        return Err(
            "仍有应用处于代理接管状态，请先在设置中关闭对应应用接管后再停止本地路由。".to_string(),
        );
    }

    state.proxy_service.stop().await
}

/// 停止代理服务器（恢复 Live 配置）
#[tauri::command]
pub async fn stop_proxy_with_restore(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.proxy_service.stop_with_restore().await
}

/// 获取各应用接管状态
#[tauri::command]
pub async fn get_proxy_takeover_status(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyTakeoverStatus, String> {
    state.proxy_service.get_takeover_status().await
}

/// 为指定应用开启/关闭接管
#[tauri::command]
pub async fn set_proxy_takeover_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .proxy_service
        .set_takeover_for_app(&app_type, enabled)
        .await
}

/// 获取代理服务器状态
#[tauri::command]
pub async fn get_proxy_status(state: tauri::State<'_, AppState>) -> Result<ProxyStatus, String> {
    state.proxy_service.get_status().await
}

/// 获取代理配置
#[tauri::command]
pub async fn get_proxy_config(state: tauri::State<'_, AppState>) -> Result<ProxyConfig, String> {
    state.proxy_service.get_config().await
}

/// 更新代理配置
#[tauri::command]
pub async fn update_proxy_config(
    state: tauri::State<'_, AppState>,
    config: ProxyConfig,
) -> Result<(), String> {
    state.proxy_service.update_config(&config).await
}

// ==================== Global & Per-App Config ====================

/// 获取全局代理配置
///
/// 返回统一的全局配置字段（代理开关、监听地址、端口、日志开关）
#[tauri::command]
pub async fn get_global_proxy_config(
    state: tauri::State<'_, AppState>,
) -> Result<GlobalProxyConfig, String> {
    let db = &state.db;
    db.get_global_proxy_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新全局代理配置
///
/// 更新统一的全局配置字段，会同时更新三行（claude/codex/gemini）
#[tauri::command]
pub async fn update_global_proxy_config(
    state: tauri::State<'_, AppState>,
    config: GlobalProxyConfig,
) -> Result<(), String> {
    let db = &state.db;
    db.update_global_proxy_config(config)
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定应用的代理配置
///
/// 返回应用级配置（enabled、auto_failover、超时、熔断器等）
#[tauri::command]
pub async fn get_proxy_config_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<AppProxyConfig, String> {
    require_proxy_app(&app_type)?;
    let db = &state.db;
    db.get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 规则变更后尽力重写接管 live env（spec §6：注入值必须与决策基准同源）
///
/// 接管生效期间，注入到 Claude live 配置的 CLAUDE_CODE_SUBAGENT_MODEL 与
/// subagent 路由决策共用同一来源；若保存规则后不重写 live，运行中的请求
/// 仍按旧模型名识别 subagent，导致改道静默失效。仅当接管开启且 live 配置
/// 确实处于接管态时才重写（与 sibling 判定一致，组合使用接管状态与
/// live 探测，不引入新判定）。任何失败仅告警，不阻塞配置保存。
async fn resync_claude_live_subagent_route(state: &AppState) {
    let takeover_enabled = match state.proxy_service.get_takeover_status().await {
        Ok(status) => status.claude,
        Err(e) => {
            log::warn!("[SubagentRoute] 读取接管状态失败，跳过 live env 重同步: {e}");
            return;
        }
    };
    if !takeover_enabled
        || !state
            .proxy_service
            .detect_takeover_in_live_config_for_app(&crate::app_config::AppType::Claude)
    {
        return;
    }

    // 与 sibling 调用方一致：从 SSOT（本地 settings + 数据库 is_current）取当前供应商
    let current_id = match crate::settings::get_effective_current_provider(
        &state.db,
        &crate::app_config::AppType::Claude,
    ) {
        Ok(Some(id)) => id,
        Ok(None) => return,
        Err(e) => {
            log::warn!("[SubagentRoute] 获取当前供应商失败，跳过 live env 重同步: {e}");
            return;
        }
    };
    let provider = match state
        .db
        .get_provider_by_id(&current_id, crate::app_config::AppType::Claude.as_str())
    {
        Ok(Some(provider)) => provider,
        Ok(None) => return,
        Err(e) => {
            log::warn!("[SubagentRoute] 读取当前供应商失败，跳过 live env 重同步: {e}");
            return;
        }
    };

    if let Err(e) = state
        .proxy_service
        .sync_claude_live_from_provider_while_proxy_active(&provider)
        .await
    {
        log::warn!(
            "[SubagentRoute] 规则变更后重写 claude live env 失败（规则已保存，重启接管后生效）: {e}"
        );
    }
}

async fn update_proxy_config_for_app_internal(
    state: &AppState,
    config: AppProxyConfig,
) -> Result<(), String> {
    let db = &state.db;
    let app_type = config.app_type.clone();
    let app = require_proxy_app(&app_type)?;
    let circuit_config = CircuitBreakerConfig::from(&config);

    db.update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())?;

    state
        .proxy_service
        .update_circuit_breaker_config_for_app(&app_type, circuit_config)
        .await?;

    // 仅 claude 有接管 live env 注入，规则变更后需保持注入值与决策基准同源
    if matches!(app, crate::app_config::AppType::Claude) {
        resync_claude_live_subagent_route(state).await;
    }
    Ok(())
}

/// 更新指定应用的代理配置
///
/// 更新应用级配置（enabled、auto_failover、超时、熔断器等）；
/// claude 接管生效期间会顺带重写 live env，使注入的 subagent 模型与刚保存的规则一致
#[tauri::command]
pub async fn update_proxy_config_for_app(
    state: tauri::State<'_, AppState>,
    config: AppProxyConfig,
) -> Result<(), String> {
    update_proxy_config_for_app_internal(&state, config).await
}

async fn get_default_cost_multiplier_internal(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    let db = &state.db;
    db.get_default_cost_multiplier(app_type).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn get_default_cost_multiplier_test_hook(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    get_default_cost_multiplier_internal(state, app_type).await
}

/// 获取默认成本倍率
#[tauri::command]
pub async fn get_default_cost_multiplier(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<String, String> {
    get_default_cost_multiplier_internal(&state, &app_type)
        .await
        .map_err(|e| e.to_string())
}

async fn set_default_cost_multiplier_internal(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    let db = &state.db;
    db.set_default_cost_multiplier(app_type, value).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn set_default_cost_multiplier_test_hook(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    set_default_cost_multiplier_internal(state, app_type, value).await
}

/// 设置默认成本倍率
#[tauri::command]
pub async fn set_default_cost_multiplier(
    state: tauri::State<'_, AppState>,
    app_type: String,
    value: String,
) -> Result<(), String> {
    set_default_cost_multiplier_internal(&state, &app_type, &value)
        .await
        .map_err(|e| e.to_string())
}

async fn get_pricing_model_source_internal(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    let db = &state.db;
    db.get_pricing_model_source(app_type).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn get_pricing_model_source_test_hook(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    get_pricing_model_source_internal(state, app_type).await
}

/// 获取计费模式来源
#[tauri::command]
pub async fn get_pricing_model_source(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<String, String> {
    get_pricing_model_source_internal(&state, &app_type)
        .await
        .map_err(|e| e.to_string())
}

async fn set_pricing_model_source_internal(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    let db = &state.db;
    db.set_pricing_model_source(app_type, value).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn set_pricing_model_source_test_hook(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    set_pricing_model_source_internal(state, app_type, value).await
}

/// 设置计费模式来源
#[tauri::command]
pub async fn set_pricing_model_source(
    state: tauri::State<'_, AppState>,
    app_type: String,
    value: String,
) -> Result<(), String> {
    set_pricing_model_source_internal(&state, &app_type, &value)
        .await
        .map_err(|e| e.to_string())
}

/// 检查代理服务器是否正在运行
#[tauri::command]
pub async fn is_proxy_running(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    Ok(state.proxy_service.is_running().await)
}

/// 检查是否处于 Live 接管模式
#[tauri::command]
pub async fn is_live_takeover_active(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    state.proxy_service.is_takeover_active().await
}

/// 代理模式下切换供应商（热切换）
#[tauri::command]
pub async fn switch_proxy_provider(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    let app = require_proxy_app(&app_type)?;
    // Codex official account cards can use the client's native OpenAI login
    // through takeover. Other apps' official providers remain blocked.
    let provider = state
        .db
        .get_provider_by_id(&provider_id, &app_type)
        .map_err(|e| format!("读取供应商失败: {e}"))?
        .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
    if provider.category.as_deref() == Some("official")
        && !crate::services::provider::official_provider_supports_proxy_takeover(&app, &provider)
    {
        return Err(
            "代理接管模式下不能切换到官方供应商 (Cannot switch to official provider during proxy takeover)"
                .to_string(),
        );
    }

    state
        .proxy_service
        .switch_proxy_target(&app_type, &provider_id)
        .await
}

// ==================== 故障转移相关命令 ====================

/// 获取供应商健康状态
#[tauri::command]
pub async fn get_provider_health(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<ProviderHealth, String> {
    require_proxy_app(&app_type)?;
    let db = &state.db;
    db.get_provider_health(&provider_id, &app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 重置熔断器
///
/// 重置后会检查是否应该切回队列中优先级更高的供应商：
/// 1. 检查自动故障转移是否开启
/// 2. 如果恢复的供应商在队列中优先级更高（queue_order 更小），则自动切换
#[tauri::command]
pub async fn reset_circuit_breaker(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<(), String> {
    require_proxy_app(&app_type)?;
    // 1. 重置数据库健康状态
    let db = &state.db;
    db.update_provider_health(&provider_id, &app_type, true, None)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 如果代理正在运行，重置内存中的熔断器状态
    state
        .proxy_service
        .reset_provider_circuit_breaker(&provider_id, &app_type)
        .await?;

    // 3. 检查是否应该切回优先级更高的供应商（从 proxy_config 表读取）
    // 只有当该应用已被代理接管（enabled=true）且开启了自动故障转移时才执行
    let (app_enabled, auto_failover_enabled) = match db.get_proxy_config_for_app(&app_type).await {
        Ok(config) => (config.enabled, config.auto_failover_enabled),
        Err(e) => {
            log::error!("[{app_type}] Failed to read proxy_config: {e}, defaulting to disabled");
            (false, false)
        }
    };

    if app_enabled && auto_failover_enabled && state.proxy_service.is_running().await {
        // 获取当前供应商 ID
        let current_id = db
            .get_current_provider(&app_type)
            .map_err(|e| e.to_string())?;

        if let Some(current_id) = current_id {
            // 获取故障转移队列
            let queue = db
                .get_failover_queue(&app_type)
                .map_err(|e| e.to_string())?;

            // 找到恢复的供应商和当前供应商在队列中的位置（使用 sort_index）
            let restored_order = queue
                .iter()
                .find(|item| item.provider_id == provider_id)
                .and_then(|item| item.sort_index);

            let current_order = queue
                .iter()
                .find(|item| item.provider_id == current_id)
                .and_then(|item| item.sort_index);

            // 如果恢复的供应商优先级更高（sort_index 更小），则切换
            if let (Some(restored), Some(current)) = (restored_order, current_order) {
                if restored < current {
                    log::info!(
                        "[Recovery] 供应商 {provider_id} 已恢复且优先级更高 (P{restored} vs P{current})，自动切换"
                    );

                    // 获取供应商名称用于日志和事件
                    let provider_name = db
                        .get_all_providers(&app_type)
                        .ok()
                        .and_then(|providers| providers.get(&provider_id).map(|p| p.name.clone()))
                        .unwrap_or_else(|| provider_id.clone());

                    // 创建故障转移切换管理器并执行切换
                    let switch_manager =
                        crate::proxy::failover_switch::FailoverSwitchManager::new(db.clone());
                    if let Err(e) = switch_manager
                        .try_switch(Some(&app_handle), &app_type, &provider_id, &provider_name)
                        .await
                    {
                        log::error!("[Recovery] 自动切换失败: {e}");
                    }
                }
            }
        }
    }

    Ok(())
}

/// 获取熔断器配置
#[tauri::command]
pub async fn get_circuit_breaker_config(
    state: tauri::State<'_, AppState>,
) -> Result<CircuitBreakerConfig, String> {
    let db = &state.db;
    db.get_circuit_breaker_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新熔断器配置
#[tauri::command]
pub async fn update_circuit_breaker_config(
    state: tauri::State<'_, AppState>,
    config: CircuitBreakerConfig,
) -> Result<(), String> {
    let db = &state.db;

    // 1. 更新数据库配置
    db.update_circuit_breaker_config(&config)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 如果代理正在运行，热更新内存中的熔断器配置
    state
        .proxy_service
        .update_circuit_breaker_configs(config)
        .await?;

    Ok(())
}

/// 获取熔断器统计信息（仅当代理服务器运行时）
#[tauri::command]
pub async fn get_circuit_breaker_stats(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<Option<CircuitBreakerStats>, String> {
    require_proxy_app(&app_type)?;
    // 这个功能需要访问运行中的代理服务器的内存状态
    // 目前先返回 None，后续可以通过 ProxyService 暴露接口来实现
    let _ = (state, provider_id, app_type);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::provider::Provider;
    use crate::proxy::types::SubagentRoute;
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    /// 与 services::proxy::tests 中的 TempHome 相同：隔离 HOME，
    /// 保证 claude live 配置写入临时目录（需 #[serial]）。
    struct TempHome {
        #[allow(dead_code)]
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

    fn claude_provider(id: &str, env: serde_json::Value) -> Provider {
        Provider::with_id(id.to_string(), id.to_string(), json!({"env": env}), None)
    }

    fn app_config(app_type: &str, enabled: bool, route: Option<SubagentRoute>) -> AppProxyConfig {
        AppProxyConfig {
            app_type: app_type.to_string(),
            enabled,
            auto_failover_enabled: false,
            max_retries: 3,
            streaming_first_byte_timeout: 60,
            streaming_idle_timeout: 120,
            non_streaming_timeout: 600,
            circuit_failure_threshold: 4,
            circuit_success_threshold: 2,
            circuit_timeout_seconds: 60,
            circuit_error_rate_threshold: 0.6,
            circuit_min_requests: 10,
            subagent_route: route,
        }
    }

    /// 写入一个处于接管态的 claude live 配置（含占位符与旧的注入值）
    fn write_taken_over_claude_live(subagent_model: &str) {
        let settings_path = crate::config::get_claude_settings_path();
        std::fs::create_dir_all(settings_path.parent().unwrap()).expect("create claude dir");
        crate::config::write_json_file(
            &settings_path,
            &json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED",
                    "CLAUDE_CODE_SUBAGENT_MODEL": subagent_model,
                }
            }),
        )
        .expect("write taken-over claude live");
    }

    fn read_claude_live_env() -> serde_json::Value {
        let settings_path = crate::config::get_claude_settings_path();
        let value: serde_json::Value =
            crate::config::read_json_file(&settings_path).expect("read claude live");
        value.get("env").cloned().unwrap_or(serde_json::Value::Null)
    }

    #[tokio::test]
    #[serial]
    async fn update_config_resyncs_claude_live_subagent_env_while_takeover_active() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = std::sync::Arc::new(crate::database::Database::memory().expect("init db"));
        db.save_provider(
            "claude",
            &claude_provider("a", json!({"ANTHROPIC_AUTH_TOKEN": "sk-a"})),
        )
        .expect("save provider a");
        db.set_current_provider("claude", "a").expect("set current");
        let state = crate::store::AppState::new(db.clone());

        // live 已处于接管态，注入的是旧模型名
        write_taken_over_claude_live("old-model");
        assert!(state
            .proxy_service
            .detect_takeover_in_live_config_for_app(&AppType::Claude));

        // 保存新规则（enabled=true 保持接管）→ live env 应同步为新模型名
        let config = app_config(
            "claude",
            true,
            Some(SubagentRoute {
                provider_id: "b".to_string(),
                model: Some("glm-flash".to_string()),
            }),
        );
        update_proxy_config_for_app_internal(&state, config)
            .await
            .expect("save config should succeed");

        let env = read_claude_live_env();
        assert_eq!(
            env.get("CLAUDE_CODE_SUBAGENT_MODEL")
                .and_then(|v| v.as_str()),
            Some("glm-flash"),
            "live env should be re-synced to the just-saved rule model"
        );
        // 重写后接管字段仍在（live 仍处于接管态）
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
            Some("PROXY_MANAGED")
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_config_skips_live_resync_when_takeover_off() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = std::sync::Arc::new(crate::database::Database::memory().expect("init db"));
        db.save_provider(
            "claude",
            &claude_provider("a", json!({"ANTHROPIC_AUTH_TOKEN": "sk-a"})),
        )
        .expect("save provider a");
        db.set_current_provider("claude", "a").expect("set current");
        let state = crate::store::AppState::new(db.clone());

        // 未接管：live 就是普通供应商配置，保存规则不应改写它
        write_taken_over_claude_live("user-own-env");
        // 去掉占位符，模拟未接管状态
        let settings_path = crate::config::get_claude_settings_path();
        crate::config::write_json_file(
            &settings_path,
            &json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-a"}}),
        )
        .expect("write plain claude live");

        let config = app_config(
            "claude",
            false,
            Some(SubagentRoute {
                provider_id: "b".to_string(),
                model: Some("glm-flash".to_string()),
            }),
        );
        update_proxy_config_for_app_internal(&state, config)
            .await
            .expect("save config should succeed");

        let env = read_claude_live_env();
        assert!(
            env.get("CLAUDE_CODE_SUBAGENT_MODEL").is_none(),
            "live env must not be injected when takeover is off"
        );
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
            Some("sk-a")
        );
    }
}
