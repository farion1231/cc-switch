# CC Switch 技术架构

## 1. 整体架构

CC Switch 是一个 **Tauri 2 桌面应用**，采用前后端分离架构：

```
┌─────────────────────────────────────────────────────┐
│                    Tauri 2 Shell                     │
│  ┌──────────────────────┐  ┌──────────────────────┐ │
│  │   前端 (WebView)      │  │   后端 (Rust)         │ │
│  │   React 18 + TS      │  │   Tauri Commands      │ │
│  │   Vite + TailwindCSS  │  │   Services            │ │
│  │   shadcn/ui           │  │   DAO + SQLite        │ │
│  │   TanStack Query v5   │  │   Axum Proxy Server   │ │
│  └──────────┬───────────┘  └──────────┬───────────┘ │
│             │    Tauri IPC (invoke)    │             │
│             │◄────────────────────────►│             │
│             │    Tauri Events (emit)   │             │
│             │◄─────────────────────────│             │
└─────────────────────────────────────────────────────┘
```

### 1.1 通信机制

| 方向 | 机制 | 用途 |
|------|------|------|
| 前端 → 后端 | `invoke()` IPC | 调用 Tauri Command |
| 后端 → 前端 | `emit()` 事件 | 推送状态变更（供应商切换、用量更新等） |
| 外部 → 应用 | DeepLink (`cc-switch://`) | 导入供应商/MCP/Skill/Prompt |

## 2. 前端架构

### 2.1 目录结构

```
src/
├── components/          # UI 组件
│   ├── ui/              # shadcn/ui 基础组件（禁止手动修改）
│   ├── providers/       # 供应商管理组件
│   ├── mcp/             # MCP 服务器管理组件
│   ├── prompts/         # Prompt 管理组件
│   ├── skills/          # Skill 管理组件
│   ├── proxy/           # 代理配置组件
│   ├── sessions/        # 会话管理组件
│   ├── settings/        # 设置页面组件
│   ├── profiles/        # Profile 管理组件
│   ├── env/             # 环境变量组件
│   ├── agents/          # Agent 管理组件
│   ├── hermes/          # Hermes 专用组件
│   ├── openclaw/        # OpenClaw 专用组件
│   ├── universal/       # 通用供应商组件
│   ├── workspace/       # 工作区组件
│   ├── deeplink/        # DeepLink 导入组件
│   ├── common/          # 通用业务组件
│   └── icons/           # 图标组件
├── hooks/               # 业务逻辑 Hook
├── lib/
│   ├── api/             # Tauri IPC 封装层（26 个模块）
│   ├── query/           # TanStack Query 配置
│   │   ├── queries.ts   # Query 定义
│   │   ├── mutations.ts # Mutation 定义
│   │   ├── queryClient.ts # QueryClient 配置
│   │   └── *.ts         # 领域 Query 文件
│   ├── schemas/         # Zod 校验 Schema
│   ├── errors/          # 错误处理
│   └── utils/           # 工具函数
├── config/              # 预设配置（供应商/MCP/模板等）
├── types/               # TypeScript 类型定义
├── i18n/                # 国际化（react-i18next）
└── locales/             # 翻译文件（zh/zh-TW/en/ja）
```

### 2.2 数据流

```
用户操作 → Component → Hook (useMutation) → lib/api (invoke) → Rust Command
                                                                    ↓
UI 更新 ← Component ← Hook (useQuery) ← lib/api (invoke) ← Rust Command
                                                                    ↓
实时推送 ← useTauriEvent ← Tauri Event ← Rust Service
```

**关键模式**：
- **Query 缓存**：`useQuery` + `placeholderData: keepPreviousData` 避免闪烁
- **乐观更新**：Mutation 成功后 `invalidateQueries` 刷新缓存
- **代理运行时轮询**：代理运行时供应商列表每 10 秒自动刷新
- **事件驱动刷新**：`usage-log-recorded` 事件触发用量仪表盘刷新

### 2.3 API 封装层

所有 Tauri IPC 调用通过 `src/lib/api/` 封装，每个领域一个文件：

