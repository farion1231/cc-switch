# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

CC-Switch 是一个桌面应用程序,用于在 Claude Code、Codex 和 Gemini CLI 之间切换和管理 API 配置。使用 Tauri 2.8 + Rust 构建,前端是 React 18 + TypeScript,采用 SQLite + JSON 双层存储架构。

## 开发命令

### 包管理器
- 使用 `pnpm` 作为 JavaScript/TypeScript 包管理器
- 使用 `cargo` 作为 Rust 包管理器

### 前端开发
```bash
# 类型检查
pnpm typecheck

# 代码格式化
pnpm format        # 写入
pnpm format:check  # 只检查

# 单元测试 (使用 vitest + MSW)
pnpm test:unit         # 运行一次
pnpm test:unit:watch   # 监视模式

# 开发服务器 (包含热重载)
pnpm dev               # 完整 Tauri 应用
pnpm dev:renderer      # 只启动前端

# 构建前端
pnpm build:renderer
```

### 后端开发 (Rust)
```bash
cd src-tauri

# 代码格式化和检查
cargo fmt
cargo clippy

# 运行测试
cargo test                    # 运行所有测试
cargo test test_name          # 运行特定测试
cargo test --features test-hooks  # 带测试钩子运行

# 构建完整应用
pnpm build                    # 从项目根目录
cargo tauri build --debug     # 调试版本
```

### 运行单个测试
```bash
# 前端测试
pnpm test:unit useProviderActions

# 后端测试
cargo test test_provider_switch
```

## 架构设计原则

### 双向同步架构
应用采用 SSOT (Single Source of Truth) + 双向同步模式:
- **SSOT**: 所有数据存储在 `~/.cc-switch/cc-switch.db` (SQLite)
- **双向写入**: 切换 provider 时写入到各个 CLI 的 live config,编辑 active provider 时回写到数据库
- **原子写入**: 使用临时文件 + 重命名模式防止配置损坏 (tempfile + rename)

### 分层架构

```
Frontend (React + TS)
├── Components (UI)    → src/components/
├── Hooks (业务逻辑)    → src/hooks/
├── API Wrapper (类型安全) → src/lib/api/
└── TanStack Query (缓存/同步) → src/lib/query/

Backend (Rust)
├── Commands (Tauri IPC 层) → src-tauri/src/commands/
├── Services (业务逻辑层)    → src-tauri/src/services/
├── Database (数据访问层)    → src-tauri/src/database/
└── Domain Models (领域模型) → src-tauri/src/*.rs
```

### 数据流向
1. **前端发起操作** → 调用 `src/lib/api/*.ts` 的类型安全 API
2. **Tauri IPC** → `src-tauri/src/commands/` 接收请求
3. **业务逻辑** → `src-tauri/src/services/` 处理核心逻辑
4. **数据持久化** → `src-tauri/src/database/dao/` 操作 SQLite
5. **状态同步** → TanStack Query 自动刷新前端缓存

### 关键服务层

**ProviderService** (`src-tauri/src/services/provider.rs`)
- Provider 的 CRUD 操作
- 切换、回填、排序功能
- 支持通用 provider (Universal Provider)

**McpService** (`src-tauri/src/services/mcp.rs`)
- MCP 服务器的统一管理
- 支持 stdio/http/sse 三种传输类型
- 跨应用 (Claude/Codex/Gemini) 同步

**ConfigService** (`src-tauri/src/services/config.rs`)
- 配置导入/导出
- 自动备份轮转 (保留最近 10 个)

**ProxyService** (`src-tauri/src/services/proxy.rs`)
- 本地 API 代理服务器 (Axum-based)
- 自动故障转移 (Circuit Breaker)
- 按应用接管和独立队列

**SpeedtestService** (`src-tauri/src/services/speedtest.rs`)
- API 端点延迟测量
- 流式检查功能

### 数据库架构

**Schema 版本**: 当前为 v5 (`src-tauri/src/database/mod.rs` 中的 `SCHEMA_VERSION`)
- 每次修改表结构时递增版本号
- 在 `schema.rs` 中添加相应的迁移逻辑
- 使用 `rusqlite` with bundled SQLite

**DAO 模式** (`src-tauri/src/database/dao/`)
- `providers.rs` - Provider CRUD
- `mcp.rs` - MCP 服务器配置
- `prompts.rs` - 提示词管理
- `skills.rs` - Skills 管理
- `settings.rs` - 设置存储

**并发安全**: Database 使用 `Arc<Mutex<Connection>>` 保护数据库连接

### 全局状态管理

**AppState** (`src-tauri/src/store.rs`)
```rust
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
}
```
- 通过 Tauri 的 `manage()` API 注入到所有 commands
- 所有 Tauri commands 通过 `State<AppState>` 访问

### 前端状态管理

**TanStack Query v5**
- 所有异步操作通过 `src/lib/query/` 的 queries 和 mutations
- API 调用通过 `src/lib/api/*.ts` 的类型安全包装器
- Hooks 封装在 `src/hooks/` 中 (如 `useProviderActions`)

**测试覆盖**: Hooks 单元测试覆盖率 100%
- 使用 MSW (Mock Service Worker) 模拟 Tauri API
- 测试文件: `tests/hooks/*.test.tsx`

