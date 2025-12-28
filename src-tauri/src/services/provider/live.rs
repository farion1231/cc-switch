//! Live configuration operations
//!
//! Handles reading and writing live configuration files for Claude, Codex, and Gemini.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::codex_config::{get_codex_auth_path, get_codex_config_path};
use crate::config::{delete_file, get_claude_settings_path, read_json_file, write_json_file};
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::mcp::McpService;
use crate::store::AppState;

use super::gemini_auth::{
    detect_gemini_auth_type, ensure_google_oauth_security_flag, GeminiAuthType,
};
use super::normalize_claude_models_in_value;

/// Live configuration snapshot for backup/restore
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum LiveSnapshot {
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

impl LiveSnapshot {
    #[allow(dead_code)]
    pub(crate) fn restore(&self) -> Result<(), AppError> {
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
                    crate::config::write_text_file(&config_path, text)?;
                } else if config_path.exists() {
                    delete_file(&config_path)?;
                }
            }
            LiveSnapshot::Gemini { env, .. } => {
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

/// Write live configuration snapshot for a provider
pub(crate) fn write_live_snapshot(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
    match app_type {
        AppType::Claude => {
            let path = get_claude_settings_path();
            write_json_file(&path, &provider.settings_config)?;
        }
        AppType::Codex => {
            let obj = provider
                .settings_config
                .as_object()
                .ok_or_else(|| AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string()))?;
            let auth = obj
                .get("auth")
                .ok_or_else(|| AppError::Config("Codex 供应商配置缺少 'auth' 字段".to_string()))?;
            let config_str = obj.get("config").and_then(|v| v.as_str()).ok_or_else(|| {
                AppError::Config("Codex 供应商配置缺少 'config' 字段或不是字符串".to_string())
            })?;

            let auth_path = get_codex_auth_path();
            write_json_file(&auth_path, auth)?;
            let config_path = get_codex_config_path();
            std::fs::write(&config_path, config_str).map_err(|e| AppError::io(&config_path, e))?;
        }
        AppType::Gemini => {
            // Delegate to write_gemini_live which handles env file writing correctly
            write_gemini_live(provider)?;
        }
        AppType::Droid => {
            // Droid 配置写入 ~/.factory/config.json
            // 配置机制：
            // - 主配置文件: ~/.factory/config.json (用户编辑此文件)
            // - 运行时配置: ~/.factory/settings.json (重启后从 config.json 同步)
            // - 需要清理 settings.json 中的 customModels 和 sessionDefaultSettings.model 才能生效
            write_droid_live(provider)?;
        }
    }
    Ok(())
}

/// Sync current provider to live configuration
///
/// 使用有效的当前供应商 ID（验证过存在性）。
/// 优先从本地 settings 读取，验证后 fallback 到数据库的 is_current 字段。
/// 这确保了配置导入后无效 ID 会自动 fallback 到数据库。
pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
    for app_type in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        // Use validated effective current provider
        let current_id =
            match crate::settings::get_effective_current_provider(&state.db, &app_type)? {
                Some(id) => id,
                None => continue,
            };

        let providers = state.db.get_all_providers(app_type.as_str())?;
        if let Some(provider) = providers.get(&current_id) {
            write_live_snapshot(&app_type, provider)?;
        }
        // Note: get_effective_current_provider already validates existence,
        // so providers.get() should always succeed here
    }

    // MCP sync
    McpService::sync_all_enabled(state)?;
    Ok(())
}

/// Read current live settings for an app type
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

            // Read .env file (environment variables)
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

            // Read settings.json file (MCP config etc.)
            let settings_path = get_gemini_settings_path();
            let config_obj = if settings_path.exists() {
                read_json_file(&settings_path)?
            } else {
                json!({})
            };

            // Return complete structure: { "env": {...}, "config": {...} }
            Ok(json!({
                "env": env_obj,
                "config": config_obj
            }))
        }
        AppType::Droid => {
            // 读取 Droid config.json (主配置文件)
            use crate::droid_config::read_droid_config;
            read_droid_config()
        }
    }
}

