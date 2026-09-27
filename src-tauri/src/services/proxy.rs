//! 代理服务业务逻辑层
//!
//! 提供代理服务器的启动、停止和配置管理

use crate::app_config::AppType;
use crate::config::{
    get_claude_settings_path, read_json_file, write_json_file_private, write_text_file_private,
};
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::providers::codex_oauth_auth::{CodexLiveAuthSwitchGuard, CodexOAuthManager};
use crate::proxy::server::ProxyServer;
use crate::proxy::switch_lock::SwitchLockManager;
use crate::proxy::types::*;
use crate::services::provider::{
    build_effective_provider_for_live_with_codex_oauth_manager,
    build_effective_settings_with_common_config,
    write_live_with_common_config_for_codex_oauth_manager,
};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::RwLock;

use crate::live::project::claude::PROXY_TOKEN_PLACEHOLDER;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexAuthFileSnapshot {
    contents: Option<Vec<u8>>,
}

impl CodexAuthFileSnapshot {
    fn capture() -> Result<Self, String> {
        let path = crate::codex_config::get_codex_auth_path();
        match std::fs::read(&path) {
            Ok(contents) => Ok(Self {
                contents: Some(contents),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Self { contents: None })
            }
            Err(error) => Err(format!(
                "读取 Codex auth 失败 ({}): {error}",
                path.display()
            )),
        }
    }

    fn value(&self) -> Result<Option<Value>, String> {
        self.contents
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(|error| format!("读取 Codex auth 失败: {error}"))
    }
}

#[derive(Clone)]
pub struct ProxyService {
    db: Arc<Database>,
    codex_oauth_manager: Arc<CodexOAuthManager>,
    server: Arc<RwLock<Option<ProxyServer>>>,
    /// AppHandle，用于传递给 ProxyServer 以支持故障转移时的 UI 更新
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    switch_locks: SwitchLockManager,
}

impl ProxyService {
    pub fn new(db: Arc<Database>) -> Self {
        let codex_oauth_manager =
            Arc::new(CodexOAuthManager::new(crate::config::get_app_config_dir()));

        Self::new_with_codex_oauth_manager(db, codex_oauth_manager)
    }

    pub fn new_with_codex_oauth_manager(
        db: Arc<Database>,
        codex_oauth_manager: Arc<CodexOAuthManager>,
    ) -> Self {
        Self {
            db,
            codex_oauth_manager,
            server: Arc::new(RwLock::new(None)),
            app_handle: Arc::new(RwLock::new(None)),
            switch_locks: SwitchLockManager::new(),
        }
    }

    pub async fn sync_codex_live_from_provider_while_proxy_active(
        &self,
        provider: &Provider,
    ) -> Result<(), String> {
        self.sync_codex_live_from_provider_while_proxy_active_guarded(provider, None, None)
            .await
    }

    pub(crate) async fn sync_codex_live_from_provider_while_proxy_active_guarded(
        &self,
        provider: &Provider,
        outgoing_managed_account_id: Option<&str>,
        outgoing_guard: Option<&CodexLiveAuthSwitchGuard>,
    ) -> Result<(), String> {
        let existing_live = self.read_codex_live().ok();
        let mut effective_settings = build_effective_provider_for_live_with_codex_oauth_manager(
            self.db.as_ref(),
            &AppType::Codex,
            provider,
            &self.codex_oauth_manager,
        )
        .map_err(|e| format!("构建 codex 有效配置失败: {e}"))?
        .settings_config;
        if let Some(existing_live) = existing_live.as_ref() {
            Self::preserve_toml_mcp_servers_from_existing_config(
                &mut effective_settings,
                existing_live,
            )?;
        }
        let (_, proxy_codex_base_url) = self.build_proxy_urls().await?;

        Self::apply_codex_takeover_fields_for_provider(
            &mut effective_settings,
            &proxy_codex_base_url,
            provider,
        )?;

        if let (Some(account_id), Some(guard)) = (outgoing_managed_account_id, outgoing_guard) {
            guard
                .ensure_unchanged(account_id)
                .map_err(|error| error.to_string())?;
        }

        self.write_codex_takeover_live_for_provider(&effective_settings, Some(provider))?;
        Ok(())
    }

    pub async fn sync_grok_live_from_provider_while_proxy_active(
        &self,
        provider: &Provider,
    ) -> Result<(), String> {
        let existing_live = self.read_grok_live().ok();
        let mut effective_settings = build_effective_settings_with_common_config(
            self.db.as_ref(),
            &AppType::GrokBuild,
            provider,
        )
        .map_err(|e| format!("构建 Grok Build 有效配置失败: {e}"))?;
        if let Some(existing_live) = existing_live.as_ref() {
            Self::preserve_toml_mcp_servers_from_existing_config(
                &mut effective_settings,
                existing_live,
            )?;
        }
        let (proxy_url, _) = self.build_proxy_urls().await?;
        let proxy_grok_base_url = format!("{}/grokbuild/v1", proxy_url.trim_end_matches('/'));
        Self::apply_grok_takeover_fields(&mut effective_settings, &proxy_grok_base_url)?;
        self.write_grok_live(&effective_settings)
    }

