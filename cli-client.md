# CC-Switch CLI Client

## 概述

CC-Switch CLI 是 CC-Switch 的命令行版本，提供与 GUI 版本相同的核心功能，适用于：
- 服务器环境 / 无头环境
- CI/CD 集成
- 自动化脚本
- 高级用户的快速操作

## 架构设计

### 项目结构

```
cc-switch/
├── crates/
│   ├── cc-switch-core/              # 共享核心库
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs               # 统一导出
│   │       ├── app_config.rs        # AppType, McpServer 等数据模型
│   │       ├── provider.rs          # Provider 数据模型
│   │       ├── error.rs             # AppError 错误类型
│   │       ├── settings.rs          # AppSettings 设置
│   │       ├── database/            # SQLite 数据持久化
│   │       │   ├── mod.rs
│   │       │   ├── schema.rs        # 表结构 + 迁移
│   │       │   └── dao/             # 数据访问对象
│   │       ├── services/            # 业务逻辑服务
│   │       │   ├── mod.rs
│   │       │   ├── provider.rs      # ProviderService
│   │       │   ├── mcp.rs           # McpService
│   │       │   ├── proxy.rs         # ProxyService
│   │       │   ├── prompt.rs        # PromptService
│   │       │   └── skill.rs         # SkillService
│   │       ├── mcp/                 # MCP 配置处理
│   │       │   ├── mod.rs
│   │       │   ├── claude.rs
│   │       │   ├── codex.rs
│   │       │   ├── gemini.rs
│   │       │   └── opencode.rs
│   │       ├── proxy/               # 内置代理服务器
│   │       │   ├── mod.rs
│   │       │   ├── server.rs
│   │       │   ├── handlers.rs
│   │       │   ├── health.rs
│   │       │   └── failover.rs
│   │       ├── config/              # 配置文件处理
│   │       │   ├── claude.rs
│   │       │   ├── codex.rs
│   │       │   ├── gemini.rs
│   │       │   └── opencode.rs
│   │       └── utils/               # 工具函数
│   │           ├── toml.rs
│   │           └── env.rs
│   │
│   └── cc-switch-cli/               # CLI 客户端
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs              # 入口
│           ├── cli.rs               # clap 命令定义
│           ├── handlers/            # 命令处理器
│           │   ├── mod.rs
│           │   ├── provider.rs
│           │   ├── mcp.rs
│           │   ├── proxy.rs
│           │   ├── prompt.rs
│           │   ├── skill.rs
│           │   ├── config.rs
│           │   └── usage.rs
│           ├── output/              # 输出格式化
│           │   ├── mod.rs
│           │   ├── table.rs
│           │   ├── json.rs
│           │   └── yaml.rs
│           └── interactive.rs       # 交互式输入
│
├── src-tauri/                       # Tauri GUI (保留)
│   ├── Cargo.toml                   # depends on cc-switch-core
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── commands/                # Tauri commands (简化为适配层)
│       └── tray.rs
│
└── src/                             # React 前端 (保留)
```

### 依赖关系

```
┌─────────────────┐     ┌─────────────────┐
│  cc-switch-cli  │     │   src-tauri     │
│  (CLI Client)   │     │  (Tauri GUI)    │
└────────┬────────┘     └────────┬────────┘
         │                       │
         └───────────┬───────────┘
                     │
                     ▼
         ┌─────────────────────┐
         │   cc-switch-core    │
         │   (共享核心库)       │
         ├─────────────────────┤
         │ • Services          │
         │ • Database          │
         │ • Data Models       │
         │ • MCP/Proxy Core    │
         └─────────────────────┘
```

### 技术栈

| 组件 | 技术 |
|------|------|
| 命令行框架 | clap 4.x (derive) |
| 交互式输入 | dialoguer |
| 表格输出 | tabled |
| 彩色输出 | colored |
| 异步运行时 | tokio |
| 序列化 | serde + serde_json |
| 数据库 | rusqlite (bundled) |

