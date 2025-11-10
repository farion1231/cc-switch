# Changelog

All notable changes to CC Switch will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [3.6.0] - 2025-11-07

### ✨ New Features

- **Provider Duplicate** - Quick duplicate existing provider configurations for easy variant creation
- **Edit Mode Toggle** - Show/hide drag handles to optimize editing experience
- **Custom Endpoint Management** - Support multi-endpoint configuration for aggregator providers
- **Usage Query Enhancements**
  - Auto-refresh interval: Support periodic automatic usage query
  - Test Script API: Validate JavaScript scripts before execution
  - Template system expansion: Custom blank template, support for access token and user ID parameters
- **Configuration Editor Improvements**
  - Add JSON format button
  - Real-time TOML syntax validation for Codex configuration
- **Auto-sync on Directory Change** - When switching Claude/Codex config directories (e.g., WSL environment), automatically sync current provider to new directory without manual operation
- **Load Live Config When Editing Active Provider** - When editing the currently active provider, prioritize displaying the actual effective configuration to protect user manual modifications
- **New Provider Presets** - DMXAPI, Azure Codex, AnyRouter, AiHubMix, MiniMax
- **Partner Promotion Mechanism** - Support ecosystem partner promotion (e.g., Zhipu GLM Z.ai)

### 🔧 Improvements

- **Configuration Directory Switching**
  - Introduced unified post-change sync utility (`postChangeSync.ts`)
  - Auto-sync current providers to new directory when changing Claude/Codex config directories
  - Perfect support for WSL environment switching
  - Auto-sync after config import to ensure immediate effectiveness
  - Use Result pattern for graceful error handling without blocking main flow
  - Distinguish "fully successful" and "partially successful" states for precise user feedback
- **UI/UX Enhancements**
  - Provider cards: Unique icons and color identification
  - Unified border design system across all components
  - Drag interaction optimization: Push effect animation, improved handle icons
  - Enhanced current provider visual feedback
  - Dialog size standardization and layout consistency
  - Form experience: Optimized model placeholders, simplified provider hints, category-specific hints
- **Complete Internationalization Coverage**
  - Error messages internationalization
  - Tray menu internationalization
  - All UI components internationalization
- **Usage Display Moved Inline** - Usage display moved next to enable button

### 🐛 Bug Fixes

- **Configuration Sync**
  - Fixed `apiKeyUrl` priority issue
  - Fixed MCP sync-to-other-side functionality failure
  - Fixed sync issues after config import
  - Prevent silent fallback and data loss on config error
- **Usage Query**
  - Fixed auto-query interval timing issue
  - Ensure refresh button shows loading animation on click
- **UI Issues**
  - Fixed name collision error (`get_init_error` command)
  - Fixed language setting rollback after successful save
  - Fixed language switch state reset (dependency cycle)
  - Fixed edit mode button alignment
- **Configuration Management**
  - Fixed Codex API Key auto-sync
  - Fixed endpoint speed test functionality
  - Fixed provider duplicate insertion position (next to original provider)
  - Fixed custom endpoint preservation in edit mode
- **Startup Issues**
  - Force exit on config error (no silent fallback)
  - Eliminate code duplication causing initialization errors

### 🏗️ Technical Improvements (For Developers)

**Backend Refactoring (Rust)** - Completed 5-phase refactoring:
- **Phase 1**: Unified error handling (`AppError` + i18n error messages)
- **Phase 2**: Command layer split by domain (`commands/{provider,mcp,config,settings,plugin,misc}.rs`)
- **Phase 3**: Integration tests and transaction mechanism (config snapshot + failure rollback)
- **Phase 4**: Extracted Service layer (`services/{provider,mcp,config,speedtest}.rs`)
- **Phase 5**: Concurrency optimization (`RwLock` instead of `Mutex`, scoped guard to avoid deadlock)

**Frontend Refactoring (React + TypeScript)** - Completed 4-stage refactoring:
- **Stage 1**: Test infrastructure (vitest + MSW + @testing-library/react)
- **Stage 2**: Extracted custom hooks (`useProviderActions`, `useMcpActions`, `useSettings`, `useImportExport`, etc.)
- **Stage 3**: Component splitting and business logic extraction
- **Stage 4**: Code cleanup and formatting unification

