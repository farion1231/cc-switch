# CC-Switch "工作目录" 功能 — 实施方案

## Context

CC-Switch 管理 5 个 CLI 工具（Claude Code / Codex / Gemini CLI / OpenCode / OpenClaw）的供应商、MCP 服务器、Skills、提示词配置。当前所有启用状态是全局的——用户在不同项目间切换时需要手动 toggle。

本功能允许用户注册多个工作目录（项目文件夹），切换目录时自动保存/恢复各实体的启用状态。**不做数据隔离**——所有实体共享全局池，仅 "谁是激活的" 按目录区分。

---

## 一、需要按目录区分的实体（完整清单）

| 实体 | 当前状态字段 | 存储方式 | 需要区分？ | 理由 |
|------|-------------|---------|-----------|------|
| **Provider** | `is_current` | per `(id, app_type)` | **YES** | 不同项目用不同供应商 |
| **Provider (Failover)** | `in_failover_queue` | per `(id, app_type)` | **YES** | 备用供应商队列跟随主供应商配置 |
| **MCP Server** | `enabled_claude/codex/gemini/opencode` | per `id`, 4列 | **YES** | 不同项目需要不同 MCP 工具 |
| **Skill** | `enabled_claude/codex/gemini/opencode` | per `id`, 4列 | **YES** | 不同项目需要不同 Skills |
| **Prompt** | `enabled` | per `(id, app_type)`, 单选 | **YES** | 不同项目用不同系统提示词 |
| Proxy Config | `enabled`, thresholds | per `app_type` | NO | 基础设施级别，非项目相关 |
| Settings | key-value | flat table | NO | 全局用户偏好 |
| Provider Health | failures, errors | runtime | **CLEAR** | 切换时清除，重新计算 |
| Common Config | `common_config_{app}` | settings table | NO | 全局模板，非项目相关 |
| Usage/Logs | historical | various tables | NO | 历史数据，不应分区 |

> 原计划遗漏了 **Failover Queue** 和 **Provider Health 清除**。

---

## 二、数据库变更（Schema v8 → v9）

### 新增 5 张表

```sql
-- 1. 工作目录注册表
CREATE TABLE IF NOT EXISTS working_directories (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT,
    is_current BOOLEAN NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0
);

-- 2. Provider 状态快照 (is_current + in_failover_queue)
--    每个目录保存所有 provider 的两个状态标志
CREATE TABLE IF NOT EXISTS dir_provider_state (
    dir_id TEXT NOT NULL,
    app_type TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    is_current BOOLEAN NOT NULL DEFAULT 0,
    in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
    PRIMARY KEY (dir_id, app_type, provider_id)
);

-- 3. MCP 启用状态快照 (直接镜像 4 列，不做行展开)
CREATE TABLE IF NOT EXISTS dir_mcp_state (
    dir_id TEXT NOT NULL,
    mcp_id TEXT NOT NULL,
    enabled_claude BOOLEAN NOT NULL DEFAULT 0,
    enabled_codex BOOLEAN NOT NULL DEFAULT 0,
    enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
    enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
    PRIMARY KEY (dir_id, mcp_id)
);

-- 4. Skill 启用状态快照 (直接镜像 4 列)
CREATE TABLE IF NOT EXISTS dir_skill_state (
    dir_id TEXT NOT NULL,
    skill_id TEXT NOT NULL,
    enabled_claude BOOLEAN NOT NULL DEFAULT 0,
    enabled_codex BOOLEAN NOT NULL DEFAULT 0,
    enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
    enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
    PRIMARY KEY (dir_id, skill_id)
);

-- 5. Prompt 启用状态快照 (每个 app_type 只存激活的 prompt_id)
CREATE TABLE IF NOT EXISTS dir_prompt_state (
    dir_id TEXT NOT NULL,
    app_type TEXT NOT NULL,
    prompt_id TEXT NOT NULL,
    PRIMARY KEY (dir_id, app_type)
);
```

