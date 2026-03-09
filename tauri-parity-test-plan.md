# Tauri Command Parity Test Plan

## 当前状态（2026-03-09）

这轮迁移已经完成了主计划里的 `Layer A + Layer B + Layer C`，并用现有前端 integration tests 补上了轻量 `Layer D` 主链 smoke：

- `src-tauri baseline` 已落地并通过
- `legacy vs core parity` 已落地并通过
- 前端 `GUI API smoke` 已通过 `tests/api/TauriContracts.test.ts` 和现有 integration tests 补齐
- `Layer D` 当前由 `tests/integration/App.test.tsx`、`tests/integration/SettingsDialog.test.tsx` 等主链 smoke 覆盖

已完成迁移并具备 baseline/parity 覆盖的域：

- `provider`
- `mcp`
- `prompt`
- `skill`
- `usage`
- `proxy + failover`
- `settings + config + import/export + backup + webdav`
- `deeplink`
- `workspace + sessions`
- `openclaw + omo`
- `global_proxy`
- `env`
- `plugin + stream_check`

当前明确留在壳层、不纳入 `core parity` 主结论的命令仍然是这些：

- 文件/目录选择器：`pick_directory`、`save_file_dialog`、`open_file_dialog`、`open_zip_file_dialog`
- 打开外部资源：`open_external`、`open_config_folder`、`open_app_config_folder`、`open_workspace_directory`
- 终端拉起：`launch_session_terminal`、`open_provider_terminal`
- 窗口/桌面壳层：`restart_app`、`set_window_theme`
- 启动态只读信息：`get_init_error`、`get_migration_result`、`get_skills_migration_result`

也就是说，后续如果继续推进，重点已经不是“再把业务迁去 core”，而是：

1. 决定哪些壳层命令需要保留
2. 决定是否要补真实 Tauri runtime / Playwright 级别的桌面 smoke
3. 在前端页面层继续做更高价值的主流程回归

## 目标

在把 `src-tauri` 命令层切到 `cc-switch-core` 之前，先建立一套可重复、可比较、可逐域推进的测试框架。

这套框架分三层，顺序不能反：

1. 先给当前 `src-tauri` 建 baseline，冻结现状行为
2. 再做 `legacy vs core` parity，对比新旧实现
3. 最后再做 GUI smoke，确认页面仍然可用

这套框架要回答的不是“新实现能不能跑”，而是：

- 当前 `src-tauri` 的真实行为到底是什么
- 同一组输入下，旧 Tauri 实现和新 core 实现的返回值是否等价
- 同一组操作下，数据库、live config、settings、导入导出文件是否等价
- GUI 现有页面是否可以无感切换到新命令实现

## 成功标准

当一个业务域完成迁移时，需要同时满足：

1. 该域的 `src-tauri baseline` case 全部通过
2. 该域的 parity case 全部通过
3. 该域涉及的文件副作用快照一致
4. 该域涉及的数据库快照一致，或差异被明确允许
5. 前端现有 API 包装层不需要重写业务逻辑
6. 对应 GUI smoke flow 可以完整跑通

## 不在本阶段硬做的内容

这些能力不属于 core parity 主阻塞，只做壳层 smoke：

- 文件/目录选择器：`pick_directory`、`save_file_dialog`、`open_file_dialog`、`open_zip_file_dialog`
- 打开外部资源：`open_external`、`open_config_folder`、`open_app_config_folder`、`open_workspace_directory`
- 终端拉起：`launch_session_terminal`、`open_provider_terminal`
- 窗口/桌面壳层：`restart_app`、`set_window_theme`
- App 启动态信息：`get_init_error`、`get_migration_result`、`get_skills_migration_result`

这些命令后续仍然需要测试，但不需要塞进 “core 是否正确复用” 的主 parity 结论里。

## 推荐目录结构

### 1. baseline + parity 集成测试

放在：

- `src-tauri/tests/parity/`

建议文件：

