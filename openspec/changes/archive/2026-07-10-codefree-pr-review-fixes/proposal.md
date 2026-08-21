## Why

PR #5167 (CodeFree-O integration) 收到 4 个 review 反馈，涉及数据兼容性、路径逻辑、前端状态恢复和目录配置问题。这些问题会导致老用户升级后 CodeFree 不可见、自定义目录下会话/用量数据丢失、应用重载后无法恢复 CodeFree 选择。

## What Changes

- 修复 `VisibleApps.codefree` 的 `#[serde(default)]` 问题：字段缺失时使用 `VisibleApps::default()` 而非 `false`，确保老用户升级后 CodeFree 默认可见
- 修复 `get_codefree_db_path()` 自定义目录逻辑：与 `get_codefree_dir()` 保持一致，追加 `.local/share` 子路径
- 修复 `App.tsx` 中 `VALID_APPS` 缺少 `"codefree"`，导致重载后无法恢复上次选择的 CodeFree
- 修改 CodeFree skills 默认目录为 `~/.codefree-o/.config/skills`

## Capabilities

### New Capabilities

（无新增能力）

### Modified Capabilities

- `codefree-frontend-ui`: 修复 VALID_APPS 缺少 codefree 导致的状态恢复问题
- `codefree-session-manager`: 修复自定义目录下数据库路径缺少 .local/share 子路径
- `codefree-skills-mcp`: 修复 VisibleApps serde default 兼容性；修改 skills 默认目录为 .config/skills

## Impact

- `src-tauri/src/settings.rs`：VisibleApps 反序列化兼容性
- `src-tauri/src/codefree_config.rs`：数据库路径逻辑
- `src/App.tsx`：VALID_APPS 常量
- `src-tauri/src/services/skill.rs`：CodeFree skills 默认路径
