use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;
use crate::settings::get_opencode_override_dir;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn get_opencode_dir() -> PathBuf {
    if let Some(override_dir) = get_opencode_override_dir() {
        return override_dir;
    }

    dirs::home_dir()
        .map(|h| h.join(".config").join("opencode"))
        .unwrap_or_else(|| PathBuf::from(".config").join("opencode"))
}

pub fn get_opencode_config_path() -> PathBuf {
    get_opencode_dir().join("opencode.json")
}

#[allow(dead_code)]
pub fn get_opencode_env_path() -> PathBuf {
    get_opencode_dir().join(".env")
}

pub fn read_opencode_config() -> Result<Value, AppError> {
    let path = get_opencode_config_path();

    if !path.exists() {
        return Ok(json!({
            "$schema": "https://opencode.ai/config.json"
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))
}

pub fn write_opencode_config(config: &Value) -> Result<(), AppError> {
    let path = get_opencode_config_path();
    write_json_file(&path, config)?;

    log::debug!("OpenCode config written to {path:?}");
    Ok(())
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_opencode_config(&config)
}

pub fn get_typed_providers() -> Result<IndexMap<String, OpenCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &OpenCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_opencode_config(&config)
}

pub fn add_plugin(plugin_name: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    let plugins = config.get_mut("plugin").and_then(|v| v.as_array_mut());

    match plugins {
        Some(arr) => {
            // Mutual exclusion: standard OMO and OMO Slim cannot coexist as plugins
            if plugin_name.starts_with("oh-my-opencode")
                && !plugin_name.starts_with("oh-my-opencode-slim")
            {
                // Adding standard OMO -> remove all Slim variants
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| !s.starts_with("oh-my-opencode-slim"))
                        .unwrap_or(true)
                });
            } else if plugin_name.starts_with("oh-my-opencode-slim") {
                // Adding Slim -> remove all standard OMO variants (but keep slim)
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| {
                            !s.starts_with("oh-my-opencode") || s.starts_with("oh-my-opencode-slim")
                        })
                        .unwrap_or(true)
                });
            }

            let already_exists = arr.iter().any(|v| v.as_str() == Some(plugin_name));
            if !already_exists {
                arr.push(Value::String(plugin_name.to_string()));
            }
        }
        None => {
            config["plugin"] = json!([plugin_name]);
        }
    }

    write_opencode_config(&config)
}

pub fn remove_plugin_by_prefix(prefix: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(arr) = config.get_mut("plugin").and_then(|v| v.as_array_mut()) {
        arr.retain(|v| {
            v.as_str()
                .map(|s| {
                    if !s.starts_with(prefix) {
                        return true; // Keep: doesn't match prefix at all
                    }
                    let rest = &s[prefix.len()..];
                    rest.starts_with('-')
                })
                .unwrap_or(true)
        });

        if arr.is_empty() {
            config.as_object_mut().map(|obj| obj.remove("plugin"));
        }
    }

    write_opencode_config(&config)
}

// ============================================================================
// OpenCode Model Picker Support
// ============================================================================

/// OpenCode model information for model picker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub description: String,
    pub context_window: i64,
    pub max_output_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_cost_per_token: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_cost_per_token: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_for: Option<Vec<String>>,
}

/// OpenCode model picker with predefined model selections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeModelPicker {
    pub provider_id: String,
    pub provider_name: String,
    pub models: Vec<OpenCodeModelInfo>,
    pub default_model: String,
}

/// OpenCode provider config with model picker support
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenCodeProviderWithModels {
    #[serde(default)]
    pub id: String,

    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub settings_config: HashMap<String, Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_index: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,

    /// Model picker array (exactly 5 models for consistency with ii-agent)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<OpenCodeModelInfo>>,
}

/// Get provider with model picker array
pub fn get_provider_with_models(id: &str) -> Result<Option<OpenCodeProviderWithModels>, AppError> {
    let providers = get_typed_providers()?;
    
    // Try to find the provider
    if let Some(config) = providers.get(id) {
        let mut provider_with_models = OpenCodeProviderWithModels {
            id: id.to_string(),
            name: config.name.clone().unwrap_or_default(),
            settings_config: HashMap::new(),
            website_url: None,
            category: None,
            notes: None,
            icon: None,
            icon_color: None,
            sort_index: None,
            created_at: None,
            models: None,
        };

        // Convert OpenCodeProviderConfig to HashMap
        provider_with_models.settings_config = serde_json::to_value(config)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .map(|obj| obj.into_iter().collect())
            .unwrap_or_default();

        return Ok(Some(provider_with_models));
    }

    Ok(None)
}