## 功能列表

### Provider 管理

| 命令 | 描述 |
|------|------|
| `provider list` | 列出所有供应商 |
| `provider show <id>` | 显示供应商详情 |
| `provider add` | 添加新供应商 |
| `provider edit <id>` | 编辑供应商 |
| `provider delete <id>` | 删除供应商 |
| `provider switch <id>` | 切换当前供应商 |
| `provider usage <id>` | 查询用量 |
| `provider universal list` | 列出通用供应商 |
| `provider universal add` | 添加通用供应商 |
| `provider universal sync <id>` | 同步通用供应商 |

### MCP 管理

| 命令 | 描述 |
|------|------|
| `mcp list` | 列出所有 MCP 服务器 |
| `mcp show <id>` | 显示服务器详情 |
| `mcp add` | 添加服务器 |
| `mcp edit <id>` | 编辑服务器 |
| `mcp delete <id>` | 删除服务器 |
| `mcp enable <id>` | 启用服务器 (指定应用) |
| `mcp disable <id>` | 禁用服务器 (指定应用) |
| `mcp import` | 从各应用导入配置 |

### Proxy 管理

| 命令 | 描述 |
|------|------|
| `proxy start` | 启动代理服务器 |
| `proxy stop` | 停止代理服务器 |
| `proxy status` | 显示代理状态 |
| `proxy config show` | 显示代理配置 |
| `proxy config set` | 修改代理配置 |
| `proxy takeover enable` | 启用应用接管 |
| `proxy takeover disable` | 禁用应用接管 |
| `proxy failover queue show` | 显示故障转移队列 |
| `proxy failover queue add` | 添加到队列 |
| `proxy failover switch` | 切换供应商 |
| `proxy circuit show` | 显示熔断器状态 |
| `proxy circuit reset` | 重置熔断器 |

### Prompt 管理

| 命令 | 描述 |
|------|------|
| `prompt list` | 列出所有 Prompts |
| `prompt show <id>` | 显示 Prompt 详情 |
| `prompt add` | 添加 Prompt |
| `prompt edit <id>` | 编辑 Prompt |
| `prompt delete <id>` | 删除 Prompt |
| `prompt enable <id>` | 启用 Prompt |
| `prompt import` | 从文件导入 |

### Skill 管理

| 命令 | 描述 |
|------|------|
| `skill list` | 列出已安装 Skills |
| `skill search <keyword>` | 搜索 Skills |
| `skill install <id>` | 安装 Skill |
| `skill uninstall <id>` | 卸载 Skill |
| `skill enable <id>` | 启用 Skill |
| `skill disable <id>` | 禁用 Skill |

### 配置与工具

| 命令 | 描述 |
|------|------|
| `config show` | 显示配置 |
| `config set <key> <value>` | 设置配置项 |
| `config get <key>` | 获取配置项 |
| `config path` | 显示配置文件路径 |
| `usage summary` | 用量统计 |
| `usage logs` | 请求日志 |
| `export` | 导出配置备份 |
| `import` | 导入配置 |
| `import-deeplink <url>` | 导入深链接 |

## 命令详细设计

### 全局选项

```
cc-switch [OPTIONS] <COMMAND>

Options:
  -f, --format <FORMAT>  输出格式 [default: table] [possible: table, json, yaml]
  -q, --quiet            静默模式，仅输出错误
  -v, --verbose          详细输出
  -h, --help             显示帮助
  -V, --version          显示版本
```

### Provider 命令

