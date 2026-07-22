# Repository Guidelines

本仓库是 CC Switch 的贡献者指南。主项目位于 `cc-switch-main/`，是一个基于 Tauri 2.0（React 18 + TypeScript + Rust）的桌面应用，用于在 Claude Code、Codex、Gemini CLI 之间切换配置。

## 项目结构

- `cc-switch-main/src/` — 前端源码（`components/`、`hooks/`、`contexts/`、`lib/`、`utils/`、`i18n/`、`types/`）
- `cc-switch-main/src-tauri/src/` — Rust 后端与 Tauri 命令
- `cc-switch-main/tests/` — 单元与集成测试（`components/`、`hooks/`、`integration/`、`lib/`、`msw/`、`utils/`）
- `cc-switch-main/src/locales/{en,zh,ja}/translation.json` — 三种语言的国际化文案
- `reference/` — 参考项目，请勿修改

## 构建、测试与开发

所有命令在 `cc-switch-main/` 下执行：

```bash
pnpm install          # 安装依赖
pnpm dev              # 启动开发服务器（热重载）
pnpm build            # 生产构建
pnpm typecheck        # TypeScript 类型检查
pnpm test:unit        # 运行单元测试（vitest）
pnpm format           # Prettier 格式化
pnpm format:check     # 检查格式
```

Rust 后端在 `cc-switch-main/src-tauri/` 下：

```bash
cargo fmt             # 格式化
cargo clippy          # 静态检查
cargo test            # 运行测试
```

## 编码风格与命名

- 前端：Prettier 格式化，严格 TypeScript，使用 `pnpm typecheck` 校验
- 后端：`cargo fmt` 格式化，`cargo clippy` 检查
- Tauri 命令名必须使用 camelCase
- 提交前执行：`pnpm typecheck && pnpm format:check && pnpm test:unit`

## 测试指南

- 框架：Vitest + Testing Library + MSW（模拟网络请求）
- 测试与源码同构分布在 `tests/` 下，按领域分目录
- 命名：测试文件以 `.test.ts(x)` 结尾，用 `describe/it` 描述行为
- 新增功能或修复 Bug 时必须补充对应测试

## 提交与 Pull Request

遵循 [Conventional Commits](https://www.conventionalcommits.org/)：

```
feat(provider): 新增服务商支持
fix(tray): 修复切换后菜单未更新
docs(readme): 更新安装说明
chore(deps): 升级依赖
```

PR 要求：先开 Issue 讨论，从 `main` 创建 `feat/` 或 `fix/` 分支，每个 PR 只做一件事，填写模板并关联 Issue。提交前确认类型检查、格式检查、`cargo clippy`（如改动 Rust）均通过。

## 国际化

修改用户可见文本时，必须同时更新 `en`、`zh`、`ja` 三个 `translation.json`，UI 文本一律通过 i18next 的 `t()` 函数获取，禁止硬编码字符串。

## 编译与打包

### 前置准备

- **Rust toolchain**：`rust-toolchain.toml` 当前设为 `stable`（本机 1.97.1），首次编译会自动下载工具链。
- **前端依赖**：如果 `node_modules` 不存在或报构建脚本错误，先跑 `pnpm install --ignore-scripts` 安装依赖。
- **产物**：打包结果在 `src-tauri/target/release/bundle/` 下。

### 全量重建（清理 + 打包）

```bash
cd cc-switch-main
cd src-tauri && cargo clean && 
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
pnpm install --ignore-scripts
pnpm build
```

### 增量编译（只编译改动）

```bash
cd cc-switch-main
pnpm build
```

### 产物位置

- DMG 安装包：`src-tauri/target/release/bundle/dmg/CC Switch_3.17.0_aarch64.dmg`
- macOS 应用：`src-tauri/target/release/bundle/macos/CC Switch.app`
- 更新包：`src-tauri/target/release/bundle/macos/CC Switch.app.tar.gz`（供 updater 使用）

### 注意事项

- **签名**：未配置 Apple 开发者证书，DMG 打开时 Gatekeeper 会拦截，到"系统设置 > 隐私与安全性"点"仍要打开"即可。
- **updater 签名**：打包时若提示 `TAURI_SIGNING_PRIVATE_KEY` 未设置，仅 updater 签名步骤失败，DMG 和 .app 不受影响。
- **首次编译**：全量 `cargo clean` 后首次 release 编译约 8–10 分钟（依赖多）；增量编译快很多。