| 模块 | 职责 |
|------|------|
| `providers.ts` | 供应商 CRUD + 切换 |
| `mcp.ts` | MCP 服务器管理 |
| `prompts.ts` | Prompt 管理 |
| `skills.ts` | Skill 管理 |
| `profiles.ts` | Profile 管理 |
| `proxy.ts` | 代理配置与状态 |
| `settings.ts` | 全局设置 |
| `usage.ts` | 用量统计 |
| `sessions.ts` | 会话管理 |
| `env.ts` | 环境变量 |
| `auth.ts` | 托管认证 |
| `copilot.ts` | GitHub Copilot 认证 |
| `openclaw.ts` | OpenClaw 专用 API |
| `hermes.ts` | Hermes 专用 API |
| `config.ts` | 配置读写 |
| `workspace.ts` | 工作区管理 |
| `subscription.ts` | 订阅管理 |
| `model-fetch.ts` | 模型列表获取 |
| `failover.ts` | 故障转移 |
| `globalProxy.ts` | 全局代理 |
| `deeplink.ts` | DeepLink 处理 |
| `vscode.ts` | VS Code 集成 |

## 3. 后端架构

### 3.1 分层架构

```
Commands (API 层)  →  Services (业务层)  →  DAO (数据访问层)  →  Database
   34 个文件            34 个文件            13 个文件          SQLite
```

**严格规则**：
- Commands 层仅做参数转换和调用 Service，不包含业务逻辑
- Services 层包含业务逻辑，可调用 DAO 和其他 Service
- DAO 层仅做数据库操作，不包含业务逻辑
- 禁止跨层调用（如 Command 直接调用 DAO）

### 3.2 Commands 层 (`src-tauri/src/commands/`)

每个 Command 函数标注 `#[tauri::command]`，按领域拆分：

| 文件 | 职责 |
|------|------|
| `provider.rs` | 供应商 CRUD、切换、排序 |
| `mcp.rs` | MCP 服务器管理 |
| `prompt.rs` | Prompt 管理 |
| `skill.rs` | Skill 安装/卸载/同步 |
| `profile.rs` | Profile 管理 |
| `proxy.rs` | 代理启停、配置 |
| `settings.rs` | 全局设置 |
| `usage.rs` | 用量统计查询 |
| `session_manager.rs` | 会话扫描/删除 |
| `env.rs` | 环境变量管理 |
| `auth.rs` | 托管认证 |
| `copilot.rs` | GitHub Copilot 认证 |
| `failover.rs` | 故障转移 |
| `global_proxy.rs` | 全局代理 |
| `import_export.rs` | 导入导出 |
| `deeplink.rs` | DeepLink 处理 |
| `config.rs` | 配置读写 |
| `model_fetch.rs` | 模型列表获取 |
| `stream_check.rs` | 流式检查 |
| `subscription.rs` | 订阅管理 |
| `balance.rs` | 余额查询 |
| `coding_plan.rs` | 编码计划 |
| `omo.rs` | OMO (One Model One) 管理 |
| `openclaw.rs` | OpenClaw 专用 |
| `hermes.rs` | Hermes 专用 |
| `plugin.rs` | 插件管理 |
| `workspace.rs` | 工作区 |
| `s3_sync.rs` | S3 同步 |
| `webdav_sync.rs` | WebDAV 同步 |
| `sync_support.rs` | 同步辅助 |
| `lightweight.rs` | 轻量模式 |
| `misc.rs` | 杂项 |
| `codex_oauth.rs` | Codex OAuth |

### 3.3 Services 层 (`src-tauri/src/services/`)

| 文件 | 职责 |
|------|------|
| `config.rs` | 配置服务（ConfigService） |
| `provider/` | 供应商服务（ProviderService） |
| `mcp.rs` | MCP 服务（McpService） |
| `prompt.rs` | Prompt 服务（PromptService） |
| `skill.rs` | Skill 服务（SkillService） |
| `profile.rs` | Profile 服务（ProfileService） |
| `proxy.rs` | 代理服务（ProxyService） |
| `speedtest.rs` | 速度测试（SpeedtestService） |
| `model_fetch.rs` | 模型获取 |
| `stream_check.rs` | 流式检查 |
| `subscription.rs` | 订阅服务 |
| `balance.rs` | 余额服务 |
| `coding_plan.rs` | 编码计划服务 |
| `omo.rs` | OMO 服务（OmoService） |
| `env_checker.rs` | 环境检查 |
| `env_manager.rs` | 环境变量管理 |
| `usage_cache.rs` | 用量缓存（UsageCache） |
| `usage_stats.rs` | 用量统计 |
| `session_usage*.rs` | 各工具会话用量解析 |
| `s3*.rs` | S3 同步服务 |
| `webdav*.rs` | WebDAV 同步服务 |
| `sync_protocol.rs` | 同步协议 |
| `sql_helpers.rs` | SQL 辅助 |
| `codex_oauth_models.rs` | Codex OAuth 模型 |

### 3.4 DAO 层 (`src-tauri/src/database/dao/`)

