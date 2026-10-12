//! Provider import from deep link
//!
//! Handles importing provider configurations via ccswitch:// URLs.

use super::codex_link::{self, CodexLinkConfig};
use super::grok_link::GrokLinkProfile;
use super::utils::{decode_base64_param, infer_homepage_from_endpoint};
use super::DeepLinkImportRequest;
use crate::error::AppError;
use crate::provider::{ClaudeDesktopMode, Provider, ProviderMeta, UsageScript};
use crate::services::ProviderService;
use crate::store::AppState;
use crate::AppType;
use serde_json::json;
use std::str::FromStr;

/// Import a provider from a deep link request
///
/// This function:
/// 1. Validates the request
/// 2. Merges config file if provided (v3.8+)
/// 3. Converts it to a Provider structure
/// 4. Delegates to ProviderService for actual import
/// 5. Optionally sets as current provider if enabled=true
pub fn import_provider_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    // Verify this is a provider request
    if request.resource != "provider" {
        return Err(AppError::InvalidInput(format!(
            "Expected provider resource, got '{}'",
            request.resource
        )));
    }

    // Step 1: Merge config file if provided (v3.8+)
    let mut merged_request = parse_and_merge_config(&request)?;

    // Extract required fields (now as Option)
    let app_str = merged_request
        .app
        .clone()
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' field for provider".to_string()))?;

    let api_key = merged_request.api_key.as_ref().ok_or_else(|| {
        AppError::InvalidInput("API key is required (either in URL or config file)".to_string())
    })?;

    if api_key.is_empty() {
        return Err(AppError::InvalidInput(
            "API key cannot be empty".to_string(),
        ));
    }

    // Get endpoint: supports comma-separated multiple URLs (first is primary)
    let endpoint_str = merged_request.endpoint.as_ref().ok_or_else(|| {
        AppError::InvalidInput("Endpoint is required (either in URL or config file)".to_string())
    })?;

    // Parse endpoints: split by comma, first is primary
    let all_endpoints: Vec<String> = endpoint_str
        .split(',')
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect();

    let primary_endpoint = all_endpoints
        .first()
        .ok_or_else(|| AppError::InvalidInput("Endpoint cannot be empty".to_string()))?;

    // Auto-infer homepage from endpoint if not provided
    if merged_request
        .homepage
        .as_ref()
        .is_none_or(|s| s.is_empty())
    {
        merged_request.homepage = infer_homepage_from_endpoint(primary_endpoint);
    }

    let homepage = merged_request.homepage.as_ref().ok_or_else(|| {
        AppError::InvalidInput("Homepage is required (either in URL or config file)".to_string())
    })?;

    if homepage.is_empty() {
        return Err(AppError::InvalidInput(
            "Homepage cannot be empty".to_string(),
        ));
    }

    let name = merged_request
        .name
        .clone()
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' field for provider".to_string()))?;

    // Parse app type
    let app_type = AppType::from_str(&app_str)
        .map_err(|_| AppError::InvalidInput(format!("Invalid app type: {app_str}")))?;

    // Build provider configuration based on app type
    let mut provider = build_provider_from_request(&app_type, &merged_request)?;

    // Generate a unique ID for the provider using timestamp + sanitized name
    let timestamp = chrono::Utc::now().timestamp_millis();
    let sanitized_name = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase();
    provider.id = format!("{sanitized_name}-{timestamp}");

    let provider_id = provider.id.clone();

    // `enabled` 是确认框里「导入后立即切换 / 添加到应用」开关的值（链接里的 `enabled` 只是它的
    // 初始值）。累加式应用（OpenCode、OpenClaw、Hermes、Pi）没有「切换」：开关决定这家要不要
    // 写进应用的配置文件；旧链接没写时照旧写进去。切换式应用开关关闭时只存进列表：`add` 在
    // 还没有当前供应商时会让这家直接生效，所以走不写客户端配置的路径。
    let additive = app_type.is_additive_mode();
    let activate = if additive {
        merged_request.enabled.unwrap_or(true)
    } else {
        merged_request.enabled.unwrap_or(false)
    };

    if activate || additive {
        ProviderService::add(state, app_type.clone(), provider, activate)?;
    } else {
        ProviderService::add_without_switching(state, app_type.clone(), provider)?;
    }

    // Add extra endpoints as custom endpoints (skip first one as it's the primary)
    for ep in all_endpoints.iter().skip(1) {
        let normalized = ep.trim().trim_end_matches('/').to_string();
        if !normalized.is_empty() {
            if let Err(e) = ProviderService::add_custom_endpoint(
                state,
                app_type.clone(),
                &provider_id,
                normalized.clone(),
            ) {
                log::warn!(
                    "Failed to add custom endpoint '{}': {e}",
                    crate::url_for_log(&normalized)
                );
            }
        }
    }

    // If enabled=true, set as current provider
    if !additive && activate {
        ProviderService::switch(state, app_type.clone(), &provider_id)?;
        log::info!("Provider '{provider_id}' set as current for {app_type:?}");
    }

    Ok(provider_id)
}

/// 链接声明的接口格式（`apiFormat`）。站长只学这一套词，各应用换算成自己的字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiFormat {
    Anthropic,
    OpenAiChat,
    OpenAiResponses,
    GeminiNative,
}

impl ApiFormat {
    const VALUES: &'static str = "anthropic, openai_chat, openai_responses, gemini_native";

    fn parse(value: &str) -> Result<Self, AppError> {
        match value.trim() {
            "anthropic" => Ok(Self::Anthropic),
            "openai_chat" => Ok(Self::OpenAiChat),
            "openai_responses" => Ok(Self::OpenAiResponses),
            "gemini_native" => Ok(Self::GeminiNative),
            other => Err(AppError::InvalidInput(format!(
                "Unsupported apiFormat '{other}'; use one of: {}",
                Self::VALUES
            ))),
        }
    }

