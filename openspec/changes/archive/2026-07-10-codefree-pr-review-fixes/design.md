## Context

PR #5167 集成 CodeFree-O 后收到 4 个 review 反馈，均为功能性缺陷：

1. `VisibleApps.codefree` 使用 `#[serde(default)]` 导致老用户升级后字段缺失时反序列化为 `false`，而非 `VisibleApps::default()` 中定义的 `true`
2. `get_codefree_db_path()` 在自定义目录时直接使用根目录，缺少 `.local/share` 子路径，导致会话/用量数据消失
3. `App.tsx` 中 `VALID_APPS` 不包含 `"codefree"`，导致重载后无法恢复上次选择的 CodeFree
4. CodeFree skills 目录应为 `~/.codefree-o/.config/skills` 而非 `~/.codefree-o/skills`

## Goals / Non-Goals

**Goals:**
- 修复 4 个 review 反馈，确保 CodeFree 集成的数据兼容性和功能正确性
- 保持与 OpenCode 实现模式的一致性

**Non-Goals:**
- 不新增功能特性
- 不修改 Hermes 相关代码

## Decisions

### D1: VisibleApps 反序列化兼容性

**选择**：移除 `codefree` 字段的 `#[serde(default)]`，改用自定义 `Deserialize` 实现，在字段缺失时调用 `VisibleApps::default()` 获取默认值。

**理由**：`#[serde(default)]` 对 `bool` 类型默认为 `false`，而 `VisibleApps::default()` 中 `codefree` 应为 `true`（与 opencode 等新增应用一致）。自定义反序列化可确保所有字段缺失时统一使用结构体默认值。

**备选**：使用 `#[serde(default = "default_true")]` 为单个字段指定默认函数——更简单但不够通用，未来新增字段仍需逐个处理。

### D2: 数据库路径追加 .local/share

**选择**：`get_codefree_db_path()` 在使用自定义目录时，追加 `.local/share` 子路径，与 `get_codefree_dir()` 的处理方式一致。

**理由**：CodeFree 的数据目录结构为 `<root>/.local/share/codefree.db`，自定义目录只应覆盖根目录，不应改变数据子路径结构。

### D3: VALID_APPS 添加 codefree

**选择**：在 `App.tsx` 的 `VALID_APPS` 数组中添加 `"codefree"`。

**理由**：`getInitialApp()` 使用 `VALID_APPS` 验证保存的应用选择，缺少 `codefree` 会导致重载后回退到 Claude。

### D4: Skills 目录改为 .config/skills

**选择**：将 CodeFree skills 默认路径从 `~/.codefree-o/skills` 改为 `~/.codefree-o/.config/skills`。

**理由**：CodeFree 实际使用 `.config/skills` 作为 skills 目录，与用户确认的路径一致。

## Risks / Trade-offs

- [VisibleApps 自定义反序列化] → 增加代码复杂度，但确保所有字段默认值一致
- [数据库路径变更] → 已使用自定义目录的用户可能需要重新设置，但修复后行为与默认路径一致