    /// 设置 AppHandle（在应用初始化时调用）
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        futures::executor::block_on(async {
            *self.app_handle.write().await = Some(handle);
        });
    }

    pub(crate) async fn lock_switch_for_app(
        &self,
        app_type: &str,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.switch_locks.lock_for_app(app_type).await
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
        if let Err(e) = self
            .persist_ephemeral_listen_port_if_needed(&config, info.port)
            .await
        {
            let _ = server.stop().await;
            return Err(e);
        }

        // 5. 保存服务器实例
        *self.server.write().await = Some(server);

        log::info!("代理服务器已启动: {}:{}", info.address, info.port);
        Ok(info)
    }

    async fn persist_ephemeral_listen_port_if_needed(
        &self,
        config: &ProxyConfig,
        actual_port: u16,
    ) -> Result<(), String> {
        if config.listen_port != 0 {
            return Ok(());
        }

        // 端口是全局字段，不能通过旧接口回写各应用独立的重试和超时配置。
        let mut resolved_config = self
            .db
            .get_global_proxy_config()
            .await
            .map_err(|e| format!("获取全局代理配置失败: {e}"))?;
        resolved_config.listen_port = actual_port;
        self.db
            .update_global_proxy_config(resolved_config)
            .await
            .map_err(|e| format!("保存动态代理端口失败: {e}"))
    }

    /// 各应用是否处于代理模式（读设备本地的模式状态，不读会随云同步的
    /// `proxy_config.enabled`）。
    pub async fn get_takeover_status(&self) -> Result<ProxyTakeoverStatus, String> {
        use crate::mode::current::is_proxy;
        Ok(ProxyTakeoverStatus {
            claude: is_proxy(&AppType::Claude),
            codex: is_proxy(&AppType::Codex),
            gemini: is_proxy(&AppType::Gemini),
            grokbuild: is_proxy(&AppType::GrokBuild),
            // OpenCode and OpenClaw don't support proxy features
            opencode: false,
            openclaw: false,
        })
    }

    /// 把代理的上游指向 `provider`（状态面板里的「使用中」）。
    pub(crate) async fn set_active_target(&self, app_type: &AppType, provider: &Provider) {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .set_active_target(app_type.as_str(), &provider.id, &provider.name)
                .await;
        }
    }

    pub(crate) async fn emit(&self, event: &str, payload: Value) {
        if let Some(handle) = self.app_handle.read().await.as_ref() {
            if let Err(error) = handle.emit(event, payload) {
                log::warn!("发送事件 {event} 失败: {error}");
            }
        }
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

    /// 构造写入 Live 的代理地址（处理 0.0.0.0 / IPv6 等特殊情况）
    pub(crate) async fn build_proxy_urls(&self) -> Result<(String, String), String> {
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

        let mut listen_port = config.listen_port;
        if let Some(server) = self.server.read().await.as_ref() {
            let status = server.get_status().await;
            if status.running {
                listen_port = status.port;
            }
        }
        if listen_port == 0 {
            return Err("代理监听端口为 0，但代理服务器尚未运行，无法生成接管地址".to_string());
        }

        let proxy_origin = format!("http://{}:{}", connect_host_for_url, listen_port);
        let proxy_url = proxy_origin.clone();
        let proxy_codex_base_url = format!("{}/v1", proxy_origin.trim_end_matches('/'));

        Ok((proxy_url, proxy_codex_base_url))
    }

    fn apply_grok_takeover_fields(config: &mut Value, proxy_base_url: &str) -> Result<(), String> {
        let config_toml = config
            .get("config")
            .and_then(Value::as_str)
            .ok_or_else(|| "Grok Build 配置缺少 config 字段".to_string())?;
        let updated = crate::grok_config::apply_proxy_takeover(
            config_toml,
            proxy_base_url,
            PROXY_TOKEN_PLACEHOLDER,
        )
        .map_err(|e| format!("更新 Grok Build 接管配置失败: {e}"))?;
        config["config"] = json!(updated);
        Ok(())
    }

    /// 客户端文件里有没有接管占位符 `PROXY_MANAGED`（旧版接管的遗留物，或新版接上代理
    /// 时写的契约）。
    pub(crate) fn live_has_proxy_placeholder(&self, app_type: &AppType) -> bool {
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
            AppType::GrokBuild => match self.read_grok_live() {
                Ok(config) => Self::is_grok_live_taken_over(&config),
                Err(_) => false,
            },
            _ => false,
        }
    }

    /// 按直连指针的供应商整份写回 live（Codex 之外的 Gemini、Grok Build；改成只写关键
    /// 字段之前的过渡做法）。没有直连供应商、或行里本身带着占位符（旧版接管期间被导入
    /// 的残留）时，退一步只清掉占位符和本地代理地址。
    pub(crate) fn restore_live_from_direct_provider(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        match self.restore_live_from_ssot_for_app(app_type) {
            Ok(true) => return Ok(()),
            Ok(false) => log::warn!("{app_type:?} 没有可写回的直连供应商，只清理接管占位符"),
            Err(error) => {
                log::error!("{app_type:?} 写回直连供应商失败，只清理接管占位符: {error}")
            }
        }
        self.cleanup_takeover_placeholders_in_live_for_app(app_type)
    }

    /// 一份配置（客户端文件或供应商行）里有没有接管占位符。
    pub(crate) fn config_has_proxy_placeholder(app_type: &AppType, config: &Value) -> bool {
        Self::live_has_proxy_placeholder_for_app(app_type, config)
    }

    /// 只清掉客户端文件里的接管占位符和本地代理地址（直连供应商写不出来时的兜底）。
    pub(crate) fn clear_proxy_placeholders(&self, app_type: &AppType) -> Result<(), String> {
        self.cleanup_takeover_placeholders_in_live_for_app(app_type)
    }

    /// 返回值：
    /// - Ok(true)：已成功写回
    /// - Ok(false)：缺少直连供应商/供应商不存在/供应商本身含占位符，无法写回
    fn restore_live_from_ssot_for_app(&self, app_type: &AppType) -> Result<bool, String> {
        let current_id = crate::mode::current::provider_for(
            &self.db,
            app_type,
            crate::mode::current::Purpose::Direct,
        )
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

        // 供应商配置本身含接管占位符时不可写回（历史异常：接管期间 Live 被
        // 误导入成了供应商）。写回只会把占位符固化进 Live；返回 Ok(false)
        // 让调用方落到"清理占位符"兜底。
        if Self::live_has_proxy_placeholder_for_app(app_type, &provider.settings_config) {
            log::warn!(
                "{app_type:?} 当前供应商配置含代理接管占位符（疑似接管期间被导入的残留），跳过 SSOT 写回，改走占位符清理"
            );
            return Ok(false);
        }

        write_live_with_common_config_for_codex_oauth_manager(
            self.db.as_ref(),
            app_type,
            provider,
            &self.codex_oauth_manager,
        )
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
            AppType::GrokBuild => self.cleanup_grok_takeover_placeholders_in_live(),
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
            let updated =
                crate::codex_config::remove_codex_experimental_bearer_token_if(&updated, |token| {
                    token == PROXY_TOKEN_PLACEHOLDER
                })
                .map_err(|e| format!("清理 Codex 接管占位符失败: {e}"))?;
            let updated = crate::codex_config::remove_codex_official_proxy_route(&updated)
                .map_err(|e| format!("清理 Codex 官方接管路由失败: {e}"))?;
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

    fn cleanup_grok_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let config = self.read_grok_live()?;
        let Some(config_toml) = config.get("config").and_then(Value::as_str) else {
            return Ok(());
        };
        if !crate::grok_config::has_proxy_placeholder(config_toml, PROXY_TOKEN_PLACEHOLDER) {
            return Ok(());
        }

        // A valid provider snapshot should normally restore before this fallback.
        // Clearing the token prevents a stale local route from looking usable.
        let updated = crate::grok_config::update_api_key(config_toml, "")
            .map_err(|e| format!("清理 Grok Build 接管占位符失败: {e}"))?;
        write_text_file_private(&crate::grok_config::get_grok_config_path(), &updated)
            .map_err(|e| format!("写入 Grok Build 配置失败: {e}"))
    }

    /// 是否有应用处于代理模式
    pub async fn is_takeover_active(&self) -> Result<bool, String> {
        let status = self.get_takeover_status().await?;
        Ok(status.claude || status.codex || status.gemini || status.grokbuild)
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

    fn codex_live_has_proxy_placeholder(config: &Value) -> bool {
        if config
            .get("auth")
            .and_then(|v| v.as_object())
            .and_then(|auth| auth.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str())
            == Some(PROXY_TOKEN_PLACEHOLDER)
        {
            return true;
        }

        config
            .get("config")
            .and_then(|v| v.as_str())
            .and_then(crate::codex_config::extract_codex_experimental_bearer_token)
            .as_deref()
            == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    fn is_codex_live_taken_over(config: &Value) -> bool {
        Self::codex_live_has_proxy_placeholder(config)
            || config
                .get("config")
                .and_then(|v| v.as_str())
                .is_some_and(crate::codex_config::codex_config_has_official_proxy_route)
    }

    fn is_gemini_live_taken_over(config: &Value) -> bool {
        let env = match config.get("env").and_then(|v| v.as_object()) {
            Some(env) => env,
            None => return false,
        };
        env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    fn is_grok_live_taken_over(config: &Value) -> bool {
        config
            .get("config")
            .and_then(Value::as_str)
            .is_some_and(|config_toml| {
                crate::grok_config::has_proxy_placeholder(config_toml, PROXY_TOKEN_PLACEHOLDER)
            })
    }

    /// 判断给定的 Live/备份配置是否已被代理接管（包含占位符）
    ///
    /// 用途：检测"备份里存的其实是代理配置"这种异常历史状态。
    /// 如果发现，备份不可信，备份路径不能写入（否则会把代理配置固化进备份槽），
    /// 恢复路径不能读取（否则会把代理占位符原样写回 Live，永久卡在代理地址）。
    /// 两种情况下都应该走 SSOT 兜底重建 Live。
    fn live_has_proxy_placeholder_for_app(app_type: &AppType, config: &Value) -> bool {
        match app_type {
            AppType::Claude => Self::is_claude_live_taken_over(config),
            AppType::Codex => Self::is_codex_live_taken_over(config),
            AppType::Gemini => Self::is_gemini_live_taken_over(config),
            AppType::GrokBuild => Self::is_grok_live_taken_over(config),
            _ => false,
        }
    }

    fn preserve_toml_mcp_servers_from_existing_config(
        target_settings: &mut Value,
        existing_config: &Value,
    ) -> Result<(), String> {
        let target_obj = target_settings
            .as_object_mut()
            .ok_or_else(|| "TOML 应用备份必须是 JSON 对象".to_string())?;

        let target_config = target_obj
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mut target_doc = if target_config.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            target_config
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析新的 config.toml 失败: {e}"))?
        };

        let existing_config = existing_config
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if existing_config.trim().is_empty() {
            target_obj.insert("config".to_string(), json!(target_doc.to_string()));
            return Ok(());
        }

        let existing_doc = existing_config
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("解析现有 config.toml 备份失败: {e}"))?;

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
                            "config.toml contains a non-table mcp_servers section; skipping MCP merge"
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

    // ==================== Live 配置读写辅助方法 ====================

    /// 接管 Codex 时，本地客户端必须继续以 Responses wire API 访问代理。
    /// 真实上游是否走 Chat Completions 由 provider 配置决定，并在代理内部转换。
    fn apply_codex_proxy_toml_config_for_provider(
        toml_str: &str,
        proxy_url: &str,
        provider: Option<&Provider>,
    ) -> Result<String, String> {
        if provider.is_some_and(crate::proxy::providers::is_codex_official_provider) {
            return crate::codex_config::apply_codex_official_proxy_route(toml_str, proxy_url)
                .map_err(|e| format!("生成 Codex 官方接管配置失败: {e}"));
        }

        let updated = crate::codex_config::update_codex_toml_field(toml_str, "base_url", proxy_url)
            .map_err(|e| format!("更新 Codex 代理地址失败: {e}"))?;
        let mut updated =
            crate::codex_config::update_codex_toml_field(&updated, "wire_api", "responses")
                .map_err(|e| format!("更新 Codex wire_api 失败: {e}"))?;

        if let Some(upstream_model) =
            provider.and_then(crate::proxy::providers::codex_provider_upstream_model)
        {
            updated =
                crate::codex_config::update_codex_toml_field(&updated, "model", &upstream_model)
                    .map_err(|e| format!("更新 Codex 上游模型失败: {e}"))?;
        }

        Ok(updated)
    }

    fn apply_codex_takeover_auth_placeholder(settings: &mut Value, provider: Option<&Provider>) {
        if provider.is_some_and(crate::proxy::providers::is_codex_official_provider) {
            return;
        }

        if let Some(auth) = settings.get_mut("auth").and_then(|v| v.as_object_mut()) {
            auth.insert("OPENAI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
        } else if let Some(root) = settings.as_object_mut() {
            root.insert(
                "auth".to_string(),
                json!({ "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER }),
            );
        }
    }

    fn apply_codex_takeover_fields_for_provider(
        settings: &mut Value,
        proxy_base_url: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        Self::apply_codex_takeover_auth_placeholder(settings, Some(provider));
        let config_text = settings
            .get("config")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let projected = Self::apply_codex_proxy_toml_config_for_provider(
            &config_text,
            proxy_base_url,
            Some(provider),
        )?;
        settings["config"] = json!(projected);
        Self::attach_codex_model_catalog_from_provider(settings, Some(provider));
        Ok(())
    }

    fn attach_codex_model_catalog_from_provider(
        live_config: &mut Value,
        provider: Option<&Provider>,
    ) {
        let Some(provider) = provider else {
            return;
        };

        let model_catalog = provider
            .settings_config
            .get("modelCatalog")
            .cloned()
            .unwrap_or_else(|| json!({ "models": [] }));

        if let Some(root) = live_config.as_object_mut() {
            root.insert("modelCatalog".to_string(), model_catalog);
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
        write_json_file_private(&path, &settings).map_err(|e| format!("写入 Claude 配置失败: {e}"))
    }

    fn read_codex_live(&self) -> Result<Value, String> {
        crate::codex_config::read_codex_live_settings()
            .map_err(|e| format!("读取 Codex Live 配置失败: {e}"))
    }

    fn write_codex_live(&self, config: &Value) -> Result<(), String> {
        self.write_codex_live_verbatim(config)
    }

    fn write_codex_live_for_provider(
        &self,
        config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), String> {
        let Some(provider) = provider else {
            if crate::settings::preserve_codex_official_auth_on_switch() {
                if let (Some(auth), Some(config_str)) = (
                    config.get("auth"),
                    config.get("config").and_then(|v| v.as_str()),
                ) {
                    if auth.get("OPENAI_API_KEY").and_then(|v| v.as_str())
                        == Some(PROXY_TOKEN_PLACEHOLDER)
                    {
                        let live_config = crate::codex_config::prepare_codex_provider_live_config(
                            auth, config_str,
                        )
                        .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                        crate::codex_config::write_codex_live_config_atomic(Some(&live_config))
                            .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                        return Ok(());
                    }
                }
            }

            return self.write_codex_live_verbatim(config);
        };

        let auth = config
            .get("auth")
            .ok_or_else(|| "Codex 配置缺少 auth 字段".to_string())?;
        let config_str = config.get("config").and_then(|v| v.as_str());
        let profile = crate::proxy::providers::resolve_codex_catalog_tool_profile(provider);

        crate::codex_config::write_codex_provider_live_with_catalog(
            config,
            provider.category.as_deref(),
            auth,
            config_str,
            profile,
        )
        .map_err(|e| format!("写入 Codex 配置失败: {e}"))
    }

    fn codex_auth_has_proxy_placeholder(auth: &Value) -> bool {
        auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    /// The login state Codex will observe for `config_text`, as far as
    /// cc-switch can tell without touching the keyring: `Some(true)` signed
    /// in, `Some(false)` signed out, `None` undecidable. Which store Codex
    /// reads is decided first (`cli_auth_credentials_store`), and
    /// `auth.json` is only opened for the one mode that reads it:
    /// - `file` (the default): the file decides — see
    ///   `codex_auth_file_has_login`;
    /// - `ephemeral`: every process starts signed out, whatever is on disk;
    /// - `keyring`: Codex never opens the file (and deletes it after saving
    ///   to the keyring), so the file says nothing — undecidable;
    /// - `auto` (`AutoAuthStorage::load`): the keyring wins whenever it holds
    ///   anything and the file is only a fallback, so even a login in the
    ///   file cannot be ranked without reading the keyring — undecidable;
    /// - anything Codex would reject: undecidable.
    fn codex_live_login_state(config_text: &str) -> Option<bool> {
        use crate::codex_config::CodexAuthStoreMode;

        match crate::codex_config::codex_config_auth_store_mode(config_text) {
            CodexAuthStoreMode::File => Some(Self::codex_auth_file_has_login()),
            CodexAuthStoreMode::Ephemeral => Some(false),
            CodexAuthStoreMode::Keyring
            | CodexAuthStoreMode::Auto
            | CodexAuthStoreMode::Unknown => None,
        }
    }

    /// Whether the live `auth.json` holds a login Codex's file store would
    /// load. The takeover placeholder is not a login, and neither are
    /// Bedrock credentials or leftover metadata
    /// (`codex_auth_has_openai_account_material`). A file that is missing,
    /// unreadable or unparsable is "no stored auth" to Codex as well
    /// (`FileAuthStorage::load` fails and `AuthManager::load_auth` swallows
    /// it with `.ok()`), so it means signed out here and must never fail the
    /// takeover write.
    fn codex_auth_file_has_login() -> bool {
        let auth = match CodexAuthFileSnapshot::capture().and_then(|snapshot| snapshot.value()) {
            Ok(Some(auth)) => auth,
            Ok(None) => return false,
            Err(error) => {
                log::warn!("Codex auth.json 不可读，按未登录处理: {error}");
                return false;
            }
        };
        !Self::codex_auth_has_proxy_placeholder(&auth)
            && crate::codex_config::codex_auth_has_openai_account_material(&auth)
    }

    fn write_codex_takeover_live_for_provider(
        &self,
        config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), String> {
        let official_passthrough =
            provider.is_some_and(crate::proxy::providers::is_codex_official_provider);
        let managed_account_id = provider
            .and_then(|provider| provider.meta.as_ref())
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
            .filter(|account_id| !account_id.trim().is_empty());
        let managed_official = official_passthrough && managed_account_id.is_some();
        let placeholder_auth = config
            .get("auth")
            .is_some_and(Self::codex_auth_has_proxy_placeholder);

        // Takeover must never overwrite Codex's long-lived ChatGPT login. For
        // third-party providers the placeholder is moved into config.toml; for
        // codex-official no placeholder is needed because requires_openai_auth
        // makes Codex supply its native authorization.
        if official_passthrough || placeholder_auth {
            let config_str = config.get("config").and_then(|v| v.as_str()).unwrap_or("");
            let profile = provider
                .map(crate::proxy::providers::resolve_codex_catalog_tool_profile)
                .unwrap_or(crate::codex_config::CodexCatalogToolProfile::ProxyChat);
            let prepared_config =
                crate::codex_config::prepare_codex_live_config_text_with_optional_catalog(
                    config, config_str, profile,
                )
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
            if managed_official {
                let auth = config
                    .get("auth")
                    .ok_or_else(|| "Codex 托管官方配置缺少 auth 字段".to_string())?;
                // An explicitly managed official account is different from the
                // unbound native-login passthrough: the selected account owns
                // auth.json and must replace any previously active account.
                crate::codex_config::write_codex_live_for_provider(
                    Some("official"),
                    auth,
                    Some(&prepared_config),
                )
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                crate::codex_config::record_codex_managed_oauth_live_auth(
                    auth,
                    managed_account_id
                        .as_deref()
                        .expect("managed official account checked"),
                )
                .map_err(|e| format!("记录 Codex 托管认证标记失败: {e}"))?;
                return Ok(());
            }
            let live_config = if official_passthrough {
                prepared_config
            } else {
                let injected = crate::codex_config::prepare_codex_provider_live_config(
                    config.get("auth").unwrap_or(&Value::Null),
                    &prepared_config,
                )
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                // Takeover never touches auth.json, but it no longer owns the
                // file's presence: a preservation-off direct switch deletes
                // the login before takeover is enabled, and `codex logout`
                // can remove it mid-takeover. The stored card's
                // `requires_openai_auth` (presets carried `true` from the
                // pre-0.149 era) would then trap the TUI in the login screen
                // — Codex decides that screen from the flag and its account
                // probe alone, never from the bearer token — so stamp the
                // flag to the observed login state, exactly as the direct
                // switch does. When the state is undecidable from disk
                // (keyring-backed or auto stores) the card's flag is left
                // alone. Proxy-injected OAuth cards (xai_oauth, copilot) are
                // excluded outright: the effective snapshot already carries
                // the neutralized `false` (`neutralize_codex_proxy_oauth_fallback`)
                // because the official login is never their credential, and
                // a login on disk must not raise it back to `true`.
                let proxy_injected_oauth =
                    provider.is_some_and(Provider::uses_proxy_injected_oauth);
                let live_login_state = if proxy_injected_oauth {
                    None
                } else {
                    Self::codex_live_login_state(&injected)
                };
                match live_login_state {
                    Some(live_has_login) => {
                        crate::codex_config::align_codex_requires_openai_auth_with_login_preservation(
                            &injected,
                            live_has_login,
                        )
                        .map_err(|e| format!("写入 Codex 配置失败: {e}"))?
                    }
                    None => injected,
                }
            };
            crate::codex_config::write_codex_live_config_atomic(Some(&live_config))
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
            return Ok(());
        }

        self.write_codex_live_for_provider(config, provider)
    }

    /// 把一份 `{auth, config, modelCatalog?}` 原样写进 Codex live（清理占位符用）。
    /// `auth` 为空对象时保留现有的登录，不删 auth.json。
    fn write_codex_live_verbatim(&self, config: &Value) -> Result<(), String> {
        use crate::codex_config::{get_codex_auth_path, get_codex_config_path};

        let auth = config.get("auth");
        let config_str = config.get("config").and_then(|v| v.as_str());
        // 没有 Provider 可用，模型目录的工具形态按 ProxyChat 处理。
        let prepared_cfg = config_str
            .map(|cfg| {
                crate::codex_config::prepare_codex_live_config_text_with_optional_catalog(
                    config,
                    cfg,
                    crate::codex_config::CodexCatalogToolProfile::ProxyChat,
                )
            })
            .transpose()
            .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;

        match (auth, prepared_cfg.as_deref()) {
            (Some(auth), Some(cfg)) => {
                if auth.as_object().is_some_and(Map::is_empty) {
                    write_text_file_private(&get_codex_config_path(), cfg)
                        .map_err(|e| format!("写入 Codex config 失败: {e}"))
                } else {
                    crate::codex_config::write_codex_live_atomic(auth, Some(cfg))
                        .map_err(|e| format!("写入 Codex 配置失败: {e}"))
                }
            }
            (Some(auth), None) => {
                if auth.as_object().is_some_and(Map::is_empty) {
                    Ok(())
                } else {
                    write_json_file_private(&get_codex_auth_path(), auth)
                        .map_err(|e| format!("写入 Codex auth 失败: {e}"))
                }
            }
            (None, Some(cfg)) => write_text_file_private(&get_codex_config_path(), cfg)
                .map_err(|e| format!("写入 Codex config 失败: {e}")),
            (None, None) => Ok(()),
        }
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

    fn read_grok_live(&self) -> Result<Value, String> {
        crate::grok_config::read_grok_live_settings()
            .map_err(|e| format!("读取 Grok Build 配置失败: {e}"))
    }

    fn write_grok_live(&self, config: &Value) -> Result<(), String> {
        crate::grok_config::write_grok_live_settings(config)
            .map_err(|e| format!("写入 Grok Build 配置失败: {e}"))
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

    /// 更新代理配置。返回代理是否因地址或端口变了而重启：重启后调用方要按新地址重写
    /// 接上代理的客户端（`mode::controller::resync_route`）。
    pub async fn update_config(&self, config: &ProxyConfig) -> Result<bool, String> {
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
            return Ok(false);
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
            let new_server = ProxyServer::new(new_config.clone(), self.db.clone(), app_handle);
            let info = new_server
                .start()
                .await
                .map_err(|e| format!("重启代理服务器失败: {e}"))?;
            if let Err(e) = self
                .persist_ephemeral_listen_port_if_needed(&new_config, info.port)
                .await
            {
                let _ = new_server.stop().await;
                return Err(e);
            }

            *server_guard = Some(new_server);
            log::info!("代理配置已更新，服务器已自动重启应用最新配置");

            return Ok(true);
        } else if let Some(server) = server_guard.as_ref() {
            server.apply_runtime_config(&new_config).await;
            log::info!("代理配置已实时应用，无需重启代理服务器");
        }

        Ok(false)
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
    use crate::provider::{AuthBinding, AuthBindingSource, ProviderMeta};
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

    async fn seed_distinct_app_proxy_configs(db: &Database) -> Vec<Value> {
        let mut configs = Vec::new();
        for (app, retries) in [("claude", 6), ("codex", 0), ("gemini", 2), ("grokbuild", 3)] {
            let mut config = db.get_proxy_config_for_app(app).await.unwrap();
            config.enabled = retries % 2 == 0;
            config.auto_failover_enabled = retries % 2 != 0;
            config.max_retries = retries;
            config.streaming_first_byte_timeout = 30 + retries;
            config.streaming_idle_timeout = 90 + retries;
            config.non_streaming_timeout = 300 + retries;
            config.circuit_failure_threshold = 5 + retries;
            configs.push(serde_json::to_value(&config).unwrap());
            db.update_proxy_config_for_app(config).await.unwrap();
        }
        configs
    }

    async fn assert_app_proxy_configs_unchanged(db: &Database, configs: &[Value]) {
        for expected in configs {
            let app = expected["appType"].as_str().unwrap();
            let actual = db.get_proxy_config_for_app(app).await.unwrap();
            assert_eq!(serde_json::to_value(actual).unwrap(), *expected, "{app}");
        }
    }

    #[tokio::test]
    async fn ephemeral_port_preserves_app_proxy_configs() {
        let db = Arc::new(Database::memory().unwrap());
        let configs = seed_distinct_app_proxy_configs(&db).await;
        let service = ProxyService::new(db.clone());
        let mut config = db.get_proxy_config().await.unwrap();
        config.listen_port = 0;
        let mut expected_global = db.get_global_proxy_config().await.unwrap();
        expected_global.listen_port = 23456;

        service
            .persist_ephemeral_listen_port_if_needed(&config, 23456)
            .await
            .unwrap();

        assert_app_proxy_configs_unchanged(&db, &configs).await;
        assert_eq!(
            serde_json::to_value(db.get_global_proxy_config().await.unwrap()).unwrap(),
            serde_json::to_value(expected_global).unwrap()
        );

        config.listen_port = 23456;
        service
            .persist_ephemeral_listen_port_if_needed(&config, 34567)
            .await
            .unwrap();
        assert_eq!(
            db.get_global_proxy_config().await.unwrap().listen_port,
            23456
        );
        assert_app_proxy_configs_unchanged(&db, &configs).await;
    }

    #[test]
    #[serial]
    fn codex_custom_provider_live_write_preserves_oauth_auth_json() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: true,
            ..Default::default()
        })
        .expect("enable Codex official auth preservation");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        let oauth_auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": "oauth-id",
                "access_token": "oauth-access"
            }
        });
        crate::codex_config::write_codex_live_atomic(
            &oauth_auth,
            Some(
                r#"model_provider = "openai"
model = "gpt-5-codex"
"#,
            ),
        )
        .expect("seed live OAuth auth");

        let mut provider = Provider::with_id(
            "rightcode".to_string(),
            "RightCode".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "rightcode-key"
                },
                "config": r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        provider.category = Some("custom".to_string());
        let takeover_settings = json!({
            "auth": {
                "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
            },
            "config": r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
name = "RightCode"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
"#
        });

        service
            .write_codex_live_for_provider(&takeover_settings, Some(&provider))
            .expect("write provider-driven Codex live config");

        let live_auth: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read live auth");
        assert_eq!(
            live_auth, oauth_auth,
            "third-party Codex proxy writes must not overwrite ChatGPT OAuth login state"
        );

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains("experimental_bearer_token"),
            "proxy placeholder should move into config.toml instead of auth.json"
        );
        assert!(
            live_config.contains(PROXY_TOKEN_PLACEHOLDER),
            "live config should carry the proxy placeholder token"
        );

        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset settings");
    }

    #[tokio::test]
    #[serial]
    async fn codex_takeover_preserves_oauth_auth_json_when_preserve_enabled() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: true,
            ..Default::default()
        })
        .expect("enable Codex official auth preservation");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());
        let oauth_auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": "oauth-id",
                "access_token": "oauth-access"
            }
        });
        let deepseek_live_config = r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
