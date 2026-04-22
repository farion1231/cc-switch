//! Copilot 请求优化器
//!
//! 解决 GitHub Copilot 代理消耗量异常问题（Issue #1813）。
//!
//! Copilot 使用 `x-initiator` 请求头区分「用户发起」和「agent 续写」：
//! - `user`：计为一次 premium interaction（扣额度）
//! - `agent`：视为上一次交互的延续（不额外扣费）
//!
//! 参考实现: https://github.com/caozhiyuan/copilot-api

use std::collections::HashSet;

use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// 请求分类结果
#[derive(Debug, Clone)]
pub struct CopilotClassification {
    /// "user" 或 "agent" — 映射到 x-initiator 请求头
    pub initiator: &'static str,
    /// 是否为 warmup/探针请求（可降级到小模型）
    pub is_warmup: bool,
    /// 是否为上下文压缩请求
    pub is_compact: bool,
    /// 是否为 Claude Code 子代理请求（Agent tool 生成的 subagent）
    /// 子代理请求应设置 x-interaction-type=conversation-subagent，不计 premium interaction
    pub is_subagent: bool,
}

/// 分类 Anthropic 格式的请求体，决定 Copilot 请求头。
///
/// 分类算法（只检查最后一条消息，与参考实现 caozhiyuan/copilot-api 对齐）：
/// 1. 无消息 → "user"（安全默认，首次请求）
/// 2. 最后消息 role=user：
///    - content 中存在非 tool_result 类型 block → "user"
///    - content 全部是 tool_result → "agent"
///    - 匹配 compact 模式 → "agent"
/// 3. 最后消息 role 非 user → "user"（安全默认）
///
/// Warmup 检测（与参考实现对齐）：
/// - 请求头中有 `anthropic-beta` + 无 tools + 非 compact → warmup
///
/// `compact_detection`：是否启用 compact 检测。为 false 时跳过，
/// 确保 `CopilotOptimizerConfig.compact_detection` 开关真正生效。
///
/// `subagent_detection`：是否启用子代理检测。为 true 时，会扫描首条用户消息
/// 中的 `__SUBAGENT_MARKER__` 标记，将子代理请求标记为不计费。
pub fn classify_request(
    body: &Value,
    has_anthropic_beta: bool,
    compact_detection: bool,
    subagent_detection: bool,
) -> CopilotClassification {
    let is_compact = compact_detection && is_compact_request(body);
    let is_subagent = subagent_detection && detect_subagent(body);

    let messages = match body.get("messages").and_then(|m| m.as_array()) {
        Some(msgs) if !msgs.is_empty() => msgs,
        _ => {
            return CopilotClassification {
                initiator: "user",
                is_warmup: is_warmup_request(body, has_anthropic_beta, false),
                is_compact: false,
                is_subagent,
            }
        }
    };

    let last_msg = &messages[messages.len() - 1];
    let role = last_msg.get("role").and_then(|r| r.as_str()).unwrap_or("");

    // 只有 role=user 的消息需要细分
    if role != "user" {
        return CopilotClassification {
            initiator: if is_subagent { "agent" } else { "user" },
            is_warmup: false,
            is_compact,
            is_subagent,
        };
    }

    // 判定逻辑（与 copilot-api 的 merge-then-classify 效果对齐）：
    // 只要 content 数组中包含 tool_result → 视为工具续写 → agent
    // 这覆盖了 skill/edit hook/plan follow-up 等常见场景，
    // 它们的 content 通常是 [tool_result, text] 混合形态。
    // copilot-api 通过先 merge（text 吸收进 tool_result）再 classify 实现同等效果；
    // 直接在分类层处理更稳健，不依赖 merge 启用状态和执行顺序。
    let is_user_initiated = match last_msg.get("content") {
        Some(content) if content.is_array() => {
            let blocks = content.as_array().unwrap();
            // 含有 tool_result → 工具续写（agent），否则 → 用户发起（user）
            !blocks
                .iter()
                .any(|block| block.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
        }
        Some(content) if content.is_string() => true,
        _ => false,
    };

    // 子代理请求始终标记为 agent（即使首条消息包含用户文本）
    let initiator = if is_subagent || !is_user_initiated || is_compact {
        "agent"
    } else {
        "user"
    };

    CopilotClassification {
        initiator,
        is_warmup: initiator == "user" && is_warmup_request(body, has_anthropic_beta, is_compact),
        is_compact,
        is_subagent,
    }
}

/// 检测是否为 warmup/探针请求（适合降级到小模型）。
///
/// 与参考实现对齐，三个条件同时满足：
/// 1. 请求头有 `anthropic-beta`（Claude Code warmup 探针的标志）
/// 2. 无 tools 定义
/// 3. 非 compact 请求
fn is_warmup_request(body: &Value, has_anthropic_beta: bool, is_compact: bool) -> bool {
    if !has_anthropic_beta || is_compact {
        return false;
    }
    // 无工具定义
    !matches!(body.get("tools"), Some(tools) if tools.is_array() && !tools.as_array().unwrap().is_empty())
}

/// 检测是否为 Claude Code 上下文压缩/compact 请求。
///
/// 只匹配 Claude Code **内部生成**的机器特征，不匹配用户可能手动输入的通用短语，
/// 避免将真实用户请求误标为 agent。
///
/// 强特征来源：
/// 1. system prompt — Claude Code compact 模式会设置专用 system prompt，用户无法手动设置
/// 2. "CRITICAL: Respond with TEXT ONLY. Do NOT call any tools." — 机器指令
/// 3. 同时包含 "Pending Tasks:" 和 "Current Work:" — Claude Code compact 的结构标记
fn is_compact_request(body: &Value) -> bool {
    // 信号 1: system prompt 以 Claude Code compact 专用前缀开头
    // 用户在 Claude Code 中无法直接控制 system prompt，这是最可靠的信号
    let system_text = extract_system_text(body);
    if system_text
        .starts_with("You are a helpful AI assistant tasked with summarizing conversations")
    {
        return true;
    }

    // 信号 2 & 3: 检查最后一条用户消息中的机器生成特征
    let messages = match body.get("messages").and_then(|m| m.as_array()) {
        Some(msgs) => msgs,
        None => return false,
    };

    if let Some(last_msg) = messages.last() {
        if last_msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            return false;
        }

        let text = extract_text_from_message(last_msg);

        // 信号 2: Claude Code compact 的机器指令（大小写敏感，精确匹配）
        if text.contains("CRITICAL: Respond with TEXT ONLY. Do NOT call any tools.") {
            return true;
        }

        // 信号 3: Claude Code compact 的结构标记（两个同时出现才算）
        if text.contains("Pending Tasks:") && text.contains("Current Work:") {
            return true;
        }
    }

    false
}

