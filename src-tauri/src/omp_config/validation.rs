//! Known `models.yml` fields and business rules from omp v18.8.7.
//! Unknown fields are intentionally retained for newer native releases.

use crate::error::AppError;
use serde_json::{Map, Value};

const APIS: &[&str] = &[
    "openai-completions",
    "openai-responses",
    "openai-codex-responses",
    "azure-openai-responses",
    "anthropic-messages",
    "bedrock-converse-stream",
    "google-generative-ai",
    "google-gemini-cli",
    "google-vertex",
    "openrouter-decisions",
    "typesafe",
];
const RUNNERS: &[&str] = &[
    "openai-images",
    "openrouter-images",
    "xai-tts",
    "openai-speech",
    "openai-embeddings",
    "openrouter-rerank",
    "openrouter-video",
    "openai-transcriptions",
];
const EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max"];

fn invalid(path: &str, expected: &str) -> AppError {
    AppError::InvalidInput(format!("Oh My Pi {path}: expected {expected}"))
}

fn object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, AppError> {
    value.as_object().ok_or_else(|| invalid(path, "an object"))
}

fn fields(
    value: &Map<String, Value>,
    path: &str,
    strings: &[&str],
    bools: &[&str],
    numbers: &[&str],
) -> Result<(), AppError> {
    for (keys, predicate, expected) in [
        (strings, Value::is_string as fn(&Value) -> bool, "a string"),
        (bools, Value::is_boolean as fn(&Value) -> bool, "a boolean"),
        (numbers, Value::is_number as fn(&Value) -> bool, "a number"),
    ] {
        for key in keys {
            if value.get(*key).is_some_and(|entry| !predicate(entry)) {
                return Err(invalid(&format!("{path}.{key}"), expected));
            }
        }
    }
    Ok(())
}

fn nonempty(value: &Map<String, Value>, path: &str, keys: &[&str]) -> Result<(), AppError> {
    fields(value, path, keys, &[], &[])?;
    for key in keys {
        if value.get(*key).and_then(Value::as_str) == Some("") {
            return Err(invalid(&format!("{path}.{key}"), "a non-empty string"));
        }
    }
    Ok(())
}

fn enumeration(
    value: &Map<String, Value>,
    path: &str,
    key: &str,
    allowed: &[&str],
) -> Result<(), AppError> {
    if let Some(entry) = value.get(key) {
        if !entry.as_str().is_some_and(|text| allowed.contains(&text)) {
            return Err(invalid(&format!("{path}.{key}"), &allowed.join(" | ")));
        }
    }
    Ok(())
}

fn string_map(value: &Value, path: &str) -> Result<(), AppError> {
    for (key, entry) in object(value, path)? {
        if !entry.is_string() {
            return Err(invalid(&format!("{path}.{key}"), "a string"));
        }
    }
    Ok(())
}

fn string_array(value: &Value, path: &str, allowed: &[&str]) -> Result<(), AppError> {
    let entries = value.as_array().ok_or_else(|| invalid(path, "an array"))?;
    for entry in entries {
        if !entry
            .as_str()
            .is_some_and(|text| allowed.is_empty() || allowed.contains(&text))
        {
            return Err(invalid(path, "an array of allowed strings"));
        }
    }
    Ok(())
}