```bash
# 列出供应商
cc-switch provider list --app claude [--format json]

# 显示详情
cc-switch provider show openai --app claude

# 添加供应商 (交互式)
cc-switch provider add --app claude

# 添加供应商 (命令行)
cc-switch provider add --app claude \
  --name "OpenAI" \
  --base-url "https://api.openai.com/v1" \
  --api-key "sk-xxx"

# 从 JSON 文件添加
cc-switch provider add --app claude --from-json provider.json

# 编辑供应商
cc-switch provider edit openai --app claude \
  --set-api-key "sk-new-key" \
  --set-base-url "https://new-url.com"

# 删除供应商 (需要确认)
cc-switch provider delete openai --app claude

# 强制删除
cc-switch provider delete openai --app claude --yes

# 切换供应商
cc-switch provider switch anthropic --app claude

# 查询用量
cc-switch provider usage openai --app claude
```

### MCP 命令

```bash
# 列出服务器
cc-switch mcp list

# 添加服务器 (交互式)
cc-switch mcp add

# 添加服务器 (命令行)
cc-switch mcp add \
  --id "filesystem" \
  --command "npx" \
  --args "-y,@modelcontextprotocol/server-filesystem,~/" \
  --apps claude,codex

# 从 JSON 添加
cc-switch mcp add --from-json mcp.json

# 编辑服务器 (启用/禁用应用)
cc-switch mcp edit filesystem --enable-app gemini --disable-app codex

# 删除服务器
cc-switch mcp delete filesystem --yes

# 启用/禁用
cc-switch mcp enable filesystem --app claude
cc-switch mcp disable filesystem --app codex

# 从各应用导入
cc-switch mcp import
```

### Proxy 命令

```bash
# 启动代理
cc-switch proxy start [--port 9527] [--host 127.0.0.1]

# 停止代理
cc-switch proxy stop

# 查看状态
cc-switch proxy status

# 查看配置
cc-switch proxy config show

# 修改配置
cc-switch proxy config set --port 8080 --log-enabled true

# 接管管理
cc-switch proxy takeover status
cc-switch proxy takeover enable --app claude
cc-switch proxy takeover disable --app claude

# 故障转移队列
cc-switch proxy failover queue show --app claude
cc-switch proxy failover queue add openai --app claude --priority 1
cc-switch proxy failover queue remove openai --app claude
cc-switch proxy failover switch anthropic --app claude

# 熔断器
cc-switch proxy circuit show openai --app claude
cc-switch proxy circuit reset openai --app claude
cc-switch proxy circuit config show
cc-switch proxy circuit config set --failure-threshold 5 --recovery-timeout 60
```

### Prompt 命令

```bash
# 列出
cc-switch prompt list --app claude

# 添加 (从文件)
cc-switch prompt add --app claude --id "code-review" --file ./prompt.md

# 编辑
cc-switch prompt edit code-review --app claude --file ./prompt.md

# 删除
cc-switch prompt delete code-review --app claude --yes

# 启用
cc-switch prompt enable code-review --app claude

# 导入
cc-switch prompt import --app claude
```

### Skill 命令

```bash
# 列出已安装
cc-switch skill list

# 搜索
cc-switch skill search "git"

# 安装
cc-switch skill install "skill-id" --app claude

# 卸载
cc-switch skill uninstall "skill-id" --yes

# 启用/禁用
cc-switch skill enable "skill-id" --app claude
cc-switch skill disable "skill-id" --app codex
```

### Config 命令

```bash
# 显示所有配置
cc-switch config show

# 获取单个配置
cc-switch config get language

# 设置配置
cc-switch config set language zh

# 显示配置路径
cc-switch config path
```

### Usage 命令

```bash
# 用量统计
cc-switch usage summary --app claude --days 7

# 请求日志
cc-switch usage logs --app claude --from 2024-01-01 --to 2024-01-31

# 导出
cc-switch usage export --output usage.csv
```

### 导入导出

```bash
# 导出配置
cc-switch export --output backup.json

# 导入配置 (覆盖)
cc-switch import --input backup.json

# 导入配置 (合并)
cc-switch import --input backup.json --merge

# 深链接导入
cc-switch import-deeplink "ccswitch://provider?name=OpenAI&..."
```

