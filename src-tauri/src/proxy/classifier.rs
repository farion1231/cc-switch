//! Auto Mode 安全分类器请求的识别与降配
//!
//! Claude Code 的 Auto Mode 在执行 Bash 命令前会先发一条「安全分类器」请求，
//! 客户端对该请求有硬超时，超时即判定分类器不可用、连带拦下工具调用。
//! 若当前供应商默认开启 thinking，一次思考往返极易撞上这个上限。
//! 识别出这类请求后可以：
//!   1) 路由到专用的「分类器队列」（快 / 便宜的供应商）
//!   2) 强制关闭 thinking
//!
//! 识别规则硬编码、不开放配置。代价是 Claude Code 改动提示词文案后会静默失效
//! （fail-open：请求原样透传，不报错），`[CLS-001]` 日志是用户唯一的自查手段。

use serde_json::{json, Value};

/// 计费头前缀 —— 普通会话同样携带，必须与安全监控提示词同时命中
const BILLING_HEADER_PREFIX: &str = "x-anthropic-billing-header:";
/// 安全分类器系统提示词前缀
const SECURITY_MONITOR_PREFIX: &str = "You are a security monitor";

/// stage1 请求独有的停止序列（Claude Code 侧 `stop_sequences`）
///
/// 这是最强的单一信号：普通对话不会带这两个串。
const CLASSIFIER_STOP_SEQUENCES: [&str; 2] = ["</severity>", "</block>"];

/// 分类器提示词特征串
///
/// stage2 **不带** stop_sequences，只能靠末条 user 消息里的这些串兜底识别。
const CLASSIFIER_PROMPT_MARKERS: [&str; 3] = [
    "Output <severity>N</severity> where N is an integer 0-100",
    "Your ENTIRE response MUST begin with <block>",
    "Respond with <severity>N</severity> ONLY",
];

/// 分类器请求的总时间预算（秒）
///
/// 客户端硬超时的**确切值未经本仓库验证**，外部说法互相冲突且都没复现过，
/// 因此这里不按某个具体数字推导，只取一个明显偏保守的上限：既远短于代理默认的
/// 600 秒（否则我们永远比客户端晚放弃，故障转移到下一家时对方早已断开），
/// 又宽到不会掐死一个本来能成功的响应。实测该请求通常 1.5~2 秒返回，
/// 距这个上限有一个数量级的余量。
const CLASSIFIER_TOTAL_BUDGET_SECS: u32 = 28;

/// 分类器请求最多尝试几家供应商
///
/// 队列再长也不额外消耗墙钟时间；多出来的成员仍作为「熔断跳过」的替补有效。
const CLASSIFIER_MAX_ATTEMPTS: u32 = 2;

/// 分类器请求的单次尝试预算
///
/// 按**实际会尝试的家数**均分总预算，而不是给每家发一个固定的小值：
/// 队列里只有一家时，把 28 秒全给它——否则单供应商场景下 12 秒就掐断，
/// 会把「20 秒本可成功」变成硬失败，而客户端此时还远没有放弃。
///
/// 返回 `(单次超时秒数, max_retries)`。
pub fn attempt_budget(provider_count: usize) -> (u32, u32) {
    let attempts = (provider_count.max(1) as u32).min(CLASSIFIER_MAX_ATTEMPTS);
    let per_attempt = (CLASSIFIER_TOTAL_BUDGET_SECS / attempts).max(1);
    (per_attempt, attempts - 1)
}

/// 判断是否为 Claude Code Auto Mode 的 Bash 安全分类器请求
///
/// 三个**互相独立**的锚点，任一命中即可 —— 单一签名扛不住 Claude Code 改文案，
/// 也覆盖不了两个 stage：
/// 1. `stop_sequences` 含 `</severity>` / `</block>`（stage1 最强信号）
/// 2. 末条 user 消息含分类器提示词特征串（stage2 唯一可用的信号）
/// 3. `system` 数组里**同时**有计费头前缀和安全监控提示词前缀
///
/// 第 3 条的两个条件缺一不可 —— 计费头在普通会话里也会出现，只判它必然误伤
/// 每一次对话。前两条则各自足够特异，普通对话不会出现。
pub fn is_security_classifier_request(body: &Value) -> bool {
    has_classifier_stop_sequence(body)
        || last_user_text_has_marker(body)
        || has_security_monitor_system(body)
}

/// stage1：`stop_sequences` 锚点
fn has_classifier_stop_sequence(body: &Value) -> bool {
    body.get("stop_sequences")
        .and_then(Value::as_array)
        .is_some_and(|seqs| {
            seqs.iter().filter_map(Value::as_str).any(|s| {
                CLASSIFIER_STOP_SEQUENCES
                    .iter()
                    .any(|marker| s.eq_ignore_ascii_case(marker))
            })
        })
}

/// stage2：末条 user 消息的提示词特征串
fn last_user_text_has_marker(body: &Value) -> bool {
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return false;
    };
    let Some(last_user) = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
    else {
        return false;
    };

    match last_user.get("content") {
        Some(Value::String(text)) => text_has_marker(text),
        Some(Value::Array(blocks)) => blocks.iter().any(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(text_has_marker)
        }),
        _ => false,
    }
}

