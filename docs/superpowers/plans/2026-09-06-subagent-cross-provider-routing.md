# Subagent 跨供应商路由 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Claude 应用的代理接管模式下，新增一条「subagent 路由」规则：匹配 subagent 的请求改道到指定的另一家供应商（B），其余请求仍走当前供应商（A），B 失败自动回退 A。

**Architecture:** 决策发生在代理请求上下文组装处——把 `select_providers` 返回的候选列表 `[A]` 改写为 `[B, A]`，重试/熔断/整流/模型映射机制全部复用。规则持久化在 `proxy_config` 表（per-app、设备级）。接管模式下自动向 live `settings.json` 注入 `CLAUDE_CODE_SUBAGENT_MODEL`，实现 Claude Code 端零手工配置。

**Tech Stack:** Rust（Tauri 2 后端、rusqlite、axum 反向代理）、React + TypeScript + TanStack Query（前端）、i18n（en / zh / zh-TW / ja）。

**Spec:** `docs/superpowers/specs/2026-09-06-subagent-cross-provider-routing-design.md`（实现争议以 spec 为准）

## Global Constraints

- 仅 `claude` 应用生效；`codex` / `gemini` / `grokbuild` / `claude-desktop` 的代理路径零行为变化。
- 识别在前、模型名覆盖在后（顺序不可反，spec §5.4）。
- 用户在供应商上显式设置的 `CLAUDE_CODE_SUBAGENT_MODEL` 永远优先于自动注入（spec §6）。
- 规则指向的供应商不存在时按无规则处理 + `log::warn!`，不 panic、不清除规则（spec §9）。
- serde 字段用 camelCase（仓库惯例）；Rust 注释用中文（仓库惯例）。
- 提交信息用 conventional commits（`feat(proxy): ...` / `feat(web): ...`）。
- 验证命令：Rust 在 `src-tauri/` 下 `cargo test`；前端 `pnpm typecheck`、`pnpm test:unit`、`pnpm format:check`。

## File Structure

| 文件 | 动作 | 职责 |
| --- | --- | --- |
| `src-tauri/src/proxy/types.rs` | 修改 | `SubagentRoute` 结构体 + `AppProxyConfig.subagent_route` 字段 |
| `src-tauri/src/database/mod.rs` | 修改 | `SCHEMA_VERSION` 18 → 19 |
| `src-tauri/src/database/schema.rs` | 修改 | CREATE TABLE 加列 + `migrate_v18_to_v19` |
| `src-tauri/src/database/dao/proxy.rs` | 修改 | `subagent_route` 列的读写与容错 |
| `src-tauri/src/proxy/subagent_route.rs` | 新建 | 路由决策纯函数（识别 + 候选改写 + 模型覆盖） |
| `src-tauri/src/proxy/mod.rs` | 修改 | 导出新模块 |
| `src-tauri/src/proxy/handler_context.rs` | 修改 | `RequestContext::new` 接线决策函数 |
| `src-tauri/src/proxy/handlers.rs` | 修改 | claude handler 应用 `body.model` 覆盖 |
| `src-tauri/src/services/proxy.rs` | 修改 | 接管 live env 注入 `CLAUDE_CODE_SUBAGENT_MODEL` |
| `src/types/proxy.ts` | 修改 | `AppProxyConfig` TS 接口加字段 |
| `src/components/proxy/SubagentRouteConfigPanel.tsx` | 新建 | 规则配置面板 |
| `src/components/proxy/ProxyPanel.tsx` | 修改 | 在 claude 区块挂载面板 |
| `src/i18n/locales/{en,zh,zh-TW,ja}.json` | 修改 | `proxy.subagentRoute.*` 文案 |

---

### Task 1: 数据模型 + 数据库迁移

**Files:**
- Modify: `src-tauri/src/proxy/types.rs`（`AppProxyConfig` 定义处，约 L162）
- Modify: `src-tauri/src/database/mod.rs:56`（`SCHEMA_VERSION`）
- Modify: `src-tauri/src/database/schema.rs`（CREATE TABLE 约 L126；迁移链 `apply_schema_migrations_on_conn` 约 L443；测试 mod 约 L3303）
- Test: 各文件内 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `proxy::types::SubagentRoute { provider_id: String, model: Option<String> }`（serde camelCase → `providerId` / `model`）；`AppProxyConfig.subagent_route: Option<SubagentRoute>`（serde `#[serde(default)]`）。后续所有任务依赖此类型。

- [ ] **Step 1: 写失败测试（serde 兼容性）**

在 `src-tauri/src/proxy/types.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn app_proxy_config_subagent_route_roundtrip() {
        let config = AppProxyConfig {
            app_type: "claude".to_string(),
            enabled: false,
            auto_failover_enabled: false,
            max_retries: 3,
            streaming_first_byte_timeout: 60,
            streaming_idle_timeout: 120,
            non_streaming_timeout: 600,
            circuit_failure_threshold: 4,
            circuit_success_threshold: 2,
            circuit_timeout_seconds: 60,
            circuit_error_rate_threshold: 0.6,
            circuit_min_requests: 10,
            subagent_route: Some(SubagentRoute {
                provider_id: "b".to_string(),
                model: Some("glm-5.5-flash".to_string()),
            }),
        };

        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(
            value["subagentRoute"],
            json!({"providerId": "b", "model": "glm-5.5-flash"})
        );
        let parsed: AppProxyConfig = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn app_proxy_config_without_subagent_route_is_backward_compatible() {
        // 旧前端/旧库数据没有 subagentRoute 字段，反序列化必须成功且为 None
        let legacy = json!({
            "appType": "claude", "enabled": false, "autoFailoverEnabled": false,
            "maxRetries": 3, "streamingFirstByteTimeout": 60, "streamingIdleTimeout": 120,
            "nonStreamingTimeout": 600, "circuitFailureThreshold": 4,
            "circuitSuccessThreshold": 2, "circuitTimeoutSeconds": 60,
            "circuitErrorRateThreshold": 0.6, "circuitMinRequests": 10
        });
        let parsed: AppProxyConfig = serde_json::from_value(legacy).unwrap();
        assert!(parsed.subagent_route.is_none());
    }
}
```

- [ ] **Step 2: 运行确认编译失败**

