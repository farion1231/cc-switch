# Claude CLI 执行指令

## Goal

确认 DeepSeek Chat API 实际支持的 thinking 参数，验证当前 adapter 的 `reasoning_effort` 映射和 `thinking` 注入是否正确。

## Background

当前 `transform_codex_chat.rs` 在 DeepSeek Thinking 模式下做了两件事：
1. 映射 `reasoning/effort`：low/medium/high → `"high"`, xhigh/max → `"max"`
2. 注入 `thinking: { "type": "enabled" }`

但 `reasoning_effort` 是 OpenAI Chat Completions 的参数，DeepSeek 的 API 可能不识别它。同时 DeepSeek 可能支持 `thinking: { type: "enabled", budget_tokens: N }` 来控制思维深度。

## Plan

1. 搜索 DeepSeek Chat API 文档（platform.deepseek.com）中 `thinking`、`reasoning_effort`、`budget_tokens` 的参数定义
2. 确认 `reasoning_effort` 在 DeepSeek API 中是否生效，还是被忽略
3. 确认 DeepSeek 是否支持 `budget_tokens`，以及合理取值范围
4. 检查 Codex CLI 是否在 Responses API 的 `reasoning` 块中发送除 `effort` 外的其他字段（如 `max_tokens`）
5. 输出结论：当前代码是否需要调整

## Scope

- 只读分析
- 搜索 `src-tauri/src/proxy/providers/transform_codex_chat.rs` 中 thinking/reasoning 相关代码
- 搜索 DeepSeek 官方 API 文档

## Non-Goals

- 不改任何代码
- 不做代码修改

## Acceptance Criteria

- [ ] 确定 `reasoning_effort` 在 DeepSeek API 中是否有效
- [ ] 确定 DeepSeek 是否支持 `budget_tokens`
- [ ] 给出是否需要对当前代码做调整的建议

## Suggested Verification

```bash
cd cc-switch-src/src-tauri
# 搜索当前 thinking 相关代码
sed -n '118,145p' src/proxy/providers/transform_codex_chat.rs
```
