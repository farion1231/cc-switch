use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app_config::{AppType, MultiAppConfig};
use crate::codex_config::{get_codex_auth_path, get_codex_config_path, write_codex_live_atomic};
use crate::config::{
    delete_file, get_claude_settings_path, get_provider_config_path, read_json_file,
    write_json_file, write_text_file,
};
use crate::error::AppError;
use crate::mcp;
use crate::provider::{Provider, ProviderMeta, UsageData, UsageResult};
use crate::settings::{self, CustomEndpoint};
use crate::store::AppState;
use crate::usage_script;

/// 供应商相关业务逻辑
pub struct ProviderService;

#[derive(Clone)]
enum LiveSnapshot {
    Claude {
        settings: Option<Value>,
    },
    Codex {
        auth: Option<Value>,
        config: Option<String>,
    },
}

#[derive(Clone)]
struct PostCommitAction {
    app_type: AppType,
    provider: Provider,
    backup: LiveSnapshot,
    sync_mcp: bool,
    refresh_snapshot: bool,
}

impl LiveSnapshot {
    fn restore(&self) -> Result<(), AppError> {
        match self {
            LiveSnapshot::Claude { settings } => {
                let path = get_claude_settings_path();
                if let Some(value) = settings {
                    write_json_file(&path, value)?;
                } else if path.exists() {
                    delete_file(&path)?;
                }
            }
            LiveSnapshot::Codex { auth, config } => {
                let auth_path = get_codex_auth_path();
                let config_path = get_codex_config_path();
                if let Some(value) = auth {
                    write_json_file(&auth_path, value)?;
                } else if auth_path.exists() {
                    delete_file(&auth_path)?;
                }

                if let Some(text) = config {
                    write_text_file(&config_path, text)?;
                } else if config_path.exists() {
                    delete_file(&config_path)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_provider_settings_rejects_missing_auth() {
        let provider = Provider::with_id(
            "codex".into(),
            "Codex".into(),
            json!({ "config": "base_url = \"https://example.com\"" }),
            None,
        );
        let err = ProviderService::validate_provider_settings(&AppType::Codex, &provider)
            .expect_err("missing auth should be rejected");
        assert!(
            err.to_string().contains("auth"),
            "expected auth error, got {err:?}"
        );
    }

    #[test]
    fn extract_credentials_returns_expected_values() {
        let provider = Provider::with_id(
            "claude".into(),
            "Claude".into(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token",
                    "ANTHROPIC_BASE_URL": "https://claude.example"
                }
            }),
            None,
        );
        let (api_key, base_url) =
            ProviderService::extract_credentials(&provider, &AppType::Claude).unwrap();
        assert_eq!(api_key, "token");
        assert_eq!(base_url, "https://claude.example");
    }
}

impl ProviderService {
    /// 归一化 Claude 模型键：读旧键(ANTHROPIC_SMALL_FAST_MODEL)，写新键(DEFAULT_*), 并删除旧键
    fn normalize_claude_models_in_value(settings: &mut Value) -> bool {
        let mut changed = false;
        let env = match settings.get_mut("env") {
            Some(v) if v.is_object() => v.as_object_mut().unwrap(),
            _ => return changed,
        };

        let model = env
            .get("ANTHROPIC_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let small_fast = env
            .get("ANTHROPIC_SMALL_FAST_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let current_haiku = env
            .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let current_sonnet = env
            .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let current_opus = env
            .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let target_haiku = current_haiku
            .or_else(|| small_fast.clone())
            .or_else(|| model.clone());
        let target_sonnet = current_sonnet
            .or_else(|| model.clone())
            .or_else(|| small_fast.clone());
        let target_opus = current_opus
            .or_else(|| model.clone())
            .or_else(|| small_fast.clone());

        if env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL").is_none() {
            if let Some(v) = target_haiku {
                env.insert(
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
                    Value::String(v),
                );
                changed = true;
            }
        }
        if env.get("ANTHROPIC_DEFAULT_SONNET_MODEL").is_none() {
            if let Some(v) = target_sonnet {
                env.insert(
                    "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
                    Value::String(v),
                );
                changed = true;
            }
        }
        if env.get("ANTHROPIC_DEFAULT_OPUS_MODEL").is_none() {
            if let Some(v) = target_opus {
                env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(), Value::String(v));
                changed = true;
            }
        }

        if env.remove("ANTHROPIC_SMALL_FAST_MODEL").is_some() {
            changed = true;
        }

        changed
    }

    fn normalize_provider_if_claude(app_type: &AppType, provider: &mut Provider) {
        if matches!(app_type, AppType::Claude) {
            let mut v = provider.settings_config.clone();
            if Self::normalize_claude_models_in_value(&mut v) {
                provider.settings_config = v;
            }
        }
    }
    fn run_transaction<R, F>(state: &AppState, f: F) -> Result<R, AppError>
    where
        F: FnOnce(&mut MultiAppConfig) -> Result<(R, Option<PostCommitAction>), AppError>,
    {
        let mut guard = state.config.write().map_err(AppError::from)?;
        let original = guard.clone();
        let (result, action) = match f(&mut guard) {
            Ok(value) => value,
            Err(err) => {
                *guard = original;
                return Err(err);
            }
        };
        drop(guard);

        if let Err(save_err) = state.save() {
            if let Err(rollback_err) = Self::restore_config_only(state, original.clone()) {
                return Err(AppError::localized(
                    "config.save.rollback_failed",
                    format!("保存配置失败: {}；回滚失败: {}", save_err, rollback_err),
                    format!(
                        "Failed to save config: {}; rollback failed: {}",
                        save_err, rollback_err
                    ),
                ));
            }
            return Err(save_err);
        }

        if let Some(action) = action {
            if let Err(err) = Self::apply_post_commit(state, &action) {
                if let Err(rollback_err) =
                    Self::rollback_after_failure(state, original.clone(), action.backup.clone())
                {
                    return Err(AppError::localized(
                        "post_commit.rollback_failed",
                        format!("后置操作失败: {}；回滚失败: {}", err, rollback_err),
                        format!(
                            "Post-commit step failed: {}; rollback failed: {}",
                            err, rollback_err
                        ),
                    ));
                }
                return Err(err);
            }
        }

        Ok(result)
    }

    fn restore_config_only(state: &AppState, snapshot: MultiAppConfig) -> Result<(), AppError> {
        {
            let mut guard = state.config.write().map_err(AppError::from)?;
            *guard = snapshot;
        }
        state.save()
    }

    fn rollback_after_failure(
        state: &AppState,
        snapshot: MultiAppConfig,
        backup: LiveSnapshot,
    ) -> Result<(), AppError> {
        Self::restore_config_only(state, snapshot)?;
        backup.restore()
    }

    fn apply_post_commit(state: &AppState, action: &PostCommitAction) -> Result<(), AppError> {
        Self::write_live_snapshot(&action.app_type, &action.provider)?;
        if action.sync_mcp {
            let config_clone = {
                let guard = state.config.read().map_err(AppError::from)?;
                guard.clone()
            };
            mcp::sync_enabled_to_codex(&config_clone)?;
        }
        if action.refresh_snapshot {
            Self::refresh_provider_snapshot(state, &action.app_type, &action.provider.id)?;
        }
        Ok(())
    }

    fn refresh_provider_snapshot(
        state: &AppState,
        app_type: &AppType,
        provider_id: &str,
    ) -> Result<(), AppError> {
        match app_type {
            AppType::Claude => {
                let settings_path = get_claude_settings_path();
                if !settings_path.exists() {
                    return Err(AppError::localized(
                        "claude.live.missing",
                        "Claude 设置文件不存在，无法刷新快照",
                        "Claude settings file missing; cannot refresh snapshot",
                    ));
                }
                let mut live_after = read_json_file::<Value>(&settings_path)?;
                let _ = Self::normalize_claude_models_in_value(&mut live_after);
                {
                    let mut guard = state.config.write().map_err(AppError::from)?;
                    if let Some(manager) = guard.get_manager_mut(app_type) {
                        if let Some(target) = manager.providers.get_mut(provider_id) {
                            target.settings_config = live_after;
                        }
                    }
                }
                state.save()?;
            }
            AppType::Codex => {
                let auth_path = get_codex_auth_path();
                if !auth_path.exists() {
                    return Err(AppError::localized(
                        "codex.live.missing",
                        "Codex auth.json 不存在，无法刷新快照",
                        "Codex auth.json missing; cannot refresh snapshot",
                    ));
                }
                let auth: Value = read_json_file(&auth_path)?;
                let cfg_text = crate::codex_config::read_and_validate_codex_config_text()?;

                {
                    let mut guard = state.config.write().map_err(AppError::from)?;
                    if let Some(manager) = guard.get_manager_mut(app_type) {
                        if let Some(target) = manager.providers.get_mut(provider_id) {
                            let obj = target.settings_config.as_object_mut().ok_or_else(|| {
                                AppError::Config(format!(
                                    "供应商 {} 的 Codex 配置必须是 JSON 对象",
                                    provider_id
                                ))
                            })?;
                            obj.insert("auth".to_string(), auth.clone());
                            obj.insert("config".to_string(), Value::String(cfg_text.clone()));
                        }
                    }
                }
                state.save()?;
            }
        }
        Ok(())
    }

    fn capture_live_snapshot(app_type: &AppType) -> Result<LiveSnapshot, AppError> {
        match app_type {
            AppType::Claude => {
                let path = get_claude_settings_path();
                let settings = if path.exists() {
                    Some(read_json_file::<Value>(&path)?)
                } else {
                    None
                };
                Ok(LiveSnapshot::Claude { settings })
            }
            AppType::Codex => {
                let auth_path = get_codex_auth_path();
                let config_path = get_codex_config_path();
                let auth = if auth_path.exists() {
                    Some(read_json_file::<Value>(&auth_path)?)
                } else {
                    None
                };
                let config = if config_path.exists() {
                    Some(
                        std::fs::read_to_string(&config_path)
                            .map_err(|e| AppError::io(&config_path, e))?,
                    )
                } else {
                    None
                };
                Ok(LiveSnapshot::Codex { auth, config })
            }
        }
    }

    /// 列出指定应用下的所有供应商
    pub fn list(
        state: &AppState,
        app_type: AppType,
    ) -> Result<HashMap<String, Provider>, AppError> {
        let config = state.config.read().map_err(AppError::from)?;
        let manager = config
            .get_manager(&app_type)
            .ok_or_else(|| Self::app_not_found(&app_type))?;
        Ok(manager.get_all_providers().clone())
    }

    /// 获取当前供应商 ID
    pub fn current(state: &AppState, app_type: AppType) -> Result<String, AppError> {
        let config = state.config.read().map_err(AppError::from)?;
        let manager = config
            .get_manager(&app_type)
            .ok_or_else(|| Self::app_not_found(&app_type))?;
        Ok(manager.current.clone())
    }

    /// 新增供应商
    pub fn add(state: &AppState, app_type: AppType, provider: Provider) -> Result<bool, AppError> {
        let mut provider = provider;
        // 归一化 Claude 模型键
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;

        let app_type_clone = app_type.clone();
        let provider_clone = provider.clone();

        Self::run_transaction(state, move |config| {
            config.ensure_app(&app_type_clone);
            let manager = config
                .get_manager_mut(&app_type_clone)
                .ok_or_else(|| Self::app_not_found(&app_type_clone))?;

            let is_current = manager.current == provider_clone.id;
            manager
                .providers
                .insert(provider_clone.id.clone(), provider_clone.clone());

            let action = if is_current {
                let backup = Self::capture_live_snapshot(&app_type_clone)?;
                Some(PostCommitAction {
                    app_type: app_type_clone.clone(),
                    provider: provider_clone.clone(),
                    backup,
                    sync_mcp: false,
                    refresh_snapshot: false,
                })
            } else {
                None
            };

            Ok((true, action))
        })
    }

    /// 更新供应商
    pub fn update(
        state: &AppState,
        app_type: AppType,
        provider: Provider,
    ) -> Result<bool, AppError> {
        let mut provider = provider;
        // 归一化 Claude 模型键
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        let provider_id = provider.id.clone();
        let app_type_clone = app_type.clone();
        let provider_clone = provider.clone();

        Self::run_transaction(state, move |config| {
            let manager = config
                .get_manager_mut(&app_type_clone)
                .ok_or_else(|| Self::app_not_found(&app_type_clone))?;

            if !manager.providers.contains_key(&provider_id) {
                return Err(AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {}", provider_id),
                    format!("Provider not found: {}", provider_id),
                ));
            }

            let is_current = manager.current == provider_id;
            let merged = if let Some(existing) = manager.providers.get(&provider_id) {
                let mut updated = provider_clone.clone();
                match (existing.meta.as_ref(), updated.meta.take()) {
                    // 前端未提供 meta，表示不修改，沿用旧值
                    (Some(old_meta), None) => {
                        updated.meta = Some(old_meta.clone());
                    }
                    (None, None) => {
                        updated.meta = None;
                    }
                    // 前端提供的 meta 视为权威，直接覆盖（其中 custom_endpoints 允许是空，表示删除所有自定义端点）
                    (_old, Some(new_meta)) => {
                        updated.meta = Some(new_meta);
                    }
                }
                updated
            } else {
                provider_clone.clone()
            };

            manager.providers.insert(provider_id.clone(), merged);

            let action = if is_current {
                let backup = Self::capture_live_snapshot(&app_type_clone)?;
                Some(PostCommitAction {
                    app_type: app_type_clone.clone(),
                    provider: provider_clone.clone(),
                    backup,
                    sync_mcp: false,
                    refresh_snapshot: false,
                })
            } else {
                None
            };

            Ok((true, action))
        })
    }

    /// 导入当前 live 配置为默认供应商
    pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<(), AppError> {
        {
            let config = state.config.read().map_err(AppError::from)?;
            if let Some(manager) = config.get_manager(&app_type) {
                if !manager.get_all_providers().is_empty() {
                    return Ok(());
                }
            }
        }

        let settings_config = match app_type {
            AppType::Codex => {
                let auth_path = get_codex_auth_path();
                if !auth_path.exists() {
                    return Err(AppError::localized(
                        "codex.live.missing",
                        "Codex 配置文件不存在",
                        "Codex configuration file is missing",
                    ));
                }
                let auth: Value = read_json_file(&auth_path)?;
                let config_str = crate::codex_config::read_and_validate_codex_config_text()?;
                json!({ "auth": auth, "config": config_str })
            }
            AppType::Claude => {
                let settings_path = get_claude_settings_path();
                if !settings_path.exists() {
                    return Err(AppError::localized(
                        "claude.live.missing",
                        "Claude Code 配置文件不存在",
                        "Claude settings file is missing",
                    ));
                }
                let mut v = read_json_file::<Value>(&settings_path)?;
                let _ = Self::normalize_claude_models_in_value(&mut v);
                v
            }
        };

        let mut provider = Provider::with_id(
            "default".to_string(),
            "default".to_string(),
            settings_config,
            None,
        );
        provider.category = Some("custom".to_string());

        {
            let mut config = state.config.write().map_err(AppError::from)?;
            let manager = config
                .get_manager_mut(&app_type)
                .ok_or_else(|| Self::app_not_found(&app_type))?;
            manager
                .providers
                .insert(provider.id.clone(), provider.clone());
            manager.current = provider.id.clone();
        }

        state.save()?;
        Ok(())
    }

    /// 读取当前 live 配置
    pub fn read_live_settings(app_type: AppType) -> Result<Value, AppError> {
        match app_type {
            AppType::Codex => {
                let auth_path = get_codex_auth_path();
                if !auth_path.exists() {
                    return Err(AppError::localized(
                        "codex.auth.missing",
                        "Codex 配置文件不存在：缺少 auth.json",
                        "Codex configuration missing: auth.json not found",
                    ));
                }
                let auth: Value = read_json_file(&auth_path)?;
                let cfg_text = crate::codex_config::read_and_validate_codex_config_text()?;
                Ok(json!({ "auth": auth, "config": cfg_text }))
            }
            AppType::Claude => {
                let path = get_claude_settings_path();
                if !path.exists() {
                    return Err(AppError::localized(
                        "claude.live.missing",
                        "Claude Code 配置文件不存在",
                        "Claude settings file is missing",
                    ));
                }
                read_json_file(&path)
            }
        }
    }

    /// 获取自定义端点列表
    pub fn get_custom_endpoints(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
    ) -> Result<Vec<CustomEndpoint>, AppError> {
        let cfg = state.config.read().map_err(AppError::from)?;
        let manager = cfg
            .get_manager(&app_type)
            .ok_or_else(|| Self::app_not_found(&app_type))?;

        let Some(provider) = manager.providers.get(provider_id) else {
            return Ok(vec![]);
        };
        let Some(meta) = provider.meta.as_ref() else {
            return Ok(vec![]);
        };
        if meta.custom_endpoints.is_empty() {
            return Ok(vec![]);
        }

        let mut result: Vec<_> = meta.custom_endpoints.values().cloned().collect();
        result.sort_by(|a, b| b.added_at.cmp(&a.added_at));
        Ok(result)
    }

    /// 新增自定义端点
    pub fn add_custom_endpoint(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        url: String,
    ) -> Result<(), AppError> {
        let normalized = url.trim().trim_end_matches('/').to_string();
        if normalized.is_empty() {
            return Err(AppError::localized(
                "provider.endpoint.url_required",
                "URL 不能为空",
                "URL cannot be empty",
            ));
        }

        {
            let mut cfg = state.config.write().map_err(AppError::from)?;
            let manager = cfg
                .get_manager_mut(&app_type)
                .ok_or_else(|| Self::app_not_found(&app_type))?;
            let provider = manager.providers.get_mut(provider_id).ok_or_else(|| {
                AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {}", provider_id),
                    format!("Provider not found: {}", provider_id),
                )
            })?;
            let meta = provider.meta.get_or_insert_with(ProviderMeta::default);

            let endpoint = CustomEndpoint {
                url: normalized.clone(),
                added_at: Self::now_millis(),
                last_used: None,
            };
            meta.custom_endpoints.insert(normalized, endpoint);
        }

        state.save()?;
        Ok(())
    }