fn compat(value: &Value, path: &str, when_thinking: bool) -> Result<(), AppError> {
    let value = object(value, path)?;
    fields(
        value,
        path,
        &[],
        &[
            "supportsStore",
            "supportsDeveloperRole",
            "supportsMultipleSystemMessages",
            "supportsReasoningEffort",
            "supportsUsageInStreaming",
            "requiresToolResultName",
            "requiresMistralToolIds",
            "requiresAssistantAfterToolResult",
            "requiresThinkingAsText",
            "requiresReasoningContentForToolCalls",
            "allowsSyntheticReasoningContentForToolCalls",
            "requiresAssistantContentForToolCalls",
            "supportsToolChoice",
            "supportsForcedToolChoice",
            "disableReasoningOnForcedToolChoice",
            "disableReasoningOnToolChoice",
            "disableReasoningWithTools",
            "qwenTemplateReasoningEffort",
            "supportsStrictMode",
            "supportsLongPromptCacheRetention",
            "supportsReasoningParams",
            "supportsReasoningSummary",
            "statefulResponses",
            "alwaysSendMaxTokens",
            "strictResponsesPairing",
            "supportsImageDetailOriginal",
            "supportsConfigurationUpdate",
            "supportsSteering",
            "stripImageInput",
            "supportsContextManagement",
            "supportsEagerToolInputStreaming",
            "allowAnthropicHeaderOverrides",
            "requiresToolResultId",
            "replayUnsignedThinking",
            "bedrockMessagesApi",
        ],
        &[
            "streamIdleTimeoutMs",
            "promptCacheMinimumTokens",
            "promptCacheMaximumCheckpoints",
        ],
    )?;
    for (key, allowed) in [
        (
            "thinkingFormat",
            &["openai", "openrouter", "zai", "qwen", "qwen-chat-template"][..],
        ),
        (
            "maxTokensField",
            &["max_completion_tokens", "max_tokens"][..],
        ),
        (
            "reasoningContentField",
            &["reasoning_content", "reasoning", "reasoning_text"][..],
        ),
        ("cacheControlFormat", &["anthropic"][..]),
        ("toolStrictMode", &["all_strict", "none"][..]),
        (
            "streamMarkupHealingPattern",
            &["kimi", "dsml", "qwen", "thinking"][..],
        ),
        ("promptCacheMode", &["none", "automatic", "explicit"][..]),
    ] {
        enumeration(value, path, key, allowed)?;
    }
    for key in [
        "streamIdleTimeoutMs",
        "promptCacheMinimumTokens",
        "promptCacheMaximumCheckpoints",
    ] {
        if value
            .get(key)
            .and_then(Value::as_f64)
            .is_some_and(|number| number < 0.0)
        {
            return Err(invalid(&format!("{path}.{key}"), "a non-negative number"));
        }
    }
    if let Some(map) = value.get("reasoningEffortMap") {
        fields(object(map, path)?, path, EFFORTS, &[], &[])?;
    }
    for key in ["openRouterRouting", "vercelGatewayRouting"] {
        if let Some(routing) = value.get(key) {
            for (field, entry) in object(routing, path)? {
                if matches!(field.as_str(), "only" | "order") {
                    string_array(entry, path, &[])?;
                }
            }
        }
    }
    if let Some(extra) = value.get("extraBody") {
        object(extra, path)?;
    }
    if !when_thinking {
        if let Some(thinking) = value.get("whenThinking") {
            compat(thinking, &format!("{path}.whenThinking"), true)?;
        }
    }
    Ok(())
}

fn thinking(value: &Value, path: &str) -> Result<(), AppError> {
    let value = object(value, path)?;
    if !value.contains_key("mode") {
        return Err(invalid(path, "thinking.mode"));
    }
    enumeration(
        value,
        path,
        "mode",
        &[
            "effort",
            "budget",
            "google-level",
            "anthropic-adaptive",
            "anthropic-budget-effort",
        ],
    )?;
    fields(
        value,
        path,
        &[],
        &["supportsDisplay", "requiresEffort"],
        &[],
    )?;
    for key in ["defaultLevel", "minLevel", "maxLevel"] {
        enumeration(value, path, key, EFFORTS)?;
    }
    for key in ["efforts", "levels"] {
        if let Some(entry) = value.get(key) {
            string_array(entry, &format!("{path}.{key}"), EFFORTS)?;
        }
    }
    if let Some(map) = value.get("effortMap") {
        fields(object(map, path)?, path, EFFORTS, &[], &[])?;
    }
    if !(value.contains_key("efforts")
        || value.contains_key("levels")
        || value.contains_key("minLevel") && value.contains_key("maxLevel"))
    {
        return Err(invalid(path, "efforts, levels, or minLevel and maxLevel"));
    }
    Ok(())
}