---

## 开发任务计划

### Phase 0: 准备工作

- [ ] **T-0.1** 创建 `crates/` 目录结构
- [ ] **T-0.2** 创建根目录 Cargo.toml workspace 配置
- [ ] **T-0.3** 创建 `crates/cc-switch-core/Cargo.toml`
- [ ] **T-0.4** 创建 `crates/cc-switch-cli/Cargo.toml`

### Phase 1: 核心库抽取 (预计 3 天)

#### 1.1 基础模块迁移

- [ ] **T-1.1.1** 迁移 `error.rs` → `cc-switch-core/src/error.rs`
- [ ] **T-1.1.2** 迁移 `app_config.rs` → `cc-switch-core/src/app_config.rs`
  - 移除 Tauri 依赖
  - 保持 `AppType`, `McpServer`, `McpApps` 等数据结构
- [ ] **T-1.1.3** 迁移 `provider.rs` → `cc-switch-core/src/provider.rs`
- [ ] **T-1.1.4** 迁移 `settings.rs` → `cc-switch-core/src/settings.rs`
- [ ] **T-1.1.5** 迁移 `prompt.rs` → `cc-switch-core/src/prompt.rs`
- [ ] **T-1.1.6** 创建 `cc-switch-core/src/lib.rs` 统一导出

#### 1.2 数据库层迁移

- [ ] **T-1.2.1** 迁移 `database/mod.rs`
- [ ] **T-1.2.2** 迁移 `database/schema.rs`
- [ ] **T-1.2.3** 迁移 `database/dao/` 所有文件
  - providers.rs
  - mcp.rs
  - prompts.rs
  - skills.rs
  - settings.rs
  - proxy.rs
  - failover.rs
- [ ] **T-1.2.4** 迁移 `database/backup.rs`
- [ ] **T-1.2.5** 迁移 `database/migration.rs`
- [ ] **T-1.2.6** 编写数据库层单元测试

#### 1.3 Services 层迁移

- [ ] **T-1.3.1** 迁移 `services/provider/mod.rs`
  - 移除 `tauri::State` 依赖
  - 改用 `&AppState` 参数
- [ ] **T-1.3.2** 迁移 `services/provider/live.rs`
- [ ] **T-1.3.3** 迁移 `services/provider/endpoints.rs`
- [ ] **T-1.3.4** 迁移 `services/provider/usage.rs`
- [ ] **T-1.3.5** 迁移 `services/provider/gemini_auth.rs`
- [ ] **T-1.3.6** 迁移 `services/mcp.rs`
- [ ] **T-1.3.7** 迁移 `services/prompt.rs`
- [ ] **T-1.3.8** 迁移 `services/skill.rs`
- [ ] **T-1.3.9** 迁移 `services/proxy.rs`
- [ ] **T-1.3.10** 迁移 `services/config.rs`
- [ ] **T-1.3.11** 迁移 `services/env_checker.rs`
- [ ] **T-1.3.12** 迁移 `services/env_manager.rs`
- [ ] **T-1.3.13** 迁移 `services/speedtest.rs`
- [ ] **T-1.3.14** 迁移 `services/stream_check.rs`
- [ ] **T-1.3.15** 迁移 `services/usage_stats.rs`
- [ ] **T-1.3.16** 编写 Services 层单元测试

#### 1.4 MCP 配置模块迁移

- [ ] **T-1.4.1** 迁移 `mcp/mod.rs`
- [ ] **T-1.4.2** 迁移 `mcp/claude.rs`
- [ ] **T-1.4.3** 迁移 `mcp/codex.rs`
- [ ] **T-1.4.4** 迁移 `mcp/gemini.rs`
- [ ] **T-1.4.5** 迁移 `mcp/opencode.rs`
- [ ] **T-1.4.6** 迁移 `mcp/validation.rs`

#### 1.5 Proxy 模块迁移

