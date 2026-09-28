# RFC 0002: 跨 codex 档位保留 inert `[model_providers.*]` 表

| 字段 | 值 |
| --- | --- |
| 状态 | Accepted（实现完成，待 cherry-pick） |
| 作者 | cc-switch 维护者 + 2026-09-28 接力会话 |
| 相关 commit | `f4970ee9`（grok-4.6 补登，作为本文触发的 SSOT 对账证据） |
| 适用版本 | codex 0.158+（实测 0.158 必现；更新版未做回归） |

## TL;DR

Codex 0.158 在加载 session 时**严格校验** session 元数据里的
`model_provider` 在 live config 里必须存在对应 `[model_providers.<id>]` 表。
cc-switch 切到 `codex-official` / `default` 这类内置档时，会写一份**最小化**的
live config（顶层 `model = "gpt-reserve"`、无 `model_provider`、无任何
`[model_providers.*]` 表）。这导致：

- 旧 session（`session_meta.payload.model_provider = "custom"`）重新打开时
  Codex 报错 `Model provider 'custom' not found.`
- 用户必须手工 patch `~/.codex/config.toml`，下次切档又会再次擦掉。

**修复**：cc-switch 写 live config 时**惰性合并** DB 里其它 codex provider 的
`[model_providers.*]` 表（旧 provider id ≠ 当前 provider id）。新会话仍以当前
provider 为准；旧 session 元数据 `model_provider="custom"` 永远能解析到表。

## 1. 背景

### 1.1 Codex 0.158 的会话恢复校验

Codex CLI 在加载 `~/.codex/sessions/<date>/rollout-<...>.jsonl` 时，第一步是读
`session_meta.payload.model_provider`，再去 live `config.toml` 找
`[model_providers.<model_provider>]`。缺表则整段 config 拒绝加载：

```
ChatGPT 无法加载 config.toml，因此此对话串无法继续。
Model provider `custom` not found。
```

这条 401/拒绝 不区分 "session 创建后 provider 表被删" 还是 "用户切换到内置档"——
只要 live config 缺表就拒绝。

### 1.2 cc-switch 切档时的写盘语义

`src-tauri/src/codex_config.rs::write_codex_live_for_provider` → `plan_codex_live_write`：

- `category == "official"`：走 `migrate_stale_reserved_provider_tables` +
  `backfill_codex_custom_provider_names` + `inject_codex_unified_session_bucket`
  → 写一份 `unified_official_config`。该路径**完全不知道**当前激活的第三方
  provider 在 DB 里还有什么 `[model_providers.<id>]` 表。
- `category == Some(other)`（第三方）：走 `prepare_codex_provider_live_config`
  → 写一份以 `[model_providers.custom]`（或该 provider 自己的 id）为中心的内容。
  同样**完全不知道**其它第三方 provider 的表。

两条路径都把 "其它 provider 的 inert 表" 当垃圾扔掉。

### 1.3 用户实际撞到的现象（2026-09-28）

| 时刻 | 事件 |
|---|---|
| 03:30 | 用户在 Kaixuan（kxpms-gateway，model=`kx-claude-opus-5`）创建 session |
| 03:36 | cc-switch 切到 `codex-official`，live config 被最小化覆盖 |
| 03:37 | 重开 03:30 session → "Model provider `custom` not found" |
| 03:38 | 人工 patch live config 加回 `[model_providers.custom]` 表 |
| 12:19 | 用户在 Kaixuan（kxpms-gateway，model=`gpt-6-sol`）创建新 session |
| 12:30+ | cc-switch 又切了一次档到 `codex-official` |
| 12:42 | 重开 12:19 session → **同样的错误** |

两次问题的根因完全一致，但**人工 fix 不是系统修复**——只要 cc-switch 继续
按"当前 provider 决定 live config"写盘，问题就周期性复发。

### 1.4 这是 cc-switch 设计上的"切档 vs 合并"张力

| 维度 | Codex 假设 | cc-switch 假设 |
|---|---|---|
| live config 内容 | 历史上所有 provider 的并集 | 当前 provider 的最小集 |
| session 元数据 | `model_provider` 写盘后不变 | 跟随当前 provider 重写（部分场景） |
| 切档语义 | 不能擦任何 provider 表 | 当前 provider 接管整张文件 |
| 历史 session | 必须能 resume | 由 Codex 自动迁移（实测：不会） |

两边对"历史 session 兼容性"的预期冲突。RFC 0001 解决了
`[model_providers.custom]` 下虚假 retry 配置的问题；本 RFC 解决**反向**问题：
live config 缺失 provider 表导致旧 session 永远卡死。

