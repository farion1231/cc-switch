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
    Gemini {
        env: Option<HashMap<String, String>>, // 新增
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
            LiveSnapshot::Gemini { env } => {
                // 新增
                use crate::gemini_config::{get_gemini_env_path, write_gemini_env_atomic};
                let path = get_gemini_env_path();
                if let Some(env_map) = env {
                    write_gemini_env_atomic(env_map)?;
                } else if path.exists() {
                    delete_file(&path)?;
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

/// Gemini 认证类型枚举
///
/// 用于优化性能，避免重复检测供应商类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeminiAuthType {
    /// PackyCode 供应商（使用 API Key）
    Packycode,
    /// Google 官方（使用 OAuth）
    GoogleOfficial,
    /// 通用 Gemini 供应商（使用 API Key）
    Generic,
}

impl ProviderService {
    // 认证类型常量
    const PACKYCODE_SECURITY_SELECTED_TYPE: &'static str = "gemini-api-key";
    const GOOGLE_OAUTH_SECURITY_SELECTED_TYPE: &'static str = "oauth-personal";

    // Partner Promotion Key 常量
    const PACKYCODE_PARTNER_KEY: &'static str = "packycode";
    const GOOGLE_OFFICIAL_PARTNER_KEY: &'static str = "google-official";

    // PackyCode 关键词常量
    const PACKYCODE_KEYWORDS: [&'static str; 3] = ["packycode", "packyapi", "packy"];

    /// 检测 Gemini 供应商的认证类型
    ///
    /// 一次性检测，避免在多个地方重复调用 `is_packycode_gemini` 和 `is_google_official_gemini`
    ///
    /// # 返回值
    ///
    /// - `GeminiAuthType::GoogleOfficial`: Google 官方，使用 OAuth
    /// - `GeminiAuthType::Packycode`: PackyCode 供应商，使用 API Key
    /// - `GeminiAuthType::Generic`: 其他通用供应商，使用 API Key
    fn detect_gemini_auth_type(provider: &Provider) -> GeminiAuthType {
        // 优先检查 partner_promotion_key（最可靠）
        if let Some(key) = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.partner_promotion_key.as_deref())
        {
            if key.eq_ignore_ascii_case(Self::GOOGLE_OFFICIAL_PARTNER_KEY) {
                return GeminiAuthType::GoogleOfficial;
            }
            if key.eq_ignore_ascii_case(Self::PACKYCODE_PARTNER_KEY) {
                return GeminiAuthType::Packycode;
            }
        }

        // 检查 Google 官方（名称匹配）
        let name_lower = provider.name.to_ascii_lowercase();
        if name_lower == "google" || name_lower.starts_with("google ") {
            return GeminiAuthType::GoogleOfficial;
        }

        // 检查 PackyCode 关键词
        if Self::contains_packycode_keyword(&provider.name) {
            return GeminiAuthType::Packycode;
        }

        if let Some(site) = provider.website_url.as_deref() {
            if Self::contains_packycode_keyword(site) {
                return GeminiAuthType::Packycode;
            }
        }

        if let Some(base_url) = provider
            .settings_config
            .pointer("/env/GOOGLE_GEMINI_BASE_URL")
            .and_then(|v| v.as_str())
        {
            if Self::contains_packycode_keyword(base_url) {
                return GeminiAuthType::Packycode;
            }
        }

        GeminiAuthType::Generic
    }

    /// 检查字符串是否包含 PackyCode 相关关键词（不区分大小写）
    ///
    /// 关键词列表：["packycode", "packyapi", "packy"]
    fn contains_packycode_keyword(value: &str) -> bool {
        let lower = value.to_ascii_lowercase();
        Self::PACKYCODE_KEYWORDS
            .iter()
            .any(|keyword| lower.contains(keyword))
    }

    /// 检测供应商是否为 PackyCode Gemini（使用 API Key 认证）
    ///
    /// PackyCode 是官方合作伙伴，需要特殊的安全配置。
    ///
    /// # 检测规则（优先级从高到低）
    ///
    /// 1. **Partner Promotion Key**（最可靠）:
    ///    - `provider.meta.partner_promotion_key == "packycode"`
    ///
    /// 2. **供应商名称**:
    ///    - 名称包含 "packycode"、"packyapi" 或 "packy"（不区分大小写）
    ///
    /// 3. **网站 URL**:
    ///    - `provider.website_url` 包含关键词
    ///
    /// 4. **Base URL**:
    ///    - `settings_config.env.GOOGLE_GEMINI_BASE_URL` 包含关键词
    ///
    /// # 为什么需要多重检测
    ///
    /// - 用户可能手动创建供应商，没有 `partner_promotion_key`
    /// - 从预设复制后可能修改了 meta 字段
    /// - 确保所有 PackyCode 供应商都能正确设置安全标志
    fn is_packycode_gemini(provider: &Provider) -> bool {
        // 策略 1: 检查 partner_promotion_key（最可靠）
        if provider
            .meta
            .as_ref()
            .and_then(|meta| meta.partner_promotion_key.as_deref())
            .is_some_and(|key| key.eq_ignore_ascii_case(Self::PACKYCODE_PARTNER_KEY))
        {
            return true;
        }

        // 策略 2: 检查供应商名称
        if Self::contains_packycode_keyword(&provider.name) {
            return true;
        }

        // 策略 3: 检查网站 URL
        if let Some(site) = provider.website_url.as_deref() {
            if Self::contains_packycode_keyword(site) {
                return true;
            }
        }

        // 策略 4: 检查 Base URL
        if let Some(base_url) = provider
            .settings_config
            .pointer("/env/GOOGLE_GEMINI_BASE_URL")
            .and_then(|v| v.as_str())
        {
            if Self::contains_packycode_keyword(base_url) {
                return true;
            }
        }

        false
    }

    /// 检测供应商是否为 Google 官方 Gemini（使用 OAuth 认证）
    ///
    /// Google 官方 Gemini 使用 OAuth 个人认证，不需要 API Key。
    ///
    /// # 检测规则（优先级从高到低）
    ///
    /// 1. **Partner Promotion Key**（最可靠）:
    ///    - `provider.meta.partner_promotion_key == "google-official"`
    ///
    /// 2. **供应商名称**:
    ///    - 名称完全等于 "google"（不区分大小写）
    ///    - 或名称以 "google " 开头（例如 "Google Official"）
    ///
    /// # OAuth vs API Key
    ///
    /// - **OAuth 模式**: `security.auth.selectedType = "oauth-personal"`
    ///   - 用户需要通过浏览器登录 Google 账号
    ///   - 不需要在 `.env` 文件中配置 API Key
    ///
    /// - **API Key 模式**: `security.auth.selectedType = "gemini-api-key"`
    ///   - 用于第三方中转服务（如 PackyCode）
    ///   - 需要在 `.env` 文件中配置 `GEMINI_API_KEY`
    fn is_google_official_gemini(provider: &Provider) -> bool {
        // 策略 1: 检查 partner_promotion_key（最可靠）
        if provider
            .meta
            .as_ref()
            .and_then(|meta| meta.partner_promotion_key.as_deref())
            .is_some_and(|key| key.eq_ignore_ascii_case(Self::GOOGLE_OFFICIAL_PARTNER_KEY))
        {
            return true;
        }

        // 策略 2: 检查名称匹配（备用方案）
        let name_lower = provider.name.to_ascii_lowercase();
        name_lower == "google" || name_lower.starts_with("google ")
    }

    /// 确保 PackyCode Gemini 供应商的安全标志正确设置
    ///
    /// PackyCode 是官方合作伙伴，使用 API Key 认证模式。
    ///
    /// # 写入两处 settings.json 的原因
    ///
    /// 1. **`~/.cc-switch/settings.json`** (应用级配置):
    ///    - CC-Switch 应用的全局设置
    ///    - 确保应用知道当前使用的认证类型
    ///    - 用于 UI 显示和其他应用逻辑
    ///
    /// 2. **`~/.gemini/settings.json`** (Gemini 客户端配置):
    ///    - Gemini CLI 客户端读取的配置文件
    ///    - 直接影响 Gemini 客户端的认证行为
    ///    - 确保 Gemini 使用正确的认证方式连接 API
    ///
    /// # 设置的值
    ///
    /// ```json
    /// {
    ///   "security": {
    ///     "auth": {
    ///       "selectedType": "gemini-api-key"
    ///     }
    ///   }
    /// }
    /// ```
    ///
    /// # 错误处理
    ///
    /// 如果供应商不是 PackyCode，函数立即返回 `Ok(())`，不做任何操作。
    pub(crate) fn ensure_packycode_security_flag(provider: &Provider) -> Result<(), AppError> {
        if !Self::is_packycode_gemini(provider) {
            return Ok(());
        }

        // 写入应用级别的 settings.json (~/.cc-switch/settings.json)
        settings::ensure_security_auth_selected_type(Self::PACKYCODE_SECURITY_SELECTED_TYPE)?;

        // 写入 Gemini 目录的 settings.json (~/.gemini/settings.json)
        use crate::gemini_config::write_packycode_settings;
        write_packycode_settings()?;

        Ok(())
    }

    /// 确保 Google 官方 Gemini 供应商的安全标志正确设置（OAuth 模式）
    ///
    /// Google 官方 Gemini 使用 OAuth 个人认证，不需要 API Key。
    ///
    /// # 写入两处 settings.json 的原因
    ///
    /// 同 `ensure_packycode_security_flag`，需要同时配置应用级和客户端级设置。
    ///
    /// # 设置的值
    ///
    /// ```json
    /// {
    ///   "security": {
    ///     "auth": {
    ///       "selectedType": "oauth-personal"
    ///     }
    ///   }
    /// }
    /// ```
    ///
    /// # OAuth 认证流程
    ///
    /// 1. 用户切换到 Google 官方供应商
    /// 2. CC-Switch 设置 `selectedType = "oauth-personal"`
    /// 3. 用户首次使用 Gemini CLI 时，会自动打开浏览器进行 OAuth 登录
    /// 4. 登录成功后，凭证保存在 Gemini 的 credential store 中
    /// 5. 后续请求自动使用保存的凭证
    ///
    /// # 错误处理
    ///
    /// 如果供应商不是 Google 官方，函数立即返回 `Ok(())`，不做任何操作。
    pub(crate) fn ensure_google_oauth_security_flag(provider: &Provider) -> Result<(), AppError> {
        if !Self::is_google_official_gemini(provider) {
            return Ok(());
        }

        // 写入应用级别的 settings.json (~/.cc-switch/settings.json)
        settings::ensure_security_auth_selected_type(Self::GOOGLE_OAUTH_SECURITY_SELECTED_TYPE)?;

        // 写入 Gemini 目录的 settings.json (~/.gemini/settings.json)
        use crate::gemini_config::write_google_oauth_settings;
        write_google_oauth_settings()?;

        Ok(())
    }

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
                    format!("保存配置失败: {save_err}；回滚失败: {rollback_err}"),
                    format!("Failed to save config: {save_err}; rollback failed: {rollback_err}"),
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
                        format!("后置操作失败: {err}；回滚失败: {rollback_err}"),
                        format!("Post-commit step failed: {err}; rollback failed: {rollback_err}"),
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
            // 使用 v3.7.0 统一的 MCP 同步机制，支持所有应用
            use crate::services::mcp::McpService;
            McpService::sync_all_enabled(state)?;
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
                                    "供应商 {provider_id} 的 Codex 配置必须是 JSON 对象"
                                ))
                            })?;
                            obj.insert("auth".to_string(), auth.clone());
                            obj.insert("config".to_string(), Value::String(cfg_text.clone()));
                        }
                    }
                }
                state.save()?;
            }
            AppType::Gemini => {
                use crate::gemini_config::{env_to_json, get_gemini_env_path, read_gemini_env};

                let env_path = get_gemini_env_path();
                if !env_path.exists() {
                    return Err(AppError::localized(
                        "gemini.live.missing",
                        "Gemini .env 文件不存在，无法刷新快照",
                        "Gemini .env file missing; cannot refresh snapshot",
                    ));
                }
                let env_map = read_gemini_env()?;
                let live_after = env_to_json(&env_map);

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
            AppType::Gemini => {
                // 新增
                use crate::gemini_config::{get_gemini_env_path, read_gemini_env};
                let path = get_gemini_env_path();
                let env = if path.exists() {
                    Some(read_gemini_env()?)
                } else {
                    None
                };
                Ok(LiveSnapshot::Gemini { env })
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
                    format!("供应商不存在: {provider_id}"),
                    format!("Provider not found: {provider_id}"),
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
            AppType::Gemini => {
                // 新增
                use crate::gemini_config::{env_to_json, get_gemini_env_path, read_gemini_env};

                let path = get_gemini_env_path();
                if !path.exists() {
                    return Err(AppError::localized(
                        "gemini.live.missing",
                        "Gemini 配置文件不存在",
                        "Gemini configuration file is missing",
                    ));
                }
                let env_map = read_gemini_env()?;
                env_to_json(&env_map)
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
            AppType::Gemini => {
                // 新增
                use crate::gemini_config::{env_to_json, get_gemini_env_path, read_gemini_env};

                let path = get_gemini_env_path();
                if !path.exists() {
                    return Err(AppError::localized(
                        "gemini.env.missing",
                        "Gemini .env 文件不存在",
                        "Gemini .env file not found",
                    ));
                }

                let env_map = read_gemini_env()?;
                Ok(env_to_json(&env_map))
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
                    format!("供应商不存在: {provider_id}"),
                    format!("Provider not found: {provider_id}"),
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
                            format!("数据格式错误: {e}"),
                            format!("Data format error: {e}"),
                        )
                    })?
                } else {
                    let single: UsageData = serde_json::from_value(data).map_err(|e| {
                        AppError::localized(
                            "usage_script.data_format_error",
                            format!("数据格式错误: {e}"),
                            format!("Data format error: {e}"),
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
        let (script_code, timeout, api_key, base_url, access_token, user_id) = {
            let config = state.config.read().map_err(AppError::from)?;
            let manager = config
                .get_manager(&app_type)
                .ok_or_else(|| Self::app_not_found(&app_type))?;
            let provider = manager.providers.get(provider_id).cloned().ok_or_else(|| {
                AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {provider_id}"),
                    format!("Provider not found: {provider_id}"),
                )
            })?;

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

            // 直接从 UsageScript 中获取凭证，不再从供应商配置提取
            (
                usage_script.code.clone(),
                usage_script.timeout.unwrap_or(10),
                usage_script.api_key.clone().unwrap_or_default(),
                usage_script.base_url.clone().unwrap_or_default(),
                usage_script.access_token.clone(),
                usage_script.user_id.clone(),
            )
        };

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
    #[allow(clippy::too_many_arguments)]
    pub async fn test_usage_script(
        _state: &AppState,
        _app_type: AppType,
        _provider_id: &str,
        script_code: &str,
        timeout: u64,
        api_key: Option<&str>,
        base_url: Option<&str>,
        access_token: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<UsageResult, AppError> {
        // 直接使用传入的凭证参数进行测试
        Self::execute_and_format_usage_result(
            script_code,
            api_key.unwrap_or(""),
            base_url.unwrap_or(""),
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
                AppType::Gemini => Self::prepare_switch_gemini(config, &provider_id_owned)?,
            };

            let action = PostCommitAction {
                app_type: app_type_clone.clone(),
                provider,
                backup,
                sync_mcp: true, // v3.7.0: 所有应用切换时都同步 MCP，防止配置丢失
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
                    format!("供应商不存在: {provider_id}"),
                    format!("Provider not found: {provider_id}"),
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
                    format!("供应商不存在: {provider_id}"),
                    format!("Provider not found: {provider_id}"),
                )
            })?;

        Self::backfill_claude_current(config, provider_id)?;

        if let Some(manager) = config.get_manager_mut(&AppType::Claude) {
            manager.current = provider_id.to_string();
        }

        Ok(provider)
    }

    fn prepare_switch_gemini(
        config: &mut MultiAppConfig,
        provider_id: &str,
    ) -> Result<Provider, AppError> {
        let provider = config
            .get_manager(&AppType::Gemini)
            .ok_or_else(|| Self::app_not_found(&AppType::Gemini))?
            .providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| {
                AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {provider_id}"),
                    format!("Provider not found: {provider_id}"),
                )
            })?;

        Self::backfill_gemini_current(config, provider_id)?;

        if let Some(manager) = config.get_manager_mut(&AppType::Gemini) {
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

    fn backfill_gemini_current(
        config: &mut MultiAppConfig,
        next_provider: &str,
    ) -> Result<(), AppError> {
        use crate::gemini_config::{env_to_json, get_gemini_env_path, read_gemini_env};

        let env_path = get_gemini_env_path();
        if !env_path.exists() {
            return Ok(());
        }

        let current_id = config
            .get_manager(&AppType::Gemini)
            .map(|m| m.current.clone())
            .unwrap_or_default();
        if current_id.is_empty() || current_id == next_provider {
            return Ok(());
        }

        let env_map = read_gemini_env()?;
        let live = env_to_json(&env_map);
        if let Some(manager) = config.get_manager_mut(&AppType::Gemini) {
            if let Some(current) = manager.providers.get_mut(&current_id) {
                current.settings_config = live;
            }
        }

        Ok(())
    }

    fn write_claude_live(provider: &Provider) -> Result<(), AppError> {
        let settings_path = get_claude_settings_path();
        let mut content = provider.settings_config.clone();
        let _ = Self::normalize_claude_models_in_value(&mut content);
        write_json_file(&settings_path, &content)?;
        Ok(())
    }

    fn write_gemini_live(provider: &Provider) -> Result<(), AppError> {
        use crate::gemini_config::{
            json_to_env, validate_gemini_settings_strict, write_gemini_env_atomic,
        };

        // 一次性检测认证类型，避免重复检测
        let auth_type = Self::detect_gemini_auth_type(provider);

        match auth_type {
            GeminiAuthType::GoogleOfficial => {
                // Google 官方使用 OAuth，清空 env
                let empty_env = std::collections::HashMap::new();
                write_gemini_env_atomic(&empty_env)?;
                Self::ensure_google_oauth_security_flag(provider)?;
            }
            GeminiAuthType::Packycode => {
                // PackyCode 供应商，使用 API Key（切换时严格验证）
                validate_gemini_settings_strict(&provider.settings_config)?;
                let env_map = json_to_env(&provider.settings_config)?;
                write_gemini_env_atomic(&env_map)?;
                Self::ensure_packycode_security_flag(provider)?;
            }
            GeminiAuthType::Generic => {
                // 通用供应商，使用 API Key（切换时严格验证）
                validate_gemini_settings_strict(&provider.settings_config)?;
                let env_map = json_to_env(&provider.settings_config)?;
                write_gemini_env_atomic(&env_map)?;
            }
        }

        Ok(())
    }

    fn write_live_snapshot(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
        match app_type {
            AppType::Codex => Self::write_codex_live(provider),
            AppType::Claude => Self::write_claude_live(provider),
            AppType::Gemini => Self::write_gemini_live(provider), // 新增
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
            AppType::Gemini => {
                // 新增
                use crate::gemini_config::validate_gemini_settings;
                validate_gemini_settings(&provider.settings_config)?
            }
        }

        // 🔧 验证并清理 UsageScript 配置（所有应用类型通用）
        if let Some(meta) = &provider.meta {
            if let Some(usage_script) = &meta.usage_script {
                Self::validate_usage_script(usage_script)?;
            }
        }

        Ok(())
    }

    /// 验证 UsageScript 配置（边界检查）
    fn validate_usage_script(script: &crate::provider::UsageScript) -> Result<(), AppError> {
        // 验证自动查询间隔 (0-1440 分钟，即最大24小时)
        if let Some(interval) = script.auto_query_interval {
            if interval > 1440 {
                return Err(AppError::localized(
                    "usage_script.interval_too_large",
                    format!(
                        "自动查询间隔不能超过 1440 分钟（24小时），当前值: {interval}"
                    ),
                    format!(
                        "Auto query interval cannot exceed 1440 minutes (24 hours), current: {interval}"
                    ),
                ));
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
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
                            format!("正则初始化失败: {e}"),
                            format!("Failed to initialize regex: {e}"),
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
            AppType::Gemini => {
                // 新增
                use crate::gemini_config::json_to_env;

                let env_map = json_to_env(&provider.settings_config)?;

                let api_key = env_map.get("GEMINI_API_KEY").cloned().ok_or_else(|| {
                    AppError::localized(
                        "gemini.missing_api_key",
                        "缺少 GEMINI_API_KEY",
                        "Missing GEMINI_API_KEY",
                    )
                })?;

                let base_url = env_map
                    .get("GOOGLE_GEMINI_BASE_URL")
                    .cloned()
                    .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());

                Ok((api_key, base_url))
            }
        }
    }

    fn app_not_found(app_type: &AppType) -> AppError {
        AppError::localized(
            "provider.app_not_found",
            format!("应用类型不存在: {app_type:?}"),
            format!("App type not found: {app_type:?}"),
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
                    format!("供应商不存在: {provider_id}"),
                    format!("Provider not found: {provider_id}"),
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
            AppType::Gemini => {
                // Gemini 使用单一的 .env 文件，不需要删除单独的供应商配置文件
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
