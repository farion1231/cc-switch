pub mod auth;
pub mod claude_config;
pub(crate) mod extension;
pub mod fingerprint;
pub mod proxy_hooks;
pub mod proxy_processor;

pub fn init(app: &tauri::App) {
    use auth::commands::HuaweiCodeAgentAuthState;
    use auth::manager::HuaweiCodeAgentAuthManager;
    use std::sync::Arc;
    use tauri::Manager;

    let app_config_dir = crate::config::get_app_config_dir();
    let huawei_codeagent_manager = Arc::new(HuaweiCodeAgentAuthManager::new(app_config_dir));
    if let Err(e) = huawei_codeagent_manager.load_tokens_sync() {
        log::warn!("[HuaweiCodeAgent] 加载 Token 失败: {}", e);
    }

    // 启动时主动刷新过期 Token（与旧 HuaweiIdeaAuthManager 行为一致）
    let manager_for_refresh = Arc::clone(&huawei_codeagent_manager);
    tauri::async_runtime::spawn(async move {
        let accounts = manager_for_refresh.list_accounts().await;
        for account in &accounts {
            if account.requires_reauth {
                log::warn!(
                    "[HuaweiCodeAgent] 账号 {} Token 已过期且无存储密码，需重新登录",
                    account.user
                );
            }
        }
        // list_accounts 中 requires_reauth=false 不代表 token 未过期
        // （有 encrypted_password 时 requires_reauth=false 但 token 可能已过期）。
        // 直接检查 token 过期状态并刷新。
        let needs_refresh = {
            let status = manager_for_refresh.get_status().await;
            !status.authenticated
        };
        if needs_refresh {
            if let Some(account_id) = accounts.first().map(|a| a.id.clone()) {
                log::info!("[HuaweiCodeAgent] Token 已过期，启动时主动刷新");
                match manager_for_refresh.refresh_token(&account_id).await {
                    Ok(_) => log::info!("[HuaweiCodeAgent] 启动时 Token 刷新成功"),
                    Err(e) => log::warn!("[HuaweiCodeAgent] 启动时 Token 刷新失败: {}", e),
                }
            }
        }
    });

    app.manage(HuaweiCodeAgentAuthState(huawei_codeagent_manager));
    log::info!("✓ HuaweiCodeAgentAuthManager initialized");

    crate::proxy::hooks::HookRegistry::global()
        .register(Box::new(proxy_hooks::HuaweiCodeAgentProxyHook::new()));
    log::info!("✓ HuaweiCodeAgentProxyHook registered");
}