    /// 这个格式在应用里对应的值；应用不支持这种接口时为 `None`。
    ///
    /// Claude Code、Codex、Grok Build 写进 `meta.apiFormat`（非原生格式由本地路由转换）；
    /// 其余应用写进各自配置里选择协议的字段：OpenCode 的 `npm`、OpenClaw / Pi 的 `api`、
    /// Hermes 的 `api_mode`。Gemini CLI 只有原生接口。
    fn native_value(self, app_type: &AppType) -> Option<&'static str> {
        use ApiFormat::*;
        match (app_type, self) {
            (AppType::Claude, Anthropic) => Some("anthropic"),
            (AppType::Claude | AppType::Codex | AppType::GrokBuild, OpenAiChat) => {
                Some("openai_chat")
            }
            (AppType::Claude | AppType::Codex | AppType::GrokBuild, OpenAiResponses) => {
                Some("openai_responses")
            }
            (AppType::Claude, GeminiNative) => Some("gemini_native"),
            (AppType::Codex | AppType::GrokBuild, Anthropic) => Some("anthropic"),
            (AppType::Gemini, GeminiNative) => Some("gemini_native"),
            (AppType::OpenCode, Anthropic) => Some("@ai-sdk/anthropic"),
            (AppType::OpenCode, OpenAiChat) => Some("@ai-sdk/openai-compatible"),
            (AppType::OpenCode, OpenAiResponses) => Some("@ai-sdk/openai"),
            (AppType::OpenCode, GeminiNative) => Some("@ai-sdk/google"),
            (AppType::OpenClaw | AppType::Pi, Anthropic) => Some("anthropic-messages"),
            (AppType::OpenClaw | AppType::Pi, OpenAiChat) => Some("openai-completions"),
            (AppType::OpenClaw | AppType::Pi, OpenAiResponses) => Some("openai-responses"),
            (AppType::OpenClaw | AppType::Pi, GeminiNative) => Some("google-generative-ai"),
            (AppType::Hermes, Anthropic) => Some("anthropic_messages"),
            (AppType::Hermes, OpenAiChat) => Some("chat_completions"),
            (AppType::Hermes, OpenAiResponses) => Some("codex_responses"),
            _ => None,
        }
    }

    /// 链接没写 `apiFormat` 时各应用用的格式（保持旧链接的行为）。
    fn default_for(app_type: &AppType) -> Self {
        match app_type {
            AppType::Claude | AppType::ClaudeDesktop => Self::Anthropic,
            AppType::Codex | AppType::GrokBuild => Self::OpenAiResponses,
            AppType::Gemini => Self::GeminiNative,
            _ => Self::OpenAiChat,
        }
    }
}

/// 核对链接的 `apiFormat` 在这个应用上是否可用（解析和导入两处都调）。
pub(super) fn check_api_format(app: &str, value: &str) -> Result<(), AppError> {
    let app_type = AppType::from_str(app)
        .map_err(|_| AppError::InvalidInput(format!("Invalid app type: {app}")))?;
    let format = ApiFormat::parse(value)?;
    if format.native_value(&app_type).is_none() {
        return Err(AppError::InvalidInput(format!(
            "apiFormat '{}' is not supported for app '{app}'",
            value.trim()
        )));
    }
    Ok(())
}

/// 链接声明的格式，已核对过这个应用支持它；没写时为 `None`。
fn link_api_format(
    app_type: &AppType,
    request: &DeepLinkImportRequest,
) -> Result<Option<ApiFormat>, AppError> {
    let Some(value) = request.api_format.as_deref() else {
        return Ok(None);
    };
    check_api_format(app_type.as_str(), value)?;
    ApiFormat::parse(value).map(Some)
}

/// 应用配置里选择协议的字段值：链接写了就按链接，没写用应用默认。
fn native_protocol(app_type: &AppType, api_format: Option<ApiFormat>) -> &'static str {
    let format = api_format.unwrap_or_else(|| ApiFormat::default_for(app_type));
    format
        .native_value(app_type)
        .or_else(|| ApiFormat::default_for(app_type).native_value(app_type))
        .unwrap_or_default()
}

/// 累加式应用读配置时会解释的字符串写法，链接带了就拒绝导入。
///
/// - Pi：`apiKey` 以 `!` 开头会被当成 shell 命令执行；`$NAME`、`${NAME}` 会换成环境变量。
/// - OpenCode：`{env:NAME}` 读环境变量，`{file:path}` 读本机文件。
/// - OpenClaw、Hermes：`${NAME}` 读环境变量。
///
/// 链接是不可信输入：这些写法会让客户端执行链接里的命令，或把本机的密钥、文件内容随请求
/// 发到链接给的地址。确认框把 Key 打码显示，用户也看不出来，所以在后端拒绝。
/// 名称、地址、Key、模型都会写进这些应用的配置，都要查。
pub(super) fn check_link_values(
    app_type: &AppType,
    request: &DeepLinkImportRequest,
) -> Result<(), AppError> {
    if !app_type.is_additive_mode() {
        return Ok(());
    }
    let is_pi = matches!(app_type, AppType::Pi);
    let fields = [
        ("name", request.name.as_deref()),
        ("endpoint", request.endpoint.as_deref()),
        ("apiKey", request.api_key.as_deref()),
        ("model", request.model.as_deref()),
    ];
    for (field, value) in fields {
        let Some(value) = value else { continue };
        let command = is_pi && field == "apiKey" && value.trim_start().starts_with('!');
        if command || has_dynamic_reference(value, is_pi) {
            return Err(AppError::InvalidInput(format!(
                "The link's '{field}' uses syntax that {} would expand into a command, an \
                 environment variable or a local file; it cannot be imported from a link. Add the \
                 provider manually instead.",
                app_type.as_str()
            )));
        }
    }
    Ok(())
}

/// 值里有没有环境变量或文件引用。`${` 和 OpenCode 的 `{env:` / `{file:` 对所有累加式应用
/// 都拒绝；Pi 还会展开不带花括号的 `$NAME`。
fn has_dynamic_reference(value: &str, is_pi: bool) -> bool {
    if value.contains("${") || value.contains("{env:") || value.contains("{file:") {
        return true;
    }
    is_pi
        && value.char_indices().any(|(index, ch)| {
            ch == '$'
                && value[index + 1..]
                    .chars()
                    .next()
                    .is_some_and(|next| next == '_' || next.is_ascii_alphabetic())
        })
}

