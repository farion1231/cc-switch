//! API Key 变量：在「设置 → API Key 变量」中定义、由本机脚本自动刷新的命名 Key。
//!
//! 供应商的 API Key 字段写 `{{NAME}}` 引用变量（数据库保存字面模板），在用到 Key 的
//! 地方再解析（见 [`resolve`]）。变量刷新成功后，重写所有引用它的当前供应商的 live 配置。

pub mod resolve;
pub mod runner;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::app_config::AppType;
use crate::database::ApiKeyVarRecord;
use crate::database::Database;
use crate::error::AppError;
use crate::store::AppState;

pub use resolve::RefStatus;
pub use runner::{ApiKeyRefreshConfig, ApiKeyRefreshState, RefreshReason, ResolvedScript};

use runner::{
    key_expired_now, mask_key, needs_refresh, now_ms, run_script, should_refresh_on_switch,
    RunFailure, ScriptContext, ScriptRun,
};

pub const EVENT_REFRESHED: &str = "api-key-var-refreshed";
pub const EVENT_REFRESH_FAILED: &str = "api-key-var-refresh-failed";
const SCHEDULER_TICK: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// 前端视图
// ---------------------------------------------------------------------------

/// 引用某变量的供应商
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VarReference {
    pub app: String,
    pub provider_id: String,
    pub provider_name: String,
    pub is_current: bool,
}

/// 返回前端的变量信息（只含掩码值）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyVarView {
    pub name: String,
    pub enabled: bool,
    pub script_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_time_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked_value: Option<String>,
    pub has_value: bool,
    pub state: ApiKeyRefreshState,
    pub references: Vec<VarReference>,
}

/// 前端提交的变量定义
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyVarInput {
    pub name: String,
    #[serde(default)]
    pub script_dir: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub lead_time_secs: Option<u64>,
}

impl ApiKeyVarInput {
    fn config(&self) -> ApiKeyRefreshConfig {
        ApiKeyRefreshConfig {
            enabled: self.enabled,
            script_dir: self.script_dir.trim().to_string(),
            timeout_secs: self.timeout_secs,
            lead_time_secs: self.lead_time_secs,
        }
    }
}

/// 一次刷新的结果（事件 payload / 命令返回值）
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RefreshOutcome {
    pub success: bool,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_script: Option<String>,
    pub needs_attention: bool,
    /// 失败且当前值已过期（或从未获取成功）
    pub value_expired: bool,
    /// 重写了 live 配置的供应商
    pub updated_providers: Vec<VarReference>,
    /// 重写 live 失败的说明
    pub live_errors: Vec<String>,
}

/// 测试脚本结果（不保存）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestOutcome {
    pub api_key_masked: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_after: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub file: String,
}

/// 供应商表单中某个字段的引用检查结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefCheck {
    pub name: String,
    pub status: RefStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// 校验与查询
// ---------------------------------------------------------------------------

pub fn validate_name(name: &str) -> Result<(), AppError> {
    if resolve::is_valid_name(name) {
        return Ok(());
    }
    Err(AppError::localized(
        "api_key_vars.name_invalid",
        format!("变量名 {name} 无效：只能包含大写字母、数字和下划线，不能以数字开头，不能以 CC_SWITCH_ 开头，最长 64 个字符"),
        format!("Invalid variable name {name}: use A-Z, 0-9 and _, not starting with a digit or CC_SWITCH_, at most 64 characters"),
    ))
}

fn var_not_found(name: &str) -> AppError {
    AppError::localized(
        "api_key_vars.not_found",
        format!("API Key 变量 {name} 不存在"),
        format!("API key variable {name} does not exist"),
    )
}

fn current_provider_id(db: &Database, app: &AppType) -> Option<String> {
    crate::mode::current::provider_for(db, app, crate::mode::current::Purpose::InUse)
        .ok()
        .flatten()
}

/// 所有供应商对变量的引用：name -> 引用列表
pub fn all_references(db: &Database) -> HashMap<String, Vec<VarReference>> {
    let mut out: HashMap<String, Vec<VarReference>> = HashMap::new();
    for app in resolve::supported_apps() {
        let Ok(providers) = db.get_all_providers(app.as_str()) else {
            continue;
        };
        let current = current_provider_id(db, &app);
        for provider in providers.values() {
            for name in resolve::referenced_vars(&app, &provider.settings_config) {
                out.entry(name).or_default().push(VarReference {
                    app: app.as_str().to_string(),
                    provider_id: provider.id.clone(),
                    provider_name: provider.name.clone(),
                    is_current: current.as_deref() == Some(provider.id.as_str()),
                });
            }
        }
    }
    out
}

