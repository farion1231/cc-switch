# 精简用量仪表盘设计

## 目标

将 CC Switch fork 收缩为一个本地用量采集与展示应用。它管理多个独立 Provider，并按两种计费语义展示数据：

- **订阅 Provider**：展示 5 小时和 7 天额度的剩余量、重置时间，以及 Provider 支持时的手动重置次数。
- **按量 Provider**：展示选定时间范围内由本地代理逐请求记录的真实费用与 Token。
- **全部 Provider**：展示可追溯的 Token 使用量。

前端可将同一产品的多个 Provider 归在一个产品组中，但后端账单和事件保持独立。

## 非目标

首个版本不提供以下能力：

- Provider 快速切换、自动故障转移或预设商城；
- OpenClaw、MCP、Skills、终端启动、完整会话管理或云同步；
- 仅依赖 `app_type`、模型名称或估算比例的费用和 Token 去重。

## 领域模型

### Provider

`Provider` 是一个可计费的独立账户或路由，不是客户端应用或产品名称。

- `billingKind`：`subscription` 或 `metered`。
- `productGroupID`：用于前端聚合，例如 `codex`、`claude`、`kimi`。
- `tokenSource`：`proxy`、`sessionLog` 或两者的已验证组合。
- `quotaSource`：订阅 Provider 的额度查询脚本或 API；按量 Provider 为 `nil`。

同一产品同时有官方订阅和第三方 API 路由时，必须创建两个 Provider。例如：`codex-official-subscription` 与 `openrouter-codex-api`。

### 用量事件与额度快照

`UsageEvent` 是不可变的、逐请求的标准化记录，至少包含：

- `eventID`、`source`、`providerID`、`productGroupID`、时间戳和模型；
- 输入、输出、Cache Read、Cache Creation Token；
- API Provider 的真实分项费用与总费用；
- 可用时的请求 ID、会话 ID 或上游关联 ID。

`QuotaSnapshot` 独立保存订阅 Provider 的 5 小时/7 天窗口、重置时间和可选剩余手动重置次数。额度快照绝不能伪装为每日 Token 或费用事件。

## 数据采集

### 轻量本地代理

保留 CC Switch 的请求转发、协议适配、Token/费用解析和 SQLite 请求日志。代理按预先配置的静态 `RouteBinding` 将客户端请求转发到指定 Provider；不提供切换 UI 或故障转移。

代理日志是按量 Provider 费用的权威来源。它记录实际发生的请求，保留上游返回的 Token 与费用字段，不能用用户设置的单价覆盖真实账单。

### 订阅与会话来源

订阅 Provider 周期性获取额度快照。Claude、Codex 等本地 session 解析可补充 Token，但只在它明确对应的 Provider 上生效；不承担 API Provider 的真实费用计算。

跨来源去重只允许使用稳定的请求/会话关联 ID，或已验证的一对一映射。模型名、`app_type`、相同 Token 数或相近时间戳均不足以证明重复；无法证明时必须保留来源边界，而不是静默扣除费用。

## 前端展示

产品组是展示层概念：

1. 产品组头部显示该产品下的 Token 摘要，并标明数据来源和统计时间范围。
2. 订阅卡展示两个额度窗口、重置时间和可选重置次数。
3. 按量卡展示可选时间范围（今天、7 天、30 天和自定义范围）的真实费用、Token、模型与请求明细。
4. 当配额或数据源读取失败时，保留最后成功值并标记“数据过期/读取失败”；不影响其他 Provider 的费用。

前端不得将订阅额度、API 真实费用或不同 Provider 的费用混成一个未标注的总数。

## 最小保留模块

- Provider 配置与静态路由绑定；
- 轻量代理、请求日志、Token/费用标准化和 SQLite DAO；
- 订阅额度查询；
- 必需的 Claude/Codex session Token 解析；
- 用量仪表盘及其产品分组视图。

## 验证标准

- 代理请求生成一条带真实费用的 `UsageEvent`，并能按 Provider 和时间范围查询。
- 同一产品下的订阅与按量 Provider 分别展示，且不会基于 `app_type` 进行费用扣除。
- 有关联 ID 的 proxy/session 重复记录只计一次；没有关联证据的记录保持独立并显示来源。
- 额度 API 失败不会清空历史额度，也不会改变按量费用。
- 订阅额度快照不会出现在每日 Token 或费用图表中。
