# Subagent 跨供应商路由 — 设计文档

- 日期：2026-09-06
- 状态：已与需求方对齐，待实现
- 目标仓库：cc-switch（upstream PR）
- 范围：Claude（Claude Code）应用 · 代理接管模式

## 1. 背景与问题

cc-switch 目前对每个应用严格「单供应商独占」：切换供应商时，live 配置整体被目标供应商的
`ANTHROPIC_BASE_URL` + auth 覆盖（`services/provider/live.rs` 的 `write_live_snapshot`）。
现有的「模型映射」（`ANTHROPIC_MODEL` / `ANTHROPIC_DEFAULT_*_MODEL` /
`CLAUDE_CODE_SUBAGENT_MODEL`）只能在**同一个供应商内**改模型名。

用户的真实诉求是**按角色分流到不同供应商**：主对话用 A 家旗舰（贵、能力强，负责计划与审核），
subagent 用 B 家平价模型（便宜、够用，负责干活）。当前架构下无法实现。

### 关键技术约束

Claude Code 的 `settings.json` 只有一个 `ANTHROPIC_BASE_URL`，官方不支持按模型指定不同端点。
因此**不经代理就无法实现真·跨供应商**。而 cc-switch 的代理接管模式下，所有请求（含 subagent
请求）都经过本地代理，代理已能识别请求的模型角色并改写模型名（`proxy/model_mapper.rs`），
且具备按供应商各自凭据转发的基础设施（`proxy/provider_router.rs` + `proxy/forwarder.rs`，
故障转移队列已在用）。跨供应商路由是对该架构的自然延伸。

## 2. 目标 / 非目标

**目标**

1. Claude 应用在代理接管模式下，支持一条「subagent 路由」规则：匹配 subagent 的请求转发到
   指定的另一家供应商（B），其余请求仍走当前供应商（A）。
2. B 失败或熔断时自动回退到 A，恢复后自动回到 B。
3. 用户在 UI 上选好 B 和模型名即可，Claude Code 端零手工配置。
4. 规则为设备级持久配置，切换主供应商不影响。

**非目标（本期不做）**

- 其他角色档位（haiku / sonnet / opus / fable）的跨供应商路由（已确认 YAGNI，留作演进方向）。
- 按 subagent 名字的细粒度路由。
- Codex / Gemini / Claude Desktop / 其他应用的跨供应商路由。
- 不依赖代理的实现路径（受单 base_url 限制，不存在）。

## 3. 已确认的关键决策

| 决策点 | 结论 | 理由 |
| --- | --- | --- |
| 实现前提 | 仅代理接管模式下生效 | Claude Code 单 base_url 硬限制；代理基础设施（模型角色识别、按供应商凭据转发、熔断）已存在 |
| 路由粒度 | 只做 subagent 一条规则 | 最贴合省钱场景；PR 最小、最容易被上游接受；结构可平滑扩展为通用「角色 → 供应商」表 |
| 实现方案 | 候选列表改道（方案 1） | 在 `RequestContext::new` 组装候选列表处改写 `[A]` → `[B, A]`，重试/熔断/整流/模型映射机制全部免费复用；不在 700+ 行转发循环里加分支（方案 2），也不引入「组合供应商」新类型（方案 3） |

## 4. 数据模型与存储

### 4.1 结构体

`proxy/types.rs` 的 `AppProxyConfig` 新增：

```rust
/// Subagent 跨供应商路由规则（仅 claude 应用生效）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRoute {
    /// 目标供应商 B 的 id（claude 应用下的供应商）
    pub provider_id: String,
    /// 发给 B 的模型名；None = 透传请求中的模型名
    pub model: Option<String>,
}

pub subagent_route: Option<SubagentRoute>,
```

serde 带 `#[serde(default)]`，旧配置反序列化兼容。

### 4.2 数据库

`proxy_config` 表（`database/schema.rs`，按应用一行的平铺列结构）新增
`subagent_route TEXT NULL` 列，存 JSON。迁移遵循现有 `apply_schema_migrations()` +
`has_column` 惯例执行 `ALTER TABLE ... ADD COLUMN`。

### 4.3 语义

- 应用级（仅 `claude`）、设备级：存于 DB `proxy_config`，切换当前供应商不影响规则。
- 仅代理路径生效；未开启接管模式时规则不产生任何作用（UI 给引导提示）。

## 5. 代理路由决策（核心逻辑）

新建 `proxy/subagent_route.rs`，决策抽成纯函数以便单测：

```
输入：候选列表 providers（select_providers 结果）、app_config.subagent_route、
      原始请求模型名 request_model、供应商解析器（查 B）
输出：(新候选列表, Option<String> 模型名覆盖)
```

1. **识别基准（生效 subagent 模型名，与第 6 节注入同源）**：若供应商 A 的
   `settings_config.env` 显式设置了 `CLAUDE_CODE_SUBAGENT_MODEL`，以其为准；否则若规则
   启用且 `route.model` 非空，取 `route.model`（自动注入写入 live 的正是这个值，Claude
   Code 发出的 subagent 请求模型名与之一致，代理从 DB 读 provider 也能完成识别）。
   两侧均经 `strip_one_m_suffix_for_upstream` 归一后比较（复用 `model_mapper` 已有的
   `[1M]` 后缀剥离）。