fn to_view(rec: ApiKeyVarRecord, refs: &HashMap<String, Vec<VarReference>>) -> ApiKeyVarView {
    ApiKeyVarView {
        references: refs.get(&rec.name).cloned().unwrap_or_default(),
        name: rec.name,
        enabled: rec.config.enabled,
        script_dir: rec.config.script_dir,
        timeout_secs: rec.config.timeout_secs,
        lead_time_secs: rec.config.lead_time_secs,
        has_value: rec.value.is_some(),
        masked_value: rec.value.as_deref().map(mask_key),
        state: rec.state,
    }
}

pub fn list_views(db: &Database) -> Result<Vec<ApiKeyVarView>, AppError> {
    let refs = all_references(db);
    Ok(db
        .list_api_key_vars()?
        .into_iter()
        .map(|rec| to_view(rec, &refs))
        .collect())
}

/// 检查一份（表单中的）供应商配置里的引用
pub fn check_refs(
    db: &Database,
    app: &AppType,
    settings: &serde_json::Value,
) -> Result<Vec<RefCheck>, AppError> {
    let mut out = Vec::new();
    for (_, value) in resolve::key_field_entries(app, settings) {
        if resolve::is_malformed_ref(&value) {
            out.push(RefCheck {
                name: value.trim().to_string(),
                status: RefStatus::Malformed,
                masked_value: None,
                expires_at: None,
            });
            continue;
        }
        let Some(name) = resolve::parse_ref(&value) else {
            continue;
        };
        if out.iter().any(|c: &RefCheck| c.name == name) {
            continue;
        }
        let (status, v) = resolve::lookup(name);
        let expires_at = db
            .get_api_key_var(name)?
            .and_then(|rec| rec.state.expires_at);
        out.push(RefCheck {
            name: name.to_string(),
            status,
            masked_value: v.as_deref().map(mask_key),
            expires_at,
        });
    }
    Ok(out)
}

/// 检查单个值（前端 Key 输入框实时校验用）
pub fn check_value(db: &Database, value: &str) -> Result<Option<RefCheck>, AppError> {
    if resolve::is_malformed_ref(value) {
        return Ok(Some(RefCheck {
            name: value.trim().to_string(),
            status: RefStatus::Malformed,
            masked_value: None,
            expires_at: None,
        }));
    }
    let Some(name) = resolve::parse_ref(value) else {
        return Ok(None);
    };
    let (status, v) = resolve::lookup(name);
    Ok(Some(RefCheck {
        name: name.to_string(),
        status,
        masked_value: v.as_deref().map(mask_key),
        expires_at: db
            .get_api_key_var(name)?
            .and_then(|rec| rec.state.expires_at),
    }))
}

// ---------------------------------------------------------------------------
// 增删改
// ---------------------------------------------------------------------------

/// 新增 / 修改变量。改名时自动改写所有供应商中的引用。
pub fn upsert(
    state: &AppState,
    input: &ApiKeyVarInput,
    original_name: Option<&str>,
) -> Result<ApiKeyVarView, AppError> {
    let name = input.name.trim();
    validate_name(name)?;
    let config = input.config();
    if config.enabled {
        runner::resolve_script(&config.script_dir)?;
    }
    if let Some(t) = config.timeout_secs {
        if !(runner::MIN_TIMEOUT_SECS..=runner::MAX_TIMEOUT_SECS).contains(&t) {
            return Err(AppError::localized(
                "api_key_vars.timeout_invalid",
                format!(
                    "超时需在 {}–{} 秒之间",
                    runner::MIN_TIMEOUT_SECS,
                    runner::MAX_TIMEOUT_SECS
                ),
                format!(
                    "Timeout must be between {} and {} seconds",
                    runner::MIN_TIMEOUT_SECS,
                    runner::MAX_TIMEOUT_SECS
                ),
            ));
        }
    }
    let db = &state.db;
    let original = original_name.map(str::trim).filter(|o| !o.is_empty());
    if let Some(orig) = original {
        if db.get_api_key_var(orig)?.is_none() {
            return Err(var_not_found(orig));
        }
    }
    let renaming = original.is_some_and(|o| o != name);
    if (original.is_none() || renaming) && db.get_api_key_var(name)?.is_some() {
        return Err(AppError::localized(
            "api_key_vars.name_duplicate",
            format!("变量名 {name} 已存在"),
            format!("Variable name {name} already exists"),
        ));
    }

    let record = db.upsert_api_key_var(original, name, &config)?;

    if renaming {
        let old = original.unwrap_or_default();
        let mut rewritten = 0usize;
        for app in resolve::supported_apps() {
            for provider in db.get_all_providers(app.as_str())?.values() {
                if let Some(new_settings) =
                    resolve::rewrite_refs(&app, &provider.settings_config, old, name)
                {
                    db.update_provider_settings_config(app.as_str(), &provider.id, &new_settings)?;
                    rewritten += 1;
                }
            }
        }
        log::info!("[ApiKeyVars] 变量 {old} 改名为 {name}，改写了 {rewritten} 个供应商的引用");
    }
    wake_scheduler();
    let refs = all_references(db);
    Ok(to_view(record, &refs))
}

