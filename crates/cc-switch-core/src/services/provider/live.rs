//! Provider live config read/write helpers.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::codex_config::{get_codex_auth_path, get_codex_config_path};
use crate::config::{delete_file, get_claude_settings_path, read_json_file, write_json_file};
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::McpService;
use crate::store::AppState;

use super::gemini_auth::{
    detect_gemini_auth_type, ensure_google_oauth_security_flag, GeminiAuthType,
};
use super::normalize_claude_models_in_value;

pub(crate) fn sanitize_claude_settings_for_live(settings: &Value) -> Value {
    let mut value = settings.clone();
    if let Some(obj) = value.as_object_mut() {
        obj.remove("api_format");
        obj.remove("apiFormat");
        obj.remove("openrouter_compat_mode");
        obj.remove("openrouterCompatMode");
    }
    value
}

pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
    for app_type in AppType::all() {
        if app_type.is_additive_mode() {
            sync_all_providers_to_live(state, &app_type)?;
            continue;
        }

        let Some(current_id) =
            crate::settings::get_effective_current_provider(&state.db, &app_type)?
        else {
            continue;
        };

        let providers = state.db.get_all_providers(app_type.as_str())?;
        if let Some(provider) = providers.get(&current_id) {
            super::ProviderService::write_live_snapshot(&app_type, provider)?;
        }
    }

    McpService::sync_all_enabled(state)?;
    Ok(())
}

pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
    if app_type.is_additive_mode() {
        return Ok(false);
    }

    if !state.db.get_all_providers(app_type.as_str())?.is_empty() {
        return Ok(false);
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
            let config = crate::codex_config::read_and_validate_codex_config_text()?;
            json!({ "auth": auth, "config": config })
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

            let mut value = read_json_file::<Value>(&settings_path)?;
            let _ = normalize_claude_models_in_value(&mut value);
            value
        }
        AppType::Gemini => {
            use crate::gemini_config::{
                env_to_json, get_gemini_env_path, get_gemini_settings_path, read_gemini_env,
            };

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
            let settings_path = get_gemini_settings_path();
            let config = if settings_path.exists() {
                read_json_file(&settings_path)?
            } else {
                json!({})
            };

            json!({ "env": env_obj, "config": config })
        }
        AppType::OpenCode | AppType::OpenClaw => {
            unreachable!("additive mode apps are handled above")
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
    state.db.set_current_provider(app_type.as_str(), &provider.id)?;
    crate::settings::set_current_provider(&app_type, Some(&provider.id))?;

    Ok(true)
}

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
            let config = crate::codex_config::read_and_validate_codex_config_text()?;
            Ok(json!({ "auth": auth, "config": config }))
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
            let settings_path = get_gemini_settings_path();
            let config = if settings_path.exists() {
                read_json_file(&settings_path)?
            } else {
                json!({})
            };

            Ok(json!({ "env": env_obj, "config": config }))
        }
        AppType::OpenCode => {
            let path = crate::opencode_config::get_opencode_config_path();
            if !path.exists() {
                return Err(AppError::localized(
                    "opencode.config.missing",
                    "OpenCode 配置文件不存在",
                    "OpenCode configuration file not found",
                ));
            }

            crate::opencode_config::read_opencode_config()
        }
        AppType::OpenClaw => {
            let path = crate::openclaw_config::get_openclaw_config_path();
            if !path.exists() {
                return Err(AppError::localized(
                    "openclaw.config.missing",
                    "OpenClaw 配置文件不存在",
                    "OpenClaw configuration file not found",
                ));
            }

            crate::openclaw_config::read_openclaw_config()
        }
    }
}

pub(crate) fn write_gemini_live(provider: &Provider) -> Result<(), AppError> {
    use crate::gemini_config::{
        get_gemini_settings_path, json_to_env, validate_gemini_settings_strict,
        write_gemini_env_atomic,
    };

    let auth_type = detect_gemini_auth_type(provider);
    let mut env_map = json_to_env(&provider.settings_config)?;
    let settings_path = get_gemini_settings_path();
    let mut config_to_write: Option<Value> = None;

    if let Some(config_value) = provider.settings_config.get("config") {
        if config_value.is_object() {
            let mut merged = if settings_path.exists() {
                read_json_file::<Value>(&settings_path).unwrap_or_else(|_| json!({}))
            } else {
                json!({})
            };

            if let (Some(merged_obj), Some(config_obj)) =
                (merged.as_object_mut(), config_value.as_object())
            {
                for (key, value) in config_obj {
                    merged_obj.insert(key.clone(), value.clone());
                }
            }
            config_to_write = Some(merged);
        } else if !config_value.is_null() {
            return Err(AppError::localized(
                "gemini.validation.invalid_config",
                "Gemini 配置格式错误: config 必须是对象或 null",
                "Gemini config invalid: config must be an object or null",
            ));
        }
    }

    if config_to_write.is_none() && settings_path.exists() {
        config_to_write = Some(read_json_file(&settings_path)?);
    }

    match auth_type {
        GeminiAuthType::GoogleOfficial => {
            env_map.clear();
            write_gemini_env_atomic(&env_map)?;
        }
        GeminiAuthType::Packycode | GeminiAuthType::Generic => {
            validate_gemini_settings_strict(&provider.settings_config)?;
            write_gemini_env_atomic(&env_map)?;
        }
    }

    if let Some(config) = config_to_write {
        write_json_file(&settings_path, &config)?;
    }

    match auth_type {
        GeminiAuthType::GoogleOfficial => ensure_google_oauth_security_flag(provider)?,
        GeminiAuthType::Packycode | GeminiAuthType::Generic => {
            crate::gemini_config::write_packycode_settings()?;
        }
    }

    Ok(())
}

