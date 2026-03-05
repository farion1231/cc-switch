# CC-Switch Bedrock 请求优化器增强

## 概述

为 CC-Switch 代理新增 **PRE-SEND 请求优化器**，在请求转发到 AWS Bedrock 之前自动优化请求体，提升推理质量并降低 token 消耗。

**适用范围**：仅对 AWS Bedrock (AKSK) 和 AWS Bedrock (API Key) 两种供应商生效。通过检测 `CLAUDE_CODE_USE_BEDROCK = "1"` 精确门控，其他所有供应商（Claude Official、中转服务、OpenRouter、DeepSeek、Zhipu 等）完全不受影响。

---

## 新增功能

### 1. Thinking 优化器

在请求转发前，根据请求体中的 `model` 字段自动识别模型类型，对 thinking 和 effort 配置进行针对性优化。

**Adaptive 路径（Opus 4.6 / Sonnet 4.6）：**

当检测到模型名称包含 `opus-4-6` 或 `sonnet-4-6` 时，执行以下三项优化：

- **`thinking.type → "adaptive"`**：启用自适应思考模式。与固定 budget 的 `"enabled"` 不同，adaptive 模式让模型自主判断是否需要深度推理——简单问题快速回答，复杂问题自动深入思考，不浪费 token。这是 Opus 4.6 / Sonnet 4.6 新支持的模式。
- **`output_config.effort → "max"`**：将模型的输出努力级别设为最大。Anthropic API 支持 `low / medium / max` 三档，`max` 让模型不惜 token 成本给出最高质量的输出。配合 adaptive thinking，意思是"思不思考你自己决定，但一旦决定思考就全力以赴"。
- **`anthropic_beta += "context-1m-2025-08-07"`**：启用 100 万 token 上下文窗口。这是一个 Anthropic beta 功能开关（格式：`功能名-版本日期`）。Claude 模型默认上下文窗口为 200K token，加上这个 beta 后扩展到 1M。在 Bedrock 上如果不显式声明此 beta，即使模型支持 1M，Bedrock 端也只给 200K。对于大型代码库和长对话的 agent 场景尤其重要。

修改前：
```json
{ "thinking": { "type": "enabled", "budget_tokens": 10000 } }
```
修改后：
```json
{
  "thinking": { "type": "adaptive" },
  "output_config": { "effort": "max" },
  "anthropic_beta": ["context-1m-2025-08-07"]
}
```

> 三者配合的效果：解锁模型的全部能力——智能思考 + 最大努力 + 最大上下文，让 Bedrock 上的 Claude 表现与直连 API 一致。

**Legacy 路径（Sonnet 4.5、Opus 4.5 等旧模型）：**

当模型不属于 adaptive 列表且不属于 skip 列表时，为旧模型注入 thinking 支持：

- 若 thinking 为 `null` 或 `type = "disabled"`：注入 `thinking.type = "enabled"` + `budget_tokens = max_tokens - 1`（将几乎全部 token 预算分配给思考），并追加 `"interleaved-thinking-2025-05-14"` beta header（启用交错思考功能，允许模型在输出过程中穿插思考步骤）
- 若 thinking 已存在但 `budget_tokens` 低于 `max_tokens - 1`：升级到最大值
- 若 budget_tokens 已是最大值：不做修改

修改前：
```json
{ "max_tokens": 16384, "thinking": null }
```
修改后：
```json
{
  "max_tokens": 16384,
  "thinking": { "type": "enabled", "budget_tokens": 16383 },
  "anthropic_beta": ["interleaved-thinking-2025-05-14"]
}
```

**Skip 路径（Haiku）：**

当模型名称包含 `haiku` 时，不做任何修改，直接跳过。Haiku 定位为轻量快速模型，强制开启 thinking 会增加延迟和成本，与其设计目标相悖。

### 2. Cache 断点注入

在请求转发前，自动在请求体的关键位置注入 `cache_control` 标记，启用 Bedrock 的 Prompt Caching 功能，减少重复 token 的计费。

**断点上限**：Anthropic API 限制每个请求最多 4 个 `cache_control` 断点。注入器通过 `budget = 4 - 已有断点数` 计算可注入数量，绝不超限。

**注入位置**（按优先级依次尝试，直到 budget 用完）：

