//! OpenCode 网关（`opencode.ai`）在 Claude → OpenAI Chat 转换下的推理档位。
//!
//! Claude Code / Claude Desktop 的 `openai_chat` 供应商经
//! [`super::transform::anthropic_to_openai_with_reasoning_content`] 转换时，客户端选定的
//! 档位只在模型名命中 [`super::transform::supports_reasoning_effort`]（OpenAI o 系 /
//! GPT-5+ / Grok 4.5+）时才会被写成 `reasoning_effort`——第三方与本地模型 id
//! （`deepseek-*`、`glm-*` 等）永远不命中，档位被静默丢弃，上游只能跑默认档。
//!
//! OpenCode 网关的推理接口由**平台**决定、与模型厂商无关：其自家客户端对该传输发顶层
//! `reasoning_effort`（不发任何厂商原生 `thinking` 形状），合法档位逐模型声明，网关会
//! 拒绝未声明的值。因此这里先按**平台身份**（活动 base URL 的 host）判定，再按
//! **逐模型档位表**钳制——与 Codex 侧 `codexChatReasoning` 的 `effortValueMode: "zen"`
//! 同一套语义（`codex::infer_aggregator_platform_config` 的平台规则 +
//! [`super::transform_codex_chat::map_reasoning_effort`] 的 zen 分支）。钳制直接复用后者，
//! 不另写一份档位序，避免两条路径的钳制语义漂移。

use super::ProviderAdapter;
use crate::provider::Provider;

/// 平台身份只看 **adapter 真正会请求的那条 base URL** 的 host：优先级与
/// `ClaudeAdapter::extract_base_url` 完全一致（Codex / xAI OAuth 的强制端点 →
/// `env.ANTHROPIC_BASE_URL` → `base_url` → `baseURL` → `apiEndpoint`）。
///
/// 未被使用的历史字段与 provider 名字都不参与判定：名字是用户可改的自由文本，残留的
/// `base_url` 也不会把请求发往网关（`env` 优先），据其注入会误伤别家后端。
pub fn is_opencode_gateway(provider: &Provider) -> bool {
    super::ClaudeAdapter::new()
        .extract_base_url(provider)
        .is_ok_and(|url| is_opencode_url(&url))
}

/// host 边界：只认 `opencode.ai` 本身及其真子域（`zen.opencode.ai`）。
///
/// 接受 `host[:port]` 与 IPv6 方括号形态（`forwarder` 传的是 `Uri::authority()`），
/// 也接受 `Uri::host()` 的结果；`notopencode.ai`、`opencode.ai.evil.com` 一律不命中。
pub(crate) fn is_opencode_host(host: &str) -> bool {
    let host = host
        .rsplit_once(':')
        .map_or(host, |(h, _)| h)
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    host == "opencode.ai" || host.ends_with(".opencode.ai")
}

/// 从一条 base URL 取 host 再判边界；没有 scheme/host 的输入不命中。
fn is_opencode_url(url: &str) -> bool {
    let Ok(uri) = url.parse::<http::Uri>() else {
        return false;
    };
    uri.host().is_some_and(is_opencode_host)
}

/// 逐模型合法档位。查表分两层：
///
/// 1. **精确条目**（[`exact_effort_levels`]）：网关逐模型声明的档位。数据源与 Codex 预设
///    里 OpenCode Go 目录条目的 `reasoningLevels` 相同（镜像 models.dev，见
///    `src/config/codexProviderPresets.ts`）；`deepseek-v4.1-flash` 尚未进入该预设目录，
///    档位按网关侧声明收录——与 opencode 客户端严格按模型声明发 `reasoning_effort` 的
///    行为一致（档位表同样镜像 models.dev 的 `reasoning_options`）。
/// 2. **家族兜底**（[`DEEPSEEK_FAMILY_EFFORT_LEVELS`]）：只收录该家族「所有已知成员都
///    接受」的交集档位，让新发布的同族模型无需改代码即可生效。
///
/// 两层都不命中返回 `None`：**刻意不发该字段**——宁可保持上游默认，也不猜一个网关可能
/// 拒绝的档位（同 `transform_codex_chat` 的 zen 语义：无表不发）。
fn declared_effort_levels(model: &str) -> Option<&'static [&'static str]> {
    let model = model.trim().to_ascii_lowercase();

    if let Some(levels) = exact_effort_levels(&model) {
        return Some(levels);
    }
    if model.starts_with("deepseek-") {
        return Some(DEEPSEEK_FAMILY_EFFORT_LEVELS);
    }
    None
}

/// 精确条目：逐模型声明的档位。命中时优先于家族兜底，因此家族里档位更窄或更宽的成员
/// （`deepseek-v4-pro` 仅 high|max、`deepseek-v4-flash` 为 low|high|max）各自如实生效。
fn exact_effort_levels(model: &str) -> Option<&'static [&'static str]> {
    match model {
        "glm-5.3" | "glm-5.3-flash" => Some(&["low", "high", "max"]),
        "deepseek-v4-flash" | "deepseek-v4.1-flash" => Some(&["low", "high", "max"]),
        "deepseek-v4-pro" => Some(&["high", "max"]),
        "kimi-k3" => Some(&["max"]),
        _ => None,
    }
}

