<div align="center">

# Claude Code & Codex Provider Switcher

[![Version](https://img.shields.io/badge/version-3.6.0-blue.svg)](https://github.com/farion1231/cc-switch/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/farion1231/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

English | [中文](README_ZH.md) | [Changelog](CHANGELOG.md)

A desktop application for managing and switching between different provider configurations & MCP for Claude Code and Codex.

</div>

## ❤️ Sponsor

![Zhipu GLM](assets/partners/banners/glm-en.jpg)

This project is sponsored by Z.ai, supporting us with their GLM CODING PLAN.

GLM CODING PLAN is a subscription service designed for AI coding, starting at just $3/month. It provides access to their flagship GLM-4.6 model across 10+ popular AI coding tools (Claude Code, Cline, Roo Code, etc.), offering developers top-tier, fast, and stable coding experiences.

Get 10% OFF the GLM CODING PLAN with [this link](https://z.ai/subscribe?ic=8JVLJQFSKB)!

## Release Notes

> **v3.6.0**: Added edit mode (provider duplication, manual sorting), custom endpoint management, usage query features. Optimized config directory switching experience (perfect WSL environment support). Added multiple provider presets (DMXAPI, Azure Codex, AnyRouter, AiHubMix, MiniMax). Completed full-stack architecture refactoring and testing infrastructure.

> v3.5.0: Added MCP management, config import/export, endpoint speed testing. Complete i18n coverage. Added Longcat and kat-coder presets. Standardized release file naming conventions.

> v3.4.0: Added i18next internationalization, support for new models (qwen-3-max, GLM-4.6, DeepSeek-V3.2-Exp), Claude plugin, single-instance daemon, tray minimize, and installer optimizations.

> v3.3.0: One-click VS Code Codex plugin configuration/removal (auto-sync by default), Codex common config snippets, enhanced custom wizard, WSL environment support, cross-platform tray and UI optimizations. (VS Code write feature deprecated in v3.4.x)

> v3.2.0: Brand new UI, macOS system tray, built-in updater, atomic write with rollback, improved dark mode, Single Source of Truth (SSOT) with one-time migration/archival.

> v3.1.0: Added Codex provider management with one-click switching. Import current Codex config as default provider. Auto-backup before internal config v1 → v2 migration (see "Migration & Archival" below).

> v3.0.0 Major Update: Complete migration from Electron to Tauri 2.0. Significantly reduced app size and greatly improved startup performance.

## Features (v3.6.0)

### Core Features

- **MCP (Model Context Protocol) Management**: Complete MCP server configuration management system
  - Support for stdio and http server types with command validation
  - Built-in templates for popular MCP servers (e.g., mcp-fetch)
  - Real-time enable/disable MCP servers with atomic file writes to prevent configuration corruption
- **Config Import/Export**: Backup and restore your provider configurations
  - One-click export all configurations to JSON file
  - Import configs with automatic validation and backup, auto-rotate backups (keep 10 most recent)
  - Auto-sync to live config files after import to ensure immediate effect
- **Endpoint Speed Testing**: Test API endpoint response times
  - Measure latency to different provider endpoints with visual connection quality indicators
  - Help users choose the fastest provider
- **Internationalization & Language Switching**: Complete i18next i18n coverage (including error messages, tray menu, all UI components)
- **Claude Plugin Sync**: Built-in button to apply or restore Claude plugin configurations with one click. Takes effect immediately after switching providers.

### v3.6 New Features

- **Provider Duplication**: Quickly duplicate existing provider configs to easily create variants
- **Manual Sorting**: Drag and drop to manually reorder providers
- **Custom Endpoint Management**: Support multi-endpoint configuration for aggregator providers
- **Custom Configuration Directory (Cloud Sync Support)**:
  - Customize CC Switch's configuration storage location
  - Point to cloud sync folders (Dropbox, OneDrive, iCloud, etc.) to enable automatic config synchronization across devices
  - Supports independent management via Tauri Store
- **Claude Configuration Data Structure Enhancements**
  - **Granular Model Configuration**: Migrated from dual-key to quad-key system for better model tier differentiation
    - New fields: `ANTHROPIC_DEFAULT_HAIKU_MODEL`, `ANTHROPIC_DEFAULT_SONNET_MODEL`, `ANTHROPIC_DEFAULT_OPUS_MODEL`, `ANTHROPIC_MODEL`
    - Replaces legacy `ANTHROPIC_SMALL_FAST_MODEL` with automatic migration
    - Backend normalizes old configs on first read/write with smart fallback chain
    - UI expanded from 2 to 4 model input fields with intelligent defaults
  - Support for `ANTHROPIC_API_KEY` field (in addition to `ANTHROPIC_AUTH_TOKEN`)
  - Template variable system for dynamic configuration replacement (e.g., KAT-Coder's ENDPOINT_ID)
  - Endpoint candidates list for speed testing and endpoint management
  - Visual theme configuration (custom icons and colors for provider cards)
  - Partner promotion mechanism with i18n support
- **Updated Provider Models**
  - Kimi: Updated to latest `kimi-k2-thinking` model
- **Usage Query Features**
  - Auto-refresh interval: Supports periodic automatic usage queries
  - Test Script API: Validate JavaScript scripts before execution
  - Template system expansion: Custom blank templates, support for access token and user ID parameters
- **Config Editor Improvements**
  - Added JSON format button
  - Real-time TOML syntax validation (for Codex configs)
- **Auto-sync on Directory Change**: When switching Claude/Codex config directories (e.g., switching to WSL environment), automatically sync current provider to new directory to avoid config file conflicts
- **Load Live Config When Editing Active Provider**: When editing the currently active provider, prioritize displaying the actual effective configuration to protect user manual modifications
- **New Provider Presets**: DMXAPI, Azure Codex, AnyRouter, AiHubMix, MiniMax
- **Partner Promotion Mechanism**: Support ecosystem partner promotion (e.g., Zhipu GLM Z.ai)

### v3.6 Architecture Improvements

- **Backend Refactoring**: Completed 5-phase refactoring (unified error handling → command layer split → integration tests → Service layer extraction → concurrency optimization)
- **Frontend Refactoring**: Completed 4-stage refactoring (test infrastructure → Hooks extraction → component splitting → code cleanup)
- **Testing System**: 100% Hooks unit test coverage, integration tests covering critical flows (vitest + MSW + @testing-library/react)

### System Features

- **System Tray & Window Behavior**: Window can minimize to tray, macOS supports hide/show Dock in tray mode, tray switching syncs Claude/Codex/plugin status.
- **Single Instance**: Ensures only one instance runs at a time to avoid multi-instance conflicts.
- **Standardized Release Naming**: All platform release files use consistent version-tagged naming (macOS: `.tar.gz` / `.zip`, Windows: `.msi` / `-Portable.zip`, Linux: `.AppImage` / `.deb`).

## Screenshots

### Main Interface

![Main Interface](assets/screenshots/main-en.png)

### Add Provider

![Add Provider](assets/screenshots/add-en.png)

## Download & Installation

### System Requirements

- **Windows**: Windows 10 and above
- **macOS**: macOS 10.15 (Catalina) and above
- **Linux**: Ubuntu 22.04+ / Debian 11+ / Fedora 34+ and other mainstream distributions

### Windows Users

Download the latest `CC-Switch-v{version}-Windows.msi` installer or `CC-Switch-v{version}-Windows-Portable.zip` portable version from the [Releases](../../releases) page.

### macOS Users

**Method 1: Install via Homebrew (Recommended)**

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

Update:

```bash
brew upgrade --cask cc-switch
```

**Method 2: Manual Download**

Download `CC-Switch-v{version}-macOS.zip` from the [Releases](../../releases) page and extract to use.

> **Note**: Since the author doesn't have an Apple Developer account, you may see an "unidentified developer" warning on first launch. Please close it first, then go to "System Settings" → "Privacy & Security" → click "Open Anyway", and you'll be able to open it normally afterwards.

### Linux Users

Download the latest `CC-Switch-v{version}-Linux.deb` package or `CC-Switch-v{version}-Linux.AppImage` from the [Releases](../../releases) page.

## Usage Guide

1. Click "Add Provider" to add your API configuration
2. Switching methods:
   - Select a provider on the main interface and click switch
   - Or directly select target provider from "System Tray (Menu Bar)" for immediate effect
3. Switching will write to the corresponding app's "live config file" (Claude: `settings.json`; Codex: `auth.json` + `config.toml`)
4. Restart or open new terminal to ensure it takes effect
5. To switch back to official login, select "Official Login" from presets and switch; after restarting terminal, follow the official login process

### MCP Configuration Guide (v3.5.x)

- Management Location: All MCP server definitions are centrally saved in `~/.cc-switch/config.json` (categorized by client `claude` / `codex`)
- Sync Mechanism:
  - Enabled Claude MCP servers are projected to `~/.claude.json` (path may vary with override directory)
  - Enabled Codex MCP servers are projected to `~/.codex/config.toml`
- Validation & Normalization: Auto-validate field legality (stdio/http) when adding/importing, and auto-fix/populate keys like `id`
- Import Sources: Support importing from `~/.claude.json` and `~/.codex/config.toml`; existing entries only force `enabled=true`, don't override other fields

### Check for Updates

- Click "Check for Updates" in Settings. If built-in Updater config is available, it will detect and download directly; otherwise, it will fall back to opening the Releases page

### Codex Guide (SSOT)

- Config Directory: `~/.codex/`
  - Live main config: `auth.json` (required), `config.toml` (can be empty)
- API Key Field: Uses `OPENAI_API_KEY` in `auth.json`
- Switching Behavior (no longer writes "copy files"):
  - Provider configs are uniformly saved in `~/.cc-switch/config.json`
  - When switching, writes target provider back to live files (`auth.json` + `config.toml`)
  - Uses "atomic write + rollback on failure" to avoid half-written state; `config.toml` can be empty
- Import Default: When the app has no providers, creates a default entry from existing live main config and sets it as current
- Official Login: Can switch to preset "Codex Official Login", restart terminal and follow official login process

### Claude Code Guide (SSOT)

- Config Directory: `~/.claude/`
  - Live main config: `settings.json` (preferred) or legacy-compatible `claude.json`
- API Key Field: `env.ANTHROPIC_AUTH_TOKEN`
- Switching Behavior (no longer writes "copy files"):
  - Provider configs are uniformly saved in `~/.cc-switch/config.json`
  - When switching, writes target provider JSON directly to live file (preferring `settings.json`)
  - When editing current provider, writes live first successfully, then updates app main config to ensure consistency
- Import Default: When the app has no providers, creates a default entry from existing live main config and sets it as current
- Official Login: Can switch to preset "Claude Official Login", restart terminal and use `/login` to complete login

### Migration & Archival

#### v3.6 Technical Improvements

**Internal Optimizations (User Transparent)**:

- **Removed Legacy Migration Logic**: v3.6 removed v1 config auto-migration and copy file scanning logic
  - **Impact**: Improved startup performance, cleaner code
  - **Compatibility**: v2 format configs are fully compatible, no action required
  - ⚠️ **Note**: Users upgrading from v3.1.0 or earlier should first upgrade to v3.2.x or v3.5.x for one-time migration, then upgrade to v3.6

- **Command Parameter Standardization**: Backend unified to use `app` parameter (values: `claude` or `codex`)
  - **Impact**: More standardized code, friendlier error messages
  - **Compatibility**: Frontend fully adapted, users don't need to care about this change

#### Startup Failure & Recovery

- Trigger Conditions: Triggered when `~/.cc-switch/config.json` doesn't exist, is corrupted, or fails to parse.
- User Action: Check JSON syntax according to popup prompt, or restore from backup files.
- Backup Location & Rotation: `~/.cc-switch/backups/backup_YYYYMMDD_HHMMSS.json` (keep up to 10, see `src-tauri/src/services/config.rs`).
- Exit Strategy: To protect data safety, the app will show a popup and force exit when the above errors occur; restart after fixing.

#### Migration Mechanism (v3.2.0+)

- One-time Migration: First launch of v3.2.0+ will scan old "copy files" and merge into `~/.cc-switch/config.json`
  - Claude: `~/.claude/settings-*.json` (excluding `settings.json` / legacy `claude.json`)
  - Codex: `~/.codex/auth-*.json` and `config-*.toml` (merged in pairs by name)
- Deduplication & Current Item: Deduplicate by "name (case-insensitive) + API Key"; if current is empty, set live merged item as current
- Archival & Cleanup:
  - Archive directory: `~/.cc-switch/archive/<timestamp>/<category>/...`
  - Delete original copies after successful archival; keep original files on failure (conservative strategy)
- v1 → v2 Structure Upgrade: Additionally generates `~/.cc-switch/config.v1.backup.<timestamp>.json` for rollback
- Note: After migration, daily switch/edit operations are no longer archived; prepare your own backup solution if long-term auditing is needed

## Architecture Overview (v3.6)

### Architecture Refactoring Highlights (v3.6)

**Backend Refactoring (Rust)**: Completed 5-phase refactoring

- **Phase 1**: Unified error handling (`AppError` + i18n error messages)
- **Phase 2**: Command layer split by domain (`commands/{provider,mcp,config,settings,plugin,misc}.rs`)
- **Phase 3**: Introduced integration tests and transaction mechanism (config snapshot + failure rollback)
- **Phase 4**: Extracted Service layer (`services/{provider,mcp,config,speedtest}.rs`)
- **Phase 5**: Concurrency optimization (`RwLock` instead of `Mutex`, scoped guard to avoid deadlock)

**Frontend Refactoring (React + TypeScript)**: Completed 4-stage refactoring

- **Stage 1**: Established test infrastructure (vitest + MSW + @testing-library/react)
- **Stage 2**: Extracted custom hooks (`useProviderActions`, `useMcpActions`, `useSettings`, `useImportExport`, etc.)
- **Stage 3**: Component splitting and business logic extraction
- **Stage 4**: Code cleanup and formatting unification

**Test Coverage**:

- 100% Hooks unit test coverage
- Integration tests covering critical flows (App, SettingsDialog, MCP Panel)
- MSW mocking backend API to ensure test independence

### Layered Architecture

- **Frontend (Renderer)**
  - Tech Stack: TypeScript + React 18 + Vite + TailwindCSS 4
  - Data Layer: TanStack React Query unified queries and mutations (`@/lib/query`), Tauri API unified wrapper (`@/lib/api`)
  - Business Logic Layer: Custom Hooks (`@/hooks`) carry domain logic, components stay simple
  - Event Flow: Listen to backend `provider-switched` events, drive UI refresh and tray state consistency
  - Organization: Components split by domain (`providers/settings/mcp/ui`)

- **Backend (Tauri + Rust)**
  - **Commands Layer** (Interface Layer): `src-tauri/src/commands/*` split by domain, only responsible for parameter parsing and permission validation
  - **Services Layer** (Business Layer): `src-tauri/src/services/*` carry core logic, reusable and testable
    - `ProviderService`: Provider CRUD, switch, backfill, sorting
    - `McpService`: MCP server management, import/export, sync
    - `ConfigService`: Config file import/export, backup/restore
    - `SpeedtestService`: API endpoint latency testing
  - **Models & State**:
    - `provider.rs`: Domain models (`Provider`, `ProviderManager`, `ProviderMeta`)
    - `app_config.rs`: Multi-app config (`MultiAppConfig`, `AppId`, `McpRoot`)
    - `store.rs`: Global state (`AppState` + `RwLock<MultiAppConfig>`)
  - **Reliability**:
    - Unified error type `AppError` (with localized messages)
    - Transactional changes (config snapshot + failure rollback)
    - Atomic writes (temp file + rename, avoid half-writes)
    - Tray menu & events: Rebuild menu after switch and emit `provider-switched` event to frontend

- **Design Points (SSOT + Dual-way Sync)**
  - **Single Source of Truth**: Provider configs centrally stored in `~/.cc-switch/config.json`
  - **Write on Switch**: Write target provider config to live files (Claude: `settings.json`; Codex: `auth.json` + `config.toml`)
  - **Backfill Mechanism**: Immediately read back live files after switch, update SSOT to protect user manual modifications
  - **Directory Switch Sync**: Auto-sync current provider to new directory when changing config directories (perfect WSL environment support)
  - **Prioritize Live When Editing**: When editing current provider, prioritize loading live config to ensure display of actually effective configuration

- **Compatibility & Changes**
  - Command Parameters Unified: Tauri commands only accept `app` (values: `claude` / `codex`)
  - Frontend Types Unified: Use `AppId` to express app identifiers (replacing legacy `AppType` export)

## Development

### Environment Requirements

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### Development Commands

```bash
# Install dependencies
pnpm install

# Dev mode (hot reload)
pnpm dev

# Type check
pnpm typecheck

# Format code
pnpm format

# Check code format
pnpm format:check

# Run frontend unit tests
pnpm test:unit

# Run tests in watch mode (recommended for development)
pnpm test:unit:watch

# Build application
pnpm build

# Build debug version
pnpm tauri build --debug
```

### Rust Backend Development

```bash
cd src-tauri

# Format Rust code
cargo fmt

# Run clippy checks
cargo clippy

# Run backend tests
cargo test

# Run specific tests
cargo test test_name

# Run tests with test-hooks feature
cargo test --features test-hooks
```

### Testing Guide (v3.6 New)

**Frontend Testing**:

- Uses **vitest** as test framework
- Uses **MSW (Mock Service Worker)** to mock Tauri API calls
- Uses **@testing-library/react** for component testing

**Test Coverage**:

- ✅ Hooks unit tests (100% coverage)
  - `useProviderActions` - Provider operations
  - `useMcpActions` - MCP management
  - `useSettings` series - Settings management
  - `useImportExport` - Import/export
- ✅ Integration tests
  - App main application flow
  - SettingsDialog complete interaction
  - MCP panel functionality

**Running Tests**:

```bash
# Run all tests
pnpm test:unit

# Watch mode (auto re-run)
pnpm test:unit:watch

# With coverage report
pnpm test:unit --coverage
```

## Tech Stack

### Frontend

- **[React 18](https://react.dev/)** - User interface library
- **[TypeScript](https://www.typescriptlang.org/)** - Type-safe JavaScript
- **[Vite](https://vitejs.dev/)** - Lightning fast frontend build tool
- **[TailwindCSS 4](https://tailwindcss.com/)** - Utility-first CSS framework
- **[TanStack Query v5](https://tanstack.com/query/latest)** - Powerful data fetching and caching
- **[react-i18next](https://react.i18next.com/)** - React internationalization framework
- **[react-hook-form](https://react-hook-form.com/)** - High-performance forms library
- **[zod](https://zod.dev/)** - TypeScript-first schema validation
- **[shadcn/ui](https://ui.shadcn.com/)** - Reusable React components
- **[@dnd-kit](https://dndkit.com/)** - Modern drag and drop toolkit

### Backend

- **[Tauri 2.8](https://tauri.app/)** - Cross-platform desktop app framework
  - tauri-plugin-updater - Auto update
  - tauri-plugin-process - Process management
  - tauri-plugin-dialog - File dialogs
  - tauri-plugin-store - Persistent storage
  - tauri-plugin-log - Logging
- **[Rust](https://www.rust-lang.org/)** - Systems programming language
- **[serde](https://serde.rs/)** - Serialization/deserialization framework
- **[tokio](https://tokio.rs/)** - Async runtime
- **[thiserror](https://github.com/dtolnay/thiserror)** - Error handling derive macro

### Testing Tools

- **[vitest](https://vitest.dev/)** - Fast unit testing framework
- **[MSW](https://mswjs.io/)** - API mocking tool
- **[@testing-library/react](https://testing-library.com/react)** - React testing utilities

## Project Structure

```
├── src/                      # Frontend code (React + TypeScript)
│   ├── components/           # React components
│   │   ├── providers/        # Provider management components
│   │   │   ├── forms/        # Form sub-components (Claude/Codex fields)
│   │   │   ├── ProviderList.tsx
│   │   │   ├── ProviderForm.tsx
│   │   │   ├── AddProviderDialog.tsx
│   │   │   └── EditProviderDialog.tsx
│   │   ├── settings/         # Settings related components
│   │   │   ├── SettingsDialog.tsx
│   │   │   ├── DirectorySettings.tsx
│   │   │   └── ImportExportSection.tsx
│   │   ├── mcp/              # MCP management components
│   │   │   ├── McpPanel.tsx
│   │   │   ├── McpFormModal.tsx
│   │   │   └── McpWizard.tsx
│   │   └── ui/               # shadcn/ui base components
│   ├── hooks/                # Custom Hooks (business logic layer)
│   │   ├── useProviderActions.ts    # Provider operations
│   │   ├── useMcpActions.ts         # MCP operations
│   │   ├── useSettings.ts           # Settings management
│   │   ├── useImportExport.ts       # Import/export
│   │   └── useDirectorySettings.ts  # Directory config
│   ├── lib/
│   │   ├── api/              # Tauri API wrapper (type-safe)
│   │   │   ├── providers.ts  # Provider API
│   │   │   ├── settings.ts   # Settings API
│   │   │   ├── mcp.ts        # MCP API
│   │   │   └── usage.ts      # Usage query API
│   │   └── query/            # TanStack Query config
│   │       ├── queries.ts    # Query definitions
│   │       ├── mutations.ts  # Mutation definitions
│   │       └── queryClient.ts
│   ├── i18n/                 # Internationalization resources
│   │   └── locales/
│   │       ├── zh/           # Chinese translations
│   │       └── en/           # English translations
│   ├── config/               # Config & presets
│   │   ├── claudeProviderPresets.ts  # Claude provider presets
│   │   ├── codexProviderPresets.ts   # Codex provider presets
│   │   └── mcpPresets.ts             # MCP server templates
│   ├── utils/                # Utility functions
│   │   ├── postChangeSync.ts         # Config sync utility
│   │   └── ...
│   └── types/                # TypeScript type definitions
├── src-tauri/                # Backend code (Rust)
│   ├── src/
│   │   ├── commands/         # Tauri command layer (split by domain)
│   │   │   ├── provider.rs   # Provider commands
│   │   │   ├── mcp.rs        # MCP commands
│   │   │   ├── config.rs     # Config query commands
│   │   │   ├── settings.rs   # Settings commands
│   │   │   ├── plugin.rs     # Plugin commands
│   │   │   ├── import_export.rs  # Import/export commands
│   │   │   └── misc.rs       # Misc commands
│   │   ├── services/         # Service layer (business logic)
│   │   │   ├── provider.rs   # ProviderService
│   │   │   ├── mcp.rs        # McpService
│   │   │   ├── config.rs     # ConfigService
│   │   │   └── speedtest.rs  # SpeedtestService
│   │   ├── app_config.rs     # Config data models
│   │   ├── provider.rs       # Provider domain models
│   │   ├── store.rs          # Global state management
│   │   ├── mcp.rs            # MCP sync & validation
│   │   ├── error.rs          # Unified error type
│   │   ├── usage_script.rs   # Usage script execution
│   │   ├── claude_plugin.rs  # Claude plugin management
│   │   └── lib.rs            # App entry point
│   ├── capabilities/         # Tauri permission config
│   └── icons/                # App icons
├── tests/                    # Frontend tests (v3.6 new)
│   ├── hooks/                # Hooks unit tests
│   ├── components/           # Component integration tests
│   └── setup.ts              # Test config
└── assets/                   # Static resources
    ├── screenshots/          # Interface screenshots
    └── partners/             # Partner resources
        ├── logos/            # Partner logos
        └── banners/          # Partner banners/promotional images
```

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for version update details.

## Legacy Electron Version

[Releases](../../releases) retains v2.0.3 legacy Electron version

If you need legacy Electron code, you can pull the electron-legacy branch

## Contributing

Issues and suggestions are welcome!

Before submitting PRs, please ensure:

- Pass type check: `pnpm typecheck`
- Pass format check: `pnpm format:check`
- Pass unit tests: `pnpm test:unit`
- Functional PRs should be discussed in the issue area first

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=farion1231/cc-switch&type=Date)](https://www.star-history.com/#farion1231/cc-switch&Date)

## License

MIT © Jason Young