/// 删除变量：仍被引用时拒绝
pub fn delete(state: &AppState, name: &str) -> Result<(), AppError> {
    let refs = all_references(&state.db);
    if let Some(list) = refs.get(name).filter(|l| !l.is_empty()) {
        let names = list
            .iter()
            .map(|r| format!("{}/{}", r.app, r.provider_name))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(AppError::localized(
            "api_key_vars.in_use",
            format!("变量 {name} 正被以下供应商引用，无法删除：{names}"),
            format!(
                "Variable {name} is referenced by these providers and cannot be deleted: {names}"
            ),
        ));
    }
    if !state.db.delete_api_key_var(name)? {
        return Err(var_not_found(name));
    }
    Ok(())
}

/// 切换前检查：目标供应商引用的变量都能解析
pub fn ensure_provider_resolvable(
    app: &AppType,
    settings: &serde_json::Value,
) -> Result<(), AppError> {
    resolve::ensure_resolvable(app, settings)
}

// ---------------------------------------------------------------------------
// 刷新
// ---------------------------------------------------------------------------

fn var_lock(name: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let locks = LOCKS.get_or_init(Default::default);
    let mut guard = locks.lock().unwrap_or_else(|e| e.into_inner());
    guard.entry(name.to_string()).or_default().clone()
}

/// 执行脚本（异步，不写库）
async fn execute(rec: &ApiKeyVarRecord, reason: RefreshReason) -> Result<ScriptRun, RunFailure> {
    let ctx = ScriptContext {
        var_name: rec.name.clone(),
        reason: reason.as_str(),
        current_expires_at: rec.state.expires_at,
    };
    let node_setting = crate::settings::get_settings().api_key_refresh_node_path;
    let mut secrets: Vec<String> = rec.value.iter().cloned().collect();
    secrets.extend(resolve::known_values(&rec.name));
    let secrets: Vec<&str> = secrets.iter().map(String::as_str).collect();
    log::info!("[ApiKeyVars] {} 开始刷新（{}）", rec.name, reason.as_str());
    run_script(&rec.config, &ctx, node_setting.as_deref(), &secrets).await
}

/// 重写所有引用 `name` 的当前供应商的 live 配置
pub fn propagate(state: &AppState, name: &str) -> (Vec<VarReference>, Vec<String>) {
    let mut updated = Vec::new();
    let mut errors = Vec::new();
    for app in resolve::supported_apps() {
        let Some(current) = current_provider_id(&state.db, &app) else {
            continue;
        };
        let Ok(Some(provider)) = state.db.get_provider_by_id(&current, app.as_str()) else {
            continue;
        };
        if !resolve::referenced_vars(&app, &provider.settings_config)
            .iter()
            .any(|n| n == name)
        {
            continue;
        }
        match crate::services::ProviderService::sync_current_provider_for_app(state, app.clone()) {
            Ok(()) => updated.push(VarReference {
                app: app.as_str().to_string(),
                provider_id: provider.id.clone(),
                provider_name: provider.name.clone(),
                is_current: true,
            }),
            Err(e) => {
                log::warn!(
                    "[ApiKeyVars] 变量 {name} 刷新后重写 {} live 配置失败: {e}",
                    app.as_str()
                );
                errors.push(format!("{}/{}: {e}", app.as_str(), provider.name));
            }
        }
    }
    (updated, errors)
}