### 设计决策说明

**MCP/Skill 用 4 列镜像而非 `(entity_id, app_type, enabled)` 行展开**：
- 与主表 `mcp_servers` / `skills` 结构一致，snapshot/apply 代码直接 copy 4 列
- 避免 4 倍行膨胀（每个 MCP 服务器 1 行 vs 4 行）
- 未来增加新 app 时，两边同步加列即可

**Prompt 只存 `(dir_id, app_type, prompt_id)`**：
- 每个 app_type 最多一个 enabled prompt，不需要存 boolean
- 无记录 = 该 app 无激活 prompt

**Provider 合并 `is_current` + `in_failover_queue`**：
- 两个标志都是 per `(app_type, provider_id)` 的状态
- 存在同一表中避免多表 JOIN

### 迁移脚本

在 `schema.rs` 中：
- `create_tables_on_conn()` 添加 5 个 CREATE TABLE
- 新增 `migrate_v8_to_v9(conn)`: 创建 5 张表 + 插入 `__default__` 行
- `SCHEMA_VERSION` 升至 9
- 迁移循环添加 `7 => ...` 后加 `8 => { Self::migrate_v8_to_v9(conn)?; Self::set_user_version(conn, 9)?; }`

```rust
fn migrate_v8_to_v9(conn: &Connection) -> Result<(), AppError> {
    // 创建 5 张表（使用 IF NOT EXISTS，幂等）
    // ...
    // 插入 __default__ 虚拟目录，代表"全局默认"状态
    conn.execute(
        "INSERT OR IGNORE INTO working_directories (id, path, name, is_current, created_at) \
         VALUES ('__default__', '__default__', NULL, 0, ?1)",
        [crate::database::get_unix_timestamp()?],
    )?;
    Ok(())
}
```

---

## 三、后端实现

### 3.1 DAO 层 — `src-tauri/src/database/dao/working_dir.rs`

所有方法都是 `impl Database` 块，遵循现有 DAO 模式。

**关键方法签名**（需要 `_on_conn` 变体支持事务）：

```rust
// ═══ 工作目录 CRUD ═══
pub fn list_working_directories(&self) -> Result<Vec<WorkingDirectory>, AppError>
pub fn add_working_directory(&self, id: &str, path: &str, name: Option<&str>) -> Result<(), AppError>
pub fn delete_working_directory(&self, id: &str) -> Result<(), AppError>
pub fn rename_working_directory(&self, id: &str, name: &str) -> Result<(), AppError>
pub fn get_current_working_directory(&self) -> Result<Option<WorkingDirectory>, AppError>

// 使用 _on_conn 变体，在 Service 层的事务中调用
fn set_current_working_directory_on_conn(conn: &Connection, id: &str) -> Result<(), AppError>

// ═══ 快照写入 ═══ (都有 _on_conn 变体)
fn snapshot_providers_on_conn(conn: &Connection, dir_id: &str) -> Result<(), AppError>
fn snapshot_mcp_on_conn(conn: &Connection, dir_id: &str) -> Result<(), AppError>
fn snapshot_skills_on_conn(conn: &Connection, dir_id: &str) -> Result<(), AppError>
fn snapshot_prompts_on_conn(conn: &Connection, dir_id: &str) -> Result<(), AppError>

// ═══ 快照恢复 ═══ (都有 _on_conn 变体, 返回 bool = 是否有快照)
fn apply_provider_snapshot_on_conn(conn: &Connection, dir_id: &str) -> Result<bool, AppError>
fn apply_mcp_snapshot_on_conn(conn: &Connection, dir_id: &str) -> Result<bool, AppError>
fn apply_skill_snapshot_on_conn(conn: &Connection, dir_id: &str) -> Result<bool, AppError>
fn apply_prompt_snapshot_on_conn(conn: &Connection, dir_id: &str) -> Result<bool, AppError>
```