- [ ] **T-1.5.1** 迁移 `proxy/mod.rs`
- [ ] **T-1.5.2** 迁移 `proxy/server.rs`
- [ ] **T-1.5.3** 迁移 `proxy/handlers.rs`
- [ ] **T-1.5.4** 迁移 `proxy/handler_context.rs`
- [ ] **T-1.5.5** 迁移 `proxy/provider_router.rs`
- [ ] **T-1.5.6** 迁移 `proxy/health.rs`
- [ ] **T-1.5.7** 迁移 `proxy/circuit_breaker.rs`
- [ ] **T-1.5.8** 迁移 `proxy/failover_switch.rs`
- [ ] **T-1.5.9** 迁移 `proxy/providers/` 子模块
- [ ] **T-1.5.10** 迁移 `proxy/usage/` 子模块
- [ ] **T-1.5.11** 迁移 `proxy/types.rs`

#### 1.6 应用配置文件处理迁移

- [ ] **T-1.6.1** 迁移 `claude_mcp.rs`
- [ ] **T-1.6.2** 迁移 `claude_plugin.rs`
- [ ] **T-1.6.3** 迁移 `codex_config.rs`
- [ ] **T-1.6.4** 迁移 `gemini_config.rs`
- [ ] **T-1.6.5** 迁移 `gemini_mcp.rs`
- [ ] **T-1.6.6** 迁移 `opencode_config.rs`

#### 1.7 其他模块迁移

- [ ] **T-1.7.1** 迁移 `config.rs`
- [ ] **T-1.7.2** 迁移 `prompt_files.rs`
- [ ] **T-1.7.3** 迁移 `provider_defaults.rs`
- [ ] **T-1.7.4** 迁移 `deeplink/` 模块
- [ ] **T-1.7.5** 创建 `store.rs` (AppState)

#### 1.8 核心库完善

- [ ] **T-1.8.1** 处理所有 `pub(crate)` 可见性问题
- [ ] **T-1.8.2** 解决循环依赖
- [ ] **T-1.8.3** 编译验证
- [ ] **T-1.8.4** 运行所有单元测试
- [ ] **T-1.8.5** 文档注释补充

### Phase 2: Tauri 项目改造 (预计 1 天)

- [ ] **T-2.1** 修改 `src-tauri/Cargo.toml`，添加对 `cc-switch-core` 依赖
- [ ] **T-2.2** 删除 `src-tauri/src/` 下已迁移的文件
- [ ] **T-2.3** 改造 `commands/provider.rs`，调用 core 层
- [ ] **T-2.4** 改造 `commands/mcp.rs`，调用 core 层
- [ ] **T-2.5** 改造 `commands/proxy.rs`，调用 core 层
- [ ] **T-2.6** 改造 `commands/prompt.rs`，调用 core 层
- [ ] **T-2.7** 改造 `commands/skill.rs`，调用 core 层
- [ ] **T-2.8** 改造 `commands/settings.rs`，调用 core 层
- [ ] **T-2.9** 改造 `commands/config.rs`，调用 core 层
- [ ] **T-2.10** 改造 `commands/usage.rs`，调用 core 层
- [ ] **T-2.11** 改造 `commands/import_export.rs`，调用 core 层
- [ ] **T-2.12** 改造其他 commands 文件
- [ ] **T-2.13** 更新 `src-tauri/src/lib.rs`
- [ ] **T-2.14** 编译验证 Tauri 项目
- [ ] **T-2.15** 运行 Tauri GUI 功能测试

### Phase 3: CLI 项目开发 (预计 4 天)

#### 3.1 CLI 框架搭建

- [ ] **T-3.1.1** 创建 `cc-switch-cli/src/main.rs` 入口
- [ ] **T-3.1.2** 创建 `cc-switch-cli/src/cli.rs` clap 命令定义
  - 定义 `Cli` struct
  - 定义 `Commands` enum
  - 定义各子命令 enum