**Testing System**:
- Hooks unit tests 100% coverage
- Integration tests covering key processes (App, SettingsDialog, MCP Panel)
- MSW mocking backend API to ensure test independence

**Code Quality**:
- Unified parameter format: All Tauri commands migrated to camelCase (Tauri 2 specification)
- `AppType` renamed to `AppId`: Semantically clearer
- Unified parsing with `FromStr` trait: Centralized `app` parameter parsing
- Eliminate code duplication: DRY violations cleanup
- Remove unused code: `missing_param` helper function, deprecated `tauri-api.ts`, redundant `KimiModelSelector` component

**Internal Optimizations**:
- **Removed Legacy Migration Logic**: v3.6 removed v1 config auto-migration and copy file scanning logic
  - ✅ **Impact**: Improved startup performance, cleaner code
  - ✅ **Compatibility**: v2 format configs fully compatible, no action required
  - ⚠️ **Note**: Users upgrading from v3.1.0 or earlier should first upgrade to v3.2.x or v3.5.x for one-time migration, then upgrade to v3.6
- **Command Parameter Standardization**: Backend unified to use `app` parameter (values: `claude` or `codex`)
  - ✅ **Impact**: More standardized code, friendlier error prompts
  - ✅ **Compatibility**: Frontend fully adapted, users don't need to care about this change

### 📦 Dependencies

- Updated to Tauri 2.8.x
- Updated to TailwindCSS 4.x
- Updated to TanStack Query v5.90.x
- Maintained React 18.2.x and TypeScript 5.3.x

## [3.5.0] - 2025-01-15

### ⚠ Breaking Changes

- Tauri 命令仅接受参数 `app`（取值：`claude`/`codex`）；移除对 `app_type`/`appType` 的兼容。
- 前端类型命名统一为 `AppId`（移除 `AppType` 导出），变量命名统一为 `appId`。

### ✨ New Features

- **MCP (Model Context Protocol) Management** - Complete MCP server configuration management system
  - Add, edit, delete, and toggle MCP servers in `~/.claude.json`
  - Support for stdio and http server types with command validation
  - Built-in templates for popular MCP servers (mcp-fetch, etc.)
  - Real-time enable/disable toggle for MCP servers
  - Atomic file writing to prevent configuration corruption
- **Configuration Import/Export** - Backup and restore your provider configurations
  - Export all configurations to JSON file with one click
  - Import configurations with validation and automatic backup
  - Automatic backup rotation (keeps 10 most recent backups)
  - Progress modal with detailed status feedback
- **Endpoint Speed Testing** - Test API endpoint response times
  - Measure latency to different provider endpoints
  - Visual indicators for connection quality
  - Help users choose the fastest provider

### 🔧 Improvements

- Complete internationalization (i18n) coverage for all UI components
- Enhanced error handling and user feedback throughout the application
- Improved configuration file management with better validation
- Added new provider presets: Longcat, kat-coder
- Updated GLM provider configurations with latest models
- Refined UI/UX with better spacing, icons, and visual feedback
- Enhanced tray menu functionality and responsiveness
- **Standardized release artifact naming** - All platform releases now use consistent version-tagged filenames:
  - macOS: `CC-Switch-v{version}-macOS.tar.gz` / `.zip`
  - Windows: `CC-Switch-v{version}-Windows.msi` / `-Portable.zip`
  - Linux: `CC-Switch-v{version}-Linux.AppImage` / `.deb`

### 🐛 Bug Fixes

- Fixed layout shifts during provider switching
- Improved config file path handling across different platforms
- Better error messages for configuration validation failures
- Fixed various edge cases in configuration import/export

### 📦 Technical Details

- Enhanced `import_export.rs` module with backup management
- New `claude_mcp.rs` module for MCP configuration handling
- Improved state management and lock handling in Rust backend
- Better TypeScript type safety across the codebase

## [3.4.0] - 2025-10-01

### ✨ Features

- Enable internationalization via i18next with a Chinese default and English fallback, plus an in-app language switcher
- Add Claude plugin sync while retiring the legacy VS Code integration controls (Codex no longer requires settings.json edits)
- Extend provider presets with optional API key URLs and updated models, including DeepSeek-V3.1-Terminus and Qwen3-Max
- Support portable mode launches and enforce a single running instance to avoid conflicts

### 🔧 Improvements

