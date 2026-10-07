# 会话粘性负载均衡实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 同一应用的多个供应商账号从"主备"升级为"会话粘性轮询负载均衡"——N 账号 ≈ N 倍并发额度加和，同会话固定同账号（保 prompt cache 与 Codex previous_response_id 连续性）。

**Architecture:** 新增 `proxy/load_balance.rs` 内存态核心（粘性表 + 在途计数 + 限流冷却），挂在 `ProviderRouter` 上；选号入口换为 `select_providers_for_session`（off 模式行为与现状逐字节一致）；forwarder 两处小挂点（failover 成功重映射 + 429/403 冷却记录）；在途计数经 `ActiveConnectionGuard` 顺流到响应结束才递减，handlers/response_processor 零改动。

**Tech Stack:** Rust (axum/tokio/rusqlite) + React 18/TS + i18next 四语言。

**设计文档:** `docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md`（下称"设计"）

## Global Constraints

- 门禁：`cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test`（`src-tauri/` 下）+ `pnpm typecheck` + `pnpm test:unit` 全绿
- i18n：任何用户可见字符串经 `t()`，en/zh/zh-TW/ja 四语言同步（`src/i18n/locales/*.json`）
- Tauri 命令名 camelCase；`AppProxyConfig` serde camelCase（`types.rs:160-161`）
- 工作区有无关未提交改动 `src-tauri/src/proxy/providers/transform_codex_chat.rs`——**所有 commit 用显式文件路径 git add，禁止 `git add -A`**
- Conventional Commits；提交信息结尾 `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`
- 设计决策锁定值：粘性表空闲 TTL 30 分钟、LRU 上限 4096、冷却退避 60s→2m→5m 封顶 10m、缺省冷却 60s、粘性命中不查并发上限、上限 0 = 不限

## 与设计的偏差记录（锚点校验结论，已确认事实）

1. **proxy_config 是列式表非 key-value**（设计 4.3 错）：加 `load_balance_mode TEXT NOT NULL DEFAULT 'off'` 列，走 `add_column_if_missing` 迁移（`schema.rs:415` 已有模式）。
2. **会话提取已在选号之前**（设计 4.1 的"挪动"免做）：`handler_context.rs:121` 在 :134 之前。
3. **previous_response_id 已被 fork 弃用为会话身份**（`session.rs:172-175`）：无头 Codex 会话 = Generated = 不写粘性表，与设计决策 3 一致，免做。
4. **并发上限存 `provider.meta` 而非 `settings_config`**（设计 4.3）：`costMultiplier` 先例（`provider.rs:479-484`，前端表单 ↔ meta ↔ 后端读取已跑通），且 meta 承诺"不写入 live 配置"，不会泄漏进 CLI 工具配置文件。
5. **Generated/Extracted 的落地形式**：无 `SessionId` 枚举，用 `SessionIdResult.client_provided`（false ⇔ Generated）。
6. 阶段 1 中新会话/Generated 会话的选号 = 纯轮询游标（无饱和检查——计数是阶段 2 的事）；阶段 2 把 Generated 改为"在途最少"（设计 4.1.d）。

## File Structure

```
src-tauri/src/
├── proxy/
│   ├── load_balance.rs            [新] 粘性表/在途计数/冷却/InFlightHandle + 全量单测
│   ├── tests_load_balance.rs      [新] #[cfg(test)] 双 mock 上游集成测试（mod.rs 引入）
│   ├── provider_router.rs         [改] select_providers_for_session + LB 挂载 + 单测
│   ├── handler_context.rs         [改] 选号调用换新方法（1 处）
│   ├── forwarder.rs               [改] 重映射（wrapper 1 处）/ 在途计数（迭代 1 处 + 4 个 Ok 返回点）/ 冷却（Retryable 分支 1 处）/ retry-after 解析
│   ├── server.rs                  [改] 无（LB 经 ProviderRouter 传递，ProxyState 不加字段）
│   ├── mod.rs                     [改] pub mod load_balance + tests_load_balance
│   ├── error.rs                   [改] UpstreamError + retry_after_secs
│   └── types.rs                   [改] AppProxyConfig.load_balance_mode + 校验函数
├── database/
│   ├── schema.rs                  [改] 建表列 + add_column_if_missing 迁移
│   └── dao/proxy.rs               [改] SELECT/UPDATE 增列
├── commands/proxy.rs              [改] update 校验 load_balance_mode
└── provider.rs                    [改] ProviderMeta.loadBalance + max_concurrent_for
src/
├── types/proxy.ts                 [改] AppProxyConfig.loadBalanceMode
├── types/provider.ts              [改] ProviderMeta.loadBalance（找到实际定义文件后落位）
├── config/codingPlanPresets.ts    [新] 套餐并发预设（Kimi/GLM/MiMo）+ 单测
├── components/proxy/AutoFailoverConfigPanel.tsx [改] 负载均衡下拉
└── components/providers/forms/{ProviderAdvancedConfig.tsx,ProviderForm.tsx} [改] 并发上限区 + 套餐下拉
src/i18n/locales/{en,zh,zh-TW,ja}.json [改] proxy.loadBalance.* / providerAdvanced.loadBalance.* / codingPlanPresets.*
```

---

## 阶段 1（设计 7.1：粘性表 + 路由接入 + 重映射）

### Task 1: 后端 `load_balance_mode` 配置全链路

**Files:**
- Modify: `src-tauri/src/proxy/types.rs:162-187`（结构体）
- Modify: `src-tauri/src/database/schema.rs:126-131`（建表）、`schema.rs` 迁移区（`add_column_if_missing` 调用串）
- Modify: `src-tauri/src/database/dao/proxy.rs:223-247`（SELECT）、`:255-270`（无行默认）、`:275-314`（UPDATE）
- Modify: `src-tauri/src/commands/proxy.rs:143-160`（校验）

**Interfaces:**
- Produces: `AppProxyConfig.load_balance_mode: String`（serde camelCase `loadBalanceMode`，默认 `"off"`）；`pub fn is_valid_load_balance_mode(v: &str) -> bool`（后续任务/前端契约：合法值仅 `"off" | "session"`）

- [ ] **Step 1: 写失败测试** — `src-tauri/src/proxy/types.rs` 文件尾部（或已有 `#[cfg(test)] mod` 内）加：

```rust
#[cfg(test)]
mod load_balance_mode_tests {
    use super::*;

    #[test]
    fn default_load_balance_mode_is_off() {
        assert_eq!(default_load_balance_mode(), "off");
    }

    #[test]
    fn serde_roundtrip_camel_case() {
        let json = r#"{"appType":"claude","enabled":false,"autoFailoverEnabled":false,
            "maxRetries":3,"streamingFirstByteTimeout":60,"streamingIdleTimeout":120,
            "nonStreamingTimeout":600,"circuitFailureThreshold":4,"circuitSuccessThreshold":2,
            "circuitTimeoutSeconds":60,"circuitErrorRateThreshold":0.6,"circuitMinRequests":10,
            "loadBalanceMode":"session"}"#;
        let cfg: AppProxyConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.load_balance_mode, "session");
        // 旧载荷（无该字段）解析回退为 off
        let legacy = json.replace(",\"loadBalanceMode\":\"session\"", "");
        let cfg: AppProxyConfig = serde_json::from_str(&legacy).unwrap();
        assert_eq!(cfg.load_balance_mode, "off");
    }

    #[test]
    fn validates_mode_values() {
        assert!(is_valid_load_balance_mode("off"));
        assert!(is_valid_load_balance_mode("session"));
        assert!(!is_valid_load_balance_mode("round_robin"));
        assert!(!is_valid_load_balance_mode(""));
    }
}
```

```rust
// dao/proxy.rs 已有 #[cfg(test)] 模块的话并入，否则新增：
#[cfg(test)]
mod load_balance_mode_db_tests {
    use super::*;

    #[tokio::test]
    async fn load_balance_mode_roundtrip_and_default() {
        let db = Database::memory().unwrap();
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        assert_eq!(config.load_balance_mode, "off"); // 新库默认 off
        config.load_balance_mode = "session".to_string();
        db.update_proxy_config_for_app(config).await.unwrap();
        assert_eq!(
            db.get_proxy_config_for_app("claude").await.unwrap().load_balance_mode,
            "session"
        );
    }
}
```

- [ ] **Step 2: 跑测试确认失败** — `cd src-tauri && cargo test load_balance_mode` → 编译错误（字段/函数不存在）

- [ ] **Step 3: 实现**

`types.rs` `AppProxyConfig` 末尾（`circuit_min_requests` 之后）加字段与函数（同文件顶层）：

```rust
/// 负载均衡模式："off"（主备/顺序故障转移，默认）| "session"（会话粘性轮询）
#[serde(default = "default_load_balance_mode")]
pub load_balance_mode: String,
```

```rust
fn default_load_balance_mode() -> String {
    "off".to_string()
}

/// load_balance_mode 合法值校验（commands 层入库前调用）
pub fn is_valid_load_balance_mode(value: &str) -> bool {
    matches!(value, "off" | "session")
}
```

`schema.rs` 建表语句 `circuit_min_requests INTEGER NOT NULL DEFAULT 10,` 后加：

```sql
load_balance_mode TEXT NOT NULL DEFAULT 'off',
```

`schema.rs` `apply_schema_migrations_on_conn` 的 `add_column_if_missing` 调用串（:415 附近，超时列那组之后）加：

```rust
Self::add_column_if_missing(
    conn,
    "proxy_config",
    "load_balance_mode",
    "TEXT NOT NULL DEFAULT 'off'",
)?;
```

`dao/proxy.rs` `get_proxy_config_for_app`：
- SELECT 列清单 `circuit_min_requests` 后加 `, load_balance_mode`
- 闭包加 `load_balance_mode: row.get(12)?,`
- `QueryReturnedNoRows` 默认分支加 `load_balance_mode: "off".to_string(),`

`update_proxy_config_for_app`：SET 串加 `load_balance_mode = ?13,`（放 `circuit_min_requests = ?12,` 之后、`updated_at` 之前），params 末尾加 `config.load_balance_mode,`

`commands/proxy.rs` `update_proxy_config_for_app` 在 `let circuit_config = ...`（:150）前加：

```rust
if !crate::proxy::types::is_valid_load_balance_mode(&config.load_balance_mode) {
    return Err(format!(
        "无效的负载均衡模式: {}（合法值: off | session）",
        config.load_balance_mode
    ));
}
```

- [ ] **Step 4: 跑测试确认通过** — `cargo test load_balance_mode` 全 PASS
- [ ] **Step 5: 快速门禁** — `cargo fmt` && `cargo clippy -- -D warnings`（此时全库仍应绿）
- [ ] **Step 6: Commit**（阶段 1 一次性提交，见 Task 6；本任务不单独 commit）

### Task 2: 前端配置类型 + 负载均衡下拉 + i18n