fn remote_compaction(value: &Value, path: &str) -> Result<(), AppError> {
    let value = object(value, path)?;
    fields(value, path, &[], &["enabled", "v2StreamingEnabled"], &[])?;
    nonempty(
        value,
        path,
        &["endpoint", "model", "v2Endpoint", "streamingEndpoint"],
    )?;
    enumeration(value, path, "api", APIS)
}

fn kind_matches_api(
    value: &Map<String, Value>,
    inherited_api: Option<&str>,
    path: &str,
) -> Result<(), AppError> {
    let Some(kind) = value.get("kind").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(api) = value.get("api").and_then(Value::as_str).or(inherited_api) else {
        return Ok(());
    };
    let allowed = match api {
        "typesafe" | "openrouter-decisions" => &["judge"][..],
        "openai-images" | "openrouter-images" => &["image"][..],
        "xai-tts" | "openai-speech" => &["tts"][..],
        "openai-embeddings" => &["embedding"][..],
        "openrouter-rerank" => &["rerank"][..],
        "openrouter-video" => &["video"][..],
        "openai-transcriptions" => &["stt"][..],
        "google-generative-ai"
        | "google-gemini-cli"
        | "openai-responses"
        | "openai-codex-responses" => &["chat", "tiny", "image"][..],
        _ => &["chat", "tiny"][..],
    };
    if !allowed.contains(&kind) {
        return Err(invalid(path, "kind matching the selected api"));
    }
    Ok(())
}