- [ ] **T-3.1.3** 创建 `cc-switch-cli/src/handlers/mod.rs` 分发器
- [ ] **T-3.1.4** 实现全局选项处理

#### 3.2 Provider 命令实现

- [ ] **T-3.2.1** 实现 `handlers/provider.rs`
- [ ] **T-3.2.2** 实现 `provider list` 命令
- [ ] **T-3.2.3** 实现 `provider show` 命令
- [ ] **T-3.2.4** 实现 `provider add` 命令 (命令行参数)
- [ ] **T-3.2.5** 实现 `provider add` 命令 (交互式)
- [ ] **T-3.2.6** 实现 `provider add --from-json` 命令
- [ ] **T-3.2.7** 实现 `provider edit` 命令
- [ ] **T-3.2.8** 实现 `provider delete` 命令
- [ ] **T-3.2.9** 实现 `provider switch` 命令
- [ ] **T-3.2.10** 实现 `provider usage` 命令
- [ ] **T-3.2.11** 实现 `provider universal` 子命令

#### 3.3 MCP 命令实现

- [ ] **T-3.3.1** 实现 `handlers/mcp.rs`
- [ ] **T-3.3.2** 实现 `mcp list` 命令
- [ ] **T-3.3.3** 实现 `mcp show` 命令
- [ ] **T-3.3.4** 实现 `mcp add` 命令 (命令行参数)
- [ ] **T-3.3.5** 实现 `mcp add` 命令 (交互式)
- [ ] **T-3.3.6** 实现 `mcp add --from-json` 命令
- [ ] **T-3.3.7** 实现 `mcp edit` 命令
- [ ] **T-3.3.8** 实现 `mcp delete` 命令
- [ ] **T-3.3.9** 实现 `mcp enable/disable` 命令
- [ ] **T-3.3.10** 实现 `mcp import` 命令

#### 3.4 Proxy 命令实现

- [ ] **T-3.4.1** 实现 `handlers/proxy.rs`
- [ ] **T-3.4.2** 实现 `proxy start` 命令
- [ ] **T-3.4.3** 实现 `proxy stop` 命令
- [ ] **T-3.4.4** 实现 `proxy status` 命令
- [ ] **T-3.4.5** 实现 `proxy config show/set` 命令
- [ ] **T-3.4.6** 实现 `proxy takeover enable/disable/status` 命令
- [ ] **T-3.4.7** 实现 `proxy failover queue show/add/remove` 命令
- [ ] **T-3.4.8** 实现 `proxy failover switch` 命令
- [ ] **T-3.4.9** 实现 `proxy circuit show/reset` 命令
- [ ] **T-3.4.10** 实现 `proxy circuit config show/set` 命令

#### 3.5 Prompt 命令实现

- [ ] **T-3.5.1** 实现 `handlers/prompt.rs`
- [ ] **T-3.5.2** 实现 `prompt list` 命令
- [ ] **T-3.5.3** 实现 `prompt show` 命令
- [ ] **T-3.5.4** 实现 `prompt add` 命令
- [ ] **T-3.5.5** 实现 `prompt edit` 命令
- [ ] **T-3.5.6** 实现 `prompt delete` 命令
- [ ] **T-3.5.7** 实现 `prompt enable` 命令
- [ ] **T-3.5.8** 实现 `prompt import` 命令

#### 3.6 Skill 命令实现

- [ ] **T-3.6.1** 实现 `handlers/skill.rs`
- [ ] **T-3.6.2** 实现 `skill list` 命令
- [ ] **T-3.6.3** 实现 `skill search` 命令
- [ ] **T-3.6.4** 实现 `skill install` 命令
- [ ] **T-3.6.5** 实现 `skill uninstall` 命令
- [ ] **T-3.6.6** 实现 `skill enable/disable` 命令

#### 3.7 Config & Usage 命令实现

