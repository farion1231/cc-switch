//! Claude 路由经 OpenAI Chat / Responses 转发时，给上游发哪一档思考强度（#9062）。
//!
//! 转换层只给认得的模型（[`super::transform::supports_reasoning_effort`]：o 系、
//! GPT-5+、Grok 4.5+）写档位，其余模型在这里接着判断：
//! - 供应商设了 `meta.claudeReasoningEffort`，按设置来：不发，或客户端要哪档原样发哪档。
//! - 没设时按供应商名、地址、模型名认厂商（与 Codex Responses → Chat 共用
//!   [`super::codex::infer_chat_reasoning_config`]）：认出不收档位的（Kimi、GLM、Qwen…）
//!   不发；认出有专门档位的（DeepSeek、OpenRouter…）按它的换算发。
//! - 已知不收档位的模型（GPT-3.5 / GPT-4 系、Llama）不发；Mistral 开推理后的响应格式
//!   转换层还不解析，也不发。
//! - 都认不出时只发 low / medium / high：这三档是 OpenAI 的标准档，收档位字段的上游
//!   基本都认；xhigh / max 折成 high，免得发出对方不认的值被 400。
//!
//! 关思考（`thinking` 缺省或 disabled）时不发档位，交给上游默认。托管账号（Copilot、
//! Codex / xAI OAuth）的模型和参数由平台限定，自动模式下保持转换层的结果。

use crate::provider::Provider;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffortPlan {
    /// 保持转换层的结果（认得的模型，或托管账号）。
    Unchanged,
    /// 不发档位。
    Omit,
    /// 发这一档。
    Send(&'static str),
}

/// 用户在供应商上能选的覆盖值；缺省或不认识的值都按自动处理。
const OVERRIDE_MODES: [&str; 2] = ["off", "passthrough"];

/// 决定这次请求发哪一档。`body` 是模型映射后的 Anthropic 请求体，`base_url` 是
/// 供应商的上游地址。
pub(crate) fn plan(body: &Value, provider: &Provider, base_url: &str) -> EffortPlan {
    let (plan, reason) = decide(body, provider, base_url);
    if plan != EffortPlan::Unchanged {
        log::debug!(
            "[Reasoning] Claude 路由档位：provider={}, model={}, plan={plan:?}, reason={reason}",
            provider.id,
            body.get("model")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
    }
    plan
}

fn decide(body: &Value, provider: &Provider, base_url: &str) -> (EffortPlan, &'static str) {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if let Some(mode) = override_mode(provider) {
        if mode == "off" {
            return (EffortPlan::Omit, "override:off");
        }
        // 原样发送：客户端要哪档发哪档（自动判定为不发的厂商也发），关思考时照样不发。
        let plan = requested_effort(body).map_or(EffortPlan::Omit, EffortPlan::Send);
        return (plan, "override:passthrough");
    }

    if super::transform::supports_reasoning_effort(model) {
        return (EffortPlan::Unchanged, "known-model");
    }
    if is_managed_account(provider, base_url) {
        return (EffortPlan::Unchanged, "managed-account");
    }
    if is_known_non_reasoning_model(model) {
        return (EffortPlan::Omit, "non-reasoning-model");
    }
    if is_mistral(provider, base_url, model) {
        return (EffortPlan::Omit, "mistral-chunked-content");
    }

    let Some(requested) = requested_effort(body) else {
        return (EffortPlan::Omit, "thinking-off");
    };

    match super::codex::infer_chat_reasoning_config(&provider.name, base_url, model) {
        Some(config) => {
            let sends_effort = config.supports_effort.unwrap_or(false)
                && config.effort_param.as_deref().map(str::trim) != Some("none");
            if !sends_effort {
                return (EffortPlan::Omit, "vendor-without-effort");
            }
            // Codex 侧的 passthrough 靠客户端只发模型声明过的档位；Claude Code 还会发
            // xhigh / max，这里按标准三档收敛。
            let mode = config
                .effort_value_mode
                .as_deref()
                .filter(|mode| *mode != "passthrough");
            let Some(mode) = mode else {
                return (EffortPlan::Send(conservative(requested)), "vendor");
            };
            let mapped: Option<&'static str> =
                super::transform_codex_chat::map_reasoning_effort(requested, Some(mode), None);
            (mapped.map_or(EffortPlan::Omit, EffortPlan::Send), "vendor")
        }
        None => (EffortPlan::Send(conservative(requested)), "conservative"),
    }
}

/// 已知不收思考档位的模型：OpenAI GPT-3.5 / GPT-4 系（含 gpt-4o、gpt-4.1）和 Llama。
/// 带厂商前缀的名字（`openai/gpt-4o`、`meta-llama/Llama-3.3-70B`）按最后一段判断。
fn is_known_non_reasoning_model(model: &str) -> bool {
    let name = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .trim()
        .to_ascii_lowercase();
    let gpt_before_5 = name
        .strip_prefix("gpt-")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_digit() && c < '5');
    gpt_before_5 || name.starts_with("chatgpt-4") || name.starts_with("llama")
}

