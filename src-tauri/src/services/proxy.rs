//! 代理服务业务逻辑层
//!
//! 提供代理服务器的启动、停止和配置管理

use crate::app_config::AppType;
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::server::ProxyServer;
use crate::proxy::types::*;
use crate::services::provider::write_live_snapshot;
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 用于接管 Live 配置时的占位符（避免客户端提示缺少 key，同时不泄露真实 Token）
const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";
const CODEX_DEFAULT_BASE_URL: &str = "https://api.openai.com";
const CODEX_PROXY_DUMMY_KEY: &str = "sk-cc-switch-proxy";
const CODEX_PRESERVED_FEATURE_FLAGS: [&str; 1] = ["multi_agent"];
#[cfg(target_os = "macos")]
const CODEX_PROXY_ENV_BLOCK_BEGIN: &str = "# >>> CC Switch Codex Proxy (managed) >>>";
#[cfg(target_os = "macos")]
const CODEX_PROXY_ENV_BLOCK_END: &str = "# <<< CC Switch Codex Proxy (managed) <<<";
#[cfg(target_os = "macos")]
const LEGACY_CODEX_PROXY_ENV_LAUNCH_AGENT_LABEL: &str = "com.ccswitch.codex-proxy-env";

/// 代理接管模式下需要从 Claude Live 配置中移除的“模型覆盖”字段。
///
/// 原因：接管模式切换供应商时不会写回 Live 配置，如果保留这些字段，
/// Claude Code 会继续以旧模型名发起请求，导致新供应商不支持时失败。
const CLAUDE_MODEL_OVERRIDE_ENV_KEYS: [&str; 6] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_REASONING_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    // Legacy key (已废弃)：历史版本使用该字段区分 small/fast 模型
    "ANTHROPIC_SMALL_FAST_MODEL",
];

#[derive(Clone)]
pub struct ProxyService {
    db: Arc<Database>,
    server: Arc<RwLock<Option<ProxyServer>>>,
    /// AppHandle，用于传递给 ProxyServer 以支持故障转移时的 UI 更新
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
}

impl ProxyService {
    fn is_proxy_managed_token(token: &str) -> bool {
        let trimmed = token.trim();
        trimmed.is_empty() || trimmed == PROXY_TOKEN_PLACEHOLDER || trimmed == CODEX_PROXY_DUMMY_KEY
    }

    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            server: Arc::new(RwLock::new(None)),
            app_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// 清理接管模式下 Claude Live 配置中的模型覆盖字段。
    ///
    /// 这可以避免“接管开启后切换供应商仍使用旧模型”的问题。
    /// 注意：此方法不会修改 Token/Base URL 的接管占位符，仅移除模型字段。
    pub fn cleanup_claude_model_overrides_in_live(&self) -> Result<(), String> {
        let mut config = self.read_claude_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
        };

        let mut changed = false;
        for key in CLAUDE_MODEL_OVERRIDE_ENV_KEYS {
            if env.remove(key).is_some() {
                changed = true;
            }
        }

        if changed {
            self.write_claude_live(&config)?;
        }

        Ok(())
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
            if app == AppType::Codex {
                self.prepare_codex_takeover_prerequisites().await?;
            }

            // 1) 代理服务未运行则自动启动
            if !self.is_running().await {
                self.start().await?;
            }

            // 2) 已接管则直接返回（幂等）；但如果缺少备份或占位符残留，需要重建接管
            let current_config = self
                .db
                .get_proxy_config_for_app(app_type_str)
                .await
                .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;

            if current_config.enabled {
                let has_backup = match self.db.get_live_backup(app_type_str).await {
                    Ok(v) => v.is_some(),
                    Err(e) => {
                        log::warn!("读取 {app_type_str} 备份失败（将继续重建接管）: {e}");
                        false
                    }
                };
                let live_taken_over = self.detect_takeover_in_live_config_for_app(&app);

                if has_backup || live_taken_over {
                    return Ok(());
                }

                log::warn!(
                    "{app_type_str} 标记为已接管，但缺少备份或占位符，正在重新接管并补齐备份"
                );
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
        self.restore_live_config_for_app(&app).await?;

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

        if app == AppType::Codex {
            if let Err(e) = self.clear_codex_proxy_environment().await {
                log::warn!("清理 Codex 代理环境变量失败: {e}");
            }
        }

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

            if let Err(e) = self.clear_codex_proxy_environment().await {
                log::warn!("清理 Codex 代理环境变量失败: {e}");
            }
        }

