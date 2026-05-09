# DeepSeek Anthropic 兼容代理层设计

**日期：** 2026-05-09（v2，参考 free-claude-code 修订）
**范围：** Claude Code → DeepSeek（Anthropic 兼容端点）无错代理
**集成方式：** 扩展现有 Claude 提供商，新增 `deepseek_anthropic` api_format

---

## 背景

DeepSeek 提供 Anthropic Messages API 兼容端点（`https://api.deepseek.com/anthropic/v1/messages`），可直接接收 Anthropic 格式请求。但 Claude Code 发出的请求包含多项 DeepSeek 不支持的内容，会触发 400/422 错误：

| 问题 | 触发原因 |
|------|----------|
| `redacted_thinking` 历史块 | Claude Code 缓存的加密思考块，DeepSeek 不认识 |
| `image` / `document` 块 | DeepSeek 无视觉/文档能力 |
| `server_tool_use` / `web_search_tool_result` / `web_fetch_tool_result` 块 | Anthropic 服务端工具，DeepSeek 不支持 |
| `tool_result.content` 为数组 | DeepSeek 要求该字段为字符串 |
| `mcp_servers` 顶层字段 | DeepSeek 不支持 |
| `output_config` 顶层字段 | Anthropic 扩展字段，DeepSeek 不认识 |
| `context_management.edits` 含 `clear_thinking_*` | Anthropic 内部字段 |
| Anthropic server-listed tools（`web_search`/`web_fetch`） | DeepSeek Anthropic 端点不支持 |
| `tool_result` 与 `tool_use` 不相邻 | DeepSeek 要求二者紧邻，Claude Code 多轮对话可能中间插入 user 消息 |
| `anthropic-beta` 请求头 | DeepSeek 遇到不认识的 beta 头可能拒绝请求 |
| assistant 消息中的 `reasoning_content` 字段 | DeepSeek 自身注入的字段，回放到历史时触发 400 |
| 响应中模型名为 `deepseek-*` | Claude Code 校验模型名，不匹配则报错 |

目标：在 cc-switch 代理层自动拦截并修正全部不兼容之处，让用户无感知地使用 DeepSeek 作为 Claude Code 后端。

---

## 架构决策

**选择：** 在现有 Claude 提供商内新增 `"deepseek_anthropic"` api_format
**理由：** DeepSeek 使用 Anthropic 原生格式，不需要协议转换（不像 `openai_chat`），只需净化层。复用现有透传路径，改动最小。

---

## 改动详情

### 1. 后端 Rust

#### 新文件：`src-tauri/src/proxy/providers/deepseek_anthropic.rs`

负责所有 DeepSeek 特定的转换逻辑，分两个公开函数：

---

**`sanitize_request(body: &mut Value, target_model: &str, thinking_enabled: bool) -> String`**

返回保存的原始 `model` 值（fake_model），按以下顺序处理：

**① 保存并覆写 model**
- 读取 `body["model"]`，保存为 `fake_model`（供响应 patch 用）
- 将 `body["model"]` 替换为 `target_model`（由调用方从 `provider.settings_config["target_model"]` 读取后传入）

**② 删除不支持的顶层字段**
- 删除：`output_config`、`mcp_servers`
- 根据 `thinking_enabled`：
  - `false`：删除 `thinking` 字段
  - `true`：保留 `thinking` 字段（DeepSeek Anthropic 端点支持）

**③ 过滤 `tools` 数组中的 server-listed tools**
- 检测 `tools[].type` 为 `web_search` 或 `web_fetch` → 从数组中删除，记录 warn 日志
- 其余工具原样保留（不做类型重映射）

**④ 过滤 `context_management.edits`**
- 删除 `edits` 数组中 `type` 以 `clear_thinking_` 开头的条目
- 若 `edits` 变为空数组则删除整个 `context_management`

**⑤ 净化消息历史 `messages[].content`**

对每条消息的 `content` 数组执行以下过滤（仅处理 `role == "assistant"` 的 thinking/redacted_thinking；对所有 role 处理附件块）：

| 块类型 | `thinking_enabled=false` | `thinking_enabled=true` |
|--------|--------------------------|-------------------------|
| `thinking`（有 `thinking` 字段） | 删除 | 保留 |
| `redacted_thinking` | 删除 | 删除 |
| `image` / `document` | 删除，替换为占位文字块 | 删除，替换为占位文字块 |
| `server_tool_use` / `web_search_tool_result` / `web_fetch_tool_result` | 删除 | 删除 |
| `tool_result` | 递归过滤其 `content`，并将 `content` 数组序列化为字符串 | 同左 |
| `reasoning_content` 字段（assistant 消息顶层） | 删除 | 删除 |
| 其余 | 原样保留 | 原样保留 |

- 占位文字：`"[attachment omitted: DeepSeek does not support image or document inputs]"`
- `tool_result.content` 为数组时：拼接各 `text` 块为字符串（非文本块忽略）
- 过滤后若消息 `content` 为空数组，替换为 `[{"type":"text","text":"..."}]`
- `reasoning_content`：DeepSeek 会在响应 assistant 消息中注入该字段；当这些消息进入下一轮历史时需删除，否则触发 400