/// 合并用户消息中的 tool_result 和 text block。
///
/// 与参考实现 `mergeToolResultForClaude` 对齐：
///
/// **消息内部合并**（核心）：在单条 user 消息内，将 text block 吸收进 tool_result block，
/// 使整条消息只剩 tool_result 类型 block。这样 Copilot 不会将其视为用户发起的交互。
///
/// 场景：Claude Code 在 skill 调用、edit hook、plan 提醒等场景下，会发送混合了
/// tool_result + text 的用户消息。text block 的存在让 Copilot 将其计为 premium request。
///
/// **跨消息合并**（补充）：连续的 tool_result-only 用户消息合并为一条。
pub fn merge_tool_results(mut body: Value) -> Value {
    let messages = match body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        Some(msgs) if !msgs.is_empty() => msgs,
        _ => return body,
    };

    // Phase 1: 消息内部合并 — 将 text block 吸收进 tool_result block
    for msg in messages.iter_mut() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        let content = match msg.get("content").and_then(|c| c.as_array()) {
            Some(blocks) => blocks,
            None => continue,
        };

        // 分离 tool_result 和 text block
        let mut tool_results: Vec<Value> = Vec::new();
        let mut text_blocks: Vec<Value> = Vec::new();
        let mut valid = true;

        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("tool_result") => tool_results.push(block.clone()),
                Some("text") => text_blocks.push(block.clone()),
                _ => {
                    // 存在其他类型 block → 跳过此消息
                    valid = false;
                    break;
                }
            }
        }

        // 必须同时有 tool_result 和 text 才需要合并
        if !valid || tool_results.is_empty() || text_blocks.is_empty() {
            continue;
        }

        // 合并策略（与参考实现对齐）
        let merged = merge_blocks_into_tool_results(tool_results, text_blocks);
        msg["content"] = Value::Array(merged);
    }

    // Phase 2: 跨消息合并 — 连续的 tool_result-only 用户消息合并
    let messages = body["messages"].as_array().unwrap().clone();
    if messages.len() <= 1 {
        return body;
    }

    let mut merged_msgs: Vec<Value> = Vec::with_capacity(messages.len());
    let mut i = 0;

    while i < messages.len() {
        if is_tool_result_only_message(&messages[i]) {
            let mut combined_content: Vec<Value> = Vec::new();
            while i < messages.len() && is_tool_result_only_message(&messages[i]) {
                if let Some(content) = messages[i].get("content").and_then(|c| c.as_array()) {
                    combined_content.extend(content.iter().cloned());
                }
                i += 1;
            }
            if !combined_content.is_empty() {
                merged_msgs.push(serde_json::json!({
                    "role": "user",
                    "content": combined_content
                }));
            }
        } else {
            merged_msgs.push(messages[i].clone());
            i += 1;
        }
    }

    body["messages"] = Value::Array(merged_msgs);
    body
}

/// 基于最后一条用户消息内容生成确定性 Request ID。
///
/// CC Switch 额外策略（参考项目 copilot-api 使用随机 UUID）：
/// - 哈希输入: sessionId + lastUserContent（排除 tool_result 和 cache_control）
/// - 相同内容产生相同 ID，可能帮助 Copilot 去重
/// - 找不到用户内容时退化为随机 UUID
/// - 使用 UUID v4 格式
pub fn deterministic_request_id(body: &Value, session_id: &str) -> String {
    let last_user_content = find_last_user_content(body);

    match last_user_content {
        Some(content) => {
            let mut hasher = Sha256::new();
            hasher.update(session_id.as_bytes());
            hasher.update(content.as_bytes());
            let result = hasher.finalize();

            let mut bytes = [0u8; 16];
            bytes.copy_from_slice(&result[..16]);
            // UUID v4 版本位和变体位（与参考实现一致）
            bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
            bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 1

            Uuid::from_bytes(bytes).to_string()
        }
        None => Uuid::new_v4().to_string(),
    }
}

