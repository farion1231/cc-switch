//! 代理服务业务逻辑层
//!
//! 提供代理服务器的启动、停止和配置管理

use crate::app_config::AppType;
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::server::ProxyServer;
use crate::proxy::switch_lock::SwitchLockManager;
use crate::proxy::types::*;
use crate::services::provider::{
    build_effective_settings_with_common_config, write_live_with_common_config,
};
use serde_json::{json, Map, Value};
use std::str::FromStr;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::RwLock;

/// 用于接管 Live 配置时的占位符（避免客户端提示缺少 key，同时不泄露真实 Token）
const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";

/// 代理接管模式下需要从 Claude Live 配置中移除的"模型覆盖"字段。
///
/// 原因：接管模式下 `*_MODEL` 必须由 CC Switch 写成稳定的 Claude 角色别名，
/// 再由本地代理映射到当前供应商真实模型；`*_MODEL_NAME` 也需要同步接管，
/// 否则 Claude Code 模型菜单会残留上一个供应商的显示名称。
const CLAUDE_MODEL_OVERRIDE_ENV_KEYS: [&str; 9] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_REASONING_MODEL", // legacy: 已废弃，但旧配置可能残留
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    // Legacy key (已废弃)：历史版本使用该字段区分 small/fast 模型
    "ANTHROPIC_SMALL_FAST_MODEL",
];

const CLAUDE_PROVIDER_SYNC_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "OPENROUTER_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_REASONING_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ENABLE_TOOL_SEARCH",
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS",
    "API_TIMEOUT_MS",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
];

const CLAUDE_TAKEOVER_HAIKU_MODEL: &str = "claude-haiku-4-5";
const CLAUDE_TAKEOVER_SONNET_MODEL: &str = "claude-sonnet-4-6";
const CLAUDE_TAKEOVER_OPUS_MODEL: &str = "claude-opus-4-7";
// 写给 Claude Code 时沿用文档示例的大写形式；解析侧大小写不敏感。
const CLAUDE_ONE_M_MARKER_FOR_CLIENT: &str = "[1M]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeTakeoverAuthPolicy {
    PreserveExistingOrAuthToken,
    ManagedAccount,
}