**Files:**
- Modify: `src/types/proxy.ts:128-141`
- Modify: `src/components/proxy/AutoFailoverConfigPanel.tsx`（formData/Save/渲染三处）
- Modify: `src/i18n/locales/{en,zh,zh-TW,ja}.json`（`proxy` 对象内）

**Interfaces:**
- Consumes: Task 1 的 `loadBalanceMode`（camelCase IPC 字段）
- Produces: `AppProxyConfig.loadBalanceMode: "off" | "session"`（TS 契约）

- [ ] **Step 1: TS 类型** — `src/types/proxy.ts` `AppProxyConfig` 加：

```ts
/** 负载均衡模式："off" 主备 | "session" 会话粘性轮询 */
loadBalanceMode: "off" | "session";
```

- [ ] **Step 2: i18n 四语言** — 各 locale `proxy` 对象内加（zh 示例，其余语言按下表翻译）：

```json
"loadBalance": {
  "label": "负载均衡",
  "off": "关闭（主备）",
  "session": "会话粘性轮询",
  "hint": "同一会话固定走同一账号，新会话在故障转移队列的账号间轮询分摊并发。需开启自动故障转移且队列中有多个账号。",
  "modeLabel": "负载均衡模式"
}
```

| key | en | zh-TW | ja |
|---|---|---|---|
| label | Load Balancing | 負載均衡 | 負荷分散 |
| off | Off (primary/backup) | 關閉（主備） | オフ（プライマリ/バックアップ） |
| session | Session-sticky round robin | 會話粘性輪詢 | セッション粘着ラウンドロビン |
| hint | Same session stays on one account; new sessions are spread across the failover queue. Requires auto failover with multiple accounts in the queue. | 同一會話固定走同一帳號，新會話在故障轉移佇列的帳號間輪詢分攤並發。需開啟自動故障轉移且佇列中有多個帳號。 | 同一セッションは同じアカウントに固定し、新規セッションをフェイルオーバーキューのアカウントへラウンドロビン分散。自動フェイルオーバーとキュー内の複数アカウントが必要です。 |
| modeLabel | Load balancing mode | 負載均衡模式 | 負荷分散モード |

- [ ] **Step 3: 面板接线** — `AutoFailoverConfigPanel.tsx`：
  1. formData 初始态（:27 附近）加 `loadBalanceMode: "off" as "off" | "session",`
  2. 两个 `useEffect` 回填处（:41、:190 区域）加 `loadBalanceMode: config.loadBalanceMode ?? "off",`
  3. `handleSave` 的 payload（:165 区域）加 `loadBalanceMode: formData.loadBalanceMode,`
  4. 渲染：在"最大重试次数"输入框 JSX（:243-260）之后加：

```tsx
<div className="space-y-2">
  <Label htmlFor={`loadBalanceMode-${appType}`}>
    {t("proxy.loadBalance.modeLabel", "负载均衡模式")}
  </Label>
  <Select
    value={formData.loadBalanceMode}
    onValueChange={(v) =>
      setFormData({ ...formData, loadBalanceMode: v as "off" | "session" })
    }
    disabled={disabled}
  >
    <SelectTrigger id={`loadBalanceMode-${appType}`}>
      <SelectValue />
    </SelectTrigger>
    <SelectContent>
      <SelectItem value="off">
        {t("proxy.loadBalance.off", "关闭（主备）")}
      </SelectItem>
      <SelectItem value="session">
        {t("proxy.loadBalance.session", "会话粘性轮询")}
      </SelectItem>
    </SelectContent>
  </Select>
  <p className="text-xs text-muted-foreground">
    {t("proxy.loadBalance.hint")}
  </p>
</div>
```

顶部 import 补 `Select, SelectContent, SelectItem, SelectTrigger, SelectValue`（`@/components/ui/select`）。

- [ ] **Step 4: 门禁** — `pnpm typecheck` && `pnpm test:unit` 全绿
- [ ] **Step 5: 不单独 commit**（并入阶段 1 提交）

### Task 3: `proxy/load_balance.rs` 粘性表核心（阶段 1 范围）

**Files:**
- Create: `src-tauri/src/proxy/load_balance.rs`
- Modify: `src-tauri/src/proxy/mod.rs`（`pub mod load_balance;` + `pub use load_balance::SessionLoadBalancer;`）

**Interfaces:**
- Produces（后续任务依赖的确切签名）:
  - `pub struct SessionLoadBalancer`；`SessionLoadBalancer::new() -> Self`
  - `pub(crate) fn with_limits(ttl: Duration, max_entries: usize) -> Self`（测试用）
  - `pub fn lookup_sticky(&self, app_type: &str, session_id: &str) -> Option<String>`（命中即续期）
  - `pub fn insert_sticky(&self, app_type: &str, session_id: &str, provider_id: &str)`（先到者优先，过期覆盖）
  - `pub fn remap_sticky(&self, app_type: &str, session_id: &str, provider_id: &str)`（failover 后无条件改写）
  - `pub fn next_cursor_index(&self, app_type: &str, len: usize) -> usize`（游标推进）
  - `pub(crate) fn in_flight_len(&self, app_type: &str) -> usize`（测试观察在途表规模）
  - 常量 `pub(crate) const STICKY_IDLE_TTL_SECS: u64 = 1800;`、`pub(crate) const STICKY_MAX_ENTRIES: usize = 4096;`

- [ ] **Step 1: 写失败测试** — 新文件先写测试骨架（TDD），实现为空/未定义：

```rust
//! 会话粘性负载均衡核心状态
//!
//! 与熔断器的分工：熔断（circuit_breaker.rs）管错误率/不可用；本模块管
//! "哪个账号服务哪个会话"与限流冷却。全部内存态，重启清空（设计决策：
//! 每活跃会话首个请求冷一次，可接受，不做持久化）。

use std::time::Duration;

#[cfg(test)]
mod sticky_tests {
    use super::*;

    fn lb_small(ttl_secs: u64, cap: usize) -> SessionLoadBalancer {
        SessionLoadBalancer::with_limits(Duration::from_secs(ttl_secs), cap)
    }

    #[test]
    fn insert_then_lookup_hits_and_renews() {
        let lb = lb_small(1800, 4096);
        lb.insert_sticky("claude", "s1", "a");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("a".to_string()));
        // 不同应用互不串表
        assert_eq!(lb.lookup_sticky("codex", "s1"), None);
    }

    #[test]
    fn first_writer_wins_on_concurrent_insert() {
        // 设计 4.1.c：entry().or_insert()，后到者采纳先到者
        let lb = lb_small(1800, 4096);
        lb.insert_sticky("claude", "s1", "a");
        lb.insert_sticky("claude", "s1", "b");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("a".to_string()));
    }

    #[test]
    fn remap_overwrites_unconditionally() {
        let lb = lb_small(1800, 4096);
        lb.insert_sticky("claude", "s1", "a");
        lb.remap_sticky("claude", "s1", "b");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("b".to_string()));
        // 无既有条目时也建立映射（failover 后粘性自愈）
        lb.remap_sticky("claude", "s2", "b");
        assert_eq!(lb.lookup_sticky("claude", "s2"), Some("b".to_string()));
    }

    #[test]
    fn expired_entry_is_evicted_on_lookup() {
        let lb = lb_small(0, 4096); // TTL=0：立即过期
        lb.insert_sticky("claude", "s1", "a");
        assert_eq!(lb.lookup_sticky("claude", "s1"), None);
        // 过期后 insert 可写入新值（先到者已失效）
        lb.insert_sticky("claude", "s1", "b");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("b".to_string()));
    }

    #[test]
    fn capacity_cap_evicts_idlest_entry() {
        let lb = lb_small(1800, 2);
        lb.insert_sticky("claude", "s1", "a");
        lb.insert_sticky("claude", "s2", "a");
        lb.lookup_sticky("claude", "s1"); // s1 续期，s2 更旧
        lb.insert_sticky("claude", "s3", "b"); // 触发惰性驱逐
        assert_eq!(lb.lookup_sticky("claude", "s2"), None); // 最旧的 s2 被驱逐
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("a".to_string()));
        assert_eq!(lb.lookup_sticky("claude", "s3"), Some("b".to_string()));
    }

    #[test]
    fn cursor_round_robins_and_wraps() {
        let lb = lb_small(1800, 4096);
        assert_eq!(lb.next_cursor_index("claude", 3), 0);
        assert_eq!(lb.next_cursor_index("claude", 3), 1);
        assert_eq!(lb.next_cursor_index("claude", 3), 2);
        assert_eq!(lb.next_cursor_index("claude", 3), 0);
        // 池大小变化时取模安全
        let idx = lb.next_cursor_index("claude", 2);
        assert!(idx < 2);
    }
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test sticky_tests`（先在 `mod.rs` 加 `pub mod load_balance;` 使其参与编译）→ 编译错误

- [ ] **Step 3: 实现**（同文件，测试模块之上）：

```rust
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 粘性表空闲 TTL（30 分钟，设计 4.1）
pub(crate) const STICKY_IDLE_TTL_SECS: u64 = 30 * 60;
/// 粘性表容量上限（LRU 惰性驱逐，设计 4.1）
pub(crate) const STICKY_MAX_ENTRIES: usize = 4096;

#[derive(Debug, Clone)]
struct StickyEntry {
    provider_id: String,
    last_access: Instant,
}

#[derive(Default)]
struct LbInner {
    /// app_type -> (session_id -> 粘性条目)
    sticky: HashMap<String, HashMap<String, StickyEntry>>,
    /// 游标：app_type -> 下一个起始下标
    cursors: HashMap<String, usize>,
}

/// 会话粘性负载均衡器（每 ProviderRouter 一份，进程内单例语义）
pub struct SessionLoadBalancer {
    inner: Mutex<LbInner>,
    ttl: Duration,
    max_entries: usize,
}

impl SessionLoadBalancer {
    pub fn new() -> Self {
        Self::with_limits(Duration::from_secs(STICKY_IDLE_TTL_SECS), STICKY_MAX_ENTRIES)
    }

    pub(crate) fn with_limits(ttl: Duration, max_entries: usize) -> Self {
        Self {
            inner: Mutex::new(LbInner::default()),
            ttl,
            max_entries,
        }
    }

    /// 粘性查询：命中即续期；过期惰性清除
    pub fn lookup_sticky(&self, app_type: &str, session_id: &str) -> Option<String> {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let app_map = inner.sticky.get_mut(app_type)?;
        match app_map.get_mut(session_id) {
            Some(entry) if now.duration_since(entry.last_access) < self.ttl => {
                entry.last_access = now;
                Some(entry.provider_id.clone())
            }
            Some(_) => {
                app_map.remove(session_id);
                None
            }
            None => None,
        }
    }

    /// 新会话写入：先到者优先（并发抢建时后到者采纳先到者，设计 4.1.c）
    pub fn insert_sticky(&self, app_type: &str, session_id: &str, provider_id: &str) {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let app_map = inner.sticky.entry(app_type.to_string()).or_default();
        let exists_valid = app_map
            .get(session_id)
            .is_some_and(|e| now.duration_since(e.last_access) < self.ttl);
        if !exists_valid {
            app_map.insert(
                session_id.to_string(),
                StickyEntry {
                    provider_id: provider_id.to_string(),
                    last_access: now,
                },
            );
            if app_map.len() > self.max_entries {
                evict_idlest(app_map, now, self.ttl, self.max_entries);
            }
        }
    }

    /// failover 成功后的会话重映射：无条件改写（设计 4.1 迁移回写）
    pub fn remap_sticky(&self, app_type: &str, session_id: &str, provider_id: &str) {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let app_map = inner.sticky.entry(app_type.to_string()).or_default();
        app_map.insert(
            session_id.to_string(),
            StickyEntry {
                provider_id: provider_id.to_string(),
                last_access: now,
            },
        );
        if app_map.len() > self.max_entries {
            evict_idlest(app_map, now, self.ttl, self.max_entries);
        }
    }

    /// 轮询游标推进：返回本次起始下标，并推进到下一个位置
    pub fn next_cursor_index(&self, app_type: &str, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let cursor = inner.cursors.entry(app_type.to_string()).or_insert(0);
        let idx = *cursor % len;
        *cursor = idx + 1;
        idx
    }

    /// 测试观察用：粘性表条目数
    pub(crate) fn in_flight_len(&self, app_type: &str) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sticky
            .get(app_type)
            .map_or(0, |m| m.len())
    }
}

impl Default for SessionLoadBalancer {
    fn default() -> Self {
        Self::new()
    }
}

/// 惰性清理：先扫过期；仍超限则逐出最久未访问者
fn evict_idlest(
    app_map: &mut HashMap<String, StickyEntry>,
    now: Instant,
    ttl: Duration,
    max_entries: usize,
) {
    app_map.retain(|_, entry| now.duration_since(entry.last_access) < ttl);
    while app_map.len() > max_entries {
        let Some(idlest_key) = app_map
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        app_map.remove(&idlest_key);
    }
}
```