## 2. 设计

### 2.1 目标

> cc-switch 每次写 codex live config 时，把 DB 里**所有其它 codex provider**的
> `[model_providers.*]` 表惰性合并进来。新 provider 表以当前 active provider 为
> 准；inert 表保留 inert 状态（仅作 session 恢复用途，不影响新流量路由）。

### 2.2 不做什么

- **不修改 session 元数据**——保留历史 session 的 `model_provider` 字段原样
- **不删除 DB 里的 inert provider**——它们继续存在，仅在 live config 留个 inert 影子
- **不重写 Codex 上游行为**——Codex 0.158+ 仍按 session 元数据找表
- **不污染 `auth.json`**——inert 表自带 `experimental_bearer_token`，
  走 provider-scoped 凭据而非 auth.json（Codex 0.49+ 行为）

### 2.3 inert 表保留的内容

每个 inert provider 在 live config 里落一个 `[model_providers.<id>]` 表，包含：

| 字段 | 保留？ | 理由 |
|---|---|---|
| `name` | ✅ | Codex 启动校验必填 |
| `base_url` | ✅ | Codex 启动校验必填 |
| `wire_api` | ✅ | Codex 启动校验必填 |
| `requires_openai_auth` | ✅ | 决定 auth 路径 |
| `experimental_bearer_token` | ✅ | provider-scoped 凭据，旧 session resume 用 |
| `http_headers` / `env_http_headers` | ✅ | 用户可能设了 custom header |
| `env_key` | ⚠️ 看情况 | 若 `env_key` 指向的环境变量未导出，Codex 启动会卡；默认保留，由 Codex 报错 |
| `request_max_retries` 等 | ✅ inert | Codex 0.158 在 `[model_providers.*]` 层不消费，是死 key（参见 RFC 0001） |

### 2.4 inert 表的"被放弃"条件

满足任一条件，该 inert provider **不**合并到 live config：

1. inert provider 的 `id` 与当前 active provider 的 `id` 相同（同 id 冲突）
2. inert provider 在 DB 中被标记 `enabled=false`（未来扩展，本 RFC 不实现）
3. inert provider 的 stored `config.toml` 解析失败（坏数据不污染 live）
4. inert provider 的 stored `config.toml` 不含任何 `[model_providers.*]` 表
   （说明该 provider 是 `codex-official` 类型的内置档，本身不需要表）

### 2.5 实现 hook 点

`src-tauri/src/codex_config.rs::plan_codex_live_write` 返回
`CodexLiveWritePlan { config_text, ... }` 之前，调用新函数
`merge_inert_provider_tables_into_live_config`：

```rust
fn plan_codex_live_write(
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
    preserve_official_login: bool,
) -> Result<CodexLiveWritePlan, AppError> {
    // ... existing logic produces config_text ...

    // Plan A：惰性合并 inert provider 表
    let live_config = config_text.unwrap_or("");
    let merged = merge_inert_provider_tables_into_live_config(
        live_config,
        category,
        auth,
    )?;
    Ok(CodexLiveWritePlan {
        write_full_auth,
        config_text: Some(merged),
        remove_auth_file,
    })
}
```

### 2.6 `merge_inert_provider_tables_into_live_config` 伪码

```rust
fn merge_inert_provider_tables_into_live_config(
    live_config: &str,
    active_category: Option<&str>,
    active_auth: &Value,
) -> Result<String, AppError> {
    // 1. 解析 active provider 的 id（顶层 model_provider），无则默认 "openai"
    let active_provider_id = parse_active_provider_id(live_config);  // "openai" | "custom" | ...

    // 2. 枚举 DB 里所有 codex provider（除当前 active 的）
    let all_codex_providers = state.db.get_all_providers("codex")?;
    let inert_providers: Vec<_> = all_codex_providers
        .into_iter()
        .filter(|p| p.id != active_provider_id_in_db)
        .collect();

    // 3. 从每个 inert provider 的 stored config.toml 提取 [model_providers.*] 表
    let mut inert_tables: Vec<(String, toml_edit::Item)> = Vec::new();
    for p in inert_providers {
        let stored_cfg = p.settings_config["config"].as_str().unwrap_or("");
        let Ok(doc) = stored_cfg.parse::<DocumentMut>() else { continue };
        let Some(mp_table) = doc.get("model_providers").and_then(|t| t.as_table_like()) else { continue };
        for (id, item) in mp_table.iter() {
            if id == active_provider_id { continue; }  // 跳过与 active 同 id 的
            inert_tables.push((id.to_string(), item.clone()));
        }
    }

    // 4. 把 inert 表合并进 live config
    let mut live_doc = live_config.parse::<DocumentMut>()?;
    let live_mp = live_doc.entry("model_providers")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let live_mp_table = live_mp.as_table_like_mut().unwrap();
    for (id, item) in inert_tables {
        if live_mp_table.contains_key(&id) { continue; }  // live 已有则不覆盖
        live_mp_table.insert(&id, item);
    }
    Ok(live_doc.to_string())
}
```

