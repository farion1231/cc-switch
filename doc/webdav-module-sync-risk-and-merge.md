# WebDAV 模块化同步风险与合并建议

## 范围

本文档仅针对当前工作区未提交的 WebDAV 模块化同步相关改动，不评价其他历史代码。

涉及的主要文件：

- `src-tauri/src/services/webdav_sync.rs`
- `src-tauri/src/services/webdav_auto_sync.rs`
- `src-tauri/src/database/backup.rs`
- `src-tauri/src/settings.rs`
- `src/components/settings/WebdavSyncSection.tsx`
- `src/lib/schemas/settings.ts`
- `src/types.ts`
- `tests/components/WebdavSyncSection.test.tsx`
- `src/i18n/locales/{en,ja,zh}.json`

## 当前结论

当前工作区内，本文档跟踪的 WebDAV 模块化同步正确性与数据安全问题已完成最小必要修复。

当前剩余的主要合并风险不是功能逻辑，而是 merge scope：

- `src-tauri/tauri.conf.json`
- `vite.config.ts`

这两处仍包含与 WebDAV 模块化同步无直接关系的改动，建议拆分或单独说明后再合并。

## 已解决风险

### 1. `model_pricing` 存在静默漏同步风险

原问题：

- 自动同步触发表与模块映射仍将 `model_pricing` 归入 API 模块。
- `webdav_sync.rs` 的 `API_TABLES` 中缺少 `model_pricing`。

影响：

- 用户修改了定价数据后，自动同步会触发，看起来上传成功。
- 但远端快照不包含该表，后续下载无法恢复这部分数据。

当前状态：

- 已解决。
- `model_pricing` 已纳入 API 模块同步表。
- 已补充回归测试覆盖该表的选择性导入/替换行为。

### 2. 选择性下载丢失数据库安全备份

原问题：

- 新的选择性下载路径通过 `replace_tables_from_sql_strings()` 覆盖目标表。
- 该路径目前不会像旧的整库导入那样先创建安全备份。

影响：

- 一旦远端快照数据异常但 SQL 仍可成功导入，应用内将失去原有的可恢复保障。

当前状态：

- 已解决。
- 选择性表替换前已恢复数据库安全备份。
- Skills 文件夹回滚逻辑仍保留。

### 3. 前端默认模块回归

原问题：

- 后端默认值为：上传仅 `api` 开启，下载四个模块全部开启。
- 前端 `normalizeModules()` 对缺失字段默认补 `true`。
- 旧配置缺少 `uploadModules` / `downloadModules` 时，打开设置并保存可能把上传范围放大到全部模块。

影响：

- 行为变更不是用户主动选择，属于静默回归。

当前状态：

- 已解决。
- 前端已区分“整个对象缺失”和“对象部分字段缺失”两种情况。
- 缺失整个对象时，上传/下载默认值已与后端保持一致。

## 已解决次级问题

### 4. 有一条前端测试不构成有效回归保护

原问题：

- 当前测试使用的 `baseConfig` 已显式带有模块配置。
- 因此它没有覆盖“旧配置缺少模块字段”的真实回归场景。

当前状态：

- 已解决。
- 已改为用缺失模块字段的配置断言前端默认行为。
- 额外补充了“部分 legacy 模块对象缺字段”的兼容测试。

### 5. legacy v2 快照的模块可用性展示不准确

原问题：

- 代码把 v2 快照的可用模块默认为 `WebDavSyncModules::default()`。
- 这会把 legacy 快照错误展示成仅 API 可用。

影响：

- 下载前预览会低估 legacy 快照可恢复的模块范围。

当前状态：

- 已解决。
- v2 快照的远端可用模块展示已视为全模块可恢复。
- 已补充对应断言。

## 本次最小必要清理边界

本次已处理内容：

- 与上述风险直接相关的逻辑修复。
- 与修复直接绑定的测试补充与修正。
- 一处明显重复的前端模块标签 helper 清理。

本次不建议混入的内容：

- WebDAV 模块同步以外的新功能。
- 大范围重构或抽象化。
- 与本功能无关的 UI 调整。

## 剩余合并范围风险

当前仍建议优先处理的剩余项：

- `src-tauri/tauri.conf.json`
- `vite.config.ts`

这些改动不属于本文档跟踪的 WebDAV 模块化同步修复本身。

## 建议从本次功能 diff 中剥离的无关改动

下列改动与 WebDAV 模块化同步无直接关系，建议单独评估或拆出：

- `src-tauri/tauri.conf.json`
  - `productName` 改为 `CC Switch Dev`
  - `identifier` 改为 `com.ccswitch.desktop.dev`
- `vite.config.ts`
  - `strictPort` 从 `true` 改为 `false`

原因：

- 这些改动会影响开发体验、应用身份或数据目录，风险独立于本次功能。
- 将其混入当前功能 diff 会增加 review 噪音，并削弱回归定位能力。

## 合并前检查项

- `model_pricing` 已包含在 API 模块同步中。
- 选择性下载前会创建数据库安全备份。
- 前后端模块默认值一致。
- 针对旧配置缺字段场景的前端回归测试已覆盖。
- 针对 `model_pricing` 的后端回归测试已覆盖。
- legacy v2 快照的模块可见性断言已修正。
- 无关的 dev 配置改动已确认是否保留、拆分，或明确记录为有意保留。