**snapshot_providers 实现思路**：
```sql
-- 先清除旧快照
DELETE FROM dir_provider_state WHERE dir_id = ?1;
-- 从主表复制当前状态
INSERT INTO dir_provider_state (dir_id, app_type, provider_id, is_current, in_failover_queue)
SELECT ?1, app_type, id, is_current, in_failover_queue
FROM providers
WHERE is_current = 1 OR in_failover_queue = 1;
```

**apply_provider_snapshot 实现思路**：
```sql
-- 检查是否有快照
SELECT COUNT(*) FROM dir_provider_state WHERE dir_id = ?1;  -- 如果 0，返回 false

-- 在事务中：先清除主表所有 is_current 和 in_failover_queue
UPDATE providers SET is_current = 0;
UPDATE providers SET in_failover_queue = 0;

-- 从快照恢复
UPDATE providers SET is_current = 1
WHERE (id, app_type) IN (SELECT provider_id, app_type FROM dir_provider_state WHERE dir_id = ?1 AND is_current = 1);

UPDATE providers SET in_failover_queue = 1
WHERE (id, app_type) IN (SELECT provider_id, app_type FROM dir_provider_state WHERE dir_id = ?1 AND in_failover_queue = 1);
```

**snapshot_mcp / snapshot_skills 实现思路**（直接镜像 4 列）：
```sql
DELETE FROM dir_mcp_state WHERE dir_id = ?1;
INSERT INTO dir_mcp_state (dir_id, mcp_id, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode)
SELECT ?1, id, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode
FROM mcp_servers;
```

**apply_mcp_snapshot 实现思路**：
```sql
-- 先全部禁用
UPDATE mcp_servers SET enabled_claude = 0, enabled_codex = 0, enabled_gemini = 0, enabled_opencode = 0;

-- 从快照恢复
UPDATE mcp_servers SET
    enabled_claude = (SELECT enabled_claude FROM dir_mcp_state WHERE dir_id = ?1 AND mcp_id = mcp_servers.id),
    enabled_codex  = (SELECT enabled_codex  FROM dir_mcp_state WHERE dir_id = ?1 AND mcp_id = mcp_servers.id),
    enabled_gemini = (SELECT enabled_gemini FROM dir_mcp_state WHERE dir_id = ?1 AND mcp_id = mcp_servers.id),
    enabled_opencode = (SELECT enabled_opencode FROM dir_mcp_state WHERE dir_id = ?1 AND mcp_id = mcp_servers.id)
WHERE id IN (SELECT mcp_id FROM dir_mcp_state WHERE dir_id = ?1);
```

### 3.2 Service 层 — `src-tauri/src/services/working_dir.rs`