experimental_bearer_token = "deepseek-key"
"#;
        crate::codex_config::write_codex_live_atomic(&oauth_auth, Some(deepseek_live_config))
            .expect("seed live OAuth auth with DeepSeek config");

        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "deepseek-key"
                },
                "config": r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        provider.category = Some("cn_official".to_string());
        db.save_provider("codex", &provider)
            .expect("save DeepSeek provider");
        db.set_current_provider("codex", "deepseek")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("deepseek"))
            .expect("set local current provider");

        service
            .sync_codex_live_from_provider_while_proxy_active(&provider)
            .await
            .expect("take over Codex live config");

        let live_auth: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read live auth");
        assert_eq!(
            live_auth, oauth_auth,
            "Codex takeover should not overwrite ChatGPT OAuth auth when preservation is enabled"
        );

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains(PROXY_TOKEN_PLACEHOLDER),
            "takeover placeholder should move into config.toml"
        );
        assert!(
            service.live_has_proxy_placeholder(&AppType::Codex),
            "Codex takeover detection should recognize config.toml placeholders"
        );

        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset settings");
    }

    #[tokio::test]
    #[serial]
    async fn codex_takeover_preserves_oauth_auth_json_even_when_provider_category_is_official() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: true,
            ..Default::default()
        })
        .expect("enable Codex official auth preservation");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());
        let oauth_auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": "oauth-id",
                "access_token": "oauth-access"
            }
        });
        let deepseek_live_config = r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