/// Mistral 开推理后流式 `content` 是 thinking / text 分块数组，转换层还不解析（正文会丢），
/// 先不给它发档位（与 Codex 的 Mistral 预设同口径）。
fn is_mistral(provider: &Provider, base_url: &str, model: &str) -> bool {
    format!("{} {base_url} {model}", provider.name)
        .to_ascii_lowercase()
        .contains("mistral")
}

fn override_mode(provider: &Provider) -> Option<&str> {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.claude_reasoning_effort.as_deref())
        .map(str::trim)
        .filter(|mode| OVERRIDE_MODES.contains(mode))
}

fn is_managed_account(provider: &Provider, base_url: &str) -> bool {
    provider.is_github_copilot()
        || provider.is_codex_oauth()
        || provider.is_xai_oauth()
        || base_url.contains("githubcopilot.com")
}

/// 客户端要的档位，不按目标模型折算。关思考时为 `None`。
fn requested_effort(body: &Value) -> Option<&'static str> {
    if matches!(
        body.pointer("/thinking/type").and_then(Value::as_str),
        None | Some("disabled")
    ) {
        return None;
    }
    if let Some(effort) = body
        .pointer("/output_config/effort")
        .and_then(Value::as_str)
    {
        return match effort {
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "xhigh" => Some("xhigh"),
            "max" => Some("max"),
            _ => None,
        };
    }
    super::transform::effort_from_thinking(body)
}