`mod.rs`：模块声明区加 `pub mod load_balance;`，导出区加 `pub use load_balance::SessionLoadBalancer;`

- [ ] **Step 4: 跑测试确认通过** — `cargo test sticky_tests` 全 PASS
- [ ] **Step 5: `cargo fmt` + clippy**，不 commit（并入阶段 1）

### Task 4: `select_providers_for_session` 路由接入

**Files:**
- Modify: `src-tauri/src/proxy/provider_router.rs`（结构体字段 + 新方法 + 抽取共享池构造 + 单测）
- Modify: `src-tauri/src/proxy/handler_context.rs:134-144`（换调用）

**Interfaces:**
- Consumes: Task 1 `load_balance_mode`、Task 3 `SessionLoadBalancer::{lookup_sticky,insert_sticky,next_cursor_index}`
- Produces: `pub async fn select_providers_for_session(&self, app_type: &str, session_id: &str, session_client_provided: bool) -> Result<Vec<Provider>, AppError>`；`pub fn load_balancer(&self) -> &SessionLoadBalancer`（Task 5/8/9 依赖）

- [ ] **Step 1: 写失败测试** — `provider_router.rs` 测试模块（沿用既有 `TempHome` fixture）追加：

```rust
async fn seed_two_providers_with_mode(db: &Arc<Database>, mode: &str) {
    let mut a = Provider::with_id("a".to_string(), "A".to_string(), json!({}), None);
    a.sort_index = Some(1);
    let mut b = Provider::with_id("b".to_string(), "B".to_string(), json!({}), None);
    b.sort_index = Some(2);
    db.save_provider("claude", &a).unwrap();
    db.save_provider("claude", &b).unwrap();
    db.add_to_failover_queue("claude", "a").unwrap();
    db.add_to_failover_queue("claude", "b").unwrap();
    let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
    config.auto_failover_enabled = true;
    config.load_balance_mode = mode.to_string();
    db.update_proxy_config_for_app(config).await.unwrap();
}

#[tokio::test]
#[serial]
async fn session_mode_off_behaves_identically_to_select_providers() {
    let _home = TempHome::new();
    let db = Arc::new(Database::memory().unwrap());
    seed_two_providers_with_mode(&db, "off").await;
    let router = ProviderRouter::new(db);
    let via_session = router
        .select_providers_for_session("claude", "s1", true)
        .await
        .unwrap();
    let plain = router.select_providers("claude").await.unwrap();
    assert_eq!(
        via_session.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        plain.iter().map(|p| p.id.as_str()).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[serial]
async fn session_mode_round_robins_new_sessions_and_writes_sticky() {
    let _home = TempHome::new();
    let db = Arc::new(Database::memory().unwrap());
    seed_two_providers_with_mode(&db, "session").await;
    let router = ProviderRouter::new(db);

    let first = router
        .select_providers_for_session("claude", "s1", true)
        .await
        .unwrap();
    // 返回 [选中] + 其余队列（forwarder 零改动，failover 语义保留）
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].id, "a");
    assert_eq!(first[1].id, "b");
    assert_eq!(router.load_balancer().lookup_sticky("claude", "s1"), Some("a".into()));

    let second = router
        .select_providers_for_session("claude", "s2", true)
        .await
        .unwrap();
    assert_eq!(second[0].id, "b"); // 游标推进到 b
    assert_eq!(second[1].id, "a");
}

#[tokio::test]
#[serial]
async fn sticky_hit_returns_provider_first_without_cursor_advance() {
    let _home = TempHome::new();
    let db = Arc::new(Database::memory().unwrap());
    seed_two_providers_with_mode(&db, "session").await;
    let router = ProviderRouter::new(db);
    router.load_balancer().insert_sticky("claude", "s1", "b");
    let result = router
        .select_providers_for_session("claude", "s1", true)
        .await
        .unwrap();
    assert_eq!(result[0].id, "b");
    assert_eq!(result[1].id, "a");
    // 粘性命中不动游标：下一个新会话仍从 a 开始
    let next = router
        .select_providers_for_session("claude", "s2", true)
        .await
        .unwrap();
    assert_eq!(next[0].id, "a");
}

#[tokio::test]
#[serial]
async fn generated_session_round_robins_without_sticky_write() {
    let _home = TempHome::new();
    let db = Arc::new(Database::memory().unwrap());
    seed_two_providers_with_mode(&db, "session").await;
    let router = ProviderRouter::new(db);
    router
        .select_providers_for_session("claude", "gen", false)
        .await
        .unwrap();
    assert_eq!(router.load_balancer().lookup_sticky("claude", "gen"), None);
}

#[tokio::test]
#[serial]
async fn sticky_provider_dropped_from_pool_falls_back_to_new_selection() {
    let _home = TempHome::new();
    let db = Arc::new(Database::memory().unwrap());
    seed_two_providers_with_mode(&db, "session").await;
    let router = ProviderRouter::new(db);
    router.load_balancer().insert_sticky("claude", "s1", "ghost");
    let result = router
        .select_providers_for_session("claude", "s1", true)
        .await
        .unwrap();
    // ghost 不在池内（熔断/移出队列）→ 按新会话重选并改写粘性
    assert_eq!(result[0].id, "a");
    assert_eq!(router.load_balancer().lookup_sticky("claude", "s1"), Some("a".into()));
}
```

- [ ] **Step 2: 确认失败** — `cargo test session_mode` 编译错误

- [ ] **Step 3: 实现** — `provider_router.rs`：

1. 结构体加字段并在 `new()` 初始化：

```rust
pub struct ProviderRouter {
    db: Arc<Database>,
    circuit_breakers: Arc<RwLock<HashMap<String, Arc<CircuitBreaker>>>>,
    /// 会话粘性负载均衡状态（粘性表/游标/在途/冷却）
    load_balancer: Arc<SessionLoadBalancer>,
}
```

```rust
pub fn new(db: Arc<Database>) -> Self {
    Self {
        db,
        circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
        load_balancer: Arc::new(SessionLoadBalancer::new()),
    }
}

/// 供 forwarder 记录重映射/冷却/在途（内部挂点用）
pub fn load_balancer(&self) -> &SessionLoadBalancer {
    &self.load_balancer
}
```

2. 把 `select_providers` 中 failover 开启分支的池构造循环（:84-111）抽成私有共享方法：

```rust
/// 故障转移队列 -> 熔断过滤后的候选池（返回 (池, 总数, 熔斷数)）
async fn circuit_filtered_queue(&self, app_type: &str) -> Result<(Vec<Provider>, usize, usize), AppError> {
    let all_providers = self.db.get_all_providers(app_type)?;
    let ordered_ids: Vec<String> = self
        .db
        .get_failover_queue(app_type)?
        .into_iter()
        .map(|item| item.provider_id)
        .collect();

    let mut result = Vec::new();
    let mut total = 0usize;
    let mut open = 0usize;
    for provider_id in ordered_ids {
        let Some(provider) = all_providers.get(&provider_id).cloned() else {
            continue;
        };
        if !provider_supports_failover(app_type, &provider) {
            continue;
        }
        total += 1;
        let circuit_key = format!("{app_type}:{}", provider.id);
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;
        if breaker.is_available().await {
            result.push(provider);
        } else {
            open += 1;
        }
    }
    Ok((result, total, open))
}
```

`select_providers` 的 failover 分支改调它（行为不变——`test_failover_enabled_uses_queue_order_ignoring_current` 等既有测试守护回归）。

3. 新方法（放在 `select_providers` 之后）：

```rust
/// 会话感知选号：load_balance_mode = off 时与 select_providers 完全一致；
/// session 模式下粘性命中优先，新会话轮询并写粘性表。
///
/// 返回 [选中账号] + 其余候选（按队列序），forwarder 顺序尝试语义不变，
/// 请求内 failover 自动保留。
pub async fn select_providers_for_session(
    &self,
    app_type: &str,
    session_id: &str,
    session_client_provided: bool,
) -> Result<Vec<Provider>, AppError> {
    let config = match self.db.get_proxy_config_for_app(app_type).await {
        Ok(config) => config,
        Err(e) => {
            // 与 select_providers 的容错语义对齐：读不到配置按关闭处理
            log::error!("[{app_type}] 读取 proxy_config 失败: {e}，负载均衡不生效");
            return self.select_providers(app_type).await;
        }
    };
    if config.load_balance_mode != "session" || !config.auto_failover_enabled {
        // off 模式 / 故障转移关闭（池只有当前供应商，粘性无意义）
        return self.select_providers(app_type).await;
    }

    let (pool, total_providers, circuit_open_count) = self.circuit_filtered_queue(app_type).await?;
    if pool.is_empty() {
        if total_providers > 0 && circuit_open_count == total_providers {
            return Err(AppError::AllProvidersCircuitOpen);
        }
        return Err(AppError::NoProvidersConfigured);
    }
    if pool.len() == 1 {
        // 单账号池：无需粘性，避免污染粘性表
        return Ok(pool);
    }

    // 1) 粘性命中且账号仍在池内 → 沿用（不查并发上限，设计决策 4）
    if session_client_provided {
        if let Some(hit) = self.load_balancer.lookup_sticky(app_type, session_id) {
            if let Some(idx) = pool.iter().position(|p| p.id == hit) {
                let mut result = vec![pool[idx].clone()];
                result.extend(
                    pool.iter()
                        .enumerate()
                        .filter(|(i, _)| *i != idx)
                        .map(|(_, p)| p.clone()),
                );
                return Ok(result);
            }
            // 粘性账号被熔断/移出队列 → 按新会话重选（设计 5.1）
        }
    }

    // 2) 新会话：轮询游标选号（阶段 2 在此叠加冷却过滤与饱和跳过）
    let idx = self.load_balancer.next_cursor_index(app_type, pool.len());
    if session_client_provided {
        self.load_balancer.insert_sticky(app_type, session_id, &pool[idx].id);
    }
    let mut result = vec![pool[idx].clone()];
    result.extend(
        pool.iter()
            .enumerate()
            .filter(|(i, _)| *i != idx)
            .map(|(_, p)| p.clone()),
    );
    Ok(result)
}
```

