# CLI Gap Todo

## P0 Core 先补

- [x] 把 Sessions 能力下沉到 `cc-switch-core`，提供统一的 `SessionService` 用于扫描会话和读取消息。
- [x] 把 Global Outbound Proxy 的 `get/set/test/scan` 收口到 `cc-switch-core`，不要继续只挂在 Tauri command 上。
- [x] 给 `cc-switch-core` 增加 Claude plugin integration / skip onboarding 的 adapter 或 service 边界。
- [x] 给 `cc-switch-core` 增加真正可复用的 `SettingsService` / `HostService`，统一承接 GUI 结构化设置流程。

## P1 CLI 直接接现有 Core

- [x] 给 `provider` 增加 duplicate / sort-order / remove-from-live / import-live / read-live 子命令。
- [x] 给 `provider` 增加 custom-endpoints / endpoint-last-used / endpoint-speedtest 子命令。
- [x] 给 `provider` 增加 common-config-snippet extract/get/set 子命令。
- [x] 给 `provider` 增加 usage-script save/test/query 的完整子命令面。
- [x] 给 `provider` 增加 stream-check single/all/config 子命令。
- [x] 给 `provider` 增加 OpenClaw default-model / model-catalog 相关子命令或单独的 `openclaw` 命令组。
- [x] 给 `provider universal` 增加 edit / save-and-sync / 更完整结果回显。
- [x] 给 `usage` 增加 trends / provider-stats / model-stats / request-detail 子命令。
- [x] 给 `usage` 增加 model-pricing list/update/delete 子命令。
- [x] 给 `usage` 增加 provider-limits check 子命令。
- [x] 给 `skill` 增加 unmanaged scan/import 子命令。
- [x] 给 `skill` 增加 repo list/add/remove 子命令。
- [x] 给 `skill` 增加 zip-install 子命令。
- [x] 给 `proxy` 增加 auto-failover enable/config 子命令。
- [x] 给 `proxy` 增加 provider-health / circuit-stats / available-providers 子命令。
- [x] 给 `proxy` 增加 default-cost-multiplier / pricing-model-source / global-proxy-config 子命令。
- [x] 给 `mcp` 补 validate / docs-link / richer app toggle 输出，做到和 GUI 操作闭环一致。
- [x] 给 `prompt` 增加 current-live-file-content 查看能力。
- [x] 给 `deeplink` 增加 parse / merge / preview 子命令，不再只有最终 import。
- [x] 给 `workspace` 增加 read/write/list-memory/search-memory/delete-memory 子命令。
- [x] 给 `workspace` 补 open-dir 或等价的显式路径能力。
- [x] 给 `webdav` 增加 test/save/upload/download/fetch-remote-info 子命令。
- [x] 给 `backup` 增加 create/list/restore/rename/delete 子命令。
- [x] 给 `env` 增加 check/delete/restore 子命令。
- [x] 给 `omo` / `omo-slim` 增加 read-local / import-local / current / disable-current 子命令。
- [x] 给 `openclaw` 增加 env / tools / agents-defaults / default-model / model-catalog 子命令。

## P2 壳层能力与信息面

- [x] 给 CLI 增加 sessions list/messages/resume-command 能力。
- [x] 给 CLI 增加 settings structured subcommands，覆盖 language / visible-apps / terminal / startup / plugin / onboarding。
- [x] 给 CLI 增加 auto-launch / portable-mode / tool-versions 命令。
- [x] 给 CLI 增加 update / release-notes / about 信息命令。

## 当前阶段明确后置

- [ ] `sessions terminal launch` 继续留在壳层，不纳入当前 `core + CLI` 收口范围。
- [ ] GUI 文件对话框能力不做 CLI 1:1 复刻，后续只在确有必要时补显式路径参数。
- [ ] `open-external / open-folder / terminal-launch` 继续视为壳层能力，不下沉到 `core`。

## 通用收尾

- [x] 每补完一个功能域就补对应的 CLI 黑盒测试和 `qa/cli-e2e` 场景。
- [x] 每补完一个功能域就回写 `cli-gap.md` 的状态，避免文档和代码漂移。
- [x] 每一批命令补完后都跑 `cargo test -p cc-switch-core`、`cargo test -p cc-switch-cli`、`qa/cli-e2e`。
- [x] 在进入 Tauri 迁移前，先把这份 checklist 中的 P0 和 P1 清空。
