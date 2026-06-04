# Token Budget — 魔改规划（MVP：L0 + L1）

> Branch: `feat/token-budget-mvp`，从 `main` 拉出。
> 上游：`farion1231/cc-switch`（MIT）；当前仓库：`CacinieP/cc-switch`（私有魔改稳后提 PR）。

## 0. 目标

在 cc-switch 已有的"用量统计"之上，新增"**预算规划**"层：

- **L0 定义**：用户可按 *全局 / app / provider / model* 维度设定周期（日/周/月）预算，单位 USD 或 tokens。
- **L1 进度对比**：实时计算"当前周期已用 / 配额"，按现有 usage 数据流（无需新增采集）。
- 告警 / 熔断 / 预测 留到后续版本（L2+）。

不改 `proxy_request_logs`、`usage_daily_rollups` 等任何旧表，只新增。

---

## 1. cc-switch 既有钩子（我们能复用）

| 钩子 | 路径 | 作用 |
|---|---|---|
| 完整请求级 token/cost 表 | `proxy_request_logs` (`src-tauri/src/database/schema.rs:184`) | 预算计算的源数据，按 (provider_id, app_type, model, created_at) 索引 |
| 日聚合 | `usage_daily_rollups` (`schema.rs:259`) | "今天/本月花了多少"近乎免费可查 |
| 实时事件 | `usage_events.rs::notify_log_recorded` 200ms 防抖 emit `usage-log-recorded` | 前端 invalidate；L1 进度条天然刷新点 |
| 聚合查询服务 | `services/usage_stats.rs` (`UsageSummary`、`real_total_tokens`、`cache_hit_rate`) | 直接复用 SQL 模式 |
| Tauri command 注册 | `commands/usage.rs` + `lib.rs` | 复制粘贴模式 |
| 前端 react-query | `lib/query/usage.ts` + `hooks/useUsageEventBridge.ts` | 仿写一个 budget 版 |
| 前端面板 | `components/usage/UsageDashboard.tsx` | 在其上加 "Budgets" tab，或独立一级页面 |

**Grep 全仓库确认：**`budget` 关键字仅命中 `proxy/thinking_budget_*`（每条消息的 reasoning token 预算）。**无任何成本预算功能**。空缺正是这次工作要填的。

---

## 2. 数据模型

### 2.1 新增表（追加到 `src-tauri/src/database/schema.rs::create_tables_on_conn`）

```sql
-- 19. Token Budgets 表 (L0+L1)
CREATE TABLE IF NOT EXISTS token_budgets (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    scope           TEXT NOT NULL,        -- 'global' | 'app' | 'provider' | 'model'
    scope_value     TEXT,                 -- scope='app' -> 'claude'/'codex'/...;
                                          -- scope='provider' -> provider_id;
                                          -- scope='model' -> model id;
                                          -- scope='global' -> NULL
    period          TEXT NOT NULL,        -- 'daily' | 'weekly' | 'monthly'
    period_start_day INTEGER NOT NULL DEFAULT 1,  -- monthly: 1~28；weekly: 0=Sun..6=Sat
    limit_tokens    BIGINT,               -- 与 limit_usd 二选一或都填，先到为准
    limit_usd       TEXT,
    enabled         BOOLEAN NOT NULL DEFAULT 1,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);
CREATE INDEX idx_token_budgets_scope ON token_budgets(scope, scope_value, enabled);
```

> MVP 不做 `budget_alerts` 历史表（那是 L2 告警去重用的）。L1 只读、只展示。

### 2.2 迁移

`src-tauri/src/database/migration.rs`：`SCHEMA_VERSION + 1`，旧库走 `ALTER` / 新建表的兼容路径（参考仓库现有 `migration.rs` 写法）。

---

## 3. 后端（Rust）

```
src-tauri/src/
├── database/dao/token_budget.rs      # DAO：CRUD，仿 dao/provider.rs
├── services/token_budget.rs          # 业务：周期窗口计算 + 当前消耗聚合
├── commands/token_budget.rs          # #[tauri::command]
└── 在 lib.rs 注册 6 个 command
```

### 3.1 `services/token_budget.rs` 关键函数

```rust
pub struct BudgetPeriod { start: i64, end: i64 }      // unix ms

pub fn compute_period(period_kind: &str, start_day: i64, now: i64) -> BudgetPeriod;
// daily   → 今天 00:00 ~ 明天 00:00（按 Local 时区，与 usage_stats.rs 保持一致）
// weekly  → 本周 start_day 00:00 ~ +7d
// monthly → 本月 start_day 00:00 ~ 下月同日 00:00

pub struct BudgetStatus {
    budget: TokenBudget,
    period: BudgetPeriod,
    consumed_tokens: u64,
    consumed_usd: Decimal,
    pct_tokens: Option<f64>,    // None 表示预算单位是 USD
    pct_usd:    Option<f64>,
    remaining_tokens: Option<u64>,
    remaining_usd:    Option<Decimal>,
}

pub fn get_budget_status(db: &Database, budget: &TokenBudget, now: i64)
    -> Result<BudgetStatus, AppError>;
// 内部对 proxy_request_logs + usage_daily_rollups 做 WHERE created_at BETWEEN 聚合
// 复用 usage_stats::derive_real_total_and_hit_rate 的语义（cache 归一化）
```

