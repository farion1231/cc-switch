## Why

Claude Code 在 Windows 下开多个终端时，所有终端共享同一个 API provider。
单个 provider 的并发/速率限制导致多任务并行时互相阻塞。
需要为每个终端会话分配独立的 API provider，实现真正的多任务并行。

## What Changes

- 在 cc-switch 代理中添加 session 级 provider 路由能力
- 新终端会话自动分配 provider（Round-Robin / Least-Loaded）
- 同一会话的后续请求始终走同一 provider（会话一致性）
- Provider 熔断时自动故障转移到下一个可用 provider
- 新增 Session 路由管理 UI（配置、监控、清理）
- 新增数据库表 `session_routes` 和 `session_routing_config`

## Capabilities

### New Capabilities
- `session-routing`: Session 级多 Provider 路由。基于 `X-Claude-Code-Session-Id` header 识别终端会话，分配独立 provider，支持自动故障转移和会话一致性。

### Modified Capabilities
- （无，纯新增功能，未修改现有能力）

## Impact

- **后端**: 新增 `SessionRouter` 模块、DAO 层、数据库迁移（v17→v18）
- **前端**: 新增 Session 路由管理页面
- **代理**: Handler 层新增 session 路由分支，与现有熔断器/故障转移兼容
- **数据库**: 新增 2 张表，迁移脚本幂等
- 默认关闭，不影响现有用户