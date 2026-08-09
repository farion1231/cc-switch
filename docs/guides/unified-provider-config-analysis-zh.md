# 不同客户端配置与统一供应商配置分析

## 1. 不同客户端的配置概述

CC Switch 支持多种 AI 客户端/供应商，包括 Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build、OpenCode、OpenClaw、Hermes 等。这些客户端的配置格式差异较大，需要手动维护 JSON、TOML 或 .env 文件，容易出现配置漂移和同步问题。

### 1.1 Claude 系列客户端
- **配置文件位置**：`~/.claude/settings-{name}.json`（或 Claude Desktop 特定路径）
- **配置格式**：JSON 结构，核心字段在 `env` 节点下
  - ANTHROPIC_BASE_URL
  - ANTHROPIC_AUTH_TOKEN
  - ANTHROPIC_MODEL、ANTHROPIC_DEFAULT_HAIKU_MODEL 等多个模型别名
- **特点**：环境变量风格，模型配置以多个默认字段存在

### 1.2 Codex / OpenAI 兼容客户端
- **配置文件位置**：`~/.config/codex/` 或类似
- **配置格式**：TOML 格式
  - `model_provider = "custom"`
  - `[model_providers.custom]` 节，包含 base_url、name 等
  - 认证信息在 `auth` 节点下（如 OPENAI_API_KEY）
- **特点**：支持 reasoning effort、disable_response_storage 等额外选项；base_url 处理需特殊逻辑（如补 /v1）

### 1.3 Gemini 客户端
- **配置文件位置**：特定环境变量或 JSON 文件
- **配置格式**：类似 Claude，JSON with `env` 节点
  - GOOGLE_GEMINI_BASE_URL
  - GEMINI_API_KEY
  - GEMINI_MODEL
- **特点**：Google 生态风格，模型配置较简单

### 1.4 OpenCode 客户端
- **配置格式**：复杂 JSON 结构（settings_config）
  - `npm` 字段指定 AI SDK 包名（如 `@ai-sdk/openai-compatible`）
  - `options` 包含 baseURL、apiKey、headers 等
  - `models` 映射模型定义，支持额外字段（如 cost、thinking）
- **特点**：与传统供应商不同，采用 AI SDK 兼容模式，模型列表更灵活

### 1.5 OpenClaw、Hermes 等客户端
- **配置格式**：各自的 JSON/TOML/YAML
  - OpenClaw：models.providers 结构，baseUrl、apiKey、models 数组
  - Hermes：snake_case 字段，如 base_url、api_key
- **特点**：格式更多样化，认证方式各异，模型支持特殊字段（如 reasoning、input 类型）

**通用差异点**：
- 认证方式：env 变量 vs auth 节点 vs options 对象
- 文件位置和格式各异
- 模型映射逻辑不同（Claude 有多个默认模型，Codex 有 reasoningEffort，Gemini 较简单）
- 额外元数据支持（pricing、timeout、headers 等）缺失或不一致

## 2. 统一供应商配置的实现原理

统一供应商配置（Universal Provider）是 CC Switch 的核心抽象层，旨在将不同客户端的配置差异统一到一个共享结构中，实现“一处修改，同步到所有启用客户端”的功能。

### 2.1 核心数据结构
- **Rust 侧**：`UniversalProvider` 结构体
  - 字段：`id`、`name`、`providerType`（e.g. "newapi"）、`apps`（启用状态：claude/codex/gemini）、`base_url`、`api_key`、`models`（各客户端模型配置）、`websiteUrl`、`icon` 等
  - 支持序列化/反序列化，兼容不同客户端
- **JS 侧**：相同类型的 TypeScript 接口，用于表单、状态管理和 API
- **预设模板**：`universalProviderPresets.ts` 包含 NewAPI、自定义网关等模板，包含默认模型映射

### 2.2 转换机制（转换逻辑）
- `UniversalProvider` 提供方法如 `to_claude_provider()`、`to_codex_provider()`、`to_gemini_provider()` 等
  - **Claude**：生成 JSON with `env` 字段，映射模型（默认值处理、haiku/sonnet/opus 别名）
  - **Codex**：生成 TOML 字符串，处理 base_url 的 `/v1` 补全逻辑（origin-only vs prefix-aware）、model/reasoning_effort
  - **Gemini**：类似 Claude，生成 `env` 结构
- 其他客户端（如 OpenCode、OpenClaw、Hermes）有专用转换逻辑，在 `provider.rs` 中实现
- 处理特殊情况：托管账号（github_copilot 等）、live config 管理、累加模式等