| 文件 | 职责 |
|------|------|
| `providers.rs` | 供应商 DAO |
| `providers_seed.rs` | 供应商种子数据 |
| `universal_providers.rs` | 通用供应商 DAO |
| `mcp.rs` | MCP 服务器 DAO |
| `prompts.rs` | Prompt DAO |
| `skills.rs` | Skill DAO |
| `profiles.rs` | Profile DAO |
| `proxy.rs` | 代理配置 DAO |
| `settings.rs` | 设置 DAO |
| `failover.rs` | 故障转移 DAO |
| `stream_check.rs` | 流式检查 DAO |
| `usage_rollup.rs` | 用量聚合 DAO |

### 3.5 数据库 (`src-tauri/src/database/`)

- **引擎**：SQLite (rusqlite)，文件路径 `~/.cc-switch/cc-switch.db`
- **并发安全**：`Mutex<Connection>` 保护，宏 `lock_conn!` 获取锁
- **Schema 版本**：`SCHEMA_VERSION` 常量，`user_version` PRAGMA 追踪
- **迁移**：`migration.rs` + `schema.rs` 中的 `apply_schema_migrations()`
- **备份**：`backup.rs` 提供 SQLite backup API

**核心表**（19 张）：

| # | 表名 | 主键 | 用途 |
|---|------|------|------|
| 1 | providers | (id, app_type) | 供应商 |
| 2 | provider_endpoints | id AUTO | 供应商端点 |
| 3 | mcp_servers | id | MCP 服务器 |
| 4 | prompts | (id, app_type) | Prompt |
| 5 | skills | id | Skill |
| 6 | skill_repos | (owner, name) | Skill 仓库 |
| 7 | settings | key | 全局设置 |
| 8 | proxy_config | app_type | 代理配置 |
| 9 | provider_health | (provider_id, app_type) | 供应商健康 |
| 10 | proxy_request_logs | request_id | 请求日志 |
| 11 | model_pricing | model_id | 模型定价 |
| 12 | stream_check_logs | id AUTO | 流式检查日志 |
| 16 | proxy_live_backup | app_type | Live 配置备份 |
| 17 | usage_daily_rollups | (date, app_type, provider_id, model, ...) | 日聚合 |
| 18 | session_log_sync | file_path | 会话日志同步 |
| 19 | profiles | id | Profile |

## 4. 代理服务器架构

### 4.1 整体结构

```
AI CLI 工具 → HTTP 请求 → Axum Proxy Server → Provider Router → 上游 API
                              ↓                      ↓
                         Handler 处理           熔断器检查
                              ↓                      ↓
                         故障转移管理          供应商选择
                              ↓
                         响应处理 (SSE/JSON)
                              ↓
                         日志记录 + 用量统计
```

### 4.2 核心模块 (`src-tauri/src/proxy/`)

| 模块 | 职责 |
|------|------|
| `server.rs` | Axum HTTP 服务器，手动 hyper accept loop |
| `handlers.rs` | 请求处理器 |
| `forwarder.rs` | 请求转发 |
| `provider_router.rs` | 供应商路由选择 |
| `failover_switch.rs` | 故障转移切换管理 |
| `circuit_breaker.rs` | 熔断器 |
| `response_handler.rs` | 响应处理（流式/非流式） |
| `response_processor.rs` | 响应后处理 |
| `sse.rs` | SSE 流处理 |
| `session.rs` | 会话 ID 提取 |
| `model_mapper.rs` | 模型映射 |
| `thinking_optimizer.rs` | Thinking 预算优化 |
| `thinking_rectifier.rs` | Thinking 修正 |
| `thinking_budget_rectifier.rs` | Thinking 预算修正 |
| `copilot_optimizer.rs` | Copilot 请求优化 |
| `gemini_url.rs` | Gemini URL 处理 |
| `media_sanitizer.rs` | 媒体内容清理 |
| `cache_injector.rs` | 缓存注入 |
| `body_filter.rs` | 请求体过滤 |
| `content_encoding.rs` | 内容编码处理 |
| `error_mapper.rs` | 错误映射 |
| `health.rs` | 健康检查 |
| `http_client.rs` | HTTP 客户端 |
| `hyper_client.rs` | Hyper 客户端 |
| `log_codes.rs` | 日志编码 |
| `json_canonical.rs` | JSON 规范化 |
| `switch_lock.rs` | 切换锁 |
| `handler_config.rs` | Handler 配置 |
| `handler_context.rs` | Handler 上下文 |
| `types.rs` | 类型定义 |
| `error.rs` | 错误定义 |
| `usage/` | 用量统计子模块 |
| `providers/` | 供应商特定处理子模块 |

