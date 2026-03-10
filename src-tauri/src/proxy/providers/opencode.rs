//! OpenCode Provider Adapter
//!
//! OpenCode uses AI SDK format with npm package specification.
//! This adapter handles OpenCode's unique provider structure:
//! - npm: AI SDK package name (@ai-sdk/openai-compatible, @ai-sdk/anthropic, etc.)
//! - options: { baseURL, apiKey, headers, ... }
//! - models: { modelId: { name, limit, options, ... } }
//!
//! ## API Formats
//! - **openai_chat** (default): OpenAI Chat Completions format
//! - **anthropic**: Anthropic Messages API format
//!
//! ## Provider Types
//! - **OpenCode**: Standard OpenCode provider with AI SDK format
//! - **OpenCodeCC**: cc-switch managed provider with -cc suffix

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::provider::{OpenCodeProviderConfig, Provider};
use crate::proxy::error::ProxyError;
use reqwest::RequestBuilder;
use serde_json::Value;

/// OpenCode adapter
pub struct OpenCodeAdapter;

impl OpenCodeAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Extract OpenCode provider config from Provider
    fn extract_opencode_config(&self, provider: &Provider) -> Option<OpenCodeProviderConfig> {
        serde_json::from_value::<OpenCodeProviderConfig>(provider.settings_config.clone()).ok()
    }

    /// Get API format from provider meta or config
    fn get_api_format(&self, provider: &Provider) -> &'static str {
        // Check meta.api_format first (SSOT)
        if let Some(meta) = provider.meta.as_ref() {
            if let Some(api_format) = meta.api_format.as_deref() {
                return match api_format {
                    "anthropic" => "anthropic",
                    _ => "openai_chat",
                };
            }
        }

        // Check settings_config.api_format
        if let Some(api_format) = provider
            .settings_config
            .get("api_format")
            .and_then(|v| v.as_str())
        {
            return match api_format {
                "anthropic" => "anthropic",
                _ => "openai_chat",
            };
        }

        // Check npm package to infer format
        if let Some(config) = self.extract_opencode_config(provider) {
            if config.npm.contains("anthropic") {
                return "anthropic";
            }
        }

        // Default to OpenAI Chat format
        "openai_chat"
    }

    /// Check if provider is a cc-switch managed provider (-cc suffix)
    pub fn is_cc_managed(&self, provider: &Provider) -> bool {
        provider.id.ends_with("-cc")
    }
}

impl ProviderAdapter for OpenCodeAdapter {
    fn name(&self) -> &'static str {
        "OpenCode"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // Try to extract from OpenCode config structure
        if let Some(config) = self.extract_opencode_config(provider) {
            if let Some(base_url) = config.options.base_url {
                return Ok(base_url.trim_end_matches('/').to_string());
            }
        }

        // Fallback: try to extract from settings_config directly
        if let Some(options) = provider.settings_config.get("options") {
            if let Some(base_url) = options.get("baseURL").and_then(|v| v.as_str()) {
                return Ok(base_url.trim_end_matches('/').to_string());
            }
        }

        Err(ProxyError::ConfigError("Missing base_url".to_string()))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        // Try to extract from OpenCode config structure
        if let Some(config) = self.extract_opencode_config(provider) {
            if let Some(api_key) = config.options.api_key {
                // Handle environment variable references like "{env:API_KEY}"
                let key = if api_key.starts_with("{env:") && api_key.ends_with("}") {
                    // Extract env var name and try to get value
                    let env_name = &api_key[5..api_key.len() - 1];
                    std::env::var(env_name).unwrap_or(api_key)
                } else {
                    api_key
                };

                return Some(AuthInfo::new(key, AuthStrategy::Bearer));
            }
        }

        // Fallback: try to extract from settings_config directly
        if let Some(options) = provider.settings_config.get("options") {
            if let Some(api_key) = options.get("apiKey").and_then(|v| v.as_str()) {
                return Some(AuthInfo::new(api_key.to_string(), AuthStrategy::Bearer));
            }
        }

        None
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        format!("{}{}", base_url.trim_end_matches('/'), endpoint)
    }

    fn add_auth_headers(&self, request: RequestBuilder, auth: &AuthInfo) -> RequestBuilder {
        request.header("Authorization", format!("Bearer {}", auth.api_key))
    }

    fn needs_transform(&self, provider: &Provider) -> bool {
        self.get_api_format(provider) == "anthropic"
    }

    fn transform_request(&self, body: Value, provider: &Provider) -> Result<Value, ProxyError> {
        let api_format = self.get_api_format(provider);

        match api_format {
            "anthropic" => {
                // Transform OpenAI Chat format to Anthropic Messages format
                transform_openai_to_anthropic(body)
            }
            _ => {
                // OpenAI Chat format - pass through
                Ok(body)
            }
        }
    }
}