4. `handler_context.rs:134-144` 调用点替换（错误映射不变）：

```rust
let providers = state
    .provider_router
    .select_providers_for_session(app_type_str, &session_id, session_result.client_provided)
    .await
    .map_err(|e| match e {
        crate::error::AppError::AllProvidersCircuitOpen => ProxyError::AllProvidersCircuitOpen,
        crate::error::AppError::NoProvidersConfigured => ProxyError::NoProvidersConfigured,
        _ => ProxyError::DatabaseError(e.to_string()),
    })?;
```

- [ ] **Step 4: 全量路由测试** — `cargo test provider_router`（含既有 6 个回归测试）+ `cargo test session_mode` 全 PASS
- [ ] **Step 5: fmt + clippy**，不 commit

### Task 5: forwarder 会话重映射挂点

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs:389-417`（`forward_with_retry` wrapper）
- Modify: `src-tauri/src/proxy/provider_router.rs`（`remap_session` 包装）

**Interfaces:**
- Consumes: Task 4 `load_balancer()`、Task 3 `remap_sticky`
- Produces: `ProviderRouter::pub(crate) fn remap_session(&self, app_type: &str, session_id: &str, client_provided: bool, new_provider_id: &str)`

- [ ] **Step 1: 写失败测试** — `provider_router.rs` 测试加：

```rust
#[tokio::test]
#[serial]
async fn remap_session_updates_sticky_mapping() {
    let _home = TempHome::new();
    let db = Arc::new(Database::memory().unwrap());
    seed_two_providers_with_mode(&db, "session").await;
    let router = ProviderRouter::new(db);
    router.load_balancer().insert_sticky("claude", "s1", "a");
    // Generated 会话不写表
    router.remap_session("claude", "gen", false, "b");
    assert_eq!(router.load_balancer().lookup_sticky("claude", "gen"), None);
    // 真实会话重映射生效
    router.remap_session("claude", "s1", true, "b");
    assert_eq!(router.load_balancer().lookup_sticky("claude", "s1"), Some("b".into()));
}
```

- [ ] **Step 2: 确认失败** — 编译错误

- [ ] **Step 3: 实现**

`provider_router.rs`：

```rust
/// failover 成功后的会话重映射（forwarder wrapper 调用）
pub(crate) fn remap_session(
    &self,
    app_type: &str,
    session_id: &str,
    client_provided: bool,
    new_provider_id: &str,
) {
    if !client_provided {
        return; // Generated 会话不写粘性表（设计决策 3）
    }
    self.load_balancer
        .remap_sticky(app_type, session_id, new_provider_id);
}
```

`forwarder.rs` `forward_with_retry`（:389-417）——inner 调用前记下路由选中账号，Ok 出口统一重映射（一处覆盖 inner 的全部成功路径）：

```rust
// 路由选中账号（列表首位）：failover 实际落到别家时用于会话重映射
let selected_provider_id = providers.first().map(|p| p.id.clone());
let result = self
    .forward_with_retry_inner(
        app_type, method, endpoint, body, headers, extensions, providers,
    )
    .await;
// 把 guard 注入到 Ok 结果，让它随响应一起流转到 response_processor，
// 在流式 body 的 future 内才真正 drop。
// Err 路径：guard 在函数 scope 内随返回值落地时自动 drop。
match result {
    Ok(mut fr) => {
        let app_type_str = app_type.as_str();
        if let Some(selected) = selected_provider_id.as_deref() {
            if fr.provider.id != selected && self.session_client_provided {
                // 实际服务账号 ≠ 路由选中账号（failover 成功）→ 会话重映射
                self.router
                    .remap_session(app_type_str, &self.session_id, true, &fr.provider.id);
            }
        }
        fr.connection_guard = Some(guard);
        Ok(fr)
    }
    Err(e) => Err(e),
}
```

- [ ] **Step 4: 跑测试** — `cargo test remap_session` + `cargo test forwarder`（既有 forwarder 测试守护）PASS
- [ ] **Step 5: fmt + clippy**

### Task 6: 阶段 1 门禁 + 提交

- [ ] **Step 1: 全量门禁** — `cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test`；根目录 `pnpm typecheck && pnpm test:unit`。全绿才算过
- [ ] **Step 2: Commit（显式路径，避开 transform_codex_chat.rs）**

```bash
git add src-tauri/src/proxy/types.rs src-tauri/src/proxy/load_balance.rs \
  src-tauri/src/proxy/mod.rs src-tauri/src/proxy/provider_router.rs \
  src-tauri/src/proxy/handler_context.rs src-tauri/src/proxy/forwarder.rs \
  src-tauri/src/database/schema.rs src-tauri/src/database/dao/proxy.rs \
  src-tauri/src/commands/proxy.rs src/types/proxy.ts \
  src/components/proxy/AutoFailoverConfigPanel.tsx \
  src/i18n/locales/en.json src/i18n/locales/zh.json \
  src/i18n/locales/zh-TW.json src/i18n/locales/ja.json
git commit -m "feat(proxy): 会话粘性负载均衡阶段 1（粘性表+路由接入+重映射）

load_balance_mode 配置（off/session，proxy_config 新列+迁移）；SessionLoadBalancer
粘性表（TTL 30min/LRU 4096/惰性清理）；select_providers_for_session 路由接入
（off 模式与现状一致）；failover 成功后时会话重映射；设置页负载均衡下拉（i18n×4）。

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

## 阶段 2（设计 7.2：并发上限 + 冷却 + 预设 UI）

### Task 7: ProviderMeta 并发上限（两级查表）

**Files:**
- Modify: `src-tauri/src/provider.rs:444-484`（ProviderMeta + 新结构 + Provider 方法）

**Interfaces:**
- Produces:
  - `pub struct ProviderLoadBalance { pub max_concurrent_requests: Option<u32>, pub max_concurrent_by_model: HashMap<String, u32> }`（serde camelCase，键名 `loadBalance` 挂在 meta 上）
  - `impl Provider { pub fn load_balance(&self) -> Option<&ProviderLoadBalance>; pub fn max_concurrent_for(&self, outbound_model: &str) -> u32 }`（0 = 不限；模型覆盖优先于账号默认）

- [ ] **Step 1: 写失败测试** — `provider.rs` 测试模块加：

```rust
#[test]
fn max_concurrent_two_level_lookup() {
    let settings = json!({
        "loadBalance": {
            "maxConcurrentRequests": 8,
            "maxConcurrentByModel": { "glm-5.3-flash": 50 }
        }
    });
    let provider = Provider::with_id("p".into(), "P".into(), settings, None);
    assert_eq!(provider.max_concurrent_for("glm-5.3"), 8);
    assert_eq!(provider.max_concurrent_for("glm-5.3-flash"), 50); // 模型覆盖
    assert_eq!(provider.max_concurrent_for("other"), 8);          // 回退默认
}

#[test]
fn max_concurrent_defaults_to_unlimited() {
    let provider = Provider::with_id("p".into(), "P".into(), json!({}), None);
    assert_eq!(provider.max_concurrent_for("any"), 0); // 0 = 不限
    let only_model = Provider::with_id(
        "q".into(), "Q".into(),
        json!({ "loadBalance": { "maxConcurrentByModel": { "kimi-k2": 4 } } }), None,
    );
    assert_eq!(only_model.max_concurrent_for("kimi-k2"), 4);
    assert_eq!(only_model.max_concurrent_for("other"), 0); // 无默认→不限
}

#[test]
fn provider_meta_load_balance_roundtrip() {
    let settings = json!({ "loadBalance": { "maxConcurrentRequests": 30 } });
    let provider = Provider::with_id("p".into(), "P".into(), settings, None);
    let meta = provider.meta.clone().unwrap_or_default();
    assert!(meta.load_balance.is_some());
}
```

- [ ] **Step 2: 确认失败** — `cargo test max_concurrent` 编译错误

- [ ] **Step 3: 实现** — `provider.rs`：

```rust
/// 供应商级并发上限（代理负载均衡用；两级：账号默认 + 按模型覆盖）
///
/// 存放于 `meta`（与 costMultiplier 同层）：CC Switch 内部调优项，
/// 不写入各 CLI 工具的 live 配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLoadBalance {
    /// 账号默认并发上限；0 或缺省 = 不限
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent_requests: Option<u32>,
    /// 按模型覆盖（key = outbound model 名）
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub max_concurrent_by_model: std::collections::HashMap<String, u32>,
}
```

`ProviderMeta` 加字段（`pricing_model_source` 之后）：

```rust
/// 并发上限配置（代理会话粘性负载均衡）
#[serde(rename = "loadBalance", default, skip_serializing_if = "Option::is_none")]
pub load_balance: Option<ProviderLoadBalance>,
```

`impl Provider` 加：

```rust
/// 并发上限配置（meta.loadBalance）
pub fn load_balance(&self) -> Option<&ProviderLoadBalance> {
    self.meta.as_ref().and_then(|m| m.load_balance.as_ref())
}

/// 两级并发上限查表：模型覆盖优先，其次账号默认，0 = 不限
pub fn max_concurrent_for(&self, outbound_model: &str) -> u32 {
    self.load_balance()
        .map(|lb| {
            lb.max_concurrent_by_model
                .get(outbound_model)
                .copied()
                .or(lb.max_concurrent_requests)
                .unwrap_or(0)
        })
        .unwrap_or(0)
}
```

- [ ] **Step 4: 测试通过** — `cargo test max_concurrent` + `cargo test provider` PASS
- [ ] **Step 5: fmt + clippy**，不 commit

### Task 8: 在途计数 + InFlightHandle + 饱和选号

**Files:**
- Modify: `src-tauri/src/proxy/load_balance.rs`（计数表 + InFlightHandle + resolve_outbound_model + select_new_session/select_least_in_flight）
- Modify: `src-tauri/src/proxy/forwarder.rs`（ActiveConnectionGuard 扩展、ForwardResult.in_flight、迭代内在途获取、4 个 Ok 返回点、wrapper attach）
- Modify: `src-tauri/src/proxy/provider_router.rs`（acquire 包装 + 选号算法升级 + 单测）

