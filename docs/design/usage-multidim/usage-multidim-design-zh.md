# CC Switch 用量多维度归属与计费 详细设计

## 1. 文档信息

| 项目 | 内容 |
| --- | --- |
| 状态 | 已实现（P0），实现与本文一致；差异见第 10 节 |
| 上游需求 | `docs/plan/usage-multidim/usage-multidim-requirements-zh.md` |
| 上游评审 | `docs/review/usage-multidim/usage-multidim-review-zh.md` |
| 设计评审 | `docs/review/usage-multidim/usage-multidim-design-review-zh.md` |
| 上游数据库版本 | `SCHEMA_VERSION = 22`（main `717172bd`） |
| 目标数据库版本 | `23` |
| 最后更新 | 2026-10-23 |

本文把需求落到具体代码：数据模型与迁移、归属算法、后端命令、前端组件、实施顺序与测试门禁。实现位置以文件和符号定位，历史行号仅供参考；最新上游兼容调整见第 12、13 节。

## 2. 设计原则

1. 供应商与项目先写入统一用量事实，再做去重、汇总与展示。
2. 归属证据与统计结果分开存储，统计结果不能反向成为归属证据。
3. 切换式客户端用激活时间线，累加式客户端用原生供应商标识，代理模式用代理日志，三者互不覆盖。
4. 去重是 write-side skip + read-side filter 两条路径，项目证据必须在会话行被跳过前回写。
5. 所有未知值用空哨兵或 `unknown`，不用当前值补齐。
6. 本次落地 P0 全量：数据模型、归属基础设施、查询、API 与前端。

## 3. 数据模型与迁移

### 3.1 迁移骨架

`SCHEMA_VERSION` 在 `src-tauri/src/database/mod.rs`。迁移循环 `Database::apply_schema_migrations_on_conn` 逐级升级。最新上游已使用 v23，多维用量使用独立的 v23→v24：

- 把 `SCHEMA_VERSION` 改为 `24`。
- 保留上游 Antigravity、Kimi 和 Oh My Pi 的 v20→v21、v21→v22、v22→v23 迁移，在 `23 =>` 调用 `migrate_v23_to_v24(conn)`。
- `migrate_v23_to_v24` 按 3.2~3.5 执行；对于已含多维字段的早期 PR v21/v23 数据库，保留现有归属和汇总维度。早期 PR v23 尚未包含上游 `enabled_omp`，在此幂等补齐该列，不改变其他 MCP 开关。

迁移失败必须回滚，现有外层 `SAVEPOINT schema_migration` 已覆盖，无需额外处理。

### 3.2 新表

```sql
CREATE TABLE provider_activations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_type TEXT NOT NULL,
    provider_id TEXT,
    mode TEXT NOT NULL,
    effective_at_ms INTEGER NOT NULL,
    source TEXT NOT NULL,
    config_fingerprint TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_provider_activations_lookup
    ON provider_activations(app_type, effective_at_ms);
```

`provider_id` 为空表示非直连边界；`config_fingerprint` 只存哈希；`source` 取 `switch | import | startup | mode_change`。

```sql
CREATE TABLE session_projects (
    app_type TEXT NOT NULL,
    session_key TEXT NOT NULL,
    project_key TEXT NOT NULL,
    project_path TEXT NOT NULL,
    project_name TEXT NOT NULL,
    source TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (app_type, session_key)
);
CREATE INDEX idx_session_projects_project ON session_projects(project_key);
```

### 3.3 proxy_request_logs 新增列

