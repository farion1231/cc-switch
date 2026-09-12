//! CodeBuddy（additive）专属 Provider 服务。
//!
//! 以 `~/.codebuddy/models.json` 为事实源（一个 CC Switch 供应商 = 一条模型端点），
//! `settings.json` 的 `model` 字段记录"当前模型"（= 当前供应商）。DB 只做镜像与
//! UI 载体，理由与 Pi 一致：原生文件可能在 CC Switch 之外被 CodeBuddy 自己改写。
//!
//! 本模块被 ProviderService 在 AppType::CodeBuddy 时整路拦截
//! （list/add/update/delete/enable/import），不复用通用 switch-mode 流程。

use super::{ProviderService, SwitchResult};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::store::AppState;
use indexmap::IndexMap;
use serde_json::Value;

const CODEBUDDY_APP: &str = "codebuddy";

pub(super) fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(CODEBUDDY_APP));
    match crate::codebuddy_config::list_model_entries() {
        Ok(entries) => {
            if let Err(error) = sync_native_locked(state, &entries) {
                log::warn!("Failed to sync CodeBuddy providers from native config: {error}");
            }
        }
        Err(error) => {
            log::warn!("Failed to read CodeBuddy models.json; showing saved catalog: {error}");
        }
    }
    align_db_current_with_native(state)?;
    state.db.get_all_providers(CODEBUDDY_APP)
}

pub(super) fn import_from_live(state: &AppState) -> Result<usize, AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(CODEBUDDY_APP));
    let entries = crate::codebuddy_config::list_model_entries()?;
    let imported = sync_native_locked(state, &entries)?;
    align_db_current_with_native(state)?;
    Ok(imported)
}

pub(super) fn add(
    state: &AppState,
    provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(CODEBUDDY_APP));
    let app_type = AppType::CodeBuddy;
    ProviderService::validate_provider_settings(&app_type, &provider)?;

    if state
        .db
        .get_provider_by_id(&provider.id, CODEBUDDY_APP)?
        .is_some()
    {
        return Err(AppError::InvalidInput(format!(
            "CodeBuddy provider '{}' already exists",
            provider.id
        )));
    }

    // 原生 models.json 是 CodeBuddy 的事实源：新增供应商总是写入原生（合并语义
    // 由 codebuddy_config 保证），add_to_live 在此不再额外区分（与 DB 镜像一致）。
    let _ = add_to_live;
    crate::codebuddy_config::upsert_model_entry(&provider.settings_config)?;

    state.db.save_provider(CODEBUDDY_APP, &provider)?;
    Ok(true)
}

pub(super) fn update(
    state: &AppState,
    original_id: Option<&str>,
    provider: Provider,
) -> Result<bool, AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(CODEBUDDY_APP));
    let original_id = original_id.unwrap_or(&provider.id).to_string();
    if original_id != provider.id {
        return Err(AppError::InvalidInput(
            "CodeBuddy model keys cannot be renamed".to_string(),
        ));
    }
    state
        .db
        .get_provider_by_id(&original_id, CODEBUDDY_APP)?
        .ok_or_else(|| {
            AppError::InvalidInput(format!("CodeBuddy provider '{original_id}' not found"))
        })?;
    ProviderService::validate_provider_settings(&AppType::CodeBuddy, &provider)?;

    // 写回原生文件（合并语义由 codebuddy_config 保证），再落 DB。
    crate::codebuddy_config::upsert_model_entry(&provider.settings_config)?;
    state.db.save_provider(CODEBUDDY_APP, &provider)?;
    Ok(true)
}

pub(super) fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(CODEBUDDY_APP));
    if state.db.get_provider_by_id(id, CODEBUDDY_APP)?.is_none() {
        return Ok(());
    }
    remove_native_and_db(state, id)?;
    Ok(())
}

pub(super) fn remove(state: &AppState, id: &str) -> Result<(), AppError> {
    // "从 live 移除" = 仅从原生 models.json 移除，保留 DB 记录（镜像仍可重新
    // 设为当前模型，enable 会把它写回原生）。
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(CODEBUDDY_APP));
    if state.db.get_provider_by_id(id, CODEBUDDY_APP)?.is_none() {
        return Ok(());
    }
    crate::codebuddy_config::remove_model_entry(id)?;
    if crate::codebuddy_config::current_model_id()?.as_deref() == Some(id) {
        crate::codebuddy_config::set_current_model_id(None)?;
    }
    align_db_current_with_native(state)?;
    Ok(())
}

/// 切换供应商 = 把该模型端点设为 CodeBuddy 的当前模型（写入 settings.json），
/// 同时确保条目已在 models.json，并同步 DB 的 current 标记供 UI 展示。
pub(super) fn enable(state: &AppState, id: &str) -> Result<SwitchResult, AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(CODEBUDDY_APP));
    let provider = state
        .db
        .get_provider_by_id(id, CODEBUDDY_APP)?
        .ok_or_else(|| AppError::InvalidInput(format!("CodeBuddy provider '{id}' not found")))?;

    crate::codebuddy_config::upsert_model_entry(&provider.settings_config)?;
    crate::codebuddy_config::set_current_model_id(Some(&provider.id))?;
    state.db.set_current_provider(CODEBUDDY_APP, id)?;
    Ok(SwitchResult::default())
}

fn remove_native_and_db(state: &AppState, id: &str) -> Result<(), AppError> {
    crate::codebuddy_config::remove_model_entry(id)?;
    if crate::codebuddy_config::current_model_id()?.as_deref() == Some(id) {
        crate::codebuddy_config::set_current_model_id(None)?;
    }
    state.db.delete_provider(CODEBUDDY_APP, id)?;
    Ok(())
}

/// 用原生 models.json 条目镜像 DB：新增缺失、更新已存在（settings_config/名称跟随原生）。
fn sync_native_locked(state: &AppState, entries: &[Value]) -> Result<usize, AppError> {
    let saved = state.db.get_all_providers(CODEBUDDY_APP)?;
    let mut changed = 0;

    for entry in entries {
        let Some(id) = entry.get("id").and_then(Value::as_str) else {
            continue;
        };
        let mut provider = saved.get(id).cloned().unwrap_or_else(|| {
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_string();
            let mut imported = Provider::with_id(id.to_string(), name, entry.clone(), None);
            imported.category = Some("custom".to_string());
            imported.icon = Some("codebuddy".to_string());
            imported
        });
        let is_new = !saved.contains_key(id);
        let previous_config = provider.settings_config.clone();
        provider.settings_config = entry.clone();
        if let Some(name) = entry.get("name").and_then(Value::as_str) {
            provider.name = name.to_string();
        }
        if provider.settings_config != previous_config || is_new {
            state.db.save_provider(CODEBUDDY_APP, &provider)?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// 让 DB 的 current 标记跟随原生 settings.json 的当前模型。
/// - 原生有选中且 DB 有对应行 → 置为 current；
/// - 原生无选中（被清除/被外部删除）→ 清掉 DB current，避免残留高亮。
fn align_db_current_with_native(state: &AppState) -> Result<(), AppError> {
    match crate::codebuddy_config::current_model_id()? {
        Some(current)
            if state
                .db
                .get_provider_by_id(&current, CODEBUDDY_APP)?
                .is_some() =>
        {
            let _ = state.db.set_current_provider(CODEBUDDY_APP, &current);
        }
        _ => {
            let _ = state.db.clear_current_provider(CODEBUDDY_APP);
        }
    }
    Ok(())
}
