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

/// 设置页概览。`probe_stopped` 为真时（用户点「重新检测」）连停止的发行版也启动探测。
pub fn overview(probe_stopped: bool, refresh_installs: bool) -> AppTargetsOverview {
    let inventory = wsl::inventory(probe_stopped);
    let installs =
        super::installs::detect(&inventory, refresh_installs || probe_stopped, probe_stopped);
    let apps: Vec<AppTargetsView> = model::supported_apps()
        .iter()
        .map(|app| view(app, &inventory))
        .collect();
    let has_wsl_targets = apps.iter().any(|v| !v.config.wsl.is_empty());
    let any_installed = installs
        .values()
        .any(|tools| tools.values().any(|t| t.present()));
    AppTargetsOverview {
        multi_env: inventory.host_windows && (any_installed || has_wsl_targets),
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
        .get_all_providers(app.as_str())?
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
    // 「接入路由」只经 set_route 改，表单带来的值可能是旧的：沿用已保存的。
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
    for spec in &inspected {
        let dir = sync::resolve_dir(app, spec, true)?;
        if let Some(conflict) = sync::inspect_target(state, app, spec, dir)? {
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
    let result =
        crate::services::ProviderService::sync_current_provider_for_app(state, app.clone());
    match result {
        Ok(()) => {
            let mut warnings = Vec::new();
            if let Err(err) = crate::services::PromptService::sync_to_live(state, app.clone()) {
                warnings.push(format!("prompt_failed:{err}"));
            }
            if let Err(err) = crate::services::SkillService::sync_to_app(&state.db, app) {
                warnings.push(format!("skills_failed:{err}"));
            }
            TargetReport {
                target: key.to_string(),
                record: TargetRecord {
                    warnings,
                    ..TargetRecord::new(TargetStatus::Ok)
                },
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

    // 导入、备份都在保存之前：失败时什么都不保存，目标原配置不动。
    let mut imported = Vec::new();
    let mut imported_for: BTreeMap<String, String> = BTreeMap::new();
    let mut deferred: Vec<TargetId> = Vec::new();
    for conflict in &inspect.conflicts {
        let Some(id) = TargetId::parse(&conflict.target) else {
            continue;
        };
        let Some(spec) = config.target(&id) else {
            continue;
        };
        let dir = sync::resolve_dir(&app, &spec, true)?;
        match resolution_for(&conflict.target).unwrap_or(Resolution::Defer) {
            Resolution::Defer => deferred.push(id),
            Resolution::Overwrite => {
                let backup = sync::backup_target(&app, &spec, dir)?;
                log::info!(
                    "[app-targets] {} 的 {} 已备份到 {}",
                    app.as_str(),
                    id.key(),
                    backup.display()
                );
            }
            Resolution::Import => {
                sync::backup_target(&app, &spec, dir.clone())?;
                if let Some(provider_id) = sync::import_target(state, &app, &spec, dir)? {
                    imported_for.insert(id.key(), provider_id.clone());
                    imported.push(provider_id);
                }
            }
        }
    }

    let legacy = model::legacy_projection(&config, &inspect.resolved_dirs);
    let new_primary = config.primary_id().expect("validated primary");
    let old_primary = previous.primary_id();
    crate::settings::set_app_targets(&app, config.clone(), legacy)?;
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
            let dir = sync::resolve_dir(&app, &spec, false).ok().flatten();
            let (fingerprint, _) = super::scope::run(sync::scope_for(&app, old, dir), || {
                sync::fingerprint_in_scope(&app)
            });
            status::set(
                app.as_str(),
                &old.key(),
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
            match crate::services::ProviderService::switch(state, app.clone(), imported_id) {
                Ok(_) => TargetReport {
                    target: new_primary.key(),
                    record: TargetRecord {
                        provider_id: Some(imported_id.clone()),
                        ..TargetRecord::new(TargetStatus::Ok)
                    },
                },
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

    // 跟随环境各用各的：导入的卡用导入的那张；已经在用某家的继续用它；新环境默认用主环境
    // 当前这家（托管登录除外，那样就先不选，只同步 MCP 等共享内容）。
    if !follower_writes.is_empty() {
        let main_current = crate::mode::current::provider_for(
            &state.db,
            &app,
            crate::mode::current::Purpose::Direct,
        )
        .ok()
        .flatten()
        .filter(|id| {
            state
                .db
                .get_all_providers(app.as_str())
                .ok()
                .and_then(|all| all.get(id).cloned())
                .is_some_and(|p| !sync::is_managed_login(&app, &p))
        });
        let mut activate = BTreeMap::new();
        for id in &follower_writes {
            let key = id.key();
            if let Some(imported_id) = imported_for.get(&key) {
                activate.insert(key, imported_id.clone());
                continue;
            }
            let active = status::get(app.as_str(), &key).and_then(|r| r.provider_id);
            if active.is_none() {
                if let Some(main) = &main_current {
                    activate.insert(key, main.clone());
                }
            }
        }
        out.targets.extend(sync::sync_followers_with(
            state,
            &app,
            Some(&follower_writes),
            SyncKind::Full,
            &activate,
        ));
    }
    Ok(out)
}

/// 在一个环境里改用 `provider_id`（供应商页的环境开关）。主环境走现有切换；跟随环境
/// 只写那个环境，成功后才算换过去，失败时保持原来那家。装了客户端、还没加入写入环境的
/// WSL 发行版会先加入（写入前备份它原有的配置）。
pub fn activate(
    state: &AppState,
    app: &str,
    target: &str,
    provider_id: &str,
) -> Result<TargetReport, AppError> {
    let app = parse_app(app)?;
    let id = TargetId::parse(target)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown target {target}")))?;
    let (config, _) = sync::app_config(&app);
    if config.primary_id().is_some_and(|p| p.same(&id)) {
        crate::services::ProviderService::switch(state, app.clone(), provider_id)?;
        return Ok(TargetReport {
            target: id.key(),
            record: TargetRecord {
                provider_id: Some(provider_id.to_string()),
                ..TargetRecord::new(TargetStatus::Ok)
            },
        });
    }
    let provider = state
        .db
        .get_all_providers(app.as_str())?
        .get(provider_id)
        .cloned()
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;
    if sync::is_managed_login(&app, &provider) {
        return Err(AppError::localized(
            "app_targets.managed_login_windows_only",
            "绑定了托管账号的官方卡目前只能在 Windows 使用；WSL 可以用没绑账号的官方卡（用它自己的登录）或 API Key 供应商",
            "Official cards bound to a managed account only work on Windows for now; in WSL use an unbound official card (its own sign-in) or an API key provider",
        ));
    }
    let spec = match config.target(&id).filter(|t| t.enabled) {
        Some(spec) => spec,
        None => enable_wsl_target(&app, &config, &id)?,
    };
    let activate = BTreeMap::from([(spec.id.key(), provider_id.to_string())]);
    let report = sync::sync_followers_with(
        state,
        &app,
        Some(std::slice::from_ref(&spec.id)),
        SyncKind::Provider,
        &activate,
    )
    .into_iter()
    .next()
    .ok_or_else(|| AppError::Message("没有可写入的目标".to_string()))?;
    Ok(report)
}

/// 把一个 WSL 发行版加入这个应用的写入环境（目录用它已保存的自定义目录或自动目录），并在
/// 写入前备份它原有的托管文件。不写客户端配置，由调用方接着写。
fn enable_wsl_target(
    app: &AppType,
    config: &AppTargetsConfig,
    id: &TargetId,
) -> Result<TargetSpec, AppError> {
    let TargetId::Wsl(distro) = id else {
        return Err(AppError::localized(
            "app_targets.target_disabled",
            format!("{} 没有勾选为写入环境", sync::env_label(id)),
            format!("{} is not a write target", sync::env_label(id)),
        ));
    };
    let _guard = save_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut next = config.clone();
    match next
        .wsl
        .iter_mut()
        .find(|t| t.distro.eq_ignore_ascii_case(distro))
    {
        Some(target) => target.enabled = true,
        None => next.wsl.push(model::WslTarget {
            distro: distro.clone(),
            enabled: true,
            route: false,
            dir: None,
        }),
    }
    let next = next.normalized();
    let resolved = sync::resolved_dirs(app, &next, &|spec| spec.enabled);
    if let Some(error) = model::validate(&next, &resolved).first() {
        return Err(AppError::localized(
            "app_targets.enable_failed",
            format!(
                "没能把 {} 加入写入环境（{}）",
                sync::env_label(id),
                error.code
            ),
            format!(
                "Could not add {} as a write target ({})",
                sync::env_label(id),
                error.code
            ),
        ));
    }
    let spec = next
        .target(id)
        .ok_or_else(|| AppError::Message("target missing after enabling".to_string()))?;
    let dir = sync::resolve_dir(app, &spec, true)?;
    let backup = sync::backup_target(app, &spec, dir)?;
    log::info!(
        "[app-targets] {} 加入 {} 的写入环境，原配置已备份到 {}",
        sync::env_label(id),
        app.as_str(),
        backup.display()
    );
    let legacy = model::legacy_projection(&next, &resolved);
    crate::settings::set_app_targets(app, next, legacy)?;
    Ok(spec)
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
    out.targets = sync::sync_followers(state, &app, followers.as_deref(), SyncKind::Full);
    Ok(out)
}

/// 某个跟随环境是否接入共用的路由。保存偏好后立刻按应用当前的模式同步：应用在路由 /
/// 聚合模式时接上或退回直连；在直连模式时只记下，下次进入路由时生效。
pub fn set_route(
    state: &AppState,
    app: &str,
    target: &str,
    join: bool,
) -> Result<TargetReport, AppError> {
    let app = parse_app(app)?;
    if !app.has_route_mode() {
        return Err(AppError::localized(
            "app_targets.route_unsupported",
            format!("{} 不支持本地路由", app.as_str()),
            format!("{} does not support local routing", app.as_str()),
        ));
    }
    let id = TargetId::parse(target)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown target {target}")))?;
    let _guard = save_lock().lock().unwrap_or_else(|e| e.into_inner());
    let (mut config, source) = sync::app_config(&app);
    if source != ConfigSource::Saved || config.primary_id().is_some_and(|p| p.same(&id)) {
        return Err(AppError::localized(
            "app_targets.route_primary",
            "主环境总是跟随应用的路由模式，不需要单独接入",
            "The primary environment always follows the app's routing mode",
        ));
    }
    match &id {
        TargetId::Windows => config.windows.route = join,
        TargetId::Wsl(distro) => {
            let entry = config
                .wsl
                .iter_mut()
                .find(|t| t.distro.eq_ignore_ascii_case(distro))
                .ok_or_else(|| AppError::InvalidInput(format!("unknown target {target}")))?;
            entry.route = join;
        }
    }
    let legacy = match &app {
        AppType::Claude => crate::settings::get_settings().claude_config_dir,
        AppType::Codex => crate::settings::get_settings().codex_config_dir,
        AppType::Gemini => crate::settings::get_settings().gemini_config_dir,
        _ => None,
    };
    crate::settings::set_app_targets(&app, config, legacy)?;
    let reports = sync::sync_follower_routes(state, &app, false);
    Ok(reports
        .into_iter()
        .find(|r| TargetId::parse(&r.target).is_some_and(|t| t.same(&id)))
        .unwrap_or_else(|| TargetReport {
            target: id.key(),
            record: TargetRecord::new(TargetStatus::Ok).reason(if join {
                "route_when_proxy"
            } else {
                "direct"
            }),
        }))
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
    let listen = futures::executor::block_on(state.db.get_proxy_config())
        .map(|c| c.listen_address)
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let listen_loopback = listen == "127.0.0.1" || listen == "localhost" || listen == "::1";
    inventory
        .distros
        .iter()
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
    for app in model::supported_apps() {
        sync::spawn_route_sync(state, &app);
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTestResult {
    pub ok: bool,
    pub url: String,
    pub detail: String,
}

/// 从发行版里访问路由服务的 /health，验证这个发行版真的连得上（自动检测只给候选地址）。
pub fn test_wsl_route(state: &AppState, distro: &str) -> Result<RouteTestResult, AppError> {
    let host = sync::client_host_for(&TargetId::Wsl(distro.to_string()))?
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let probe_scope = super::scope::TargetScope {
        app: AppType::Claude,
        key: format!("route-test:{distro}"),
        dir: None,
        store_root: crate::config::get_home_dir().join(".cc-switch"),
        wsl: true,
        direct: None,
        client_host: Some(host),
    };
    let (urls, _) = super::scope::run(probe_scope, || {
        futures::executor::block_on(state.proxy_service.build_proxy_urls())
    });
    let (origin, _) = urls.map_err(AppError::Message)?;
    let url = format!("{}/health", origin.trim_end_matches('/'));
    let (ok, detail) = wsl::fetch_from(distro, &url).map_err(AppError::Message)?;
    Ok(RouteTestResult { ok, url, detail })
}
