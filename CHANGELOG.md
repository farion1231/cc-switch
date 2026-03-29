# Changelog

All notable changes to CC Switch will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

---

## [3.12.3] - 2026-03-24

Major release adding GitHub Copilot reverse proxy support, macOS code signing & Apple notarization, intelligent reasoning effort mapping for o-series models, skill backup/restore lifecycle, proxy gzip compression, and critical fixes for WebDAV password safety, tool message parsing, and dark mode.

**Stats**: 36 commits | 107 files changed | +9,124 insertions | -802 deletions

### Added

- **GitHub Copilot Reverse Proxy**: Full GitHub Copilot integration as a Claude Code provider via OAuth Device Code flow; includes multi-account management, automatic token refresh, Anthropic ↔ OpenAI format conversion, real-time model list fetching, and usage statistics (#930)
- **Copilot Auth Center**: New Auth Center panel in Settings for managing GitHub accounts globally, with per-provider account binding via `meta.authBinding`
- **Tool Search Toggle**: Added `ENABLE_TOOL_SEARCH` env var support for Claude 2.1.76+; exposed as a checkbox in the provider Common Config editor (#930)
- **Reasoning Effort Mapping**: Two-tier `resolve_reasoning_effort()` for OpenAI o-series and GPT-5+ models — explicit `output_config.effort` takes priority, falling back to thinking `budget_tokens` thresholds (<4 000→low, 4 000–16 000→medium, ≥16 000→high); covers both Chat Completions and Responses API paths with 17 unit tests
- **OpenCode SQLite Backend**: Added SQLite session storage support for OpenCode alongside existing JSON backend; dual-backend scan with SQLite priority on ID conflicts, atomic session deletion, and path validation (#1401)
- **Skill Auto-Backup**: Skill files are automatically backed up to `~/.cc-switch/skill-backups/` before uninstall, with metadata preserved in `meta.json`; old backups pruned to keep at most 20
- **Skill Backup Restore & Delete**: Added list/restore/delete commands for skill backups; restore copies files back to SSOT, saves the DB record, and syncs to the current app with rollback on failure
- **macOS Code Signing & Notarization**: CI now imports an Apple Developer ID certificate, signs the universal binary, submits for Apple notarization, and staples the ticket to both `.app` and `.dmg`; a hard-fail verification step (`codesign --verify` + `spctl -a` + `stapler validate`) gates the release for both artifacts
- **Codex 1M Context Window Toggle**: One-click checkbox in Codex config editor to set `model_context_window = 1000000` with auto-populated `model_auto_compact_token_limit = 900000`; unchecking removes both fields
- **Disable Auto-Upgrade Toggle**: Added `DISABLE_AUTOUPDATER` env var checkbox in the Claude Common Config editor to prevent Claude Code from auto-upgrading

### Changed

- **Skills Cache Strategy**: Replaced `invalidateQueries` with direct `setQueryData` updates for skill install/uninstall/import operations; added `staleTime: Infinity` with `keepPreviousData` to eliminate loading flicker (#1573)
- **Proxy Gzip Compression**: Non-streaming proxy requests now auto-negotiate gzip compression instead of forcing `identity`; streaming requests conservatively keep `identity` to avoid SSE decompression errors
- **o1/o3 Model Compatibility**: Chat Completions proxy forwarding now correctly uses `max_completion_tokens` instead of `max_tokens` for OpenAI o-series models such as o1/o3/o4-mini (#1451)
- **OpenCode Model Variants**: Placed OpenCode model variants at top level instead of inside options for better discoverability (#1317)
- **Skills Import Flow**: Replaced implicit filesystem-based app inference with explicit `ImportSkillSelection` to prevent incorrect multi-app activation; added reconciliation to remove disabled/orphaned symlinks and MCP servers from live config
- **Claude 4.6 Context Window**: Updated Claude Opus 4.6 and Sonnet 4.6 context window from 200K to 1M across OpenClaw and OpenCode presets (GA release)
- **MiniMax Model Upgrade**: Updated MiniMax presets from M2.5 to M2.7 across Claude, OpenClaw, and OpenCode configurations with updated partner descriptions in all three locales
- **Xiaomi MiMo Model Upgrade**: Updated MiMo presets from mimo-v2-flash to mimo-v2-pro across all supported applications
- **AddProviderDialog Simplification**: Removed redundant OAuth tab, reducing dialog from 3 tabs to 2 (app-specific + universal)
- **Provider Form Advanced Options Collapse**: Model mapping, API format, and other advanced fields in the Claude provider form now auto-collapse when empty; auto-expands when any value is set or when a preset fills them in

### Fixed

- **WebDAV Password Silent Clear**: Fixed WebDAV password being silently wiped when ProviderList or UsageScriptModal saved settings by stripping `webdavSync` from frontend payloads and adding backend backfill logic in `merge_settings_for_save()` to preserve existing passwords
- **Tool Message Parsing**: Fixed tool_use/tool_result message classification across Claude (tool_result content blocks), Codex (function_call/function_call_output payloads), and Gemini (array content + toolCalls extraction) session providers (#1401)
- **Dark Mode Selector**: Changed Tailwind `darkMode` from `["selector", "class"]` to `["selector", ".dark"]` to ensure correct dark mode activation (#1596)
- **Copilot Request Fingerprint**: Unified Copilot request fingerprint headers across all API call sites to prevent User-Agent leakage and stream check mismatches
- **o-series Responses API Tokens**: Kept Responses API on the correct `max_output_tokens` field for o-series models instead of incorrectly injecting `max_completion_tokens`
- **Provider Form Double Submit**: Prevented duplicate submissions on rapid button clicks in provider add/edit forms (#1352)
- **Ghostty Session Restore**: Fixed Claude session restore in Ghostty terminal (#1506)
- **Skill ZIP Import Extension**: Added `.skill` file extension support in ZIP import dialog (#1240, #1455)
- **Skill ZIP Install Target App**: ZIP skill installs now use the currently active app instead of always defaulting to Claude
- **OpenClaw Active Card Highlight**: Fixed active OpenClaw provider card not being highlighted (#1419)
- **Responsive Layout with TOC**: Improved responsive design when TOC title exists (#1491)
- **Import Skills Dialog White Screen**: Added missing TooltipProvider in ImportSkillsDialog to prevent runtime crash when opening the dialog
- **Panel Bottom Blank Area**: Replaced hardcoded `h-[calc(100vh-8rem)]` with `flex-1 min-h-0` across all content panels to eliminate bottom gap caused by mismatched offset values

### Docs

- **Pricing Model ID Normalization**: Added documentation section explaining model ID normalization rules (prefix stripping, suffix trimming, `@`→`-` replacement) in EN/ZH/JA user manuals (#1591)
- **macOS Signed & Notarized**: Removed all `xattr` workaround instructions and "unidentified developer" warnings from README, README_ZH, installation guides (EN/ZH/JA), and FAQ pages (EN/ZH/JA); replaced with "signed and notarized by Apple" messaging

---

## [3.12.2] - 2026-03-12

Post-v3.12.1 work focuses on Common Config safety during proxy takeover and more reliable Codex TOML editing.

**Stats**: 5 commits | 22 files changed | +1,716 insertions | -288 deletions

### Added

- **Empty State Guidance**: Improved first-run experience with detailed import instructions and a conditional Common Config snippet hint for Claude/Codex/Gemini providers

### Changed

- **Proxy Takeover Restore Flow**: Proxy takeover hot-switch and provider sync now refresh the restore backup instead of overwriting live config files, rebuilding effective provider settings with Common Config applied so rollback preserves the real user configuration
- **Codex TOML Editing Engine**: Refactored Codex `config.toml` updates onto shared section-aware TOML helpers in Rust and TypeScript, covering `base_url` and `model` field edits across provider forms and takeover cleanup
- **Common Config Initialization Lifecycle**: Startup now auto-extracts Common Config snippets from clean live configs before takeover restoration, tracks explicit "snippet cleared" state, and persists a one-time legacy migration flag to avoid repeated backfills

### Fixed

- **Common Config Loss During Takeover**: Fixed cases where proxy takeover could drop Common Config changes, overwrite live configs during sync, or produce incomplete restore snapshots when switching providers
- **Codex Restore Snapshot Preservation**: Fixed Codex takeover restore backups so existing `mcp_servers` blocks survive provider hot-switches instead of being discarded; changed MCP backup preservation from wholesale table replacement to per-server-id merge so provider/common-config MCP updates win on conflict while live-only servers are retained
- **Cleared Snippet Resurrection**: Fixed startup auto-extraction recreating Common Config snippets that users had intentionally cleared
- **Codex `base_url` Misplacement**: Fixed Codex `base_url` extraction and editing to target the active `[model_providers.<name>]` section instead of appending to the file tail or confusing `mcp_servers.*.base_url` entries for provider endpoints

---

## [3.12.1] - 2026-03-12

### Patch Release

Stability-focused patch release fixing the Common Config modal infinite reopen loop, a WebDAV sync foreign key constraint failure, several i18n interpolation issues, and a Windows toolbar compact mode bug. Also adds **StepFun** provider presets, **OpenClaw input type selection** and **authHeader** support, upgrades Gemini to **3.1-pro**, and welcomes four new sponsor partners.

**Stats**: 19 commits | 56 files changed | +1,429 insertions | -396 deletions

### Added

#### Provider Presets

- **StepFun**: Added StepFun (阶跃星辰) provider presets including the step-3.5-flash model across supported applications (#1369, thanks @hengm3467)

#### OpenClaw Enhancements

- **Input Type Selection**: Added input type selection dropdown for model Advanced Options in OpenClaw configuration form (#1368, thanks @liuxxxu)
- **authHeader Field**: Added optional `authHeader` boolean to OpenClawProviderConfig for vendor-specific auth header support (e.g. Longcat), and refactored form state to reuse the shared type

#### Sponsor Partners

- **Micu API**: Added Micu API as sponsor partner with affiliate links
- **XCodeAPI**: Added XCodeAPI as sponsor partner
- **SiliconFlow**: Added SiliconFlow (硅基流动) as sponsor partner with affiliate links
- **CTok**: Added CTok as sponsor partner

### Changed

- **UCloud → Compshare**: Renamed UCloud provider to Compshare (优云智算) with full i18n support across all three locales (EN/ZH/JA)
- **Compshare Links**: Updated Compshare sponsor registration links to coding-plan page
- **Gemini Model Upgrade**: Upgraded default Gemini model from 2.5-pro to 3.1-pro in provider presets

### Fixed

#### Common Config & UI

- **Common Config Modal Loop**: Fixed an infinite reopen loop in the Common Config modal and added draft editing support to prevent data loss during edits
- **Toolbar Compact Mode (Windows)**: Fixed toolbar compact mode not triggering on Windows due to left-side overflow (#1375, thanks @zuoliangyu)
- **Session Search Index**: Fixed session search index not syncing with query data, causing stale list display after session deletion

#### Sync & Data

- **WebDAV Provider Health FK**: Fixed foreign key constraint failure when restoring `provider_health` table during WebDAV sync

#### Provider & Preset

- **Longcat authHeader**: Added missing `authHeader: true` to Longcat provider preset (#1377, thanks @wavever)
- **OpenClaw Tool Permissions**: Aligned OpenClaw tool permission profiles with upstream schema (#1355, thanks @bigsongeth)
- **X-Code API URL**: Corrected X-Code API URL from `www.x-code.cn` to `x-code.cc`

#### i18n & Localization

- **Stream Check Toast**: Fixed stream check toast i18n interpolation keys not matching translation placeholders
- **Proxy Startup Toast**: Fixed proxy startup toast not interpolating address and port values (#1399, thanks @Mason-mengze)
- **OpenCode API Format Label**: Renamed OpenCode API format label from "OpenAI" to "OpenAI Responses" for accuracy

---

## [3.12.0] - 2026-03-09

### Feature Release

This release restores the **Model Health Check (Stream Check)** UI, adds **OpenAI Responses API** format conversion, introduces the **Bedrock Optimizer** for thinking + cache injection, expands provider presets (Ucloud, Micu, X-Code API, Novita, Bailian For Coding), overhauls **OpenClaw config panels** with a JSON5 round-trip write engine, enhances **WebDAV sync** with dual-layer versioning, and delivers a comprehensive **i18n audit** fixing 69 missing keys alongside 20+ bug fixes.

**Stats**: 56 commits | 221 files changed | +20,582 insertions | -8,026 deletions

### Added

#### Stream Check (Model Health Check)

- **Restore Stream Check UI**: Brought back the model health check (Stream Check) panel for testing provider endpoint availability with live streaming validation
- **First-Run Confirmation**: Added a confirmation dialog on first use of Stream Check to inform users about the feature's purpose and network requests
- **OpenAI Chat Format Support**: Stream Check now supports `openai_chat` api_format, enabling health checks for providers using OpenAI-compatible endpoints

#### OpenAI Responses API

- **Responses API Format Conversion**: New `api_format = "openai_responses"` option enabling Anthropic Messages ↔ OpenAI Responses API bidirectional conversion for providers that implement the Responses API
- **Responses API Deduplication**: Deduplicated and improved the Responses API conversion logic, consolidating shared transformation code

#### Bedrock Optimizer

- **Bedrock Request Optimizer**: PRE-SEND optimizer that injects thinking parameters and cache control blocks into AWS Bedrock requests, enabling extended thinking and prompt caching on Bedrock endpoints (#1301)

#### OpenClaw Enhancements

- **JSON5 Round-Trip Write Engine**: Overhauled OpenClaw config panels with a JSON5 round-trip write engine that preserves comments, formatting, and ordering when saving configuration changes
- **Config Panel Improvements**: Redesigned EnvPanel as a full JSON editor, added `tools.profile` selection to ToolsPanel, introduced OpenClawHealthBanner for config validation warnings, and added legacy timeout migration support in Agents Defaults
- **Agent Model Dropdown**: Replaced text inputs with dropdown selects for OpenClaw agent model configuration, offering a curated list of available models
- **User-Agent Toggle**: Added a User-Agent header toggle for OpenClaw, defaulting to off to avoid potential compatibility issues with certain providers

#### Provider Presets

- **Ucloud**: Added Ucloud partner provider preset for Claude, Codex, and OpenClaw with endpointCandidates, unified apiKeyUrl, refreshed model defaults, and OpenClaw `templateValues` / `suggestedDefaults`
- **Micu**: Added Micu partner provider preset for Claude, Codex, OpenClaw, and OpenCode with OpenClaw `templateValues` / `suggestedDefaults`
- **X-Code API**: Added X-Code API partner provider preset for Claude, Codex, and OpenCode with endpointCandidates
- **Novita**: Added Novita provider presets and icon across all supported apps (#1192)
- **Bailian For Coding**: Added Bailian For Coding preset configuration (#1263)
- **SiliconFlow Partner Badge**: Added partner badge designation for SiliconFlow provider presets
- **Model Role Badges**: Added model role badges (e.g., Opus, Sonnet) to provider presets and reordered presets to prioritize Opus models

#### WebDAV Sync

- **Dual-Layer Versioning**: Added protocol v2 + db-v6 dual-layer versioning to WebDAV sync, enabling backward-compatible sync format evolution and automatic migration detection
- **Auto-Sync Confirmation**: Added a confirmation dialog when toggling WebDAV auto-sync on/off to prevent accidental changes

#### Usage & Data

- **Daily Rollups & Auto-Vacuum**: Added usage daily rollups for aggregated statistics, incremental auto-vacuum for storage management, and sync-aware backup that coordinates with WebDAV sync cycles
- **UsageFooter Extra Fields**: Added extra field display in UsageFooter component for normal mode, showing additional usage metadata (#1137)

#### Session Management

- **Session Deletion**: Added session deletion with per-provider cleanup and path safety validation, allowing users to remove individual conversation sessions

#### UI & Config

- **Auth Field Selector**: Restored Claude provider auth field selector supporting both AUTH_TOKEN and API_KEY authentication modes
- **Failover Toggle**: Moved failover toggle to display independently on the main page with a confirmation dialog for enabling/disabling
- **Common Config Auto-Extract**: Auto-extract Common Config Snippets from live configuration files on first run, seeding initial common config without manual setup
- **New Provider Page Improvements**: Improved the new provider page with API endpoint and model name fields (#1155)

### Changed

#### Architecture

- **Common Config Runtime Overlay**: Common Config is now applied as a runtime overlay during provider switching instead of being materialized (merged) into each provider's stored config. This preserves the original provider config in the database and applies common settings dynamically at write time
- **First-Run Auto-Extract**: On first run, Common Config Snippets are automatically extracted from the current live configuration files, eliminating the need for manual initial setup

### Fixed

#### Proxy & Streaming

- **OpenAI Streaming Conversion**: Fixed OpenAI ChatCompletion → Anthropic Messages streaming conversion that could produce malformed events under certain response structures
- **Codex /responses/compact Route**: Added support for Codex `/responses/compact` route in proxy forwarding (#1194)
- **Codex Common Config TOML Merge**: Fixed Codex Common Config to use structural TOML merge/subset instead of raw string comparison, correctly handling key ordering and formatting differences
- **Proxy Forwarder Failure Logs**: Improved proxy forwarder failure logging with more descriptive error messages

#### Provider & Preset

- **X-Code Rename**: Renamed "X-Code" provider to "X-Code API" for consistency with the official branding
- **SSSAiCode Missing /v1**: Added missing `/v1` path to SSSAiCode default endpoint for Codex and OpenCode
- **AICoding URL Fix**: Removed `www` prefix from aicoding.sh provider URLs to match the correct domain
- **New Provider Page Input Handling**: Fixed the new provider page so API endpoint / model fields handle line-break deletion correctly and added the missing `codexConfig.modelNameHint` i18n key for zh/en/ja

#### Platform

- **Cache Hit Token Statistics**: Fixed missing token statistics for cache hits in streaming responses (#1244)
- **Minimize-to-Tray Auto Exit**: Fixed issue where the application would automatically exit after being minimized to the system tray for a period of time (#1245)

#### i18n & Localization

- **Comprehensive i18n Audit**: Added 69 missing i18n keys and fixed hardcoded Chinese strings across the application, improving localization coverage for all three languages (zh/en/ja)
- **Model Test Panel i18n**: Corrected i18n key paths for model test panel title and description
- **JSON5 Slash Escaping**: Normalized JSON5 slash escaping and added i18n support for OpenClaw panel labels

#### UI

- **Skills Count Display**: Fixed skills count not displaying correctly when adding new skills (#1295)
- **Endpoint Speed Test**: Removed HTTP status code display from endpoint speed test results to reduce visual noise
- **Outline Button Text Tone**: Aligned outline button text color tone with usage refresh control for visual consistency (#1222)

### Performance

- **OpenClaw Config Write Skip**: Skip backup and atomic write when OpenClaw configuration content is unchanged, avoiding unnecessary I/O operations

### Documentation

- **User Manual i18n**: Restructured user manual for internationalization and added complete EN/JA translations alongside the existing ZH documentation
- **User Manual OpenClaw**: Added OpenClaw coverage and completed settings documentation for the user manual
- **UCloud CompShare Sponsor**: Added UCloud CompShare as a sponsor partner
- **Docs Directory Reorganization**: Reorganized docs directory structure, added user manual links to all three README files, removed cross-language links from user manual sections, and synced README features across EN/ZH/JA

### Maintenance

- **Periodic Maintenance Timer**: Consolidated periodic maintenance timers into a unified scheduler, combining vacuum and rollup operations into a single timer
- **OpenClaw Save Toast**: Removed backup path display from OpenClaw save toasts for cleaner notification messages

---

## [3.11.1] - 2026-02-28

### Hotfix Release

This release reverts the Partial Key-Field Merging architecture introduced in v3.11.0, restoring the proven "full config overwrite + Common Config Snippet" mechanism, and fixes several UI and platform compatibility issues.

**Stats**: 8 commits | 52 files changed | +3,948 insertions | -1,411 deletions

### Reverted

- **Restore Full Config Overwrite + Common Config Snippet** (revert 992dda5c): Reverted the partial key-field merging refactoring from v3.11.0 due to critical issues — non-whitelisted custom fields were lost during provider switching, backfill permanently stripped non-key fields from the database, and the whitelist required constant maintenance. Restores full config snapshot write, Common Config Snippet UI and backend commands, and 6 frontend components/hooks

### Changed

- **Proxy Panel Layout**: Moved proxy on/off toggle from accordion header into panel content area, placed directly above app takeover options, ensuring users see takeover configuration immediately after enabling the proxy
- **Manual Import for OpenCode/OpenClaw**: Removed auto-import on startup; empty state now shows an "Import Current Config" button, consistent with Claude/Codex/Gemini behavior

### Fixed

- **"Follow System" Theme Not Auto-Updating**: Delegated to Tauri's native theme tracking (`set_window_theme(None)`) so the WebView's `prefers-color-scheme` media query stays in sync with OS theme changes
- **Compact Mode Cannot Exit**: Restored `flex-1` on `toolbarRef` so `useAutoCompact`'s exit condition triggers correctly based on available width instead of content width
- **Proxy Takeover Toast Shows {{app}}**: Added missing `app` interpolation parameter to i18next `t()` calls for proxy takeover enabled/disabled messages
- **Windows Protocol Handler Side Effects**: Disabled environment check and one-click install on Windows to prevent unintended protocol handler registration

---

## [3.11.0] - 2026-02-26

### Feature Release

This release introduces **OpenClaw** as the fifth supported application, a full **Session Manager** for browsing conversation history across all apps, an independent **Backup Management** panel, **Oh My OpenCode (OMO)** integration, and 50+ other features, fixes, and improvements across 147 commits.

**Stats**: 147 commits | 274 files changed | +32,179 insertions | -5,467 deletions

### Added

#### OpenClaw Support (New Application)

- **OpenClaw Integration**: Full management support for OpenClaw as the fifth application in CC Switch, including provider switching, configuration panels (Env / Tools / Agents Defaults), Workspace file management (HEARTBEAT / BOOTSTRAP / BOOT), daily memory files, and additive overlay mode
- **OpenClaw Provider Presets**: 13+ built-in provider presets with brand icon and complete i18n (zh/en/ja)
- **OpenClaw Form Fields**: Dedicated provider form with providerKey input, model allowlist auto-registration, and default model button
- **OpenClaw Config Panels**: Env editor, Tools editor, and Agents Defaults editor backed by JSON5 read/write (`openclaw_config.rs`)

#### Session Manager

- **Session Manager**: Browse and search conversation history for Claude Code, Codex, Gemini CLI, OpenCode, and OpenClaw with table-of-contents navigation and in-session search
- **Session App Filter**: Auto-filter sessions by current app when entering the session page
- **Session Performance**: Parallel directory scanning and head-tail JSONL reading for faster session list loading

#### Backup Management

- **Backup Panel**: Independent backup management panel with configurable backup policy (max count, auto-cleanup) and backup rename support
- **Periodic Backup**: Hourly automatic backup timer during runtime
- **Pre-Migration Backup**: Automatic backup before database schema migrations with backfill warning
- **Delete Backup**: Delete individual backup files with confirmation dialog
- **Backup Time Fix**: Use local time instead of UTC for backup file names

#### Oh My OpenCode (OMO)

- **OMO Integration**: Full Oh My OpenCode config file management with agent model selection, category configuration, and recommended model fill
- **OMO Slim**: Lightweight oh-my-opencode-slim mode support with OmoVariant parameterization
- **OMO Cross-Exclusion**: Enforce OMO ↔ OMO Slim mutual exclusion at the database level

#### Workspace

- **Daily Memory Search**: Full-text search across daily memory files with date-sorted display
- **Clickable Paths**: Directory paths in workspace panels are now clickable; renamed “Today's Note” to “Add Memory”
- **Workspace Files Panel**: Manage bootstrap markdown files for OpenClaw (HEARTBEAT / BOOTSTRAP / BOOT types)

#### Provider Presets

- **AWS Bedrock**: Support for AKSK and API Key authentication modes (Claude and OpenCode)
- **SSAI Code**: Partner provider preset across all five apps
- **CrazyRouter**: Partner provider preset with custom icon
- **AICoding**: Partner provider preset with i18n promotion text
- **Bailian**: Renamed from Qwen Coder with new icon; updated domestic model providers to latest versions

#### Proxy & Network

- **Thinking Budget Rectifier**: New rectifier for thinking budget parameters with dedicated module (`thinking_budget_rectifier.rs`)
- **WebDAV Auto Sync**: Automatic periodic sync with large file protection mechanism

#### UI & UX

- **Theme Animation**: Circular reveal animation when toggling between light and dark themes
- **Claude Quick Toggles**: Quick toggle switches in the Claude config JSON editor for common settings
- **Dynamic Endpoint Hint**: Context-aware hint text in endpoint input based on API format selection
- **AppSwitcher Auto Compact**: Automatically collapse to compact mode based on available width, with smooth transition animation
- **App Transition**: Fade-in/fade-out animation when switching between OpenClaw and other apps
- **Silent Startup Conditional**: Show silent startup option only when launch-on-startup is enabled

#### Settings & Environment

- **First-Run Confirmation**: Confirmation dialogs for proxy and usage features on first use
- **Local Proxy Toggle**: `enableLocalProxy` setting to control proxy UI visibility on the home page
- **Environment Check**: More granular local environment detection (installed CLI tool versions, Volta path detection)

#### Usage & Pricing

- **Usage Dashboard Enhancement**: Auto-refresh control, robust formatting, and request log table improvements
- **New Model Pricing**: Added pricing data for claude-opus-4-6 and gpt-5.3-codex with incremental data seeding

### Changed

#### Architecture

- **Partial Key-Field Merging (⚠️ Breaking, reverted in v3.11.1)**: Provider switching now uses partial key-field merging instead of full config overwrite, preserving user's non-provider settings (plugins, MCP, permissions). The "Common Config Snippet" feature has been removed as it is no longer needed. Removes 6 frontend files and ~150 lines of backend dead code (#1098)
- **Manual Import**: Replaced auto-import on startup with manual “Import Current Config” button in empty state, reducing ~47 lines of startup code
- **OMO Variant Parameterization**: Eliminated ~250 lines of OMO/OMO Slim code duplication via `OmoVariant` struct with STANDARD/SLIM constants
- **OMO Common Config Removal**: Removed the two-layer merge system for OMO common config (-1,733 lines across 21 files)

#### Code Quality

- **ProviderForm Decomposition**: Extracted ProviderForm.tsx from 2,227 lines to 1,526 lines by splitting into 5 focused modules (opencodeFormUtils, useOmoModelSource, useOpencodeFormState, useOmoDraftState, useOpenclawFormState)
- **Shared MCP/Skills Components**: Extracted AppCountBar, AppToggleGroup, and ListItemRow shared components to eliminate duplication across MCP and Skills panels
- **OpenClaw TanStack Query Migration**: Migrated Env, Tools, and AgentsDefaults panels from manual useState/useEffect to centralized TanStack Query hooks

#### Settings Layout

- **Proxy Tab**: Split Advanced tab into dedicated Proxy tab (local proxy, failover, rectifiers, global outbound proxy); moved pricing config to Usage dashboard as collapsible accordion. SettingsPage reduced from ~716 to ~426 lines with 5-tab layout: General | Proxy | Advanced | Usage | About
- **Data Section Split**: Split data accordion into Import/Export and Cloud Sync sections for better discoverability

#### Terminal & Config

- **Unified Terminal Selection**: Consolidated terminal preference to global settings; added WezTerm support and terminal name mapping (iterm2 → iterm)
- **OpenClaw Agents Panel**: Primary model field set to read-only; detailed model fields (context window, max tokens, reasoning, cost) moved to advanced options
- **Claude Model Update**: Updated Claude model references from 4.5 to 4.6 across all provider presets

### Fixed

#### Critical

- **Windows Home Dir Regression**: Restored default home directory resolution on Windows to prevent providers/settings “disappearing” when `HOME` env var differs from the real user profile directory (Git/MSYS environments); auto-detects v3.10.3 legacy database location
- **Linux White Screen**: Disabled WebKitGTK hardware acceleration on AMD GPUs (Cezanne/Radeon Vega) to prevent EGL initialization failure causing blank screen on startup
- **OpenAI Beta Parameter**: Stopped appending `?beta=true` to OpenAI Chat Completions endpoints, fixing request failures for Nvidia and other `apiFormat=”openai_chat”` providers
- **Health Check Auth Mode**: Health check now respects provider's auth_mode setting instead of always using x-api-key header

#### Provider & Preset

- **OpenClaw /v1 Prefix**: Removed /v1 prefix from OpenClaw anthropic-messages presets to prevent double path (/v1/v1/messages) with Anthropic SDK auto-append
- **Opus Pricing**: Corrected Opus pricing from $15/$75 to $5/$25 and upgraded model ID to claude-opus-4-6
- **AIGoCode URLs**: Unified API base URL to https://api.aigocode.com across all apps; removed trailing /v1 suffix
- **Zhipu GLM**: Removed outdated partner status from Claude, OpenCode, and OpenClaw presets
- **API Key Visibility**: Restored API Key input field when creating new Claude providers (was incorrectly hidden for non-cloud_provider categories)

#### OMO / OMO Slim

- **OMO Slim Category Checks**: Added missing omo-slim category checks across add/form/mutation paths
- **OMO Slim Cache Invalidation**: Invalidate OMO Slim query cache after provider mutations to prevent stale UI state
- **OMO Recommended Models**: Synced agent/category recommended models with upstream sources; fixed provider/model format to pure model IDs
- **OMO Fill Feedback**: Added toast feedback when “Fill Recommended” button silently fails
- **OMO Last-Provider Restriction**: Removed last-provider deletion restriction for OMO/OMO Slim plugins
- **OpenCode Model Validation**: Reject saving OpenCode providers without at least one configured model

#### OpenClaw

- **OpenClaw P0-P3 Fixes**: Fixed 25 missing i18n keys, replaced key={index} with stable crypto.randomUUID(), excluded openclaw from ProxyToggle/FailoverToggle, added deep link merge_additive_config(), unified serde(flatten) naming, added directory existence checks, removed dead code, added duplicate key validation
- **OpenClaw Robustness**: Fixed EnvPanel visibleKeys using entry key names instead of array indices; added NaN guards; validated provider ID and model before import
- **OpenClaw i18n Dedup**: Merged duplicate openclaw i18n keys to restore provider form translations

#### Platform

- **Window Flash**: Prevented window flicker on silent startup (Windows)
- **Title Bar Theme**: Title bar now follows dark/light mode theme changes
- **Skills Path Separator**: Fixed path separator matching for skill installation status on Windows (supports both `/` and `\`)
- **WSL Conditional Compilation**: Added `#[cfg(target_os = “windows”)]` to WSL helper functions to eliminate dead_code warnings on non-Windows platforms

#### UI

- **Toolbar Clipping**: Removed toolbar height limit that was clipping AppSwitcher
- **Update Badge**: Show update badge instead of green check when a newer version is available
- **Session Button Visibility**: Only show Session Manager button for Claude and Codex apps
- **Directory Spacing**: Added vertical spacing between directory setting sections
- **Dark Mode Cards**: Unified SQL import/export card styling in dark mode
- **OpenClaw Scroll**: Enabled scrolling for OpenClaw configuration panel content

#### i18n & Localization

- **Session Manager i18n**: Replaced hardcoded Chinese strings with i18n keys for relative time, role labels, and UI elements
- **OpenClaw Default Model Label**: Renamed “Enable/Default” to “Set as Default / Current Default” with wider button
- **Daily Memory Sort**: Sort daily memory files by filename date (YYYY-MM-DD.md) instead of modification time
- **Backup Name i18n**: Use local time for backup file names

#### Other

- **Skill Doc URL**: Use actual branch from download_repo for documentation URL; switched from /tree/ to /blob/ pointing to SKILL.md
- **OpenCode Install Detection**: Added install.sh priority paths (OPENCODE_INSTALL_DIR > XDG_BIN_DIR > ~/bin > ~/.opencode/bin) with path dedup and cross-platform executable candidates
- **Provider Auto-Import**: Removed auto-import side effect from useProvidersQuery queryFn; users now trigger import manually via empty state button
- **Manual Backup Validation**: Treat missing database file as error during manual backup to prevent false success toast

### Performance

- **Session Panel Loading**: Parallel directory scanning and head-tail JSONL reading for Codex, OpenClaw, and OpenCode session providers
- **Query Cache Cleanup**: Removed unnecessary TanStack Query cache overhead for Tauri local IPC calls

### Documentation

- **Sponsors**: Added/updated SSSAiCode, Crazyrouter, AICoding, Right Code, and MiniMax sponsor entries across all README languages
- **User Manual**: Added user manual documentation (#979)

### Maintenance

- **Pre-Release Cleanup**: Removed debug logs, fixed clippy warnings, added missing Japanese translations, and formatted code
- **UI Exclusions**: Hidden MCP, Skills, proxy/pricing, stream check, and model test panels for OpenClaw where not applicable

---

## [3.10.3] - 2026-01-30

### Feature Release

This release introduces a generic API format selector, pricing configuration enhancements, and multiple UX improvements.

### Added

- **API Key Link for OpenCode**: API key link support for OpenCode provider form, enabling quick access to provider key management pages
- **AICodeMirror Partner Preset**: Added AICodeMirror partner preset for all apps (Claude, Codex, Gemini, OpenCode)
- **API Format Selector**: Generic API format chooser for Claude providers, replacing the OpenRouter-specific toggle. Supports Anthropic Messages (native) and OpenAI Chat Completions format
- **API Format Presets**: Allow preset providers to specify API format (anthropic or openai_chat) for third-party proxy services
- **Proxy Hint**: Display info toast when switching to OpenAI Chat format provider, reminding users to enable proxy
- **Pricing Config Enhancement**: Per-provider cost multiplier, pricing model source (request/response), request model logging, and enriched usage UI (#781)
- **Skills ZIP Install**: Install skills directly from local ZIP files with recursive scanning support
- **Preferred Terminal**: Choose preferred terminal app per platform (macOS: Terminal.app/iTerm2/Alacritty/Kitty/Ghostty; Windows: cmd/PowerShell/Windows Terminal; Linux: GNOME Terminal/Konsole/Xfce4/Alacritty/Kitty/Ghostty)
- **Silent Startup**: Option to prevent window popup on launch (#713)
- **OpenCode Environment Check**: Version detection with Go path scanning and one-click install from GitHub Releases
- **OpenCode Directory Sync**: Auto-sync all providers to live config on directory change with additive mode support
- **NVIDIA NIM Preset**: New provider preset for Claude and OpenCode with nvidia.svg icon
- **n1n.ai Preset**: New provider preset (#667)
- **Update Badge Icon**: Replace update badge dot with ArrowUpCircle icon
- **Linux ARM64**: CI build support for Linux ARM64 architecture

### Changed

- **API Format Migration**: Migrate api_format from settings_config to ProviderMeta to prevent polluting ~/.claude/settings.json
- **DeepSeek max_tokens**: Remove max_tokens clamp from proxy transform layer
- **Terminal Functions**: Consolidate redundant terminal launch functions
- **Home Dir Utility**: Consolidate get_home_dir into single public function
- **Kimi/Moonshot**: Upgrade provider presets to k2.5 model

### Fixed

- **Codex 404 & Timeout**: Fix 404 errors and connection timeout with custom base_url; improve /v1 prefix handling and system proxy detection (#760)
- **Proxy URL Building**: Fix duplicate /v1/v1 in URL; extend ?beta=true to /v1/chat/completions endpoint
- **OpenRouter Compat Mode**: Improve backward compatibility supporting number and string types
- **Gemini Visibility**: Correct Gemini default visibility to true (#818)
- **Footer Layout**: Correct footer layout in advanced settings tab
- **Claude Code Detection**: Prioritize native install path for detection
- **Tray Menu**: Simplify title labels and optimize menu separators (#796)
- **Duplicate Skills**: Prevent duplicate skill installation from different repos (#778)
- **Windows Tests**: Stabilize test environment (#644)
- **i18n**: Update apiFormatOpenAIChat label to mention proxy requirement
- **Error Display**: Use extractErrorMessage for complete error display in mutations
- **Sponsors**: Add AICodeMirror and reorder sponsor list

---

## [3.10.2] - 2026-01-24

### Patch Release

This maintenance release adds skill sync options and includes important bug fixes.

### Added

- **Skills**: Add skill sync method setting with symlink/copy options
- **Partners**: Add RightCode as official partner

### Fixed

- **Prompts**: Clear prompt file when all prompts are disabled
- **OpenCode**: Preserve extra model fields during serialization
- **Provider Form**: Backfill model fields when editing Claude provider

---

## [3.10.1] - 2026-01-23

### Patch Release

This maintenance release includes important bug fixes for Windows platform, UI improvements, and code quality enhancements.

### Added

- **Provider Icons**: Updated RightCode provider icon with improved visual design

### Changed

- **Proxy Rectifier**: Changed rectifier default state to disabled for better stability
- **Window Settings**: Reordered window settings and updated default values for improved UX
- **UI Layout**: Increased app icon collapse threshold from 3 to 4 icons
- **Code Quality**: Simplified `RectifierConfig` implementation using `#[derive(Default)]`

### Fixed

- **Windows Platform**:
  - Fixed terminal window closing immediately after execution on Windows
  - Corrected OpenCode config path resolution on Windows
- **UI Improvements**:
  - Fixed ProviderIcon color validation to prevent black icons from appearing
  - Unified layout padding across all panels for consistent spacing
  - Fixed panel content alignment with header constraints
- **Code Quality**: Resolved Rust Clippy warnings and applied consistent formatting

---

## [3.10.0] - 2026-01-21

### Feature Release

This release introduces OpenCode support and brings improvements across proxy, usage tracking, and overall UX.

### Added

- **OpenCode Support** - Manage OpenCode providers, MCP servers, and Skills, with first-launch import and full internationalization (#695)
- **Global Proxy** - Add global proxy settings for outbound network requests (#596)
- **Claude Rectifier** - Add thinking signature rectifier for Claude API (#595)
- **Health Check Enhancements** - Configurable prompt and CLI-compatible requests for stream health check (#623)
- **Per-Provider Config** - Support provider-specific configuration and persistence (#663)
- **App Visibility Controls** - Show/hide apps and keep tray menu in sync (Gemini hidden by default)
- **Takeover Compact Mode** - Use a compact AppSwitcher layout when showing 3+ visible apps
- **Keyboard Shortcut** - Press `ESC` to quickly go back/close panels (#670)
- **Terminal Improvements** - Provider-specific terminal button, `fnm` path support, and safer cross-platform launching (#564)
- **WSL Tool Detection** - Detect tool versions in WSL with additional security hardening (#627)
- **Skills Presets** - Add `baoyu-skills` preset repo and auto-supplement missing default repos

### Changed

- **Proxy Logging** - Simplify proxy log output (#585)
- **Pricing Editor UX** - Unify pricing edit modal with `FullScreenPanel`
- **Advanced Settings Layout** - Move rectifier section below failover for better flow
- **OpenRouter Compat Mode** - Disable OpenRouter compatibility mode by default and hide UI toggle

### Fixed

- **Auto Failover** - Switch to P1 immediately when enabling auto failover
- **Provider Edit Dialog** - Fix stale data when reopening provider editor after save (#654)
- **Deeplink** - Support multiple endpoints and prioritize `GOOGLE_GEMINI_BASE_URL` over `GEMINI_BASE_URL` (#597)
- **MCP (WSL)** - Skip `cmd /c` wrapper for WSL target paths (#592)
- **Usage Templates** - Add variable hints and validation fixes; prevent config leaking between providers (#628)
- **Gemini Timeout Format** - Convert timeout params to Gemini CLI format (#580)
- **UI** - Fix Select dropdown rendering in `FullScreenPanel`; auto-apply default icon color when unset
- **Usage UI** - Auto-adapt usage block offset based on action buttons width (#613)
- **Provider Endpoint** - Persist endpoint auto-select state (#611)
- **Provider Form** - Reset baseUrl and apiKey states when switching presets

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

- 51 commits since v3.7.1; 207 files changed; +17,297 / -6,870 lines. See [release-note-v3.8.0](docs/release-notes/v3.8.0-en.md) for details.

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
