## Why

第三轮合并 `github/main`（`87b0e3fb` → `0b5da510`，合并提交 `80a327d5`）后，发现 `schema.rs`、`dao/mcp.rs`、`dao/skills.rs` 中 CodeFree 相关的数据库定义在第二轮合并时被上游版本覆盖丢失。具体表现：

- `mcp_servers` 表 `create_tables_on_conn` 缺少 `enabled_codefree` 列
- `skills` 表 `create_tables_on_conn` 缺少 `enabled_codefree` 列
- 缺少为 CodeFree 添加 `enabled_codefree` 列的迁移函数
- `dao/mcp.rs` 的 `save_mcp_server` INSERT 语句缺少 `enabled_codefree`
- `dao/skills.rs` 的 `update_skill_apps` UPDATE 语句缺少 `enabled_codefree`

根因：第二轮合并（`06e40250`）时 codefree 分支（`a436491a`）的 schema.rs 包含 `enabled_codefree` 列和 `migrate_v16_to_v17`（codefree 版），但上游 `87b0e3fb` 的 schema.rs 不包含这些，合并后上游版本覆盖了 codefree 内容。第三轮合并后上游 `migrate_v16_to_v17` 用于创建 `session_usage_dedup` 表，与 codefree 的迁移冲突。

## What Changes

- `SCHEMA_VERSION` 从 17 提升到 18
- `create_tables_on_conn` 中 `mcp_servers` 和 `skills` 表添加 `enabled_codefree BOOLEAN NOT NULL DEFAULT 0` 列
- 新增 `migrate_v17_to_v18` 迁移函数：为 `mcp_servers`、`skills` 表添加 `enabled_codefree` 列
- `migrate` match 中添加 `17 => migrate_v17_to_v18(conn)` 分支
- `dao/mcp.rs` 的 `save_mcp_server` INSERT 语句补回 `enabled_codefree` 列和 `server.apps.codefree` 参数
- `dao/skills.rs` 的 `update_skill_apps` UPDATE 语句补回 `enabled_codefree = ?7` 和 `apps.codefree` 参数
- 新增测试 `migrate_v17_to_v18_adds_codefree_flags` 验证迁移正确性

## Capabilities

### New Capabilities

（无新增能力）

### Modified Capabilities

- `codefree-skills-mcp`: 恢复数据库层对 CodeFree 的 MCP/Skills 启用状态持久化支持

## Impact

- `src-tauri/src/database/mod.rs`：`SCHEMA_VERSION` 17 → 18
- `src-tauri/src/database/schema.rs`：`create_tables_on_conn` 添加 `enabled_codefree` 列；新增 `migrate_v17_to_v18`；match 添加 `17 =>` 分支；新增测试
- `src-tauri/src/database/dao/mcp.rs`：`save_mcp_server` INSERT 补回 `enabled_codefree`
- `src-tauri/src/database/dao/skills.rs`：`update_skill_apps` UPDATE 补回 `enabled_codefree`
