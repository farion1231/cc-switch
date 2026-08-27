## Context

第三轮合并 `github/main`（`87b0e3fb` → `0b5da510`）后，发现第二轮合并时 CodeFree 的数据库定义被上游版本覆盖。需要在不修改上游已有迁移的前提下，恢复 CodeFree 的数据库支持。

## Goals / Non-Goals

**Goals**：
- 恢复 `mcp_servers` 和 `skills` 表的 `enabled_codefree` 列
- 提供从 v17 到 v18 的迁移路径，不修改上游 `migrate_v16_to_v17`
- 恢复 DAO 层对 `enabled_codefree` 的读写

**Non-Goals**：
- 不修改上游已有的 `migrate_v16_to_v17`（创建 `session_usage_dedup` 表）
- 不修改上游 `create_tables_on_conn` 中已有的列定义
- 不重构迁移链结构

## Decisions

### 决策 1：SCHEMA_VERSION 17 → 18

上游第三轮合并后 `SCHEMA_VERSION = 17`（v16→v17 创建 `session_usage_dedup`）。CodeFree 的迁移作为新版本 v18 追加，遵循"追加新版本添加 codefree 变更，不修改上游已有迁移"原则。

### 决策 2：migrate_v17_to_v18 实现

迁移函数为 `mcp_servers` 和 `skills` 表添加 `enabled_codefree BOOLEAN NOT NULL DEFAULT 0` 列。使用 `ALTER TABLE ... ADD COLUMN`，与 codefree 分支原始实现一致。

### 决策 3：create_tables_on_conn 同时添加列

`create_tables_on_conn` 用于全新建库，必须包含 `enabled_codefree` 列，确保新用户首次启动时数据库结构完整。这与迁移函数互补：迁移函数处理老用户升级，`create_tables_on_conn` 处理新用户初始化。

### 决策 4：DAO 层 INSERT/UPDATE 补回 enabled_codefree

- `dao/mcp.rs` 的 `save_mcp_server`：INSERT 语句必须包含 `enabled_codefree` 列和 `server.apps.codefree` 参数，否则保存 MCP 服务器时 CodeFree 启用状态会丢失
- `dao/skills.rs` 的 `update_skill_apps`：UPDATE 语句必须包含 `enabled_codefree = ?7` 和 `apps.codefree` 参数，否则更新 Skill 应用启用状态时 CodeFree 会被忽略

## Risks

- **风险**：老用户从 v16 升级到 v18 时，`migrate_v16_to_v17`（上游）和 `migrate_v17_to_v18`（codefree）会顺序执行，需确保两者无副作用冲突
- **缓解**：`migrate_v16_to_v17` 只创建 `session_usage_dedup` 表，`migrate_v17_to_v18` 只添加 `enabled_codefree` 列，两者操作对象不重叠

## Testing

- 新增单元测试 `migrate_v17_to_v18_adds_codefree_flags`：构造 v17 数据库，执行迁移，验证 `enabled_codefree` 列存在且默认值为 0
- 运行所有 database 相关测试（93 个）全部通过
- `cargo fmt --check` / `cargo clippy -- -D warnings` / `pnpm typecheck` / `pnpm format:check` 全部通过