experimental_bearer_token = "deepseek-key"
"#;
        crate::codex_config::write_codex_live_atomic(&oauth_auth, Some(deepseek_live_config))
            .expect("seed live OAuth auth with DeepSeek config");

        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "deepseek-key"
                },
                "config": r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        provider.category = Some("official".to_string());
        db.save_provider("codex", &provider)
            .expect("save misclassified DeepSeek provider");
        db.set_current_provider("codex", "deepseek")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("deepseek"))
            .expect("set local current provider");

        service
            .sync_codex_live_from_provider_while_proxy_active(&provider)
            .await
            .expect("take over Codex live config");

        let live_auth: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read live auth");
        assert_eq!(
            live_auth, oauth_auth,
            "Codex takeover must not rewrite auth.json when preservation is enabled, even if provider category is stale or misclassified"
        );

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains(PROXY_TOKEN_PLACEHOLDER),
            "takeover placeholder should move into config.toml"
        );

        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset settings");
    }

    #[test]
    #[serial]
    fn codex_empty_restore_snapshot_does_not_delete_existing_auth_json() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        let auth = json!({ "OPENAI_API_KEY": "sk-real" });
        crate::codex_config::write_codex_live_atomic(&auth, Some("model = \"gpt-5.4\"\n"))
            .expect("seed auth");

        service
            .write_codex_live_verbatim(&json!({
                "auth": {},
                "config": "model = \"gpt-5.4-mini\"\n"
            }))
            .expect("restore empty snapshot");

        let live_auth: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read auth");
        assert_eq!(live_auth, auth);
    }

    #[tokio::test]
    #[serial]
    async fn codex_takeover_preserves_native_auth_even_when_legacy_toggle_is_disabled() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: false,
            ..Default::default()
        })
        .expect("disable Codex official auth preservation");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());
        let oauth_auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": "oauth-id",
                "access_token": "oauth-access"
            }
        });
        let deepseek_live_config = r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#;
        crate::codex_config::write_codex_live_atomic(&oauth_auth, Some(deepseek_live_config))
            .expect("seed live OAuth auth with DeepSeek config");

        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "deepseek-key"
                },
                "config": r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        provider.category = Some("cn_official".to_string());
        db.save_provider("codex", &provider)
            .expect("save DeepSeek provider");
        db.set_current_provider("codex", "deepseek")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("deepseek"))
            .expect("set local current provider");

        service
            .sync_codex_live_from_provider_while_proxy_active(&provider)
            .await
            .expect("take over Codex live config");

        let live_auth: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read live auth");
        assert_eq!(
            live_auth, oauth_auth,
            "takeover must preserve native OAuth independently of the legacy toggle"
        );

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains(PROXY_TOKEN_PLACEHOLDER),
            "third-party takeover should carry its local placeholder in config.toml"
        );

        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset settings");
    }

    #[test]
    #[serial]
    fn codex_takeover_cleanup_removes_config_placeholder_without_touching_oauth_auth() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        let oauth_auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": "oauth-id",
                "access_token": "oauth-access"
            }
        });
        crate::codex_config::write_codex_live_atomic(
            &oauth_auth,
            Some(
                r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
experimental_bearer_token = "PROXY_MANAGED"
"#,
            ),
        )
        .expect("seed taken-over Codex live config");

        assert!(
            service.live_has_proxy_placeholder(&AppType::Codex),
            "config.toml placeholder should be detected before cleanup"
        );

        service
            .cleanup_codex_takeover_placeholders_in_live()
            .expect("cleanup Codex takeover placeholders");

        let live_auth: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read live auth");
        assert_eq!(
            live_auth, oauth_auth,
            "cleanup should preserve ChatGPT OAuth auth"
        );

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            !live_config.contains(PROXY_TOKEN_PLACEHOLDER),
            "cleanup should remove config.toml proxy bearer placeholder"
        );
        assert!(
            !live_config.contains("http://127.0.0.1:15721"),
            "cleanup should remove local proxy base_url"
        );
    }

    #[test]
    #[serial]
    fn codex_takeover_stamps_requires_openai_auth_false_when_auth_json_absent() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        // Preservation off: the direct switch that preceded takeover already
        // deleted auth.json (see plan_codex_live_write).
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: false,
            ..Default::default()
        })
        .expect("disable Codex official auth preservation");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        assert!(!crate::codex_config::get_codex_auth_path().exists());

        // Stored cards carry `requires_openai_auth = true` from the pre-0.149
        // presets; the takeover projection must not replay it onto a login-less
        // auth.json or Codex traps the TUI in the login screen.
        let mut provider = Provider::with_id(
            "kimi".to_string(),
            "Kimi".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "kimi-key" },
                "config": r#"model_provider = "kimi"