- [ ] **T-3.7.1** 实现 `handlers/config.rs`
- [ ] **T-3.7.2** 实现 `config show` 命令
- [ ] **T-3.7.3** 实现 `config get/set` 命令
- [ ] **T-3.7.4** 实现 `config path` 命令
- [ ] **T-3.7.5** 实现 `handlers/usage.rs`
- [ ] **T-3.7.6** 实现 `usage summary` 命令
- [ ] **T-3.7.7** 实现 `usage logs` 命令
- [ ] **T-3.7.8** 实现 `usage export` 命令

#### 3.8 导入导出命令实现

- [ ] **T-3.8.1** 实现 `export` 命令
- [ ] **T-3.8.2** 实现 `import` 命令
- [ ] **T-3.8.3** 实现 `import-deeplink` 命令

### Phase 4: 输出格式化 (预计 1 天)

- [ ] **T-4.1** 创建 `output/mod.rs` OutputPrinter trait
- [ ] **T-4.2** 实现 `output/table.rs` 表格输出
  - Provider 表格
  - MCP 表格
  - Prompt 表格
  - Skill 表格
  - Usage 表格
- [ ] **T-4.3** 实现 `output/json.rs` JSON 输出
- [ ] **T-4.4** 实现 `output/yaml.rs` YAML 输出
- [ ] **T-4.5** 实现彩色输出支持

### Phase 5: 交互式输入 (预计 1 天)

- [ ] **T-5.1** 创建 `interactive.rs`
- [ ] **T-5.2** 实现文本输入 (带验证)
- [ ] **T-5.3** 实现密码输入 (隐藏显示)
- [ ] **T-5.4** 实现选择列表
- [ ] **T-5.5** 实现确认对话框
- [ ] **T-5.6** 实现 Provider 交互式添加向导
- [ ] **T-5.7** 实现 MCP 交互式添加向导

### Phase 6: 测试与文档 (预计 2 天)

#### 6.1 单元测试

- [ ] **T-6.1.1** 编写 CLI handlers 单元测试
- [ ] **T-6.1.2** 编写 output 模块测试
- [ ] **T-6.1.3** 编写 interactive 模块测试

#### 6.2 集成测试

- [ ] **T-6.2.1** 编写 Provider 命令集成测试
- [ ] **T-6.2.2** 编写 MCP 命令集成测试
- [ ] **T-6.2.3** 编写 Proxy 命令集成测试
- [ ] **T-6.2.4** 编写导入导出测试

#### 6.3 文档

- [ ] **T-6.3.1** 编写 README.md
- [ ] **T-6.3.2** 编写安装文档
- [ ] **T-6.3.3** 编写命令参考文档
- [ ] **T-6.3.4** 编写示例脚本

### Phase 7: 发布准备 (预计 1 天)

- [ ] **T-7.1** 配置 GitHub Actions CI/CD
- [ ] **T-7.2** 配置多平台编译
  - macOS (x86_64, aarch64)
  - Linux (x86_64)
  - Windows (x86_64)
- [ ] **T-7.3** 配置 Homebrew formula
- [ ] **T-7.4** 配置发布脚本
- [ ] **T-7.5** 创建 v1.0.0 release

---

## 风险与依赖

### 技术风险

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 循环依赖 | 编译失败 | 仔细规划模块边界，使用 trait 解耦 |
| Tauri 依赖耦合 | 改造困难 | Phase 1 优先处理，确保 core 无 Tauri 依赖 |
| 异步运行时冲突 | 运行时错误 | 统一使用 tokio |

### 外部依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| clap | 4.x | 命令行解析 |
| dialoguer | 0.11 | 交互式输入 |
| tabled | 0.15 | 表格输出 |
| colored | 2.x | 彩色输出 |
| tokio | 1.x | 异步运行时 |

---

## 时间估算