/// 基于 session ID 生成稳定的 Interaction ID。
///
/// 与参考实现（copilot-api session.ts）对齐：
/// - 同一主对话的所有请求共享同一个 interaction ID
/// - 哈希输入: 仅 session ID（不包含消息内容，与 request ID 不同）
/// - Copilot 用此 ID 将请求聚合为同一个 "interaction"，影响 premium 计费归属
/// - 空 session ID 时返回 None（不应注入随机值，避免 interaction 碎片化）
pub fn deterministic_interaction_id(session_id: &str) -> Option<String> {
    if session_id.is_empty() {
        return None;
    }

    let mut hasher = Sha256::new();
    hasher.update(b"interaction:");
    hasher.update(session_id.as_bytes());
    let result = hasher.finalize();

    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&result[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 1

    Some(Uuid::from_bytes(bytes).to_string())
}

/// 检测请求是否来自 Claude Code 子代理（Agent tool 生成的 subagent）。
///
/// Claude Code 的 Agent tool 会在子代理首条用户消息的 `<system-reminder>` 标签中
/// 注入 `__SUBAGENT_MARKER__` JSON 标记，格式如：
/// ```json
/// {"__SUBAGENT_MARKER__": {"session_id": "...", "agent_id": "...", "agent_type": "..."}}
/// ```
///
/// 扫描策略（与 copilot-api 的 subagent-marker.ts 对齐）：
/// 1. 遍历所有 user 消息（不仅是第一条，因为 context 压缩可能重排消息）
/// 2. 在消息文本中查找 `__SUBAGENT_MARKER__` 关键字
/// 3. 找到即判定为子代理请求
fn detect_subagent(body: &Value) -> bool {
    // 信号 1: 显式 __SUBAGENT_MARKER__（Claude Code 2.x+ 自动注入）
    if extract_system_text(body).contains("__SUBAGENT_MARKER__") {
        return true;
    }

    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
                continue;
            }
            let text = extract_text_from_message(msg);
            if text.contains("__SUBAGENT_MARKER__") {
                return true;
            }
        }
    }

    // 信号 2（fallback）: metadata.user_id 包含子代理标识
    // Claude Code 的 Agent tool 会将 subagent session 标记为
    // "parentSessionId_agent_agentId" 格式，检测 "_agent_" 后缀
    if let Some(user_id) = body.pointer("/metadata/user_id").and_then(|v| v.as_str()) {
        // "_agent_" 是 Claude Code Agent tool 的内部标记
        if user_id.contains("_agent_") {
            return true;
        }
    }

    // 信号 3（fallback）: system prompt 包含 Claude Code 子代理的典型框架文本
    // Agent tool 生成的子代理会在 system prompt 中包含由 Agent tool 注入的任务描述，
    // 但主对话的 system prompt 由 Claude Code CLI 直接生成，两者格式不同
    // 这个信号不够可靠（用户 prompt 也可能包含这些词），因此只作为辅助判据
    // 暂不启用，预留接口

    false
}

/// 清理孤立的 tool_result — 没有对应 tool_use 的 tool_result 转为 text block。
///
/// 场景：上下文压缩、消息截断等可能导致 assistant 消息中的 tool_use 被删除，
/// 但后续 user 消息中的 tool_result 仍在。上游 API 可能因不匹配而报错/重试。
///
/// 与 copilot-api 的 `sanitizeOrphanToolResults` 对齐。
pub fn sanitize_orphan_tool_results(mut body: Value) -> Value {
    let messages = match body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        Some(msgs) if msgs.len() >= 2 => msgs,
        _ => return body,
    };

    // Anthropic 协议要求 tool_result 紧跟其对应 tool_use 所在的 assistant turn。
    // 只检查 messages[i-1]（紧邻上一条 assistant）来判定是否 orphan，
    // 与参考实现 sanitizeOrphanToolResults 对齐。
    for i in 1..messages.len() {
        if messages[i].get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }

        // 收集紧邻上一条 assistant 的 tool_use id
        let prev_tool_use_ids: HashSet<String> =
            if messages[i - 1].get("role").and_then(|r| r.as_str()) == Some("assistant") {
                messages[i - 1]
                    .get("content")
                    .and_then(|c| c.as_array())
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                            .filter_map(|b| b.get("id").and_then(|i| i.as_str()).map(String::from))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                // 上一条不是 assistant → 这条 user 中的所有 tool_result 都是 orphan
                HashSet::new()
            };

        let content = match messages[i]
            .get_mut("content")
            .and_then(|c| c.as_array_mut())
        {
            Some(blocks) => blocks,
            None => continue,
        };

        for block in content.iter_mut() {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            let tool_use_id = block
                .get("tool_use_id")
                .and_then(|id| id.as_str())
                .unwrap_or("");
            // 空 tool_use_id 或不在紧邻 assistant 的 tool_use 中 → orphan
            if tool_use_id.is_empty() || !prev_tool_use_ids.contains(tool_use_id) {
                let content_text = match block.get("content") {
                    Some(c) if c.is_string() => c.as_str().unwrap_or("").to_string(),
                    Some(c) if c.is_array() => c
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                };
                *block = serde_json::json!({
                    "type": "text",
                    "text": format!("[Tool result for {}]: {}", tool_use_id, content_text)
                });
            }
        }
    }

    body
}

/// 请求前主动剥离所有 assistant 消息里的 thinking / redacted_thinking block
///
/// Copilot 的三条目标端点（`/chat/completions`、`/v1/responses`、`/v1/chat/completions`）
/// 均为 OpenAI 兼容格式，不识别 Anthropic 的 thinking block。若原样转发，上游会
/// 拒绝并返回 invalid_request_error —— 届时 `thinking_rectifier` 才做反应式清理并
/// 重试。那次已经失败的请求依旧消耗一次 premium quota，所以此处提前剥离。
///
/// 与 `thinking_rectifier::rectify_anthropic_request` 的区别：
/// - 本函数只剥 thinking / redacted_thinking 两类 block，不触碰 signature，也不
///   移除顶层 thinking 字段——那些是错误路径上的激进整流，常规路径不需要。
/// - 保持与 `merge_tool_results` / `sanitize_orphan_tool_results` 一致的"消费 body、
///   返回新 body"签名，便于接入 forwarder 管道。
pub fn strip_thinking_blocks(mut body: Value) -> Value {
    let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return body;
    };

    for msg in messages.iter_mut() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) else {
            continue;
        };
        content.retain(|block| {
            !matches!(
                block.get("type").and_then(|t| t.as_str()),
                Some("thinking") | Some("redacted_thinking")
            )
        });
    }

    body
}