- `src-tauri/tests/parity/mod.rs`
- `src-tauri/tests/parity/support.rs`
- `src-tauri/tests/parity/provider_baseline.rs`
- `src-tauri/tests/parity/provider_parity.rs`
- `src-tauri/tests/parity/mcp_baseline.rs`
- `src-tauri/tests/parity/mcp_parity.rs`
- `src-tauri/tests/parity/prompt_baseline.rs`
- `src-tauri/tests/parity/prompt_parity.rs`
- `src-tauri/tests/parity/skill_baseline.rs`
- `src-tauri/tests/parity/skill_parity.rs`
- `src-tauri/tests/parity/proxy_baseline.rs`
- `src-tauri/tests/parity/proxy_parity.rs`
- `src-tauri/tests/parity/usage_baseline.rs`
- `src-tauri/tests/parity/usage_parity.rs`
- `src-tauri/tests/parity/settings_baseline.rs`
- `src-tauri/tests/parity/settings_parity.rs`
- `src-tauri/tests/parity/deeplink_baseline.rs`
- `src-tauri/tests/parity/deeplink_parity.rs`
- `src-tauri/tests/parity/workspace_baseline.rs`
- `src-tauri/tests/parity/workspace_parity.rs`
- `src-tauri/tests/parity/openclaw_omo_baseline.rs`
- `src-tauri/tests/parity/openclaw_omo_parity.rs`

### 2. baseline/parity fixtures

放在：

- `src-tauri/tests/fixtures/parity/`

建议子目录：

- `providers/`
- `mcp/`
- `prompts/`
- `skills/`
- `deeplink/`
- `imports/`
- `workspace/`
- `usage/`
- `proxy/`

### 3. GUI smoke

GUI smoke 单独放，避免和 parity 混在一起：

- `qa/tauri-smoke/`

它的职责不是做精细断言，而是验证页面流程还能走通。

## 推荐实现方式

## Step 0: 先做 baseline，不要先接 core

第一步不是改命令实现，而是把当前 `src-tauri` 当成真值源测稳。

baseline 的职责是：

- 冻结当前返回结构
- 冻结当前副作用
- 明确哪些奇怪行为是现状，哪些是 bug

只有 baseline 跑稳之后，parity 才有比较对象。

## Step 1: 再抽一层 bridge

在真正开始迁移前，先把 Tauri command body 从宏函数里抽出来。

建议目录：

- `src-tauri/src/bridges/`

每个 bridge 都提供两套 plain Rust 入口：

- `legacy_*`：保留当前 `src-tauri` 逻辑
- `core_*`：调用 `cc-switch-core` 对应 service

命令函数最后只做参数解包和壳层对象注入：

- `#[tauri::command]`
- 调 bridge
- 做极少量 `AppHandle` / `State` / dialog / shell 适配

这样 baseline 和 parity 测试都不需要直接构造 `tauri::State<'_, AppState>` 或 GUI 运行时。

## Step 2: 定义统一快照

每个 parity case 跑完后，都产出一份统一快照：

- `result`: 归一化后的返回 JSON
- `db`: 相关表的归一化快照
- `files`: 相关 live config / workspace / backup 文件快照
- `settings`: 当前 settings 快照
- `warnings`: 容忍的已知差异

建议结构：

```rust
struct ParitySnapshot {
    result: serde_json::Value,
    db: serde_json::Value,
    files: BTreeMap<String, String>,
    settings: serde_json::Value,
    warnings: Vec<String>,
}
```

## Step 3: 统一归一化

以下字段不能直接做逐字比较：

- 时间戳
- UUID / request id / backup id
- 文件绝对路径中的临时目录前缀
- 排序不稳定的数组
- 错误字符串里的系统细节

需要在 compare 前做 normalize：

- 统一排序
- 去掉波动字段
- 路径转相对标记
- 时间戳转占位
- 错误按 code / category 比较，而不是按整句比较

## Step 4: 三层断言

每个 parity case 都做三层断言：

1. `contract parity`
   - 返回结构一致
2. `state parity`
   - DB / settings / live file 一致
3. `behavior parity`
   - 再次读取相关状态时，表现一致

baseline case 至少要做两层断言：

1. `contract baseline`
   - 当前返回结构被稳定记录
2. `state baseline`
   - 当前 DB / settings / live file 副作用被稳定记录

## Step 5: 每次只切一个域

迁移顺序建议：

1. `provider`
2. `mcp`
3. `prompt`
4. `skill`
5. `usage`
6. `proxy + failover`
7. `settings + config + import/export + backup + webdav`
8. `deeplink`
9. `workspace + sessions`
10. `openclaw + omo`
11. `global_proxy`
12. `misc shell adapters`

