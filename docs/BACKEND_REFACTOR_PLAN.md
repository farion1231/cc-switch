# CC Switch Rust 后端重构方案

## 目录
- [背景与现状](#背景与现状)
- [问题确认](#问题确认)
- [方案评估](#方案评估)
- [渐进式重构路线](#渐进式重构路线)
- [测试策略](#测试策略)
- [风险与对策](#风险与对策)
- [总结](#总结)

## 背景与现状
- 前端已完成重构，后端 (Tauri + Rust) 仍维持历史结构。
- 核心文件集中在 `src-tauri/src/commands.rs`、`lib.rs` 等超大文件中，业务逻辑与界面事件耦合严重。
- 测试覆盖率低，只有零散单元测试，缺乏集成验证。

## 问题确认

| 提案问题 | 实际情况 | 严重程度 |
| --- | --- | --- |
| `commands.rs` 过长 | ✅ 1526 行，包含 32 个命令，职责混杂 | 🔴 高 |
| `lib.rs` 缺少服务层 | ✅ 541 行，托盘/事件/业务逻辑耦合 | 🟡 中 |
| `Result<T, String>` 泛滥 | ✅ 118 处，错误上下文丢失 | 🟡 中 |
| 全局 `Mutex` 阻塞 | ✅ 31 处 `.lock()` 调用，读写不分离 | 🟡 中 |
| 配置逻辑分散 | ✅ 分布在 5 个文件 (`config`/`app_config`/`app_store`/`settings`/`codex_config`) | 🟢 低 |

代码规模分布（约 5.4k SLOC）：
- `commands.rs`: 1526 行（28%）→ 第一优先级 🎯
- `lib.rs`: 541 行（10%）→ 托盘逻辑与业务耦合
- `mcp.rs`: 732 行（14%）→ 相对清晰
- `migration.rs`: 431 行（8%）→ 一次性逻辑
- 其他文件合计：2156 行（40%）

## 方案评估

### ✅ 优点
1. **分层架构清晰**  
   - `commands/`：Tauri 命令薄层  
   - `services/`：业务流程，如供应商切换、MCP 同步  
   - `infrastructure/`：配置读写、外设交互  
   - `domain/`：数据模型 (`Provider`, `AppType` 等)  
   → 提升可测试性、降低耦合度、方便团队协作。

2. **统一错误处理**  
   - 引入 `AppError`（`thiserror`），保留错误链和上下文。  
   - Tauri 命令仍返回 `Result<T, String>`，通过 `From<AppError>` 自动转换。  
   - 改善日志可读性，利于排查。

3. **并发优化**  
   - `AppState` 切换为 `RwLock<MultiAppConfig>`。  
   - 读多写少的场景提升吞吐（如频繁查询供应商列表）。

### ⚠️ 风险
1. **过度设计**  
   - 完整 DDD 四层在 5k 行项目中会增加 30-50% 维护成本。  
   - Rust trait + repository 样板较多，收益不足。  
   - 推荐“轻量分层”而非正统 DDD。

2. **迁移成本高**  
   - `commands.rs` 拆分、错误统一、锁改造触及多文件。  
   - 测试缺失导致重构风险高，需先补测试。  
   - 估算完整改造需 5-6 周；建议分阶段输出可落地价值。

3. **技术选型需谨慎**  
   - `parking_lot` 相比标准库 `RwLock` 提升有限，不必引入。  
   - `spawn_blocking` 仅用于 >100ms 的阻塞任务，避免滥用。  
   - 以现有依赖为主，控制复杂度。

## 实施进度
- **阶段 1：统一错误处理 ✅**  
  - 引入 `thiserror` 并在 `src-tauri/src/error.rs` 定义 `AppError`，提供常用构造函数和 `From<AppError> for String`，保留错误链路。  
  - 配置、存储、同步等核心模块（`config.rs`、`app_config.rs`、`app_store.rs`、`store.rs`、`codex_config.rs`、`claude_mcp.rs`、`claude_plugin.rs`、`import_export.rs`、`mcp.rs`、`migration.rs`、`speedtest.rs`、`usage_script.rs`、`settings.rs`、`lib.rs` 等）已统一返回 `Result<_, AppError>`，避免字符串错误丢失上下文。  
  - Tauri 命令层继续返回 `Result<_, String>`，通过 `?` + `Into<String>` 统一转换，前端无需调整。  
  - `cargo check` 通过，`rg "Result<[^>]+, String"` 巡检确认除命令层外已无字符串错误返回。
- **阶段 2：拆分命令层 ✅**  
  - 已将单一 `src-tauri/src/commands.rs` 拆分为 `commands/{provider,mcp,config,settings,misc,plugin}.rs` 并通过 `commands/mod.rs` 统一导出，保持对外 API 不变。  
  - 每个文件聚焦单一功能域（供应商、MCP、配置、设置、杂项、插件），命令函数平均 150-250 行，可读性与后续维护性显著提升。  
  - 相关依赖调整后 `cargo check` 通过，静态巡检确认无重复定义或未注册命令。
- **阶段 3：补充测试 ✅**  
  - `tests/import_export_sync.rs` 集成测试涵盖配置备份、Claude/Codex live 同步、MCP 投影与 Codex/Claude 双向导入流程，并新增启用项清理、非法 TOML 抛错等失败场景验证；统一使用隔离 HOME 目录避免污染真实用户环境。  
  - 扩展 `lib.rs` re-export，暴露 `AppType`、`MultiAppConfig`、`AppError`、配置 IO 以及 Codex/Claude MCP 路径与同步函数，方便服务层及测试直接复用核心逻辑。  
  - 新增负向测试验证 Codex 供应商缺少 `auth` 字段时的错误返回，并补充备份数量上限测试；顺带修复 `create_backup` 采用内存读写避免拷贝继承旧的修改时间，确保最新备份不会在清理阶段被误删。  
  - 针对 `codex_config::write_codex_live_atomic` 补充成功与失败场景测试，覆盖 auth/config 原子写入与失败回滚逻辑（模拟目标路径为目录时的 rename 失败），降低 Codex live 写入回归风险。  
  - 新增 `tests/provider_commands.rs` 覆盖 `switch_provider` 的 Codex 正常流程与供应商缺失分支，并抽取 `switch_provider_internal` 以复用 `AppError`，通过 `switch_provider_test_hook` 暴露测试入口；同时共享 `tests/support.rs` 提供隔离 HOME / 互斥工具函数。  
  - 补充 Claude 切换集成测试，验证 live `settings.json` 覆写、新旧供应商快照回填以及 `.cc-switch/config.json` 持久化结果，确保阶段四提取服务层时拥有可回归的用例。  
  - 增加 Codex 缺失 `auth` 场景测试，确认 `switch_provider_internal` 在关键字段缺失时返回带上下文的 `AppError`，同时保持内存状态未被污染。  
  - 为配置导入命令抽取复用逻辑 `import_config_from_path` 并补充成功/失败集成测试，校验备份生成、状态同步、JSON 解析与文件缺失等错误回退路径；`export_config_to_file` 亦具备成功/缺失源文件的命令级回归。  
  - 新增 `tests/mcp_commands.rs`，通过测试钩子覆盖 `import_default_config`、`import_mcp_from_claude`、`set_mcp_enabled` 等命令层行为，验证缺失文件/非法 JSON 的错误回滚以及成功路径落盘效果；阶段三目标达成，命令层关键边界已具备回归保障。
- **阶段 4：服务层抽象 🚧（进行中）**  
  - 新增 `services/provider.rs` 并实现 `ProviderService::switch` / `delete`，集中处理供应商切换、回填、MCP 同步等核心业务；命令层改为薄封装并在 `tests/provider_service.rs`、`tests/provider_commands.rs` 中完成成功与失败路径的集成验证。  
  - 新增 `services/mcp.rs` 提供 `McpService`，封装 MCP 服务器的查询、增删改、启用同步与导入流程；命令层改为参数解析 + 调用服务，`tests/mcp_commands.rs` 直接使用 `McpService` 验证成功与失败路径，阶段三测试继续适配。  
  - `McpService` 在内部先复制内存快照、释放写锁，再执行文件同步，避免阶段五升级后的 `RwLock` 在 I/O 场景被长时间占用；`upsert/delete/set_enabled/sync_enabled` 均已修正。  
  - 新增 `services/config.rs` 提供 `ConfigService`，统一处理配置导入导出、备份与 live 同步；命令层迁移至 `commands/import_export.rs`，在落盘操作前释放锁并复用现有集成测试。  
  - 新增 `services/speedtest.rs` 并实现 `SpeedtestService::test_endpoints`，将 URL 校验、超时裁剪与网络请求封装在服务层，命令改为薄封装；补充单元测试覆盖空列表与非法 URL 分支。  
  - 后续可选：应用设置（Store）命令仍较薄，可按需评估是否抽象；当前阶段四核心服务已基本齐备。  
- **阶段 5：锁与阻塞优化 ✅（首轮）**  
  - `AppState` 已由 `Mutex<MultiAppConfig>` 切换为 `RwLock<MultiAppConfig>`，托盘、命令与测试均按读写语义区分 `read()` / `write()`；`cargo test` 全量通过验证并未破坏现有流程。  
  - 针对高开销 IO 的配置导入/导出命令提取 `load_config_for_import`，并通过 `tauri::async_runtime::spawn_blocking` 将文件读写与备份迁至阻塞线程，保持命令处理线程轻量。  
  - 其余命令梳理后确认仍属轻量同步操作，暂不额外引入 `spawn_blocking`；若后续出现新的长耗时流程，再按同一模式扩展。  

## 渐进式重构路线

### 阶段 1：统一错误处理（高收益 / 低风险）
- 新增 `src-tauri/src/error.rs`，定义 `AppError`。  
- 底层文件 IO、配置解析等函数返回 `Result<T, AppError>`。  
- 命令层通过 `?` 自动传播，最终 `.map_err(Into::into)`。
- 预估 3-5 天，立即启动。

### 阶段 2：拆分 `commands.rs`（高收益 / 中风险）
- 按业务拆分为 `commands/provider.rs`、`commands/mcp.rs`、`commands/config.rs`、`commands/settings.rs`、`commands/misc.rs`。  
- `commands/mod.rs` 统一导出和注册。  
- 文件行数降低到 200-300 行/文件，职责单一。  
- 预估 5-7 天，可并行进行部分重构。

### 阶段 3：补充测试（中收益 / 中风险）
- 引入 `tests/` 或 `src-tauri/tests/` 集成测试，覆盖供应商切换、MCP 同步、配置迁移。  
- 使用 `tempfile`/`tempdir` 隔离文件系统，组合少量回归脚本。  
- 预估 5-7 天，为后续重构提供安全网。

### 阶段 4：提取轻量服务层（中收益 / 中风险）
- 新增 `services/provider_service.rs`、`services/mcp_service.rs`。  
- 不强制使用 trait；直接以自由函数/结构体实现业务流程。  
   ```rust
   pub struct ProviderService;
   impl ProviderService {
       pub fn switch(config: &mut MultiAppConfig, app: AppType, id: &str) -> Result<(), AppError> {
           // 业务流程：验证、回填、落盘、更新 current、触发事件
       }
   }
   ```
- 命令层负责参数解析，服务层处理业务逻辑，托盘逻辑重用同一接口。  
- 预估 7-10 天，可在测试补齐后执行。

### 阶段 5：锁与阻塞优化（低收益 / 低风险）
- ✅ `AppState` 已从 `Mutex` 切换为 `RwLock`，命令与托盘读写按需区分，现有测试全部通过。  
- ✅ 配置导入/导出命令通过 `spawn_blocking` 处理高开销文件 IO；其他命令维持同步执行以避免不必要调度。  
- 🔄 持续监控：若后续引入新的批量迁移或耗时任务，再按相同模式扩展到阻塞线程；观察运行时锁竞争情况，必要时考虑进一步拆分状态或引入缓存。  

## 测试策略
- **优先覆盖场景**  
  - 供应商切换：状态更新 + live 配置同步  
  - MCP 同步：enabled 服务器快照与落盘  
  - 配置迁移：归档、备份与版本升级
- **推荐结构**
  ```rust
  #[cfg(test)]
  mod integration {
      use super::*;
      #[test]
      fn switch_provider_updates_live_config() { /* ... */ }
      #[test]
      fn sync_mcp_to_codex_updates_claude_config() { /* ... */ }
      #[test]
      fn migration_preserves_backup() { /* ... */ }
  }
  ```
- 目标覆盖率：关键路径 >80%，文件 IO/迁移 >70%。

## 风险与对策
- **测试不足** → 阶段 3 强制补齐，建立基础集成测试。  
- **重构跨度大** → 按阶段在独立分支推进（如 `refactor/backend-step1` 等）。  
- **回滚困难** → 每阶段结束打 tag（如 `v3.6.0-backend-step1`），保留回滚点。  
- **功能回归** → 重构后执行手动冒烟流程：供应商切换、托盘操作、MCP 同步、配置导入导出。

## 总结
- 当前规模下不建议整体引入完整 DDD/四层架构，避免过度设计。  
- 建议遵循“错误统一 → 命令拆分 → 补测试 → 服务层抽象 → 锁优化”的渐进式策略。  
- 完成阶段 1-3 后即可显著提升可维护性与可靠性；阶段 4-5 可根据资源灵活安排。  
- 重构过程中同步维护文档与测试，确保团队成员对架构演进保持一致认知。