- Allow minimizing the window to the system tray and add macOS Dock visibility management for tray workflows
- Refresh the Settings modal with a scrollable layout, save icon, and cleaner language section
- Smooth provider toggle states with consistent button widths/icons and prevent layout shifts when switching between Claude and Codex
- Adjust the Windows MSI installer to target per-user LocalAppData and improve component tracking reliability

### 🐛 Fixes

- Remove the unnecessary OpenAI auth requirement from third-party provider configurations
- Fix layout shifts while switching app types with Claude plugin sync enabled
- Align Enable/In Use button states to avoid visual jank across app views

## [3.3.0] - 2025-09-22

### ✨ Features

- Add “Apply to VS Code / Remove from VS Code” actions on provider cards, writing settings for Code/Insiders/VSCodium variants _(Removed in 3.4.x)_
- Enable VS Code auto-sync by default with window broadcast and tray hooks so Codex switches sync silently _(Removed in 3.4.x)_
- Extend the Codex provider wizard with display name, dedicated API key URL, and clearer guidance
- Introduce shared common config snippets with JSON/TOML reuse, validation, and consistent error surfaces

### 🔧 Improvements

- Keep the tray menu responsive when the window is hidden and standardize button styling and copy
- Disable modal backdrop blur on Linux (WebKitGTK/Wayland) to avoid freezes; restore the window when clicking the macOS Dock icon
- Support overriding config directories on WSL, refine placeholders/descriptions, and fix VS Code button wrapping on Windows
- Add a `created_at` timestamp to provider records for future sorting and analytics

### 🐛 Fixes

- Correct regex escapes and common snippet trimming in the Codex wizard to prevent validation issues
- Harden the VS Code sync flow with more reliable TOML/JSON parsing while reducing layout jank
- Bundle `@codemirror/lint` to reinstate live linting in config editors

## [3.2.0] - 2025-09-13

### ✨ New Features

- System tray provider switching with dynamic menu for Claude/Codex
- Frontend receives `provider-switched` events and refreshes active app
- Built-in update flow via Tauri Updater plugin with dismissible UpdateBadge

### 🔧 Improvements

- Single source of truth for provider configs; no duplicate copy files
- One-time migration imports existing copies into `config.json` and archives originals
- Duplicate provider de-duplication by name + API key at startup
- Atomic writes for Codex `auth.json` + `config.toml` with rollback on failure
- Logging standardized (Rust): use `log::{info,warn,error}` instead of stdout prints
- Tailwind v4 integration and refined dark mode handling

### 🐛 Fixes

- Remove/minimize debug console logs in production builds
- Fix CSS minifier warnings for scrollbar pseudo-elements
- Prettier formatting across codebase for consistent style

### 📦 Dependencies

- Tauri: 2.8.x (core, updater, process, opener, log plugins)
- React: 18.2.x · TypeScript: 5.3.x · Vite: 5.x

### 🔄 Notes

- `connect-src` CSP remains permissive for compatibility; can be tightened later as needed

## [3.1.1] - 2025-09-03

### 🐛 Bug Fixes

- Fixed the default codex config.toml to match the latest modifications
- Improved provider configuration UX with custom option

### 📝 Documentation

- Updated README with latest information

## [3.1.0] - 2025-09-01

### ✨ New Features

- **Added Codex application support** - Now supports both Claude Code and Codex configuration management
  - Manage auth.json and config.toml for Codex
  - Support for backup and restore operations
  - Preset providers for Codex (Official, PackyCode)
  - API Key auto-write to auth.json when using presets
- **New UI components**
  - App switcher with segmented control design
  - Dual editor form for Codex configuration
  - Pills-style app switcher with consistent button widths
- **Enhanced configuration management**
  - Multi-app config v2 structure (claude/codex)
  - Automatic v1→v2 migration with backup
  - OPENAI_API_KEY validation for non-official presets
  - TOML syntax validation for config.toml

### 🔧 Technical Improvements

- Unified Tauri command API with app_type parameter
- Backward compatibility for app/appType parameters
- Added get_config_status/open_config_folder/open_external commands
- Improved error handling for empty config.toml

### 🐛 Bug Fixes

- Fixed config path reporting and folder opening for Codex
- Corrected default import behavior when main config is missing
- Fixed non_snake_case warnings in commands.rs

## [3.0.0] - 2025-08-27

### 🚀 Major Changes

