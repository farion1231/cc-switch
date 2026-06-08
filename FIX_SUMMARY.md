# CC Switch Bug Fix — 三轮对话卡死

## 问题描述

**症状**: CC Switch (v3.16.1) 在多轮工具调用时，前几轮正常，一旦涉及工具调用（读文件、执行命令），下一轮就永远卡住。用户反馈"三轮对话就卡了"。

**根因链**（两个独立 bug，共同导致卡死）:

### Bug 1: `content: null` 导致 MiMo 返回 400

```
Codex 发 assistant 消息 (content: null + tool_calls)
  → CC Switch 转成 Chat Completions 格式，保留 content: null
    → MiMo API 不接受 content: null，返回 HTTP 400
      → CC Switch 标记为 NonRetryable，直接失败
        → Codex 收到错误，可能重试同样的无效请求
```

### Bug 2: 连续 assistant 消息导致 MiMo 返回 400

```
Responses API 中，一个 message item (assistant, text) 和一个 function_call item
是两个独立的 output items。转换到 Chat Completions 时，产生了：

  {role: "assistant", content: "some text"}       ← from message item
  {role: "assistant", content: "", tool_calls: …} ← from function_call

MiMo 严格拒绝连续的 assistant 消息，返回 HTTP 400 → 同样的卡死链
```

## 修复内容

### 文件: `src-tauri/src/proxy/providers/transform_codex_chat.rs`

### Fix 1: `flush_pending_tool_calls` — content: null → content: ""

**修改**: 将 assistant 消息中的 `"content": null` 改为 `"content": ""`

### Fix 2: `responses_message_item_to_chat_message` — 兜底 null 处理

**修改**: 当 assistant 角色的 content 为 null 时，替换为空字符串

### Fix 3: `flush_pending_tool_calls` — 合并连续 assistant 消息

**关键修改**: 当 `flush_pending_tool_calls` 要创建新的 assistant+tool_calls 消息时，如果前一条消息已经是 assistant 消息（但没有 tool_calls），则将 tool_calls 合并到现有消息中，而非创建新消息。

```rust
// 修改前：总是创建新的 assistant 消息
let mut message = json!({
    "role": "assistant",
    "content": "",
    "tool_calls": tool_calls
});
messages.push(message);

// 修改后：如果前一条是 assistant 消息，合并 tool_calls 进去
if let Some(last) = messages.last_mut() {
    if last["role"] == "assistant" && last.get("tool_calls").is_none() {
        last["tool_calls"] = json!(tool_calls);
        return;
    }
}
// 否则创建新的
```

**为什么这是根本原因**：在 Responses API 中，`message` item（assistant 的文本回复）和 `function_call` item 是分开的 output items。但 Chat Completions API 要求它们是同一条 assistant 消息的一部分。不合并就会产生连续的 assistant 消息，MiMo 等严格提供商直接拒绝。

### Fix 4: `append_responses_item_as_chat_message` — 反向合并

**修改**: 当要添加 assistant message item 时，如果前一条已经是有 tool_calls 的 assistant 消息，将 content 合并进去而非创建新消息。这处理了另一种排序情况：function_call 先出现，然后 assistant 的文本消息跟随其后。

## 测试结果

```
running 53 tests
test responses_tool_call_produces_empty_string_content_not_null ... ok
test responses_multi_turn_tool_call_no_null_content ... ok
test responses_consecutive_assistant_messages_are_merged ... ok
test responses_three_round_codex_no_consecutive_assistant_no_null_content ... ok
test responses_assistant_text_then_function_call_merges ... ok

test result: ok. 53 passed; 0 failed
```

### 新增回归测试

1. **`responses_tool_call_produces_empty_string_content_not_null`**: 单轮工具调用，assistant 消息 content 必须是 "" 而非 null。

2. **`responses_multi_turn_tool_call_no_null_content`**: 多轮工具调用，所有 assistant 消息的 content 不能是 null。

3. **`responses_consecutive_assistant_messages_are_merged`**: 验证 assistant text + function_call 被合并为一条消息，不会产生连续的 assistant 消息。

4. **`responses_three_round_codex_no_consecutive_assistant_no_null_content`**: 模拟真实的三轮 Codex 对话场景，验证：
   - 无连续 assistant 消息
   - 无 content: null
   - tool 消息前有对应的 assistant+tool_calls
   - 消息序列正确：user → assistant → tool → assistant → tool → assistant → tool

5. **`responses_assistant_text_then_function_call_merges`**: 当 assistant 有文本但没有 tool_calls，紧跟着一个 function_call 时，tool_calls 合并到现有 assistant 消息中。

## 不需要修改的部分

- **`streaming_codex_chat.rs`**: 流式路径使用 Responses API 格式输出（`content: []`），不涉及 Chat Completions 格式转换。
- **`transform.rs`**: Anthropic ↔ OpenAI 转换路径（OpenRouter 专用），不受 MiMo API 约束。
- **`forwarder.rs`**: 错误分类和重试逻辑本身正确（400 确实应该是 NonRetryable）。
- **`codex_chat_history.rs`**: 历史恢复逻辑正确，不会产生重复或错误的消息。
- **`handlers.rs`**: 错误传播和响应处理正常。

## 提交信息建议

```
fix: merge consecutive assistant messages in Responses→Chat conversion

Two bugs caused multi-round tool calls to hang with MiMo/MiniMax providers:

1. `content: null` → `content: ""`: Strict Chat Completions providers reject
   null content on assistant messages with tool_calls, returning HTTP 400.

2. Consecutive assistant messages: In the Responses API, a `message` item
   (assistant with text) and a `function_call` item are separate output items.
   When converted to Chat Completions, they must be merged into a single
   assistant message with both `content` and `tool_calls`. Without merging,
   strict providers reject the request as having invalid consecutive
   assistant messages.

Fix: In `flush_pending_tool_calls`, merge tool_calls into the previous
assistant message if it has no tool_calls. In `append_responses_item_as_chat_message`,
merge content into the previous assistant+tool_calls message if one exists.

Fixes: multi-round tool call hang when using MiMo/MiniMax providers
```
