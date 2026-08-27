## 1. VisibleApps 反序列化兼容性

- [x] 1.1 移除 `settings.rs` 中 `VisibleApps.codefree` 的 `#[serde(default)]`，实现自定义反序列化确保字段缺失时使用 `VisibleApps::default()`
- [x] 1.2 验证老用户升级场景：settings 中无 codefree 字段时反序列化为 `true`

## 2. 数据库路径修复

- [x] 2.1 修改 `codefree_config.rs` 中 `get_codefree_db_path()`，自定义目录时追加 `.local/share` 子路径
- [x] 2.2 验证默认路径和自定义路径均正确解析为 `<root>/.local/share/codefree.db`

## 3. VALID_APPS 修复

- [x] 3.1 在 `App.tsx` 的 `VALID_APPS` 数组中添加 `"codefree"`
- [x] 3.2 验证选择 CodeFree 后重载应用能正确恢复

## 4. Skills 目录修复

- [x] 4.1 修改 `services/skill.rs` 中 CodeFree skills 默认路径为 `~/.codefree-o/.config/skills`
- [x] 4.2 修改 `useDirectorySettings.ts` 中 CodeFree 的 `APP_DIRECTORY_META` 默认目录描述（如需要）

## 5. 验证

- [x] 5.1 `pnpm typecheck` 通过
- [x] 5.2 `cargo clippy` 通过
- [x] 5.3 完整构建验证
