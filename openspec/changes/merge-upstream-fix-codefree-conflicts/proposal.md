## Why

`guoddt/main` 分支（用于添加 CodeFree-O 功能的 PR 分支）需要持续同步上游 `github/main` 分支。已完成的合并轮次：

- **第一轮（2026-07-27）**：`878c26f3` → `934a2d03`，7 个提交，自动合并无冲突
- **第二轮（2026-07-29）**：`934a2d03` → `87b0e3fb`，17 个提交，涉及安全修复、deeplink 风险分类、models.dev 定价同步、skill 安装加固、session 终端转义、CI R2 mirror 等

第二轮上游新增了重大安全修复（zip-slip 漏洞、凭证泄露、panic 路径）、deeplink 导入风险分类、models.dev 自动定价同步、SQL 导入跨文件语句拦截、POSIX 终端转义、CI Cloudflare R2 mirror 等变更。需要将主干合并到当前分支，解决与 CodeFree-O 集成相关的冲突，并在版本号中添加考生姓名标识。

## What Changes

### 第一轮（已完成）

- 合并 `github/main` 的 7 个新提交到当前 `guoddt/main` 分支（本地 `main`）
- 自动合并无冲突，codefree 功能完整性验证通过
- 打包 exe 验证成功（30.95MB）

### 第二轮（进行中）

- 合并 `github/main` 的 17 个新提交（`934a2d03`→`87b0e3fb`）到当前分支
- 解决合并冲突，重点处理以下区域：
  - `src-tauri/src/services/skill.rs`（+1583 行）：上游 skill 安装加固与 codefree skills 逻辑冲突
  - `src-tauri/src/services/provider/mod.rs`（+713 行）：上游 provider 安全修复冲突
  - `src-tauri/src/mcp/codex.rs`（+175 行）：上游 MCP 安全修复冲突
  - `src-tauri/src/commands/usage.rs`（+127 行）：上游 models.dev 定价同步冲突
  - `src/i18n/locales/*.json`（+52 行/语言）：上游新增翻译 key 与 codefree 翻译 key 冲突
  - `src-tauri/src/database/schema.rs`：确认 codefree v16 迁移不被上游变更破坏
  - `src-tauri/tauri.conf.json`：上游新增 updater 配置冲突
- 在版本号中添加考生姓名标识：`3.18.0` → `3.18.0-郭孔泉`（package.json / tauri.conf.json / Cargo.toml 三处）
- 验证合并后 codefree 功能完整性
- 运行质量门禁：cargo fmt --check / cargo clippy / pnpm typecheck / pnpm format:check
- 重新打包 exe 验证

## Capabilities

### New Capabilities

无新增能力。本次变更是合并上游主干并修复冲突，不引入新功能。

### Modified Capabilities

以下现有能力的实现需要适配上游变更（非 spec 级别需求变更，仅实现层调整）：

- `codefree-session-usage`：schema.rs 迁移链需与上游 schema 变更协调，确保 v15→v16 迁移不被上游变更破坏
- `codefree-skills-mcp`：i18n 翻译文件合并后需保留 codefree 相关 key
- `codefree-frontend-ui`：前端组件合并后需保留 codefree UI 元素
- `codefree-version-check`：AboutSection 合并后需保留 codefree 版本检测逻辑

## Impact

### 第一轮影响（已完成）

- **后端 Rust 代码**：`schema.rs`（迁移链）、`claude_desktop_config.rs`、`commands/provider.rs`、`commands/misc.rs`
- **前端 TypeScript 代码**：`types/omo.ts`、`i18n/locales/*.json`（4 语言）、`components/providers/forms/*.tsx`、`config/*ProviderPresets.ts`
- **CI/构建**：`.github/workflows/release.yml`（新增 R2 mirror）、`scripts/generate-download-manifest.mjs`
- **文档**：`README.md`、`README_DE.md`、`README_JA.md`、`README_ZH.md`
- **测试**：`tests/config/*ProviderPresets.test.ts`、`tests/components/ClaudeDesktopProviderForm.test.tsx`
- **数据库**：schema 版本保持 v16，迁移链不变
- **构建产物**：exe 已重新打包验证（30.95MB）

### 第二轮影响（进行中）

- **后端 Rust 代码**：`services/skill.rs`（+1583 行安全加固）、`services/provider/mod.rs`（+713 行）、`mcp/codex.rs`（+175 行）、`commands/usage.rs`（+127 行 models.dev 同步）、`services/model_pricing.rs`（新增 761 行）、`database/backup.rs`（SQL 注入拦截）、`session_manager/terminal/mod.rs`（POSIX 转义）、`deeplink/*`（风险分类）
- **前端 TypeScript 代码**：`i18n/locales/*.json`（4 语言，+52 行/语言）、`components/usage/ModelsDevAutoSyncPanel.tsx`（新增 668 行）、`components/DeepLinkImportDialog.tsx`、`utils/deeplinkRisk.ts`（新增）、`lib/modelsDev*.ts`（新增）
- **CI/构建**：`.github/workflows/sync-r2.yml`（新增）、`scripts/rewrite-updater-manifest.mjs`（新增）、`tauri.conf.json`（updater 配置）
- **安全**：`SECURITY.md`（威胁模型文档）、zip-slip 修复、凭证泄露修复、panic 路径修复
- **版本号**：`package.json` / `tauri.conf.json` / `Cargo.toml` 添加考生姓名 `3.18.0-郭孔泉`
- **数据库**：schema 版本保持 v16，迁移链不变
- **构建产物**：需重新打包 exe 验证