- **Complete migration from Electron to Tauri 2.0** - The application has been completely rewritten using Tauri, resulting in:
  - **90% reduction in bundle size** (from ~150MB to ~15MB)
  - **Significantly improved startup performance**
  - **Native system integration** without Chromium overhead
  - **Enhanced security** with Rust backend

### ✨ New Features

- **Native window controls** with transparent title bar on macOS
- **Improved file system operations** using Rust for better performance
- **Enhanced security model** with explicit permission declarations
- **Better platform detection** using Tauri's native APIs

### 🔧 Technical Improvements

- Migrated from Electron IPC to Tauri command system
- Replaced Node.js file operations with Rust implementations
- Implemented proper CSP (Content Security Policy) for enhanced security
- Added TypeScript strict mode for better type safety
- Integrated Rust cargo fmt and clippy for code quality

### 🐛 Bug Fixes

- Fixed bundle identifier conflict on macOS (changed from .app to .desktop)
- Resolved platform detection issues
- Improved error handling in configuration management

### 📦 Dependencies

- **Tauri**: 2.8.2
- **React**: 18.2.0
- **TypeScript**: 5.3.0
- **Vite**: 5.0.0

### 🔄 Migration Notes

For users upgrading from v2.x (Electron version):

- Configuration files remain compatible - no action required
- The app will automatically migrate your existing provider configurations
- Window position and size preferences have been reset to defaults

#### Backup on v1→v2 Migration (cc-switch internal config)

- When the app detects an old v1 config structure at `~/.cc-switch/config.json`, it now creates a timestamped backup before writing the new v2 structure.
- Backup location: `~/.cc-switch/config.v1.backup.<timestamp>.json`
- This only concerns cc-switch's own metadata file; your actual provider files under `~/.claude/` and `~/.codex/` are untouched.

### 🛠️ Development

- Added `pnpm typecheck` command for TypeScript validation
- Added `pnpm format` and `pnpm format:check` for code formatting
- Rust code now uses cargo fmt for consistent formatting

## [2.0.0] - Previous Electron Release

### Features

- Multi-provider configuration management
- Quick provider switching
- Import/export configurations
- Preset provider templates

---

## [1.0.0] - Initial Release

### Features

- Basic provider management
- Claude Code integration
- Configuration file handling
## [Unreleased]

### ⚠️ Breaking Changes

- **Runtime auto-migration from v1 to v2 config format has been removed**
  - `MultiAppConfig::load()` no longer automatically migrates v1 configs
  - When a v1 config is detected, the app now returns a clear error with migration instructions
  - **Migration path**: Install v3.2.x to perform one-time auto-migration, OR manually edit `~/.cc-switch/config.json` to v2 format
  - **Rationale**: Separates concerns (load() should be read-only), fail-fast principle, simplifies maintenance
  - Related: `app_config.rs` (v1 detection improved with structural analysis), `app_config_load.rs` (comprehensive test coverage added)

- **Legacy v1 copy file migration logic has been removed**
  - Removed entire `migration.rs` module (435 lines) that handled one-time migration from v3.1.0 to v3.2.0
  - No longer scans/merges legacy copy files (`settings-*.json`, `auth-*.json`, `config-*.toml`)
  - No longer archives copy files or performs automatic deduplication
  - **Migration path**: Users upgrading from v3.1.0 must first upgrade to v3.2.x to automatically migrate their configurations
  - **Benefits**: Improved startup performance (no file scanning), reduced code complexity, cleaner codebase

- **Tauri commands now only accept `app` parameter**
  - Removed legacy `app_type`/`appType` compatibility paths
  - Explicit error with available values when unknown `app` is provided

### 🔧 Improvements

- Unified `AppType` parsing: centralized to `FromStr` implementation, command layer no longer implements separate `parse_app()`, reducing code duplication and drift
- Localized and user-friendly error messages: returns bilingual (Chinese/English) hints for unsupported `app` values with a list of available options
- Simplified startup logic: Only ensures config structure exists, no migration overhead

### 🧪 Tests

- Added unit tests covering `AppType::from_str`: case sensitivity, whitespace trimming, unknown value error messages
- Added comprehensive config loading tests:
  - `load_v1_config_returns_error_and_does_not_write`
  - `load_v1_with_extra_version_still_treated_as_v1`
  - `load_invalid_json_returns_parse_error_and_does_not_write`
  - `load_valid_v2_config_succeeds`