#[derive(Clone)]
pub struct ProxyService {
    db: Arc<Database>,
    server: Arc<RwLock<Option<ProxyServer>>>,
    /// AppHandle，用于传递给 ProxyServer 以支持故障转移时的 UI 更新
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    switch_locks: SwitchLockManager,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HotSwitchOutcome {
    pub logical_target_changed: bool,
}

impl ProxyService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            server: Arc::new(RwLock::new(None)),
            app_handle: Arc::new(RwLock::new(None)),
            switch_locks: SwitchLockManager::new(),
        }
    }

    fn merge_claude_provider_settings_into_live(
        live_config: &mut Value,
        provider_settings: &Value,
    ) {
        if !live_config.is_object() {
            *live_config = json!({});
        }

        let root = live_config
            .as_object_mut()
            .expect("Claude live config should be normalized to an object");

        let env = root.entry("env".to_string()).or_insert_with(|| json!({}));
        if !env.is_object() {
            *env = json!({});
        }
        let env = env
            .as_object_mut()
            .expect("Claude live env should be normalized to an object");

        for key in CLAUDE_PROVIDER_SYNC_ENV_KEYS {
            env.remove(*key);
        }

        if let Some(provider_env) = provider_settings.get("env").and_then(Value::as_object) {
            for key in CLAUDE_PROVIDER_SYNC_ENV_KEYS {
                if let Some(value) = provider_env.get(*key) {
                    env.insert((*key).to_string(), value.clone());
                }
            }
        }
    }

    #[cfg(test)]
    fn apply_claude_takeover_fields(config: &mut Value, proxy_url: &str) {
        Self::apply_claude_takeover_fields_with_policy(
            config,
            proxy_url,
            ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken,
        );
    }

    fn apply_claude_takeover_fields_for_provider(
        config: &mut Value,
        proxy_url: &str,
        provider: &Provider,
    ) {
        let auth_policy = if provider.uses_managed_account_auth() {
            ClaudeTakeoverAuthPolicy::ManagedAccount
        } else {
            ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken
        };

        Self::apply_claude_takeover_fields_with_policy(config, proxy_url, auth_policy);
    }

    fn apply_claude_takeover_fields_with_policy(
        config: &mut Value,
        proxy_url: &str,
        auth_policy: ClaudeTakeoverAuthPolicy,
    ) {
        // 必须在 remove/insert 前 snapshot：避免读到自己刚写入的接管别名。
        let takeover_model_fields = Self::build_claude_takeover_model_fields(config);

        if !config.is_object() {
            *config = json!({});
        }

        let root = config
            .as_object_mut()
            .expect("Claude config should be normalized to an object");

        let env = root.entry("env".to_string()).or_insert_with(|| json!({}));
        if !env.is_object() {
            *env = json!({});
        }

        let env = env
            .as_object_mut()
            .expect("Claude env should be normalized to an object");
        env.insert("ANTHROPIC_BASE_URL".to_string(), json!(proxy_url));

        for key in CLAUDE_MODEL_OVERRIDE_ENV_KEYS {
            env.remove(key);
        }

        for (key, value) in takeover_model_fields {
            env.insert(key.to_string(), Value::String(value));
        }

        let token_keys = [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ];

        match auth_policy {
            ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken => {
                let mut replaced_any = false;
                for key in token_keys {
                    if env.contains_key(key) {
                        env.insert(key.to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                        replaced_any = true;
                    }
                }

                if !replaced_any {
                    env.insert(
                        "ANTHROPIC_AUTH_TOKEN".to_string(),
                        json!(PROXY_TOKEN_PLACEHOLDER),
                    );
                }
            }
            ClaudeTakeoverAuthPolicy::ManagedAccount => {
                for key in token_keys {
                    env.remove(key);
                }
                env.insert(
                    "ANTHROPIC_API_KEY".to_string(),
                    json!(PROXY_TOKEN_PLACEHOLDER),
                );
            }
        }
    }

    fn build_claude_takeover_model_fields(config: &Value) -> Vec<(&'static str, String)> {
        let Some(env) = config.get("env").and_then(Value::as_object) else {
            return Vec::new();
        };

        let default_model = Self::claude_env_string(env, "ANTHROPIC_MODEL");
        let small_fast_model = Self::claude_env_string(env, "ANTHROPIC_SMALL_FAST_MODEL");
        let haiku_model = Self::claude_env_string(env, "ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .or(small_fast_model)
            .or(default_model);
        let sonnet_model = Self::claude_env_string(env, "ANTHROPIC_DEFAULT_SONNET_MODEL")
            .or(default_model)
            .or(small_fast_model);
        let opus_model = Self::claude_env_string(env, "ANTHROPIC_DEFAULT_OPUS_MODEL")
            .or(default_model)
            .or(small_fast_model);

        let mut fields = Vec::with_capacity(6);
        Self::push_claude_takeover_role_fields(
            &mut fields,
            env,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            CLAUDE_TAKEOVER_HAIKU_MODEL,
            false,
            haiku_model,
        );
        Self::push_claude_takeover_role_fields(
            &mut fields,
            env,
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            CLAUDE_TAKEOVER_SONNET_MODEL,
            true,
            sonnet_model,
        );
        Self::push_claude_takeover_role_fields(
            &mut fields,
            env,
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            CLAUDE_TAKEOVER_OPUS_MODEL,
            true,
            opus_model,
        );
        fields
    }

    fn push_claude_takeover_role_fields(
        fields: &mut Vec<(&'static str, String)>,
        env: &Map<String, Value>,
        model_key: &'static str,
        name_key: &'static str,
        takeover_model: &'static str,
        supports_one_m: bool,
        upstream_model: Option<&str>,
    ) {
        let Some(upstream_model) = upstream_model else {
            return;
        };

        let mut client_model = takeover_model.to_string();
        if supports_one_m && Self::has_claude_one_m_marker(upstream_model) {
            client_model.push_str(CLAUDE_ONE_M_MARKER_FOR_CLIENT);
        }
        fields.push((model_key, client_model));

        let display_name = Self::claude_env_string(env, name_key)
            .map(str::to_string)
            .unwrap_or_else(|| Self::strip_claude_one_m_marker(upstream_model));
        if !display_name.is_empty() {
            fields.push((name_key, display_name));
        }
    }

    fn claude_env_string<'a>(env: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
        env.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn has_claude_one_m_marker(model: &str) -> bool {
        model
            .trim_end()
            .to_ascii_lowercase()
            .ends_with(crate::claude_desktop_config::ONE_M_CONTEXT_MARKER)
    }

    fn strip_claude_one_m_marker(model: &str) -> String {
        crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model)
            .trim()
            .to_string()
    }

    pub async fn sync_claude_live_from_provider_while_proxy_active(
        &self,
        provider: &Provider,
    ) -> Result<(), String> {
        let effective_settings = build_effective_settings_with_common_config(
            self.db.as_ref(),
            &AppType::Claude,
            provider,
        )
        .map_err(|e| format!("构建 claude 有效配置失败: {e}"))?;
        let (proxy_url, _) = self.build_proxy_urls().await?;

        let mut live_config = self.read_claude_live().unwrap_or_else(|_| json!({}));
        Self::merge_claude_provider_settings_into_live(&mut live_config, &effective_settings);
        Self::apply_claude_takeover_fields_for_provider(&mut live_config, &proxy_url, provider);
        self.write_claude_live(&live_config)?;
        Ok(())
    }

    async fn apply_claude_current_provider_takeover_to_live(
        &self,
        mut live_config: Value,
    ) -> Result<Value, String> {
        let (proxy_url, _) = self.build_proxy_urls().await?;

        if let Some(provider) = self.get_current_provider_for_app(&AppType::Claude)? {
            let effective_settings = build_effective_settings_with_common_config(
                self.db.as_ref(),
                &AppType::Claude,
                &provider,
            )
            .map_err(|e| format!("构建 claude 有效配置失败: {e}"))?;
            Self::merge_claude_provider_settings_into_live(&mut live_config, &effective_settings);
            Self::apply_claude_takeover_fields_for_provider(
                &mut live_config,
                &proxy_url,
                &provider,
            );
        } else {
            Self::apply_claude_takeover_fields_with_policy(
                &mut live_config,
                &proxy_url,
                ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken,
            );
        }

        Ok(live_config)
    }

    fn get_current_provider_for_app(&self, app_type: &AppType) -> Result<Option<Provider>, String> {
        let Some(current_id) = crate::settings::get_effective_current_provider(&self.db, app_type)
            .map_err(|e| format!("获取 {app_type:?} 当前供应商失败: {e}"))?
        else {
            return Ok(None);
        };

        self.db
            .get_provider_by_id(&current_id, app_type.as_str())
            .map_err(|e| format!("读取 {app_type:?} 当前供应商失败: {e}"))
    }

    /// 设置 AppHandle（在应用初始化时调用）
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        futures::executor::block_on(async {
            *self.app_handle.write().await = Some(handle);
        });
    }

    /// 启动代理服务器
    pub async fn start(&self) -> Result<ProxyServerInfo, String> {
        // 1. 启动时自动设置 proxy_enabled = true
        let mut global_config = self
            .db
            .get_global_proxy_config()
            .await
            .map_err(|e| format!("获取全局代理配置失败: {e}"))?;

        if !global_config.proxy_enabled {
            global_config.proxy_enabled = true;
            self.db
                .update_global_proxy_config(global_config.clone())
                .await
                .map_err(|e| format!("更新代理总开关失败: {e}"))?;
        }

        // 2. 获取配置
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 3. 若已在运行：确保持久化状态（如需要）并返回当前信息
        if let Some(server) = self.server.read().await.as_ref() {
            let status = server.get_status().await;
            return Ok(ProxyServerInfo {
                address: status.address,
                port: status.port,
                // 无法精确取回首次启动时间，返回当前时间用于 UI 展示即可
                started_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        // 4. 创建并启动服务器
        let app_handle = self.app_handle.read().await.clone();
        let server = ProxyServer::new(config.clone(), self.db.clone(), app_handle);
        let info = server
            .start()
            .await
            .map_err(|e| format!("启动代理服务器失败: {e}"))?;

        // 5. 保存服务器实例
        *self.server.write().await = Some(server);

        log::info!("代理服务器已启动: {}:{}", info.address, info.port);
        Ok(info)
    }

    /// 启动代理服务器（带 Live 配置接管）
    pub async fn start_with_takeover(&self) -> Result<ProxyServerInfo, String> {
        // 1. 备份各应用的 Live 配置
        self.backup_live_configs().await?;

        // 2. 同步 Live 配置中的 Token 到数据库（确保代理能读到最新的 Token）
        if let Err(e) = self.sync_live_to_providers().await {
            // 同步失败时尚未写入接管配置，但备份可能包含敏感信息，尽量清理
            if let Err(clean_err) = self.db.delete_all_live_backups().await {
                log::warn!("清理 Live 备份失败: {clean_err}");
            }
            return Err(e);
        }

        // 3. 在写入接管配置之前先落盘接管标志：
        //    这样即使在接管过程中断电/kill，下次启动也能检测到并自动恢复。
        if let Err(e) = self.db.set_live_takeover_active(true).await {
            if let Err(clean_err) = self.db.delete_all_live_backups().await {
                log::warn!("清理 Live 备份失败: {clean_err}");
            }
            return Err(format!("设置接管状态失败: {e}"));
        }

        // 4. 接管各应用的 Live 配置（写入代理地址，清空 Token）
        if let Err(e) = self.takeover_live_configs().await {
            // 接管失败（可能是部分写入），尝试恢复原始配置；若恢复失败则保留标志与备份，等待下次启动自动恢复。
            log::error!("接管 Live 配置失败，尝试恢复原始配置: {e}");
            match self.restore_live_configs().await {
                Ok(()) => {
                    let _ = self.db.set_live_takeover_active(false).await;
                    let _ = self.db.delete_all_live_backups().await;
                }
                Err(restore_err) => {
                    log::error!("恢复原始配置失败，将保留备份以便下次启动恢复: {restore_err}");
                }
            }
            return Err(e);
        }

        // 5. 启动代理服务器
        match self.start().await {
            Ok(info) => Ok(info),
            Err(e) => {
                // 启动失败，恢复原始配置
                log::error!("代理启动失败，尝试恢复原始配置: {e}");
                match self.restore_live_configs().await {
                    Ok(()) => {
                        let _ = self.db.set_live_takeover_active(false).await;
                        let _ = self.db.delete_all_live_backups().await;
                    }
                    Err(restore_err) => {
                        log::error!("恢复原始配置失败，将保留备份以便下次启动恢复: {restore_err}");
                    }
                }
                Err(e)
            }
        }
    }

    /// 获取各应用的接管状态（是否改写该应用的 Live 配置指向本地代理）
    pub async fn get_takeover_status(&self) -> Result<ProxyTakeoverStatus, String> {
        // 从 proxy_config.enabled 读取（优先），兼容旧的 live_backup 备份检测
        let claude_enabled = self
            .db
            .get_proxy_config_for_app("claude")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        let codex_enabled = self
            .db
            .get_proxy_config_for_app("codex")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        let gemini_enabled = self
            .db
            .get_proxy_config_for_app("gemini")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        // OpenCode and OpenClaw don't support proxy features, always return false
        let opencode_enabled = false;
        let openclaw_enabled = false;

        Ok(ProxyTakeoverStatus {
            claude: claude_enabled,
            codex: codex_enabled,
            gemini: gemini_enabled,
            opencode: opencode_enabled,
            openclaw: openclaw_enabled,
        })
    }

    /// 为指定应用开启/关闭 Live 接管
    ///
    /// - 开启：自动启动代理服务，仅接管当前 app 的 Live 配置
    /// - 关闭：仅恢复当前 app 的 Live 配置；若无其它接管，则自动停止代理服务
    pub async fn set_takeover_for_app(&self, app_type: &str, enabled: bool) -> Result<(), String> {
        let app = AppType::from_str(app_type).map_err(|e| format!("无效的应用类型: {e}"))?;
        let app_type_str = app.as_str();

        if enabled {
            let current_config = self
                .db
                .get_proxy_config_for_app(app_type_str)
                .await
                .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;

            if let Err(e) = self.preflight_takeover_for_app(&app).await {
                self.recover_failed_takeover_preflight(&app, current_config)
                    .await;
                return Err(e);
            }

            // 1) 代理服务未运行则自动启动
            if !self.is_running().await {
                self.start().await?;
            }

            // 2) 已接管则直接返回（幂等）；但如果缺少备份或占位符残留，需要重建接管
            if current_config.enabled {
                let has_backup = match self.db.get_live_backup(app_type_str).await {
                    Ok(v) => v.is_some(),
                    Err(e) => {
                        log::warn!("读取 {app_type_str} 备份失败（将继续重建接管）: {e}");
                        false
                    }
                };
                let live_taken_over = self.detect_takeover_in_live_config_for_app(&app);

                // 必须 backup AND live 占位符同时存在才算真接管。
                // 只看其一会出现「UI 显示已接管但 Live 已被恢复」或「Live 仍是占位符但备份丢失」
                // 两种脏角落，下面的重建分支会把这些情况修复成一致状态。
                if has_backup && live_taken_over {
                    if matches!(app, AppType::Claude)
                        && !self.claude_takeover_live_matches_current_provider().await?
                    {
                        log::warn!(
                            "{app_type_str} 已接管，但 Live 模型字段与当前 provider 不一致，正在按当前 provider 重新同步"
                        );
                        if let Some(provider) = self.current_provider_for_app(&app)? {
                            self.sync_claude_live_from_provider_while_proxy_active(&provider)
                                .await?;
                        }
                    }
                    return Ok(());
                }

                log::warn!(
                    "{app_type_str} 标记为已接管，但 backup={has_backup} live_taken_over={live_taken_over}，正在重新接管并补齐备份"
                );

                // 旧版本可能留下「无备份但 Live 仍是 PROXY_MANAGED」的坏状态。
                // 不能把这个坏 live 再备份成原始配置；先用当前 provider 写回真实 live，
                // 然后下面的严格备份才会捕获可恢复的原始快照。
                if live_taken_over && !has_backup {
                    match self.restore_live_from_ssot_for_app(&app) {
                        Ok(true) => {
                            log::info!("{app_type_str} 已先从当前 provider 重建 Live，再重新接管");
                        }
                        Ok(false) => {
                            log::warn!(
                                "{app_type_str} 无备份且 Live 被接管，但找不到当前 provider；继续走清理兜底"
                            );
                            self.cleanup_takeover_placeholders_in_live_for_app(&app)?;
                        }
                        Err(e) => {
                            log::warn!(
                                "{app_type_str} 从当前 provider 重建 Live 失败，将清理接管占位符后重试备份: {e}"
                            );
                            self.cleanup_takeover_placeholders_in_live_for_app(&app)?;
                        }
                    }
                }
            }

            // 3) 备份 Live 配置（严格：目标 app 不存在则报错）
            self.backup_live_config_strict(&app).await?;

            // 4) 同步 Live Token 到数据库（仅当前 app）
            if let Err(e) = self.sync_live_to_provider(&app).await {
                let _ = self.db.delete_live_backup(app_type_str).await;
                return Err(e);
            }

            // 5) 写入接管配置（仅当前 app）
            if let Err(e) = self.takeover_live_config_strict(&app).await {
                log::error!("{app_type_str} 接管 Live 配置失败，尝试恢复: {e}");
                match self.restore_live_config_for_app(&app).await {
                    Ok(()) => {
                        // 恢复成功才清理备份，避免失败场景下丢失唯一可回滚来源
                        let _ = self.db.delete_live_backup(app_type_str).await;
                    }
                    Err(restore_err) => {
                        log::error!(
                            "{app_type_str} 恢复 Live 配置失败，将保留备份以便下次启动恢复: {restore_err}"
                        );
                    }
                }
                return Err(e);
            }

            // 6) 设置 proxy_config.enabled = true
            let mut updated_config = self
                .db
                .get_proxy_config_for_app(app_type_str)
                .await
                .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;
            updated_config.enabled = true;
            self.db
                .update_proxy_config_for_app(updated_config)
                .await
                .map_err(|e| format!("设置 {app_type_str} enabled 状态失败: {e}"))?;

            // 7) 兼容旧逻辑：写入 any-of 标志（失败不影响功能）
            let _ = self.db.set_live_takeover_active(true).await;

            // 8) Warn if the current provider is official (risk of account ban via proxy)
            if let Ok(Some(current_id)) =
                crate::settings::get_effective_current_provider(&self.db, &app)
            {
                if let Ok(Some(provider)) = self.db.get_provider_by_id(&current_id, app_type_str) {
                    if provider.category.as_deref() == Some("official") {
                        if let Some(handle) = self.app_handle.read().await.as_ref() {
                            let _ = handle.emit(
                                "proxy-official-warning",
                                serde_json::json!({
                                    "appType": app_type_str,
                                    "providerName": provider.name,
                                }),
                            );
                        }
                    }
                }
            }

            return Ok(());
        }

        // 关闭接管：检查 enabled 状态
        let current_config = self
            .db
            .get_proxy_config_for_app(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;

        if !current_config.enabled {
            return Ok(()); // 未接管，幂等返回
        }

        // 1) 恢复 Live 配置
        //
        // 必须走 with_fallback 版本：备份 → SSOT → 清理占位符 的三层兜底。
        // 简版 restore_live_config_for_app 在备份缺失时会静默 Ok(())，
        // 留下接管时写入的占位符（代理地址/PROXY_MANAGED token），客户端无法工作。
        self.restore_live_config_for_app_with_fallback(&app).await?;

        // 2) 删除该 app 的备份（避免长期存储敏感 Token）
        self.db
            .delete_live_backup(app_type_str)
            .await
            .map_err(|e| format!("删除 {app_type_str} Live 备份失败: {e}"))?;

        // 3) 设置 proxy_config.enabled = false
        let mut updated_config = self
            .db
            .get_proxy_config_for_app(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;
        updated_config.enabled = false;
        self.db
            .update_proxy_config_for_app(updated_config)
            .await
            .map_err(|e| format!("清除 {app_type_str} enabled 状态失败: {e}"))?;

        // 4) 清除该应用的健康状态（关闭代理时重置队列状态）
        self.db
            .clear_provider_health_for_app(app_type_str)
            .await
            .map_err(|e| format!("清除 {app_type_str} 健康状态失败: {e}"))?;

        // 5) 若无其它接管，更新旧标志，并停止代理服务
        // 检查是否还有其它 app 的 enabled = true
        let any_enabled = self
            .db
            .is_live_takeover_active()
            .await
            .map_err(|e| format!("检查接管状态失败: {e}"))?;

        if !any_enabled {
            let _ = self.db.set_live_takeover_active(false).await;

            if self.is_running().await {
                // 此时没有任何 app 处于接管状态，停止服务即可
                let _ = self.stop().await;
            }
        }

        Ok(())
    }

    /// 同步 Live 配置中的 Token 到数据库
    ///
    /// 在清空 Live Token 之前调用，确保数据库中的 Provider 配置有最新的 Token。
    /// 这样代理才能从数据库读取到正确的认证信息。
    async fn sync_live_to_provider(&self, app_type: &AppType) -> Result<(), String> {
        let live_config = match app_type {
            AppType::Claude => self.read_claude_live()?,
            AppType::Codex => self.read_codex_live()?,
            AppType::Gemini => self.read_gemini_live()?,
            _ => return Err("该应用不支持代理功能".to_string()),
        };

        self.sync_live_config_to_provider(app_type, &live_config)
            .await
    }

    async fn sync_live_config_to_provider(
        &self,
        app_type: &AppType,
        live_config: &Value,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::Claude)
                        .map_err(|e| format!("获取 Claude 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "claude")
                    {
                        if Self::is_proxy_only_claude_provider(&provider) {
                            log::debug!(
                                "跳过 proxy-only Claude provider 的 Live Token 反向同步 (provider: {provider_id})"
                            );
                            return Ok(());
                        }

                        if let Some(env) = live_config.get("env").and_then(|v| v.as_object()) {
                            let token_pair = [
                                "ANTHROPIC_AUTH_TOKEN",
                                "ANTHROPIC_API_KEY",
                                "OPENROUTER_API_KEY",
                                "OPENAI_API_KEY",
                            ]
                            .into_iter()
                            .find_map(|key| {
                                env.get(key)
                                    .and_then(|v| v.as_str())
                                    .map(|s| (key, s.trim()))
                            })
                            .filter(|(_, token)| {
                                !token.is_empty() && *token != PROXY_TOKEN_PLACEHOLDER
                            });

                            if let Some((token_key, token)) = token_pair {
                                let env_obj = provider
                                    .settings_config
                                    .get_mut("env")
                                    .and_then(|v| v.as_object_mut());

                                match env_obj {
                                    Some(obj) => {
                                        if token_key == "ANTHROPIC_AUTH_TOKEN"
                                            || token_key == "ANTHROPIC_API_KEY"
                                        {
                                            let mut updated = false;
                                            if obj.contains_key("ANTHROPIC_AUTH_TOKEN") {
                                                obj.insert(
                                                    "ANTHROPIC_AUTH_TOKEN".to_string(),
                                                    json!(token),
                                                );
                                                updated = true;
                                            }
                                            if obj.contains_key("ANTHROPIC_API_KEY") {
                                                obj.insert(
                                                    "ANTHROPIC_API_KEY".to_string(),
                                                    json!(token),
                                                );
                                                updated = true;
                                            }
                                            if !updated {
                                                obj.insert(token_key.to_string(), json!(token));
                                            }
                                        } else {
                                            obj.insert(token_key.to_string(), json!(token));
                                        }
                                    }
                                    None => {
                                        // 至少写入一份可用的 Token
                                        if provider.settings_config.is_null() {
                                            provider.settings_config = json!({});
                                        }

                                        if let Some(root) = provider.settings_config.as_object_mut()
                                        {
                                            root.insert(
                                                "env".to_string(),
                                                json!({ token_key: token }),
                                            );
                                        } else {
                                            log::warn!(
                                                "Claude provider settings_config 格式异常（非对象），跳过写入 Token (provider: {provider_id})"
                                            );
                                        }
                                    }
                                }

                                if let Err(e) = self.db.update_provider_settings_config(
                                    "claude",
                                    &provider_id,
                                    &provider.settings_config,
                                ) {
                                    log::warn!("同步 Claude Token 到数据库失败: {e}");
                                } else {
                                    log::info!(
                                        "已同步 Claude Token 到数据库 (provider: {provider_id})"
                                    );
                                }
                            }
                        }
                    }
                }
            }
            AppType::Codex => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::Codex)
                        .map_err(|e| format!("获取 Codex 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "codex")
                    {
                        if let Some(token) = live_config
                            .get("auth")
                            .and_then(|v| v.get("OPENAI_API_KEY"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty() && *s != PROXY_TOKEN_PLACEHOLDER)
                        {
                            if let Some(auth_obj) = provider
                                .settings_config
                                .get_mut("auth")
                                .and_then(|v| v.as_object_mut())
                            {
                                auth_obj.insert("OPENAI_API_KEY".to_string(), json!(token));
                            } else {
                                if provider.settings_config.is_null() {
                                    provider.settings_config = json!({});
                                }

                                if let Some(root) = provider.settings_config.as_object_mut() {
                                    root.insert(
                                        "auth".to_string(),
                                        json!({ "OPENAI_API_KEY": token }),
                                    );
                                } else {
                                    log::warn!(
                                        "Codex provider settings_config 格式异常（非对象），跳过写入 Token (provider: {provider_id})"
                                    );
                                }
                            }

                            if let Err(e) = self.db.update_provider_settings_config(
                                "codex",
                                &provider_id,
                                &provider.settings_config,
                            ) {
                                log::warn!("同步 Codex Token 到数据库失败: {e}");
                            } else {
                                log::info!("已同步 Codex Token 到数据库 (provider: {provider_id})");
                            }
                        }
                    }
                }
            }
            AppType::Gemini => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::Gemini)
                        .map_err(|e| format!("获取 Gemini 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "gemini")
                    {
                        if let Some(token) = live_config
                            .get("env")
                            .and_then(|v| v.get("GEMINI_API_KEY"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty() && *s != PROXY_TOKEN_PLACEHOLDER)
                        {
                            if let Some(env_obj) = provider
                                .settings_config
                                .get_mut("env")
                                .and_then(|v| v.as_object_mut())
                            {
                                env_obj.insert("GEMINI_API_KEY".to_string(), json!(token));
                            } else {
                                if provider.settings_config.is_null() {
                                    provider.settings_config = json!({});
                                }

                                if let Some(root) = provider.settings_config.as_object_mut() {
                                    root.insert(
                                        "env".to_string(),
                                        json!({ "GEMINI_API_KEY": token }),
                                    );
                                } else {
                                    log::warn!(
                                        "Gemini provider settings_config 格式异常（非对象），跳过写入 Token (provider: {provider_id})"
                                    );
                                }
                            }

                            if let Err(e) = self.db.update_provider_settings_config(
                                "gemini",
                                &provider_id,
                                &provider.settings_config,
                            ) {
                                log::warn!("同步 Gemini Token 到数据库失败: {e}");
                            } else {
                                log::info!(
                                    "已同步 Gemini Token 到数据库 (provider: {provider_id})"
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn is_proxy_only_claude_provider(provider: &Provider) -> bool {
        matches!(
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref()),
            Some("opencode_go_subscription" | "opencode_zen_subscription")
        )
    }

    async fn preflight_takeover_for_app(&self, app_type: &AppType) -> Result<(), String> {
        if !matches!(app_type, AppType::Claude) {
            return Ok(());
        }

        let Some(provider) = self.current_provider_for_app(app_type)? else {
            return Err("Claude 本地代理接管失败：没有选中的 Claude provider。".to_string());
        };
        if !Self::is_proxy_only_claude_provider(&provider) {
            return Ok(());
        }

        let effective_settings =
            build_effective_settings_with_common_config(self.db.as_ref(), app_type, &provider)
                .map_err(|e| format!("构建 Claude 当前供应商配置失败: {e}"))?;

        Self::preflight_opencode_chat_completions(&provider, &effective_settings).await
    }

    async fn recover_failed_takeover_preflight(
        &self,
        app_type: &AppType,
        mut current_config: AppProxyConfig,
    ) {
        let app_type_str = app_type.as_str();
        let live_taken_over = self.detect_takeover_in_live_config_for_app(app_type);
        if current_config.enabled || live_taken_over {
            log::warn!("{app_type_str} 接管预检失败，正在恢复 Live 配置并关闭该应用接管状态");
            if let Err(error) = self
                .restore_live_config_for_app_with_fallback(app_type)
                .await
            {
                log::warn!("{app_type_str} 接管预检失败后的 Live 恢复也失败: {error}");
            }
            current_config.enabled = false;
            if let Err(error) = self.db.update_proxy_config_for_app(current_config).await {
                log::warn!("{app_type_str} 接管预检失败后清除 enabled 状态失败: {error}");
            }
            if let Err(error) = self.db.delete_live_backup(app_type_str).await {
                log::warn!("{app_type_str} 接管预检失败后删除备份失败: {error}");
            }
        }

        match self.is_takeover_active().await {
            Ok(false) => match self.db.get_global_proxy_config().await {
                Ok(mut global_config) if global_config.proxy_enabled => {
                    global_config.proxy_enabled = false;
                    if let Err(error) = self.db.update_global_proxy_config(global_config).await {
                        log::warn!("{app_type_str} 接管预检失败后关闭代理总开关失败: {error}");
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    log::warn!("{app_type_str} 接管预检失败后读取代理总开关失败: {error}");
                }
            },
            Ok(true) => {}
            Err(error) => log::warn!("{app_type_str} 接管预检失败后读取接管状态失败: {error}"),
        }

        match self.db.is_live_takeover_active().await {
            Ok(false) if self.is_running().await => {
                if let Err(error) = self.stop().await {
                    log::warn!("{app_type_str} 接管预检失败后停止空闲代理失败: {error}");
                }
            }
            Ok(_) => {}
            Err(error) => log::warn!("{app_type_str} 接管预检失败后读取接管状态失败: {error}"),
        }
    }

    async fn preflight_opencode_chat_completions(
        provider: &Provider,
        effective_settings: &Value,
    ) -> Result<(), String> {
        let env = effective_settings
            .get("env")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                format!(
                    "OpenCode provider `{}` 不能接管：缺少 env 配置。",
                    provider.name
                )
            })?;
        let base_url = env
            .get("ANTHROPIC_BASE_URL")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                format!(
                    "OpenCode provider `{}` 不能接管：缺少 ANTHROPIC_BASE_URL。",
                    provider.name
                )
            })?;
        let api_key = env
            .get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| env.get("ANTHROPIC_API_KEY"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                format!(
                    "OpenCode provider `{}` 不能接管：缺少 API key。",
                    provider.name
                )
            })?;
        let model = Self::claude_preflight_model(effective_settings).ok_or_else(|| {
            format!(
                "OpenCode provider `{}` 不能接管：缺少可用于 Claude Code 的模型配置。",
                provider.name
            )
        })?;
        let model = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(&model).to_string();
        let url = Self::openai_chat_completions_url(base_url);
        let mut client_builder =
            reqwest::Client::builder().timeout(std::time::Duration::from_secs(20));
        if url.starts_with("http://127.0.0.1:")
            || url.starts_with("http://localhost:")
            || url.starts_with("http://[::1]:")
        {
            client_builder = client_builder.no_proxy();
        }
        let client = client_builder
            .build()
            .map_err(|e| format!("OpenCode 接管预检失败：创建 HTTP 客户端失败: {e}"))?;
        let response = client
            .post(&url)
            .bearer_auth(api_key)
            .json(&json!({
                "model": model,
                "stream": false,
                "messages": [
                    { "role": "user", "content": "Reply exactly ok." }
                ],
                "max_tokens": 64
            }))
            .send()
            .await
            .map_err(|e| {
                format!(
                    "OpenCode 接管预检失败：无法连接 `{}`。请检查端点、网络或订阅状态。详情: {e}",
                    Self::redact_url_for_log(&url)
                )
            })?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "OpenCode 接管预检失败：上游返回 HTTP {}。未写入本地代理配置，请先修复 API key、endpoint 或模型。详情: {}",
                status.as_u16(),
                Self::redact_preflight_body(&body, api_key)
            ));
        }

        let parsed: Value = serde_json::from_str(&body).map_err(|e| {
            format!(
                "OpenCode 接管预检失败：上游返回的不是合法 JSON。详情: {e}; body={}",
                Self::redact_preflight_body(&body, api_key)
            )
        })?;
        if !Self::chat_completion_has_usable_output(&parsed) {
            return Err(format!(
                "OpenCode 接管预检失败：上游 2xx 但没有返回 chat completion 内容。详情: {}",
                Self::redact_preflight_body(&body, api_key)
            ));
        }

        Ok(())
    }

    fn chat_completion_has_usable_output(parsed: &Value) -> bool {
        let Some(message) = parsed.pointer("/choices/0/message") else {
            return false;
        };

        Self::value_has_non_empty_text(message.get("content"))
            || Self::value_has_non_empty_text(message.get("reasoning_content"))
            || Self::value_has_non_empty_text(message.get("reasoning"))
            || message
                .get("tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
    }

    fn value_has_non_empty_text(value: Option<&Value>) -> bool {
        match value {
            Some(Value::String(text)) => !text.trim().is_empty(),
            Some(Value::Array(items)) => items.iter().any(|item| match item {
                Value::String(text) => !text.trim().is_empty(),
                Value::Object(object) => object
                    .get("text")
                    .or_else(|| object.get("content"))
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty()),
                _ => false,
            }),
            _ => false,
        }
    }

    fn claude_preflight_model(settings: &Value) -> Option<String> {
        let env = settings.get("env").and_then(Value::as_object)?;
        [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
        ]
        .into_iter()
        .find_map(|key| {
            env.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
    }

    fn openai_chat_completions_url(base_url: &str) -> String {
        let base = base_url.trim().trim_end_matches('/');
        if base.to_ascii_lowercase().ends_with("/chat/completions") {
            base.to_string()
        } else if base.to_ascii_lowercase().ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }

    fn redact_url_for_log(url: &str) -> String {
        url.split('?').next().unwrap_or(url).to_string()
    }

    fn redact_preflight_body(body: &str, api_key: &str) -> String {
        let redacted = body.replace(api_key, "<redacted>");
        redacted.chars().take(800).collect()
    }

    async fn sync_live_to_providers(&self) -> Result<(), String> {
        if let Ok(live_config) = self.read_claude_live() {
            self.sync_live_config_to_provider(&AppType::Claude, &live_config)
                .await?;
        }

        if let Ok(live_config) = self.read_codex_live() {
            self.sync_live_config_to_provider(&AppType::Codex, &live_config)
                .await?;
        }

        if let Ok(live_config) = self.read_gemini_live() {
            self.sync_live_config_to_provider(&AppType::Gemini, &live_config)
                .await?;
        }

        log::info!("Live 配置 Token 同步完成");
        Ok(())
    }

    /// 停止代理服务器
    pub async fn stop(&self) -> Result<(), String> {
        if let Some(server) = self.server.write().await.take() {
            server
                .stop()
                .await
                .map_err(|e| format!("停止代理服务器失败: {e}"))?;

            // 停止时设置 proxy_enabled = false
            let mut global_config = self
                .db
                .get_global_proxy_config()
                .await
                .map_err(|e| format!("获取全局代理配置失败: {e}"))?;

            if global_config.proxy_enabled {
                global_config.proxy_enabled = false;
                if let Err(e) = self.db.update_global_proxy_config(global_config).await {
                    log::warn!("更新代理总开关失败: {e}");
                }
            }

            log::info!("代理服务器已停止");
            Ok(())
        } else {
            Err("代理服务器未运行".to_string())
        }
    }

    /// 停止代理服务器（恢复 Live 配置，用户手动关闭时使用）
    ///
    /// 会清除 settings 表中的代理状态，下次启动不会自动恢复。
    pub async fn stop_with_restore(&self) -> Result<(), String> {
        // 1. 停止代理服务器（即使未运行也继续执行恢复逻辑）
        if let Err(e) = self.stop().await {
            log::warn!("停止代理服务器失败（将继续恢复 Live 配置）: {e}");
        }

        // 2. 恢复原始 Live 配置
        self.restore_live_configs().await?;

        // 3. 清除 proxy_config 表中的接管状态（兼容旧版）
        self.db
            .set_live_takeover_active(false)
            .await
            .map_err(|e| format!("清除接管状态失败: {e}"))?;

        // 4. 清除所有应用的 enabled 状态（用户手动关闭，不需要下次自动恢复）
        for app_type in ["claude", "codex", "gemini"] {
            if let Ok(mut config) = self.db.get_proxy_config_for_app(app_type).await {
                if config.enabled {
                    config.enabled = false;
                    if let Err(e) = self.db.update_proxy_config_for_app(config).await {
                        log::warn!("清除 {app_type} enabled 状态失败: {e}");
                    }
                }
            }
        }

        // 5. 删除备份
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        // 6. 重置健康状态（让健康徽章恢复为正常）
        self.db
            .clear_all_provider_health()
            .await
            .map_err(|e| format!("重置健康状态失败: {e}"))?;

        // 注意：不清除故障转移队列和开关状态，保留供下次开启代理时使用
        log::info!("代理已停止，Live 配置已恢复");
        Ok(())
    }

    /// 停止代理服务器（恢复 Live 配置，但保留 settings 表中的代理状态）
    ///
    /// 用于程序正常退出时，保留代理状态以便下次启动时自动恢复
    pub async fn stop_with_restore_keep_state(&self) -> Result<(), String> {
        // 1. 停止代理服务器（即使未运行也继续执行恢复逻辑）
        if let Err(e) = self.stop().await {
            log::warn!("停止代理服务器失败（将继续恢复 Live 配置）: {e}");
        }

        // 2. 恢复原始 Live 配置
        self.restore_live_configs().await?;

        // 3. 更新 proxy_config 表中的 live_takeover_active 标志（兼容旧版）
        //    注意：保留 proxy_config.enabled 状态，下次启动时自动恢复
        if let Ok(mut config) = self.db.get_proxy_config().await {
            config.live_takeover_active = false;
            let _ = self.db.update_proxy_config(config).await;
        }

        // 4. 删除备份（Live 配置已恢复，备份不再需要）
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        // 5. 重置健康状态
        self.db
            .clear_all_provider_health()
            .await
            .map_err(|e| format!("重置健康状态失败: {e}"))?;

        log::info!("代理已停止，Live 配置已恢复（保留代理状态，下次启动将自动恢复）");
        Ok(())
    }

    /// 备份各应用的 Live 配置
    async fn backup_live_configs(&self) -> Result<(), String> {
        // Claude
        if let Ok(config) = self.read_claude_live() {
            self.save_live_backup_if_absent("claude", &config).await?;
        }

        // Codex
        if let Ok(config) = self.read_codex_live() {
            self.save_live_backup_if_absent("codex", &config).await?;
        }

        // Gemini
        if let Ok(config) = self.read_gemini_live() {
            self.save_live_backup_if_absent("gemini", &config).await?;
        }

        log::info!("已备份所有应用的 Live 配置");
        Ok(())
    }

    /// 备份指定应用的 Live 配置（严格模式：目标配置不存在则返回错误）
    async fn backup_live_config_strict(&self, app_type: &AppType) -> Result<(), String> {
        let (app_type_str, config) = match app_type {
            AppType::Claude => ("claude", self.read_claude_live()?),
            AppType::Codex => ("codex", self.read_codex_live()?),
            AppType::Gemini => ("gemini", self.read_gemini_live()?),
            _ => return Err("该应用不支持代理功能".to_string()),
        };

        self.save_live_backup_if_absent(app_type_str, &config).await
    }

    async fn save_live_backup_if_absent(
        &self,
        app_type_str: &str,
        config: &Value,
    ) -> Result<(), String> {
        if self
            .db
            .get_live_backup(app_type_str)
            .await
            .map_err(|e| format!("读取 {app_type_str} Live 备份失败: {e}"))?
            .is_some()
        {
            log::debug!("{app_type_str} Live 备份已存在，保留原始快照");
            return Ok(());
        }

        let json_str = serde_json::to_string(config)
            .map_err(|e| format!("序列化 {app_type_str} 配置失败: {e}"))?;
        self.db
            .save_live_backup(app_type_str, &json_str)
            .await
            .map_err(|e| format!("备份 {app_type_str} 配置失败: {e}"))?;

        Ok(())
    }

    /// 构造写入 Live 的代理地址（处理 0.0.0.0 / IPv6 等特殊情况）
    async fn build_proxy_urls(&self) -> Result<(String, String), String> {
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // listen_address 可能是 0.0.0.0（用于监听所有网卡），但客户端无法用 0.0.0.0 连接；
        // 因此写回到各应用配置时，优先使用本机回环地址。
        let connect_host = match config.listen_address.as_str() {
            "0.0.0.0" => "127.0.0.1".to_string(),
            "::" => "::1".to_string(),
            _ => config.listen_address.clone(),
        };
        let connect_host_for_url = if connect_host.contains(':') && !connect_host.starts_with('[') {
            format!("[{connect_host}]")
        } else {
            connect_host
        };

        let proxy_origin = format!("http://{}:{}", connect_host_for_url, config.listen_port);
        let proxy_url = proxy_origin.clone();
        let proxy_codex_base_url = format!("{}/v1", proxy_origin.trim_end_matches('/'));

        Ok((proxy_url, proxy_codex_base_url))
    }

    /// 接管各应用的 Live 配置（写入代理地址）
    ///
    /// 代理服务器的路由已经根据 API 端点自动区分应用类型：
    /// - `/v1/messages` → Claude
    /// - `/v1/chat/completions`, `/v1/responses` → Codex
    /// - `/v1beta/*` → Gemini
    ///
    /// 因此不需要在 URL 中添加应用前缀。
    async fn takeover_live_configs(&self) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;

        // Claude: 修改 ANTHROPIC_BASE_URL，使用占位符替代真实 Token（代理会注入真实 Token）
        if let Ok(live_config) = self.read_claude_live() {
            let live_config = self
                .apply_claude_current_provider_takeover_to_live(live_config)
                .await?;
            self.write_claude_live(&live_config)?;
            log::info!("Claude Live 配置已接管，代理地址: {proxy_url}");
        }

        // Codex: 修改 config.toml 的 base_url，auth.json 的 OPENAI_API_KEY（代理会注入真实 Token）
        if let Ok(mut live_config) = self.read_codex_live() {
            // 1. 修改 auth.json 中的 OPENAI_API_KEY（使用占位符）
            if let Some(auth) = live_config.get_mut("auth").and_then(|v| v.as_object_mut()) {
                auth.insert("OPENAI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
            }

            // 2. 修改 config.toml 中的 base_url
            let config_str = live_config
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let updated_config =
                Self::apply_codex_proxy_toml_config(config_str, &proxy_codex_base_url);
            live_config["config"] = json!(updated_config);

            self.write_codex_live(&live_config)?;
            log::info!("Codex Live 配置已接管，代理地址: {proxy_codex_base_url}");
        }

        // Gemini: 修改 GOOGLE_GEMINI_BASE_URL，使用占位符替代真实 Token（代理会注入真实 Token）
        if let Ok(mut live_config) = self.read_gemini_live() {
            if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                // 使用占位符，避免显示缺少 key 的警告
                env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
            } else {
                live_config["env"] = json!({
                    "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                    "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                });
            }
            self.write_gemini_live(&live_config)?;
            log::info!("Gemini Live 配置已接管，代理地址: {proxy_url}");
        }

        Ok(())
    }

    /// 接管指定应用的 Live 配置（严格模式：目标配置不存在则返回错误）
    async fn takeover_live_config_strict(&self, app_type: &AppType) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;

        match app_type {
            AppType::Claude => {
                let live_config = self.read_claude_live()?;
                let live_config = self
                    .apply_claude_current_provider_takeover_to_live(live_config)
                    .await?;
                self.write_claude_live(&live_config)?;
                log::info!("Claude Live 配置已接管，代理地址: {proxy_url}");
            }
            AppType::Codex => {
                let mut live_config = self.read_codex_live()?;

                if let Some(auth) = live_config.get_mut("auth").and_then(|v| v.as_object_mut()) {
                    auth.insert("OPENAI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                }

                let config_str = live_config
                    .get("config")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let updated_config =
                    Self::apply_codex_proxy_toml_config(config_str, &proxy_codex_base_url);
                live_config["config"] = json!(updated_config);

                self.write_codex_live(&live_config)?;
                log::info!("Codex Live 配置已接管，代理地址: {proxy_codex_base_url}");
            }
            AppType::Gemini => {
                let mut live_config = self.read_gemini_live()?;

                if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                    env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                    env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                } else {
                    live_config["env"] = json!({
                        "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                        "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                    });
                }

                self.write_gemini_live(&live_config)?;
                log::info!("Gemini Live 配置已接管，代理地址: {proxy_url}");
            }
            _ => return Err("该应用不支持代理功能".to_string()),
        }

        Ok(())
    }

    /// 接管指定应用的 Live 配置（尽力而为：配置不存在/读取失败则跳过）
    async fn takeover_live_config_best_effort(&self, app_type: &AppType) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;

        match app_type {
            AppType::Claude => {
                if let Ok(live_config) = self.read_claude_live() {
                    let live_config = self
                        .apply_claude_current_provider_takeover_to_live(live_config)
                        .await?;
                    let _ = self.write_claude_live(&live_config);
                }
            }
            AppType::Codex => {
                if let Ok(mut live_config) = self.read_codex_live() {
                    if let Some(auth) = live_config.get_mut("auth").and_then(|v| v.as_object_mut())
                    {
                        auth.insert("OPENAI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                    }

                    let config_str = live_config
                        .get("config")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let updated_config =
                        Self::apply_codex_proxy_toml_config(config_str, &proxy_codex_base_url);
                    live_config["config"] = json!(updated_config);

                    let _ = self.write_codex_live(&live_config);
                }
            }
            AppType::Gemini => {
                if let Ok(mut live_config) = self.read_gemini_live() {
                    if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                        env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                        env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                    } else {
                        live_config["env"] = json!({
                            "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                            "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                        });
                    }

                    let _ = self.write_gemini_live(&live_config);
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// 恢复指定应用的 Live 配置（若无备份则不做任何操作）
    async fn restore_live_config_for_app(&self, app_type: &AppType) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.restore_live_config_for_app_inner(app_type).await
    }

    async fn restore_live_config_for_app_inner(&self, app_type: &AppType) -> Result<(), String> {
        match app_type {
            AppType::Claude => {
                if let Ok(Some(backup)) = self.db.get_live_backup("claude").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Claude 备份失败: {e}"))?;
                    self.write_claude_live(&config)?;
                    log::info!("Claude Live 配置已恢复");
                }
            }
            AppType::Codex => {
                if let Ok(Some(backup)) = self.db.get_live_backup("codex").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Codex 备份失败: {e}"))?;
                    self.write_codex_live(&config)?;
                    log::info!("Codex Live 配置已恢复");
                }
            }
            AppType::Gemini => {
                if let Ok(Some(backup)) = self.db.get_live_backup("gemini").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Gemini 备份失败: {e}"))?;
                    self.write_gemini_live(&config)?;
                    log::info!("Gemini Live 配置已恢复");
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// 恢复原始 Live 配置
    async fn restore_live_configs(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        for app_type in [AppType::Claude, AppType::Codex, AppType::Gemini] {
            if let Err(e) = self
                .restore_live_config_for_app_with_fallback(&app_type)
                .await
            {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }

    async fn restore_live_config_for_app_with_fallback(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.restore_live_config_for_app_with_fallback_inner(app_type)
            .await
    }

    async fn restore_live_config_for_app_with_fallback_inner(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let app_type_str = app_type.as_str();

        // 1) 优先从 Live 备份恢复（这是"原始 Live"的唯一可靠来源）
        let backup = self
            .db
            .get_live_backup(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} Live 备份失败: {e}"))?;
        if let Some(backup) = backup {
            let config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 {app_type_str} 备份失败: {e}"))?;
            self.write_live_config_for_app(app_type, &config)?;
            log::info!("{app_type_str} Live 配置已从备份恢复");
            return Ok(());
        }

        // 2) 兜底：备份缺失，但 Live 仍包含接管占位符（异常退出/历史 bug 场景）
        if !self.detect_takeover_in_live_config_for_app(app_type) {
            return Ok(());
        }

        // 2.1) 优先从 SSOT（当前供应商）重建 Live（比"清理字段"更可用）
        match self.restore_live_from_ssot_for_app(app_type) {
            Ok(true) => {
                log::info!("{app_type_str} Live 配置已从 SSOT 恢复（无备份兜底）");
                return Ok(());
            }
            Ok(false) => {
                log::warn!(
                    "{app_type_str} Live 备份缺失，且无法从 SSOT 恢复，将尝试清理接管占位符"
                );
            }
            Err(e) => {
                log::error!(
                    "{app_type_str} Live 备份缺失，SSOT 恢复失败，将尝试清理接管占位符: {e}"
                );
            }
        }

        // 2.2) 最后兜底：尽力清理占位符与本地代理地址，避免长期卡在代理占位符状态
        self.cleanup_takeover_placeholders_in_live_for_app(app_type)?;
        log::info!("{app_type_str} Live 接管占位符已清理（无备份兜底）");
        Ok(())
    }

    fn write_live_config_for_app(&self, app_type: &AppType, config: &Value) -> Result<(), String> {
        match app_type {
            AppType::Claude => self.write_claude_live(config),
            AppType::Codex => self.write_codex_live(config),
            AppType::Gemini => self.write_gemini_live(config),
            _ => Err("该应用不支持代理功能".to_string()),
        }
    }

    pub fn detect_takeover_in_live_config_for_app(&self, app_type: &AppType) -> bool {
        match app_type {
            AppType::Claude => match self.read_claude_live() {
                Ok(config) => Self::is_claude_live_taken_over(&config),
                Err(_) => false,
            },
            AppType::Codex => match self.read_codex_live() {
                Ok(config) => Self::is_codex_live_taken_over(&config),
                Err(_) => false,
            },
            AppType::Gemini => match self.read_gemini_live() {
                Ok(config) => Self::is_gemini_live_taken_over(&config),
                Err(_) => false,
            },
            _ => false,
        }
    }

    fn current_provider_for_app(&self, app_type: &AppType) -> Result<Option<Provider>, String> {
        let current_id = crate::settings::get_effective_current_provider(&self.db, app_type)
            .map_err(|e| format!("获取 {app_type:?} 当前供应商失败: {e}"))?;

        let Some(current_id) = current_id else {
            return Ok(None);
        };

        self.db
            .get_provider_by_id(&current_id, app_type.as_str())
            .map_err(|e| format!("读取 {app_type:?} 当前供应商失败: {e}"))
    }

    async fn claude_takeover_live_matches_current_provider(&self) -> Result<bool, String> {
        let Some(provider) = self.current_provider_for_app(&AppType::Claude)? else {
            return Ok(false);
        };

        let live = self.read_claude_live()?;
        let live_env = match live.get("env").and_then(Value::as_object) {
            Some(env) => env,
            None => return Ok(false),
        };

        let mut expected = build_effective_settings_with_common_config(
            self.db.as_ref(),
            &AppType::Claude,
            &provider,
        )
        .map_err(|e| format!("构建 Claude 当前供应商配置失败: {e}"))?;
        let (proxy_url, _) = self.build_proxy_urls().await?;
        Self::apply_claude_takeover_fields(&mut expected, &proxy_url);
        let expected_env = match expected.get("env").and_then(Value::as_object) {
            Some(env) => env,
            None => return Ok(false),
        };

        for key in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            "ENABLE_TOOL_SEARCH",
            "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS",
            "API_TIMEOUT_MS",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
        ] {
            if live_env.get(key) != expected_env.get(key) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// 当 Live 备份缺失时，尝试用 SSOT（当前供应商）写回 Live，以解除占位符接管。
    ///
    /// 返回值：
    /// - Ok(true)：已成功写回
    /// - Ok(false)：缺少当前供应商/供应商不存在，无法写回
    fn restore_live_from_ssot_for_app(&self, app_type: &AppType) -> Result<bool, String> {
        let current_id = crate::settings::get_effective_current_provider(&self.db, app_type)
            .map_err(|e| format!("获取 {app_type:?} 当前供应商失败: {e}"))?;

        let Some(current_id) = current_id else {
            return Ok(false);
        };

        let providers = self
            .db
            .get_all_providers(app_type.as_str())
            .map_err(|e| format!("读取 {app_type:?} 供应商列表失败: {e}"))?;

        let Some(provider) = providers.get(&current_id) else {
            return Ok(false);
        };

        if matches!(app_type, AppType::Claude) {
            let effective_settings =
                build_effective_settings_with_common_config(self.db.as_ref(), app_type, provider)
                    .map_err(|e| format!("构建 {app_type:?} Live 配置失败: {e}"))?;
            self.write_live_config_for_app(app_type, &effective_settings)?;
        } else {
            write_live_with_common_config(self.db.as_ref(), app_type, provider)
                .map_err(|e| format!("写入 {app_type:?} Live 配置失败: {e}"))?;
        }

        Ok(true)
    }

    fn cleanup_takeover_placeholders_in_live_for_app(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => self.cleanup_claude_takeover_placeholders_in_live(),
            AppType::Codex => self.cleanup_codex_takeover_placeholders_in_live(),
            AppType::Gemini => self.cleanup_gemini_takeover_placeholders_in_live(),
            _ => Ok(()),
        }
    }

    fn is_local_proxy_url(url: &str) -> bool {
        let url = url.trim();
        if !url.starts_with("http://") {
            return false;
        }
        let rest = &url["http://".len()..];
        rest.starts_with("127.0.0.1")
            || rest.starts_with("localhost")
            || rest.starts_with("0.0.0.0")
            || rest.starts_with("[::1]")
            || rest.starts_with("[::]")
            || rest.starts_with("::1")
            || rest.starts_with("::")
    }

    fn cleanup_claude_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_claude_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
        };

        for key in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ] {
            if env.get(key).and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
                env.remove(key);
            }
        }

        if env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .map(Self::is_local_proxy_url)
            .unwrap_or(false)
        {
            env.remove("ANTHROPIC_BASE_URL");
        }

        self.write_claude_live(&config)?;
        Ok(())
    }

    fn cleanup_codex_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_codex_live()?;

        if let Some(auth) = config.get_mut("auth").and_then(|v| v.as_object_mut()) {
            if auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
            {
                auth.remove("OPENAI_API_KEY");
            }
        }

        if let Some(cfg_str) = config.get("config").and_then(|v| v.as_str()) {
            let updated = Self::remove_local_toml_base_url(cfg_str);
            config["config"] = json!(updated);
        }

        self.write_codex_live(&config)?;
        Ok(())
    }

    /// Remove local proxy base_url from TOML（委托给 codex_config 共享实现）
    fn remove_local_toml_base_url(toml_str: &str) -> String {
        crate::codex_config::remove_codex_toml_base_url_if(toml_str, Self::is_local_proxy_url)
    }

    fn cleanup_gemini_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_gemini_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
        };

        if env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
            env.remove("GEMINI_API_KEY");
        }

        if env
            .get("GOOGLE_GEMINI_BASE_URL")
            .and_then(|v| v.as_str())
            .map(Self::is_local_proxy_url)
            .unwrap_or(false)
        {
            env.remove("GOOGLE_GEMINI_BASE_URL");
        }

        self.write_gemini_live(&config)?;
        Ok(())
    }

    /// 检查是否处于 Live 接管模式
    pub async fn is_takeover_active(&self) -> Result<bool, String> {
        let status = self.get_takeover_status().await?;
        Ok(status.claude || status.codex || status.gemini)
    }

    /// 从异常退出中恢复（启动时调用）
    ///
    /// 检测到 Live 备份残留时调用此方法。
    /// 会恢复 Live 配置、清除接管标志、删除备份。
    pub async fn recover_from_crash(&self) -> Result<(), String> {
        // 1. 恢复 Live 配置
        self.restore_live_configs().await?;

        // 2. 清除接管标志
        self.db
            .set_live_takeover_active(false)
            .await
            .map_err(|e| format!("清除接管状态失败: {e}"))?;

        // 3. 删除备份
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        log::info!("已从异常退出中恢复 Live 配置");
        Ok(())
    }

    /// 检测 Live 配置是否处于"被接管"的残留状态
    ///
    /// 用于兜底处理：当数据库备份缺失但 Live 文件已经写成代理占位符时，
    /// 启动流程可以据此触发恢复逻辑。
    pub fn detect_takeover_in_live_configs(&self) -> bool {
        if let Ok(config) = self.read_claude_live() {
            if Self::is_claude_live_taken_over(&config) {
                return true;
            }
        }

        if let Ok(config) = self.read_codex_live() {
            if Self::is_codex_live_taken_over(&config) {
                return true;
            }
        }

        if let Ok(config) = self.read_gemini_live() {
            if Self::is_gemini_live_taken_over(&config) {
                return true;
            }
        }

        false
    }

    fn is_claude_live_taken_over(config: &Value) -> bool {
        let env = match config.get("env").and_then(|v| v.as_object()) {
            Some(env) => env,
            None => return false,
        };

        for key in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ] {
            if env.get(key).and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
                return true;
            }
        }

        false
    }

    fn is_codex_live_taken_over(config: &Value) -> bool {
        let auth = match config.get("auth").and_then(|v| v.as_object()) {
            Some(auth) => auth,
            None => return false,
        };
        auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    fn is_gemini_live_taken_over(config: &Value) -> bool {
        let env = match config.get("env").and_then(|v| v.as_object()) {
            Some(env) => env,
            None => return false,
        };
        env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    /// 从供应商配置更新 Live 备份（用于代理模式下的热切换）
    ///
    /// 与 backup_live_configs() 不同，此方法从供应商的 settings_config 生成备份，
    /// 而不是从 Live 文件读取（因为 Live 文件已被代理接管）。
    pub async fn update_live_backup_from_provider(
        &self,
        app_type: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type).await;
        self.update_live_backup_from_provider_inner(app_type, provider)
            .await
    }

    /// 仅供已持有 per-app 切换锁的调用方使用。
    async fn update_live_backup_from_provider_inner(
        &self,
        app_type: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("未知的应用类型: {app_type}"))?;
        let mut effective_settings =
            build_effective_settings_with_common_config(self.db.as_ref(), &app_type_enum, provider)
                .map_err(|e| format!("构建 {app_type} 有效配置失败: {e}"))?;

        if matches!(app_type_enum, AppType::Codex) {
            let existing_backup_value = self
                .db
                .get_live_backup(app_type)
                .await
                .map_err(|e| format!("读取 {app_type} 现有备份失败: {e}"))?
                .map(|backup| {
                    serde_json::from_str::<Value>(&backup.original_config)
                        .map_err(|e| format!("解析 {app_type} 现有备份失败: {e}"))
                })
                .transpose()?;

            if let Some(existing_value) = existing_backup_value.as_ref() {
                Self::preserve_codex_mcp_servers_in_backup(
                    &mut effective_settings,
                    existing_value,
                )?;
            }

            let anchor_config_text = existing_backup_value
                .as_ref()
                .and_then(|value| value.get("config"))
                .and_then(|value| value.as_str());
            crate::codex_config::normalize_codex_settings_config_model_provider(
                &mut effective_settings,
                anchor_config_text,
            )
            .map_err(|e| format!("归一化 Codex restore backup 失败: {e}"))?;
        }

        let backup_json = match app_type_enum {
            AppType::Claude => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Claude 配置失败: {e}"))?,
            AppType::Codex => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Codex 配置失败: {e}"))?,
            AppType::Gemini => {
                // Gemini takeover 仅修改 .env；settings.json（含 mcpServers）保持原样。
                let env_backup = if let Some(env) = effective_settings.get("env") {
                    json!({ "env": env })
                } else {
                    json!({ "env": {} })
                };
                serde_json::to_string(&env_backup)
                    .map_err(|e| format!("序列化 Gemini 配置失败: {e}"))?
            }
            _ => return Err(format!("未知的应用类型: {app_type}")),
        };

        self.db
            .save_live_backup(app_type, &backup_json)
            .await
            .map_err(|e| format!("更新 {app_type} 备份失败: {e}"))?;

        log::info!("已更新 {app_type} Live 备份（热切换）");
        Ok(())
    }

    pub async fn hot_switch_provider(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<HotSwitchOutcome, String> {
        let _guard = self.switch_locks.lock_for_app(app_type).await;

        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("无效的应用类型: {app_type}"))?;
        let provider = self
            .db
            .get_provider_by_id(provider_id, app_type)
            .map_err(|e| format!("读取供应商失败: {e}"))?
            .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;

        // Defense-in-depth: block official providers during proxy takeover
        if provider.category.as_deref() == Some("official") {
            return Err(
                "代理接管模式下不能切换到官方供应商 (Cannot switch to official provider during proxy takeover)"
                    .to_string(),
            );
        }

        let logical_target_changed =
            crate::settings::get_effective_current_provider(&self.db, &app_type_enum)
                .map_err(|e| format!("读取当前供应商失败: {e}"))?
                .as_deref()
                != Some(provider_id);

        let has_backup = self
            .db
            .get_live_backup(app_type_enum.as_str())
            .await
            .map_err(|e| format!("读取 {app_type} 备份失败: {e}"))?
            .is_some();
        let live_taken_over = self.detect_takeover_in_live_config_for_app(&app_type_enum);
        let should_sync_proxy_live = has_backup || live_taken_over;

        self.db
            .set_current_provider(app_type_enum.as_str(), provider_id)
            .map_err(|e| format!("更新当前供应商失败: {e}"))?;
        crate::settings::set_current_provider(&app_type_enum, Some(provider_id))
            .map_err(|e| format!("更新本地当前供应商失败: {e}"))?;

        if should_sync_proxy_live && matches!(app_type_enum, AppType::Claude) {
            self.sync_claude_live_from_provider_while_proxy_active(&provider)
                .await?;
        }

        if let Some(server) = self.server.read().await.as_ref() {
            server
                .set_active_target(app_type_enum.as_str(), &provider.id, &provider.name)
                .await;
        }

        Ok(HotSwitchOutcome {
            logical_target_changed,
        })
    }

    #[cfg(test)]
    async fn lock_switch_for_test(&self, app_type: &str) -> tokio::sync::OwnedMutexGuard<()> {
        self.switch_locks.lock_for_app(app_type).await
    }

    fn preserve_codex_mcp_servers_in_backup(
        target_settings: &mut Value,
        existing_backup: &Value,
    ) -> Result<(), String> {
        let target_obj = target_settings
            .as_object_mut()
            .ok_or_else(|| "Codex 备份必须是 JSON 对象".to_string())?;

        let target_config = target_obj
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mut target_doc = if target_config.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            target_config
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析新的 Codex config.toml 失败: {e}"))?
        };

        let existing_config = existing_backup
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if existing_config.trim().is_empty() {
            target_obj.insert("config".to_string(), json!(target_doc.to_string()));
            return Ok(());
        }

        let existing_doc = existing_config
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("解析现有 Codex 备份失败: {e}"))?;

        if let Some(existing_mcp_servers) = existing_doc.get("mcp_servers") {
            match target_doc.get_mut("mcp_servers") {
                Some(target_mcp_servers) => {
                    if let (Some(target_table), Some(existing_table)) = (
                        target_mcp_servers.as_table_like_mut(),
                        existing_mcp_servers.as_table_like(),
                    ) {
                        for (server_id, server_item) in existing_table.iter() {
                            if target_table.get(server_id).is_none() {
                                target_table.insert(server_id, server_item.clone());
                            }
                        }
                    } else {
                        log::warn!(
                            "Codex config contains a non-table mcp_servers section; skipping backup MCP merge"
                        );
                    }
                }
                None => {
                    target_doc["mcp_servers"] = existing_mcp_servers.clone();
                }
            }
        }

        target_obj.insert("config".to_string(), json!(target_doc.to_string()));
        Ok(())
    }

    /// 代理模式下切换供应商（热切换，不写 Live）
    pub async fn switch_proxy_target(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), String> {
        let outcome = self.hot_switch_provider(app_type, provider_id).await?;

        if outcome.logical_target_changed {
            log::info!("代理模式：已切换 {app_type} 的目标供应商为 {provider_id}");
        } else {
            log::debug!("代理模式：{app_type} 已对齐到目标供应商 {provider_id}");
        }
        Ok(())
    }

    // ==================== Live 配置读写辅助方法 ====================

    /// 更新 TOML 字符串中的 base_url（委托给 codex_config 共享实现）
    fn update_toml_base_url(toml_str: &str, new_url: &str) -> String {
        crate::codex_config::update_codex_toml_field(toml_str, "base_url", new_url)
            .unwrap_or_else(|_| toml_str.to_string())
    }

    /// 接管 Codex 时，本地客户端必须继续以 Responses wire API 访问代理。
    /// 真实上游是否走 Chat Completions 由 provider 配置决定，并在代理内部转换。
    fn apply_codex_proxy_toml_config(toml_str: &str, proxy_url: &str) -> String {
        let updated = Self::update_toml_base_url(toml_str, proxy_url);
        crate::codex_config::update_codex_toml_field(&updated, "wire_api", "responses")
            .unwrap_or(updated)
    }

    fn read_claude_live(&self) -> Result<Value, String> {
        let path = get_claude_settings_path();
        if !path.exists() {
            return Err("Claude 配置文件不存在".to_string());
        }

        let mut value: Value =
            read_json_file(&path).map_err(|e| format!("读取 Claude 配置失败: {e}"))?;

        if value.is_null() {
            value = json!({});
        }

        if !value.is_object() {
            let kind = match &value {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            return Err(format!(
                "Claude 配置文件格式错误：根节点必须是 JSON 对象（当前为 {kind}），路径: {}",
                path.display()
            ));
        }

        Ok(value)
    }

    fn write_claude_live(&self, config: &Value) -> Result<(), String> {
        let path = get_claude_settings_path();
        let settings = crate::services::provider::sanitize_claude_settings_for_live(config);
        write_json_file(&path, &settings).map_err(|e| format!("写入 Claude 配置失败: {e}"))
    }

    fn read_codex_live(&self) -> Result<Value, String> {
        use crate::codex_config::{get_codex_auth_path, get_codex_config_path};

        let auth_path = get_codex_auth_path();
        if !auth_path.exists() {
            return Err("Codex auth.json 不存在".to_string());
        }

        let auth: Value =
            read_json_file(&auth_path).map_err(|e| format!("读取 Codex auth 失败: {e}"))?;

        let config_path = get_codex_config_path();
        let config_str = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .map_err(|e| format!("读取 Codex config 失败: {e}"))?
        } else {
            String::new()
        };

        Ok(json!({
            "auth": auth,
            "config": config_str
        }))
    }

    fn write_codex_live(&self, config: &Value) -> Result<(), String> {
        use crate::codex_config::{
            get_codex_auth_path, get_codex_config_path, write_codex_live_atomic,
        };

        let auth = config.get("auth");
        let config_str = config.get("config").and_then(|v| v.as_str());

        // Proxy restore writes saved live backups verbatim. Provider-driven writes go
        // through write_live_with_common_config(), which normalizes Codex provider ids.
        match (auth, config_str) {
            (Some(auth), Some(cfg)) => write_codex_live_atomic(auth, Some(cfg))
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?,
            (Some(auth), None) => {
                let auth_path = get_codex_auth_path();
                write_json_file(&auth_path, auth)
                    .map_err(|e| format!("写入 Codex auth 失败: {e}"))?;
            }
            (None, Some(cfg)) => {
                let config_path = get_codex_config_path();
                crate::config::write_text_file(&config_path, cfg)
                    .map_err(|e| format!("写入 Codex config 失败: {e}"))?;
            }
            (None, None) => {}
        }

        Ok(())
    }

    fn read_gemini_live(&self) -> Result<Value, String> {
        use crate::gemini_config::{env_to_json, get_gemini_env_path, read_gemini_env};

        let env_path = get_gemini_env_path();
        if !env_path.exists() {
            return Err("Gemini .env 文件不存在".to_string());
        }

        let env_map = read_gemini_env().map_err(|e| format!("读取 Gemini env 失败: {e}"))?;
        Ok(env_to_json(&env_map))
    }

    fn write_gemini_live(&self, config: &Value) -> Result<(), String> {
        use crate::gemini_config::{json_to_env, write_gemini_env_atomic};

        let env_map = json_to_env(config).map_err(|e| format!("转换 Gemini 配置失败: {e}"))?;
        write_gemini_env_atomic(&env_map).map_err(|e| format!("写入 Gemini env 失败: {e}"))?;
        Ok(())
    }

    // ==================== 原有方法 ====================

    /// 获取服务器状态
    pub async fn get_status(&self) -> Result<ProxyStatus, String> {
        if let Some(server) = self.server.read().await.as_ref() {
            Ok(server.get_status().await)
        } else {
            // 服务器未运行时返回默认状态
            Ok(ProxyStatus {
                running: false,
                ..Default::default()
            })
        }
    }

    /// 获取代理配置
    pub async fn get_config(&self) -> Result<ProxyConfig, String> {
        self.db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))
    }

    /// 更新代理配置
    pub async fn update_config(&self, config: &ProxyConfig) -> Result<(), String> {
        // 记录旧配置用于判定是否需要重启
        let previous = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 保存到数据库（保持 live_takeover_active 状态不变）
        let mut new_config = config.clone();
        new_config.live_takeover_active = previous.live_takeover_active;

        self.db
            .update_proxy_config(new_config.clone())
            .await
            .map_err(|e| format!("保存代理配置失败: {e}"))?;

        // 检查服务器当前状态
        let mut server_guard = self.server.write().await;
        if server_guard.is_none() {
            return Ok(());
        }

        // 判断是否需要重启（地址或端口变更）
        let require_restart = new_config.listen_address != previous.listen_address
            || new_config.listen_port != previous.listen_port;

        if require_restart {
            if let Some(server) = server_guard.take() {
                server
                    .stop()
                    .await
                    .map_err(|e| format!("重启前停止代理服务器失败: {e}"))?;
            }

            let app_handle = self.app_handle.read().await.clone();
            let new_server = ProxyServer::new(new_config, self.db.clone(), app_handle);
            new_server
                .start()
                .await
                .map_err(|e| format!("重启代理服务器失败: {e}"))?;

            *server_guard = Some(new_server);
            log::info!("代理配置已更新，服务器已自动重启应用最新配置");

            // 如果当前存在任意 app 的 Live 接管，需要同步更新 Live 中的代理地址（否则客户端仍指向旧端口）
            drop(server_guard);
            if let Ok(takeover) = self.get_takeover_status().await {
                let mut updated_any = false;

                if takeover.claude {
                    self.takeover_live_config_best_effort(&AppType::Claude)
                        .await?;
                    updated_any = true;
                }
                if takeover.codex {
                    self.takeover_live_config_best_effort(&AppType::Codex)
                        .await?;
                    updated_any = true;
                }
                if takeover.gemini {
                    self.takeover_live_config_best_effort(&AppType::Gemini)
                        .await?;
                    updated_any = true;
                }

                if updated_any {
                    log::info!("已同步更新 Live 配置中的代理地址");
                }
            }

            return Ok(());
        } else if let Some(server) = server_guard.as_ref() {
            server.apply_runtime_config(&new_config).await;
            log::info!("代理配置已实时应用，无需重启代理服务器");
        }

        Ok(())
    }

    /// 检查服务器是否正在运行
    pub async fn is_running(&self) -> bool {
        self.server.read().await.is_some()
    }

    /// 热更新熔断器配置
    ///
    /// 如果代理服务器正在运行，将新配置应用到所有已创建的熔断器实例
    pub async fn update_circuit_breaker_configs(
        &self,
        config: crate::proxy::CircuitBreakerConfig,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server.update_circuit_breaker_configs(config).await;
            log::info!("已热更新运行中的熔断器配置");
        } else {
            log::debug!("代理服务器未运行，熔断器配置将在下次启动时生效");
        }
        Ok(())
    }

    /// 热更新指定应用的熔断器配置
    pub async fn update_circuit_breaker_config_for_app(
        &self,
        app_type: &str,
        config: crate::proxy::CircuitBreakerConfig,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .update_circuit_breaker_config_for_app(app_type, config)
                .await;
            log::info!("已热更新 {app_type} 运行中的熔断器配置");
        } else {
            log::debug!("{app_type} 熔断器配置将在下次代理启动时生效");
        }
        Ok(())
    }

    /// 重置指定 Provider 的熔断器
    ///
    /// 如果代理服务器正在运行，立即重置内存中的熔断器状态
    pub async fn reset_provider_circuit_breaker(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .reset_provider_circuit_breaker(provider_id, app_type)
                .await;
            log::info!("已重置 Provider {provider_id} (app: {app_type}) 的熔断器");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

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

    async fn spawn_opencode_mock(
        status: u16,
        body: &'static str,
    ) -> (
        String,
        tokio::sync::oneshot::Receiver<String>,
        tokio::task::JoinHandle<()>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock local addr");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = vec![0_u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let _ = tx.send(request);
                let response = format!(
                    "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        (
            format!("http://{addr}/zen/go/v1/chat/completions"),
            rx,
            handle,
        )
    }

    async fn use_unused_proxy_port(db: &Database) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind unused port");
        let port = listener.local_addr().expect("read unused port").port();
        drop(listener);

        let mut config = db.get_proxy_config().await.expect("get proxy config");
        config.listen_port = port;
        db.update_proxy_config(config)
            .await
            .expect("set unused proxy port");
        port
    }

    #[test]
    fn managed_account_claude_takeover_uses_api_key_placeholder() {
        let mut provider = Provider::with_id(
            "copilot".to_string(),
            "GitHub Copilot".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com",
                    "ANTHROPIC_MODEL": "claude-haiku-4.5"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            ..Default::default()
        });

        let mut live_config = provider.settings_config.clone();
        ProxyService::apply_claude_takeover_fields_for_provider(
            &mut live_config,
            "http://127.0.0.1:15721",
            &provider,
        );

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        assert_eq!(
            env.get("ANTHROPIC_API_KEY")
                .and_then(|value| value.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );
        assert!(
            env.get("ANTHROPIC_AUTH_TOKEN").is_none(),
            "managed OAuth providers should avoid Claude Auth Token login semantics"
        );
    }

    #[test]
    fn normal_claude_takeover_without_token_keeps_auth_token_fallback() {
        let mut live_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_MODEL": "claude-haiku-4.5"
            }
        });

        ProxyService::apply_claude_takeover_fields(&mut live_config, "http://127.0.0.1:15721");

        assert_eq!(
            live_config
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
                .and_then(|value| value.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );
        assert!(
            live_config
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .is_none(),
            "non-managed providers should retain the legacy fallback behavior"
        );
    }

    #[test]
    fn update_toml_base_url_updates_active_model_provider_base_url() {
        let input = r#"
model_provider = "any"
model = "gpt-5.1-codex"
disable_response_storage = true

[model_providers.any]
name = "any"
base_url = "https://anyrouter.top/v1"
wire_api = "responses"
requires_openai_auth = true
"#;

        let new_url = "http://127.0.0.1:5000/v1";
        let output = ProxyService::update_toml_base_url(input, new_url);

        let parsed: toml::Value =
            toml::from_str(&output).expect("updated config should be valid TOML");

        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str())
            .expect("model_providers.any.base_url should exist");

        assert_eq!(base_url, new_url);
        assert!(
            parsed.get("base_url").is_none(),
            "should not write top-level base_url"
        );

        let wire_api = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("wire_api"))
            .and_then(|v| v.as_str())
            .expect("model_providers.any.wire_api should exist");
        assert_eq!(wire_api, "responses");
    }

    #[test]
    fn apply_codex_proxy_toml_config_forces_local_responses_wire_api() {
        let input = r#"
model_provider = "chat_only"
model = "gpt-5.1-codex"

[model_providers.chat_only]
name = "Chat Only"
base_url = "https://chat-only.example/v1"
wire_api = "chat"
"#;

        let proxy_url = "http://127.0.0.1:5000/v1";
        let output = ProxyService::apply_codex_proxy_toml_config(input, proxy_url);
        let parsed: toml::Value =
            toml::from_str(&output).expect("updated config should be valid TOML");

        let provider = parsed
            .get("model_providers")
            .and_then(|v| v.get("chat_only"))
            .expect("model_providers.chat_only should exist");

        assert_eq!(
            provider.get("base_url").and_then(|v| v.as_str()),
            Some(proxy_url)
        );
        assert_eq!(
            provider.get("wire_api").and_then(|v| v.as_str()),
            Some("responses")
        );
    }

    #[test]
    fn update_toml_base_url_falls_back_to_top_level_base_url() {
        let input = r#"
model = "gpt-5.1-codex"
"#;

        let new_url = "http://127.0.0.1:5000/v1";
        let output = ProxyService::update_toml_base_url(input, new_url);

        let parsed: toml::Value =
            toml::from_str(&output).expect("updated config should be valid TOML");

        let base_url = parsed
            .get("base_url")
            .and_then(|v| v.as_str())
            .expect("base_url should exist");

        assert_eq!(base_url, new_url);
    }

    #[tokio::test]
    #[serial]
    async fn sync_claude_token_does_not_add_anthropic_api_key() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                    "ANTHROPIC_AUTH_TOKEN": "stale"
                }
            }),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");

        let live_config = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "fresh"
            }
        });

        service
            .sync_live_config_to_provider(&AppType::Claude, &live_config)
            .await
            .expect("sync");

        let updated = db
            .get_provider_by_id("p1", "claude")
            .expect("get provider")
            .expect("provider exists");
        let env = updated
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
            .expect("env object");

        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
            Some("fresh")
        );
        assert!(
            !env.contains_key("ANTHROPIC_API_KEY"),
            "should not add ANTHROPIC_API_KEY when absent"
        );
    }

    #[tokio::test]
    #[serial]
    async fn sync_claude_token_respects_existing_api_key_field() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                    "ANTHROPIC_API_KEY": "stale"
                }
            }),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");

        let live_config = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "fresh"
            }
        });

        service
            .sync_live_config_to_provider(&AppType::Claude, &live_config)
            .await
            .expect("sync");

        let updated = db
            .get_provider_by_id("p1", "claude")
            .expect("get provider")
            .expect("provider exists");
        let env = updated
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
            .expect("env object");

        assert_eq!(
            env.get("ANTHROPIC_API_KEY").and_then(|v| v.as_str()),
            Some("fresh")
        );
        assert!(
            !env.contains_key("ANTHROPIC_AUTH_TOKEN"),
            "should not add ANTHROPIC_AUTH_TOKEN when absent"
        );
    }

    #[test]
    fn claude_takeover_preserves_env_and_exposes_haiku_one_m_role() {
        let mut config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                "ANTHROPIC_AUTH_TOKEN": "sk-test",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]",
                "ENABLE_TOOL_SEARCH": "true",
                "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1",
                "API_TIMEOUT_MS": "3000000",
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": 1
            },
            "model": "opus"
        });

        ProxyService::apply_claude_takeover_fields(&mut config, "http://127.0.0.1:15721");

        assert_eq!(
            config.get("model").and_then(Value::as_str),
            Some("opus"),
            "takeover must not modify top-level settings outside the env whitelist"
        );
        let env = config
            .get("env")
            .and_then(Value::as_object)
            .expect("env object");
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(Value::as_str),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str),
            Some("http://127.0.0.1:15721")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(Value::as_str),
            Some(CLAUDE_TAKEOVER_HAIKU_MODEL)
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME")
                .and_then(Value::as_str),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                .and_then(Value::as_str),
            Some("claude-opus-4-7[1M]")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_OPUS_MODEL_NAME")
                .and_then(Value::as_str),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .and_then(Value::as_str),
            Some("claude-sonnet-4-6[1M]")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME")
                .and_then(Value::as_str),
            Some("deepseek-v4-flash")
        );
        assert_eq!(env.get("ENABLE_TOOL_SEARCH"), Some(&json!("true")));
        assert_eq!(
            env.get("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"),
            Some(&json!(1))
        );
        assert_eq!(
            env.get("ANTHROPIC_MODEL"),
            None,
            "takeover should expose Claude role aliases instead of the provider fallback model"
        );
    }

    #[tokio::test]
    #[serial]
    async fn claude_restore_returns_exact_pre_takeover_snapshot() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let original = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                "ANTHROPIC_AUTH_TOKEN": "sk-test",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]",
                "ENABLE_TOOL_SEARCH": "true",
                "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1",
                "API_TIMEOUT_MS": "3000000",
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": 1
            }
        });

        service
            .write_claude_live(&original)
            .expect("write original live");
        service
            .backup_live_config_strict(&AppType::Claude)
            .await
            .expect("backup original");
        service
            .takeover_live_config_strict(&AppType::Claude)
            .await
            .expect("takeover live");

        let mut polluted = service.read_claude_live().expect("read takeover live");
        polluted["model"] = json!("opus");
        service
            .write_claude_live(&polluted)
            .expect("simulate claude model write");

        service
            .restore_live_config_for_app_with_fallback(&AppType::Claude)
            .await
            .expect("restore live");

        assert_eq!(
            service.read_claude_live().expect("read restored live"),
            original
        );
    }

    #[tokio::test]
    #[serial]
    async fn restore_without_backup_rebuilds_opencode_live_with_real_provider_config() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let mut provider = Provider::with_id(
            "opencode-go".to_string(),
            "OpenCode Go".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                    "ANTHROPIC_AUTH_TOKEN": "sk-test",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("opencode_go_subscription".to_string()),
            api_format: Some("openai_chat".to_string()),
            is_full_url: Some(true),
            ..Default::default()
        });
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "opencode-go")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Claude, Some("opencode-go"))
            .expect("set local current");

        service
            .write_claude_live(&json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                    "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER,
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]"
                }
            }))
            .expect("seed stale takeover live");

        service
            .restore_live_config_for_app_with_fallback(&AppType::Claude)
            .await
            .expect("restore fallback");

        let live = service.read_claude_live().expect("read live");
        let env = live
            .get("env")
            .and_then(Value::as_object)
            .expect("env object");
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str),
            Some("https://opencode.ai/zen/go/v1/chat/completions")
        );
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(Value::as_str),
            Some("sk-test")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(Value::as_str),
            Some("deepseek-v4-pro[1M]")
        );
    }

    #[tokio::test]
    #[serial]
    async fn opencode_takeover_preflight_rejects_invalid_upstream_without_touching_live() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());
        let (base_url, request_rx, server) =
            spawn_opencode_mock(401, r#"{"error":{"message":"Invalid API key."}}"#).await;

        let original = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "direct-key",
                "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
            }
        });
        service
            .write_claude_live(&original)
            .expect("write original live");

        let mut provider = Provider::with_id(
            "opencode-go".to_string(),
            "OpenCode Go".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_AUTH_TOKEN": "bad-key",
                    "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("opencode_go_subscription".to_string()),
            api_format: Some("openai_chat".to_string()),
            is_full_url: Some(true),
            ..Default::default()
        });
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "opencode-go")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Claude, Some("opencode-go"))
            .expect("set local current");

        let error = service
            .set_takeover_for_app("claude", true)
            .await
            .expect_err("preflight should reject invalid upstream");
        assert!(error.contains("HTTP 401"), "{error}");

        let request = request_rx.await.expect("mock request");
        assert!(
            request.contains(r#""model":"deepseek-v4-flash""#),
            "preflight should strip local [1M] before calling OpenCode: {request}"
        );
        server.await.expect("mock server task");
        assert_eq!(service.read_claude_live().expect("read live"), original);
        assert!(
            !db.get_proxy_config_for_app("claude")
                .await
                .expect("proxy config")
                .enabled,
            "failed preflight must not persist enabled=true"
        );
        assert!(
            db.get_live_backup("claude")
                .await
                .expect("read backup")
                .is_none(),
            "failed preflight must not create a restore backup"
        );
        assert!(
            !service.is_running().await,
            "failed preflight should happen before starting the local proxy"
        );
    }

    #[tokio::test]
    #[serial]
    async fn opencode_takeover_preflight_accepts_valid_upstream_and_uses_current_models() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());
        let proxy_port = use_unused_proxy_port(&db).await;
        let proxy_url = format!("http://127.0.0.1:{proxy_port}");
        let (base_url, request_rx, server) =
            spawn_opencode_mock(200, r#"{"choices":[{"message":{"content":"pong"}}]}"#).await;

        let original = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "direct-key",
                "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
            }
        });
        service
            .write_claude_live(&original)
            .expect("write original live");

        let mut provider = Provider::with_id(
            "opencode-go".to_string(),
            "OpenCode Go".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_AUTH_TOKEN": "valid-key",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("opencode_go_subscription".to_string()),
            api_format: Some("openai_chat".to_string()),
            is_full_url: Some(true),
            ..Default::default()
        });
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "opencode-go")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Claude, Some("opencode-go"))
            .expect("set local current");

        service
            .set_takeover_for_app("claude", true)
            .await
            .expect("valid OpenCode preflight should allow takeover");
        let request = request_rx.await.expect("mock request");
        assert!(
            request.contains(r#""model":"deepseek-v4-flash""#),
            "preflight should call OpenCode with the stripped upstream model: {request}"
        );
        server.await.expect("mock server task");

        let live = service.read_claude_live().expect("read takeover live");
        let env = live
            .get("env")
            .and_then(Value::as_object)
            .expect("live env");
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN"),
            Some(&json!(PROXY_TOKEN_PLACEHOLDER))
        );
        assert_eq!(env.get("ANTHROPIC_BASE_URL"), Some(&json!(proxy_url)));
        assert_eq!(env.get("ANTHROPIC_MODEL"), None);
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
            Some(&json!(CLAUDE_TAKEOVER_HAIKU_MODEL))
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_SONNET_MODEL"),
            Some(&json!("claude-sonnet-4-6[1M]"))
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_OPUS_MODEL"),
            Some(&json!("claude-opus-4-7[1M]"))
        );
        assert!(
            db.get_proxy_config_for_app("claude")
                .await
                .expect("proxy config")
                .enabled,
            "successful preflight should persist enabled=true"
        );

        service
            .stop_with_restore()
            .await
            .expect("restore after positive preflight");
        assert_eq!(service.read_claude_live().expect("restored live"), original);
    }

    #[tokio::test]
    #[serial]
    async fn opencode_takeover_preflight_accepts_reasoning_only_upstream_response() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());
        use_unused_proxy_port(&db).await;
        let (base_url, request_rx, server) = spawn_opencode_mock(
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":"","reasoning_content":"We are asked to reply exactly ok."},"finish_reason":"length"}]}"#,
        )
        .await;

        let original = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                "ANTHROPIC_AUTH_TOKEN": "direct-key",
                "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
            }
        });
        service
            .write_claude_live(&original)
            .expect("write original live");

        let mut provider = Provider::with_id(
            "opencode-go".to_string(),
            "OpenCode Go".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_AUTH_TOKEN": "valid-key",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("opencode_go_subscription".to_string()),
            api_format: Some("openai_chat".to_string()),
            is_full_url: Some(true),
            ..Default::default()
        });
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "opencode-go")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Claude, Some("opencode-go"))
            .expect("set local current");

        service
            .set_takeover_for_app("claude", true)
            .await
            .expect("reasoning-only OpenCode preflight should allow takeover");
        let request = request_rx.await.expect("mock request");
        assert!(
            request.contains(r#""max_tokens":64"#),
            "preflight should leave enough room for reasoning models: {request}"
        );
        server.await.expect("mock server task");

        service
            .stop_with_restore()
            .await
            .expect("restore after reasoning-only preflight");
        assert_eq!(service.read_claude_live().expect("restored live"), original);
    }

    #[tokio::test]
    #[serial]
    async fn claude_proxy_lifecycle_preserves_plugins_during_takeover_and_restores_exact_snapshot()
    {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());
        let (base_url, _request_rx, server) =
            spawn_opencode_mock(200, r#"{"choices":[{"message":{"content":"pong"}}]}"#).await;

        let original = json!({
            "enabledPlugins": {
                "chrome-devtools-mcp@claude-plugins-official": true,
                "github@claude-plugins-official": true,
                "pua@pua-skills": true
            },
            "env": {
                "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                "ANTHROPIC_AUTH_TOKEN": "sk-original",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1M]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]",
                "ENABLE_TOOL_SEARCH": "true",
                "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1",
                "API_TIMEOUT_MS": "3000000"
            },
            "permissions": {
                "defaultMode": "auto"
            },
            "skillListingBudgetFraction": 0.1,
            "skipAutoPermissionPrompt": true,
            "skipDangerousModePermissionPrompt": true,
            "model": "sonnet"
        });
        service
            .write_claude_live(&original)
            .expect("write original live");

        let mut provider = Provider::with_id(
            "opencode-go".to_string(),
            "OpenCode Go".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_AUTH_TOKEN": "valid-key",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("opencode_go_subscription".to_string()),
            api_format: Some("openai_chat".to_string()),
            is_full_url: Some(true),
            ..Default::default()
        });
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "opencode-go")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Claude, Some("opencode-go"))
            .expect("set local current");

        service
            .set_takeover_for_app("claude", true)
            .await
            .expect("enable proxy takeover");
        server.await.expect("mock server task");

        let taken_over = service.read_claude_live().expect("read takeover live");
        assert_eq!(
            taken_over.get("enabledPlugins"),
            original.get("enabledPlugins"),
            "proxy takeover must keep Claude Code plugin enablement"
        );
        assert_eq!(
            taken_over.get("permissions"),
            original.get("permissions"),
            "proxy takeover must keep Claude Code permission settings"
        );
        assert_eq!(
            taken_over.get("skillListingBudgetFraction"),
            original.get("skillListingBudgetFraction"),
            "proxy takeover must keep plugin/skill budget settings"
        );
        assert_eq!(
            taken_over.get("skipAutoPermissionPrompt"),
            original.get("skipAutoPermissionPrompt"),
            "proxy takeover must keep prompt settings used by plugins"
        );
        assert_eq!(
            taken_over.get("skipDangerousModePermissionPrompt"),
            original.get("skipDangerousModePermissionPrompt"),
            "proxy takeover must keep dangerous-mode prompt settings"
        );
        assert_eq!(
            taken_over.get("model"),
            original.get("model"),
            "proxy takeover must keep top-level Claude Code settings outside the env whitelist"
        );

        let mut polluted = taken_over;
        polluted["model"] = json!("opus");
        service
            .write_claude_live(&polluted)
            .expect("simulate Claude Code writing transient model");

        service
            .set_takeover_for_app("claude", false)
            .await
            .expect("disable proxy takeover");
        assert_eq!(
            service.read_claude_live().expect("read restored live"),
            original,
            "disabling proxy must restore the exact pre-takeover settings.json snapshot"
        );
        assert!(
            db.get_live_backup("claude")
                .await
                .expect("read live backup")
                .is_none(),
            "successful restore should remove the sensitive live backup"
        );
    }

    #[tokio::test]
    #[serial]
    async fn opencode_startup_preflight_failure_restores_stale_takeover_and_disables_state() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());
        let (base_url, _request_rx, server) =
            spawn_opencode_mock(401, r#"{"error":{"message":"Invalid API key."}}"#).await;

        let original = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "direct-key",
                "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
            }
        });
        let taken_over = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER,
                "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
            }
        });
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&original).expect("serialize"),
        )
        .await
        .expect("save backup");
        service
            .write_claude_live(&taken_over)
            .expect("write stale takeover live");

        let mut config = db
            .get_proxy_config_for_app("claude")
            .await
            .expect("get proxy config");
        config.enabled = true;
        db.update_proxy_config_for_app(config)
            .await
            .expect("mark enabled");
        let mut global_config = db
            .get_global_proxy_config()
            .await
            .expect("get global proxy config");
        global_config.proxy_enabled = true;
        db.update_global_proxy_config(global_config)
            .await
            .expect("mark global proxy enabled");

        let mut provider = Provider::with_id(
            "opencode-go".to_string(),
            "OpenCode Go".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_AUTH_TOKEN": "bad-key",
                    "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("opencode_go_subscription".to_string()),
            api_format: Some("openai_chat".to_string()),
            is_full_url: Some(true),
            ..Default::default()
        });
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "opencode-go")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Claude, Some("opencode-go"))
            .expect("set local current");

        let error = service
            .set_takeover_for_app("claude", true)
            .await
            .expect_err("startup preflight should reject invalid upstream");
        assert!(error.contains("HTTP 401"), "{error}");
        server.await.expect("mock server task");

        assert_eq!(service.read_claude_live().expect("read live"), original);
        assert!(
            !db.get_proxy_config_for_app("claude")
                .await
                .expect("proxy config")
                .enabled,
            "failed startup restore must clear enabled=true"
        );
        assert!(
            !db.get_global_proxy_config()
                .await
                .expect("global proxy config")
                .proxy_enabled,
            "failed startup restore must clear proxy_enabled=true when no app remains active"
        );
        assert!(
            db.get_live_backup("claude")
                .await
                .expect("read backup")
                .is_none(),
            "restored startup failure should delete consumed backup"
        );
    }

    #[tokio::test]
    #[serial]
    async fn claude_takeover_uses_current_provider_models_while_preserving_original_live_shell() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let original_siliconflow = json!({
            "enabledPlugins": {
                "github@claude-plugins-official": true
            },
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.siliconflow.cn",
                "ANTHROPIC_AUTH_TOKEN": "siliconflow-key",
                "ANTHROPIC_MODEL": "deepseek-ai/DeepSeek-V4-Flash[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-ai/DeepSeek-V4-Flash[1M]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-ai/DeepSeek-V4-Flash[1M]",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-ai/DeepSeek-V4-Flash[1M]",
                "ENABLE_TOOL_SEARCH": "true",
                "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
            },
            "permissions": {
                "defaultMode": "auto"
            },
            "model": "opus"
        });
        service
            .write_claude_live(&original_siliconflow)
            .expect("write original live shell");
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&original_siliconflow).expect("serialize backup"),
        )
        .await
        .expect("seed original backup");

        let mut opencode = Provider::with_id(
            "opencode-go".to_string(),
            "OpenCode Go".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                    "ANTHROPIC_AUTH_TOKEN": "opencode-key",
                    "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1M]",
                    "API_TIMEOUT_MS": "3000000"
                }
            }),
            None,
        );
        opencode.meta = Some(ProviderMeta {
            provider_type: Some("opencode_go_subscription".to_string()),
            api_format: Some("openai_chat".to_string()),
            is_full_url: Some(true),
            ..Default::default()
        });
        db.save_provider("claude", &opencode)
            .expect("save opencode");
        db.set_current_provider("claude", "opencode-go")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Claude, Some("opencode-go"))
            .expect("set local current");

        service
            .takeover_live_config_strict(&AppType::Claude)
            .await
            .expect("strict takeover");

        let live = service.read_claude_live().expect("read takeover live");
        assert_eq!(
            live.get("model").and_then(Value::as_str),
            Some("opus"),
            "takeover must preserve top-level Claude Code settings outside the env whitelist"
        );
        assert_eq!(
            live.get("enabledPlugins")
                .and_then(|plugins| plugins.get("github@claude-plugins-official"))
                .and_then(Value::as_bool),
            Some(true),
            "takeover should preserve non-provider top-level live settings"
        );
        assert_eq!(
            live.get("permissions")
                .and_then(|permissions| permissions.get("defaultMode"))
                .and_then(Value::as_str),
            Some("auto"),
            "takeover should preserve user permission settings"
        );

        let env = live
            .get("env")
            .and_then(Value::as_object)
            .expect("env object");
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str),
            Some("http://127.0.0.1:15721")
        );
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(Value::as_str),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );
        assert_eq!(env.get("ANTHROPIC_MODEL"), None);
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(Value::as_str),
            Some(CLAUDE_TAKEOVER_HAIKU_MODEL)
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME")
                .and_then(Value::as_str),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                .and_then(Value::as_str),
            Some("claude-opus-4-7[1M]")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_OPUS_MODEL_NAME")
                .and_then(Value::as_str),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .and_then(Value::as_str),
            Some("claude-sonnet-4-6[1M]")
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME")
                .and_then(Value::as_str),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            env.get("ENABLE_TOOL_SEARCH"),
            None,
            "provider-owned env absent from the current provider should not leak from stale live"
        );
        assert_eq!(
            env.get("API_TIMEOUT_MS").and_then(Value::as_str),
            Some("3000000"),
            "provider env should be merged into takeover live"
        );

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("read backup")
            .expect("backup exists");
        let backup_config: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup");
        assert_eq!(backup_config, original_siliconflow);
    }

    #[tokio::test]
    #[serial]
    async fn backup_live_config_strict_preserves_existing_original_snapshot() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let original = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.siliconflow.cn",
                "ANTHROPIC_AUTH_TOKEN": "siliconflow-key"
            }
        });
        let later_live = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                "ANTHROPIC_AUTH_TOKEN": "opencode-key"
            }
        });

        db.save_live_backup(
            "claude",
            &serde_json::to_string(&original).expect("serialize original"),
        )
        .await
        .expect("seed original backup");
        service
            .write_claude_live(&later_live)
            .expect("write later live");

        service
            .backup_live_config_strict(&AppType::Claude)
            .await
            .expect("backup should be idempotent");

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("read backup")
            .expect("backup exists");
        let backup_config: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup");
        assert_eq!(
            backup_config, original,
            "existing original backup must not be overwritten by later live state"
        );
    }

    #[tokio::test]
    #[serial]
    async fn claude_takeover_resyncs_stale_live_models_without_touching_original_backup() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let original_siliconflow = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.siliconflow.cn",
                "ANTHROPIC_AUTH_TOKEN": "siliconflow-key",
                "ANTHROPIC_MODEL": "deepseek-ai/DeepSeek-V4-Flash[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-ai/DeepSeek-V4-Flash[1M]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-ai/DeepSeek-V4-Flash[1M]",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-ai/DeepSeek-V4-Flash[1M]"
            }
        });
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&original_siliconflow).expect("serialize backup"),
        )
        .await
        .expect("save backup");

        let mut opencode = Provider::with_id(
            "opencode-go".to_string(),
            "OpenCode Go".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                    "ANTHROPIC_AUTH_TOKEN": "opencode-key",
                    "ANTHROPIC_MODEL": "deepseek-v4-flash[1M]",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-flash[1M]",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1M]"
                }
            }),
            None,
        );
        opencode.meta = Some(ProviderMeta {
            provider_type: Some("opencode_go_subscription".to_string()),
            api_format: Some("openai_chat".to_string()),
            is_full_url: Some(true),
            ..Default::default()
        });
        db.save_provider("claude", &opencode)
            .expect("save opencode");
        db.set_current_provider("claude", "opencode-go")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Claude, Some("opencode-go"))
            .expect("set local current");

        let mut stale_live = original_siliconflow.clone();
        ProxyService::apply_claude_takeover_fields(&mut stale_live, "http://127.0.0.1:15721");
        service
            .write_claude_live(&stale_live)
            .expect("write stale takeover live");

        assert!(
            !service
                .claude_takeover_live_matches_current_provider()
                .await
                .expect("check stale live"),
            "stale SiliconFlow takeover live should not match current OpenCode provider"
        );

        service
            .sync_claude_live_from_provider_while_proxy_active(&opencode)
            .await
            .expect("sync current provider into takeover live");

        let live = service.read_claude_live().expect("read live");
        let env = live
            .get("env")
            .and_then(Value::as_object)
            .expect("env object");
        assert_eq!(env.get("ANTHROPIC_MODEL"), None);
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(Value::as_str),
            Some(CLAUDE_TAKEOVER_HAIKU_MODEL)
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME")
                .and_then(Value::as_str),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(Value::as_str),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("read backup")
            .expect("backup exists");
        let backup_config: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup");
        assert_eq!(backup_config, original_siliconflow);
    }

    #[tokio::test]
    #[serial]
    async fn restore_without_backup_rebuilds_codex_live_through_provider_writer() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "rightcode".to_string(),
            "RightCode".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "rightcode-key"
                },
                "config": r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");
        db.set_current_provider("codex", "rightcode")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Codex, Some("rightcode"))
            .expect("set local current");

        service
            .write_codex_live(&json!({
                "auth": {
                    "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                },
                "config": r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
requires_openai_auth = true
"#
            }))
            .expect("seed stale takeover live");

        service
            .restore_live_config_for_app_with_fallback(&AppType::Codex)
            .await
            .expect("restore fallback");

        let live = service.read_codex_live().expect("read Codex live");
        assert_eq!(
            live.get("auth")
                .and_then(|auth| auth.get("OPENAI_API_KEY"))
                .and_then(Value::as_str),
            Some("rightcode-key")
        );
        let config = live
            .get("config")
            .and_then(Value::as_str)
            .expect("config string");
        let parsed: toml::Value = toml::from_str(config).expect("parse config");
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("rightcode"))
                .and_then(|v| v.get("base_url"))
                .and_then(toml::Value::as_str),
            Some("https://rightcode.example/v1")
        );
    }

    #[tokio::test]
    #[serial]
    async fn restore_without_backup_rebuilds_gemini_live_through_provider_writer() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "gemini-direct".to_string(),
            "Gemini Direct".to_string(),
            json!({
                "env": {
                    "GEMINI_API_KEY": "gemini-key",
                    "GOOGLE_GEMINI_BASE_URL": "https://generativelanguage.googleapis.com"
                }
            }),
            None,
        );
        db.save_provider("gemini", &provider)
            .expect("save provider");
        db.set_current_provider("gemini", "gemini-direct")
            .expect("set db current");
        crate::settings::set_current_provider(&AppType::Gemini, Some("gemini-direct"))
            .expect("set local current");

        service
            .write_gemini_live(&json!({
                "env": {
                    "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER,
                    "GOOGLE_GEMINI_BASE_URL": "http://127.0.0.1:15721"
                }
            }))
            .expect("seed stale takeover live");

        service
            .restore_live_config_for_app_with_fallback(&AppType::Gemini)
            .await
            .expect("restore fallback");

        let live = service.read_gemini_live().expect("read Gemini live");
        let env = live
            .get("env")
            .and_then(Value::as_object)
            .expect("env object");
        assert_eq!(
            env.get("GEMINI_API_KEY").and_then(Value::as_str),
            Some("gemini-key")
        );
        assert_eq!(
            env.get("GOOGLE_GEMINI_BASE_URL").and_then(Value::as_str),
            Some("https://generativelanguage.googleapis.com")
        );
    }

    #[tokio::test]
    #[serial]
    async fn switch_proxy_target_preserves_original_live_backup_when_taken_over() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "a-key"
                }
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "b-key"
                }
            }),
            None,
        );
        db.save_provider("claude", &provider_a)
            .expect("save provider a");
        db.save_provider("claude", &provider_b)
            .expect("save provider b");
        db.set_current_provider("claude", "a")
            .expect("set current provider");

        let original_backup = json!({"env": {"ANTHROPIC_API_KEY": "a-key"}});
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&original_backup).expect("serialize original backup"),
        )
        .await
        .expect("seed live backup");

        service
            .switch_proxy_target("claude", "b")
            .await
            .expect("switch proxy target");

        // 断言：本地 settings 的 current provider 已同步
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("b")
        );

        // 断言：Live 备份仍是启用代理前的原始配置，热切换不能覆盖恢复快照。
        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        assert_eq!(
            backup.original_config,
            serde_json::to_string(&original_backup).expect("serialize")
        );
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_updates_claude_live_while_preserving_takeover_fields() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "a-key",
                    "ANTHROPIC_BASE_URL": "https://api.a.example",
                    "ANTHROPIC_MODEL": "claude-old"
                },
                "permissions": { "allow": ["Bash"] }
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "b-key",
                    "ANTHROPIC_BASE_URL": "https://api.b.example",
                    "ANTHROPIC_MODEL": "claude-new",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-flash",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "DeepSeek V4 Flash",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "DeepSeek V4 Pro",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-ultra [1m]"
                },
                "permissions": { "allow": ["Read"] }
            }),
            None,
        );

        db.save_provider("claude", &provider_a)
            .expect("save provider a");
        db.save_provider("claude", &provider_b)
            .expect("save provider b");
        db.set_current_provider("claude", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&provider_a.settings_config).expect("serialize provider a"),
        )
        .await
        .expect("seed live backup");
        service
            .write_claude_live(&json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                    "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER,
                    "ANTHROPIC_MODEL": "stale-model",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Stale Sonnet",
                    "CUSTOM_ENV_SHOULD_STAY": "keep-me"
                },
                "permissions": { "allow": ["Bash"] }
            }))
            .expect("seed taken-over live file");

        service
            .hot_switch_provider("claude", "b")
            .await
            .expect("hot switch provider");

        let live = service.read_claude_live().expect("read live config");
        assert_eq!(
            live.get("permissions"),
            Some(&json!({ "allow": ["Bash"] })),
            "hot switch takeover must preserve top-level settings outside the env whitelist"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "takeover token placeholder should be preserved"
        );
        assert!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
                .is_none(),
            "stale token keys should be removed when the new provider uses API_KEY"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721"),
            "takeover proxy URL should remain active"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str()),
            None,
            "takeover mode should expose Claude role aliases instead of provider fallback models"
        );
        let live_env = live
            .get("env")
            .and_then(|env| env.as_object())
            .expect("live env");
        assert_eq!(
            live_env
                .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(|v| v.as_str()),
            Some(CLAUDE_TAKEOVER_HAIKU_MODEL),
            "model menu should show the stable Claude Haiku role alias"
        );
        assert_eq!(
            live_env
                .get("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME")
                .and_then(|v| v.as_str()),
            Some("DeepSeek V4 Flash"),
            "model menu should keep the provider Haiku display name"
        );
        assert_eq!(
            live_env
                .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .and_then(|v| v.as_str()),
            Some("claude-sonnet-4-6[1M]"),
            "Sonnet role should keep 1M capability on the stable alias"
        );
        assert_eq!(
            live_env
                .get("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME")
                .and_then(|v| v.as_str()),
            Some("DeepSeek V4 Pro"),
            "hot switch takeover should refresh provider-owned display names"
        );
        assert_eq!(
            live_env
                .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                .and_then(|v| v.as_str()),
            Some("claude-opus-4-7[1M]"),
            "Opus role should keep 1M capability on the stable alias"
        );
        assert_eq!(
            live_env
                .get("ANTHROPIC_DEFAULT_OPUS_MODEL_NAME")
                .and_then(|v| v.as_str()),
            Some("deepseek-v4-ultra"),
            "Opus display name should use the provider model without the 1M marker"
        );
        assert_eq!(
            live_env
                .get("CUSTOM_ENV_SHOULD_STAY")
                .and_then(|v| v.as_str()),
            Some("keep-me"),
            "custom env outside the whitelist must survive takeover hot switch"
        );

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let expected = serde_json::to_string(&provider_a.settings_config).expect("serialize");
        assert_eq!(backup.original_config, expected);
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_serializes_same_app_switches() {
        use tokio::time::{sleep, Duration};

        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
            None,
        );
        let provider_c = Provider::with_id(
            "c".to_string(),
            "C".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "c-key" } }),
            None,
        );

        db.save_provider("claude", &provider_a)
            .expect("save provider a");
        db.save_provider("claude", &provider_b)
            .expect("save provider b");
        db.save_provider("claude", &provider_c)
            .expect("save provider c");
        db.set_current_provider("claude", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("a"))
            .expect("set local current provider");
        let original_backup = json!({"env": {"ANTHROPIC_API_KEY": "original"}});
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&original_backup).expect("serialize original backup"),
        )
        .await
        .expect("seed live backup");

        let guard = service.lock_switch_for_test("claude").await;
        let service_for_b = service.clone();
        let service_for_c = service.clone();

        let switch_b = tokio::spawn(async move {
            service_for_b
                .hot_switch_provider("claude", "b")
                .await
                .expect("switch to b")
        });
        sleep(Duration::from_millis(20)).await;
        let switch_c = tokio::spawn(async move {
            service_for_c
                .hot_switch_provider("claude", "c")
                .await
                .expect("switch to c")
        });

        sleep(Duration::from_millis(20)).await;
        drop(guard);

        let outcome_b = switch_b.await.expect("join switch b");
        let outcome_c = switch_c.await.expect("join switch c");
        assert!(outcome_b.logical_target_changed);
        assert!(outcome_c.logical_target_changed);

        assert_eq!(
            crate::settings::get_effective_current_provider(&db, &AppType::Claude)
                .expect("effective current"),
            Some("c".to_string())
        );
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("c")
        );
        assert_eq!(
            db.get_current_provider("claude").expect("db current"),
            Some("c".to_string())
        );

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let expected = serde_json::to_string(&original_backup).expect("serialize");
        assert_eq!(backup.original_config, expected);
    }

    #[tokio::test]
    #[serial]
    async fn restore_waits_for_hot_switch_and_restores_original_backup() {
        use tokio::time::{sleep, Duration};

        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
            None,
        );

        db.save_provider("claude", &provider_a)
            .expect("save provider a");
        db.save_provider("claude", &provider_b)
            .expect("save provider b");
        db.set_current_provider("claude", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&provider_a.settings_config).expect("serialize provider a"),
        )
        .await
        .expect("seed live backup");
        service
            .write_claude_live(&json!({ "env": { "ANTHROPIC_API_KEY": "stale" } }))
            .expect("seed live file");

        let guard = service.lock_switch_for_test("claude").await;
        let service_for_switch = service.clone();
        let service_for_restore = service.clone();

        let switch_to_b = tokio::spawn(async move {
            service_for_switch
                .hot_switch_provider("claude", "b")
                .await
                .expect("switch to b")
        });
        sleep(Duration::from_millis(20)).await;
        let restore = tokio::spawn(async move {
            service_for_restore
                .restore_live_config_for_app_with_fallback(&AppType::Claude)
                .await
                .expect("restore claude live")
        });

        sleep(Duration::from_millis(20)).await;
        drop(guard);

        let outcome = switch_to_b.await.expect("join switch");
        restore.await.expect("join restore");
        assert!(outcome.logical_target_changed);

        assert_eq!(
            crate::settings::get_effective_current_provider(&db, &AppType::Claude)
                .expect("effective current"),
            Some("b".to_string())
        );

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let expected = serde_json::to_string(&provider_a.settings_config).expect("serialize");
        assert_eq!(backup.original_config, expected);
        assert_eq!(
            service.read_claude_live().expect("read live"),
            provider_a.settings_config
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_live_backup_from_provider_applies_claude_common_config() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        db.set_config_snippet(
            "claude",
            Some(
                serde_json::json!({
                    "includeCoAuthoredBy": false
                })
                .to_string(),
            ),
        )
        .expect("set common config snippet");

        let service = ProxyService::new(db.clone());

        let mut provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token",
                    "ANTHROPIC_BASE_URL": "https://claude.example"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });

        service
            .update_live_backup_from_provider("claude", &provider)
            .await
            .expect("update live backup");

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");

        assert_eq!(
            stored.get("includeCoAuthoredBy").and_then(|v| v.as_bool()),
            Some(false),
            "common config should be applied into Claude restore backup"
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_live_backup_from_provider_applies_codex_common_config() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        db.set_config_snippet(
            "codex",
            Some("disable_response_storage = true\n".to_string()),
        )
        .expect("set common config snippet");

        let service = ProxyService::new(db.clone());

        let mut provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "token"
                },
                "config": r#"model_provider = "any"