Run: `cd src-tauri && cargo test --lib proxy::types`
Expected: FAIL（`SubagentRoute` / `subagent_route` 未定义）

- [ ] **Step 3: 实现 — 类型与字段**

`proxy/types.rs`：在 `AppProxyConfig` 结构体定义**上方**加：

```rust
/// Subagent 跨供应商路由规则（仅 claude 应用生效）
///
/// 代理接管模式下，模型名匹配 subagent 角色的请求改道到目标供应商，
/// 其余请求仍走当前供应商。设计见
/// docs/superpowers/specs/2026-09-06-subagent-cross-provider-routing-design.md
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRoute {
    /// 目标供应商 id（claude 应用下的供应商）
    pub provider_id: String,
    /// 发给目标供应商的模型名；None = 透传请求中的模型名
    #[serde(default)]
    pub model: Option<String>,
}
```

在 `AppProxyConfig` 的最后一个字段 `circuit_min_requests` 之后加：

```rust
    /// Subagent 跨供应商路由规则；None = 关闭
    #[serde(default)]
    pub subagent_route: Option<SubagentRoute>,
```

- [ ] **Step 4: 迁移 — SCHEMA_VERSION 与 v19**

`database/mod.rs:56`：`pub(crate) const SCHEMA_VERSION: i32 = 18;` 改为 `19`。

`database/schema.rs`：

1. CREATE TABLE（约 L126）：在 `circuit_min_requests INTEGER NOT NULL DEFAULT 10,` 之后、`default_cost_multiplier` 之前插入一行：

```sql
            subagent_route TEXT,
```

2. 迁移链 `apply_schema_migrations_on_conn` 的 `match version` 中，在现有最大臂（`17 =>`）之后追加：

```rust
                    18 => {
                        log::info!("迁移数据库从 v18 到 v19（proxy_config 增加 subagent_route 列）");
                        Self::migrate_v18_to_v19(conn)?;
                        Self::set_user_version(conn, 19)?;
                    }
```

3. 在 `migrate_v17_to_v18` 函数之后新增（复用现有 `add_column_if_missing`，定义在 schema.rs 约 L3276）：

```rust
    fn migrate_v18_to_v19(conn: &Connection) -> Result<(), AppError> {
        // 缺表的库（异常/测试夹具）跳过：create_tables 会以含列的新 DDL 建表。
        if Self::table_exists(conn, "proxy_config")? {
            Self::add_column_if_missing(conn, "proxy_config", "subagent_route", "TEXT")?;
        }
        Ok(())
    }
```

4. 测试 mod（约 L3303 起，参照 `migrate_v17_to_v18_...` 测试）新增：

```rust
    #[test]
    fn migrate_v18_to_v19_adds_subagent_route_column() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE proxy_config (
                app_type TEXT PRIMARY KEY,
                enabled INTEGER NOT NULL DEFAULT 0
            );",
        )?;

        Self::migrate_v18_to_v19(&conn)?;

        assert!(Self::has_column(&conn, "proxy_config", "subagent_route")?);
        Ok(())
    }
```

（若测试 mod 内已有旧库建表/迁移测试工具函数，优先复用；保持与相邻迁移测试同构。）

- [ ] **Step 5: 运行全部相关测试**

Run: `cd src-tauri && cargo test --lib proxy::types && cargo test --lib database`
Expected: 全部 PASS（含既有迁移测试——SCHEMA_VERSION 升到 19 后旧库会自动走 v18→v19）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/proxy/types.rs src-tauri/src/database/mod.rs src-tauri/src/database/schema.rs
git commit -m "feat(proxy): add subagent_route to AppProxyConfig with v19 migration"
```

---

### Task 2: DAO 读写 subagent_route

**Files:**
- Modify: `src-tauri/src/database/dao/proxy.rs`（`get_proxy_config_for_app` 约 L215，`update_proxy_config_for_app` 约 L275）
- Test: `src-tauri/src/database/dao/proxy.rs` 文件内 tests mod

**Interfaces:**
- Consumes: `proxy::types::SubagentRoute`（Task 1）
- Produces: `Database::get_proxy_config_for_app` / `update_proxy_config_for_app` 完整读写 `subagent_route`；损坏 JSON 容错为 `None`。Task 4/5 依赖。

- [ ] **Step 1: 写失败测试**

在 `dao/proxy.rs` 的 tests mod 中追加（沿用文件内既有测试的 harness 风格；仓库已有 `Database::memory()`，参考 `proxy/provider_router.rs` 测试用法）：

```rust
    #[tokio::test]
    #[serial]
    async fn subagent_route_roundtrip_disable_and_corruption_fallback() {
        let db = Database::memory().unwrap();

        // 1. 写入 Some 并读回
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.subagent_route = Some(crate::proxy::types::SubagentRoute {
            provider_id: "b".to_string(),
            model: Some("glm-5.5-flash".to_string()),
        });
        db.update_proxy_config_for_app(config).await.unwrap();
        let read = db.get_proxy_config_for_app("claude").await.unwrap();
        assert_eq!(
            read.subagent_route,
            Some(crate::proxy::types::SubagentRoute {
                provider_id: "b".to_string(),
                model: Some("glm-5.5-flash".to_string()),
            })
        );

        // 2. 置回 None 并读回
        let mut config = read;
        config.subagent_route = None;
        db.update_proxy_config_for_app(config).await.unwrap();
        let read = db.get_proxy_config_for_app("claude").await.unwrap();
        assert!(read.subagent_route.is_none());

        // 3. 库中 JSON 损坏 → 容错为 None，不报错
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE proxy_config SET subagent_route = '{bad json' WHERE app_type = 'claude'",
                [],
            )
            .unwrap();
        }
        let broken = db.get_proxy_config_for_app("claude").await.unwrap();
        assert!(broken.subagent_route.is_none());
    }
```

（若该文件测试 mod 未引入 `#[serial]`，按文件内既有用法对齐；`lock_conn!` 宏在 `database/mod.rs`，同 crate 可用。）

- [ ] **Step 2: 运行确认失败**

Run: `cd src-tauri && cargo test --lib subagent_route_roundtrip`
Expected: FAIL（`subagent_route` 字段尚未参与读写，断言不相等 / 字段不存在导致编译失败）

- [ ] **Step 3: 实现 DAO**

