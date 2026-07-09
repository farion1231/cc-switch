## 1. 审计与还原冗余修改

- [x] 1.1 运行 `git diff` 审计所有未提交修改，标记与核心功能无关的更改
- [x] 1.2 还原 `app_config.rs` 中 McpRoot/PromptRoot 的 codefree 字段（保留 — match 穷尽性需要）
- [x] 1.3 还原 `deeplink/provider.rs` 中 Codefree 分支（改为返回错误而非还原）
- [x] 1.4 还原 `proxy/providers/mod.rs` 中 Codefree proxy adapter（保留 — match 穷尽性需要）
- [x] 1.5 还原 `services/config.rs` 中不必要的 Codefree sync_to_live 分支（保留 — match 穷尽性需要）
- [x] 1.6 还原 `services/provider/live.rs` 和 `services/provider/mod.rs` 中不必要的 Codefree 分支（保留 — match 穷尽性需要）
- [x] 1.7 还原 `prompt_files.rs` 中不必要的 Codefree 分支（改为返回错误而非还原）
- [x] 1.8 还原 `provider.rs` 中不必要的 Codefree extract_credentials 分支（改为返回空值而非还原）
- [x] 1.9 运行 `cargo check` 验证还原后编译通过

## 2. 后端核心 — 数据库路径与会话同步

- [x] 2.1 确认 `codefree_config.rs` 中 `get_codefree_db_path()` 和 `get_codefree_data_dir()` 实现正确
- [x] 2.2 确认 `session_usage_codefree.rs` 中同步逻辑正确：app_type="codefree", provider_id="_codefree_session", request_id 格式 `codefree_session:{session_id}:{message_id}`
- [x] 2.3 确认 `lib.rs` 中 codefree 首次同步和定期同步调用存在
- [x] 2.4 确认 `commands/usage.rs` 中 `sync_session_usage` 包含 codefree 同步并合并结果
- [x] 2.5 确认 `services/usage_stats.rs` 中 `allow_missing_cache_creation` 包含 "codefree"

## 3. 后端核心 — 会话管理

- [x] 3.1 确认 `session_manager/providers/codefree.rs` 实现正确：仅 SQLite 模式，provider_id="codefree"，resume_command=`codefree -s {session_id}`
- [x] 3.2 确认 `session_manager/providers/mod.rs` 注册了 codefree 模块
- [x] 3.3 确认 `session_manager/mod.rs` 中 scan_sessions 添加了 codefree 线程（7线程并行）
- [x] 3.4 确认 `session_manager/mod.rs` 中 load_messages 添加了 codefree SQLite 分支
- [x] 3.5 确认 `session_manager/mod.rs` 中 delete_session 添加了 codefree SQLite 分支
- [x] 3.6 确认 `session_manager/mod.rs` 中 provider_roots 添加了 codefree 分支

## 4. 后端核心 — AppType 与 Settings

- [x] 4.1 确认 `app_config.rs` 中 AppType::Codefree 枚举变体存在
- [x] 4.2 确认 `settings.rs` 中 VisibleApps 包含 `codefree: bool`（默认 true）
- [x] 4.3 确认 `settings.rs` 中 `current_provider_codefree: Option<String>` 字段存在
- [x] 4.4 确认 `settings.rs` 中 `get_current_provider`/`set_current_provider` 包含 Codefree 分支

## 5. 后端新增 — 技能管理

- [x] 5.1 修改 `services/skill.rs`，在技能根目录 match 中添加 Codefree 分支，返回 `%HOME%/.codefree-o/skills`
- [x] 5.2 确认技能软链接创建/删除/列表操作对 CodeFree-O 目录生效

## 6. 后端新增 — MCP 配置管理

- [x] 6.1 修改 `services/mcp.rs`，在 MCP 配置读写 match 中添加 Codefree 分支，路径为 `%HOME%/.codefree-o/.config/codefree.json`
- [x] 6.2 确认配置目录不存在时自动创建

## 7. 后端新增 — 版本升级检查

- [x] 7.1 修改 `commands/config.rs`，在版本检查 match 中添加 Codefree 分支
- [x] 7.2 实现版本检测命令 `codefree-o --version`
- [x] 7.3 实现升级命令 `codefree-o upgrade`
- [x] 7.4 实现安装脚本 `npm install -g @srdcloud/codefree-o --registry=https://registry.npmjs.org/`

## 8. 前端 — 类型与配置

- [x] 8.1 确认 `types.ts` 中 AppType 包含 "codefree"
- [x] 8.2 确认 `usage.ts` 中 usage 类型包含 codefree 相关字段
- [x] 8.3 确认 `appConfig.tsx` 中 APP_IDS 和 APP_ICON_MAP 包含 codefree
- [x] 8.4 确认 i18n 文件（en, zh, zh-TW, ja）包含 CodeFree 翻译

## 9. 前端 — 导航与 UI

- [x] 9.1 确认 `App.tsx` 中 `hasProviderSupport = sharedFeatureApp !== "codefree"`
- [x] 9.2 确认 `App.tsx` 中 `hasSkillsSupport` 排除 codefree
- [x] 9.3 确认 `App.tsx` 中 `hasSessionSupport` 包含 codefree
- [x] 9.4 确认 `App.tsx` 中 codefree 选中时 providers view 自动重定向到 sessions
- [x] 9.5 确认 `App.tsx` 中 codefree 导航栏仅显示 sessions 按钮
- [x] 9.6 确认 `App.tsx` 中 ProxyToggle/FailoverToggle/ProfileSwitcher 排除 codefree
- [x] 9.7 确认 `App.tsx` 中 codefree sessions view header 显示 CC Switch logo + Settings 按钮
- [x] 9.8 确认 `AppVisibilitySettings.tsx` 中首页选项不包含 codefree
- [x] 9.9 确认 `AppSwitcher.tsx` 中包含 codefree 切换选项和 teal `</>` 图标

## 10. 前端修复 — Skills 和 MCP 面板 CodeFree 配置项

- [x] 10.1 修改 `src/config/appConfig.tsx`，将 `"codefree"` 添加到 `SKILLS_APP_IDS` 和 `MCP_APP_IDS`
- [x] 10.2 修改 `src/components/skills/UnifiedSkillsPanel.tsx`，在 `enabledCounts` 中添加 `codefree: 0` 键
- [x] 10.3 修改 `src/components/skills/UnifiedSkillsPanel.tsx`，移除 `skill.apps` 的 `as unknown as Record<string, boolean | undefined>` 断言（因 `SKILLS_APP_IDS` 已包含 codefree，与 `SkillApps` 类型一致）
- [x] 10.4 确认 `src/components/mcp/UnifiedMcpPanel.tsx` 中 `enabledCounts` 已有 `codefree: 0`，且遍历使用 `MCP_APP_IDS` 后自动生效
- [x] 10.5 确认 i18n 翻译文件中 Skills/MCP 相关的 codefree 条目存在
- [x] 10.6 运行 `pnpm typecheck` 验证前端类型检查通过
- [x] 10.7 运行 `cargo check` 验证后端编译通过

## 11. 编译验证与打包

- [x] 11.1 运行 `cargo check` 确认无编译错误
- [x] 11.2 运行 `npx vite build` 确认前端编译通过
- [x] 11.3 运行 `npx tauri build` 生成最终 exe
- [x] 11.4 确认 exe 输出位置：`src-tauri\target\release\cc-switch.exe`（约 29MB）
