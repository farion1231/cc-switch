# Research: Pi CLI 配置管理集成

**Date**: 2026-05-23
**Feature**: specs/001-pi-cli-support/spec.md

## 1. Pi 配置体系分析

### Decision: 采用 models.json + settings.json 双文件写入策略

**Rationale**: Pi 的提供商配置分两层：内置提供商通过环境变量认证（如 `ANTHROPIC_API_KEY`），自定义/中继提供商通过 `~/.pi/agent/models.json` 定义。CC Switch 统一将提供商写入 `models.json`，同时更新 `settings.json` 中的 `defaultProvider`/`defaultModel` 完成切换。

**Alternatives considered**:
- 仅写环境变量：无法支持自定义 Base URL 的中继提供商，且环境变量管理复杂
- 仅写 settings.json：pi 的 settings.json 不包含 API 密钥，无法管理自定义端点
- 双文件策略（选中）：覆盖所有场景，与 CC Switch 的统一管理理念一致

### Pi 配置目录结构

```
~/.pi/agent/
├── settings.json       # 全局设置（defaultProvider, defaultModel, thinkingLevel, theme 等）
├── models.json         # 自定义提供商/模型定义
├── AGENTS.md           # 全局上下文文件
├── SYSTEM.md           # 系统提示词（替换默认）
├── sessions/           # 会话历史（不纳入 CC Switch 管理范围）
├── skills/             # Agent Skills 安装目录
├── extensions/         # TypeScript 扩展
├── prompts/            # Prompt 模板
├── themes/             # 主题文件
└── keybindings.json    # 键盘快捷键（不纳入 CC Switch 管理范围）
```

## 2. Provider 写入策略

### Decision: 将 CC Switch 提供商映射为 models.json 中的独立 provider 条目

**Rationale**: Pi 的 `models.json` 支持多个并列的 provider 定义。每个 CC Switch 提供商对应一个独立的 provider 条目，`defaultProvider` 控制当前激活的提供商。

**models.json 格式**:
```json
{
  "providers": {
    "cc-switch-anthropic": {
      "baseUrl": "https://api.anthropic.com",
      "api": "anthropic-messages",
      "apiKey": "ANTHROPIC_API_KEY",
      "authHeader": true,
      "models": [
        {
          "id": "claude-sonnet-4-20250514",
          "name": "Claude Sonnet 4",
          "reasoning": true,
          "input": ["text", "image"],
          "contextWindow": 200000,
          "maxTokens": 16384,
          "cost": { "input": 3, "output": 15, "cacheRead": 0.3, "cacheWrite": 3.75 }
        }
      ]
    },
    "cc-switch-openai": {
      "baseUrl": "https://api.openai.com/v1",
      "api": "openai-completions",
      "apiKey": "OPENAI_API_KEY",
      "authHeader": true,
      "compat": { "supportsDeveloperRole": false, "supportsReasoningEffort": false },
      "models": [
        {
          "id": "gpt-5",
          "name": "GPT-5",
          "reasoning": true,
          "input": ["text", "image"],
          "contextWindow": 128000,
          "maxTokens": 16384,
          "cost": { "input": 3.0, "output": 15.0, "cacheRead": 0.3, "cacheWrite": 3.75 }
        }
      ]
    }
  }
}
```

**settings.json 更新**:
```json
{
  "defaultProvider": "cc-switch-anthropic",
  "defaultModel": "claude-sonnet-4-20250514"
}
```

### 合并策略

- CC Switch 管理的 providers 使用 `cc-switch-` 前缀命名空间，避免与用户手动添加的 providers 冲突
- 写入时读取现有 `models.json`，保留非 CC Switch 前缀的 provider 条目，仅替换/添加 CC Switch 管理的条目
- 如果 models.json 不存在，创建仅含 CC Switch 条目的文件

## 3. Pi 内置提供商预设映射

### Decision: 为 Pi 支持的主流 API 类型提供内置预设