`dao/proxy.rs` 顶部确认已引入 `crate::proxy::types::AppProxyConfig`（已有）。

`get_proxy_config_for_app` 的 SELECT 列表末尾追加 `subagent_route`，映射处追加索引 12：

```rust
                "SELECT app_type, enabled, auto_failover_enabled,
                        max_retries, streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                        circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                        circuit_error_rate_threshold, circuit_min_requests, subagent_route
                 FROM proxy_config WHERE app_type = ?1",
```

```rust
                        circuit_error_rate_threshold: row.get(10)?,
                        circuit_min_requests: row.get::<_, i32>(11)? as u32,
                        subagent_route: row
                            .get::<_, Option<String>>(12)?
                            .and_then(|s| {
                                serde_json::from_str::<Option<SubagentRoute>>(&s).ok()
                            })
                            .flatten(),
```

`QueryReturnedNoRows` 默认分支的字段初始化追加 `subagent_route: None,`。

`update_proxy_config_for_app`：在执行前序列化，UPDATE 追加参数：

```rust
        let subagent_route_json = match config.subagent_route.as_ref() {
            Some(route) => Some(
                serde_json::to_string(route).map_err(|e| AppError::Database(e.to_string()))?,
            ),
            None => None,
        };

        conn.execute(
            "UPDATE proxy_config SET
                enabled = ?2,
                ...（原参数不变）...
                circuit_min_requests = ?12,
                subagent_route = ?13,
                updated_at = datetime('now')
             WHERE app_type = ?1",
            rusqlite::params![
                // ...原参数不变...
                config.circuit_min_requests as i32,
                subagent_route_json,
            ],
        )
```

（`...（原参数不变）...` 处保留文件中原有的列与参数，仅新增 `subagent_route = ?13` 与末尾的 `subagent_route_json`——这是对既有 UPDATE 语句的增量修改，不是重写。）

- [ ] **Step 4: 运行测试**

Run: `cd src-tauri && cargo test --lib database::dao::proxy`
Expected: 全部 PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/database/dao/proxy.rs
git commit -m "feat(proxy): persist subagent_route in proxy_config DAO"
```

---

### Task 3: 路由决策纯函数（核心）

**Files:**
- Create: `src-tauri/src/proxy/subagent_route.rs`
- Modify: `src-tauri/src/proxy/mod.rs`（模块导出，`pub mod session;` 之后加 `pub mod subagent_route;`）
- Test: `src-tauri/src/proxy/subagent_route.rs` 文件内 tests mod

**Interfaces:**
- Consumes: `SubagentRoute`（Task 1）、`strip_one_m_suffix_for_upstream`（`model_mapper.rs` 已有 pub fn）、`Provider::with_id`
- Produces:
  - `pub struct SubagentRoutePlan { pub providers: Vec<Provider>, pub model_override: Option<String> }`
  - `pub fn effective_subagent_model(current: &Provider, route: Option<&SubagentRoute>) -> Option<String>`
  - `pub fn plan_subagent_route(providers: Vec<Provider>, app_type: &AppType, route: Option<&SubagentRoute>, request_model: &str, resolve_provider: impl Fn(&str) -> Option<Provider>) -> SubagentRoutePlan`

- [ ] **Step 1: 写失败测试**

创建 `src-tauri/src/proxy/subagent_route.rs`，先只写模块头与测试：

```rust
//! Subagent 跨供应商路由决策
//!
//! 接管模式下，模型名匹配 subagent 角色的请求改道到目标供应商（B），
//! 候选列表变为 [B, 其余]，失败转移、熔断等机制复用现有转发循环。
//! 决策为纯函数，便于单测。设计见
//! docs/superpowers/specs/2026-09-06-subagent-cross-provider-routing-design.md §5

use crate::app_config::AppType;
use crate::provider::Provider;
use crate::proxy::model_mapper::strip_one_m_suffix_for_upstream;
use crate::proxy::types::SubagentRoute;

/// 决策结果
pub struct SubagentRoutePlan {
    /// 改道后的候选列表（B 在首位，其余保持原顺序）
    pub providers: Vec<Provider>,
    /// 非空时 handler 在转发前把 body.model 覆盖为该值
    pub model_override: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider_with_env(id: &str, env: serde_json::Value) -> Provider {
        Provider::with_id(id.to_string(), id.to_string(), json!({"env": env}), None)
    }

    fn route(provider_id: &str, model: Option<&str>) -> SubagentRoute {
        SubagentRoute {
            provider_id: provider_id.to_string(),
            model: model.map(String::from),
        }
    }

    fn resolve_exists(id: &str) -> Option<Provider> {
        Some(provider_with_env(id, json!({})))
    }

