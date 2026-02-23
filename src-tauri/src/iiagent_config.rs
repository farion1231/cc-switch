use crate::config::write_json_file;
use crate::error::AppError;
use crate::settings::get_iiagent_override_dir;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn get_iiagent_dir() -> PathBuf {
    if let Some(override_dir) = get_iiagent_override_dir() {
        return override_dir;
    }

    dirs::home_dir()
        .map(|h| h.join(".ii-agent").join("providers"))
        .unwrap_or_else(|| PathBuf::from(".ii-agent").join("providers"))
}

pub fn get_iiagent_config_path() -> PathBuf {
    get_iiagent_dir().join("providers.json")
}

pub fn read_iiagent_config() -> Result<Value, AppError> {
    let path = get_iiagent_config_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))
}

pub fn write_iiagent_config(config: &Value) -> Result<(), AppError> {
    let path = get_iiagent_config_path();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    write_json_file(&path, config)?;

    log::debug!("IIAgent config written to {path:?}");
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IIAgentAPIFormat {
    Anthropic,
    OpenaiChat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IIAgentModelInfo {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IIAgentModelPicker {
    pub provider_id: String,
    pub provider_name: String,
    pub models: Vec<IIAgentModelInfo>,  // Exactly 5 models
    pub default_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IIAgentProviderConfig {
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
    pub meta: Option<IIAgentProviderMeta>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_index: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<IIAgentModelInfo>>,  // Array of 5 models for Tauri proxy
}

impl Default for IIAgentProviderConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            settings_config: HashMap::new(),
            website_url: None,
            category: None,
            notes: None,
            icon: None,
            icon_color: None,
            meta: None,
            sort_index: None,
            created_at: None,
            models: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IIAgentProviderMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_format: Option<IIAgentAPIFormat>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_new_api: Option<bool>,

    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_iiagent_config()?;
    Ok(config.as_object().cloned().unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_iiagent_config()?;

    if let Some(obj) = full_config.as_object_mut() {
        obj.insert(id.to_string(), config);
    }

    write_iiagent_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_iiagent_config()?;

    if let Some(obj) = config.as_object_mut() {
        obj.remove(id);
    }

    write_iiagent_config(&config)
}

pub fn get_typed_providers() -> Result<IndexMap<String, IIAgentProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        if id.starts_with("_app_") {
            continue;
        }
        match serde_json::from_value::<IIAgentProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse IIAgent provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &IIAgentProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

/// Get provider with model picker array (exactly 5 models)
pub fn get_provider_with_models(id: &str) -> Result<Option<IIAgentProviderConfig>, AppError> {
    let providers = get_typed_providers()?;
    Ok(providers.get(id).cloned())
}

/// Get all providers with their model arrays
pub fn get_all_providers_with_models() -> Result<Vec<IIAgentProviderConfig>, AppError> {
    let providers = get_typed_providers()?;
    Ok(providers.values().cloned().collect())
}

/// Add model picker array to a provider (exactly 5 models)
pub fn set_provider_models(
    id: &str,
    models: Vec<IIAgentModelInfo>,
) -> Result<(), AppError> {
    let mut provider = get_provider_with_models(id)?
        .unwrap_or_default();
    
    provider.models = Some(models);
    set_typed_provider(id, &provider)
}

/// Get predefined model pickers for common providers
pub fn get_model_picker(provider_id: &str) -> Option<IIAgentModelPicker> {
    match provider_id {
        "openrouter" => Some(get_openrouter_models()),
        "anthropic" => Some(get_anthropic_models()),
        "openai" => Some(get_openai_models()),
        "google" => Some(get_google_models()),
        "deepseek" => Some(get_deepseek_models()),
        _ => None,
    }
}

fn get_openrouter_models() -> IIAgentModelPicker {
    IIAgentModelPicker {
        provider_id: "openrouter".to_string(),
        provider_name: "OpenRouter".to_string(),
        default_model: "anthropic/claude-3.5-sonnet".to_string(),
        models: vec![
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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

fn get_anthropic_models() -> IIAgentModelPicker {
    IIAgentModelPicker {
        provider_id: "anthropic".to_string(),
        provider_name: "Anthropic".to_string(),
        default_model: "claude-sonnet-4-20250514".to_string(),
        models: vec![
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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

fn get_openai_models() -> IIAgentModelPicker {
    IIAgentModelPicker {
        provider_id: "openai".to_string(),
        provider_name: "OpenAI".to_string(),
        default_model: "gpt-4.1".to_string(),
        models: vec![
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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

fn get_google_models() -> IIAgentModelPicker {
    IIAgentModelPicker {
        provider_id: "google".to_string(),
        provider_name: "Google".to_string(),
        default_model: "gemini-2.5-pro".to_string(),
        models: vec![
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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

fn get_deepseek_models() -> IIAgentModelPicker {
    IIAgentModelPicker {
        provider_id: "deepseek".to_string(),
        provider_name: "DeepSeek".to_string(),
        default_model: "deepseek-chat".to_string(),
        models: vec![
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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
            IIAgentModelInfo {
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

pub fn get_current_provider() -> Result<Option<String>, AppError> {
    let config = read_iiagent_config()?;
    Ok(config
        .get("_app_ii-agent")
        .and_then(|v| v.get("current_provider"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

pub fn set_current_provider(provider_id: &str) -> Result<(), AppError> {
    let mut config = read_iiagent_config()?;

    if let Some(obj) = config.as_object_mut() {
        obj.insert(
            "_app_ii-agent".to_string(),
            json!({"current_provider": provider_id}),
        );
    }

    write_iiagent_config(&config)
}