注意：

- `merge_inert_provider_tables_into_live_config` 只在 `write_codex_live_for_provider`
  这一条主路径被调用；proxy takeover / backup restore / runtime inject 等
  旁路不进（它们各自的"恢复正确性"由各自的 invariant 保证）。
- 用 `toml_edit` 而不是 `toml`，保留注释与格式。
- 错误处理：DB 读失败 inert provider → log warn，跳过该 provider，不阻断切档。

### 2.7 测试计划

#### 2.7.1 单元测试（`src-tauri/src/codex_config.rs::tests`）

| 测试 | 断言 |
|---|---|
| `merge_inert_provider_tables_preserves_custom_when_switching_to_official` | live config 是 official（无 `[model_providers.*]`），DB 里有一个 `kxpms-gateway` 持 `[model_providers.custom]`。merge 后 live 多了 inert `[model_providers.custom]`。 |
| `merge_inert_provider_tables_skips_active_provider` | 当前 active = `kxpms-gateway`，DB 里另一个 provider id 也叫 `custom`（同 id）。merge 后不重复。 |
| `merge_inert_provider_tables_handles_invalid_db_config` | DB 中某 inert provider 的 config 解析失败 → merge 跳过该 provider，其它 inert 正常合并，merge 不返回 Err。 |
| `merge_inert_provider_tables_idempotent` | 同一 live config merge 两次，第二次是 no-op。 |

#### 2.7.2 集成测试（`src-tauri/src/services/provider/mod.rs::tests`）

| 测试 | 断言 |
|---|---|
| `write_codex_live_for_official_preserves_inert_custom_table_for_resume` | 在 codex-official 上 write → live config 同时含官方档（`model = "gpt-reserve"`）和 inert `[model_providers.custom]`；下次 Codex 加载 `model_provider="custom"` 的 session 不再报 "Model provider 'custom' not found"。 |
| `write_codex_live_for_third_party_preserves_inert_other_third_party_table` | 当前激活 `kxpms-gateway`，DB 里还有 `local-gateway-8782`（`[model_providers.local]`）。write 后 live config 同时含两个 provider 表。 |

#### 2.7.3 端到端手测（不写进 CI，留作 verifier 文档）

1. 启动 cc-switch，确保 DB 里有 `kxpms-gateway` + `codex-official` 两个 codex provider
2. 切到 `kxpms-gateway`，启动 Codex 执行 `codex exec -m kx-claude-opus-5 '...'`，
   记下 session id
3. 切到 `codex-official`
4. 用 `codex exec resume <session-id>` 恢复 session → 不再报
   "Model provider `custom' not found"
5. 切回 `kxpms-gateway` → 旧 session 仍能正常 resume

## 3. 风险与限制

### 3.1 inert 表的 token 暴露

inert provider 的 `experimental_bearer_token` 会被永久留在 live config 里。
用户如果**撤回了**该 token（外部 gateway 上 disable 了），inert 表仍持有
旧 token，Codex 启动校验通过但**调用时**会失败。这是惰性保留的固有 trade-off：

- 优点：用户撤回前/后的 session 都能 resume
- 缺点：inert token 一直存在，用户需要主动重写 cc-switch preset 来清理

**缓解**：本 RFC 不实现自动清理 inert token；文档明确告知用户：删 provider 时
应同时清理对应 inert 表（已通过 plan_codex_react 上游路径处理）。

### 3.2 inert 表可能与新 official 配置冲突

切到 official 时 cc-switch 会注入
`[model_providers.custom]` 字段（如 `requires_openai_auth = true` 等用于 OAuth
fallback）。inert 表 merge 后可能与这个新注入冲突。

**缓解**：merge 函数不覆盖 live_mp_table 中已存在的 key（"live 已有则不覆盖"
那条规则）。注入发生在 inert merge 之前，所以 inert 表不会覆盖官方注入。

### 3.3 Codex 上游 0.149+ 校验所有 provider 表

Codex 0.149+ 在加载时**严格校验每个** `[model_providers.*]` 表。inert 表如果
缺 `name` 或 `base_url`，整个 config 拒绝加载。