### 2.3 同步与存储机制
- **存储**：统一供应商存储在主配置系统（SQLite DB 或 JSON）中，独立于各客户端的 live 配置
- **同步流程**：
  1. 修改统一供应商（UI 表单或 API）
  2. 调用 Rust 命令 `sync_universal_provider` / `upsert_universal_provider`
  3. 遍历启用 apps，调用对应 `to_xxx_provider()` 生成客户端特定配置
  4. 原子写入各客户端配置文件（保护机制如原子操作）
- 支持手动同步、复制、从其他客户端导入等功能
- 双向同步：更新客户端后可同步回统一供应商（可选）

### 2.4 实现优势与设计原则
- **抽象隔离**：屏蔽客户端差异，提供一致的 API（baseUrl + apiKey + models）
- **兼容性**：专为聚合网关（如 NewAPI）设计，支持统一模型路由
- **可靠性**：原子写入、版本控制、回退机制，防止配置损坏
- **扩展性**：添加新客户端只需实现转换方法和类型定义
- **用户友好**：预设、模型字段映射、JSON 预览、启用开关

## 3. 差异分析与技术考量

### 3.1 抽象 vs 具体
- 统一供应商提供**一致视图**，而客户端提供**具体实现**。差异通过转换逻辑桥接。
- 例如，Codex 的 base_url 处理逻辑在转换时特殊处理，避免用户输入错误。

### 3.2 模型配置映射差异
- Claude：多模型别名（haiku/sonnet/opus）
- Codex：reasoningEffort 字段
- Gemini：单一 model
- 统一供应商在 `UniversalProviderModels` 中抽象这些差异

### 3.3 认证与元数据
- 不同客户端认证方式不同（env vs options vs auth），统一供应商统一为 `api_key` + `base_url`，转换层处理细节。

### 3.4 性能与维护
- Rust 转换逻辑处理复杂字符串拼接和 JSON 嵌套
- JS 侧 UI 提供友好映射，避免用户直接编辑客户端文件
- 潜在问题：转换不一致导致配置不匹配，通过测试和回滚机制缓解

### 3.5 与传统供应商的区别
- 传统供应商：直接在客户端中配置，格式特定
- 统一供应商：跨客户端共享，适合聚合场景，减少维护工作

## 4. 实际使用场景

- **聚合网关场景**：配置一次 NewAPI 网关，即可同步到所有 Claude/Codex/Gemini 客户端。
- **多客户端切换**：修改统一供应商模型/地址，即可一键更新所有客户端。
- **迁移/导入**：从旧客户端配置导入统一供应商，或反向同步。
- **自定义扩展**：支持自定义模板，添加自定义模型/元数据。

## 6. 代码层面详细分析

以下从 Rust 和 TypeScript 代码层面深入拆解统一供应商配置的实现。