```sql
ALTER TABLE proxy_request_logs ADD COLUMN project_key TEXT NOT NULL DEFAULT '';
ALTER TABLE proxy_request_logs ADD COLUMN project_name TEXT NOT NULL DEFAULT '';
ALTER TABLE proxy_request_logs ADD COLUMN project_path TEXT NOT NULL DEFAULT '';
ALTER TABLE proxy_request_logs ADD COLUMN session_key TEXT NOT NULL DEFAULT '';
ALTER TABLE proxy_request_logs ADD COLUMN attribution_source TEXT NOT NULL DEFAULT 'proxy';
ALTER TABLE proxy_request_logs ADD COLUMN attribution_confidence TEXT NOT NULL DEFAULT 'exact';
ALTER TABLE proxy_request_logs ADD COLUMN attribution_reason TEXT NOT NULL DEFAULT '';
ALTER TABLE proxy_request_logs ADD COLUMN provider_name_snapshot TEXT NOT NULL DEFAULT '';
```

新增列统一走 `add_column_if_missing`。`session_key` 是项目关联的规范化键，`session_id` 保留原始值，避免影响现有代理缓存身份。

首次引入归属列时，必须把存量会话行标记为未识别，不能用列默认的 `proxy/exact` 误标；已有归属列和证据的早期 PR 数据库不执行这一步：

```sql
UPDATE proxy_request_logs
SET attribution_source = 'unknown',
    attribution_confidence = 'unknown',
    attribution_reason = 'pre_migration_no_evidence'
WHERE data_source IS NOT NULL AND data_source != 'proxy';
```

新增索引：

```sql
CREATE INDEX idx_request_logs_project_time ON proxy_request_logs(project_key, created_at);
CREATE INDEX idx_request_logs_provider_project_time
    ON proxy_request_logs(provider_id, project_key, created_at);
CREATE INDEX idx_request_logs_session_key ON proxy_request_logs(app_type, session_key);
```

### 3.4 usage_daily_rollups 重建

上游表含 `input_token_semantics`。缺少 `project_key` 或 `data_source` 时，采用 rename + 重建模式，把两者纳入主键。下列 SQL 展示上游原始表的默认回填路径；已有部分维度时动态复制原值，已含完整维度时跳过重建，仅补缺失列：

```sql
ALTER TABLE usage_daily_rollups RENAME TO usage_daily_rollups_v22;
CREATE TABLE usage_daily_rollups (
    date TEXT NOT NULL,
    app_type TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    request_model TEXT NOT NULL DEFAULT '',
    pricing_model TEXT NOT NULL DEFAULT '',
    data_source TEXT NOT NULL DEFAULT 'proxy',
    project_key TEXT NOT NULL DEFAULT '',
    project_name TEXT NOT NULL DEFAULT '',
    project_path TEXT NOT NULL DEFAULT '',
    provider_name_snapshot TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    input_token_semantics INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0',
    avg_latency_ms INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model, data_source, project_key)
);
INSERT INTO usage_daily_rollups
    (date, app_type, provider_id, model, request_model, pricing_model, data_source, project_key,
     project_name, project_path, provider_name_snapshot,
     request_count, success_count, input_tokens, output_tokens,
     cache_read_tokens, cache_creation_tokens, input_token_semantics,
     total_cost_usd, avg_latency_ms)
SELECT date, app_type, provider_id, model, request_model, pricing_model, 'proxy', '',
     '', '', '',
     request_count, success_count, input_tokens, output_tokens,
     cache_read_tokens, cache_creation_tokens, input_token_semantics,
     total_cost_usd, avg_latency_ms
FROM usage_daily_rollups_v22;
DROP TABLE usage_daily_rollups_v22;
```

### 3.5 迁移基线

- 为处于直连模式且能证明当前供应商的切换式客户端写入一条 `source = 'startup'` 基线，`effective_at_ms` 取迁移完成时间，不回溯历史。
- 尽力从现存会话文件构建 `session_projects`；旧日汇总的 `provider_name_snapshot` 从现存 `providers` 表按 `(id, app_type)` 尽力回填，缺失保持为空。

## 4. 归属算法

### 4.1 供应商激活时间线写入

代码里的模式系统只有 `Mode::Direct | Mode::Proxy`（`src-tauri/src/mode/state.rs:54`），没有独立“聚合”模式；故障转移与多供应商栈路由都仍是 `Mode::Proxy`。设计中的模式取值因此收敛为 `direct | proxy`。