fn text_has_marker(text: &str) -> bool {
    CLASSIFIER_PROMPT_MARKERS
        .iter()
        .any(|marker| text.contains(marker))
}

/// `system` 数组双前缀锚点（deepseek-claude-proxy 的判定方式）
fn has_security_monitor_system(body: &Value) -> bool {
    // `system` 也可能是纯字符串或缺失，这两种形态一律不命中本条
    let Some(system) = body.get("system").and_then(Value::as_array) else {
        return false;
    };

    let mut has_billing_header = false;
    let mut has_security_monitor = false;

    for block in system {
        // 只看 text block；image / tool_result 之类没有 text 字段，自然跳过
        let Some(text) = block.get("text").and_then(Value::as_str) else {
            continue;
        };
        let trimmed = text.trim_start();
        if trimmed.starts_with(BILLING_HEADER_PREFIX) {
            has_billing_header = true;
        }
        if trimmed.starts_with(SECURITY_MONITOR_PREFIX) {
            has_security_monitor = true;
        }
        if has_billing_header && has_security_monitor {
            return true;
        }
    }

    false
}

/// 对分类器请求关闭 thinking，返回是否真正改动了请求体（用于日志）
///
/// 三步缺一不可：
/// - `thinking = {"type":"disabled"}` —— Anthropic 原生上游据此关闭思考
/// - 删除 `output_config` —— `providers::transform::resolve_reasoning_effort` 里
///   `output_config.effort` 的优先级**高于** thinking；不删则 Chat / Responses
///   上游仍会注入 reasoning_effort，thinking-off 白做
/// - 删除 `reasoning_effort` —— 客户端可能直接透传该字段
pub fn disable_thinking(body: &mut Value) -> bool {
    let Some(obj) = body.as_object_mut() else {
        return false;
    };

    let already_disabled = obj
        .get("thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str)
        == Some("disabled");

    let mut changed = !already_disabled;
    obj.insert("thinking".to_string(), json!({ "type": "disabled" }));
    changed |= obj.remove("reasoning_effort").is_some();
    changed |= obj.remove("output_config").is_some();
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text_block(text: &str) -> Value {
        json!({ "type": "text", "text": text })
    }

    fn classifier_body() -> Value {
        json!({
            "model": "claude-haiku-4-5",
            "system": [
                text_block("x-anthropic-billing-header: abc123"),
                text_block("You are a security monitor for Bash commands."),
            ],
        })
    }

    #[test]
    fn detects_when_both_prefixes_present() {
        assert!(is_security_classifier_request(&classifier_body()));
    }

    // ---- stage1: stop_sequences 锚点 ----

    #[test]
    fn detects_stage1_via_stop_sequences() {
        for marker in ["</severity>", "</block>"] {
            let body = json!({
                "model": "claude-sonnet-5",
                "stop_sequences": [marker],
                "messages": [{ "role": "user", "content": "ls -la" }],
            });
            assert!(
                is_security_classifier_request(&body),
                "should detect stop sequence {marker}"
            );
        }
    }

    #[test]
    fn stop_sequence_match_is_case_insensitive() {
        let body = json!({ "stop_sequences": ["</SEVERITY>"] });
        assert!(is_security_classifier_request(&body));
    }

    #[test]
    fn ordinary_stop_sequences_do_not_match() {
        let body = json!({ "stop_sequences": ["\n\nHuman:", "</thinking>"] });
        assert!(!is_security_classifier_request(&body));
    }

    // ---- stage2: 提示词特征串（stage2 不带 stop_sequences） ----

    #[test]
    fn detects_stage2_via_prompt_marker_in_string_content() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": "Output <severity>N</severity> where N is an integer 0-100",
            }],
        });
        assert!(is_security_classifier_request(&body));
    }

    #[test]
    fn detects_stage2_via_prompt_marker_in_block_content() {
        let body = json!({
            "messages": [
                { "role": "user", "content": "earlier turn" },
                { "role": "assistant", "content": "ok" },
                { "role": "user", "content": [
                    { "type": "text", "text": "Your ENTIRE response MUST begin with <block>" }
                ]},
            ],
        });
        assert!(is_security_classifier_request(&body));
    }

    #[test]
    fn marker_only_checked_on_last_user_message() {
        // 历史里出现过分类器提示词，但当前这轮是普通对话 —— 不该命中
        let body = json!({
            "messages": [
                { "role": "user", "content": "Respond with <severity>N</severity> ONLY" },
                { "role": "assistant", "content": "50" },
                { "role": "user", "content": "现在帮我重构这个函数" },
            ],
        });
        assert!(!is_security_classifier_request(&body));
    }

    #[test]
    fn ordinary_conversation_does_not_match_any_signal() {
        let body = json!({
            "model": "claude-sonnet-5",
            "system": [text_block("x-anthropic-billing-header: abc123")],
            "stop_sequences": ["\n\nHuman:"],
            "messages": [{ "role": "user", "content": "写个快排" }],
        });
        assert!(!is_security_classifier_request(&body));
    }

    // ---- 时间预算 ----

    #[test]
    fn single_provider_gets_the_whole_budget() {
        // 只有一家时不能掐成小超时：否则 20 秒本可成功的请求会被硬失败，
        // 而客户端此时还远没有放弃
        let (per_attempt, max_retries) = attempt_budget(1);
        assert_eq!(max_retries, 0);
        assert_eq!(per_attempt, CLASSIFIER_TOTAL_BUDGET_SECS);
    }

    #[test]
    fn two_providers_split_the_budget() {
        let (per_attempt, max_retries) = attempt_budget(2);
        assert_eq!(max_retries, 1);
        assert_eq!(per_attempt, CLASSIFIER_TOTAL_BUDGET_SECS / 2);
    }

    #[test]
    fn long_queue_never_exceeds_the_total_budget() {
        // 队列再长也不额外消耗墙钟时间
        for count in [3usize, 5, 20] {
            let (per_attempt, max_retries) = attempt_budget(count);
            let attempts = max_retries + 1;
            assert_eq!(attempts, CLASSIFIER_MAX_ATTEMPTS);
            assert!(
                per_attempt * attempts <= CLASSIFIER_TOTAL_BUDGET_SECS,
                "count={count} blew the total budget"
            );
        }
    }

    #[test]
    fn empty_provider_count_is_not_a_zero_timeout() {
        // 0 会被 create_forwarder 解读成「禁用超时」，绝不能算出 0
        let (per_attempt, _) = attempt_budget(0);
        assert!(per_attempt > 0);
    }

    #[test]
    fn rejects_billing_header_only() {
        // 关键守卫：普通会话同样携带计费头，只判它会误伤每一次对话
        let body = json!({
            "system": [
                text_block("x-anthropic-billing-header: abc123"),
                text_block("You are Claude Code, Anthropic's official CLI."),
            ],
        });
        assert!(!is_security_classifier_request(&body));
    }

    #[test]
    fn rejects_security_monitor_only() {
        let body = json!({
            "system": [text_block("You are a security monitor for Bash commands.")],
        });
        assert!(!is_security_classifier_request(&body));
    }

    #[test]
    fn rejects_system_as_plain_string() {
        let body = json!({ "system": "You are a security monitor" });
        assert!(!is_security_classifier_request(&body));
    }

    #[test]
    fn rejects_missing_system() {
        assert!(!is_security_classifier_request(&json!({ "model": "x" })));
        assert!(!is_security_classifier_request(&json!("not an object")));
    }

    #[test]
    fn ignores_non_text_blocks() {
        let body = json!({
            "system": [
                json!({ "type": "image", "source": { "data": "..." } }),
                text_block("x-anthropic-billing-header: abc123"),
                json!({ "type": "text" }),
                text_block("You are a security monitor."),
            ],
        });
        assert!(is_security_classifier_request(&body));
    }

    #[test]
    fn matches_regardless_of_block_order() {
        let body = json!({
            "system": [
                text_block("You are a security monitor."),
                text_block("x-anthropic-billing-header: abc123"),
            ],
        });
        assert!(is_security_classifier_request(&body));
    }

    #[test]
    fn tolerates_leading_whitespace() {
        let body = json!({
            "system": [
                text_block("\n  x-anthropic-billing-header: abc123"),
                text_block("  You are a security monitor."),
            ],
        });
        assert!(is_security_classifier_request(&body));
    }

    #[test]
    fn disable_thinking_sets_disabled_and_strips_reasoning_fields() {
        let mut body = json!({
            "model": "claude-sonnet-5",
            "thinking": { "type": "enabled", "budget_tokens": 8000 },
            "reasoning_effort": "high",
            "output_config": { "effort": "max" },
        });

        assert!(disable_thinking(&mut body));
        assert_eq!(body["thinking"], json!({ "type": "disabled" }));
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn disable_thinking_is_idempotent() {
        let mut body = json!({ "thinking": { "type": "disabled" } });
        assert!(!disable_thinking(&mut body));
        assert_eq!(body["thinking"], json!({ "type": "disabled" }));
    }

    #[test]
    fn disable_thinking_on_non_object_body_is_noop() {
        let mut body = json!("not an object");
        assert!(!disable_thinking(&mut body));
    }

    #[test]
    fn patched_body_resolves_to_no_reasoning_effort() {
        // 跨模块回归锁：钉死「必须删 output_config」的理由 ——
        // resolve_reasoning_effort 里 output_config.effort 优先级高于 thinking，
        // 只设 thinking:disabled 而不删 output_config，Chat/Responses 上游照样开思考。
        use crate::proxy::providers::transform::resolve_reasoning_effort;

        let mut body = json!({
            "output_config": { "effort": "high" },
            "thinking": { "type": "enabled", "budget_tokens": 32000 },
        });
        assert_eq!(resolve_reasoning_effort(&body), Some("high"));

        disable_thinking(&mut body);
        assert_eq!(resolve_reasoning_effort(&body), None);
    }
}