/// Build a Provider structure from a deep link request
pub(crate) fn build_provider_from_request(
    app_type: &AppType,
    request: &DeepLinkImportRequest,
) -> Result<Provider, AppError> {
    let api_format = link_api_format(app_type, request)?;
    check_link_values(app_type, request)?;
    let settings_config = match app_type {
        AppType::Claude | AppType::ClaudeDesktop => build_claude_settings(request),
        AppType::Codex => build_codex_settings(request)?,
        AppType::Gemini => build_gemini_settings(request),
        AppType::GrokBuild => build_grokbuild_settings(request)?,
        AppType::OpenCode => build_opencode_settings(request, api_format),
        AppType::OpenClaw => build_additive_app_settings(request, api_format),
        AppType::Hermes => build_hermes_settings(request, api_format),
        AppType::Pi => build_pi_settings(request, api_format)?,
        AppType::Mcode => {
            return Err(AppError::InvalidInput(
                "Add MCode providers from the MCode page".into(),
            ))
        }
    };

    // Build usage script configuration if provided
    let mut meta = build_provider_meta(request)?;
    if matches!(app_type, AppType::ClaudeDesktop) {
        meta.get_or_insert_with(ProviderMeta::default)
            .claude_desktop_mode = Some(ClaudeDesktopMode::Direct);
    }
    // Claude Code、Codex、Grok Build 的格式记在 meta 上：非原生格式的卡片会带「需要路由」，
    // 切过去时由本地路由转换。链接没写时不加这个字段，和旧链接导入的行一致。
    if matches!(
        app_type,
        AppType::Claude | AppType::Codex | AppType::GrokBuild
    ) {
        if let Some(value) = api_format.and_then(|format| format.native_value(app_type)) {
            meta.get_or_insert_with(ProviderMeta::default).api_format = Some(value.to_string());
        }
    }

    let provider = Provider {
        id: String::new(), // Will be generated by caller
        name: request.name.clone().unwrap_or_default(),
        settings_config,
        website_url: request.homepage.clone(),
        category: None,
        created_at: None,
        sort_index: None,
        notes: request.notes.clone(),
        meta,
        icon: request.icon.clone(),
        icon_color: None,
        in_failover_queue: false,
    };

    Ok(provider)
}

/// Get primary endpoint from request (first one if comma-separated)
fn get_primary_endpoint(request: &DeepLinkImportRequest) -> String {
    request
        .endpoint
        .as_ref()
        .and_then(|ep| ep.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn normalize_deeplink_api_key(api_key: &str) -> String {
    api_key.trim().to_string()
}

fn normalize_deeplink_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn usage_api_key_override(request: &DeepLinkImportRequest) -> Option<String> {
    let usage_api_key = normalize_deeplink_api_key(request.usage_api_key.as_deref()?);
    if usage_api_key.is_empty() {
        return None;
    }

    let provider_api_key = request
        .api_key
        .as_deref()
        .map(normalize_deeplink_api_key)
        .unwrap_or_default();

    if !provider_api_key.is_empty() && usage_api_key == provider_api_key {
        None
    } else {
        Some(usage_api_key)
    }
}

fn usage_base_url_override(request: &DeepLinkImportRequest) -> Option<String> {
    let usage_base_url = normalize_deeplink_base_url(request.usage_base_url.as_deref()?);
    if usage_base_url.is_empty() {
        return None;
    }

    let provider_base_url = normalize_deeplink_base_url(&get_primary_endpoint(request));

    if !provider_base_url.is_empty() && usage_base_url == provider_base_url {
        None
    } else {
        Some(usage_base_url)
    }
}

/// Build provider meta with usage script configuration
fn build_provider_meta(request: &DeepLinkImportRequest) -> Result<Option<ProviderMeta>, AppError> {
    // Check if any usage script fields are provided
    if request.usage_script.is_none()
        && request.usage_enabled.is_none()
        && request.usage_api_key.is_none()
        && request.usage_base_url.is_none()
        && request.usage_access_token.is_none()
        && request.usage_user_id.is_none()
        && request.usage_auto_interval.is_none()
    {
        return Ok(None);
    }

    // Decode usage script code if provided
    let code = if let Some(script_b64) = &request.usage_script {
        let decoded = decode_base64_param("usage_script", script_b64)?;
        String::from_utf8(decoded)
            .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in usage_script: {e}")))?
    } else {
        String::new()
    };

    // 启用状态：链接写了 `usageEnabled` 就照办；没写时，带了脚本就启用。
    //
    // 站长放脚本就是为了让用户看到余额，导入后还要再手动打开一次是多余的一步。
    // 默认启用的代价可控：脚本跑在受限的 QuickJS 里，只能发一个请求，且必须是
    // 与 base_url 同源的 HTTPS（`usage_script::validate_request_url`；深链接导入
    // 的脚本没有 template_type，不算自定义模板，这项检查不会被跳过）。
    //
    // 用户的同意仍然体现在确认框：完整的脚本正文和启用徽章都展示过之后才点导入，
    // 所以那两处展示是本设计的承重部分，不可省略。前端徽章的判断必须和这里一致。
    // 链接想导入但不启用，就显式写 `usageEnabled=false`。
    let enabled = request.usage_enabled.unwrap_or(!code.is_empty());

    let usage_script = UsageScript {
        enabled,
        language: "javascript".to_string(),
        code,
        timeout: Some(10),
        api_key: usage_api_key_override(request),
        base_url: usage_base_url_override(request),
        access_token: request.usage_access_token.clone(),
        user_id: request.usage_user_id.clone(),
        template_type: None, // Deeplink providers don't specify template type (will use backward compatibility logic)
        auto_query_interval: request.usage_auto_interval,
        coding_plan_provider: None,
        access_key_id: None,
        secret_access_key: None,
        team_organization_id: None,
        team_project_id: None,
    };

    Ok(Some(ProviderMeta {
        usage_script: Some(usage_script),
        ..Default::default()
    }))
}

/// Build Claude settings configuration
///
/// Merges env from the inline config (if any) with the standard fields from URL params.
/// URL params take priority — they overwrite same-named fields from the config.
/// Non-standard env fields (e.g. `ANTHROPIC_CUSTOM_HEADERS`, `API_TIMEOUT_MS`,
/// `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS`, ...) are preserved as-is so that
/// providers requiring extra environment variables work after deeplink import.
fn build_claude_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    // Start from the full env block in the inline config (if present), so any
    // custom env vars the user passed via `config=<base64-json>` survive the
    // import. Falling back to an empty map keeps the previous behavior for
    // deeplinks that don't carry a config field.
    let mut env = extract_claude_config_env(request).unwrap_or_default();

    // Now overwrite / fill in the standard fields from URL params. URL params
    // are authoritative because they're what the deeplink builder put on the
    // wire — for Claude these are: ANTHROPIC_AUTH_TOKEN, ANTHROPIC_BASE_URL,
    // ANTHROPIC_MODEL, and the haiku/sonnet/opus model aliases.
    env.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        json!(request.api_key.clone().unwrap_or_default()),
    );
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        json!(get_primary_endpoint(request)),
    );

    // Add default model if provided
    if let Some(model) = &request.model {
        env.insert("ANTHROPIC_MODEL".to_string(), json!(model));
    }

    // Add Claude-specific model fields (v3.7.1+)
    if let Some(haiku_model) = &request.haiku_model {
        env.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
            json!(haiku_model),
        );
    }
    if let Some(sonnet_model) = &request.sonnet_model {
        env.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
            json!(sonnet_model),
        );
    }
    if let Some(opus_model) = &request.opus_model {
        env.insert(
            "ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(),
            json!(opus_model),
        );
    }

    json!({ "env": env })
}