## parity 测试支撑

建议尽量复用现有测试基础：

- `src-tauri/tests/support.rs`

已有内容可复用：

- fake HOME
- 测试目录重置
- 全局互斥
- 基础 `AppState` 构造

建议在 parity support 里补这些 helper：

- `seed_db_from_fixture(...)`
- `seed_live_files(...)`
- `snapshot_tables(...)`
- `snapshot_paths(...)`
- `run_baseline_case(...)`
- `run_legacy_case(...)`
- `run_core_case(...)`
- `assert_parity(...)`

## 执行层次

### Layer A: Rust baseline integration

用途：

- 先确认当前 `src-tauri` 行为
- 作为后续 parity 的比较基线

运行方式：

- `cargo test -p cc-switch baseline::provider`
- `cargo test -p cc-switch baseline::proxy`

### Layer B: Rust parity integration

用途：

- 比较旧 bridge 和新 bridge
- 是主判定层

运行方式：

- `cargo test -p cc-switch parity::provider`
- `cargo test -p cc-switch parity::proxy`

### Layer C: GUI API smoke

用途：

- 确认 `src/lib/api/*` 和前端 hooks/query 不需要大改
- 每个域做 1 到 2 条页面级 smoke

### Layer D: Full GUI smoke

用途：

- 域迁移完成后跑主链
- 只关心“用户能不能完成任务”

## case 清单

下面的清单分成四类：

- `P0 Tauri Baseline`: 先冻结现状行为
- `P1 Core Parity`: 再比较 legacy 和 core
- `P2 Shell Smoke`: 壳层能力，只需要 smoke
- `P3 End-to-End Flow`: 跨域链路，作为最终把关

## P0 Tauri Baseline Cases

`P0` 和 `P1` 使用同一套 case 域，但执行目标不同：

- `P0` 只跑当前 `src-tauri`
- `P1` 跑 `legacy vs core`

也就是说，下面列出的业务 case 先全部做 baseline，再逐域补 parity。

## P1 Core Parity Cases

### Provider

| Case ID | Case | 涉及命令 |
|---|---|---|
| `provider.list` | 读取 provider 列表，排序和字段一致 | `get_providers` |
| `provider.current` | 当前 provider 解析一致 | `get_current_provider` |
| `provider.add` | 新增 provider 后 DB 和 live state 一致 | `add_provider` |
| `provider.update` | 更新 provider 后字段和副作用一致 | `update_provider` |
| `provider.delete` | 删除 provider 后 DB 和 live state 一致 | `delete_provider` |
| `provider.remove_live` | 从 live config 中移除 provider 的副作用一致 | `remove_provider_from_live_config` |
| `provider.switch` | 切换 provider 后 current/live files 一致 | `switch_provider` |
| `provider.import_default` | 从 live 默认配置导入结果一致 | `import_default_config` |
| `provider.read_live` | 读取 live provider settings 结果一致 | `read_live_provider_settings` |
| `provider.endpoint_test` | endpoint 测试返回结构一致 | `test_api_endpoints` |
| `provider.custom_endpoints.get` | custom endpoints 列表一致 | `get_custom_endpoints` |
| `provider.custom_endpoints.add` | 新增 endpoint 的 DB/meta 一致 | `add_custom_endpoint` |
| `provider.custom_endpoints.remove` | 删除 endpoint 后状态一致 | `remove_custom_endpoint` |
| `provider.custom_endpoints.touch` | last used 更新逻辑一致 | `update_endpoint_last_used` |
| `provider.sort_order` | provider 排序更新一致 | `update_providers_sort_order` |
| `provider.universal.list` | universal provider 列表一致 | `get_universal_providers` |
| `provider.universal.get` | universal provider 详情一致 | `get_universal_provider` |
| `provider.universal.upsert` | universal provider 保存一致 | `upsert_universal_provider` |
| `provider.universal.delete` | universal provider 删除一致 | `delete_universal_provider` |
| `provider.universal.sync` | sync 到目标 app 的结果一致 | `sync_universal_provider` |
| `provider.opencode.import_live` | OpenCode live provider 导入一致 | `import_opencode_providers_from_live` |
| `provider.opencode.live_ids` | OpenCode live provider id 列表一致 | `get_opencode_live_provider_ids` |
| `provider.openclaw.import_live` | OpenClaw live provider 导入一致 | `import_openclaw_providers_from_live` |
| `provider.openclaw.live_ids` | OpenClaw live provider id 列表一致 | `get_openclaw_live_provider_ids` |
| `provider.usage.query` | provider usage 查询结果一致 | `queryProviderUsage` |
| `provider.usage.test_script` | usage script 测试结果一致 | `testUsageScript` |

