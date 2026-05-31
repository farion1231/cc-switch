# Codex Chat Completions 适配器修复记录

## 背景

cc-switch 作为代理网关，将 Codex 的 Responses API 转换为上游 Chat Completions API。原始适配层存在多处协议转换缺口，导致 Codex agent 在多轮工具调用时行为不连贯：模型输出纯文本而非 tool_call，工具循环中断。

参考项目：icebear0828/codex-proxy、litellm#27276。

## 已修复问题

### 1. usage 始终为 0
- **根因**：转换后的请求缺少 `stream_options: {include_usage: true}`
- **修复**：当 `stream: true` 时自动注入 `stream_options`
- **文件**：`transform_codex_chat.rs`

### 2. tool_call call_id 为空
- **根因**：后续 SSE chunk 的空 `id` 字段覆盖了首个 chunk 中的真实 ID
- **修复**：仅在非空且未设置时接受 call_id
- **文件**：`streaming_codex_chat.rs`

### 3. assistant 消息被拆分（工具调用断节的主因）
- **根因**：Responses 的 `assistant content` 和 `assistant tool_calls` 被转成两条独立的 assistant 消息。Chat Completions 模型（qwen3.6-plus）学习了这种模式，后续也先输出文本再调用工具，导致工具循环断裂
- **修复**：合并为单条 `assistant content + tool_calls` 消息；提供 `flush_pending_tool_calls()` 和 `try_merge_content_into_last_tool_call_assistant()` 两个辅助函数
- **文件**：`transform_codex_chat.rs`

### 4. developer 角色未转换
- **根因**：Responses 的 `developer` 角色未映射为 Chat Completions 的 `system`
- **修复**：角色归一化时转换 `developer` → `system`
- **文件**：`transform_codex_chat.rs`

### 5. finish_reason 映射不完整
- **根因**：仅处理 `stop`，遗漏 `length`、`content_filter`、`refusal`
- **修复**：扩展映射表
- **文件**：`transform_codex_chat.rs`

### 6. Agent Loop Hint
- **内容**：在含 `instructions` 的首条 system 消息末尾追加提示，引导模型在同一 turn 中立即调用工具而非等待确认
- **文件**：`transform_codex_chat.rs`

### 7. Responses-only 工具过滤
- **内容**：`web_search_preview`、`file_search`、`computer_use_preview` 等 Responses 专用工具在转换时返回 `None`，不发送给 Chat Completions 上游
- **文件**：`transform_codex_chat.rs`

## 根因结论

工具调用断节的根本原因不是 SSE、usage 或 call_id，而是 **历史消息格式不符合 Chat Completions 模型学习到的调用模式**。拆分 assistant 消息导致模型模仿"先说后做"的行为。合并为单条 `content + tool_calls` 消息后，工具循环恢复连贯。

## 重要经验

1. **协议转换不是字段映射**：Responses 和 Chat Completions 在消息组织方式上有结构性差异（如 tool_call 与 content 的归属），仅做字段名对应不够
2. **streaming 状态机需要防御空值**：SSE 流中同一字段在不同 chunk 可能有/无值，必须防止空值覆盖已设置的真实值
3. **验证不能只看 response.completed**：该事件正常不代表工具循环正常。必须检查实际发出的 request body 中 messages 的结构
4. **调试日志是白盒化关键**：通过 `~/.cc-switch/logs/cc-switch.log` 中的 request/response JSON 定位问题，比纯客户端行为观察可靠得多
5. **测试覆盖转换逻辑**：18 个单元测试覆盖角色映射、消息合并、finish_reason、AGENT_LOOP_HINT 注入等核心路径

## 验证方法

### 单元测试
```bash
# 转换逻辑专项测试
cargo test --lib transform_codex_chat

# 全量测试
cargo test --lib
```

### 类型检查
```bash
pnpm typecheck
```

### 前端测试
```bash
pnpm test:unit
```

### 构建
```bash
pnpm tauri build
```

产物路径：
- `.app`: `src-tauri/target/release/bundle/macos/CC Switch.app`
- `.dmg`: `src-tauri/target/release/bundle/dmg/CC Switch_3.15.0_aarch64.dmg`

### 实测验证
1. 在 cc-switch 中配置 Codex 提供商，API 格式选 "Chat Completions"
2. 向 Codex 发送多步工具调用任务
3. 观察工具循环是否连贯（无"先说后做"断裂）
4. 检查 `~/.cc-switch/logs/cc-switch.log` 确认 request body 中 assistant 消息结构

## 涉及文件

| 文件 | 改动类型 |
|------|----------|
| `src-tauri/src/proxy/providers/transform_codex_chat.rs` | 核心修复 |
| `src-tauri/src/proxy/providers/streaming_codex_chat.rs` | streaming 修复 |
| `src/components/providers/forms/CodexFormFields.tsx` | UI 配置 |
| `src/i18n/locales/en.json` | 翻译 |
| `src/i18n/locales/ja.json` | 翻译 |
| `src/i18n/locales/zh.json` | 翻译 |
| `pnpm-workspace.yaml` | 构建依赖修复 |