写入钩子不能放在 DAO 的 `set_current_provider`（会被 seed、导入兜底、测试路径误触发），必须放在 service 层的四个入口：

| 入口 | 代码位置 | source | mode / provider_id |
| --- | --- | --- | --- |
| 用户切换供应商 | `ProviderService::switch`（`src-tauri/src/services/provider/mod.rs:6552`）`Ok` 分支 | `switch` | 当前为直连：`direct` + 目标供应商；当前为代理（`switch_inner` 的 `is_proxy` 分支只改 `proxy_route`）：`proxy` + 路由供应商 |
| 手动导入 live 配置 | `import_default_config` 成功后 | `import` | 按导入后实际模式与供应商 |
| 进入 / 退出代理 | `mode/operation.rs::run`、`commit_target` 提交点 | `mode_change` | 进入代理：`proxy` + 路由供应商或空；退出代理：`direct` + 直连指针供应商或空 |
| 启动恢复 | `mode/operation.rs::recover_on_startup` 完成后 | `startup` | 按恢复后的稳定模式与供应商，仅在迁移后首次启动写基线 |

记录内容：

```text
provider_activations(app_type, provider_id, mode, effective_at_ms=now_ms,
    source, config_fingerprint=hash(live config), created_at=now_ms)
```

时间线写入失败不能阻断切换本身，只记录日志与数据质量计数。同一供应商连续重复激活可以合并，但不能跨模式或跨配置指纹合并。

时间单位：`proxy_request_logs.created_at` 是秒级，`effective_at_ms` 是毫秒级。归属查询统一换算为毫秒：`effective_at_ms <= start_s * 1000`；同秒歧义判定按秒对齐后再比较。

### 4.2 直连供应商归属

```text
输入：请求区间 [start, end]、app_type、mode
若会话/原生配置有唯一供应商 ID：直接用（source=session_meta，confidence=exact）
否则若 mode != direct：不查时间线（用代理结果或未识别）
否则：
    取 effective_at_ms <= start 的最后一条激活记录，按 (effective_at_ms DESC, id DESC)
    无记录 → 未识别
    记录非 direct 或 provider_id 为空 → 未识别
    [start, end] 内还有另一条激活记录 → 歧义
    start/end 秒级相同且边界秒内有激活 → 歧义
    否则用该 provider_id（source=activation，confidence=high）
```

Codex 的 `model_provider` 分桶规则保持专属：`openai` → 官方、唯一 ID → 直接映射、`custom` → 查时间线、其余 → `_codex_session` 未识别。

### 4.3 会话项目识别

每个会话用量适配器读取本客户端声明的项目字段（Codex `session_meta.cwd`、Claude/Gemini/OpenClaw/Hermes `cwd`、Grok Build `cwd`、Pi `cwd`、OpenCode `directory`、MiniMax `workspace_dir`），规范化为 `project_key` 后写入 `session_projects`：

```text
project_key = git_root(cwd) 若可解析，否则 normalize(cwd)
```

规范化：去尾斜杠、解析存在的路径、Windows 分隔符统一、`file://` URI 转本地路径；路径不存在保留原始串不丢会话。

### 4.4 代理日志项目关联与去重修正

代理行写入时生成 `session_key`（适配器统一代理 session_id 与会话 ID 格式，处理 Codex `codex_` 前缀、Claude `metadata.user_id`/会话头差异）。项目关联优先级：明细行 `project_key` → `session_projects` 关联 → 未识别。

去重是两条路径（见评审 R-22）：

- write-side：`should_skip_session_insert`（`src-tauri/src/services/usage_stats.rs:693`）在写入会话行前判定命中代理行即跳过，调用点 `session_usage.rs:1188/:1272`、`session_usage_codex.rs:2194`、`session_usage_gemini.rs:275`、`session_usage_opencode.rs:460`。
- read-side：`effective_usage_log_filter`（`usage_stats.rs:477`）对已落库会话行用 `NOT EXISTS` 过滤。

