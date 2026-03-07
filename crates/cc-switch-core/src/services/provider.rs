//! Provider service - business logic for provider management.

mod endpoints;
mod gemini_auth;
mod live;

use indexmap::IndexMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{OpenCodeProviderConfig, Provider, UniversalProvider};
use crate::settings::CustomEndpoint;
use crate::store::AppState;

use super::McpService;
use live::{
    remove_openclaw_provider_from_live, remove_opencode_provider_from_live,
    sanitize_claude_settings_for_live, write_gemini_live,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderSortUpdate {
    pub id: String,
    #[serde(rename = "sortIndex")]
    pub sort_index: usize,
}

pub struct ProviderService;

impl ProviderService {
    fn normalize_provider_if_claude(app_type: &AppType, provider: &mut Provider) {
        if matches!(app_type, AppType::Claude) {
            let mut value = provider.settings_config.clone();
            if normalize_claude_models_in_value(&mut value) {
                provider.settings_config = value;
            }
        }
    }

    pub fn list(
        state: &AppState,
        app_type: AppType,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        state.db.get_all_providers(app_type.as_str())
    }

    pub fn current(state: &AppState, app_type: AppType) -> Result<String, AppError> {
        if app_type.is_additive_mode() {
            return Ok(String::new());
        }

        crate::settings::get_effective_current_provider(&state.db, &app_type)
            .map(|id| id.unwrap_or_default())
    }

    pub fn add(state: &AppState, app_type: AppType, provider: Provider) -> Result<bool, AppError> {
        let mut provider = provider;
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        state.db.save_provider(app_type.as_str(), &provider)?;

        if app_type.is_additive_mode() {
            if matches!(app_type, AppType::OpenCode)
                && Self::omo_variant_for(provider.category.as_deref()).is_some()
            {
                return Ok(true);
            }
            Self::write_live_snapshot(&app_type, &provider)?;
            return Ok(true);
        }

        let current = crate::settings::get_effective_current_provider(&state.db, &app_type)?;
        if current.is_none() {
            crate::settings::set_current_provider(&app_type, Some(&provider.id))?;
            state
                .db
                .set_current_provider(app_type.as_str(), &provider.id)?;
            Self::write_live_snapshot(&app_type, &provider)?;
        }

        Ok(true)
    }

    pub fn update(
        state: &AppState,
        app_type: AppType,
        provider: Provider,
    ) -> Result<bool, AppError> {
        let mut provider = provider;
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        state.db.save_provider(app_type.as_str(), &provider)?;

        if app_type.is_additive_mode() {
            if matches!(app_type, AppType::OpenCode) {
                if let Some((category, variant)) =
                    Self::omo_variant_for(provider.category.as_deref())
                {
                    let is_current = state.db.is_omo_provider_current(
                        app_type.as_str(),
                        &provider.id,
                        category,
                    )?;
                    if is_current {
                        crate::services::omo::OmoService::write_config_to_file(state, variant)?;
                    }
                    return Ok(true);
                }
            }
            Self::write_live_snapshot(&app_type, &provider)?;
            return Ok(true);
        }

        let current = crate::settings::get_effective_current_provider(&state.db, &app_type)?;
        if current.as_deref() == Some(provider.id.as_str()) {
            Self::write_live_snapshot(&app_type, &provider)?;
        }

        Ok(true)
    }

    pub fn delete(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        if app_type.is_additive_mode() {
            if matches!(app_type, AppType::OpenCode) {
                let category = state
                    .db
                    .get_provider_by_id(id, app_type.as_str())?
                    .and_then(|provider| provider.category);

                if let Some((omo_category, variant)) = Self::omo_variant_for(category.as_deref()) {
                    let was_current =
                        state
                            .db
                            .is_omo_provider_current(app_type.as_str(), id, omo_category)?;
                    state.db.delete_provider(app_type.as_str(), id)?;
                    if was_current {
                        crate::services::omo::OmoService::delete_config_file(variant)?;
                    }
                    return Ok(());
                }
            }

            state.db.delete_provider(app_type.as_str(), id)?;
            match app_type {
                AppType::OpenCode => remove_opencode_provider_from_live(id)?,
                AppType::OpenClaw => remove_openclaw_provider_from_live(id)?,
                _ => {}
            }
            return Ok(());
        }

        let local_current = crate::settings::get_current_provider(&app_type);
        let db_current = state.db.get_current_provider(app_type.as_str())?;
        if local_current.as_deref() == Some(id) || db_current.as_deref() == Some(id) {
            return Err(AppError::Message(
                "Cannot delete the currently active provider".to_string(),
            ));
        }

        state.db.delete_provider(app_type.as_str(), id)
    }

    pub fn switch(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers
            .get(id)
            .ok_or_else(|| AppError::Message(format!("Provider {id} not found")))?;

        if app_type.is_additive_mode() {
            if matches!(app_type, AppType::OpenCode) {
                if let Some((category, variant)) =
                    Self::omo_variant_for(provider.category.as_deref())
                {
                    state
                        .db
                        .set_omo_provider_current(app_type.as_str(), id, category)?;
                    crate::services::omo::OmoService::write_config_to_file(state, variant)?;

                    let opposite = if category == "omo" {
                        Some(&crate::services::omo::SLIM)
                    } else {
                        Some(&crate::services::omo::STANDARD)
                    };
                    if let Some(opposite_variant) = opposite {
                        let _ =
                            crate::services::omo::OmoService::delete_config_file(opposite_variant);
                    }

                    return Ok(());
                }
            }
            Self::write_live_snapshot(&app_type, provider)?;
            return Ok(());
        }

        if let Some(current_id) =
            crate::settings::get_effective_current_provider(&state.db, &app_type)?
        {
            if current_id != id {
                if let Ok(live_config) = Self::read_live_settings(app_type.clone()) {
                    let _ = state.db.update_provider_settings_config(
                        app_type.as_str(),
                        &current_id,
                        &live_config,
                    );
                }
            }
        }

        crate::settings::set_current_provider(&app_type, Some(id))?;
        state.db.set_current_provider(app_type.as_str(), id)?;
        Self::write_live_snapshot(&app_type, provider)?;
        McpService::sync_all_enabled(state)?;

        Ok(())
    }

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

    pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
        live::sync_current_to_live(state)
    }

    pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
        live::import_default_config(state, app_type)
    }

    pub fn read_live_settings(app_type: AppType) -> Result<Value, AppError> {
        live::read_live_settings(app_type)
    }

    pub fn import_opencode_providers_from_live(state: &AppState) -> Result<usize, AppError> {
        live::import_opencode_providers_from_live(state)
    }

    pub fn import_openclaw_providers_from_live(state: &AppState) -> Result<usize, AppError> {
        live::import_openclaw_providers_from_live(state)
    }

    pub fn get_custom_endpoints(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
    ) -> Result<Vec<CustomEndpoint>, AppError> {
        endpoints::get_custom_endpoints(state, app_type, provider_id)
    }

    pub fn add_custom_endpoint(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        url: String,
    ) -> Result<(), AppError> {
        endpoints::add_custom_endpoint(state, app_type, provider_id, url)
    }

    pub fn remove_custom_endpoint(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        url: String,
    ) -> Result<(), AppError> {
        endpoints::remove_custom_endpoint(state, app_type, provider_id, url)
    }

    pub fn update_endpoint_last_used(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        url: String,
    ) -> Result<(), AppError> {
        endpoints::update_endpoint_last_used(state, app_type, provider_id, url)
    }

    pub fn remove_from_live_config(
        state: &AppState,
        app_type: AppType,
        id: &str,
    ) -> Result<(), AppError> {
        match app_type {
            AppType::OpenCode => {
                let category = state
                    .db
                    .get_provider_by_id(id, app_type.as_str())?
                    .and_then(|provider| provider.category);

                if let Some((omo_category, variant)) = Self::omo_variant_for(category.as_deref()) {
                    state
                        .db
                        .clear_omo_provider_current(app_type.as_str(), id, omo_category)?;
                    let still_has_current = state
                        .db
                        .get_current_omo_provider(app_type.as_str(), omo_category)?
                        .is_some();
                    if still_has_current {
                        crate::services::omo::OmoService::write_config_to_file(state, variant)?;
                    } else {
                        crate::services::omo::OmoService::delete_config_file(variant)?;
                    }
                    Ok(())
                } else {
                    remove_opencode_provider_from_live(id)
                }
            }
            AppType::OpenClaw => remove_openclaw_provider_from_live(id),
            _ => Err(AppError::Message(format!(
                "App {} does not support remove from live config",
                app_type.as_str()
            ))),
        }
    }

    pub fn extract_common_config_snippet(
        state: &AppState,
        app_type: AppType,
    ) -> Result<String, AppError> {
        let current_id = Self::current(state, app_type.clone())?;
        if current_id.is_empty() {
            return Err(AppError::Message("No current provider".to_string()));
        }

        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers
            .get(&current_id)
            .ok_or_else(|| AppError::Message(format!("Provider {current_id} not found")))?;

        Self::extract_common_config_snippet_from_settings(app_type, &provider.settings_config)
    }

    pub fn extract_common_config_snippet_from_settings(
        app_type: AppType,
        settings_config: &Value,
    ) -> Result<String, AppError> {
        match app_type {
            AppType::Claude => Self::extract_claude_common_config(settings_config),
            AppType::Codex => Self::extract_codex_common_config(settings_config),
            AppType::Gemini => Self::extract_gemini_common_config(settings_config),
            AppType::OpenCode => Self::extract_opencode_common_config(settings_config),
            AppType::OpenClaw => Self::extract_openclaw_common_config(settings_config),
        }
    }

    pub fn list_universal(
        state: &AppState,
    ) -> Result<HashMap<String, UniversalProvider>, AppError> {
        state.db.get_all_universal_providers()
    }

    pub fn get_universal(
        state: &AppState,
        id: &str,
    ) -> Result<Option<UniversalProvider>, AppError> {
        state.db.get_universal_provider(id)
    }

    pub fn upsert_universal(
        state: &AppState,
        provider: UniversalProvider,
    ) -> Result<bool, AppError> {
        state.db.save_universal_provider(&provider)
    }

    pub fn sync_universal_to_apps(state: &AppState, id: &str) -> Result<(), AppError> {
        let provider = state
            .db
            .get_universal_provider(id)?
            .ok_or_else(|| AppError::Message(format!("Universal provider {id} not found")))?;

        if provider.apps.claude {
            if let Some(claude_provider) = provider.to_claude_provider() {
                Self::add(state, AppType::Claude, claude_provider)?;
            }
        }

        if provider.apps.codex {
            if let Some(codex_provider) = provider.to_codex_provider() {
                Self::add(state, AppType::Codex, codex_provider)?;
            }
        }

        if provider.apps.gemini {
            if let Some(gemini_provider) = provider.to_gemini_provider() {
                Self::add(state, AppType::Gemini, gemini_provider)?;
            }
        }

        Ok(())
    }

    pub fn delete_universal(state: &AppState, id: &str) -> Result<(), AppError> {
        state.db.delete_universal_provider(id)
    }

    fn validate_provider_settings(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
        if !provider.settings_config.is_object() {
            return Err(AppError::Config(format!(
                "{} provider config must be a JSON object",
                app_type.as_str()
            )));
        }

        match app_type {
            AppType::Claude => Ok(()),
            AppType::Codex => {
                let obj = provider.settings_config.as_object().ok_or_else(|| {
                    AppError::Config("Codex provider config must be an object".to_string())
                })?;

                if !obj.get("auth").is_some_and(Value::is_object) {
                    return Err(AppError::localized(
                        "codex.validation.missing_auth",
                        "Codex 供应商配置缺少 auth 对象",
                        "Codex provider config is missing the auth object",
                    ));
                }

                let config = obj.get("config").and_then(Value::as_str).ok_or_else(|| {
                    AppError::localized(
                        "codex.validation.missing_config",
                        "Codex 供应商配置缺少 config 文本",
                        "Codex provider config is missing config text",
                    )
                })?;

                crate::codex_config::validate_config_toml(config)
            }
            AppType::Gemini => {
                crate::gemini_config::validate_gemini_settings(&provider.settings_config)
            }
            AppType::OpenCode => {
                let _ = serde_json::from_value::<OpenCodeProviderConfig>(
                    provider.settings_config.clone(),
                )
                .or_else(|_| Ok(OpenCodeProviderConfig::default()))
                .map_err(|e: serde_json::Error| AppError::Config(e.to_string()))?;
                Ok(())
            }
            AppType::OpenClaw => Ok(()),
        }
    }

    fn omo_variant_for(
        category: Option<&str>,
    ) -> Option<(&'static str, &'static crate::services::omo::OmoVariant)> {
        match category {
            Some("omo") => Some(("omo", &crate::services::omo::STANDARD)),
            Some("omo-slim") => Some(("omo-slim", &crate::services::omo::SLIM)),
            _ => None,
        }
    }

    fn extract_claude_common_config(settings: &Value) -> Result<String, AppError> {
        let mut config = settings.clone();

        const ENV_EXCLUDES: &[&str] = &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_REASONING_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_BASE_URL",
        ];

        const TOP_LEVEL_EXCLUDES: &[&str] = &["apiBaseUrl", "primaryModel", "smallFastModel"];

        if let Some(env) = config.get_mut("env").and_then(Value::as_object_mut) {
            for key in ENV_EXCLUDES {
                env.remove(*key);
            }
            if env.is_empty() {
                config.as_object_mut().map(|obj| obj.remove("env"));
            }
        }

        if let Some(obj) = config.as_object_mut() {
            for key in TOP_LEVEL_EXCLUDES {
                obj.remove(*key);
            }
        }

        if config.as_object().is_none_or(|obj| obj.is_empty()) {
            return Ok("{}".to_string());
        }

        serde_json::to_string_pretty(&config)
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
    }

    fn extract_codex_common_config(settings: &Value) -> Result<String, AppError> {
        let config_toml = settings
            .get("config")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if config_toml.is_empty() {
            return Ok(String::new());
        }

        let mut doc = config_toml
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::Message(format!("TOML parse error: {e}")))?;

        let root = doc.as_table_mut();
        root.remove("model");
        root.remove("model_provider");
        root.remove("base_url");
        root.remove("model_providers");

        let mut cleaned = String::new();
        let mut blank_run = 0usize;
        for line in doc.to_string().lines() {
            if line.trim().is_empty() {
                blank_run += 1;
                if blank_run <= 1 {
                    cleaned.push('\n');
                }
                continue;
            }

            blank_run = 0;
            cleaned.push_str(line);
            cleaned.push('\n');
        }

        Ok(cleaned.trim().to_string())
    }

    fn extract_gemini_common_config(settings: &Value) -> Result<String, AppError> {
        let env = settings.get("env").and_then(Value::as_object);
        let mut snippet = serde_json::Map::new();

        if let Some(env) = env {
            for (key, value) in env {
                if key == "GOOGLE_GEMINI_BASE_URL" || key == "GEMINI_API_KEY" {
                    continue;
                }

                let Some(text) = value.as_str() else {
                    continue;
                };
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    snippet.insert(key.clone(), Value::String(trimmed.to_string()));
                }
            }
        }

        if snippet.is_empty() {
            return Ok("{}".to_string());
        }

        serde_json::to_string_pretty(&Value::Object(snippet))
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
    }

    fn extract_opencode_common_config(settings: &Value) -> Result<String, AppError> {
        let mut config = settings.clone();
        if let Some(obj) = config.as_object_mut() {
            if let Some(options) = obj.get_mut("options").and_then(Value::as_object_mut) {
                options.remove("apiKey");
                options.remove("baseURL");
            }
        }

        if config.is_null() || config.as_object().is_some_and(|obj| obj.is_empty()) {
            return Ok("{}".to_string());
        }

        serde_json::to_string_pretty(&config)
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
    }

    fn extract_openclaw_common_config(settings: &Value) -> Result<String, AppError> {
        let mut config = settings.clone();
        if let Some(obj) = config.as_object_mut() {
            obj.remove("apiKey");
            obj.remove("baseUrl");
        }

        if config.is_null() || config.as_object().is_some_and(|obj| obj.is_empty()) {
            return Ok("{}".to_string());
        }

        serde_json::to_string_pretty(&config)
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
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
                    .and_then(Value::as_object)
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
                    .and_then(Value::as_str)
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
                    .and_then(Value::as_str)
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
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.codex.auth.missing",
                            "配置格式错误: 缺少 auth",
                            "Invalid configuration: missing auth section",
                        )
                    })?;

                let api_key = auth
                    .get("OPENAI_API_KEY")
                    .and_then(Value::as_str)
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
                    .and_then(Value::as_str)
                    .unwrap_or_default();

                let base_url = if config_toml.contains("base_url") {
                    let regex = Regex::new(r#"base_url\s*=\s*["']([^"']+)["']"#).map_err(|e| {
                        AppError::localized(
                            "provider.regex_init_failed",
                            format!("正则初始化失败: {e}"),
                            format!("Failed to initialize regex: {e}"),
                        )
                    })?;
                    regex
                        .captures(config_toml)
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
                let env_map = crate::gemini_config::json_to_env(&provider.settings_config)?;
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
            AppType::OpenCode => {
                let options = provider
                    .settings_config
                    .get("options")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.opencode.options.missing",
                            "配置格式错误: 缺少 options",
                            "Invalid configuration: missing options section",
                        )
                    })?;

                let api_key = options
                    .get("apiKey")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.opencode.api_key.missing",
                            "缺少 API Key",
                            "API key is missing",
                        )
                    })?
                    .to_string();

                let base_url = options
                    .get("baseURL")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                Ok((api_key, base_url))
            }
            AppType::OpenClaw => {
                let api_key = provider
                    .settings_config
                    .get("apiKey")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.openclaw.api_key.missing",
                            "缺少 API Key",
                            "API key is missing",
                        )
                    })?
                    .to_string();

                let base_url = provider
                    .settings_config
                    .get("baseUrl")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                Ok((api_key, base_url))
            }
        }
    }

    fn write_live_snapshot(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
        match app_type {
            AppType::Claude => {
                let path = crate::config::get_claude_settings_path();
                let settings = sanitize_claude_settings_for_live(&provider.settings_config);
                crate::config::write_json_file(&path, &settings)?;
            }
            AppType::Codex => {
                let obj = provider.settings_config.as_object().ok_or_else(|| {
                    AppError::Config("Codex provider config must be a JSON object".to_string())
                })?;
                let auth = obj.get("auth").ok_or_else(|| {
                    AppError::Config("Codex provider config is missing 'auth'".to_string())
                })?;
                let config = obj.get("config").and_then(Value::as_str).ok_or_else(|| {
                    AppError::Config("Codex provider config is missing 'config'".to_string())
                })?;
                crate::codex_config::write_codex_live_atomic(auth, Some(config))?;
            }
            AppType::Gemini => write_gemini_live(provider)?,
            AppType::OpenCode => {
                match serde_json::from_value::<OpenCodeProviderConfig>(
                    provider.settings_config.clone(),
                ) {
                    Ok(config) => {
                        crate::opencode_config::set_typed_provider(&provider.id, &config)?
                    }
                    Err(_) => crate::opencode_config::set_provider(
                        &provider.id,
                        provider.settings_config.clone(),
                    )?,
                }
            }
            AppType::OpenClaw => {
                match serde_json::from_value::<crate::openclaw_config::OpenClawProviderConfig>(
                    provider.settings_config.clone(),
                ) {
                    Ok(config) => {
                        crate::openclaw_config::set_typed_provider(&provider.id, &config)?
                    }
                    Err(_) => crate::openclaw_config::set_provider(
                        &provider.id,
                        provider.settings_config.clone(),
                    )?,
                }
            }
        }

        Ok(())
    }
}