/// Builds the result for a refresh request that was merged into an already
/// successful run. Re-project live configuration here as the earlier run may
/// have persisted the value while its live write failed.
fn merged_refresh_outcome(state: &AppState, name: &str) -> Result<RefreshOutcome, AppError> {
    let rec = state
        .db
        .get_api_key_var(name)?
        .ok_or_else(|| var_not_found(name))?;
    let mut outcome = RefreshOutcome {
        success: true,
        name: rec.name.clone(),
        masked_value: rec.value.as_deref().map(mask_key),
        expires_at: rec.state.expires_at,
        resolved_script: rec.state.resolved_script.clone(),
        ..Default::default()
    };
    if rec.config.enabled {
        let (updated, errors) = propagate(state, &rec.name);
        outcome.updated_providers = updated;
        outcome.live_errors = errors;
    }
    Ok(outcome)
}

/// 把脚本结果写回变量（同步；会写库并传播到 live）
pub fn commit_result(
    state: &AppState,
    name: &str,
    result: Result<ScriptRun, RunFailure>,
) -> Result<RefreshOutcome, AppError> {
    let rec = state
        .db
        .get_api_key_var(name)?
        .ok_or_else(|| var_not_found(name))?;
    commit_result_for_snapshot(state, rec, result)
}

/// Commits the result for the exact record that launched the script. The DAO
/// compares this snapshot atomically so an old script cannot overwrite a newer
/// edit, rename, deletion, or same-name recreation.
fn commit_result_for_snapshot(
    state: &AppState,
    rec: ApiKeyVarRecord,
    result: Result<ScriptRun, RunFailure>,
) -> Result<RefreshOutcome, AppError> {
    let name = &rec.name;
    let mut st = rec.state.clone();
    let now = now_ms();
    st.last_attempt_at = Some(now);
    let mut outcome = RefreshOutcome {
        name: name.to_string(),
        ..Default::default()
    };
    let new_value = match result {
        Ok(run) => {
            st.expires_at = run.output.expires_at;
            st.refresh_after = run.output.refresh_after;
            st.last_success_at = Some(now);
            st.last_error = None;
            st.last_message = run.output.message.clone();
            st.resolved_script = Some(run.resolved_file.clone());
            st.consecutive_failures = 0;
            st.needs_attention = false;
            outcome.success = true;
            outcome.masked_value = Some(mask_key(&run.output.api_key));
            outcome.expires_at = run.output.expires_at;
            outcome.message = run.output.message;
            outcome.resolved_script = Some(run.resolved_file);
            log::info!(
                "[ApiKeyVars] {name} 刷新成功，value={}，expiresAt={:?}",
                outcome.masked_value.as_deref().unwrap_or_default(),
                st.expires_at
            );
            Some(run.output.api_key)
        }
        Err(failure) => {
            st.consecutive_failures = st.consecutive_failures.saturating_add(1);
            st.last_error = Some(failure.error.clone());
            if failure.permanent {
                st.needs_attention = true;
            }
            outcome.error = Some(failure.error.clone());
            outcome.needs_attention = st.needs_attention;
            outcome.value_expired = key_expired_now(&st, now);
            outcome.expires_at = st.expires_at;
            log::warn!(
                "[ApiKeyVars] {name} 刷新失败（第 {} 次）: {}",
                st.consecutive_failures,
                failure.error
            );
            None
        }
    };
    if !state
        .db
        .update_api_key_var_result_if_current(&rec, new_value.as_deref(), &st)?
    {
        return Err(AppError::Conflict(format!(
            "API Key 变量 {name} 在刷新期间已被修改，已丢弃旧脚本结果"
        )));
    }
    if outcome.success && rec.config.enabled {
        let (updated, errors) = propagate(state, name);
        outcome.updated_providers = updated;
        outcome.live_errors = errors;
    }
    Ok(outcome)
}

