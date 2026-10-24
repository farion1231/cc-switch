//! Known fields from omp v18.10.4's models-config-schema-bundle.ts and models-config.ts.
//! Validate only the replacement; unknown fields and other providers stay untouched.

use super::invalid_provider;
use crate::error::AppError;
use serde_json::Value;

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
    "openai-decisions",
    "workers-ai-systemone",
    "typesafe",
];

fn check_type(node: &Value, keys: &[&str], check: impl Fn(&Value) -> bool) -> Result<(), AppError> {
    for key in keys {
        if node.get(key).is_some_and(|v| !check(v)) {
            return Err(invalid_provider());
        }
    }
    Ok(())
}

fn string_map(v: &Value) -> bool {
    v.as_object()
        .is_some_and(|m| m.values().all(Value::is_string))
}

fn check_common(node: &Value) -> Result<(), AppError> {
    object(node)?;
    check_type(node, &["headers"], string_map)?;
    check_nested(node, "compat", check_compat)?;
    check_nested(node, "remoteCompaction", check_remote_compaction)
}

pub(crate) fn validate_provider(id: &str, node: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() {
        return Err(invalid_provider());
    }
    check_common(node)?;
    check_type(node, &["baseUrl", "apiKey"], nonempty)?;
    check_type(
        node,
        &["guardrailIdentifier", "guardrailVersion"],
        Value::is_string,
    )?;
    check_type(
        node,
        &["authHeader", "disableStrictTools"],
        Value::is_boolean,
    )?;
    check_type(node, &["requestMetadata"], string_map)?;
    check_enum(node, "api", APIS)?;
    check_enum(node, "auth", &["apiKey", "none", "oauth"])?;
    check_enum(
        node,
        "guardrailTrace",
        &["enabled", "disabled", "enabled_full"],
    )?;
    check_enum(node, "transport", &["pi-native"])?;
    check_nested(node, "discovery", check_discovery)?;
    let models = match node.get("models") {
        None => &[][..],
        Some(v) => v.as_array().ok_or_else(invalid_provider)?.as_slice(),
    };
    if models.is_empty() {
        let overrides = [
            "baseUrl",
            "headers",
            "apiKey",
            "compat",
            "disableStrictTools",
            "guardrailIdentifier",
            "requestMetadata",
            "remoteCompaction",
            "discovery",
        ];
        if !overrides.iter().any(|k| {
            node.get(k).is_some_and(|value| match value {
                Value::Bool(b) => *b,
                Value::String(s) => !s.is_empty(),
                _ => !value.is_null(),
            })
        }) && node.get("auth").and_then(Value::as_str) != Some("none")
            && node
                .get("modelOverrides")
                .and_then(Value::as_object)
                .is_none_or(|m| m.is_empty())
        {
            return Err(invalid_provider());
        }
    } else if node.get("baseUrl").is_none()
        || (node.get("apiKey").is_none()
            && !matches!(
                node.get("auth").and_then(Value::as_str),
                Some("none" | "oauth")
            ))
    {
        return Err(invalid_provider());
    }
    for model in models {
        check_model(model, false)?;
        check_type(model, &["baseUrl"], nonempty)?;
        if !model.get("id").is_some_and(nonempty) {
            return Err(invalid_provider());
        }
        let api = model
            .get("api")
            .or_else(|| node.get("api"))
            .and_then(Value::as_str);
        if api.is_none() {
            return Err(invalid_provider());
        }
        check_kind(model, api)?;
    }
    if let Some(overrides) = node.get("modelOverrides") {
        for (id, model) in object(overrides)? {
            check_model(model, true)?;
            let declared = models.iter().find(|model| model["id"] == *id);
            let api = model
                .get("api")
                .or_else(|| declared.and_then(|model| model.get("api").or_else(|| node.get("api"))))
                .and_then(Value::as_str);
            check_kind(model, api)?;
        }
    }
    // Proxy discovery supplies its API at runtime, as in omp's native validator.
    if let Some(discovery) = node.get("discovery") {
        if discovery.get("type").and_then(Value::as_str) != Some("proxy")
            && node.get("api").is_none()
        {
            return Err(invalid_provider());
        }
    }
    Ok(())
}

const EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max"];
const RUNNER_KINDS: &[(&str, &str)] = &[
    ("typesafe", "judge"),
    ("openrouter-decisions", "judge"),
    ("openai-decisions", "judge"),
    ("workers-ai-systemone", "judge"),
    ("openai-images", "image"),
    ("openrouter-images", "image"),
    ("xai-tts", "tts"),
    ("openai-speech", "tts"),
    ("openai-embeddings", "embedding"),
    ("openrouter-rerank", "rerank"),
    ("openrouter-video", "video"),
    ("openai-transcriptions", "stt"),
];

fn object(node: &Value) -> Result<&serde_json::Map<String, Value>, AppError> {
    node.as_object().ok_or_else(invalid_provider)
}

fn one_of(value: &Value, choices: &[&str]) -> bool {
    value.as_str().is_some_and(|s| choices.contains(&s))
}

fn check_enum(node: &Value, key: &str, choices: &[&str]) -> Result<(), AppError> {
    check_type(node, &[key], |v| one_of(v, choices))
}

fn check_nested(
    node: &Value,
    key: &str,
    check: fn(&Value) -> Result<(), AppError>,
) -> Result<(), AppError> {
    node.get(key).map_or(Ok(()), check)
}

fn string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|a| a.iter().all(Value::is_string))
}

fn nonempty(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.is_empty())
}

fn positive(value: &Value) -> bool {
    value.as_f64().is_some_and(|n| n > 0.0)
}

fn check_routing(node: &Value) -> Result<(), AppError> {
    object(node)?;
    check_type(node, &["only", "order"], string_array)
}

fn check_effort_map(node: &Value) -> Result<(), AppError> {
    object(node)?;
    check_type(node, EFFORTS, Value::is_string)
}

fn check_compat_fields(node: &Value) -> Result<(), AppError> {
    object(node)?;
    check_type(
        node,
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
        Value::is_boolean,
    )?;
    check_enum(
        node,
        "maxTokensField",
        &["max_completion_tokens", "max_tokens"],
    )?;
    check_enum(
        node,
        "reasoningContentField",
        &["reasoning_content", "reasoning", "reasoning_text"],
    )?;
    check_enum(
        node,
        "thinkingFormat",
        &["openai", "openrouter", "zai", "qwen", "qwen-chat-template"],
    )?;
    check_enum(node, "cacheControlFormat", &["anthropic"])?;
    check_enum(node, "toolStrictMode", &["all_strict", "none"])?;
    check_enum(
        node,
        "streamMarkupHealingPattern",
        &["kimi", "dsml", "qwen", "thinking"],
    )?;
    check_type(node, &["streamIdleTimeoutMs"], |v| {
        v.as_f64().is_some_and(|n| n >= 0.0)
    })?;
    check_type(node, &["extraBody"], Value::is_object)?;
    check_nested(node, "reasoningEffortMap", check_effort_map)?;
    check_nested(node, "openRouterRouting", check_routing)?;
    check_nested(node, "vercelGatewayRouting", check_routing)
}

fn check_compat(node: &Value) -> Result<(), AppError> {
    check_compat_fields(node)?;
    check_enum(node, "promptCacheMode", &["none", "automatic", "explicit"])?;
    check_type(
        node,
        &["supportsThinkingBindingControls"],
        Value::is_boolean,
    )?;
    check_type(
        node,
        &["promptCacheMinimumTokens", "promptCacheMaximumCheckpoints"],
        |v| v.as_f64().is_some_and(|n| n >= 0.0),
    )?;
    check_nested(node, "whenThinking", check_compat_fields)
}

fn check_remote_compaction(node: &Value) -> Result<(), AppError> {
    object(node)?;
    check_type(node, &["enabled", "v2StreamingEnabled"], Value::is_boolean)?;
    check_type(
        node,
        &["endpoint", "model", "v2Endpoint", "streamingEndpoint"],
        nonempty,
    )?;
    check_enum(node, "api", APIS)
}

