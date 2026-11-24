//! 设置页用的查询和保存：概览、保存前检查（校验 + 新目标冲突）、按应用保存、立即同步。
//!
//! 保存目标设置和写入客户端是两个结果：设置成功保存、写入失败时如实分开报告。

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::model::{self, AppTargetsConfig, ConfigSource, FieldError, TargetId, TargetSpec};
use super::status::{self, TargetRecord, TargetStatus};
use super::sync::{self, SyncKind, TargetConflict, TargetReport};
use super::wsl::{self, WslInventory};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTargetsView {
    pub app: String,
    pub source: ConfigSource,
    pub config: AppTargetsConfig,
    /// 每个目标解析出的目录（不为此启动发行版；解析不出的没有这一项）。
    pub resolved_dirs: BTreeMap<String, String>,
    /// 跟随环境的最近结果。
    pub records: BTreeMap<String, TargetRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTargetsOverview {
    pub inventory: WslInventory,
    /// 各发行版里四个客户端的安装情况（只有装了的才给出 WSL 配置入口）。
    pub installs: super::installs::Installs,
    /// 显示多环境界面：宿主是 Windows，且某个 WSL 里装了支持的客户端或已有 WSL 目标。
    pub multi_env: bool,
    pub apps: Vec<AppTargetsView>,
}

fn parse_app(app: &str) -> Result<AppType, AppError> {
    let parsed = <AppType as std::str::FromStr>::from_str(app)?;
    if !model::supports(&parsed) {
        return Err(AppError::localized(
            "app_targets.unsupported_app",
            format!("{app} 暂不支持多环境写入"),
            format!("{app} does not support multiple write targets yet"),
        ));
    }
    Ok(parsed)
}

fn is_running(inventory: &WslInventory, distro: &str) -> bool {
    inventory
        .distros
        .iter()
        .any(|d| d.running && d.distro.eq_ignore_ascii_case(distro))
}

fn view(app: &AppType, inventory: &WslInventory) -> AppTargetsView {
    let (config, source) = sync::app_config(app);
    if source == ConfigSource::Saved {
        sync::refresh_external(app, &config, &|spec: &TargetSpec| match &spec.id {
            TargetId::Windows => true,
            TargetId::Wsl(distro) => is_running(inventory, distro),
        });
    }
    let resolved_dirs = sync::resolved_dirs(app, &config, &|_| false);
    // 检测到、但还不在配置里的发行版也给出自动目录，表单里勾选前就能看到。
    let mut resolved_dirs = resolved_dirs;
    for distro in &inventory.distros {
        let id = TargetId::Wsl(distro.distro.clone());
        if config.target(&id).is_some() {
            continue;
        }
        let spec = TargetSpec {
            id: id.clone(),
            enabled: false,
            route: false,
            dir: None,
        };
        if let Some(dir) = sync::display_dir(app, &spec, false) {
            resolved_dirs.insert(id.key(), dir);
        }
    }
    let mut records = status::records(app.as_str());
    for record in records.values_mut() {
        *record = record.clone().with_sync_warnings(record.warnings.clone());
    }
    for spec in config.followers() {
        let routed = sync::follower_routed(app, &spec.id);
        if let Some(record) = records.get_mut(&spec.id.key()) {
            record.routed = routed;
        }
    }
    AppTargetsView {
        app: app.as_str().to_string(),
        source,
        records,
        config,
        resolved_dirs,
    }
}

/// 设置页概览。重新检测只刷新运行中发行版的文件信息，绝不启动发行版或执行 shell。
pub fn overview(probe_stopped: bool, _refresh_installs: bool) -> AppTargetsOverview {
    let mut inventory = wsl::inventory(probe_stopped);
    // 缓存只供恢复配置使用；概览不能把已经关闭的发行版当成当前环境。
    if inventory.host_windows {
        inventory.distros.retain(|env| env.available && env.running);
    }
    let installs = Default::default();
    let apps: Vec<AppTargetsView> = model::supported_apps()
        .iter()
        .map(|app| view(app, &inventory))
        .collect();
    let has_wsl_targets = apps.iter().any(|v| {
        v.config
            .wsl
            .iter()
            .any(|target| is_running(&inventory, &target.distro))
    });
    AppTargetsOverview {
        multi_env: inventory.host_windows && (has_wsl_targets || !inventory.distros.is_empty()),
        installs,
        inventory,
        apps,
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectResult {
    pub errors: Vec<FieldError>,
    /// 保存后的主环境（自动推导：勾选了 Windows 就是 Windows）。主环境的冲突没有
    /// 「暂不写入以外」的独立供应商，前端据此调整选项。
    pub primary_target: String,
    pub resolved_dirs: BTreeMap<String, String>,
    pub conflicts: Vec<TargetConflict>,
    /// 整个保存被规则挡住的原因（比如托管官方登录要求 Windows 主环境）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
}

/// 新启用、或目录变了的已启用目标。只改了未启用目标的目录不算。
fn changed_targets(previous: &AppTargetsConfig, next: &AppTargetsConfig) -> Vec<TargetSpec> {
    next.enabled_targets()
        .into_iter()
        .filter(|target| match previous.target(&target.id) {
            Some(old) => !old.enabled || old.dir != target.dir,
            None => true,
        })
        .collect()
}

fn primary_changed(previous: &AppTargetsConfig, next: &AppTargetsConfig) -> bool {
    match (previous.primary_id(), next.primary_id()) {
        (Some(a), Some(b)) => !a.same(&b),
        _ => true,
    }
}

fn current_is_managed_login(state: &AppState, app: &AppType) -> Result<bool, AppError> {
    let Some(id) =
        crate::mode::current::provider_for(&state.db, app, crate::mode::current::Purpose::Direct)?
    else {
        return Ok(false);
    };
    Ok(state
        .db
        .get_all_providers(app)?
        .get(&id)
        .is_some_and(|provider| sync::is_managed_login(app, provider)))
}

struct Checked {
    config: AppTargetsConfig,
    previous: AppTargetsConfig,
    result: InspectResult,
    changed: Vec<TargetSpec>,
}

fn check(state: &AppState, app: &AppType, draft: AppTargetsConfig) -> Result<Checked, AppError> {
    let mut config = draft.normalized();
    let (previous, _) = sync::app_config(app);
    // 保留旧版本 route 字段供兼容；本阶段运行时始终跟随 Windows 路由。
    config.windows.route = previous.windows.route;
    for target in &mut config.wsl {
        target.route = previous
            .wsl
            .iter()
            .find(|p| p.distro.eq_ignore_ascii_case(&target.distro))
            .is_some_and(|p| p.route);
    }
    // 已启用的目标允许为解析 HOME 启动发行版（写入时本来就要启动）。
    let resolved_dirs = sync::resolved_dirs(app, &config, &|spec| spec.enabled);
    let errors = model::validate(&config, &resolved_dirs);
    let mut result = InspectResult {
        errors,
        resolved_dirs,
        primary_target: config.primary.clone(),
        ..Default::default()
    };
    if !result.errors.is_empty() {
        return Ok(Checked {
            config,
            previous,
            result,
            changed: Vec::new(),
        });
    }
    if config.primary != model::WINDOWS
        && primary_changed(&previous, &config)
        && current_is_managed_login(state, app)?
    {
        result.blocked = Some("managed_login_requires_windows_primary".to_string());
    }
    let changed = changed_targets(&previous, &config);
    let mut inspected: Vec<TargetSpec> = changed.clone();
    // 主环境换了（比如取消了 Windows）：新主环境里原有的配置也要先问，不能悄悄覆盖。
    if primary_changed(&previous, &config) {
        if let Some(primary) = config
            .primary_id()
            .and_then(|id| config.target(&id))
            .filter(|p| !inspected.iter().any(|t| t.id.same(&p.id)))
        {
            inspected.push(primary);
        }
    }
    let providers = state.db.get_all_providers(app)?;
    let is_primary_target = |id: &TargetId| config.primary_id().is_some_and(|p| p.same(id));
    for spec in &inspected {
        let dir = sync::resolve_dir(app, spec, true)?;
        let incoming = if !is_primary_target(&spec.id) && previous.target(&spec.id).is_some() {
            status::get(app.as_str(), &spec.id.key())
                .and_then(|r| r.provider_id)
                .and_then(|id| providers.get(&id))
        } else {
            None
        };
        if let Some(conflict) = sync::inspect_target_with(state, app, spec, dir, incoming)? {
            result.conflicts.push(conflict);
        }
    }
    Ok(Checked {
        config,
        previous,
        result,
        changed,
    })
}

/// 保存前检查：校验目录，列出新目标里已有、且和当前选择不同的配置。只读。
pub fn inspect(
    state: &AppState,
    app: &str,
    draft: AppTargetsConfig,
) -> Result<InspectResult, AppError> {
    let app = parse_app(app)?;
    Ok(check(state, &app, draft)?.result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Resolution {
    /// 暂不写入（默认）：保存目标，标「待同步」。
    Defer,
    /// 备份后同步当前供应商。
    Overwrite,
    /// 先导入成供应商卡，再同步。
    Import,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    /// 目标设置已保存。
    pub saved: bool,
    #[serde(flatten)]
    pub inspect: InspectResult,
    /// 有冲突还没给出处理方式：什么都没保存、没写。
    pub needs_resolution: bool,
    /// 导入创建（或匹配到）的供应商卡。
    pub imported: Vec<String>,
    /// 主环境这次的写入结果（主环境没变时为空）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<TargetReport>,
    pub targets: Vec<TargetReport>,
}

fn save_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

/// 主环境：沿用现有写入（尊重直连 / 代理模式，重投影 MCP），再补提示词和 Skills。
fn write_primary(state: &AppState, app: &AppType, key: &str) -> TargetReport {
    let proxy = app.spec().route_mode && crate::mode::current::mode_state(app).is_proxy();
    let result = if proxy {
        tauri::async_runtime::block_on(crate::mode::controller::sync_primary_route(state, app))
            .map_err(AppError::Message)
    } else {
        crate::services::ProviderService::sync_current_provider_for_app(state, app.clone())
    };
    match result {
        Ok(()) => {
            let mut warnings = Vec::new();
            if proxy {
                if let Err(err) = crate::services::McpService::sync_enabled_for_app(state, app) {
                    warnings.push(format!("mcp_failed:{err}"));
                }
            }
            if let Err(err) = crate::services::PromptService::sync_to_live(state, app.clone()) {
                warnings.push(format!("prompt_failed:{err}"));
            }
            warnings.extend(sync::sync_skills(state, app));
            TargetReport {
                target: key.to_string(),
                record: TargetRecord::new(TargetStatus::Ok).with_sync_warnings(warnings),
            }
        }
        Err(err) => TargetReport {
            target: key.to_string(),
            record: TargetRecord::new(TargetStatus::Error)
                .reason("write_failed")
                .detail(
                    err.to_string()
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                ),
        },
    }
}

/// 按应用保存目标设置，再按处理方式写入新目标。
pub fn save(
    state: &AppState,
    app: &str,
    draft: AppTargetsConfig,
    resolutions: BTreeMap<String, Resolution>,
) -> Result<SaveResult, AppError> {
    let app = parse_app(app)?;
    if resolutions.iter().any(|(target, resolution)| {
        matches!(TargetId::parse(target), Some(TargetId::Wsl(_)))
            && matches!(resolution, Resolution::Import)
    }) {
        return Err(follows_windows_error());
    }
    let _guard = save_lock().lock().unwrap_or_else(|e| e.into_inner());
    let checked = check(state, &app, draft)?;
    let Checked {
        config,
        previous,
        result: inspect,
        changed,
    } = checked;
    if !inspect.errors.is_empty() || inspect.blocked.is_some() {
        return Ok(SaveResult {
            inspect,
            ..Default::default()
        });
    }
    let resolution_for = |key: &str| {
        resolutions
            .iter()
            .find(|(k, _)| {
                TargetId::parse(k)
                    .zip(TargetId::parse(key))
                    .is_some_and(|(a, b)| a.same(&b))
            })
            .map(|(_, r)| *r)
    };
    if inspect
        .conflicts
        .iter()
        .any(|c| resolution_for(&c.target).is_none())
    {
        return Ok(SaveResult {
            inspect,
            needs_resolution: true,
            ..Default::default()
        });
    }

    // 冲突目录解析和备份放在分离路由之前：只读原目录并写独立备份目录，失败时不会留下半分离状态。
    let mut prepared_conflicts: Vec<(
        TargetId,
        TargetSpec,
        Option<std::path::PathBuf>,
        Resolution,
    )> = Vec::new();
    let mut deferred: Vec<TargetId> = Vec::new();
    for conflict in &inspect.conflicts {
        let Some(id) = TargetId::parse(&conflict.target) else {
            continue;
        };
        let Some(spec) = config.target(&id) else {
            continue;
        };
        let resolution = resolution_for(&conflict.target).unwrap_or(Resolution::Defer);
        let dir = sync::resolve_dir(&app, &spec, true)?;
        match resolution {
            Resolution::Defer => deferred.push(id.clone()),
            Resolution::Overwrite | Resolution::Import => {
                let backup = sync::backup_target(&app, &spec, dir.clone())?;
                log::info!(
                    "[app-targets] {} 的 {} 已备份到 {}",
                    app.as_str(),
                    id.key(),
                    backup.display()
                );
            }
        }
        prepared_conflicts.push((id, spec, dir, resolution));
    }

    // 和后台路由同步共用锁：恢复旧目录后、保存新设置前，不能被旧目标列表重新接上路由。
    let route_lock = sync::route_sync_lock(&app);
    let route_guard = route_lock.lock().unwrap_or_else(|e| e.into_inner());
    let next_followers = config.followers();
    let replaced = previous
        .followers()
        .into_iter()
        .filter(|old| {
            !next_followers
                .iter()
                .any(|next| next.id.same(&old.id) && next.dir == old.dir)
        })
        .collect();
    let mut detached = sync::sync_routes_locked(state, &app, replaced, true);
    for report in &mut detached {
        if report.record.status != TargetStatus::Error {
            continue;
        }
        let Some(TargetId::Wsl(distro)) = TargetId::parse(&report.target) else {
            continue;
        };
        let id = TargetId::Wsl(distro.clone());
        let path_missing = previous.target(&id).is_none_or(|spec| {
            spec.dir
                .as_deref()
                .is_none_or(|d| model::looks_like_wsl_unc(d) || !std::path::Path::new(d).exists())
        });
        if (report.record.reason.as_deref() == Some("home_unresolved") || path_missing)
            && wsl::is_registered(&distro) == Ok(false)
        {
            let store = crate::live::engine::DeviceStore::at(sync::store_root(&app, &id));
            let _ = crate::mode::state::update(&store, |live| {
                if let Some(entry) = live.apps.get_mut(app.as_str()) {
                    entry.attached = false;
                }
            });
            *report = sync::record(
                &app,
                &report.target,
                TargetRecord::new(TargetStatus::Skipped).reason("distro_unregistered"),
            );
        }
    }
    if app.spec().route_mode
        && !detached
            .iter()
            .any(|r| r.record.status == TargetStatus::Error)
        && (primary_changed(&previous, &config)
            || changed
                .iter()
                .any(|t| previous.primary_id().is_some_and(|id| id.same(&t.id))))
    {
        if let Err(error) = tauri::async_runtime::block_on(
            crate::mode::controller::detach_for_target_change(state, &app),
        ) {
            let cleared_unregistered = match previous.primary_id() {
                Some(ref id @ TargetId::Wsl(ref distro))
                    if previous.target(id).is_none_or(|spec| {
                        spec.dir.as_deref().is_none_or(|d| {
                            model::looks_like_wsl_unc(d) || !std::path::Path::new(d).exists()
                        })
                    }) && wsl::is_registered(distro) == Ok(false) =>
                {
                    let store = crate::live::engine::DeviceStore::for_device();
                    let _ = crate::mode::state::update(&store, |live| {
                        if let Some(entry) = live.apps.get_mut(app.as_str()) {
                            entry.attached = false;
                        }
                    });
                    true
                }
                _ => false,
            };
            if !cleared_unregistered {
                detached.push(TargetReport {
                    target: previous.primary.clone(),
                    record: TargetRecord::new(TargetStatus::Error)
                        .reason("route_detach_failed")
                        .detail(error.lines().next().unwrap_or_default().to_string()),
                });
            }
        }
    }
    if detached
        .iter()
        .any(|r| r.record.status == TargetStatus::Error)
    {
        return Ok(SaveResult {
            inspect,
            targets: detached,
            ..Default::default()
        });
    }

    let mut imported = Vec::new();
    let mut imported_for: BTreeMap<String, String> = BTreeMap::new();
    for (id, spec, dir, resolution) in prepared_conflicts {
        if matches!(resolution, Resolution::Import) {
            if let Some(provider_id) = sync::import_target(state, &app, &spec, dir)? {
                imported_for.insert(id.key(), provider_id.clone());
                imported.push(provider_id);
            }
        }
    }

    let legacy = model::legacy_projection(&config, &inspect.resolved_dirs);
    let new_primary = config.primary_id().expect("validated primary");
    let old_primary = previous.primary_id();
    crate::settings::set_app_targets(&app, config.clone(), legacy)?;
    drop(route_guard);
    log::info!(
        "[app-targets] 保存 {} 写入目标：主环境 {}，启用 {}",
        app.as_str(),
        new_primary.key(),
        config
            .enabled_targets()
            .iter()
            .map(|t| t.id.key())
            .collect::<Vec<_>>()
            .join(", ")
    );

    // 原主环境变成跟随环境：它上面就是当前选择，记下来，之后切换能删掉上一家的独有字段。
    if let Some(old) = old_primary.as_ref().filter(|old| !old.same(&new_primary)) {
        if let Some(spec) = config.target(old).filter(|t| t.enabled) {
            let current = crate::mode::current::provider_for(
                &state.db,
                &app,
                crate::mode::current::Purpose::Direct,
            )
            .ok()
            .flatten();
            let dir_result = sync::resolve_dir(&app, &spec, false);
            let fingerprint = match dir_result {
                Ok(dir) => {
                    super::scope::run(sync::scope_for(&app, &spec.id, dir), || {
                        sync::fingerprint_in_scope(&app)
                    })
                    .0
                }
                Err(_) => None,
            };
            status::set(
                app.as_str(),
                &spec.id.key(),
                TargetRecord {
                    provider_id: current,
                    fingerprint,
                    ..TargetRecord::new(TargetStatus::Ok)
                },
            );
        }
    }

    let primary_spec_changed = changed.iter().any(|t| t.id.same(&new_primary));
    let needs_primary = primary_changed(&previous, &config) || primary_spec_changed;
    let primary_deferred = deferred.iter().any(|id| id.same(&new_primary));
    let follower_writes: Vec<TargetId> = changed
        .iter()
        .filter(|t| !t.id.same(&new_primary) && !deferred.iter().any(|d| d.same(&t.id)))
        .map(|t| t.id.clone())
        .collect();

    let mut out = SaveResult {
        saved: true,
        inspect,
        imported,
        ..Default::default()
    };
    for id in deferred.iter().filter(|id| !id.same(&new_primary)) {
        out.targets.push(sync::record(
            &app,
            &id.key(),
            TargetRecord::new(TargetStatus::Deferred).reason("user_deferred"),
        ));
    }

    // 主环境：沿用现有链路。导入的卡直接切过去（它就是那个环境里原有的配置）；暂缓则不写。
    if primary_deferred {
        out.primary = Some(TargetReport {
            target: new_primary.key(),
            record: TargetRecord::new(TargetStatus::Deferred).reason("user_deferred"),
        });
    } else if let Some(imported_id) = imported_for.get(&new_primary.key()) {
        out.primary = Some(
            match crate::services::ProviderService::switch_primary(state, app.clone(), imported_id)
            {
                Ok(_) => {
                    let mut report = if app.spec().route_mode
                        && crate::mode::current::mode_state(&app).is_proxy()
                    {
                        write_primary(state, &app, &new_primary.key())
                    } else {
                        TargetReport {
                            target: new_primary.key(),
                            record: TargetRecord::new(TargetStatus::Ok),
                        }
                    };
                    if report.record.status != TargetStatus::Error {
                        report.record.provider_id = Some(imported_id.clone());
                    }
                    report
                }
                Err(err) => TargetReport {
                    target: new_primary.key(),
                    record: TargetRecord::new(TargetStatus::Error)
                        .reason("write_failed")
                        .detail(
                            err.to_string()
                                .lines()
                                .next()
                                .unwrap_or_default()
                                .to_string(),
                        ),
                },
            },
        );
    } else if needs_primary {
        out.primary = Some(write_primary(state, &app, &new_primary.key()));
    }

    // Windows 是本阶段唯一配置源。先接入共享路由，再同步其他配置。
    if !follower_writes.is_empty() {
        let mut ready_writes = follower_writes.clone();
        if app.spec().route_mode && crate::mode::current::mode_state(&app).is_proxy() {
            let routes = sync::sync_follower_routes_for(state, &app, false, Some(&follower_writes));
            let failed_routes: Vec<TargetId> = routes
                .iter()
                .filter(|r| r.record.status == TargetStatus::Error)
                .filter_map(|r| TargetId::parse(&r.target))
                .collect();
            out.targets.extend(
                routes
                    .into_iter()
                    .filter(|r| r.record.status == TargetStatus::Error),
            );
            ready_writes.retain(|id| !failed_routes.iter().any(|failed| failed.same(id)));
        }
        if !ready_writes.is_empty() {
            out.targets.extend(sync::sync_followers(
                state,
                &app,
                Some(&ready_writes),
                SyncKind::Full,
            ));
        }
    }
    Ok(out)
}

fn follows_windows_error() -> AppError {
    AppError::localized("app_targets.follows_windows", "本阶段 WSL 统一跟随 Windows 的供应商和路由配置；如需停止同步，请取消该写入目标", "WSL currently follows Windows providers and routing; disable the write target to stop syncing")
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<TargetReport>,
    pub targets: Vec<TargetReport>,
}

/// 立即同步 / 重试：用当前最新的选择重新生成补丁。`targets` 为空时同步全部跟随环境；
/// 包含主环境时先写主环境，失败则本次不写其他目标。已取消的目标不会被写。
pub fn sync_now(
    state: &AppState,
    app: &str,
    targets: Option<Vec<String>>,
) -> Result<SyncResult, AppError> {
    let app = parse_app(app)?;
    let (config, _) = sync::app_config(&app);
    let primary = config.primary_id();
    let requested: Option<Vec<TargetId>> =
        targets.map(|list| list.iter().filter_map(|raw| TargetId::parse(raw)).collect());
    let mut out = SyncResult::default();
    if let (Some(primary), Some(requested)) = (primary.as_ref(), requested.as_ref()) {
        if requested.iter().any(|id| id.same(primary)) {
            let report = write_primary(state, &app, &primary.key());
            let failed = report.record.status == TargetStatus::Error;
            out.primary = Some(report);
            if failed {
                return Ok(out);
            }
        }
    }
    let followers: Option<Vec<TargetId>> = requested.map(|ids| {
        ids.into_iter()
            .filter(|id| primary.as_ref().is_none_or(|p| !p.same(id)))
            .collect()
    });
    if followers.as_ref().is_some_and(|ids| ids.is_empty()) {
        return Ok(out);
    }
    let routes = sync::sync_follower_routes_for(state, &app, false, followers.as_deref());
    let failed: Vec<TargetId> = routes
        .iter()
        .filter(|r| r.record.status == TargetStatus::Error)
        .filter_map(|r| TargetId::parse(&r.target))
        .collect();
    let ready: Vec<TargetId> = config
        .followers()
        .into_iter()
        .map(|spec| spec.id)
        .filter(|id| {
            followers
                .as_ref()
                .is_none_or(|ids| ids.iter().any(|t| t.same(id)))
        })
        .filter(|id| !failed.iter().any(|failed| failed.same(id)))
        .collect();
    out.targets = routes
        .into_iter()
        .filter(|r| r.record.status == TargetStatus::Error)
        .collect();
    // 路由失败的目标保留原错误，不能再写直连并把状态覆盖成「已同步」。
    out.targets.extend(sync::sync_followers(
        state,
        &app,
        Some(&ready),
        SyncKind::Full,
    ));
    Ok(out)
}

/// 客户端主机：IP 或主机名，不带协议、端口、路径；不能是 0.0.0.0 / ::。
pub fn validate_client_host(host: &str) -> Result<String, AppError> {
    let host = host.trim();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let invalid = || {
        AppError::localized(
            "app_targets.client_host_invalid",
            "请填写 IP 或主机名，不要带协议、端口或路径，也不能是 0.0.0.0",
            "Enter an IP or host name without scheme, port or path; 0.0.0.0 is not allowed",
        )
    };
    if host.is_empty() {
        return Ok(String::new());
    }
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        if ip.is_unspecified() {
            return Err(invalid());
        }
        return Ok(bare.to_string());
    }
    let ok = bare.len() <= 253
        && bare.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        });
    if ok {
        Ok(bare.to_string())
    } else {
        Err(invalid())
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslRouteInfo {
    pub distro: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub networking_mode: Option<String>,
    pub listen_address: String,
    /// 监听只在回环上，而这个发行版要经 Windows 网关访问：连不上，要改成 0.0.0.0。
    pub listen_unreachable: bool,
}

/// 各 WSL 发行版写给客户端的代理主机（保存的、自动推导的）和监听地址冲突提示。
/// `probe` 为真时进发行版重新检测网络（会启动它）。
pub fn wsl_route_info(state: &AppState, probe: bool) -> Vec<WslRouteInfo> {
    let inventory = wsl::inventory(false);
    let listen = futures::executor::block_on(state.db.get_listen_config())
        .map(|c| c.listen_address)
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let listen_loopback = listen == "127.0.0.1" || listen == "localhost" || listen == "::1";
    inventory
        .distros
        .iter()
        .filter(|env| !inventory.host_windows || (env.available && env.running))
        .map(|env| {
            let linux = if probe {
                wsl::probe(&env.distro).unwrap_or_else(|_| env.env.clone())
            } else {
                env.env.clone()
            };
            let saved_host = crate::settings::get_proxy_wsl_client_host(&env.distro);
            let detected_host = sync::detected_client_host(&linux);
            let effective = saved_host.clone().or(detected_host.clone());
            let listen_unreachable = listen_loopback
                && effective
                    .as_deref()
                    .is_some_and(|h| h != "127.0.0.1" && h != "localhost" && h != "::1");
            WslRouteInfo {
                distro: env.distro.clone(),
                saved_host,
                detected_host,
                networking_mode: linux.networking_mode.clone(),
                listen_address: listen.clone(),
                listen_unreachable,
            }
        })
        .collect()
}

/// 保存某个发行版的客户端主机，并让接入路由的环境用新地址重写。
pub fn set_wsl_client_host(state: &AppState, distro: &str, host: &str) -> Result<(), AppError> {
    if !model::is_valid_distro_name(distro) {
        return Err(AppError::InvalidInput(format!(
            "invalid distro name: {distro}"
        )));
    }
    let host = validate_client_host(host)?;
    crate::settings::set_proxy_wsl_client_host(distro, &host)?;
    let target_id = TargetId::Wsl(distro.to_string());
    for app in model::supported_apps() {
        sync::spawn_route_sync(state, &app);
        if app.spec().route_mode
            && sync::app_config(&app)
                .0
                .primary_id()
                .is_some_and(|primary| primary.same(&target_id))
        {
            let state = state.clone();
            let app_copy = app.clone();
            std::thread::spawn(move || {
                if let Err(error) = tauri::async_runtime::block_on(
                    crate::mode::controller::resync_route(&state, &app_copy),
                ) {
                    log::warn!(
                        "按新的 WSL 主机重写 {} 主环境路由失败: {error}",
                        app_copy.as_str()
                    );
                }
            });
        }
    }
    Ok(())
}