/// Get all providers with their model arrays
pub fn get_all_providers_with_models() -> Result<Vec<OpenCodeProviderWithModels>, AppError> {
    let providers = get_typed_providers()?;
    let mut result = Vec::new();

    for (id, config) in providers {
        let mut provider_with_models = OpenCodeProviderWithModels {
            id: id.clone(),
            name: config.name.clone().unwrap_or_default(),
            settings_config: serde_json::to_value(&config)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .map(|obj| obj.into_iter().collect())
                .unwrap_or_default(),
            website_url: None,
            category: None,
            notes: None,
            icon: None,
            icon_color: None,
            sort_index: None,
            created_at: None,
            models: None,
        };

        // Add models from the config if present
        if !config.models.is_empty() {
            let mut models = Vec::new();
            for (model_id, model_info) in &config.models {
                models.push(OpenCodeModelInfo {
                    id: model_id.clone(),
                    name: model_info.name.clone(),
                    provider: config.name.clone().unwrap_or_else(|| "Unknown".to_string()),
                    description: format!("{} model", model_info.name),
                    context_window: model_info.limit.as_ref().and_then(|l| l.context).map(|c| c as i64).unwrap_or(128000),
                    max_output_tokens: model_info.limit.as_ref().and_then(|l| l.output).map(|o| o as i64).unwrap_or(4096),
                    input_cost_per_token: None,
                    output_cost_per_token: None,
                    capabilities: None,
                    recommended_for: None,
                });
            }
            if !models.is_empty() {
                provider_with_models.models = Some(models);
            }
        }

        result.push(provider_with_models);
    }

    Ok(result)
}

/// Get predefined model pickers for common OpenCode providers
pub fn get_model_picker(provider_id: &str) -> Option<OpenCodeModelPicker> {
    match provider_id {
        "openrouter" | "openrouter-cc" => Some(get_openrouter_models()),
        "anthropic" | "anthropic-cc" => Some(get_anthropic_models()),
        "openai" | "openai-cc" => Some(get_openai_models()),
        "google" | "google-cc" => Some(get_google_models()),
        "deepseek" | "deepseek-cc" => Some(get_deepseek_models()),
        _ => None,
    }
}

fn get_openrouter_models() -> OpenCodeModelPicker {
    OpenCodeModelPicker {
        provider_id: "openrouter".to_string(),
        provider_name: "OpenRouter".to_string(),
        default_model: "anthropic/claude-3.5-sonnet".to_string(),
        models: vec![
            OpenCodeModelInfo {
                id: "anthropic/claude-3.5-sonnet".to_string(),
                name: "Claude 3.5 Sonnet".to_string(),
                provider: "Anthropic".to_string(),
                description: "Best overall performance for most tasks".to_string(),
                context_window: 200000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.000003),
                output_cost_per_token: Some(0.000015),
                capabilities: Some(vec!["vision".into(), "function_calling".into(), "reasoning".into()]),
                recommended_for: Some(vec!["coding".into(), "chat".into(), "analysis".into()]),
            },
            OpenCodeModelInfo {
                id: "openai/gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: "OpenAI".to_string(),
                description: "Fast and capable multimodal model".to_string(),
                context_window: 128000,
                max_output_tokens: 4096,
                input_cost_per_token: Some(0.000005),
                output_cost_per_token: Some(0.000015),
                capabilities: Some(vec!["vision".into(), "function_calling".into()]),
                recommended_for: Some(vec!["chat".into(), "vision".into(), "multilingual".into()]),
            },
            OpenCodeModelInfo {
                id: "google/gemini-pro-1.5".to_string(),
                name: "Gemini 1.5 Pro".to_string(),
                provider: "Google".to_string(),
                description: "Largest context window with strong reasoning".to_string(),
                context_window: 1048576,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.0000035),
                output_cost_per_token: Some(0.0000105),
                capabilities: Some(vec!["vision".into(), "long_context".into()]),
                recommended_for: Some(vec!["analysis".into(), "summarization".into(), "research".into()]),
            },
            OpenCodeModelInfo {
                id: "meta-llama/llama-3.1-405b-instruct".to_string(),
                name: "Llama 3.1 405B".to_string(),
                provider: "Meta".to_string(),
                description: "Most powerful open source model".to_string(),
                context_window: 128000,
                max_output_tokens: 4096,
                input_cost_per_token: Some(0.000005),
                output_cost_per_token: Some(0.000005),
                capabilities: Some(vec!["function_calling".into(), "reasoning".into()]),
                recommended_for: Some(vec!["coding".into(), "analysis".into(), "open_source".into()]),
            },
            OpenCodeModelInfo {
                id: "mistralai/mistral-large-2411".to_string(),
                name: "Mistral Large".to_string(),
                provider: "Mistral".to_string(),
                description: "European model with multilingual excellence".to_string(),
                context_window: 128000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.000004),
                output_cost_per_token: Some(0.000012),
                capabilities: Some(vec!["function_calling".into(), "multilingual".into()]),
                recommended_for: Some(vec!["multilingual".into(), "coding".into(), "european".into()]),
            },
        ],
    }
}