    #[test]
    fn reroutes_matching_subagent_request_to_target() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}),
        );
        let providers = vec![a];
        let plan = plan_subagent_route(
            providers,
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(plan.providers.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(plan.model_override, Some("glm-5.5-flash".to_string()));
    }

    #[test]
    fn baseline_falls_back_to_route_model_without_explicit_env() {
        // 供应商未显式设置 subagent env，识别基准取规则 model（与接管注入同源）
        let a = provider_with_env("a", json!({}));
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "b");
        assert_eq!(plan.model_override, Some("glm-5.5-flash".to_string()));
    }

    #[test]
    fn explicit_env_wins_over_route_model_as_baseline() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "my-subagent-alias"}),
        );
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "my-subagent-alias",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "b");
        // 覆盖值仍取规则 model
        assert_eq!(plan.model_override, Some("glm-5.5-flash".to_string()));
    }

    #[test]
    fn non_matching_request_is_unchanged() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}),
        );
        let original = vec![a];
        let plan = plan_subagent_route(
            original.clone(),
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "claude-sonnet-4-5",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn one_m_suffix_is_normalized_for_matching() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}),
        );
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("b", None)),
            "glm-5.5-flash[1M]",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "b");
        // 规则无 model → 透传请求中的模型名
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn disabled_route_is_unchanged() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}),
        );
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            None,
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn non_claude_app_short_circuits() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}),
        );
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Codex,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn target_equals_current_is_noop() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}),
        );
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("a", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert_eq!(plan.providers.len(), 1);
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn missing_target_falls_back_to_current() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}),
        );
        let plan = plan_subagent_route(
            vec![a],
            &AppType::Claude,
            Some(&route("deleted", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            |_| None,
        );
        assert_eq!(plan.providers[0].id, "a");
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn target_in_failover_chain_is_deduplicated() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "glm-5.5-flash"}),
        );
        let b = provider_with_env("b", json!({}));
        let c = provider_with_env("c", json!({}));
        // 故障转移叠加：候选队列 [c, b, a]，规则目标 b 不在首位但已在链中，
        // 改道后应提到首位且去重（不重复入列）。
        let plan = plan_subagent_route(
            vec![c, b, a],
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        let ids = plan.providers.iter().map(|p| p.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, ["b", "c", "a"]);
        assert_eq!(ids.iter().filter(|id| **id == "b").count(), 1);
    }

    #[test]
    fn empty_providers_is_unchanged() {
        let plan = plan_subagent_route(
            Vec::new(),
            &AppType::Claude,
            Some(&route("b", Some("glm-5.5-flash"))),
            "glm-5.5-flash",
            resolve_exists,
        );
        assert!(plan.providers.is_empty());
        assert!(plan.model_override.is_none());
    }

    #[test]
    fn effective_subagent_model_prefers_explicit_env() {
        let a = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "  my-alias  "}),
        );
        assert_eq!(
            effective_subagent_model(&a, Some(&route("b", Some("other")))),
            Some("my-alias".to_string())
        );
        let blank = provider_with_env(
            "a",
            json!({"CLAUDE_CODE_SUBAGENT_MODEL": "   "}),
        );
        assert_eq!(
            effective_subagent_model(&blank, Some(&route("b", Some("glm-5.5-flash")))),
            Some("glm-5.5-flash".to_string())
        );
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd src-tauri && cargo test --lib subagent_route`
Expected: FAIL（`plan_subagent_route` / `effective_subagent_model` 未定义）

- [ ] **Step 3: 实现**

在同文件的 tests mod 之前补上实现：

```rust
/// 生效的 subagent 模型名（识别基准，与接管 live 注入同源，spec §5.1）：
/// 供应商显式设置的 CLAUDE_CODE_SUBAGENT_MODEL 优先，否则取规则中的 model。
pub fn effective_subagent_model(
    current: &Provider,
    route: Option<&SubagentRoute>,
) -> Option<String> {
    let env_value = current
        .settings_config
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_SUBAGENT_MODEL"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    env_value.or_else(|| {
        route
            .and_then(|r| r.model.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    })
}

/// 按 spec §5 计算 subagent 改道。
///
/// 前置条件任一不满足即原样返回：非 claude 应用、规则关闭、识别基准缺失、
/// 请求模型不匹配、目标等于当前、目标不存在。
pub fn plan_subagent_route(
    providers: Vec<Provider>,
    app_type: &AppType,
    route: Option<&SubagentRoute>,
    request_model: &str,
    resolve_provider: impl Fn(&str) -> Option<Provider>,
) -> SubagentRoutePlan {
    let unchanged = SubagentRoutePlan {
        providers,
        model_override: None,
    };
    if *app_type != AppType::Claude {
        return unchanged;
    }
    let Some(route) = route else {
        return unchanged;
    };
    let Some(current) = unchanged.providers.first() else {
        return unchanged;
    };
    let Some(baseline) = effective_subagent_model(current, Some(route)) else {
        return unchanged;
    };
    if strip_one_m_suffix_for_upstream(request_model)
        != strip_one_m_suffix_for_upstream(&baseline)
    {
        return unchanged;
    }
    let current_id = unchanged.providers[0].id.clone();
    if route.provider_id == current_id {
        return unchanged;
    }
    let Some(target) = resolve_provider(&route.provider_id) else {
        log::warn!(
            "[SubagentRoute] 规则指向的供应商 {} 不存在，忽略改道",
            route.provider_id
        );
        return unchanged;
    };

    let mut providers = Vec::with_capacity(unchanged.providers.len() + 1);
    providers.push(target);
    for provider in unchanged.providers {
        if providers.iter().all(|p| p.id != provider.id) {
            providers.push(provider);
        }
    }
    SubagentRoutePlan {
        model_override: route
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        providers,
    }
}
```

`proxy/mod.rs`：在 `pub mod session;` 之后按字母序插入 `pub mod subagent_route;`。

- [ ] **Step 4: 运行测试**

Run: `cd src-tauri && cargo test --lib proxy::subagent_route`
Expected: 全部 PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/proxy/subagent_route.rs src-tauri/src/proxy/mod.rs
git commit -m "feat(proxy): add subagent route decision function"
```

---

### Task 4: RequestContext 接线 + handler 模型覆盖

**Files:**
- Modify: `src-tauri/src/proxy/handler_context.rs`（`RequestContext::new` L134-176 区域；结构体字段定义 L44-77 区域）
- Modify: `src-tauri/src/proxy/handlers.rs`（`handle_messages_for_app`，约 L188 之后）

**Interfaces:**
- Consumes: `plan_subagent_route`（Task 3）、`AppProxyConfig.subagent_route`（Task 1）
- Produces: `RequestContext::take_model_override(&mut self) -> Option<String>`；claude 请求的候选列表已含改道结果

- [ ] **Step 1: 实现 RequestContext 接线**

`handler_context.rs` 顶部的 `use crate::proxy::{...}` 导入组内，在 `server::ProxyState,` 之后加一行 `subagent_route,`。

`RequestContext` 结构体加私有字段（放在 `providers` 字段之后）：

```rust
    /// Subagent 改道的模型名覆盖；由 handler 在转发前取走并写入 body.model
    ///
    /// 识别发生在 RequestContext::new（用原始请求模型名），覆盖发生在其后，
    /// 顺序不可反（spec §5.4）。
    model_override: Option<String>,
```

`RequestContext::new` 中，把现有的：

```rust
        let providers = state
            .provider_router
            .select_providers(app_type_str)
            .await
            .map_err(|e| match e {
                crate::error::AppError::AllProvidersCircuitOpen => {
                    ProxyError::AllProvidersCircuitOpen
                }
                crate::error::AppError::NoProvidersConfigured => ProxyError::NoProvidersConfigured,
                _ => ProxyError::DatabaseError(e.to_string()),
            })?;
```

之后插入（在 `let provider = providers.first()...` 之前）：