fn check_thinking(node: &Value) -> Result<(), AppError> {
    object(node)?;
    if !node.get("mode").is_some_and(|v| {
        one_of(
            v,
            &[
                "effort",
                "budget",
                "google-level",
                "anthropic-adaptive",
                "anthropic-budget-effort",
            ],
        )
    }) {
        return Err(invalid_provider());
    }
    check_type(node, &["efforts", "levels"], |v| {
        v.as_array()
            .is_some_and(|a| a.iter().all(|v| one_of(v, EFFORTS)))
    })?;
    for key in ["defaultLevel", "minLevel", "maxLevel"] {
        check_enum(node, key, EFFORTS)?;
    }
    check_type(
        node,
        &["supportsDisplay", "requiresEffort"],
        Value::is_boolean,
    )?;
    check_nested(node, "effortMap", check_effort_map)?;
    if node.get("efforts").is_none()
        && node.get("levels").is_none()
        && (node.get("minLevel").is_none() || node.get("maxLevel").is_none())
    {
        return Err(invalid_provider());
    }
    Ok(())
}

fn check_discovery(node: &Value) -> Result<(), AppError> {
    object(node)?;
    if !node.get("type").is_some_and(|v| {
        one_of(
            v,
            &[
                "ollama",
                "llama.cpp",
                "lm-studio",
                "openai-models-list",
                "proxy",
                "litellm",
                "apple-foundation-models",
            ],
        )
    }) {
        return Err(invalid_provider());
    }
    check_type(node, &["timeoutMs"], positive)?;
    check_type(node, &["injectV1"], Value::is_boolean)?;
    if node.get("injectV1").is_some() && node["type"] != "openai-models-list" {
        return Err(invalid_provider());
    }
    Ok(())
}

fn check_model(node: &Value, is_override: bool) -> Result<(), AppError> {
    check_common(node)?;
    check_type(node, &["api"], |v| {
        one_of(v, APIS)
            || v.as_str()
                .is_some_and(|api| RUNNER_KINDS.iter().any(|(a, _)| *a == api))
    })?;
    check_type(
        node,
        &["name", "contextPromotionTarget", "compactionModel"],
        nonempty,
    )?;
    check_enum(
        node,
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
    check_enum(node, "imageInputDecoder", &["stb"])?;
    check_enum(
        node,
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
            "o200k",
        ],
    )?;
    check_type(
        node,
        &[
            "reasoning",
            "supportsTools",
            "omitMaxOutputTokens",
            "preferWebsockets",
        ],
        Value::is_boolean,
    )?;
    check_type(node, &["input"], |v| {
        v.as_array()
            .is_some_and(|a| a.iter().all(|v| one_of(v, &["text", "image"])))
    })?;
    check_type(node, &["premiumMultiplier"], Value::is_number)?;
    check_type(node, &["contextWindow", "maxTokens"], |v| {
        if is_override {
            v.is_number()
        } else {
            positive(v)
        }
    })?;
    if let Some(max) = node.get("maxContextWindow") {
        let valid = max.as_f64().is_some_and(|n| {
            n > 0.0
                && n.fract() == 0.0
                && n <= 9_007_199_254_740_991.0
                && node
                    .get("contextWindow")
                    .and_then(Value::as_f64)
                    .is_none_or(|context| n >= context)
        });
        if !valid {
            return Err(invalid_provider());
        }
    }
    if let Some(cost) = node.get("cost") {
        object(cost)?;
        let keys = &["input", "output", "cacheRead", "cacheWrite"];
        check_type(cost, keys, Value::is_number)?;
        if !is_override && keys.iter().any(|k| cost.get(k).is_none()) {
            return Err(invalid_provider());
        }
    }
    if let Some(cache) = node.get("promptCache") {
        object(cache)?;
        check_type(cache, &["short", "long"], Value::is_number)?;
    }
    check_nested(node, "thinking", check_thinking)
}

fn check_kind(node: &Value, api: Option<&str>) -> Result<(), AppError> {
    let (Some(kind), Some(api)) = (node.get("kind").and_then(Value::as_str), api) else {
        return Ok(());
    };
    let valid = if let Some((_, served)) = RUNNER_KINDS.iter().find(|(a, _)| *a == api) {
        kind == *served
    } else {
        matches!(kind, "chat" | "tiny")
            || (kind == "image"
                && [
                    "google-generative-ai",
                    "google-gemini-cli",
                    "openai-responses",
                    "openai-codex-responses",
                ]
                .contains(&api))
    };
    if !valid {
        return Err(invalid_provider());
    }
    Ok(())
}
