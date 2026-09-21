//! Claude Code 辅助流量（`x-claude-code-request-class: auxiliary`）的识别与降配
//!
//! Claude Code 的 Auto Mode 在执行 Bash 命令前会先发一条「安全分类器」请求，
//! 客户端对该请求有硬超时，超时即判定分类器不可用、连带拦下工具调用。
//! 若当前供应商默认开启 thinking，一次思考往返极易撞上这个上限。
//! 识别出这类请求后可以：
//!   1) 路由到专用的「分类器队列」（快 / 便宜的供应商）
//!   2) 强制关闭 thinking
//!
//! 识别只看一个请求头，不再猜提示词文案。代价见 [`ROUTED_REQUEST_CLASSES`]：
//! 这个头的粒度只到「辅助流量」，分类器和标题生成、记忆抽取等同桶，一起分流。
//!
//! 头不出现时 fail-open（请求原样透传，不报错），[`warn_missing_hint_header_once`]
//! 会打一条 `[CLS-006]` 提醒用户开 `CLAUDE_CODE_GATEWAY_HINT_HEADERS=1`。

use axum::http::HeaderMap;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

/// Claude Code 为 LLM 网关准备的请求分类头（2.1.273 起提供）
///
/// 仅当客户端设了 `CLAUDE_CODE_GATEWAY_HINT_HEADERS=1`（或登录方式被识别为
/// gateway）时才会发出 —— cc-switch 接管 Claude 配置时会写入该变量，
/// 见 `services::proxy::apply_claude_takeover_fields_with_policy_and_models`。
pub const REQUEST_CLASS_HEADER: &str = "x-claude-code-request-class";

/// 需要分流到分类器队列的 class 取值
///
/// 客户端 2.1.278 的映射只产出五个值：
/// `main` / `subagent` / `auxiliary` / `compaction` / `workflow`
/// （`querySource` 经 `Xs()`：`repl_main_thread*` 与 `sdk` → main，
/// `agent:*` 与 `hook_agent` → subagent，其余 → auxiliary；
/// `compact` → compaction；workflow 里的子代理 → workflow）。
///
/// Auto Mode 的权限判定 `querySource` 是 `auto_mode`，落在 `auxiliary`。
/// **这个桶里不止分类器**：`generate_session_title`、`extract_memories`、
/// `insights`、`narration`、`side_question`、`tool_use_summary_generation`、
/// `web_search_tool` 等约 25 种辅助请求同样是 auxiliary。头信号分不出它们，
/// 因此分流口径就是「全部辅助流量」——这是拿「文案改动即静默失效」换来的确定性。
const ROUTED_REQUEST_CLASSES: [&str; 1] = ["auxiliary"];

/// 分流请求的总时间预算（秒）
///
/// 分类器本身通常 1.5~2 秒返回，但同一个桶里还有 insights、web 搜索、摘要生成
/// 这类慢活儿，预算按最慢的那类给，不按分类器给：掐断一个本来能成功的辅助请求，
/// 比多等几十秒更糟。同时仍远短于代理默认的 600 秒 —— 否则我们永远比客户端晚
/// 放弃，故障转移到下一家时对方早已断开。
const CLASSIFIER_TOTAL_BUDGET_SECS: u32 = 60;

/// 分流请求最多尝试几家供应商
///
/// 队列再长也不额外消耗墙钟时间；多出来的成员仍作为「熔断跳过」的替补有效。
const CLASSIFIER_MAX_ATTEMPTS: u32 = 2;

/// 分流请求的单次尝试预算
///
/// 按**实际会尝试的家数**均分总预算，而不是给每家发一个固定的小值：
/// 队列里只有一家时，把整份预算全给它——否则单供应商场景下折半就掐断，
/// 会把「40 秒本可成功」变成硬失败，而客户端此时还远没有放弃。
///
/// 返回 `(单次超时秒数, max_retries)`。
pub fn attempt_budget(provider_count: usize) -> (u32, u32) {
    let attempts = (provider_count.max(1) as u32).min(CLASSIFIER_MAX_ATTEMPTS);
    let per_attempt = (CLASSIFIER_TOTAL_BUDGET_SECS / attempts).max(1);
    (per_attempt, attempts - 1)
}