### 配置文件路径

**Claude Code**:
- Live config: `~/.claude/settings.json` 或 `claude.json`
- MCP: `~/.claude.json` → `mcpServers`

**Codex**:
- Live config: `~/.codex/auth.json` (必需) + `config.toml` (可选)
- MCP: `~/.codex/config.toml` → `[mcp_servers]` 表

**Gemini**:
- Live config: `~/.gemini/.env` (API key) + `~/.gemini/settings.json`
- MCP: `~/.gemini/settings.json` → `mcpServers`

**CC Switch 存储**:
- 数据库 (SSOT): `~/.cc-switch/cc-switch.db` (SQLite)
- 本地设置: `~/.cc-switch/settings.json`
- 备份: `~/.cc-switch/backups/` (自动轮转,保留 10 个)

### MCP 同步机制

每个应用使用不同的配置格式:
- **Claude**: JSON (`.claude.json`)
- **Codex**: TOML (`config.toml`)
- **Gemini**: JSON (`settings.json`)

同步逻辑在 `src-tauri/src/mcp/` 和 `src-tauri/src/gemini_mcp.rs`/`claude_mcp.rs`:
- `sync_enabled_to_*()` - 同步所有启用的服务器
- `sync_single_server_to_*()` - 同步单个服务器
- `import_from_*()` - 从应用导入现有配置

### 错误处理

**统一错误类型** (`src-tauri/src/error.rs`):
- 使用 `thiserror` 定义 `AppError`
- 所有 Tauri commands 返回 `Result<T, String>`
- 前端通过 `throw new Error()` 处理

### 环境变量冲突检测

**EnvChecker** (`src-tauri/src/services/env_checker.rs`)
- 自动检测跨应用配置冲突 (Claude/Codex/Gemini/MCP)
- 视觉冲突指示器 + 解决建议

### 深链接协议

**ccswitch://** 协议 (`src-tauri/src/deeplink/`)
- 用于通过共享链接导入 provider 配置
- 安全验证 + 生命周期集成

### 国际化 (i18n)

- 支持中文/英文/日文
- 翻译文件: `src/i18n/locales/{zh,en,ja}.json`
- 使用 `react-i18next` 进行前端翻译

### 代码风格

**Rust**:
- 使用 `cargo fmt` 格式化
- 使用 `cargo clippy` 检查
- 遵循 Rust API 指南

**TypeScript**:
- 使用 `prettier` 格式化
- 严格模式: `tsconfig.json` 中 `"strict": true`
- 使用 `@dnd-kit` 进行拖放操作

### 常见开发任务

**添加新的 Tauri Command**:
1. 在 `src-tauri/src/commands/` 中添加函数,使用 `#[tauri::command]`
2. 在 `src-tauri/src/lib.rs` 的 `invoke_handler()` 中注册
3. 在 `src/lib/api/*.ts` 中添加类型安全的包装器
4. 在前端使用 TanStack Query 的 mutation 或 query

**修改数据库 Schema**:
1. 在 `src-tauri/src/database/schema.rs` 中更新 `SCHEMA_VERSION`
2. 在 `migration.rs` 中添加迁移逻辑
3. 在相应的 DAO 文件中更新 CRUD 操作
4. 编写测试验证迁移

**添加新的 Provider 预设**:
1. 在 `src/config/presets/*.ts` 中添加预设配置
2. 在 `src-tauri/src/provider_defaults.rs` 中添加默认值
3. 更新相关的类型定义

**添加新的 MCP 模板**:
1. 在 `src/config/mcp-templates.ts` 中添加模板
2. 更新 `src-tauri/src/mcp/` 中的验证逻辑
3. 在 MCP 管理面板中测试

### 调试技巧

**启用详细日志** (Rust):
- 日志通过 `tauri-plugin-log` 配置
- 查看 `src-tauri/tauri.conf.json` 中的日志配置

**前端调试**:
- 使用 React DevTools
- TanStack Query DevTools (`src/lib/query/queryClient.ts` 中配置)

**测试数据库**:
- 使用临时目录进行测试 (`tempfile` crate)
- 测试钩子 feature: `--features test-hooks`

### 平台特定代码

**Windows**: `target.os = "windows"` (注册表操作等)
**macOS**: `target.os = "macos"` (LaunchAgent, NSColor 等)
**Linux**: `target.os = "linux"` (XDG autostart, WebKitGTK 等)

### 性能优化

**前端**:
- 使用 `@tanstack/react-query` 缓存 API 调用
- 组件懒加载 (`React.lazy()` + `Suspense`)

**后端**:
- 数据库连接池 (`Arc<Mutex<Connection>>`)
- 异步操作 (`tokio`)
- 原子写入防止数据损坏

### 构建产物

**Windows**: `.msi` (安装包) / `.zip` (便携版)
**macOS**: `.zip` / `.dmg`
**Linux**: `.deb` / `.rpm` / `.AppImage` / `.flatpak`

**优化配置** (`.cargo/config.toml` 或 `Cargo.toml`):
- `lto = "thin"` - 链接时优化
- `opt-level = "s"` - 优化体积
- `strip = true` - 移除符号表
