//! Outbound request body rewrites that do not depend on forwarder state.

use super::*;

/// Shared request pipeline after native Responses transport selection.
/// Normalize standalone receipts for third parties before the strict-Grok gate.
pub(super) fn apply_native_responses_request_compat(
    provider: &Provider,
    body: &mut Value,
    keep_resolved_model: bool,
) -> Result<(), ProxyError> {
    if !crate::proxy::providers::is_codex_official_provider(provider)
        && crate::proxy::providers::transform_codex_responses_xai_sanitize::rewrite_xai_call_id_less_tool_outputs(body)
    {
        log::info!(
            "[Codex] Rewrote function_call_output items without call_id for native Responses upstream (provider={})",
            provider.id
        );
    }
    if !crate::proxy::providers::provider_needs_responses_namespace_flatten(
        provider,
        body.get("model").and_then(Value::as_str),
    ) {
        return Ok(());
    }
    if crate::proxy::providers::transform_codex_responses_namespace::flatten_request_namespaces(
        body,
    )? {
        log::debug!(
            "[Codex] Flattened namespace tools for native Responses upstream (provider={})",
            provider.id
        );
    }
    // Copilot already resolved against its live catalog; never replace that
    // model with the card default. Direct xAI keeps its existing fallback.
    let upstream_model = if keep_resolved_model || provider.is_github_copilot() {
        None
    } else {
        crate::proxy::providers::codex_provider_upstream_model(provider)
    };
    crate::proxy::providers::transform_codex_responses_xai_sanitize::apply_xai_native_responses_request_compat(
        body,
        &provider.id,
        upstream_model.as_deref(),
        &provider.settings_config,
    );
    Ok(())
}

pub(super) const CLAUDE_CODE_SYSTEM_IDENTITY: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

/// Insert the Claude Code identity as the first line before the `system` field in
/// the Anthropic request body.
///
/// Anthropic subscription/OAuth plans require the first system block to be exactly
/// this identity line. After conversion `system` is a string (from Codex
/// instructions); normalize it into an array here: [identity line, original system...].
pub(super) fn prepend_claude_code_system_prompt(body: &mut Value) {
    let identity = serde_json::json!({ "type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY });
    let mut blocks: Vec<Value> = vec![identity];
    match body.get("system") {
        Some(Value::String(existing)) if !existing.is_empty() => {
            blocks.push(serde_json::json!({ "type": "text", "text": existing }));
        }
        Some(Value::Array(existing)) => {
            // Idempotent: skip re-injection if the first block is already the identity line.
            if existing
                .first()
                .and_then(|b| b.get("text"))
                .and_then(|t| t.as_str())
                == Some(CLAUDE_CODE_SYSTEM_IDENTITY)
            {
                return;
            }
            blocks.extend(existing.iter().cloned());
        }
        _ => {}
    }
    body["system"] = Value::Array(blocks);
}

/// Codex 只对内置 openai 发的字段（`output.encrypted_content`、
/// `internal_chat_message_metadata_passthrough` 等，见
/// `strip_codex_openai_only_request_fields`）只有 ChatGPT 后端认。第三方与本地原生
/// Responses 网关收到会把整条请求 400 掉（#9042）。官方卡和 ChatGPT 托管账号那条路逐字节
/// 不变；转 Chat / Anthropic 的路径重新组装请求，本来就不带这些字段。
pub(super) fn should_strip_codex_openai_only_fields(
    app_type: &AppType,
    provider: &Provider,
    codex_responses_to_chat: bool,
    codex_responses_to_anthropic: bool,
) -> bool {
    matches!(app_type, AppType::Codex)
        && !codex_responses_to_chat
        && !codex_responses_to_anthropic
        && !crate::proxy::providers::is_codex_official_provider(provider)
        && !provider.is_codex_oauth()
}

/// Prompt caching is part of the Codex→Anthropic protocol bridge rather than an
/// optional Bedrock optimizer. Codex requests do not contain Anthropic
/// `cache_control`, so keep bridge caching on by default while still honoring the
/// dedicated cache-injection switch. Injected breakpoints always use Anthropic's
/// standard 5-minute TTL.
pub(super) fn codex_anthropic_cache_config(config: &OptimizerConfig) -> OptimizerConfig {
    OptimizerConfig {
        enabled: true,
        thinking_optimizer: false,
        cache_injection: config.cache_injection,
    }
}

pub(super) fn apply_local_proxy_body_overrides(
    body: &mut Value,
    overrides: &LocalProxyRequestOverrides,
) -> bool {
    let Some(override_body) = overrides.body.as_ref() else {
        return false;
    };

    if !override_body.is_object() {
        log::warn!("[LocalProxyOverrides] Ignoring body override because it is not an object");
        return false;
    }

    merge_json_override(body, override_body)
}

pub(super) fn merge_json_override(target: &mut Value, patch: &Value) -> bool {
    merge_json_override_inner(target, patch, true)
}

pub(super) fn merge_json_override_inner(
    target: &mut Value,
    patch: &Value,
    is_top_level: bool,
) -> bool {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            let mut changed = false;
            for (key, patch_value) in patch_map {
                if is_top_level && key == "stream" {
                    log::warn!(
                        "[LocalProxyOverrides] Ignoring body override for protected field: stream"
                    );
                    continue;
                }
                match target_map.get_mut(key) {
                    Some(target_value) => {
                        changed |= merge_json_override_inner(target_value, patch_value, false);
                    }
                    None => {
                        target_map.insert(key.clone(), patch_value.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        (target_value, patch_value) => {
            if target_value == patch_value {
                false
            } else {
                *target_value = patch_value.clone();
                true
            }
        }
    }
}

pub(super) fn prepare_upstream_request_body(request_body: Value) -> Value {
    canonicalize_value(filter_private_params_with_whitelist(request_body, &[]))
}