/// 判断请求是否属于应当分流的辅助流量，命中则返回具体的 class 取值（供日志用）
///
/// 只认请求头，不看请求体。`x-claude-code-request-class` 是官方给网关的信号，
/// 不随提示词文案漂移；而任何体签名都得押注某一版的措辞，且覆盖不全
/// （fast stage1 既没有 stop_sequences，指令也不在末条 user 消息里）。
pub fn routed_request_class(headers: &HeaderMap) -> Option<&'static str> {
    let value = headers.get(REQUEST_CLASS_HEADER)?.to_str().ok()?.trim();
    ROUTED_REQUEST_CLASSES
        .iter()
        .find(|class| value.eq_ignore_ascii_case(class))
        .copied()
}

/// 请求是否带了 `x-claude-code-request-class`
///
/// 与 [`routed_request_class`] 的区别：这里只问「客户端到底发没发这个头」，
/// 用来区分「发了但不是辅助流量」和「压根没开网关提示头」两种情况。
fn has_request_class_header(headers: &HeaderMap) -> bool {
    headers.contains_key(REQUEST_CLASS_HEADER)
}

/// 整个进程只提醒一次：Claude 请求里完全没有网关提示头
static HINT_HEADER_WARNED: AtomicBool = AtomicBool::new(false);