**Interfaces:**
- Consumes: Task 7 `Provider::max_concurrent_for`
- Produces:
  - `load_balance.rs`: `pub struct InFlightHandle`（`InFlightHandle::acquire(counter) -> Self`、`detach(self) -> Arc<AtomicIsize>`）；`pub(crate) fn resolve_outbound_model(provider: &Provider, request_model: &str) -> String`；`pub fn select_new_session(&self, app_type: &str, pool: &[Provider], request_model: &str) -> usize`（冷却过滤 + 游标 + 饱和跳过 + 全饱和最少在途）；`pub fn select_least_in_flight(&self, app_type: &str, pool: &[Provider], request_model: &str) -> usize`；`pub fn acquire_in_flight(&self, app_type: &str, provider_id: &str, outbound_model: &str) -> InFlightHandle`；`pub fn in_flight_of(&self, app_type: &str, provider_id: &str, outbound_model: &str) -> isize`
  - `forwarder.rs`: `ActiveConnectionGuard { status, in_flight: Vec<Arc<AtomicIsize>> }` + `pub(crate) fn attach_counters(&mut self, counters: Vec<Arc<AtomicIsize>>)`；`ForwardResult.in_flight: Option<InFlightHandle>`
  - `provider_router.rs`: `pub(crate) fn acquire_for_provider(&self, app_type: &str, provider: &Provider, request_model: &str) -> InFlightHandle`

- [ ] **Step 1: 写失败测试** — `load_balance.rs` 加测试模块：

```rust
#[cfg(test)]
mod inflight_tests {
    use super::*;
    use crate::provider::{Provider, ProviderLoadBalance};
    use serde_json::json;
    use std::collections::HashMap;

    fn provider_with_limit(id: &str, limit: u32) -> Provider {
        Provider::with_id(
            id.to_string(),
            id.to_string(),
            json!({ "loadBalance": { "maxConcurrentRequests": limit } }),
            None,
        )
    }

    #[test]
    fn resolve_outbound_model_applies_provider_mapping() {
        let mapped = Provider::with_id(
            "p".into(), "P".into(),
            json!({ "model_mapping": { "alias": "real-model" } }), None,
        );
        // 映射表键名以 model_mapper::from_provider 实际读取为准；
        // 无映射时原样返回
        let plain = Provider::with_id("q".into(), "Q".into(), json!({}), None);
        assert_eq!(resolve_outbound_model(&plain, "claude-x[1M]"), "claude-x");
        assert_eq!(resolve_outbound_model(&plain, "glm-5.3"), "glm-5.3");
    }

    #[test]
    fn in_flight_acquire_and_release() {
        let lb = SessionLoadBalancer::new();
        let h1 = lb.acquire_in_flight("claude", "a", "glm-5.3");
        let h2 = lb.acquire_in_flight("claude", "a", "glm-5.3");
        assert_eq!(lb.in_flight_of("claude", "a", "glm-5.3"), 2);
        drop(h1);
        assert_eq!(lb.in_flight_of("claude", "a", "glm-5.3"), 1);
        drop(h2);
        assert_eq!(lb.in_flight_of("claude", "a", "glm-5.3"), 0);
    }

    #[test]
    fn in_flight_handle_detach_does_not_decrement() {
        let lb = SessionLoadBalancer::new();
        let h = lb.acquire_in_flight("claude", "a", "m");
        let counter = h.detach();
        assert_eq!(lb.in_flight_of("claude", "a", "m"), 1); // 仍占用
        counter.fetch_sub(1, Ordering::AcqRel); // 移交方负责递减
        assert_eq!(lb.in_flight_of("claude", "a", "m"), 0);
    }

    #[test]
    fn select_new_session_skips_saturated_then_falls_back_to_least_in_flight() {
        let lb = SessionLoadBalancer::new();
        let pool = vec![provider_with_limit("a", 1), provider_with_limit("b", 0)];
        // a 饱和（在途 1/上限 1），b 不限 → 从游标 0 起跳过 a 选 b
        let _h = lb.acquire_in_flight("claude", "a", "m");
        assert_eq!(lb.select_new_session("claude", &pool, "m"), 1);
        // 全饱和（b 也占 2/不限永不满？不限=0 永不饱和；改用两限账号）
        let lb2 = SessionLoadBalancer::new();
        let pool2 = vec![provider_with_limit("a", 1), provider_with_limit("b", 1)];
        let _ha = lb2.acquire_in_flight("claude", "a", "m");
        let _hb1 = lb2.acquire_in_flight("claude", "b", "m");
        let _hb2 = lb2.acquire_in_flight("claude", "b", "m");
        // 全饱和 → 在途最少者（a=1 < b=2）
        assert_eq!(lb2.select_new_session("claude", &pool2, "m"), 0);
    }

    #[test]
    fn select_least_in_flight_prefers_lower_count() {
        let lb = SessionLoadBalancer::new();
        let pool = vec![provider_with_limit("a", 0), provider_with_limit("b", 0)];
        let _h = lb.acquire_in_flight("claude", "a", "m");
        assert_eq!(lb.select_least_in_flight("claude", &pool, "m"), 1);
    }
}
```

（注：`resolve_outbound_model_applies_provider_mapping` 中映射键名若与 `model_mapper.rs` 的 `from_provider` 实际读取键不符，以 `model_mapper.rs` 为准调整测试 JSON——实现时核对一次。）

- [ ] **Step 2: 确认失败** — 编译错误

- [ ] **Step 3: 实现 load_balance.rs 增量**

依赖引入（文件头）：

```rust
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use crate::provider::Provider;
use crate::proxy::model_mapper::{strip_one_m_suffix_for_upstream, ModelMapping};
```

`LbInner` 加字段：

```rust
/// "provider_id\u{0}outbound_model" -> 在途计数
in_flight: HashMap<String, Arc<AtomicIsize>>,
```

新类型与函数：

```rust
/// 在途计数 RAII 句柄：构造 +1，Drop 同步 -1。
///
/// 计数是纯原子量，Drop 无需异步调度（对照 ActiveConnectionGuard 需要异步
/// 归还是因为它写 tokio RwLock）。成功路径经 `detach()` 把计数所有权移交给
/// ActiveConnectionGuard，随响应流结束才递减；失败路径随迭代作用域 Drop。
pub struct InFlightHandle {
    counter: Arc<AtomicIsize>,
}

impl InFlightHandle {
    pub fn acquire(counter: Arc<AtomicIsize>) -> Self {
        counter.fetch_add(1, Ordering::AcqRel);
        Self { counter }
    }

    /// 移交计数所有权（不递减）；移交方负责最终 fetch_sub
    pub fn detach(self) -> Arc<AtomicIsize> {
        let counter = self.counter.clone();
        std::mem::forget(self);
        counter
    }
}

impl Drop for InFlightHandle {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

/// 选号用的出站模型名：供应商模型映射 + [1M] 后缀剥离（与 forward 实际发出的模型一致）
pub(crate) fn resolve_outbound_model(provider: &Provider, request_model: &str) -> String {
    let mapping = ModelMapping::from_provider(provider);
    let mapped = mapping.map_model(request_model);
    strip_one_m_suffix_for_upstream(&mapped).to_string()
}
```

`SessionLoadBalancer` impl 追加：

```rust
fn in_flight_key(provider_id: &str, outbound_model: &str) -> String {
    format!("{provider_id}\u{0}{outbound_model}")
}

/// 在途计数 +1（RAII），计数 key = provider_id + outbound model（设计 4.1）
pub fn acquire_in_flight(
    &self,
    app_type: &str,
    provider_id: &str,
    outbound_model: &str,
) -> InFlightHandle {
    let key = Self::in_flight_key(provider_id, outbound_model);
    let counter = {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.in_flight.entry(key).or_insert_with(|| Arc::new(AtomicIsize::new(0))).clone()
    };
    InFlightHandle::acquire(counter)
}

/// 指定账号+模型的当前在途数
pub fn in_flight_of(&self, app_type: &str, provider_id: &str, outbound_model: &str) -> isize {
    let _ = app_type; // 计数按 provider+model 全局计（provider id 已含应用隔离）
    let key = Self::in_flight_key(provider_id, outbound_model);
    self.inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .in_flight
        .get(&key)
        .map_or(0, |c| c.load(Ordering::Acquire))
}

/// 该账号在 (provider, request_model 映射后) 维度的在途数
fn provider_in_flight(&self, provider: &Provider, request_model: &str) -> isize {
    let model = resolve_outbound_model(provider, request_model);
    self.in_flight_of("", &provider.id, &model)
}

fn is_saturated(&self, provider: &Provider, request_model: &str) -> bool {
    let limit = provider.max_concurrent_for(&resolve_outbound_model(provider, request_model));
    limit != 0 && self.provider_in_flight(provider, request_model) >= limit as isize
}

/// 新会话选号（设计 4.1.b）：
/// 1. 过滤冷却中账号（全冷却则忽略冷却——本地拒绝会打断客户端，宁可放行让上游兜底）
/// 2. 从轮询游标起找第一个未饱和账号（上限 0 = 不限），游标推进
/// 3. 全饱和 → 在途最少者放行
pub fn select_new_session(
    &self,
    app_type: &str,
    pool: &[Provider],
    request_model: &str,
) -> usize {
    let candidates = self.cooldown_filtered_indices(app_type, pool);
    let len = candidates.len();
    let start = self.next_cursor_index(app_type, len);
    for offset in 0..len {
        let idx = candidates[(start + offset) % len];
        if !self.is_saturated(&pool[idx], request_model) {
            return idx;
        }
    }
    // 全饱和 → 在途最少者（设计 4.1.b）
    self.select_least_in_flight(app_type, pool, request_model)
}

/// Generated 会话选号：在途最少（不写粘性表，设计 4.1.d）
pub fn select_least_in_flight(
    &self,
    app_type: &str,
    pool: &[Provider],
    request_model: &str,
) -> usize {
    let candidates = self.cooldown_filtered_indices(app_type, pool);
    candidates
        .into_iter()
        .min_by_key(|&idx| self.provider_in_flight(&pool[idx], request_model))
        .expect("cooldown_filtered_indices 保证非空")
}

/// 冷却过滤：返回可用下标；全冷却时退回全部（Task 9 提供 is_cooling，此处前瞻引用）
fn cooldown_filtered_indices(&self, app_type: &str, pool: &[Provider]) -> Vec<usize> {
    let available: Vec<usize> = (0..pool.len())
        .filter(|&i| !self.is_cooling(app_type, &pool[i].id))
        .collect();
    if available.is_empty() { (0..pool.len()).collect() } else { available }
}
```