1. **tools 末尾** — 在 `tools` 数组最后一个元素上添加 `cache_control`。tool 定义在对话中几乎不变，缓存收益最高。
2. **system prompt 末尾** — 在 `system` 数组最后一个 block 上添加 `cache_control`。若 system 是字符串格式，自动转换为数组格式后注入。
3. **最后一条 assistant 消息** — 在 `messages` 中逆序查找最后一条 `role = "assistant"` 的消息，在其 `content` 中最后一个**非 thinking/redacted_thinking** block 上添加 `cache_control`。

**TTL 管理**：

- 新注入的断点格式：`{ "type": "ephemeral", "ttl": "<配置值>" }`，默认 TTL 为 `"1h"`
- 已有断点的 TTL 自动升级到配置值（例如 `"5m"` → `"1h"`）
- 当 TTL 配置为 `"5m"` 时（Anthropic API 默认值），省略 ttl 字段，仅写 `{ "type": "ephemeral" }`

**日志输出示例**：
- 注入模式：`3bp(tools+system+msgs,1h,pre=0)` — 注入了 3 个断点，原有 0 个
- 升级模式：`ttl-upgrade(2->1h,existing=4)` — 升级了 2 个已有断点的 TTL，总数已满
- 无操作：`no-op(existing=4)` — 断点已满且 TTL 无需升级

### 3. Bedrock 供应商精确门控

优化器仅在检测到 `CLAUDE_CODE_USE_BEDROCK = "1"` 环境变量时激活。该变量由 AWS Bedrock (AKSK) 和 AWS Bedrock (API Key) 两个预设模板自动配置。

**门控检查位置**：`forward_with_retry()` 函数内，provider loop 之前，检查第一个 provider 的 `settings_config.env.CLAUDE_CODE_USE_BEDROCK`。

```rust
fn is_bedrock_provider(provider: &Provider) -> bool {
    provider.settings_config
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_USE_BEDROCK"))
        .and_then(|v| v.as_str())
        .map(|v| v == "1")
        .unwrap_or(false)
}
```

**各供应商生效情况**：

| 供应商 | `CLAUDE_CODE_USE_BEDROCK` | 优化器 |
|--------|--------------------------|--------|
| AWS Bedrock (AKSK) | `"1"` | 生效 |
| AWS Bedrock (API Key) | `"1"` | 生效 |
| Claude Official | 无 | 不生效 |
| DeepSeek / Zhipu / Bailian 等 | 无 | 不生效 |
| OpenRouter | 无 | 不生效 |
| ClaudeAuth 中转 | 无 | 不生效 |
| Codex / Gemini | 无 | 不生效 |

---

## 配置管理

### OptimizerConfig 结构体

```json
{
  "enabled": false,
  "thinkingOptimizer": true,
  "cacheInjection": true,
  "cacheTtl": "1h"
}
```

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | `false` | 总开关。默认关闭，用户需手动启用 |
| `thinkingOptimizer` | bool | `true` | Thinking 优化子开关。总开关开启后默认生效 |
| `cacheInjection` | bool | `true` | Cache 注入子开关。总开关开启后默认生效 |
| `cacheTtl` | string | `"1h"` | Cache TTL。Bedrock 支持 `"5m"` 和 `"1h"` 两种 |

配置存储在 SQLite settings 表中，key 为 `optimizer_config`，与现有的 `rectifier_config` 完全独立。

---

## 架构设计

### 管线位置

```
Claude Code 请求
  │
  ├── handler (handle_messages 等，不变)
  │     │
  │     ├── RequestContext 创建（加载 OptimizerConfig + RectifierConfig）
  │     │
  │     └── forward_with_retry(mut body, providers)
  │           │
  │           ├── [NEW] PRE-SEND 优化器
  │           │     ├── is_bedrock_provider(first_provider)?
  │           │     │     NO  → 跳过，body 原样
  │           │     │     YES ↓
  │           │     ├── optimizer_config.enabled?
  │           │     │     NO  → 跳过
  │           │     │     YES ↓
  │           │     ├── thinking_optimizer::optimize(&mut body, model)
  │           │     └── cache_injector::inject(&mut body, cache_ttl)
  │           │
  │           ├── for provider in providers  ← 现有逻辑，完全不变
  │           │     ├── forward(provider, body) → 成功 → return
  │           │     └── 失败 → Rectifier 整流 (POST-ERROR，现有逻辑不变)
  │           └── ...
  │
  └── 响应处理（不变）
```