改造要求：在 `should_skip_session_insert` 判定为跳过的**同一处**，先把会话日志的项目证据写回命中的代理行或 `session_projects`，再跳过；不能依赖读取时从“被跳过的会话行”继承。代理日志的 `provider_id` 始终是供应商强证据，会话日志不得覆盖。

### 4.5 名称快照写入时机

迁移只回填存量数据。新写入路径必须在 insert 时带上 `provider_name_snapshot`：

- 各会话用量适配器的 `INSERT INTO proxy_request_logs`（`session_usage*.rs`）：归属成功时写当时 `providers.name`；未识别时写空串。
- 代理请求写日志处：写被路由供应商的当前名称。

请求详情优先取 `provider_name_snapshot`，为空才回退实时 `providers.name`（需求 FR-10）。汇总、筛选菜单和日志列表沿用当前卡片名，删除后再回退快照；过滤仍使用原始供应商 ID 和应用类型。

### 4.6 存量归属与项目补齐

会话同步有 `session_usage_dedup` 账本，普通重复同步不会重新导入已存在的用量。Codex 的专用一次性回填读取已有 `session_meta` 补项目及可证明的供应商证据；没有证据时保持未识别，不能依赖当前选中供应商推断历史。

一次性回填同时覆盖因去重只保留真实供应商代理行的会话，先规范化旧 `session_key`，再按应用和会话键补齐空项目。完成标记采用 v2，修复旧标记提前完成的部署，完成后仍不重复扫描。运行时写入 `session_projects` 也补齐同会话中尚无项目的代理明细，包含失败请求；已有项目、供应商 ID、名称快照和归属证据均不覆盖，不重导 Token 或重建已裁剪历史。

## 5. 后端命令与查询

### 5.1 命令签名变更

给现有聚合接口增加可选 `projectKey`，未传统计全部项目；除单会话外继续接受可选 `appType`：

```text
get_usage_summary(startDate, endDate, appType?, providerName?, model?, projectKey?)
get_usage_trends(startDate, endDate, appType?, providerName?, model?, projectKey?)
get_provider_stats(startDate, endDate, appType?, providerName?, projectKey?)
get_model_stats(startDate, endDate, appType?, providerName?, projectKey?)
get_request_logs(filters: LogFilters{ projectKey? })
get_usage_first_date(appType?, projectKey?)
```

`get_session_usage_summary`、`get_usage_summary_by_app` 不增加项目分组。项目过滤在 `effective_usage_log_filter` 的去重条件之后、聚合之前追加 `log.project_key = ?`。

### 5.2 新增命令

```text
get_project_stats(startDate, endDate, appType?, providerName?, model?) -> ProjectStats[]
get_project_provider_stats(projectKey, startDate, endDate, appType?) -> ProviderStats[]
get_usage_projects(startDate?, endDate?, appType?) -> ProjectOption[]
```

`ProjectStats` 含 `projectKey/projectName/projectPath/requestCount/totalTokens/totalCost/freshInputTokens/cacheCreationTokens/cacheReadTokens/cacheableInputTokens/cacheHitRate/appCount/providerCount/lastActiveAt`。`ProviderStats` 增加三类 Token、`cacheableInputTokens`、`cacheHitRate`。

### 5.3 缓存命中率 SQL

复用 `fresh_input_sql`（`src-tauri/src/services/sql_helpers.rs:43`）归一化输入语义，命中率：

```text
cacheable = freshInput + cacheCreation + cacheRead
cacheHitRate = cacheRead / cacheable
```

四个层级（整体/项目/供应商/模型）必须共享同一批有效行和同一去重条件，禁止单行命中率算术平均；分母为 0 返回可缓存输入 0，UI 显示 `—`。

### 5.4 日汇总 rollup 改造

`rollup_and_prune`（`src-tauri/src/database/dao/usage_rollup.rs`）的聚合要加 `project_key` 维度：

