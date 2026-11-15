# DeepSeek Harness 接入与 PR #8771 的兼容性

核对日期：2026-10-24。

- 本分支：`feat/deepseek-harness-integration`，基于上游 `d8983bb10b82a1ccc15ae5f44ab070902623e990`。
- [PR #8771](https://github.com/farion1231/cc-switch/pull/8771)：head `36bd710237bdd05afc6d43193abcf01429e1a648`。
- 通过 `git merge-tree --write-tree` 以及隔离工作区中的实际 `git merge --no-commit --no-ff` 核对。

结论：功能可以共存，需要处理一处文本冲突、两个自动合并后的重复对象键，并统一“DSH 已成为受管理应用”的界面行为。

## 重叠与解决方式

| 文件 | 实际结果 | 合并方式 |
| --- | --- | --- |
| `src/types/usage.ts` | 文本冲突；两边都注册 `dsh`，插入位置不同 | `AppType` 和 `KNOWN_APP_TYPES` 中各保留一次 `dsh`；可保留 PR 对缓存写入覆盖范围的标记 |
| `src/components/shell/AppGlyph.tsx` | Git 自动合并，但名称表和图标表各出现两个 `dsh` 键 | 保留本分支的受管理 `AppId`、DeepSeek 图标与品牌名；移除重复项及不再适用的 `UsageOnlyAppId` 扩展；PR 的供应商标签仍引用 `AppGlyphId`，可保留 `AppGlyphId = AppId` 类型别名 |
| `src/components/usage/RequestLogTable.tsx` | Git 自动合并，但短名表出现两个 `dsh` 键 | 保留一个短名（本分支为 `DSH`）；`AppId` 已包含 `dsh`，无需仅用于统计的额外图标类型 |
| `src/components/usage/UsageDashboard.tsx` | 没有文本冲突；PR 将 DSH 筛选项设为始终显示 | 恢复与其他受管理应用一致的可见性逻辑：读取「应用」页显示开关，当前选中的筛选项仍可保留 |

只解决 Git 标记的文本冲突仍不够：重复对象键会使 TypeScript 检查失败。PR 正文和类型注释中的“DSH 不是 CC Switch 管理的应用、不提供供应商、MCP、Skills”也应随最终合并范围更新。

解决上述问题后，隔离合并树的 `pnpm typecheck` 通过，用量面板 smoke 与 DeepSeek 预设测试共 6 项通过；PR 的 `cargo test --lib dsh_usage` 共 9 项通过。

## 可以直接保留的改动

PR 的 `session_usage_dsh`、会话用量同步注册、统计供应商标签及其测试可以保留。本分支的会话浏览器只读取会话内容，没有第二套用量入库逻辑，因此不会因两者同时接入而重复计费或重复导入请求。

双方都使用 `dsh` 应用标识、`DSH_HOME`、默认 `~/.dsh` 和原生会话目录，无需数据迁移。本分支的供应商、MCP、Skills、提示词、npm 安装升级卸载不与 PR 的只读用量导入器争用写入路径。

本分支不合入 PR #8771，便于分别评审。兼容性验证在临时工作区进行，解决方式为保留本分支的三个公共类型／图标文件，保留 PR 的全部后端导入改动，并使筛选栏遵守应用可见性设置。