    /// 删除自定义端点
    pub fn remove_custom_endpoint(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        url: String,
    ) -> Result<(), AppError> {
        let normalized = url.trim().trim_end_matches('/').to_string();

        {
            let mut cfg = state.config.write().map_err(AppError::from)?;
            if let Some(manager) = cfg.get_manager_mut(&app_type) {
                if let Some(provider) = manager.providers.get_mut(provider_id) {
                    if let Some(meta) = provider.meta.as_mut() {
                        meta.custom_endpoints.remove(&normalized);
                    }
                }
            }
        }

        state.save()?;
        Ok(())
    }

    /// 更新端点最后使用时间
    pub fn update_endpoint_last_used(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        url: String,
    ) -> Result<(), AppError> {
        let normalized = url.trim().trim_end_matches('/').to_string();

        {
            let mut cfg = state.config.write().map_err(AppError::from)?;
            if let Some(manager) = cfg.get_manager_mut(&app_type) {
                if let Some(provider) = manager.providers.get_mut(provider_id) {
                    if let Some(meta) = provider.meta.as_mut() {
                        if let Some(endpoint) = meta.custom_endpoints.get_mut(&normalized) {
                            endpoint.last_used = Some(Self::now_millis());
                        }
                    }
                }
            }
        }

        state.save()?;
        Ok(())
    }

