## Why

cc-switch 目前支持 Claude、Codex、Gemini、OpenCode、OpenClaw、Hermes 等应用的会话管理和 token 统计，但缺少对中国电信 CodeFree-O 的支持。CodeFree-O 是基于 opencode 二开的 AI 编程助手，内嵌模型不支持自定义，其数据库 schema 与 opencode 完全一致，用户需要在 cc-switch 中查看 CodeFree-O 的会话历史和 token 使用统计。此外，CodeFree-O 的技能目录、MCP 配置、版本升级检查等也需要集成。

## What Changes

- **新增 CodeFree-O 作为独立 AppType**：与 opencode 平级，app_type="codefree"，独立统计 token 使用
- **会话查看**：扫描 CodeFree-O 的 SQLite 数据库（`%HOME%/.codefree-o/.local/share/codefree.db`），支持会话列表浏览、消息加载、会话删除
- **Token 使用统计**：同步 CodeFree-O 的 session/message 数据，provider_id="_codefree_session"，request_id 格式 `codefree_session:{session_id}:{message_id}`
- **首页不显示模型管理**：CodeFree-O 内嵌模型不支持自定义，选中 CodeFree-O 时默认进入会话视图，不显示 providers/skills/prompts/mcp 导航按钮
- **设置-通用-主页面显示不提供 CodeFree 选项**：CodeFree-O 不应在"设置-通用-主页面显示"中作为可切换的默认首页应用
- **技能管理支持 CodeFree-O**：软链接配置 CodeFree-O 的技能目录 `C:\Users\<user>\.codefree-o\skills`，前端 Skills 面板显示 CodeFree 的 app 切换按钮和计数
- **MCP 管理支持 CodeFree-O**：读写 CodeFree-O 的 MCP 配置文件 `C:\Users\<user>\.codefree-o\.config\codefree.json`，前端 MCP 面板显示 CodeFree 的 app 计数
- **版本升级检查**：设置-关于-本地环境检测中增加 CodeFree-O 版本检查，升级命令 `codefree-o upgrade`，安装脚本 `npm install -g @srdcloud/codefree-o --registry=https://registry.npmjs.org/`
- **还原不必要的修改**：对比 git 未提交修订，还原不属于上述功能的冗余更改，保持最少代码修改

## Capabilities

### New Capabilities
- `codefree-session-usage`: CodeFree-O 会话扫描和 token 使用统计同步，包括数据库路径发现、会话同步逻辑、费用计算
- `codefree-session-manager`: CodeFree-O 会话管理（浏览、加载消息、删除），仅 SQLite 模式
- `codefree-frontend-ui`: CodeFree-O 前端集成，包括 AppSwitcher 图标、导航栏（仅 sessions）、首页不显示模型管理、设置页不显示 CodeFree 首页选项
- `codefree-skills-mcp`: CodeFree-O 技能目录软链接管理和 MCP 配置文件读写
- `codefree-version-check`: CodeFree-O 版本升级检查和安装脚本集成

### Modified Capabilities
<!-- 无现有 spec 需要修改 -->

## Impact

- **后端 Rust 代码**：新增 `codefree_config.rs`、`session_usage_codefree.rs`、`session_manager/providers/codefree.rs`；修改 `app_config.rs`（AppType 枚举）、`settings.rs`（VisibleApps）、`lib.rs`（同步调用）、`session_manager/mod.rs`（7 线程并行扫描）、`commands/usage.rs`（同步合并）、`services/skill.rs`（技能目录）、`services/mcp.rs`（MCP 配置）、`commands/config.rs`（版本检查）等 16+ 文件
- **前端 React 代码**：修改 `App.tsx`（导航逻辑、hasProviderSupport、hasSessionSupport）、`appConfig.tsx`（APP_IDS、APP_ICON_MAP）、`AppSwitcher.tsx`、`AppVisibilitySettings.tsx`（排除首页选项）、`types.ts`、`usage.ts`、i18n 文件等
- **依赖**：无新外部依赖，复用现有 rusqlite/tauri 依赖
- **构建**：`npx tauri build` 生成 exe，输出 `src-tauri\target\release\cc-switch.exe`（约 29MB）
