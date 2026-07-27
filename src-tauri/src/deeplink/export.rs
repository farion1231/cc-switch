//! Provider share deeplink generation.
//!
//! Builds a `ccswitch://v1/import?...` URL that carries the provider's full
//! `settings_config` as base64 JSON, so the receiving side can restore an
//! identical provider via the existing import flow.

use crate::error::AppError;
use crate::provider::Provider;
use crate::AppType;
use base64::prelude::*;

/// Build a shareable deeplink for a single provider.
///
/// Fidelity design: the URL carries `config=base64(settings_config JSON)` and
/// deliberately NO standalone endpoint/apiKey/model params — the import side
/// extracts them from the config, avoiding double-source conflicts.
pub fn build_provider_share_url(
    app_type: &AppType,
    provider: &Provider,
) -> Result<String, AppError> {
    if matches!(app_type, AppType::ClaudeDesktop) {
        return Err(AppError::InvalidInput(
            "Claude Desktop providers cannot be shared via deeplink".to_string(),
        ));
    }

    // Claude routing-metadata guard. The link carries `meta.apiFormat` (below)
    // but NOT the other proxy-routing meta fields. A provider that depends on
    // any of them would import with silently different routing (e.g. a full
    // base_url getting the endpoint path appended, or a custom User-Agent /
    // prompt-cache key dropped). Reject at generation time rather than ship a
    // link that breaks the "identical" promise. (`custom_endpoints` is
    // intentionally not guarded: only the primary endpoint is shared, which
    // degrades gracefully without breaking routing.)
    if matches!(app_type, AppType::Claude) {
        if let Some(meta) = provider.meta.as_ref() {
            let unsupported = unsupported_claude_routing_meta(meta);
            if let Some(field) = unsupported {
                return Err(AppError::InvalidInput(format!(
                    "This provider cannot be shared: uses unsupported routing metadata ({field})"
                )));
            }
        }
    }

    let config_json = serde_json::to_string(&provider.settings_config)
        .map_err(|e| AppError::Message(format!("Failed to serialize provider config: {e}")))?;
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

    let mut url = url::Url::parse("ccswitch://v1/import")
        .map_err(|e| AppError::Message(format!("Failed to build deeplink: {e}")))?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("resource", "provider");
        qp.append_pair("app", app_type.as_str());
        qp.append_pair("name", &provider.name);
        qp.append_pair("config", &config_b64);
        qp.append_pair("configFormat", "json");
        if let Some(homepage) = provider.website_url.as_deref().filter(|s| !s.is_empty()) {
            qp.append_pair("homepage", homepage);
        }
        if let Some(icon) = provider.icon.as_deref().filter(|s| !s.is_empty()) {
            qp.append_pair("icon", icon);
        }
        if let Some(notes) = provider.notes.as_deref().filter(|s| !s.is_empty()) {
            qp.append_pair("notes", notes);
        }
        if let Some(api_format) = provider
            .meta
            .as_ref()
            .and_then(|m| m.api_format.as_deref())
            .filter(|s| !s.is_empty())
        {
            // Claude wire-format routing (meta.apiFormat). Without it the
            // receiver falls back to "anthropic" and sends the wrong wire
            // format to OpenAI-Chat / Responses / Gemini-native upstreams.
            qp.append_pair("apiFormat", api_format);
        }
        if let Some(script) = provider.meta.as_ref().and_then(|m| m.usage_script.as_ref()) {
            // Carry the FULL UsageScript (templateType, codingPlanProvider,
            // Volcengine AK/SK, Zhipu org/project, ...) as a single base64
            // JSON blob. The scattered usage_* params would drop native
            // template fields and leave the receiver running an empty script.
            let script_json = serde_json::to_string(script).map_err(|e| {
                AppError::Message(format!("Failed to serialize usage_script: {e}"))
            })?;
            qp.append_pair("usageScriptConfig", &BASE64_STANDARD.encode(script_json.as_bytes()));
        }
    }
    let share_url = url.to_string();

    // Shareability gate: the import side hard-requires an extractable API key
    // and endpoint. OAuth/official providers lack them — fail here instead of
    // producing a link that is doomed to be rejected on the receiving side.
    let parsed = super::parse_deeplink_url(&share_url)?;
    let merged = super::parse_and_merge_config(&parsed)?;
    let has_key = merged.api_key.as_deref().is_some_and(|s| !s.is_empty());
    let has_endpoint = merged.endpoint.as_deref().is_some_and(|s| !s.is_empty());
    if !has_key || !has_endpoint {
        return Err(AppError::InvalidInput(
            "This provider cannot be shared: no exportable API key or endpoint".to_string(),
        ));
    }

    // The import side also hard-requires a homepage: when none is carried it
    // falls back to infer_homepage_from_endpoint(primary endpoint), and
    // additive-mode apps (OpenClaw/OpenCode/Hermes/GrokBuild) have no
    // hardcoded homepage fallback. A scheme-less endpoint (e.g. a LAN relay
    // like `192.168.1.5:8080/v1`) defeats inference, so reject here instead
    // of shipping a link doomed to fail with "Homepage is required".
    let has_homepage = merged.homepage.as_deref().is_some_and(|s| !s.is_empty())
        || merged
            .endpoint
            .as_deref()
            .and_then(|s| s.split(',').map(str::trim).find(|e| !e.is_empty()))
            .and_then(super::utils::infer_homepage_from_endpoint)
            .is_some();
    if !has_homepage {
        return Err(AppError::InvalidInput(
            "This provider cannot be shared: no homepage and none can be inferred from the endpoint"
                .to_string(),
        ));
    }

    // Fidelity invariant: rebuild the provider from our own link and require the
    // settings_config to round-trip verbatim. Rejects configs that would import
    // as a near-copy (e.g. a legacy env-key alias re-serialized under a
    // different canonical key) rather than silently breaking the "identical"
    // promise on the receiving side.
    let rebuilt = super::provider::build_provider_from_request(app_type, &merged)?;
    if rebuilt.settings_config != provider.settings_config {
        return Err(AppError::InvalidInput(
            "This provider cannot be shared: its config does not round-trip identically"
                .to_string(),
        ));
    }

    // Extend the fidelity invariant to the carried meta fields (`apiFormat` and
    // the full `usage_script`). A mismatch means the link would drop or alter
    // routing / usage-template metadata on the receiving side.
    let rebuilt_meta = rebuilt.meta.as_ref();
    let orig_meta = provider.meta.as_ref();
    if json_str(rebuilt_meta.and_then(|m| m.api_format.as_ref()))
        != json_str(orig_meta.and_then(|m| m.api_format.as_ref()))
        || json_str(rebuilt_meta.and_then(|m| m.usage_script.as_ref()))
            != json_str(orig_meta.and_then(|m| m.usage_script.as_ref()))
    {
        return Err(AppError::InvalidInput(
            "This provider cannot be shared: its metadata does not round-trip identically"
                .to_string(),
        ));
    }

    Ok(share_url)
}