/// Transform OpenAI Chat Completions format to Anthropic Messages format
fn transform_openai_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    use serde_json::{Map, Value};

    let input = body.as_object().ok_or_else(|| {
        ProxyError::TransformError("Request body must be a JSON object".into())
    })?;

    // Extract model
    let model = input
        .get("model")
        .cloned()
        .unwrap_or(Value::String("claude-sonnet-4-20250514".into()));

    // Extract max_tokens
    let max_tokens = input
        .get("max_tokens")
        .or_else(|| input.get("max_completion_tokens"))
        .cloned()
        .unwrap_or(Value::Number(4096.into()));

    // Extract and transform messages
    let messages = input
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ProxyError::TransformError("Missing 'messages' array".into()))?;

    let mut transformed_messages: Vec<Value> = Vec::new();
    let mut system_message: Option<Value> = None;

    for msg in messages {
        if let Some(msg_obj) = msg.as_object() {
            let role = msg_obj.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = msg_obj.get("content").cloned().unwrap_or(Value::String("".into()));

            match role {
                "system" => {
                    // System message becomes a separate field in Anthropic format
                    system_message = Some(content);
                }
                "user" => {
                    // User message: wrap content in array format
                    let content_array = if content.is_string() {
                        Value::Array(vec![Value::Object({
                            let mut map = Map::new();
                            map.insert("type".to_string(), Value::String("text".to_string()));
                            map.insert("text".to_string(), content);
                            map
                        })])
                    } else {
                        // Already in array format or complex content
                        content
                    };

                    transformed_messages.push(Value::Object({
                        let mut map = Map::new();
                        map.insert("role".to_string(), Value::String("user".to_string()));
                        map.insert("content".to_string(), content_array);
                        map
                    }));
                }
                "assistant" => {
                    // Assistant message
                    transformed_messages.push(Value::Object({
                        let mut map = Map::new();
                        map.insert("role".to_string(), Value::String("assistant".to_string()));
                        map.insert("content".to_string(), content);
                        map
                    }));
                }
                _ => {}
            }
        }
    }

    // Build Anthropic Messages API format
    let mut output = Map::new();
    output.insert("model".to_string(), model);
    output.insert("max_tokens".to_string(), max_tokens);
    output.insert("messages".to_string(), Value::Array(transformed_messages));

    if let Some(system) = system_message {
        output.insert("system".to_string(), system);
    }

    // Copy tool definitions if present
    if let Some(tools) = input.get("tools") {
        let transformed_tools = transform_tools_to_anthropic(tools)?;
        output.insert("tools".to_string(), transformed_tools);
    }

    // Copy tool_choice if present
    if let Some(tool_choice) = input.get("tool_choice") {
        let transformed_tool_choice = transform_tool_choice_to_anthropic(tool_choice)?;
        output.insert("tool_choice".to_string(), transformed_tool_choice);
    }

    // Copy other fields (temperature, top_p, stream, etc.)
    for (key, value) in input {
        if !matches!(
            key.as_str(),
            "model"
                | "max_tokens"
                | "max_completion_tokens"
                | "messages"
                | "tools"
                | "tool_choice"
        ) {
            output.insert(key.clone(), value.clone());
        }
    }

    Ok(Value::Object(output))
}

/// Transform OpenAI tools to Anthropic tools format
fn transform_tools_to_anthropic(tools: &Value) -> Result<Value, ProxyError> {
    let tools_array = tools.as_array().ok_or_else(|| {
        ProxyError::TransformError("'tools' must be an array".into())
    })?;

    let mut transformed = Vec::new();

    for tool in tools_array {
        if let Some(tool_obj) = tool.as_object() {
            if let Some(function) = tool_obj.get("function").and_then(|v| v.as_object()) {
                // OpenAI format: { function: { name, description, parameters } }
                // Anthropic format: { name, description, input_schema }
                let mut anthropic_tool = serde_json::Map::new();

                if let Some(name) = function.get("name") {
                    anthropic_tool.insert("name".to_string(), name.clone());
                }

                if let Some(description) = function.get("description") {
                    anthropic_tool.insert("description".to_string(), description.clone());
                }

                if let Some(parameters) = function.get("parameters") {
                    // OpenAI uses JSON Schema, Anthropic uses input_schema
                    anthropic_tool.insert("input_schema".to_string(), parameters.clone());
                }

                transformed.push(Value::Object(anthropic_tool));
            }
        }
    }

    Ok(Value::Array(transformed))
}

