## 1. 数据库迁移

- [x] 1.1 更新 SCHEMA_VERSION 17→18
- [x] 1.2 添加 migrate_v17_to_v18 迁移函数（创建 session_routes 和 session_routing_config 表）
- [x] 1.3 创建 DAO 层（session_routes.rs）：CRUD 操作、过期清理、负载统计

## 2. Session Router 核心

- [x] 2.1 创建 SessionRouter 模块（session_router.rs）
- [x] 2.2 实现 Round-Robin 分配策略
- [x] 2.3 实现 Least-Loaded 分配策略
- [x] 2.4 实现 session 故障转移逻辑
- [x] 2.5 注册到 ProxyServer（ProxyState.session_router）

## 3. 集成到请求处理

- [x] 3.1 修改 handler_context.rs：在 select_providers 前检查 session 路由
- [x] 3.2 实现 session 路由优先、熔断器检查、故障转移回退链
- [x] 3.3 添加熔断器检查 + 故障转移后的 session 映射更新

## 4. 后端命令

- [x] 4.1 创建 commands/session_routes.rs（6 个 Tauri 命令）
- [x] 4.2 注册命令到 mod.rs 和 lib.rs

## 5. 前端 API 层

- [x] 5.1 创建 lib/api/sessionRoutes.ts
- [x] 5.2 创建 lib/query/sessionRoutes.ts（TanStack Query hooks）

## 6. 前端 UI 组件

- [x] 6.1 创建 SessionRoutingPage.tsx（配置面板、session 列表、负载展示）
- [ ] 6.2 集成到 App.tsx 侧边栏导航
- [ ] 6.3 添加 i18n 翻译（已完成 zh/en，需补充其他语言）

## 7. 验证

- [ ] 7.1 启动开发模式，验证后端编译通过
- [ ] 7.2 验证前端编译通过
- [ ] 7.3 端到端测试：开多个终端，确认各自分配不同 provider