        Ok(())
    }

    async fn prepare_codex_takeover_prerequisites(&self) -> Result<(), String> {
        let mut providers = self
            .db
            .get_all_providers("codex")
            .map_err(|e| format!("读取 Codex Provider 列表失败: {e}"))?;
        let live_codex_config = self.read_codex_live().ok().and_then(|cfg| {
            cfg.get("config")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });

        if providers.is_empty() {
            return Err("未配置 Codex Provider，请先在供应商管理中添加账号".to_string());
        }

        let mut healthy_candidates = Vec::new();
        let mut all_provider_ids = Vec::new();

        for (provider_id, provider) in &mut providers {
            all_provider_ids.push(provider_id.clone());
            let mut settings = provider.settings_config.clone();
            let mut changed = false;

            let existing_base_url = Self::extract_codex_base_url(&settings);
            let endpoint = Self::codex_provider_first_endpoint(provider)
                .or_else(|| existing_base_url.clone())
                .unwrap_or_else(|| CODEX_DEFAULT_BASE_URL.to_string());

            if Self::codex_provider_first_endpoint(provider).is_none() {
                self.db
                    .add_custom_endpoint("codex", provider_id, &endpoint)
                    .map_err(|e| format!("补全 Codex Provider 端点失败 ({provider_id}): {e}"))?;
            }

            if existing_base_url.is_none() {
                Self::set_codex_base_url(&mut settings, &endpoint);
                changed = true;
            }

            if let Some(source_toml) = live_codex_config.as_deref() {
                let target_toml = settings
                    .get("config")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let merged_toml =
                    Self::sync_codex_preserved_feature_flags_from_source(target_toml, source_toml);
                if merged_toml != target_toml {
                    if !settings.is_object() {
                        settings = json!({});
                    }
                    if let Some(root) = settings.as_object_mut() {
                        root.insert("config".to_string(), json!(merged_toml));
                        changed = true;
                    }
                }
            }

            if Self::sync_codex_auth_openai_api_key(&mut settings) {
                changed = true;
            }

            if changed {
                self.db
                    .update_provider_settings_config("codex", provider_id, &settings)
                    .map_err(|e| format!("更新 Codex Provider 配置失败 ({provider_id}): {e}"))?;
            }

            let has_base_url = Self::extract_codex_base_url(&settings).is_some();
            let has_auth = Self::extract_codex_api_token(&settings).is_some();
            if has_base_url && has_auth {
                healthy_candidates.push(provider_id.clone());
            }
        }

        if healthy_candidates.is_empty() {
            return Err(
                "Codex Provider 自检失败：未找到同时具备 base_url 和可用认证信息的账号".to_string(),
            );
        }

        let current_id = crate::settings::get_effective_current_provider(&self.db, &AppType::Codex)
            .map_err(|e| format!("获取 Codex 当前供应商失败: {e}"))?;
        let current_is_healthy = current_id
            .as_ref()
            .map(|id| healthy_candidates.iter().any(|candidate| candidate == id))
            .unwrap_or(false);

        if !current_is_healthy {
            let next_provider = &healthy_candidates[0];
            self.db
                .set_current_provider("codex", next_provider)
                .map_err(|e| format!("切换 Codex 当前供应商失败: {e}"))?;
            crate::settings::set_current_provider(&AppType::Codex, Some(next_provider))
                .map_err(|e| format!("写入 Codex 当前供应商到本地设置失败: {e}"))?;
            log::info!("Codex 当前供应商已自动切换为 {next_provider}");
        }

        self.db
            .clear_provider_health_for_app("codex")
            .await
            .map_err(|e| format!("重置 Codex 健康状态失败: {e}"))?;
        for provider_id in &all_provider_ids {
            if let Err(e) = self
                .reset_provider_circuit_breaker(provider_id, "codex")
                .await
            {
                log::warn!("重置 Codex 熔断器失败 ({provider_id}): {e}");
            }
        }

        self.ensure_codex_proxy_environment().await?;
        Ok(())
    }

    fn normalize_codex_url(input: &str) -> Option<String> {
        let normalized = input.trim().trim_end_matches('/').to_string();
        if normalized.is_empty() {
            return None;
        }
        Some(normalized)
    }

    fn extract_codex_base_url(settings: &Value) -> Option<String> {
        if let Some(url) = settings.get("base_url").and_then(|v| v.as_str()) {
            return Self::normalize_codex_url(url);
        }
        if let Some(url) = settings.get("baseURL").and_then(|v| v.as_str()) {
            return Self::normalize_codex_url(url);
        }

        if let Some(config) = settings.get("config") {
            if let Some(url) = config.get("base_url").and_then(|v| v.as_str()) {
                return Self::normalize_codex_url(url);
            }

            if let Some(config_str) = config.as_str() {
                if let Some(start) = config_str.find("base_url = \"") {
                    let rest = &config_str[start + 12..];
                    if let Some(end) = rest.find('"') {
                        return Self::normalize_codex_url(&rest[..end]);
                    }
                }
                if let Some(start) = config_str.find("base_url = '") {
                    let rest = &config_str[start + 12..];
                    if let Some(end) = rest.find('\'') {
                        return Self::normalize_codex_url(&rest[..end]);
                    }
                }
            }
        }

        None
    }

    fn set_codex_base_url(settings: &mut Value, base_url: &str) {
        let normalized = Self::normalize_codex_url(base_url)
            .unwrap_or_else(|| CODEX_DEFAULT_BASE_URL.to_string());

        if let Some(config_str) = settings.get("config").and_then(|v| v.as_str()) {
            let updated = Self::update_toml_base_url(config_str, &normalized);
            if let Some(root) = settings.as_object_mut() {
                root.insert("config".to_string(), json!(updated));
                return;
            }
        }

        if !settings.is_object() {
            *settings = json!({});
        }
        if let Some(root) = settings.as_object_mut() {
            root.insert("base_url".to_string(), json!(normalized));
        }
    }

    fn extract_codex_api_token(settings: &Value) -> Option<String> {
        let from_env = settings
            .get("env")
            .and_then(|v| v.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str());
        if let Some(token) = from_env
            .map(str::trim)
            .filter(|v| !Self::is_proxy_managed_token(v))
        {
            return Some(token.to_string());
        }

        let from_auth = settings
            .get("auth")
            .and_then(|v| v.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str());
        if let Some(token) = from_auth
            .map(str::trim)
            .filter(|v| !Self::is_proxy_managed_token(v))
        {
            return Some(token.to_string());
        }

        let access_token = settings
            .get("auth")
            .and_then(|v| v.get("tokens"))
            .and_then(|v| v.get("access_token"))
            .and_then(|v| v.as_str());
        access_token
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
    }

    fn sync_codex_auth_openai_api_key(settings: &mut Value) -> bool {
        let has_api_key = settings
            .get("auth")
            .and_then(|v| v.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !Self::is_proxy_managed_token(v))
            .is_some();

        if has_api_key {
            return false;
        }

        let Some(access_token) = settings
            .get("auth")
            .and_then(|v| v.get("tokens"))
            .and_then(|v| v.get("access_token"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
        else {
            return false;
        };

        if !settings.is_object() {
            *settings = json!({});
        }

        if let Some(root) = settings.as_object_mut() {
            if !root.get("auth").map(|v| v.is_object()).unwrap_or(false) {
                root.insert("auth".to_string(), json!({}));
            }

            if let Some(auth_obj) = root.get_mut("auth").and_then(|v| v.as_object_mut()) {
                auth_obj.insert("OPENAI_API_KEY".to_string(), json!(access_token));
                return true;
            }
        }

        false
    }

    fn codex_provider_first_endpoint(provider: &Provider) -> Option<String> {
        let mut entries = Vec::new();
        if let Some(meta) = &provider.meta {
            for endpoint in meta.custom_endpoints.values() {
                if let Some(url) = Self::normalize_codex_url(&endpoint.url) {
                    entries.push((endpoint.added_at, url));
                }
            }
        }

        entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        entries.first().map(|(_, url)| url.clone())
    }

    async fn ensure_codex_proxy_environment(&self) -> Result<(), String> {
        let (_, codex_base_url) = self.build_proxy_urls().await?;
        #[cfg(target_os = "macos")]
        {
            Self::ensure_codex_proxy_env_for_macos(&codex_base_url, CODEX_PROXY_DUMMY_KEY)?;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = codex_base_url;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn ensure_codex_proxy_env_for_macos(base_url: &str, api_key: &str) -> Result<(), String> {
        use std::fs;
        use std::process::Command;

        fn run_command(command: &str, args: &[&str]) -> Result<(), String> {
            let status = Command::new(command)
                .args(args)
                .status()
                .map_err(|e| format!("执行命令失败: {command} {} ({e})", args.join(" ")))?;

            if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "命令执行失败: {command} {} (exit: {status})",
                    args.join(" ")
                ))
            }
        }

        fn upsert_managed_zshrc_block(base_url: &str, api_key: &str) -> Result<(), String> {
            let home = crate::config::get_home_dir();
            let zshrc_path = home.join(".zshrc");
            let content = fs::read_to_string(&zshrc_path).unwrap_or_default();

            let mut output_lines = Vec::new();
            let mut in_managed_block = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed == CODEX_PROXY_ENV_BLOCK_BEGIN {
                    in_managed_block = true;
                    continue;
                }
                if trimmed == CODEX_PROXY_ENV_BLOCK_END {
                    in_managed_block = false;
                    continue;
                }
                if !in_managed_block {
                    output_lines.push(line.to_string());
                }
            }

            let mut output = output_lines.join("\n");
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            if !output.is_empty() {
                output.push('\n');
            }

            output.push_str(CODEX_PROXY_ENV_BLOCK_BEGIN);
            output.push('\n');
            output.push_str(&format!("export OPENAI_BASE_URL=\"{base_url}\""));
            output.push('\n');
            output.push_str(&format!("export OPENAI_API_KEY=\"{api_key}\""));
            output.push('\n');
            output.push_str(CODEX_PROXY_ENV_BLOCK_END);
            output.push('\n');

            fs::write(&zshrc_path, output)
                .map_err(|e| format!("写入 ~/.zshrc 失败 ({}): {e}", zshrc_path.display()))?;
            Ok(())
        }

        fn cleanup_legacy_launch_agent() -> Result<(), String> {
            let home = crate::config::get_home_dir();
            let launch_agents_dir = home.join("Library").join("LaunchAgents");
            let plist_path = launch_agents_dir
                .join(format!("{LEGACY_CODEX_PROXY_ENV_LAUNCH_AGENT_LABEL}.plist"));
            let plist_path_str = plist_path.to_string_lossy().to_string();
            let _ = Command::new("launchctl")
                .args(["unload", plist_path_str.as_str()])
                .status();
            let _ = Command::new("launchctl")
                .args(["remove", LEGACY_CODEX_PROXY_ENV_LAUNCH_AGENT_LABEL])
                .status();
            if plist_path.exists() {
                fs::remove_file(&plist_path).map_err(|e| {
                    format!("删除旧 LaunchAgent 失败 ({}): {e}", plist_path.display())
                })?;
            }
            Ok(())
        }

        upsert_managed_zshrc_block(base_url, api_key)?;
        cleanup_legacy_launch_agent()?;

        run_command("launchctl", &["setenv", "OPENAI_BASE_URL", base_url])?;
        run_command("launchctl", &["setenv", "OPENAI_API_KEY", api_key])?;

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn clear_codex_proxy_env_for_macos() -> Result<(), String> {
        use std::fs;
        use std::process::Command;

        fn run_command(command: &str, args: &[&str]) -> Result<(), String> {
            let status = Command::new(command)
                .args(args)
                .status()
                .map_err(|e| format!("执行命令失败: {command} {} ({e})", args.join(" ")))?;
            if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "命令执行失败: {command} {} (exit: {status})",
                    args.join(" ")
                ))
            }
        }

        let home = crate::config::get_home_dir();
        let zshrc_path = home.join(".zshrc");
        if zshrc_path.exists() {
            let content = fs::read_to_string(&zshrc_path).unwrap_or_default();
            let mut output_lines = Vec::new();
            let mut in_managed_block = false;
            let mut removed = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed == CODEX_PROXY_ENV_BLOCK_BEGIN {
                    in_managed_block = true;
                    removed = true;
                    continue;
                }
                if trimmed == CODEX_PROXY_ENV_BLOCK_END {
                    in_managed_block = false;
                    continue;
                }
                if !in_managed_block {
                    output_lines.push(line.to_string());
                }
            }
            if removed {
                let mut output = output_lines.join("\n");
                if !output.is_empty() && !output.ends_with('\n') {
                    output.push('\n');
                }
                fs::write(&zshrc_path, output)
                    .map_err(|e| format!("更新 ~/.zshrc 失败 ({}): {e}", zshrc_path.display()))?;
            }
        }

        let _ = run_command("launchctl", &["unsetenv", "OPENAI_BASE_URL"]);
        let _ = run_command("launchctl", &["unsetenv", "OPENAI_API_KEY"]);

        let launch_agents_dir = home.join("Library").join("LaunchAgents");
        let plist_path =
            launch_agents_dir.join(format!("{LEGACY_CODEX_PROXY_ENV_LAUNCH_AGENT_LABEL}.plist"));
        let plist_path_str = plist_path.to_string_lossy().to_string();
        let _ = Command::new("launchctl")
            .args(["unload", plist_path_str.as_str()])
            .status();
        let _ = Command::new("launchctl")
            .args(["remove", LEGACY_CODEX_PROXY_ENV_LAUNCH_AGENT_LABEL])
            .status();
        if plist_path.exists() {
            let _ = fs::remove_file(plist_path);
        }

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn clear_codex_proxy_env_for_macos() -> Result<(), String> {
        Ok(())
    }

    async fn clear_codex_proxy_environment(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            Self::clear_codex_proxy_env_for_macos()?;
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
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features
                return Err("OpenCode 不支持代理功能".to_string());
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features
                return Err("OpenClaw 不支持代理功能".to_string());
            }
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
                            .filter(|s| !Self::is_proxy_managed_token(s))
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
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features, skip silently
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features, skip silently
            }
        }

        Ok(())
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

        if let Err(e) = self.clear_codex_proxy_environment().await {
            log::warn!("清理 Codex 代理环境变量失败: {e}");
        }

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
            let json_str = serde_json::to_string(&config)
                .map_err(|e| format!("序列化 Claude 配置失败: {e}"))?;
            self.db
                .save_live_backup("claude", &json_str)
                .await
                .map_err(|e| format!("备份 Claude 配置失败: {e}"))?;
        }

        // Codex
        if let Ok(config) = self.read_codex_live() {
            let json_str = serde_json::to_string(&config)
                .map_err(|e| format!("序列化 Codex 配置失败: {e}"))?;
            self.db
                .save_live_backup("codex", &json_str)
                .await
                .map_err(|e| format!("备份 Codex 配置失败: {e}"))?;
        }

        // Gemini
        if let Ok(config) = self.read_gemini_live() {
            let json_str = serde_json::to_string(&config)
                .map_err(|e| format!("序列化 Gemini 配置失败: {e}"))?;
            self.db
                .save_live_backup("gemini", &json_str)
                .await
                .map_err(|e| format!("备份 Gemini 配置失败: {e}"))?;
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
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features
                return Err("OpenCode 不支持代理功能".to_string());
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features
                return Err("OpenClaw 不支持代理功能".to_string());
            }
        };

        let json_str = serde_json::to_string(&config)
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
        if let Ok(mut live_config) = self.read_claude_live() {
            if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                env.insert("ANTHROPIC_BASE_URL".to_string(), json!(&proxy_url));
                // 关键：接管模式下移除模型覆盖字段，避免切换供应商后仍用旧模型名发起请求
                for key in CLAUDE_MODEL_OVERRIDE_ENV_KEYS {
                    env.remove(key);
                }
                // 仅覆盖已存在的 Token 字段，避免新增字段导致用户困惑；
                // 若完全没有 Token 字段，则写入 ANTHROPIC_AUTH_TOKEN 占位符用于避免客户端警告。
                let token_keys = [
                    "ANTHROPIC_AUTH_TOKEN",
                    "ANTHROPIC_API_KEY",
                    "OPENROUTER_API_KEY",
                    "OPENAI_API_KEY",
                ];

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
            } else {
                live_config["env"] = json!({
                    "ANTHROPIC_BASE_URL": &proxy_url,
                    "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER
                });
            }
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
            let updated_config = Self::update_toml_base_url(config_str, &proxy_codex_base_url);
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
                let mut live_config = self.read_claude_live()?;
                if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                    env.insert("ANTHROPIC_BASE_URL".to_string(), json!(&proxy_url));
                    // 关键：接管模式下移除模型覆盖字段，避免切换供应商后仍用旧模型名发起请求
                    for key in CLAUDE_MODEL_OVERRIDE_ENV_KEYS {
                        env.remove(key);
                    }

                    let token_keys = [
                        "ANTHROPIC_AUTH_TOKEN",
                        "ANTHROPIC_API_KEY",
                        "OPENROUTER_API_KEY",
                        "OPENAI_API_KEY",
                    ];

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
                } else {
                    live_config["env"] = json!({
                        "ANTHROPIC_BASE_URL": &proxy_url,
                        "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER
                    });
                }

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
                let updated_config = Self::update_toml_base_url(config_str, &proxy_codex_base_url);
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
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features
                return Err("OpenCode 不支持代理功能".to_string());
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features
                return Err("OpenClaw 不支持代理功能".to_string());
            }
        }

        Ok(())
    }

    /// 接管指定应用的 Live 配置（尽力而为：配置不存在/读取失败则跳过）
    async fn takeover_live_config_best_effort(&self, app_type: &AppType) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;

        match app_type {
            AppType::Claude => {
                if let Ok(mut live_config) = self.read_claude_live() {
                    if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                        env.insert("ANTHROPIC_BASE_URL".to_string(), json!(&proxy_url));
                        // 关键：接管模式下移除模型覆盖字段，避免切换供应商后仍用旧模型名发起请求
                        for key in CLAUDE_MODEL_OVERRIDE_ENV_KEYS {
                            env.remove(key);
                        }

                        let token_keys = [
                            "ANTHROPIC_AUTH_TOKEN",
                            "ANTHROPIC_API_KEY",
                            "OPENROUTER_API_KEY",
                            "OPENAI_API_KEY",
                        ];

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
                    } else {
                        live_config["env"] = json!({
                            "ANTHROPIC_BASE_URL": &proxy_url,
                            "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER
                        });
                    }

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
                        Self::update_toml_base_url(config_str, &proxy_codex_base_url);
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
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features, skip silently
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features, skip silently
            }
        }

        Ok(())
    }

    /// 恢复指定应用的 Live 配置（若无备份则不做任何操作）
    async fn restore_live_config_for_app(&self, app_type: &AppType) -> Result<(), String> {
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
                    let mut config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Codex 备份失败: {e}"))?;
                    if let Some(source_toml) = self.read_codex_live().ok().and_then(|v| {
                        v.get("config")
                            .and_then(|cfg| cfg.as_str())
                            .map(str::to_string)
                    }) {
                        Self::sync_codex_preserved_feature_flags_in_json(
                            &mut config,
                            source_toml.as_str(),
                        );
                    }
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
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features, skip silently
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features, skip silently
            }
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
        let app_type_str = app_type.as_str();

        // 1) 优先从 Live 备份恢复（这是“原始 Live”的唯一可靠来源）
        let backup = self
            .db
            .get_live_backup(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} Live 备份失败: {e}"))?;
        if let Some(backup) = backup {
            let mut config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 {app_type_str} 备份失败: {e}"))?;
            if matches!(app_type, AppType::Codex) {
                if let Some(source_toml) = self.read_codex_live().ok().and_then(|v| {
                    v.get("config")
                        .and_then(|cfg| cfg.as_str())
                        .map(str::to_string)
                }) {
                    Self::sync_codex_preserved_feature_flags_in_json(
                        &mut config,
                        source_toml.as_str(),
                    );
                }
            }
            self.write_live_config_for_app(app_type, &config)?;
            log::info!("{app_type_str} Live 配置已从备份恢复");
            return Ok(());
        }

        // 2) 兜底：备份缺失，但 Live 仍包含接管占位符（异常退出/历史 bug 场景）
        if !self.detect_takeover_in_live_config_for_app(app_type) {
            return Ok(());
        }

        // 2.1) 优先从 SSOT（当前供应商）重建 Live（比“清理字段”更可用）
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
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features
                Err("OpenCode 不支持代理功能".to_string())
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features
                Err("OpenClaw 不支持代理功能".to_string())
            }
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
            AppType::OpenCode => {
                // OpenCode doesn't support proxy takeover
                false
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy takeover
                false
            }
        }
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

        write_live_snapshot(app_type, provider)
            .map_err(|e| format!("写入 {app_type:?} Live 配置失败: {e}"))?;

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
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features
                Ok(())
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features
                Ok(())
            }
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

    fn remove_local_toml_base_url(toml_str: &str) -> String {
        use toml_edit::DocumentMut;

        let mut doc = match toml_str.parse::<DocumentMut>() {
            Ok(doc) => doc,
            Err(_) => return toml_str.to_string(),
        };

        let model_provider = doc
            .get("model_provider")
            .and_then(|item| item.as_str())
            .map(str::to_string);

        if let Some(provider_key) = model_provider {
            if let Some(model_providers) = doc
                .get_mut("model_providers")
                .and_then(|v| v.as_table_mut())
            {
                if let Some(provider_table) = model_providers
                    .get_mut(provider_key.as_str())
                    .and_then(|v| v.as_table_mut())
                {
                    let should_remove = provider_table
                        .get("base_url")
                        .and_then(|item| item.as_str())
                        .map(Self::is_local_proxy_url)
                        .unwrap_or(false);
                    if should_remove {
                        provider_table.remove("base_url");
                    }
                }
            }
        }

        // 兜底：清理顶层 base_url（仅当它看起来像本地代理地址）
        let should_remove_root = doc
            .get("base_url")
            .and_then(|item| item.as_str())
            .map(Self::is_local_proxy_url)
            .unwrap_or(false);
        if should_remove_root {
            doc.as_table_mut().remove("base_url");
        }

        doc.to_string()
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

    /// 检测 Live 配置是否处于“被接管”的残留状态
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
        let backup_json = match app_type {
            "claude" => {
                // Claude: settings_config 直接作为备份
                serde_json::to_string(&provider.settings_config)
                    .map_err(|e| format!("序列化 Claude 配置失败: {e}"))?
            }
            "codex" => {
                // Codex: settings_config 包含 {"auth": ..., "config": ...}
                // 在接管状态下保留 live 中用户显式开启的 feature flags，避免重启恢复时被旧备份覆盖。
                let mut backup_settings = provider.settings_config.clone();
                if let Some(source_toml) = self.read_codex_live().ok().and_then(|v| {
                    v.get("config")
                        .and_then(|cfg| cfg.as_str())
                        .map(str::to_string)
                }) {
                    Self::sync_codex_preserved_feature_flags_in_json(
                        &mut backup_settings,
                        source_toml.as_str(),
                    );
                }
                serde_json::to_string(&backup_settings)
                    .map_err(|e| format!("序列化 Codex 配置失败: {e}"))?
            }
            "gemini" => {
                // Gemini: 只提取 env 字段（与原始备份格式一致）
                // proxy.rs 的 read_gemini_live() 返回 {"env": {...}}
                let env_backup = if let Some(env) = provider.settings_config.get("env") {
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

    /// 代理模式下切换供应商（热切换，不写 Live）
    pub async fn switch_proxy_target(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), String> {
        // 代理模式切换供应商（热切换）：
        // - 更新 SSOT（数据库 is_current）
        // - 同步本地 settings（设备级 current_provider_*）
        // - 若该应用正处于接管模式，则同步更新 Live 备份（用于停止代理时恢复）
        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("无效的应用类型: {app_type}"))?;

        self.db
            .set_current_provider(app_type_enum.as_str(), provider_id)
            .map_err(|e| format!("更新当前供应商失败: {e}"))?;

        // 同步本地 settings（设备级优先）
        crate::settings::set_current_provider(&app_type_enum, Some(provider_id))
            .map_err(|e| format!("更新本地当前供应商失败: {e}"))?;

        // 仅在确实处于接管状态时才更新 Live 备份，避免无接管时误写覆盖 Live
        let has_backup = self
            .db
            .get_live_backup(app_type_enum.as_str())
            .await
            .ok()
            .flatten()
            .is_some();
        let live_taken_over = self.detect_takeover_in_live_config_for_app(&app_type_enum);

        if let Ok(Some(provider)) = self.db.get_provider_by_id(provider_id, app_type) {
            // 同步更新 Live 备份（用于 stop_with_restore 恢复）
            if has_backup || live_taken_over {
                self.update_live_backup_from_provider(app_type, &provider)
                    .await?;
            }

            // 同步更新 ProxyStatus.active_targets（用于 UI 立即反映切换目标）
            if let Some(server) = self.server.read().await.as_ref() {
                server
                    .set_active_target(app_type_enum.as_str(), &provider.id, &provider.name)
                    .await;
            }
        }

        log::info!("代理模式：已切换 {app_type} 的目标供应商为 {provider_id}");
        Ok(())
    }

    // ==================== Live 配置读写辅助方法 ====================

    /// 更新 TOML 字符串中的 base_url
    fn update_toml_base_url(toml_str: &str, new_url: &str) -> String {
        use toml_edit::DocumentMut;

        let mut doc = match toml_str.parse::<DocumentMut>() {
            Ok(doc) => doc,
            Err(_) => return toml_str.to_string(),
        };

        // Codex 的 config.toml 通常是：
        // model_provider = "any"
        //
        // [model_providers.any]
        // base_url = "https://.../v1"
        //
        // 所以接管时要“精准”修改当前 model_provider 对应的 model_providers.<name>.base_url，
        // 避免写错位置导致 Codex 仍然走旧地址。
        let model_provider = doc
            .get("model_provider")
            .and_then(|item| item.as_str())
            .map(str::to_string);

        if let Some(provider_key) = model_provider {
            if doc.get("model_providers").is_none() {
                doc["model_providers"] = toml_edit::table();
            }

            if let Some(model_providers) = doc["model_providers"].as_table_mut() {
                if !model_providers.contains_key(&provider_key) {
                    model_providers[&provider_key] = toml_edit::table();
                }

                if let Some(provider_table) = model_providers[&provider_key].as_table_mut() {
                    provider_table["base_url"] = toml_edit::value(new_url);
                    return doc.to_string();
                }
            }
        }

        // 兜底：如果没有 model_provider 或结构不符合预期，则退回修改顶层 base_url。
        doc["base_url"] = toml_edit::value(new_url);

        doc.to_string()
    }

    fn read_toml_feature_flag_bool(toml_str: &str, feature_name: &str) -> Option<bool> {
        if toml_str.trim().is_empty() {
            return None;
        }
        let parsed = match toml::from_str::<toml::Value>(toml_str) {
            Ok(v) => v,
            Err(_) => return None,
        };
        parsed
            .get("features")
            .and_then(|v| v.get(feature_name))
            .and_then(|v| v.as_bool())
    }

    fn set_toml_feature_flag_bool(toml_str: &str, feature_name: &str, value: bool) -> String {
        use toml_edit::DocumentMut;

        let mut doc = if toml_str.trim().is_empty() {
            DocumentMut::new()
        } else {
            match toml_str.parse::<DocumentMut>() {
                Ok(doc) => doc,
                Err(_) => return toml_str.to_string(),
            }
        };

        if doc.get("features").is_none() {
            doc["features"] = toml_edit::table();
        }
        if let Some(features) = doc.get_mut("features").and_then(|v| v.as_table_mut()) {
            features[feature_name] = toml_edit::value(value);
        }
        doc.to_string()
    }

    fn sync_codex_preserved_feature_flags_from_source(
        target_toml: &str,
        source_toml: &str,
    ) -> String {
        let mut output = target_toml.to_string();
        for feature_name in CODEX_PRESERVED_FEATURE_FLAGS {
            if let Some(value) = Self::read_toml_feature_flag_bool(source_toml, feature_name) {
                output = Self::set_toml_feature_flag_bool(&output, feature_name, value);
            }
        }
        output
    }

    fn sync_codex_preserved_feature_flags_in_json(config: &mut Value, source_toml: &str) {
        let target_toml = config.get("config").and_then(|v| v.as_str()).unwrap_or("");
        let merged = Self::sync_codex_preserved_feature_flags_from_source(target_toml, source_toml);

        if !config.is_object() {
            *config = json!({});
        }
        if let Some(root) = config.as_object_mut() {
            root.insert("config".to_string(), json!(merged));
        }
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
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());

            Self {
                dir,
                original_home,
                original_userprofile,
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
        }
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

    #[test]
    fn sync_codex_preserved_feature_flags_from_source_copies_multi_agent_when_enabled() {
        let target = r#"
model = "gpt-5.3-codex"
"#;

        let source = r#"
[features]
multi_agent = true
"#;

        let merged = ProxyService::sync_codex_preserved_feature_flags_from_source(target, source);
        let parsed: toml::Value = toml::from_str(&merged).expect("merged should be valid TOML");
        assert_eq!(
            parsed
                .get("features")
                .and_then(|v| v.get("multi_agent"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn sync_codex_preserved_feature_flags_from_source_copies_multi_agent_when_disabled() {
        let target = r#"
model = "gpt-5.3-codex"
"#;

        let source = r#"
[features]
multi_agent = false
"#;

        let merged = ProxyService::sync_codex_preserved_feature_flags_from_source(target, source);
        let parsed: toml::Value = toml::from_str(&merged).expect("merged should be valid TOML");
        assert_eq!(
            parsed
                .get("features")
                .and_then(|v| v.get("multi_agent"))
                .and_then(|v| v.as_bool()),
            Some(false)
        );
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

    #[tokio::test]
    #[serial]
    async fn switch_proxy_target_updates_live_backup_when_taken_over() {
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

        // 模拟“已接管”状态：存在 Live 备份（内容不重要，会被热切换更新）
        db.save_live_backup("claude", "{\"env\":{}}")
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

        // 断言：Live 备份已更新为目标供应商配置（用于 stop_with_restore 恢复）
        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let expected = serde_json::to_string(&provider_b.settings_config).expect("serialize");
        assert_eq!(backup.original_config, expected);
    }

    #[tokio::test]
    #[serial]
    async fn update_live_backup_from_provider_preserves_multi_agent_from_codex_live() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let codex_dir = crate::codex_config::get_codex_config_dir();
        std::fs::create_dir_all(&codex_dir).expect("create codex dir");
        write_json_file(
            &crate::codex_config::get_codex_auth_path(),
            &json!({
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": "live-token"
            }),
        )
        .expect("write codex auth");
        crate::config::write_text_file(
            &crate::codex_config::get_codex_config_path(),
            "model = \"gpt-5.3-codex\"\n[features]\nmulti_agent = true\n",
        )
        .expect("write codex config");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "codex-p1".to_string(),
            "Codex P1".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "provider-token"
                },
                "config": "model = \"gpt-5.3-codex\"\n"
            }),
            None,
        );

        service
            .update_live_backup_from_provider("codex", &provider)
            .await
            .expect("update codex live backup");

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("read backup")
            .expect("backup exists");
        let backup_json: serde_json::Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let backup_config = backup_json
            .get("config")
            .and_then(|v| v.as_str())
            .expect("backup config should be string");
        let parsed: toml::Value =
            toml::from_str(backup_config).expect("backup config should be valid TOML");
        assert_eq!(
            parsed
                .get("features")
                .and_then(|v| v.get("multi_agent"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn sync_codex_auth_openai_api_key_backfills_from_access_token() {
        let mut settings = json!({
            "auth": {
                "tokens": {
                    "access_token": "eyJ.test.access-token"
                }
            }
        });

        let changed = ProxyService::sync_codex_auth_openai_api_key(&mut settings);
        assert!(
            changed,
            "expected access token to be copied into OPENAI_API_KEY"
        );
        assert_eq!(
            settings
                .get("auth")
                .and_then(|v| v.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str()),
            Some("eyJ.test.access-token")
        );
    }

    #[tokio::test]
    #[serial]
    async fn prepare_codex_takeover_prerequisites_autofixes_provider_and_switches_current() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let broken = Provider::with_id(
            "broken".to_string(),
            "Broken".to_string(),
            json!({
                "auth": {}
            }),
            None,
        );
        let healthy = Provider::with_id(
            "healthy".to_string(),
            "Healthy".to_string(),
            json!({
                "auth": {
                    "tokens": {
                        "access_token": "eyJ-healthy-token"
                    }
                },
                "config": "model = \"gpt-5.3-codex\"\n"
            }),
            None,
        );

        db.save_provider("codex", &broken)
            .expect("save broken provider");
        db.save_provider("codex", &healthy)
            .expect("save healthy provider");
        db.set_current_provider("codex", "broken")
            .expect("set current provider");
        db.update_provider_health("broken", "codex", false, Some("boom".to_string()))
            .await
            .expect("seed provider health");

        service
            .prepare_codex_takeover_prerequisites()
            .await
            .expect("prepare codex prerequisites");

        let current = crate::settings::get_effective_current_provider(&db, &AppType::Codex)
            .expect("read effective current provider");
        assert_eq!(current.as_deref(), Some("healthy"));

        let updated = db
            .get_provider_by_id("healthy", "codex")
            .expect("read provider")
            .expect("provider exists");
        assert_eq!(
            updated
                .settings_config
                .get("auth")
                .and_then(|v| v.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str()),
            Some("eyJ-healthy-token")
        );
        assert!(
            ProxyService::extract_codex_base_url(&updated.settings_config).is_some(),
            "base_url should be auto-filled"
        );
        let endpoint_count = db
            .get_all_providers("codex")
            .expect("read providers with endpoints")
            .get("healthy")
            .and_then(|p| p.meta.as_ref())
            .as_ref()
            .map(|m| m.custom_endpoints.len())
            .unwrap_or(0);
        assert!(
            endpoint_count > 0,
            "provider_endpoints should be auto-filled for codex provider"
        );

        let health = db
            .get_provider_health("broken", "codex")
            .await
            .expect("read health after reset");
        assert_eq!(health.consecutive_failures, 0);
    }
}