/// Serialize an `Option<&T: Serialize>` to a JSON string, treating `None` and
/// serialization failure as the empty string so two `None`s compare equal.
fn json_str<T: serde::Serialize>(value: Option<&T>) -> String {
    value
        .and_then(|v| serde_json::to_string(v).ok())
        .unwrap_or_default()
}

/// Return the name of the first proxy-routing meta field that is set but not
/// carried by the share link, or `None` if the provider is safe to share.
///
/// Scoped to Claude (Codex routing lives in the carried TOML; additive apps
/// use the carried JSON). `custom_endpoints` is deliberately excluded - only
/// the primary endpoint is shared, which degrades without breaking routing.
fn unsupported_claude_routing_meta(meta: &crate::provider::ProviderMeta) -> Option<&'static str> {
    if meta.is_full_url == Some(true) {
        return Some("isFullUrl");
    }
    if meta.custom_user_agent.as_deref().is_some_and(|s| !s.is_empty()) {
        return Some("customUserAgent");
    }
    if meta.impersonate_claude_code == Some(true) {
        return Some("impersonateClaudeCode");
    }
    if meta.prompt_cache_key.as_deref().is_some_and(|s| !s.is_empty()) {
        return Some("promptCacheKey");
    }
    if meta.prompt_cache_routing.as_deref().is_some_and(|s| !s.is_empty()) {
        return Some("promptCacheRouting");
    }
    if meta
        .local_proxy_request_overrides
        .as_ref()
        .is_some_and(|o| !o.is_empty())
    {
        return Some("localProxyRequestOverrides");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deeplink::provider::build_provider_from_request;
    use crate::provider::{ProviderMeta, UsageScript};
    use serde_json::json;
    use std::str::FromStr;

    fn make_provider(name: &str, settings_config: serde_json::Value) -> Provider {
        Provider {
            id: "test-id".to_string(),
            name: name.to_string(),
            settings_config,
            website_url: Some("https://example.com".to_string()),
            category: None,
            created_at: None,
            sort_index: None,
            notes: Some("团队渠道".to_string()),
            meta: None,
            icon: Some("kimi".to_string()),
            icon_color: None,
            in_failover_queue: false,
        }
    }

    /// 核心验收：生成 URL → 解析 → 合并 → 构建，settings_config 深度相等
    fn assert_round_trip(app: &str, name: &str, settings_config: serde_json::Value) {
        let app_type = AppType::from_str(app).unwrap();
        let provider = make_provider(name, settings_config.clone());
        let url = build_provider_share_url(&app_type, &provider).expect("share url");
        assert!(url.starts_with("ccswitch://v1/import?"), "url: {url}");

        let parsed = crate::deeplink::parse_deeplink_url(&url).expect("parse");
        assert_eq!(parsed.name.as_deref(), Some(name));
        let merged = crate::deeplink::parse_and_merge_config(&parsed).expect("merge");
        let imported = build_provider_from_request(&app_type, &merged).expect("build");
        assert_eq!(
            imported.settings_config, settings_config,
            "settings_config must round-trip verbatim for app {app}"
        );
    }

    #[test]
    fn round_trip_claude_with_custom_env_and_top_level_keys() {
        assert_round_trip(
            "claude",
            "团队 Claude 🚀",
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-team",
                    "ANTHROPIC_BASE_URL": "https://relay.example.com",
                    "ANTHROPIC_MODEL": "claude-opus-4-6",
                    "ANTHROPIC_CUSTOM_HEADERS": "X-Team: a",
                    "API_TIMEOUT_MS": "600000"
                },
                "permissions": { "allow": ["Bash"] }
            }),
        );
    }

    #[test]
    fn round_trip_codex_with_custom_wire_api() {
        assert_round_trip(
            "codex",
            "Codex 中转",
            json!({
                "auth": { "OPENAI_API_KEY": "sk-relay" },
                "config": "model_provider = \"custom\"\nmodel = \"gpt-5.2\"\n\n[model_providers.custom]\nname = \"Relay\"\nbase_url = \"https://relay.example.com/v1\"\nwire_api = \"chat\"\nrequires_openai_auth = false\n"
            }),
        );
    }

    #[test]
    fn round_trip_gemini() {
        assert_round_trip(
            "gemini",
            "Gemini 团队",
            json!({
                "env": {
                    "GEMINI_API_KEY": "gk-1",
                    "GOOGLE_GEMINI_BASE_URL": "https://g.example.com",
                    "GOOGLE_CLOUD_PROJECT": "my-proj"
                }
            }),
        );
    }

    #[test]
    fn round_trip_grokbuild() {
        assert_round_trip(
            "grokbuild",
            "Grok",
            json!({
                "config": "[models]\ndefault = \"grok-5\"\n\n[model.grok-5]\nmodel = \"grok-5\"\nbase_url = \"https://grok.example.com/v1\"\nname = \"Team Grok\"\napi_key = \"xai-key\"\napi_backend = \"anthropic\"\ncontext_window = 131072\n"
            }),
        );
    }

    #[test]
    fn round_trip_opencode() {
        assert_round_trip(
            "opencode",
            "OC",
            json!({
                "npm": "@ai-sdk/anthropic",
                "options": { "baseURL": "https://oc.example.com/v1", "apiKey": "sk-oc" },
                "models": { "claude-opus-4": { "name": "claude-opus-4" } }
            }),
        );
    }

    #[test]
    fn round_trip_openclaw() {
        assert_round_trip(
            "openclaw",
            "Claw",
            json!({
                "baseUrl": "https://claw.example.com",
                "apiKey": "sk-claw",
                "api": "anthropic-messages",
                "models": [{ "id": "m1", "name": "m1" }]
            }),
        );
    }

    #[test]
    fn round_trip_hermes() {
        assert_round_trip(
            "hermes",
            "H",
            json!({
                "name": "H",
                "base_url": "https://h.example.com/v1",
                "api_key": "sk-h",
                "api_mode": "anthropic_messages",
                "models": [{ "id": "opus", "name": "opus" }]
            }),
        );
    }

    #[test]
    fn claude_desktop_is_rejected() {
        let provider = make_provider("cd", json!({ "env": {} }));
        let err = build_provider_share_url(&AppType::ClaudeDesktop, &provider).unwrap_err();
        assert!(err.to_string().contains("Claude Desktop"));
    }

    #[test]
    fn provider_without_extractable_key_is_rejected() {
        // 官方/OAuth 型：env 中无 token 无 base_url → 导入端必拒，生成时就报错
        let provider = make_provider("官方", json!({ "env": {} }));
        let err = build_provider_share_url(&AppType::Claude, &provider).unwrap_err();
        assert!(
            err.to_string().contains("cannot be shared"),
            "actual: {err}"
        );
    }

    #[test]
    fn provider_with_uninferrable_homepage_is_rejected() {
        // OpenClaw：endpoint 无 scheme（如局域网自建中转），导入端无法推断 homepage，
        // 会在接收侧报 "Homepage is required"。门禁应在生成时直接拒绝。
        let mut provider = make_provider(
            "局域网中转",
            json!({
                "baseUrl": "192.168.1.5:8080/v1",
                "apiKey": "sk-lan",
                "api": "anthropic-messages"
            }),
        );
        provider.website_url = None;
        let err = build_provider_share_url(&AppType::OpenClaw, &provider).unwrap_err();
        assert!(err.to_string().contains("homepage"), "actual: {err}");
    }

    #[test]
    fn provider_that_does_not_round_trip_identically_is_rejected() {
        // 端点存于旧别名 GEMINI_BASE_URL：导入端会规范化为 GOOGLE_GEMINI_BASE_URL，
        // 使 settings_config 不再逐字相等。fidelity 门禁应在生成时直接拒绝，
        // 而非产出一个"近似复制"的链接。
        let provider = make_provider(
            "旧别名 Gemini",
            json!({
                "env": {
                    "GEMINI_API_KEY": "gm-key",
                    "GEMINI_BASE_URL": "https://legacy.example.com"
                }
            }),
        );
        let err = build_provider_share_url(&AppType::Gemini, &provider).unwrap_err();
        assert!(
            err.to_string().contains("round-trip identically"),
            "actual: {err}"
        );
    }

    #[test]
    fn usage_script_config_carries_full_script() {
        // usageScriptConfig 携带完整 UsageScript（含原生模板字段），而非仅 code。
        // 接收方应逐字段还原，使 token_plan / coding-plan 类渠道的用量查询可用。
        let mut provider = make_provider(
            "带用量",
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-1",
                    "ANTHROPIC_BASE_URL": "https://u.example.com"
                }
            }),
        );
        let original_script = UsageScript {
            enabled: true,
            language: "javascript".to_string(),
            code: "return { used: 1 }".to_string(),
            timeout: Some(10),
            api_key: Some("usage-key".to_string()),
            base_url: Some("https://usage.example.com".to_string()),
            access_token: None,
            user_id: Some("42".to_string()),
            template_type: Some("token_plan".to_string()),
            auto_query_interval: Some(30),
            coding_plan_provider: Some("zhipu_team".to_string()),
            access_key_id: Some("AKID".to_string()),
            secret_access_key: Some("SK".to_string()),
            team_organization_id: Some("org-1".to_string()),
            team_project_id: Some("proj-1".to_string()),
        };
        provider.meta = Some(ProviderMeta {
            usage_script: Some(original_script.clone()),
            ..Default::default()
        });

        let url = build_provider_share_url(&AppType::Claude, &provider).unwrap();
        let parsed = crate::deeplink::parse_deeplink_url(&url).unwrap();
        // 不再发出零散 usage_* 参数
        assert!(parsed.usage_script.is_none());
        assert!(parsed.usage_api_key.is_none());
        assert!(parsed.usage_enabled.is_none());
        // usageScriptConfig blob 解码后应与原 UsageScript 深度相等
        let blob_b64 = parsed
            .usage_script_config
            .as_deref()
            .expect("usageScriptConfig present");
        let decoded =
            crate::deeplink::utils::decode_base64_param("usageScriptConfig", blob_b64).unwrap();
        let restored: UsageScript =
            serde_json::from_slice(&decoded).expect("deserialize usage script");
        assert_eq!(
            serde_json::to_string(&restored).unwrap(),
            serde_json::to_string(&original_script).unwrap(),
            "usage_script must round-trip verbatim"
        );
    }

    /// 带 meta 的往返：生成 -> 解析 -> 合并 -> 构建，断言 settings_config +
    /// 携带的 meta 字段（apiFormat / usage_script）均深度相等。
    fn assert_round_trip_with_meta(
        app: &str,
        name: &str,
        settings_config: serde_json::Value,
        meta: ProviderMeta,
    ) {
        let app_type = AppType::from_str(app).unwrap();
        let provider = {
            let mut p = make_provider(name, settings_config.clone());
            p.meta = Some(meta);
            p
        };
        let url = build_provider_share_url(&app_type, &provider).expect("share url");
        let parsed = crate::deeplink::parse_deeplink_url(&url).expect("parse");
        let merged = crate::deeplink::parse_and_merge_config(&parsed).expect("merge");
        let imported = build_provider_from_request(&app_type, &merged).expect("build");
        assert_eq!(imported.settings_config, settings_config);
        assert_eq!(
            super::json_str(imported.meta.as_ref().and_then(|m| m.api_format.as_ref())),
            super::json_str(provider.meta.as_ref().and_then(|m| m.api_format.as_ref()))
        );
        assert_eq!(
            super::json_str(imported.meta.as_ref().and_then(|m| m.usage_script.as_ref())),
            super::json_str(provider.meta.as_ref().and_then(|m| m.usage_script.as_ref()))
        );
    }

    #[test]
    fn round_trip_claude_with_api_format() {
        // OpenAI-Chat 上游的 Claude 渠道：meta.apiFormat 是 wire-format 路由的 SSOT。
        // 未携带时接收方回退 "anthropic"，会跳过代理转换、发错 wire format。
        assert_round_trip_with_meta(
            "claude",
            "OpenAI Chat Claude",
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-1",
                    "ANTHROPIC_BASE_URL": "https://relay.example.com"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_chat".to_string()),
                ..Default::default()
            },
        );
    }

    #[test]
    fn round_trip_claude_with_anthropic_api_key() {
        // PatewayAI / Gemini Native / AiHubMix 形态：凭证仅存于 ANTHROPIC_API_KEY。
        // 修复前 merge_claude_config 提取不到 key、build_claude_settings 会注入多余的
        // ANTHROPIC_AUTH_TOKEN，破坏逐字相等；修复后应可分享且逐字一致。
        assert_round_trip(
            "claude",
            "API Key Claude",
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "sk-from-api-key",
                    "ANTHROPIC_BASE_URL": "https://relay.example.com"
                }
            }),
        );
    }

    #[test]
    fn round_trip_claude_with_token_plan_usage() {
        // 原生用量模板（token_plan + 智谱团队 org/project）：修复前 templateType 与
        // coding-plan 字段被丢弃，接收方用量查询走空脚本。修复后应逐字还原。
        assert_round_trip_with_meta(
            "claude",
            "智谱团队",
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-zhipu",
                    "ANTHROPIC_BASE_URL": "https://zhipu.example.com"
                }
            }),
            ProviderMeta {
                usage_script: Some(UsageScript {
                    enabled: true,
                    language: "javascript".to_string(),
                    code: String::new(),
                    timeout: Some(10),
                    api_key: None,
                    base_url: None,
                    access_token: None,
                    user_id: None,
                    template_type: Some("token_plan".to_string()),
                    auto_query_interval: Some(60),
                    coding_plan_provider: Some("zhipu_team".to_string()),
                    access_key_id: None,
                    secret_access_key: None,
                    team_organization_id: Some("org-1".to_string()),
                    team_project_id: Some("proj-1".to_string()),
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn claude_with_unsupported_routing_meta_is_rejected() {
        // is_full_url 影响代理 URL 拼接但未被携带 -> 生成时直接拒绝，避免静默行为不一致。
        let mut provider = make_provider(
            "完整 URL Claude",
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-1",
                    "ANTHROPIC_BASE_URL": "https://relay.example.com/full"
                }
            }),
        );
        provider.meta = Some(ProviderMeta {
            is_full_url: Some(true),
            ..Default::default()
        });
        let err = build_provider_share_url(&AppType::Claude, &provider).unwrap_err();
        assert!(
            err.to_string().contains("unsupported routing metadata"),
            "actual: {err}"
        );
    }
}
