//! 代理契约里的凭据和模型别名（从旧的接管字段测试迁过来：#3784、#4919、#1049）。
use super::*;
use crate::provider::ProviderMeta;
use serde_json::Map;
use std::path::Path;

/// 启动、退出、重写路由按这个顺序处理各应用，Claude Desktop 在最后。
#[test]
fn route_mode_apps_keep_desktop_last() {
    assert_eq!(
        route_mode_apps().collect::<Vec<_>>(),
        [
            AppType::Claude,
            AppType::Codex,
            AppType::Gemini,
            AppType::GrokBuild,
            AppType::ClaudeDesktop,
        ]
    );
}

fn assert_env_str(env: &Map<String, Value>, key: &str, expected: Option<&str>) {
    assert_eq!(env.get(key).and_then(Value::as_str), expected, "{key}");
}

fn stack_member(id: &str, env: Value) -> Provider {
    Provider::with_id(id.to_string(), id.to_string(), json!({ "env": env }), None)
}

/// Stack 模式下的契约独有字段：`route` 是默认那家，`others` 是其余发布模型的成员。
fn stack_exclusive(route: &Provider, others: &[&Provider]) -> Map<String, Value> {
    let stack: Vec<StackModel> = std::iter::once(route)
        .chain(others.iter().copied())
        .flat_map(|provider| stack::claude_models(&provider.id, provider))
        .collect();
    let default = stack.first().cloned();
    let (projection, contract) = claude_contract(
        route,
        "http://127.0.0.1:15721",
        &stack,
        default.as_ref(),
        others,
    );
    assert_eq!(contract.exclusive, projection.exclusive);
    projection.exclusive
}

#[test]
fn stack_turns_off_a_feature_when_any_member_needs_it_off() {
    let route = stack_member("kimi", json!({ "ANTHROPIC_MODEL": "kimi-k3" }));
    let atlas = stack_member(
        "atlas",
        json!({
            "ANTHROPIC_MODEL": "sonnet-5",
            "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS": "1",
            "CLAUDE_CODE_AUTO_MODE_SERVER": "0",
            "CLAUDE_CODE_EXTRA_BODY": "{\"x\":1}",
            "CLAUDE_CODE_DISABLE_THINKING": "1"
        }),
    );
    let exclusive = stack_exclusive(&route, &[&atlas]);

    assert_env_str(
        &exclusive,
        "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS",
        Some("1"),
    );
    assert_env_str(&exclusive, "CLAUDE_CODE_AUTO_MODE_SERVER", Some("0"));
    // 请求体附加、关思考只跟默认那家，不并进来。
    assert_env_str(&exclusive, "CLAUDE_CODE_EXTRA_BODY", None);
    assert_env_str(&exclusive, "CLAUDE_CODE_DISABLE_THINKING", None);
}

#[test]
fn stack_does_not_take_a_member_switch_that_keeps_the_feature_on() {
    let route = stack_member("kimi", json!({ "ANTHROPIC_MODEL": "kimi-k3" }));
    let other = stack_member(
        "other",
        json!({
            "ANTHROPIC_MODEL": "m",
            "CLAUDE_CODE_AUTO_MODE_SERVER": "1",
            "CLAUDE_CODE_DISABLE_ARTIFACT": "0"
        }),
    );
    let exclusive = stack_exclusive(&route, &[&other]);

    assert_env_str(&exclusive, "CLAUDE_CODE_AUTO_MODE_SERVER", None);
    assert_env_str(&exclusive, "CLAUDE_CODE_DISABLE_ARTIFACT", None);
}

#[test]
fn stack_keeps_a_feature_on_only_when_every_member_turns_it_on() {
    let route = stack_member(
        "eflow",
        json!({ "ANTHROPIC_MODEL": "m-1", "ENABLE_TOOL_SEARCH": "true" }),
    );
    let with = stack_member(
        "with",
        json!({ "ANTHROPIC_MODEL": "m-2", "ENABLE_TOOL_SEARCH": "1" }),
    );
    let without = stack_member("without", json!({ "ANTHROPIC_MODEL": "m-3" }));

    assert_env_str(
        &stack_exclusive(&route, &[&with]),
        "ENABLE_TOOL_SEARCH",
        Some("true"),
    );
    assert_env_str(
        &stack_exclusive(&route, &[&with, &without]),
        "ENABLE_TOOL_SEARCH",
        None,
    );
}

