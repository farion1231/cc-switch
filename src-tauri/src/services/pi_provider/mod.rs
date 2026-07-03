use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug)]
pub struct PiProviderError {
    message: String,
}

impl PiProviderError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PiProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PiProviderError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PiProviderMode {
    Custom,
    BuiltinOverride,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PiProviderTemplate {
    OpenAiCompatible,
    OpenAiResponses,
    AnthropicCompatible,
    GoogleGenerativeAi,
    LocalOpenAiCompatible,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PiApiKeyMode {
    Env,
    Literal,
    Command,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiApiKeyDraft {
    pub mode: PiApiKeyMode,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiHeaderDraft {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiModelCost {
    pub input: f64,
    pub output: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiModelDraft {
    pub id: String,
    pub name: Option<String>,
    pub name_touched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<PiModelCost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiProviderCompat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_developer_role: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_reasoning_effort: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_usage_in_streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens_field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_eager_tool_input_streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_long_cache_retention: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_adaptive_thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_empty_signature: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiProviderDraft {
    pub mode: PiProviderMode,
    pub provider_id: String,
    pub template: PiProviderTemplate,
    pub base_url: Option<String>,
    pub api: String,
    pub api_key: PiApiKeyDraft,
    pub headers: Vec<PiHeaderDraft>,
    pub models: Vec<PiModelDraft>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compat: Option<PiProviderCompat>,
    pub advanced_json: Option<Value>,
}

pub fn draft_to_provider_value(draft: &PiProviderDraft) -> Result<Value, PiProviderError> {
    validate_draft(draft)?;

    let mut obj = Map::new();

    if let Some(base_url) = draft
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        obj.insert("baseUrl".to_string(), json!(base_url));
    }
    if !draft.api.trim().is_empty() {
        obj.insert("api".to_string(), json!(draft.api.trim()));
    }
    if let Some(api_key) = render_api_key(&draft.api_key)? {
        obj.insert("apiKey".to_string(), json!(api_key));
    }

    let headers = render_headers(&draft.headers)?;
    if !headers.is_empty() {
        obj.insert("headers".to_string(), Value::Object(headers));
    }

    // Compat flags
    if let Some(compat) = &draft.compat {
        let compat_obj = render_compat(compat);
        if !compat_obj.is_empty() {
            obj.insert("compat".to_string(), Value::Object(compat_obj));
        }
    }

    // Models: always include for custom mode, include for builtin override if non-empty
    if draft.mode == PiProviderMode::Custom || !draft.models.is_empty() {
        let rendered = render_models(&draft.models)?;
        if !rendered.as_array().is_none_or(|a| a.is_empty()) {
            obj.insert("models".to_string(), rendered);
        }
    }

    if let Some(advanced) = &draft.advanced_json {
        merge_advanced_json(&mut obj, advanced)?;
    }

    Ok(Value::Object(obj))
}

pub fn validate_draft(draft: &PiProviderDraft) -> Result<(), PiProviderError> {
    validate_provider_id(&draft.provider_id)?;
    if draft.mode == PiProviderMode::Custom {
        if draft.base_url.as_deref().unwrap_or("").trim().is_empty() {
            return Err(PiProviderError::new(
                "baseUrl is required for custom providers",
            ));
        }
        if draft.api.trim().is_empty() {
            return Err(PiProviderError::new("api is required for custom providers"));
        }
        if draft.models.is_empty() || draft.models.iter().all(|m| m.id.trim().is_empty()) {
            return Err(PiProviderError::new(
                "at least one model is required for custom providers",
            ));
        }
    }
    Ok(())
}

fn validate_provider_id(provider_id: &str) -> Result<(), PiProviderError> {
    let trimmed = provider_id.trim();
    if trimmed.is_empty() {
        return Err(PiProviderError::new("providerId is required"));
    }
    if trimmed
        .chars()
        .any(|c| c.is_whitespace() || c == '/' || c == '\\' || c.is_control())
    {
        return Err(PiProviderError::new(
            "providerId contains invalid characters",
        ));
    }
    Ok(())
}

fn render_api_key(api_key: &PiApiKeyDraft) -> Result<Option<String>, PiProviderError> {
    let value = api_key.value.trim();
    match api_key.mode {
        PiApiKeyMode::None => Ok(None),
        PiApiKeyMode::Env => {
            if value.is_empty() {
                return Err(PiProviderError::new(
                    "environment variable name is required",
                ));
            }
            // Allow valid env var names (letters, digits, underscores)
            if !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(PiProviderError::new("environment variable name is invalid"));
            }
            Ok(Some(format!("${value}")))
        }
        PiApiKeyMode::Literal => {
            if value.is_empty() {
                return Err(PiProviderError::new("literal apiKey is required"));
            }
            Ok(Some(value.to_string()))
        }
        PiApiKeyMode::Command => {
            if !value.starts_with('!') {
                return Err(PiProviderError::new("command apiKey must start with !"));
            }
            Ok(Some(value.to_string()))
        }
    }
}

fn render_headers(headers: &[PiHeaderDraft]) -> Result<Map<String, Value>, PiProviderError> {
    let mut rendered = Map::new();
    for header in headers {
        let key = header.key.trim();
        if key.is_empty() {
            continue; // Skip empty headers silently
        }
        rendered.insert(key.to_string(), json!(header.value.trim()));
    }
    Ok(rendered)
}

fn render_compat(compat: &PiProviderCompat) -> Map<String, Value> {
    let mut obj = Map::new();

    if let Some(v) = compat.supports_developer_role {
        obj.insert("supportsDeveloperRole".to_string(), json!(v));
    }
    if let Some(v) = compat.supports_reasoning_effort {
        obj.insert("supportsReasoningEffort".to_string(), json!(v));
    }
    if let Some(v) = compat.supports_usage_in_streaming {
        obj.insert("supportsUsageInStreaming".to_string(), json!(v));
    }
    if let Some(v) = &compat.max_tokens_field {
        obj.insert("maxTokensField".to_string(), json!(v));
    }
    if let Some(v) = &compat.thinking_format {
        obj.insert("thinkingFormat".to_string(), json!(v));
    }
    if let Some(v) = compat.supports_eager_tool_input_streaming {
        obj.insert("supportsEagerToolInputStreaming".to_string(), json!(v));
    }
    if let Some(v) = compat.supports_long_cache_retention {
        obj.insert("supportsLongCacheRetention".to_string(), json!(v));
    }
    if let Some(v) = compat.force_adaptive_thinking {
        obj.insert("forceAdaptiveThinking".to_string(), json!(v));
    }
    if let Some(v) = compat.allow_empty_signature {
        obj.insert("allowEmptySignature".to_string(), json!(v));
    }

    obj
}

fn render_models(models: &[PiModelDraft]) -> Result<Value, PiProviderError> {
    let mut seen = std::collections::HashSet::new();
    let mut rendered = Vec::new();
    for model in models {
        let id = model.id.trim();
        if id.is_empty() {
            continue; // Skip empty model entries
        }
        if !seen.insert(id.to_string()) {
            return Err(PiProviderError::new(format!("duplicate model id: {id}")));
        }
        let name = model.name.as_deref().unwrap_or(id).trim();
        let mut entry = Map::new();
        entry.insert("id".to_string(), json!(id));
        if !name.is_empty() && name != id {
            entry.insert("name".to_string(), json!(name));
        }
        if model.reasoning == Some(true) {
            entry.insert("reasoning".to_string(), json!(true));
        }
        if let Some(input) = &model.input {
            if !input.is_empty() && input != &["text"] {
                entry.insert("input".to_string(), json!(input));
            }
        }
        if let Some(ctx) = model.context_window {
            if ctx != 128000 {
                entry.insert("contextWindow".to_string(), json!(ctx));
            }
        }
        if let Some(max) = model.max_tokens {
            if max != 16384 {
                entry.insert("maxTokens".to_string(), json!(max));
            }
        }
        if let Some(cost) = &model.cost {
            let mut cost_obj = Map::new();
            cost_obj.insert("input".to_string(), json!(cost.input));
            cost_obj.insert("output".to_string(), json!(cost.output));
            if let Some(cr) = cost.cache_read {
                cost_obj.insert("cacheRead".to_string(), json!(cr));
            }
            if let Some(cw) = cost.cache_write {
                cost_obj.insert("cacheWrite".to_string(), json!(cw));
            }
            entry.insert("cost".to_string(), Value::Object(cost_obj));
        }
        rendered.push(Value::Object(entry));
    }
    Ok(Value::Array(rendered))
}

fn merge_advanced_json(
    target: &mut Map<String, Value>,
    advanced: &Value,
) -> Result<(), PiProviderError> {
    let Some(advanced_obj) = advanced.as_object() else {
        return Err(PiProviderError::new("advancedJson must be an object"));
    };
    for managed in ["baseUrl", "api", "apiKey", "headers", "models", "compat"] {
        if advanced_obj.contains_key(managed) {
            return Err(PiProviderError::new(format!(
                "advancedJson cannot override {managed}"
            )));
        }
    }
    for (key, value) in advanced_obj {
        target.insert(key.clone(), value.clone());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiProviderPatchPreview {
    pub next_models_json: Value,
    pub current_file_hash: String,
    pub summary: Vec<String>,
}

pub fn upsert_provider_value(
    mut current: Value,
    draft: &PiProviderDraft,
) -> Result<Value, PiProviderError> {
    let provider_value = draft_to_provider_value(draft)?;
    if current.get("providers").is_none() {
        current["providers"] = json!({});
    }
    let Some(providers) = current.get_mut("providers").and_then(|v| v.as_object_mut()) else {
        return Err(PiProviderError::new(
            "models.json providers must be an object",
        ));
    };
    providers.insert(draft.provider_id.trim().to_string(), provider_value);
    Ok(current)
}

pub fn delete_provider_value(
    mut current: Value,
    provider_id: &str,
) -> Result<Value, PiProviderError> {
    if current.get("providers").is_none() {
        current["providers"] = json!({});
    }
    let Some(providers) = current.get_mut("providers").and_then(|v| v.as_object_mut()) else {
        return Err(PiProviderError::new(
            "models.json providers must be an object",
        ));
    };
    providers.remove(provider_id);
    Ok(current)
}

pub fn build_upsert_preview(
    current: crate::pi_config::PiModelsJson,
    draft: &PiProviderDraft,
) -> Result<PiProviderPatchPreview, PiProviderError> {
    let next = upsert_provider_value(current.value, draft)?;

    // Build a descriptive summary
    let model_count = draft
        .models
        .iter()
        .filter(|m| !m.id.trim().is_empty())
        .count();
    let mut summary = vec![format!(
        "Upsert provider \"{}\" ({} api, {} model{})",
        draft.provider_id.trim(),
        draft.api,
        model_count,
        if model_count != 1 { "s" } else { "" },
    )];
    if let Some(base_url) = draft.base_url.as_deref().filter(|s| !s.trim().is_empty()) {
        summary.push(format!("Base URL: {}", base_url.trim()));
    }
    for model in &draft.models {
        if !model.id.trim().is_empty() {
            let reasoning_tag = if model.reasoning == Some(true) {
                " [reasoning]"
            } else {
                ""
            };
            summary.push(format!("  + model: {}{}", model.id.trim(), reasoning_tag));
        }
    }

    Ok(PiProviderPatchPreview {
        next_models_json: next,
        current_file_hash: current.file_hash,
        summary,
    })
}