/// 分类器队列开着、却一个 `x-claude-code-request-class` 都没见到时，提醒一次
///
/// 这是这套识别唯一的失效模式 —— 客户端没开 `CLAUDE_CODE_GATEWAY_HINT_HEADERS`，
/// 或跑的是 2.1.273 之前的版本。静默 fail-open 会让用户以为队列在工作，
/// 所以必须留一条自查线索；每进程一次，避免刷屏。
pub fn warn_missing_hint_header_once(headers: &HeaderMap, tag: &str) {
    if has_request_class_header(headers) {
        return;
    }
    if HINT_HEADER_WARNED.swap(true, Ordering::Relaxed) {
        return;
    }
    log::warn!(
        "[{tag}] [CLS-006] 请求未携带 {REQUEST_CLASS_HEADER}，分类器队列不会接管。\
         请确认 Claude Code 侧已设置 CLAUDE_CODE_GATEWAY_HINT_HEADERS=1（cc-switch \
         接管配置时会自动写入，手改过 settings.json 的需重新切换一次供应商），\
         且客户端版本 >= 2.1.273"
    );
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

/// 把分类器请求的出站模型名改写为队列条目指定的值，返回是否真的改动了请求体
///
/// 只写 `model` 字段，不碰任何别的东西：这里的目的仅仅是「这家供应商认得的模型名」，
/// 上下文窗口、思考开关等都由各自的机制负责。
pub fn override_model(body: &mut Value, model: &str) -> bool {
    let model = model.trim();
    if model.is_empty() {
        return false;
    }
    let Some(obj) = body.as_object_mut() else {
        return false;
    };
    if obj.get("model").and_then(Value::as_str) == Some(model) {
        return false;
    }
    obj.insert("model".to_string(), Value::String(model.to_string()));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn headers_with(class: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(REQUEST_CLASS_HEADER, class.parse().unwrap());
        headers
    }

    // ---- 请求头锚点 ----

    #[test]
    fn detects_auxiliary_class() {
        assert_eq!(
            routed_request_class(&headers_with("auxiliary")),
            Some("auxiliary")
        );
    }

    #[test]
    fn class_match_is_case_and_whitespace_insensitive() {
        for value in ["AUXILIARY", " auxiliary ", "Auxiliary"] {
            assert_eq!(
                routed_request_class(&headers_with(value)),
                Some("auxiliary"),
                "should match {value:?}"
            );
        }
    }

    #[test]
    fn main_conversation_is_never_routed() {
        // 这是整套分流最要命的误伤面：主对话被拖进分类器队列，
        // 等于用户的每一轮对话都被换成便宜模型 + 强制关思考。
        assert!(routed_request_class(&headers_with("main")).is_none());
    }

    #[test]
    fn other_known_classes_are_not_routed() {
        // 2.1.278 的映射只产出这五个值，除 auxiliary 外都走常规路由：
        // subagent / workflow 是真干活的子代理，compaction 是压缩上下文，
        // 都不该被 60 秒预算和模型覆写碰。
        for class in ["subagent", "compaction", "workflow"] {
            assert!(
                routed_request_class(&headers_with(class)).is_none(),
                "{class} should not be routed"
            );
        }
    }

    #[test]
    fn unknown_class_is_not_routed() {
        // 客户端将来新增取值时保持 fail-open：不认识就当普通请求，别乱分流
        assert!(routed_request_class(&headers_with("something_new")).is_none());
    }

    #[test]
    fn missing_header_is_not_routed() {
        assert!(routed_request_class(&HeaderMap::new()).is_none());
    }

    #[test]
    fn non_ascii_header_value_is_not_routed() {
        // 头值不是合法 UTF-8 时 to_str() 会失败，不能 panic
        let mut headers = HeaderMap::new();
        headers.insert(
            REQUEST_CLASS_HEADER,
            axum::http::HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap(),
        );
        assert!(routed_request_class(&headers).is_none());
    }

    #[test]
    fn header_presence_is_reported_independently_of_routing() {
        // 「发了头但不是 auxiliary」不该触发「没开提示头」的提醒
        assert!(has_request_class_header(&headers_with("main")));
        assert!(!has_request_class_header(&HeaderMap::new()));
    }

    // ---- 时间预算 ----

    #[test]
    fn single_provider_gets_the_whole_budget() {
        // 只有一家时不能掐成小超时：否则 40 秒本可成功的请求会被硬失败，
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

    // ---- 模型覆写 ----

    #[test]
    fn override_model_rewrites_and_reports_change() {
        let mut body = json!({ "model": "claude-sonnet-5", "messages": [] });
        assert!(override_model(&mut body, "glm-4-flash"));
        assert_eq!(body["model"], json!("glm-4-flash"));
    }

    #[test]
    fn override_model_trims_and_ignores_blank() {
        let mut body = json!({ "model": "claude-sonnet-5" });
        assert!(override_model(&mut body, "  glm-4-flash "));
        assert_eq!(body["model"], json!("glm-4-flash"));

        // 空白覆写等于没配，绝不能把 model 抹成空串发给上游
        let mut body = json!({ "model": "claude-sonnet-5" });
        assert!(!override_model(&mut body, "   "));
        assert_eq!(body["model"], json!("claude-sonnet-5"));
    }

    #[test]
    fn override_model_is_a_noop_when_already_equal() {
        let mut body = json!({ "model": "glm-4-flash" });
        assert!(!override_model(&mut body, "glm-4-flash"));
    }

    #[test]
    fn override_model_adds_the_field_when_absent() {
        let mut body = json!({ "messages": [] });
        assert!(override_model(&mut body, "glm-4-flash"));
        assert_eq!(body["model"], json!("glm-4-flash"));
    }

    #[test]
    fn override_model_leaves_the_rest_of_the_body_alone() {
        // 覆写只负责模型名；thinking / stop_sequences 由各自的机制处理
        let mut body = json!({
            "model": "claude-sonnet-5",
            "stop_sequences": ["</severity>"],
            "thinking": { "type": "enabled", "budget_tokens": 32000 },
        });
        override_model(&mut body, "glm-4-flash");
        assert_eq!(body["stop_sequences"], json!(["</severity>"]));
        assert_eq!(body["thinking"]["type"], json!("enabled"));
    }
}
