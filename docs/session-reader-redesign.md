# 会话详情阅读页重新设计

> 状态：设计稿（2026-10-03）；P0（契约与 fixture）已完成。分支 `feat/ui-redesign-v9`。
> 范围：`src-tauri/src/session_manager/**`、`src-tauri/src/commands/session_manager.rs`、`src/components/sessions/**`、`src/lib/api/sessions.ts`、`src/lib/query/queries.ts`、`src/types.ts`、`src/i18n/locales/*.json`。不涉及数据库（`~/.cc-switch/cc-switch.db`）。
>
> 文中「已核实」指在本机数据或上游源码里看到了原文；「待核实」指凭常识推断、实现前要再确认。

## 0. 结论

1. 根因在数据层：后端把每条消息压成一个 `content: String`，工具调用只剩 `[Tool: Bash]`、思考和图片被丢、结果分不出成败，前端只能靠字符串拼凑。展示层再怎么改也救不回来。
2. 方案：后端新增结构化 `blocks`（text / thinking / tool_call / tool_result / image / event / step），保留 `content` 作为纯文本投影；工具输出和图片只传「预览 + 引用」，展开时按需取；解析结果按 (mtime, len) 缓存；传输分块首屏先出。
3. 前端按「一问一答」分 turn：提问 → 执行过程（默认折叠成一行摘要，失败步骤常显）→ 最终回复（Markdown，视觉重点）。五家 Agent 共用一套版式骨架，只用一份「风格配置」区分符号、动作叫法、主题色和合并规则。
4. Markdown 渲染复用未合并 PR #6332 的 `SessionMarkdown.tsx`（lezer 解析、协议白名单、折叠边界、搜索兜底），迁到 v7 token，并补上链接拦截（系统浏览器打开）与图片引用。
5. 量化目标：55MB Claude 会话首屏 ≤ 300ms、全量 payload ≤ 1.5MB；177MB Codex 会话首屏 ≤ 600ms、payload ≤ 1MB（现在后者 160MB 工具输出会把 IPC 压死）。

---

## 1. 现状问题清单

### 1.1 数据层

| # | 问题 | 依据 |
|---|------|------|
| D1 | 所有内容压成文本：`extract_text_from_item` 把 `tool_use`/`toolCall` 变成 `[Tool: name]`，`input` 全丢（命令、路径、搜索词、diff） | `providers/utils.rs` 第 `extract_text_from_item` 分支 |
| D2 | `thinking` / `reasoning` 块被丢弃（`extract_text_from_item` 没有该分支，落到 `None`） | 同上；本机最近 120 个 Claude 会话 thinking 块 9270 个 |
| D3 | `image` 块被丢弃；Claude 把 base64 直接内联在 jsonl（当前会话 2013 行里图片占 930KB） | 本机统计 `('image','base64','image/webp')` 51 次、`tool_result` 子项 `image` 524 次 |
| D4 | `tool_result.is_error` 被丢，结果成败无法区分；顶层 `toolUseResult`（stdout/stderr/interrupted/structuredPatch/…）完全没用 | `claude.rs::load_messages` 只读 `message.content` |
| D5 | Codex 解析器只认 `function_call`/`function_call_output`，而本机 Codex 工具调用 1273 次里 `custom_tool_call`(name=exec/apply_patch) 占绝大多数 → 177MB 会话只解析出 79 条、工具输出 0KB | `codex.rs::load_messages` 的 `match payload_type`；本机统计 `('ri','custom_tool_call','exec') 823`、`function_call 220+62` |
| D6 | Codex `reasoning`（summary_text）、`web_search_call`、`compaction`、`event_msg.turn_aborted/task_complete`、`item_completed`（含 CommandExecution 的 exit_code/duration、FileChange 的 unified_diff、McpToolCall、WebSearch、ImageView）全部未解析 | 本机 `item_completed` 项统计：CommandExecution 1079、Reasoning 875、AgentMessage 415、McpToolCall 220、WebSearch 42、FileChange 31、ImageView 21、ContextCompaction 10 |
| D7 | Codex `function_call.namespace`（MCP 服务器名，如 `mcp__cua_repl` / `clock` / `collaboration`）被丢 | 样本 `('ri','function_call','js')` 带 `namespace` |
| D8 | OpenCode `part.type=tool` 的 `state.{status,input,output,time}`、`step-start/step-finish` 的 tokens/cost、`reasoning` 全丢；文件存储版和 SQLite v1/v2 三套路径各自一份 `extract_part_text` | `opencode.rs::extract_part_text` 只认 text/tool |
| D9 | Pi `thinking`、`toolCall.arguments`、`toolResult.isError`、`model_change`、`thinking_level_change` 丢失 | `pi.rs::parse_message` 走 `extract_text` |
| D10 | Gemini `thoughts[]`（subject/description）、`tokens`、`model`、`error`/`info` 消息被跳过或丢弃 | `gemini.rs::load_messages` 的 `Some("info") \| Some("error") => continue` |
| D11 | 工具调用与结果没有配对信息（`tool_use_id`/`call_id`/`callID`/`toolCallId` 均未输出） | 各解析器 |
| D12 | 注入型内容（Claude `<system-reminder>`/`<local-command-caveat>`、Codex `AGENTS.md instructions` / `<environment_context>` / IDE context / `developer` 角色）与真人提问混在 `user` 里，前端靠字符串前缀兜底 | `utils.ts::shouldHideCodexMessageFromToc` |

### 1.2 展示层

| # | 问题 | 依据 |
|---|------|------|
| V1 | 提问、工具过程、最终回复同一种排版（同字号、同背景逻辑），最终回复淹没在过程里 | `SessionMessageItem.tsx`：仅 user 有 `bg-subtle`，assistant 无区分 |
| V2 | 工具调用是灰 chip「Bash ×5」+「工具输出 · 13.2k 字」，连续多次输出 `\n` 拼成一坨 | `utils.ts::buildSessionDisplayItems` 的 `addTool` 把 output 直接拼接 |
| V3 | 正文只处理围栏代码块和行内代码，不渲染 Markdown（标题/列表/表格/链接/图片全是原文） | `splitMarkdownBlocks` / `InlineText` |
| V4 | 链接不可点，图片不可见 | 同上；PR #6332 已有方案但未合并 |
| V5 | 页头标题区是被截断的长路径（无 title 时回退 `getBaseName(projectDir)`，再回退 sessionId 前 8 位） | `formatSessionTitle` + `AppPageHeader truncateTitle` |
| V6 | 没有「一轮」概念：目录只列提问，无法看出每轮做了什么、是否成功 | `SessionToc.tsx` |
| V7 | 五家 Agent 一个样，用户无法从界面感知当前读的是哪家的会话（只有页头 16px 图标） | `SessionReader.tsx` |

### 1.3 性能

| # | 问题 | 依据 |
|---|------|------|
| P1 | 工具输出全量走 IPC：55MB Claude 会话 → 2.4MB payload，其中工具输出 1.8MB、单条最大 44KB（本会话最大 93KB） | 任务给定实测；本机统计 `max_tool_result 93526` |
| P2 | Codex 177MB 会话里 84 条 `custom_tool_call_output` 合计 160MB、单行最大 9MB；一旦 D5 修好、若仍全量传输，IPC 会直接卡死 | 本机 profile：`84 160166 KB ('response_item','custom_tool_call_output')`，`maxline 9020957` |
| P3 | 图片 base64 若随消息下发，当前会话就要多传 930KB | 1.1 D3 |
| P4 | 阅读页没有解析缓存：每次打开/刷新/切换回来都重新解析整个文件（列表页有 `FileParseCache`，阅读页没有） | `commands/session_manager.rs::get_session_messages` 直接 `load_messages` |
| P5 | 一次性返回整个数组，首屏要等完整 JSON 序列化 + 反序列化；前端用 `useDeferredValue` 只是不卡交互，不是更早出内容 | `SessionReader.tsx` `contentReady` 逻辑 |
| P6 | 大文件单行 JSON（9MB）用 `BufReader::lines()` + `serde_json::from_str::<Value>` 会分配整个 `Value` 树；可接受但要测 | `codex.rs::load_messages` |

---

## 2. 各 Agent 原始数据结构 × 原生 UI 对照

### 2.1 Claude Code（`~/.claude/projects/<proj>/<sessionId>.jsonl`）

**数据结构（已核实，本机样本）**

- 顶层 `type`：`user` / `assistant`（带 `message`）、`attachment`（环境快照等，`isMeta` 风格）、`system`（`subtype: stop_hook_summary` 等，含 `hookErrors[]`）、`custom-title`、`agent-name`、`mode`、`atis-latch`、`last-prompt`、`queue-operation`、`file-history-snapshot/delta`、`pr-link`（`prUrl`）、`cost-state`、`worktree-state`、`relocated`。
- `message.content[]` 块：`text{text}`、`thinking{thinking, signature}`（**本机大量 `thinking:""` 只有 signature**，即已加密/不可见）、`tool_use{id, name, input, caller?}`、`tool_result{tool_use_id, is_error?, content: string | [{type:text}|{type:image,source{base64,media_type,data}}]}`、`image{source{type:base64, media_type, data}}`（用户贴图）。
- 结果记录顶层 `toolUseResult`：Bash → `{stdout, stderr, interrupted, isImage, noOutputExpected, returnCodeInterpretation?, backgroundTaskId?, gitOperation?, bashEditDiff?, persistedOutputPath?, staleReadFileStateHint?}`；Edit → `{filePath, oldString, newString, replaceAll, structuredPatch, originalFile, userModified}`；Write → `{type: create|update, filePath, content, structuredPatch}`；Read → `{type:text, file{filePath, content, numLines?}}`；Agent → `{isAsync, status, agentId, description, resolvedModel, prompt}`；AskUserQuestion → `{questions, answers}`；ToolSearch → `{matches, query}`。
- 助手记录：`message.model`、`message.usage`、`thinkingDurationMs`、`attributionMcpServer/Tool`、`requestId`。
- 用户记录：`isMeta`（跳过）、`isSidechain`（子代理，跳过）、`imagePasteIds`、`sourceToolAssistantUUID`（结果对应的助手记录）。
- 附属目录 `<sessionId>/tool-results/*.txt|*.json|*.jpg`（超长输出和 MCP 图片落盘），`<sessionId>/subagents/agent-*.jsonl`。
- 工具名分布（120 会话）：Bash 11511、Write 500、Read 327、`mcp__<server>__<tool>` 数百、Agent 154、WebFetch 91、Edit 85、WebSearch 72、AskUserQuestion 26、Skill、Workflow、ToolSearch、SendMessage…。

**原生 UI（Claude Code CLI）**

- 助手文本前缀 `⏺`；工具调用一行 `⏺ Bash(git status)` / `⏺ Read(src/a.ts)` / `⏺ Update(path)`，结果用 `⎿` 引出，默认只给摘要（如 `Read 120 lines`、`+3 lines (ctrl+o to expand)`），失败结果 `⎿ Error: …` 红色（符号与格式为常识，**待核实**最新版本是否仍如此）。
- 思考：`✻ Thinking…` 斜体灰字，默认折叠（**待核实**）。
- 已核实（官方文档 interactive-mode）：`Ctrl+O` 打开 transcript viewer「显示详细工具用法和执行，带时间戳与模型」，并「展开默认折叠的行，例如 MCP 调用折叠成一行 `Called slack 3 times`」。→ 合并规则：连续同一 MCP 服务器调用合并计数。

### 2.2 Codex（`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`、`archived_sessions`）

**数据结构（已核实）**

- 顶层 `type`：`session_meta`、`turn_context`（turn_id、cwd、model）、`world_state`、`token_usage_record`、`compacted{message}`、`inter_agent_communication_metadata`、`response_item{payload}`、`event_msg{payload}`。
- `response_item.payload.type`：
  - `message{role: user|assistant|developer, content[{type: input_text|output_text|input_image{image_url: data:…}}]}`。`developer` 是系统注入；`user` 里也有 `# AGENTS.md instructions for …`、`<environment_context>`、IDE context 等注入文本。
  - `reasoning{summary[{type:summary_text,text}], content?[{type:reasoning_text,text}], encrypted_content}`：多数只有 `encrypted_content`（不可见）。
  - `function_call{name, namespace?, arguments: JSON 字串, call_id}` / `function_call_output{call_id, output: string}`。本机 name：`exec_command`、`write_stdin`、`js`(namespace `mcp__cua_repl`)、`sleep`(namespace `clock`)、`spawn_agent`/`send_message`(namespace `collaboration`)、`request_user_input_async`、`write_file`、`read_file`、`new_context`。
  - `custom_tool_call{name: exec|apply_patch, input: string, call_id, status}` / `custom_tool_call_output{call_id, output: string | [{type:input_text,text}]}`。`exec` 的 `input` 是一段 JS（`const r = await tools.exec_command({cmd:"…", workdir:"…"}); text(r.output);`），输出以 `Script completed\nWall time 0.1 seconds\nOutput:\n` 开头。`apply_patch` 的 `input` 是 `*** Begin Patch / *** Update File: … / *** Add File: …` 文本。
  - `web_search_call{status, action{type: search|open_page…, query, queries[]}}`、`compaction{encrypted_content}`、`agent_message{author, recipient, content}`。
