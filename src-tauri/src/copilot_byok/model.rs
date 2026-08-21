use crate::error::AppError;
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};

pub const LEGACY_MANAGED_PREFIX: &str = "CC Switch:";
const MANAGED_MARKER: &str = "ccSwitchManaged";
const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;
fn default_true() -> bool {
    true
}

fn default_api_type() -> String {
    "chat-completions".to_string()
}

fn is_vscode_secret_reference(value: &str) -> bool {
    value
        .strip_prefix("${input:")
        .and_then(|value| value.strip_suffix('}'))
        .is_some_and(|key| !key.trim().is_empty())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KimiModelFamily {
    K3,
    K27,
    K26,
    K25,
}

fn kimi_model_family(model_id: &str) -> Option<KimiModelFamily> {
    let normalized = model_id.trim().to_ascii_lowercase();
    let model_id = normalized
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(normalized.as_str());
    if matches!(model_id, "k3" | "kimi-k3")
        || model_id.starts_with("k3-")
        || model_id.starts_with("kimi-k3-")
    {
        return Some(KimiModelFamily::K3);
    }
    if model_id.contains("k2.7") || model_id.starts_with("kimi-for-coding") {
        return Some(KimiModelFamily::K27);
    }
    if model_id.contains("k2.6") {
        return Some(KimiModelFamily::K26);
    }
    if model_id.contains("k2.5") {
        return Some(KimiModelFamily::K25);
    }
    None
}

#[derive(Clone, Copy)]
enum MiniMaxModelFamily {
    M3,
    M2,
}

fn minimax_model_family(model_id: &str) -> Option<MiniMaxModelFamily> {
    let normalized = model_id.trim().to_ascii_lowercase();
    let model_id = normalized
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(normalized.as_str());
    let belongs_to_family = |prefix: &str| {
        model_id == prefix
            || model_id
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with('-') || suffix.starts_with('.'))
    };
    if belongs_to_family("minimax-m3") {
        Some(MiniMaxModelFamily::M3)
    } else if belongs_to_family("minimax-m2") {
        Some(MiniMaxModelFamily::M2)
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CopilotByokModel {
    #[serde(default)]
    pub id: String,
    pub model_id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calling: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub edit_tools: Vec<String>,
    #[serde(default)]
    pub zero_data_retention_enabled: bool,
    #[serde(default)]
    pub supports_reasoning_effort: Vec<String>,
    #[serde(default)]
    pub reasoning_effort_format: Option<String>,
    #[serde(default)]
    pub model_options: Value,
    /// Unknown VS Code fields are retained so importing and editing a provider
    /// never silently downgrades a newer chatLanguageModels.json schema.
    #[serde(default)]
    pub extra: BTreeMap<String, Value>,
}

impl CopilotByokModel {
    pub fn normalize(&mut self) {
        self.id = self.id.trim().to_string();
        if self.id.is_empty() {
            self.id = uuid::Uuid::new_v4().to_string();
        }
        self.model_id = self.model_id.trim().to_string();
        self.name = self.name.trim().to_string();
        let mut edit_tools = HashSet::new();
        self.edit_tools = self
            .edit_tools
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty() && edit_tools.insert(value.clone()))
            .collect();
        let mut efforts = HashSet::new();
        self.supports_reasoning_effort = self
            .supports_reasoning_effort
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty() && efforts.insert(value.clone()))
            .collect();
        self.reasoning_effort_format = self
            .reasoning_effort_format
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.context_window = self.context_window.filter(|value| *value > 0);
        self.max_input_tokens = self.max_input_tokens.filter(|value| *value > 0);
        self.max_output_tokens = self.max_output_tokens.filter(|value| *value > 0);
    }

    fn apply_sampling_defaults(&mut self, temperature: f64, top_p: f64, fixed: bool) {
        if self.model_options.is_null() {
            self.model_options = json!({});
        }
        let Some(options) = self.model_options.as_object_mut() else {
            return;
        };
        if fixed {
            options.insert("temperature".to_string(), json!(temperature));
            options.insert("top_p".to_string(), json!(top_p));
        } else {
            options
                .entry("temperature".to_string())
                .or_insert_with(|| json!(temperature));
            options
                .entry("top_p".to_string())
                .or_insert_with(|| json!(top_p));
        }
    }

    fn apply_known_defaults(&mut self, api_type: &str) {
        // Unknown models must still be visible to VS Code's agent-mode model
        // picker. Explicit user overrides, including `false`, remain intact.
        self.tool_calling.get_or_insert(true);

        if let Some(family) = kimi_model_family(&self.model_id) {
            // Kimi's documented model-family contract. The frontend enriches
            // the broader catalog from models.dev; these model-id fallbacks
            // keep imported/offline coding-plan aliases usable too.
            self.vision.get_or_insert(true);
            self.thinking.get_or_insert(true);
            self.streaming.get_or_insert(true);
            match family {
                KimiModelFamily::K3 => {
                    let context_window = if self.model_id.to_ascii_lowercase().contains("256k") {
                        262_144
                    } else {
                        1_000_000
                    };
                    self.context_window.get_or_insert(context_window);
                    if self.supports_reasoning_effort.is_empty() {
                        self.supports_reasoning_effort = ["low", "high", "max"]
                            .into_iter()
                            .map(str::to_string)
                            .collect();
                    }
                    if self.reasoning_effort_format.is_none() {
                        self.reasoning_effort_format = Some(api_type.to_string());
                    }
                    // VS Code otherwise sends its generic top_p=1 default,
                    // which K3 rejects.
                    self.apply_sampling_defaults(1.0, 0.95, true);
                }
                KimiModelFamily::K27 => {
                    self.context_window.get_or_insert(262_144);
                    self.max_output_tokens.get_or_insert(262_144);
                    // K2.7 and its coding-plan aliases have the same fixed
                    // sampling contract as K3, but no reasoning_effort field.
                    self.apply_sampling_defaults(1.0, 0.95, true);
                }
                KimiModelFamily::K26 | KimiModelFamily::K25 => {
                    self.context_window.get_or_insert(262_144);
                }
            }
            return;
        }

        let Some(family) = minimax_model_family(&self.model_id) else {
            // Do not guess any capability beyond tool calling for unknown
            // model families.
            return;
        };
        self.thinking.get_or_insert(true);
        self.streaming.get_or_insert(true);
        match family {
            MiniMaxModelFamily::M3 => {
                self.vision.get_or_insert(true);
                self.context_window.get_or_insert(1_000_000);
                self.max_output_tokens.get_or_insert(524_288);
                self.apply_sampling_defaults(1.0, 0.95, false);
            }
            MiniMaxModelFamily::M2 => {
                self.vision.get_or_insert(false);
                self.context_window.get_or_insert(204_800);
                self.max_output_tokens.get_or_insert(204_800);
                self.apply_sampling_defaults(1.0, 0.9, false);
            }
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.model_id.is_empty() {
            return Err(AppError::InvalidInput(
                "Copilot BYOK model id is required".to_string(),
            ));
        }
        if self.name.is_empty() {
            return Err(AppError::InvalidInput(
                "Copilot BYOK display name is required".to_string(),
            ));
        }
        if !self.model_options.is_null() && !self.model_options.is_object() {
            return Err(AppError::InvalidInput(
                "Copilot BYOK model options must be a JSON object".to_string(),
            ));
        }
        if self
            .context_window
            .is_some_and(|value| value > MAX_SAFE_JSON_INTEGER)
            || self
                .max_output_tokens
                .is_some_and(|value| value > MAX_SAFE_JSON_INTEGER)
            || self
                .max_input_tokens
                .is_some_and(|value| value > MAX_SAFE_JSON_INTEGER)
        {
            return Err(AppError::InvalidInput(
                "Copilot BYOK token limits must be JavaScript-safe integers".to_string(),
            ));
        }
        Ok(())
    }

    fn to_language_model(&self, url: &str, request_headers: &BTreeMap<String, String>) -> Value {
        let mut model: Map<String, Value> = self.extra.clone().into_iter().collect();
        model.insert("id".to_string(), json!(self.model_id));
        model.insert("name".to_string(), json!(self.name));
        model.insert("url".to_string(), json!(url));
        if let Some(tool_calling) = self.tool_calling {
            model.insert("toolCalling".to_string(), json!(tool_calling));
        }
        if let Some(vision) = self.vision {
            model.insert("vision".to_string(), json!(vision));
        }
        if let Some(thinking) = self.thinking {
            model.insert("thinking".to_string(), json!(thinking));
        }
        if let Some(streaming) = self.streaming {
            model.insert("streaming".to_string(), json!(streaming));
        }
        if let Some(context_window) = self.context_window {
            model.insert("contextWindow".to_string(), json!(context_window));
        }
        if let Some(max_output_tokens) = self.max_output_tokens {
            model.insert("maxOutputTokens".to_string(), json!(max_output_tokens));
        }
        if let Some(max_input_tokens) = self.max_input_tokens {
            model.insert("maxInputTokens".to_string(), json!(max_input_tokens));
        }
        if !self.edit_tools.is_empty() {
            model.insert("editTools".to_string(), json!(self.edit_tools));
        }
        if self.zero_data_retention_enabled {
            model.insert("zeroDataRetentionEnabled".to_string(), json!(true));
        }
        if !self.supports_reasoning_effort.is_empty() {
            model.insert(
                "supportsReasoningEffort".to_string(),
                json!(self.supports_reasoning_effort),
            );
        }
        if let Some(format) = &self.reasoning_effort_format {
            model.insert("reasoningEffortFormat".to_string(), json!(format));
        }
        if !request_headers.is_empty() {
            model.insert("requestHeaders".to_string(), json!(request_headers));
        }
        if !self.model_options.is_null() && self.model_options != json!({}) {
            model.insert("modelOptions".to_string(), self.model_options.clone());
        }
        Value::Object(model)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CopilotByokGroup {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_api_type")]
    pub api_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
    /// Provider classification used by first-class catalog entries. Custom
    /// providers normally use `custom`; the fixed Copilot CLI fallback uses
    /// `official`, matching the main provider catalog convention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Optional usage-query configuration. It is stored in the provider row's
    /// metadata and never projected into VS Code or Copilot CLI runtime files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_script: Option<crate::provider::UsageScript>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub request_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub models: Vec<CopilotByokModel>,
    /// Unknown provider-level VS Code fields preserved across import and sync.
    #[serde(default)]
    pub extra: BTreeMap<String, Value>,
}

impl CopilotByokGroup {
    fn effective_request_headers(&self) -> BTreeMap<String, String> {
        // VS Code resolves `${input:...}` references from its own SecretStorage
        // before handing the configuration to the Custom Endpoint provider. Keep
        // `${apiKey}` placeholders intact in that case so the provider can expand
        // them with the resolved secret rather than with the reference text.
        let vscode_resolves_api_key = is_vscode_secret_reference(&self.api_key);
        let mut headers: BTreeMap<String, String> = self
            .request_headers
            .iter()
            .map(|(name, value)| {
                (
                    name.clone(),
                    if vscode_resolves_api_key {
                        value.clone()
                    } else {
                        value.replace("${apiKey}", self.api_key.as_str())
                    },
                )
            })
            .collect();

        // VS Code stores schema-declared secrets in SecretStorage and ignores a raw
        // `apiKey` value written by an external application.  Custom Endpoint does,
        // however, allow auth headers on each static model. Materialize the shared
        // group key there so managed BYOK models work without a local proxy.
        let has_explicit_auth = headers.keys().any(|name| {
            matches!(
                name.trim().to_ascii_lowercase().as_str(),
                "authorization"
                    | "proxy-authorization"
                    | "api-key"
                    | "x-api-key"
                    | "anthropic-api-key"
                    | "x-goog-api-key"
            )
        });
        if !self.api_key.is_empty() && !vscode_resolves_api_key && !has_explicit_auth {
            if self.api_type == "messages" {
                headers.insert("x-api-key".to_string(), self.api_key.clone());
            } else {
                headers.insert(
                    "Authorization".to_string(),
                    format!("Bearer {}", self.api_key),
                );
            }
        }
        if self.api_type == "messages"
            && !headers
                .keys()
                .any(|name| name.eq_ignore_ascii_case("anthropic-version"))
        {
            headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
        }

        headers
    }

    pub fn normalize(&mut self) {
        self.id = self.id.trim().to_string();
        if self.id.is_empty() {
            self.id = uuid::Uuid::new_v4().to_string();
        }
        self.name = self.name.trim().to_string();
        self.url = self.url.trim().to_string();
        self.api_key = self.api_key.trim().to_string();
        self.api_type = self.api_type.trim().to_ascii_lowercase();
        self.website_url = self
            .website_url
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.notes = self
            .notes
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.icon = self
            .icon
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.icon_color = self
            .icon_color
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.category = self
            .category
            .take()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        self.request_headers = std::mem::take(&mut self.request_headers)
            .into_iter()
            .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
            .filter(|(name, _)| !name.is_empty())
            .collect();
        for model in &mut self.models {
            model.normalize();
            model.apply_known_defaults(&self.api_type);
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.name.is_empty() {
            return Err(AppError::InvalidInput(
                "Copilot BYOK provider name is required".to_string(),
            ));
        }
        let parsed = url::Url::parse(&self.url).map_err(|error| {
            AppError::InvalidInput(format!("Invalid Copilot BYOK endpoint URL: {error}"))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(AppError::InvalidInput(
                "Copilot BYOK endpoint must be an absolute HTTP(S) URL".to_string(),
            ));
        }
        if !matches!(
            self.api_type.as_str(),
            "chat-completions" | "responses" | "messages"
        ) {
            return Err(AppError::InvalidInput(format!(
                "Unsupported Copilot BYOK API type: {}",
                self.api_type
            )));
        }
        if self.api_key.contains(['\r', '\n']) {
            return Err(AppError::InvalidInput(
                "Copilot BYOK API key must not contain newlines".to_string(),
            ));
        }
        if let Some(website_url) = self.website_url.as_deref() {
            let parsed = url::Url::parse(website_url).map_err(|error| {
                AppError::InvalidInput(format!("Invalid provider website URL: {error}"))
            })?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err(AppError::InvalidInput(
                    "Provider website must be an absolute HTTP(S) URL".to_string(),
                ));
            }
        }
        if self.models.is_empty() {
            return Err(AppError::InvalidInput(
                "Copilot BYOK provider must contain at least one model".to_string(),
            ));
        }
        let mut normalized_header_names = HashSet::new();
        for (name, value) in &self.request_headers {
            if name.trim().is_empty() || name.contains(['\r', '\n']) || value.contains(['\r', '\n'])
            {
                return Err(AppError::InvalidInput(
                    "Copilot BYOK request headers must not contain empty names or newlines"
                        .to_string(),
                ));
            }
            HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                AppError::InvalidInput(format!(
                    "Invalid Copilot BYOK request header name {name}: {error}"
                ))
            })?;
            if !normalized_header_names.insert(name.to_ascii_lowercase()) {
                return Err(AppError::InvalidInput(format!(
                    "Duplicate Copilot BYOK request header name: {name}"
                )));
            }
        }
        for (name, value) in self.effective_request_headers() {
            HeaderValue::from_str(&value).map_err(|error| {
                AppError::InvalidInput(format!(
                    "Invalid Copilot BYOK request header value for {name}: {error}"
                ))
            })?;
        }

        let mut ids = HashSet::new();
        let mut model_ids = HashSet::new();
        let mut names = HashSet::new();
        for model in &self.models {
            model.validate()?;
            if !ids.insert(model.id.clone()) {
                return Err(AppError::InvalidInput(format!(
                    "Duplicate Copilot BYOK internal model id: {}",
                    model.id
                )));
            }
            if !model_ids.insert(model.model_id.to_lowercase()) {
                return Err(AppError::InvalidInput(format!(
                    "Duplicate Copilot BYOK model id in {}: {}",
                    self.name, model.model_id
                )));
            }
            if !names.insert(model.name.to_lowercase()) {
                return Err(AppError::InvalidInput(format!(
                    "Duplicate Copilot BYOK display name in {}: {}",
                    self.name, model.name
                )));
            }
        }
        Ok(())
    }

    pub fn enabled_model_count(&self) -> usize {
        if !self.enabled {
            return 0;
        }
        self.models.iter().filter(|model| model.enabled).count()
    }

    pub fn to_language_model_group(&self) -> Value {
        let request_headers = self.effective_request_headers();
        let models: Vec<Value> = self
            .models
            .iter()
            .filter(|model| model.enabled)
            .map(|model| model.to_language_model(&self.url, &request_headers))
            .collect();
        // CustomEndpoint treats a group-level `url` as a model-discovery base URL and
        // bypasses the static `models` array. Keep the connection shared in our store,
        // then project the same URL and headers onto every static model entry.
        let mut group: Map<String, Value> = self.extra.clone().into_iter().collect();
        group.insert("name".to_string(), json!(self.name));
        group.insert("vendor".to_string(), json!("customendpoint"));
        group.insert("apiKey".to_string(), json!(self.api_key));
        group.insert("apiType".to_string(), json!(self.api_type));
        group.insert(MANAGED_MARKER.to_string(), json!(true));
        group.insert("ccSwitchGroupId".to_string(), json!(self.id));
        group.insert("models".to_string(), json!(models));
        Value::Object(group)
    }
}

pub fn is_managed_group(value: &Value) -> bool {
    value.get("vendor").and_then(Value::as_str) == Some("customendpoint")
        && (value.get(MANAGED_MARKER).and_then(Value::as_bool) == Some(true)
            || value
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.starts_with(LEGACY_MANAGED_PREFIX)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(model_id: &str, name: &str) -> CopilotByokModel {
        CopilotByokModel {
            id: model_id.to_string(),
            model_id: model_id.to_string(),
            name: name.to_string(),
            enabled: true,
            tool_calling: Some(true),
            vision: Some(false),
            thinking: Some(true),
            streaming: Some(true),
            context_window: Some(262_144),
            max_input_tokens: None,
            max_output_tokens: Some(32_768),
            edit_tools: Vec::new(),
            zero_data_retention_enabled: false,
            supports_reasoning_effort: Vec::new(),
            reasoning_effort_format: None,
            model_options: json!({}),
            extra: BTreeMap::new(),
        }
    }

    fn group() -> CopilotByokGroup {
        CopilotByokGroup {
            id: "moonshot".to_string(),
            name: "Moonshot".to_string(),
            url: "https://api.example.com/v1/chat/completions".to_string(),
            api_key: "secret".to_string(),
            api_type: "chat-completions".to_string(),
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            category: None,
            usage_script: None,
            enabled: true,
            request_headers: BTreeMap::new(),
            models: vec![model("kimi-k3", "Kimi K3"), model("kimi-k2", "Kimi K2")],
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn managed_group_detection_supports_marker_and_legacy_prefix() {
        assert!(is_managed_group(&json!({
            "name": "Moonshot",
            "vendor": "customendpoint",
            "ccSwitchManaged": true
        })));
        assert!(is_managed_group(&json!({
            "name": "CC Switch: Kimi",
            "vendor": "customendpoint"
        })));
        assert!(!is_managed_group(&json!({
            "name": "My Kimi",
            "vendor": "customendpoint"
        })));
    }

    #[test]
    fn validation_rejects_non_http_endpoint() {
        let mut value = group();
        value.url = "file:///tmp/model".to_string();
        assert!(value.validate().is_err());
    }

    #[test]
    fn validation_rejects_invalid_headers_and_model_options() {
        let mut invalid_header = group();
        invalid_header
            .request_headers
            .insert("Not A Header".to_string(), "value".to_string());
        assert!(invalid_header.validate().is_err());

        let mut duplicate_header = group();
        duplicate_header
            .request_headers
            .insert("Authorization".to_string(), "Bearer first".to_string());
        duplicate_header
            .request_headers
            .insert("authorization".to_string(), "Bearer second".to_string());
        assert!(duplicate_header.validate().is_err());

        let mut invalid_options = group();
        invalid_options.models[0].model_options = json!(["invalid"]);
        assert!(invalid_options.validate().is_err());

        let mut unsafe_limit = group();
        unsafe_limit.models[0].context_window = Some(MAX_SAFE_JSON_INTEGER + 1);
        assert!(unsafe_limit.validate().is_err());
    }

    #[test]
    fn generated_group_uses_provider_name_and_shared_connection() {
        let mut value = group();
        value
            .request_headers
            .insert("X-Test".to_string(), "${apiKey}".to_string());
        value.models[0].edit_tools = vec!["apply-patch".to_string()];
        value.models[0].zero_data_retention_enabled = true;
        value.models[0].supports_reasoning_effort = vec!["high".to_string()];
        let rendered = value.to_language_model_group();

        assert_eq!(rendered["name"], "Moonshot");
        assert_eq!(rendered["vendor"], "customendpoint");
        assert_eq!(rendered["apiKey"], "secret");
        assert_eq!(rendered["apiType"], "chat-completions");
        assert_eq!(rendered["models"].as_array().map(Vec::len), Some(2));
        assert_eq!(rendered["models"][0]["url"], rendered["models"][1]["url"]);
        assert!(rendered.get("url").is_none());
        assert_eq!(rendered["models"][0]["requestHeaders"]["X-Test"], "secret");
        assert_eq!(rendered["models"][1]["requestHeaders"]["X-Test"], "secret");
        assert_eq!(
            rendered["models"][0]["requestHeaders"]["Authorization"],
            "Bearer secret"
        );
        assert_eq!(rendered["models"][0]["editTools"][0], "apply-patch");
        assert_eq!(rendered["models"][0]["zeroDataRetentionEnabled"], true);
        assert_eq!(rendered["models"][0]["supportsReasoningEffort"][0], "high");
        assert_eq!(rendered["ccSwitchManaged"], true);
    }

    #[test]
    fn unknown_model_defaults_tool_calling_without_inventing_other_capabilities() {
        let mut value = group();
        value.models[0].model_id = "kimi-k2".to_string();
        let model = &mut value.models[0];
        model.tool_calling = None;
        model.vision = None;
        model.thinking = None;
        model.streaming = None;
        model.context_window = None;
        model.max_output_tokens = None;
        value.normalize();

        assert_eq!(value.models[0].tool_calling, Some(true));
        let rendered = value.to_language_model_group();
        let model = &rendered["models"][0];
        assert_eq!(model["toolCalling"], true);
        for field in [
            "vision",
            "thinking",
            "streaming",
            "contextWindow",
            "maxOutputTokens",
        ] {
            assert!(model.get(field).is_none(), "unexpected field: {field}");
        }
    }

    #[test]
    fn kimi_k3_defaults_make_agent_requests_compatible() {
        let mut value = group();
        value.name = "Custom Gateway".to_string();
        value.url = "https://gateway.example.com/v1".to_string();
        value.models.truncate(1);
        let model = &mut value.models[0];
        model.model_id = "k3".to_string();
        model.tool_calling = None;
        model.vision = None;
        model.thinking = None;
        model.streaming = None;
        model.context_window = None;
        model.max_input_tokens = None;
        model.max_output_tokens = None;
        model.supports_reasoning_effort.clear();
        model.reasoning_effort_format = None;
        model.model_options = json!({ "top_p": 1, "custom": true });

        value.normalize();

        let model = &value.models[0];
        assert_eq!(model.tool_calling, Some(true));
        assert_eq!(model.vision, Some(true));
        assert_eq!(model.thinking, Some(true));
        assert_eq!(model.streaming, Some(true));
        assert_eq!(model.context_window, Some(1_000_000));
        assert_eq!(model.supports_reasoning_effort, vec!["low", "high", "max"]);
        assert_eq!(
            model.reasoning_effort_format.as_deref(),
            Some("chat-completions")
        );
        assert_eq!(model.model_options["temperature"], 1.0);
        assert_eq!(model.model_options["top_p"], 0.95);
        assert_eq!(model.model_options["custom"], true);

        let rendered = value.to_language_model_group();
        assert_eq!(rendered["models"][0]["toolCalling"], true);
        assert_eq!(rendered["models"][0]["contextWindow"], 1_000_000);
        assert_eq!(rendered["models"][0]["modelOptions"]["top_p"], 0.95);
    }

    #[test]
    fn kimi_k3_keeps_explicit_capability_overrides() {
        let mut value = group();
        value.name = "Custom Gateway".to_string();
        value.url = "https://gateway.example.com/v1".to_string();
        value.models.truncate(1);
        let model = &mut value.models[0];
        model.model_id = "k3".to_string();
        model.tool_calling = Some(false);
        model.vision = Some(false);
        model.context_window = Some(128_000);

        value.normalize();

        assert_eq!(value.models[0].tool_calling, Some(false));
        assert_eq!(value.models[0].vision, Some(false));
        assert_eq!(value.models[0].context_window, Some(128_000));
    }

    #[test]
    fn kimi_coding_plan_aliases_receive_family_defaults() {
        let mut value = group();
        value.url = "https://api.kimi.com/coding/v1".to_string();
        for model in &mut value.models {
            model.tool_calling = None;
            model.vision = None;
            model.thinking = None;
            model.streaming = None;
            model.context_window = None;
            model.max_output_tokens = None;
            model.supports_reasoning_effort.clear();
            model.reasoning_effort_format = None;
            model.model_options = json!({ "top_p": 1 });
        }
        value.models[0].model_id = "k3-256k".to_string();
        value.models[1].model_id = "kimi-for-coding-highspeed".to_string();

        value.normalize();

        let k3 = &value.models[0];
        assert_eq!(k3.context_window, Some(262_144));
        assert_eq!(k3.supports_reasoning_effort, vec!["low", "high", "max"]);
        assert_eq!(k3.model_options["top_p"], 0.95);

        let coding = &value.models[1];
        assert_eq!(coding.tool_calling, Some(true));
        assert_eq!(coding.vision, Some(true));
        assert_eq!(coding.thinking, Some(true));
        assert_eq!(coding.streaming, Some(true));
        assert_eq!(coding.context_window, Some(262_144));
        assert_eq!(coding.max_output_tokens, Some(262_144));
        assert!(coding.supports_reasoning_effort.is_empty());
        assert_eq!(coding.model_options["temperature"], 1.0);
        assert_eq!(coding.model_options["top_p"], 0.95);
    }

    #[test]
    fn minimax_m_series_defaults_follow_the_documented_model_contract() {
        let mut value = group();
        value.name = "Custom Gateway".to_string();
        value.url = "https://gateway.example.com/v1".to_string();
        for model in &mut value.models {
            model.tool_calling = None;
            model.vision = None;
            model.thinking = None;
            model.streaming = None;
            model.context_window = None;
            model.max_input_tokens = None;
            model.max_output_tokens = None;
            model.supports_reasoning_effort.clear();
            model.reasoning_effort_format = None;
            model.model_options = json!({});
        }
        value.models[0].model_id = "MiniMax-M3".to_string();
        value.models[0].model_options = json!({ "top_p": 0.7 });
        value.models[1].model_id = "MiniMax-M2.7-highspeed".to_string();

        value.normalize();

        let m3 = &value.models[0];
        assert_eq!(m3.tool_calling, Some(true));
        assert_eq!(m3.vision, Some(true));
        assert_eq!(m3.thinking, Some(true));
        assert_eq!(m3.streaming, Some(true));
        assert_eq!(m3.context_window, Some(1_000_000));
        assert_eq!(m3.max_output_tokens, Some(524_288));
        assert_eq!(m3.model_options["temperature"], 1.0);
        assert_eq!(m3.model_options["top_p"], 0.7);

        let m2 = &value.models[1];
        assert_eq!(m2.tool_calling, Some(true));
        assert_eq!(m2.vision, Some(false));
        assert_eq!(m2.thinking, Some(true));
        assert_eq!(m2.streaming, Some(true));
        assert_eq!(m2.context_window, Some(204_800));
        assert_eq!(m2.max_output_tokens, Some(204_800));
        assert_eq!(m2.model_options["temperature"], 1.0);
        assert_eq!(m2.model_options["top_p"], 0.9);
    }

    #[test]
    fn messages_protocol_uses_x_api_key() {
        let mut value = group();
        value.api_type = "messages".to_string();

        let rendered = value.to_language_model_group();

        assert_eq!(
            rendered["models"][0]["requestHeaders"]["x-api-key"],
            "secret"
        );
        assert_eq!(
            rendered["models"][0]["requestHeaders"]["anthropic-version"],
            "2023-06-01"
        );
        assert!(rendered["models"][0]["requestHeaders"]
            .get("Authorization")
            .is_none());
    }

    #[test]
    fn explicit_authorization_header_is_preserved() {
        let mut value = group();
        value
            .request_headers
            .insert("authorization".to_string(), "Token ${apiKey}".to_string());

        let rendered = value.to_language_model_group();

        assert_eq!(
            rendered["models"][0]["requestHeaders"]["authorization"],
            "Token secret"
        );
        assert!(rendered["models"][0]["requestHeaders"]
            .get("Authorization")
            .is_none());
    }

    #[test]
    fn vscode_secret_reference_is_left_for_vscode_to_resolve() {
        let mut value = group();
        value.api_key = "${input:chat.lm.secret.test}".to_string();
        value
            .request_headers
            .insert("X-Auth".to_string(), "Token ${apiKey}".to_string());

        let rendered = value.to_language_model_group();

        assert_eq!(rendered["apiKey"], "${input:chat.lm.secret.test}");
        assert_eq!(
            rendered["models"][0]["requestHeaders"]["X-Auth"],
            "Token ${apiKey}"
        );
        assert!(rendered["models"][0]["requestHeaders"]
            .get("Authorization")
            .is_none());
    }
}
