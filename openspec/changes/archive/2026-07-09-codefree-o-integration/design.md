## Context

cc-switch 是一个基于 Tauri (Rust + React) 的桌面应用，用于管理多个 AI 编程助手（Claude、Codex、Gemini、OpenCode、OpenClaw、Hermes）的 provider 配置、会话查看和 token 使用统计。

当前代码中已有部分 CodeFree-O 集成（git 未提交状态），但存在以下问题：
1. 前端"设置-通用-主页面显示"仍提供 CodeFree 选项，但 CodeFree-O 不应作为首页默认应用
2. 技能管理和 MCP 管理未支持 CodeFree-O 的目录/配置路径
3. 版本升级检查未集成 CodeFree-O
4. 部分修改可能冗余，需要对比 git diff 还原不必要的更改

CodeFree-O 关键特征：
- 基于 opencode 二开，数据库 schema 与 opencode 完全一致（SQLite）
- 内嵌模型，不支持自定义 provider/model
- 数据库路径：`%HOME%/.codefree-o/.local/share/codefree.db`
- 技能目录：`%HOME%/.codefree-o/skills`
- MCP 配置：`%HOME%/.codefree-o/.config/codefree.json`
- 安装方式：`npm install -g @srdcloud/codefree-o --registry=https://registry.npmjs.org/`
- 升级命令：`codefree-o upgrade`

## Goals / Non-Goals

**Goals:**
- CodeFree-O 作为独立 AppType 集成到 cc-switch，支持会话查看和 token 统计
- 选中 CodeFree-O 时默认进入会话视图，不显示模型管理/providers 导航
- 技能管理支持 CodeFree-O 技能目录软链接
- MCP 管理支持 CodeFree-O 的 codefree.json 配置
- 版本检查支持 CodeFree-O 升级检测
- 保持最少代码修改，还原冗余更改

**Non-Goals:**
- CodeFree-O 不支持自定义 provider/model 配置
- CodeFree-O 不支持 proxy/failover 功能
- CodeFree-O 不支持 prompts 管理
- CodeFree-O 不作为"设置-通用-主页面显示"的可选首页应用
- 不修改 CodeFree-O 本身的任何代码

## Decisions

### D1: AppType 使用 "codefree" 而非 "codefree-o"

**选择**: `app_type = "codefree"`
**理由**: 与现有 AppType 命名风格一致（opencode、openclaw 等均为简短标识），前端显示名 "CodeFree" 足以区分。
**替代方案**: "codefree-o" — 更精确但过长，与现有风格不一致。

### D2: CodeFree-O 数据库路径发现策略

**选择**: 默认路径 `%HOME%/.codefree-o/.local/share/codefree.db`，支持 `CODEFREE_DB` 环境变量覆盖。
**理由**: 与 opencode 的路径发现模式一致（`OPENCODE_DB` 环境变量），便于开发调试。
**实现**: `codefree_config.rs` 中 `get_codefree_db_path()` 和 `pub fn get_codefree_data_dir()`。

### D3: CodeFree-O 选中时的导航行为

**选择**: 默认进入 sessions view，导航栏仅显示 sessions 按钮，不显示 providers/prompts/mcp。Skills 按钮在 CodeFree 选中时显示（CodeFree 支持 Skills 管理）。
**理由**: CodeFree-O 内嵌模型不支持自定义，providers view 无意义；prompts/mcp 在 CodeFree-O 中通过独立目录管理，主界面不需要导航入口；但 Skills 管理需要显示，因为 CodeFree-O 的技能目录 `~/.codefree-o/skills` 已在后端支持。
**实现**: `hasProviderSupport = sharedFeatureApp !== "codefree"`，`hasSkillsSupport` 包含 codefree，codefree 导航分支含 sessions + skills 按钮。

### D4: CodeFree-O 费用计算

**选择**: CodeFree-O 的 `cost` 字段为 0（免费内嵌模型），回退到 `find_model_pricing` 查价。
**理由**: 与 opencode 的费用计算逻辑一致，保持统计完整性。

### D5: CodeFree-O 会话管理仅 SQLite

**选择**: session_manager/providers/codefree.rs 仅实现 SQLite 模式，无 JSON 文件存储。
**理由**: CodeFree-O 与 opencode 共享数据库 schema，但不像 opencode 那样有 JSON 会话文件备份。

### D6: 技能管理 — 复用现有 skill 软链接机制

**选择**: 在 `services/skill.rs` 的 match 中添加 Codefree 分支，技能根目录指向 `%HOME%/.codefree-o/skills`。
**理由**: cc-switch 已有技能软链接管理机制（opencode/hermes 等），CodeFree-O 只需添加路径映射。

### D7: MCP 管理 — 复用现有 MCP 配置读写机制

**选择**: 在 `services/mcp.rs` 的 match 中添加 Codefree 分支，配置文件路径指向 `%HOME%/.codefree-o/.config/codefree.json`。
**理由**: cc-switch 已有 MCP 配置读写机制，CodeFree-O 只需添加路径映射。配置文件格式与 opencode 的 opencode.json 一致。

### D8: 版本升级检查 — 复用现有环境检测机制

**选择**: 在 `commands/config.rs` 的版本检查 match 中添加 Codefree 分支，检测命令 `codefree-o --version`，升级命令 `codefree-o upgrade`，安装脚本 `npm install -g @srdcloud/codefree-o --registry=https://registry.npmjs.org/`。
**理由**: cc-switch 已有 opencode 等的版本检查机制，CodeFree-O 只需添加命令映射。

### D9: 设置-通用-主页面显示排除 CodeFree

**选择**: 在 `AppVisibilitySettings.tsx` 的首页选项列表中不包含 codefree，但保留 AppSwitcher 中的 codefree 切换。
**理由**: CodeFree-O 不支持 providers view，不应作为首页默认应用；但用户仍需在 AppSwitcher 中切换到 CodeFree-O 查看会话。

### D10: 还原冗余修改

**选择**: 对比 git diff，还原不属于上述功能的冗余更改。
**理由**: 保持最少代码修改原则，减少引入 bug 的风险。重点关注：
- `app_config.rs` 中 McpRoot/PromptRoot 的 codefree 字段（CodeFree-O 不需要 MCP/Prompt root，应还原）
- `deeplink/provider.rs` 中不必要的 Codefree 分支
- `proxy/providers/mod.rs` 中不必要的 Codefree 分支
- 其他与核心功能无关的修改

## Risks / Trade-offs

- **[数据库路径硬编码]** → 通过 `CODEFREE_DB` 环境变量覆盖缓解，与 opencode 模式一致
- **[CodeFree-O schema 变更]** → CodeFree-O 基于 opencode 二开，schema 变更风险低；若变更，需同步修改 `session_usage_codefree.rs` 和 `session_manager/providers/codefree.rs`
- **[7 线程并行扫描性能]** → 新增 codefree 线程对整体扫描时间影响可忽略（SQLite 查询快）
- **[还原冗余修改可能破坏现有功能]** → 还原后需重新 `cargo check` + `npx vite build` 验证
- **[MCP 配置文件格式兼容性]** → 假设 codefree.json 与 opencode.json 格式一致，需实际验证