| 预设名称 | API 类型 | 默认 Base URL | 环境变量 |
|---------|---------|--------------|---------|
| Anthropic (API Key) | `anthropic-messages` | `https://api.anthropic.com` | `ANTHROPIC_API_KEY` |
| OpenAI | `openai-completions` | `https://api.openai.com/v1` | `OPENAI_API_KEY` |
| Google Gemini | `google-generative-ai` | `https://generativelanguage.googleapis.com/v1beta` | `GEMINI_API_KEY` |
| DeepSeek | `openai-completions` | `https://api.deepseek.com` | `DEEPSEEK_API_KEY` |
| OpenRouter | `openai-completions` | `https://openrouter.ai/api/v1` | `OPENROUTER_API_KEY` |
| Groq | `openai-completions` | `https://api.groq.com/openai/v1` | `GROQ_API_KEY` |
| Cerebras | `openai-completions` | `https://api.cerebras.ai/v1` | `CEREBRAS_API_KEY` |
| xAI | `openai-completions` | `https://api.x.ai/v1` | `XAI_API_KEY` |
| Mistral | `openai-completions` | `https://api.mistral.ai/v1` | `MISTRAL_API_KEY` |
| Fireworks | `openai-completions` | `https://api.fireworks.ai/inference/v1` | `FIREWORKS_API_KEY` |
| 自定义 (Custom) | 用户选择 | 用户输入 | 用户定义 |

## 4. Skills 集成策略

### Decision: 复用现有 Skill 管理基础设施

**Rationale**: Pi 遵循 Agent Skills 标准（agentskills.io），与 CC Switch 已有的 Skills 管理完全兼容。只需：
1. 在 `AppType::Pi` 的 SkillApps 枚举中添加条目
2. 将 Pi 的 skills 目标目录设为 `~/.pi/agent/skills/`
3. 复用现有的 `SkillStore` 服务进行安装、卸载、同步

**同步路径**:
- SSOT 目录: `~/.cc-switch/skills/<owner>--<repo>--<dir>/`（已有）
- Pi 目标目录: `~/.pi/agent/skills/<dir>/`（新增）

## 5. Settings 可视化管理范围

### Decision: 管理常用设置项，保留高级设置的 JSON 手动编辑入口

**Rationale**: 完全覆盖所有 settings.json 字段会使 UI 过于复杂。聚焦用户最频繁修改的选项。

**纳入 UI 管理**:
- `defaultProvider` / `defaultModel` — 提供商切换时自动更新
- `defaultThinkingLevel` — 下拉选择（off/minimal/low/medium/high/xhigh）
- `theme` — 下拉选择（dark/light）
- `hideThinkingBlock` — 开关
- `quietStartup` — 开关
- `compaction.enabled` — 开关
- `retry.enabled` — 开关
- `retry.maxRetries` — 数字输入

**不纳入 UI 管理**（保留 JSON 编辑）:
- `terminal.*`, `images.*`, `shell.*`, `warnings.*`, `branchSummary.*` 等高级配置

## 6. 原子写入与备份

### Decision: 复用现有 `atomic_write` 基础设施

**Rationale**: CC Switch 已有完善的原子写入函数（`write_json_file`、`atomic_write`），Pi 配置文件直接复用。写入流程：先写临时文件 → 验证 JSON 有效性 → 创建备份 → rename 到目标路径。与宪法原则 III（数据完整性与安全）一致。

## 7. AppType 扩展影响范围

添加 `AppType::Pi` 需要更新以下位置（基于代码分析）：

| 文件 | 变更 |
|------|------|
| `src-tauri/src/app_config.rs` | 新增 `Pi` 枚举值、`as_str()`、`FromStr`、`all()`、`is_additive_mode()`、McpApps、SkillApps |
| `src-tauri/src/settings.rs` | VisibleApps 新增 `pi` 字段、current_provider_pi、pi_config_dir |
| `src-tauri/src/config.rs` | Pi 配置目录路径函数 |
| 前端 `src/types.ts` / `src/types/` | AppType 类型扩展 |
| 前端 `src/components/` | Pi 选项卡、提供商表单、设置面板 |
| 前端 `src/i18n/` | zh/en/ja 翻译条目 |
| `src-tauri/src/database/dao/` | Pi 提供商种子数据 |

## 8. Pi 提供商采用累加模式

### Decision: Pi 使用累加模式（additive mode）

**Rationale**: Pi 的 `models.json` 支持同时定义多个提供商，所有提供商共存。这与 OpenCode、OpenClaw、Hermes 的模式一致（`is_additive_mode() = true`）。用户通过 `defaultProvider` 字段切换当前激活的提供商，而其他提供商配置保留在配置文件中。