pub(crate) fn normalize_claude_models_in_value(settings: &mut Value) -> bool {
    let mut changed = false;
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return changed;
    };

    let model = env
        .get("ANTHROPIC_MODEL")
        .and_then(Value::as_str)
        .map(str::to_string);
    let small_fast = env
        .get("ANTHROPIC_SMALL_FAST_MODEL")
        .and_then(Value::as_str)
        .map(str::to_string);

    let current_haiku = env
        .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
        .and_then(Value::as_str)
        .map(str::to_string);
    let current_sonnet = env
        .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
        .and_then(Value::as_str)
        .map(str::to_string);
    let current_opus = env
        .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
        .and_then(Value::as_str)
        .map(str::to_string);

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
        if let Some(value) = target_haiku {
            env.insert(
                "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
                Value::String(value),
            );
            changed = true;
        }
    }

    if env.get("ANTHROPIC_DEFAULT_SONNET_MODEL").is_none() {
        if let Some(value) = target_sonnet {
            env.insert(
                "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
                Value::String(value),
            );
            changed = true;
        }
    }

    if env.get("ANTHROPIC_DEFAULT_OPUS_MODEL").is_none() {
        if let Some(value) = target_opus {
            env.insert(
                "ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(),
                Value::String(value),
            );
            changed = true;
        }
    }

    if env.remove("ANTHROPIC_SMALL_FAST_MODEL").is_some() {
        changed = true;
    }

    changed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointLatency {
    pub url: String,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use tempfile::tempdir;

    use crate::database::Database;

    #[test]
    fn additive_mode_apps_have_no_current_provider() -> Result<(), AppError> {
        let state = AppState::new(Database::memory()?);

        assert_eq!(ProviderService::current(&state, AppType::OpenCode)?, "");
        assert_eq!(ProviderService::current(&state, AppType::OpenClaw)?, "");

        Ok(())
    }

    #[test]
    #[serial]
    fn add_openclaw_provider_writes_live_config() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        crate::settings::update_settings(crate::settings::AppSettings::default())?;

        let state = AppState::new(Database::memory()?);
        let provider = Provider::with_id(
            "openclaw-provider".to_string(),
            "OpenClaw Provider".to_string(),
            json!({
                "baseUrl": "https://example.com/v1",
                "apiKey": "test-key",
                "api": "openai-completions",
                "models": [{ "id": "gpt-4.1", "name": "GPT 4.1" }]
            }),
            None,
        );

        ProviderService::add(&state, AppType::OpenClaw, provider)?;

        let config = crate::openclaw_config::read_openclaw_config()?;
        assert_eq!(
            config
                .pointer("/models/providers/openclaw-provider/baseUrl")
                .and_then(Value::as_str),
            Some("https://example.com/v1")
        );

        Ok(())
    }

    #[test]
    #[serial]
    fn switch_omo_provider_writes_exclusive_config() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let state = AppState::new(Database::memory()?);
        let mut provider = Provider::with_id(
            "omo-demo".to_string(),
            "OMO Demo".to_string(),
            json!({
                "agents": { "demo": { "prompt": "hi" } },
                "categories": ["tools"],
                "otherFields": { "theme": "default" }
            }),
            None,
        );
        provider.category = Some("omo".to_string());

        ProviderService::add(&state, AppType::OpenCode, provider)?;
        ProviderService::switch(&state, AppType::OpenCode, "omo-demo")?;

        let written: Value = crate::config::read_json_file(
            &temp.path().join(".config/opencode/oh-my-opencode.jsonc"),
        )?;
        assert!(written.get("agents").is_some());

        let config = crate::opencode_config::read_opencode_config()?;
        assert!(config
            .get("plugin")
            .and_then(Value::as_array)
            .is_some_and(|plugins| plugins
                .iter()
                .any(|item| item.as_str() == Some("oh-my-opencode@latest"))));

        Ok(())
    }
}