#[test]
fn route_mode_keeps_only_the_route_switches() {
    let route = stack_member(
        "eflow",
        json!({ "ANTHROPIC_MODEL": "m-1", "ENABLE_TOOL_SEARCH": "true" }),
    );
    let other = stack_member(
        "atlas",
        json!({
            "ANTHROPIC_MODEL": "m-2",
            "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS": "1"
        }),
    );
    // 没有发布 Stack 模型（路由模式）时契约和原来一样，不看别家。
    let (projection, _) = claude_contract(&route, "http://127.0.0.1:15721", &[], None, &[&other]);
    assert_env_str(&projection.exclusive, "ENABLE_TOOL_SEARCH", Some("true"));
    assert_env_str(
        &projection.exclusive,
        "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS",
        None,
    );
}

/// 以 `live` 为底写入 `provider` 的代理契约，和进入代理时的补丁相同。
fn takeover(live: &Value, provider: &Provider) -> Value {
    let (projection, _) = claude_contract(provider, "http://127.0.0.1:15721", &[], None, &[]);
    let mut doc = live.clone();
    direct_patch(None, &projection)
        .apply_to(Path::new("settings.json"), &mut doc)
        .expect("apply proxy contract");
    doc
}

#[test]
fn managed_account_claude_takeover_uses_auth_token_placeholder() {
    let mut provider = Provider::with_id(
        "copilot".to_string(),
        "GitHub Copilot".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com",
                "ANTHROPIC_MODEL": "claude-haiku-4.5"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("github_copilot".to_string()),
        ..Default::default()
    });

    let mut live_config = provider.settings_config.clone();
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_eq!(
        env.get("ANTHROPIC_AUTH_TOKEN")
            .and_then(|value| value.as_str()),
        Some(PROXY_TOKEN_PLACEHOLDER)
    );
    assert!(
        env.get("ANTHROPIC_API_KEY").is_none(),
        "API_KEY placeholders trigger Claude Code's custom-key approval prompt (defaults to No), landing users in Not logged in"
    );
}

#[test]
fn managed_account_claude_takeover_sources_copilot_models_from_provider() {
    let mut provider = Provider::with_id(
        "copilot".to_string(),
        "GitHub Copilot".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com",
                "ANTHROPIC_MODEL": "claude-sonnet-4.6",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4.5",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4.6",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-sonnet-4.6",
                "CLAUDE_CODE_SUBAGENT_MODEL": "claude-sonnet-4.6[1M]"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("github_copilot".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://stale.example.com",
            "ANTHROPIC_API_KEY": "stale-key",
            "ANTHROPIC_MODEL": "stale-model",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "stale-haiku",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "Stale Haiku",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "stale-sonnet",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Stale Sonnet",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "stale-opus",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "Stale Opus",
            "CLAUDE_CODE_SUBAGENT_MODEL": "stale-subagent"
        }
    });
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_MODEL", None);
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        Some("claude-haiku-5-5"),
    );
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
        Some("claude-haiku-4.5"),
    );
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        Some("claude-sonnet-5"),
    );
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
        Some("claude-sonnet-4.6"),
    );
    assert_env_str(env, "ANTHROPIC_DEFAULT_OPUS_MODEL", Some("claude-opus-5"));
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
        Some("claude-sonnet-4.6"),
    );
    assert_env_str(
        env,
        "CLAUDE_CODE_SUBAGENT_MODEL",
        Some("claude-sonnet-4.6[1M]"),
    );
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
}

#[test]
fn managed_account_claude_takeover_removes_stale_subagent_model_when_provider_omits_it() {
    let mut provider = Provider::with_id(
        "codex".to_string(),
        "Codex".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "provider-sonnet"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://stale.example.com",
            "ANTHROPIC_API_KEY": "stale-key",
            "CLAUDE_CODE_SUBAGENT_MODEL": "stale-subagent"
        }
    });
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "CLAUDE_CODE_SUBAGENT_MODEL", None);
}

#[test]
fn managed_account_claude_takeover_sources_codex_models_from_provider() {
    let mut provider = Provider::with_id(
        "codex".to_string(),
        "Codex".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex",
                "ANTHROPIC_MODEL": "gpt-5.4",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.4-mini",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.4",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "gpt-5.4"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://stale.example.com",
            "ANTHROPIC_AUTH_TOKEN": "stale-token",
            "ANTHROPIC_MODEL": "stale-model",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "stale-haiku",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "Stale Haiku",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "stale-sonnet",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Stale Sonnet",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "stale-opus",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "Stale Opus"
        }
    });
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_MODEL", None);
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        Some("claude-haiku-5-5"),
    );
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
        Some("gpt-5.4-mini"),
    );
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        Some("claude-sonnet-5"),
    );
    assert_env_str(env, "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME", Some("gpt-5.4"));
    assert_env_str(env, "ANTHROPIC_DEFAULT_OPUS_MODEL", Some("claude-opus-5"));
    assert_env_str(env, "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME", Some("gpt-5.4"));
    // Codex 系只保留 AUTH_TOKEN；双键会触发 Claude Code 告警（#4919）
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
}