/// 完整刷新流程（加锁 → 执行 → 写回 → 传播 → 事件）
pub async fn refresh_var(
    app: &tauri::AppHandle,
    name: &str,
    reason: RefreshReason,
) -> Result<RefreshOutcome, AppError> {
    use tauri::{Emitter, Manager};

    let requested_at = now_ms();
    let lock = var_lock(name);
    let _guard = lock.lock().await;

    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| AppError::Message("应用状态不可用".to_string()))?;
    let rec = state
        .db
        .get_api_key_var(name)?
        .ok_or_else(|| var_not_found(name))?;
    // 合并并发请求：等锁期间已有一次成功刷新
    if rec.state.last_success_at.is_some_and(|t| t >= requested_at) {
        let handle = app.clone();
        let name = name.to_string();
        return tauri::async_runtime::spawn_blocking(move || {
            let state = handle
                .try_state::<AppState>()
                .ok_or_else(|| AppError::Message("应用状态不可用".to_string()))?;
            merged_refresh_outcome(state.inner(), &name)
        })
        .await
        .map_err(|e| AppError::Message(format!("刷新任务执行失败: {e}")))?;
    }
    if !rec.config.enabled && reason != RefreshReason::Manual {
        return Err(AppError::localized(
            "api_key_vars.disabled",
            format!("变量 {name} 已停用"),
            format!("Variable {name} is disabled"),
        ));
    }

    let expected = rec.clone();
    let result = execute(&rec, reason).await;
    let handle = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let state = handle
            .try_state::<AppState>()
            .ok_or_else(|| AppError::Message("应用状态不可用".to_string()))?;
        commit_result_for_snapshot(state.inner(), expected, result)
    })
    .await
    .map_err(|e| AppError::Message(format!("刷新任务执行失败: {e}")))??;

    let event = if outcome.success {
        EVENT_REFRESHED
    } else {
        EVENT_REFRESH_FAILED
    };
    if let Err(e) = app.emit(event, &outcome) {
        log::warn!("[ApiKeyVars] 发送事件失败: {e}");
    }
    // 托盘的问题区按失败变量重建（失败状态变化时）
    let was_failing = rec.state.consecutive_failures > 0 || rec.state.needs_attention;
    if was_failing == outcome.success || !outcome.success {
        crate::tray::refresh_tray_menu(app);
    }
    Ok(outcome)
}

/// 切换前刷新：目标供应商引用的变量已过期 / 即将过期时先刷新。返回警告（刷新失败）。
pub async fn refresh_before_switch(
    app: &tauri::AppHandle,
    app_type: &AppType,
    id: &str,
) -> Option<String> {
    use tauri::Manager;
    if !resolve::supports_app(app_type) {
        return None;
    }
    let state = app.try_state::<AppState>()?;
    let provider = state.db.get_provider_by_id(id, app_type.as_str()).ok()??;
    let names = resolve::referenced_vars(app_type, &provider.settings_config);
    let mut warnings = Vec::new();
    for name in names {
        let Ok(Some(rec)) = state.db.get_api_key_var(&name) else {
            continue;
        };
        if !should_refresh_on_switch(&rec.config, &rec.state, now_ms()) {
            continue;
        }
        match refresh_var(app, &name, RefreshReason::Switch).await {
            Ok(outcome) if outcome.success => {}
            Ok(outcome) => warnings.push(format!(
                "API Key 变量 {name} 刷新失败: {}",
                outcome.error.unwrap_or_default()
            )),
            Err(e) => warnings.push(format!("API Key 变量 {name} 刷新失败: {e}")),
        }
    }
    (!warnings.is_empty()).then(|| warnings.join("; "))
}

/// 测试脚本（不保存）
pub async fn test_script(input: &ApiKeyVarInput) -> Result<TestOutcome, AppError> {
    let mut cfg = input.config();
    cfg.enabled = true;
    let ctx = ScriptContext {
        var_name: input.name.trim().to_string(),
        reason: RefreshReason::Test.as_str(),
        current_expires_at: None,
    };
    let node_setting = crate::settings::get_settings().api_key_refresh_node_path;
    match run_script(&cfg, &ctx, node_setting.as_deref(), &[]).await {
        Ok(run) => Ok(TestOutcome {
            api_key_masked: mask_key(&run.output.api_key),
            expires_at: run.output.expires_at,
            refresh_after: run.output.refresh_after,
            message: run.output.message,
            file: run.resolved_file,
        }),
        Err(failure) => Err(AppError::Message(failure.error)),
    }
}

/// 处于失败状态的变量（托盘问题区用）：(name, 错误摘要)
pub fn failing_vars(db: &Database) -> Vec<(String, String)> {
    db.list_api_key_vars()
        .unwrap_or_default()
        .into_iter()
        .filter(|v| {
            v.config.enabled && (v.state.consecutive_failures > 0 || v.state.needs_attention)
        })
        .map(|v| (v.name, v.state.last_error.unwrap_or_default()))
        .collect()
}

