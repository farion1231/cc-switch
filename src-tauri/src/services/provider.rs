use indexmap::IndexMap;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app_config::AppType;
use crate::codex_config::{get_codex_auth_path, get_codex_config_path, write_codex_live_atomic};
use crate::config::{
    delete_file, get_claude_settings_path, read_json_file, write_json_file, write_text_file,
};
use crate::error::AppError;
use crate::provider::{Provider, UsageData, UsageResult};
use crate::services::mcp::McpService;
use crate::settings::{self, CustomEndpoint};
use crate::store::AppState;
use crate::usage_script;

/// 供应商相关业务逻辑
pub struct ProviderService;

#[derive(Clone)]
#[allow(dead_code)]
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
        config: Option<Value>,                // 新增：settings.json 内容
    },
}

#[derive(Clone)]
#[allow(dead_code)]
struct PostCommitAction {
    app_type: AppType,
    provider: Provider,
    backup: LiveSnapshot,
    sync_mcp: bool,
    refresh_snapshot: bool,
}

impl LiveSnapshot {
    #[allow(dead_code)]
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
            LiveSnapshot::Gemini { env, .. } => {
                // 新增
                use crate::gemini_config::{
                    get_gemini_env_path, get_gemini_settings_path, write_gemini_env_atomic,
                };
                let path = get_gemini_env_path();
                if let Some(env_map) = env {
                    write_gemini_env_atomic(env_map)?;
                } else if path.exists() {
                    delete_file(&path)?;
                }

                let settings_path = get_gemini_settings_path();
                match self {
                    LiveSnapshot::Gemini {
                        config: Some(cfg), ..
                    } => {
                        write_json_file(&settings_path, cfg)?;
                    }
                    LiveSnapshot::Gemini { config: None, .. } if settings_path.exists() => {
                        delete_file(&settings_path)?;
                    }
                    _ => {}
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
        let env = match settings.get_mut("env").and_then(|v| v.as_object_mut()) {
            Some(obj) => obj,
            None => return changed,
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

    fn write_live_snapshot(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
        match app_type {
            AppType::Claude => {
                let path = get_claude_settings_path();
                write_json_file(&path, &provider.settings_config)?;
            }
            AppType::Codex => {
                let obj = provider.settings_config.as_object().ok_or_else(|| {
                    AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string())
                })?;
                let auth = obj.get("auth").ok_or_else(|| {
                    AppError::Config("Codex 供应商配置缺少 'auth' 字段".to_string())
                })?;
                let config_str = obj.get("config").and_then(|v| v.as_str()).ok_or_else(|| {
                    AppError::Config("Codex 供应商配置缺少 'config' 字段或不是字符串".to_string())
                })?;

                let auth_path = get_codex_auth_path();
                write_json_file(&auth_path, auth)?;
                let config_path = get_codex_config_path();
                std::fs::write(&config_path, config_str)
                    .map_err(|e| AppError::io(&config_path, e))?;
            }
            AppType::Gemini => {
                use crate::gemini_config::{
                    get_gemini_settings_path, json_to_env, write_gemini_env_atomic,
                };

                // Extract env and config from provider settings
                let env_value = provider.settings_config.get("env");
                let config_value = provider.settings_config.get("config");

                // Write env file
                if let Some(env) = env_value {
                    let env_map = json_to_env(env)?;
                    write_gemini_env_atomic(&env_map)?;
                }

                // Write settings file
                if let Some(config) = config_value {
                    let settings_path = get_gemini_settings_path();
                    write_json_file(&settings_path, config)?;
                }
            }
        }
        Ok(())
    }

    /// 将数据库中的当前供应商同步到对应 live 配置
    pub fn sync_current_from_db(state: &AppState) -> Result<(), AppError> {
        for app_type in [AppType::Claude, AppType::Codex, AppType::Gemini] {
            let current_id = match state.db.get_current_provider(app_type.as_str())? {
                Some(id) => id,
                None => continue,
            };
            let providers = state.db.get_all_providers(app_type.as_str())?;
            if let Some(provider) = providers.get(&current_id) {
                Self::write_live_snapshot(&app_type, provider)?;
            } else {
                log::warn!(
                    "无法同步 live 配置: 当前供应商 {} ({}) 未找到",
                    current_id,
                    app_type.as_str()
                );
            }
        }

        // MCP 同步
        McpService::sync_all_enabled(state)?;
        Ok(())
    }

    /// 列出指定应用下的所有供应商
    pub fn list(
        state: &AppState,
        app_type: AppType,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        state.db.get_all_providers(app_type.as_str())
    }

    /// 获取当前供应商 ID
    pub fn current(state: &AppState, app_type: AppType) -> Result<String, AppError> {
        state
            .db
            .get_current_provider(app_type.as_str())
            .map(|opt| opt.unwrap_or_default())
    }

    /// 新增供应商
    pub fn add(state: &AppState, app_type: AppType, provider: Provider) -> Result<bool, AppError> {
        let mut provider = provider;
        // 归一化 Claude 模型键
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;

        // 保存到数据库
        state.db.save_provider(app_type.as_str(), &provider)?;

        // 检查是否需要同步（如果是当前供应商，或者没有当前供应商）
        let current = state.db.get_current_provider(app_type.as_str())?;
        if current.is_none() {
            // 如果没有当前供应商，设为当前并同步
            state
                .db
                .set_current_provider(app_type.as_str(), &provider.id)?;
            Self::write_live_snapshot(&app_type, &provider)?;
        }

        Ok(true)
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

        // 检查是否为当前供应商
        let current_id = state.db.get_current_provider(app_type.as_str())?;
        let is_current = current_id.as_deref() == Some(provider.id.as_str());

        // 保存到数据库
        state.db.save_provider(app_type.as_str(), &provider)?;

        if is_current {
            Self::write_live_snapshot(&app_type, &provider)?;
            // Sync MCP
            use crate::services::mcp::McpService;
            McpService::sync_all_enabled(state)?;
        }

        Ok(true)
    }

    /// 导入当前 live 配置为默认供应商
    pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<(), AppError> {
        {
            let providers = state.db.get_all_providers(app_type.as_str())?;
            if !providers.is_empty() {
                return Ok(());
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
                use crate::gemini_config::{
                    env_to_json, get_gemini_env_path, get_gemini_settings_path, read_gemini_env,
                };

                // 读取 .env 文件（环境变量）
                let env_path = get_gemini_env_path();
                if !env_path.exists() {
                    return Err(AppError::localized(
                        "gemini.live.missing",
                        "Gemini 配置文件不存在",
                        "Gemini configuration file is missing",
                    ));
                }

                let env_map = read_gemini_env()?;
                let env_json = env_to_json(&env_map);
                let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));

                // 读取 settings.json 文件（MCP 配置等）
                let settings_path = get_gemini_settings_path();
                let config_obj = if settings_path.exists() {
                    read_json_file(&settings_path)?
                } else {
                    json!({})
                };

                // 返回完整结构：{ "env": {...}, "config": {...} }
                json!({
                    "env": env_obj,
                    "config": config_obj
                })
            }
        };

        let mut provider = Provider::with_id(
            "default".to_string(),
            "default".to_string(),
            settings_config,
            None,
        );
        provider.category = Some("custom".to_string());

        state.db.save_provider(app_type.as_str(), &provider)?;
        state
            .db
            .set_current_provider(app_type.as_str(), &provider.id)?;

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
                use crate::gemini_config::{
                    env_to_json, get_gemini_env_path, get_gemini_settings_path, read_gemini_env,
                };

                // 读取 .env 文件（环境变量）
                let env_path = get_gemini_env_path();
                if !env_path.exists() {
                    return Err(AppError::localized(
                        "gemini.env.missing",
                        "Gemini .env 文件不存在",
                        "Gemini .env file not found",
                    ));
                }

                let env_map = read_gemini_env()?;
                let env_json = env_to_json(&env_map);
                let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));

                // 读取 settings.json 文件（MCP 配置等）
                let settings_path = get_gemini_settings_path();
                let config_obj = if settings_path.exists() {
                    read_json_file(&settings_path)?
                } else {
                    json!({})
                };

                // 返回完整结构：{ "env": {...}, "config": {...} }
                Ok(json!({
                    "env": env_obj,
                    "config": config_obj
                }))
            }
        }
    }

    /// 获取自定义端点列表
    pub fn get_custom_endpoints(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
    ) -> Result<Vec<CustomEndpoint>, AppError> {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let Some(provider) = providers.get(provider_id) else {
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

        state
            .db
            .add_custom_endpoint(app_type.as_str(), provider_id, &normalized)?;
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
        state
            .db
            .remove_custom_endpoint(app_type.as_str(), provider_id, &normalized)?;
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

        // Get provider, update last_used, save back
        let mut providers = state.db.get_all_providers(app_type.as_str())?;
        if let Some(provider) = providers.get_mut(provider_id) {
            if let Some(meta) = provider.meta.as_mut() {
                if let Some(endpoint) = meta.custom_endpoints.get_mut(&normalized) {
                    endpoint.last_used = Some(Self::now_millis());
                    state.db.save_provider(app_type.as_str(), provider)?;
                }
            }
        }
        Ok(())
    }

    /// 更新供应商排序
    pub fn update_sort_order(
        state: &AppState,
        app_type: AppType,
        updates: Vec<ProviderSortUpdate>,
    ) -> Result<bool, AppError> {
        let mut providers = state.db.get_all_providers(app_type.as_str())?;

        for update in updates {
            if let Some(provider) = providers.get_mut(&update.id) {
                provider.sort_index = Some(update.sort_index);
                state.db.save_provider(app_type.as_str(), provider)?;
            }
        }

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
            let providers = state.db.get_all_providers(app_type.as_str())?;
            let provider = providers.get(provider_id).ok_or_else(|| {
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    fn write_claude_live(provider: &Provider) -> Result<(), AppError> {
        let settings_path = get_claude_settings_path();
        let mut content = provider.settings_config.clone();
        let _ = Self::normalize_claude_models_in_value(&mut content);
        write_json_file(&settings_path, &content)?;
        Ok(())
    }

    pub(crate) fn write_gemini_live(provider: &Provider) -> Result<(), AppError> {
        use crate::gemini_config::{
            get_gemini_settings_path, json_to_env, validate_gemini_settings_strict,
            write_gemini_env_atomic,
        };

        // 一次性检测认证类型，避免重复检测
        let auth_type = Self::detect_gemini_auth_type(provider);

        let mut env_map = json_to_env(&provider.settings_config)?;

        // 准备要写入 ~/.gemini/settings.json 的配置（缺省时保留现有文件内容）
        let mut config_to_write = if let Some(config_value) = provider.settings_config.get("config")
        {
            if config_value.is_null() {
                Some(json!({}))
            } else if config_value.is_object() {
                Some(config_value.clone())
            } else {
                return Err(AppError::localized(
                    "gemini.validation.invalid_config",
                    "Gemini 配置格式错误: config 必须是对象或 null",
                    "Gemini config invalid: config must be an object or null",
                ));
            }
        } else {
            None
        };

        if config_to_write.is_none() {
            let settings_path = get_gemini_settings_path();
            if settings_path.exists() {
                config_to_write = Some(read_json_file(&settings_path)?);
            }
        }

        match auth_type {
            GeminiAuthType::GoogleOfficial => {
                // Google 官方使用 OAuth，清空 env
                env_map.clear();
                write_gemini_env_atomic(&env_map)?;
            }
            GeminiAuthType::Packycode => {
                // PackyCode 供应商，使用 API Key（切换时严格验证）
                validate_gemini_settings_strict(&provider.settings_config)?;
                write_gemini_env_atomic(&env_map)?;
            }
            GeminiAuthType::Generic => {
                // 通用供应商，使用 API Key（切换时严格验证）
                validate_gemini_settings_strict(&provider.settings_config)?;
                write_gemini_env_atomic(&env_map)?;
            }
        }

        if let Some(config_value) = config_to_write {
            let settings_path = get_gemini_settings_path();
            write_json_file(&settings_path, &config_value)?;
        }

        match auth_type {
            GeminiAuthType::GoogleOfficial => Self::ensure_google_oauth_security_flag(provider)?,
            GeminiAuthType::Packycode => Self::ensure_packycode_security_flag(provider)?,
            GeminiAuthType::Generic => {}
        }

        Ok(())
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

    #[allow(dead_code)]
    fn app_not_found(app_type: &AppType) -> AppError {
        AppError::localized(
            "provider.app_not_found",
            format!("应用类型不存在: {app_type:?}"),
            format!("App type not found: {app_type:?}"),
        )
    }

    /// 删除供应商
    pub fn delete(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        let current = state.db.get_current_provider(app_type.as_str())?;
        if current.as_deref() == Some(id) {
            return Err(AppError::Message(
                "无法删除当前正在使用的供应商".to_string(),
            ));
        }
        state.db.delete_provider(app_type.as_str(), id)
    }

    /// 切换供应商
    pub fn switch(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        // Check if provider exists
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers
            .get(id)
            .ok_or_else(|| AppError::Message(format!("供应商 {id} 不存在")))?;

        // Set current
        state.db.set_current_provider(app_type.as_str(), id)?;

        // Sync to live
        Self::write_live_snapshot(&app_type, provider)?;

        // Sync MCP
        use crate::services::mcp::McpService;
        McpService::sync_all_enabled(state)?;

        Ok(())
    }

    fn now_millis() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderSortUpdate {
    pub id: String,
    #[serde(rename = "sortIndex")]
    pub sort_index: usize,
}
