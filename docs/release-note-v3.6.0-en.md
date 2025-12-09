## Major architecture refactoring with enhanced config sync and data protection

**[中文更新说明 Chinese Documentation →](https://github.com/farion1231/cc-switch/blob/main/docs/release-note-v3.6.0-zh.md)**

---

## What's New

### Edit Mode & Provider Management

- **Provider Duplication** - Quickly duplicate existing provider configurations to create variants with one click
- **Manual Sorting** - Drag and drop to reorder providers, with visual push effect animations. Thanks to @ZyphrZero
- **Edit Mode Toggle** - Show/hide drag handles to optimize editing experience

### Custom Endpoint Management

- **Multi-Endpoint Configuration** - Support for aggregator providers with multiple API endpoints
- **Endpoint Input Visibility** - Shows endpoint field for all non-official providers automatically

### Usage Query Enhancements

- **Auto-Refresh Interval** - Configure periodic automatic usage queries with customizable intervals
- **Test Script API** - Validate JavaScript usage query scripts before execution
- **Enhanced Templates** - Custom blank templates with access token and user ID parameter support
  Thanks to @Sirhexs

### Custom Configuration Directory (Cloud Sync)

- **Customizable Storage Location** - Customize CC Switch's configuration storage directory
- **Cloud Sync Support** - Point to cloud sync folders (Dropbox, OneDrive, iCloud Drive, etc.) to enable automatic config synchronization across devices
- **Independent Management** - Managed via Tauri Store for better isolation and reliability
  Thanks to @ZyphrZero

### Configuration Directory Switching (WSL Support)

- **Auto-Sync on Directory Change** - When switching Claude/Codex config directories (e.g., WSL environment), automatically sync current provider to the new directory without manual operation
- **Post-Change Sync Utility** - Unified `postChangeSync.ts` utility for graceful error handling without blocking main flow
- **Import Config Auto-Sync** - Automatically sync after config import to ensure immediate effectiveness
- **Smart Conflict Resolution** - Distinguishes "fully successful" and "partially successful" states for precise user feedback

### Configuration Editor Improvements

- **JSON Format Button** - One-click JSON formatting in configuration editors
- **Real-Time TOML Validation** - Live syntax validation for Codex configuration with error highlighting

### Load Live Config When Editing

- **Protect Manual Modifications** - When editing the currently active provider, prioritize displaying the actual effective configuration from live files
- **Dual-Source Strategy** - Automatically loads from live config for active provider, SSOT for inactive ones

### Claude Configuration Data Structure Enhancements

- **Granular Model Configuration** - Migrated from dual-key to quad-key system for better model tier differentiation
  - New fields: `ANTHROPIC_DEFAULT_HAIKU_MODEL`, `ANTHROPIC_DEFAULT_SONNET_MODEL`, `ANTHROPIC_DEFAULT_OPUS_MODEL`, `ANTHROPIC_MODEL`
  - Replaces legacy `ANTHROPIC_SMALL_FAST_MODEL` with automatic migration
  - Backend normalizes old configs on first read/write with smart fallback chain
  - UI expanded from 2 to 4 model input fields with intelligent defaults
- **ANTHROPIC_API_KEY Support** - Providers can now use `ANTHROPIC_API_KEY` field in addition to `ANTHROPIC_AUTH_TOKEN`
- **Template Variable System** - Support for dynamic configuration replacement (e.g., KAT-Coder's `ENDPOINT_ID` parameter)
- **Endpoint Candidates** - Predefined endpoint list for speed testing and endpoint management
- **Visual Theme Configuration** - Custom icons and colors for provider cards

### Updated Provider Models

- **Kimi k2** - Updated to latest `kimi-k2-thinking` model

### New Provider Presets

Added 5 new provider presets:

- **DMXAPI** - Multi-model aggregation service
- **Azure Codex** - Microsoft Azure OpenAI endpoint
- **AnyRouter** - None-profit routing service
- **AiHubMix** - Multi-model aggregation service
- **MiniMax** - Open source AI model provider

### Partner Promotion Mechanism

- Support for ecosystem partner promotion (Zhipu GLM Z.ai)
- Sponsored banner integration in README

---

## Improvements

### Configuration & Sync

- **Unified Error Handling** - AppError with internationalized error messages throughout backend
- **Fixed apiKeyUrl Priority** - Correct priority order for API key URL resolution
- **Fixed MCP Sync Issues** - Resolved sync-to-other-side functionality failures
- **Import Config Sync** - Fixed sync issues after configuration import
- **Config Error Handling** - Force exit on config error to prevent silent fallback and data loss

### UI/UX Enhancements

- **Unique Provider Icons** - Each provider card now has unique icons and color identification
- **Unified Border System** - Consistent border design across all components
- **Drag Interaction** - Push effect animation and improved drag handle icons
- **Enhanced Visual Feedback** - Better current provider visual indication
- **Dialog Standardization** - Unified dialog sizes and layout consistency
- **Form Improvements** - Optimized model placeholders, simplified provider hints, category-specific hints
- **Usage Display Inline** - Usage info moved next to enable button for better space utilization

### Complete Internationalization

- **Error Messages i18n** - All backend error messages support Chinese/English
- **Tray Menu i18n** - System tray menu fully internationalized
- **UI Components i18n** - 100% coverage across all user-facing components

---

## Bug Fixes

### Configuration Management

- Fixed `apiKeyUrl` priority issue
- Fixed MCP sync-to-other-side functionality failure
- Fixed sync issues after config import
- Fixed Codex API Key auto-sync
- Fixed endpoint speed test functionality
- Fixed provider duplicate insertion position (now inserts next to original)
- Fixed custom endpoint preservation in edit mode
- Prevent silent fallback and data loss on config error

### Usage Query

- Fixed auto-query interval timing issue
- Ensured refresh button shows loading animation on click

### UI Issues

- Fixed name collision error (`get_init_error` command)
- Fixed language setting rollback after successful save
- Fixed language switch state reset (dependency cycle)
- Fixed edit mode button alignment

### Startup Issues

- Force exit on config error (no silent fallback)
- Eliminated code duplication causing initialization errors

---

## Architecture Refactoring

### Backend (Rust) - 5 Phase Refactoring

1. **Phase 1**: Unified error handling (`AppError` + i18n error messages)
2. **Phase 2**: Command layer split by domain (`commands/{provider,mcp,config,settings,plugin,misc}.rs`)
3. **Phase 3**: Integration tests and transaction mechanism (config snapshot + failure rollback)
4. **Phase 4**: Extracted Service layer (`services/{provider,mcp,config,speedtest}.rs`)
5. **Phase 5**: Concurrency optimization (`RwLock` instead of `Mutex`, scoped guard to avoid deadlock)

### Frontend (React + TypeScript) - 4 Stage Refactoring

1. **Stage 1**: Test infrastructure (vitest + MSW + @testing-library/react)
2. **Stage 2**: Extracted custom hooks (`useProviderActions`, `useMcpActions`, `useSettings`, `useImportExport`, etc.)
3. **Stage 3**: Component splitting and business logic extraction
4. **Stage 4**: Code cleanup and formatting unification

### Testing System

- **Hooks Unit Tests** - 100% coverage for all custom hooks
- **Integration Tests** - Coverage for key processes (App, SettingsDialog, MCP Panel)
- **MSW Mocking** - Backend API mocking to ensure test independence
- **Test Infrastructure** - vitest + MSW + @testing-library/react

### Code Quality

- **Unified Parameter Format** - All Tauri commands migrated to camelCase (Tauri 2 specification)
- **Semantic Clarity** - `AppType` renamed to `AppId` for better semantics
- **Centralized Parsing** - Unified `app` parameter parsing with `FromStr` trait
- **DRY Violations Cleanup** - Eliminated code duplication throughout codebase
- **Dead Code Removal** - Removed unused `missing_param` helper, deprecated `tauri-api.ts`, redundant `KimiModelSelector`

---

## Internal Optimizations (User Transparent)

### Removed Legacy Migration Logic

v3.6.0 removed v1 config auto-migration and copy file scanning logic:

- **Impact**: Improved startup performance, cleaner codebase
- **Compatibility**: v2 format configs fully compatible, no action required
- **Note**: Users upgrading from v3.1.0 or earlier should first upgrade to v3.2.x or v3.5.x for one-time migration, then upgrade to v3.6.0

### Command Parameter Standardization

Backend unified to use `app` parameter (values: `claude` or `codex`):

- **Impact**: More standardized code, friendlier error prompts
- **Compatibility**: Frontend fully adapted, users don't need to care about this change

---

## Dependencies

- Updated to **Tauri 2.8.x**
- Updated to **TailwindCSS 4.x**
- Updated to **TanStack Query v5.90.x**
- Maintained **React 18.2.x** and **TypeScript 5.3.x**

---

## Installation

### macOS

**Via Homebrew (Recommended):**

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

**Manual Download:**

- Download `CC-Switch-v3.6.0-macOS.zip` from [Assets](#assets) below

> **Note**: Due to lack of Apple Developer account, you may see "unidentified developer" warning. Go to System Settings → Privacy & Security → Click "Open Anyway"

### Windows

- **Installer**: `CC-Switch-v3.6.0-Windows.msi`
- **Portable**: `CC-Switch-v3.6.0-Windows-Portable.zip`

### Linux

- **AppImage**: `CC-Switch-v3.6.0-Linux.AppImage`
- **Debian**: `CC-Switch-v3.6.0-Linux.deb`

---

## Documentation

- [中文文档 (Chinese)](https://github.com/farion1231/cc-switch/blob/main/README_ZH.md)
- [English Documentation](https://github.com/farion1231/cc-switch/blob/main/README.md)
- [完整更新日志 (Full Changelog)](https://github.com/farion1231/cc-switch/blob/main/CHANGELOG.md)

---

## Acknowledgments

Special thanks to **Zhipu AI** for sponsoring this project with their GLM CODING PLAN!

---

**Full Changelog**: https://github.com/farion1231/cc-switch/compare/v3.5.1...v3.6.0