- `event_msg.payload.type`：`task_started{turn_id}`、`task_complete{turn_id, last_agent_message}`、`turn_aborted{reason: interrupted, duration_ms}`、`token_count`、`thread_settings_applied{model}`、**`item_completed{item}`**：
  - `CommandExecution{command[], cwd, parsed_cmd[{type: read|search|list|unknown, cmd, name?}], status, exit_code, duration{secs,nanos}, aggregated_output, formatted_output, source}` ← 这是 Codex 自己渲染「Ran / Explored」用的结构化数据。
  - `FileChange{changes{path:{type: add|update|delete, unified_diff|content, move_path}}, status, stdout, stderr}`、`McpToolCall{server, tool, arguments, result{content[], isError}, duration, status}`、`WebSearch{query, action}`、`Reasoning{summary_text[], raw_content[]}`、`AgentMessage{content[{type:Text,text}], phase: commentary|final_answer, questions?}`、`UserMessage`、`ImageView{path: file://…}`、`ContextCompaction`、`Extension{kind: web.search|clock.sleep, …}`、`SubAgentActivity`。

**原生 UI（codex-rs/tui，已核实源码字符串）**

- 用户消息前缀 `› `（dim bold）；助手消息前缀 `• `；reasoning 摘要 `• ` + dim italic。
- 命令：`• Ran <cmd>` / `Running`；探索合并 `• Explored` → 子行 `Read a.rs, b.rs` / `Search foo in src` / `List dir`（`parsed_cmd.type` 为 read/search/list 且无错误时合并）；输出子行前缀 `  └ `，续行 `    `；`TOOL_CALL_MAX_LINES = 5`（头尾各取，中间 `… +N lines`）；失败 `(exit N)` 红色（搜索类 exit 1 用 dim）。
- 文件改动：`• Edited path (+12 -3)`（`+` 绿 `-` 红）、`Added`、`Deleted`、多文件 `Edited 2 files (+5 -3)` + `└ ` 每文件；hunk 间 `⋮`；失败 `✘ Failed to apply patch`。
- MCP：`• Called server.tool(args)` / `Calling`；错误 `Error: …`。
- 网页：`• Searched the web` / `Searched for …` / `Opened page …`，多 query 用 `, ` 连接。
- 图片：`Viewed image`、`Generated Image:`。

### 2.3 Gemini CLI（`~/.gemini/tmp/<hash>/chats/session-*.json`）

**数据结构（已核实，本机样本少）**

- 顶层 `{sessionId, projectHash, startTime, lastUpdated, kind, messages[]}`。
- `messages[].type`：`user{content: string|[{text}]}`、`gemini{content, thoughts[{subject, description, timestamp}], tokens{input,output,cached,thoughts,tool,total}, model}`、`error{content}`、`info{content}`。
- `toolCalls[]`：现有解析器读 `name`；Gemini CLI 的记录形如 `{id, name, args, status, result?, resultDisplay?, timestamp}`（**待核实**：本机 30 个文件没有一条 toolCalls）。

**原生 UI（gemini-cli，已核实源码）**

- 助手前缀 `✦ `（accent 色）；用户前缀 `> `（accent），用户消息有背景色块。
- 工具消息：左侧圆角边框（`borderStyle="round"`，只画左边），状态符 `✓` 成功、`o` 待执行、`⊷` 执行中、`?` 待确认、`-` 取消、`x` 错误，`STATUS_INDICATOR_WIDTH = 3`；`<Text bold>{name}</Text>` + 次要色 `{description}`（如 `✓ Shell  ls -la`）；子代理输出 `SUBAGENT_MAX_LINES` 截断。
- 思考：面板里以 subject 加粗 + description 显示（**待核实**具体样式）。

### 2.4 OpenCode（`~/.local/share/opencode/opencode.db`；旧版 `storage/message|part/**.json`）

**数据结构（已核实）**

- `message.data`：`{id, parentID, role, agent/mode, modelID, providerID, cost, tokens{total,input,output,reasoning,cache{read,write}}, time{created,completed}, finish, path{cwd,root}}`。
- `part.data.type`：`text{text, time}`、`reasoning{text, time}`、`step-start{}`、`step-finish{reason, tokens, cost}`、`tool{callID, tool, state{status: completed|error|running|pending, input, output, title, metadata{truncated…}, time{start,end}, error?}}`、`agent{name, source}`、`patch`、`file`（附件，**待核实**）。本机 tool：read 48、bash 28、grep 23、task 16、background_output 12、glob 8、webfetch 3、todowrite 3、invalid 3、background_cancel 2、ast_grep_search 2、question 1。
- V2 `session_message{type: user|assistant|system, data{content[{type: reasoning|tool|text}]}, seq}`。

**原生 UI（opencode TUI `routes/session/index.tsx`，已核实摘要）**

- bash `$ <command>`（输出最多 10 行）；read `→ Read path`；write `← Wrote path`；edit `← Edit path`（diff）；grep/glob `✱ Grep "pattern" in path`（给匹配数）；webfetch `% WebFetch url`；websearch `◈`；task `│ / ✓ Agent Task — description`；execute 子调用 `↳ tool`；apply_patch `# Created/Deleted/Moved/← Patched path`；todowrite `⚙ # Todos`；question `→ Asked N question(s)`；通用工具 `⚙ tool [args]`（输出 3 行）；错误 `✗`，待定 `~`；思考 `Thought`（可折叠，带时长）。step-finish 的 tokens/cost 在 TUI 底栏累计，不在每步显示（**待核实**）。

### 2.5 Pi（`~/.pi/agent/sessions/<cwd-encoded>/<ts>_<id>.jsonl`，树形 `id/parentId`）

**数据结构（已核实）**

- 头 `{type: session, version: 3, id, cwd, timestamp}`；`model_change{provider, modelId}`、`thinking_level_change{thinkingLevel}`、`message{message{role, content, timestamp, api, provider, model, usage{…cost}}}`、`compaction`/`branch_summary{summary}`、`custom_message`。
- `role`：`user`（content[{text}]）、`assistant`（`thinking{thinking, thinkingSignature}`、`text{text}`、`toolCall{id, name: bash|read|write|edit, arguments}`）、`toolResult{toolCallId, toolName, content[{text}|{image}], isError?, details?}`、`bashExecution{command, output}`（用户 `!cmd`）、`system`（`sections{preamble, tools, rules}`）。

**原生 UI（pi-mono coding-agent，已核实源码）**

- 工具标题 `theme.bold(toolName)` + 路径，如 **read** `src/a.ts:10-40`；bash 标题 `$ cmd`（`BASH_PREVIEW_LINES = 5`，`"Took 1.2s"`，`(timeout 30s)`）；read 折叠预览 10 行 `... (N more lines, to expand)`；edit 用 diff；整块背景按状态着色（`toolPendingBg` / `toolSuccessBg` / `toolErrorBg`）；思考 `Thinking...` 斜体，点开显示全文；中断 `Operation aborted`，截断 `Response was truncated before completion.`。

### 2.6 其余四家：现状与降级

| Agent | 数据 | 现状 | 降级方案 |
|---|---|---|---|
| Hermes | SQLite `messages(role, content, tool_calls JSON(OpenAI shape), tool_call_id, tool_name, …)` + JSONL | 文本 + `[Tool: name]` | `tool_calls[].function{name,arguments}` → ToolCall；`role=tool` 行按 `tool_call_id` 配对 → ToolResult；没有 is_error 字段 → `status: unknown` |
| OpenClaw | JSONL，Pi 同构（`toolResult` 角色） | 文本 | 复用 Pi 的 block 映射（无树形过滤） |
| Grok Build | `chat_history.jsonl` `type: system|user|assistant|tool`，reasoning 记录加密 | 文本 | `tool` 记录 → 无配对的 ToolResult（call_id 缺失 → 显示为「工具输出」通用步骤）；assistant 里若有 tool_calls（**待核实**）再配对 |
| MiniMax Code | SQLite 只暴露 user/assistant 展示行 | 文本 | 只有对话，无执行过程；界面隐藏「执行过程」区 |

四家都用「通用」风格配置（中性色、`•` 符号、通用动词）。本机没有 Hermes/OpenClaw/Grok/MiniMax 会话样本，映射只能按现有解析器与单元测试里的 fixture 写，标注 **待核实**。

---

## 3. 统一数据模型

### 3.1 Rust（`src-tauri/src/session_manager/model.rs`，新文件）