### 4.3 代理状态管理

```rust
pub struct ProxyState {
    db: Arc<Database>,
    config: Arc<RwLock<ProxyConfig>>,
    status: Arc<RwLock<ProxyStatus>>,
    current_providers: Arc<RwLock<HashMap<String, (String, String)>>>,
    provider_router: Arc<ProviderRouter>,
    gemini_shadow: Arc<GeminiShadowStore>,
    codex_chat_history: Arc<CodexChatHistoryStore>,
    app_handle: Option<tauri::AppHandle>,
    failover_manager: Arc<FailoverSwitchManager>,
}
```

## 5. MCP 同步架构

每种受管应用有独立的 MCP 同步实现：

```
McpService
  ├── mcp/claude.rs    → ~/.claude/settings.json (mcpServers)
  ├── mcp/codex.rs     → ~/.codex/config.toml    (mcp_servers)
  ├── mcp/gemini.rs    → ~/.gemini/settings.json (mcpServers)
  ├── mcp/opencode.rs  → opencode.json           (mcp_servers)
  ├── mcp/hermes.rs    → config.yaml             (mcp)
  └── mcp/codefree.rs  → codefree 配置
```

**同步操作**：
- `sync_single_server_to_*`：同步单个 MCP 服务器到目标应用
- `sync_enabled_to_*`：同步所有启用的 MCP 服务器
- `import_from_*`：从目标应用导入 MCP 服务器
- `remove_server_from_*`：从目标应用移除 MCP 服务器

## 6. 会话管理架构

```
session_manager/
  ├── mod.rs              # 统一扫描入口（并行 7 种工具）
  ├── providers/
  │   ├── claude.rs       # Claude 会话扫描
  │   ├── codex.rs        # Codex 会话扫描
  │   ├── gemini.rs       # Gemini 会话扫描
  │   ├── opencode.rs     # OpenCode 会话扫描
  │   ├── openclaw.rs     # OpenClaw 会话扫描
  │   ├── hermes.rs       # Hermes 会话扫描
  │   └── codefree.rs     # CodeFree 会话扫描
  └── terminal/           # 终端集成
```

## 7. 配置文件架构

### 7.1 CC Switch 自身配置

- **数据库**：`~/.cc-switch/cc-switch.db`（SQLite）
- **配置目录**：`~/.cc-switch/`
- **原子写入**：临时文件 + rename 模式

### 7.2 受管应用配置路径

| AppType | 配置路径 | 格式 |
|---------|----------|------|
| Claude | `~/.claude/settings.json` | JSON |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) | JSON |
| Codex | `~/.codex/config.toml` | TOML |
| Gemini | `~/.gemini/settings.json` | JSON |
| OpenCode | `opencode.json` | TOML |
| OpenClaw | `openclaw.json` | JSON |
| Hermes | `config.yaml` | YAML |
| CodeFree | CodeFree 配置 | JSON |

## 8. 错误处理架构

- **Rust 端**：`thiserror` 定义 `AppError` 枚举，支持本地化错误消息
- **前端**：`extractErrorMessage()` 统一提取错误信息
- **代理**：`ProxyError` 枚举，`error_mapper.rs` 映射上游错误

## 9. 测试架构

### 9.1 前端测试

```
tests/
├── components/     # 组件测试
├── config/         # 配置测试
├── hooks/          # Hook 测试
├── integration/    # 集成测试
├── lib/            # 库测试
├── msw/            # MSW Mock 处理器
├── utils/          # 测试工具
├── setupGlobals.ts # 全局 Setup
└── setupTests.ts   # 测试 Setup
```

- **框架**：Vitest + jsdom
- **Mock**：MSW (Mock Service Worker) mock Tauri API
- **组件测试**：@testing-library/react

### 9.2 后端测试

- **框架**：cargo test
- **Feature gate**：`test-hooks` feature 用于测试专用 hook
- **数据库测试**：`database/tests.rs`，使用内存 SQLite
- **并发测试**：`serial_test` crate 保证顺序

## 10. 构建与发布

- **前端构建**：Vite → `dist/`
- **后端构建**：Cargo → Tauri bundle
- **Release 优化**：LTO thin, opt-level=s, strip=symbols, codegen-units=1
- **平台支持**：macOS (arm64/x86_64), Windows (x64/arm64), Linux (x64)
- **自动更新**：tauri-plugin-updater
- **单实例**：tauri-plugin-single-instance
- **DeepLink**：tauri-plugin-deep-link (`cc-switch://`)