// ─── 内部辅助 ─────────────────────────────────

/// 从请求体的 `system` 字段提取文本（处理 string/array 两种格式）。
fn extract_system_text(body: &Value) -> String {
    match body.get("system") {
        Some(s) if s.is_string() => s.as_str().unwrap_or("").to_string(),
        Some(arr) if arr.is_array() => arr
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// 查找最后一条 user 消息的非 tool_result 文本内容。
///
/// 与参考实现的 `findLastUserContent` 对齐：
/// - 从后往前遍历消息
/// - 排除 tool_result block
/// - 排除 cache_control 字段
fn find_last_user_content(body: &Value) -> Option<String> {
    let messages = body.get("messages").and_then(|m| m.as_array())?;

    for msg in messages.iter().rev() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        let content = msg.get("content")?;

        if let Some(s) = content.as_str() {
            return Some(s.to_string());
        }

        if let Some(blocks) = content.as_array() {
            // 过滤 tool_result，保留其他 block（去掉 cache_control）
            let filtered: Vec<Value> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) != Some("tool_result"))
                .map(|b| {
                    let mut b = b.clone();
                    if let Some(obj) = b.as_object_mut() {
                        obj.remove("cache_control");
                    }
                    b
                })
                .collect();

            if !filtered.is_empty() {
                return Some(serde_json::to_string(&filtered).unwrap_or_default());
            }
        }
    }

    None
}

/// 将 text block 合并进 tool_result block。
///
/// 两种合并策略（与参考实现对齐）：
/// - 数量相等：一一对应，text 追加到对应 tool_result 的 content 中
/// - 数量不等：所有 text 追加到最后一个 tool_result 的 content 中
fn merge_blocks_into_tool_results(
    mut tool_results: Vec<Value>,
    text_blocks: Vec<Value>,
) -> Vec<Value> {
    if tool_results.len() == text_blocks.len() {
        // 一一对应合并
        for (tr, tb) in tool_results.iter_mut().zip(text_blocks.iter()) {
            append_text_to_tool_result(tr, tb);
        }
    } else {
        // 所有 text 追加到最后一个 tool_result
        if let Some(last_tr) = tool_results.last_mut() {
            for tb in &text_blocks {
                append_text_to_tool_result(last_tr, tb);
            }
        }
    }
    tool_results
}

/// 将 text block 的内容追加到 tool_result 的 content 中
fn append_text_to_tool_result(tool_result: &mut Value, text_block: &Value) {
    let text = text_block
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("");
    if text.trim().is_empty() {
        return;
    }

    // tool_result 的 content 可以是字符串或数组
    match tool_result.get("content") {
        Some(c) if c.is_string() => {
            let existing = c.as_str().unwrap_or("");
            tool_result["content"] = Value::String(format!("{existing}\n{text}"));
        }
        Some(c) if c.is_array() => {
            let arr = tool_result["content"].as_array_mut().unwrap();
            arr.push(serde_json::json!({"type": "text", "text": text}));
        }
        _ => {
            // content 缺失或 null — 直接设置
            tool_result["content"] = Value::String(text.to_string());
        }
    }
}