- `GROUP BY` 增加 `project_key`，与新主键一致。
- `project_name`、`project_path`、`provider_name_snapshot` 从同组明细行带出（组内取一条非空快照即可，同一组的快照语义一致）。
- 明细裁剪后，项目维度只保留在日汇总行里，因此快照列必须在汇总时写全。

## 6. 前端改动

| 文件 | 变更 |
| --- | --- |
| `src/types/usage.ts` | 新增 `ProjectStats`、`ProjectOption`、`ProviderStats` 缓存字段；`RequestLog` 增加 `projectKey/projectName/projectPath/attributionSource/attributionConfidence/attributionReason/providerNameSnapshot`；`CACHE_INCLUSIVE_APP_TYPES` 与 Rust 同步 |
| `src/lib/api/usage.ts` | 命令加 `projectKey` 参数，新增三个项目接口 |
| `src/components/usage/UsageDashboard.tsx` | 应用筛选旁增加项目筛选；新增“项目”页签 |
| `src/components/usage/ProviderStatsTable.tsx` | 增加缓存命中率列 |
| `src/components/usage/ProjectStatsTable.tsx` | 新增：项目列表 + 展开后供应商/模型占比图 |
| `src/components/usage/UsageShareChart.tsx` | 新增：环形占比图 + 图例，超过 6 项合并「其他」 |
| `src/components/usage/ProjectUsageBreakdown.tsx` | 新增：项目展开面板，供应商占比 + 模型占比，口径可切换 |
| `src/components/usage/ProviderUsageBreakdown.tsx` | 新增：供应商展开面板，模型占比 |
| `src/components/usage/RequestDetailPanel.tsx` | 展示项目、归属来源、置信度、未归属原因 |
| `src/i18n/locales/*.json` | 项目、未识别、缓存口径文案 |

新增客户端必须同步 Rust `CACHE_INCLUSIVE_APP_TYPES`（`sql_helpers.rs:23`）与前端同名集合，否则缓存分母错误（评审 R-19）。

## 7. 实施顺序

1. 数据库迁移 v23→v24（本设计第 3 节）+ 多个来源版本的迁移测试。
2. `provider_activations`、`session_projects` 的 DAO 写入接口与类型。
3. 切换式客户端激活时间线写入（`switch_provider` 后钩子）。
4. 各会话适配器项目字段解析与 `session_projects` 写入。
5. 各适配器供应商字段提取与映射，未识别原因。
6. 去重时项目证据回写（第 4.4 节）。
7. 项目/应用/供应商/缓存统计查询。
8. 日汇总项目维度与快照。
9. API 与前端筛选、页签、列。
10. 迁移测试、多客户端集成测试、UI 验收。

## 8. 测试计划

- 迁移：上游 v20/v21/v22/v23→v24 不丢行、新主键生效、首次增加归属列时存量会话行标记 `unknown`；早期 PR v21/v23 的归属和汇总分组原样保留，补齐 Oh My Pi 开关且重复迁移幂等。旧版本拒绝打开 v24（已有 `apply_schema_migrations_on_conn` 的超版本报错路径）。
- 归属：请求早于/落在/跨激活记录、同秒歧义、路由不用时间线、配置指纹不匹配。
- 项目：尾斜杠、符号链接、非 Git、路径不存在、子 agent 继承。
- 缓存：全新增 / 全命中 / 仅写入 / 可缓存为 0 / 多供应商 Token 加权。
- 去重：会话行被跳过前项目已回写（写入侧断言，不只验读侧）。
- 前端：项目筛选联动、供应商缓存列、未识别可筛选、无分母显示 `—`。

## 9. 风险与门禁

发布门禁沿用评审结论的 8 项（迁移备份恢复、去重项目不丢、三层缓存加权一致、未识别不静默回退、能力矩阵/注册表/适配器一致、跨 app_type 供应商隔离、历史详情名称快照优先、输入 Token 语义三处一致），其中迁移与去重是本设计直接触及的两项，必须先有测试再继续上层查询与 UI。

## 10. 实施状态与差异

本轮已按 P0 落地，主要实现位置：