/// Import default configuration from live files
///
/// Returns `Ok(true)` if a provider was actually imported,
/// `Ok(false)` if skipped (providers already exist for this app).
pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
    {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        if !providers.is_empty() {
            return Ok(false); // 已有供应商，跳过
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
            let _ = normalize_claude_models_in_value(&mut v);
            v
        }
        AppType::Gemini => {
            use crate::gemini_config::{
                env_to_json, get_gemini_env_path, get_gemini_settings_path, read_gemini_env,
            };

            // Read .env file (environment variables)
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

            // Read settings.json file (MCP config etc.)
            let settings_path = get_gemini_settings_path();
            let config_obj = if settings_path.exists() {
                read_json_file(&settings_path)?
            } else {
                json!({})
            };

            // Return complete structure: { "env": {...}, "config": {...} }
            json!({
                "env": env_obj,
                "config": config_obj
            })
        }
        AppType::Droid => {
            // Droid 从 config.json 导入
            use crate::droid_config::read_droid_config;
            let config = read_droid_config()?;
            
            // 如果 config.json 中有 customModels，取第一个作为默认配置
            if let Some(custom_models) = config.get("customModels").and_then(|v| v.as_array()) {
                if let Some(first_model) = custom_models.first() {
                    first_model.clone()
                } else {
                    json!({})
                }
            } else {
                json!({})
            }
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

    Ok(true) // 真正导入了
}

/// Write Gemini live configuration with authentication handling
pub(crate) fn write_gemini_live(provider: &Provider) -> Result<(), AppError> {
    use crate::gemini_config::{
        get_gemini_settings_path, json_to_env, validate_gemini_settings_strict,
        write_gemini_env_atomic,
    };

    // One-time auth type detection to avoid repeated detection
    let auth_type = detect_gemini_auth_type(provider);

    let mut env_map = json_to_env(&provider.settings_config)?;

    // Prepare config to write to ~/.gemini/settings.json
    // Behavior:
    // - config is object: use it (merge with existing to preserve mcpServers etc.)
    // - config is null or absent: preserve existing file content
    let settings_path = get_gemini_settings_path();
    let mut config_to_write: Option<Value> = None;

    if let Some(config_value) = provider.settings_config.get("config") {
        if config_value.is_object() {
            // Merge with existing settings to preserve mcpServers and other fields
            let mut merged = if settings_path.exists() {
                read_json_file::<Value>(&settings_path).unwrap_or_else(|_| json!({}))
            } else {
                json!({})
            };

            // Merge provider config into existing settings
            if let (Some(merged_obj), Some(config_obj)) =
                (merged.as_object_mut(), config_value.as_object())
            {
                for (k, v) in config_obj {
                    merged_obj.insert(k.clone(), v.clone());
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
        // config is null: don't modify existing settings.json (preserve mcpServers etc.)
    }

    // If no config specified or config is null, preserve existing file
    if config_to_write.is_none() && settings_path.exists() {
        config_to_write = Some(read_json_file(&settings_path)?);
    }

    match auth_type {
        GeminiAuthType::GoogleOfficial => {
            // Google official uses OAuth, clear env
            env_map.clear();
            write_gemini_env_atomic(&env_map)?;
        }
        GeminiAuthType::Packycode => {
            // PackyCode provider, uses API Key (strict validation on switch)
            validate_gemini_settings_strict(&provider.settings_config)?;
            write_gemini_env_atomic(&env_map)?;
        }
        GeminiAuthType::Generic => {
            // Generic provider, uses API Key (strict validation on switch)
            validate_gemini_settings_strict(&provider.settings_config)?;
            write_gemini_env_atomic(&env_map)?;
        }
    }

    if let Some(config_value) = config_to_write {
        write_json_file(&settings_path, &config_value)?;
    }

    // Set security.auth.selectedType based on auth type
    // - Google Official: OAuth mode
    // - All others: API Key mode
    match auth_type {
        GeminiAuthType::GoogleOfficial => ensure_google_oauth_security_flag(provider)?,
        GeminiAuthType::Packycode | GeminiAuthType::Generic => {
            crate::gemini_config::write_packycode_settings()?;
        }
    }

    Ok(())
}

/// Write Droid live configuration directly to ~/.factory/settings.json
///
/// 新的配置机制（热更新）：
/// - 直接写入 settings.json 的 customModels 数组
/// - 重启 Droid 后立即生效，无需修改 config.json
/// - 每个模型需要唯一的 id 和 index
///
/// customModels 格式 (camelCase):
/// {
///   "model": "sonnet-4-5",
///   "id": "custom:provider-name-0",
///   "index": 0,
///   "baseUrl": "https://api.example.com",
///   "apiKey": "your-api-key",
///   "displayName": "Provider Name",
///   "maxOutputTokens": 131072,
///   "noImageSupport": false,
///   "provider": "anthropic"
/// }
pub(crate) fn write_droid_live(provider: &Provider) -> Result<(), AppError> {
    use crate::droid_config::{
        get_droid_settings_path, read_droid_settings, write_droid_settings,
    };

    let settings_path = get_droid_settings_path();

    // 读取现有 settings.json 或创建新配置
    let mut settings = read_droid_settings().unwrap_or_else(|_| json!({}));

    // 从 provider.settings_config 提取 Droid 配置
    let provider_settings = &provider.settings_config;

    // 获取或创建 customModels 数组 (camelCase)
    let settings_obj = settings.as_object_mut().ok_or_else(|| {
        AppError::Config("Droid settings.json 必须是 JSON 对象".to_string())
    })?;

    let custom_models = settings_obj
        .entry("customModels")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            AppError::Config("Droid settings.json 的 customModels 必须是数组".to_string())
        })?;

    // 计算下一个可用的 index
    let next_index = custom_models
        .iter()
        .filter_map(|m| m.get("index").and_then(|v| v.as_i64()))
        .max()
        .map(|max| max + 1)
        .unwrap_or(0) as i64;

    // 构建 custom_model 对象 (camelCase 格式)
    let custom_model = build_droid_settings_model(provider_settings, &provider.name, next_index)?;

    // 获取新模型的 displayName 用于匹配
    let display_name = custom_model
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // 用于保存最终的模型 id
    let final_model_id: String;

    // 检查是否已存在同名模型 (通过 displayName 匹配)
    let existing_index = custom_models.iter().position(|m| {
        m.get("displayName").and_then(|v| v.as_str()) == Some(&display_name)
    });

    if let Some(idx) = existing_index {
        // 保留原有的 index 和 id
        let old_index = custom_models[idx].get("index").and_then(|v| v.as_i64()).unwrap_or(idx as i64);
        let old_id = custom_models[idx].get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
        
        let mut updated_model = custom_model;
        updated_model["index"] = json!(old_index);
        if let Some(id) = &old_id {
            updated_model["id"] = json!(id);
            final_model_id = id.clone();
        } else {
            final_model_id = updated_model.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
        
        custom_models[idx] = updated_model;
        log::info!("更新 Droid 自定义模型: {}", display_name);
    } else {
        final_model_id = custom_model.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        custom_models.push(custom_model);
        log::info!("添加 Droid 自定义模型: {}", display_name);
    }

    // 重新获取 settings_obj 的可变引用（因为之前的借用已结束）
    let settings_obj = settings.as_object_mut().unwrap();

    // 更新 sessionDefaultSettings.model 为当前模型的 id
    if !final_model_id.is_empty() {
        let session_settings = settings_obj
            .entry("sessionDefaultSettings")
            .or_insert_with(|| json!({}));
        
        if let Some(session_obj) = session_settings.as_object_mut() {
            session_obj.insert("model".to_string(), json!(final_model_id.clone()));
            log::info!("设置 sessionDefaultSettings.model = {}", final_model_id);
        }
    }

    // 写入 settings.json
    write_droid_settings(&settings)?;
    log::info!("已写入 Droid settings.json: {}", settings_path.display());

    Ok(())
}

/// 从 provider settings 构建 Droid settings.json 的 customModels 对象
/// 使用 camelCase 字段名（settings.json 格式）
/// 支持 camelCase 和 snake_case 两种输入格式
fn build_droid_settings_model(settings: &Value, provider_name: &str, index: i64) -> Result<Value, AppError> {
    log::debug!("build_droid_settings_model: settings = {:?}", settings);
    
    let obj = settings.as_object().ok_or_else(|| {
        AppError::Config("Droid 供应商配置必须是 JSON 对象".to_string())
    })?;

    // 提取必要字段 (支持 camelCase 和 snake_case 两种格式)
    let api_key = obj
        .get("apiKey")
        .or_else(|| obj.get("api_key"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let base_url = obj
        .get("baseUrl")
        .or_else(|| obj.get("base_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let model = obj
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("claude-sonnet-4-5-20250929");

    let provider_type = obj
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("anthropic");

    let max_tokens = obj
        .get("maxOutputTokens")
        .or_else(|| obj.get("max_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(131072);

    let no_image_support = obj
        .get("noImageSupport")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    log::debug!("build_droid_settings_model: api_key={}, base_url={}, model={}", 
        if api_key.is_empty() { "(empty)" } else { "***" }, 
        base_url, 
        model
    );

    // 生成唯一的 id: "custom:{displayName}-{index}"
    // 清理 displayName 中的特殊字符
    let clean_name = provider_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>();
    let id = format!("custom:{}-{}", clean_name, index);

    // 构建 customModel 对象 (使用 settings.json 的 camelCase 格式)
    Ok(json!({
        "model": model,
        "id": id,
        "index": index,
        "baseUrl": base_url,
        "apiKey": api_key,
        "displayName": provider_name,
        "maxOutputTokens": max_tokens,
        "noImageSupport": no_image_support,
        "provider": provider_type
    }))
}

/// 从 Droid settings.json 中删除指定的 customModel
/// 通过 displayName 匹配
pub(crate) fn remove_droid_custom_model(provider_name: &str) -> Result<(), AppError> {
    use crate::droid_config::{read_droid_settings, write_droid_settings};

    let mut settings = read_droid_settings()?;

    let settings_obj = match settings.as_object_mut() {
        Some(obj) => obj,
        None => return Ok(()), // 配置不是对象，无需处理
    };

    let custom_models = match settings_obj.get_mut("customModels") {
        Some(Value::Array(arr)) => arr,
        _ => return Ok(()), // 没有 customModels 数组，无需处理
    };

    // 查找并删除匹配的 model
    let original_len = custom_models.len();
    custom_models.retain(|m| {
        m.get("displayName")
            .and_then(|v| v.as_str())
            != Some(provider_name)
    });

    if custom_models.len() < original_len {
        write_droid_settings(&settings)?;
        log::info!("已从 Droid settings.json 删除 customModel: {}", provider_name);
    }

    Ok(())
}
