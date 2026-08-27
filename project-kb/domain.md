# CC Switch 领域模型

## 1. 核心领域

CC Switch 的核心业务是**统一管理多个 AI CLI 工具的配置**。用户无需手动编辑各工具的 JSON/TOML/YAML 配置文件，而是在一个桌面应用中可视化管理。

### 1.1 受管应用 (AppType)

CC Switch 管理以下 8 种 AI CLI 工具：

| AppType | 标识 | 配置格式 | 供应商模式 |
|---------|------|----------|-----------|
| Claude | `claude` | JSON | Switch（单选当前） |
| Claude Desktop | `claude-desktop` | JSON | Switch（单选当前） |
| Codex | `codex` | TOML | Switch（单选当前） |
| Gemini CLI | `gemini` | JSON | Switch（单选当前） |
| OpenCode | `opencode` | TOML | Additive（全量写入） |
| OpenClaw | `openclaw` | JSON | Additive（全量写入） |
| Hermes | `hermes` | YAML | Additive（全量写入） |
| CodeFree | `codefree` | JSON | Additive（全量写入） |

**供应商模式说明**：
- **Switch 模式**：同一时刻只有一个供应商生效，切换时仅将当前供应商写入 live 配置
- **Additive 模式**：所有供应商都写入 live 配置，由目标工具自行选择

### 1.2 供应商 (Provider)

供应商是 CC Switch 最核心的实体，代表一个 API 端点配置。

**核心属性**：
- `id`：唯一标识（复合主键：id + app_type）
- `name`：显示名称
- `settings_config`：供应商配置 JSON（包含 baseUrl、apiKey、模型列表等）
- `icon` / `icon_color`：图标和颜色
- `in_failover_queue`：是否加入故障转移队列
- `meta`：元数据（不写入 live 配置，仅存于 CC Switch 数据库）
- `is_current`：是否为当前激活供应商

**供应商类型**（从 settings_config 解析）：
- `codex_oauth`：Codex OAuth 官方认证
- `github_copilot`：GitHub Copilot 托管账户
- 通用第三方：自定义 baseUrl + apiKey

**供应商预设**：每种 AppType 有独立的预设列表（`src/config/*ProviderPresets.ts`），包含主流 API 供应商的默认配置。

### 1.3 MCP 服务器 (McpServer)

MCP (Model Context Protocol) 服务器为 AI 工具提供扩展能力。

**核心属性**：
- `id`：唯一标识
- `name`：显示名称
- `server`：服务器配置 JSON（包含 command、args、env 等）
- `apps`：启用状态（`McpApps`，标记应用到哪些客户端）
- `description` / `homepage` / `docs` / `tags`：元信息

**同步机制**：每个 MCP 服务器可独立同步到多个受管应用，同步逻辑按 AppType 分别实现（`src-tauri/src/mcp/claude.rs`、`codex.rs`、`gemini.rs` 等）。

**注意**：OpenClaw 和 Claude Desktop 不支持 MCP 同步。

### 1.4 Prompts

Prompt 是用户自定义的系统提示词，按 AppType 分别管理。

**核心属性**：
- `id` + `app_type`：复合主键
- `name`：显示名称
- `content`：提示词内容
- `enabled`：是否启用

**同步机制**：启用的 Prompt 写入各工具的配置文件（Claude 写入 `CLAUDE.md`，Codex 写入 `instructions.md`，Gemini 写入 `GEMINI.md` 等）。

### 1.5 Skills

Skill 是可安装的 AI 能力扩展包，通常来自 GitHub 仓库。

**核心属性**：
- `id`：格式 `"owner/repo:directory"` 或 `"local:directory"`
- `name` / `description`：显示信息
- `directory`：安装目录名
- `repo_owner` / `repo_name` / `repo_branch`：来源仓库信息
- `apps`：启用状态（`SkillApps`）
- `content_hash`：SHA-256 哈希，用于更新检测

**Skill 仓库 (SkillRepo)**：
- `owner` + `name`：复合主键
- `branch`：默认分支
- `enabled`：是否启用自动同步

**注意**：OpenClaw、Claude Desktop 和 CodeFree 不支持 Skill 同步。

### 1.6 Profiles

Profile 是项目级配置快照，保存供应商/MCP/Skills/Prompt 的完整状态。

**核心属性**：
- `id`：唯一标识
- `name`：显示名称
- `payload`：快照 JSON（按 AppType 分槽存储）
- `sort_order`：排序

**当前 Profile**：通过 `settings` 表的 `current_profile_id_<scope>` 键管理，按 AppType 独立维护。

## 2. 代理与故障转移领域

### 2.1 代理服务器 (Proxy)

CC Switch 内置 HTTP 代理服务器，拦截 AI CLI 工具的 API 请求，实现供应商切换和故障转移。

**核心配置**（按 AppType 独立，`proxy_config` 表）：
- `proxy_enabled`：是否启用代理
- `listen_address` / `listen_port`：监听地址（默认 `127.0.0.1:15721`）
- `enable_logging`：是否记录请求日志
- `auto_failover_enabled`：是否启用自动故障转移
- 超时配置：`streaming_first_byte_timeout`、`streaming_idle_timeout`、`non_streaming_timeout`
- 熔断器配置：`circuit_failure_threshold`、`circuit_success_threshold`、`circuit_timeout_seconds` 等