fn model(value: &Value, path: &str, definition: bool) -> Result<(), AppError> {
    let value = object(value, path)?;
    nonempty(
        value,
        path,
        &["name", "contextPromotionTarget", "compactionModel"],
    )?;
    fields(
        value,
        path,
        &[],
        &[
            "reasoning",
            "supportsTools",
            "omitMaxOutputTokens",
            "preferWebsockets",
        ],
        &[
            "premiumMultiplier",
            "contextWindow",
            "maxContextWindow",
            "maxTokens",
        ],
    )?;
    let model_apis: Vec<_> = APIS.iter().chain(RUNNERS).copied().collect();
    enumeration(value, path, "api", &model_apis)?;
    enumeration(
        value,
        path,
        "kind",
        &[
            "chat",
            "tiny",
            "image",
            "tts",
            "stt",
            "judge",
            "embedding",
            "rerank",
            "video",
        ],
    )?;
    enumeration(value, path, "imageInputDecoder", &["stb"])?;
    enumeration(
        value,
        path,
        "tokenizer",
        &[
            "claude-v3",
            "claude-v47",
            "claude-v5",
            "claude-v5-sonnet",
            "qwen3",
            "deepseek-v3",
            "kimi-k2",
            "glm5",
        ],
    )?;
    if definition {
        if !value.contains_key("id") {
            return Err(invalid(path, "a model id"));
        }
        nonempty(value, path, &["id", "baseUrl"])?;
        for key in ["contextWindow", "maxTokens"] {
            if value
                .get(key)
                .and_then(Value::as_f64)
                .is_some_and(|number| number <= 0.0)
            {
                return Err(invalid(&format!("{path}.{key}"), "a positive number"));
            }
        }
    }
    if let Some(number) = value.get("maxContextWindow").and_then(Value::as_f64) {
        if number <= 0.0
            || number.fract() != 0.0
            || number > 9_007_199_254_740_991.0
            || value
                .get("contextWindow")
                .and_then(Value::as_f64)
                .is_some_and(|context| number < context)
        {
            return Err(invalid(
                &format!("{path}.maxContextWindow"),
                "a positive safe integer no smaller than contextWindow",
            ));
        }
    }
    for (key, entry) in value {
        let nested = format!("{path}.{key}");
        match key.as_str() {
            "input" => string_array(entry, &nested, &["text", "image"])?,
            "thinking" => thinking(entry, &nested)?,
            "headers" => string_map(entry, &nested)?,
            "compat" => compat(entry, &nested, false)?,
            "remoteCompaction" => remote_compaction(entry, &nested)?,
            "cost" => {
                let cost = object(entry, &nested)?;
                let keys = &["input", "output", "cacheRead", "cacheWrite"];
                fields(cost, &nested, &[], &[], keys)?;
                if definition && keys.iter().any(|key| !cost.contains_key(*key)) {
                    return Err(invalid(
                        &nested,
                        "input, output, cacheRead and cacheWrite prices",
                    ));
                }
            }
            "promptCache" => fields(
                object(entry, &nested)?,
                &nested,
                &[],
                &[],
                &["short", "long"],
            )?,
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn validate_provider_node(provider_key: &str, config: &Value) -> Result<(), AppError> {
    if provider_key.trim().is_empty() {
        return Err(invalid("provider key", "a non-empty string"));
    }
    let path = format!("providers.{provider_key}");
    let provider = object(config, &path)?;
    nonempty(provider, &path, &["baseUrl", "apiKey"])?;
    fields(
        provider,
        &path,
        &["guardrailIdentifier", "guardrailVersion"],
        &["authHeader", "disableStrictTools"],
        &[],
    )?;
    enumeration(provider, &path, "api", APIS)?;
    enumeration(provider, &path, "auth", &["apiKey", "none", "oauth"])?;
    enumeration(provider, &path, "transport", &["pi-native"])?;
    enumeration(
        provider,
        &path,
        "guardrailTrace",
        &["enabled", "disabled", "enabled_full"],
    )?;
    for (key, entry) in provider {
        let nested = format!("{path}.{key}");
        match key.as_str() {
            "headers" | "requestMetadata" => string_map(entry, &nested)?,
            "compat" => compat(entry, &nested, false)?,
            "remoteCompaction" => remote_compaction(entry, &nested)?,
            "discovery" => {
                let discovery = object(entry, &nested)?;
                if !discovery.contains_key("type") {
                    return Err(invalid(&nested, "a discovery type"));
                }
                enumeration(
                    discovery,
                    &nested,
                    "type",
                    &[
                        "ollama",
                        "llama.cpp",
                        "lm-studio",
                        "openai-models-list",
                        "proxy",
                        "litellm",
                        "apple-foundation-models",
                    ],
                )?;
                fields(discovery, &nested, &[], &["injectV1"], &["timeoutMs"])?;
                if discovery
                    .get("timeoutMs")
                    .and_then(Value::as_f64)
                    .is_some_and(|number| number <= 0.0)
                {
                    return Err(invalid(&nested, "a positive timeoutMs"));
                }
                if discovery.contains_key("injectV1")
                    && discovery.get("type").and_then(Value::as_str) != Some("openai-models-list")
                {
                    return Err(invalid(
                        &nested,
                        "injectV1 only on openai-models-list discovery",
                    ));
                }
                if !provider.contains_key("api")
                    && discovery.get("type").and_then(Value::as_str) != Some("proxy")
                {
                    return Err(invalid(&path, "api when discovery is enabled"));
                }
            }
            _ => {}
        }
    }
    let models = match provider.get("models") {
        Some(value) => value
            .as_array()
            .ok_or_else(|| invalid(&format!("{path}.models"), "an array"))?
            .as_slice(),
        None => &[],
    };
    let api = provider.get("api").and_then(Value::as_str);
    for (index, entry) in models.iter().enumerate() {
        let nested = format!("{path}.models[{index}]");
        model(entry, &nested, true)?;
        let entry = object(entry, &nested)?;
        if api.is_none() && !entry.contains_key("api") {
            return Err(invalid(&nested, "api at provider or model level"));
        }
        kind_matches_api(entry, api, &nested)?;
    }
    let overrides = provider
        .get("modelOverrides")
        .map(|entry| object(entry, &format!("{path}.modelOverrides")))
        .transpose()?;
    if let Some(overrides) = overrides {
        for (id, entry) in overrides {
            let nested = format!("{path}.modelOverrides.{id}");
            model(entry, &nested, false)?;
            let declared = models
                .iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id));
            let declared_api = declared
                .and_then(|entry| entry.get("api"))
                .and_then(Value::as_str)
                .or(if declared.is_some() { api } else { None });
            kind_matches_api(object(entry, &nested)?, declared_api, &nested)?;
        }
    }
    if !models.is_empty() {
        if !provider.contains_key("baseUrl") {
            return Err(invalid(&path, "baseUrl when defining custom models"));
        }
        if !provider.contains_key("apiKey")
            && !matches!(
                provider.get("auth").and_then(Value::as_str),
                Some("none" | "oauth")
            )
        {
            return Err(invalid(&path, "apiKey unless auth is none or oauth"));
        }
    } else {
        let has_override = [
            "baseUrl",
            "headers",
            "compat",
            "apiKey",
            "requestMetadata",
            "remoteCompaction",
            "discovery",
        ]
        .iter()
        .any(|key| provider.contains_key(*key))
            || provider.get("auth").and_then(Value::as_str) == Some("none")
            || provider.get("disableStrictTools").and_then(Value::as_bool) == Some(true)
            || provider
                .get("guardrailIdentifier")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
            || overrides.is_some_and(|entries| !entries.is_empty());
        if !has_override {
            return Err(invalid(
                &path,
                "models, discovery, or a native provider override",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_invalid_native_fields_without_rejecting_sparse_or_future_fields() {
        for config in [
            json!({"name": "Display name"}),
            json!({"baseUrl": "https://test", "compat": {"thinkingFormat": "deepseek"}}),
            json!({"baseUrl": "https://test", "api": "invalid"}),
            json!({"baseUrl": "https://test", "headers": {"x-header": 1}}),
            json!({"baseUrl": "https://test", "modelOverrides": {"m": {"thinking": "high"}}}),
            json!({"baseUrl": "https://test", "modelOverrides": {"m": {"thinking": {"mode": "effort"}}}}),
            json!({"api": "openai-completions", "auth": "none", "models": [{"id": "m"}]}),
            json!({"baseUrl": "https://test", "api": "openai-completions", "models": [{"id": "m"}]}),
            json!({"baseUrl": "https://test", "api": "openai-completions", "auth": "none", "models": [{"id": "m", "cost": {"input": 0.15, "output": 0.25}}]}),
            json!({"baseUrl": "https://test", "api": "openai-completions", "auth": "none", "models": [{"id": "m", "kind": "tts"}]}),
            json!({"baseUrl": "https://test", "discovery": {"type": "ollama"}}),
        ] {
            assert!(
                validate_provider_node("test", &config).is_err(),
                "accepted {config}"
            );
        }
        for config in [
            json!({"baseUrl": "https://proxy", "apiKey": "test"}),
            json!({"compat": {"supportsStore": false, "futureCompat": {"enabled": true}}, "futureProvider": [1,2]}),
            json!({"modelOverrides": {"m": {"cost": {"input": 0.15}, "futureModel": true}}}),
            json!({"baseUrl": "https://test", "auth": "none", "models": [{"id": "m", "api": "openai-images", "kind": "image", "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "thinking": {"mode": "effort", "minLevel": "low", "maxLevel": "high"}}]}),
            json!({"discovery": {"type": "proxy"}}),
        ] {
            assert!(
                validate_provider_node("test", &config).is_ok(),
                "rejected {config}"
            );
        }
    }
}