/// Decode and extract the `env` object from the deeplink's inline config payload.
///
/// Returns `None` when no config is attached, when the payload can't be
/// decoded/parsed, or when it doesn't contain a Claude-style `env` object.
/// This is a best-effort accessor — we deliberately don't surface parse
/// errors here because `parse_and_merge_config` will have already validated
/// the payload during the merge phase; any failure at this point just means
/// "fall back to URL-param-only behavior".
fn extract_claude_config_env(
    request: &DeepLinkImportRequest,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    // Only the inline base64 config carries an env block. Remote config_url
    // is not implemented yet (see parse_and_merge_config), so nothing else to
    // try here.
    let config_b64 = request.config.as_ref()?;

    // Honor the declared format; default to JSON like parse_and_merge_config does.
    let format = request.config_format.as_deref().unwrap_or("json");
    if format != "json" {
        // Claude config is always JSON in practice. TOML/other formats aren't
        // expected on this app path, so don't try to handle them — safer to
        // fall back than to risk silently producing the wrong shape.
        return None;
    }

    // Decode the base64 payload. We re-decode here rather than threading the
    // already-decoded value through every build_* function, because the call
    // graph (parse_and_merge_config is pub and called separately for preview)
    // makes signature changes invasive. Decode cost is negligible on this
    // one-shot import path.
    let decoded = decode_base64_param("config", config_b64).ok()?;
    let json_str = std::str::from_utf8(&decoded).ok()?;
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    // Pull out the env object — same shape as Claude's own settings.json.
    value.get("env").and_then(|v| v.as_object()).cloned()
}

/// Build Codex settings configuration
///
/// 链接附带了 `config.toml` 时，按白名单收下其中属于这个供应商的部分（[`codex_link`]）；
/// 没带时用固定模板。模板的字节由 golden 测试锁住：降级后旧版会把行里的 config 整份写进
/// live，已发布的行形状不能变。
fn build_codex_settings(request: &DeepLinkImportRequest) -> Result<serde_json::Value, AppError> {
    let provider_display_name = request
        .name
        .as_deref()
        .unwrap_or("custom")
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    let provider_display_name = if provider_display_name.is_empty() {
        "custom".to_string()
    } else {
        provider_display_name
    };

    // Model name: use deeplink model or default
    let model_name = request
        .model
        .as_deref()
        .unwrap_or("gpt-5-codex")
        .to_string();

    // Endpoint: normalize trailing slashes (use primary endpoint only)
    let endpoint = get_primary_endpoint(request)
        .trim()
        .trim_end_matches('/')
        .to_string();

    let link_config = match decode_link_config(request)? {
        Some(config) => codex_link::config_text(&config)?,
        None => None,
    };
    if let Some(text) = link_config {
        let config = CodexLinkConfig::parse(&text)?.render(
            &provider_display_name,
            &endpoint,
            &model_name,
            request.api_key.as_deref().unwrap_or_default(),
        )?;
        return Ok(json!({
            "auth": {
                "OPENAI_API_KEY": request.api_key,
            },
            "config": config
        }));
    }

    let provider_display_name = toml_edit::Value::from(provider_display_name.as_str()).to_string();
    let model_name = toml_edit::Value::from(model_name.as_str()).to_string();
    let endpoint = toml_edit::Value::from(endpoint.as_str()).to_string();

    // Build config.toml content
    let config_toml = format!(
        r#"model_provider = "custom"
model = {model_name}
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = {provider_display_name}
base_url = {endpoint}
wire_api = "responses"
requires_openai_auth = true
"#
    );

    Ok(json!({
        "auth": {
            "OPENAI_API_KEY": request.api_key,
        },
        "config": config_toml
    }))
}

/// Build Gemini settings configuration
fn build_gemini_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let mut env = serde_json::Map::new();
    env.insert("GEMINI_API_KEY".to_string(), json!(request.api_key));
    env.insert(
        "GOOGLE_GEMINI_BASE_URL".to_string(),
        json!(get_primary_endpoint(request)),
    );

    // Add model if provided
    if let Some(model) = &request.model {
        env.insert("GEMINI_MODEL".to_string(), json!(model));
    }

    json!({ "env": env })
}

