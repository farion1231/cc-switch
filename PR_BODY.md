## Summary / 概述

<!-- Briefly describe what this PR does and why. / 简要描述这个 PR 做了什么以及为什么。 -->

**EN** — Adds **DevEco Code** as a supported harness.

DevEco Code (`@deveco/deveco-code`) is HarmonyOS's AI coding agent. It is a
secondary-development fork of **OpenCode**, so its provider and MCP schemas are
identical — the differences are paths and filenames:

| Aspect | OpenCode | DevEco Code |
|---|---|---|
| Config dir | `~/.config/opencode` | `~/.config/deveco` (`DEVECO_CONFIG_DIR`) |
| Config file | `opencode.json` | `deveco.jsonc` → `deveco.json` → `config.json` |
| Data dir | `~/.local/share/opencode` | `~/.local/share/deveco` (`XDG_DATA_HOME`) |
| DB | `opencode.db` | `deveco.db` (`DEVECO_DB`) |
| Global instructions | `AGENTS.md` | `<config>/AGENTS.md` |
| Resume | `opencode -s <id>` | `deveco --session <id>` |

Scope, as agreed before implementation:

- **Provider management** — additive mode, like OpenCode/Mcode. The native file
  stays the source of truth for what is live; the DB is a catalog.
- **MCP configuration** — reuses the existing OpenCode converters
  (`local`/`remote` ↔ `stdio`/`sse`), which is possible precisely because the
  schemas are identical.
- **Skills management** — deploys to `<config>/skills`, grouped with OpenCode
  (standard symlink/copy, participates in auto-cleanup).
- **Global instructions** — writes `<config>/AGENTS.md`.
- **Session history (read-only)** and **usage import** (incremental watermark)
  from `deveco.db`.
- **No local proxy takeover** — `proxy/providers` returns `None` for DevEco.

Notable implementation decisions:

- The config file is **probed**, not hardcoded. DevEco loads the first existing
  file in that order, so writing a new `deveco.jsonc` when the user's config
  lives in `deveco.json` would silently shadow it.
- `OpenCodeProviderConfig` could not be reused: it requires `npm` and
  `OpenCodeModel.name`, neither of which native DevEco configs reliably have.
  Deserialization would fail and fall through to the raw-JSON path, breaking
  both the typed form and import — hence `DevEcoProviderConfig`.
- `DevEcoProviderConfig` / `DevEcoModel` carry flattened `extra` maps so a
  typed round-trip does not drop `provider.api`, `env`, `whitelist`/`blacklist`
  or `limit.input`. Unknown fields do not make Serde fail, so the raw-JSON
  fallback would never fire — lossless preservation has to be explicit.
- The protocol picker writes `npm`, not an `api` enum: DevEco picks its SDK from
  `provider.npm`, while `provider.api` is the endpoint URL.
- Deleting a provider (or model) that is still referenced by `model`,
  `small_model` or `agent.*.model` is **refused**, since it would leave a
  dangling default.
- Session deletion is **refused**: DevEco sessions are co-owned with the event
  log / todos / projections tables, so removing a row leaves suspended state.
- **DB schema**: `SCHEMA_VERSION` 20 → **21** (upstream's 20 → 21 numbering was
  already taken by the Pi MCP column), adding `enabled_deveco` to `mcp_servers`
  and `skills`. Both columns coexist in the DDL for fresh installs, and the
  migration chain is 19 → 20 (upstream, `enabled_pi`) → 21 (this PR).

**中文** — 新增 **DevEco Code** harness 支持。

DevEco Code 是 HarmonyOS 的 AI 编程 Agent，为 **OpenCode 的二次开发分支**，provider 与
MCP 结构与 OpenCode 完全同构，差异仅在路径与文件名（见上表）。

实现范围：供应商管理（累加模式，原生文件为准、数据库作目录）、MCP 配置（直接复用
OpenCode 转换器）、Skills 统一管理（落到 `<config>/skills`）、全局指令 AGENTS.md、
会话历史只读 + 用量增量导入；**不做**本地代理接管。

几处关键判断：

- 配置文件采用**探测**而非硬编码：DevEco 按 `jsonc → json → config.json` 顺序加载首个
  存在者，若固定写 `deveco.jsonc` 而用户配置在 `deveco.json`，会凭空多出遮蔽文件。
- 不能直接复用 `OpenCodeProviderConfig`：它要求 `npm` 与 `OpenCodeModel.name` 必填，
  而原生 DevEco 配置常省略，反序列化会失败并退化到 raw JSON 路径，导致类型化表单与
  导入双双失效。
- 类型上用 `flatten` 的 `extra` 保留未建模字段：未知字段不会让 Serde 报错，兜底路径
  不会触发，因此无损必须显式做。
- 协议选择写 `npm`（DevEco 由此选 SDK），`api` 是端点 URL，不能复用 Mcode 的枚举。
- 删除仍被 `model` / `small_model` / `agent.*.model` 引用的 provider 或 model 会被
  **拒绝**，避免留下悬挂的默认模型引用。
- 会话删除**拒绝**：DevEco 会话与 event 日志 / todos / projections 等表共同持有，
  直接删行会留下悬挂状态。
- **数据库 schema**：`SCHEMA_VERSION` 20 → **21**（上游的 20 → 21 已被 Pi 的 MCP 列
  占用），给 `mcp_servers` 与 `skills` 增加 `enabled_deveco`。全新安装的 DDL 同时含有
  两列，迁移链为 19 → 20（上游，`enabled_pi`）→ 21（本 PR）。