fn get_anthropic_models() -> OpenCodeModelPicker {
    OpenCodeModelPicker {
        provider_id: "anthropic".to_string(),
        provider_name: "Anthropic".to_string(),
        default_model: "claude-sonnet-4-20250514".to_string(),
        models: vec![
            OpenCodeModelInfo {
                id: "claude-sonnet-4-20250514".to_string(),
                name: "Claude Sonnet 4".to_string(),
                provider: "Anthropic".to_string(),
                description: "Latest Sonnet with balanced performance".to_string(),
                context_window: 200000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.000003),
                output_cost_per_token: Some(0.000015),
                capabilities: Some(vec!["vision".into(), "function_calling".into(), "reasoning".into()]),
                recommended_for: Some(vec!["coding".into(), "chat".into(), "analysis".into()]),
            },
            OpenCodeModelInfo {
                id: "claude-opus-4-20250514".to_string(),
                name: "Claude Opus 4".to_string(),
                provider: "Anthropic".to_string(),
                description: "Most powerful for complex tasks".to_string(),
                context_window: 200000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.000015),
                output_cost_per_token: Some(0.000075),
                capabilities: Some(vec!["vision".into(), "function_calling".into(), "reasoning".into()]),
                recommended_for: Some(vec!["analysis".into(), "research".into(), "complex_tasks".into()]),
            },
            OpenCodeModelInfo {
                id: "claude-3-5-haiku-20241022".to_string(),
                name: "Claude 3.5 Haiku".to_string(),
                provider: "Anthropic".to_string(),
                description: "Fast and cost-effective".to_string(),
                context_window: 200000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.000001),
                output_cost_per_token: Some(0.000005),
                capabilities: Some(vec!["vision".into(), "function_calling".into()]),
                recommended_for: Some(vec!["chat".into(), "cost_effective".into(), "fast_response".into()]),
            },
            OpenCodeModelInfo {
                id: "claude-3-haiku-20240307".to_string(),
                name: "Claude Haiku 3".to_string(),
                provider: "Anthropic".to_string(),
                description: "Ultra-fast for simple tasks".to_string(),
                context_window: 200000,
                max_output_tokens: 4096,
                input_cost_per_token: Some(0.00000025),
                output_cost_per_token: Some(0.00000125),
                capabilities: Some(vec!["vision".into(), "function_calling".into()]),
                recommended_for: Some(vec!["chat".into(), "simple_tasks".into(), "batch_processing".into()]),
            },
            OpenCodeModelInfo {
                id: "claude-3-opus-20240229".to_string(),
                name: "Claude Opus 3".to_string(),
                provider: "Anthropic".to_string(),
                description: "Previous generation flagship".to_string(),
                context_window: 200000,
                max_output_tokens: 4096,
                input_cost_per_token: Some(0.000015),
                output_cost_per_token: Some(0.000075),
                capabilities: Some(vec!["vision".into(), "function_calling".into(), "reasoning".into()]),
                recommended_for: Some(vec!["analysis".into(), "legacy_support".into()]),
            },
        ],
    }
}