model = "gpt-5"

[model_providers.any]
base_url = "https://codex.example/v1"
"#
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });

        service
            .update_live_backup_from_provider("codex", &provider)
            .await
            .expect("update live backup");

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let config = stored
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config string");

        assert!(
            config.contains("disable_response_storage = true"),
            "common config should be applied into Codex restore backup"
        );
    }

    #[tokio::test]
    #[serial]
    async fn stop_with_restore_restores_live_and_clears_enabled_for_app_exit() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let original = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_AUTH_TOKEN": "real-token"
            }
        });
        let taken_over = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:15722",
                "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER
            }
        });

        db.save_live_backup(
            "claude",
            &serde_json::to_string(&original).expect("serialize"),
        )
        .await
        .expect("save backup");
        service
            .write_claude_live(&taken_over)
            .expect("write taken over live");

        let mut config = db
            .get_proxy_config_for_app("claude")
            .await
            .expect("get claude proxy config");
        config.enabled = true;
        db.update_proxy_config_for_app(config)
            .await
            .expect("mark takeover enabled");

        service
            .stop_with_restore()
            .await
            .expect("restore on app exit");

        assert_eq!(service.read_claude_live().expect("read live"), original);
        assert!(
            !db.get_proxy_config_for_app("claude")
                .await
                .expect("get claude proxy config")
                .enabled,
            "normal app exit should clear enabled so startup does not reapply takeover"
        );
        assert!(
            db.get_live_backup("claude")
                .await
                .expect("read backup")
                .is_none(),
            "backup should be deleted after a successful restore"
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_live_backup_from_provider_preserves_codex_mcp_servers() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        db.save_live_backup(
            "codex",
            &serde_json::to_string(&json!({
                "auth": {
                    "OPENAI_API_KEY": "old-token"
                },
                "config": r#"model_provider = "any"
model = "gpt-4"

[model_providers.any]
base_url = "https://old.example/v1"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
            }))
            .expect("serialize seed backup"),
        )
        .await
        .expect("seed live backup");

        let provider = Provider::with_id(
            "p2".to_string(),
            "P2".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "new-token"
                },
                "config": r#"model_provider = "any"
model = "gpt-5"

[model_providers.any]
base_url = "https://new.example/v1"
"#
            }),
            None,
        );

        service
            .update_live_backup_from_provider("codex", &provider)
            .await
            .expect("update live backup");

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let config = stored
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config string");

        assert!(
            config.contains("[mcp_servers.echo]"),
            "existing Codex MCP section should survive proxy hot-switch backup update"
        );
        assert!(
            config.contains("https://new.example/v1"),
            "provider-specific base_url should still update to the new provider"
        );
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_codex_provider_preserves_original_backup_and_restore() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "RightCode".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "rightcode-key"
                },
                "config": r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "AiHubMix".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "aihubmix-key"
                },
                "config": r#"model_provider = "aihubmix"