---

## Related Issue / 关联 Issue

<!-- Link the related issue. Use "Fixes #123" to auto-close it when merged. -->

Fixes #7679

---

## Screenshots / 截图

<!-- If applicable, add before/after screenshots. / 如有需要，请添加修改前后的截图。 -->

| Before / 修改前 | After / 修改后 |
|-----------------|---------------|
| DevEco Code 不可用 | Provider 页导入原生 provider，显示模型与 limit |
| — | 会话页列出 `deveco.db` 中的会话，读取消息；删除入口禁用 |
| — | MCP / Skills 页可启用 DevEco，写入 `deveco.jsonc` 的 `mcp` 段 |
| — | 用量面板出现 `DevEco Code (Session)` |

> ⚠️ **Screenshots are not attached** — the GUI smoke test has not been run
> locally against real DevEco data. Automated coverage is listed below.

> ⚠️ **尚未附截图** —— GUI 手动冒烟尚未在本机真实数据上执行，自动化覆盖见下。

---

## Checklist / 检查清单

- [x] `pnpm typecheck` passes / 通过 TypeScript 类型检查
- [x] `pnpm format:check` passes / 通过代码格式检查
- [x] `cargo clippy` passes (if Rust code changed) / 通过 Clippy 检查（如修改了 Rust 代码）
- [x] Updated i18n files if user-facing text changed / 如修改了用户可见文本，已更新国际化文件
- [x] `cargo fmt --check` passes / 通过 Rust 格式检查

### Verification / 验证

| Check | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --all-targets -- -D warnings` | pass, no warnings |
| `cargo nextest run --profile ci` (what CI runs) | 3663 passed, 5 failed |
| `cargo test --lib deveco` | 19 passed |
| `npx tsc --noEmit` | pass |
| `prettier --check "src/**/*.{js,jsx,ts,tsx,css,json}"` | pass |
| `vitest run` (serial) | 200/203 files |

The 5 Rust failures are **pre-existing and Windows-only** — four are
`os error 1314` (`SeCreateSymbolicLinkPrivilege` not granted) and one is
`os error 10048` (port already in use). None touch DevEco code; they pass on the
Linux/macOS CI runners. The 3 frontend files that fail only in the full parallel
run pass individually, and the same files fail on an unmodified `main`.

i18n: all **four** locale files updated (`en` / `zh` / `zh-TW` / `ja`) with
`apps.deveco`, `usage.appFilter.deveco`, `mcp.unifiedPanel.apps.deveco`, plus
`settings.devecoConfigDir` / `devecoConfigDirDescription` /
`browsePlaceholderDeveco`. `tests/config/localeCoverage.test.ts` passes.

Docs: `README.md` / `README_DE.md` / `README_JA.md` / `README_ZH.md` (tool count
ten → eleven and the tool lists, incl. the feature matrix) and
`docs/user-manual/{en,ja,zh}/5-faq/5.1-config-files.md`.

---

## Review feedback addressed / 已处理的审查意见

The 2026-09-21 automated review (2×P1, 3×P2) is addressed in
`24f7824a fix(deveco): preserve provider config and harden reference guards`:

- **P1 lossless write-back** → flattened `extra` maps on the provider/model types.
- **P1 protocol selection** → the picker now writes `npm`; presets translate the
  Mcode `api` enum into a real DevEco package. Legacy `api` migration is
  deliberately narrow: a real `api` URL is preserved.
- **P2 reference guard on remove-from-live** → `remove_from_live_config` now
  refuses a referenced provider, so the UI card's "remove from config" cannot
  bypass the protection `delete` applies.
- **P2 usage watermark** → only completed messages are imported and the
  watermark stops at a pending row instead of stepping over it.
- **P2 live-cache invalidation** → `invalidateDevEcoProviderCaches` wired into
  add / switch / delete / remove-from-live.

---

## Two things worth flagging / 两点需要提请注意

1. **Icon licensing (needs a maintainer decision).**
   `src/icons/extracted/deveco.png` was extracted from
   `DevEco Studio/bin/devecostudio.ico` — a proprietary Huawei application
   icon. The repository already ships 100+ third-party brand marks, so this
   follows existing practice, but redistributing a proprietary app icon in a
   packaged installer is a judgement call. If it is not acceptable, the wiring
   is a one-line change at six call sites (all resolve the icon name through
   `AppGlyph` / `appConfig`), so swapping in a self-drawn mark is cheap.

2. **The GUI smoke test has not been run.** Automated tests cover the config
   probe order, native-write rollback, MCP round-trip, session scan and the
   usage watermark, but nobody has launched the app against real DevEco data.

---

## Notes for reviewers / 给审查者的说明

- The branch was rebased onto current `main` (7 linear commits, no merge
  commits). The conflict of note: upstream's v20 migration adds `enabled_pi`
  while this PR's added `enabled_deveco` for the same version number — hence
  the renumber to 21. Merging naively would have left already-upgraded users
  without the DevEco column.
- Two files this PR originally touched (`AppSwitcher.tsx`,
  `AppVisibilitySettings.tsx`) were deleted upstream and rebuilt as
  `shell/AppGlyph.tsx` and `apps/AppsPage.tsx`. DevEco is registered in the new
  locations rather than resurrecting the old components.
- Version numbers and `CHANGELOG.md` are intentionally untouched: upstream bumps
  them in release commits, and the mcode addition (#7383) did not touch them
  either.
