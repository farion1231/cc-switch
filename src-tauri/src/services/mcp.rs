use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

use crate::app_config::{AppType, McpApps, McpServer};
use crate::error::AppError;
use crate::mcp;
use crate::settings;
use crate::store::AppState;

/// MCP 相关业务逻辑（v3.7.0 统一结构）
pub struct McpService;

impl McpService {
    const DEFAULT_ENV_ID: &'static str = "default";

    /// 获取当前激活的配置环境 ID（空值时回落为 "default"）
    fn active_env_id() -> String {
        settings::get_settings()
            .active_config_directory_set_id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| Self::DEFAULT_ENV_ID.to_string())
    }

    /// 读取指定环境的 MCP 启用状态映射
    fn load_env_apps(env_id: &str) -> HashMap<String, crate::app_config::McpApps> {
        settings::get_mcp_apps_for_env(env_id).unwrap_or_default()
    }

    /// 持久化指定环境的 MCP 启用状态映射
    fn persist_env_apps(
        env_id: &str,
        apps: HashMap<String, crate::app_config::McpApps>,
    ) -> Result<(), AppError> {
        settings::set_mcp_apps_for_env(env_id, apps)
    }

    /// 从当前环境的 live 配置推断各服务器的启用状态（仅针对已知服务器）
    fn detect_env_apps_from_live(
        known_servers: &IndexMap<String, McpServer>,
    ) -> HashMap<String, crate::app_config::McpApps> {
        let mut env_apps: HashMap<String, crate::app_config::McpApps> = HashMap::new();

        if let Ok(map) = crate::claude_mcp::read_mcp_servers_map() {
            for id in map.keys() {
                if known_servers.contains_key(id) {
                    env_apps.entry(id.clone()).or_default().claude = true;
                }
            }
        }

        if let Ok(text) = crate::codex_config::read_and_validate_codex_config_text() {
            for id in Self::extract_codex_mcp_ids(&text) {
                if known_servers.contains_key(&id) {
                    env_apps.entry(id).or_default().codex = true;
                }
            }
        }

        if let Ok(map) = crate::gemini_mcp::read_mcp_servers_map() {
            for id in map.keys() {
                if known_servers.contains_key(id) {
                    env_apps.entry(id.clone()).or_default().gemini = true;
                }
            }
        }

        env_apps
    }

    /// 从 Codex 配置文本提取 MCP server ID（支持 mcp.servers 与 mcp_servers）
    fn extract_codex_mcp_ids(text: &str) -> Vec<String> {
        let mut ids: HashSet<String> = HashSet::new();

        if let Ok(root) = toml::from_str::<toml::Table>(text) {
            if let Some(mcp_tbl) = root.get("mcp").and_then(|v| v.as_table()) {
                if let Some(servers) = mcp_tbl.get("servers").and_then(|v| v.as_table()) {
                    ids.extend(servers.keys().cloned());
                }
            }

            if let Some(servers) = root.get("mcp_servers").and_then(|v| v.as_table()) {
                ids.extend(servers.keys().cloned());
            }
        }

        ids.into_iter().collect()
    }

    /// 获取所有 MCP 服务器（统一结构）
    pub fn get_all_servers(state: &AppState) -> Result<IndexMap<String, McpServer>, AppError> {
        let settings_snapshot = settings::get_settings();
        let env_id = settings_snapshot
            .active_config_directory_set_id
            .as_ref()
            .and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .unwrap_or_else(|| Self::DEFAULT_ENV_ID.to_string());
        let has_multiple_envs = settings_snapshot.config_directory_sets.len() > 1;

        let mut servers = state.db.get_all_mcp_servers()?;

        // 优先使用已存储的环境启用状态，否则尝试从当前环境的 live 配置推断
        let mut env_apps = settings_snapshot
            .mcp_env_apps
            .get(&env_id)
            .cloned()
            .unwrap_or_default();
        let mut should_persist = false;

        if env_apps.is_empty() {
            let detected = Self::detect_env_apps_from_live(&servers);
            if !detected.is_empty() {
                env_apps = detected;
                should_persist = true;
            }
        }

        for (id, server) in servers.iter_mut() {
            if let Some(apps) = env_apps.get(id) {
                server.apps = apps.clone();
            } else if has_multiple_envs {
                // 多环境下未记录的服务器默认禁用，避免继承其它环境的状态
                server.apps = McpApps::default();
                env_apps.insert(id.clone(), server.apps.clone());
                should_persist = true;
            } else {
                // 单环境场景保持原有行为：沿用数据库中的启用状态
                env_apps.insert(id.clone(), server.apps.clone());
                should_persist = true;
            }
        }

        if should_persist {
            Self::persist_env_apps(&env_id, env_apps)?;
        }

        Ok(servers)
    }

    /// 添加或更新 MCP 服务器
    pub fn upsert_server(state: &AppState, server: McpServer) -> Result<(), AppError> {
        let env_id = Self::active_env_id();
        let mut env_apps = Self::load_env_apps(&env_id);

        // 读取旧状态：用于处理“编辑时取消勾选某个应用”的场景（需要从对应 live 配置中移除）
        let prev_apps = env_apps
            .get(&server.id)
            .cloned()
            .or_else(|| {
                state
                    .db
                    .get_all_mcp_servers()
                    .ok()
                    .and_then(|map| map.get(&server.id).cloned().map(|s| s.apps))
            })
            .unwrap_or_default();

        env_apps.insert(server.id.clone(), server.apps.clone());
        state.db.save_mcp_server(&server)?;
        Self::persist_env_apps(&env_id, env_apps)?;

        // 处理禁用：若旧版本启用但新版本取消，则需要从该应用的 live 配置移除
        if prev_apps.claude && !server.apps.claude {
            Self::remove_server_from_app(state, &server.id, &AppType::Claude)?;
        }
        if prev_apps.codex && !server.apps.codex {
            Self::remove_server_from_app(state, &server.id, &AppType::Codex)?;
        }
        if prev_apps.gemini && !server.apps.gemini {
            Self::remove_server_from_app(state, &server.id, &AppType::Gemini)?;
        }

        // 同步到各个启用的应用
        Self::sync_server_to_apps(state, &server)?;

        Ok(())
    }

    /// 删除 MCP 服务器
    pub fn delete_server(state: &AppState, id: &str) -> Result<bool, AppError> {
        let server = state.db.get_all_mcp_servers()?.shift_remove(id);

        if let Some(server) = server {
            state.db.delete_mcp_server(id)?;
            settings::remove_mcp_server_from_envs(id)?;

            // 从所有应用的 live 配置中移除
            Self::remove_server_from_all_apps(state, id, &server)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 切换指定应用的启用状态
    pub fn toggle_app(
        state: &AppState,
        server_id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        let env_id = Self::active_env_id();
        let mut env_apps = Self::load_env_apps(&env_id);
        let mut servers = state.db.get_all_mcp_servers()?;

        if let Some(server) = servers.get_mut(server_id) {
            let mut apps = env_apps
                .get(server_id)
                .cloned()
                .unwrap_or(server.apps.clone());

            apps.set_enabled_for(&app, enabled);
            env_apps.insert(server_id.to_string(), apps.clone());
            Self::persist_env_apps(&env_id, env_apps)?;

            server.apps = apps;
            state.db.save_mcp_server(server)?;

            // 同步到对应应用
            if enabled {
                Self::sync_server_to_app(state, server, &app)?;
            } else {
                Self::remove_server_from_app(state, server_id, &app)?;
            }
        }

        Ok(())
    }

    /// 将 MCP 服务器同步到所有启用的应用
    fn sync_server_to_apps(_state: &AppState, server: &McpServer) -> Result<(), AppError> {
        for app in server.apps.enabled_apps() {
            Self::sync_server_to_app_no_config(server, &app)?;
        }

        Ok(())
    }

    /// 将 MCP 服务器同步到指定应用
    fn sync_server_to_app(
        _state: &AppState,
        server: &McpServer,
        app: &AppType,
    ) -> Result<(), AppError> {
        Self::sync_server_to_app_no_config(server, app)
    }

    fn sync_server_to_app_no_config(server: &McpServer, app: &AppType) -> Result<(), AppError> {
        match app {
            AppType::Claude => {
                mcp::sync_single_server_to_claude(&Default::default(), &server.id, &server.server)?;
            }
            AppType::Codex => {
                // Codex uses TOML format, must use the correct function
                mcp::sync_single_server_to_codex(&Default::default(), &server.id, &server.server)?;
            }
            AppType::Gemini => {
                mcp::sync_single_server_to_gemini(&Default::default(), &server.id, &server.server)?;
            }
        }
        Ok(())
    }

    /// 从所有曾启用过该服务器的应用中移除
    fn remove_server_from_all_apps(
        state: &AppState,
        id: &str,
        server: &McpServer,
    ) -> Result<(), AppError> {
        // 从所有曾启用的应用中移除
        for app in server.apps.enabled_apps() {
            Self::remove_server_from_app(state, id, &app)?;
        }
        Ok(())
    }

    fn remove_server_from_app(_state: &AppState, id: &str, app: &AppType) -> Result<(), AppError> {
        match app {
            AppType::Claude => mcp::remove_server_from_claude(id)?,
            AppType::Codex => mcp::remove_server_from_codex(id)?,
            AppType::Gemini => mcp::remove_server_from_gemini(id)?,
        }
        Ok(())
    }

    /// 手动同步所有启用的 MCP 服务器到对应的应用
    pub fn sync_all_enabled(state: &AppState) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;

        let mut claude_map = HashMap::new();
        let mut codex_map = HashMap::new();
        let mut gemini_map = HashMap::new();

        for (id, server) in servers.iter() {
            if server.apps.claude {
                claude_map.insert(id.clone(), server.server.clone());
            }
            if server.apps.codex {
                codex_map.insert(id.clone(), server.server.clone());
            }
            if server.apps.gemini {
                gemini_map.insert(id.clone(), server.server.clone());
            }
        }

        // 全量覆盖各应用的 mcpServers，避免切换多环境时遗留旧配置
        mcp::sync_servers_map_to_claude(&claude_map)?;
        mcp::sync_servers_map_to_codex(&codex_map)?;
        mcp::sync_servers_map_to_gemini(&gemini_map)?;

        Ok(())
    }

    // ========================================================================
    // 兼容层：支持旧的 v3.6.x 命令（已废弃，将在 v4.0 移除）
    // ========================================================================

    /// [已废弃] 获取指定应用的 MCP 服务器（兼容旧 API）
    #[deprecated(since = "3.7.0", note = "Use get_all_servers instead")]
    pub fn get_servers(
        state: &AppState,
        app: AppType,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        let all_servers = Self::get_all_servers(state)?;
        let mut result = HashMap::new();

        for (id, server) in all_servers {
            if server.apps.is_enabled_for(&app) {
                result.insert(id, server.server);
            }
        }

        Ok(result)
    }

    /// [已废弃] 设置 MCP 服务器在指定应用的启用状态（兼容旧 API）
    #[deprecated(since = "3.7.0", note = "Use toggle_app instead")]
    pub fn set_enabled(
        state: &AppState,
        app: AppType,
        id: &str,
        enabled: bool,
    ) -> Result<bool, AppError> {
        Self::toggle_app(state, id, app, enabled)?;
        Ok(true)
    }

    /// [已废弃] 同步启用的 MCP 到指定应用（兼容旧 API）
    #[deprecated(since = "3.7.0", note = "Use sync_all_enabled instead")]
    pub fn sync_enabled(state: &AppState, app: AppType) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;

        for server in servers.values() {
            if server.apps.is_enabled_for(&app) {
                Self::sync_server_to_app(state, server, &app)?;
            }
        }

        Ok(())
    }

    /// 从 Claude 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_claude(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp.rs）
        let count = crate::mcp::import_from_claude(&mut temp_config)?;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Claude，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.claude = true;
                        merged
                    } else {
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 同步到对应应用 live 配置
                    Self::sync_server_to_apps(state, &to_save)?;
                }
            }
        }

        Ok(count)
    }

    /// 从 Codex 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_codex(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp.rs）
        let count = crate::mcp::import_from_codex(&mut temp_config)?;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Codex，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.codex = true;
                        merged
                    } else {
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 同步到对应应用 live 配置
                    Self::sync_server_to_apps(state, &to_save)?;
                }
            }
        }

        Ok(count)
    }

    /// 从 Gemini 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_gemini(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp.rs）
        let count = crate::mcp::import_from_gemini(&mut temp_config)?;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Gemini，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.gemini = true;
                        merged
                    } else {
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 同步到对应应用 live 配置
                    Self::sync_server_to_apps(state, &to_save)?;
                }
            }
        }

        Ok(count)
    }
}
