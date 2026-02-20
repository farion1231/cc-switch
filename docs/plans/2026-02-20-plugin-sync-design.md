# 插件同步功能设计文档

**日期**：2026-02-20
**状态**：已批准，待实现

## 背景

当前 cc-switch 在写入 `~/.claude/config.json` 时，仅设置 `primaryApiKey: "any"`，未处理插件的启用/禁用配置。Claude Code 的 `config.json` 支持 `enabledPlugins` 字段来控制插件状态，但 cc-switch 目前既不读取也不写入该字段。

## 目标

当 Claude Code 安装/卸载插件时，cc-switch 自动感知变化，在用户通过 cc-switch UI 管理插件 enabled/disabled 状态后，将 `enabledPlugins` 写入 `~/.claude/config.json`：

```json
{
  "primaryApiKey": "any",
  "enabledPlugins": {
    "superpowers@superpowers-marketplace": true,
    "context7@claude-plugins-official": true,
    "ralph-loop@claude-plugins-official": false
  }
}
```

## 触发方式

文件监听（方案 A）：使用 `notify` crate 监听 `~/.claude/plugins/installed_plugins.json`，实时感知插件安装/卸载事件。

## 第一节：数据层

### 新增 SQLite 表 `plugin_states`

```sql
CREATE TABLE plugin_states (
    plugin_id    TEXT PRIMARY KEY,
    enabled      BOOLEAN NOT NULL DEFAULT 1,
    install_path TEXT NOT NULL,
    scope        TEXT NOT NULL DEFAULT 'user',
    version      TEXT,
    created_at   DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

- `plugin_id` 格式：`name@registry`（与 `installed_plugins.json` 一致）
- 新发现插件默认 `enabled=1`
- 插件卸载后直接从表中删除

## 第二节：后端架构

### 新增文件：`src-tauri/src/services/plugin_watcher.rs`

职责：
- 应用启动时初始化 `notify` watcher，监听 `installed_plugins.json`
- 文件变化时调用 `sync_plugins_from_installed()`
- 同步完成后 emit Tauri 事件 `plugins://changed`

同步逻辑：
1. 读取 `installed_plugins.json`
2. diff 与 DB 现有记录：新增插件 → `upsert(enabled=true)`，已删除插件 → `remove()`
3. 若 `enableClaudePluginIntegration=true`，重写 `config.json`
4. emit `plugins://changed`

### 新增文件：`src-tauri/src/database/dao/plugin_states.rs`

- `get_all() → Vec<PluginState>`
- `upsert(plugin_id, install_path, version, scope)`
- `remove(plugin_id)`
- `set_enabled(plugin_id, enabled) → bool`

### 扩展现有文件：`src-tauri/src/claude_plugin.rs`

扩展 `write_claude_config()`：
1. 读取 `config.json`（已有）
2. 设置 `primaryApiKey="any"`（已有）
3. **新增**：从 DB 读取 plugin_states，构建 `enabledPlugins` 对象写入 config
4. 原子写入（已有）

### 扩展现有文件：`src-tauri/src/commands/plugin.rs`

新增 Tauri 命令：
- `list_plugins()` → 返回所有插件及其启用状态
- `set_plugin_enabled(plugin_id, enabled)` → 更新 DB + 重写 config.json

## 第三节：前端设计

### 新增文件：`src/hooks/usePlugins.ts`

- `usePluginList()`：TanStack Query，调用 `list_plugins`
- `useSetPluginEnabled()`：mutation，调用 `set_plugin_enabled`
- 监听 `plugins://changed` Tauri 事件，自动 invalidate 查询

### 新增文件：`src/components/plugins/PluginList.tsx`

- 展示插件名称、来源 registry、版本
- Toggle 开关控制 enabled/disabled
- 空状态提示（"未检测到已安装插件"）
- 仅在 Claude 应用且 `enableClaudePluginIntegration=true` 时显示

**UI 示意**：
```
Claude 插件 (3)
┌─────────────────────────────────────────┐
│ superpowers                    [●] 启用  │
│ superpowers-marketplace · 4.3.0          │
├─────────────────────────────────────────┤
│ context7                       [●] 启用  │
│ claude-plugins-official · 8deab84        │
├─────────────────────────────────────────┤
│ ralph-loop                     [○] 禁用  │
│ claude-plugins-official · 8deab84        │
└─────────────────────────────────────────┘
```

## 第四节：边界处理

| 场景 | 处理方式 |
|------|---------|
| `installed_plugins.json` 不存在 | 静默跳过，不报错 |
| 文件 JSON 格式损坏 | 记录 warn 日志，保持 DB 现有状态 |
| 监听器初始化失败 | 降级为启动时同步一次 |
| `enableClaudePluginIntegration=false` | 不写 `enabledPlugins` 字段 |
| DB 中无插件记录 | 写 `"enabledPlugins": {}` |
| 并发写入 | 依赖现有 SQLite 连接池 mutex 保护 |

**跨平台支持**：`notify` crate 统一处理 macOS (FSEvents)、Linux (inotify)、Windows (ReadDirectoryChanges)。

## 第五节：测试策略

### 后端单元测试（Rust）

```rust
// database/dao/plugin_states.rs
test_upsert_new_plugin()
test_upsert_existing_plugin_preserves_enabled()
test_set_enabled_toggle()
test_remove_plugin()

// claude_plugin.rs
test_write_config_with_plugins()
test_write_config_no_plugins_writes_empty_object()
test_write_config_preserves_other_fields()
test_write_config_integration_disabled_skips_enabled_plugins()

// services/plugin_watcher.rs
test_sync_new_plugin_added()
test_sync_plugin_removed()
test_sync_invalid_json_no_panic()
test_sync_missing_file_no_panic()
```

### 前端单元测试（Vitest）

```typescript
// hooks/usePlugins.test.ts
- 正常加载插件列表
- set_plugin_enabled 成功后 invalidate 查询
- "plugins://changed" 事件触发列表刷新
- 空列表时显示提示文本

// 新增 MSW handler
invokeHandlers.listPlugins(() => mockPlugins)
invokeHandlers.setPluginEnabled(({ pluginId, enabled }) => true)
```

### 集成验收标准

- [ ] 安装新插件后 config.json 自动出现对应条目（enabled=true）
- [ ] 卸载插件后 config.json 对应条目消失
- [ ] Toggle 关闭后 config.json 对应条目变为 false
- [ ] 切换 provider 后 enabledPlugins 状态保持不变
- [ ] `enableClaudePluginIntegration=false` 时 enabledPlugins 不写入

## 涉及文件清单

### 新增
- `src-tauri/src/services/plugin_watcher.rs`
- `src-tauri/src/database/dao/plugin_states.rs`
- `src/hooks/usePlugins.ts`
- `src/components/plugins/PluginList.tsx`
- `tests/hooks/usePlugins.test.ts`
- `tests/msw/handlers/plugins.ts`（或扩展现有 handlers）

### 修改
- `src-tauri/src/claude_plugin.rs`（扩展 `write_claude_config`）
- `src-tauri/src/commands/plugin.rs`（新增命令）
- `src-tauri/src/commands/mod.rs`（注册新命令）
- `src-tauri/src/database/dao/mod.rs`（导出新 DAO）
- `src-tauri/src/database/migration.rs`（新增 migration）
- `src-tauri/src/lib.rs`（注册 plugin_watcher 启动）
- 前端 provider/settings 相关组件（集成插件面板入口）