pub(crate) fn remove_opencode_provider_from_live(provider_id: &str) -> Result<(), AppError> {
    if !crate::opencode_config::get_opencode_dir().exists() {
        return Ok(());
    }

    crate::opencode_config::remove_provider(provider_id)
}

pub(crate) fn remove_openclaw_provider_from_live(provider_id: &str) -> Result<(), AppError> {
    if !crate::openclaw_config::get_openclaw_dir().exists() {
        return Ok(());
    }

    crate::openclaw_config::remove_provider(provider_id)
}

pub fn import_opencode_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    let providers = crate::opencode_config::get_typed_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let existing = state.db.get_all_providers(AppType::OpenCode.as_str())?;
    let mut imported = 0usize;

    for (id, config) in providers {
        if existing.contains_key(&id) {
            continue;
        }

        let mut provider = Provider::with_id(
            id.clone(),
            config.name.clone().unwrap_or_else(|| id.clone()),
            serde_json::to_value(&config).map_err(|e| AppError::JsonSerialize { source: e })?,
            None,
        );
        provider.category = Some("custom".to_string());
        state.db.save_provider(AppType::OpenCode.as_str(), &provider)?;
        imported += 1;
    }

    Ok(imported)
}

pub fn import_openclaw_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    let providers = crate::openclaw_config::get_typed_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let existing = state.db.get_all_providers(AppType::OpenClaw.as_str())?;
    let mut imported = 0usize;

    for (id, config) in providers {
        if existing.contains_key(&id) {
            continue;
        }

        let provider = Provider::with_id(
            id.clone(),
            id.clone(),
            serde_json::to_value(&config).map_err(|e| AppError::JsonSerialize { source: e })?,
            None,
        );
        state
            .db
            .save_provider(AppType::OpenClaw.as_str(), &provider)?;
        imported += 1;
    }

    Ok(imported)
}

fn sync_all_providers_to_live(state: &AppState, app_type: &AppType) -> Result<(), AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;
    for provider in providers.values() {
        if let Err(error) = super::ProviderService::write_live_snapshot(app_type, provider) {
            log::warn!(
                "Failed to sync {:?} provider '{}' to live config: {error}",
                app_type,
                provider.id
            );
        }
    }
    Ok(())
}

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
        env: Option<HashMap<String, String>>,
        config: Option<Value>,
    },
}

#[allow(dead_code)]
impl LiveSnapshot {
    fn restore(&self) -> Result<(), AppError> {
        match self {
            Self::Claude { settings } => {
                let path = get_claude_settings_path();
                if let Some(value) = settings {
                    write_json_file(&path, value)?;
                } else if path.exists() {
                    delete_file(&path)?;
                }
            }
            Self::Codex { auth, config } => {
                let auth_path = get_codex_auth_path();
                let config_path = get_codex_config_path();

                if let Some(value) = auth {
                    write_json_file(&auth_path, value)?;
                } else if auth_path.exists() {
                    delete_file(&auth_path)?;
                }

                if let Some(text) = config {
                    crate::config::write_text_file(&config_path, text)?;
                } else if config_path.exists() {
                    delete_file(&config_path)?;
                }
            }
            Self::Gemini { env, config } => {
                use crate::gemini_config::{
                    get_gemini_env_path, get_gemini_settings_path, write_gemini_env_atomic,
                };

                let env_path = get_gemini_env_path();
                if let Some(env_map) = env {
                    write_gemini_env_atomic(env_map)?;
                } else if env_path.exists() {
                    delete_file(&env_path)?;
                }

                let settings_path = get_gemini_settings_path();
                if let Some(config) = config {
                    write_json_file(&settings_path, config)?;
                } else if settings_path.exists() {
                    delete_file(&settings_path)?;
                }
            }
        }

        Ok(())
    }
}