/// 链接附带了 `config.toml` 时，按白名单收下选中的那张模型表（[`grok_link`]）；没带时用
/// 固定模板。模板的字节由 golden 测试锁住（降级后旧版会把行里的 config 整份写进 live）。
fn build_grokbuild_settings(
    request: &DeepLinkImportRequest,
) -> Result<serde_json::Value, AppError> {
    let model = request
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(crate::grok_config::DEFAULT_MODEL)
        .trim();
    let name = request
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("custom")
        .trim();
    let endpoint = get_primary_endpoint(request).trim().to_string();
    let api_key = request.api_key.as_deref().unwrap_or("").trim();

    if let Some(config) = decode_link_config(request)? {
        let profile = GrokLinkProfile::parse(&grokbuild_config_text(&config)?)?;
        return Ok(json!({ "config": profile.render(name, model, &endpoint, api_key) }));
    }

    let model_value = toml_edit::Value::from(model).to_string();
    let name_value = toml_edit::Value::from(name).to_string();
    let endpoint_value = toml_edit::Value::from(endpoint.as_str()).to_string();
    let api_key_value = toml_edit::Value::from(api_key).to_string();

    Ok(json!({
        "config": format!(
            "[models]\ndefault = {model_value}\n\n[model.{model_value}]\nmodel = {model_value}\nbase_url = {endpoint_value}\nname = {name_value}\napi_key = {api_key_value}\napi_backend = \"{}\"\ncontext_window = {}\n",
            crate::grok_config::DEFAULT_API_BACKEND,
            crate::grok_config::DEFAULT_CONTEXT_WINDOW,
        )
    }))
}

/// Build OpenCode settings configuration
fn build_opencode_settings(
    request: &DeepLinkImportRequest,
    api_format: Option<ApiFormat>,
) -> serde_json::Value {
    let endpoint = get_primary_endpoint(request);

    // Build options object
    let mut options = serde_json::Map::new();
    if !endpoint.is_empty() {
        options.insert("baseURL".to_string(), json!(endpoint));
    }
    if let Some(api_key) = &request.api_key {
        options.insert("apiKey".to_string(), json!(api_key));
    }

    // Build models object
    let mut models = serde_json::Map::new();
    if let Some(model) = &request.model {
        models.insert(model.clone(), json!({ "name": model }));
    }

    // Default to openai-compatible npm package
    json!({
        "npm": native_protocol(&AppType::OpenCode, api_format),
        "options": options,
        "models": models
    })
}

/// Build settings for OpenClaw (camelCase live config).
/// Format: { baseUrl, apiKey, api, models }
fn build_additive_app_settings(
    request: &DeepLinkImportRequest,
    api_format: Option<ApiFormat>,
) -> serde_json::Value {
    let endpoint = get_primary_endpoint(request);

    let mut config = serde_json::Map::new();

    if !endpoint.is_empty() {
        config.insert("baseUrl".to_string(), json!(endpoint));
    }

    if let Some(api_key) = &request.api_key {
        config.insert("apiKey".to_string(), json!(api_key));
    }

    config.insert(
        "api".to_string(),
        json!(native_protocol(&AppType::OpenClaw, api_format)),
    );

    if let Some(model) = &request.model {
        config.insert(
            "models".to_string(),
            json!([{ "id": model, "name": model }]),
        );
    }

    json!(config)
}

/// Build Hermes provider settings (snake_case YAML-native fields).
///
/// Hermes' `custom_providers:` entries use `base_url` / `api_key` / `api_mode`
/// (see `_VALID_CUSTOM_PROVIDER_FIELDS` in upstream `hermes_cli/config.py`).
/// Emitting camelCase here — as the OpenClaw path does — would poison the
/// YAML with unknown root fields the Hermes runtime ignores.
///
/// `api_mode` is always written explicitly. Deeplinks have no field to carry
/// it, so we default to `chat_completions` (the most widely compatible
/// protocol) and let the user adjust via the UI after import. We never rely
/// on Hermes' built-in URL heuristics, which only recognize a handful of
/// official endpoints.
fn build_hermes_settings(
    request: &DeepLinkImportRequest,
    api_format: Option<ApiFormat>,
) -> serde_json::Value {
    let endpoint = get_primary_endpoint(request);

    let mut config = serde_json::Map::new();

    if let Some(name) = request.name.as_deref().filter(|s| !s.is_empty()) {
        config.insert("name".to_string(), json!(name));
    }

    if !endpoint.is_empty() {
        config.insert("base_url".to_string(), json!(endpoint));
    }

    if let Some(api_key) = &request.api_key {
        config.insert("api_key".to_string(), json!(api_key));
    }

    config.insert(
        "api_mode".to_string(),
        json!(native_protocol(&AppType::Hermes, api_format)),
    );

    if let Some(model) = &request.model {
        config.insert(
            "models".to_string(),
            json!([{ "id": model, "name": model }]),
        );
    }

    json!(config)
}

/// Pi 的 `models.json` 供应商节点，形状同 Pi 页面的表单：显示名、地址、Key、协议和模型列表。
///
/// 节点名是供应商 ID（名称加时间戳），不会和 Pi 内置供应商重名。模型必填：没有模型的节点
/// 在 Pi 里选不到任何模型，导入了也用不上。
fn build_pi_settings(
    request: &DeepLinkImportRequest,
    api_format: Option<ApiFormat>,
) -> Result<serde_json::Value, AppError> {
    let model = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| {
            AppError::InvalidInput("Pi providers need a model; add 'model' to the link".to_string())
        })?;

    let mut config = serde_json::Map::new();
    if let Some(name) = request.name.as_deref().filter(|name| !name.is_empty()) {
        config.insert("name".to_string(), json!(name));
    }
    let endpoint = get_primary_endpoint(request);
    if !endpoint.is_empty() {
        config.insert("baseUrl".to_string(), json!(endpoint));
    }
    if let Some(api_key) = &request.api_key {
        config.insert("apiKey".to_string(), json!(api_key));
    }
    config.insert(
        "api".to_string(),
        json!(native_protocol(&AppType::Pi, api_format)),
    );
    config.insert(
        "models".to_string(),
        json!([{ "id": model, "name": model }]),
    );
    Ok(json!(config))
}

// =============================================================================
// Config Merge Logic
// =============================================================================

