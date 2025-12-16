//! 代理服务业务逻辑层
//!
//! 提供代理服务器的启动、停止和配置管理

use crate::app_config::AppType;
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::server::ProxyServer;
use crate::proxy::types::*;
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ProxyService {
    db: Arc<Database>,
    server: Arc<RwLock<Option<ProxyServer>>>,
    /// AppHandle，用于传递给 ProxyServer 以支持故障转移时的 UI 更新
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
}

impl ProxyService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            server: Arc::new(RwLock::new(None)),
            app_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// 设置 AppHandle（在应用初始化时调用）
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        futures::executor::block_on(async {
            *self.app_handle.write().await = Some(handle);
        });
    }

    /// 启动代理服务器
    pub async fn start(&self) -> Result<ProxyServerInfo, String> {
        // 1. 获取配置
        let mut config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 2. 确保配置启用（用户通过UI启动即表示希望启用）
        config.enabled = true;

        // 3. 检查是否已在运行
        if self.server.read().await.is_some() {
            return Err("代理服务已在运行中".to_string());
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

        // 6. 持久化 enabled 状态
        self.db
            .update_proxy_config(config)
            .await
            .map_err(|e| format!("保存代理配置失败: {e}"))?;

        log::info!("代理服务器已启动: {}:{}", info.address, info.port);
        Ok(info)
    }

    /// 启动代理服务器（带 Live 配置接管）
    pub async fn start_with_takeover(&self) -> Result<ProxyServerInfo, String> {
        // 1. 备份各应用的 Live 配置
        self.backup_live_configs().await?;

        // 2. 同步 Live 配置中的 Token 到数据库（确保代理能读到最新的 Token）
        self.sync_live_to_providers().await?;

        // 3. 接管各应用的 Live 配置（写入代理地址，清空 Token）
        self.takeover_live_configs().await?;

        // 4. 设置接管状态
        self.db
            .set_live_takeover_active(true)
            .await
            .map_err(|e| format!("设置接管状态失败: {e}"))?;

        // 5. 启动代理服务器
        match self.start().await {
            Ok(info) => Ok(info),
            Err(e) => {
                // 启动失败，恢复原始配置
                log::error!("代理启动失败，尝试恢复原始配置: {e}");
                let _ = self.restore_live_configs().await;
                let _ = self.db.set_live_takeover_active(false).await;
                Err(e)
            }
        }
    }

    /// 同步 Live 配置中的 Token 到数据库
    ///
    /// 在清空 Live Token 之前调用，确保数据库中的 Provider 配置有最新的 Token。
    /// 这样代理才能从数据库读取到正确的认证信息。
    async fn sync_live_to_providers(&self) -> Result<(), String> {
        // Claude: 同步 ANTHROPIC_AUTH_TOKEN
        if let Ok(live_config) = self.read_claude_live() {
            if let Some(provider_id) = self.db.get_current_provider("claude").ok().flatten() {
                if let Ok(Some(mut provider)) = self.db.get_provider_by_id(&provider_id, "claude") {
                    // 从 live 配置提取 token
                    if let Some(env) = live_config.get("env") {
                        if let Some(token) =
                            env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str())
                        {
                            if !token.is_empty() {
                                // 更新 provider 的 settings_config
                                if let Some(env_obj) = provider
                                    .settings_config
                                    .get_mut("env")
                                    .and_then(|v| v.as_object_mut())
                                {
                                    env_obj
                                        .insert("ANTHROPIC_AUTH_TOKEN".to_string(), json!(token));
                                } else {
                                    provider.settings_config["env"] = json!({
                                        "ANTHROPIC_AUTH_TOKEN": token
                                    });
                                }
                                // 保存到数据库
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
        }

        // Codex: 同步 OPENAI_API_KEY
        if let Ok(live_config) = self.read_codex_live() {
            if let Some(provider_id) = self.db.get_current_provider("codex").ok().flatten() {
                if let Ok(Some(mut provider)) = self.db.get_provider_by_id(&provider_id, "codex") {
                    // 从 live 配置提取 token
                    if let Some(auth) = live_config.get("auth") {
                        if let Some(token) = auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) {
                            if !token.is_empty() {
                                // 更新 provider 的 settings_config
                                if let Some(auth_obj) = provider
                                    .settings_config
                                    .get_mut("auth")
                                    .and_then(|v| v.as_object_mut())
                                {
                                    auth_obj.insert("OPENAI_API_KEY".to_string(), json!(token));
                                } else {
                                    provider.settings_config["auth"] = json!({
                                        "OPENAI_API_KEY": token
                                    });
                                }
                                // 保存到数据库
                                if let Err(e) = self.db.update_provider_settings_config(
                                    "codex",
                                    &provider_id,
                                    &provider.settings_config,
                                ) {
                                    log::warn!("同步 Codex Token 到数据库失败: {e}");
                                } else {
                                    log::info!(
                                        "已同步 Codex Token 到数据库 (provider: {provider_id})"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Gemini: 同步 GEMINI_API_KEY
        if let Ok(live_config) = self.read_gemini_live() {
            if let Some(provider_id) = self.db.get_current_provider("gemini").ok().flatten() {
                if let Ok(Some(mut provider)) = self.db.get_provider_by_id(&provider_id, "gemini") {
                    // 从 live 配置提取 token
                    if let Some(env) = live_config.get("env") {
                        if let Some(token) = env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) {
                            if !token.is_empty() {
                                // 更新 provider 的 settings_config
                                if let Some(env_obj) = provider
                                    .settings_config
                                    .get_mut("env")
                                    .and_then(|v| v.as_object_mut())
                                {
                                    env_obj.insert("GEMINI_API_KEY".to_string(), json!(token));
                                } else {
                                    provider.settings_config["env"] = json!({
                                        "GEMINI_API_KEY": token
                                    });
                                }
                                // 保存到数据库
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
            }
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

            // 将 enabled 设为 false，避免下次启动时自动开启
            if let Ok(mut config) = self.db.get_proxy_config().await {
                config.enabled = false;
                let _ = self.db.update_proxy_config(config).await;
            }

            log::info!("代理服务器已停止");
            Ok(())
        } else {
            Err("代理服务器未运行".to_string())
        }
    }

    /// 停止代理服务器（恢复 Live 配置）
    pub async fn stop_with_restore(&self) -> Result<(), String> {
        // 1. 停止代理服务器
        self.stop().await?;

        // 2. 恢复原始 Live 配置
        self.restore_live_configs().await?;

        // 3. 清除接管状态
        self.db
            .set_live_takeover_active(false)
            .await
            .map_err(|e| format!("清除接管状态失败: {e}"))?;

        // 4. 删除备份
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        // 5. 重置健康状态（让健康徽章恢复为正常）
        self.db
            .clear_all_provider_health()
            .await
            .map_err(|e| format!("重置健康状态失败: {e}"))?;

        log::info!("代理已停止，Live 配置已恢复");
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

    /// 接管各应用的 Live 配置（写入代理地址）
    ///
    /// 代理服务器的路由已经根据 API 端点自动区分应用类型：
    /// - `/v1/messages` → Claude
    /// - `/v1/chat/completions`, `/v1/responses` → Codex
    /// - `/v1beta/*` → Gemini
    ///
    /// 因此不需要在 URL 中添加应用前缀。
    async fn takeover_live_configs(&self) -> Result<(), String> {
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        let proxy_url = format!("http://{}:{}", config.listen_address, config.listen_port);

        // Claude: 修改 ANTHROPIC_BASE_URL，使用占位符替代真实 Token（代理会注入真实 Token）
        if let Ok(mut live_config) = self.read_claude_live() {
            if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                env.insert("ANTHROPIC_BASE_URL".to_string(), json!(&proxy_url));
                // 使用占位符，避免 Claude Code 显示缺少 key 的警告
                env.insert("ANTHROPIC_AUTH_TOKEN".to_string(), json!("PROXY_MANAGED"));
            } else {
                live_config["env"] = json!({
                    "ANTHROPIC_BASE_URL": &proxy_url,
                    "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED"
                });
            }
            self.write_claude_live(&live_config)?;
            log::info!("Claude Live 配置已接管，代理地址: {proxy_url}");
        }

        // Codex: 修改 config.toml 的 base_url，auth.json 的 OPENAI_API_KEY（代理会注入真实 Token）
        if let Ok(mut live_config) = self.read_codex_live() {
            // 1. 修改 auth.json 中的 OPENAI_API_KEY（使用占位符）
            if let Some(auth) = live_config.get_mut("auth").and_then(|v| v.as_object_mut()) {
                auth.insert("OPENAI_API_KEY".to_string(), json!("PROXY_MANAGED"));
            }

            // 2. 修改 config.toml 中的 base_url
            let config_str = live_config
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let updated_config = Self::update_toml_base_url(config_str, &proxy_url);
            live_config["config"] = json!(updated_config);

            self.write_codex_live(&live_config)?;
            log::info!("Codex Live 配置已接管，代理地址: {proxy_url}");
        }

        // Gemini: 修改 GOOGLE_GEMINI_BASE_URL，使用占位符替代真实 Token（代理会注入真实 Token）
        if let Ok(mut live_config) = self.read_gemini_live() {
            if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                // 使用占位符，避免显示缺少 key 的警告
                env.insert("GEMINI_API_KEY".to_string(), json!("PROXY_MANAGED"));
            } else {
                live_config["env"] = json!({
                    "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                    "GEMINI_API_KEY": "PROXY_MANAGED"
                });
            }
            self.write_gemini_live(&live_config)?;
            log::info!("Gemini Live 配置已接管，代理地址: {proxy_url}");
        }

        Ok(())
    }

    /// 恢复原始 Live 配置
    async fn restore_live_configs(&self) -> Result<(), String> {
        // Claude
        if let Ok(Some(backup)) = self.db.get_live_backup("claude").await {
            let config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 Claude 备份失败: {e}"))?;
            self.write_claude_live(&config)?;
            log::info!("Claude Live 配置已恢复");
        }

        // Codex
        if let Ok(Some(backup)) = self.db.get_live_backup("codex").await {
            let config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 Codex 备份失败: {e}"))?;
            self.write_codex_live(&config)?;
            log::info!("Codex Live 配置已恢复");
        }

        // Gemini
        if let Ok(Some(backup)) = self.db.get_live_backup("gemini").await {
            let config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 Gemini 备份失败: {e}"))?;
            self.write_gemini_live(&config)?;
            log::info!("Gemini Live 配置已恢复");
        }

        Ok(())
    }

    /// 检查是否处于 Live 接管模式
    pub async fn is_takeover_active(&self) -> Result<bool, String> {
        self.db
            .is_live_takeover_active()
            .await
            .map_err(|e| format!("检查接管状态失败: {e}"))
    }

    /// 从异常退出中恢复（启动时调用）
    ///
    /// 检测到 live_takeover_active=true 但代理未运行时调用此方法。
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
                // Codex: settings_config 包含 {"auth": ..., "config": ...}，直接使用
                serde_json::to_string(&provider.settings_config)
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
        // 更新数据库中的 is_current 标记
        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("无效的应用类型: {app_type}"))?;

        self.db
            .set_current_provider(app_type_enum.as_str(), provider_id)
            .map_err(|e| format!("更新当前供应商失败: {e}"))?;

        log::info!("代理模式：已切换 {app_type} 的目标供应商为 {provider_id}");
        Ok(())
    }

    // ==================== Live 配置读写辅助方法 ====================

    /// 更新 TOML 字符串中的 base_url
    fn update_toml_base_url(toml_str: &str, new_url: &str) -> String {
        use toml_edit::DocumentMut;

        let mut doc = toml_str
            .parse::<DocumentMut>()
            .unwrap_or_else(|_| DocumentMut::new());

        doc["base_url"] = toml_edit::value(new_url);

        doc.to_string()
    }

    fn read_claude_live(&self) -> Result<Value, String> {
        let path = get_claude_settings_path();
        if !path.exists() {
            return Err("Claude 配置文件不存在".to_string());
        }
        read_json_file(&path).map_err(|e| format!("读取 Claude 配置失败: {e}"))
    }

    fn write_claude_live(&self, config: &Value) -> Result<(), String> {
        let path = get_claude_settings_path();
        write_json_file(&path, config).map_err(|e| format!("写入 Claude 配置失败: {e}"))
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
        use crate::codex_config::{get_codex_auth_path, get_codex_config_path};

        if let Some(auth) = config.get("auth") {
            let auth_path = get_codex_auth_path();
            write_json_file(&auth_path, auth).map_err(|e| format!("写入 Codex auth 失败: {e}"))?;
        }

        if let Some(config_str) = config.get("config").and_then(|v| v.as_str()) {
            let config_path = get_codex_config_path();
            std::fs::write(&config_path, config_str)
                .map_err(|e| format!("写入 Codex config 失败: {e}"))?;
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

        // 保存到数据库（保持 enabled 和 live_takeover_active 状态不变）
        let mut new_config = config.clone();
        new_config.enabled = previous.enabled;
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