fn get_openai_models() -> OpenCodeModelPicker {
    OpenCodeModelPicker {
        provider_id: "openai".to_string(),
        provider_name: "OpenAI".to_string(),
        default_model: "gpt-4.1".to_string(),
        models: vec![
            OpenCodeModelInfo {
                id: "gpt-4.1".to_string(),
                name: "GPT-4.1".to_string(),
                provider: "OpenAI".to_string(),
                description: "Latest GPT-4 with improved capabilities".to_string(),
                context_window: 128000,
                max_output_tokens: 4096,
                input_cost_per_token: Some(0.000005),
                output_cost_per_token: Some(0.000015),
                capabilities: Some(vec!["vision".into(), "function_calling".into(), "reasoning".into()]),
                recommended_for: Some(vec!["coding".into(), "chat".into(), "analysis".into()]),
            },
            OpenCodeModelInfo {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: "OpenAI".to_string(),
                description: "Fast multimodal flagship".to_string(),
                context_window: 128000,
                max_output_tokens: 4096,
                input_cost_per_token: Some(0.000005),
                output_cost_per_token: Some(0.000015),
                capabilities: Some(vec!["vision".into(), "function_calling".into()]),
                recommended_for: Some(vec!["vision".into(), "chat".into(), "multilingual".into()]),
            },
            OpenCodeModelInfo {
                id: "gpt-4o-mini".to_string(),
                name: "GPT-4o Mini".to_string(),
                provider: "OpenAI".to_string(),
                description: "Cost-effective with strong performance".to_string(),
                context_window: 128000,
                max_output_tokens: 4096,
                input_cost_per_token: Some(0.00000015),
                output_cost_per_token: Some(0.0000006),
                capabilities: Some(vec!["vision".into(), "function_calling".into()]),
                recommended_for: Some(vec!["chat".into(), "cost_effective".into(), "high_volume".into()]),
            },
            OpenCodeModelInfo {
                id: "o1".to_string(),
                name: "o1".to_string(),
                provider: "OpenAI".to_string(),
                description: "Advanced reasoning model".to_string(),
                context_window: 128000,
                max_output_tokens: 4096,
                input_cost_per_token: Some(0.000015),
                output_cost_per_token: Some(0.00006),
                capabilities: Some(vec!["reasoning".into(), "math".into(), "science".into()]),
                recommended_for: Some(vec!["reasoning".into(), "math".into(), "science".into()]),
            },
            OpenCodeModelInfo {
                id: "o3-mini".to_string(),
                name: "o3 Mini".to_string(),
                provider: "OpenAI".to_string(),
                description: "Fast reasoning model".to_string(),
                context_window: 128000,
                max_output_tokens: 4096,
                input_cost_per_token: Some(0.0000011),
                output_cost_per_token: Some(0.0000044),
                capabilities: Some(vec!["reasoning".into(), "coding".into()]),
                recommended_for: Some(vec!["coding".into(), "reasoning".into(), "cost_effective".into()]),
            },
        ],
    }
}

fn get_google_models() -> OpenCodeModelPicker {
    OpenCodeModelPicker {
        provider_id: "google".to_string(),
        provider_name: "Google".to_string(),
        default_model: "gemini-2.5-pro".to_string(),
        models: vec![
            OpenCodeModelInfo {
                id: "gemini-2.5-pro".to_string(),
                name: "Gemini 2.5 Pro".to_string(),
                provider: "Google".to_string(),
                description: "Latest Pro model with advanced reasoning".to_string(),
                context_window: 1048576,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.0000035),
                output_cost_per_token: Some(0.0000105),
                capabilities: Some(vec!["vision".into(), "long_context".into(), "reasoning".into()]),
                recommended_for: Some(vec!["analysis".into(), "research".into(), "multimodal".into()]),
            },
            OpenCodeModelInfo {
                id: "gemini-2.5-flash".to_string(),
                name: "Gemini 2.5 Flash".to_string(),
                provider: "Google".to_string(),
                description: "Fast and efficient model".to_string(),
                context_window: 1048576,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.0000003),
                output_cost_per_token: Some(0.0000012),
                capabilities: Some(vec!["vision".into(), "long_context".into()]),
                recommended_for: Some(vec!["chat".into(), "cost_effective".into(), "high_volume".into()]),
            },
            OpenCodeModelInfo {
                id: "gemini-2.0-flash".to_string(),
                name: "Gemini 2.0 Flash".to_string(),
                provider: "Google".to_string(),
                description: "Previous generation fast model".to_string(),
                context_window: 1048576,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.00000015),
                output_cost_per_token: Some(0.0000006),
                capabilities: Some(vec!["vision".into()]),
                recommended_for: Some(vec!["chat".into(), "legacy_support".into()]),
            },
            OpenCodeModelInfo {
                id: "gemini-2.0-flash-lite".to_string(),
                name: "Gemini 2.0 Flash Lite".to_string(),
                provider: "Google".to_string(),
                description: "Most cost-effective Google model".to_string(),
                context_window: 1048576,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.000000075),
                output_cost_per_token: Some(0.0000003),
                capabilities: Some(vec!["vision".into()]),
                recommended_for: Some(vec!["cost_effective".into(), "batch_processing".into()]),
            },
            OpenCodeModelInfo {
                id: "gemini-1.5-pro".to_string(),
                name: "Gemini 1.5 Pro".to_string(),
                provider: "Google".to_string(),
                description: "Previous Pro with large context".to_string(),
                context_window: 1048576,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.0000035),
                output_cost_per_token: Some(0.0000105),
                capabilities: Some(vec!["vision".into(), "long_context".into()]),
                recommended_for: Some(vec!["analysis".into(), "legacy_support".into()]),
            },
        ],
    }
}