/// 从消息中提取文本内容
fn extract_text_from_message(msg: &Value) -> String {
    match msg.get("content") {
        Some(content) if content.is_string() => content.as_str().unwrap_or("").to_string(),
        Some(content) if content.is_array() => {
            let blocks = content.as_array().unwrap();
            blocks
                .iter()
                .filter_map(|block| {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        block.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
        _ => String::new(),
    }
}

/// 判断消息是否为 tool_result-only 的用户消息
fn is_tool_result_only_message(msg: &Value) -> bool {
    if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
        return false;
    }
    match msg.get("content").and_then(|c| c.as_array()) {
        Some(blocks) if !blocks.is_empty() => blocks
            .iter()
            .all(|block| block.get("type").and_then(|t| t.as_str()) == Some("tool_result")),
        _ => false,
    }
}

// ─── 测试 ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // === classify_request 测试 ===

    #[test]
    fn test_classify_user_text_message() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": "Hello, please help me write some code"}
            ]
        });
        let result = classify_request(&body, false, true, false);
        assert_eq!(result.initiator, "user");
        assert!(!result.is_compact);
    }

    #[test]
    fn test_classify_user_text_array_message() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Please explain this code"}
                ]}
            ]
        });
        let result = classify_request(&body, false, true, false);
        assert_eq!(result.initiator, "user");
    }

    #[test]
    fn test_classify_tool_result_only() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "tools": [{"name": "Read", "description": "Read a file", "input_schema": {}}],
            "messages": [
                {"role": "user", "content": "Read the file"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "I'll read that file."},
                    {"type": "tool_use", "id": "toolu_123", "name": "Read", "input": {"path": "/tmp/test.rs"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_123", "content": "file contents here"}
                ]}
            ]
        });
        let result = classify_request(&body, true, true, false);
        assert_eq!(result.initiator, "agent");
        assert!(!result.is_warmup);
    }

    #[test]
    fn test_classify_tool_result_with_text_block() {
        // tool_result + text block（skill/edit hook/plan follow-up 的常见形态）
        // 含有 tool_result → 视为工具续写 → agent
        // 与 copilot-api 的 merge-then-classify 效果对齐
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_123", "content": "file contents"},
                    {"type": "text", "text": "Now please refactor this code"}
                ]}
            ]
        });
        let result = classify_request(&body, false, true, false);
        assert_eq!(result.initiator, "agent");
    }

    #[test]
    fn test_classify_empty_messages() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": []
        });
        let result = classify_request(&body, false, true, false);
        assert_eq!(result.initiator, "user");
    }

    #[test]
    fn test_classify_no_messages() {
        let body = json!({"model": "claude-sonnet-4-20250514"});
        let result = classify_request(&body, false, true, false);
        assert_eq!(result.initiator, "user");
    }

    #[test]
    fn test_classify_compact_request_system_prompt() {
        // compact 通过 system prompt 强特征检测
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "You are a helpful AI assistant tasked with summarizing conversations. Please create a summary.",
            "messages": [
                {"role": "user", "content": "Here is the conversation history to summarize..."}
            ]
        });
        let result = classify_request(&body, false, true, false);
        assert_eq!(result.initiator, "agent");
        assert!(result.is_compact);
    }

    #[test]
    fn test_classify_compact_request_critical_marker() {
        // compact 通过 CRITICAL 机器指令检测
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "CRITICAL: Respond with TEXT ONLY. Do NOT call any tools. Summarize the conversation."}
                ]}
            ]
        });
        let result = classify_request(&body, false, true, false);
        assert_eq!(result.initiator, "agent");
        assert!(result.is_compact);
    }

    #[test]
    fn test_classify_compact_disabled_by_config() {
        // compact_detection=false 时，即使内容匹配也不标记为 compact
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "You are a helpful AI assistant tasked with summarizing conversations.",
            "messages": [
                {"role": "user", "content": "Summarize"}
            ]
        });
        let result = classify_request(&body, false, false, false); // compact_detection=false
        assert_eq!(result.initiator, "user"); // 不被标记为 agent
        assert!(!result.is_compact);
    }

    #[test]
    fn test_no_false_positive_on_user_summarize_request() {
        // P1 修复验证：用户手动输入 "summarize the conversation" 不应被误判为 compact
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": "Please summarize the conversation so far into a concise summary."}
            ]
        });
        let result = classify_request(&body, false, true, false);
        // 没有 system prompt 强特征，也没有 CRITICAL 指令 → 不是 compact → user
        assert_eq!(result.initiator, "user");
        assert!(!result.is_compact);
    }

    // === warmup 测试（与参考实现对齐） ===

    #[test]
    fn test_warmup_with_anthropic_beta_no_tools() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        // has_anthropic_beta=true, 无 tools → warmup
        let result = classify_request(&body, true, true, false);
        assert!(result.is_warmup);
    }

    #[test]
    fn test_not_warmup_without_anthropic_beta() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        // has_anthropic_beta=false → 不是 warmup
        let result = classify_request(&body, false, true, false);
        assert!(!result.is_warmup);
    }

    #[test]
    fn test_not_warmup_with_tools() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "tools": [{"name": "Read", "description": "Read a file", "input_schema": {}}],
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        // 有 tools → 不是 warmup（即使有 anthropic-beta）
        let result = classify_request(&body, true, true, false);
        assert!(!result.is_warmup);
    }

    #[test]
    fn test_not_warmup_when_agent() {
        // tool_result → agent → 不判定 warmup
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_123", "content": "ok"}
                ]}
            ]
        });
        let result = classify_request(&body, true, true, false);
        assert_eq!(result.initiator, "agent");
        assert!(!result.is_warmup);
    }

    // === merge_tool_results 测试 ===

    #[test]
    fn test_merge_intra_message_tool_result_text() {
        // 核心场景：消息内部 tool_result + text → text 被吸收进 tool_result
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "file contents"},
                    {"type": "text", "text": "skill output here"}
                ]}
            ]
        });
        let result = merge_tool_results(body);
        let content = result["messages"][0]["content"].as_array().unwrap();
        // 应只剩 1 个 tool_result block（text 被吸收）
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "tool_result");
        // tool_result 的 content 应包含原始内容 + 吸收的 text
        let tr_content = content[0]["content"].as_str().unwrap();
        assert!(tr_content.contains("file contents"));
        assert!(tr_content.contains("skill output here"));
    }

    #[test]
    fn test_merge_intra_message_equal_count() {
        // 数量相等：一一对应合并
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "result1"},
                    {"type": "text", "text": "text1"},
                    {"type": "tool_result", "tool_use_id": "t2", "content": "result2"},
                    {"type": "text", "text": "text2"}
                ]}
            ]
        });
        let result = merge_tool_results(body);
        let content = result["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert!(content[0]["content"].as_str().unwrap().contains("text1"));
        assert!(content[1]["content"].as_str().unwrap().contains("text2"));
    }

    #[test]
    fn test_merge_intra_message_empty_text_ignored() {
        // 空 text block 不追加内容
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "result"},
                    {"type": "text", "text": ""}
                ]}
            ]
        });
        let result = merge_tool_results(body);
        let content = result["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        // 空 text 不改变原始 content
        assert_eq!(content[0]["content"], "result");
    }

    #[test]
    fn test_merge_intra_skips_other_block_types() {
        // 有非 tool_result/text 的 block → 跳过整条消息
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "result"},
                    {"type": "image", "source": {"data": "..."}},
                    {"type": "text", "text": "caption"}
                ]}
            ]
        });
        let result = merge_tool_results(body);
        let content = result["messages"][0]["content"].as_array().unwrap();
        // 未合并，保持原样 3 个 block
        assert_eq!(content.len(), 3);
    }

    #[test]
    fn test_merge_cross_message_consecutive() {
        // 跨消息合并：连续 tool_result-only 用户消息
        let body = json!({
            "messages": [
                {"role": "user", "content": "Read files"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "Read", "input": {}},
                    {"type": "tool_use", "id": "t2", "name": "Read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "file1"}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t2", "content": "file2"}
                ]}
            ]
        });
        let result = merge_tool_results(body);
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        let merged_content = messages[2]["content"].as_array().unwrap();
        assert_eq!(merged_content.len(), 2);
    }

    #[test]
    fn test_merge_does_not_affect_normal_messages() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi!"},
                {"role": "user", "content": "How are you?"}
            ]
        });
        let result = merge_tool_results(body.clone());
        assert_eq!(result["messages"], body["messages"]);
    }

    // === deterministic_request_id 测试 ===

    #[test]
    fn test_deterministic_id_stable() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let id1 = deterministic_request_id(&body, "session1");
        let id2 = deterministic_request_id(&body, "session1");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_deterministic_id_varies_by_content() {
        let body1 = json!({
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body2 = json!({
            "messages": [{"role": "user", "content": "Goodbye"}]
        });
        let id1 = deterministic_request_id(&body1, "session1");
        let id2 = deterministic_request_id(&body2, "session1");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_deterministic_id_varies_by_session() {
        let body = json!({
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let id1 = deterministic_request_id(&body, "session1");
        let id2 = deterministic_request_id(&body, "session2");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_deterministic_id_ignores_tool_result() {
        // tool_result 内容不同，但 user text 相同 → 相同 ID
        let body1 = json!({
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi"},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "version_A"}
                ]},
                {"role": "user", "content": "do something"}
            ]
        });
        let body2 = json!({
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi"},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "version_B"}
                ]},
                {"role": "user", "content": "do something"}
            ]
        });
        let id1 = deterministic_request_id(&body1, "s");
        let id2 = deterministic_request_id(&body2, "s");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_deterministic_id_fallback_when_no_user_content() {
        // 无用户消息 → 退化为随机 UUID（每次不同）
        let body = json!({
            "messages": [
                {"role": "assistant", "content": "Hi"}
            ]
        });
        let id1 = deterministic_request_id(&body, "s");
        let id2 = deterministic_request_id(&body, "s");
        // 随机 UUID，每次应不同
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_deterministic_id_is_valid_uuid() {
        let body = json!({
            "messages": [{"role": "user", "content": "test"}]
        });
        let id = deterministic_request_id(&body, "session");
        assert!(Uuid::parse_str(&id).is_ok());
    }

    // === deterministic_interaction_id 测试 ===

    #[test]
    fn test_interaction_id_stable_for_same_session() {
        let id1 = deterministic_interaction_id("session_abc");
        let id2 = deterministic_interaction_id("session_abc");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_interaction_id_differs_across_sessions() {
        let id1 = deterministic_interaction_id("session_abc");
        let id2 = deterministic_interaction_id("session_def");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_interaction_id_differs_from_request_id() {
        let body = json!({
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let interaction = deterministic_interaction_id("session_abc").unwrap();
        let request = deterministic_request_id(&body, "session_abc");
        assert_ne!(interaction, request);
    }

    #[test]
    fn test_interaction_id_empty_session_is_none() {
        // 无 session 时不应生成 interaction ID（避免碎片化）
        assert!(deterministic_interaction_id("").is_none());
    }

    #[test]
    fn test_interaction_id_is_valid_uuid() {
        let id = deterministic_interaction_id("test_session").unwrap();
        assert!(Uuid::parse_str(&id).is_ok());
    }

    // === compact 检测增强测试 ===

    #[test]
    fn test_compact_detection_system_prompt() {
        let body = json!({
            "system": "You are a helpful AI assistant tasked with summarizing conversations. Please provide a concise summary.",
            "messages": [
                {"role": "user", "content": "Here is the conversation to summarize..."}
            ]
        });
        assert!(is_compact_request(&body));
    }

    #[test]
    fn test_compact_detection_critical_keyword() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "CRITICAL: Respond with TEXT ONLY. Do NOT call any tools. Summarize this conversation."}
            ]
        });
        assert!(is_compact_request(&body));
    }

    #[test]
    fn test_compact_detection_structural_markers() {
        // Claude Code compact 特有的结构标记
        let body = json!({
            "messages": [
                {"role": "user", "content": "Summary of conversation:\n\nPending Tasks:\n- Fix bug\n\nCurrent Work:\n- Implementing feature"}
            ]
        });
        assert!(is_compact_request(&body));
    }

    #[test]
    fn test_compact_no_false_positive_on_generic_summary() {
        // 通用短语不应触发 compact 检测
        let body = json!({
            "messages": [
                {"role": "user", "content": "Your task is to create a detailed summary of the conversation so far."}
            ]
        });
        assert!(!is_compact_request(&body));
    }

    #[test]
    fn test_compact_detection_negative() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "What is the weather today?"}
            ]
        });
        assert!(!is_compact_request(&body));
    }

    #[test]
    fn test_compact_detection_system_array() {
        let body = json!({
            "system": [
                {"type": "text", "text": "You are a helpful AI assistant tasked with summarizing conversations."}
            ],
            "messages": [
                {"role": "user", "content": "Summarize"}
            ]
        });
        assert!(is_compact_request(&body));
    }

    // === detect_subagent 测试 ===

    #[test]
    fn test_detect_subagent_with_marker_in_user_message() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "<system-reminder>\n{\"__SUBAGENT_MARKER__\":{\"session_id\":\"abc123\",\"agent_id\":\"explore-1\",\"agent_type\":\"Explore\"}}\n</system-reminder>\nPlease search the codebase for auth handlers"}
                ]}
            ]
        });
        assert!(detect_subagent(&body));
    }

    #[test]
    fn test_detect_subagent_with_marker_in_system() {
        let body = json!({
            "system": "You are an agent. {\"__SUBAGENT_MARKER__\":{\"session_id\":\"abc\",\"agent_id\":\"plan-1\",\"agent_type\":\"Plan\"}}",
            "messages": [
                {"role": "user", "content": "Design the implementation plan"}
            ]
        });
        assert!(detect_subagent(&body));
    }

    #[test]
    fn test_detect_subagent_no_marker() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "Hello, please help me write code"}
            ]
        });
        assert!(!detect_subagent(&body));
    }

    #[test]
    fn test_detect_subagent_via_metadata_user_id() {
        // fallback 信号: metadata.user_id 包含 "_agent_" 标记
        let body = json!({
            "metadata": {
                "user_id": "session_abc123_agent_explore-1"
            },
            "messages": [
                {"role": "user", "content": "Search for files"}
            ]
        });
        assert!(detect_subagent(&body));
    }

    #[test]
    fn test_detect_subagent_normal_user_id_not_matched() {
        // 普通 session ID 不应被误判
        let body = json!({
            "metadata": {
                "user_id": "session_abc123"
            },
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        assert!(!detect_subagent(&body));
    }

    #[test]
    fn test_classify_subagent_sets_agent_initiator() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "<system-reminder>\n{\"__SUBAGENT_MARKER__\":{\"session_id\":\"abc\",\"agent_id\":\"explore-1\",\"agent_type\":\"Explore\"}}\n</system-reminder>\nSearch for files"}
                ]}
            ]
        });
        let result = classify_request(&body, false, true, true);
        assert_eq!(result.initiator, "agent");
        assert!(result.is_subagent);
    }

    #[test]
    fn test_classify_subagent_disabled_flag() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "<system-reminder>\n{\"__SUBAGENT_MARKER__\":{\"session_id\":\"abc\",\"agent_id\":\"explore-1\",\"agent_type\":\"Explore\"}}\n</system-reminder>\nSearch for files"}
                ]}
            ]
        });
        // subagent_detection=false → 不检测子代理
        let result = classify_request(&body, false, true, false);
        assert_eq!(result.initiator, "user");
        assert!(!result.is_subagent);
    }

    // === sanitize_orphan_tool_results 测试 ===

    #[test]
    fn test_sanitize_orphan_tool_results_converts_orphans() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "Help me"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tool_1", "name": "read_file", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tool_1", "content": "file contents"},
                    {"type": "tool_result", "tool_use_id": "tool_orphan", "content": "orphan data"}
                ]}
            ]
        });
        let result = sanitize_orphan_tool_results(body);
        let msgs = result["messages"].as_array().unwrap();
        let last_content = msgs[2]["content"].as_array().unwrap();
        // tool_1 保留为 tool_result
        assert_eq!(last_content[0]["type"], "tool_result");
        // tool_orphan 转为 text
        assert_eq!(last_content[1]["type"], "text");
        assert!(last_content[1]["text"]
            .as_str()
            .unwrap()
            .contains("tool_orphan"));
    }

    #[test]
    fn test_sanitize_orphan_tool_results_no_orphans() {
        let body = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tool_1", "name": "read_file", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tool_1", "content": "ok"}
                ]}
            ]
        });
        let result = sanitize_orphan_tool_results(body.clone());
        // 无孤立 tool_result，不应有变化
        assert_eq!(result["messages"][1]["content"][0]["type"], "tool_result");
    }

    #[test]
    fn test_sanitize_orphan_non_adjacent_assistant_tool_use_is_orphan() {
        // tool_use 在更早的 assistant 中，但 tool_result 的紧邻上一条是另一个 assistant
        // → 对 Anthropic 协议来说这个 tool_result 是 orphan
        let body = json!({
            "messages": [
                {"role": "user", "content": "step 1"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "old_tool", "name": "search", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "old_tool", "content": "found it"}
                ]},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "OK, now let me think..."}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "old_tool", "content": "stale ref"}
                ]}
            ]
        });
        let result = sanitize_orphan_tool_results(body);
        let msgs = result["messages"].as_array().unwrap();
        // messages[2]: 紧邻 assistant 有 old_tool → 保留
        assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
        // messages[4]: 紧邻 assistant 无 tool_use → orphan → text
        assert_eq!(msgs[4]["content"][0]["type"], "text");
    }

    #[test]
    fn test_sanitize_orphan_prev_not_assistant() {
        // tool_result 紧邻上一条是 user（非 assistant）→ 全部 orphan
        let body = json!({
            "messages": [
                {"role": "user", "content": "first"},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "data"}
                ]}
            ]
        });
        let result = sanitize_orphan_tool_results(body);
        assert_eq!(result["messages"][1]["content"][0]["type"], "text");
    }

    /// 关键场景：orphan tool_result（上下文压缩丢失了紧邻 tool_use）
    /// 在分类时仍应被视为 agent continuation，不能因为后续的 sanitize
    /// 将其转为 text 而变成 user 请求。
    ///
    /// 这个测试验证 classify_request 在原始（未 sanitize）的 body 上
    /// 正确识别 orphan tool_result 为 agent。
    #[test]
    fn test_orphan_tool_result_classified_as_agent_before_sanitize() {
        // 场景：最后一条 user 消息全是 tool_result，但紧邻的 assistant
        // 消息里没有对应的 tool_use（因上下文压缩丢失了）
        let body = json!({
            "messages": [
                {"role": "assistant", "content": "I'll help you with that."},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "orphan_tool_1", "content": "file contents here"},
                    {"type": "tool_result", "tool_use_id": "orphan_tool_2", "content": "another result"}
                ]}
            ]
        });
        // 在原始 body 上分类 → 全是 tool_result → agent
        let classification = classify_request(&body, false, false, false);
        assert_eq!(classification.initiator, "agent");

        // sanitize 后 → tool_result 变为 text → 如果再分类就会变成 user
        let sanitized = sanitize_orphan_tool_results(body);
        let classification_after = classify_request(&sanitized, false, false, false);
        assert_eq!(
            classification_after.initiator, "user",
            "sanitize 后 orphan tool_result 变为 text，分类变成 user — \
             这就是为什么分类必须在 sanitize 之前执行"
        );
    }

    /// orphan tool_result + text 混合场景：
    /// 分类器直接识别含 tool_result 的消息为 agent（无论是否有 text block），
    /// 不依赖 merge 的执行顺序。即使 orphan tool_result 后续被 sanitize 转为 text，
    /// 分类结果在此之前已经确定为 agent。
    #[test]
    fn test_orphan_tool_result_with_text_classified_as_agent() {
        let body = json!({
            "messages": [
                {"role": "assistant", "content": "Processing..."},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "orphan_1", "content": "result data"},
                    {"type": "text", "text": "Here's the output from the tool"}
                ]}
            ]
        });
        // 含有 tool_result → agent（无论是否有 text block）
        let classification = classify_request(&body, false, false, false);
        assert_eq!(classification.initiator, "agent");

        // sanitize 后 orphan tool_result 变为 text → 纯 text → 分类会变成 user
        // 但正确的执行顺序是先分类再 sanitize，所以这不是问题
        let sanitized = sanitize_orphan_tool_results(body);
        let classification_after = classify_request(&sanitized, false, false, false);
        assert_eq!(classification_after.initiator, "user");
    }

    #[test]
    fn test_sanitize_orphan_empty_tool_use_id_is_orphan() {
        // tool_use_id 为空或缺失 → 无法匹配任何 tool_use → orphan
        let body = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tool_1", "name": "read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "", "content": "empty id"},
                    {"type": "tool_result", "content": "missing id field"}
                ]}
            ]
        });
        let result = sanitize_orphan_tool_results(body);
        let content = result["messages"][1]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "text");
    }

    // === strip_thinking_blocks 测试 ===

    #[test]
    fn test_strip_thinking_removes_assistant_thinking_blocks() {
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "let me ponder", "signature": "sig"},
                    {"type": "redacted_thinking", "data": "opaque"},
                    {"type": "text", "text": "hello"},
                    {"type": "tool_use", "id": "t1", "name": "read", "input": {}}
                ]}
            ]
        });
        let result = strip_thinking_blocks(body);
        let content = result["messages"][1]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "tool_use");
    }

    #[test]
    fn test_strip_thinking_leaves_user_messages_untouched() {
        // 仅处理 assistant，user 的 thinking 块（极少见，但可能）不动
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "thinking", "thinking": "x"},
                    {"type": "text", "text": "hi"}
                ]}
            ]
        });
        let result = strip_thinking_blocks(body);
        let content = result["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
    }

    #[test]
    fn test_strip_thinking_handles_missing_messages() {
        let body = serde_json::json!({ "model": "claude-3-5-sonnet" });
        let result = strip_thinking_blocks(body.clone());
        assert_eq!(result, body);
    }

    #[test]
    fn test_strip_thinking_leaves_empty_content_array() {
        // 仅含 thinking 的 assistant 消息剥完后 content 为空——保留上游自处理
        let body = serde_json::json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "solo"}
                ]}
            ]
        });
        let result = strip_thinking_blocks(body);
        let content = result["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 0);
    }

    #[test]
    fn test_strip_thinking_preserves_signature_on_non_thinking_blocks() {
        // signature 留给 thinking_rectifier 在错误路径处理，此处不动
        let body = serde_json::json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "x", "input": {}, "signature": "s"}
                ]}
            ]
        });
        let result = strip_thinking_blocks(body);
        let block = &result["messages"][0]["content"][0];
        assert_eq!(block["signature"], "s");
    }

    #[test]
    fn test_strip_thinking_multiple_assistant_turns() {
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "q1"}]},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "a"},
                    {"type": "text", "text": "r1"}
                ]},
                {"role": "user", "content": [{"type": "text", "text": "q2"}]},
                {"role": "assistant", "content": [
                    {"type": "redacted_thinking", "data": "x"},
                    {"type": "text", "text": "r2"}
                ]}
            ]
        });
        let result = strip_thinking_blocks(body);
        let a1 = result["messages"][1]["content"].as_array().unwrap();
        let a2 = result["messages"][3]["content"].as_array().unwrap();
        assert_eq!(a1.len(), 1);
        assert_eq!(a1[0]["text"], "r1");
        assert_eq!(a2.len(), 1);
        assert_eq!(a2[0]["text"], "r2");
    }

    #[test]
    fn test_strip_thinking_ignores_string_content() {
        // assistant.content 是字符串而非 block 数组 — 历史请求或极简客户端会这样
        // 不应崩溃，也不应转换结构
        let body = serde_json::json!({
            "messages": [
                {"role": "assistant", "content": "plain text response"}
            ]
        });
        let result = strip_thinking_blocks(body.clone());
        assert_eq!(result, body);
    }

    #[test]
    fn test_strip_thinking_preserves_block_order() {
        let body = serde_json::json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "pre"},
                    {"type": "text", "text": "A"},
                    {"type": "tool_use", "id": "t1", "name": "x", "input": {}},
                    {"type": "redacted_thinking", "data": "mid"},
                    {"type": "text", "text": "B"}
                ]}
            ]
        });
        let result = strip_thinking_blocks(body);
        let content = result["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 3);
        assert_eq!(content[0]["text"], "A");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[2]["text"], "B");
    }
}