**⑥ Tool 消息顺序修复（`repair_tool_message_order`）**

DeepSeek 要求 `tool_result`（role=user，type=tool_result）必须紧跟对应的 `tool_use` assistant 消息。Claude Code 多轮对话可能在二者之间插入其他 user 消息，导致 422。

处理逻辑：
- 遍历 messages，找到 assistant 消息中的每个 `tool_use` 块，记录 `tool_use_id`
- 检查其后第一条非 tool_result 消息是否出现在 tool_result 之前
- 若顺序错乱，将 tool_result 移到对应 assistant 消息之后
- 若某 `tool_use_id` 完全没有对应 tool_result，插入合成占位：`{"type":"tool_result","tool_use_id":"...","content":"[no result]","is_error":false}`

**⑦ 确保 `max_tokens` 存在**
- 若 `body["max_tokens"]` 为 null 或不存在，设为默认值 `8192`

**⑧ 强制 `stream: true`**

---

**`patch_sse_chunk(chunk: &str, fake_model: &str) -> String`**

对每个 SSE 文本块做字符串替换（不解析 JSON，性能优先）：

- `"model":"<任意值>"` → `"model":"<fake_model>"`（正则：`"model"\s*:\s*"[^"]+"`)
- `"type":"redacted_thinking"` → `"type":"text"`

> **不**将 `thinking` 类型替换为 `text`：当 `thinking_enabled=true` 时，思考块在响应流中是合法的，Claude Code 能正常处理。

---

#### 修改：`src-tauri/src/proxy/providers/claude.rs`

- `get_claude_api_format()`：新增 `"deepseek_anthropic"` 分支
- `claude_api_format_needs_transform()`：`"deepseek_anthropic"` 返回 `false`

---

#### 修改：`src-tauri/src/proxy/providers/mod.rs`

- `pub mod deepseek_anthropic;`

---

#### 修改：`src-tauri/src/proxy/forwarder.rs`

在 `forward()` 的请求构造阶段，当 `api_format == "deepseek_anthropic"` 时：

1. 从 `provider.settings_config["target_model"]` 读取目标模型名，缺省 `"deepseek-chat"`
2. 判断 `thinking_enabled`：检查 `body["thinking"]["type"] == "enabled"`
3. 调用 `deepseek_anthropic::sanitize_request(&mut body, &target_model, thinking_enabled)` → 得到 `fake_model`
4. 将 `fake_model` 存入 `ForwardResult.deepseek_fake_model`
5. **剥离 Anthropic 专属请求头**，避免 DeepSeek 因不认识这些头而拒绝请求：
   - `anthropic-beta`（如 `interleaved-thinking-2025-05-14`、`prompt-caching-2024-07-31` 等）
   - `anthropic-dangerous-direct-browser-access`
   - 保留：`x-api-key`、`anthropic-version`、`authorization`、`content-type`

---

#### 修改：`src-tauri/src/proxy/forwarder.rs`（ForwardResult）

```rust
pub struct ForwardResult {
    pub response: ProxyResponse,
    pub provider: Provider,
    pub claude_api_format: Option<String>,
    pub deepseek_fake_model: Option<String>,  // 新增
}
```

---

#### 修改：`src-tauri/src/proxy/providers/streaming.rs`

当 `ForwardResult.deepseek_fake_model.is_some()` 时，对每个 SSE chunk 调用：
```rust
deepseek_anthropic::patch_sse_chunk(chunk, &fake_model)
```

---

### 2. 前端 TypeScript

#### 修改：`src/types.ts`

```ts
export type ClaudeApiFormat =
  | "anthropic"
  | "openai_chat"
  | "openai_responses"
  | "gemini_native"
  | "deepseek_anthropic";  // 新增
```

#### 修改：`src/config/claudeProviderPresets.ts`

```ts
{
  name: "DeepSeek V4 Flash",
  websiteUrl: "https://platform.deepseek.com",
  apiKeyUrl: "https://platform.deepseek.com/api_keys",
  settingsConfig: {
    env: { ANTHROPIC_API_KEY: "" },
    baseURL: "https://api.deepseek.com/anthropic",
    api_format: "deepseek_anthropic",
    target_model: "deepseek-chat",   // 实际发给 DeepSeek 的模型名
    fake_model: "claude-sonnet-4-6", // Claude Code 看到的伪装模型名
  },
  apiFormat: "deepseek_anthropic",
  apiKeyField: "ANTHROPIC_API_KEY",
  category: "cn_official",
  endpointCandidates: ["https://api.deepseek.com/anthropic"],
},
{
  name: "DeepSeek V4 Pro",
  websiteUrl: "https://platform.deepseek.com",
  apiKeyUrl: "https://platform.deepseek.com/api_keys",
  settingsConfig: {
    env: { ANTHROPIC_API_KEY: "" },
    baseURL: "https://api.deepseek.com/anthropic",
    api_format: "deepseek_anthropic",
    target_model: "deepseek-reasoner", // 实际发给 DeepSeek 的模型名
    fake_model: "claude-opus-4-7",     // Claude Code 看到的伪装模型名
  },
  apiFormat: "deepseek_anthropic",
  apiKeyField: "ANTHROPIC_API_KEY",
  category: "cn_official",
  endpointCandidates: ["https://api.deepseek.com/anthropic"],
},
```