```rust
        // Subagent 跨供应商改道（仅 claude；spec §5）
        let plan = subagent_route::plan_subagent_route(
            providers,
            &app_type,
            app_config.subagent_route.as_ref(),
            &request_model,
            |id| state.db.get_provider_by_id(id, app_type_str).ok().flatten(),
        );
        let model_override = plan.model_override;
        let providers = plan.providers;
```

`Ok(Self { ... })` 初始化中追加 `model_override,`，并为结构体新增方法：

```rust
    /// 取走 Subagent 改道的模型名覆盖（若有）
    pub fn take_model_override(&mut self) -> Option<String> {
        self.model_override.take()
    }
```

- [ ] **Step 2: 实现 handler 覆盖**

`handlers.rs` 的 `handle_messages_for_app` 中，在 `let mut ctx = RequestContext::new(...)...?;` 之后、`let is_stream = ...` 之前插入：

```rust
    // Subagent 改道的模型名覆盖：识别已用原始模型名完成，此处改写转发体（spec §5.4）。
    // as_object_mut 守卫：非 object 的异常请求体直接忽略覆盖，避免索引 panic。
    if let Some(model) = ctx.take_model_override() {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("model".to_string(), serde_json::json!(model));
        }
    }
```

- [ ] **Step 3: 编译 + 全量 Rust 测试**