model = "kimi-k2"

[model_providers.kimi]
name = "Kimi"
base_url = "https://api.moonshot.cn/v1"
wire_api = "responses"
requires_openai_auth = true
"#
            }),
            None,
        );
        provider.category = Some("custom".to_string());

        let mut takeover_settings = provider.settings_config.clone();
        ProxyService::apply_codex_takeover_fields_for_provider(
            &mut takeover_settings,
            "http://127.0.0.1:15721/v1",
            &provider,
        )
        .expect("apply takeover fields");
        service
            .write_codex_takeover_live_for_provider(&takeover_settings, Some(&provider))
            .expect("write takeover live config");

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains("requires_openai_auth = false"),
            "no login on disk: the stored `true` must be stamped false; got:\n{live_config}"
        );
        assert!(
            live_config.contains(&format!(
                "experimental_bearer_token = \"{PROXY_TOKEN_PLACEHOLDER}\""
            )),
            "placeholder must still ride as the provider token; got:\n{live_config}"
        );
        assert!(live_config.contains("base_url = \"http://127.0.0.1:15721/v1\""));
        assert!(
            !crate::codex_config::get_codex_auth_path().exists(),
            "takeover must not create auth.json"
        );

        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset settings");
    }

    #[test]
    #[serial]
    fn codex_takeover_stamps_requires_openai_auth_true_when_login_present() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        let oauth_auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": "oauth-id",
                "access_token": "oauth-access",
                "refresh_token": "oauth-refresh"
            }
        });
        crate::codex_config::write_codex_live_atomic(
            &oauth_auth,
            Some(
                r#"model_provider = "openai"
model = "gpt-5-codex"
"#,
            ),
        )
        .expect("seed live OAuth auth");

        // The card omits the flag: with a login on disk it is stamped true so
        // Codex keeps showing the account and refreshing the preserved tokens
        // (the placeholder bearer token still short-circuits request auth).
        let mut provider = Provider::with_id(
            "kimi".to_string(),
            "Kimi".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "kimi-key" },
                "config": r#"model_provider = "kimi"
model = "kimi-k2"

[model_providers.kimi]
name = "Kimi"
base_url = "https://api.moonshot.cn/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        provider.category = Some("custom".to_string());

        let mut takeover_settings = provider.settings_config.clone();
        ProxyService::apply_codex_takeover_fields_for_provider(
            &mut takeover_settings,
            "http://127.0.0.1:15721/v1",
            &provider,
        )
        .expect("apply takeover fields");
        service
            .write_codex_takeover_live_for_provider(&takeover_settings, Some(&provider))
            .expect("write takeover live config");

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains("requires_openai_auth = true"),
            "login on disk: the flag must be stamped true; got:\n{live_config}"
        );
        assert!(live_config.contains(&format!(
            "experimental_bearer_token = \"{PROXY_TOKEN_PLACEHOLDER}\""
        )));

        let auth_after: Value = serde_json::from_str(
            &std::fs::read_to_string(crate::codex_config::get_codex_auth_path())
                .expect("read auth.json"),
        )
        .expect("parse auth.json");
        assert_eq!(
            auth_after, oauth_auth,
            "takeover must leave the OAuth login untouched"
        );
    }

    #[test]
    #[serial]
    fn codex_takeover_does_not_stamp_true_over_bedrock_only_auth_json() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        // Bedrock credentials are a login for the Bedrock provider only: on
        // any requires_openai_auth provider Codex's account probe returns
        // UnsupportedBedrockApiKeyAuth and the TUI fails to start.
        let bedrock_auth = json!({ "bedrock_api_key": "bedrock-key" });
        crate::codex_config::write_codex_live_atomic(
            &bedrock_auth,
            Some("model_provider = \"amazon-bedrock\"\nmodel = \"claude\"\n"),
        )
        .expect("seed bedrock auth");

        let mut provider = Provider::with_id(
            "kimi".to_string(),
            "Kimi".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "kimi-key" },
                "config": r#"model_provider = "kimi"