```rust
use crate::store::AppState;
use crate::error::AppError;
use crate::database::lock_conn;
use crate::app_config::AppType;
use crate::services::{McpService, ProviderService, SkillService};
use crate::config::write_text_file;
use crate::prompt_files::prompt_file_path;

pub struct WorkingDirService;

impl WorkingDirService {
    /// 核心切换逻辑
    pub fn switch(state: &AppState, target_dir_id: &str) -> Result<(), AppError> {
        // ═══ 前置检查 ═══
        // 1. 检查代理接管状态，若活跃则拒绝切换
        //    使用 db.is_live_takeover_active() 或同步检查 proxy_config.live_takeover_active
        //    （因为 ProxyService::is_running() 是 async，而此函数是 sync）
        Self::check_proxy_not_active(state)?;

        // ═══ Phase 1: 回填 Prompt ═══
        // 在 snapshot 之前，将 live 文件内容回填到当前 enabled prompt
        // 这样即使用户手动编辑了 live 文件，内容也不会丢失
        Self::backfill_prompt_content(state)?;

        // ═══ Phase 2: 数据库操作（事务） ═══
        {
            let conn = lock_conn!(state.db.conn);
            conn.execute("BEGIN IMMEDIATE", [])?;

            let result = (|| -> Result<(), AppError> {
                // 获取当前工作目录
                let current = Self::get_current_dir_id_on_conn(&conn)?;

                // 保存当前状态到旧目录
                if let Some(old_id) = &current {
                    Database::snapshot_providers_on_conn(&conn, old_id)?;
                    Database::snapshot_mcp_on_conn(&conn, old_id)?;
                    Database::snapshot_skills_on_conn(&conn, old_id)?;
                    Database::snapshot_prompts_on_conn(&conn, old_id)?;
                } else {
                    // 无当前目录 = 全局模式，保存到 __default__
                    Database::snapshot_providers_on_conn(&conn, "__default__")?;
                    Database::snapshot_mcp_on_conn(&conn, "__default__")?;
                    Database::snapshot_skills_on_conn(&conn, "__default__")?;
                    Database::snapshot_prompts_on_conn(&conn, "__default__")?;
                }

                // 加载目标目录快照（如果有的话）
                // 如果无快照（首次进入），保持主表不变
                Database::apply_provider_snapshot_on_conn(&conn, target_dir_id)?;
                Database::apply_mcp_snapshot_on_conn(&conn, target_dir_id)?;
                Database::apply_skill_snapshot_on_conn(&conn, target_dir_id)?;
                Database::apply_prompt_snapshot_on_conn(&conn, target_dir_id)?;

                // 更新 is_current 标记
                Database::set_current_working_directory_on_conn(&conn, target_dir_id)?;

                Ok(())
            })();

            match result {
                Ok(()) => conn.execute("COMMIT", [])?,
                Err(e) => {
                    let _ = conn.execute("ROLLBACK", []);
                    return Err(e);
                }
            };
        }
        // conn 锁在此处释放

        // ═══ Phase 3: 同步 live 配置文件 ═══
        Self::sync_all_live(state)?;

        // ═══ Phase 4: 清除 Provider Health ═══
        state.db.clear_all_provider_health()?;

        Ok(())
    }

    /// 回填 live prompt 文件内容到 DB（切换前调用）
    fn backfill_prompt_content(state: &AppState) -> Result<(), AppError> {
        for app in AppType::all() {
            let path = prompt_file_path(&app)?;
            if !path.exists() { continue; }
            let live_content = std::fs::read_to_string(&path).unwrap_or_default();
            if live_content.trim().is_empty() { continue; }

            let mut prompts = state.db.get_prompts(app.as_str())?;
            if let Some((_, prompt)) = prompts.iter_mut().find(|(_, p)| p.enabled) {
                prompt.content = live_content;
                prompt.updated_at = Some(get_unix_timestamp()?);
                state.db.save_prompt(app.as_str(), prompt)?;
            }
        }
        Ok(())
    }

    /// 将 DB 中的 enabled prompt 内容写入 live 文件（切换后调用）
    /// 注意：不做回填！只写入。区别于 PromptService::enable_prompt()
    fn write_prompts_to_live(state: &AppState) -> Result<(), AppError> {
        for app in AppType::all() {
            let path = prompt_file_path(&app)?;
            let prompts = state.db.get_prompts(app.as_str())?;
            if let Some(prompt) = prompts.values().find(|p| p.enabled) {
                write_text_file(&path, &prompt.content)?;
            }
            // 无 enabled prompt 时不清空文件（保留现状）
        }
        Ok(())
    }

    /// 同步所有 live 配置（Provider + MCP + Skill + Prompt）
    fn sync_all_live(state: &AppState) -> Result<(), AppError> {
        // 1. Provider → live files
        ProviderService::sync_current_to_live(state)?;
        // sync_current_to_live 内部已调用 McpService::sync_all_enabled()

        // 2. Skills → app dirs (循环每个 app)
        for app in AppType::all() {
            let _ = SkillService::sync_to_app(&state.db, &app);
        }

        // 3. Prompts → live files
        Self::write_prompts_to_live(state)?;

        Ok(())
    }

    /// 检查代理是否活跃（同步检查数据库标志）
    fn check_proxy_not_active(state: &AppState) -> Result<(), AppError> {
        // 检查 proxy_config 表中 live_takeover_active 列
        // 如果有任何 app 的 live_takeover_active = 1，拒绝切换
        let conn = lock_conn!(state.db.conn);
        let active: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_config WHERE live_takeover_active = 1)",
            [], |r| r.get(0)
        ).unwrap_or(false);

        if active {
            return Err(AppError::Message(
                "代理接管模式运行中，请先停止代理再切换工作目录".into()
            ));
        }
        Ok(())
    }
}
```