/// `deepseek-*` 家族的兜底档位 = 已知成员档位的**交集**：`deepseek-v4-pro` 仅
/// high|max，`deepseek-v4-flash` / `deepseek-v4.1-flash` 为 low|high|max，交集即
/// `{high, max}`。未收录的同族新模型按此钳制：`low` / `medium` 抬到 `high`，不会发出该
/// 家族窄档位成员无法接受的值。
///
/// 残余风险（刻意接受）：尚未出现的同族成员若收窄到仅 `max`、或根本不接受 effort 档位，
/// 兜底仍会发 `high`，上游可能拒绝；这类成员出现时补一条精确条目即可（精确条目优先）。
///
/// `glm-*` **刻意没有**家族兜底：`glm-5.1` 只有思考开关（无 effort 档位）、`glm-5.2`
/// 仅 high|max、`glm-5.3` 系列为 low|high|max，家族交集为空——按家族发会误伤前两者，
/// 因此新 glm 版本仍需补精确条目。
const DEEPSEEK_FAMILY_EFFORT_LEVELS: &[&str] = &["high", "max"];

/// 平台规则入口：命中 OpenCode 网关且该模型有档位表时，返回应写入的
/// `reasoning_effort`；否则 `None`，调用方保持原状。
///
/// `effort` 来自 [`super::transform::resolve_reasoning_effort`]（`low` / `medium` /
/// `high` / `xhigh` / `max`），钳制语义见复用函数：取「不低于请求的最近合法档」，
/// 请求超出最高档则取最高档，无法识别的值不发。
pub fn resolve_opencode_effort(provider: &Provider, model: &str, effort: &str) -> Option<String> {
    if !is_opencode_gateway(provider) {
        return None;
    }

    let levels: Vec<String> = declared_effort_levels(model)?
        .iter()
        .map(|level| (*level).to_string())
        .collect();

    super::transform_codex_chat::map_reasoning_effort(effort, Some("zen"), Some(levels.as_slice()))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(name: &str, settings_config: serde_json::Value) -> Provider {
        serde_json::from_value(json!({
            "id": "test",
            "name": name,
            "settingsConfig": settings_config,
        }))
        .expect("provider should deserialize")
    }

    fn opencode_provider() -> Provider {
        provider(
            "OpenCode Go",
            json!({"env": {"ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1"}}),
        )
    }

    #[test]
    fn detects_platform_from_the_active_base_url() {
        assert!(is_opencode_gateway(&opencode_provider()));
        assert!(is_opencode_gateway(&provider(
            "Relay",
            json!({"base_url": "https://opencode.ai/zen/v1"})
        )));
        // env 缺席时按适配器自己的优先级继续往下找。
        assert!(is_opencode_gateway(&provider(
            "Relay",
            json!({"baseURL": "https://zen.opencode.ai/v1"})
        )));
        assert!(is_opencode_gateway(&provider(
            "Relay",
            json!({"apiEndpoint": "https://opencode.ai/v1"})
        )));
        assert!(is_opencode_gateway(&provider(
            "Zen",
            json!({"env": {"ANTHROPIC_BASE_URL": "https://zen.opencode.ai/v1"}})
        )));
    }

    #[test]
    fn provider_name_alone_is_not_a_platform_signal() {
        // 名字是用户可改的自由文本，且没有地址本来就无法路由：不能据此判定平台。
        assert!(!is_opencode_gateway(&provider(
            "opencode.ai relay",
            json!({})
        )));
    }

    #[test]
    fn ignores_hosts_that_only_contain_the_gateway_name() {
        for url in [
            "https://notopencode.ai/v1",
            "https://opencode.ai.evil.com/v1",
            "https://example.com/proxy/opencode.ai/v1",
            "https://opencode.ai@evil.com/v1",
        ] {
            assert!(
                !is_opencode_gateway(&provider("Relay", json!({"base_url": url}))),
                "{url} must not be recognised as OpenCode"
            );
        }
    }

    #[test]
    fn accepts_the_gateway_host_its_subdomains_and_ports() {
        for url in [
            "https://opencode.ai/v1",
            "https://OPENCODE.AI:443/v1",
            "https://zen.opencode.ai/v1",
        ] {
            assert!(
                is_opencode_gateway(&provider("Relay", json!({"base_url": url}))),
                "{url} should be recognised as OpenCode"
            );
        }
    }

    #[test]
    fn unused_legacy_base_url_does_not_override_the_active_endpoint() {
        // 与 `ClaudeAdapter::extract_base_url` 同序：env 赢，历史 base_url 只是残留。
        let stale = provider(
            "Relay",
            json!({
                "env": {"ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic"},
                "base_url": "https://opencode.ai/zen/v1",
            }),
        );
        assert!(!is_opencode_gateway(&stale));
    }

    #[test]
    fn does_not_detect_unrelated_providers() {
        // 名字里带裸词 opencode、地址却是别家：按域名判定，不该命中。
        assert!(!is_opencode_gateway(&provider(
            "my-opencode-relay",
            json!({"base_url": "https://relay.example.com/v1"})
        )));
        assert!(!is_opencode_gateway(&provider(
            "DeepSeek",
            json!({"env": {"ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic"}})
        )));
        assert!(!is_opencode_gateway(&provider(
            "OpenRouter",
            json!({"base_url": "https://openrouter.ai/api/v1"})
        )));
    }

    #[test]
    fn clamps_to_declared_levels() {
        let provider = opencode_provider();

        // Desktop 的档位形态：max / low 原样（表内），medium 抬到最近合法档。
        assert_eq!(
            resolve_opencode_effort(&provider, "deepseek-v4.1-flash", "max").as_deref(),
            Some("max")
        );
        assert_eq!(
            resolve_opencode_effort(&provider, "deepseek-v4.1-flash", "low").as_deref(),
            Some("low")
        );
        assert_eq!(
            resolve_opencode_effort(&provider, "deepseek-v4.1-flash", "medium").as_deref(),
            Some("high")
        );
        // adaptive → xhigh，超出表内最高档 → 取最高档 max。
        assert_eq!(
            resolve_opencode_effort(&provider, "deepseek-v4.1-flash", "xhigh").as_deref(),
            Some("max")
        );

        // 逐模型差异：pro 只有 high/max，kimi-k3 只有 max。
        assert_eq!(
            resolve_opencode_effort(&provider, "deepseek-v4-pro", "low").as_deref(),
            Some("high")
        );
        assert_eq!(
            resolve_opencode_effort(&provider, "kimi-k3", "low").as_deref(),
            Some("max")
        );
    }

    #[test]
    fn sends_nothing_for_models_without_a_declared_table() {
        let provider = opencode_provider();

        // mimo-v2.5-pro 在预设目录里没有 reasoningLevels（只支持思考开关）→ 不发字段。
        assert_eq!(
            resolve_opencode_effort(&provider, "mimo-v2.5-pro", "high"),
            None
        );
        // 未收录的自定义模型同样不发，而不是猜一个值。
        assert_eq!(
            resolve_opencode_effort(&provider, "my-local-model", "max"),
            None
        );
    }

    #[test]
    fn family_fallback_covers_new_deepseek_models_with_the_intersection() {
        let provider = opencode_provider();

        // 未收录的同族新模型：按家族交集 {high, max} 钳制，low/medium 抬到 high。
        for (effort, expected) in [
            ("low", "high"),
            ("medium", "high"),
            ("high", "high"),
            ("xhigh", "max"),
            ("max", "max"),
        ] {
            assert_eq!(
                resolve_opencode_effort(&provider, "deepseek-v5-flash", effort).as_deref(),
                Some(expected),
                "effort={effort}"
            );
        }
    }

    #[test]
    fn exact_entries_win_over_the_family_fallback() {
        let provider = opencode_provider();

        // 精确条目 deepseek-v4-flash 是 low|high|max：家族兜底不得把 low 抬成 high。
        assert_eq!(
            resolve_opencode_effort(&provider, "deepseek-v4-flash", "low").as_deref(),
            Some("low")
        );
        assert_eq!(
            resolve_opencode_effort(&provider, "deepseek-v4.1-flash", "low").as_deref(),
            Some("low")
        );
    }

    #[test]
    fn glm_family_has_no_fallback_because_members_disagree() {
        let provider = opencode_provider();

        // glm-5.1 无 effort 档位、glm-5.2 仅 high|max、glm-5.3 系列 low|high|max：
        // 家族交集为空，未收录的 glm 版本刻意不发字段，避免给窄档位/开关型成员发非法值。
        assert_eq!(resolve_opencode_effort(&provider, "glm-5.4", "high"), None);
        assert_eq!(resolve_opencode_effort(&provider, "glm-5.2", "high"), None);
    }

    #[test]
    fn ignores_other_platforms_and_unknown_efforts() {
        let other = provider(
            "DeepSeek",
            json!({"env": {"ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic"}}),
        );
        assert_eq!(
            resolve_opencode_effort(&other, "deepseek-v4.1-flash", "max"),
            None
        );

        let provider = opencode_provider();
        // 无法识别的档位值：不发（同 zen 语义）。
        assert_eq!(
            resolve_opencode_effort(&provider, "deepseek-v4.1-flash", "turbo"),
            None
        );
        assert_eq!(
            resolve_opencode_effort(&provider, "deepseek-v4.1-flash", "none"),
            None
        );
    }

    #[test]
    fn model_lookup_ignores_case_and_surrounding_space() {
        let provider = opencode_provider();
        assert_eq!(
            resolve_opencode_effort(&provider, " GLM-5.3-Flash ", "low").as_deref(),
            Some("low")
        );
    }
}