### MCP

| Case ID | Case | 涉及命令 |
|---|---|---|
| `mcp.claude.status` | Claude MCP 状态读取一致 | `get_claude_mcp_status` |
| `mcp.claude.read` | Claude MCP 原始配置读取一致 | `read_claude_mcp_config` |
| `mcp.claude.upsert` | Claude MCP server 写入一致 | `upsert_claude_mcp_server` |
| `mcp.claude.delete` | Claude MCP server 删除一致 | `delete_claude_mcp_server` |
| `mcp.validate` | MCP command 校验行为一致 | `validate_mcp_command` |
| `mcp.config.get` | 某 app 的 MCP config 读取一致 | `get_mcp_config` |
| `mcp.config.upsert` | 往 app config 写入 MCP server 一致 | `upsert_mcp_server_in_config` |
| `mcp.config.delete` | 从 app config 删除 MCP server 一致 | `delete_mcp_server_in_config` |
| `mcp.config.enable` | MCP enable/disable 写回 app config 一致 | `set_mcp_enabled` |
| `mcp.registry.list` | SSOT MCP 列表一致 | `get_mcp_servers` |
| `mcp.registry.upsert` | SSOT MCP 保存一致 | `upsert_mcp_server` |
| `mcp.registry.delete` | SSOT MCP 删除一致 | `delete_mcp_server` |
| `mcp.registry.toggle_app` | MCP app enable/disable 一致 | `toggle_mcp_app` |
| `mcp.import_from_apps` | 从 live configs 导入 MCP 一致 | `import_mcp_from_apps` |

### Prompt

| Case ID | Case | 涉及命令 |
|---|---|---|
| `prompt.list` | prompt 列表一致 | `get_prompts` |
| `prompt.upsert` | prompt 保存和覆盖一致 | `upsert_prompt` |
| `prompt.delete` | prompt 删除一致 | `delete_prompt` |
| `prompt.enable` | enable 后 live prompt 副作用一致 | `enable_prompt` |
| `prompt.import_live` | 从 live prompt 导入一致 | `import_prompt_from_file` |
| `prompt.current_file` | 当前 prompt live file 内容一致 | `get_current_prompt_file_content` |

### Skill

| Case ID | Case | 涉及命令 |
|---|---|---|
| `skill.installed.list` | installed skills 列表一致 | `get_installed_skills` |
| `skill.unified.install` | unified install 的 DB 和文件副作用一致 | `install_skill_unified` |
| `skill.unified.uninstall` | unified uninstall 一致 | `uninstall_skill_unified` |
| `skill.toggle_app` | app 开关同步一致 | `toggle_skill_app` |
| `skill.scan_unmanaged` | 未托管扫描结果一致 | `scan_unmanaged_skills` |
| `skill.import_from_apps` | 从 apps 导入 skill 一致 | `import_skills_from_apps` |
| `skill.discover` | 可安装 skills 列表结构一致 | `discover_available_skills` |
| `skill.list_all` | 所有 skills 列表一致 | `get_skills` |
| `skill.list_for_app` | 按 app 过滤结果一致 | `get_skills_for_app` |
| `skill.install` | 安装 skill 一致 | `install_skill` |
| `skill.install_for_app` | 安装 skill 到 app 一致 | `install_skill_for_app` |
| `skill.uninstall` | 卸载 skill 一致 | `uninstall_skill` |
| `skill.uninstall_for_app` | 卸载 app skill 一致 | `uninstall_skill_for_app` |
| `skill.repo.list` | skill repo 列表一致 | `get_skill_repos` |
| `skill.repo.add` | skill repo 新增一致 | `add_skill_repo` |
| `skill.repo.remove` | skill repo 删除一致 | `remove_skill_repo` |
| `skill.zip_install` | zip 安装结果一致 | `install_skills_from_zip` |

