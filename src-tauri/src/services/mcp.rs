use indexmap::IndexMap;

use crate::app_config::{AppType, McpServer, MultiAppConfig};
use crate::database::UnreadableEntry;
use crate::error::AppError;
use crate::mcp;
use crate::store::AppState;

/// MCP 相关业务逻辑（v3.7.0 统一结构）
pub struct McpService;

/// 「重新同步到各应用」里单个应用的结果
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppSyncOutcome {
    pub app: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 这些应用的 live 写入和数据库提交在同一步完成（各自的 `sync_and_commit`），
/// 保存/删除之后的逐应用同步和移除要跳过它们。
fn commits_with_db(app: &AppType) -> bool {
    matches!(
        app,
        AppType::Mcode
            | AppType::Kimi
            | AppType::Pi
            | AppType::Agy
            | AppType::Dsh
            | AppType::Omp
            | AppType::Copilot
            | AppType::WorkBuddy
    )
}

impl McpService {
    /// 获取所有 MCP 服务器（统一结构）
    /// 读不出来的行不在这里，见 [`Self::list_unreadable`]。
    pub fn get_all_servers(state: &AppState) -> Result<IndexMap<String, McpServer>, AppError> {
        Ok(state.db.get_mcp_server_rows()?.readable)
    }

    /// 库里读不出来的 MCP 服务器：界面单独列出，只能删除。
    pub fn list_unreadable(state: &AppState) -> Result<Vec<UnreadableEntry>, AppError> {
        Ok(state.db.get_mcp_server_rows()?.unreadable)
    }

    /// 添加或更新 MCP 服务器
    pub fn upsert_server(state: &AppState, server: McpServer) -> Result<(), AppError> {
        // 读取旧状态：用于处理“编辑时取消勾选某个应用”的场景（需要从对应 live 配置中移除）。
        // 别的服务器读不出来不影响；这个 id 本身读不出来就在写任何 live 之前拒绝。
        let previous = state.db.get_mcp_server_rows()?.get(&server.id)?.cloned();
        let prev_apps = previous
            .as_ref()
            .map(|s| s.apps.clone())
            .unwrap_or_default();

        // Keep our disabled OMP tombstone in step with edits while the checkbox is off.
        let managed_omp = server.apps.omp
            || prev_apps.omp
            || previous.as_ref().is_some_and(|saved| {
                mcp::omp::is_disabled_managed(&saved.id, &saved.server).unwrap_or(false)
            });
        // Native MCP writes and managed state commit together.
        let save_with_copilot = || {
            if server.apps.copilot || prev_apps.copilot {
                mcp::copilot::sync_and_commit(
                    &server.id,
                    server.apps.copilot.then_some(&server.server),
                    || state.db.save_mcp_server(&server),
                )
            } else {
                state.db.save_mcp_server(&server)
            }
        };
        let save_with_omp = || {
            if managed_omp {
                mcp::omp::sync_and_commit(
                    &server.id,
                    mcp::omp::OmpChange::from_previous(
                        &server.server,
                        previous
                            .as_ref()
                            .map(|s| &s.server)
                            .unwrap_or(&server.server),
                        server.apps.omp,
                    ),
                    save_with_copilot,
                )
            } else {
                save_with_copilot()
            }
        };
        // MCode / Pi 的文件和数据库一起提交：任一步失败都恢复原样
        let save_with_workbuddy = || {
            if server.apps.workbuddy || prev_apps.workbuddy {
                mcp::workbuddy::sync_and_commit(
                    &server.id,
                    server.apps.workbuddy.then_some(&server.server),
                    save_with_omp,
                )
            } else {
                save_with_omp()
            }
        };
        let save_with_dsh = || {
            if server.apps.dsh || prev_apps.dsh {
                mcp::dsh::sync_and_commit(
                    &server.id,
                    server.apps.dsh.then_some(&server.server),
                    save_with_workbuddy,
                )
            } else {
                save_with_workbuddy()
            }
        };
        let save_with_kimi = || {
            if server.apps.kimi || prev_apps.kimi {
                mcp::kimi::sync_and_commit(
                    &server.id,
                    server.apps.kimi.then_some(&server.server),
                    save_with_dsh,
                )
            } else {
                save_with_dsh()
            }
        };
        let save_with_agy = || {
            if server.apps.agy || prev_apps.agy {
                mcp::agy::sync_and_commit(
                    &server.id,
                    mcp::agy::AgyChange::from_enabled(server.apps.agy.then_some(&server.server)),
                    save_with_kimi,
                )
            } else {
                save_with_kimi()
            }
        };
        let save_with_pi = || {
            if server.apps.pi || prev_apps.pi {
                mcp::pi::sync_and_commit(
                    &state.db,
                    &server.id,
                    mcp::pi::PiChange::from_enabled(server.apps.pi.then_some(&server.server)),
                    save_with_agy,
                )
            } else {
                save_with_agy()
            }
        };
        if server.apps.mcode || prev_apps.mcode {
            mcp::mcode::sync_and_commit(
                &server.id,
                server.apps.mcode.then_some(&server.server),
                save_with_pi,
            )?;
        } else {
            save_with_pi()?;
        }

        // 处理禁用：若旧版本启用但新版本取消，则需要从该应用的 live 配置移除
        for app in AppType::all().filter(|app| !commits_with_db(app)) {
            if prev_apps.is_enabled_for(&app) && !server.apps.is_enabled_for(&app) {
                Self::remove_server_from_app(&server.id, &app)?;
            }
        }

        // 同步到各个启用的应用
        Self::sync_server_to_apps(&server)?;

        Ok(())
    }

    /// 删除 MCP 服务器
    pub fn delete_server(state: &AppState, id: &str) -> Result<bool, AppError> {
        // 读不出来的行只删库里这一行：不知道它往各应用写过什么，不碰 live，也不写回任何东西。
        if state.db.get_unreadable_mcp_server(id)?.is_some() {
            state.db.delete_mcp_server(id)?;
            return Ok(true);
        }
        let server = state.db.get_mcp_server_rows()?.readable.shift_remove(id);

        if let Some(server) = server {
            let delete_with_copilot = || {
                if server.apps.copilot {
                    mcp::copilot::delete_and_commit(id, || state.db.delete_mcp_server(id))
                } else {
                    mcp::copilot::forget_options_and_commit(id, || state.db.delete_mcp_server(id))
                }
            };
            let delete_with_workbuddy = || {
                if server.apps.workbuddy {
                    mcp::workbuddy::sync_and_commit(id, None, delete_with_copilot)
                } else {
                    delete_with_copilot()
                }
            };
            let delete_with_dsh = || {
                if server.apps.dsh {
                    mcp::dsh::sync_and_commit(id, None, delete_with_workbuddy)
                } else {
                    delete_with_workbuddy()
                }
            };
            let delete_with_kimi = || {
                if server.apps.kimi {
                    mcp::kimi::sync_and_commit(id, None, delete_with_dsh)
                } else {
                    delete_with_dsh()
                }
            };
            let delete_with_omp = || {
                if server.apps.omp {
                    mcp::omp::sync_and_commit(
                        id,
                        mcp::omp::OmpChange::Remove(Some(&server.server)),
                        delete_with_kimi,
                    )
                } else {
                    delete_with_kimi()
                }
            };
            let delete_with_agy = || {
                if server.apps.agy {
                    mcp::agy::sync_and_commit(id, mcp::agy::AgyChange::Remove, delete_with_omp)
                } else {
                    delete_with_omp()
                }
            };
            let delete_with_pi = || {
                if server.apps.pi {
                    mcp::pi::sync_and_commit(
                        &state.db,
                        id,
                        mcp::pi::PiChange::Remove,
                        delete_with_agy,
                    )
                } else {
                    delete_with_agy()
                }
            };
            if server.apps.mcode {
                mcp::mcode::sync_and_commit(id, None, delete_with_pi)?;
            } else {
                delete_with_pi()?;
            }
            // A false agy flag carries no ownership history. Even an identical disabled
            // native entry may belong to the user; leave it intact without proof of ownership.
            if !server.apps.pi {
                // 取消勾选只在 Pi 里禁用、条目还留着：服务器删了就一并清掉（尽力而为）
                if let Err(err) = mcp::pi::remove_disabled_if_managed(id, &server.server) {
                    log::warn!("清理 Pi 中已禁用的 MCP 条目 '{id}' 失败: {err}");
                }
            }

            if !server.apps.omp {
                if let Err(err) = mcp::omp::remove_disabled_if_managed(id, &server.server) {
                    log::warn!("Cleaning disabled Oh My Pi MCP '{id}' failed: {err}");
                }
            }
            // 从所有应用的 live 配置中移除
            Self::remove_server_from_all_apps(id, &server)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 命令入口用的 [`Self::toggle_app`]：先拿这个应用的切换锁再读改写 live 配置。
    ///
    /// 逐条写入是「读文件 → 改 → 写回」，并发的两次切换（批量勾选、和切换供应商或
    /// 重新同步撞在一起）会互相覆盖、丢掉对方写的条目。和 `resync_mcp_to_apps` 用同一把
    /// 锁。`toggle_app` 内部不拿这把锁（tokio 的锁不可重入），已经持锁的调用方直接调它。
    pub async fn toggle_app_locked(
        state: &AppState,
        server_id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
        Self::toggle_app(state, server_id, app, enabled)
    }

    /// 切换指定应用的启用状态。调用方负责先拿这个应用的切换锁（见 [`Self::toggle_app_locked`]）。
    pub fn toggle_app(
        state: &AppState,
        server_id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        if commits_with_db(&app) {
            if let Some(server) = state.db.get_mcp_server_rows()?.get(server_id)? {
                let spec = enabled.then_some(&server.server);
                let commit = || {
                    state
                        .db
                        .update_mcp_server_app_enabled(server_id, &app, enabled)
                };
                if app == AppType::Agy {
                    mcp::agy::sync_and_commit(
                        server_id,
                        mcp::agy::AgyChange::from_enabled(spec),
                        commit,
                    )?;
                } else if app == AppType::Dsh {
                    mcp::dsh::sync_and_commit(server_id, spec, commit)?;
                } else if app == AppType::Kimi {
                    mcp::kimi::sync_and_commit(server_id, spec, commit)?;
                } else if app == AppType::Copilot {
                    mcp::copilot::sync_and_commit(server_id, spec, commit)?;
                } else if app == AppType::WorkBuddy {
                    mcp::workbuddy::sync_and_commit(server_id, spec, commit)?;
                } else if app == AppType::Mcode {
                    mcp::mcode::sync_and_commit(server_id, spec, commit)?;
                } else if app == AppType::Omp {
                    mcp::omp::sync_and_commit(
                        server_id,
                        mcp::omp::OmpChange::from_enabled(&server.server, enabled),
                        commit,
                    )?;
                } else {
                    mcp::pi::sync_and_commit(
                        &state.db,
                        server_id,
                        mcp::pi::PiChange::from_enabled(spec),
                        commit,
                    )?;
                }
            }
            return Ok(());
        }
        if let Some(server) = state
            .db
            .update_mcp_server_app_enabled(server_id, &app, enabled)?
        {
            // 同步到对应应用
            if enabled {
                Self::sync_server_to_app(&server, &app)?;
            } else {
                Self::remove_server_from_app(server_id, &app)?;
            }
        }

        Ok(())
    }

    /// 将 MCP 服务器同步到所有启用的应用
    fn sync_server_to_apps(server: &McpServer) -> Result<(), AppError> {
        for app in server.apps.enabled_apps() {
            if commits_with_db(&app) {
                continue; // Already written before saving the managed state.
            }
            Self::sync_server_to_app(server, &app)?;
        }

        Ok(())
    }

    /// 将 MCP 服务器同步到指定应用
    fn sync_server_to_app(server: &McpServer, app: &AppType) -> Result<(), AppError> {
        match app {
            AppType::Claude => {
                mcp::sync_single_server_to_claude(&server.id, &server.server)?;
                // 各官方账号目录镜像默认 .claude.json 的 mcpServers
                crate::services::claude_account::sync_all_shared(
                    &crate::config::get_app_config_dir(),
                );
            }
            AppType::ClaudeDesktop => {
                log::debug!("Claude Desktop 3P profiles do not use CC Switch MCP sync, skipping");
            }
            AppType::Codex => {
                // Codex uses TOML format, must use the correct function
                mcp::sync_single_server_to_codex(&server.id, &server.server)?;
            }
            AppType::Gemini => {
                mcp::sync_single_server_to_gemini(&server.id, &server.server)?;
            }
            AppType::GrokBuild => {
                mcp::sync_single_server_to_grokbuild(&server.id, &server.server)?;
            }
            AppType::OpenCode => {
                mcp::sync_single_server_to_opencode(&server.id, &server.server)?;
            }
            AppType::OpenClaw => {
                // OpenClaw MCP support is still in development (Issue #4834)
                // Skip for now
                log::debug!("OpenClaw MCP support is still in development, skipping sync");
            }
            AppType::Hermes => {
                mcp::sync_single_server_to_hermes(&server.id, &server.server)?;
            }
            AppType::Mcode => mcp::mcode::sync(&server.id, Some(&server.server))?,
            AppType::Kimi => mcp::kimi::sync(&server.id, Some(&server.server))?,
            AppType::Dsh => mcp::dsh::sync(&server.id, Some(&server.server))?,
            AppType::Copilot => mcp::copilot::sync(&server.id, Some(&server.server))?,
            AppType::Pi => mcp::pi::sync(&server.id, Some(&server.server))?,
            AppType::Agy => mcp::agy::sync(&server.id, Some(&server.server))?,
            AppType::Omp => mcp::sync_single_server_to_omp(&server.id, &server.server)?,
            AppType::WorkBuddy => mcp::workbuddy::sync(&server.id, Some(&server.server))?,
        }
        Ok(())
    }

    /// 从所有曾启用过该服务器的应用中移除
    fn remove_server_from_all_apps(id: &str, server: &McpServer) -> Result<(), AppError> {
        // 从所有曾启用的应用中移除
        for app in server.apps.enabled_apps() {
            if commits_with_db(&app) {
                continue; // Already removed before deleting the managed record.
            }
            Self::remove_server_from_app(id, &app)?;
        }
        Ok(())
    }

    fn remove_server_from_app(id: &str, app: &AppType) -> Result<(), AppError> {
        match app {
            AppType::Claude => {
                mcp::remove_server_from_claude(id)?;
                crate::services::claude_account::sync_all_shared(
                    &crate::config::get_app_config_dir(),
                );
            }
            AppType::ClaudeDesktop => {
                log::debug!("Claude Desktop 3P profiles do not use CC Switch MCP sync, skipping");
            }
            AppType::Codex => mcp::remove_server_from_codex(id)?,
            AppType::Gemini => mcp::remove_server_from_gemini(id)?,
            AppType::GrokBuild => mcp::remove_server_from_grokbuild(id)?,
            AppType::OpenCode => {
                mcp::remove_server_from_opencode(id)?;
            }
            AppType::OpenClaw => {
                // OpenClaw MCP support is still in development
                log::debug!("OpenClaw MCP support is still in development, skipping remove");
            }
            AppType::Hermes => {
                mcp::remove_server_from_hermes(id)?;
            }
            AppType::Mcode => mcp::mcode::sync(id, None)?,
            AppType::Kimi => mcp::kimi::sync(id, None)?,
            AppType::Dsh => mcp::dsh::sync(id, None)?,
            AppType::Copilot => mcp::copilot::sync(id, None)?,
            AppType::Pi => mcp::pi::sync(id, None)?,
            AppType::Agy => mcp::agy::sync(id, None)?,
            AppType::Omp => mcp::remove_server_from_omp(id)?,
            AppType::WorkBuddy => mcp::workbuddy::sync(id, None)?,
        }
        Ok(())
    }

    /// 手动同步所有启用的 MCP 服务器到对应的应用。
    ///
    /// Best-effort：单个应用投影失败（如 ~/.claude.json 坏 JSON）不阻断
    /// 其余应用——各应用的 live 文件互相独立，一处损坏没有理由让其他
    /// 应用的 MCP 状态陈旧。全部跑完后若有失败，聚合成一个错误上报，
    /// 保留调用方的可见性。
    pub fn sync_all_enabled(state: &AppState) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;

        let mut failures: Vec<String> = Vec::new();
        for app in AppType::all() {
            if let Err(err) = Self::project_servers_to_app(&servers, &app) {
                log::warn!("同步 MCP 到 {app:?} 失败: {err}");
                failures.push(format!("{}: {err}", app.as_str()));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "部分应用 MCP 同步失败: {}",
                failures.join("; ")
            )))
        }
    }

    /// 只把启用状态投影到单个应用。某个应用的 live 被整体重写后用它做
    /// 定向重投影，避免把无关应用的失败面（如 ~/.claude.json 坏 JSON）
    /// 牵连进目标应用的关键路径。
    pub fn sync_enabled_for_app(state: &AppState, app: &AppType) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;
        Self::project_servers_to_app(&servers, app)
    }

    /// 由 CC Switch 管理 MCP 的应用（Claude Desktop、OpenClaw 不支持）
    pub fn live_sync_apps() -> Vec<AppType> {
        AppType::all()
            .filter(|app| !matches!(app, AppType::OpenClaw | AppType::ClaudeDesktop))
            .collect()
    }

    /// 解析「重新同步」的目标应用：缺省或空列表＝全部受管应用；
    /// 不认识或不支持 MCP 的应用直接报错，不静默跳过。
    pub fn resync_targets(apps: Option<&[String]>) -> Result<Vec<AppType>, AppError> {
        let managed = Self::live_sync_apps();
        let Some(apps) = apps.filter(|apps| !apps.is_empty()) else {
            return Ok(managed);
        };
        let mut targets = Vec::new();
        for raw in apps {
            let app = <AppType as std::str::FromStr>::from_str(raw)?;
            if !managed.contains(&app) {
                return Err(AppError::Message(format!(
                    "{} 不支持由 CC Switch 管理 MCP",
                    app.as_str()
                )));
            }
            if !targets.contains(&app) {
                targets.push(app);
            }
        }
        Ok(targets)
    }

    /// 把数据库里的启用状态重新投影到一个应用的 live 配置，结果按应用报告。
    /// 调用方负责先拿这个应用的切换锁。
    pub fn resync_app(state: &AppState, app: &AppType) -> McpAppSyncOutcome {
        match Self::sync_enabled_for_app(state, app) {
            Ok(()) => McpAppSyncOutcome {
                app: app.as_str().to_string(),
                ok: true,
                error: None,
            },
            Err(err) => {
                log::warn!("重新同步 MCP 到 {app:?} 失败: {err}");
                McpAppSyncOutcome {
                    app: app.as_str().to_string(),
                    ok: false,
                    error: Some(err.to_string()),
                }
            }
        }
    }

    fn project_servers_to_app(
        servers: &IndexMap<String, McpServer>,
        app: &AppType,
    ) -> Result<(), AppError> {
        if matches!(app, AppType::OpenClaw | AppType::ClaudeDesktop) {
            return Ok(());
        }

        // 逐条容错：一条写不进去（规范不是对象、这个应用不认的类型）不该让排在
        // 它后面的服务器全都停在旧状态。全部跑完后聚合上报，调用方照旧能看到
        // 失败。错误相同的条目归并成一项：live 文件本身损坏时每一条都报同一句。
        let mut failures: IndexMap<String, Vec<&str>> = IndexMap::new();
        for server in servers.values() {
            let result = if server.apps.is_enabled_for(app) {
                Self::sync_server_to_app(server, app)
            } else if !commits_with_db(app) {
                Self::remove_server_from_app(&server.id, app)
            } else {
                // MCode / Pi's false flag also covers pre-existing, unmanaged servers.
                // Only explicit disable/delete operations may remove those entries.
                Ok(())
            };
            if let Err(err) = result {
                log::warn!("同步 MCP 服务器 '{}' 到 {app:?} 失败: {err}", server.id);
                failures
                    .entry(err.to_string())
                    .or_default()
                    .push(&server.id);
            }
        }

        if failures.is_empty() {
            return Ok(());
        }
        let detail = failures
            .iter()
            .map(|(error, ids)| format!("{}: {error}", ids.join(", ")))
            .collect::<Vec<_>>()
            .join("; ");
        Err(AppError::Message(format!(
            "部分 MCP 服务器同步到 {} 失败: {detail}",
            app.as_str()
        )))
    }

    /// 从单个应用的 live 配置导入 MCP，返回新导入的数量。
    ///
    /// 已存在的服务器只启用该应用，不覆盖其他字段。导入只读 live 配置，
    /// 不反向写回任何应用；显式编辑、启用/禁用或手动同步时再写回。
    pub fn import_from_app(state: &AppState, app: &AppType) -> Result<usize, AppError> {
        let read_live: fn(&mut MultiAppConfig) -> Result<usize, AppError> = match app {
            AppType::Claude => mcp::import_from_claude,
            AppType::Codex => mcp::import_from_codex,
            AppType::Gemini => mcp::import_from_gemini,
            AppType::GrokBuild => mcp::import_from_grokbuild,
            AppType::OpenCode => mcp::import_from_opencode,
            AppType::Hermes => mcp::import_from_hermes,
            AppType::Mcode => return mcp::mcode::import(state),
            AppType::Kimi => return mcp::kimi::import(state),
            AppType::Dsh => return mcp::dsh::import(state),
            AppType::WorkBuddy => return mcp::workbuddy::import(state),
            AppType::Agy => return mcp::agy::import(state),
            AppType::Pi => return mcp::pi::import(state),
            AppType::Omp => return mcp::omp::import(state),
            AppType::Copilot => return mcp::copilot::import(state),
            AppType::ClaudeDesktop | AppType::OpenClaw => return Ok(0),
        };

        let mut temp_config = MultiAppConfig::default();
        if read_live(&mut temp_config)? == 0 {
            return Ok(0);
        }
        let Some(servers) = &temp_config.mcp.servers else {
            return Ok(0);
        };
        let (mut existing, unreadable) = state
            .db
            .get_mcp_server_rows()?
            .into_readable_and_unreadable_ids();
        let mut new_count = 0;
        for server in servers.values() {
            if unreadable.contains(&server.id) {
                log::warn!(
                    "从 {} 导入 MCP 时跳过 '{}'：库里同名的这一行读不出来",
                    app.as_str(),
                    server.id
                );
                continue;
            }
            let to_save = if let Some(existing_server) = existing.get(&server.id) {
                let mut merged = existing_server.clone();
                merged.apps.set_enabled_for(app, true);
                merged
            } else {
                new_count += 1;
                server.clone()
            };
            state.db.save_mcp_server(&to_save)?;
            existing.insert(to_save.id.clone(), to_save);
        }
        Ok(new_count)
    }

    /// 从所有支持 MCP 的应用导入服务器，返回新导入的数量。
    ///
    /// Best-effort：单个应用导入失败（如坏 config.toml）不阻断其余应用；
    /// 全部跑完后若有失败，聚合成一个错误上报——历史实现逐应用
    /// `unwrap_or(0)` 吞错，坏文件只会表现为"导入成功 0 个"，用户
    /// 无从得知哪个应用出了问题。
    pub fn import_from_all_apps(state: &AppState) -> Result<usize, AppError> {
        let mut total = 0;
        let mut failures: Vec<String> = Vec::new();

        for app in [
            AppType::Claude,
            AppType::Codex,
            AppType::Gemini,
            AppType::GrokBuild,
            AppType::OpenCode,
            AppType::Hermes,
            AppType::Mcode,
            AppType::Kimi,
            AppType::Dsh,
            AppType::WorkBuddy,
            AppType::Agy,
            AppType::Pi,
            AppType::Omp,
            AppType::Copilot,
        ] {
            match Self::import_from_app(state, &app) {
                Ok(count) => total += count,
                Err(err) => {
                    log::warn!("从 {} 导入 MCP 失败: {err}", app.as_str());
                    failures.push(format!("{}: {err}", app.as_str()));
                }
            }
        }

        if failures.is_empty() {
            Ok(total)
        } else {
            Err(AppError::Message(format!(
                "已导入 {total} 个，部分应用导入失败: {}",
                failures.join("; ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::McpApps;
    use crate::database::Database;
    use std::sync::Arc;

    fn server(id: &str) -> McpServer {
        McpServer {
            id: id.into(),
            name: format!("{id} name"),
            server: serde_json::json!({"command": "npx"}),
            apps: McpApps::default(),
            description: None,
            homepage: None,
            docs: None,
            tags: vec![],
        }
    }

    fn stored(state: &AppState, id: &str) -> Option<String> {
        use rusqlite::OptionalExtension;
        state
            .db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT server_config || '|' || tags || '|' || enabled_claude FROM mcp_servers WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .unwrap()
    }

    #[test]
    fn an_unreadable_server_is_listed_apart_left_untouched_and_can_only_be_deleted() {
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        for id in ["good", "broken"] {
            state.db.save_mcp_server(&server(id)).unwrap();
        }
        state
            .db
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE mcp_servers SET server_config = '{\"command\":' WHERE id = 'broken'",
                [],
            )
            .unwrap();
        let bytes = stored(&state, "broken");

        let listed = McpService::get_all_servers(&state).expect("list still loads");
        assert_eq!(listed.keys().collect::<Vec<_>>(), ["good"]);
        let unreadable = McpService::list_unreadable(&state).unwrap();
        assert_eq!(unreadable.len(), 1);
        assert_eq!(unreadable[0].id, "broken");
        assert!(
            unreadable[0].error.contains("server_config"),
            "{}",
            unreadable[0].error
        );
        // 切换启用会读整行，读不出来就报错，库里的字节不变。
        assert!(McpService::toggle_app(&state, "broken", AppType::Claude, true).is_err());
        assert_eq!(stored(&state, "broken"), bytes);

        assert!(McpService::delete_server(&state, "broken").expect("delete is allowed"));
        assert_eq!(stored(&state, "broken"), None);
        assert!(McpService::list_unreadable(&state).unwrap().is_empty());
        assert!(stored(&state, "good").is_some());
    }

    #[test]
    #[serial_test::serial]
    fn an_unreadable_server_only_blocks_writes_to_its_own_id() {
        let home = tempfile::tempdir().unwrap();
        let _env = crate::test_support::EnvVars::new()
            .set("CC_SWITCH_TEST_HOME", home.path())
            .set("HOME", home.path());
        crate::settings::reload_settings().unwrap();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        for id in ["good", "broken"] {
            state.db.save_mcp_server(&server(id)).unwrap();
        }
        state
            .db
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE mcp_servers SET server_config = '{\"command\":' WHERE id = 'broken'",
                [],
            )
            .unwrap();
        let bytes = stored(&state, "broken");
        let agy_native = || {
            std::fs::read_to_string(
                crate::config::get_home_dir().join(".gemini/config/mcp_config.json"),
            )
            .unwrap_or_default()
        };

        // Other ids: add, edit, toggle and delete all work.
        McpService::upsert_server(&state, server("new")).expect("add another server");
        let mut edited = server("good");
        edited.name = "renamed".into();
        McpService::upsert_server(&state, edited).expect("edit another server");
        McpService::toggle_app(&state, "good", AppType::Agy, true).expect("toggle another server");
        assert!(agy_native().contains("\"good\""), "{}", agy_native());
        let listed = McpService::get_all_servers(&state).unwrap();
        assert_eq!(listed["good"].name, "renamed");
        assert!(listed["good"].apps.agy);
        assert!(McpService::delete_server(&state, "new").expect("delete another server"));

        // The unreadable id itself: every write is refused and names it.
        let error = McpService::upsert_server(&state, server("broken"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("broken"), "{error}");
        let error = McpService::toggle_app(&state, "broken", AppType::Agy, true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("broken"), "{error}");
        assert!(!agy_native().contains("\"broken\""), "{}", agy_native());
        assert_eq!(stored(&state, "broken"), bytes);

        // Imports bring in the other ids and skip the unreadable one by name.
        std::fs::write(
            crate::config::get_home_dir().join(".gemini/config/mcp_config.json"),
            r#"{"mcpServers":{"broken":{"command":"evil"},"fresh":{"command":"npx"}}}"#,
        )
        .unwrap();
        let error = McpService::import_from_app(&state, &AppType::Agy)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("'broken'") && !error.contains("'fresh'"),
            "{error}"
        );
        assert!(
            McpService::get_all_servers(&state).unwrap()["fresh"]
                .apps
                .agy
        );
        std::fs::write(
            crate::config::get_home_dir().join(".claude.json"),
            r#"{"mcpServers":{"broken":{"command":"evil"},"fresh-claude":{"command":"npx"}}}"#,
        )
        .unwrap();
        assert_eq!(
            McpService::import_from_app(&state, &AppType::Claude).unwrap(),
            1
        );
        assert!(
            McpService::get_all_servers(&state).unwrap()["fresh-claude"]
                .apps
                .claude
        );
        assert_eq!(stored(&state, "broken"), bytes);

        let config = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            r#"{"mcpServers":{"broken":{"command":"evil"},"linked":{"command":"npx"}}}"#,
        );
        let request = crate::deeplink::parse_deeplink_url(&format!(
            "ccswitch://v1/import?resource=mcp&apps=claude&config={config}"
        ))
        .unwrap();
        let result = crate::deeplink::import_mcp_from_deeplink(&state, request).unwrap();
        assert_eq!(result.imported_ids, ["linked"]);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].id, "broken");
        assert_eq!(stored(&state, "broken"), bytes);

        // A project snapshot and switch still go through, toggling only readable servers.
        use crate::services::profile::{ProfileScope, ProfileService};
        let profile = ProfileService::create(&state, "p", ProfileScope::Claude)
            .expect("snapshot with an unreadable server");
        McpService::toggle_app(&state, "good", AppType::Claude, true).unwrap();
        ProfileService::apply(&state, &profile.id, ProfileScope::Claude)
            .expect("switch with an unreadable server");
        let listed = McpService::get_all_servers(&state).unwrap();
        assert!(!listed["good"].apps.claude && listed["linked"].apps.claude);
        assert_eq!(stored(&state, "broken"), bytes);
    }
}
