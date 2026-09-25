# Contributing to CC Switch

> [中文版本](#贡献指南)

Thank you for your interest in contributing to CC Switch! Please read our [Code of Conduct](./CODE_OF_CONDUCT.md) before participating.

## How to Contribute

There are many ways to contribute:

- **Report bugs** — Found something broken? [Open a bug report](https://github.com/farion1231/cc-switch/issues/new?template=bug_report.yml).
- **Suggest features** — Have an idea? [Submit a feature request](https://github.com/farion1231/cc-switch/issues/new?template=feature_request.yml).
- **Improve docs** — Spot a typo or missing info? [Report a doc issue](https://github.com/farion1231/cc-switch/issues/new?template=doc_issue.yml).
- **Contribute code** — Fix bugs or implement features via pull requests.
- **Translate** — Help us improve translations for Simplified Chinese, Traditional Chinese, English, and Japanese.

> **Security vulnerabilities**: Please do NOT use public issues. See our [Security Policy](./SECURITY.md) instead.

## Development Setup

### Prerequisites

- **Node.js** 20.19+ or 22.12+ (`.node-version` at the repository root is 22.12.0)
- **pnpm** 10: the version is pinned by the `packageManager` field in `package.json`; run `corepack enable` once and that version is used automatically. To upgrade pnpm, edit this field directly (Dependabot no longer does it)
- **Rust** 1.95: pinned by `rust-toolchain.toml`; rustup installs it automatically on the first build
- Platform-specific [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/): Windows needs the Microsoft C++ Build Tools and WebView2 (usually preinstalled on Windows 10/11); macOS needs the Xcode Command Line Tools; on Debian / Ubuntu you can use the install command from CI:

  ```bash
  sudo apt-get install build-essential pkg-config libssl-dev libgtk-3-dev librsvg2-dev \
    libayatana-appindicator3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev
  ```

- The Tauri CLI is a dev dependency installed by `pnpm install`; no global install is needed

### Quick Start

```bash
# Install dependencies
pnpm install

# Start development server with hot reload
pnpm dev
```

`pnpm dev` first starts Vite on `http://localhost:3000`, then opens the Tauri window. If port 3000 is already in use, it fails with an error instead of switching to another port.

### Useful Commands

| Command | Description |
|---------|-------------|
| `pnpm dev` | Start dev server (hot reload) |
| `pnpm build` | Production build and packaging |
| `pnpm build:renderer` | Build the frontend only, output to `dist/` |
| `pnpm typecheck` | TypeScript type checking (strict; unused imports or parameters are errors) |
| `pnpm format` | Format `src/` with Prettier |
| `pnpm format:check` | Check code formatting in `src/` |
| `pnpm test:unit` | Run frontend unit tests |
| `pnpm test:unit:watch` | Run frontend unit tests in watch mode |

Rust backend commands (run inside `src-tauri/`):

```bash
cd src-tauri
cargo fmt                     # Format Rust code
cargo clippy -- -D warnings   # Clippy check (same as CI, warnings are errors)
cargo test                    # Run all backend tests
cargo test some_test_name     # Run tests matching a name
```

> On a fresh clone where the frontend has never been built, first run `mkdir -p dist` (or `pnpm build:renderer`) at the repository root. Otherwise, cargo commands under `src-tauri` fail to compile because the frontend output directory is missing.

### Local Build

As its final step, `pnpm build` signs the auto-update bundles, which requires the release private key `TAURI_SIGNING_PRIVATE_KEY`. If you don't have the key locally, you can turn off updater bundle generation:

```bash
pnpm tauri build -c '{"bundle":{"createUpdaterArtifacts":false}}'
```

For a debug build, append `--debug` to the command above. This single-quote form works on macOS and Linux; on Windows, save `{"bundle":{"createUpdaterArtifacts":false}}` to a JSON file and pass it with `-c <file path>`.

## Testing

**Frontend**: vitest runs `*.test.ts(x)` under `tests/` and `src/`; Tauri calls are mocked with MSW (see `tests/msw/`); component tests use @testing-library/react.

**Backend**: unit tests live next to the source (`#[cfg(test)]`), and integration tests live in `src-tauri/tests/`. CI runs `cargo test` on Linux, macOS, and Windows.

> **Isolate your real config**: some backend tests read and write directories such as `~/.cc-switch` and `~/.codex`. When running them locally, point `CC_SWITCH_TEST_HOME` at a temporary directory:
>
> ```bash
> CC_SWITCH_TEST_HOME="$(mktemp -d)" cargo test
> ```
>
> Set only this variable, not `HOME`: changing `HOME` also changes the default Cargo and rustup directories, which triggers a full rebuild.

## Architecture

### System Overview

```
Frontend (React + TypeScript)
  Components ── Hooks ── TanStack Query ── src/lib/api
                              │ Tauri IPC (invoke)
Backend (Tauri 2 + Rust)
  Commands ─► Services ─┬─► DAO ─► SQLite (~/.cc-switch/cc-switch.db)
                        ├─► Live config writers (atomic write)
                        │     ~/.claude, ~/.codex, ~/.gemini, ...
                        └─► Local routing (proxy/): forwarding, format
                              conversion, failover, usage accounting
```

### Core Design

- **SSOT** (single source of truth): providers, MCP, prompts, Skills, projects, usage, and other data are all stored in `~/.cc-switch/cc-switch.db` (SQLite)
- **Device-level settings**: settings that belong to this machine only, such as directory overrides and backup policy, are stored in `~/.cc-switch/settings.json` and are not cloud-synced
- **Two write modes**: for switch-mode tools (Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build), switching first backfills the current live config into the current provider (except for Claude Desktop), then writes the new provider; for coexist-mode tools (OpenCode, OpenClaw, Hermes, Pi, MiniMax Code), all providers coexist in a single live file and nothing is backfilled
- **Atomic writes**: all live configs are written via "temp file + rename" to avoid corruption
- **Concurrency safety**: the database connection is protected by a Mutex to avoid race conditions
- **Layered architecture**: Commands (parameter validation and forwarding) → Services (business logic) → DAO → Database; the Commands layer stays thin
- **Local routing**: a separate path from "writing live configs" that takes over requests from Claude Code, Codex, Gemini CLI, and Grok Build on the local machine (Claude Desktop also routes through it when "Model Mapping" is selected), handling forwarding, format conversion, failover, and billing

### Core Components

- **ProviderService**: provider CRUD, switching, backfill, and sorting
- **McpService**: MCP server management, importing from each tool, and syncing to live files
- **SkillService / PromptService**: installing and syncing Skills, and each tool's prompt files
- **ProfileService**: saving and switching project snapshots
- **ProxyService**: starting and stopping the local routing service, and toggling local routing per tool
- **session_manager module**: scanning, browsing, and resuming each tool's session history
- **SpeedtestService**: endpoint latency testing
- **database/backup.rs**: database import/export and backup rotation

### Tech Stack

**Frontend**: React 18 · TypeScript · Vite 7 · TailwindCSS 3.4 · shadcn/ui (Radix) · TanStack Query v5 · react-hook-form + zod · react-i18next · @dnd-kit · CodeMirror 6 · Recharts

**Backend**: Tauri 2 · Rust · tokio · serde · rusqlite (bundled SQLite) · axum / hyper / reqwest (local routing) · rquickjs (usage scripts) · Tauri plugins updater / process / dialog / store / log / deep-link / single-instance / window-state / opener

**Testing**: vitest · MSW · @testing-library/react · cargo test (serial_test, tempfile)

### Project Structure

```
├── src/                      # Frontend (React + TypeScript)
│   ├── components/           # One directory per feature: providers, mcp, prompts, skills, sessions,
│   │                         #   proxy, usage, profiles, hermes, openclaw, workspace, settings, ui, etc.
│   ├── config/               # Provider presets per tool (*ProviderPresets.ts), MCP presets
│   ├── hooks/                # Custom hooks (business logic)
│   ├── lib/api/              # Tauri invoke wrappers (lib/query/ holds the TanStack Query config)
│   ├── i18n/locales/         # UI strings (zh / zh-TW / en / ja)
│   ├── icons/                # Provider icons (index is maintained by hand)
│   └── types/ utils/ contexts/
├── src-tauri/                # Backend (Rust)
│   ├── src/
│   │   ├── lib.rs            # Plugins, tray, command registration
│   │   ├── commands/         # Tauri command layer (one file per domain)
│   │   ├── services/         # Business logic layer
│   │   ├── database/         # Schema and migrations (schema.rs), backup (backup.rs), DAO (dao/)
│   │   ├── proxy/            # Local routing: forwarding, format conversion, circuit breaking, billing
│   │   ├── mcp/              # MCP sync for each tool
│   │   ├── session_manager/  # Session scanning and terminal resume
│   │   ├── deeplink/         # ccswitch:// protocol
│   │   └── *_config.rs, pi_config/  # Live config read/write for each tool
│   └── tests/                # Backend integration tests
├── tests/                    # Frontend tests (vitest)
├── docs/                     # User manual, guides, release notes
├── flatpak/                  # Flatpak manifest and build notes
└── assets/                   # Screenshots and partner assets
```

## Code Style

- **Frontend**: Prettier for formatting (`src/` only, default config), strict TypeScript (`pnpm typecheck`); the project does not use ESLint
- **Backend**: `cargo fmt` for formatting, `cargo clippy -- -D warnings` for linting
- **Tauri 2 commands**: the command name is the Rust function name and stays snake_case; when calling `invoke` from the frontend, write argument names in camelCase (e.g., Rust `provider_id` becomes `providerId` on the frontend). Register new commands in `generate_handler!` in `src-tauri/src/lib.rs`, and put the frontend wrappers in `src/lib/api/`

Run the same checks as CI before submitting:

```bash
pnpm typecheck && pnpm format:check && pnpm test:unit && pnpm build:renderer
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

## Pull Request Guidelines

1. **Open an issue first** for new features — PRs for features that are not a good fit may be closed.
2. **Fork and branch** — Create a feature branch from `main` (e.g., `feat/my-feature` or `fix/issue-123`).
3. **Keep PRs focused** — One feature or fix per PR. Avoid unrelated changes.
4. **Follow the PR template** — Fill in the summary, related issue, and checklist.

### PR Checklist

- [ ] `pnpm typecheck` passes
- [ ] `pnpm format:check` passes
- [ ] `pnpm test:unit` passes
- [ ] If Rust code changed: `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` pass
- [ ] If user-facing text changed: all four locale files are updated

### Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(provider): add support for new provider
fix(tray): resolve menu not updating after switch
docs(readme): update installation instructions
ci: add format check workflow
chore(deps): update dependencies
```

## AI-Assisted Contributions

We welcome AI-assisted contributions, but **the responsibility stays with you**. AI tools lower the cost of writing code — they do not lower the cost of reviewing it. Maintainers are not obligated to clean up AI-generated output.

By submitting a PR, you agree to the following:

1. **You have read and understood your code.** You must be able to explain any line in your PR. If you cannot, it is not ready for review.
2. **You have tested it yourself.** Every change must be verified locally — not just "it looks right." Do not submit code for platforms or features you cannot test.
3. **PRs must be small and focused.** One issue, one PR. Large, sprawling, multi-topic PRs will be closed.
4. **Open an issue first.** Drive-by PRs with no prior discussion — especially AI-generated ones — may be closed without review.
5. **Maintainers may close without explanation.** PRs that appear to be unreviewed AI output — hallucinated fixes, unnecessary refactors, bulk changes with no context — may be closed at the maintainer's discretion.

**In short**: AI is a tool, not a substitute for understanding. Use it to help you contribute better, not to shift work onto maintainers.

## Internationalization (i18n)

The CC Switch UI supports four languages: Simplified Chinese, Traditional Chinese, English, and Japanese. When modifying user-facing text:

1. Update **all four** locale files:
   - `src/i18n/locales/zh.json`
   - `src/i18n/locales/zh-TW.json`
   - `src/i18n/locales/en.json`
   - `src/i18n/locales/ja.json`
2. Use the `t()` function from i18next for all UI text.
3. Never hardcode user-facing strings.

> Forgetting to update a locale file does not fail the build: the missing strings fall back to English, and if the English file lacks them too, the UI shows the raw key.

## Questions?

- [Open a question](https://github.com/farion1231/cc-switch/issues/new?template=question.yml)
- [GitHub Discussions](https://github.com/farion1231/cc-switch/discussions)

---

# 贡献指南

> [English Version](#contributing-to-cc-switch)

感谢你对 CC Switch 的贡献兴趣！参与之前请阅读我们的[行为准则](./CODE_OF_CONDUCT.md)。

## 如何贡献

你可以通过多种方式参与贡献：

- **报告 Bug** — 发现问题？[提交 Bug 报告](https://github.com/farion1231/cc-switch/issues/new?template=bug_report.yml)。
- **建议功能** — 有想法？[提交功能请求](https://github.com/farion1231/cc-switch/issues/new?template=feature_request.yml)。
- **改进文档** — 发现错误或缺失？[报告文档问题](https://github.com/farion1231/cc-switch/issues/new?template=doc_issue.yml)。
- **贡献代码** — 通过 Pull Request 修复 Bug 或实现新功能。
- **翻译** — 帮助改进简体中文、繁体中文、英文和日文的翻译。

> **安全漏洞**：请不要使用公开 Issue 报告。请参阅我们的[安全策略](./SECURITY.md)。

## 开发环境搭建

### 前提条件

- **Node.js** 20.19+ 或 22.12+（仓库根目录的 `.node-version` 为 22.12.0）
- **pnpm** 10：版本由 `package.json` 的 `packageManager` 字段固定，执行一次 `corepack enable` 后会自动使用该版本；升级 pnpm 时直接修改这个字段（不再由 Dependabot 代劳）
- **Rust** 1.95：由 `rust-toolchain.toml` 固定，rustup 会在首次构建时自动安装
- 各平台的 [Tauri 2 前置依赖](https://v2.tauri.app/zh-cn/start/prerequisites/)：Windows 需要 Microsoft C++ 生成工具和 WebView2（Windows 10/11 通常已预装 WebView2）；macOS 需要 Xcode Command Line Tools；Debian / Ubuntu 可以参考 CI 使用的安装命令：

  ```bash
  sudo apt-get install build-essential pkg-config libssl-dev libgtk-3-dev librsvg2-dev \
    libayatana-appindicator3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev
  ```

- Tauri CLI 已作为开发依赖随 `pnpm install` 安装，无需全局安装

### 快速开始

```bash
# 安装依赖
pnpm install

# 启动开发服务器（热重载）
pnpm dev
```

`pnpm dev` 会先在 `http://localhost:3000` 启动 Vite，再打开 Tauri 窗口；3000 端口被占用时会直接报错，不会自动换端口。

### 常用命令

| 命令 | 说明 |
|------|------|
| `pnpm dev` | 启动开发服务器（热重载） |
| `pnpm build` | 构建生产版本并打包 |
| `pnpm build:renderer` | 只构建前端，输出到 `dist/` |
| `pnpm typecheck` | TypeScript 类型检查（strict，未使用的 import 或参数会报错） |
| `pnpm format` | 用 Prettier 格式化 `src/` |
| `pnpm format:check` | 检查 `src/` 的代码格式 |
| `pnpm test:unit` | 运行前端单元测试 |
| `pnpm test:unit:watch` | 以监听模式运行前端单元测试 |

Rust 后端命令（在 `src-tauri/` 目录下执行）：

```bash
cd src-tauri
cargo fmt                     # 格式化 Rust 代码
cargo clippy -- -D warnings   # Clippy 检查（与 CI 一致，警告视为错误）
cargo test                    # 运行所有后端测试
cargo test some_test_name     # 按名称运行部分测试
```

> 刚 clone 下来、还没构建过前端时，请先在仓库根目录执行 `mkdir -p dist`（或 `pnpm build:renderer`）。否则 `src-tauri` 下的 cargo 命令会因为找不到前端产物目录而编译失败。

### 本地打包

`pnpm build` 最后会为自动更新包签名，这一步需要发布用的私钥 `TAURI_SIGNING_PRIVATE_KEY`。本地没有私钥时，可以关闭更新包生成：

```bash
pnpm tauri build -c '{"bundle":{"createUpdaterArtifacts":false}}'
```

需要调试版本时，在上面的命令后加 `--debug`。这种单引号写法适用于 macOS 和 Linux；在 Windows 上，可以把 `{"bundle":{"createUpdaterArtifacts":false}}` 存成一个 JSON 文件，再用 `-c <文件路径>` 传入。

## 测试

**前端**：使用 vitest 运行 `tests/` 和 `src/` 下的 `*.test.ts(x)`；Tauri 调用由 MSW 模拟（见 `tests/msw/`）；组件测试使用 @testing-library/react。

**后端**：单元测试写在源码旁边（`#[cfg(test)]`），集成测试放在 `src-tauri/tests/`。CI 会在 Linux、macOS 和 Windows 上运行 `cargo test`。

> **隔离真实配置**：部分后端测试会读写 `~/.cc-switch`、`~/.codex` 等目录。本地运行时，建议把 `CC_SWITCH_TEST_HOME` 指向一个临时目录：
>
> ```bash
> CC_SWITCH_TEST_HOME="$(mktemp -d)" cargo test
> ```
>
> 只设置这个变量即可，不要改 `HOME`：改了 `HOME` 会连带改变 Cargo 和 rustup 的默认目录，导致全量重新编译。

## 架构

### 系统总览

```
Frontend (React + TypeScript)
  Components ── Hooks ── TanStack Query ── src/lib/api
                              │ Tauri IPC (invoke)
Backend (Tauri 2 + Rust)
  Commands ─► Services ─┬─► DAO ─► SQLite (~/.cc-switch/cc-switch.db)
                        ├─► Live config writers (atomic write)
                        │     ~/.claude, ~/.codex, ~/.gemini, ...
                        └─► Local routing (proxy/): forwarding, format
                              conversion, failover, usage accounting
```

### 核心设计

- **SSOT**（单一事实源）：供应商、MCP、提示词、Skills、项目和用量等数据都存储在 `~/.cc-switch/cc-switch.db`（SQLite）
- **设备级设置**：目录覆盖、备份策略等只属于本机的设置存放在 `~/.cc-switch/settings.json`，不参与云同步
- **两种写入模式**：切换式工具（Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build）切换时，先把当前 live 配置回填到当前供应商（Claude Desktop 除外），再写入新供应商；共存式工具（OpenCode、OpenClaw、Hermes、Pi、MiniMax Code）的所有供应商共存于同一个 live 文件，不做回填
- **原子写入**：所有 live 配置都通过“临时文件 + 重命名”写入，避免配置损坏
- **并发安全**：数据库连接由 Mutex 保护，避免竞态条件
- **分层架构**：Commands（参数校验与转发）→ Services（业务逻辑）→ DAO → Database，Commands 层保持精简
- **本地路由**：独立于“写 live 配置”的另一条路径，在本机接管 Claude Code、Codex、Gemini CLI、Grok Build 的请求（Claude Desktop 选“模型映射”时也经由这里转发），负责转发、格式转换、故障转移和计费

### 核心组件

- **ProviderService**：供应商增删改查、切换、回填、排序
- **McpService**：MCP 服务器管理、从各工具导入、同步到 live 文件
- **SkillService / PromptService**：Skills 的安装与同步、各工具的提示词文件
- **ProfileService**：项目快照的保存与切换
- **ProxyService**：本地路由服务的启停，以及各工具本地路由的开关
- **session_manager 模块**：各工具会话历史的扫描、浏览与恢复
- **SpeedtestService**：端点延迟测速
- **database/backup.rs**：数据库导入导出与备份轮换

### 技术栈

**前端**：React 18 · TypeScript · Vite 7 · TailwindCSS 3.4 · shadcn/ui（Radix）· TanStack Query v5 · react-hook-form + zod · react-i18next · @dnd-kit · CodeMirror 6 · Recharts

**后端**：Tauri 2 · Rust · tokio · serde · rusqlite（内置 SQLite）· axum / hyper / reqwest（本地路由）· rquickjs（用量脚本）· Tauri 插件 updater / process / dialog / store / log / deep-link / single-instance / window-state / opener

**测试**：vitest · MSW · @testing-library/react · cargo test（serial_test、tempfile）

### 项目结构

```
├── src/                      # 前端（React + TypeScript）
│   ├── components/           # 按功能分目录：providers、mcp、prompts、skills、sessions、
│   │                         #   proxy、usage、profiles、hermes、openclaw、workspace、settings、ui 等
│   ├── config/               # 各工具的供应商预设（*ProviderPresets.ts）、MCP 预设
│   ├── hooks/                # 自定义 hooks（业务逻辑）
│   ├── lib/api/              # Tauri invoke 封装（lib/query/ 为 TanStack Query 配置）
│   ├── i18n/locales/         # 界面文案（zh / zh-TW / en / ja）
│   ├── icons/                # 供应商图标（索引需手工维护）
│   └── types/ utils/ contexts/
├── src-tauri/                # 后端（Rust）
│   ├── src/
│   │   ├── lib.rs            # 插件、托盘、命令注册
│   │   ├── commands/         # Tauri 命令层（按领域一个文件）
│   │   ├── services/         # 业务逻辑层
│   │   ├── database/         # 建表与迁移（schema.rs）、备份（backup.rs）、DAO（dao/）
│   │   ├── proxy/            # 本地路由：转发、格式转换、熔断、计费
│   │   ├── mcp/              # 各工具的 MCP 同步
│   │   ├── session_manager/  # 会话扫描与终端恢复
│   │   ├── deeplink/         # ccswitch:// 协议
│   │   └── *_config.rs、pi_config/  # 各工具 live 配置的读写
│   └── tests/                # 后端集成测试
├── tests/                    # 前端测试（vitest）
├── docs/                     # 用户手册、使用攻略、发布说明
├── flatpak/                  # Flatpak 打包清单与构建说明
└── assets/                   # 截图与合作伙伴素材
```

## 代码规范

- **前端**：Prettier 格式化（仅 `src/`，使用默认配置）、严格 TypeScript（`pnpm typecheck`）；项目没有使用 ESLint
- **后端**：`cargo fmt` 格式化，`cargo clippy -- -D warnings` 检查
- **Tauri 2 命令**：命令名就是 Rust 函数名，保持 snake_case；前端 `invoke` 传参时，参数名要写成 camelCase（例如 Rust 的 `provider_id` 在前端写成 `providerId`）。新命令需要在 `src-tauri/src/lib.rs` 的 `generate_handler!` 中注册，前端封装放在 `src/lib/api/`

提交前运行与 CI 相同的检查：

```bash
pnpm typecheck && pnpm format:check && pnpm test:unit && pnpm build:renderer
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

## Pull Request 指南

1. **先开 Issue 讨论** — 新功能请先开 Issue，不适合项目方向的 PR 可能会被关闭。
2. **Fork 并创建分支** — 从 `main` 创建功能分支（如 `feat/my-feature` 或 `fix/issue-123`）。
3. **保持 PR 专注** — 每个 PR 只做一件事，避免无关改动。
4. **遵循 PR 模板** — 填写概述、关联 Issue 和检查清单。

### PR 检查清单

- [ ] `pnpm typecheck` 通过
- [ ] `pnpm format:check` 通过
- [ ] `pnpm test:unit` 通过
- [ ] 如修改了 Rust 代码：`cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test` 通过
- [ ] 如修改了用户可见文本，已同步更新四个语言文件

### 提交信息规范

我们使用 [Conventional Commits](https://www.conventionalcommits.org/)：

```
feat(provider): add support for new provider
fix(tray): resolve menu not updating after switch
docs(readme): update installation instructions
ci: add format check workflow
chore(deps): update dependencies
```

## AI 辅助贡献

我们欢迎 AI 辅助的贡献，但**责任始终在你身上**。AI 工具降低了写代码的成本，但并没有降低 review 的成本。维护者没有义务替你清理 AI 的产出。

提交 PR 即表示你同意以下规则：

1. **你已阅读并理解了你的代码。** 你必须能解释 PR 中的每一行。如果做不到，说明还没准备好提交 review。
2. **你已亲自测试过。** 每个改动都必须在本地验证——而不是"看起来对"。不要提交你自己无法测试的平台或功能的代码。
3. **PR 必须小而聚焦。** 一个 Issue 对应一个 PR。大而散、跨多个主题的 PR 会被直接关闭。
4. **先开 Issue 讨论。** 没有事先讨论的"路过式 PR"——尤其是 AI 生成的——可能会被直接关闭。
5. **维护者可以直接关闭。** 看起来是未经审阅的 AI 产出的 PR——虚构的修复、不必要的重构、缺乏上下文的批量改动——维护者可自行决定关闭。

**一句话总结**：AI 是工具，不是理解力的替代品。用它来帮助你更好地贡献，而不是把工作转移给维护者。

## 国际化（i18n）

CC Switch 的界面支持简体中文、繁体中文、英文和日文四种语言。修改用户可见文本时：

1. **同时更新四个**语言文件：
   - `src/i18n/locales/zh.json`
   - `src/i18n/locales/zh-TW.json`
   - `src/i18n/locales/en.json`
   - `src/i18n/locales/ja.json`
2. 所有 UI 文本使用 i18next 的 `t()` 函数。
3. 不要硬编码用户可见的字符串。

> 漏掉某个语言文件不会导致构建失败：缺少的文案会回退显示英文，英文文件也缺时，界面上会直接显示 key 的字面量。

## 有疑问？

- [提问](https://github.com/farion1231/cc-switch/issues/new?template=question.yml)
- [GitHub 讨论区](https://github.com/farion1231/cc-switch/discussions)