### Usage

| Case ID | Case | 涉及命令 |
|---|---|---|
| `usage.summary` | summary 聚合一致 | `get_usage_summary` |
| `usage.trends` | trend 聚合一致 | `get_usage_trends` |
| `usage.provider_stats` | provider stats 一致 | `get_provider_stats` |
| `usage.model_stats` | model stats 一致 | `get_model_stats` |
| `usage.logs` | request logs 过滤/分页一致 | `get_request_logs` |
| `usage.detail` | request detail 一致 | `get_request_detail` |
| `usage.pricing.list` | model pricing 列表一致 | `get_model_pricing` |
| `usage.pricing.update` | model pricing 更新一致 | `update_model_pricing` |
| `usage.pricing.delete` | model pricing 删除一致 | `delete_model_pricing` |
| `usage.provider_limits` | provider limits 检查一致 | `check_provider_limits` |

### Proxy + Failover

| Case ID | Case | 涉及命令 |
|---|---|---|
| `proxy.start` | 启动 proxy 的返回和状态一致 | `start_proxy_server` |
| `proxy.stop_restore` | 停止并恢复 live takeover 一致 | `stop_proxy_with_restore` |
| `proxy.takeover.status` | takeover 状态一致 | `get_proxy_takeover_status` |
| `proxy.takeover.set` | per-app takeover 开关一致 | `set_proxy_takeover_for_app` |
| `proxy.status` | proxy 运行状态一致 | `get_proxy_status` |
| `proxy.config.get` | proxy config 读取一致 | `get_proxy_config` |
| `proxy.config.update` | proxy config 更新一致 | `update_proxy_config` |
| `proxy.global_config.get` | global proxy config 一致 | `get_global_proxy_config` |
| `proxy.global_config.update` | global proxy config 更新一致 | `update_global_proxy_config` |
| `proxy.app_config.get` | app proxy config 一致 | `get_proxy_config_for_app` |
| `proxy.app_config.update` | app proxy config 更新一致 | `update_proxy_config_for_app` |
| `proxy.cost_multiplier.get` | 默认 cost multiplier 一致 | `get_default_cost_multiplier` |
| `proxy.cost_multiplier.set` | 默认 cost multiplier 更新一致 | `set_default_cost_multiplier` |
| `proxy.pricing_source.get` | pricing model source 一致 | `get_pricing_model_source` |
| `proxy.pricing_source.set` | pricing model source 更新一致 | `set_pricing_model_source` |
| `proxy.running` | running bool 一致 | `is_proxy_running` |
| `proxy.live_takeover_active` | live takeover bool 一致 | `is_live_takeover_active` |
| `proxy.switch_provider` | proxy target 切换副作用一致 | `switch_proxy_provider` |
| `proxy.health` | provider health 一致 | `get_provider_health` |
| `proxy.circuit.reset` | circuit reset 副作用一致 | `reset_circuit_breaker` |
| `proxy.circuit.config.get` | circuit config 一致 | `get_circuit_breaker_config` |
| `proxy.circuit.config.update` | circuit config 更新一致 | `update_circuit_breaker_config` |
| `proxy.circuit.stats` | circuit stats 一致 | `get_circuit_breaker_stats` |
| `failover.queue.get` | failover queue 一致 | `get_failover_queue` |
| `failover.available` | 可加入 failover 的 provider 列表一致 | `get_available_providers_for_failover` |
| `failover.queue.add` | 加入 failover 队列一致 | `add_to_failover_queue` |
| `failover.queue.remove` | 移除 failover 队列一致 | `remove_from_failover_queue` |
| `failover.auto.get` | auto failover 状态一致 | `get_auto_failover_enabled` |
| `failover.auto.set` | auto failover 开关一致 | `set_auto_failover_enabled` |

### Settings + Config + Import/Export + Backup + WebDAV