Run: `cd src-tauri && cargo test`
Expected: 编译通过、全部 PASS（行为由 Task 3 纯函数测试覆盖；本任务为薄接线，集成验收见 Task 7）

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/proxy/handler_context.rs src-tauri/src/proxy/handlers.rs
git commit -m "feat(proxy): reroute subagent requests to the routed provider in request context"
```

---

### Task 5: 接管 live env 自动注入

**Files:**
- Modify: `src-tauri/src/services/proxy.rs`（`apply_claude_takeover_fields_for_provider` L433；调用点 L702 / L2015 / L2073 / L2138）
- Test: `src-tauri/src/services/proxy.rs` 文件内 tests mod

**Interfaces:**
- Consumes: `AppProxyConfig.subagent_route`（Task 1/2）、`effective_subagent_model` 同源的判定（显式配置优先，spec §6）
- Produces: `ProxyService::apply_subagent_route_injection(fields, provider, injection)`、`ProxyService::claude_subagent_route_injection(db, provider).await -> Option<String>`；`apply_claude_takeover_fields_for_provider` 增加第四参数 `subagent_injection: Option<&str>`

- [ ] **Step 1: 写失败测试**

在 `services/proxy.rs` tests mod 追加：

```rust
    #[test]
    fn subagent_injection_appends_when_rule_active_and_env_unset() {
        let provider = Provider::with_id("a".to_string(), "A".to_string(), json!({}), None);
        let fields = vec![("ANTHROPIC_MODEL", "claude-sonnet-4-5".to_string())];
        let merged = ProxyService::apply_subagent_route_injection(
            fields,
            &provider,
            Some("glm-5.5-flash"),
        );
        assert_eq!(
            merged,
            vec![
                ("ANTHROPIC_MODEL", "claude-sonnet-4-5".to_string()),
                ("CLAUDE_CODE_SUBAGENT_MODEL", "glm-5.5-flash".to_string()),
            ]
        );
    }

    #[test]
    fn subagent_injection_skips_when_provider_sets_env_explicitly() {
        let provider = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({"env": {"CLAUDE_CODE_SUBAGENT_MODEL": "my-own"}}),
            None,
        );
        let fields: Vec<(&'static str, String)> = Vec::new();
        let merged = ProxyService::apply_subagent_route_injection(
            fields,
            &provider,
            Some("glm-5.5-flash"),
        );
        assert!(merged.is_empty());
    }

    #[test]
    fn subagent_injection_noop_without_rule_model() {
        let provider = Provider::with_id("a".to_string(), "A".to_string(), json!({}), None);
        let fields: Vec<(&'static str, String)> = Vec::new();
        let merged =
            ProxyService::apply_subagent_route_injection(fields, &provider, None);
        assert!(merged.is_empty());
    }
```

（tests mod 需可见 `Provider` 与 `json!` 宏：该文件的测试区若尚未引入，在 tests mod 内补 `use crate::provider::Provider;` 与 `use serde_json::json;`。测试函数是否需要 `#[serial]`，按文件内既有测试用法对齐。）

- [ ] **Step 2: 运行确认失败**

Run: `cd src-tauri && cargo test --lib services::proxy::tests::subagent_injection`
Expected: FAIL（`apply_subagent_route_injection` 未定义）

- [ ] **Step 3: 实现**

`services/proxy.rs`，`impl ProxyService` 内新增两个函数：

```rust
    /// 计算接管 live env 需要注入的 subagent 模型名（spec §6）：
    /// 规则启用且带模型名、且供应商未显式设置 CLAUDE_CODE_SUBAGENT_MODEL 时返回 Some。
    async fn claude_subagent_route_injection(
        db: &Database,
        provider: &Provider,
    ) -> Option<String> {
        let route = db
            .get_proxy_config_for_app(AppType::Claude.as_str())
            .await
            .ok()?
            .subagent_route?;
        let injected = route.model?.trim().to_string();
        if injected.is_empty() {
            return None;
        }
        let explicit = provider
            .settings_config
            .get("env")
            .and_then(|e| e.get("CLAUDE_CODE_SUBAGENT_MODEL"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if explicit.is_some() {
            return None;
        }
        Some(injected)
    }

    /// 把 subagent 注入合并进接管模型字段（用户显式配置优先，spec §6）
    fn apply_subagent_route_injection(
        mut fields: Vec<(&'static str, String)>,
        provider: &Provider,
        injection: Option<&str>,
    ) -> Vec<(&'static str, String)> {
        let Some(injected) = injection.map(str::trim).filter(|s| !s.is_empty()) else {
            return fields;
        };
        let explicit = provider
            .settings_config
            .get("env")
            .and_then(|e| e.get("CLAUDE_CODE_SUBAGENT_MODEL"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if explicit.is_some() {
            return fields;
        }
        fields.push(("CLAUDE_CODE_SUBAGENT_MODEL", injected.to_string()));
        fields
    }
```

`apply_claude_takeover_fields_for_provider` 增加第四参数并在计算 `takeover_model_fields` 之后合并：

```rust
    fn apply_claude_takeover_fields_for_provider(
        config: &mut Value,
        proxy_url: &str,
        provider: &Provider,
        subagent_injection: Option<&str>,
    ) {
        // ...原有 auth_policy / takeover_model_fields 计算不变...
        let takeover_model_fields = Self::apply_subagent_route_injection(
            takeover_model_fields,
            provider,
            subagent_injection,
        );

        Self::apply_claude_takeover_fields_with_policy_and_models(
            config,
            proxy_url,
            auth_policy,
            takeover_model_fields,
        );
    }
```

四个调用点（`sync_claude_live_from_provider_while_proxy_active` 约 L702、`takeover_live_configs` 约 L2015、`takeover_live_config_strict` 约 L2073、`takeover_live_config_best_effort` 约 L2138——均在 `async fn` 且 `&self` 可用）统一改为：

```rust
        let subagent_injection =
            Self::claude_subagent_route_injection(&self.db, &effective_provider /* 或对应 provider 变量 */).await;
        Self::apply_claude_takeover_fields_for_provider(
            &mut ...,
            &proxy_url,
            &...,
            subagent_injection.as_deref(),
        );
```

注意：传给注入查询的 provider 必须与传给 `apply_claude_takeover_fields_for_provider` 的是同一个变量（含 effective settings 的那个），否则显式配置判定可能取错源。文件内 `#[cfg(test)]` 的 `apply_claude_takeover_fields` 辅助（L425）调用 `apply_claude_takeover_fields_with_policy`，不经此函数，无需改动。

- [ ] **Step 4: 运行测试**

Run: `cd src-tauri && cargo test --lib services::proxy && cargo test --lib`
Expected: 全部 PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/proxy.rs
git commit -m "feat(proxy): inject CLAUDE_CODE_SUBAGENT_MODEL into takeover live env from the subagent route"
```

---

### Task 6: 前端配置面板 + i18n

**Files:**
- Modify: `src/types/proxy.ts`（`AppProxyConfig` 接口 L128）
- Create: `src/components/proxy/SubagentRouteConfigPanel.tsx`
- Modify: `src/components/proxy/ProxyPanel.tsx`（claude 区块，`appType="claude"` 的 `AutoFailoverConfigPanel` 约 L434 附近）
- Modify: `src/i18n/locales/{en,zh,zh-TW,ja}.json`（`proxy` 命名空间）
- Test: `tests/components/SubagentRouteConfigPanel.test.tsx`

**Interfaces:**
- Consumes: `useAppProxyConfig` / `useUpdateAppProxyConfig`（`src/lib/query/proxy.ts` 已有）、`useProvidersQuery(appId)`（`src/lib/query/queries.ts`，返回 `{ providers: Record<string, Provider>, currentProviderId: string }`）、后端 `subagentRoute: { providerId: string; model: string | null } | null`（camelCase 自动映射）
- Produces: `<SubagentRouteConfigPanel appType="claude" disabled={boolean} />`

- [ ] **Step 1: 扩展 TS 类型**

`src/types/proxy.ts`：

```ts
export interface SubagentRoute {
  providerId: string;
  model: string | null;
}
```

`AppProxyConfig` 接口末尾加 `subagentRoute: SubagentRoute | null;`

- [ ] **Step 2: 写失败组件测试**

创建 `tests/components/SubagentRouteConfigPanel.test.tsx`（mock hooks，参照 `tests/components/ModelDropdown.test.tsx` 的 mock 风格与 vitest 用法）：

```tsx
import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { SubagentRouteConfigPanel } from "@/components/proxy/SubagentRouteConfigPanel";

const mockConfig = {
  appType: "claude",
  enabled: true,
  autoFailoverEnabled: false,
  maxRetries: 3,
  streamingFirstByteTimeout: 60,
  streamingIdleTimeout: 120,
  nonStreamingTimeout: 600,
  circuitFailureThreshold: 4,
  circuitSuccessThreshold: 2,
  circuitTimeoutSeconds: 60,
  circuitErrorRateThreshold: 0.6,
  circuitMinRequests: 10,
  subagentRoute: null as null | { providerId: string; model: string | null },
};

const updateMock = vi.fn();

vi.mock("@/lib/query/proxy", () => ({
  useAppProxyConfig: () => ({ data: mockConfig, isLoading: false, error: null }),
  useUpdateAppProxyConfig: () => ({
    mutateAsync: updateMock.mockResolvedValue(undefined),
    isPending: false,
  }),
}));

vi.mock("@/lib/query/queries", () => ({
  useProvidersQuery: () => ({
    data: {
      providers: {
        a: { id: "a", name: "Provider A", settingsConfig: { env: {} } },
        b: { id: "b", name: "Provider B", settingsConfig: { env: {} } },
      },
      currentProviderId: "a",
    },
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("SubagentRouteConfigPanel", () => {
  beforeEach(() => {
    updateMock.mockClear();
    mockConfig.subagentRoute = null;
  });

  it("目标供应商下拉不包含当前供应商", async () => {
    render(<SubagentRouteConfigPanel appType="claude" disabled={false} />);
    fireEvent.click(screen.getByTestId("subagent-route-provider-trigger"));
    const options = await screen.findAllByRole("option");
    const values = options.map((o) => o.textContent ?? "");
    expect(values.join()).toContain("Provider B");
    expect(values.join()).not.toContain("Provider A");
  });

  it("保存时把规则写入 subagentRoute", async () => {
    render(<SubagentRouteConfigPanel appType="claude" disabled={false} />);
    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getByTestId("subagent-route-provider-trigger"));
    fireEvent.click((await screen.findAllByRole("option"))[0]);
    fireEvent.change(screen.getByTestId("subagent-route-model-input"), {
      target: { value: "glm-5.5-flash" },
    });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    const saved = updateMock.mock.calls[0][0];
    expect(saved.subagentRoute).toEqual({
      providerId: "b",
      model: "glm-5.5-flash",
    });
  });

  it("规则指向已删除供应商时显示失效警告", () => {
    mockConfig.subagentRoute = { providerId: "gone", model: "x" };
    render(<SubagentRouteConfigPanel appType="claude" disabled={false} />);
    expect(screen.getByText(/proxy\.subagentRoute\.targetMissingWarning/)).toBeDefined();
  });
});
```

（选择器 testid 需与实现一致；若仓库 shadcn `Select` 渲染结构不同导致 role 断言失败，按 `ModelDropdown.test.tsx` 中对 Select 的既有测试手法调整，但「排除当前供应商」「保存内容」「失效警告」三个断言语义必须保留。）

- [ ] **Step 3: 运行确认失败**

Run: `pnpm test:unit -- SubagentRouteConfigPanel`
Expected: FAIL（组件不存在）

- [ ] **Step 4: 实现面板组件**

创建 `src/components/proxy/SubagentRouteConfigPanel.tsx`：

```tsx
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Save, Loader2, AlertTriangle } from "lucide-react";
import { toast } from "sonner";
import { useAppProxyConfig, useUpdateAppProxyConfig } from "@/lib/query/proxy";
import { useProvidersQuery } from "@/lib/query/queries";

export interface SubagentRouteConfigPanelProps {
  appType: string;
  disabled?: boolean;
}

export function SubagentRouteConfigPanel({
  appType,
  disabled = false,
}: SubagentRouteConfigPanelProps) {
  const { t } = useTranslation();
  const { data: config, isLoading } = useAppProxyConfig(appType);
  const updateConfig = useUpdateAppProxyConfig();
  const { data: providersData } = useProvidersQuery(
    appType as Parameters<typeof useProvidersQuery>[0],
  );

  const [enabled, setEnabled] = useState(false);
  const [providerId, setProviderId] = useState("");
  const [model, setModel] = useState("");

  useEffect(() => {
    if (config) {
      setEnabled(!!config.subagentRoute);
      setProviderId(config.subagentRoute?.providerId ?? "");
      setModel(config.subagentRoute?.model ?? "");
    }
  }, [config]);

  const providers = useMemo(() => {
    const all = Object.values(providersData?.providers ?? {});
    return all
      .filter((p) => p.id !== providersData?.currentProviderId)
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [providersData]);

  const targetMissing =
    enabled && providerId !== "" && !providers.some((p) => p.id === providerId);

  const handleSave = async () => {
    if (!config) return;
    const trimmed = model.trim();
    await updateConfig.mutateAsync({
      ...config,
      subagentRoute:
        enabled && providerId
          ? { providerId, model: trimmed ? trimmed : null }
          : null,
    });
    toast.success(t("proxy.settings.toast.saved"), { closeButton: true });
  };

  if (isLoading || !config) return null;

  return (
    <div
      className={
        disabled ? "space-y-3 opacity-60 pointer-events-none" : "space-y-3"
      }
    >
      <div className="flex items-center justify-between">
        <Label htmlFor="subagent-route-enabled">
          {t("proxy.subagentRoute.title")}
        </Label>
        <Switch
          id="subagent-route-enabled"
          checked={enabled}
          onCheckedChange={setEnabled}
          disabled={disabled}
        />
      </div>

      {enabled && !disabled && (
        <p className="text-xs text-muted-foreground">
          {t("proxy.subagentRoute.description")}
        </p>
      )}

      {enabled && (
        <>
          <div className="space-y-1">
            <Label>{t("proxy.subagentRoute.targetProvider")}</Label>
            <Select
              value={providerId || undefined}
              onValueChange={setProviderId}
              disabled={disabled}
            >
              <SelectTrigger data-testid="subagent-route-provider-trigger">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {providers.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1">
            <Label htmlFor="subagent-route-model">
              {t("proxy.subagentRoute.model")}
            </Label>
            <Input
              id="subagent-route-model"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              disabled={disabled}
              data-testid="subagent-route-model-input"
              placeholder={t("proxy.subagentRoute.modelPlaceholder")}
            />
          </div>

          {targetMissing && (
            <Alert>
              <AlertTriangle className="h-4 w-4" />
              <AlertDescription>
                {t("proxy.subagentRoute.targetMissingWarning")}
              </AlertDescription>
            </Alert>
          )}
          {!targetMissing && enabled && providerId && !model.trim() && (
            <Alert>
              <AlertTriangle className="h-4 w-4" />
              <AlertDescription>
                {t("proxy.subagentRoute.modelRequiredHint")}
              </AlertDescription>
            </Alert>
          )}
        </>
      )}

      <Button
        onClick={handleSave}
        disabled={disabled || updateConfig.isPending}
      >
        {updateConfig.isPending ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Save className="h-4 w-4" />
        )}
        {t("proxy.settings.save")}
      </Button>
    </div>
  );
}
```

（`proxy.settings.save` 若该 key 不存在，沿用 `AutoFailoverConfigPanel` 保存按钮所用的同一 key。`useProvidersQuery` 的参数类型为 `AppId`，调用处写 `useProvidersQuery(appType as Parameters<typeof useProvidersQuery>[0])` 以通过 typecheck。）

- [ ] **Step 5: 挂载到 ProxyPanel claude 区块**

`src/components/proxy/ProxyPanel.tsx`：claude 区块内 `appType="claude"` 的 `<AutoFailoverConfigPanel ...>`（约 L434）之后紧挨着加：

```tsx
<SubagentRouteConfigPanel appType="claude" disabled={!isEnabled} />
```

（`isEnabled` 为该区块已有的接管开关状态变量，即约 L284 的 `takeoverStatus?.[appType]`；若该变量不在作用域内，用同样的表达式在 claude 区块内取值。）文件顶部补组件导入。

- [ ] **Step 6: i18n 四语言**

`src/i18n/locales/*.json` 的 `proxy` 命名空间内新增 `subagentRoute` 键（与 `failoverQueue` 同级）。

en.json：

```json
"subagentRoute": {
  "title": "Subagent routing",
  "description": "Forward subagent requests to another provider while the main conversation keeps using the current one. Requires proxy takeover. Model name is required unless the current provider sets CLAUDE_CODE_SUBAGENT_MODEL explicitly.",
  "targetProvider": "Target provider",
  "model": "Model",
  "modelPlaceholder": "Model name sent to the target provider (e.g. glm-5.5-flash)",
  "targetMissingWarning": "The routed provider no longer exists. Pick another one or disable the switch.",
  "modelRequiredHint": "Fill in a model name so the proxy can tell subagent requests apart."
}
```

zh.json：

```json
"subagentRoute": {
  "title": "Subagent 路由",
  "description": "主对话继续使用当前供应商，subagent 请求转发到另一家供应商。需开启代理接管。除非当前供应商已显式设置 CLAUDE_CODE_SUBAGENT_MODEL，否则必须填写模型名。",
  "targetProvider": "目标供应商",
  "model": "模型",
  "modelPlaceholder": "发送给目标供应商的模型名（如 glm-5.5-flash）",
  "targetMissingWarning": "路由目标供应商已不存在，请重新选择或关闭开关。",
  "modelRequiredHint": "请填写模型名，代理据此区分 subagent 请求。"
}
```

zh-TW.json：

```json
"subagentRoute": {
  "title": "Subagent 路由",
  "description": "主對話繼續使用目前供應商，subagent 請求轉送到另一家供應商。需開啟代理接管。除非目前供應商已明確設定 CLAUDE_CODE_SUBAGENT_MODEL，否則必須填寫模型名稱。",
  "targetProvider": "目標供應商",
  "model": "模型",
  "modelPlaceholder": "傳送給目標供應商的模型名稱（如 glm-5.5-flash）",
  "targetMissingWarning": "路由目標供應商已不存在，請重新選擇或關閉開關。",
  "modelRequiredHint": "請填寫模型名稱，代理據此區分 subagent 請求。"
}
```

ja.json：

```json
"subagentRoute": {
  "title": "サブエージェントルーティング",
  "description": "メイン会話は現在のプロバイダーを使い続け、サブエージェントのリクエストを別のプロバイダーへ転送します。プロキシテイクオーバーが必要です。現在のプロバイダーが CLAUDE_CODE_SUBAGENT_MODEL を明示的に設定していない場合は、モデル名の入力が必須です。",
  "targetProvider": "転送先プロバイダー",
  "model": "モデル",
  "modelPlaceholder": "転送先に送るモデル名（例: glm-5.5-flash）",
  "targetMissingWarning": "ルーティング先のプロバイダーが存在しません。選び直すかスイッチをオフにしてください。",
  "modelRequiredHint": "プロキシがサブエージェントリクエストを判別できるようモデル名を入力してください。"
}
```

- [ ] **Step 7: 运行前端验证**

Run: `pnpm typecheck && pnpm test:unit -- SubagentRouteConfigPanel && pnpm format:check`
Expected: 全部通过

- [ ] **Step 8: Commit**

```bash
git add src/types/proxy.ts src/components/proxy/SubagentRouteConfigPanel.tsx src/components/proxy/ProxyPanel.tsx src/i18n/locales tests/components/SubagentRouteConfigPanel.test.tsx
git commit -m "feat(web): add subagent routing panel for cross-provider Claude subagents"
```

---

### Task 7: 全量验证 + 验收 + PR 材料

**Files:**
- Modify: `CHANGELOG.md`（如仓库惯例要求每个 PR 附条目；对照相邻 PR 决定）
- Create: PR 描述（提交时粘贴 GitHub，不落盘）

- [ ] **Step 1: Rust 全量**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: 零 error / 全部 PASS

- [ ] **Step 2: 前端全量**

Run: `pnpm typecheck && pnpm test:unit && pnpm format:check`
Expected: 全部通过

- [ ] **Step 3: 手工验收（按 spec §10）**

前置：配置供应商 A（旗舰，如 Anthropic 官方或高质量中转）与 B（平价，如 GLM flash 档），两个都在 claude 应用下。

1. 开启代理接管 + 面板里启用 Subagent 路由选 B 填模型名，保存。
2. Claude Code 同时跑主对话与 Task 子 agent：代理日志（`~/.cc-switch` 日志）显示主对话请求发往 A、subagent 请求发往 B；用量统计按供应商分别归因。
3. 使 B 失效（改错 key）：subagent 请求自动回退 A 不中断；恢复 B 后自动回到 B。
4. 切换主供应商 A → 另一家：subagent 路由仍指向 B。
5. 关闭接管：回落普通单供应商行为，无残留。
6. 在 A 上显式设置 `CLAUDE_CODE_SUBAGENT_MODEL` 后重开接管：live env 使用用户显式值而非注入值。

- [ ] **Step 4: PR 描述**

PR 标题：`feat(proxy): cross-provider subagent routing in takeover mode`

描述结构：动机（A 家旗舰做计划审核、B 家平价模型干活的省钱组合；现状只支持同供应商模型映射）→ 方案（接管模式候选列表改道 `[B, A]`，复用故障转移/熔断/模型映射；`proxy_config` 持久化；接管 live env 自动注入）→ 范围（仅 claude；关闭接管或规则不满足时零行为变化）→ 测试（单测清单 + 手工验收）→ spec 链接。

- [ ] **Step 5: Commit（如有 changelog）**

```bash
git add CHANGELOG.md
git commit -m "docs(changelog): mention cross-provider subagent routing"
```

---

## Self-Review 记录

- Spec 覆盖：§4 数据模型→Task 1/2；§5 决策→Task 3/4；§6 注入→Task 5；§7 熔断→复用现有机制（Task 4 候选列表 ≥2 自动生效，无代码）；§8 UI/i18n→Task 6（i18n 实际为 en / zh / zh-TW / ja，spec 中「de」为笔误，已在 spec 中修正）；§9 边界→Task 3 测试（missing target / target==current / 非 claude / 去重）+ Task 2（损坏 JSON）；§10 测试→各任务 + Task 7。
- 类型一致性：`SubagentRoutePlan { providers, model_override }`、`take_model_override()`、`apply_claude_takeover_fields_for_provider` 四参数签名、TS `subagentRoute` 各任务间已核对一致。
- 占位符修复记录：初稿 Task 6 的 JSX 为骨架（含 `opacity-???` 占位），已替换为完整组件代码；Task 3 去重测试初稿场景误入 target==current noop 分支，已改为候选 `[c, b, a]`、目标 `b` 的真实去重场景；Task 4 的 `body["model"]` 索引赋值有非 object panic 边界，已改为 `as_object_mut` 守卫。
- 遗留小项：Task 6 组件测试中 Radix `Select` 在 jsdom 下的交互方式以仓库既有 Select 测试手法为准（`tests/components/ModelDropdown.test.tsx`），三个断言语义（排除当前供应商 / 保存内容 / 失效警告）不可减。
