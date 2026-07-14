# GitHub Issue/PR 调研：每模型上下文窗口 + 自动压缩

> 调研日期：2026-07-14
> 仓库：[farion1231/cc-switch](https://github.com/farion1231/cc-switch)
> 分支：`feature/per-model-context-window`

## 概述

本调研用于支撑"每模型上下文窗口配置 + 自动压缩联动"功能的设计。从 cc-switch 官方仓库的 issue/PR 中筛选出与上下文窗口（context window）、1M 后缀、自动压缩（auto-compact）相关的条目，提炼根因与可借鉴的方案。

---

## Claude Code 侧

### #3679（已关闭）Claude 1M context marker fails: uppercase [1M] written + fallback model lacks toggle

**状态**：closed（bug）

**核心发现**：
1. cc-switch 写入的 marker 是大写 `[1M]`，但 Claude Code 官方只识别小写 `[1m]`
2. 默认兜底模型（ANTHROPIC_MODEL）输入框旁边没有"声明支持 1M"勾选框
3. 读取侧用了 `.toLowerCase()` 容错，掩盖了写入侧的 bug——写入 `[1M]` 后 `/context` 仍显示 200K

**根因**：
- `src/components/providers/forms/hooks/useModelState.ts:17`：`CLAUDE_ONE_M_MARKER = "[1M]"`
- Claude Code 官方文档明确要求小写：`Append [1m] to the model ID to enable the 1M context window`

**对设计的启示**：
- 后缀写入统一用小写：`[1m]`、`[200k]`、`[500k]`
- 解析侧保持大小写不敏感（已有 `.toLowerCase()` / `eq_ignore_ascii_case`）
- 泛化后缀时，新格式也要遵循小写写入约定

---

### #5124（已合并）feat(claude): add 1M checkbox to fallback model field

**状态**：merged（PR）

**内容**：给兜底模型（ANTHROPIC_MODEL）加了 1M checkbox，修复了 #3679 的 Bug 2。

**对设计的启示**：
- 这个 checkbox 就是我们要改造成窗口输入框的对象之一
- 改造后 checkbox 消失，替换为窗口大小输入框

---

### #5157（未合并）fix(provider): add [1m] suffix to DeepSeek ANTHROPIC_MODEL default for 1M context support

**状态**：closed（未合并）

**内容**：尝试给 DeepSeek preset 的 ANTHROPIC_MODEL 默认值加 `[1m]` 后缀。未合并，可能因为方案不够通用。

---

### #5177（open）Copilot provider 在CC下声明支持1M的模型 报 400

**状态**：open（enhancement）

**核心发现**：
- cc-switch 的 model mapping 把 `[1M]` 带进了映射后的模型 ID（如 `gpt-5.4[1M]`），Copilot 上游不认
- `settings.json` 里的 `_MODEL` 必须带 `[1M]` 才能让 Claude Code 用 1M context，不能简单去掉
- 用户的临时方案：手动从 DB 删除映射值里的 `[1M]` 后缀

**对设计的启示**：
- 后缀泛化后，代理剥离逻辑（`strip_one_m_suffix_for_upstream`）必须同步支持新格式 `[200k]` 等
- 不能让后缀泄漏到上游请求体的 `model` 字段

---

### #2117（open）stream_check ignores ANTHROPIC_BETAS and omits context-1m-2025-08-07

**状态**：open

**核心发现**：
- 健康检查（stream_check）请求不带 `context-1m-2025-08-07` beta header
- 设置 `env.ANTHROPIC_BETAS=context-1m-2025-08-07` 也被忽略
- 导致对要求显式 1M beta 的 provider 出现假阴性健康检查失败

**对设计的启示**：
- 如果检测到模型带 `[1m]` 后缀，stream_check 也应带 `context-1m-2025-08-07` beta header
- 本次设计暂不包含此修复，但列为后续增强项

---

## Codex 侧

### #4051（open）Codex App使用cc-switch本地路由转换接DeepSeek不会触发自动压缩Context

**状态**：open

**核心问题**：Codex 通过 cc-switch 代理转发 DeepSeek 时，不会自动触发上下文压缩，上下文只增不减直到崩溃。

**对设计的启示**：
- Codex 侧压缩不触发的根因是 catalog 缺 `auto_compact_token_limit` 字段 + `truncation_policy.limit` 硬编码
- 修复 catalog 生成即可解决

---

### #4832（open）codex_native_responses_template.json truncation_policy.limit 預設 10000 導致 Codex 上下文受限

**状态**：open（bug）

**核心发现**：
- `src-tauri/src/resources/codex_native_responses_template.json` 第 24-27 行硬编码：
  ```json
  { "truncation_policy": { "mode": "bytes", "limit": 10000 } }
  ```
- 无论用户在 UI 设什么上下文值，生成的 `cc-switch-model-catalog.json` 中 `truncation_policy.limit` 始终被重置为 10000
- 手动修改后被覆盖

**对设计的启示**：
- Codex 侧改造必须把 `truncation_policy.limit` 改成跟随 `context_window` 动态生成
- 或至少改成可配置选项

---

### #5110（open）Codex + OpenCode Go 2 上下文超限（HTTP 400）：truncation_policy.limit 与 context_window 不匹配，且代理不触发自动压缩

**状态**：open

**根因分析（非常完整）**：

| 问题 | 描述 |
|------|------|
| 问题1 | `context_window` 虚高（1M），Codex 以为有 1M 窗口放心累积上下文，但上游实际没那么大 → 400 |
| 问题2 | `truncation_policy.limit` 硬编码 10000（同 #4832） |
| 问题3 | 自动上下文压缩不触发（同 #4051） |
| 问题4 | 手动改 catalog 后被 cc-switch 重新生成覆盖 |

**期望行为**：
- `truncation_policy.limit` 应随用户设置的上下文长度同步更新
- `context_window` 应反映上游模型实际能力
- 自动压缩应在超限前触发
- 用户手动修改 catalog 后不应被静默覆盖

**对设计的启示**：
- catalog 生成要让 `truncation_policy.limit` 和 `context_window` 联动
- 补 `auto_compact_token_limit` 字段（当前完全缺失）
- 已有的 `preserves_user_model_catalog_json` 覆盖保护逻辑保持不动

---

### #4508（open）codex的模型只能支持400k上下文长度，但配置默认1M导致422

**状态**：open（bug）

**内容**：用户希望能自动更改上下文窗口，不用每次切换手动调整。与本次设计目标完全一致。

**PR 回复目标**：提交 PR 后回复此 issue

---

### #5199（open）自动填写获取的模型列表，尝试解析填写上下文/最大输出等属性

**状态**：open（enhancement）

**建议方案**：从 `/v1/models` 自动解析常见字段名：
- 上下文：`context_window` `context_length` `context_tokens` `max_context_tokens`
- 最大输出：`max_output_tokens` `max_completion_tokens`

**对设计的启示**：
- 可作为后续增强（自动填充窗口值），本次设计先不包含
- PR 回复时可提及"后续计划"

---

### #4709 / #5320（open）Codex /compact 失败

**状态**：open

**核心问题**：`remote compaction v2 expected exactly one compaction output item, got 0`

**对设计的启示**：
- 这是压缩响应解析的问题，和 catalog 配置是正交的
- 本次设计不直接解决，但修复 catalog 后可能间接缓解

---

## 通用压缩架构

### #4765（已关闭）功能请求：切换到小上下文模型时的通用上下文恢复 / 自动压缩机制

**状态**：closed

**提出方案**：
- 引入统一中间表示 `ConversationIR`
- 代理层 `context_budget` + `context_recovery` 管线
- 支持所有 CLI（Claude Code / Codex / Gemini / OpenCode 等）
- 高保真压缩：保留 system/developer 指令、最新 user turn、tool call 成对结构

**被关闭原因**：方案太重，维护者未采纳。

**对设计的启示**：
- 咱们的方案要轻量——靠正确配置注入窗口值 + 压缩阈值让工具自己压缩
- 不在代理层重新实现压缩逻辑
- #4765 的 `ConversationIR` 架构可作为远期参考，本次不采用

---

## PR 回复目标清单

提交 PR 后需要回复以下 issue：

| Issue # | 标题 | 关联点 |
|---------|------|--------|
| #4508 | codex模型只能支持400k但配置默认1M | Codex 上下文窗口可配 |
| #4051 | Codex不触发自动压缩 | Codex catalog 补 auto_compact_token_limit |
| #4832 | truncation_policy.limit 硬编码10000 | Codex truncation 联动修复 |
| #5110 | truncation与context_window不匹配 | Codex catalog 综合修复 |
| #5177 | Copilot声明1M报400 | 后缀泛化+代理剥离覆盖新格式 |
| #5199 | 自动填写模型列表上下文 | 后续增强计划（本次不含）|
| #3679 | [1M]大小写问题 | 已由 #5124 修复，本次泛化继承小写约定 |

---

## 关联 issue 交叉引用

- #3867：发送给 DeepSeek 的消息长度超过模型允许的最大上下文长度
- #4102：API Error 422 格式转换错误: input exceeds context window
- #4030：Codex 历史恢复问题（#4709 从此衍生）

这些 issue 描述的是同一类问题的不同表现，本次设计可一并解决或缓解。