#### 修改：`src/i18n/locales/{zh,en,ja}.json`

各新增 1 个 key 说明自动过滤行为。

---

## 数据流

```
Claude Code（使用伪装模型名 claude-sonnet-4-6 / claude-opus-4-7）
    │ POST /v1/messages
    │   headers: anthropic-beta, x-api-key, ...
    │   model: "claude-sonnet-4-6"
    │   messages: [... tool_use ... (user msg) ... tool_result ...]
    │   tools: [{type:"web_search",...}, {type:"function",...}]
    ▼
cc-switch proxy — forwarder
    │ 剥离 anthropic-beta 等专属头
    │ sanitize_request(thinking_enabled=false):
    │   ① model 保存为 fake_model，替换为 "deepseek-chat"
    │   ② 删 output_config / mcp_servers / thinking
    │   ③ tools 中 web_search 删除
    │   ④ 消息历史：redacted_thinking/image/document 过滤
    │      assistant.reasoning_content 字段删除
    │   ⑤ tool_result.content 数组→字符串
    │   ⑥ repair_tool_message_order(): 修复 tool 配对顺序
    │   ⑦ max_tokens=8192，stream=true
    ▼
DeepSeek API https://api.deepseek.com/anthropic/v1/messages
    │ SSE 响应（model: "deepseek-chat"）
    ▼
cc-switch proxy — streaming
    │ patch_sse_chunk(fake_model="claude-sonnet-4-6"):
    │   "model":"deepseek-chat" → "model":"claude-sonnet-4-6"
    │   "type":"redacted_thinking" → "type":"text"
    ▼
Claude Code（model 名匹配，校验通过，工具调用正常）
```

**伪装映射：**

| 预设 | Claude Code 看到 | 实际请求 DeepSeek |
|------|-----------------|-----------------|
| DeepSeek V4 Flash | `claude-sonnet-4-6` | `deepseek-chat` |
| DeepSeek V4 Pro | `claude-opus-4-7` | `deepseek-reasoner` |

---

## 不改动的部分

- `handlers.rs`：models 伪装复用现有 `handle_claude_desktop_models` 逻辑
- `transform.rs` / `transform_responses.rs`：无协议转换，不涉及
- `forwarder.rs` 的重试/failover 逻辑：完全不动

---

## 测试要点

**`sanitize_request` 单元测试：**
- thinking 块过滤（thinking_enabled=false 删两种，true 只删 redacted）
- image/document 块替换为占位文字，嵌套在 tool_result 中也覆盖
- tool_result.content 数组→字符串序列化
- server-listed tools（web_search）从 tools 数组剥离
- context_management.edits clear_thinking_* 过滤
- mcp_servers / output_config 删除
- 空 content 数组插入占位块
- model 覆写、max_tokens 默认值
- assistant 消息中 reasoning_content 字段被删除

**`repair_tool_message_order` 单元测试：**
- tool_result 晚于对应 tool_use 且中间夹有 user 消息 → 正确重排
- tool_use 无对应 tool_result → 插入合成占位输出
- 正常顺序无变化

**请求头剥离测试：**
- anthropic-beta 头不出现在转发请求中
- x-api-key / anthropic-version 正常保留

**`patch_sse_chunk` 单元测试：**
- model 名正确替换
- redacted_thinking 类型替换为 text
- 正常 text/tool_use 块不受影响
- thinking 块不被替换（保留供 Claude Code 处理）

**集成验证：**
- Claude Code 连接 DeepSeek Flash/Pro 无 400/422
- 多轮含工具调用对话中消息顺序正确
- thinking 历史在多轮中正确过滤

---

## 文件变更清单

| 文件 | 操作 |
|------|------|
| `src-tauri/src/proxy/providers/deepseek_anthropic.rs` | 新建 |
| `src-tauri/src/proxy/providers/mod.rs` | 修改（声明模块） |
| `src-tauri/src/proxy/providers/claude.rs` | 修改（新增 format 分支） |
| `src-tauri/src/proxy/forwarder.rs` | 修改（净化请求、ForwardResult 新增字段） |
| `src-tauri/src/proxy/providers/streaming.rs` | 修改（chunk patch） |
| `src/types.ts` | 修改（ClaudeApiFormat 联合类型） |
| `src/config/claudeProviderPresets.ts` | 修改（新增两个预设） |
| `src/i18n/locales/zh.json` | 修改 |
| `src/i18n/locales/en.json` | 修改 |
| `src/i18n/locales/ja.json` | 修改 |