| Case ID | Case | 涉及命令 |
|---|---|---|
| `settings.get` | settings 读取一致 | `get_settings` |
| `settings.save` | settings 保存及副作用一致 | `save_settings` |
| `settings.dir_override.get` | config dir override 读取一致 | `get_app_config_dir_override` |
| `settings.dir_override.set` | config dir override 保存一致 | `set_app_config_dir_override` |
| `settings.auto_launch.set` | auto launch 设置结果一致 | `set_auto_launch` |
| `settings.auto_launch.get` | auto launch 状态一致 | `get_auto_launch_status` |
| `settings.rectifier.get` | rectifier config 一致 | `get_rectifier_config` |
| `settings.rectifier.set` | rectifier config 更新一致 | `set_rectifier_config` |
| `settings.log.get` | log config 一致 | `get_log_config` |
| `settings.log.set` | log config 更新一致 | `set_log_config` |
| `config.status.claude` | Claude config status 一致 | `get_claude_config_status` |
| `config.status.app` | app config status 一致 | `get_config_status` |
| `config.path.claude_code` | Claude Code config path 一致 | `get_claude_code_config_path` |
| `config.path.dir` | app config dir 一致 | `get_config_dir` |
| `config.path.app_config` | app config path 一致 | `get_app_config_path` |
| `config.snippet.claude.get` | Claude snippet 读取一致 | `get_claude_common_config_snippet` |
| `config.snippet.claude.set` | Claude snippet 保存一致 | `set_claude_common_config_snippet` |
| `config.snippet.get` | 通用 snippet 读取一致 | `get_common_config_snippet` |
| `config.snippet.set` | 通用 snippet 保存一致 | `set_common_config_snippet` |
| `config.snippet.extract` | 从 live config 提取 snippet 一致 | `extract_common_config_snippet` |
| `import_export.export` | export 文件内容一致 | `export_config_to_file` |
| `import_export.import` | import 后 DB/live 副作用一致 | `import_config_from_file` |
| `import_export.sync_live` | sync current providers live 一致 | `sync_current_providers_live` |
| `backup.create` | backup 创建结果一致 | `create_db_backup` |
| `backup.list` | backup 列表一致 | `list_db_backups` |
| `backup.restore` | backup 恢复副作用一致 | `restore_db_backup` |
| `backup.rename` | backup 重命名一致 | `rename_db_backup` |
| `backup.delete` | backup 删除一致 | `delete_db_backup` |
| `webdav.test` | WebDAV 测试结果结构一致 | `webdav_test_connection` |
| `webdav.upload` | WebDAV upload 结果一致 | `webdav_sync_upload` |
| `webdav.download` | WebDAV download 结果一致 | `webdav_sync_download` |
| `webdav.save_settings` | WebDAV settings 保存一致 | `webdav_sync_save_settings` |
| `webdav.remote_info` | WebDAV remote info 一致 | `webdav_sync_fetch_remote_info` |

### Deeplink

| Case ID | Case | 涉及命令 |
|---|---|---|
| `deeplink.parse` | deeplink 解析一致 | `parse_deeplink` |
| `deeplink.merge` | merge 结果一致 | `merge_deeplink_config` |
| `deeplink.import_unified` | 统一导入结果和副作用一致 | `import_from_deeplink_unified` |
| `deeplink.import_legacy` | legacy 导入兼容性一致 | `import_from_deeplink` |

### Plugin + Stream Check

| Case ID | Case | 涉及命令 |
|---|---|---|
| `plugin.apply_config` | Claude plugin config 应用和文件副作用一致 | `apply_claude_plugin_config` |
| `plugin.apply_onboarding_skip` | onboarding skip 应用一致 | `apply_claude_onboarding_skip` |
| `plugin.clear_onboarding_skip` | onboarding skip 清理一致 | `clear_claude_onboarding_skip` |
| `stream_check.single` | 单 provider stream check 结果结构一致 | `stream_check_provider` |
| `stream_check.batch` | 批量 stream check 结果结构一致 | `stream_check_all_providers` |
| `stream_check.config.get` | stream check 配置读取一致 | `get_stream_check_config` |
| `stream_check.config.set` | stream check 配置保存一致 | `save_stream_check_config` |

### OpenClaw + OMO