**代理状态**：
- `enabled`：代理服务是否运行
- `running`：代理进程是否活跃
- `current_providers`：每个 AppType 当前使用的供应商

### 2.2 故障转移 (Failover)

当当前供应商请求失败时，自动切换到队列中的下一个供应商。

**故障转移队列**：基于 `providers` 表的 `in_failover_queue` 字段和 `sort_index` 排序。

**故障转移切换管理器 (FailoverSwitchManager)**：
- 去重控制：避免多个请求同时触发相同切换
- 切换后更新托盘菜单和前端状态

### 2.3 熔断器 (Circuit Breaker)

保护机制，防止持续向故障供应商发送请求。

**状态**：`Closed`（正常）→ `Open`（熔断）→ `HalfOpen`（试探）

**配置**：
- `failure_threshold`：连续失败次数阈值
- `success_threshold`：半开状态成功次数阈值
- `timeout_seconds`：熔断持续时间
- `error_rate_threshold`：错误率阈值
- `min_requests`：最小请求数

### 2.4 请求日志 (ProxyRequestLog)

记录每次代理请求的详细信息，用于用量统计和成本追踪。

**核心字段**：
- `provider_id` / `app_type` / `model`：请求标识
- `input_tokens` / `output_tokens`：Token 用量
- `cache_read_tokens` / `cache_creation_tokens`：缓存 Token
- `input_cost_usd` / `output_cost_usd` / `total_cost_usd`：成本（USD）
- `latency_ms` / `first_token_ms` / `duration_ms`：延迟指标
- `status_code` / `error_message`：请求结果
- `is_streaming`：是否流式请求
- `cost_multiplier`：成本倍率
- `pricing_model`：实际计价模型名

## 3. 用量统计领域

### 3.1 日聚合 (UsageDailyRollup)

按日期、AppType、供应商、模型聚合的统计数据，用于仪表盘展示。

**核心字段**：
- `date` + `app_type` + `provider_id` + `model`：复合主键
- `request_count` / `success_count`：请求统计
- Token 和成本聚合字段

### 3.2 模型定价 (ModelPricing)

各模型的定价信息，用于成本计算。

**核心字段**：
- `model_id`：模型标识
- `input_cost_per_million` / `output_cost_per_million`：每百万 Token 价格
- `cache_read_cost_per_million` / `cache_creation_cost_per_million`：缓存价格

### 3.3 实时事件推送

当 `proxy_request_logs` 写入新数据时，通过 Tauri 事件 `usage-log-recorded` 通知前端刷新，200ms 防抖合并。

## 4. 会话管理领域

### 4.1 会话元数据 (SessionMeta)

从各 AI CLI 工具的本地存储中扫描会话信息。

**核心字段**：
- `provider_id` / `session_id`：会话标识
- `title` / `summary`：会话摘要
- `project_dir`：项目目录
- `created_at` / `last_active_at`：时间信息
- `source_path`：源文件路径
- `resume_command`：恢复命令

**扫描机制**：并行扫描 7 种工具的会话目录（`session_manager/providers/`），使用 `std::thread::scope` 并发执行。

### 4.2 会话消息 (SessionMessage)

单条对话消息，包含 `role`、`content`、`ts`（时间戳）。

## 5. 同步与备份领域

### 5.1 配置同步

- **S3 同步**：`services/s3_sync.rs` + `services/s3_auto_sync.rs`
- **WebDAV 同步**：`services/webdav_sync.rs` + `services/webdav_auto_sync.rs`
- **同步协议**：`services/sync_protocol.rs` 定义通用同步数据结构

### 5.2 导入导出

- 导出：将 CC Switch 配置打包为 ZIP 文件
- 导入：从 ZIP 文件恢复配置
- DeepLink 导入：通过 `cc-switch://` 协议导入供应商/MCP/Skill/Prompt

### 5.3 数据库备份

- `database/backup.rs`：SQLite 数据库备份功能
- `proxy_live_backup` 表：代理 live 配置备份

## 6. 设置领域

### 6.1 全局设置 (Settings)

键值对存储（`settings` 表），包含：
- `current_profile_id_<scope>`：各 AppType 的当前 Profile
- `visible_apps`：主页面显示的应用配置（`VisibleApps`）
- 其他全局偏好设置

### 6.2 环境变量 (Env)

按 AppType 管理环境变量，写入各工具的 `.env` 文件或配置中。

## 7. 领域实体关系

```
AppType (8种)
  ├── Provider (1:N, 复合主键 id+app_type)
  │     ├── ProviderHealth (1:1)
  │     ├── ProviderEndpoint (1:N)
  │     └── in_failover_queue → FailoverQueue
  ├── McpServer (M:N, 通过 McpApps 关联)
  ├── Prompt (1:N, 复合主键 id+app_type)
  ├── Skill (M:N, 通过 SkillApps 关联)
  │     └── SkillRepo (来源)
  ├── Profile (1:N, payload 按 app 分槽)
  ├── ProxyConfig (1:1, 按 app_type)
  ├── ProxyRequestLog (1:N)
  └── UsageDailyRollup (1:N)

Provider → ProxyRequestLog (1:N, 记录请求)
ProxyRequestLog → UsageDailyRollup (聚合)
ModelPricing (全局, 按模型定价)
```
