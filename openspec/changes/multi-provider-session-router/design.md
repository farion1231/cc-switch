## Context

cc-switch 是一个 Tauri 2 桌面应用，管理 Claude Code/Codex/Gemini CLI 的 API provider 配置。
它通过本地代理（127.0.0.1:15721）接管所有 API 请求，当前支持 app 级 provider 路由和故障转移。
所有终端共享同一个 provider，无法实现多终端并行。

现有架构：
- SQLite 数据库存储 providers、proxy_config、provider_health 等表
- `ProviderRouter` 负责选 provider（支持熔断器和故障转移队列）
- `handler_context.rs` 在请求入口处调用 `select_providers()` 获取 provider
- Session ID 已从请求中提取（`session.rs`），但仅用于日志和 Gemini shadow store

## Goals / Non-Goals

**Goals:**
- 基于 `X-Claude-Code-Session-Id` 实现 session 级 provider 路由
- 支持 Round-Robin 和 Least-Loaded 两种分配策略
- 会话一致性：同一 session 始终走同一 provider
- 自动故障转移：provider 熔断时迁移到下一个可用 provider
- Session 生命周期管理：TTL 过期清理
- 管理 UI：配置开关、策略选择、session 列表、provider 负载展示
- 与现有熔断器、故障转移队列、用量统计完全兼容

**Non-Goals:**
- 不修改现有 provider 管理逻辑
- 不支持 session 级 provider 手动指定（后续迭代）
- 不支持 session 粒度的用量统计（已有 session_id 列，但未做聚合 UI）

## Decisions

### Decision 1: 代理层路由 vs 终端侧注入

选择**代理层路由**。所有终端仍指向同一代理（127.0.0.1:15721），代理根据 session ID 分发到不同 provider。
- 终端侧零改动，无需修改 Claude Code 配置
- 与 cc-switch 现有架构一致，仅需在 handler 层加一个分支
- 利用已有的 `X-Claude-Code-Session-Id` header，业界已验证可行

### Decision 2: Session 分配持久化到数据库

分配结果写入 `session_routes` 表，而非仅保存在内存中。
- 代理重启后分配状态不丢失
- 支持跨进程查询（UI 可读取）
- 数据库 SQLite 本地文件，开销可忽略

### Decision 3: Round-Robin + Least-Loaded 双策略

提供两种策略满足不同场景：
- Round-Robin：各 provider 能力相近时的简单轮转
- Least-Loaded：各 provider 并发能力不同时按负载分配

### Decision 4: 故障转移走 SessionRouter 而非 ProviderRouter

Session 故障转移由 `SessionRouter.failover()` 处理，而非回退到 `ProviderRouter.select_providers()`。
- 保持 session 映射的完整性（仍记录 session→provider 关系）
- 故障转移后的 provider 也记录在 session_routes 表中
- 与熔断器机制兼容：`SessionRouter` 在分配前检查熔断器状态

## Risks / Trade-offs

| 风险 | 可能性 | 影响 | 缓解措施 |
|------|--------|------|----------|
| 故障转移后上下文丢失 | 中 | 中 | 用户可感知，但比全部失败好；记录故障转移次数供诊断 |
| 大量 session 导致 DB 膨胀 | 低 | 低 | TTL 过期 + 定时清理 |
| 与现有功能冲突 | 低 | 高 | 开关默认禁用，不影响现有用户 |
| Session ID 为空 | 低 | 中 | 兜底走原有 ProviderRouter 逻辑 |