### 3.2 `commands/token_budget.rs`

```rust
#[tauri::command] fn list_token_budgets(state) -> Vec<TokenBudget>;
#[tauri::command] fn create_token_budget(state, input: CreateTokenBudgetInput) -> TokenBudget;
#[tauri::command] fn update_token_budget(state, id, patch) -> TokenBudget;
#[tauri::command] fn delete_token_budget(state, id) -> ();
#[tauri::command] fn get_token_budget_status(state, id) -> BudgetStatus;
#[tauri::command] fn get_all_budget_statuses(state) -> Vec<BudgetStatus>;  // dashboard 首屏一次拉
```

注册到 `lib.rs` 的 `invoke_handler!` 数组里（紧挨 usage 那些命令）。

### 3.3 为什么 MVP 不挂 `usage_events` 钩子

L1 只展示，不需要 push。前端走 react-query 轮询 + `usage-log-recorded` 触发 invalidate（仿 `useUsageEventBridge` 写 `useBudgetEventBridge`）就够，省得动 `usage_events.rs`。

---

## 4. 前端（React + TS）

```
src/
├── types/budget.ts                    # TokenBudget / BudgetStatus / CreateInput
├── lib/query/budget.ts                # budgetKeys + useBudgets / useBudgetStatus
├── hooks/useBudgetEventBridge.ts      # 监听 usage-log-recorded → invalidate
├── components/budget/
│   ├── BudgetDashboard.tsx            # 顶层；接入侧栏 / Usage Dashboard tab
│   ├── BudgetList.tsx                 # 列表 + 进度条
│   ├── BudgetCard.tsx                 # 单条：name / scope / period / 进度环 / "剩 X"
│   ├── BudgetEditor.tsx               # 新建/编辑（react-hook-form + zod，已有依赖）
│   └── BudgetDeleteDialog.tsx
└── i18n/locales/{zh-CN,en}/budget.json
```

### 4.1 入口位置

最小侵入：在 `UsageDashboard.tsx` 顶部加 `<Tabs>` 把现有内容装进 "Usage" tab，新增 "Budgets" tab 放 `BudgetDashboard`。或直接侧栏加一项。MVP 选 Tabs。

### 4.2 关键 UI 决策

- 同时显示 tokens 与 USD 双进度条（用 `real_total_tokens` + `total_cost_usd`）。
- 进度条颜色：`<70%` 绿、`70–95%` 黄、`>95%` 红（不弹通知，只是颜色）。
- 编辑器允许"仅 tokens / 仅 USD / 两者"三态。

---

## 5. 验证清单（MVP 完成判定）

- [ ] `cargo test --package cc-switch token_budget` 通过（含周期边界、跨时区）
- [ ] `pnpm test:unit` 通过（DAO + BudgetEditor 单测）
- [ ] `pnpm dev` 启动后：能新建 4 个不同 scope 的预算，刷新页面持久化
- [ ] 用 Claude Code 跑几条 prompt，BudgetCard 数字在 ~1s 内（react-query invalidate 后）变化
- [ ] 删除预算后无残留；禁用预算后状态查询返回 `enabled=false`
- [ ] 切换语言（zh/en）UI 文案完整

---

## 6. 路线图（MVP 之后）

| 阶段 | 内容 | 依赖 |
|---|---|---|
| **L2 告警** | `budget_alerts` 表 + `usage_events.rs` 钩子 + Tauri 系统通知 + 应用内 Toast | MVP |
| **L3 软熔断** | `proxy/forwarder.rs` 入口读 `BudgetBlock` flag；与 `commands/failover.rs` 联动切 provider | L2 |
| **L4 预测** | 基于 `usage_daily_rollups` N 天数据线性外推；UI "预计超支 X USD" | MVP |
| **L5 剩余可用估算** | `剩余 USD / 上 7 天平均日耗` → "还能跑约 N 天" | L4 |
| **L6 模板预设** | "学生党月 5 USD"、"团队月 200 USD" 一键模板 | MVP |
| **L7 Webhook / 邮件** | 跨设备告警 | L2 |

---

## 7. 与上游同步策略

- `upstream` → `farion1231/cc-switch`
- `origin` → `CacinieP/cc-switch`
- 主开发在 feature 分支：`feat/token-budget-mvp` → `feat/token-budget-l2` ……
- 每个 L 阶段一个独立分支、一组小 PR（便于 review 上游）。
- MVP 稳定 2 周后，将 L0+L1 整理成 PR 提 upstream，标题：`feat(usage): token budget planner (definition + progress)`。

---

## 8. 风险点

1. **本地时区**：`usage_stats.rs` 用 `chrono::Local`，月度预算的"月初"要跨 DST 处理。仓库已有 `fix/usage-stats-localtime-dst` 分支可参考。
2. **decimal 精度**：成本字段都是 `TEXT` 存 `rust_decimal::Decimal`。比较时务必走 Decimal，不要走 f64。
3. **Cache token 归一化**：聚合时复用 `derive_real_total_and_hit_rate`，不要自己另写一套，否则和 UsageHero 数字不一致用户会困惑。
4. **Provider id 漂移**：用户删除 provider 后，scope='provider' 的预算怎么办？MVP 选择"软失效"——状态返回 `pct=null`，UI 显示"未消费（provider 已删除）"。