| 能力 | 实现位置 |
| --- | --- |
| v23→v24 迁移 | `src-tauri/src/database/schema.rs::migrate_v23_to_v24` |
| 激活时间线 / 项目索引 / 归属算法 | `src-tauri/src/services/usage_attribution.rs` |
| 切换式客户端钩子 | `ProviderService::switch`、`mode::operation::commit_target`、`usage_attribution::ensure_startup_baselines`（启动调用） |
| Codex / Claude / Gemini / Grok 归属 | 各 `session_usage_*.rs`；Codex 读 `session_meta.cwd/model_provider` |
| 原生供应商映射 | OpenCode `model.providerID`；Pi `message.provider`；均按卡片 id/名称映射，未命中保留原生线索并标记 unknown |
| 代理行项目/快照 | `src-tauri/src/proxy/usage/logger.rs` |
| 去重项目回写 | `usage_attribution::skip_if_duplicate_with_project` / `backfill_if_proxy_duplicate` |
| 查询与 API | `usage_stats::{get_project_stats,get_project_provider_stats,get_usage_projects}`、`commands/usage.rs` |
| 数据质量 | `usage_stats::get_usage_data_quality`，只统计仍保留在明细里的行（日汇总无置信度列） |
| 日汇总项目维度 | `src-tauri/src/database/dao/usage_rollup.rs` |
| 前端 | 项目筛选、项目页签（应用数/供应商数分列、展开行含占项目比例）、供应商缓存命中率列、请求详情归属信息、数据质量提示 |

与设计的差异与边界：

1. 模式枚举只有 `Direct | Proxy`；进入/退出代理通过 `mode::operation::commit_target` 记录 `mode_change`，没有独立“聚合”模式。
2. `get_usage_summary_by_app` 增加了 `projectKey` 过滤参数（用于顶部 Hero 与项目筛选一致），但仍只按应用分组，不增加项目分组维度。
3. Mcode 暂无可用的项目字段，项目维度进入未识别桶；其余已接入客户端读取各自声明字段。
4. OpenCode/Pi 的原生供应商未命中卡片时保留原生标识并标记 `unknown/native_provider_not_mapped`，便于排查，不伪造到某张卡片。
5. 存量会话行受 `session_usage_dedup` 账本保护，重跑同步不会重新归属，保持迁移时的 `unknown`。
6. 配置指纹校验在激活记录与启动基线写入时进行；归属查询不做逐请求的指纹比对（无法用“当前 live 配置”去否定历史请求）。外部直接改配置造成的失真，靠启动基线与切换记录收敛，属已知边界。

验证结果（2026-10-20）：

- `cargo test --lib`：3833 通过；仅 `proxy::error::describe_reqwest_error_keeps_cause_and_drops_url` 因本机端口返回 502 失败，与本次改动无关。
- `pnpm test:unit`：238 个文件、3068 个用例全部通过。
- `pnpm typecheck`、`pnpm build:renderer`：通过。
- 真实数据副本 `~/.cc-switch/cc-switch.db`（v20，53,929 明细、127 日汇总）迁移 v21：行数全部保留，存量会话行标记 unknown，项目/供应商/缓存查询在副本上跑通。

## 11. 模型缓存下钻

2026-10-22 扩展，业务口径及验收以需求文档第 23 节为准。