注意：`is_cooling`/冷却表在 Task 9 才有。本任务先加**最小冷却占位**（`is_cooling` 恒 `false` + 空实现），Task 9 补全并测退避——或者直接把 Task 9 的冷却状态一并实现（两任务共担 TDD 步骤）。**执行时采用后者**：Task 8 与 Task 9 在 `load_balance.rs` 内连续实施，冷却状态结构一次成型（见 Task 9 测试），避免占位代码。

- [ ] **Step 4: forwarder 接线**

1. `ActiveConnectionGuard`（:130-156）扩展：

```rust
pub(crate) struct ActiveConnectionGuard {
    status: Arc<RwLock<ProxyStatus>>,
    /// 随响应生命周期递减的在途计数（负载均衡用；原子量，Drop 同步递减）
    in_flight: Vec<Arc<AtomicIsize>>,
}

impl ActiveConnectionGuard {
    pub(crate) async fn acquire(status: Arc<RwLock<ProxyStatus>>) -> Self {
        { let mut s = status.write().await; s.active_connections = s.active_connections.saturating_add(1); }
        Self { status, in_flight: Vec::new() }
    }

    /// 挂载在途计数（forwarder 成功路径调用；所有权移交给 guard）
    pub(crate) fn attach_counters(&mut self, counters: Vec<Arc<AtomicIsize>>) {
        self.in_flight.extend(counters);
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        // 在途计数是纯原子量：同步递减，无 runtime 也不泄漏
        for counter in &self.in_flight {
            counter.fetch_sub(1, Ordering::AcqRel);
        }
        // Drop 不能 await：把减量操作调度到 tokio runtime
        let status = self.status.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut s = status.write().await;
                s.active_connections = s.active_connections.saturating_sub(1);
            });
        }
    }
}
```

2. `ForwardResult`（:101-113）加字段：

```rust
/// 该次请求占用的在途计数句柄；wrapper 会移交给 connection_guard，
/// 随响应流结束递减（阶段 2 负载均衡）
pub(crate) in_flight: Option<crate::proxy::load_balance::InFlightHandle>,
```

（既有 4 处 `ForwardResult { ... }` 构造点全部补 `in_flight: None`——编译器逐一指认。）

3. `forward_with_retry_inner` 迭代内（`attempted_providers += 1;` :514 之前）获取在途句柄：

```rust
// 负载均衡在途计数：该账号该模型的在途 +1（粘性命中也计数，仅选号时不查上限）
// 迭代失败随作用域 Drop 递减；成功移入 ForwardResult
let mut in_flight_handle = self.router.acquire_for_provider(
    app_type_str,
    provider,
    body.get("model").and_then(Value::as_str).unwrap_or("unknown"),
);
```

4 个 `Ok(ForwardResult { ... })` 返回点（主路径 :585、media :688、rectifier :837、budget :997）的 `connection_guard: None,` 旁加：

```rust
in_flight: in_flight_handle.take(),
```

（`in_flight_handle` 声明为 `let mut in_flight_handle: Option<InFlightHandle> = Some(...)`，`acquire_for_provider` 返回 `InFlightHandle` 包 `Some`。）

4. wrapper（Task 5 改造后的 match）`Ok(mut fr)` 分支，`fr.connection_guard = Some(guard);` 之前加：

```rust
if let Some(handle) = fr.in_flight.take() {
    guard.attach_counters(vec![handle.detach()]);
}
```

5. `provider_router.rs` 包装：

```rust
/// 为指定供应商获取在途计数句柄（forwarder 每 attempt 调用）
pub(crate) fn acquire_for_provider(
    &self,
    app_type: &str,
    provider: &Provider,
    request_model: &str,
) -> InFlightHandle {
    let outbound_model = crate::proxy::load_balance::resolve_outbound_model(provider, request_model);
    self.load_balancer
        .acquire_in_flight(app_type, &provider.id, &outbound_model)
}
```

6. 选号算法升级——`select_providers_for_session` 的"新会话"分支（Task 4 步骤 3 第 2 段）改为：

```rust
// 2) 新会话：冷却过滤 + 游标 + 饱和跳过 + 全饱和最少在途
let request_model = /* 由调用方传入（见下） */;
let idx = if session_client_provided {
    let idx = self.load_balancer.select_new_session(app_type, &pool, request_model);
    self.load_balancer.insert_sticky(app_type, session_id, &pool[idx].id);
    idx
} else {
    // Generated：在途最少，不写粘性表（设计 4.1.d）
    self.load_balancer.select_least_in_flight(app_type, &pool, request_model)
};
```

`select_providers_for_session` 签名增加 `request_model: &str`（`handler_context.rs` 调用点传 `&request_model`——该变量在 :114-118 已先于选号解析）。同步更新 Task 4 的全部测试调用（补 `"unknown"` 或实际模型参数）。

- [ ] **Step 5: 跑测试** — `cargo test inflight_tests` + `cargo test session_mode`（升级后：饱和跳过/全饱和/Generated 最少在途三个新断言并入）+ `cargo test forwarder` PASS
- [ ] **Step 6: fmt + clippy**

### Task 9: 429/403 冷却 + Retry-After

**Files:**
- Modify: `src-tauri/src/proxy/error.rs:46`（UpstreamError 加字段）
- Modify: `src-tauri/src/proxy/forwarder.rs:2402-2426`（解析 retry-after）、Retryable 分支（:1057）挂冷却、wrapper Ok 分支加成功重置
- Modify: `src-tauri/src/proxy/load_balance.rs`（冷却状态 + record_rate_limit/is_cooling/record_success + 单测）
- Modify: `src-tauri/src/proxy/provider_router.rs`（record_lb_rate_limit/record_lb_success 包装）

**Interfaces:**
- Produces:
  - `ProxyError::UpstreamError { status: u16, body: Option<String>, retry_after_secs: Option<u64> }`
  - `SessionLoadBalancer::{pub fn record_rate_limit(&self, app_type: &str, provider_id: &str, retry_after_secs: Option<u64>), pub fn is_cooling(&self, app_type: &str, provider_id: &str) -> bool, pub fn cooldown_remaining(&self, app_type: &str, provider_id: &str) -> Option<Duration>, pub fn record_success(&self, app_type: &str, provider_id: &str)}`
  - `pub fn is_rate_limit_status(status: u16, body: Option<&str>) -> bool`（429 恒冷却；403 仅 body 含并发/限流语义才冷却——Kimi 403 concurrent limit，设计 5）
  - `pub(crate) const COOLDOWN_MAX_SECS: u64 = 600;`

- [ ] **Step 1: 写失败测试** — `load_balance.rs`：

```rust
#[cfg(test)]
mod cooldown_tests {
    use super::*;

    #[test]
    fn rate_limit_status_detection() {
        assert!(is_rate_limit_status(429, None));
        assert!(is_rate_limit_status(429, Some("rate limited")));
        assert!(is_rate_limit_status(403, Some("concurrent requests limit exceeded")));
        assert!(is_rate_limit_status(403, Some("并发数超限")));
        assert!(!is_rate_limit_status(403, Some("invalid api key"))); // 鉴权 403 不冷却
        assert!(!is_rate_limit_status(403, None)); // 无 body 证据不按限流处理
        assert!(!is_rate_limit_status(500, None));
    }

    #[test]
    fn cooldown_backoff_ladder_and_cap() {
        let lb = SessionLoadBalancer::new();
        // 无 Retry-After：缺省 60s
        lb.record_rate_limit("claude", "a", None);
        assert_eq!(lb.cooldown_remaining("claude", "a"), Some(Duration::from_secs(60)));
        // 连续触发：指数退避 60→120→300→600（封顶）
        lb.record_rate_limit("claude", "a", None);
        assert_eq!(lb.cooldown_remaining("claude", "a"), Some(Duration::from_secs(120)));
        lb.record_rate_limit("claude", "a", None);
        assert_eq!(lb.cooldown_remaining("claude", "a"), Some(Duration::from_secs(300)));
        lb.record_rate_limit("claude", "a", None);
        assert_eq!(lb.cooldown_remaining("claude", "a"), Some(Duration::from_secs(600)));
        lb.record_rate_limit("claude", "a", None);
        assert_eq!(lb.cooldown_remaining("claude", "a"), Some(Duration::from_secs(600)));
        assert!(lb.is_cooling("claude", "a"));
    }

    #[test]
    fn retry_after_takes_precedence_but_capped() {
        let lb = SessionLoadBalancer::new();
        lb.record_rate_limit("claude", "a", Some(120));
        assert_eq!(lb.cooldown_remaining("claude", "a"), Some(Duration::from_secs(120)));
        lb.record_rate_limit("claude", "a", Some(86400)); // 上游天级值封顶 10m
        assert_eq!(lb.cooldown_remaining("claude", "a"), Some(Duration::from_secs(600)));
    }

    #[test]
    fn success_resets_cooldown() {
        let lb = SessionLoadBalancer::new();
        lb.record_rate_limit("claude", "a", None);
        lb.record_success("claude", "a");
        assert!(!lb.is_cooling("claude", "a"));
        // 重置后重新从 60s 起步
        lb.record_rate_limit("claude", "a", None);
        assert_eq!(lb.cooldown_remaining("claude", "a"), Some(Duration::from_secs(60)));
    }

    #[test]
    fn cooldown_is_per_app_provider() {
        let lb = SessionLoadBalancer::new();
        lb.record_rate_limit("claude", "a", None);
        assert!(!lb.is_cooling("codex", "a"));
        assert!(!lb.is_cooling("claude", "b"));
    }
}
```

- [ ] **Step 2: 确认失败** — 编译错误

- [ ] **Step 3: 实现**

`load_balance.rs` 常量与结构（与 Task 8 的 `cooldown_filtered_indices` 对上）：

```rust
/// 429/403 冷却退避阶梯（秒），封顶 COOLDOWN_MAX_SECS（设计 4.1）
const COOLDOWN_BACKOFF_SECS: [u64; 3] = [60, 120, 300];
pub(crate) const COOLDOWN_MAX_SECS: u64 = 600;

struct CooldownEntry {
    until: Instant,
    consecutive: u32,
}
```

`LbInner` 加 `cooldown: HashMap<String, CooldownEntry>`（key = `"{app_type}:{provider_id}"`），impl 追加：