model = "gpt-5.4"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
            }),
            None,
        );

        db.save_provider("codex", &provider_a)
            .expect("save provider a");
        db.save_provider("codex", &provider_b)
            .expect("save provider b");
        db.set_current_provider("codex", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "codex",
            &serde_json::to_string(&provider_a.settings_config).expect("serialize provider a"),
        )
        .await
        .expect("seed live backup");
        service
            .write_codex_live(&json!({
                "auth": {
                    "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                },
                "config": r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
requires_openai_auth = true
"#
            }))
            .expect("seed taken-over Codex live config");

        service
            .hot_switch_provider("codex", "b")
            .await
            .expect("hot switch Codex provider");

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let backup_config = stored
            .get("config")
            .and_then(|v| v.as_str())
            .expect("backup config string");
        let parsed_backup: toml::Value =
            toml::from_str(backup_config).expect("parse backup config");
        assert_eq!(
            parsed_backup.get("model_provider").and_then(|v| v.as_str()),
            Some("rightcode"),
            "provider-derived restore backup should retain stable Codex model_provider"
        );
        let backup_model_providers = parsed_backup
            .get("model_providers")
            .and_then(|v| v.as_table())
            .expect("backup model_providers");
        assert!(backup_model_providers.get("aihubmix").is_none());
        assert_eq!(
            backup_model_providers
                .get("rightcode")
                .and_then(|v| v.get("base_url"))
                .and_then(|v| v.as_str()),
            Some("https://rightcode.example/v1"),
            "hot switch must not rewrite the original restore backup endpoint"
        );

        service
            .restore_live_config_for_app_with_fallback(&AppType::Codex)
            .await
            .expect("restore Codex live config");

        let live = service.read_codex_live().expect("read Codex live config");
        let live_config = live
            .get("config")
            .and_then(|v| v.as_str())
            .expect("live config string");
        let parsed_live: toml::Value = toml::from_str(live_config).expect("parse live config");
        assert_eq!(
            parsed_live.get("model_provider").and_then(|v| v.as_str()),
            Some("rightcode"),
            "restored Codex live config should not switch history buckets"
        );
        assert_eq!(
            live.get("auth")
                .and_then(|auth| auth.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str()),
            Some("rightcode-key"),
            "restore should return to the original pre-takeover auth"
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_live_backup_from_provider_keeps_new_codex_mcp_entries_on_conflict() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        db.save_live_backup(
            "codex",
            &serde_json::to_string(&json!({
                "auth": {
                    "OPENAI_API_KEY": "old-token"
                },
                "config": r#"[mcp_servers.shared]
command = "old-command"

[mcp_servers.legacy]
command = "legacy-command"
"#
            }))
            .expect("serialize seed backup"),
        )
        .await
        .expect("seed live backup");

        let provider = Provider::with_id(
            "p2".to_string(),
            "P2".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "new-token"
                },
                "config": r#"[mcp_servers.shared]
command = "new-command"

[mcp_servers.latest]
command = "latest-command"
"#
            }),
            None,
        );

        service
            .update_live_backup_from_provider("codex", &provider)
            .await
            .expect("update live backup");

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let config = stored
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config string");
        let parsed: toml::Value = toml::from_str(config).expect("parse merged codex config");

        let mcp_servers = parsed
            .get("mcp_servers")
            .expect("mcp_servers should be present");
        assert_eq!(
            mcp_servers
                .get("shared")
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str()),
            Some("new-command"),
            "new provider/common-config MCP definition should win on conflict"
        );
        assert_eq!(
            mcp_servers
                .get("legacy")
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str()),
            Some("legacy-command"),
            "backup-only MCP entries should still be preserved"
        );
        assert_eq!(
            mcp_servers
                .get("latest")
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str()),
            Some("latest-command"),
            "new MCP entries should remain in the restore backup"
        );
    }
}