model = "kimi-k2"

[model_providers.kimi]
name = "Kimi"
base_url = "https://api.moonshot.cn/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        provider.category = Some("custom".to_string());

        let mut takeover_settings = provider.settings_config.clone();
        ProxyService::apply_codex_takeover_fields_for_provider(
            &mut takeover_settings,
            "http://127.0.0.1:15721/v1",
            &provider,
        )
        .expect("apply takeover fields");
        service
            .write_codex_takeover_live_for_provider(&takeover_settings, Some(&provider))
            .expect("write takeover live config");

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            !live_config.contains("requires_openai_auth = true"),
            "bedrock-only auth.json must never be promoted to an OpenAI login; got:\n{live_config}"
        );
        assert!(live_config.contains("requires_openai_auth = false"));
        let auth_after: Value = serde_json::from_str(
            &std::fs::read_to_string(crate::codex_config::get_codex_auth_path())
                .expect("read auth.json"),
        )
        .expect("parse auth.json");
        assert_eq!(
            auth_after, bedrock_auth,
            "takeover must leave auth.json untouched"
        );
    }

    #[test]
    #[serial]
    fn codex_takeover_keeps_stored_flag_when_auth_store_is_keyring_backed() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        assert!(!crate::codex_config::get_codex_auth_path().exists());

        // Codex deletes auth.json after saving to the keyring, so an absent
        // file says nothing about login state under these store modes: the
        // stored `true` must survive instead of being rewritten to false
        // (which would hide a valid keyring login).
        for mode in ["keyring", "auto"] {
            let mut provider = Provider::with_id(
                "kimi".to_string(),
                "Kimi".to_string(),
                json!({
                    "auth": { "OPENAI_API_KEY": "kimi-key" },
                    "config": format!(
                        r#"model_provider = "kimi"
model = "kimi-k2"
cli_auth_credentials_store = "{mode}"

[model_providers.kimi]
name = "Kimi"
base_url = "https://api.moonshot.cn/v1"
wire_api = "responses"
requires_openai_auth = true
"#
                    )
                }),
                None,
            );
            provider.category = Some("custom".to_string());

            let mut takeover_settings = provider.settings_config.clone();
            ProxyService::apply_codex_takeover_fields_for_provider(
                &mut takeover_settings,
                "http://127.0.0.1:15721/v1",
                &provider,
            )
            .expect("apply takeover fields");
            service
                .write_codex_takeover_live_for_provider(&takeover_settings, Some(&provider))
                .expect("write takeover live config");

            let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
                .expect("read live config");
            assert!(
                live_config.contains("requires_openai_auth = true"),
                "store mode {mode}: stored flag must be left alone; got:\n{live_config}"
            );
            assert!(
                live_config.contains(&format!("cli_auth_credentials_store = \"{mode}\"")),
                "store mode key must survive projection; got:\n{live_config}"
            );
            assert!(!crate::codex_config::get_codex_auth_path().exists());
        }
    }

    #[test]
    #[serial]
    fn codex_takeover_stamps_false_when_bedrock_outranks_stale_openai_key() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        // AuthDotJson::resolved_mode ranks bedrock_api_key above
        // OPENAI_API_KEY, so Codex loads this as Bedrock auth; a stamped
        // `true` would make account_state() fail with
        // UnsupportedBedrockApiKeyAuth.
        let mixed_auth = json!({
            "OPENAI_API_KEY": "sk-stale",
            "bedrock_api_key": "bedrock-key"
        });
        crate::codex_config::write_codex_live_atomic(
            &mixed_auth,
            Some("model_provider = \"amazon-bedrock\"\nmodel = \"claude\"\n"),
        )
        .expect("seed mixed auth");

        let mut provider = Provider::with_id(
            "kimi".to_string(),
            "Kimi".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "kimi-key" },
                "config": r#"model_provider = "kimi"
model = "kimi-k2"

[model_providers.kimi]
name = "Kimi"
base_url = "https://api.moonshot.cn/v1"
wire_api = "responses"
requires_openai_auth = true
"#
            }),
            None,
        );
        provider.category = Some("custom".to_string());

        let mut takeover_settings = provider.settings_config.clone();
        ProxyService::apply_codex_takeover_fields_for_provider(
            &mut takeover_settings,
            "http://127.0.0.1:15721/v1",
            &provider,
        )
        .expect("apply takeover fields");
        service
            .write_codex_takeover_live_for_provider(&takeover_settings, Some(&provider))
            .expect("write takeover live config");

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains("requires_openai_auth = false"),
            "bedrock outranks the stale key: the stored `true` must be stamped false; got:\n{live_config}"
        );
        let auth_after: Value = serde_json::from_str(
            &std::fs::read_to_string(crate::codex_config::get_codex_auth_path())
                .expect("read auth.json"),
        )
        .expect("parse auth.json");
        assert_eq!(
            auth_after, mixed_auth,
            "takeover must leave auth.json untouched"
        );
    }

    #[test]
    #[serial]
    fn codex_takeover_keeps_stored_flag_under_auto_even_when_file_holds_login() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        // Under `auto` the keyring is consulted first and the file is only a
        // fallback: a keyring entry (say Bedrock) would shadow this login, so
        // the file alone cannot justify forcing the flag to true.
        let file_login = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": "oauth-id",
                "access_token": "oauth-access",
                "refresh_token": "oauth-refresh"
            }
        });
        crate::codex_config::write_codex_live_atomic(
            &file_login,
            Some("model_provider = \"openai\"\nmodel = \"gpt-5-codex\"\n"),
        )
        .expect("seed file login");

        let mut provider = Provider::with_id(
            "kimi".to_string(),
            "Kimi".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "kimi-key" },
                "config": r#"model_provider = "kimi"
model = "kimi-k2"
cli_auth_credentials_store = "auto"

[model_providers.kimi]
name = "Kimi"
base_url = "https://api.moonshot.cn/v1"
wire_api = "responses"
requires_openai_auth = false
"#
            }),
            None,
        );
        provider.category = Some("custom".to_string());

        let mut takeover_settings = provider.settings_config.clone();
        ProxyService::apply_codex_takeover_fields_for_provider(
            &mut takeover_settings,
            "http://127.0.0.1:15721/v1",
            &provider,
        )
        .expect("apply takeover fields");
        service
            .write_codex_takeover_live_for_provider(&takeover_settings, Some(&provider))
            .expect("write takeover live config");

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains("requires_openai_auth = false"),
            "auto store: the stored flag must be left alone, not forced true by the fallback file; got:\n{live_config}"
        );
        assert!(!live_config.contains("requires_openai_auth = true"));
        let auth_after: Value = serde_json::from_str(
            &std::fs::read_to_string(crate::codex_config::get_codex_auth_path())
                .expect("read auth.json"),
        )
        .expect("parse auth.json");
        assert_eq!(
            auth_after, file_login,
            "takeover must leave auth.json untouched"
        );
    }

    #[test]
    #[serial]
    fn codex_takeover_tolerates_corrupt_auth_json_for_every_store_mode() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        let auth_path = crate::codex_config::get_codex_auth_path();
        std::fs::create_dir_all(auth_path.parent().expect("codex dir")).expect("mkdir");
        // A leftover file Codex itself cannot parse. Keyring/ephemeral stores
        // never open it, and the file store treats the failed load as "no
        // stored auth"; in no case may it fail the takeover write.
        std::fs::write(&auth_path, "{").expect("seed corrupt auth.json");

        // (store line, expected flag after projection)
        let cases = [
            (
                "cli_auth_credentials_store = \"keyring\"\n",
                "requires_openai_auth = true",
            ),
            (
                "cli_auth_credentials_store = \"auto\"\n",
                "requires_openai_auth = true",
            ),
            (
                "cli_auth_credentials_store = \"ephemeral\"\n",
                "requires_openai_auth = false",
            ),
            ("", "requires_openai_auth = false"),
        ];
        for (store_line, expected) in cases {
            let mut provider = Provider::with_id(
                "kimi".to_string(),
                "Kimi".to_string(),
                json!({
                    "auth": { "OPENAI_API_KEY": "kimi-key" },
                    "config": format!(
                        r#"model_provider = "kimi"
model = "kimi-k2"
{store_line}
[model_providers.kimi]
name = "Kimi"
base_url = "https://api.moonshot.cn/v1"
wire_api = "responses"
requires_openai_auth = true
"#
                    )
                }),
                None,
            );
            provider.category = Some("custom".to_string());

            let mut takeover_settings = provider.settings_config.clone();
            ProxyService::apply_codex_takeover_fields_for_provider(
                &mut takeover_settings,
                "http://127.0.0.1:15721/v1",
                &provider,
            )
            .expect("apply takeover fields");
            service
                .write_codex_takeover_live_for_provider(&takeover_settings, Some(&provider))
                .unwrap_or_else(|error| {
                    panic!(
                        "store {store_line:?}: corrupt auth.json must not fail the write: {error}"
                    )
                });

            let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
                .expect("read live config");
            assert!(
                live_config.contains(expected),
                "store {store_line:?}: expected `{expected}`; got:\n{live_config}"
            );
            assert_eq!(
                std::fs::read_to_string(&auth_path).expect("read auth.json"),
                "{",
                "takeover must leave the corrupt file untouched"
            );
        }
    }

    #[test]
    #[serial]
    fn codex_takeover_never_raises_flag_on_proxy_injected_oauth_card_with_login_on_disk() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        // A preserved ChatGPT login is on disk (file store), which would
        // stamp an ordinary third-party card to `true`.
        let oauth_auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": "oauth-id",
                "access_token": "oauth-access",
                "refresh_token": "oauth-refresh"
            }
        });
        crate::codex_config::write_codex_live_atomic(
            &oauth_auth,
            Some("model_provider = \"openai\"\nmodel = \"gpt-5-codex\"\n"),
        )
        .expect("seed live OAuth auth");

        // Preset snapshot of a proxy-injected OAuth card still carrying the
        // pre-0.149 `true`; the effective builder neutralizes it to `false`
        // before the takeover writer ever sees it.
        let stored = r#"model_provider = "xai"