### 3.3 Command 层 — `src-tauri/src/commands/working_dir.rs`

遵循现有模式：`State<'_, AppState>` + `Result<T, String>` + `.map_err(|e| e.to_string())`。

```rust
#[tauri::command]
pub fn list_working_directories(state: State<'_, AppState>) -> Result<Vec<WorkingDirectory>, String>

#[tauri::command]
pub fn add_working_directory(state: State<'_, AppState>, path: String, name: Option<String>) -> Result<WorkingDirectory, String>

#[tauri::command]
pub fn delete_working_directory(state: State<'_, AppState>, id: String) -> Result<(), String>

#[tauri::command]
pub fn rename_working_directory(state: State<'_, AppState>, id: String, name: String) -> Result<(), String>

#[tauri::command]
pub fn switch_working_directory(state: State<'_, AppState>, id: String) -> Result<(), String>
// 调用 WorkingDirService::switch()

#[tauri::command]
pub fn get_current_working_directory(state: State<'_, AppState>) -> Result<Option<WorkingDirectory>, String>
```

### 3.4 需修改的现有文件

| 文件 | 修改内容 |
|------|---------|
| `src-tauri/src/database/schema.rs` | 添加 5 个 CREATE TABLE + `migrate_v8_to_v9()` |
| `src-tauri/src/database/mod.rs` | `SCHEMA_VERSION = 9` + 迁移循环加 `8 => ...` + `pub mod working_dir` in dao |
| `src-tauri/src/database/dao/mod.rs` | 添加 `pub mod working_dir;` |
| `src-tauri/src/services/mod.rs` | 添加 `pub mod working_dir;` + `pub use working_dir::WorkingDirService;` |
| `src-tauri/src/commands/mod.rs` | 添加 `mod working_dir;` + `pub use working_dir::*;` |
| `src-tauri/src/lib.rs` | invoke_handler 注册 6 个新命令 |

### 3.5 可能需要新增的 DAO 辅助方法

`src-tauri/src/database/dao/failover.rs`：
```rust
/// 清除所有 provider_health 记录（切换目录时调用）
pub fn clear_all_provider_health(&self) -> Result<(), AppError>
```

---

## 四、前端实现

### 4.1 API — `src/lib/api/workingDir.ts`

```typescript
import { invoke } from "@tauri-apps/api/core";

export interface WorkingDirectory {
  id: string;
  path: string;
  name?: string;
  isCurrent: boolean;
  createdAt: number;
}

export const workingDirApi = {
  list: () => invoke<WorkingDirectory[]>("list_working_directories"),
  add: (path: string, name?: string) =>
    invoke<WorkingDirectory>("add_working_directory", { path, name }),
  delete: (id: string) => invoke<void>("delete_working_directory", { id }),
  rename: (id: string, name: string) =>
    invoke<void>("rename_working_directory", { id, name }),
  switch: (id: string) => invoke<void>("switch_working_directory", { id }),
  getCurrent: () =>
    invoke<WorkingDirectory | null>("get_current_working_directory"),
};
```

### 4.2 组件 — `src/components/WorkingDirSwitcher.tsx`