| Case ID | Case | 涉及命令 |
|---|---|---|
| `openclaw.import_live` | OpenClaw live providers 导入一致 | `import_openclaw_providers_from_live` |
| `openclaw.live_ids` | live provider ids 一致 | `get_openclaw_live_provider_ids` |
| `openclaw.default_model.get` | default model 读取一致 | `get_openclaw_default_model` |
| `openclaw.default_model.set` | default model 保存一致 | `set_openclaw_default_model` |
| `openclaw.catalog.get` | model catalog 一致 | `get_openclaw_model_catalog` |
| `openclaw.catalog.set` | model catalog 保存一致 | `set_openclaw_model_catalog` |
| `openclaw.agent_defaults.get` | agent defaults 一致 | `get_openclaw_agents_defaults` |
| `openclaw.agent_defaults.set` | agent defaults 保存一致 | `set_openclaw_agents_defaults` |
| `openclaw.env.get` | env config 一致 | `get_openclaw_env` |
| `openclaw.env.set` | env config 保存一致 | `set_openclaw_env` |
| `openclaw.tools.get` | tools config 一致 | `get_openclaw_tools` |
| `openclaw.tools.set` | tools config 保存一致 | `set_openclaw_tools` |
| `omo.read_local` | OMO local file 读取一致 | `read_omo_local_file` |
| `omo.current_provider` | 当前 OMO provider 一致 | `get_current_omo_provider_id` |
| `omo.disable` | 禁用 OMO 副作用一致 | `disable_current_omo` |
| `omo_slim.read_local` | OMO Slim local file 读取一致 | `read_omo_slim_local_file` |
| `omo_slim.current_provider` | 当前 OMO Slim provider 一致 | `get_current_omo_slim_provider_id` |
| `omo_slim.disable` | 禁用 OMO Slim 副作用一致 | `disable_current_omo_slim` |

### Env

| Case ID | Case | 涉及命令 |
|---|---|---|
| `env.check` | 环境变量冲突扫描一致 | `check_env_conflicts` |
| `env.delete` | 删除冲突变量及备份结果一致 | `delete_env_vars` |
| `env.restore` | 恢复 env backup 一致 | `restore_env_backup` |

### Workspace + Sessions

| Case ID | Case | 涉及命令 |
|---|---|---|
| `session.list` | sessions 列表一致 | `list_sessions` |
| `session.messages` | session message 读取一致 | `get_session_messages` |
| `workspace.memory.list` | daily memory 文件列表一致 | `list_daily_memory_files` |
| `workspace.memory.read` | daily memory 内容读取一致 | `read_daily_memory_file` |
| `workspace.memory.write` | daily memory 写入一致 | `write_daily_memory_file` |
| `workspace.memory.search` | daily memory 搜索一致 | `search_daily_memory_files` |
| `workspace.memory.delete` | daily memory 删除一致 | `delete_daily_memory_file` |
| `workspace.file.read` | workspace file 读取一致 | `read_workspace_file` |
| `workspace.file.write` | workspace file 写入一致 | `write_workspace_file` |

### Global Proxy

| Case ID | Case | 涉及命令 |
|---|---|---|
| `global_proxy.url.get` | 全局代理 URL 读取一致 | `get_global_proxy_url` |
| `global_proxy.url.set` | 全局代理 URL 保存一致 | `set_global_proxy_url` |
| `global_proxy.test` | URL 探测结果结构一致 | `test_proxy_url` |
| `global_proxy.upstream_status` | 上游代理状态一致 | `get_upstream_proxy_status` |
| `global_proxy.scan_local` | 本地代理扫描结果一致 | `scan_local_proxies` |

## P2 Shell Smoke Cases

这些命令不做 core parity，只做壳层 smoke：