/// Transform OpenAI tool_choice to Anthropic tool_choice format
fn transform_tool_choice_to_anthropic(tool_choice: &Value) -> Result<Value, ProxyError> {
    // OpenAI format: "auto", "required", "none", or { function: { name: "..." } }
    // Anthropic format: "auto", "any", "tool", or { name: "..." }

    if let Some(tc_str) = tool_choice.as_str() {
        let value = match tc_str {
            "auto" => "auto",
            "required" => "any",
            "none" => "none",
            _ => "auto",
        };
        return Ok(Value::String(value.to_string()));
    }

    if let Some(tc_obj) = tool_choice.as_object() {
        if let Some(function) = tc_obj.get("function").and_then(|v| v.as_object()) {
            if let Some(name) = function.get("name") {
                let mut result = serde_json::Map::new();
                result.insert("type".to_string(), Value::String("tool".to_string()));
                result.insert("name".to_string(), name.clone());
                return Ok(Value::Object(result));
            }
        }
    }

    Ok(Value::String("auto".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider(config: Value) -> Provider {
        Provider {
            id: "test-cc".to_string(),
            name: "Test OpenCode".to_string(),
            settings_config: config,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_extract_base_url() {
        let adapter = OpenCodeAdapter::new();
        let provider = create_provider(json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {
                "baseURL": "https://api.example.com/v1",
                "apiKey": "sk-test"
            }
        }));

        let base_url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(base_url, "https://api.example.com/v1");
    }

    #[test]
    fn test_extract_auth() {
        let adapter = OpenCodeAdapter::new();
        let provider = create_provider(json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {
                "baseURL": "https://api.example.com/v1",
                "apiKey": "sk-test-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-test-key");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_extract_auth_env_reference() {
        let adapter = OpenCodeAdapter::new();

        // Set up test env var
        std::env::set_var("TEST_API_KEY", "env-key-123");

        let provider = create_provider(json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {
                "baseURL": "https://api.example.com/v1",
                "apiKey": "{env:TEST_API_KEY}"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "env-key-123");

        // Cleanup
        std::env::remove_var("TEST_API_KEY");
    }

    #[test]
    fn test_is_cc_managed() {
        let adapter = OpenCodeAdapter::new();

        let cc_provider = create_provider(json!({}));
        assert!(adapter.is_cc_managed(&cc_provider));

        let non_cc_provider = Provider {
            id: "openrouter".to_string(),
            ..create_provider(json!({}))
        };
        assert!(!adapter.is_cc_managed(&non_cc_provider));
    }

    #[test]
    fn test_get_api_format() {
        let adapter = OpenCodeAdapter::new();

        // Default should be openai_chat
        let provider = create_provider(json!({
            "npm": "@ai-sdk/openai-compatible"
        }));
        assert_eq!(adapter.get_api_format(&provider), "openai_chat");

        // Anthropic npm should infer anthropic format
        let provider = create_provider(json!({
            "npm": "@ai-sdk/anthropic"
        }));
        assert_eq!(adapter.get_api_format(&provider), "anthropic");

        // Meta api_format should override
        let mut provider = create_provider(json!({
            "npm": "@ai-sdk/openai-compatible"
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        });
        assert_eq!(adapter.get_api_format(&provider), "anthropic");
    }

    #[test]
    fn test_transform_openai_to_anthropic_basic() {
        let openai_body = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [
                {"role": "system", "content": "You are helpful"},
                {"role": "user", "content": "Hello"}
            ]
        });

        let adapter = OpenCodeAdapter::new();
        let result = adapter.transform_request(openai_body, &create_provider(json!({
            "npm": "@ai-sdk/anthropic"
        }))).unwrap();

        // Check it's in Anthropic format
        assert!(result.get("model").is_some());
        assert!(result.get("max_tokens").is_some());
        assert!(result.get("messages").is_some());
        assert!(result.get("system").is_some());

        let messages = result.get("messages").unwrap().as_array().unwrap();
        assert_eq!(messages.len(), 1); // Only user message (system extracted)

        let user_msg = &messages[0];
        assert_eq!(user_msg.get("role").unwrap(), "user");

        let content = user_msg.get("content").unwrap().as_array().unwrap();
        assert_eq!(content[0].get("type").unwrap(), "text");
        assert_eq!(content[0].get("text").unwrap(), "Hello");
    }
}
