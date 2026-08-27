# CC Switch 新人上手指南

## 1. 环境准备

### 1.1 必备工具

| 工具 | 版本要求 | 安装方式 |
|------|----------|----------|
| Node.js | 22.12.0+ | [nvm](https://github.com/nvm-sh/nvm) 或 [fnm](https://github.com/Schniz/fnm) |
| pnpm | 最新版 | `npm install -g pnpm` |
| Rust | 1.95+ | [rustup](https://rustup.rs)（项目含 `rust-toolchain.toml`，自动安装） |
| Tauri CLI | 2.x | `pnpm add -D @tauri-apps/cli`（已含在 devDependencies） |

### 1.2 平台依赖

- **macOS**：Xcode Command Line Tools (`xcode-select --install`)
- **Windows**：Visual Studio Build Tools + WebView2
- **Linux**：`libwebkit2gtk-4.1-dev` 等依赖（见 [Tauri 文档](https://v2.tauri.app/start/prerequisites/)）

### 1.3 首次启动

```bash
# 克隆仓库
git clone <repo-url> cc-switch
cd cc-switch

# 安装前端依赖
pnpm install

# 开发模式启动（前端热重载 + Rust 编译）
pnpm dev
```

## 2. 项目结构速览

```
cc-switch/
├── src/                    # 前端源码（React + TypeScript）
│   ├── components/         # UI 组件（38 个子目录）
│   ├── hooks/              # 业务逻辑 Hook（26 个文件）
│   ├── lib/
│   │   ├── api/            # Tauri IPC 封装（26 个模块）★ 入口层
│   │   ├── query/          # TanStack Query 配置
│   │   ├── schemas/        # Zod 校验
│   │   └── errors/         # 错误处理
│   ├── config/             # 预设配置（供应商/MCP/模板）
│   ├── types/              # TypeScript 类型
│   └── locales/            # 国际化翻译（zh/zh-TW/en/ja）
├── src-tauri/              # 后端源码（Rust + Tauri 2）
│   └── src/
│       ├── commands/       # Tauri Command 层（34 个文件）★ API 层
│       ├── services/       # 业务逻辑层（34 个文件）★ 核心逻辑
│       ├── database/       # 数据库层
│       │   ├── dao/        # 数据访问（13 个文件）
│       │   ├── schema.rs   # 表结构定义
│       │   └── migration.rs # 迁移脚本
│       ├── proxy/          # HTTP 代理服务器（34 个文件）★ 核心模块
│       ├── mcp/            # MCP 同步（7 种工具）
│       ├── session_manager/ # 会话扫描
│       └── deeplink/       # DeepLink 处理
├── tests/                  # 前端测试
├── project-kb/             # 项目知识库
│   ├── domain.md           # 领域模型
│   └── architecture.md     # 技术架构
└── AGENTS.md               # AI 协作指南
```

## 3. 核心概念

### 3.1 AppType（8 种受管工具）

| AppType | 模式 | MCP | Skill | 配置格式 |
|---------|------|-----|-------|----------|
| Claude | Switch | ✅ | ✅ | JSON |
| ClaudeDesktop | Switch | ❌ | ❌ | JSON |
| Codex | Switch | ✅ | ✅ | TOML |
| Gemini | Switch | ✅ | ✅ | JSON |
| OpenCode | Additive | ✅ | ✅ | TOML |
| OpenClaw | Additive | ❌ | ❌ | JSON |
| Hermes | Additive | ✅ | ❌ | YAML |
| CodeFree | Additive | ✅ | ❌ | JSON |

- **Switch 模式**：同一时刻只有一个"当前"供应商，代理路由到当前供应商
- **Additive 模式**：所有启用的供应商都写入配置文件，工具自行选择

### 3.2 代理服务器

本地 HTTP 代理（默认 `127.0.0.1:15721`），拦截 AI CLI 工具请求：

```
AI CLI → http://127.0.0.1:15721/v1/chat/completions → 代理 → 上游 API
```

核心能力：供应商切换、故障转移、熔断器、用量统计、模型映射、Thinking 优化。

### 3.3 数据流

```
前端组件 → Hook → lib/api (invoke) → Rust Command → Service → DAO → SQLite
前端组件 ← Hook ← lib/api (invoke) ← Rust Command ← Service ← DAO ← SQLite
前端组件 ← useTauriEvent ← Tauri Event ← Service (状态变更推送)
```

## 4. 开发工作流

### 4.1 常用命令

```bash
pnpm dev              # 开发模式（前端热重载）
pnpm build:renderer   # 仅构建前端
pnpm build            # 完整构建（Tauri 打包）
pnpm typecheck        # TypeScript 类型检查
pnpm format:check     # 代码格式检查
pnpm test:unit        # 前端单元测试
cd src-tauri && cargo test        # Rust 测试
cd src-tauri && cargo clippy      # Rust Lint
cd src-tauri && cargo fmt         # Rust 格式化
```

### 4.2 添加新功能的步骤

**前端**：
1. 在 `src/lib/api/` 添加 IPC 封装
2. 在 `src/lib/query/` 添加 Query/Mutation
3. 在 `src/hooks/` 添加业务 Hook
4. 在 `src/components/` 添加 UI 组件
5. 更新 `src/locales/` 四种语言翻译

**后端**：
1. 在 `src-tauri/src/database/dao/` 添加 DAO（如需新表，先写迁移）
2. 在 `src-tauri/src/services/` 添加 Service
3. 在 `src-tauri/src/commands/` 添加 Command
4. 在 `src-tauri/src/lib.rs` 注册 Command

### 4.3 数据库变更

1. 在 `schema.rs` 的 `create_tables_on_conn` 添加新表
2. Bump `SCHEMA_VERSION`
3. 在 `migration.rs` 的 `apply_schema_migrations_on_conn` 添加迁移分支
4. 在 `dao/` 添加对应 DAO

## 5. 调试技巧

### 5.1 前端调试
- 浏览器 DevTools 正常可用（WebView）
- TanStack Query DevTools 已集成
- MSW mock Tauri API 进行单元测试

### 5.2 后端调试
- `RUST_LOG=debug pnpm dev` 查看详细日志
- `RUST_LOG=cc_switch::proxy=trace` 仅代理模块 trace
- 数据库文件：`~/.cc-switch/cc-switch.db`，可用 SQLite 工具查看

### 5.3 代理调试
- 代理日志在 `proxy_request_logs` 表
- 健康检查：`GET http://127.0.0.1:15721/health`
- 熔断器状态在 `provider_health` 表

## 6. 代码规范速查

| 规则 | 说明 |
|------|------|
| 前端 IPC | 禁止组件直接调用 `@tauri-apps/api`，必须通过 `src/lib/api/` |
| 后端分层 | Command → Service → DAO，禁止跨层调用 |
| 错误处理 | Rust 禁止 `unwrap()`，使用 `AppError` + `thiserror` |
| 原子写入 | 配置文件写入使用临时文件 + rename |
| shadcn/ui | 禁止手动修改 `src/components/ui/`，通过 CLI 重新生成 |
| i18n | 新增文本必须同时更新 zh/zh-TW/en/ja 四个文件 |
| 数据库 | 新增表/列必须写迁移脚本，禁止直接改 schema |
| 路径别名 | 前端使用 `@/` 前缀导入 |

## 7. 相关文档

- [AGENTS.md](../AGENTS.md) - AI 协作指南
- [domain.md](domain.md) - 领域模型
- [architecture.md](architecture.md) - 技术架构
- [Tauri 2 文档](https://v2.tauri.app/)
- [shadcn/ui 文档](https://ui.shadcn.com/)
- [TanStack Query v5 文档](https://tanstack.com/query/latest)