    /// 更新供应商排序
    pub fn update_sort_order(
        state: &AppState,
        app_type: AppType,
        updates: Vec<ProviderSortUpdate>,
    ) -> Result<bool, AppError> {
        {
            let mut cfg = state.config.write().map_err(AppError::from)?;
            let manager = cfg
                .get_manager_mut(&app_type)
                .ok_or_else(|| Self::app_not_found(&app_type))?;

            for update in updates {
                if let Some(provider) = manager.providers.get_mut(&update.id) {
                    provider.sort_index = Some(update.sort_index);
                }
            }
        }

        state.save()?;
        Ok(true)
    }

    /// 执行用量脚本并格式化结果（私有辅助方法）
    async fn execute_and_format_usage_result(
        script_code: &str,
        api_key: &str,
        base_url: &str,
        timeout: u64,
        access_token: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<UsageResult, AppError> {
        match usage_script::execute_usage_script(
            script_code,
            api_key,
            base_url,
            timeout,
            access_token,
            user_id,
        )
        .await
        {
            Ok(data) => {
                let usage_list: Vec<UsageData> = if data.is_array() {
                    serde_json::from_value(data).map_err(|e| {
                        AppError::localized(
                            "usage_script.data_format_error",
                            format!("数据格式错误: {}", e),
                            format!("Data format error: {}", e),
                        )
                    })?
                } else {
                    let single: UsageData = serde_json::from_value(data).map_err(|e| {
                        AppError::localized(
                            "usage_script.data_format_error",
                            format!("数据格式错误: {}", e),
                            format!("Data format error: {}", e),
                        )
                    })?;
                    vec![single]
                };

                Ok(UsageResult {
                    success: true,
                    data: Some(usage_list),
                    error: None,
                })
            }
            Err(err) => {
                let lang = settings::get_settings()
                    .language
                    .unwrap_or_else(|| "zh".to_string());

                let msg = match err {
                    AppError::Localized { zh, en, .. } => {
                        if lang == "en" {
                            en
                        } else {
                            zh
                        }
                    }
                    other => other.to_string(),
                };

                Ok(UsageResult {
                    success: false,
                    data: None,
                    error: Some(msg),
                })
            }
        }
    }

    /// 查询供应商用量（使用已保存的脚本配置）
    pub async fn query_usage(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
    ) -> Result<UsageResult, AppError> {
        let (provider, script_code, timeout, access_token, user_id) = {
            let config = state.config.read().map_err(AppError::from)?;
            let manager = config
                .get_manager(&app_type)
                .ok_or_else(|| Self::app_not_found(&app_type))?;
            let provider = manager.providers.get(provider_id).cloned().ok_or_else(|| {
                AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {}", provider_id),
                    format!("Provider not found: {}", provider_id),
                )
            })?;
            let (script_code, timeout, access_token, user_id) = {
                let usage_script = provider
                    .meta
                    .as_ref()
                    .and_then(|m| m.usage_script.as_ref())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.usage.script.missing",
                            "未配置用量查询脚本",
                            "Usage script is not configured",
                        )
                    })?;
                if !usage_script.enabled {
                    return Err(AppError::localized(
                        "provider.usage.disabled",
                        "用量查询未启用",
                        "Usage query is disabled",
                    ));
                }
                (
                    usage_script.code.clone(),
                    usage_script.timeout.unwrap_or(10),
                    usage_script.access_token.clone(),
                    usage_script.user_id.clone(),
                )
            };

            (provider, script_code, timeout, access_token, user_id)
        };

        let (api_key, base_url) = Self::extract_credentials(&provider, &app_type)?;

        Self::execute_and_format_usage_result(
            &script_code,
            &api_key,
            &base_url,
            timeout,
            access_token.as_deref(),
            user_id.as_deref(),
        )
        .await
    }

    /// 测试用量脚本（使用临时脚本内容，不保存）
    pub async fn test_usage_script(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        script_code: &str,
        timeout: u64,
        access_token: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<UsageResult, AppError> {
        // 获取 provider 的 API 凭证
        let provider = {
            let config = state.config.read().map_err(AppError::from)?;
            let manager = config
                .get_manager(&app_type)
                .ok_or_else(|| Self::app_not_found(&app_type))?;
            manager.providers.get(provider_id).cloned().ok_or_else(|| {
                AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {}", provider_id),
                    format!("Provider not found: {}", provider_id),
                )
            })?
        };

        let (api_key, base_url) = Self::extract_credentials(&provider, &app_type)?;

        Self::execute_and_format_usage_result(
            script_code,
            &api_key,
            &base_url,
            timeout,
            access_token,
            user_id,
        )
        .await
    }

    /// 切换指定应用的供应商
    pub fn switch(state: &AppState, app_type: AppType, provider_id: &str) -> Result<(), AppError> {
        let app_type_clone = app_type.clone();
        let provider_id_owned = provider_id.to_string();

        Self::run_transaction(state, move |config| {
            let backup = Self::capture_live_snapshot(&app_type_clone)?;
            let provider = match app_type_clone {
                AppType::Codex => Self::prepare_switch_codex(config, &provider_id_owned)?,
                AppType::Claude => Self::prepare_switch_claude(config, &provider_id_owned)?,
            };

            let action = PostCommitAction {
                app_type: app_type_clone.clone(),
                provider,
                backup,
                sync_mcp: matches!(app_type_clone, AppType::Codex),
                refresh_snapshot: true,
            };

            Ok(((), Some(action)))
        })
    }

    fn prepare_switch_codex(
        config: &mut MultiAppConfig,
        provider_id: &str,
    ) -> Result<Provider, AppError> {
        let provider = config
            .get_manager(&AppType::Codex)
            .ok_or_else(|| Self::app_not_found(&AppType::Codex))?
            .providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| {
                AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {}", provider_id),
                    format!("Provider not found: {}", provider_id),
                )
            })?;

        Self::backfill_codex_current(config, provider_id)?;

        if let Some(manager) = config.get_manager_mut(&AppType::Codex) {
            manager.current = provider_id.to_string();
        }

        Ok(provider)
    }

    fn backfill_codex_current(
        config: &mut MultiAppConfig,
        next_provider: &str,
    ) -> Result<(), AppError> {
        let current_id = config
            .get_manager(&AppType::Codex)
            .map(|m| m.current.clone())
            .unwrap_or_default();

        if current_id.is_empty() || current_id == next_provider {
            return Ok(());
        }

        let auth_path = get_codex_auth_path();
        if !auth_path.exists() {
            return Ok(());
        }

        let auth: Value = read_json_file(&auth_path)?;
        let config_path = get_codex_config_path();
        let config_text = if config_path.exists() {
            std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?
        } else {
            String::new()
        };

        let live = json!({
            "auth": auth,
            "config": config_text,
        });

        if let Some(manager) = config.get_manager_mut(&AppType::Codex) {
            if let Some(current) = manager.providers.get_mut(&current_id) {
                current.settings_config = live;
            }
        }

        Ok(())
    }

    fn write_codex_live(provider: &Provider) -> Result<(), AppError> {
        let settings = provider
            .settings_config
            .as_object()
            .ok_or_else(|| AppError::Config("Codex 配置必须是 JSON 对象".into()))?;
        let auth = settings
            .get("auth")
            .ok_or_else(|| AppError::Config(format!("供应商 {} 缺少 auth 配置", provider.id)))?;
        if !auth.is_object() {
            return Err(AppError::Config(format!(
                "供应商 {} 的 auth 必须是对象",
                provider.id
            )));
        }
        let cfg_text = settings.get("config").and_then(Value::as_str);

        write_codex_live_atomic(auth, cfg_text)?;
        Ok(())
    }

    fn prepare_switch_claude(
        config: &mut MultiAppConfig,
        provider_id: &str,
    ) -> Result<Provider, AppError> {
        let provider = config
            .get_manager(&AppType::Claude)
            .ok_or_else(|| Self::app_not_found(&AppType::Claude))?
            .providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| {
                AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {}", provider_id),
                    format!("Provider not found: {}", provider_id),
                )
            })?;

        Self::backfill_claude_current(config, provider_id)?;

        if let Some(manager) = config.get_manager_mut(&AppType::Claude) {
            manager.current = provider_id.to_string();
        }

        Ok(provider)
    }

    fn backfill_claude_current(
        config: &mut MultiAppConfig,
        next_provider: &str,
    ) -> Result<(), AppError> {
        let settings_path = get_claude_settings_path();
        if !settings_path.exists() {
            return Ok(());
        }

        let current_id = config
            .get_manager(&AppType::Claude)
            .map(|m| m.current.clone())
            .unwrap_or_default();
        if current_id.is_empty() || current_id == next_provider {
            return Ok(());
        }

        let mut live = read_json_file::<Value>(&settings_path)?;
        let _ = Self::normalize_claude_models_in_value(&mut live);
        if let Some(manager) = config.get_manager_mut(&AppType::Claude) {
            if let Some(current) = manager.providers.get_mut(&current_id) {
                current.settings_config = live;
            }
        }

        Ok(())
    }

    fn write_claude_live(provider: &Provider) -> Result<(), AppError> {
        let settings_path = get_claude_settings_path();
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        // 归一化后再写入
        let mut content = provider.settings_config.clone();
        let _ = Self::normalize_claude_models_in_value(&mut content);
        write_json_file(&settings_path, &content)?;
        Ok(())
    }

    fn write_live_snapshot(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
        match app_type {
            AppType::Codex => Self::write_codex_live(provider),
            AppType::Claude => Self::write_claude_live(provider),
        }
    }

    fn validate_provider_settings(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
        match app_type {
            AppType::Claude => {
                if !provider.settings_config.is_object() {
                    return Err(AppError::localized(
                        "provider.claude.settings.not_object",
                        "Claude 配置必须是 JSON 对象",
                        "Claude configuration must be a JSON object",
                    ));
                }
            }
            AppType::Codex => {
                let settings = provider.settings_config.as_object().ok_or_else(|| {
                    AppError::localized(
                        "provider.codex.settings.not_object",
                        "Codex 配置必须是 JSON 对象",
                        "Codex configuration must be a JSON object",
                    )
                })?;

                let auth = settings.get("auth").ok_or_else(|| {
                    AppError::localized(
                        "provider.codex.auth.missing",
                        format!("供应商 {} 缺少 auth 配置", provider.id),
                        format!("Provider {} is missing auth configuration", provider.id),
                    )
                })?;
                if !auth.is_object() {
                    return Err(AppError::localized(
                        "provider.codex.auth.not_object",
                        format!("供应商 {} 的 auth 配置必须是 JSON 对象", provider.id),
                        format!(
                            "Provider {} auth configuration must be a JSON object",
                            provider.id
                        ),
                    ));
                }

                if let Some(config_value) = settings.get("config") {
                    if !(config_value.is_string() || config_value.is_null()) {
                        return Err(AppError::localized(
                            "provider.codex.config.invalid_type",
                            "Codex config 字段必须是字符串",
                            "Codex config field must be a string",
                        ));
                    }
                    if let Some(cfg_text) = config_value.as_str() {
                        crate::codex_config::validate_config_toml(cfg_text)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn extract_credentials(
        provider: &Provider,
        app_type: &AppType,
    ) -> Result<(String, String), AppError> {
        match app_type {
            AppType::Claude => {
                let env = provider
                    .settings_config
                    .get("env")
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.claude.env.missing",
                            "配置格式错误: 缺少 env",
                            "Invalid configuration: missing env section",
                        )
                    })?;

                let api_key = env
                    .get("ANTHROPIC_AUTH_TOKEN")
                    .or_else(|| env.get("ANTHROPIC_API_KEY"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.claude.api_key.missing",
                            "缺少 API Key",
                            "API key is missing",
                        )
                    })?
                    .to_string();

                let base_url = env
                    .get("ANTHROPIC_BASE_URL")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.claude.base_url.missing",
                            "缺少 ANTHROPIC_BASE_URL 配置",
                            "Missing ANTHROPIC_BASE_URL configuration",
                        )
                    })?
                    .to_string();

                Ok((api_key, base_url))
            }
            AppType::Codex => {
                let auth = provider
                    .settings_config
                    .get("auth")
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.codex.auth.missing",
                            "配置格式错误: 缺少 auth",
                            "Invalid configuration: missing auth section",
                        )
                    })?;

                let api_key = auth
                    .get("OPENAI_API_KEY")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.codex.api_key.missing",
                            "缺少 API Key",
                            "API key is missing",
                        )
                    })?
                    .to_string();

                let config_toml = provider
                    .settings_config
                    .get("config")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let base_url = if config_toml.contains("base_url") {
                    let re = Regex::new(r#"base_url\s*=\s*["']([^"']+)["']"#).map_err(|e| {
                        AppError::localized(
                            "provider.regex_init_failed",
                            format!("正则初始化失败: {}", e),
                            format!("Failed to initialize regex: {}", e),
                        )
                    })?;
                    re.captures(config_toml)
                        .and_then(|caps| caps.get(1))
                        .map(|m| m.as_str().to_string())
                        .ok_or_else(|| {
                            AppError::localized(
                                "provider.codex.base_url.invalid",
                                "config.toml 中 base_url 格式错误",
                                "base_url in config.toml has invalid format",
                            )
                        })?
                } else {
                    return Err(AppError::localized(
                        "provider.codex.base_url.missing",
                        "config.toml 中缺少 base_url 配置",
                        "base_url is missing from config.toml",
                    ));
                };

                Ok((api_key, base_url))
            }
        }
    }

    fn app_not_found(app_type: &AppType) -> AppError {
        AppError::localized(
            "provider.app_not_found",
            format!("应用类型不存在: {:?}", app_type),
            format!("App type not found: {:?}", app_type),
        )
    }

    fn now_millis() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    pub fn delete(state: &AppState, app_type: AppType, provider_id: &str) -> Result<(), AppError> {
        let provider_snapshot = {
            let config = state.config.read().map_err(AppError::from)?;
            let manager = config
                .get_manager(&app_type)
                .ok_or_else(|| Self::app_not_found(&app_type))?;

            if manager.current == provider_id {
                return Err(AppError::localized(
                    "provider.delete.current",
                    "不能删除当前正在使用的供应商",
                    "Cannot delete the provider currently in use",
                ));
            }

            manager.providers.get(provider_id).cloned().ok_or_else(|| {
                AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {}", provider_id),
                    format!("Provider not found: {}", provider_id),
                )
            })?
        };

        match app_type {
            AppType::Codex => {
                crate::codex_config::delete_codex_provider_config(
                    provider_id,
                    &provider_snapshot.name,
                )?;
            }
            AppType::Claude => {
                // 兼容旧版本：历史上会在 Claude 目录内为每个供应商生成 settings-*.json 副本
                // 这里继续清理这些遗留文件，避免堆积过期配置。
                let by_name = get_provider_config_path(provider_id, Some(&provider_snapshot.name));
                let by_id = get_provider_config_path(provider_id, None);
                delete_file(&by_name)?;
                delete_file(&by_id)?;
            }
        }

        {
            let mut config = state.config.write().map_err(AppError::from)?;
            let manager = config
                .get_manager_mut(&app_type)
                .ok_or_else(|| Self::app_not_found(&app_type))?;

            if manager.current == provider_id {
                return Err(AppError::localized(
                    "provider.delete.current",
                    "不能删除当前正在使用的供应商",
                    "Cannot delete the provider currently in use",
                ));
            }

            manager.providers.remove(provider_id);
        }

        state.save()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderSortUpdate {
    pub id: String,
    #[serde(rename = "sortIndex")]
    pub sort_index: usize,
}