#[test]
fn managed_account_claude_takeover_codex_injects_auth_token_without_preexisting_key() {
    let mut provider = Provider::with_id(
        "codex".to_string(),
        "Codex".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });

    // 全新安装/热切换形态：传入的 env 没有任何 token 键。
    let mut live_config = provider.settings_config.clone();
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
}

#[test]
fn managed_account_claude_takeover_xai_keeps_one_auth_key() {
    let mut provider = Provider::with_id(
        "xai".to_string(),
        "xAI".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.x.ai/v1"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("xai_oauth".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "old-token",
            "ANTHROPIC_API_KEY": "old-key",
            "OPENAI_API_KEY": "old-openai-key"
        }
    });
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(Value::as_object)
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
    // Claude Code 不读这个键：不是关键字段，归用户，契约不碰。
    assert_env_str(env, "OPENAI_API_KEY", Some("old-openai-key"));
}

#[test]
fn managed_account_claude_takeover_codex_by_base_url_keeps_auth_token() {
    // 无 provider_type meta、仅凭 base_url 识别为受管 codex 的供应商，
    // 也必须保留 AUTH_TOKEN 占位符（与策略选择共用同一判定族）。
    let provider = Provider::with_id(
        "codex-url-only".to_string(),
        "Codex (URL only)".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        None,
    );
    assert!(provider.uses_managed_account_auth());
    assert!(!provider.is_codex_oauth());

    let mut live_config = provider.settings_config.clone();
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
}

// #4919 复现场景：从第三方 Claude 供应商（live 已有 AUTH_TOKEN）切换到
// Codex 受管供应商时，只应保留 AUTH_TOKEN 占位符，不得同时写入 API_KEY。
#[test]
fn managed_account_claude_takeover_codex_from_third_party_keeps_single_auth_key() {
    let mut provider = Provider::with_id(
        "codex".to_string(),
        "Codex".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
            "ANTHROPIC_AUTH_TOKEN": "sk-third-party"
        }
    });
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
}

#[test]
fn managed_account_claude_takeover_copilot_defaults_to_auth_token() {
    let mut provider = Provider::with_id(
        "copilot".to_string(),
        "GitHub Copilot".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("github_copilot".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://stale.example.com",
            "ANTHROPIC_AUTH_TOKEN": "stale-token",
            "ANTHROPIC_API_KEY": "stale-key"
        }
    });
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    // Default Copilot takeover injects AUTH_TOKEN: the API_KEY placeholder
    // triggers Claude Code's custom-key approval prompt (defaults to
    // "No (recommended)"), which lands users in "Not logged in".
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
}

#[test]
fn managed_account_claude_takeover_copilot_honors_api_key_field_choice() {
    let mut provider = Provider::with_id(
        "copilot".to_string(),
        "GitHub Copilot".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("github_copilot".to_string()),
        api_key_field: Some("ANTHROPIC_API_KEY".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://stale.example.com",
            "ANTHROPIC_AUTH_TOKEN": "stale-token"
        }
    });
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    // Explicit API-key-field choice keeps the API_KEY placeholder to avoid
    // conflicting with the /login-managed key (#1049).
    assert_env_str(env, "ANTHROPIC_API_KEY", Some(PROXY_TOKEN_PLACEHOLDER));
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", None);
}

#[test]
fn normal_claude_takeover_without_token_keeps_auth_token_fallback() {
    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.example.com",
            "ANTHROPIC_MODEL": "claude-haiku-4.5"
        }
    });

    let plain = Provider::with_id(
        "plain".to_string(),
        "Plain".to_string(),
        live_config.clone(),
        None,
    );
    live_config = takeover(&live_config, &plain);

    assert_eq!(
        live_config
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(|value| value.as_str()),
        Some(PROXY_TOKEN_PLACEHOLDER)
    );
    assert!(
        live_config
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .is_none(),
        "non-managed providers should retain the legacy fallback behavior"
    );
}

#[test]
fn normal_claude_takeover_keeps_api_key_rows_off_the_local_hop() {
    // #8104：OpenCode Go 这类预设把凭据存在 `ANTHROPIC_API_KEY`（上游跳要
    // x-api-key），路由模式的本地占位符只能写 `ANTHROPIC_AUTH_TOKEN`；
    // 残留的 `ANTHROPIC_API_KEY` 一并清掉，否则 Claude Code 弹自定义 key
    // 确认框，默认拒绝后就是未登录会话。
    let provider = Provider::with_id(
        "opencode-go".to_string(),
        "OpenCode Go".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go",
                "ANTHROPIC_API_KEY": "sk-go"
            }
        }),
        None,
    );
    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://stale.example.com",
            "ANTHROPIC_API_KEY": "stale-key"
        }
    });
    live_config = takeover(&live_config, &provider);

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
}
