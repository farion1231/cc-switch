# Changelog

All notable changes to CC Switch will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

---

## [3.9.1] - 2026-01-09

### Bug Fix Release

This release focuses on stability improvements and crash prevention.

### Added

- **Crash Logging** - Panic hook captures crash info to `~/.cc-switch/crash.log` with full stack traces (#562)
- **Release Logging** - Enable logging for release builds with automatic rotation (keeps 2 most recent files)
- **AIGoCode Icon** - Added colored icon for AIGoCode provider preset

### Fixed

- **Proxy Panic Prevention** - Graceful degradation when HTTP client initialization fails due to invalid proxy settings; falls back to no_proxy mode (#560)
- **UTF-8 Safety** - Fix potential panic when masking API keys or truncating logs containing multi-byte characters (Chinese, emoji, etc.) (#560)
- **Default Proxy Port** - Change default port from 5000 to 15721 to avoid conflict with macOS AirPlay Receiver (#560)
- **Windows Title** - Display "CC Switch" instead of default "Tauri app" in window title
- **Windows/Linux Spacing** - Remove extra 28px blank space below native titlebar introduced in v3.9.0
- **Flatpak Tray Icon** - Bundle libayatana-appindicator for tray icon support on Flatpak (#556)
- **Provider Preset** - Correct casing from "AiGoCode" to "AIGoCode" to match official branding

---

## [3.9.0] - 2026-01-07

### Stable Release

This stable release includes all changes from `3.9.0-1`, `3.9.0-2`, and `3.9.0-3`.

### Added

- **Local API Proxy** - High-performance local HTTP proxy for Claude Code, Codex, and Gemini CLI (Axum-based)
- **Per-App Takeover** - Independently route each app through the proxy with automatic live-config backup/redirect
- **Auto Failover** - Circuit breaker + smart failover with independent queues and health tracking per app
- **Universal Provider** - Shared provider configurations that can sync to Claude/Codex/Gemini (ideal for API gateways like NewAPI)
- **Provider Search Filter** - Quick filter to find providers by name (#435)
- **Keyboard Shortcut** - Open settings with Command+comma / Ctrl+comma (#436)
- **Deeplink Usage Config** - Import usage query config via deeplink (#400)
- **Provider Icon Colors** - Customize provider icon colors (#385)
- **Skills Multi-App Support** - Skills now support both Claude Code and Codex (#365)
- **Closable Toasts** - Close button for switch toast and all success toasts (#350)
- **Skip First-Run Confirmation** - Option to skip Claude Code first-run confirmation dialog
- **MCP Import** - Import MCP servers from installed apps
- **Common Config Snippet Extraction** - Extract reusable common config snippets from the current provider or editor content (Claude/Codex/Gemini)
- **Usage Enhancements** - Model extraction, request logging improvements, cache hit/creation metrics, and auto-refresh (#455, #508)
- **Error Request Logging** - Detailed logging for proxy requests (#401)
- **Linux Packaging** - Added RPM and Flatpak packaging targets
- **Provider Presets & Icons** - Added/updated partner presets and icons (e.g., MiMo, DMXAPI, Cubence)

### Changed

- **Usage Terminology** - Rename "Cache Read/Write" to "Cache Hit/Creation" across all languages (#508)
- **Model Pricing Data** - Refresh built-in model pricing table (Claude full version IDs, GPT-5 series, Gemini ID formats, and Chinese models) (#508)
- **Proxy Header Forwarding** - Switch to a blacklist approach and improve header passthrough compatibility (#508)
- **Failover Behavior** - Bypass timeout/retry configs when failover is disabled; update default failover timeout and circuit breaker values (#508, #521)
- **Provider Presets** - Update default model versions and change the default Qwen base URL (#517)
- **Skills Management** - Unify Skills management architecture with SSOT + React Query; improve caching for discoverable skills
- **Settings UX** - Reorder items in the Advanced tab for better discoverability
- **Proxy Active Theme** - Apply emerald theme when proxy takeover is active

### Fixed

- **Security** - Security fixes for JavaScript executor and usage script (#151)
- **Usage Timezone & Parsing** - Fix datetime picker timezone handling; improve token parsing/billing for Gemini and Codex formats (#508)
- **Windows Compatibility** - Improve MCP export and version check behavior to avoid terminal popups
- **Windows Startup** - Use system titlebar to prevent black screen on startup
- **WebView Compatibility** - Add fallback for crypto.randomUUID() on older WebViews
- **macOS Autostart** - Use `.app` bundle path to prevent terminal window popups
- **Database** - Add missing schema migrations; show an error dialog on initialization failure with a retry option
- **Import/Export** - Restrict SQL import to CC Switch exported backups only; refresh providers immediately after import
- **Prompts** - Allow saving prompts with empty content
- **MCP Sync** - Skip sync when the target CLI app is not installed
- **Common Config (Codex)** - Preserve MCP server `base_url` during extraction and remove provider-specific `model_providers` blocks
- **Proxy** - Improve takeover detection and stability; clean up model override env vars when switching providers in takeover mode (#508)
- **Skills** - Skip hidden directories during discovery; fix wrong skill repo branch
- **Settings Navigation** - Navigate to About tab when clicking update badge
- **UI** - Fix dialogs not opening on first click and improve window dragging area in `FullScreenPanel`

---

## [3.9.0-3] - 2025-12-29

### Beta Release

Third beta release with important bug fixes for Windows compatibility, UI improvements, and new features.

### Added

- **Universal Provider** - Support for universal provider configurations (#348)
- **Provider Search Filter** - Quick filter to find providers by name (#435)
- **Keyboard Shortcut** - Open settings with Command+comma / Ctrl+comma (#436)
- **Xiaomi MiMo Icon** - Added MiMo icon and Claude provider configuration (#470)
- **Usage Model Extraction** - Extract model info from usage statistics (#455)
- **Skip First-Run Confirmation** - Option to skip Claude Code first-run confirmation dialog
- **Exit Animations** - Added exit animation to FullScreenPanel dialogs
- **Fade Transitions** - Smooth fade transitions for app/view/panel switching

### Fixed

#### Windows
- Wrap npx/npm commands with `cmd /c` for MCP export
- Prevent terminal windows from appearing during version check

#### macOS
- Use .app bundle path for autostart to prevent terminal window popup

#### UI
- Resolve Dialog/Modal not opening on first click (#492)
- Improve dark mode text contrast for form labels
- Reduce header spacing and fix layout shift on view switch
- Prevent header layout shift when switching views

#### Database & Schema
- Add missing base columns migration for proxy_config
- Add backward compatibility check for proxy_config seed insert

#### Other
- Use local timezone and robust DST handling in usage stats (#500)
- Remove deprecated `sync_enabled_to_codex` call
- Gracefully handle invalid Codex config.toml during MCP sync
- Add missing translations for reasoning model and OpenRouter compat mode

### Improved

- **macOS Tray** - Use macOS tray template icon
- **Header Alignment** - Remove macOS titlebar tint, align custom header
- **Shadow Removal** - Cleaner UI by removing shadow styles
- **Code Inspector** - Added code-inspector-plugin for development
- **i18n** - Complete internationalization for usage panel and settings
- **Sponsor Logos** - Made sponsor logos clickable

### Stats

- 35 commits since v3.9.0-2
- 5 files changed in test/lint fixes

---

## [3.9.0-2] - 2025-12-20

### Beta Release

Second beta release focusing on proxy stability, import safety, and provider preset polish.

### Added

- **DMXAPI Partner** - Added DMXAPI as an official partner provider preset
- **Provider Icons** - Added provider icons for OpenRouter, LongCat, ModelScope, and AiHubMix

### Changed

- **Proxy (OpenRouter)** - Switched OpenRouter to passthrough mode for native Claude API

### Fixed

- **Import/Export** - Restrict SQL import to CC Switch exported backups only; refresh providers immediately after import
- **Proxy** - Respect existing Claude token when syncing; add fallback recovery for orphaned takeover state; remove global auto-start flag
- **Windows** - Add minimum window size to Windows platform config
- **UI** - Improve About section UI (#419) and unify header toolbar styling

### Stats

- 13 commits since v3.9.0-1

---

## [3.9.0-1] - 2025-12-18

### Beta Release

This beta release introduces the **Local API Proxy** feature, along with Skills multi-app support, UI improvements, and numerous bug fixes.

### Major Features

#### Local Proxy Server
- **Local HTTP Proxy** - High-performance proxy server built on Axum framework
- **Multi-app Support** - Unified proxy for Claude Code, Codex, and Gemini CLI API requests
- **Per-app Takeover** - Independent control over which apps route through the proxy
- **Live Config Takeover** - Automatically backs up and redirects CLI configurations to local proxy

#### Auto Failover
- **Circuit Breaker** - Automatically detects provider failures and triggers protection
- **Smart Failover** - Automatically switches to backup provider when current one is unavailable
- **Health Tracking** - Real-time monitoring of provider availability
- **Independent Failover Queues** - Each app maintains its own failover queue

#### Monitoring
- **Request Logging** - Detailed logging of all proxy requests
- **Usage Statistics** - Token consumption, latency, success rate metrics
- **Real-time Status** - Frontend displays proxy status and statistics

#### Skills Multi-App Support
- **Multi-app Support** - Skills now support both Claude and Codex (#365)
- **Multi-app Migration** - Existing Skills auto-migrate to multi-app structure (#378)
- **Installation Path Fix** - Use directory basename for skill installation path (#358)

### Added
- **Provider Icon Colors** - Customize provider icon colors (#385)
- **Deeplink Usage Config** - Import usage query config via deeplink (#400)
- **Error Request Logging** - Detailed logging for proxy requests (#401)
- **Closable Toast** - Added close button to switch notification toast (#350)
- **Icon Color Component** - ProviderIcon component supports color prop (#384)

### Fixed

#### Proxy Related
- Takeover Codex base_url via model_provider
- Harden crash recovery with fallback detection
- Sync UI when active provider differs from current setting
- Resolve circuit breaker race condition and error classification
- Stabilize live takeover and provider editing
- Reset health badges when proxy stops
- Retry failover for all HTTP errors including 4xx
- Fix HalfOpen counter underflow and config field inconsistencies
- Resolve circuit breaker state persistence and HalfOpen deadlock
- Auto-recover live config after abnormal exit
- Update live backup when hot-switching provider in proxy mode
- Wait for server shutdown before exiting app
- Disable auto-start on app launch by resetting enabled flag on stop
- Sync live config tokens to database before takeover
- Resolve 404 error and auto-setup proxy targets

#### MCP Related
- Skip sync when target CLI app is not installed
- Improve upsert and import robustness
- Use browser-compatible platform detection for MCP presets

#### UI Related
- Restore fade transition for Skills button
- Add close button to all success toasts
- Prevent card jitter when health badge appears
- Update SettingsPage tab styles (#342)

#### Other
- Fix Azure website link (#407)
- Add fallback to provider config for usage credentials (#360)
- Fix Windows black screen on startup (use system titlebar)
- Add fallback for crypto.randomUUID() on older WebViews
- Use correct npm package for Codex CLI version check
- Security fixes for JavaScript executor and usage script (#151)

### Improved
- **Proxy Active Theme** - Apply emerald theme when proxy takeover is active
- **Card Animation** - Improved provider card hover animation
- **Remove Restart Prompt** - No longer prompts restart when switching providers

### Technical
- Implement per-app takeover mode
- Proxy module contains 20+ Rust files with complete layered architecture
- Add 5 new database tables for proxy functionality
- Modularize handlers.rs to reduce code duplication
- Remove is_proxy_target in favor of failover_queue

### Stats
- 55 commits since v3.8.2
- 164 files changed
- +22,164 / -570 lines

---

## [3.8.0] - 2025-11-28

### Major Updates

- **Persistence architecture upgrade** - Moved from single JSON storage to SQLite + JSON dual-layer; added schema versioning, transactions, and SQL import/export; first launch auto-migrates `config.json` to SQLite while keeping originals safe.
- **Brand new UI** - Full layout redesign, unified component/ConfirmDialog styles, smoother animations, overscroll disabled; Tailwind CSS downgraded to v3.4 for compatibility.
- **Japanese language support** - UI now localized in Chinese/English/Japanese.

### Added

- **Skills recursive scanning** - Discovers nested `SKILL.md` files across multi-level directories; same-name skills allowed by full-path dedup.
- **Provider icons** - Presets ship with default icons; custom icon colors; icons retained when duplicating providers.
- **Auto launch on startup** - One-click enable/disable using Registry/LaunchAgent/XDG autostart.
- **Provider preset** - Added MiniMax partner preset.
- **Form validation** - Required fields get real-time validation and unified toast messaging.

### Fixed

- **Custom endpoints loss** - Switched provider updates to `UPDATE` to avoid cascade deletes from `INSERT OR REPLACE`.
- **Gemini config writing** - Correctly writes custom env vars to `.env` and keeps auth configs isolated.
- **Provider validation** - Handles missing current provider IDs and preserves icon fields on duplicate.
- **Linux rendering** - Fixed WebKitGTK DMA-BUF rendering and preserved user `.desktop` customizations.
- **Misc** - Removed redundant usage queries; corrected DMXAPI auth token field; restored missing deeplink translations; fixed usage script template init.

### Technical

- **Database modules** - Added `schema`, `backup`, `migration`, and DAO layers for providers/MCP/prompts/skills/settings.
- **Service modularization** - Split provider service into live/auth/endpoints/usage modules; deeplink parsing/import logic modularized.
- **Code cleanup** - Removed legacy JSON-era import/export, unused MCP types; unified error handling; tests migrated to SQLite backend and MSW handlers updated.

### Migration Notes

- First launch auto-migrates data from `config.json` to SQLite and device settings to `settings.json`; originals kept; error dialog on failure; dry-run supported.

### Stats

- 51 commits since v3.7.1; 207 files changed; +17,297 / -6,870 lines. See [release-note-v3.8.0](docs/release-note-v3.8.0-en.md) for details.

---

## [3.7.1] - 2025-11-22

### Fixed

- **Skills third-party repository installation** (#268) - Fixed installation failure for skills repositories with custom subdirectories (e.g., `ComposioHQ/awesome-claude-skills`)
- **Gemini configuration persistence** - Resolved issue where settings.json edits were lost when switching providers
- **Dialog overlay click protection** - Prevented dialogs from closing when clicking outside, avoiding accidental form data loss (affects 11 dialog components)

### Added

- **Gemini configuration directory support** (#255) - Added custom configuration directory option for Gemini in settings
- **ArchLinux installation support** (#259) - Added AUR installation via `paru -S cc-switch-bin`

### Improved

- **Skills error messages i18n** - Added 28+ detailed error messages (English & Chinese) with specific resolution suggestions
- **Download timeout** - Extended from 15s to 60s to reduce network-related false positives
- **Code formatting** - Applied unified Rust (`cargo fmt`) and TypeScript (`prettier`) formatting standards

### Reverted

- **Auto-launch on system startup** - Temporarily reverted feature pending further testing and optimization

---

## [3.7.0] - 2025-11-19

### Major Features

#### Gemini CLI Integration

- **Complete Gemini CLI support** - Third major application added alongside Claude Code and Codex
- **Dual-file configuration** - Support for both `.env` and `settings.json` file formats
- **Environment variable detection** - Auto-detect `GOOGLE_GEMINI_BASE_URL`, `GEMINI_MODEL`, etc.
- **MCP management** - Full MCP configuration capabilities for Gemini
- **Provider presets**
  - Google Official (OAuth authentication)
  - PackyCode (partner integration)
  - Custom endpoint support
- **Deep link support** - Import Gemini providers via `ccswitch://` protocol
- **System tray integration** - Quick-switch Gemini providers from tray menu
- **Backend modules** - New `gemini_config.rs` (20KB) and `gemini_mcp.rs`

#### MCP v3.7.0 Unified Architecture

- **Unified management panel** - Single interface for Claude/Codex/Gemini MCP servers
- **SSE transport type** - New Server-Sent Events support alongside stdio/http
- **Smart JSON parser** - Fault-tolerant parsing of various MCP config formats
- **Extended field support** - Preserve custom fields in Codex TOML conversion
- **Codex format correction** - Proper `[mcp_servers]` format (auto-cleanup of incorrect `[mcp.servers]`)
- **Import/export system** - Unified import from Claude/Codex/Gemini live configs
- **UX improvements**
  - Default app selection in forms
  - JSON formatter for config validation
  - Improved layout and visual hierarchy
  - Better validation error messages

#### Claude Skills Management System

- **GitHub repository integration** - Auto-scan and discover skills from GitHub repos
- **Pre-configured repositories**
  - `ComposioHQ/awesome-claude-skills` (curated collection)
  - `anthropics/skills` (official Anthropic skills)
  - `cexll/myclaude` (community, with subdirectory scanning)
- **Lifecycle management**
  - One-click install to `~/.claude/skills/`
  - Safe uninstall with state tracking
  - Update checking (infrastructure ready)
- **Custom repository support** - Add any GitHub repo as a skill source
- **Subdirectory scanning** - Optional `skillsPath` for repos with nested skill directories
- **Backend architecture** - `SkillService` (526 lines) with GitHub API integration
- **Frontend interface**
  - SkillsPage: Browse and manage skills
  - SkillCard: Visual skill presentation
  - RepoManager: Repository management dialog
- **State persistence** - Installation state stored in `skills.json`
- **Full i18n support** - Complete Chinese/English translations (47+ keys)

#### Prompts (System Prompts) Management

- **Multi-preset management** - Create, edit, and switch between multiple system prompts
- **Cross-app support**
  - Claude: `~/.claude/CLAUDE.md`
  - Codex: `~/.codex/AGENTS.md`
  - Gemini: `~/.gemini/GEMINI.md`
- **Markdown editor** - Full-featured CodeMirror 6 editor with syntax highlighting
- **Smart synchronization**
  - Auto-write to live files on enable
  - Content backfill protection (save current before switching)
  - First-launch auto-import from live files
- **Single-active enforcement** - Only one prompt can be active at a time
- **Delete protection** - Cannot delete active prompts
- **Backend service** - `PromptService` (213 lines) with CRUD operations
- **Frontend components**
  - PromptPanel: Main management interface (177 lines)
  - PromptFormModal: Edit dialog with validation (160 lines)
  - MarkdownEditor: CodeMirror integration (159 lines)
  - usePromptActions: Business logic hook (152 lines)
- **Full i18n support** - Complete Chinese/English translations (41+ keys)

#### Deep Link Protocol (ccswitch://)

- **Protocol registration** - `ccswitch://` URL scheme for one-click imports
- **Provider import** - Import provider configurations from URLs or shared links
- **Lifecycle integration** - Deep link handling integrated into app startup
- **Cross-platform support** - Works on Windows, macOS, and Linux

#### Environment Variable Conflict Detection

- **Claude & Codex detection** - Identify conflicting environment variables
- **Gemini auto-detection** - Automatic environment variable discovery
- **Conflict management** - UI for resolving configuration conflicts
- **Prevention system** - Warn before overwriting existing configurations

### New Features

#### Provider Management

- **DouBaoSeed preset** - Added ByteDance's DouBao provider
- **Kimi For Coding** - Moonshot AI coding assistant
- **BaiLing preset** - BaiLing AI integration
- **Removed AnyRouter preset** - Discontinued provider
- **Model configuration** - Support for custom model names in Codex and Gemini
- **Provider notes field** - Add custom notes to providers for better organization

#### Configuration Management

- **Common config migration** - Moved Claude common config snippets from localStorage to `config.json`
- **Unified persistence** - Common config snippets now shared across all apps
- **Auto-import on first launch** - Automatically import configs from live files on first run
- **Backfill priority fix** - Correct priority handling when enabling prompts

#### UI/UX Improvements

- **macOS native design** - Migrated color scheme to macOS native design system
- **Window centering** - Default window position centered on screen
- **Password input fixes** - Disabled Edge/IE reveal and clear buttons
- **URL overflow prevention** - Fixed overflow in provider cards
- **Error notification enhancement** - Copy-to-clipboard for error messages
- **Tray menu sync** - Real-time sync after drag-and-drop sorting

### Improvements

#### Architecture

- **MCP v3.7.0 cleanup** - Removed legacy code and warnings
- **Unified structure** - Default initialization with v3.7.0 unified structure
- **Backward compatibility** - Compilation fixes for older configs
- **Code formatting** - Applied consistent formatting across backend and frontend

#### Platform Compatibility

- **Windows fix** - Resolved winreg API compatibility issue (v0.52)
- **Safe pattern matching** - Replaced `unwrap()` with safe patterns in tray menu

#### Configuration

- **MCP sync on switch** - Sync MCP configs for all apps when switching providers
- **Gemini form sync** - Fixed form fields syncing with environment editor
- **Gemini config reading** - Read from both `.env` and `settings.json`
- **Validation improvements** - Enhanced input validation and boundary checks

#### Internationalization

- **JSON syntax fixes** - Resolved syntax errors in locale files
- **App name i18n** - Added internationalization support for app names
- **Deduplicated labels** - Reused providerForm keys to reduce duplication
- **Gemini MCP title** - Added missing Gemini MCP panel title

### Bug Fixes

#### Critical Fixes

- **Usage script validation** - Added input validation and boundary checks
- **Gemini validation** - Relaxed validation when adding providers
- **TOML quote normalization** - Handle CJK quotes to prevent parsing errors
- **MCP field preservation** - Preserve custom fields in Codex TOML editor
- **Password input** - Fixed white screen crash (FormLabel → Label)

#### Stability

- **Tray menu safety** - Replaced unwrap with safe pattern matching
- **Error isolation** - Tray menu update failures don't block main operations
- **Import classification** - Set category to custom for imported default configs

#### UI Fixes

- **Model placeholders** - Removed misleading model input placeholders
- **Base URL population** - Auto-fill base URL for non-official providers
- **Drag sort sync** - Fixed tray menu order after drag-and-drop

### Technical Improvements

#### Code Quality

- **Type safety** - Complete TypeScript type coverage across codebase
- **Test improvements** - Simplified boolean assertions in tests
- **Clippy warnings** - Fixed `uninlined_format_args` warnings
- **Code refactoring** - Extracted templates, optimized logic flows

#### Dependencies

- **Tauri** - Updated to 2.8.x series
- **Rust dependencies** - Added `anyhow`, `zip`, `serde_yaml`, `tempfile` for Skills
- **Frontend dependencies** - Added CodeMirror 6 packages for Markdown editor
- **winreg** - Updated to v0.52 (Windows compatibility)

#### Performance

- **Startup optimization** - Removed legacy migration scanning
- **Lock management** - Improved RwLock usage to prevent deadlocks
- **Background query** - Enabled background mode for usage polling

### Statistics

- **Total commits**: 85 commits from v3.6.0 to v3.7.0
- **Code changes**: 152 files changed, 18,104 insertions(+), 3,732 deletions(-)
- **New modules**:
  - Skills: 2,034 lines (21 files)
  - Prompts: 1,302 lines (20 files)
  - Gemini: ~1,000 lines (multiple files)
  - MCP refactor: ~3,000 lines (refactored)

### Strategic Positioning

v3.7.0 represents a major evolution from "Provider Switcher" to **"All-in-One AI CLI Management Platform"**:

1. **Capability Extension** - Skills provide external ability integration
2. **Behavior Customization** - Prompts enable AI personality presets
3. **Configuration Unification** - MCP v3.7.0 eliminates app silos
4. **Ecosystem Openness** - Deep links enable community sharing
5. **Multi-AI Support** - Claude/Codex/Gemini trinity
6. **Intelligent Detection** - Auto-discovery of environment conflicts

### Notes

- Users upgrading from v3.1.0 or earlier should first upgrade to v3.2.x for one-time migration
- Skills and Prompts management are new features requiring no migration
- Gemini CLI support requires Gemini CLI to be installed separately
- MCP v3.7.0 unified structure is backward compatible with previous configs

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

- Tauri commands only accept the `app` parameter (`claude`/`codex`); removed `app_type`/`appType` compatibility.
- Frontend types are standardized to `AppId` (removed `AppType` export); variable naming is standardized to `appId`.

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