> P0 已落地，以 `model.rs` 为准；下面的代码块省略了 derive/serde 的重复部分，P0 相对初稿的调整见 §3.5。

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    /// user | assistant | tool | system（保持旧值，前端已有 roleLabel）
    pub role: String,
    /// 纯文本投影（规则见 3.4），旧搜索/复制/TOC 继续用它
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    /// 源记录 id（Claude uuid / Codex payload.id / OpenCode message id / Pi entry id）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 同一轮（turn）的标识：Codex turn_id、Claude 由解析器按 user 消息递增生成
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// 注入型内容（AGENTS.md、environment_context、system-reminder、developer 角色…）
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub injected: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<SessionBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<MessageMeta>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMeta {
    // 每个 Option 字段都带 #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum SessionBlock {
    Text {
        text: String,
    },
    Thinking {
        /// 可见正文（可能为空：Claude 只有 signature / Codex 只有 encrypted_content）
        text: String,
        /// Codex summary_text、Gemini thoughts.subject 之类的短摘要
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        /// 正文不可见（加密/只留签名）
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        redacted: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        /// 超过预览长度时给引用，按需取全文
        #[serde(skip_serializing_if = "Option::is_none")]
        full: Option<ContentRef>,
    },
    ToolCall {
        /// 配对键：Claude tool_use.id / Codex call_id / OpenCode callID / Pi toolCall.id
        id: String,
        raw_name: String,
        kind: ToolKind,
        /// 标题主体（命令、路径、搜索词、URL、问题…），已按 kind 提炼，≤ 200 字符
        title: String,
        /// 次要信息（workdir、行范围、description、agent 描述…）
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// MCP 服务器名（kind = mcp 时）
        #[serde(skip_serializing_if = "Option::is_none")]
        server: Option<String>,
        /// 参数 JSON 的预览（≤ 1200 字符）与全文引用
        input_preview: String,
        input_total_len: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        input_full: Option<ContentRef>,
        /// 文件改动摘要（Edit/Write/apply_patch/FileChange）
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<DiffSummary>,
        /// 用户自己运行的命令（Pi bashExecution、Codex "You ran"）
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        by_user: bool,
    },
    ToolResult {
        /// 为空表示源数据没有配对信息（Grok Build），前端显示为通用「工具输出」
        call_id: String,
        status: ToolStatus,
        /// 预览：前 12 行且 ≤ 1200 字符
        preview: String,
        total_len: u32,
        line_count: u32,
        truncated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        full: Option<ContentRef>,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        /// 结果里夹带的图片（MCP 截图、Read 图片）
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageRef>,
        /// 工具落盘的完整输出路径（Claude persistedOutputPath / Pi fullOutputPath）
        #[serde(skip_serializing_if = "Option::is_none")]
        saved_path: Option<String>,
    },
    Image {
        image: ImageRef,
    },
    Event {
        kind: EventKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// OpenCode step-start / step-finish；其他 Agent 不产生
    Step {
        phase: StepPhase,
        #[serde(skip_serializing_if = "Option::is_none")]
        tokens: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind { Shell, Read, Search, Edit, Write, Web, Mcp, Agent, Ask, Todo, Other }

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus { Success, Error, Interrupted, Pending, Unknown }

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Aborted,        // 用户中断 / turn_aborted
    Compaction,     // 上下文压缩（Codex compaction/compacted、Pi compaction、Claude /compact）
    ModelChange,    // Pi model_change、Codex thread_settings_applied
    ThinkingLevel,  // Pi thinking_level_change
    Hook,           // Claude system.stop_hook_summary（有错误时才产出）
    PrLink,         // Claude pr-link
    SlashCommand,   // Claude <command-name>
    Info, Error,    // Gemini info/error
    SubAgent,       // Codex agent_message / SubAgentActivity
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepPhase { Start, Finish }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    pub files: Vec<DiffFile>,
    pub added: u32,
    pub removed: u32,
    /// 完整 unified diff 的引用（有则可展开看 diff）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full: Option<ContentRef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile { pub path: String, pub op: DiffOp, pub added: u32, pub removed: u32 }

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffOp { Add, Update, Delete, Rename }

/// 大内容的「按需取」引用。前端原样回传，后端校验后读取。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ContentRef {
    /// JSONL：行的字节区间 + 行内 JSON Pointer（RFC 6901）
    Jsonl { offset: u64, len: u32, pointer: String },
    /// SQLite：表 + 主键 + 列 + 列内 JSON Pointer（空串表示整列，列本身不是 JSON 时用，如 Hermes messages.content）
    Sqlite { table: String, id: String, column: String, pointer: String },
    /// 独立 JSON 文件（OpenCode 文件存储 part）：相对于会话 sourcePath 所属 storage 根的路径
    File { rel_path: String, pointer: String },
    /// 工具落盘的完整输出文件（只允许会话附属目录内）
    Sidecar { rel_path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    pub source: ImageSource,
    pub media_type: String,
    /// 解码后的字节数估算（base64 长度 × 3/4）
    pub size: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ImageSource {
    /// base64 内联在源记录里（Claude image / tool_result image、Codex input_image data URL、Pi toolResult image）
    Inline { content: ContentRef },
    /// 本地文件（Codex ImageView、Claude tool-results/*.jpg、Markdown 里的 file://）
    LocalFile { path: String },
}
```

分块传输（见 §5）：

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum TranscriptChunk {
    /// 第一包：总数与轮次索引，让前端先画骨架
    Header { total: usize, turns: Vec<TurnIndex>, cached: bool, parse_ms: u64 },
    Messages { start: usize, messages: Vec<SessionMessage> },
    Done { payload_bytes: u64 },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnIndex {
    pub turn_id: String,
    pub first_message_index: usize,
    pub last_message_index: usize,
    pub question_preview: String, // ≤ 80 字符
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    pub step_count: u32,
    pub error_count: u32,
    pub has_final_reply: bool,
    pub aborted: bool,
}
```

### 3.2 TypeScript（`src/types.ts`，替换现有 `SessionMessage`）

> P0 已落地：各块另有具名接口 `TextBlock` / `ThinkingBlock` / `ToolCallBlock` / `ToolResultBlock` / `ImageBlock` / `EventBlock` / `StepBlock`，以及 `ImageSource`，`SessionBlock` 是它们的联合（与下文结构等价）。

```ts
export type ToolKind =
  | "shell" | "read" | "search" | "edit" | "write" | "web"
  | "mcp" | "agent" | "ask" | "todo" | "other";
export type ToolStatus = "success" | "error" | "interrupted" | "pending" | "unknown";
export type EventKind =
  | "aborted" | "compaction" | "model_change" | "thinking_level" | "hook"
  | "pr_link" | "slash_command" | "info" | "error" | "sub_agent" | "other";

export type ContentRef =
  | { kind: "jsonl"; offset: number; len: number; pointer: string }
  | { kind: "sqlite"; table: string; id: string; column: string; pointer: string }
  | { kind: "file"; relPath: string; pointer: string }
  | { kind: "sidecar"; relPath: string };

export interface ImageRef {
  source: { kind: "inline"; content: ContentRef } | { kind: "local_file"; path: string };
  mediaType: string;
  size: number;
  alt?: string;
}

export interface DiffFile { path: string; op: "add" | "update" | "delete" | "rename"; added: number; removed: number }
export interface DiffSummary { files: DiffFile[]; added: number; removed: number; full?: ContentRef }

export type SessionBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; text: string; summary?: string; redacted?: boolean; durationMs?: number; full?: ContentRef }
  | { type: "tool_call"; id: string; rawName: string; kind: ToolKind; title: string; detail?: string;
      server?: string; inputPreview: string; inputTotalLen: number; inputFull?: ContentRef;
      diff?: DiffSummary; byUser?: boolean }
  | { type: "tool_result"; callId: string; status: ToolStatus; preview: string; totalLen: number;
      lineCount: number; truncated: boolean; full?: ContentRef; exitCode?: number; durationMs?: number;
      images?: ImageRef[]; savedPath?: string }
  | { type: "image"; image: ImageRef }
  | { type: "event"; kind: EventKind; text?: string; url?: string }
  | { type: "step"; phase: "start" | "finish"; tokens?: number; costUsd?: number; reason?: string };

export interface MessageMeta {
  model?: string; inputTokens?: number; outputTokens?: number; cacheReadTokens?: number;
  cacheWriteTokens?: number; reasoningTokens?: number; costUsd?: number; durationMs?: number; stopReason?: string;
}

export interface SessionMessage {
  role: string;
  content: string;
  ts?: number;
  id?: string;
  turnId?: string;
  injected?: boolean;
  blocks?: SessionBlock[];   // 旧后端/测试 fixture 可省略 → 前端按 content 兜底
  meta?: MessageMeta;
}

export interface TurnIndex { turnId: string; firstMessageIndex: number; lastMessageIndex: number;
  questionPreview: string; ts?: number; stepCount: number; errorCount: number; hasFinalReply: boolean; aborted: boolean }

export type TranscriptChunk =
  | { type: "header"; total: number; turns: TurnIndex[]; cached: boolean; parseMs: number }
  | { type: "messages"; start: number; messages: SessionMessage[] }
  | { type: "done"; payloadBytes: number }
  | { type: "error"; message: string };
```

### 3.3 工具归一化（`providers/blocks.rs::normalize_tool`）

签名：`normalize_tool(source: ToolSource, raw_name: &str, namespace: Option<&str>) -> NormalizedTool { kind, server }`，`ToolSource ∈ {Claude, Codex, Gemini, OpenCode, Pi(含 OpenClaw), Generic(Hermes/Grok)}`。只按名称归类；需要上下文的细化由解析器做：`refine_shell_kind(cmd)`（`rg`/`grep`/`fd`/`find`/`ls` 开头 → search）、`kind_from_codex_parsed_cmd(type)`、apply_patch / FileChange 默认 edit，全是新增文件时解析器改为 write。`mcp__<server>__<tool>` 形式对所有来源都识别为 mcp。

| kind | Claude | Codex | Gemini | OpenCode | Pi / OpenClaw | Hermes / Grok |
|---|---|---|---|---|---|---|
| shell | Bash | exec(custom)、exec_command、shell、write_stdin、CommandExecution | run_shell_command | bash | bash、bashExecution | 名含 shell/bash/exec/terminal |
| read | Read | read_file；parsed_cmd.type=read | read_file、read_many_files | read | read | read_file |
| search | Grep、Glob、ToolSearch、ListAgents | parsed_cmd.type=search/list、`rg`/`grep`/`fd`/`find`/`ls` 开头的命令 | grep_search、glob、list_directory、search_file_content | grep、glob、list、ast_grep_search、codesearch | grep、find、ls | grep/glob/search |
| edit | Edit、MultiEdit、NotebookEdit | apply_patch(update/delete)、FileChange(update/delete) | replace、edit | edit、patch | edit | edit/patch |
| write | Write | apply_patch(add)、write_file、FileChange(add) | write_file | write | write | write_file |
| web | WebFetch、WebSearch | web_search_call、Extension(web.*)、open_page | web_fetch、google_web_search | webfetch、websearch | fetch | web* |
| mcp | `mcp__<server>__<tool>` → server=第 2 段 | function_call.namespace 以 `mcp__` 开头或 McpToolCall | 名含 `/`（**待核实**） | 名含 `_` 且不在内置表（**待核实**） | — | — |
| agent | Agent、Task、SendMessage、TaskStop | spawn_agent、send_message、agent_message、SubAgentActivity、create_thread | （子代理，**待核实**） | task、background_output、background_cancel | — | — |
| ask | AskUserQuestion | request_user_input_async、AgentMessage.questions | ask_user（**待核实**） | question | — | — |
| todo | TodoWrite | update_plan | write_todos | todowrite、todoread | — | — |
| other | 其余（Skill、Workflow、EnterWorktree…） | sleep、new_context、js（非 MCP）… | 其余 | invalid、其余 | 其余 | 其余 |

`title` 提炼规则（每类一个函数，输出 ≤ 200 字符，多行命令取第一行并加 `…`）：

- shell：命令文本（Claude `input.command`；Codex 优先 `CommandExecution.command` 去掉 `/bin/zsh -lc` 前缀，否则从 exec JS 用正则 `exec_command\(\{\s*cmd\s*:\s*("(?:[^"\\]|\\.)*")` 取 `cmd`，再否则整段 input 的首行）；`detail` = Claude `input.description` / Codex `workdir`。
- read：路径（相对 `project_dir` 显示由前端做，后端给绝对路径）；`detail` = 行范围 `offset/limit` → `:10-40`。
- search：`pattern` + ` in ` + `path`/`glob`。
- edit/write：路径；`diff` 由 `structuredPatch`（Claude）、`unified_diff`（Codex FileChange）、apply_patch 文本（数 `+`/`-` 行）、OpenCode `state.metadata.diff`（**待核实**字段名）计算。
- web：URL 或 query；多 query 用 `, `。
- mcp：`server.tool`；`detail` = 参数首个字符串字段。
- agent：`description` / `task_name`；`detail` = 模型名。
- ask：首个问题文本。
- todo：条目数 `N 项`。
- other：原名；`detail` = 参数首个字符串字段。

### 3.4 `content` 投影与兼容

`content` 由 `blocks` 派生（`model.rs::project_content`）：

- 各块的投影之间以空行（`\n\n`）连接，投影为空的块跳过。
- `Text` → 原文。
- `ToolCall` → `[Tool: {raw_name}] {title}`，title 为空时只有 `[Tool: {raw_name}]`（旧 Rust 测试 `load_messages_tool_use_shows_as_assistant` 的 `contains("[Tool: Write]")` 继续通过；旧前端 `splitToolCalls` 的正则原本要求行尾是 `]`，P0 已放宽为允许后接标题）。
- `ToolResult` → `preview`（不是全文；搜索范围因此变成「预览可见部分」，文档里要写明限制）。
- `Thinking` → 不进入 `content`（避免搜索命中不可见内容；「只看对话」也天然排除）。
- `Image` → `[Image: {media_type} {size}]`。
- `Event` → `text` 或空。
- 非空判断：旧逻辑「`content.trim().is_empty()` 就丢消息」改为「`blocks` 为空且 `content` 为空才丢」，因为纯图片消息 content 可能只有占位符。

`blocks` 为空的消息（P1 之前的旧解析器、或旧后端）`content` 保持旧格式，前端按 `content` 兜底。

旧功能的落点：

| 功能 | 新实现 |
|---|---|
| 会话内查找 / 列表搜索词高亮 | 对 `content` + 各块 `preview/title` 做 `countMatches`；命中在折叠区时自动展开该 turn / 该步骤（替代 PR #6332 的「原文中的匹配」兜底，同时保留它用于 Markdown 隐藏文本） |
| 复制单条 / 复制整段 Markdown | 从 blocks 生成：提问原文；步骤写成 `- ⏺ Bash: cmd (exit 0, 1.2s)` 列表；最终回复原文；可选包含思考（默认不含） |
| 只看对话 | 隐藏所有 `tool_call/tool_result/thinking/event/step` 行，只留提问与最终回复 |
| 提问目录（TOC） | 直接用 `TranscriptChunk.header.turns`（后端已排除 `injected`），不再在前端做 Codex 前缀判断 |
| 消息计数 | 「{turns} 轮 · {messages} 条」 |

### 3.5 P0 落地调整与契约 fixture

相对初稿的调整（均已同步到上文）：

1. 所有模型类型同时 derive `Deserialize` + `PartialEq`：fixture 往返校验要用，P2 的 `ContentRef`/`ImageRef` 回传与缓存也需要。
2. 带字段的 tagged enum（`SessionBlock`、`ContentRef`、`ImageSource`、`TranscriptChunk`）加 `rename_all_fields = "camelCase"`。初稿只有 `rename_all = "snake_case"`，那只改变体名，块内字段会序列化成 `raw_name` / `call_id` / `rel_path`，与 TS 的 camelCase 对不上。
3. `MessageMeta` 各字段、`TurnIndex.ts` 加 `skip_serializing_if = "Option::is_none"`，避免输出 `null`（TS 是可选字段）。
4. `ToolResult.call_id` 允许空串表示无配对；`ContentRef::Sqlite.pointer` 空串表示整列。
5. `mod.rs` 只 `pub use model::SessionMessage`，其余类型从 `session_manager::model` 引用。新增 `SessionMessage::legacy(role, content, ts)`（旧解析器过渡用，blocks 为空）、`SessionMessage::from_blocks(role, ts, blocks)`（content 自动投影）、`is_empty()`（§3.4 的非空判断）。
6. 预览口径（`blocks.rs::preview`）：`total_len` 为全文字符数（Unicode 标量），`line_count` 为 `str::lines()` 行数，预览去掉末尾换行；只有预览之后还有非换行内容时 `truncated = true`。
7. `title_*` 的骨架与常量在 `blocks.rs`；Codex shell 包装去除、exec JS 取 `cmd` 等 Agent 特有逻辑留给 P1。

契约 fixture：`tests/fixtures/sessions/{claude,codex,gemini,opencode,pi,generic}.messages.json`，每份是一次 `get_session_messages` 的返回（`SessionMessage[]`）。`generic` 以 Hermes 为主，混入 Grok Build 风格的无配对输出（`callId: ""`）和 MiniMax 风格的纯对话轮。两侧校验：

- Rust `model.rs::shared_fixtures_round_trip_and_match_projection`：反序列化再序列化必须与原 JSON 相等（字段名、省略规则一致），且每条消息 `content == project_content(blocks)`。f64 字段（`costUsd`）不要写成整数。
- TS `tests/types/sessionFixtures.test.ts`：运行时 schema 经 `satisfies` 绑定 `src/types.ts`（多/少字段、可选性不一致都编译失败）；校验每个 `tool_result` 能配上同一轮里更早的 `tool_call`、预览上限与统计口径，以及 6 份合起来覆盖全部块类型 / ToolKind / ToolStatus / EventKind / 两种图片来源。

---

## 4. 各 Agent 解析映射规则

通用约定（`providers/blocks.rs`）：

- `PREVIEW_LINES = 12`、`PREVIEW_CHARS = 1200`、`INPUT_PREVIEW_CHARS = 1200`、`THINKING_PREVIEW_CHARS = 400`、`TITLE_CHARS = 200`。超出即 `truncated = true` 并带 `full: ContentRef`。
- JSONL 解析器改用 `BufReader::read_until(b'\n')` 并累计字节偏移，给每条记录 `(offset, len)`；`pointer` 由构造块的位置生成（如 `/message/content/3/content`）。
- 配对：解析器只负责输出 `ToolCall.id` 与 `ToolResult.call_id`；同记录内已含结果（OpenCode/Gemini）时紧跟着输出 `ToolResult`。配对与「孤儿结果」处理在前端 `turns.ts`。
- `turn_id`：有原生 turn 概念（Codex `turn_context.turn_id`）直接用；其余按「非 `injected` 的 user 消息」递增 `t{n}`；首条 user 之前的内容归 `t0`。

### 4.1 Claude

| 源 | → |
|---|---|
| 顶层 `type ∈ {attachment, file-history-*, queue-operation, mode, atis-latch, last-prompt, agent-name, cost-state, worktree-state, relocated, custom-title}`、`isMeta=true`、`isSidechain=true` | 跳过 |
| `type=system, subtype=stop_hook_summary` 且 `hookErrors` 非空 | `system` 消息，`Event{Hook, text: hookErrors.join("\n")}`；无错误跳过 |
| `type=pr-link` | `system` 消息，`Event{PrLink, text: "PR #N", url: prUrl}` |
| `user` 文本以 `<command-name>` 开头 | `Event{SlashCommand, text: 命令名}`，`injected=false`（它是用户动作，进目录但显示为事件行） |
| `user` 文本含 `<local-command-caveat>` / 全是 `<system-reminder>…</system-reminder>` / `<task-notification>` | `injected=true`；system-reminder 片段从 `Text` 中剥掉（正则 `(?s)<system-reminder>.*?</system-reminder>`），剥完为空则整条 `injected` |
| `user` 文本 `[Request interrupted by user…]` | `Event{Aborted}` |
| `message.content` 为 string | `Text` |
| 块 `text` | `Text` |
| 块 `thinking` | `Thinking{text, redacted: text.is_empty() && signature 非空, duration_ms: 记录顶层 thinkingDurationMs}`；`text` 为空且 redacted → 保留块（前端决定是否显示「思考（不可见）」），**不**计入 content |
| 块 `tool_use{id,name,input}` | `ToolCall`；`kind/title/detail` 按 §3.3；Edit/Write 的 `diff` 优先取下一条结果记录的 `toolUseResult.structuredPatch`（解析器需要向后看一条——实现为「结果记录到来时回填上一条 ToolCall 的 diff」，两条消息同属一个 `Vec`，可直接索引） |
| 块 `tool_result` | `ToolResult{call_id: tool_use_id, status: is_error ? Error : Success}`；`content` 为 string → 预览；为数组 → `text` 子项拼接，`image` 子项 → `images[]`（`Inline{Jsonl{offset,len,pointer:/message/content/i/content/j/source/data}}`）；`exit_code` 从首行 `^Exit code (\d+)` 提取；`toolUseResult.interrupted=true` → `Interrupted`；`toolUseResult.persistedOutputPath` → `saved_path`；`duration_ms = ts(result) - ts(call)`（近似，前端标 `≈`） |
| 块 `image`（用户贴图） | `Image{Inline{Jsonl{…/message/content/i/source/data}}, media_type, size}` |
| `message.model/usage` | `meta` |
| 角色 | 全是 `tool_result` 的 user 记录 → `role=tool`（沿用） |

### 4.2 Codex（重写 `codex.rs::load_messages`）

两遍不如一遍带状态机：按 `ordinal` 顺序读取，维护 `current_turn_id`、`pending_calls: HashMap<call_id, 消息下标>`、`recent_items: Vec<item_completed 项>`。

| 源 | → |
|---|---|
| `session_meta`、`world_state`、`token_usage_record`（→ 累计到当前 turn 的最后一条 assistant `meta`）、`inter_agent_communication_metadata` | 不产生消息 |
| `turn_context{turn_id, model}` | 设置 `current_turn_id`；model 进后续 `meta` |
| `event_msg.task_started` | 开始新 turn（若 `turn_context` 缺失） |
| `event_msg.task_complete` | 标记 turn 完成（`last_agent_message` 可用来确认最终回复，不单独成消息） |
| `event_msg.turn_aborted{reason, duration_ms}` | `system` 消息 `Event{Aborted, text: reason}` |
| `event_msg.thread_settings_applied{thread_settings.model}` | 与上一个不同才产生 `Event{ModelChange, text: model}` |
| `event_msg.item_completed{item}` | 压入 `recent_items`（只保留 CommandExecution / FileChange / McpToolCall / WebSearch / ImageView / ContextCompaction / Extension / SubAgentActivity）；**不直接成消息**，由对应的 call/output 消费 |
| `response_item.message{role=developer}` | `system` 消息，`injected=true`（默认不显示；「显示注入内容」开关可见） |
| `response_item.message{role=user}` | `user`；`input_text` → `Text`，`input_image{image_url: data:…}` → `Image{Inline{Jsonl{…/payload/content/i/image_url}}}`（后端取全文时去掉 `data:<mime>;base64,` 前缀）；文本以 `# AGENTS.md instructions for`、`<environment_context>` 开头 → `injected=true`；以 `# Context from my IDE setup:` 开头 → 用现有 `extract_codex_prompt_from_ide_context` 提取真正提问为 `Text`，提不出则 `injected=true` |
| `response_item.message{role=assistant}` | `assistant`；`output_text` → `Text` |
| `response_item.reasoning` | `assistant` 消息 `Thinking{summary: summary[].text 拼接, text: content[].text 拼接, redacted: 两者皆空}`；两者皆空且上一块也是 redacted thinking → 合并不重复 |
| `response_item.function_call{name, namespace, arguments, call_id}` | `assistant` 消息 `ToolCall`；`namespace` 以 `mcp__` 开头 → `kind=Mcp, server=去前缀`；`exec_command`/`shell` → 解析 `arguments` JSON 的 `cmd`/`command`（数组则 join）；`request_user_input_async` → Ask，title=首个 question.title；`spawn_agent/send_message` → Agent；记入 `pending_calls` |
| `response_item.custom_tool_call{name=exec, input, call_id}` | `ToolCall{kind: Shell}`；`title` 来自随后匹配到的 `CommandExecution.command`（去 `/bin/zsh -lc` 等 shell 前缀；多条命令 → 多个子 ToolCall，id 加后缀 `#1`、`#2`），匹配不到则用正则取 `cmd`，再否则取 input 首行；`input_full` 指向 JS 源 |
| `response_item.custom_tool_call{name=apply_patch}` | `ToolCall{kind: 按文件操作，多文件以 Edit 为主}`，`diff` 解析 `*** Update/Add/Delete File:` 与 `+`/`-` 行；若有匹配的 `FileChange` 项则用其 `unified_diff` 统计并作为 `diff.full`（指向 item_completed 行 `/payload/item/changes/<path>/unified_diff`） |
| `response_item.function_call_output` / `custom_tool_call_output{call_id, output}` | `tool` 消息 `ToolResult`；`output` 为数组 → `input_text` 拼接；去掉 `Script completed\nWall time X seconds\nOutput:\n` 头（`duration_ms` 从中提取）；`exit_code`：`Process exited with code (\d+)` / 匹配到的 `CommandExecution.exit_code`；`status`：exit_code≠0 → Error，`CommandExecution.status=failed/interrupted` → 对应；`duration_ms` 优先 `CommandExecution.duration` |
| CommandExecution ↔ exec 配对 | 对每个 `custom_tool_call(exec)`，取其 ordinal 与对应 output ordinal 之间的 CommandExecution 项（`source=unified_exec_startup` 或 `unified_exec_*`）；取不到则按 output 内容包含 `aggregated_output` 前 200 字符匹配；都失败则无结构化信息（仍能显示） |
| `response_item.web_search_call{action}` | `assistant` 消息 `ToolCall{kind: Web, title: query 或 url, raw_name: web_search}` + 同消息紧跟 `ToolResult{status: Success, preview: ""}`（无输出） |
| `response_item.compaction` / 顶层 `compacted` | `system` 消息 `Event{Compaction}`；`compacted.payload.message` 的摘要文本放 `text`（预览 400 字符 + `full`） |
| `response_item.agent_message{author, content}` | `system` 消息 `Event{SubAgent, text: author + 首行}`，正文 `Text` 块 |
| `McpToolCall` 项（当对应 function_call 已消费时）| 回填 `ToolResult{status: result.isError ? Error : Success, duration_ms}`、`images` ← `result.content[].type=image`（**待核实**字段） |
| `ImageView{path}` | `assistant` 消息 `Image{LocalFile{path: 去 file:// 前缀}}` |

### 4.3 Gemini

| 源 | → |
|---|---|
| `type=user` | `user`；content string / `[{text}]` → `Text` |
| `type=gemini` | `assistant`；`thoughts[]` → 每条 `Thinking{summary: subject, text: description}`（合并成一个块，`summary` 用 ` · ` 连接，`text` 以 `**subject**\n\ndescription` 拼接）；`content` → `Text`；`tokens/model` → `meta` |
| `toolCalls[]`（**待核实**字段） | `ToolCall{id, raw_name: name, title 按 args}` + `ToolResult{status: status∈{success,completed} ? Success : status=error ? Error : status=cancelled ? Interrupted : Unknown, preview: resultDisplay 为 string 则取之，为对象则 JSON 预览}` |
| `type=error` | `system` 消息 `Event{Error, text}` |
| `type=info` | `system` 消息 `Event{Info, text}`，`injected=true`（默认折叠） |

### 4.4 OpenCode

三条读取路径（文件存储、SQLite v1、v2）统一走 `opencode_blocks::message_from_parts(role, meta, parts: Vec<Value>) -> SessionMessage`：

| part.type | → |
|---|---|
| `text` | `Text` |
| `reasoning` | `Thinking{text, duration_ms: time.end - time.start}` |
| `step-start` | `Step{Start}` |
| `step-finish{reason, tokens, cost}` | `Step{Finish, tokens: tokens.total, cost_usd: cost, reason}` |
| `tool{callID, tool, state}` | `ToolCall{id: callID, raw_name: tool, title: state.title 非空则用之，否则按 input 提炼}` + `ToolResult{call_id, status: completed→Success / error→Error / running,pending→Pending, preview: state.output 或 state.error, duration_ms: time.end-time.start}`；`full` 指向 `Sqlite{table: part, id, column: data, pointer: /state/output}` 或 `File{rel_path, pointer}` |
| `agent{name}` | `Event{Other, text: "@name"}`（@提及的 agent） |
| `patch` / `file` | `patch` → `ToolCall{kind: Edit, diff}`（**待核实**结构）；`file` → `Image`（图片 mime）或 `Event{Other, text: 文件名}` |
| 消息级 `cost/tokens/modelID/providerID/time` | `meta` |

V2 `session_message.data.content[]` 结构同上（`type: reasoning|tool|text`），`tool` 项字段为 `{type:"tool", name, id, …}`（见现有测试），缺 `state` 时 `ToolResult.status = Unknown`。

### 4.5 Pi（与 OpenClaw 共用 `pi_blocks.rs`）

| 源 | → |
|---|---|
| `model_change{provider, modelId}` | `system` `Event{ModelChange, text: "provider/modelId"}` |
| `thinking_level_change{thinkingLevel}` | `system` `Event{ThinkingLevel, text}` |
| `message.role=system` | 跳过（Pi 的 system prompt sections 非对话内容；可选 `injected=true` 保留，默认跳过） |
| `message.role=user` | `user`；`text` → `Text`；`image` → `Image` |
| `message.role=assistant` | `assistant`；`thinking{thinking, thinkingSignature}` → `Thinking{redacted: thinking 为空}`；`text` → `Text`；`toolCall{id, name, arguments}` → `ToolCall`；`message.usage/model/stopReason` → `meta`；`stopReason=aborted/length` → 追加 `Event{Aborted}` / `Event{Other, text: "truncated"}` |
| `message.role=toolResult{toolCallId, toolName, content[], isError, details}` | `tool`；`ToolResult{call_id, status: isError ? Error : Success}`；`content[].image` → `images[]`；`details.exitCode/durationMs`（**待核实**）→ 对应字段 |
| `message.role=bashExecution{command, output, exitCode?}` | `user`；`ToolCall{kind: Shell, by_user: true, id: entry id}` + `ToolResult` |
| `compaction` / `branch_summary` / `compactionSummary` | `system` `Event{Compaction, text: summary 预览}` |
| `custom_message{display≠false}` | `system` `Text` |

### 4.6 Hermes / OpenClaw / Grok Build / MiniMax Code

- Hermes SQLite：`role=assistant` 行的 `tool_calls` JSON → 每项 `ToolCall{id: call.id, raw_name: function.name, title: 按 arguments}`；`role=tool` 行 → `ToolResult{call_id: tool_call_id, status: Unknown}`；`tool_name` 用于 kind 归一化。JSONL 路径同理（**待核实**字段名）。
- OpenClaw：Pi 映射，去掉树过滤。
- Grok Build：`type=tool` → `tool` 消息 `ToolResult{call_id: "", status: Unknown}`；前端把 `call_id` 为空的结果渲染成「工具输出」通用步骤；assistant `tool_calls`（**待核实**）存在时按 id 配对。
- MiniMax Code：只有 `Text`；`TurnIndex.step_count = 0`，前端隐藏执行过程区。

---

## 5. Tauri 命令、缓存与安全

### 5.1 命令

| 命令 | 签名 | 说明 |
|---|---|---|
| `get_session_messages`（保留） | `(providerId, sourcePath) -> Vec<SessionMessage>` | 走缓存；返回新结构。给测试与「复制整段」等一次性场景用 |
| `stream_session_messages`（新） | `(providerId, sourcePath, onChunk: Channel<TranscriptChunk>) -> ()` | 走缓存；先发 `Header`，再按 **≤ 256KB 序列化字节或 ≤ 150 条** 一包发 `Messages`，最后 `Done`。前端首包到达即渲染 |
| `get_session_block_content`（新） | `(providerId, sourcePath, ref: ContentRef, offset?: u32, limit?: u32) -> BlockContent { text, totalLen, truncated, nextOffset? }` | 取工具输出 / 参数 / 思考 / diff 全文；默认 `limit = 512KB` 字符，超出分页 |
| `get_session_image`（新） | `(providerId, sourcePath, image: ImageRef) -> tauri::ipc::Response` | 返回原始字节（`Content-Type` 放在响应头或由前端用 `mediaType`）；前端 `invoke<ArrayBuffer>` → `Blob` → `URL.createObjectURL` |
| `reveal_session_path`（可选，决策 D3） | `(path) -> bool` | `tauri_plugin_opener::reveal_item_in_dir`；只允许已存在的文件 |
| `open_external`（现有，微调） | `(url)` | 现在把非 http 的一律补 `https://`；要改成白名单 `http:`/`https:`/`mailto:`，其余拒绝（Markdown 里 `javascript:` 等已被前端过滤，这里是第二道） |

`src/lib/api/sessions.ts` 对应新增 `streamMessages(providerId, sourcePath, onChunk)`、`getBlockContent(...)`、`getImage(...)`。

### 5.2 解析缓存 `TranscriptCache`（`session_manager/cache.rs`）

- Key：`(provider_id, source_path)`；Value：`{ modified: SystemTime, len: u64, transcript: Arc<Transcript> }`，`Transcript = { messages: Vec<SessionMessage>, turns: Vec<TurnIndex>, approx_bytes: usize }`。
- 命中条件：`metadata().modified == cached.modified && len == cached.len`（与 `FileParseCache` 一致；SQLite 源用 db 文件的 mtime/len）。
- 容量：LRU 最多 8 个会话、合计 `approx_bytes ≤ 96MB`（`approx_bytes` 用序列化长度估算一次）。
- 失效：`delete_session(s)` 成功后移除对应项；列表刷新不清。
- 增量解析（后续优化，不在首批）：JSONL 追加写时，只从上次 `len` 处继续解析并修正最后一个 turn。首批先全量重解析，因为缓存命中才是常态。
- 命令在 `spawn_blocking` 里跑；`stream_session_messages` 在同一个阻塞任务里串行发送 chunk（`Channel::send` 非阻塞）。

### 5.3 安全边界

- **路径归属**：所有命令先 `validate_source(provider_id, source_path) -> ValidatedSource`：复用 `provider_roots()` + `canonicalize_existing_path()`（从 `delete_session_with_roots` 抽出），要求 `source_path` 在某个 root 之下；SQLite 源解析 `sqlite:<db>:<id>` 后对 `<db>` 做同样校验（Hermes 已有 `get_hermes_db_path` 对比，OpenCode 对比 `get_opencode_data_dir`）。MCode 只允许 `mcode:` 前缀且 db 路径固定。
- **ContentRef 校验**：
  - `Jsonl`：`offset + len ≤ 文件当前大小`，`len ≤ 32MB`；读出的区间必须是以 `\n` 结尾（或文件末尾）的完整行且能 `serde_json::from_slice`；`pointer` 必须解析到 string（或 string 数组 → join）；否则返回「会话已更新，请重新读取」。
  - `Sqlite`：`table ∈ {part, message, session_message, messages}` 白名单，`column ∈ {data, content}`，`id` 用参数绑定；只读连接。
  - `File`：`rel_path` 不得含 `..`，拼在该会话的 storage 根之下再 canonicalize 并 `starts_with` 校验。
  - `Sidecar`：拼在 `<sourcePath 去扩展名>/` 之下（Claude `<sessionId>/tool-results/`），同样 canonicalize 校验；大小 ≤ 32MB。
- **图片**：`Inline` 走上面的 ContentRef 校验，base64 解码后 ≤ 20MB；`LocalFile` 只允许：(a) 会话 `project_dir` 之下，(b) provider 自己的目录（`~/.codex/visualizations/**`、Claude sidecar），(c) 扩展名 ∈ {png,jpg,jpeg,gif,webp,bmp,svg(→ 不渲染，显示路径)}，(d) ≤ 20MB；校验失败前端显示路径 chip 而不是图。
- **链接**：前端白名单 `http/https/mailto`（PR #6332 的 `safeExternalUrl`），点击一律 `event.preventDefault()` 后调 `open_external`；`<a>` 不带 `href` 真实导航（防 webview 跳转）。`file://` 链接不打开，渲染为路径 chip（复制 / 可选 reveal）。
- **文本注入**：所有块内容按文本渲染（React 转义），Markdown 渲染器不输出原生 HTML（lezer 的 `HTMLBlock/HTMLTag` 节点按字面输出，PR 已如此，需在测试里固化）。
- **大小上限**：单 chunk ≤ 1MB；`get_session_block_content` 单次 ≤ 2MB；前端对已展开的全文超过 2MB 时提示「复制源文件路径」而不是继续加载。

---

## 6. 前端设计

### 6.1 版式骨架

```
┌ AppPageHeader ──────────────────────────────────────────────────────┐
│ ←  [glyph] 会话标题                      ‹ ›   [▶ 在 iTerm 中恢复 ▾]  ⋯ │
├ 信息栏（一行）────────────────────────────────────────────────────────┤
│ ~/Projects/foo · 10月2日 15:25 → 17:33 · 7 轮 · 2068 条   只看对话 ◯  展开全部过程 ◯  提问 7 ⌄  🔍 │
├ 正文（虚拟列表，内容区最大宽 760px 居中；工具行可用到 880px）────────────┤
│                                                                       │
│  ┃ 你  15:25                                           ← 提问：左侧 3px 竖条（agent accent），bg-subtle，text-body
│  ┃ 那电脑，我用配置Rust的，是不是最新发布1.99？…
│  ┃ [图 1 缩略]
│                                                                       │
│  ▸ 执行过程 · 14 步 · 9 个命令 · 改了 2 个文件 · ⏱ 2m14s     ← 折叠摘要行（text-caption, text-fg-2）
│    ⏺ Bash(cargo build)  ✗ exit 101 · 12s                      ← 失败步骤常显（折叠时也列出）
│      ⎿ error[E0433]: failed to resolve …（预览 6 行）
│                                                                       │
│  ⏺ Claude Code  15:27 · claude-opus-5-5                      ← 最终回复：无底色，text-body，Markdown
│    基准测试已在后台运行。每个版本都用独立的 target 目录……
│    ┌ 代码块 ┐ …
│                                                                       │
│  ───────────── 分轮线（border） ─────────────
│  ┃ 你  15:31 …
```

展开后的执行过程（时间线）：

```
  ▾ 执行过程 · 14 步 · 9 个命令 · 改了 2 个文件 · ⏱ 2m14s          [全部展开] [复制]
  │ ✻ 思考 · 1.2k 字 · 8s                                        ← Thinking 行，点开斜体全文
  │ ⏺ Bash(rustc -V; cargo -V)              ✓ · 0.3s · 12 行     ← 步骤行：glyph + 标题 + 状态 + 元信息
  │ ⏺ Read(src/lib.rs:1-120)                ✓ · 120 行
  │ ⏺ Edit(src/lib.rs)                      ✓ · +3 −1            ← diff 计数（Codex 风格 +x −y）
  │ ⏺ Bash(cargo build)                     ✗ exit 101 · 12s
  │   ⎿ error[E0433]: …                                          ← 失败：预览默认展开 6 行
  │     …还有 88 行  [显示全部]                                   ← 点击按需取全文
  │ ⏺ Agent(Design session reader) · fable   ✓ · 4m
  │ ⏺ Called Claude_Browser 5 times           ✓                   ← MCP 合并（Claude 原生规则）
  │ ◦ 说明：基准测试已在后台运行…                                  ← 过程中的助手文本（非最终）作为「说明」行，灰字
```

### 6.2 行模型与虚拟化

把 turn 扁平成行，交给现有 `useVirtualizer`（动态测量）：

```ts
type ReaderRow =
  | { kind: "question"; turn: number; messageIndex: number }
  | { kind: "timeline"; turn: number; summary: TimelineSummary }        // 折叠摘要行
  | { kind: "step"; turn: number; step: TimelineStep }                  // 展开后每步一行
  | { kind: "final"; turn: number; messageIndex: number }
  | { kind: "event"; turn: number; block: EventBlock }                  // 中断 / 压缩 / 模型切换
  | { kind: "turn_divider"; turn: number };
```

- `turns.ts::buildTurns(messages, turnIndex)` → `SessionTurn { question, steps: TimelineStep[], final, aborted, events }`；`TimelineStep = { kind: "tool" | "thinking" | "note" | "merged" , call?, result?, children? }`。
- 配对：按 `call_id` 在 turn 内找结果；无结果 → `status: pending`（会话仍在跑或被中断）；无调用的结果 → 通用「工具输出」步骤。
- 「最终回复」判定：turn 内最后一条 `assistant` 消息中、位于最后一个 `tool_call` 之后的 `Text` 块；Codex 若有 `AgentMessage.phase=final_answer`（**首批不用**，因它在 item_completed 里，解析器可标记 `meta.stop_reason = "final_answer"`）。没有 → `aborted` 或「进行中」。
- 过程中的助手 `Text`（最终回复之前的）→ `note` 步骤。
- 合并（`merged`）由风格配置决定（§6.6）。
- 行高估算：question 96、timeline 36、step 32、final 160；`overscan 8`；展开/折叠调用 `virtualizer.measure()`。

### 6.3 组件拆分（文件级，`src/components/sessions/reader/`）

| 文件 | 职责 |
|---|---|
| `SessionReader.tsx`（替换现有） | 页头、信息栏、工具条、流式加载状态、虚拟列表、查找、跳转；只做编排 |
| `turns.ts` | `buildTurns`、`flattenRows`、`summarizeTimeline`、搜索命中定位；纯函数，vitest 全覆盖 |
| `toolSummary.ts` | `formatStepTitle(block, style, projectDir)`、`formatStepMeta`（时长、行数、+x −y、exit）、路径缩短 |
| `agentStyles.ts` | `AgentReaderStyle` 接口与五家 + 通用取值（§6.6） |
| `SessionQuestion.tsx` | 提问卡：竖条、角色标签、时间、文本（纯文本 + 围栏代码）、图片缩略图、复制 |
| `SessionTimeline.tsx` | 折叠摘要行 + 常显失败步骤；受控 `expanded` |
| `SessionStep.tsx` | 单步一行：glyph、标题、状态、元信息；`<button aria-expanded>`；展开渲染 `SessionStepDetails` |
| `SessionStepDetails.tsx` | 参数区（命令 / 路径 / JSON 预览）、输出预览、「显示全部」（`useBlockContent`）、diff（`SessionDiff`）、结果图片（`SessionImage`） |
| `SessionThinkingRow.tsx` | 思考行与展开态（斜体、text-fg-2） |
| `SessionNoteRow.tsx` | 过程中的助手说明（Markdown，小字） |
| `SessionFinalReply.tsx` | 最终回复：角色行（glyph + Agent 名 + 时间 + 模型）、`SessionMarkdown`、折叠（>3000 字）、复制 |
| `SessionEventRow.tsx` | 中断 / 压缩 / 模型切换 / PR 链接 / Hook 错误 |
| `SessionMarkdown.tsx` | 从 PR #6332 迁入（§6.5） |
| `SessionCodeBlock.tsx` | 现有 `CodeBlock` 独立出来，供 Markdown 与步骤详情共用 |
| `SessionDiff.tsx` | unified diff / structuredPatch 渲染：行号、`+` success-soft、`-` danger-soft、hunk 间 `⋮` |
| `SessionImage.tsx` | 缩略图（`useSessionImage` 取 Blob，`loading="lazy"`，占位 skeleton，失败显示路径 chip）；点击开 `SessionImageLightbox` |
| `SessionImageLightbox.tsx` | 基于现有 `Dialog`：原图、缩放 100%/适应、复制图片、Esc 关闭 |
| `PathChip.tsx` | 本地路径：相对 projectDir 显示、hover 全路径、点击复制、菜单「在 Finder 中显示」（决策 D3） |
| `useSessionTranscript.ts`（`src/lib/query/sessions.ts`） | `stream_session_messages` 的 TanStack Query 封装：`queryFn` 里收 chunk 写入 `queryClient.setQueryData` 累积；返回 `{ header, messages, isStreaming, progress }` |
| `useBlockContent.ts` | `get_session_block_content` 查询，`staleTime: Infinity`，key 含 ref 序列化 |
| `useSessionImage.ts` | `get_session_image` → Blob URL，组件卸载 `revokeObjectURL`，LRU 32 张 |

保留 `src/components/sessions/utils.ts` 的列表相关函数；`buildSessionDisplayItems/splitToolCalls/formatToolNames/splitMarkdownBlocks` 删除或迁入 `turns.ts`（测试同步迁移）。

### 6.4 各类 block 的渲染规格

| block | 位置 | 规格 |
|---|---|---|
| 提问 Text | question 卡 | `whitespace-pre-wrap`，围栏代码块用 `SessionCodeBlock`，不渲染 Markdown（决策 D7）；> 3000 字折叠到 1500 字 |
| 最终回复 Text | final | `SessionMarkdown`；标题 h1–h3 映射为 `text-title/section/body font-semibold`；列表、表格（`overflow-x-auto`）、引用（左 2px border）、任务列表（禁用 checkbox）；链接 `text-action-text underline-offset-[3px]`，hover 下划线，`title` 显示 URL，外链图标；图片见下 |
| 说明 Text（note） | step | 同 Markdown，但 `text-caption text-fg-2`，默认显示前 3 行，点开全文 |
| Thinking | step | 一行 `✻ 思考 · {chars} 字 · {duration}`；`redacted` → `✻ 思考（内容不可见）` 灰字无按钮；展开：`italic text-fg-2 whitespace-pre-wrap`，> 400 字按需取全文 |
| ToolCall + ToolResult | step | 一行：`[glyph][标题 mono text-caption][detail text-fg-3][状态符+文字][元信息 tabular-nums]`；状态色：success→`text-success-text`、error→`text-danger-text`、interrupted→`text-warning-text`、pending→`text-fg-3`；标题超长 `truncate`，hover `title` 全文；展开态见 6.5.2 |
| ToolResult 预览 | step 展开 | `pre font-mono text-caption bg-subtle rounded-[8px]`，最多 12 行，`max-h-[480px] overflow-auto`；底栏「…还有 N 行 · 显示全部」；全文 > 2MB 显示「复制源文件路径」 |
| DiffSummary | step 行 + 展开 | 行内 `+3 −1`（绿/红）；展开用 `SessionDiff`，多文件每文件一个小标题 `└ path (+x −y)` |
| Image（用户贴图） | question 卡 | 缩略图网格：高 96px，`object-cover rounded-[8px] border`，`alt`=「图 N」；点击放大 |
| Image（结果截图） | step 展开 | 同上，宽度随容器，最大高 320px |
| Image（Markdown 远程） | final | 默认不加载：显示 `[图片] alt · 点击加载`（PR 现有 `RemoteImage`） |
| Event | event 行 | 居中细字：`— 已中断 —` / `— 上下文已压缩 —` / `— 模型切换为 gpt-6.1 —`；Hook 错误与 Gemini error 用 `text-danger-text`；PrLink 带可点链接 |
| Step（OpenCode） | timeline 摘要/步骤 | `step-finish` 不单独成行；并入该步的元信息「· 28.5k tok · $0.012」，由风格配置 `showStepCost` 开关 |
| injected 消息 | 不入目录 | 默认隐藏；信息栏 ⋯ 菜单「显示注入的上下文」开启后以灰色 `event` 行显示「上下文 · AGENTS.md · 3.2k 字」可展开 |

### 6.5 折叠规则（硬性）

**轮（turn）级**

1. 有 ≥ 1 步时，执行过程默认折叠为一行摘要：`执行过程 · {N} 步 · {命令数} 个命令 · 改了 {文件数} 个文件 · {失败数} 个失败 · ⏱ {总时长}`（为 0 的项不写）。
2. 失败/中断的步骤（`status ∈ {error, interrupted}`）在折叠态下**仍逐条列出**（最多 5 条，超出显示「还有 N 个失败」），且其输出预览默认展开前 6 行。
3. 以下情况整轮自动展开：turn `aborted` 或没有最终回复；查找命中在步骤内；用户开了「展开全部过程」（持久化到 `settings.json` 的 UI 偏好，key `sessionReader.expandTimelines`）。
4. 最后一轮不自动展开（决策 D5 可改）。
5. 只有 1 步且成功时，不显示摘要行，直接显示该步骤行（避免「1 步」再点一次）。

**步（step）级**

6. 每步默认一行；点击展开详情：参数区 + 输出预览 ≤ 12 行 / 1200 字 + 「显示全部 N 行」；再点收起。
7. 失败步骤展开时参数与输出都展开；成功步骤展开只显示输出（参数折在「参数」小标题下，shell 命令例外——命令本身就是标题，参数区显示完整多行命令）。
8. 思考默认一行；`redacted` 不可展开。
9. 合并步骤（Explored / Called N times）展开后显示子步骤列表，子步骤再各自可展开。
10. 过程中的说明（note）默认显示前 3 行。

**内容级**

11. 最终回复 > 3000 字折叠到 1500 字（沿用 PR 的 `createCollapsedMarkdownPreview`，保证围栏闭合）；查找命中时自动展开。
12. 输出预览的「显示全部」按需取全文，一次 512KB，超出继续「加载更多」；> 2MB 停止并提示复制源文件路径。
13. 图片一律懒加载（进入视口 + 用户滚到附近才 `get_session_image`），缩略图不超过 96px 高。

### 6.6 风格配置接口与五家取值

```ts
export interface AgentReaderStyle {
  id: SessionAppId | "generic";
  /** CSS 变量名，提问竖条、助手 glyph、链接 hover 用它 */
  accentVar: string;
  /** 助手名前的符号；空串表示不画 */
  assistantGlyph: string;
  userGlyph: string;
  /** 步骤状态符 */
  step: { success: string; error: string; pending: string; interrupted: string };
  /** 结果行引出符：tree 用符号，box 用左边框，none 不画 */
  resultFrame: "tree" | "box" | "none";
  resultConnector: string;
  thinkingGlyph: string;
  /** 思考行标签的 i18n key（sessionManager.reader.thinking.* ） */
  thinkingLabelKey: string;
  /** 标题格式：call = Name(arg)；verb = 动词 + arg；icon = 符号 + Name arg */
  titleFormat: "call" | "verb" | "icon";
  /** 每类工具的动词/名称 i18n key 或固定字面（Claude 用原工具名） */
  verbs: Partial<Record<ToolKind, string>>;
  /** 合并规则 */
  merge: { readSearchRuns: boolean; mcpSameServerRuns: boolean };
  /** 元信息 */
  showDiffCounts: boolean;
  showStepCost: boolean;
  showExitCode: "always" | "nonzero";
  /** 字体：步骤标题是否 mono */
  monoTitles: boolean;
}
```

| 字段 | Claude Code | Codex | Gemini CLI | OpenCode | Pi | generic（Hermes/OpenClaw/Grok/MCode） |
|---|---|---|---|---|---|---|
| accentVar | `--agent-claude` | `--agent-codex` | `--agent-gemini` | `--agent-opencode` | `--agent-pi` | `--agent-generic`（= `--text-2`） |
| assistantGlyph | `⏺` | `•` | `✦` | `` | `` | `•` |
| userGlyph | `>` | `›` | `>` | `` | `` | `` |
| step.success / error / pending / interrupted | `⏺`(success 色) / `⏺`(danger) / `⏺`(fg-3) / `⏺`(warning) | `•` 同上着色 | `✓` / `x` / `o` / `-` | `✓` / `✗` / `~` / `✗` | `●` 按状态着色（Pi 原生用整块背景色；这里改为左侧 2px 竖条着色 + 圆点） | `•` |
| resultFrame / connector | tree `⎿` | tree `└` | box（左侧 1px 圆角边框，`border-l` + 状态色） | tree `│`（shell 输出用 `$` 开头） | none（竖条已表状态） | tree `└` |
| thinkingGlyph / label | `✻` / `thinking.claude`「思考中…」 | `•` / `thinking.codex`「推理」 | `✦` / `thinking.gemini`「想法」 | `` / `thinking.opencode`「Thought」 | `` / `thinking.pi`「Thinking…」 | `•` / `thinking.generic`「思考」 |
| titleFormat | call：`Bash(cargo build)`、`Read(src/a.ts)`、`Update(path)`（Edit→Update，Write→Write/Create） | verb：`Ran cargo build`、`Explored`、`Edited path (+3 −1)`、`Called server.tool(…)`、`Searched the web query` | verb：`Shell cargo build`、`ReadFile path`、`WriteFile`、`Edit`、`GoogleSearch`（**待核实**显示名） | icon：`$ cargo build`、`→ Read path`、`← Wrote path`、`← Edit path`、`✱ Grep "p" in dir`、`% WebFetch url`、`⚙ tool` | verb（bold 名）：`bash cmd`、`read path:10-40`、`edit path`、`write path` | verb：通用动词（运行 / 读取 / 搜索 / 修改 / 写入 / 网页 / MCP / 子代理 / 提问 / 待办） |
| verbs（shell/read/search/edit/write/web/mcp/agent/ask/todo） | 原工具名 | Ran / Read / Search·List / Edited / Added / Searched the web / Called / Spawned agent / Asked / Updated plan | Shell / ReadFile / Search / Edit / WriteFile / WebFetch·GoogleSearch / MCP / Agent / Ask / Todos | `$` / Read / Grep·Glob / Edit / Wrote / WebFetch / tool / Task / Asked / Todos | bash / read / grep / edit / write / fetch / — / — / — / — | i18n 通用动词 |
| merge.readSearchRuns | false | **true**（Explored：连续 read/search/list 且全部成功） | false | false | false | false |
| merge.mcpSameServerRuns | **true**（`Called slack 3 times`） | false | false | false | false | false |
| showDiffCounts | true（Claude 原生不显示计数，但 structuredPatch 有数据；统一显示，差异只是动词） | true | true | true | true | 有数据则显示 |
| showStepCost | false | false | false | **true** | false | false |
| showExitCode | nonzero | nonzero（`(exit N)`） | nonzero | nonzero | nonzero | nonzero |
| monoTitles | true | false（Codex 用正文字体，命令部分 mono） | true | true | true | true |

> 统一的部分（不随 Agent 变）：行高 32px、步骤缩进 24px、折叠摘要行样式、展开动画、预览行数、失败常显、颜色语义（success/warning/danger 用现有 token）、图片与链接处理、键盘交互。

### 6.7 颜色 token（`src/index.css` 新增，深浅两套）

| token | 浅色 | 深色 | 备注 |
|---|---|---|---|
| `--agent-claude` | `#c2613f` | `#e08a6a` | Anthropic 品牌陶土色 `#D97757` 调深/调亮以过 4.5:1（作文字用）；竖条用原值即可 |
| `--agent-codex` | `#3f3f46` | `#d4d4d8` | Codex TUI 用终端默认色，取中性灰 |
| `--agent-gemini` | `#3367d6` | `#8ab4f8` | Google 蓝 |
| `--agent-opencode` | `#0f766e` | `#5eead4` | OpenCode 品牌主色（**待核实**，若无品牌色就用 route 色系） |
| `--agent-pi` | `#7c3aed` | `#c4b5fd` | pi 品牌色（**待核实**） |
| `--agent-generic` | `var(--text-2)` | `var(--text-2)` | |
| `--diff-add-bg` / `--diff-del-bg` | `var(--success-soft)` / `var(--danger-soft)` | 同 | diff 行背景 |

tailwind 暴露为 `text-agent-*` / `bg-agent-*` / `border-agent-*`（`tailwind.config.cjs` colors.agent.{claude,…}）。所有文字色对 `--bg-app`、`--bg-subtle` 的对比度 ≥ 4.5:1（`better-colors` 技能或 `pnpm` 脚本校验，P4 验收）。

### 6.8 i18n 文案键（`sessionManager.reader.*`，zh / zh-TW / en / ja 四套）

```
reader.timelineSummary        "执行过程 · {{steps}} 步"
reader.summaryCommands        "{{count}} 个命令"
reader.summaryFiles           "改了 {{count}} 个文件"
reader.summaryErrors          "{{count}} 个失败"
reader.summaryDuration        "⏱ {{duration}}"
reader.expandTimeline / collapseTimeline      "展开执行过程" / "收起执行过程"
reader.expandAllTimelines     "展开全部过程"
reader.moreFailures           "还有 {{count}} 个失败"
reader.stepExpand / stepCollapse              "展开 {{title}} 的详情" / "收起"
reader.status.success / error / interrupted / pending / unknown   "成功" / "失败" / "已中断" / "进行中" / "未知"
reader.exitCode               "exit {{code}}"
reader.lines                  "{{count}} 行"
reader.moreLines              "还有 {{count}} 行"
reader.showAll / loadMore     "显示全部" / "加载更多"
reader.tooLarge               "内容超过 2 MB，请复制源文件路径后在本地查看"
reader.params                 "参数"
reader.output                 "输出"
reader.diff                   "改动"
reader.diffCounts             "+{{added}} −{{removed}}"
reader.thinking.claude / codex / gemini / opencode / pi / generic   "思考中…" / "推理" / "想法" / "Thought" / "Thinking…" / "思考"
reader.thinkingMeta           "{{chars}} 字 · {{duration}}"
reader.thinkingRedacted       "思考内容不可见"
reader.note                   "说明"
reader.finalReply             "回复"
reader.noFinalReply           "这一轮没有最终回复"
reader.turnAborted            "已中断"
reader.event.compaction       "上下文已压缩"
reader.event.modelChange      "模型切换为 {{model}}"
reader.event.thinkingLevel    "思考强度：{{level}}"
reader.event.hookError        "Hook 执行出错"
reader.event.prLink           "已关联 PR #{{number}}"
reader.event.slashCommand     "运行了 {{command}}"
reader.event.subAgent         "子代理 {{name}}"
reader.injectedToggle         "显示注入的上下文"
reader.injectedRow            "上下文 · {{label}} · {{chars}} 字"
reader.image.alt              "图 {{index}}"
reader.image.open             "放大查看"
reader.image.close            "关闭图片"
reader.image.copy             "复制图片"
reader.image.failed           "无法加载图片"
reader.image.loadRemote       "加载远程图片"（沿用 PR）
reader.link.openExternal      "在浏览器中打开 {{url}}"
reader.path.copy / reveal     "复制路径" / "在 Finder 中显示"
reader.merged.explored        "查看了 {{files}} 个文件、搜索 {{searches}} 次"
reader.merged.mcpTimes        "调用 {{server}} {{count}} 次"
reader.turnsCount             "{{turns}} 轮 · {{messages}} 条"
reader.loadingProgress        "已加载 {{loaded}} / {{total}}"
reader.verb.*                 generic 动词：run / read / search / edit / write / web / mcp / agent / ask / todo / other
reader.copyTurn               "复制这一轮"
reader.copyWithThinking       "复制时包含思考"
```

Agent 原生动词（Ran / Explored / Edited / Called / Shell / Wrote / Thought…）作为**品牌固定字面**不翻译，放在 `agentStyles.ts`，不进 i18n。

### 6.9 可访问性

- 步骤行是 `<button aria-expanded aria-controls>`；折叠摘要行同理；展开区 `role="region" aria-label`。
- 状态不只靠颜色：符号 + `sr-only` 文字（`reader.status.*`）。
- 时间线左侧竖线 `aria-hidden`；glyph `aria-hidden`，角色由文字提供。
- 键盘：`j`/`k` 在轮之间跳（已有 ‹ › 是会话间）；`Enter`/`Space` 展开；`Esc` 关闭查找/灯箱；焦点环用 `focus-visible:ring-2 ring-ring`。
- 图片有 `alt`；灯箱是 `Dialog`（焦点陷阱、Esc）。
- `prefers-reduced-motion` 下不做展开动画。
- 文本对比度：`text-fg-3` 只用于非关键元信息。

---

## 7. 性能预算与验证

### 7.1 预算

| 指标 | 55MB Claude（2068 条） | 177MB Codex（预计 ~400 条） | 一般会话（< 5MB） |
|---|---|---|---|
| 后端解析（无缓存，release） | ≤ 150ms | ≤ 800ms（必须读完 160MB 字串；若超，考虑对 `custom_tool_call_output` 行用 `memchr` 找 `"output":` 后只解码前 64KB——二期） | ≤ 30ms |
| 缓存命中到首包 | ≤ 20ms | ≤ 20ms | ≤ 10ms |
| 首包（Header + 首批消息）到达并渲染 | ≤ 300ms（点击起算） | ≤ 600ms | ≤ 100ms |
| 全量 payload（序列化 JSON） | ≤ 1.5MB | ≤ 1MB | — |
| 单条 `SessionMessage` 序列化 | ≤ 8KB（预览上限约束） | 同 | 同 |
| 图片随消息下发 | 0 字节（只传 ref） | 同 | 同 |
| 滚动 | 稳定 60fps；单行渲染 ≤ 4ms；Markdown 解析仅对可见行、单块 ≤ 64KB 否则退化为纯文本 | | |
| 展开 1MB 工具输出 | ≤ 150ms 到可见 | | |
| 前端内存（消息状态） | ≤ 60MB | ≤ 40MB | |

预览参数推导：55MB 会话约 1000 个工具结果 × ≤ 1.2KB 预览 ≈ 1.2MB 上限，实际多数输出短于预览，预计 600–900KB；加 text/thinking 预览 ≈ 1.3MB，在 1.5MB 内。若实测超出，先把 `PREVIEW_CHARS` 降到 800。

### 7.2 验证方法

- Rust：`src-tauri/src/session_manager/bench.rs` 加 `#[ignore]` 测试 `bench_parse_env_file`，读环境变量 `CC_SWITCH_BENCH_FILE` / `CC_SWITCH_BENCH_PROVIDER`，输出解析耗时、消息数、块数统计、`serde_json::to_vec` 长度、最大单条长度。用两份大文件各跑 3 次取中位数，结果写进 PR 描述。
- Rust 单测：每家解析器用 fixture 验证块类型、配对 id、预览截断、`ContentRef` 可回取（`get_block_content` 对 fixture 的 ref 取出全文与原文相等）。
- 前端：开发模式下 `performance.mark("session-reader:click")` → `"first-chunk"` → `"first-paint"`（`requestAnimationFrame` 后）→ `"done"`，`console.debug` 输出；手工对两份大文件记录。
- 前端单测（vitest）：`turns.ts`（配对、孤儿结果、最终回复判定、合并规则、折叠自动展开条件）、`toolSummary.ts`（五家标题格式）、`SessionMarkdown`（PR 自带 453 行测试迁移 + 链接拦截 + file:// 处理）、`SessionStep`（失败常显、展开取全文的 mock）、`SessionManagerPage.test.tsx` 既有用例改为新 fixture。
- 可访问性：`better-accessibility`/`design:accessibility-review` 过一遍；深浅色各截图一份放 PR。

---

## 8. 分阶段实施计划（任务包）

原则：**P0 先定 schema**，之后后端与前端以 fixture 为契约并行；每个任务包一次或多次提交到 `feat/ui-redesign-v9`，不推送；不碰数据库。

### P0 契约与 fixture（串行，1 个代理，先做）

- 文件：`src-tauri/src/session_manager/model.rs`（§3.1 全部类型 + `project_content`）、`src-tauri/src/session_manager/providers/blocks.rs`（`normalize_tool`、`preview()`、`title_*()` 骨架与常量）、`src/types.ts`（§3.2）、`tests/fixtures/sessions/{claude,codex,gemini,opencode,pi,generic}.messages.json`（手写 6 份小型「解析结果」fixture，覆盖每种块与状态）、`docs/session-reader-redesign.md` 的 §3 若有调整同步。
- 输入/输出：本文件 §3；输出可编译的类型 + fixture；旧 `SessionMessage` 字段不删。
- 验收：`cargo check`、`pnpm typecheck` 通过；fixture 能被 `src/types.ts` 的类型 `satisfies`（写一个 `tests/types/sessionFixtures.test.ts` 做类型断言）。
- 依赖：无。

### P1a 后端 · Claude 解析器（并行）

- 文件：`providers/claude.rs`、`providers/blocks.rs`（Claude 需要的 title 函数）、`providers/utils.rs`（`read_until` 带偏移的行迭代器 `LineSpans`）。
- 契约：§4.1；旧测试全部保留并追加：thinking redacted、tool_result 图片 ref、Edit diff 回填、system-reminder 剥除、slash command 事件、`[Tool: X] title` 投影。
- 验收：`cargo test claude`；对本机 55MB 文件 `bench_parse_env_file` 结果写入 PR 描述（解析 ≤ 150ms、payload ≤ 1.5MB）。
- 依赖：P0。

### P1b 后端 · Codex 解析器重写（并行）

- 文件：`providers/codex.rs::load_messages` 及新 `providers/codex_items.rs`（item_completed 状态机）。
- 契约：§4.2；fixture 覆盖：exec + CommandExecution 配对、exec 正则兜底、apply_patch 计数、FileChange unified_diff、function_call namespace→MCP、web_search_call、reasoning summary、compaction、turn_aborted、developer 注入、input_image。
- 验收：`cargo test codex`；177MB 文件解析出的消息数 ≥ 300（现在 79）、工具结果数 ≥ 84、payload ≤ 1MB、解析 ≤ 800ms。
- 依赖：P0。

### P1c 后端 · Gemini / OpenCode / Pi（并行）

- 文件：`providers/gemini.rs`、`providers/opencode.rs`（三条读取路径统一走 `opencode_blocks.rs`）、`providers/pi.rs`（`parse_message` → blocks）、`providers/openclaw.rs` 复用 pi_blocks。
- 契约：§4.3–4.5；现有测试保留并扩充（OpenCode tool state、step-finish、reasoning；Pi thinking/toolCall/toolResult/bashExecution/model_change）。
- 验收：`cargo test gemini opencode pi openclaw`。
- 依赖：P0。

### P1d 后端 · Hermes / Grok / MCode 降级（并行，小）

- 文件：`providers/hermes.rs`、`providers/grokbuild.rs`、`providers/mcode.rs`。
- 契约：§4.6；tool_calls 配对；MCode 只有 Text。
- 验收：现有测试通过 + 新增配对测试。
- 依赖：P0。

### P2 后端 · 命令、缓存、安全（并行）

- 文件：`session_manager/cache.rs`（`TranscriptCache`）、`session_manager/content.rs`（`resolve_content_ref`、`load_image`、`validate_source`）、`commands/session_manager.rs`（`stream_session_messages`、`get_session_block_content`、`get_session_image`、可选 `reveal_session_path`）、`commands/misc.rs::open_external` 白名单、`lib.rs` 注册命令、`session_manager/mod.rs`（`load_transcript` 返回 `Arc<Transcript>`，抽出 `validate_source`）。
- 契约：§5；chunk 规则（≤ 256KB / 150 条）；ContentRef 校验与错误文案。
- 验收：单测——缓存命中/失效（改 mtime）、LRU 淘汰、Jsonl ref 越界/非完整行/指针不是字符串被拒、Sidecar/File 目录穿越被拒、图片大小上限、`open_external("javascript:…")` 被拒；`cargo clippy` 无警告。
- 依赖：P0（类型）。与 P1 无代码耦合（通过 `SessionMessage` 结构）。

### P3a 前端 · 数据层与纯函数（并行）

- 文件：`src/lib/api/sessions.ts`、`src/lib/query/sessions.ts`（`useSessionTranscript`、`useBlockContent`、`useSessionImage`；从 `queries.ts` 迁出 `useSessionMessagesQuery`）、`src/components/sessions/reader/turns.ts`、`toolSummary.ts`、`agentStyles.ts`、`src/index.css` + `tailwind.config.cjs`（`--agent-*` token）。
- 契约：§3.2、§6.2、§6.6、§6.7；输入是 P0 fixture。
- 验收：vitest 覆盖 `turns.ts`/`toolSummary.ts` 100% 分支；`pnpm typecheck`。
- 依赖：P0。

### P3b 前端 · Markdown 迁入（并行）

- 文件：`src/components/sessions/reader/SessionMarkdown.tsx`（`git show fix/fix-6325-session-markdown-rendering:src/components/sessions/SessionMarkdown.tsx` 为底本）、`SessionCodeBlock.tsx`、`PathChip.tsx`、`tests/components/SessionMarkdown.test.tsx`（迁移 PR 的 453 行测试并改类名断言）。
- 改动点：类名换 v7 token；`<a>` 不带真实 `href` 导航，`onClick` → `open_external`，`title` 显示 URL；`file://` 与 `![](本地路径)` → `PathChip` / `SessionImage(LocalFile)`；行内 `code` 与 `CodeBlock` 共用样式；标题层级映射；性能边界：`content.length > 65536` 直接退化为「围栏代码 + 纯文本」。
- 验收：PR 原测试全绿 + 新增：链接点击不导航且调用 `open_external`、`javascript:` 不可点、`file://` 变 PathChip、超长内容退化。
- 依赖：P0（仅类型）。

### P3c 前端 · 阅读页组件与集成（串行，等 P3a、P3b）

- 文件：`reader/SessionReader.tsx`、`SessionQuestion.tsx`、`SessionTimeline.tsx`、`SessionStep.tsx`、`SessionStepDetails.tsx`、`SessionThinkingRow.tsx`、`SessionNoteRow.tsx`、`SessionFinalReply.tsx`、`SessionEventRow.tsx`、`SessionDiff.tsx`、`SessionImage.tsx`、`SessionImageLightbox.tsx`、`SessionToc.tsx`（改用 `TurnIndex`）、`SessionManagerPage.tsx`（接 `useSessionTranscript`、删除消失的 import）、`src/i18n/locales/{zh,zh-TW,en,ja}.json`（§6.8）、删除旧 `SessionMessageItem.tsx` 与 `utils.ts` 中迁走的函数。
- 契约：§6 全部；折叠规则 §6.5 逐条对应测试。
- 验收：`tests/components/SessionReader.test.tsx`（新）：五家 fixture 各渲染一次快照级断言（glyph、动词、合并）、失败常显、展开取全文（mock `get_session_block_content`）、图片懒加载（mock `get_session_image`）、查找命中自动展开、只看对话、复制 Markdown 含步骤、键盘展开；`SessionManagerPage.test.tsx` 既有 19 个用例全部通过；`pnpm format:check`。
- 依赖：P3a、P3b；运行态依赖 P1/P2 完成（可先用 MSW mock）。

### P4 集成、性能与收尾（串行，最后）

- 文件：`session_manager/bench.rs`、PR 描述、`docs/session-reader-redesign.md` 的「实施结果」小节。
- 工作：两份大文件跑 §7 全部指标并记录；深浅色截图；`better-accessibility` 走查；`cargo fmt && cargo clippy`、`pnpm typecheck && pnpm test:unit && pnpm format`；若 payload 超预算调 `PREVIEW_CHARS`。
- 验收：§7.1 表全部达标或写明偏差与原因。
- 依赖：全部。

### 提交切分建议

`P0` 1 次；`P1a/b/c/d` 各 1 次；`P2` 1–2 次（命令、缓存分开）；`P3a` 1 次；`P3b` 1 次（保留 PR 作者的 Co-Authored 由项目惯例决定，不加 AI 署名）；`P3c` 2–3 次（组件、集成、i18n）；`P4` 1 次。

---

## 9. 需要拍板的决策

| # | 问题 | 选项 | 推荐 |
|---|---|---|---|
| D1 | 首屏增量方式 | A. `tauri::ipc::Channel` 分块流式；B. 分页命令 `get_session_messages_page(offset, limit)` + 前端稀疏虚拟化；C. 保持一次性返回，只靠预览瘦身 | **A**。实现简单、首屏最快，payload 总量与 C 相同；B 复杂度高，留作 5000+ 条会话的二期 |
| D2 | 图片传输 | A. `get_session_image` 返回原始字节（`ipc::Response`）→ Blob URL；B. 后端解码到临时目录 + `assetProtocol` scope 放开 `$TEMP/cc-switch/images/**` + `convertFileSrc`；C. base64 data URL 字串 | **A**。不改 `tauri.conf.json` 的协议范围，不落盘；单图 ≤ 20MB 走 IPC 可接受 |
| D3 | 本地文件路径 | A. 仅显示 + 复制；B. A + 「在 Finder 中显示」（新增 `reveal_session_path`，opener 插件 `reveal_item_in_dir`）；C. B + 「用默认程序打开」 | **B**。C 会打开任意文件，风险不对等 |
| D4 | Agent 主题色 | A. 品牌色（§6.7 表）；B. 全部中性，只靠符号与动词区分；C. 品牌色只用于提问竖条和 glyph，正文全中性 | **C**。满足「整体风格一致」又能一眼辨认；品牌色准确值待核实后微调 |
| D5 | 最后一轮的执行过程 | A. 同其它轮，默认折叠；B. 最后一轮默认展开 | **A**。规则简单；中断/无回复已自动展开，覆盖「看它卡在哪」的场景 |
| D6 | 思考块默认 | A. 一行摘要（字数+时长），点开全文；B. 显示前 2 行预览；C. 完全隐藏，仅菜单开关 | **A**；Codex/Gemini 有 summary 时在该行显示 summary 文本 |
| D7 | 用户提问是否渲染 Markdown | A. 纯文本 + 围栏代码；B. 完整 Markdown | **A**。用户输入常含 `#`、`*`、路径等，Markdown 会误渲染 |
| D8 | `content` 投影中是否保留 `[Tool: X]` 行 | A. 保留（旧测试、旧 fixture 兼容）；B. 去掉，`content` 只含对话文本 | **A**，并追加 title；一期结束、旧前端代码删除后再评估 B |
| D9 | 预览尺寸 | 12 行 / 1200 字（§4 常量）；或 8 行 / 800 字 | 先 **12/1200**，P4 实测超预算再降 |
| D10 | 注入内容（AGENTS.md、developer、system-reminder） | A. 默认隐藏 + 菜单开关显示；B. 默认折叠显示为灰行 | **A** |
| D11 | PR #6332 的引入方式 | A. `git cherry-pick` 其 8 个提交再改；B. 只拿 `SessionMarkdown.tsx` 与测试文件作为底本手工迁入 | **B**。PR 其余改动（旧 `SessionMessageItem` 样式）与 v7 冲突；在提交说明里注明来源 PR 与作者 |
| D12 | Codex 命令配对失败时 | A. 退回正则取 `cmd`；B. 显示 JS 源首行 | **A→B** 级联（§4.2 已写） |
| D13 | OpenCode 每步费用 | A. 步骤元信息显示 tokens/cost（`showStepCost=true`）；B. 只在轮摘要显示合计 | **A + B**：步骤显示、摘要合计 |
| D14 | `thinking` 是否进入「复制整段 Markdown」 | 默认不含，菜单勾选可含 | 同 |

---

## 附录 A：本机数据统计速查

- Claude 最近 120 会话：tool_use 13500、tool_result 13498、thinking 9270（大量 `thinking:""` 仅签名）、assistant text 2290、user image 53、tool_result 内 image 524；顶层 attachment 14183、last-prompt 2625、custom-title 2611、queue-operation 2407、system 949、pr-link 711。
- 当前会话（6MB、2013 行）字节分布：tool_result 858KB、image 931KB、thinking 569KB、text 165KB；最大单条 tool_result 93KB；> 8KB 的 tool_result 19 条。
- Codex 最近 40 会话：custom_tool_call(exec) 823、function_call 220(js)+62(exec_command)+11(request_user_input_async)+…、reasoning 1038、message 824、web_search_call 42、compaction 10、turn_aborted 2；item_completed：CommandExecution 1079、Reasoning 875、AgentMessage 415、McpToolCall 220+20+1、WebSearch 42、FileChange 31+1、ImageView 21、ContextCompaction 10。
- Codex 177MB 文件：878 行，最大单行 9.0MB；`custom_tool_call_output` 84 条合计 160MB；`compacted` 3 条 10MB。
- OpenCode db：part 类型 tool 149、text 138、step-start 133、step-finish 130、reasoning 88、agent 6、patch 1。
- 本机无 Hermes / OpenClaw / Grok Build / MiniMax Code 会话样本。

## 附录 B：已核实的原生 UI 字面来源

- Codex：`codex-rs/tui/src/exec_cell/render.rs`（Ran / Running / Explored / Read / Search / List、`•`、`  └ `、`TOOL_CALL_MAX_LINES = 5`、`(exit N)`）、`diff_render.rs`（Added / Deleted / Edited、`(+N -M)`、`└ `、`⋮`）、`history_cell/search.rs`（Searched the web / Searched for / Opened page）、`history_cell/mcp.rs`（Called / Calling、`server.tool(args)`）、`history_cell/messages.rs`（`› ` 用户、`• ` 助手、reasoning dim italic）、`history_cell/patches.rs`（`✘ Failed to apply patch`、`Viewed image`）。
- Gemini CLI：`packages/cli/src/ui/constants.ts`（`TOOL_STATUS`：✓ o ⊷ ? - x）、`components/messages/GeminiMessage.tsx`（`✦ `）、`UserMessage.tsx`（`> `）、`ToolMessage.tsx`（round border 左侧、bold name + secondary description）、`ToolShared.tsx`（`STATUS_INDICATOR_WIDTH = 3`）。
- OpenCode：`packages/tui/src/routes/session/index.tsx`（`$`、`→ Read`、`← Wrote`、`← Edit`、`✱ Grep/Glob`、`% WebFetch`、`◈`、`│/✓/✗ Task`、`⚙`、`~`、`Thought`；shell 10 行、通用 3 行）、`util/collapse-tool-output.ts`。
- Pi：`packages/coding-agent/src/core/tools/renderers/bash.ts`（`BASH_PREVIEW_LINES = 5`、`Took …`、`(timeout Ns)`）、`renderers/read.ts`（10 行预览、`:start-end`）、`renderers/edit.ts`（bold `edit` + path、diff）、`modes/interactive/components/tool-execution.ts`（`FALLBACK_PREVIEW_LINES = 10`、`toolPendingBg/toolSuccessBg/toolErrorBg`）、`assistant-message.ts`（`Thinking...` 斜体、`Operation aborted`）。
- Claude Code：官方文档 interactive-mode（`Ctrl+O` transcript viewer、`Called slack 3 times` 折叠）；`⏺` / `⎿` / `✻ Thinking…` 为常识，**待核实**。