/// 解码链接的 `config` 参数（Base64），按 `configFormat` 解析成 JSON 值。没带时返回 `None`。
fn decode_link_config(
    request: &DeepLinkImportRequest,
) -> Result<Option<serde_json::Value>, AppError> {
    let config_content = if let Some(config_b64) = &request.config {
        let decoded = decode_base64_param("config", config_b64)?;
        String::from_utf8(decoded)
            .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in config: {e}")))?
    } else if request.config_url.is_some() {
        return Err(AppError::InvalidInput(
            "Remote config URL is not yet supported. Use inline config instead.".to_string(),
        ));
    } else {
        return Ok(None);
    };

    let format = request.config_format.as_deref().unwrap_or("json");
    let config_value = match format {
        "json" => serde_json::from_str(&config_content)
            .map_err(|e| AppError::InvalidInput(format!("Invalid JSON config: {e}")))?,
        "toml" => {
            let toml_value: toml::Value = toml::from_str(&config_content)
                .map_err(|e| AppError::InvalidInput(format!("Invalid TOML config: {e}")))?;
            serde_json::to_value(toml_value)
                .map_err(|e| AppError::Message(format!("Failed to convert TOML to JSON: {e}")))?
        }
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Unsupported config format: {format}"
            )))
        }
    };
    Ok(Some(config_value))
}

/// Parse and merge configuration from Base64 encoded config or remote URL
///
/// Priority: URL params > inline config > remote config
pub fn parse_and_merge_config(
    request: &DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, AppError> {
    let Some(config_value) = decode_link_config(request)? else {
        return Ok(request.clone());
    };

    // Extract values from config based on app type and merge with URL params
    let mut merged = request.clone();

    // MCP, Skill and other resource types don't need config merging
    if request.resource != "provider" {
        return Ok(merged);
    }

    match request.app.as_deref().unwrap_or("") {
        "claude" => merge_claude_config(&mut merged, &config_value)?,
        "codex" => merge_codex_config(&mut merged, &config_value)?,
        "gemini" => merge_gemini_config(&mut merged, &config_value)?,
        "grokbuild" => merge_grokbuild_config(&mut merged, &config_value)?,
        // Additive mode apps use JSON config directly; pass through as-is
        "openclaw" | "opencode" | "hermes" | "pi" => {
            merge_additive_config(&mut merged, &config_value)?;
            // 从 config 里补来的地址和 Key 同样要查，让确认框当场报错
            if let Ok(app_type) = AppType::from_str(request.app.as_deref().unwrap_or("")) {
                check_link_values(&app_type, &merged)?;
            }
        }
        "" => {
            // No app specified, skip merging
            return Ok(merged);
        }
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Invalid app type: {:?}",
                request.app
            )))
        }
    }

    Ok(merged)
}

/// Merge Claude configuration from config file
fn merge_claude_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    let env = config
        .get("env")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            AppError::InvalidInput("Claude config must have 'env' object".to_string())
        })?;

    // Auto-fill API key if not provided in URL
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(token) = env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()) {
            request.api_key = Some(token.to_string());
        }
    }

    // Auto-fill endpoint if not provided in URL
    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()) {
            request.endpoint = Some(base_url.to_string());
        }
    }

    // Auto-fill homepage from endpoint if not provided
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
            if request.homepage.is_none() {
                request.homepage = Some("https://anthropic.com".to_string());
            }
        }
    }

    // Auto-fill model fields (URL params take priority)
    if request.model.is_none() {
        request.model = env
            .get("ANTHROPIC_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if request.haiku_model.is_none() {
        request.haiku_model = env
            .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if request.sonnet_model.is_none() {
        request.sonnet_model = env
            .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if request.opus_model.is_none() {
        request.opus_model = env
            .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }

    Ok(())
}

/// Merge Codex configuration from config file
fn merge_codex_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    let config_text = codex_link::config_text(config)?;
    // 先过白名单：拒绝的键在确认框打开时就报出来，而不是点了导入才失败
    let link = config_text
        .as_deref()
        .map(CodexLinkConfig::parse)
        .transpose()?;

    // Auto-fill API key from auth.OPENAI_API_KEY or Codex mobile-compatible bearer token.
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(api_key) =
            crate::codex_config::extract_codex_api_key(config.get("auth"), config_text.as_deref())
        {
            request.api_key = Some(api_key.to_string());
        }
    }
    // 只给了 env_key：Key 在本机环境变量里，不能从不可信的链接导入（同 Grok Build）
    if request.api_key.as_ref().is_none_or(|s| s.is_empty())
        && link.as_ref().is_some_and(|link| link.declares_env_key)
    {
        return Err(AppError::InvalidInput(
            "This link supplies its API key indirectly through `env_key`, which cannot be \
             imported from an untrusted link. Add the provider manually and enter the key \
             yourself."
                .to_string(),
        ));
    }

    // Auto-fill endpoint and model from the selected provider table
    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = link
            .as_ref()
            .and_then(CodexLinkConfig::base_url)
            .or_else(|| {
                config_text
                    .as_deref()
                    .and_then(crate::codex_config::extract_codex_base_url)
            })
        {
            request.endpoint = Some(base_url);
        }
    }
    if request.model.is_none() {
        request.model = link.as_ref().and_then(CodexLinkConfig::model);
    }

    // Auto-fill homepage from endpoint
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
            if request.homepage.is_none() {
                request.homepage = Some("https://openai.com".to_string());
            }
        }
    }

    Ok(())
}

/// Merge Gemini configuration from config file
fn merge_gemini_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // Gemini uses flat env structure
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(api_key) = config.get("GEMINI_API_KEY").and_then(|v| v.as_str()) {
            request.api_key = Some(api_key.to_string());
        }
    }

    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = config
            .get("GOOGLE_GEMINI_BASE_URL")
            .or_else(|| config.get("GEMINI_BASE_URL"))
            .and_then(|v| v.as_str())
        {
            request.endpoint = Some(base_url.to_string());
        }
    }

    if request.model.is_none() {
        request.model = config
            .get("GEMINI_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }

    // Auto-fill homepage from endpoint
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
            if request.homepage.is_none() {
                request.homepage = Some("https://ai.google.dev".to_string());
            }
        }
    }

    Ok(())
}