### 6.1 Rust 侧核心结构体与转换逻辑 (`src-tauri/src/provider.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalProvider {
    pub id: String,
    pub name: String,
    #[serde(rename = "providerType")]
    pub provider_type: String,
    pub apps: UniversalProviderApps,  // claude/codex/gemini 启用标志
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub models: UniversalProviderModels,
    // ... (icon, websiteUrl, meta, created_at 等)
}
```

**关键转换方法示例**（`to_claude_provider`）：

```rust
pub fn to_claude_provider(&self) -> Option<Provider> {
    if !self.apps.claude { return None; }
    let models = self.models.claude.as_ref().unwrap_or(&Default::default());
    let model = models.model.clone().unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());
    // 处理多个默认模型别名
    let settings_config = serde_json::json!({
        "env": {
            "ANTHROPIC_BASE_URL": self.base_url,
            "ANTHROPIC_AUTH_TOKEN": self.api_key,
            "ANTHROPIC_MODEL": model,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": haiku,
            "ANTHROPIC_DEFAULT_SONNET_MODEL": sonnet,
            "ANTHROPIC_DEFAULT_OPUS_MODEL": opus,
        }
    });
    // 返回 Provider 结构体（包含 settings_config 等）
}
```

**Codex 转换逻辑**（处理特殊 base_url）：

```rust
let base_trimmed = self.base_url.trim_end_matches('/');
let origin_only = match base_trimmed.split_once("://") {
    Some((_scheme, rest)) => !rest.contains('/'),
    None => !base_trimmed.contains('/'),
};
let codex_base_url = if base_trimmed.ends_with("/v1") {
    base_trimmed.to_string()
} else if origin_only {
    format!("{base_trimmed}/v1")
} else {
    base_trimmed.to_string()
};
let config_toml = format!(r#"model_provider = "custom"
[model_providers.custom]
base_url = "{codex_base_url}"
... "#);
let settings_config = json!({
    "auth": { "OPENAI_API_KEY": self.api_key },
    "config": config_toml
});
```

**其他客户端转换**：
- OpenCode：构建 `OpenCodeProviderConfig`（npm, options, models）
- Hermes/OpenClaw：snake_case / camel_case 字段处理
- 所有转换均返回 `Option<Provider>`，返回 `None` 时跳过禁用应用

**Provider 结构体**（核心载体）：

```rust
pub struct Provider {
    pub id: String,
    pub name: String,
    pub settings_config: Value,  // JSON/TOML 内容
    pub category: Option<String>, // "aggregator" / "official" 等
    pub meta: Option<ProviderMeta>, // 额外元数据（如 usage_script）
    // ...
}
```

### 6.2 Rust 侧服务层 (`src-tauri/src/services/provider/mod.rs` + `live.rs`)

- `ProviderService::add` / `update` / `sync_current_to_live` 等操作
- 统一供应商同步通过 `sync_universal_provider` 命令触发
- 通用配置片段（common config snippet）机制：Gemini 等客户端支持共享片段，避免凭据泄露（`extract_gemini_common_config`, `scrub_leaked_gemini_common_config`）
- 原子写入 + live config 管理（`write_live_with_common_config`）

### 6.3 TypeScript 侧（前端实现）

```ts
// src/types.ts
export interface UniversalProvider {
    id: string;
    name: string;
    providerType: string;
    apps: UniversalProviderApps;  // { claude: true, codex: true, ... }
    baseUrl: string;
    apiKey: string;
    models: UniversalProviderModels;
    // ...
}

// src/config/universalProviderPresets.ts
export const universalProviderPresets: UniversalProviderPreset[] = [
    {
        name: "NewAPI",
        providerType: "newapi",
        defaultApps: { claude: true, codex: true, gemini: true },
        defaultModels: { /* claude: { model, haikuModel, ... }, codex: { model, reasoningEffort }, ... */ },
    },
];

// 表单与同步
// UniversalProviderFormModal.tsx / UniversalProviderPanel.tsx
// - 启用开关、模型字段映射到不同客户端
// - 调用 universalProvidersApi.sync(id) 触发后端转换
// - 支持从客户端导入、JSON 预览、复制等
```

### 6.4 同步与存储实现

- **Rust 命令**（`src-tauri/src/commands/provider_commands.rs` 或类似）：
  ```rust
  // 触发统一供应商同步
  async fn sync_universal_provider(state: State, id: String) -> Result<bool> { ... }
  // 调用对应 to_xxx_provider 方法生成 settings_config
  // 原子写入各客户端路径（使用 config::write_json_file_with_contents 等）
  ```

- **数据库存储**：统一供应商独立存储在主 DB（`database::save_provider` / `get_universal_providers`），与 live config 解耦
- **测试覆盖**：大量单元测试验证转换逻辑（如 `universal_provider_to_claude_provider_uses_models`、`universal_provider_to_codex_provider_appends_v1` 等）

### 6.5 关键注意事项与潜在坑

- **base_url 处理**：Codex 转换有 origin-only / prefix-aware 逻辑，防止用户输入错误
- **模型默认值**：统一供应商提供 fallback 模型，避免客户端特定配置缺失
- **凭据安全**：Gemini 专用凭据 scrub 机制，避免泄露到片段
- **原子性**：所有写入使用原子操作，防止中间状态损坏
- **错误处理**：转换失败返回 `None`，同步时优雅降级

此代码层面分析基于最新实现（2026-08 版本），建议参考 `provider.rs`、`live.rs`、`services/provider/` 及对应 TS 文件以获取完整上下文。

如需特定函数的更详细 diff、测试用例或扩展新客户端的代码模板，请提供更多细节！

- **Rust 核心**：`src-tauri/src/provider.rs` 中的 `UniversalProvider`、`to_claude_provider` 等方法，`Provider` 结构体
- **JS 侧**：`src/config/universalProviderPresets.ts`、`src/types.ts` 中的类型、`src/components/universal/` 中的面板和表单
- **API 层**：`src/lib/api/providers.ts` 的 `universalProvidersApi`
- **测试**：`src-tauri/tests/provider_commands.rs` 等

此文档旨在帮助开发者理解统一供应商配置的设计思路及与各客户端的差异。如需扩展特定客户端支持或修改转换逻辑，请参考上述代码文件。

---

*最后更新：2026-08-08*