2. **前置条件**：规则启用 ∧ 识别基准存在（A 显式设置 ∨ `route.model` 非空）∧ B 存在于
   DB ∧ B ≠ A ∧ `app_type == "claude"`。任一不满足 → 原候选列表原样返回。
3. **改写**：候选列表变为 `[B] + 原 candidates（去重）`。在 `RequestContext::new`
   （`proxy/handler_context.rs:134` 附近）取得 providers 后执行。
4. **模型名覆盖**：识别在前、改写在后（顺序不可反）。若 `route.model` 为 `Some`，由
   handler 在创建上下文后将 `body.model` 覆盖为该值；为 `None` 则透传。转发到 B 时，
   现有 `apply_model_mapping`（`forwarder.rs:1253`）自动应用 **B 自己的**模型映射，
   无新逻辑。

## 6. live 配置自动注入

闭环关键——Claude Code 端零手工配置：

- 条件：接管模式开启 ∧ 规则启用 ∧ `route.model` 非空 ∧ 当前供应商**未显式**设置
  `CLAUDE_CODE_SUBAGENT_MODEL`。
- 动作：生成 live `settings.json` 时（`services/provider/live.rs` 生效配置构建/接管写入
  路径，与现有 env 注入同一区域）注入 `CLAUDE_CODE_SUBAGENT_MODEL = route.model`。
- 用户显式配置优先于注入；不满足注入条件时不注入（此时若用户也没手动设置，subagent
  请求与主请求模型名相同、无法区分，规则不生效——UI 需提示这一状态）。

## 7. 熔断与回退（零新代码）

- 候选列表 ≥ 2 时，现有 `bypass_circuit_breaker`（仅单供应商时跳过熔断）不再触发，
  B 与 A 均纳入熔断器管理。
- B 连续失败 → 熔断 → subagent 请求自动回退 A；冷却恢复后自动回到 B。
- `max_retries` 上限照常约束整条候选链。
- 与自动故障转移叠加：匹配请求 → `[B] + 故障转移队列（去 B）`；不匹配请求 → 原队列不变。

## 8. UI 与 i18n

- 新组件 `src/components/proxy/SubagentRouteConfigPanel.tsx`，置于 Claude 应用代理面板，
  与 `AutoFailoverConfigPanel` 并列（`src/components/proxy/ProxyPanel.tsx`）。
- 控件：启用开关 + 目标供应商下拉（claude 供应商列表，排除当前供应商）+ 模型名输入
  （选定 B 后从其 `settings_config.env` 的 `ANTHROPIC_MODEL` /
  `CLAUDE_CODE_SUBAGENT_MODEL` 预填）。
- 状态提示：
  - 未开启代理接管 → 区块给引导提示（功能不生效）。
  - 规则指向的供应商已删除 → 显示「规则已失效」警告。
  - 规则启用但既无 `route.model`、A 也未显式设置 subagent env → 提示「当前无法区分
    subagent 请求，请填写模型名」。
- 数据读写复用现有 proxy_config 的 get/update Tauri 命令，不新增 command。
- i18n：en / zh / zh-TW / ja 四语言补齐 key。

## 9. 边界与错误处理

| 场景 | 行为 |
| --- | --- |
| B 被删除 | 运行时容错：按无规则处理并 `log::warn!`；UI 显示失效警告，不自动清除规则 |
| B == 当前供应商 | 规则等效关闭，候选列表不变 |
| A 未设置 subagent env 且注入条件不满足 | 规则静默不生效（请求无法与主请求区分）；UI 提示 |
| 规则 JSON 损坏 | 反序列化失败按 `None` 处理 |
| codex / gemini / claude-desktop | 路由决策入口对 `app_type != "claude"` 直接短路，路径零改动 |

## 10. 测试与验收

**Rust 单测**

- 决策纯函数：匹配 / 不匹配 / 规则关闭 / B 缺失 / B == current / `[1M]` 后缀归一 /
  与故障转移叠加 / 非 claude 应用短路。
- DAO：`subagent_route` 序列化往返、旧库迁移后可读写。
- live 注入：注入条件各分支（接管开/关、规则开/关、model 空/非空、显式配置优先）。

**前端**

- `SubagentRouteConfigPanel` 组件测试（开关、下拉排除当前供应商、预填、失效警告）。
- i18n key 完整性检查。

**手工验收**

1. 配置 A（旗舰）+ B（平价）双供应商，开启接管与 subagent 路由。
2. Claude Code 主对话与 Task 子 agent 同时跑：代理日志与用量统计显示主对话落 A、
   subagent 落 B。
3. 停用 B 的凭据使其持续失败：subagent 请求自动回退 A；恢复 B 后自动回到 B。
4. 切换主供应商 A → C：subagent 路由仍指向 B。
5. 关闭接管模式：一切回落到普通单供应商行为。

## 11. 未来演进（不在本期）

- 决策结构从单条 `subagent_route` 泛化为「角色 → (供应商, 模型)」路由表，开放五档角色。
- 按 subagent 名字的细粒度路由。
- 其他应用（Codex 等）的等价能力评估。