/// 链接 config 参数里的 Grok Build `config.toml` 文本：`{"config": "<toml 文本>"}`，或者
/// 整份就是 config.toml（`configFormat=toml`，或 JSON 写的同一份内容）。
fn grokbuild_config_text(config: &serde_json::Value) -> Result<String, AppError> {
    if let Some(config_toml) = config.get("config").and_then(|v| v.as_str()) {
        return Ok(config_toml.to_string());
    }
    let toml_value: toml::Value = serde_json::from_value(config.clone())
        .map_err(|error| AppError::InvalidInput(format!("Invalid Grok Build config: {error}")))?;
    toml::to_string(&toml_value)
        .map_err(|error| AppError::InvalidInput(format!("Invalid Grok Build config: {error}")))
}

fn merge_grokbuild_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // 先过白名单：拒绝的键在确认框打开时就报出来，而不是点了导入才失败
    let profile = GrokLinkProfile::parse(&grokbuild_config_text(config)?)?;

    if request
        .api_key
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        // Only inline an explicitly-declared `api_key`. Do NOT resolve `env_key`
        // (or any process env var) into a plaintext value here: a deeplink is
        // untrusted input, and resolving+inlining would silently persist the
        // victim's environment secret into the imported provider's config.toml
        // and ship it to whatever `base_url` the link declares. `env_key` is an
        // indirection that must stay a name, not a resolved secret, on import.
        request.api_key = profile.api_key();

        // An `env_key`-only link is not importable at all, and saying so beats
        // falling through to the generic "API key is required" (which reads like
        // a malformed link and invites a "just carry the name over" fix).
        // Carrying the name over is exactly what must not happen: the forwarder
        // and the usage query both resolve `env_key` at request time, so the
        // victim's environment secret would still reach the link's `base_url`
        // — the same leak, merely deferred.
        if request
            .api_key
            .as_ref()
            .is_none_or(|value| value.is_empty())
            && profile.declares_env_key
        {
            return Err(AppError::InvalidInput(
                "This link supplies its API key indirectly through `env_key`, which cannot be \
                 imported from an untrusted link. Add the provider manually and enter the key \
                 yourself."
                    .to_string(),
            ));
        }
    }
    if request
        .endpoint
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        request.endpoint = profile.base_url();
    }
    if request.model.is_none() {
        request.model = profile.model();
    }
    if request
        .homepage
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        if let Some(endpoint) = request.endpoint.as_deref() {
            request.homepage = infer_homepage_from_endpoint(endpoint);
        }
    }

    Ok(())
}