| Case ID | Case | 涉及命令 |
|---|---|---|
| `shell.pick_directory` | 文件夹选择器可打开且返回可用路径 | `pick_directory` |
| `shell.save_file_dialog` | 导出保存对话框可返回路径 | `save_file_dialog` |
| `shell.open_file_dialog` | 打开文件对话框可返回路径 | `open_file_dialog` |
| `shell.open_zip_file_dialog` | ZIP 选择器可返回路径 | `open_zip_file_dialog` |
| `shell.open_external` | 外链可被成功交给系统 | `open_external` |
| `shell.open_config_folder` | 配置目录能被打开 | `open_config_folder` |
| `shell.open_app_config_folder` | app config 目录能被打开 | `open_app_config_folder` |
| `shell.open_workspace_directory` | workspace 目录能被打开 | `open_workspace_directory` |
| `shell.provider_terminal` | provider terminal 拉起成功 | `open_provider_terminal` |
| `shell.session_terminal` | session terminal 拉起成功 | `launch_session_terminal` |
| `shell.tray_menu.update` | 托盘菜单刷新命令可执行 | `update_tray_menu` |
| `shell.restart_app` | restart 命令能正常触发 | `restart_app` |
| `shell.set_window_theme` | 主题切换命令可执行 | `set_window_theme` |
| `shell.misc.portable_mode` | portable mode 信息可读 | `is_portable_mode` |
| `shell.misc.tool_versions` | tool version 查询可执行 | `get_tool_versions` |
| `shell.misc.updates` | update check 可执行 | `check_for_updates` |

## P3 End-to-End Flow Cases

这些 case 用来防止“单命令都对，但组合流程错了”。

| Case ID | Flow |
|---|---|
| `flow.provider_switch_live_sync` | 新增 provider -> switch -> live config 更新 -> 再读 current provider |
| `flow.provider_universal_sync` | universal provider 保存 -> sync -> 目标 app provider 列表更新 |
| `flow.mcp_round_trip` | 新增 MCP -> toggle app -> 写入 live config -> import back |
| `flow.plugin_settings_round_trip` | 应用 plugin/onboarding 设置 -> 文件副作用 -> 再读取设置确认 |
| `flow.prompt_round_trip` | 新增 prompt -> enable -> live file 改变 -> import back |
| `flow.skill_round_trip` | 安装 skill -> toggle app -> app 目录同步 -> uninstall |
| `flow.import_export_round_trip` | export -> reset -> import -> provider/prompt/mcp 恢复 |
| `flow.deeplink_round_trip` | parse -> merge -> import unified -> 数据落库 + live 文件更新 |
| `flow.proxy_takeover_switch_restore` | start proxy -> enable takeover -> switch proxy provider -> stop/restore |
| `flow.failover_runtime` | 加入 failover 队列 -> 自动 failover 开启 -> queue/status 读取正确 |
| `flow.stream_check_config_and_run` | 保存 stream check config -> 单测 -> 批量测 -> 结果可读 |
| `flow.usage_from_proxy_traffic` | 真实 proxy 流量 -> usage logs/detail/summary 可读 |
| `flow.settings_side_effects` | save settings -> plugin/onboarding/override dir/live sync 副作用一致 |
| `flow.webdav_sync_round_trip` | save webdav settings -> upload -> wipe -> download -> 状态恢复 |
| `flow.openclaw_config_round_trip` | default model/catalog/env/tools 写入后再次读取一致 |
| `flow.omo_round_trip` | 写当前 provider -> 读取 local file -> disable -> 文件恢复 |
| `flow.workspace_memory_round_trip` | write/search/read/delete daily memory 文件 |
| `flow.env_backup_round_trip` | check -> delete env vars -> restore env backup |

## 建议执行顺序

### 第一阶段

- 建 baseline
- 先把 `Provider / MCP / Prompt` 的当前行为冻结下来

### 第二阶段

- 抽 bridge
- 开始 `Provider / MCP / Prompt` parity

### 第三阶段

- `Skill / Usage / Proxy / Failover`

### 第四阶段

- `Settings / Config / Import-Export / Backup / WebDAV / Deeplink`

### 第五阶段

- `Workspace / Sessions / OpenClaw / OMO / GlobalProxy`

### 第六阶段

- 跑 `P3 End-to-End Flow`
- 再跑 `P2 Shell Smoke`

## 建议的交付顺序

1. 先写 `provider baseline`
2. 再把 `src-tauri/src/bridges` 抽出来
3. 再写 `provider parity`
4. 用 provider 这套模式复制到其他域
5. 每完成一个域，就把对应 command 切到 core bridge
6. 每切一个域，就跑该域 baseline 回归 + parity + 一条 GUI smoke

## 一句话结论

Tauri 命令层的切换，不应该靠“页面点起来没坏”来判断，而应该按这个顺序：

- 先稳住 `src-tauri baseline`
- 命令契约 parity
- 副作用 parity
- 跨域 flow parity

四步一起保证。
