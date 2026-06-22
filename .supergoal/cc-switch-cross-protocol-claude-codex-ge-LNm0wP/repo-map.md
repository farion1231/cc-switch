# Repo map

_Generated 2026-06-20 18:49:10_

## Top-level layout
- CHANGELOG.md
- CODE_OF_CONDUCT.md
- CONTRIBUTING.md
- LICENSE
- README.md
- README_DE.md
- README_JA.md
- README_ZH.md
- SECURITY.md
- SUPPORT.md
- assets
- components.json
- deplink.html
- docs
- flatpak
- node_modules
- package.json
- pnpm-lock.yaml
- pnpm-workspace.yaml
- postcss.config.cjs
- rust-toolchain.toml
- scripts
- session-manager.md
- src
- src-tauri
- tailwind.config.cjs
- tests
- tsconfig.json
- tsconfig.node.json
- vite.config.ts
- vitest.config.ts

## Source directories (depth 2)
### `src/`
- src/assets
- src/assets/icons
- src/components
- src/components/agents
- src/components/common
- src/components/deeplink
- src/components/env
- src/components/hermes
- src/components/icons
- src/components/mcp
- src/components/openclaw
- src/components/prompts
- src/components/providers
- src/components/proxy
- src/components/sessions
- src/components/settings
- src/components/skills
- src/components/ui
- src/components/universal
- src/components/usage
- src/components/workspace
- src/config
- src/contexts
- src/hooks
- src/i18n
- src/i18n/locales
- src/icons
- src/icons/extracted
- src/lib
- src/lib/api

## File counts (top extensions)
- `.rs`: 212 files
- `.tsx`: 195 files
- `.ts`: 174 files
- `.md`: 160 files
- `.png`: 137 files
- `.svg`: 76 files
- `.jpg`: 15 files
- `.yml`: 12 files
- `.json`: 12 files
- `.jpeg`: 3 files

## Largest source files (top 15 by line count)
- `src-tauri/src/services/proxy.rs` (6126 lines)
- `src-tauri/src/commands/misc.rs` (5281 lines)
- `src-tauri/src/services/usage_stats.rs` (4003 lines)
- `src-tauri/icons/icon.icns` (3860 lines)
- `src-tauri/src/proxy/forwarder.rs` (3521 lines)
- `src-tauri/src/proxy/providers/transform_codex_chat.rs` (3282 lines)
- `src-tauri/src/services/skill.rs` (3127 lines)
- `src-tauri/src/services/provider/mod.rs` (2822 lines)
- `src-tauri/src/proxy/handlers.rs` (2679 lines)
- `src-tauri/src/proxy/providers/claude.rs` (2616 lines)
- `src-tauri/src/codex_history_migration.rs` (2595 lines)
- `src-tauri/src/database/schema.rs` (2574 lines)
- `src-tauri/src/codex_config.rs` (2466 lines)
- `src/components/providers/forms/ProviderForm.tsx` (2414 lines)
- `src-tauri/src/proxy/providers/transform_gemini.rs` (2310 lines)

## Test surface
- Directories named `test`: 25
- Directories named `tests`: 15
- Directories named `__tests__`: 20
- Directories named `spec`: 1
- Test files (by name pattern): 409

## Notable config / infra
- `.github/workflows`
- `pnpm-workspace.yaml`
- `postcss.config.cjs`
- `tailwind.config.cjs`
- `tsconfig.json`
- `vite.config.ts`
- `vitest.config.ts`

## Recent activity (last 10 commits)
- `0bb3b751` 2026-06-16 feat(usage): support importing model pricing from models.dev (#4079)
- `81d6002a` 2026-06-16 修复 添加供应商页面 搜索预设后无法点击选中搜索结果 (#4315)
- `caa912e3` 2026-06-16 fix: prevent duplicate codex base_url entries (#4316)
- `1042fb2a` 2026-06-16 fix(terminal): respect user shell for provider terminals (#4140)
- `de0a149d` 2026-06-16 feat(session-manager): show source file name in session detail header (#4113)
- `36b557b2` 2026-06-16 fix(codex): restore cached tool call fields (#4160)
- `3e38889c` 2026-06-16 feat(proxy): strip effort params when thinking:disabled for DeepSeek endpoint (#4239)
- `12567b32` 2026-06-16 fix(settings): reset scroll on tab switch (#4165)
- `c548e7fc` 2026-06-15 docs(guides): add trilingual Codex unified session-history guide
- `21e695f6` 2026-06-15 chore(release): prepare v3.16.3

## Files churned in last 20 commits (top 10)
- `src/components/settings/AboutSection.tsx` (4×)
- `src/i18n/locales/zh.json` (3×)
- `src/i18n/locales/zh-TW.json` (3×)
- `src/i18n/locales/ja.json` (3×)
- `src/i18n/locales/en.json` (3×)
- `src/config/opencodeProviderPresets.ts` (2×)
- `src/config/openclawProviderPresets.ts` (2×)
- `src/config/hermesProviderPresets.ts` (2×)
- `src/config/codexProviderPresets.ts` (2×)
- `src/config/claudeProviderPresets.ts` (2×)

_End repo map._
