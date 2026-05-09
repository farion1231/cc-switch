# DeepSeek Anthropic 兼容代理层设计

**日期：** 2026-05-09  
**范围：** Claude Code → DeepSeek（Anthropic 兼容端点）无错代理  
**集成方式：** 扩展现有 Claude 提供商，新增 `deepseek_anthropic` api_format

---

## 背景

DeepSeek 提供 Anthropic Messages API 兼容端点（`https://api.deepseek.com/anthropic/v1/messages`），可直接接收 Anthropic 格式请求。但 Claude Code 发出的部分字段（`thinking`、`output_config`、`redacted_thinking` 历史块）会导致 DeepSeek 返回 400 错误，且 DeepSeek 响应中的模型名会被 Claude Code 校验拒绝。

目标：在 cc-switch 代理层自动拦截并修正这些不兼容之处，让用户无感知地使用 DeepSeek 作为 Claude Code 后端。

---

## 架构决策

**选择：** 在现有 Claude 提供商内新增 `"deepseek_anthropic"` api_format  
**理由：** DeepSeek 使用 Anthropic 原生格式，不需要协议转换（不像 `openai_chat`），只需净化层。复用现有透传路径，改动最小。

---

## 改动详情

### 1. 后端 Rust

#### 新文件：`src-tauri/src/proxy/providers/deepseek_anthropic.rs`

负责所有 DeepSeek 特定的转换逻辑：

**请求净化（`sanitize_request`）：**
- 删除顶层字段：`output_config`、`thinking`
- 遍历 `messages[].content`，过滤掉 `type == "thinking"` 和 `type == "redacted_thinking"` 的块（这些是 Claude Code 缓存的历史思考块，DeepSeek 不认识）
- 过滤后若某条消息的 `content` 数组变为空，插入 `{"type":"text","text":"..."}` 占位，防止空数组报错
- 工具类型修正：`web_search` → `web_search_20250305`
- 覆写 `model` 字段为配置的目标模型（从 `settings_config.target_model` 读取，缺省 `deepseek-chat`）
- 保存原始 `model` 值，供响应流替换使用

**响应流 patch（`patch_sse_chunk`）：**
- 对每个 SSE 文本块做字符串替换（不解析 JSON，性能优先）：
  - `"model":"<任意值>"` → `"model":"<原始请求模型名>"`
  - `"type":"thinking"` → `"type":"text"`
  - `"type":"redacted_thinking"` → `"type":"text"`

#### 修改：`src-tauri/src/proxy/providers/claude.rs`

- `get_claude_api_format()`：新增 `"deepseek_anthropic"` 分支
- `claude_api_format_needs_transform()`：`"deepseek_anthropic"` 返回 `false`（走透传路径，不做协议转换）
- 新增 `prepare_deepseek_anthropic_request()`：调用 `deepseek_anthropic::sanitize_request()`，返回修改后的 body 和原始 model 名
- 新增 `patch_deepseek_anthropic_response_chunk()`：调用 `deepseek_anthropic::patch_sse_chunk()`

#### 修改：`src-tauri/src/proxy/providers/mod.rs`

- 声明 `pub mod deepseek_anthropic`

#### 修改：`src-tauri/src/proxy/forwarder.rs`

- 在 `forward()` 中，当 `api_format == "deepseek_anthropic"` 时：
  - 调用 `prepare_deepseek_anthropic_request()` 净化请求 body
  - 将原始 model 名透传给流处理层

#### 修改：`src-tauri/src/proxy/providers/streaming.rs`（或 response_processor）

- 当 `claude_api_format == Some("deepseek_anthropic")` 时，对每个 SSE chunk 调用 `patch_deepseek_anthropic_response_chunk()`

### 2. 前端 TypeScript

#### 修改：`src/types.ts`（或 `src/types/`）

```ts
export type ClaudeApiFormat =
  | "anthropic"
  | "openai_chat"
  | "openai_responses"
  | "gemini_native"
  | "deepseek_anthropic";  // 新增
```

#### 修改：`src/config/claudeProviderPresets.ts`

新增两个预设条目（flash 和 pro）：

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

各新增 1 个 key：`provider.deepseekAnthropicHint`，说明该模式会自动过滤不兼容字段。

---

## 数据流

```
Claude Code（配置伪装模型名，如 claude-sonnet-4-6 / claude-opus-4-7）
    │ POST /v1/messages (model: "claude-sonnet-4-6")
    ▼
cc-switch proxy
    │ 1. 检测 api_format == "deepseek_anthropic"
    │ 2. sanitize_request():
    │    - 保存原始 model 名 "claude-sonnet-4-6" 作为 fake_model
    │    - 将 model 替换为 target_model（如 "deepseek-chat"）
    │    - 删 output_config / thinking 顶层字段
    │    - 过滤消息历史里 type=thinking/redacted_thinking 的块
    │    - web_search → web_search_20250305
    ▼
DeepSeek API (https://api.deepseek.com/anthropic/v1/messages)
    │ SSE 响应（model: "deepseek-chat"）
    ▼
cc-switch proxy
    │ 3. 每个 SSE chunk：patch_sse_chunk(fake_model="claude-sonnet-4-6")
    │    - "model":"deepseek-chat" → "model":"claude-sonnet-4-6"
    │    - "type":"thinking" → "type":"text"
    │    - "type":"redacted_thinking" → "type":"text"
    ▼
Claude Code（看到的 model 名是 claude-sonnet-4-6，校验通过）
```

**伪装映射：**

| 预设 | Claude Code 看到 | 实际请求 DeepSeek |
|------|-----------------|-----------------|
| DeepSeek V4 Flash | `claude-sonnet-4-6` | `deepseek-chat` |
| DeepSeek V4 Pro | `claude-opus-4-7` | `deepseek-reasoner` |

---

## 不改动的部分

- `handlers.rs`：models 伪装复用现有 `handle_claude_desktop_models` 逻辑（已支持自定义 modelsUrl）
- `transform.rs` / `transform_responses.rs`：无协议转换，不涉及
- `forwarder.rs` 的重试/failover 逻辑：完全不动

---

## 测试要点

- `sanitize_request`：单元测试覆盖 thinking 块过滤、空 content 占位、web_search 修正、model 覆写
- `patch_sse_chunk`：单元测试覆盖 model 名替换、thinking 类型替换、正常文本块不受影响
- 集成：确认 Claude Code 连接 DeepSeek 时无 400/422 错误

---

## 文件变更清单

| 文件 | 操作 |
|------|------|
| `src-tauri/src/proxy/providers/deepseek_anthropic.rs` | 新建 |
| `src-tauri/src/proxy/providers/mod.rs` | 修改（声明模块） |
| `src-tauri/src/proxy/providers/claude.rs` | 修改（新增 format 分支和两个函数） |
| `src-tauri/src/proxy/forwarder.rs` | 修改（净化请求、透传 model 名） |
| `src-tauri/src/proxy/providers/streaming.rs` | 修改（chunk patch） |
| `src/types.ts` | 修改（ClaudeApiFormat 联合类型） |
| `src/config/claudeProviderPresets.ts` | 修改（新增预设） |
| `src/i18n/locales/zh.json` | 修改 |
| `src/i18n/locales/en.json` | 修改 |
| `src/i18n/locales/ja.json` | 修改 |