model = "grok-4"

[model_providers.xai]
name = "xAI"
base_url = "https://api.x.ai/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let neutralized =
            crate::codex_config::neutralize_codex_official_auth_fallback_for_proxy_oauth(stored)
                .expect("legacy true is neutralized");
        let mut provider = Provider::with_id(
            "xai".to_string(),
            "xAI".to_string(),
            json!({ "auth": {}, "config": neutralized }),
            None,
        );
        provider.category = Some("custom".to_string());
        provider.meta = Some(ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            ..Default::default()
        });
        assert!(provider.uses_proxy_injected_oauth());

        let mut takeover_settings = provider.settings_config.clone();
        ProxyService::apply_codex_takeover_fields_for_provider(
            &mut takeover_settings,
            "http://127.0.0.1:15721/v1",
            &provider,
        )
        .expect("apply takeover fields");
        service
            .write_codex_takeover_live_for_provider(&takeover_settings, Some(&provider))
            .expect("write takeover live config");

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains("requires_openai_auth = false"),
            "proxy-injected OAuth card must keep its neutralized flag; got:\n{live_config}"
        );
        assert!(
            !live_config.contains("requires_openai_auth = true"),
            "a login on disk must not raise the flag on a keyless OAuth card; got:\n{live_config}"
        );
        assert!(
            live_config.contains(&format!(
                "experimental_bearer_token = \"{PROXY_TOKEN_PLACEHOLDER}\""
            )),
            "placeholder must still ride as the provider token; got:\n{live_config}"
        );
        let auth_after: Value = serde_json::from_str(
            &std::fs::read_to_string(crate::codex_config::get_codex_auth_path())
                .expect("read auth.json"),
        )
        .expect("parse auth.json");
        assert_eq!(
            auth_after, oauth_auth,
            "takeover must leave the login untouched"
        );
    }

    #[test]
    #[serial]
    fn codex_custom_provider_live_write_removes_auth_when_preserve_disabled() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: false,
            ..Default::default()
        })
        .expect("disable Codex official auth preservation");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);
        let oauth_auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": "oauth-id",
                "access_token": "oauth-access"
            }
        });
        crate::codex_config::write_codex_live_atomic(
            &oauth_auth,
            Some(
                r#"model_provider = "openai"
model = "gpt-5-codex"
"#,
            ),
        )
        .expect("seed live OAuth auth");

        let mut provider = Provider::with_id(
            "rightcode".to_string(),
            "RightCode".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "rightcode-key"
                },
                "config": r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        provider.category = Some("custom".to_string());
        let takeover_auth = json!({
            "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
        });
        let takeover_settings = json!({
            "auth": takeover_auth,
            "config": r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
name = "RightCode"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
"#
        });

        service
            .write_codex_live_for_provider(&takeover_settings, Some(&provider))
            .expect("write provider-driven Codex live config");

        // Disabled preservation historically overwrote the OAuth login with
        // the placeholder; config-only switching removes auth.json instead —
        // the login is equally gone, and the placeholder now travels as the
        // provider-scoped bearer token that Codex >= 0.149 actually sends.
        assert!(
            !crate::codex_config::get_codex_auth_path().exists(),
            "disabled preservation removes auth.json on a third-party takeover write"
        );

        let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read live config");
        assert!(
            live_config.contains(&format!(
                "experimental_bearer_token = \"{PROXY_TOKEN_PLACEHOLDER}\""
            )),
            "the placeholder must ride in config.toml as the provider token; got:\n{live_config}"
        );

        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset settings");
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
        let output = crate::codex_config::update_codex_toml_field(input, "base_url", new_url)
            .expect("update base_url");

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
    fn codex_takeover_without_provider_selects_a_local_authenticated_route() {
        for input in [
            "",
            "model = \"gpt-5\"\nbase_url = \"https://old.example/v1\"\n",
            "model_providers = { cc-switch = { name = \"Existing\", base_url = \"https://keep.example/v1\" } }\n",
        ] {
            let url = "http://127.0.0.1:15721/v1";
            let projected = ProxyService::apply_codex_proxy_toml_config_for_provider(input, url, None).unwrap();
            let auth = json!({"OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER});
            let live = crate::codex_config::prepare_codex_provider_live_config(&auth, &projected).unwrap();
            println!("takeover_fixture={}", serde_json::to_string(&live).unwrap());
            let doc: toml::Value = toml::from_str(&live).unwrap();
            let id = doc["model_provider"].as_str().expect("explicit provider");
            assert_ne!(id, "openai");
            let table = &doc["model_providers"][id];
            assert_eq!(table["base_url"].as_str(), Some(url));
            assert_eq!(table["wire_api"].as_str(), Some("responses"));
            assert_eq!(table["experimental_bearer_token"].as_str(), Some(PROXY_TOKEN_PLACEHOLDER));
            if input.contains("Existing") {
                assert_eq!(doc["model_providers"]["cc-switch"]["base_url"].as_str(), Some("https://keep.example/v1"));
            }
            let repeated = ProxyService::apply_codex_proxy_toml_config_for_provider(&live, url, None).unwrap();
            let repeated = crate::codex_config::prepare_codex_provider_live_config(&auth, &repeated).unwrap();
            assert_eq!(toml::from_str::<toml::Value>(&repeated).unwrap(), doc);
        }
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
        let output =
            ProxyService::apply_codex_proxy_toml_config_for_provider(input, proxy_url, None)
                .expect("apply proxy config");
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
    fn apply_codex_proxy_toml_config_routes_builtin_official_with_native_auth() {
        let mut provider = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        provider.category = Some("official".to_string());
        let proxy_url = "http://127.0.0.1:5000/v1";

        let output = ProxyService::apply_codex_proxy_toml_config_for_provider(
            "experimental_bearer_token = \"PROXY_MANAGED\"\n",
            proxy_url,
            Some(&provider),
        )
        .expect("apply official proxy config");
        let parsed: toml::Value = toml::from_str(&output).expect("valid official route");
        let route_id = crate::codex_config::CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID;
        let route = &parsed["model_providers"][route_id];

        assert_eq!(parsed["model_provider"].as_str(), Some(route_id));
        assert_eq!(route["base_url"].as_str(), Some(proxy_url));
        assert_eq!(route["requires_openai_auth"].as_bool(), Some(true));
        assert!(parsed.get("experimental_bearer_token").is_none());
    }

    #[test]
    fn apply_codex_proxy_toml_config_fails_closed_for_invalid_official_config() {
        let mut provider = Provider::with_id(
            crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        provider.category = Some("official".to_string());

        let result = ProxyService::apply_codex_proxy_toml_config_for_provider(
            "model_providers = 3\n",
            "http://127.0.0.1:5000/v1",
            Some(&provider),
        );
        assert!(result.is_err());
    }

    #[test]
    fn apply_codex_proxy_toml_config_keeps_upstream_model_for_chat_provider() {
        let input = r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#;
        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({
                "config": input
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });

        let proxy_url = "http://127.0.0.1:5000/v1";
        let output = ProxyService::apply_codex_proxy_toml_config_for_provider(
            input,
            proxy_url,
            Some(&provider),
        )
        .expect("apply chat proxy config");
        let parsed: toml::Value =
            toml::from_str(&output).expect("updated config should be valid TOML");

        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("deepseek"))
                .and_then(|v| v.get("base_url"))
                .and_then(|v| v.as_str()),
            Some(proxy_url)
        );
    }

    #[test]
    fn apply_codex_proxy_toml_config_preserves_model_for_responses_provider() {
        let input = r#"
model_provider = "responses"
model = "upstream-responses-model"

[model_providers.responses]
name = "Responses"
base_url = "https://responses.example/v1"
wire_api = "responses"
"#;
        let mut provider = Provider::with_id(
            "responses".to_string(),
            "Responses".to_string(),
            json!({
                "config": input
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });

        let output = ProxyService::apply_codex_proxy_toml_config_for_provider(
            input,
            "http://127.0.0.1:5000/v1",
            Some(&provider),
        )
        .expect("apply responses proxy config");
        let parsed: toml::Value =
            toml::from_str(&output).expect("updated config should be valid TOML");

        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("upstream-responses-model")
        );
    }

    #[test]
    fn apply_codex_proxy_toml_config_restores_upstream_model_for_responses_provider() {
        let input = r#"
model_provider = "responses"
model = "gpt-5.4"

[model_providers.responses]
name = "Responses"
base_url = "http://127.0.0.1:5000/v1"
wire_api = "responses"
"#;
        let mut provider = Provider::with_id(
            "responses".to_string(),
            "Responses".to_string(),
            json!({
                "config": r#"model_provider = "responses"
model = "upstream-responses-model"

[model_providers.responses]
name = "Responses"
base_url = "https://responses.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });

        let output = ProxyService::apply_codex_proxy_toml_config_for_provider(
            input,
            "http://127.0.0.1:5000/v1",
            Some(&provider),
        )
        .expect("restore responses model");
        let parsed: toml::Value =
            toml::from_str(&output).expect("updated config should be valid TOML");

        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("upstream-responses-model")
        );
    }

    #[test]
    fn update_toml_base_url_uses_implicit_openai_override() {
        let input = r#"
model = "gpt-5.1-codex"
"#;

        let new_url = "http://127.0.0.1:5000/v1";
        let output = crate::codex_config::update_codex_toml_field(input, "base_url", new_url)
            .expect("update implicit openai base_url");

        let parsed: toml::Value =
            toml::from_str(&output).expect("updated config should be valid TOML");

        let base_url = parsed
            .get("openai_base_url")
            .and_then(|v| v.as_str())
            .expect("openai_base_url should exist");

        assert_eq!(base_url, new_url);
    }

    #[tokio::test]
    #[serial]
    async fn codex_takeover_switch_to_managed_official_replaces_native_account() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        db.update_proxy_config(ProxyConfig {
            listen_port: 15_721,
            ..Default::default()
        })
        .await
        .expect("set proxy port");
        let service = ProxyService::new(db);
        service
            .codex_oauth_manager
            .add_test_account_with_user_identity("acct-managed", "managed-access", "managed-user")
            .await
            .expect("seed managed account");

        let native_auth = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": "native-id",
                "access_token": "native-access",
                "refresh_token": "native-refresh",
                "account_id": "acct-native"
            },
            "last_refresh": "2026-01-01T00:00:00Z"
        });
        crate::codex_config::write_codex_live_atomic(&native_auth, Some("model = \"gpt-5.4\"\n"))
            .expect("seed native auth");

        let mut provider = Provider::with_id(
            "managed-official".to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "model = \"gpt-5.4\"\n" }),
            None,
        );
        provider.category = Some("official".to_string());
        provider.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("codex_oauth".to_string()),
                account_id: Some("acct-managed".to_string()),
            }),
            ..Default::default()
        });

        service
            .sync_codex_live_from_provider_while_proxy_active(&provider)
            .await
            .expect("sync managed official takeover");

        let live_auth: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read managed live auth");
        assert_eq!(
            live_auth
                .pointer("/tokens/account_id")
                .and_then(Value::as_str),
            Some("acct-managed")
        );
        assert_eq!(
            live_auth
                .pointer("/tokens/access_token")
                .and_then(Value::as_str),
            Some("managed-access")
        );
        assert!(
            crate::codex_config::codex_auth_matches_recorded_managed_oauth(
                &live_auth,
                "acct-managed"
            )
            .expect("read managed marker"),
            "takeover write must record ownership of the managed auth"
        );
    }

    #[tokio::test]
    #[serial]
    async fn codex_takeover_unbound_official_preserves_native_account() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        db.update_proxy_config(ProxyConfig {
            listen_port: 15_721,
            ..Default::default()
        })
        .await
        .expect("set proxy port");
        let service = ProxyService::new(db);
        let native_auth = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": "native-id",
                "access_token": "native-access",
                "refresh_token": "native-refresh",
                "account_id": "acct-native"
            },
            "last_refresh": "2026-01-01T00:00:00Z"
        });
        crate::codex_config::write_codex_live_atomic(&native_auth, Some("model = \"gpt-5.4\"\n"))
            .expect("seed native auth");

        let mut provider = Provider::with_id(
            crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "model = \"gpt-5.4\"\n" }),
            None,
        );
        provider.category = Some("official".to_string());

        service
            .sync_codex_live_from_provider_while_proxy_active(&provider)
            .await
            .expect("sync unbound official takeover");

        let live_auth: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read native live auth");
        assert_eq!(live_auth, native_auth);
    }

    #[test]
    #[serial]
    fn codex_snapshot_rollback_preserves_newer_native_login_without_marker() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let id_token = crate::codex_config::test_codex_id_token("native-user");

        let auth_r0 = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": id_token,
                "access_token": "access-r0",
                "refresh_token": "refresh-r0",
                "account_id": "acct-a"
            },
            "last_refresh": "2026-01-01T00:00:00Z"
        });
        crate::codex_config::write_codex_live_atomic(&auth_r0, Some("model = \"before\"\n"))
            .expect("seed R0 live");
        let snapshot = crate::codex_config::CodexLiveStateSnapshot::capture()
            .expect("capture transaction snapshot");

        let auth_r1 = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": crate::codex_config::test_codex_id_token("native-user"),
                "access_token": "access-r1",
                "refresh_token": "refresh-r1",
                "account_id": "acct-a"
            },
            "last_refresh": "2026-01-02T00:00:00Z"
        });
        crate::codex_config::write_codex_live_atomic(&auth_r1, Some("model = \"after\"\n"))
            .expect("write concurrent R1 live");

        snapshot
            .restore_preserving_newer_same_account_auth()
            .expect("selective rollback");

        let restored: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read restored auth");
        assert_eq!(restored, auth_r1, "newer same-account auth must survive");
        assert!(!crate::codex_config::codex_managed_oauth_live_auth_marker_exists());
        assert_eq!(
            std::fs::read_to_string(crate::codex_config::get_codex_config_path())
                .expect("read rolled-back config"),
            "model = \"before\"\n"
        );
    }

    #[test]
    #[serial]
    fn codex_snapshot_rollback_restores_previous_local_account_in_same_workspace() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let id_token_a = crate::codex_config::test_codex_id_token("user-a");

        let auth_a = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": id_token_a,
                "access_token": "access-a",
                "refresh_token": "refresh-a",
                "account_id": "workspace-shared"
            },
            "last_refresh": "2026-01-01T00:00:00Z"
        });
        crate::codex_config::write_codex_live_atomic(&auth_a, Some("model = \"before\"\n"))
            .expect("seed account A live");
        crate::codex_config::record_codex_managed_oauth_live_auth(&auth_a, "local-a")
            .expect("record A marker");
        let snapshot = crate::codex_config::CodexLiveStateSnapshot::capture()
            .expect("capture account A snapshot");

        let auth_b = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": crate::codex_config::test_codex_id_token("user-b"),
                "access_token": "access-b",
                "refresh_token": "refresh-b",
                "account_id": "workspace-shared"
            },
            "last_refresh": "2026-01-02T00:00:00Z"
        });
        crate::codex_config::write_codex_live_atomic(&auth_b, Some("model = \"after\"\n"))
            .expect("write account B live");
        crate::codex_config::record_codex_managed_oauth_live_auth(&auth_b, "local-b")
            .expect("record B marker");

        snapshot
            .restore_preserving_newer_same_account_auth()
            .expect("cross-account rollback");

        let restored: Value =
            crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read restored auth");
        assert_eq!(restored, auth_a, "failed A to B change must restore A");
        assert!(
            crate::codex_config::codex_auth_matches_recorded_managed_oauth(&restored, "local-a")
                .expect("check restored A marker")
        );
        assert_eq!(
            std::fs::read_to_string(crate::codex_config::get_codex_config_path())
                .expect("read rolled-back config"),
            "model = \"before\"\n"
        );
    }
}