**缓解**：merge 函数只合并那些在 stored config 中**完整存在**的表；
解析失败或字段缺失的 inert provider 直接跳过。

### 3.4 inert 表占用 live config 行数

每个 inert provider 大约 5-8 行（带注释）。DB 里有 N 个 inert provider，
live config 多 ~5N 行。N 通常 < 5，可接受。

### 3.5 多次切档累积 inert 表

理论上 inert 表可能因为反复切档累积（如果 Codex 0.149+ 校验通过）。但 inert
merge 是按"DB 当前状态"合并——如果 DB 里那个 provider 被删了，下次切档 inert
表也不会再 merge 进去。所以 inert 表**不会无限增长**。

## 4. 备选方案（已否决）

### 方案 B：切档时重写 session 元数据

切到 official 时把 `session_meta.payload.model_provider = "custom"` 重写为
`"openai"`。**否决原因**：破坏性，用户历史数据被改；且需要 backup/restore
机制（handoff docs 里 `codex-official-history-unify-v1/` 模式），超出本 RFC 范围。

### 方案 C：等 Codex 上游修

Codex 0.159+ 可能放宽 "missing provider table" 的校验。**否决原因**：
等待上游无期限，且用户当前已被阻塞。

### 方案 D：只写文档 + 兜底脚本

只更新 `docs/guides/codex-self-hosted-gateway-zh.md`，并提供一段 shell 让用户
切档后自己跑。**否决原因**：用户已经撞过两次，每次都是紧急 debug 时刻；
靠 shell 救不如系统修复。

## 5. 实施步骤

| 步骤 | 内容 | 估时 |
|---|---|---|
| 1 | 实现 `merge_inert_provider_tables_into_live_config` 函数 | 1.5h |
| 2 | 在 `plan_codex_live_write` 主路径末尾调用 | 0.5h |
| 3 | 加 4 条单元测试 + 2 条集成测试 | 1h |
| 4 | `cargo test --lib codex_config::tests::merge*` 全绿 | 0.5h |
| 5 | `cargo test --lib --features integration` 全绿 | 0.5h |
| 6 | 手动 e2e 复现 §2.7.3 步骤 | 0.5h |
| 7 | RFC 标 "Accepted"，commit + push + cherry-pick | 0.5h |
| **合计** | | **~5h** |

## 6. 后续工作（不在本 RFC 范围内，但相关）

- **自动清理 inert token**：当用户删除某个 provider 时，自动从 live config
  移除对应 inert 表（与 `codex-official-history-unify-restore-v1` 配合）。
- **inert 表的可观测性**：在 `~/.cc-switch/logs/codex.log` 记录 inert 表
  每次合并 / 跳过，便于排查 "session resume 失败" 类问题。
- **Codex 上游追踪**：等 Codex 0.159+ 放宽校验后，本 RFC 的 inert merge
  可改为惰性；保留 inert 表作为快速 fallback。

## 7. 附录

### 7.1 关键文件位置

- `src-tauri/src/codex_config.rs::plan_codex_live_write`（行 3838）— 主路径
- `src-tauri/src/codex_config.rs::write_codex_live_for_provider`（行 3996）— 入口
- `src-tauri/src/database/*::get_all_providers(app)` — DB 查询
- `src-tauri/src/services/provider/mod.rs::sync_all_enabled`（行 71）— provider 同步

### 7.2 证据

- `~/.codex/config.toml` 在两次修复前后的 diff（保留在
  `~/.cc-switch/backups/gateway-codex-20260928-*/verification/`）。
- `~/.codex/sessions/2026/09/28/rollout-2026-09-28T12-19-10-01a0e63c-...jsonl`
  的 `session_meta.payload.model_provider = "custom"`。
- Codex 0.158 binary 字符串搜索：`remote_compaction_v2`、
  `responses_websockets_v2`、`should_use_remote_compact_task` 等 token 出现
  在 `/Applications/ChatGPT.app/.../codex` 中，证实版本 = 0.158.0-alpha.2.1。

### 7.3 与 RFC 0001 的关系

| 维度 | RFC 0001 | RFC 0002（本文） |
| --- | --- | --- |
| 主题 | `[model_providers.custom]` 内的死 key（retry/timeout） | live config 整体缺 `[model_providers.*]` 表 |
| 触因 | Codex 0.158 不消费 `[model_providers.*]` 下的 retry 字段 | Codex 0.158 严格校验 session 元数据 `model_provider` 必须有表 |
| 修复方向 | 删除虚假键 | 惰性合并 inert 表 |
| 互斥？ | 不互斥——本文 §3.2 已说明 inert merge 不会覆盖 RFC 0001 撤回的死 key |