**位置**：Header toolbar，靠近 AppSwitcher。

**功能**：
- 下拉菜单显示已注册目录列表
- 当前目录高亮
- "浏览…" 按钮调用 Tauri 文件夹选择对话框
- 右键菜单：重命名、删除
- "__default__（全局）" 选项恢复到全局状态
- 切换后 invalidate 所有相关 React Query

**切换后的 Query Invalidation**：
```typescript
// 需要验证实际的 queryKey 名称
queryClient.invalidateQueries({ queryKey: ["providers"] });
queryClient.invalidateQueries({ queryKey: ["mcp-servers"] });
queryClient.invalidateQueries({ queryKey: ["installed-skills"] });
queryClient.invalidateQueries({ queryKey: ["prompts"] });
queryClient.invalidateQueries({ queryKey: ["workingDirectories"] });
```

### 4.3 i18n

三个文件都需更新：
- `src/i18n/locales/zh.json`
- `src/i18n/locales/en.json`
- `src/i18n/locales/ja.json`

---

## 五、切换流程时序

```
用户选择目录 B
    │
    ├── 1. check_proxy_not_active()
    │       → 如果代理接管中，返回错误，终止
    │
    ├── 2. backfill_prompt_content()
    │       → 读 live prompt 文件 → 更新 DB 中已启用 prompt 的 content
    │       → 保护用户手动编辑的 prompt 不丢失
    │
    ├── 3. BEGIN TRANSACTION
    │   ├── snapshot(old_dir / __default__)
    │   │   ├── providers → dir_provider_state (is_current + in_failover_queue)
    │   │   ├── mcp_servers → dir_mcp_state (4 列直接复制)
    │   │   ├── skills → dir_skill_state (4 列直接复制)
    │   │   └── prompts → dir_prompt_state (enabled prompt_id)
    │   │
    │   ├── apply(target_dir)
    │   │   ├── dir_provider_state → providers
    │   │   ├── dir_mcp_state → mcp_servers
    │   │   ├── dir_skill_state → skills
    │   │   └── dir_prompt_state → prompts
    │   │
    │   └── set_current_working_directory(target_dir)
    │
    ├── COMMIT
    │
    ├── 4. sync_all_live()
    │   ├── ProviderService::sync_current_to_live(state)
    │   │   └── 内部已调用 McpService::sync_all_enabled()
    │   ├── for app in AppType::all() { SkillService::sync_to_app(&db, &app) }
    │   └── write_prompts_to_live() ← 无回填，直接写
    │
    └── 5. clear_all_provider_health()
            → 清除运行时熔断器状态
```

---

## 六、边界情况处理

| 场景 | 处理方式 |
|------|---------|
| **首次进入目录（无快照）** | `apply_*_snapshot()` 返回 false，主表保持不变。用户调整后，下次切走时自动保存。 |
| **全局模式 → 目录** | 自动将当前状态 snapshot 到 `__default__` 虚拟目录。`__default__` 在 v9 迁移中预创建。 |
| **目录 → 全局模式** | 用户选择 `__default__`，恢复全局状态。 |
| **新增 MCP/Skill/Provider** | 新实体在 dir_*_state 中无记录。apply 时只更新有记录的实体，新增的保持 DB 默认值。 |
| **删除 MCP/Skill/Provider** | dir_*_state 中对应记录在 apply 时找不到主表行，UPDATE 影响 0 行，静默跳过。 |
| **删除工作目录** | 级联删除 dir_*_state 中所有 `dir_id` 匹配的行。若为当前目录，回退到 `__default__`。 |
| **代理接管中切换** | `check_proxy_not_active()` 检测到 `live_takeover_active = 1`，拒绝切换并提示用户先停止代理。 |
| **切换中途崩溃** | 事务保护 DB 操作的原子性。最坏情况：DB 已更新但 live 文件未同步。下次启动可添加恢复检查（Phase 2 优化）。 |
| **用户手动编辑了 prompt 文件** | `backfill_prompt_content()` 在切换前读取 live 文件回填到 DB，保护手动修改。 |

