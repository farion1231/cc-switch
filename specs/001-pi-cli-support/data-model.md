# Data Model: Pi CLI 配置管理

**Feature**: specs/001-pi-cli-support
**Date**: 2026-05-23

## Entity Changes

### 1. AppType 枚举扩展

```
Existing: Claude | ClaudeDesktop | Codex | Gemini | OpenCode | OpenClaw | Hermes
New:      Pi  (additive mode: true)
```

**Properties**:
- `as_str()` → `"pi"`
- `FromStr` → accepts `"pi"`
- `is_additive_mode()` → `true` (同级所有提供商写入 models.json)
- `all()` → includes `AppType::Pi`

### 2. McpApps 扩展

新增 `pi: bool` 字段（默认 `false`，因为 Pi 不支持原生 MCP，仅通过扩展支持）

### 3. SkillApps 扩展

新增 `pi: bool` 字段。
- `is_enabled_for(AppType::Pi)` → `self.pi`
- `set_enabled_for(AppType::Pi, enabled)` → `self.pi = enabled`
- `enabled_apps()` → includes `AppType::Pi` when `self.pi`

### 4. VisibleApps 扩展

新增 `pi: bool` 字段，默认 `true`（Pi 作为新增工具默认可见）。
- `is_visible(AppType::Pi)` → `self.pi`

### 5. CommonConfigSnippets 扩展

新增 `pi: Option<String>` 字段，用于存储 Pi 的共享配置片段。

### 6. Settings 扩展

新增字段（`src-tauri/src/settings.rs`）：
- `current_provider_pi: Option<String>` — Pi 当前激活的提供商 ID
- `pi_config_dir: Option<String>` — Pi 配置目录覆盖路径

### 7. Pi Provider 元数据

`Provider.meta` 中 Pi 提供商可能包含的字段（`ProviderMeta` 结构体）：

| 字段 | 类型 | 描述 |
|------|------|------|
| `pi_api_type` | `Option<String>` | API 类型：`anthropic-messages` / `openai-completions` / `google-generative-ai` / `openai-responses` |
| `pi_model_id` | `Option<String>` | 提供商默认模型 ID |
| `pi_models` | `Option<Vec<PiModel>>` | 手动添加的模型列表（覆盖预设） |

### 8. PiModel 子实体

```rust
struct PiModel {
    id: String,                  // 模型 ID（如 "claude-sonnet-4-20250514"）
    name: String,                // 显示名称
    reasoning: bool,             // 是否支持扩展思考
    input: Vec<String>,          // 支持的输入类型 ["text", "image"]
    context_window: u32,         // 上下文窗口
    max_tokens: u32,             // 最大输出 tokens
    cost: PiModelCost,           // 定价信息
}

struct PiModelCost {
    input: f64,                  // $/M tokens
    output: f64,
    cache_read: f64,
    cache_write: f64,
}
```

## Storage

### models.json 写入结构

```
~/.pi/agent/models.json
{
  "providers": {
    "<provider_id>": {
      "baseUrl": "<api_base_url>",
      "api": "<api_type>",
      "apiKey": "<env_var_name_or_value>",
      "authHeader": true,
      "compat": { "supportsDeveloperRole": false, "supportsReasoningEffort": false },
      "models": [{ PiModel ... }]
    }
  }
}
```

### settings.json 写入结构

```
~/.pi/agent/settings.json
{
  "defaultProvider": "<active_provider_id>",
  "defaultModel": "<active_model_id>",
  "defaultThinkingLevel": "<level>",
  "theme": "<theme_name>",
  // ... 其他设置项
}
```

## State Transitions

### Provider 生命周期

```
[创建] → [保存到 CC Switch DB + 写入 models.json]
   ↓
[设为当前] → [更新 settings.json: defaultProvider + defaultModel]
   ↓
[编辑] → [更新 models.json + 如为当前则同步 settings.json]
   ↓
[删除] → [从 models.json 移除 + 如为当前则清除 settings.json defaultProvider]
```

### Settings 写入流程

```
[用户修改设置] → [读取现有 settings.json]
   ↓
[合并修改]（保留未知字段）
   ↓
[写入临时文件] → [验证 JSON 有效性]
   ↓
[创建备份]（如启用）
   ↓
[原子 rename] → [通知前端更新成功]
```

## Relationships

```
AppType::Pi
  ├── Provider[] (1:N)       -- Pi 提供商列表，写入 models.json
  │     └── PiModel[] (1:N)  -- 每个提供商的模型列表
  ├── Skill[] (M:N)           -- 通过 SkillApps 关联，同步到 ~/.pi/agent/skills/
  ├── Settings (1:1)          -- 对应 settings.json
  └── ContextFiles (1:N)      -- AGENTS.md, SYSTEM.md (通过现有 Prompt 系统管理)
```
