# CC Switch local fixed build

中文说明见下方。

This is a personal source fork based on the official `v3.20.0` tag
(`0b5da510168914b251481654a568c3ffacd62cf4`). It is not an official CC Switch
release.

## What changes

- Automatic local session-usage scanning is opt-in and defaults to off.
- A General settings toggle can re-enable startup and 60-second session scans.
- Provider switching, proxy features, session management, manual usage sync,
  and manual Codex usage rebuild remain available.
- Startup database-only usage-cost backfill remains enabled and does not read
  Claude, Codex, Gemini, or other local session logs.
- Application self-update is disabled in the UI and backend. The Tauri updater
  capability and runtime plugin registration are removed, and updater artifacts
  are not generated.
- The automatic worker checks the setting again after acquiring the shared sync
  lock, so disabling the toggle also cancels a queued scan.

## Build

The repository pins Rust 1.95 and uses pnpm.

```bash
pnpm install --frozen-lockfile
pnpm tauri build --bundles app
```

On macOS 27, symbol stripping produced process-macro dynamic libraries rejected
by `dyld` because of LINKEDIT string-table alignment. This fork therefore uses
`strip = "none"` for reproducible local release builds. The resulting app bundle
is slightly larger.

## Focused verification

```bash
cargo test --manifest-path src-tauri/Cargo.toml automatic_session_usage_sync_is_opt_in --lib
cargo test --manifest-path src-tauri/Cargo.toml local_fixed_build_disables_app_updates --lib
pnpm exec tsc --noEmit
```

## 中文说明

这是基于官方 `v3.20.0` 标签制作的个人固定版源码 fork，不是 CC Switch
官方发行版。

- 本地会话用量自动扫描改为默认关闭，可在“设置 → 通用”中重新开启。
- 关闭后不会在启动时或每 60 秒读取 Claude、Codex、Gemini 等会话日志。
- 供应商切换、代理、会话管理、手动同步和 Codex 用量重建仍可使用。
- 仅操作应用数据库的历史费用回填仍会正常执行。
- 应用自更新已从界面、后端命令、Tauri 权限和插件注册层停用，避免补丁被
  上游版本覆盖。
- 本分支不会生成在线更新器制品；需要升级时应手动切换到其他版本。