### Optimizer 与 Rectifier 的关系

| | Optimizer（新增） | Rectifier（现有） |
|---|---|---|
| **触发时机** | 发送**前** | 发送**后**遇错时 |
| **作用** | 主动优化请求体 | 被动修复请求体 |
| **目标** | 提升质量、降本 | 解决兼容性错误 |
| **适用范围** | 仅 Bedrock | Claude/ClaudeAuth |

两者互补，互不干扰。优化后的请求如仍报错，Rectifier 按原有逻辑整流修复。

---

## 改动范围

### 新建文件（2 个）
| 文件 | 说明 | 预估行数 |
|------|------|---------|
| `src-tauri/src/proxy/thinking_optimizer.rs` | Thinking 优化：adaptive/legacy/skip 三路径 | ~80 |
| `src-tauri/src/proxy/cache_injector.rs` | Cache 注入：断点计数、TTL 升级、新断点注入 | ~100 |

### 修改文件（5 个，每个改几行）
| 文件 | 改动 | 预估行数 |
|------|------|---------|
| `src-tauri/src/proxy/types.rs` | 新增 `OptimizerConfig` 结构体 | +15 |
| `src-tauri/src/proxy/mod.rs` | 注册 `thinking_optimizer` 和 `cache_injector` 模块 | +2 |
| `src-tauri/src/proxy/handler_context.rs` | 从 DB 加载 `OptimizerConfig` 传入 `RequestForwarder` | +5 |
| `src-tauri/src/proxy/forwarder.rs` | `forward_with_retry()` 顶部加门控 + 优化器调用 | +20 |
| `src/components/proxy/RectifierConfigPanel.tsx` | 新增 Optimizer 区域：3 个 toggle + TTL 选择 | +30 |

### 零改动
- Provider 适配器（ClaudeAdapter / CodexAdapter / GeminiAdapter）
- ProviderType 枚举
- 整流器系统（thinking_rectifier.rs / thinking_budget_rectifier.rs）
- 所有 handler（handle_messages / handle_chat_completions / handle_gemini 等）
- 响应处理管线（response_handler.rs / response_processor.rs）
- 前端供应商配置（CommonConfigEditor.tsx / ProviderForm.tsx）

---

## 测试计划

### 功能验证
- [ ] Bedrock AKSK + Opus 4.6 模型：thinking 被修改为 adaptive + effort=max + 1M beta
- [ ] Bedrock AKSK + Sonnet 4.5 模型：thinking 被注入 enabled + budget_tokens = max_tokens-1
- [ ] Bedrock AKSK + Haiku 模型：thinking 不被修改
- [ ] Bedrock API Key + 任意模型：与 AKSK 行为一致
- [ ] Bedrock AKSK + 无 tools 请求：cache 仅注入 system + msgs 断点
- [ ] Bedrock AKSK + 已有 4 个 cache 断点：仅升级 TTL，不新增
- [ ] Bedrock AKSK + 已有 2 个断点：注入 2 个新断点（不超 4）

### 门控验证
- [ ] Claude Official provider：优化器**不生效**，请求体不被修改
- [ ] DeepSeek / Zhipu 等中转 provider：优化器**不生效**
- [ ] OpenRouter provider：优化器**不生效**
- [ ] ClaudeAuth 中转 provider：优化器**不生效**
- [ ] Codex / Gemini provider：优化器**不生效**

### 开关验证
- [ ] `enabled = false`：所有 Bedrock 请求也不被修改
- [ ] `enabled = true, thinkingOptimizer = false`：仅 cache 注入生效
- [ ] `enabled = true, cacheInjection = false`：仅 thinking 优化生效
- [ ] `cacheTtl = "5m"`：注入的断点不带 ttl 字段（使用 API 默认值）

### 兼容性验证
- [ ] Optimizer + Rectifier 联动：优化后请求报签名错误 → Rectifier 正常整流 → 重试成功
- [ ] Optimizer 关闭时：现有整流器行为完全不变