fn get_deepseek_models() -> OpenCodeModelPicker {
    OpenCodeModelPicker {
        provider_id: "deepseek".to_string(),
        provider_name: "DeepSeek".to_string(),
        default_model: "deepseek-chat".to_string(),
        models: vec![
            OpenCodeModelInfo {
                id: "deepseek-chat".to_string(),
                name: "DeepSeek Chat".to_string(),
                provider: "DeepSeek".to_string(),
                description: "Best for conversational tasks".to_string(),
                context_window: 128000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.00000027),
                output_cost_per_token: Some(0.0000011),
                capabilities: Some(vec!["function_calling".into(), "coding".into()]),
                recommended_for: Some(vec!["chat".into(), "coding".into(), "cost_effective".into()]),
            },
            OpenCodeModelInfo {
                id: "deepseek-coder".to_string(),
                name: "DeepSeek Coder".to_string(),
                provider: "DeepSeek".to_string(),
                description: "Specialized for code generation".to_string(),
                context_window: 128000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.00000027),
                output_cost_per_token: Some(0.0000011),
                capabilities: Some(vec!["coding".into(), "function_calling".into()]),
                recommended_for: Some(vec!["coding".into(), "code_review".into(), "debugging".into()]),
            },
            OpenCodeModelInfo {
                id: "deepseek-reasoner".to_string(),
                name: "DeepSeek Reasoner".to_string(),
                provider: "DeepSeek".to_string(),
                description: "Enhanced reasoning capabilities".to_string(),
                context_window: 64000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.00000055),
                output_cost_per_token: Some(0.0000022),
                capabilities: Some(vec!["reasoning".into(), "math".into()]),
                recommended_for: Some(vec!["reasoning".into(), "math".into(), "analysis".into()]),
            },
            OpenCodeModelInfo {
                id: "deepseek-v3".to_string(),
                name: "DeepSeek V3".to_string(),
                provider: "DeepSeek".to_string(),
                description: "Latest general purpose model".to_string(),
                context_window: 128000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.00000027),
                output_cost_per_token: Some(0.0000011),
                capabilities: Some(vec!["function_calling".into(), "vision".into()]),
                recommended_for: Some(vec!["chat".into(), "analysis".into(), "multimodal".into()]),
            },
            OpenCodeModelInfo {
                id: "deepseek-v2.5".to_string(),
                name: "DeepSeek V2.5".to_string(),
                provider: "DeepSeek".to_string(),
                description: "Previous generation balanced model".to_string(),
                context_window: 128000,
                max_output_tokens: 8192,
                input_cost_per_token: Some(0.00000014),
                output_cost_per_token: Some(0.00000056),
                capabilities: Some(vec!["function_calling".into()]),
                recommended_for: Some(vec!["cost_effective".into(), "legacy_support".into()]),
            },
        ],
    }
}

/// Set provider models array
pub fn set_provider_models(
    id: &str,
    models: Vec<OpenCodeModelInfo>,
) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if config.get("provider").is_none() {
        config["provider"] = json!({});
    }

    if let Some(providers) = config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        if let Some(provider) = providers.get_mut(id) {
            // Add models array to provider config
            let models_value = serde_json::to_value(&models)
                .map_err(|e| AppError::JsonSerialize { source: e })?;
            
            // Store models in a special _models field for cc-switch management
            if let Some(provider_obj) = provider.as_object_mut() {
                provider_obj.insert("_models".to_string(), models_value);
            }
        }
    }

    write_opencode_config(&config)
}