```rust
/// 记录限流冷却：优先 Retry-After（封顶 10m），否则按连续触发次数指数退避
pub fn record_rate_limit(&self, app_type: &str, provider_id: &str, retry_after_secs: Option<u64>) {
    let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
    let key = format!("{app_type}:{provider_id}");
    let consecutive = inner
        .cooldown
        .get(&key)
        .map_or(1, |e| e.consecutive.saturating_add(1));
    let secs = retry_after_secs
        .map(|s| s.min(COOLDOWN_MAX_SECS))
        .unwrap_or_else(|| {
            COOLDOWN_BACKOFF_SECS
                .get(consecutive.saturating_sub(1) as usize)
                .copied()
                .unwrap_or(COOLDOWN_MAX_SECS)
        });
    inner.cooldown.insert(
        key,
        CooldownEntry {
            until: Instant::now() + Duration::from_secs(secs),
            consecutive,
        },
    );
}

/// 是否冷却中（过期惰性清除）
pub fn is_cooling(&self, app_type: &str, provider_id: &str) -> bool {
    self.cooldown_remaining(app_type, provider_id).is_some()
}

/// 剩余冷却时间（UI/测试观察用）
pub fn cooldown_remaining(&self, app_type: &str, provider_id: &str) -> Option<Duration> {
    let now = Instant::now();
    let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
    let key = format!("{app_type}:{provider_id}");
    match inner.cooldown.get(&key) {
        Some(entry) if entry.until > now => Some(entry.until - now),
        Some(_) => {
            inner.cooldown.remove(&key);
            None
        }
        None => None,
    }
}

/// 该账号成功服务一次：清冷却、重置退避计数
pub fn record_success(&self, app_type: &str, provider_id: &str) {
    self.inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .cooldown
        .remove(&format!("{app_type}:{provider_id}"));
}

/// 限流判定：429 恒冷却；403 需 body 呈并发/限流语义（Kimi 403 concurrent limit，设计 5）
pub fn is_rate_limit_status(status: u16, body: Option<&str>) -> bool {
    match status {
        429 => true,
        403 => body.is_some_and(|b| {
            let lower = b.to_lowercase();
            lower.contains("concurrent") || b.contains("并发") || lower.contains("rate limit")
        }),
        _ => false,
    }
}
```

`error.rs:46`：

```rust
#[error("上游错误 (状态码 {status}): {body:?}")]
UpstreamError {
    status: u16,
    body: Option<String>,
    /// 上游 Retry-After 头（秒；仅支持秒格式，HTTP-date 不解析）。限流冷却用
    retry_after_secs: Option<u64>,
},
```

全库构造点补 `retry_after_secs: None`（枚举过的：`forwarder.rs:3788,4545,4549,4553,4582,5065,5155,5201`、`handlers.rs:3531,3551`），非 `..` 的模式（`error.rs:85`、`handlers.rs:1896`、`forwarder.rs:2858`、`error_mapper.rs` 编译器指认处）补 `..`——以 clippy/编译输出为验收清单逐一清零。

`forwarder.rs:2402-2426` 错误构造处解析 Retry-After（`get_content_encoding` 旁）：

```rust
let retry_after_secs = response
    .headers()
    .get("retry-after")
    .and_then(|v| v.to_str().ok())
    .and_then(|v| v.trim().parse::<u64>().ok())
    .map(|s| s.min(crate::proxy::load_balance::COOLDOWN_MAX_SECS));
```

构造改为：

```rust
Err(ProxyError::UpstreamError {
    status: status_code,
    body: body_text,
    retry_after_secs,
})
```

Retryable 分支（:1057 `let _ = self.router.record_result(...)` 之前）挂冷却：

```rust
// 429/403 限流冷却（与熔断器分工：熔断管错误率，冷却管限流；设计 4.1）
if let ProxyError::UpstreamError {
    status,
    body,
    retry_after_secs,
    ..
} = &e
{
    if SessionLoadBalancer::is_rate_limit_status(*status, body.as_deref()) {
        self.router
            .record_lb_rate_limit(app_type_str, &provider.id, *retry_after_secs);
    }
}
```

（已知简化：整流器/media 重试子路径的 429 不重复记冷却——主路径已覆盖同一账号；如后续需要，`handle_rectifier_retry_failure` 是统一补挂点。）

wrapper `Ok(mut fr)` 分支（Task 5 的重映射之后）加成功重置：

```rust
// 该账号成功服务：清冷却 + 重置退避计数
self.router.record_lb_success(app_type_str, &fr.provider.id);
```

`provider_router.rs` 包装：

```rust
/// forwarder 挂点：限流冷却记录
pub(crate) fn record_lb_rate_limit(
    &self,
    app_type: &str,
    provider_id: &str,
    retry_after_secs: Option<u64>,
) {
    self.load_balancer
        .record_rate_limit(app_type, provider_id, retry_after_secs);
}

/// forwarder 挂点：成功服务，清冷却
pub(crate) fn record_lb_success(&self, app_type: &str, provider_id: &str) {
    self.load_balancer.record_success(app_type, provider_id);
}
```

- [ ] **Step 4: 跑测试** — `cargo test cooldown_tests` + `cargo test`（全库，重点 forwarder/handlers/error_mapper 受字段变更影响的测试）PASS
- [ ] **Step 5: fmt + clippy**

### Task 10: 前端并发上限 UI + 套餐预设

**Files:**
- Create: `src/config/codingPlanPresets.ts` + `src/config/codingPlanPresets.test.ts`
- Modify: `src/components/providers/forms/ProviderAdvancedConfig.tsx`（并发区）
- Modify: `src/components/providers/forms/ProviderForm.tsx`（状态 + meta 读写，模式照抄 `costMultiplier` :368-411/:1779-1784）
- Modify: `src/types/`（ProviderMeta TS 类型定义文件，实现时以 `grep -rn "costMultiplier" src/types/` 定位）
- Modify: `src/i18n/locales/{en,zh,zh-TW,ja}.json`

**Interfaces:**
- Consumes: Task 7 的 meta 契约 `{ loadBalance?: { maxConcurrentRequests?: number; maxConcurrentByModel?: Record<string, number> } }`
- Produces: `CODING_PLAN_CONCURRENCY_PRESETS`（id/labelKey/maxConcurrentRequests?/maxConcurrentByModel?/sourceKey）

- [ ] **Step 1: 写失败测试** — `src/config/codingPlanPresets.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { CODING_PLAN_CONCURRENCY_PRESETS } from "./codingPlanPresets";

describe("CODING_PLAN_CONCURRENCY_PRESETS", () => {
  it("Kimi 五档并发值为 1/1/2/2/8（设计决策 5，用户指定 2026-10-07）", () => {
    const kimi = CODING_PLAN_CONCURRENCY_PRESETS.filter((p) =>
      p.id.startsWith("kimi_"),
    );
    expect(kimi.map((p) => p.maxConcurrentRequests)).toEqual([1, 1, 2, 2, 8]);
  });

  it("GLM Max 默认 30，glm-5.3-flash 覆盖 50（控制台实测 2026-10-07）", () => {
    const glm = CODING_PLAN_CONCURRENCY_PRESETS.find((p) => p.id === "glm_max");
    expect(glm?.maxConcurrentRequests).toBe(30);
    expect(glm?.maxConcurrentByModel).toEqual({ "glm-5.3-flash": 50 });
  });

  it("MiMo 官网无数字，留空不限", () => {
    const mimo = CODING_PLAN_CONCURRENCY_PRESETS.find((p) => p.id === "mimo");
    expect(mimo?.maxConcurrentRequests).toBeUndefined();
  });

  it("每条预设都带来源标注 key（tooltip）", () => {
    for (const preset of CODING_PLAN_CONCURRENCY_PRESETS) {
      expect(preset.sourceKey).toMatch(/^codingPlanPresets\.sources\./);
    }
  });
});
```

- [ ] **Step 2: 确认失败** — `pnpm test:unit codingPlanPresets` FAIL（模块不存在）

- [ ] **Step 3: 实现** `src/config/codingPlanPresets.ts`：

```ts
/**
 * Coding Plan 套餐并发上限预设（设计决策 5，2026-10-07 确认）。
 *
 * 数值来源分三类，sourceKey 指向 i18n tooltip（用户可见的来源标注）：
 * - Kimi 五档：用户指定；其中 Max=8 与官网定价页口径（Agent concurrent
 *   tasks Max=4）不一致，tooltip 双标注，以用户指定为准。
 * - GLM：控制台实测（glm-5.3=30、glm-5.3-flash=50）。
 * - MiMo：官网无公开数字，留空（不限），运行时靠 429/403 冷却自适应。
 * 三家官方均为动态调整机制，预设仅作一键填充起点，可手改。
 */

export interface CodingPlanConcurrencyPreset {
  id: string;
  /** i18n label key（套餐档位名） */
  labelKey: string;
  /** 账号默认并发上限；undefined = 留空不限 */
  maxConcurrentRequests?: number;
  /** 按模型覆盖 */
  maxConcurrentByModel?: Record<string, number>;
  /** 来源标注 i18n key（tooltip） */
  sourceKey: string;
}

export const CODING_PLAN_CONCURRENCY_PRESETS: readonly CodingPlanConcurrencyPreset[] =
  [
    { id: "kimi_tier1", labelKey: "codingPlanPresets.kimiTier1", maxConcurrentRequests: 1, sourceKey: "codingPlanPresets.sources.userSpecified" },
    { id: "kimi_tier2", labelKey: "codingPlanPresets.kimiTier2", maxConcurrentRequests: 1, sourceKey: "codingPlanPresets.sources.userSpecified" },
    { id: "kimi_tier3", labelKey: "codingPlanPresets.kimiTier3", maxConcurrentRequests: 2, sourceKey: "codingPlanPresets.sources.userSpecified" },
    { id: "kimi_tier4", labelKey: "codingPlanPresets.kimiTier4", maxConcurrentRequests: 2, sourceKey: "codingPlanPresets.sources.userSpecified" },
    { id: "kimi_max", labelKey: "codingPlanPresets.kimiMax", maxConcurrentRequests: 8, sourceKey: "codingPlanPresets.sources.kimiMax" },
    { id: "glm_max", labelKey: "codingPlanPresets.glmMax", maxConcurrentRequests: 30, maxConcurrentByModel: { "glm-5.3-flash": 50 }, sourceKey: "codingPlanPresets.sources.glmConsole" },
    { id: "mimo", labelKey: "codingPlanPresets.mimo", sourceKey: "codingPlanPresets.sources.mimoUnspecified" },
  ] as const;
```

（Kimi 档位名设计未给官方命名，用"第 N 档/Max"中性命名；label 文案进 i18n，后续有官方名改 locale 即可。）

- [ ] **Step 4: 跑测试通过** — `pnpm test:unit codingPlanPresets` PASS

- [ ] **Step 5: i18n + UI**

TS ProviderMeta 类型加 `loadBalance?: { maxConcurrentRequests?: number; maxConcurrentByModel?: Record<string, number> }`（定位：`grep -rn "costMultiplier" src/types/`）。

i18n 四语言新增（zh 示例；en/zh-TW/ja 同步翻译）：