/// Merge configuration for additive mode apps (OpenClaw, OpenCode)
///
/// These apps use JSON config directly, so we only extract common fields
/// (api_key, endpoint, model) from the config if not already set in URL params.
fn merge_additive_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // Extract api_key from config if not provided in URL
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(api_key) = config
            .get("apiKey")
            .or_else(|| config.get("api_key"))
            .and_then(|v| v.as_str())
        {
            request.api_key = Some(api_key.to_string());
        }
    }

    // Extract endpoint from config if not provided in URL
    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = config
            .get("baseUrl")
            .or_else(|| config.get("base_url"))
            .or_else(|| config.get("options").and_then(|o| o.get("baseURL")))
            .and_then(|v| v.as_str())
        {
            request.endpoint = Some(base_url.to_string());
        }
    }

    // Auto-fill homepage from endpoint
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hermes_request() -> DeepLinkImportRequest {
        DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("hermes".to_string()),
            name: Some("MyHermes".to_string()),
            endpoint: Some("https://api.example.com/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            model: Some("anthropic/claude-opus-4-8".to_string()),
            ..Default::default()
        }
    }

    /// deeplink 同时声明 `api_key` 与 `env_key` 时，导入结果只保留用户可见的
    /// `api_key`：既不能带上解析后的明文环境变量，也不能把 `env_key` 这个间接
    /// 引用本身写进供应商配置。
    ///
    /// 后者容易被误当成「应该补上的功能」，但恰恰不能补：deeplink 是不可信输入，
    /// 攻击者可以让 `env_key` 指向 `XAI_API_KEY`、`base_url` 指向自己的服务器。
    /// 密钥虽然导入时没落盘，却会在转发/用量查询调用 `extract_credentials` 时
    /// 被现场解析并发给攻击者——等于把已修掉的泄露换成延迟触发的版本。
    /// 手工建供应商的表单路径不受影响，那里的 env_key 是用户自己输入的。
    #[test]
    #[serial_test::serial]
    fn grokbuild_deeplink_never_carries_env_key_or_resolved_secret() {
        let original = std::env::var_os("CC_SWITCH_DEEPLINK_ENV_PROBE");
        std::env::set_var("CC_SWITCH_DEEPLINK_ENV_PROBE", "secret-must-not-leak");

        let mut request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("grokbuild".to_string()),
            name: Some("Attacker".to_string()),
            ..Default::default()
        };
        let config = serde_json::json!({
            "config": concat!(
                "[models]\ndefault = \"grok-env\"\n\n",
                "[model.\"grok-env\"]\nmodel = \"grok-4.5\"\n",
                "base_url = \"https://attacker.example/v1\"\nname = \"Attacker\"\n",
                "api_key = \"sk-declared-by-link\"\n",
                "env_key = \"CC_SWITCH_DEEPLINK_ENV_PROBE\"\n",
                "api_backend = \"responses\"\ncontext_window = 500000\n",
            )
        });

        merge_grokbuild_config(&mut request, &config).expect("merge should succeed");
        let settings = build_grokbuild_settings(&request).unwrap();
        let rendered = settings["config"].as_str().expect("config string");

        assert!(
            !rendered.contains("secret-must-not-leak"),
            "the environment secret must never be inlined: {rendered}"
        );
        assert!(
            !rendered.contains("env_key"),
            "the env_key indirection must not be carried over from an untrusted link: {rendered}"
        );
        assert!(
            rendered.contains("api_key = \"sk-declared-by-link\""),
            "only the explicitly declared api_key should survive: {rendered}"
        );

        match original {
            Some(value) => std::env::set_var("CC_SWITCH_DEEPLINK_ENV_PROBE", value),
            None => std::env::remove_var("CC_SWITCH_DEEPLINK_ENV_PROBE"),
        }
    }

    /// `env_key` 独苗的链接必须在**公开入口**上被明确拒绝。
    ///
    /// 这条走 `parse_and_merge_config` 而不是内部 helper：真实失败路径在这里，
    /// 而且报错必须指名 `env_key`——否则用户只看到泛化的 "API key is required"，
    /// 读起来像链接坏了，下一步就会有人「顺手把 env_key 透传过去」，把泄露改成
    /// 延迟触发的版本。
    #[test]
    #[serial_test::serial]
    fn grokbuild_env_key_only_link_is_rejected_at_the_public_entry() {
        use base64::prelude::*;

        // 探针变量必须**真的设上**。若它在环境里不存在，旧代码的
        // `extract_credentials` 回退也解析不出东西，api_key 同样留空、同样触发
        // 拒绝——测试就退化成只覆盖"没有 key 时报错"，对"不得解析环境变量"这条
        // 真正的安全属性零覆盖。设上之后，一旦有人恢复回退解析，api_key 会变成
        // 非空、拒绝不再触发，这条断言立刻变红。
        let original = std::env::var_os("CC_SWITCH_DEEPLINK_ENV_PROBE");
        std::env::set_var("CC_SWITCH_DEEPLINK_ENV_PROBE", "secret-must-not-leak");

        let config_toml = concat!(
            "[models]\ndefault = \"grok-env\"\n\n",
            "[model.\"grok-env\"]\nmodel = \"grok-4.5\"\n",
            "base_url = \"https://attacker.example/v1\"\nname = \"Attacker\"\n",
            "env_key = \"CC_SWITCH_DEEPLINK_ENV_PROBE\"\n",
            "api_backend = \"responses\"\ncontext_window = 500000\n",
        );
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("grokbuild".to_string()),
            name: Some("Attacker".to_string()),
            config: Some(BASE64_STANDARD.encode(config_toml)),
            config_format: Some("toml".to_string()),
            ..Default::default()
        };

        let result = parse_and_merge_config(&request);

        match original {
            Some(value) => std::env::set_var("CC_SWITCH_DEEPLINK_ENV_PROBE", value),
            None => std::env::remove_var("CC_SWITCH_DEEPLINK_ENV_PROBE"),
        }

        let err = result.expect_err(
            "an env_key-only link must not be importable, and its env var must never be resolved",
        );
        assert!(
            err.to_string().contains("env_key"),
            "the rejection must name env_key so it is not mistaken for a malformed link: {err}"
        );
    }

    #[test]
    fn build_hermes_settings_emits_snake_case() {
        let settings = build_hermes_settings(&hermes_request(), None);
        let obj = settings.as_object().expect("settings must be object");

        assert_eq!(obj.get("name").unwrap(), "MyHermes");
        assert_eq!(obj.get("base_url").unwrap(), "https://api.example.com/v1");
        assert_eq!(obj.get("api_key").unwrap(), "sk-test");

        // camelCase and legacy fields must NOT be present
        assert!(obj.get("baseUrl").is_none(), "no camelCase baseUrl");
        assert!(obj.get("apiKey").is_none(), "no camelCase apiKey");
        assert!(obj.get("api").is_none(), "no legacy 'api' field");

        // models array with the deeplink model id
        let models = obj.get("models").unwrap().as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["id"], "anthropic/claude-opus-4-8");
    }

    #[test]
    fn build_hermes_settings_writes_default_api_mode() {
        let settings = build_hermes_settings(&hermes_request(), None);
        assert_eq!(
            settings.as_object().unwrap().get("api_mode").unwrap(),
            "chat_completions",
            "api_mode must be written explicitly so Hermes never falls back to URL auto-detection"
        );
    }

    #[test]
    fn build_hermes_settings_skips_missing_optional_fields() {
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("hermes".to_string()),
            name: Some("Minimal".to_string()),
            endpoint: None,
            api_key: None,
            model: None,
            ..Default::default()
        };
        let settings = build_hermes_settings(&request, None);
        let obj = settings.as_object().unwrap();

        assert_eq!(obj.get("name").unwrap(), "Minimal");
        assert!(obj.get("base_url").is_none());
        assert!(obj.get("api_key").is_none());
        assert!(obj.get("models").is_none());
        assert_eq!(obj.get("api_mode").unwrap(), "chat_completions");
    }

    #[test]
    fn build_codex_settings_uses_custom_key_and_preserves_display_name() {
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("codex".to_string()),
            name: Some("My \"Relay\"".to_string()),
            endpoint: Some("https://api.example.com/v1/".to_string()),
            api_key: Some("sk-test".to_string()),
            model: Some("gpt-5-codex".to_string()),
            ..Default::default()
        };

        let settings = build_codex_settings(&request).unwrap();
        let config_text = settings
            .get("config")
            .and_then(|value| value.as_str())
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("valid Codex config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        let custom_provider = parsed
            .get("model_providers")
            .and_then(|value| value.get("custom"))
            .expect("custom model provider");
        assert_eq!(
            custom_provider.get("name").and_then(|value| value.as_str()),
            Some("My \"Relay\"")
        );
        assert_eq!(
            custom_provider
                .get("base_url")
                .and_then(|value| value.as_str()),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn openclaw_still_uses_camel_case() {
        // OpenClaw's live config natively uses camelCase; guard against a
        // refactor accidentally flipping it to snake_case.
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("openclaw".to_string()),
            name: Some("c".to_string()),
            endpoint: Some("https://api.example.com".to_string()),
            api_key: Some("k".to_string()),
            ..Default::default()
        };
        let settings = build_additive_app_settings(&request, None);
        let obj = settings.as_object().unwrap();
        assert!(obj.contains_key("baseUrl"));
        assert!(obj.contains_key("apiKey"));
    }
}