---

## 七、实施顺序

### Phase 1: 数据库
1. `database/schema.rs` — 5 个 CREATE TABLE + `migrate_v8_to_v9()`
2. `database/mod.rs` — `SCHEMA_VERSION = 9` + 迁移分支
3. `database/dao/working_dir.rs` — 全部 DAO 方法（`_on_conn` 变体）
4. `database/dao/failover.rs` — 新增 `clear_all_provider_health()`
5. `database/dao/mod.rs` — 注册模块

### Phase 2: 服务 + 命令
6. `services/working_dir.rs` — `WorkingDirService::switch()` 等
7. `commands/working_dir.rs` — 6 个 Tauri 命令
8. `services/mod.rs` — 注册模块
9. `commands/mod.rs` — 注册模块
10. `lib.rs` — invoke_handler 注册

### Phase 3: 前端
11. `src/lib/api/workingDir.ts` — API 封装
12. `src/types.ts` — WorkingDirectory 类型
13. `src/components/WorkingDirSwitcher.tsx` — UI 组件
14. `src/App.tsx` — 集成到 header toolbar
15. `src/i18n/locales/{zh,en,ja}.json` — 国际化

### Phase 4: 优化（可选）
16. 启动恢复检查（DB 状态 vs live 文件一致性）
17. 托盘菜单显示当前工作目录

---

## 八、关键文件索引

### 新增文件（5 个）
- `src-tauri/src/database/dao/working_dir.rs`
- `src-tauri/src/services/working_dir.rs`
- `src-tauri/src/commands/working_dir.rs`
- `src/lib/api/workingDir.ts`
- `src/components/WorkingDirSwitcher.tsx`

### 必须修改的文件（7 个）
- `src-tauri/src/database/schema.rs` — CREATE TABLE + 迁移
- `src-tauri/src/database/mod.rs` — 版本号 + 迁移循环
- `src-tauri/src/database/dao/mod.rs` — 模块注册
- `src-tauri/src/database/dao/failover.rs` — clear_all_provider_health
- `src-tauri/src/services/mod.rs` — 模块注册
- `src-tauri/src/commands/mod.rs` — 模块注册
- `src-tauri/src/lib.rs` — invoke_handler

### 必须修改的前端文件（4 个）
- `src/App.tsx` — 集成 WorkingDirSwitcher
- `src/types.ts` — WorkingDirectory 接口
- `src/i18n/locales/zh.json` — 中文
- `src/i18n/locales/en.json` — 英文
- `src/i18n/locales/ja.json` — 日文

### 参考文件（理解现有模式）
- `src-tauri/src/services/mcp.rs` — `sync_all_enabled()` (line 165)
- `src-tauri/src/services/skill.rs` — `sync_to_app()` (line 1707)
- `src-tauri/src/services/provider/mod.rs` — `sync_current_to_live()` (line 1552)
- `src-tauri/src/services/prompt.rs` — `enable_prompt()` (line 73) — 理解回填逻辑
- `src-tauri/src/prompt_files.rs` — prompt 文件路径
- `src-tauri/src/config.rs` — `write_text_file()` (line 176)

---

## 九、验证计划

### 后端验证
1. `cargo test` — DAO 层单元测试（使用 `Database::memory()`）
   - 快照/恢复往返一致性
   - 新增/删除实体后的 apply 行为
   - `__default__` 全局状态保护
   - 事务回滚测试
2. 手动测试 — 启动应用，创建两个目录，切换并验证 live 文件变化

### 前端验证
1. `pnpm typecheck` — TypeScript 类型检查
2. `pnpm lint` — ESLint 检查
3. 手动 UI 测试 — 工作目录切换器交互、query invalidation 后数据刷新