```json
"codingPlanPresets": {
  "placeholder": "套餐预设（一键填充）",
  "kimiTier1": "Kimi 第 1 档", "kimiTier2": "Kimi 第 2 档",
  "kimiTier3": "Kimi 第 3 档", "kimiTier4": "Kimi 第 4 档",
  "kimiMax": "Kimi Max", "glmMax": "GLM Coding Max", "mimo": "MiMo",
  "sources": {
    "userSpecified": "用户指定（2026-10-07）",
    "kimiMax": "用户指定 8（2026-10-07）；官网定价页口径 Max=4，以用户指定为准",
    "glmConsole": "控制台实测（2026-10-07）：glm-5.3=30，glm-5.3-flash=50",
    "mimoUnspecified": "官网无公开数字，留空不限；运行时按 429/403 冷却自适应"
  }
},
"providerAdvanced": {
  "loadBalance": {
    "title": "并发上限",
    "hint": "会话粘性负载均衡生效时，新会话优先分配到未达上限的账号；0 或留空 = 不限。粘性命中的会话不受上限约束。",
    "maxConcurrent": "账号默认并发上限",
    "maxConcurrentHint": "0 = 不限",
    "byModel": "按模型覆盖",
    "byModelHint": "针对指定出站模型名单独设限，优先于账号默认值",
    "model": "模型名",
    "limit": "上限",
    "addOverride": "添加模型覆盖",
    "preset": "套餐预设"
  }
}
```

`ProviderAdvancedConfig.tsx`：新增 `ProviderLoadBalanceConfig` 接口（`{ enabled: boolean; maxConcurrentRequests: string; byModel: Array<{ model: string; limit: string }> }`）+ props `loadBalanceConfig`/`onLoadBalanceConfigChange`，渲染一个与计费配置同款的折叠区：Switch（启用并发上限）→ 套餐 Select（`CODING_PLAN_CONCURRENCY_PRESETS`，选中即填充默认值与 byModel，tooltip 显示 `t(sourceKey)`）→ 数字 Input（账号默认）→ byModel 行编辑器（模型名 Input + 上限 Input + 删除按钮 + 添加按钮）。结构照抄同文件计费配置折叠区（:41-120）的样式类。

`ProviderForm.tsx`：
- state：`const [loadBalanceConfig, setLoadBalanceConfig] = useState(...)` 初始化自 `initialData?.meta?.loadBalance`（enabled = 有任一值；数字转字符串；byModel map 转行数组）
- 保存：:1779 附近 meta 组装处加 `loadBalance`（enabled 且有有效值时写入，否则 `undefined` 不写键）；byModel 行数组过滤空模型名/非法数字后转 Record
- 渲染：:2783 `<ProviderAdvancedConfig ...>` 传新 props

- [ ] **Step 6: 门禁** — `pnpm typecheck` && `pnpm test:unit` PASS；fmt（Prettier: `pnpm format`）

### Task 11: 集成测试（双 mock 上游）

**Files:**
- Create: `src-tauri/src/proxy/tests_load_balance.rs`
- Modify: `src-tauri/src/proxy/mod.rs`（`#[cfg(test)] mod tests_load_balance;`）

**Interfaces:**
- Consumes: Task 4/5/8/9 全部公开面（同模块树内可见私有项）

- [ ] **Step 1: 写测试**（先写全四个场景，/mock 上游起来前编译失败即 RED）：

```rust
//! 会话粘性负载均衡集成测试：双 mock 上游 + 真实 ProviderRouter/RequestForwarder

use super::forwarder::RequestForwarder;
use super::provider_router::ProviderRouter;
use super::types::ProxyStatus;
use super::{failover_switch::FailoverSwitchManager, ProxyResponse};
use crate::app_config::AppType;
use crate::database::Database;
use crate::provider::Provider;
use axum::http::HeaderMap;
use http::Extensions;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

/// 一条 canned 响应；responses 按序消费，耗尽后 500
struct MockResponse {
    status: u16,
    retry_after: Option<&'static str>,
    body: &'static str,
}

/// 极简 HTTP mock 上游：读完请求头+body，回 canned 响应。
/// 返回 (地址, 请求计数)。不引新 dev-dependency（手写 HTTP/1.1 应答）。
async fn spawn_mock_upstream(responses: Vec<MockResponse>) -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let responses = Arc::new(std::sync::Mutex::new(responses));
    let counter = count.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { break };
            let responses = responses.clone();
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                let mut acc = Vec::new();
                // 读到请求头结束（简化：不含 100-continue）
                loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 { return; }
                    acc.extend_from_slice(&buf[..n]);
                    if let Some(pos) = find_header_end(&acc) {
                        let content_length = extract_content_length(&acc[..pos]);
                        if acc.len() >= pos + 4 + content_length { break; }
                    }
                }
                counter.fetch_add(1, Ordering::SeqCst);
                let resp = {
                    let mut queue = responses.lock().unwrap();
                    queue.pop_first().map(|r| (r, ()))
                };
                let (status, retry_after, body) = match resp {
                    Some((r, _)) => (r.status, r.retry_after, r.body),
                    None => (500u16, None, "{\"error\":\"exhausted\"}"),
                };
                let mut head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                if let Some(ra) = retry_after {
                    head.push_str(&format!("Retry-After: {ra}\r\n"));
                }
                head.push_str("\r\n");
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(body.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, count)
}

fn find_header_end(acc: &[u8]) -> Option<usize> {
    acc.windows(4).position(|w| w == b"\r\n\r\n")
}

fn extract_content_length(headers: &[u8]) -> usize {
    let text = String::from_utf8_lossy(headers).to_lowercase();
    text.lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

struct TempHome(std::sync::Arc<()>); // 与 provider_router 测试同款 HOME 隔离，实现时照抄 TempHome 结构体

fn claude_provider(id: &str, base: &SocketAddr) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": format!("http://{base}"),
                "ANTHROPIC_AUTH_TOKEN": "sk-test"
            }
        }),
        None,
    )
}

fn make_forwarder(
    router: Arc<ProviderRouter>,
    session_id: &str,
    client_provided: bool,
) -> RequestForwarder {
    RequestForwarder::new(
        router,
        0,
        Arc::new(RwLock::new(ProxyStatus::default())),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::default(),
        Arc::default(),
        Arc::new(FailoverSwitchManager::new(Arc::new(Database::memory().unwrap()))),
        None,
        "a".to_string(),
        session_id.to_string(),
        client_provided,
        0,
        0,
        Default::default(),
        Default::default(),
        Default::default(),
        3,
    )
}

const OK_BODY: &str = "{\"id\":\"msg_1\",\"model\":\"claude-3-5-sonnet\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}],\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}";
```

四个测试（各 `#[tokio::test] #[serial_test::serial]`，需在 `Cargo.toml [dev-dependencies]` 补 `tokio = { workspace = true }`——若主依赖已有 tokio，dev 侧同样声明即可；`http`/`axum` 已是主依赖可直接用）：

```rust
// 场景①：多个会话分摊到两账号（粘性表映射两账号都有）
async fn multiple_sessions_distribute_across_accounts() {
    // 双上游各回 200；seed 两 provider + session 模式（复用 Task 4 的 seed 思路，
    // base_url 指向 mock）；对 s1/s2/s3 依次 select_for_session + forward，
    // 断言 mock A/B 请求计数非零且 s1、s2 粘性映射不同账号
}

// 场景②：同会话恒定同账号
async fn same_session_hits_same_upstream() {
    // 同一 session 转发两次，A 计数=2、B=0
}

// 场景③：杀一号后会话迁移（failover 重映射）
async fn failover_remaps_session_to_survivor() {
    // A 回 500（Retryable），B 回 200 → forward 成功；断言粘性映射从 A 改到 B；
    // 同会话再发一次（A 仍 500），断言首选已是 B、直接成功
}

// 场景④：429 后新会话避开冷却账号
async fn rate_limited_account_entering_cooldown_is_skipped() {
    // A 回 429（Retry-After: 120），B 回 200 → 会话 s1 failover 成功；
    // 新会话 s2 选号断言首选 B（A 冷却中）
}
```

（①②③④的函数体按上述注释展开为完整断言代码——执行时以 Task 4/5/8/9 的实际公开面书写，mock 计数与 `router.load_balancer()` 状态为断言依据。）

- [ ] **Step 2: 确认 RED** — `cargo test tests_load_balance` 编译失败/断言失败
- [ ] **Step 3: 调试至 PASS** — `cargo test tests_load_balance` 四场景全 PASS（允许迭代修 mock/接线 bug，遵循 systematic-debugging）
- [ ] **Step 4: fmt + clippy**

### Task 12: 阶段 2 门禁 + 设计文档同步 + 提交

- [ ] **Step 1: 全量门禁** — `cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test`；`pnpm typecheck && pnpm test:unit && pnpm format:check`。全绿
- [ ] **Step 2: 设计文档同步** — `docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md` 末尾追加"## 8. 本地 fork 校验与实现偏差（2026-10-08）"，内容 = 本计划"与设计的偏差记录"6 条 + 落地文件清单
- [ ] **Step 3: Commit（显式路径）**

```bash
git add src-tauri/src/proxy/load_balance.rs src-tauri/src/proxy/tests_load_balance.rs \
  src-tauri/src/proxy/mod.rs src-tauri/src/proxy/provider_router.rs \
  src-tauri/src/proxy/forwarder.rs src-tauri/src/proxy/error.rs \
  src-tauri/src/provider.rs src-tauri/Cargo.toml \
  src/config/codingPlanPresets.ts src/config/codingPlanPresets.test.ts \
  src/components/providers/forms/ProviderAdvancedConfig.tsx \
  src/components/providers/forms/ProviderForm.tsx \
  src/i18n/locales/en.json src/i18n/locales/zh.json \
  src/i18n/locales/zh-TW.json src/i18n/locales/ja.json \
  docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md
# 及 src/types/ 下被修改的文件（实现时确认路径）
git commit -m "feat(proxy): 会话粘性负载均衡阶段 2（并发上限+限流冷却+预设 UI）

InFlight 在途计数（RAII 随响应流结束递减）；两级并发上限（meta.loadBalance：
账号默认 + 按模型覆盖）；429/403 限流冷却（Retry-After 优先，退避 60s→2m→5m
封顶 10m）；选号饱和跳过/全饱和最少在途；供应商编辑页并发上限区 + Kimi/GLM/MiMo
套餐预设（i18n×4）；双 mock 上游集成测试四场景。

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review 结论

- **设计覆盖**：4.1 粘性表(T3)/在途计数与上限与选号算法(T8)/冷却(T9)/路由接入与 off 一致(T4)/重映射(T5)；4.2 数据流=T4+T8 wrapper/迭代；4.3 配置三件套=T1/T2/T7/T10；5 边界五条→T3(过期/容量)、T4(熔断出池重选)、T8(计数 Drop)、T9(403 识别)；6 测试四类→各任务单测+T11 四场景+门禁；7 分期→T6/T12 两 commit。
- **已知取舍**：Gemini 在选号时刻模型名未定（`with_model_from_uri` 在 ctx 之后），模型级覆盖对 Gemini 不生效、账号默认上限生效——在 T8 实现处以注释固化；整流器重试子路径不重复记冷却（主路径覆盖）；HTTP-date 格式 Retry-After 不解析（回退退避阶梯）。
- **类型一致性**：`select_providers_for_session` 在 T8 增参 `request_model`，T4 测试同步改；`InFlightHandle.detach`/`attach_counters` 名字在 T8 三处引用一致；`record_lb_success` 在 T9 引入（T5 的 wrapper 后续插入一行）。
