## 1. schema.rs 修复

- [x] 1.1 `create_tables_on_conn` 中 `mcp_servers` 表添加 `enabled_codefree BOOLEAN NOT NULL DEFAULT 0` 列
- [x] 1.2 `create_tables_on_conn` 中 `skills` 表添加 `enabled_codefree BOOLEAN NOT NULL DEFAULT 0` 列
- [x] 1.3 新增 `migrate_v17_to_v18` 迁移函数，为 `mcp_servers`、`skills` 表添加 `enabled_codefree` 列
- [x] 1.4 `migrate` match 中添加 `17 => migrate_v17_to_v18(conn)` 分支
- [x] 1.5 新增测试 `migrate_v17_to_v18_adds_codefree_flags`

## 2. mod.rs 修复

- [x] 2.1 `SCHEMA_VERSION` 从 17 改为 18

## 3. DAO 层修复

- [x] 3.1 `dao/mcp.rs` 的 `save_mcp_server` INSERT 语句补回 `enabled_codefree` 列和 `server.apps.codefree` 参数
- [x] 3.2 `dao/skills.rs` 的 `update_skill_apps` UPDATE 语句补回 `enabled_codefree = ?7` 和 `apps.codefree` 参数

## 4. 验证

- [x] 4.1 `cargo test --lib migrate_v` 全部通过（6 个迁移测试）
- [x] 4.2 `cargo test --lib database` 全部通过（93 个数据库测试）
- [x] 4.3 `cargo fmt --check` 通过
- [x] 4.4 `cargo clippy -- -D warnings` 通过
- [x] 4.5 `pnpm typecheck` 通过
- [x] 4.6 `pnpm format:check` 通过