- `ProjectUsageBreakdown` 和 `ProviderUsageBreakdown` 增加「缓存」口径，普通模型图例同时展示请求数和缓存命中率。项目入口继续按原始 `(providerId, providerAppType)` 筛选；缓存模式的供应商占比按输入 Token 计算，保留零输入条目和所有供应商，不合并成无法选择的「其他」。
- `ModelCacheBreakdown` 复用 `get_model_stats` 已有的三个输入分项，以命中 / 写入 / 新增输入的堆叠条展示所有模型，按输入量降序排列；汇总用分子分母分别求和后相除。列表限制高度且支持键盘滚动，详细读数常驻显示。
- `cacheStats` 只接受三个分项均为非负有限数的记录；有效零保留，缺失记录不填零。部分缺失时展示排除数量，全缺失时分子、分母和比例均不伪装成零。
- `BreakdownQueryState` 在首次加载或跨范围的 `isPlaceholderData` 期间展示加载状态；无有效数据且查询失败时提供重试。项目、应用及时间范围变化或供应商消失时清除选择。相同范围后台刷新可保留已有数据。
- 两个入口均继承范围栏的模型筛选。项目入口复用支持完整过滤的 `useProviderStats`，供应商及模型查询同时携带父级供应商身份、模型、项目、应用和时间；局部供应商选择只进一步收窄。父级供应商或模型变化也重置局部选择，取消局部选择时恢复父级范围。
- 不改变计价、日志归属、数据库结构、后端接口或模型分组规则；不增加查询次数。模型分组继续使用有效计价模型。

相关回归包含 `ModelCacheBreakdown.test.tsx`、`cacheStats.test.ts` 及两个下钻入口的组件测试。跨供应商旧数据误显示的原因是查询使用 `keepPreviousData`，消费端未区分占位结果；回归用例明确断言切换期间隐藏旧模型，避免同类问题复发。

## 12. 最新上游与 Kimi 兼容

2026-10-22 合并上游 `main` `e4873bc8`，保留新增的 Kimi Code 能力和原有迁移顺序。多维用量迁移单独占用 v23，覆盖上游 v21/v22 已安装用户，并保留早期 PR 构建生成的归属证据及按项目、数据源拆分的汇总。

Kimi 原生会话仍使用 `_kimi_session` 桶；新导入与已有记录刷新均写明 `unknown/unknown/kimi_native_session`，避免数据库默认值把原生记录误标为代理精确归属。刷新仅修正明细，不新增汇总用量。会话管理器已有目录信息，但用量导入尚未关联项目，当前明确保留为未识别项目，不以当前目录或当前供应商补齐。

Kimi 的统计测试调用同步新的项目、供应商应用与归属过滤参数。兼容回归覆盖 v20/v21/v22 升级、既有多维数据保留、部分汇总数据源保留，以及 Kimi 新导入和重复同步的归属修复。

## 13. 用量页面改版兼容

2026-10-23 合并上游 `main` `717172bd`。保留上游范围栏、供应商/模型级联菜单、指标与图表联动、用量设置抽屉、日志分页收缩修复、列分组和横向滚动行为。

- `ScopeModelItems` 按 `(providerId, providerAppType, projectKey)` 查询，供应商选项和选中状态按原始身份区分；同名、同 ID 但不同应用的供应商不能混合模型。
- 日志、供应商、模型页签计数和对应表格共用相同的项目及供应商过滤；项目页签沿用项目统计范围。`useProjectStats` 接受 `enabled`，未打开项目页签时不额外查询项目统计。
- 保留模型/供应商缓存命中率列和供应商展开入口，按上游“费用、Token、性能”顺序排列表格；项目/供应商模型缓存下钻继续使用原有加权与缺失数据规则。
- 原生会话详情继续显示项目、归属来源、置信度和原因，保留上游隐藏会话内部供应商 ID 的行为。
- Kimi 保留上游账号及解析器更新，同时保留本 PR 的原生会话未识别标记、归属修复和扩展统计参数。

## 14. Oh My Pi 与迁移版本兼容

2026-10-24 合并上游 `main` `cae7c375`，保留 Oh My Pi 和 Kimi 的最新能力。上游的 v22→v23 用于添加 `mcp_servers.enabled_omp`，多维用量迁移顺延至 v23→v24，避免上游 v23 用户跳过项目与供应商归属字段初始化。

此前 PR 的 v23 与上游 v23 结构不同，升级时同时补齐 Oh My Pi 开关，并保留已存在的供应商证据及项目、数据源汇总。回归覆盖上游 v20/v21/v22/v23、早期 PR v21/v23、MCP 既有开关、重复迁移及部分汇总维度。