fn conservative(effort: &'static str) -> &'static str {
    match effort {
        "low" => "low",
        "medium" => "medium",
        _ => "high",
    }
}

/// 把 [`plan`] 的结果写进转换后的请求体：Chat 写顶层 `reasoning_effort`，
/// Responses 写 `reasoning.effort`。
pub(crate) fn apply(result: &mut Value, api_format: &str, plan: EffortPlan) {
    let Some(obj) = result.as_object_mut() else {
        return;
    };
    match (api_format, plan) {
        (_, EffortPlan::Unchanged) => {}
        ("openai_chat", EffortPlan::Omit) => {
            obj.remove("reasoning_effort");
        }
        ("openai_chat", EffortPlan::Send(effort)) => {
            obj.insert("reasoning_effort".to_string(), json!(effort));
        }
        ("openai_responses", EffortPlan::Omit) => {
            let now_empty = match obj.get_mut("reasoning").and_then(Value::as_object_mut) {
                Some(reasoning) => {
                    reasoning.remove("effort");
                    reasoning.is_empty()
                }
                None => false,
            };
            if now_empty {
                obj.remove("reasoning");
            }
        }
        ("openai_responses", EffortPlan::Send(effort)) => {
            match obj.get_mut("reasoning").and_then(Value::as_object_mut) {
                Some(reasoning) => {
                    reasoning.insert("effort".to_string(), json!(effort));
                }
                None => {
                    obj.insert("reasoning".to_string(), json!({ "effort": effort }));
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;

    fn provider(name: &str, meta: Option<ProviderMeta>) -> Provider {
        Provider {
            id: "test".to_string(),
            name: name.to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn with_override(mode: &str) -> Option<ProviderMeta> {
        Some(ProviderMeta {
            claude_reasoning_effort: Some(mode.to_string()),
            ..Default::default()
        })
    }

    fn body(model: &str, effort: Option<&str>) -> Value {
        let mut body = json!({
            "model": model,
            "thinking": { "type": "adaptive" },
            "messages": [{ "role": "user", "content": "hi" }]
        });
        if let Some(effort) = effort {
            body["output_config"] = json!({ "effort": effort });
        }
        body
    }

    const RELAY: &str = "https://relay.example.com/v1";

    #[test]
    fn unknown_model_sends_only_standard_tiers() {
        let p = provider("Groq", None);
        let url = "https://api.groq.com/openai/v1";
        let model = "openai/gpt-oss-120b";
        assert_eq!(
            plan(&body(model, Some("low")), &p, url),
            EffortPlan::Send("low")
        );
        assert_eq!(
            plan(&body(model, Some("medium")), &p, url),
            EffortPlan::Send("medium")
        );
        assert_eq!(
            plan(&body(model, Some("xhigh")), &p, url),
            EffortPlan::Send("high")
        );
        assert_eq!(
            plan(&body(model, Some("max")), &p, url),
            EffortPlan::Send("high")
        );
        // adaptive 不带档位按 xhigh 算，再折成 high。
        assert_eq!(plan(&body(model, None), &p, url), EffortPlan::Send("high"));
    }

    #[test]
    fn thinking_off_sends_nothing() {
        let p = provider("Relay", None);
        let mut disabled = body("some-model", Some("high"));
        disabled["thinking"] = json!({ "type": "disabled" });
        assert_eq!(plan(&disabled, &p, RELAY), EffortPlan::Omit);

        let mut absent = body("some-model", Some("high"));
        absent.as_object_mut().unwrap().remove("thinking");
        assert_eq!(plan(&absent, &p, RELAY), EffortPlan::Omit);
    }

    #[test]
    fn enabled_budget_buckets_into_standard_tiers() {
        let p = provider("Relay", None);
        let mut b = body("some-model", None);
        b["thinking"] = json!({ "type": "enabled", "budget_tokens": 2_000 });
        assert_eq!(plan(&b, &p, RELAY), EffortPlan::Send("low"));
        b["thinking"] = json!({ "type": "enabled", "budget_tokens": 31_999 });
        assert_eq!(plan(&b, &p, RELAY), EffortPlan::Send("high"));
    }

    #[test]
    fn deepseek_maps_to_low_high_max() {
        // #9062：自建 Responses 网关上的 deepseek 模型。官方档位 low / high / max。
        let p = provider("My Gateway", None);
        let model = "deepseek-flash";
        assert_eq!(
            plan(&body(model, Some("low")), &p, RELAY),
            EffortPlan::Send("low")
        );
        assert_eq!(
            plan(&body(model, Some("medium")), &p, RELAY),
            EffortPlan::Send("high")
        );
        assert_eq!(
            plan(&body(model, Some("high")), &p, RELAY),
            EffortPlan::Send("high")
        );
        // 照官方服务端的换算：xhigh 当 high。adaptive 不带档位按 xhigh 算，也发 high。
        assert_eq!(
            plan(&body(model, Some("xhigh")), &p, RELAY),
            EffortPlan::Send("high")
        );
        assert_eq!(
            plan(&body(model, None), &p, RELAY),
            EffortPlan::Send("high")
        );
        assert_eq!(
            plan(&body(model, Some("max")), &p, RELAY),
            EffortPlan::Send("max")
        );
    }

    #[test]
    fn vendors_without_effort_get_nothing() {
        for (name, model) in [
            ("Nvidia", "moonshotai/kimi-k2.6"),
            ("Nebius", "zai-org/GLM-5.3"),
            ("Bailian", "qwen3-max"),
            ("Relay", "MiniMax-M3"),
        ] {
            let p = provider(name, None);
            assert_eq!(
                plan(&body(model, Some("high")), &p, RELAY),
                EffortPlan::Omit,
                "{name} / {model}"
            );
        }
    }

    #[test]
    fn known_non_reasoning_models_get_nothing() {
        let p = provider("Relay", None);
        for model in [
            "gpt-4o",
            "gpt-4o-mini",
            "openai/gpt-4.1",
            "gpt-3.5-turbo",
            "chatgpt-4o-latest",
            "meta-llama/Llama-3.3-70B-Instruct",
            "llama3.1:8b",
        ] {
            assert_eq!(
                plan(&body(model, Some("high")), &p, RELAY),
                EffortPlan::Omit,
                "{model}"
            );
        }
        // gpt-oss 是推理模型，不在此列。
        assert_eq!(
            plan(&body("openai/gpt-oss-120b", Some("high")), &p, RELAY),
            EffortPlan::Send("high")
        );
    }

    #[test]
    fn mistral_gets_nothing_until_chunked_content_is_parsed() {
        let official = provider("Mistral", None);
        assert_eq!(
            plan(
                &body("mistral-medium-3-5", Some("high")),
                &official,
                "https://api.mistral.ai"
            ),
            EffortPlan::Omit
        );
        let relay = provider("Relay", None);
        assert_eq!(
            plan(&body("mistral-small-2603", Some("high")), &relay, RELAY),
            EffortPlan::Omit
        );
    }

    #[test]
    fn stepfun_passthrough_keeps_standard_tiers() {
        // Codex 侧对 step-3.7-flash 是 passthrough；Claude Code 的 xhigh / max 要收敛到 high。
        let p = provider("StepFun", None);
        let model = "step-3.7-flash";
        assert_eq!(
            plan(&body(model, None), &p, RELAY),
            EffortPlan::Send("high")
        );
        assert_eq!(
            plan(&body(model, Some("max")), &p, RELAY),
            EffortPlan::Send("high")
        );
        assert_eq!(
            plan(&body(model, Some("medium")), &p, RELAY),
            EffortPlan::Send("medium")
        );
        // 2603 走 low_high，medium 折成 high。
        assert_eq!(
            plan(&body("step-3.5-flash-2603", Some("medium")), &p, RELAY),
            EffortPlan::Send("high")
        );
    }

    #[test]
    fn openrouter_platform_caps_at_xhigh() {
        let p = provider("OpenRouter", None);
        let url = "https://openrouter.ai/api/v1";
        assert_eq!(
            plan(&body("deepseek/deepseek-v4-pro", Some("max")), &p, url),
            EffortPlan::Send("xhigh")
        );
        assert_eq!(
            plan(&body("deepseek/deepseek-v4-pro", Some("medium")), &p, url),
            EffortPlan::Send("medium")
        );
    }

    #[test]
    fn known_models_and_managed_accounts_stay_unchanged() {
        let p = provider("Relay", None);
        assert_eq!(
            plan(&body("gpt-5.5", Some("high")), &p, RELAY),
            EffortPlan::Unchanged
        );
        assert_eq!(
            plan(&body("grok-4.6", Some("high")), &p, RELAY),
            EffortPlan::Unchanged
        );

        let copilot = provider("Copilot", None);
        assert_eq!(
            plan(
                &body("gemini-3-pro", Some("high")),
                &copilot,
                "https://api.githubcopilot.com"
            ),
            EffortPlan::Unchanged
        );
    }

    #[test]
    fn overrides_take_precedence() {
        let off = provider("Relay", with_override("off"));
        assert_eq!(
            plan(&body("gpt-5.5", Some("high")), &off, RELAY),
            EffortPlan::Omit
        );

        let passthrough = provider("Relay", with_override("passthrough"));
        assert_eq!(
            plan(&body("some-model", Some("max")), &passthrough, RELAY),
            EffortPlan::Send("max")
        );
        let mut thinking_off = body("some-model", Some("max"));
        thinking_off["thinking"] = json!({ "type": "disabled" });
        assert_eq!(plan(&thinking_off, &passthrough, RELAY), EffortPlan::Omit);

        // 自动判定为不发的（名称里带 Kimi），原样发送时照样发。
        let named_kimi = provider("Kimi Relay", with_override("passthrough"));
        assert_eq!(
            plan(&body("gpt-oss-120b", Some("high")), &named_kimi, RELAY),
            EffortPlan::Send("high")
        );

        // 不认识的值（含早期草稿里的 deepseek / openrouter）按自动处理。
        for mode in ["turbo", "deepseek", "openrouter"] {
            let unknown = provider("Relay", with_override(mode));
            assert_eq!(
                plan(&body("custom-model", Some("max")), &unknown, RELAY),
                EffortPlan::Send("high"),
                "{mode}"
            );
        }
    }

    #[test]
    fn apply_writes_chat_and_responses_fields() {
        let mut chat = json!({ "model": "m", "reasoning_effort": "high" });
        apply(&mut chat, "openai_chat", EffortPlan::Omit);
        assert!(chat.get("reasoning_effort").is_none());
        apply(&mut chat, "openai_chat", EffortPlan::Send("medium"));
        assert_eq!(chat["reasoning_effort"], "medium");

        let mut responses = json!({ "model": "m" });
        apply(&mut responses, "openai_responses", EffortPlan::Send("high"));
        assert_eq!(responses["reasoning"], json!({ "effort": "high" }));
        apply(&mut responses, "openai_responses", EffortPlan::Omit);
        assert!(responses.get("reasoning").is_none());

        let mut with_summary = json!({ "reasoning": { "effort": "high", "summary": "auto" } });
        apply(&mut with_summary, "openai_responses", EffortPlan::Omit);
        assert_eq!(with_summary["reasoning"], json!({ "summary": "auto" }));

        let mut untouched = json!({ "reasoning_effort": "xhigh" });
        apply(&mut untouched, "openai_chat", EffortPlan::Unchanged);
        assert_eq!(untouched["reasoning_effort"], "xhigh");
    }
}