// ---------------------------------------------------------------------------
// 调度器
// ---------------------------------------------------------------------------

fn scheduler_notify() -> &'static Notify {
    static NOTIFY: OnceLock<Notify> = OnceLock::new();
    NOTIFY.get_or_init(Notify::new)
}

/// 配置变化后唤醒调度器立即重新检查
pub fn wake_scheduler() {
    scheduler_notify().notify_one();
}

/// 到期需要刷新的变量
pub fn due_vars(db: &Database, now: i64) -> Vec<(String, RefreshReason)> {
    db.list_api_key_vars()
        .unwrap_or_default()
        .into_iter()
        .filter(|v| needs_refresh(&v.config, &v.state, now))
        .map(|v| {
            let reason = if v.state.consecutive_failures > 0 {
                RefreshReason::Retry
            } else {
                RefreshReason::Scheduled
            };
            (v.name, reason)
        })
        .collect()
}

/// 启动后台调度器（启动时全量检查一次，之后每 60 秒或被唤醒时检查）
pub fn start_worker(app: tauri::AppHandle) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        // 缓存已由应用启动流程在注册 AppState 前同步加载，避免异步重载覆盖新值。
        // 等待启动流程完成
        tokio::time::sleep(Duration::from_secs(3)).await;
        let mut first = true;
        loop {
            if let Some(state) = app.try_state::<AppState>() {
                let db = state.db.clone();
                for (name, reason) in due_vars(&db, now_ms()) {
                    let reason = if first && reason == RefreshReason::Scheduled {
                        RefreshReason::Startup
                    } else {
                        reason
                    };
                    if let Err(e) = refresh_var(&app, &name, reason).await {
                        log::warn!("[ApiKeyVars] {name} 刷新出错: {e}");
                    }
                }
            }
            first = false;
            tokio::select! {
                _ = tokio::time::sleep(SCHEDULER_TICK) => {}
                _ = scheduler_notify().notified() => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tempfile::tempdir;

    use crate::app_config::MultiAppConfig;
    use crate::provider::Provider;

    use super::*;

    struct TestHomeGuard {
        previous_test_home: Option<std::ffi::OsString>,
    }

    impl TestHomeGuard {
        fn set(path: &std::path::Path) -> Self {
            let previous_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", path);
            Self { previous_test_home }
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            match self.previous_test_home.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    fn name_validation_errors_are_localized() {
        assert!(validate_name("GOOD_NAME").is_ok());
        let e = validate_name("bad").unwrap_err();
        assert!(matches!(
            e,
            AppError::Localized {
                key: "api_key_vars.name_invalid",
                ..
            }
        ));
    }

    #[test]
    fn due_vars_respects_enabled_and_state() -> Result<(), AppError> {
        let db = Database::memory()?;
        let cfg = |enabled| ApiKeyRefreshConfig {
            enabled,
            script_dir: "/x".into(),
            timeout_secs: None,
            lead_time_secs: None,
        };
        db.upsert_api_key_var(None, "DUE_T_ON", &cfg(true))?;
        db.upsert_api_key_var(None, "DUE_T_OFF", &cfg(false))?;
        db.upsert_api_key_var(None, "DUE_T_FRESH", &cfg(true))?;
        let now = now_ms();
        db.update_api_key_var_result(
            "DUE_T_FRESH",
            Some("sk-fresh-value"),
            &ApiKeyRefreshState {
                last_success_at: Some(now),
                expires_at: Some(now + 3_600_000),
                ..Default::default()
            },
        )?;
        let due: Vec<String> = due_vars(&db, now).into_iter().map(|(n, _)| n).collect();
        assert_eq!(due, vec!["DUE_T_ON".to_string()]);
        Ok(())
    }

    #[test]
    fn stale_script_results_do_not_commit_after_variable_changes() -> Result<(), AppError> {
        let state = AppState::new(Arc::new(Database::memory()?));
        let cfg = |enabled, script_dir: &str| ApiKeyRefreshConfig {
            enabled,
            script_dir: script_dir.into(),
            timeout_secs: None,
            lead_time_secs: None,
        };
        let success = |value: &str| -> Result<ScriptRun, RunFailure> {
            Ok(ScriptRun {
                output: runner::RefreshOutput {
                    api_key: value.into(),
                    expires_at: None,
                    refresh_after: None,
                    message: None,
                },
                resolved_file: "old-script.mjs".into(),
            })
        };
        let failure = || -> Result<ScriptRun, RunFailure> {
            Err(RunFailure {
                permanent: true,
                error: "old script failed".into(),
            })
        };
        const OLD_NAME: &str = "SERVICE_STALE_RESULT";
        const RENAMED_NAME: &str = "SERVICE_STALE_RENAMED";

        state
            .db
            .upsert_api_key_var(None, OLD_NAME, &cfg(true, "/old"))?;
        let before_edit = state.db.get_api_key_var(OLD_NAME)?.unwrap();
        state
            .db
            .upsert_api_key_var(Some(OLD_NAME), OLD_NAME, &cfg(false, "/new"))?;
        assert!(matches!(
            commit_result_for_snapshot(&state, before_edit, success("old-success")),
            Err(AppError::Conflict(_))
        ));
        let disabled = state.db.get_api_key_var(OLD_NAME)?.unwrap();
        assert_eq!(disabled.value, None);
        assert_eq!(disabled.state, ApiKeyRefreshState::default());
        assert_eq!(resolve::lookup(OLD_NAME).0, RefStatus::Disabled);

        let before_rename = state.db.get_api_key_var(OLD_NAME)?.unwrap();
        state
            .db
            .upsert_api_key_var(Some(OLD_NAME), RENAMED_NAME, &cfg(false, "/new"))?;
        assert!(matches!(
            commit_result_for_snapshot(&state, before_rename, failure()),
            Err(AppError::Conflict(_))
        ));
        let renamed = state.db.get_api_key_var(RENAMED_NAME)?.unwrap();
        assert_eq!(renamed.state, ApiKeyRefreshState::default());
        assert_eq!(resolve::lookup(RENAMED_NAME).0, RefStatus::Disabled);

        let before_delete = renamed;
        assert!(state.db.delete_api_key_var(RENAMED_NAME)?);
        state
            .db
            .upsert_api_key_var(None, RENAMED_NAME, &cfg(true, "/replacement"))?;
        state.db.update_api_key_var_result(
            RENAMED_NAME,
            Some("replacement-value"),
            &ApiKeyRefreshState::default(),
        )?;
        assert!(matches!(
            commit_result_for_snapshot(&state, before_delete, failure()),
            Err(AppError::Conflict(_))
        ));
        let replacement = state.db.get_api_key_var(RENAMED_NAME)?.unwrap();
        assert_eq!(replacement.value.as_deref(), Some("replacement-value"));
        assert_eq!(replacement.state, ApiKeyRefreshState::default());
        assert_eq!(
            resolve::lookup(RENAMED_NAME),
            (RefStatus::Ok, Some("replacement-value".into()))
        );
        resolve::cache_remove(RENAMED_NAME);
        Ok(())
    }

    #[tokio::test]
    async fn running_script_result_is_discarded_after_configuration_change() -> Result<(), AppError>
    {
        let script_dir = tempdir().map_err(|error| AppError::Message(error.to_string()))?;
        std::fs::write(
            script_dir.path().join("index.mjs"),
            r#"import { existsSync, writeFileSync } from "node:fs";
writeFileSync("started", "");
const deadline = Date.now() + 4500;
while (!existsSync("continue")) {
  if (Date.now() > deadline) process.exit(2);
  await new Promise(resolve => setTimeout(resolve, 10));
}
process.stdout.write(JSON.stringify({ apiKey: "old-script-key" }));"#,
        )
        .map_err(|error| AppError::Io {
            path: script_dir.path().join("index.mjs").display().to_string(),
            source: error,
        })?;

        let state = AppState::new(Arc::new(Database::memory()?));
        let initial = ApiKeyRefreshConfig {
            enabled: true,
            script_dir: script_dir.path().display().to_string(),
            timeout_secs: Some(5),
            lead_time_secs: None,
        };
        state
            .db
            .upsert_api_key_var(None, "RUNNING_SCRIPT_STALE", &initial)?;
        let expected = state.db.get_api_key_var("RUNNING_SCRIPT_STALE")?.unwrap();
        let running = expected.clone();
        let task = tokio::spawn(async move { execute(&running, RefreshReason::Manual).await });
        let started = script_dir.path().join("started");
        tokio::time::timeout(Duration::from_secs(5), async {
            while !started.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Node script did not create its started marker");

        let changed = ApiKeyRefreshConfig {
            enabled: false,
            script_dir: "/new-script".into(),
            timeout_secs: None,
            lead_time_secs: None,
        };
        state.db.upsert_api_key_var(
            Some("RUNNING_SCRIPT_STALE"),
            "RUNNING_SCRIPT_STALE",
            &changed,
        )?;
        std::fs::write(script_dir.path().join("continue"), "go").map_err(|error| AppError::Io {
            path: script_dir.path().join("continue").display().to_string(),
            source: error,
        })?;
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("Node script did not finish after its continue marker")
            .expect("Node task panicked")
            .expect("Node script must succeed before its stale result is rejected");
        assert_eq!(result.output.api_key, "old-script-key");
        assert!(matches!(
            commit_result_for_snapshot(&state, expected, Ok(result)),
            Err(AppError::Conflict(_))
        ));
        let current = state.db.get_api_key_var("RUNNING_SCRIPT_STALE")?.unwrap();
        assert_eq!(current.config, changed);
        assert_eq!(current.value, None);
        assert_eq!(current.state, ApiKeyRefreshState::default());
        assert_eq!(
            resolve::lookup("RUNNING_SCRIPT_STALE").0,
            RefStatus::Disabled
        );
        resolve::cache_remove("RUNNING_SCRIPT_STALE");
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn merged_refresh_retries_live_projection_after_same_value_refresh() -> Result<(), AppError> {
        let home = tempdir().map_err(|error| AppError::Message(error.to_string()))?;
        let _home_guard = TestHomeGuard::set(home.path());

        {
            let mut config = MultiAppConfig::default();
            config
                .get_manager_mut(&AppType::Claude)
                .unwrap()
                .providers
                .insert(
                    "merged-claude".into(),
                    Provider::with_id(
                        "merged-claude".into(),
                        "Merged Claude".into(),
                        json!({"env": {
                            "ANTHROPIC_AUTH_TOKEN": "{{MERGED_REFRESH_KEY}}",
                            "ANTHROPIC_BASE_URL": "https://relay.example"
                        }}),
                        None,
                    ),
                );
            let db = Arc::new(Database::memory()?);
            db.migrate_from_json(&config)?;
            db.set_current_provider(AppType::Claude.as_str(), "merged-claude")?;
            let state = AppState::new(db);
            let cfg = ApiKeyRefreshConfig {
                enabled: true,
                script_dir: String::new(),
                timeout_secs: None,
                lead_time_secs: None,
            };
            state
                .db
                .upsert_api_key_var(None, "MERGED_REFRESH_KEY", &cfg)?;

            let live_path = crate::config::get_claude_settings_path();
            assert!(
                live_path.starts_with(home.path()),
                "Claude settings override points outside isolated test home: {}",
                live_path.display()
            );
            std::fs::create_dir_all(&live_path).map_err(|error| AppError::IoContext {
                context: "prepare broken isolated Claude live path".into(),
                source: error,
            })?;
            let first = commit_result_for_snapshot(
                &state,
                state.db.get_api_key_var("MERGED_REFRESH_KEY")?.unwrap(),
                Ok(ScriptRun {
                    output: runner::RefreshOutput {
                        api_key: "same-value".into(),
                        expires_at: None,
                        refresh_after: None,
                        message: None,
                    },
                    resolved_file: "first.mjs".into(),
                }),
            )?;
            assert!(first.success);
            assert!(first.updated_providers.is_empty());
            assert!(!first.live_errors.is_empty());

            std::fs::remove_dir_all(&live_path).map_err(|error| AppError::IoContext {
                context: "repair isolated Claude live path".into(),
                source: error,
            })?;
            let merged = merged_refresh_outcome(&state, "MERGED_REFRESH_KEY")?;
            assert!(merged.success);
            assert_eq!(merged.updated_providers.len(), 1, "{merged:?}");
            assert!(merged.live_errors.is_empty(), "{merged:?}");
            let live = std::fs::read_to_string(&live_path).map_err(|error| AppError::Io {
                path: live_path.display().to_string(),
                source: error,
            })?;
            assert!(live.contains("same-value"), "{live}");
            resolve::cache_remove("MERGED_REFRESH_KEY");
            Ok(())
        }
    }
}