| 阶段 | 预计时间 | 备注 |
|------|----------|------|
| Phase 0: 准备工作 | 0.5 天 | 创建目录结构 |
| Phase 1: 核心库抽取 | 3 天 | 最关键阶段 |
| Phase 2: Tauri 改造 | 1 天 | 确保 GUI 不受影响 |
| Phase 3: CLI 开发 | 4 天 | 主要开发工作 |
| Phase 4: 输出格式化 | 1 天 | |
| Phase 5: 交互式输入 | 1 天 | |
| Phase 6: 测试与文档 | 2 天 | |
| Phase 7: 发布准备 | 1 天 | |
| **总计** | **13.5 天** | 约 3 周 |

---

## 开发进度

### Phase 0: 准备工作 ✅ (已完成)

- [x] 创建 `crates/` 目录结构
- [x] 创建根目录 Cargo.toml workspace 配置
- [x] 创建 `crates/cc-switch-core/Cargo.toml`
- [x] 创建 `crates/cc-switch-cli/Cargo.toml`
- [x] 创建 CLI 入口文件 (main.rs, cli.rs)

### Phase 1: 核心库抽取 ✅ (已完成)

- [x] 迁移 error.rs
- [x] 迁移 app_config.rs (AppType, McpApps, McpServer, SkillApps, InstalledSkill)
- [x] 迁移 provider.rs (Provider, UniversalProvider, ProviderMeta)
- [x] 迁移 prompt.rs (Prompt)
- [x] 迁移 settings.rs (AppSettings, SyncMethod)
- [x] 迁移 config.rs (配置工具函数)
- [x] 迁移 database/ (schema.rs, dao.rs)
- [x] 迁移 services/ (provider, mcp, prompt, skill, config, proxy)
- [x] 创建 mcp/ 模块 (claude, codex, gemini, opencode, validation)
- [x] 创建 proxy/ 模块 (circuit_breaker, health)
- [x] 创建 store.rs (AppState)

### Phase 2: Tauri 项目改造 (待完成)

- [ ] 修改 src-tauri/Cargo.toml 依赖 cc-switch-core
- [ ] 更新 Tauri commands 使用 core 层服务
- [ ] 确保 GUI 功能正常

### Phase 3: CLI 项目开发 ✅ (基础功能完成)

- [x] provider list/show/switch/delete 命令
- [x] mcp list/show/toggle 命令
- [x] prompt list/show/enable 命令
- [x] config show/path/get/set 命令
- [x] proxy status/config/takeover 命令
- [x] import/export 命令
- [ ] provider add/edit 交互式输入
- [ ] mcp add 交互式输入
- [ ] skill install/uninstall 命令
- [ ] 速度测试功能

### Phase 4: 输出格式化 ✅ (已完成)

- [x] table 格式输出 (tabled)
- [x] json 格式输出
- [x] yaml 格式输出

### Phase 5: 交互式输入 (待完成)

- [ ] dialoguer 集成
- [ ] provider add/edit 交互式表单
- [ ] mcp add 交互式表单
- [ ] 确认对话框

### Phase 6: 测试与文档 (待完成)

- [ ] 单元测试
- [ ] 集成测试
- [ ] README 文档

### Phase 7: 发布准备 (待完成)

- [ ] GitHub Actions CI/CD
- [ ] 多平台二进制编译
- [ ] Homebrew formula
- [ ] 发布 v1.0.0

---

## 当前状态

**构建状态**: ✅ 编译通过
**测试状态**: ✅ 基本命令可用
- `cc-switch provider list` - 正常
- `cc-switch mcp list` - 正常
- `cc-switch config path` - 正常

---

## 里程碑

- **M1** (Day 4): 核心库抽取完成，Tauri GUI 正常运行
- **M2** (Day 8): CLI 基础命令实现完成
- **M3** (Day 11): CLI 所有功能实现完成
- **M4** (Day 13): 测试与文档完成
- **M5** (Day 14): v1.0.0 发布
