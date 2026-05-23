# Implementation Plan: Pi CLI 配置管理支持

**Branch**: `001-pi-cli-support` | **Date**: 2026-05-23 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/001-pi-cli-support/spec.md`

## Summary

为 CC Switch 添加 Pi CLI 工具的完整配置管理支持。主要包括：新增 `AppType::Pi` 枚举值并在 UI 中展示 Pi 选项卡、通过双文件（models.json + settings.json）写入策略管理 Pi 的提供商配置、将 Pi 集成到现有的 Skills 和 Context Files 管理系统中、提供 Pi 常用设置的图形化编辑界面。Pi 采用累加模式（additive mode），所有提供商配置共存在 models.json 中。

## Technical Context

**Language/Version**: Rust 1.85+ (backend via Tauri 2), TypeScript 5.3+ (frontend via React 18)

**Primary Dependencies**: Tauri 2.8, rusqlite 0.31, serde/serde_json, React 18, Tailwind CSS 3, Radix UI, shadcn/ui

**Storage**: CC Switch SQLite (providers DB) + Pi config files: `~/.pi/agent/models.json`, `~/.pi/agent/settings.json`

**Testing**: Vitest + Testing Library (frontend), `cargo test` (Rust backend)

**Target Platform**: Windows, macOS, Linux (Tauri 2 desktop app)

**Project Type**: Desktop application (Tauri 2: Rust backend + React/TypeScript frontend)

**Performance Goals**: Provider switch write < 100ms; UI tab switch < 50ms; config file read/merge/write < 200ms

**Constraints**: Must not corrupt existing Pi configurations; atomic writes required; i18n required (zh/en/ja); must preserve user's manually added models.json entries

**Scale/Scope**: 1 new tool (Pi) in existing 6-tool system; ~10 provider presets; ~8 settings fields in UI

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evidence |
|-----------|--------|----------|
| I. Desktop-First, Cross-Platform | ✅ PASS | Pi config directory (`~/.pi/agent/`) is cross-platform; all paths use `dirs`/home-dir resolution |
| II. Non-Intrusive Architecture | ✅ PASS | CC Switch writes to Pi configs using `cc-switch-` prefixed namespace; user's existing models.json entries are preserved via merge strategy; uninstalling CC Switch does not affect Pi functionality |
| III. Data Integrity & Safety | ✅ PASS | Atomic write strategy (temp file → validate → rename); backup before destructive writes; SQLite as SSOT for CC Switch providers; merge-not-overwrite for models.json |
| IV. Unified Management Interface | ✅ PASS | Pi tab integrates into existing sidebar tab system; provider management follows existing UX patterns; Skills and Context Files reuse existing panels |
| V. Internationalization & Accessibility | ✅ PASS | All new UI strings will have zh/en/ja translations; Pi tab follows existing keyboard navigation patterns |

## Project Structure

### Documentation (this feature)

```text
specs/001-pi-cli-support/
├── plan.md              # This file
├── research.md          # Phase 0: Pi configuration analysis
├── data-model.md        # Phase 1: Entity changes
├── quickstart.md        # Phase 1: User quickstart guide
├── contracts/           # Phase 1: Output format contracts
│   └── models-json-format.md
└── tasks.md             # Phase 2: (/speckit.tasks command)
```

### Source Code (repository root)

```text
src-tauri/src/                    # Rust backend
├── app_config.rs                 # [MODIFY] AppType::Pi, McpApps, SkillApps
├── settings.rs                   # [MODIFY] VisibleApps, current_provider_pi, pi_config_dir
├── pi_config.rs                  # [NEW] Pi 配置文件读写模块
├── commands/
│   ├── mod.rs                    # [MODIFY] Register pi commands
│   └── pi.rs                     # [NEW] Pi Tauri commands
├── database/
│   └── dao/
│       ├── mod.rs                # [MODIFY] Register pi DAO functions
│       ├── providers_seed.rs     # [MODIFY] Pi provider presets
│       └── pi_providers.rs       # [NEW] Pi provider CRUD operations
├── services/
│   └── skill.rs                  # [MODIFY] Register pi skill sync target
└── lib.rs                        # [MODIFY] Register pi module

src/                              # React/TypeScript frontend
├── types.ts                      # [MODIFY] AppType 'pi'
├── components/
│   ├── providers/
│   │   └── PiProviderCard.tsx    # [NEW] Pi 提供商卡片
│   ├── settings/
│   │   └── PiSettings.tsx        # [NEW] Pi 设置面板
│   └── layout/
│       └── Sidebar.tsx           # [MODIFY] Add Pi tab
├── hooks/
│   └── usePiConfig.ts            # [NEW] Pi 配置 hooks
├── contexts/
│   └── AppContext.tsx            # [MODIFY] Add Pi context state
├── i18n/
│   ├── zh.json                   # [MODIFY] Pi 相关中文字符串
│   ├── en.json                   # [MODIFY] Pi 相关英文字符串
│   └── ja.json                   # [MODIFY] Pi 相关日文字符串
└── lib/
    └── piDefaults.ts             # [NEW] Pi 默认配置/预设

tests/
└── unit/
    └── piConfig.test.ts          # [NEW] Pi 配置前端测试
```

**Structure Decision**: Follow the existing Tauri 2 project structure. Each tool gets a backend config module (`pi_config.rs`), Tauri commands, and frontend components following the same patterns as OpenCode and OpenClaw.

## Complexity Tracking

> No constitution violations — table intentionally empty.
