# CC Switch v3.11.0

> OpenClaw Support, Session Manager, Backup Management & 50+ Improvements

**[中文版 →](release-note-v3.11.0-zh.md) | [日本語版 →](release-note-v3.11.0-ja.md)**

---

## Overview

CC Switch v3.11.0 is a major update that adds full management support for **OpenClaw** as the fifth application, introduces a new **Session Manager** and **Backup Management** feature. Additionally, **Oh My OpenCode (OMO) integration**, the **partial key-field merging** architecture upgrade for provider switching, **settings page refactoring**, and many other improvements make the overall experience more polished.

**Release Date**: 2026-02-26

**Update Scale**: 147 commits | 274 files changed | +32,179 / -5,467 lines

---

## Highlights

- **OpenClaw Support**: Fifth managed application with 13 provider presets, Env/Tools/AgentsDefaults config editors, and Workspace file management
- **Session Manager**: Browse conversation history across all five apps with table-of-contents navigation and in-session search
- **Backup Management**: Independent backup panel with configurable policies, periodic backups, and pre-migration auto-backup
- **Oh My OpenCode Integration**: Full OMO config management with OMO Slim lightweight mode support
- **Partial Key-Field Merging (⚠️ Breaking Change)**: Provider switching now only replaces provider-related fields, preserving all other settings; the "Common Config Snippet" feature has been removed
- **Settings Page Refactoring**: 5-tab layout with ~40% code reduction
- **6 New Provider Presets**: AWS Bedrock, SSAI Code, CrazyRouter, AICoding, and more
- **Thinking Budget Rectifier**: Fine-grained thinking budget control
- **Theme Switch Animation**: Circular reveal transition animation
- **WebDAV Auto Sync**: Automatic sync with large file protection

---

## Main Features

### OpenClaw Support (New Fifth App)

Full management support for OpenClaw, the fifth managed application following Claude Code, Codex, Gemini CLI, and OpenCode.

- **Provider Management**: Add, edit, switch, and delete OpenClaw providers with 13 built-in presets
- **Config Editors**: Three dedicated panels for Env (environment variables), Tools, and AgentsDefaults
- **Workspace Panel**: HEARTBEAT/BOOTSTRAP/BOOT file management and daily memory
- **Additive Overlay Mode**: Support config overlay instead of overwrite
- **Default Model Button**: One-click to fill recommended models; auto-register suggested models to allowlist when adding providers
- **Brand & Interaction**: Dedicated brand icon, fade-in/fade-out transition animation when switching apps
- **Deep Link Support**: Import OpenClaw provider configurations via URL
- **Full Internationalization**: Complete Chinese/English/Japanese support

### Session Manager

A brand-new session manager to browse and search conversation history.

- Browse conversation history across Claude Code, Codex, Gemini CLI, OpenCode, and OpenClaw (#867, thanks @TinsFox)
- Table-of-contents navigation and in-session search
- Auto-filter by current app when entering the session page
- Parallel directory scanning + head-tail JSONL reading for optimized loading performance

### Backup Management

An independent backup management panel for better data safety.

- Configurable backup policy: maximum backup count and auto-cleanup rules
- Hourly automatic backup timer during runtime
- Auto-backup before database schema migrations with backfill warning
- Support backup rename and deletion (with confirmation dialog)
- Backup filenames use local time for better clarity

### Oh My OpenCode (OMO) Integration

Full Oh My OpenCode config file management.

- Agent model selection, category configuration, and recommended model fill (#972, thanks @yovinchen)
- Improved agent model selection UX with lowercase key fix (#1004, thanks @yovinchen)
- OMO Slim lightweight mode support
- OMO ↔ OMO Slim mutual exclusion (enforced at database level)

### Workspace

- Full-text search across daily memory files, sorted by date
- Clickable directory paths for quick file location access

### Toolbar

- AppSwitcher auto-collapses to compact mode based on available width
- Smooth transition animation for compact mode toggle

### Settings

- First-use confirmation dialogs for proxy and usage features to prevent accidental operations
- New `enableLocalProxy` switch to control proxy UI visibility on home page
- More granular local environment checks: CLI tool version detection (#870, thanks @kv-chiu), Volta path detection (#969, thanks @myjustify)

### Provider Presets

- **AWS Bedrock**: Support for AKSK and API Key authentication modes (#1047, thanks @keithyt06)
- **SSAI Code**: Partner preset across all five apps
- **CrazyRouter**: Partner preset with dedicated icon
- **AICoding**: Partner preset with i18n promotion text
- Updated domestic model provider presets to latest versions
- Renamed Qwen Coder to Bailian (#965, thanks @zhu-jl18)

### Other New Features

- **Thinking Budget Rectifier**: Fine-grained thinking budget allocation control (#1005, thanks @yovinchen)
- **WebDAV Auto Sync**: Automatic sync with large file protection (#923, thanks @clx20000410; #1043, thanks @SaladDay)
- **Theme Switch Animation**: Circular reveal transition for a smoother visual experience (#905, thanks @funnytime75)
- **Claude Config Editor Quick Toggles**: Quick toggle switches for common settings (#1012, thanks @JIA-ss)
- **Dynamic Endpoint Hint**: Context-aware hint text based on API format selection (#860, thanks @zhu-jl18)
- **Usage Dashboard Enhancement**: Auto-refresh control and robust formatting (#942, thanks @yovinchen)
- **New Pricing Data**: claude-opus-4-6 and gpt-5.3-codex (#943, thanks @yovinchen)
- **Silent Startup Optimization**: Silent startup option only shown when launch-on-startup is enabled

---

## Architecture Improvements

### Partial Key-Field Merging (⚠️ Breaking Change)

Provider switching now uses partial key-field merging instead of full config overwrite (#1098).

**Before**: Switching providers overwrote the entire `settings_config` to the live config file. This meant that any non-provider settings the user manually added to the live file (plugins, MCP config, permissions, etc.) would be lost on every switch. To work around this, previous versions offered a "Common Config Snippet" feature that let users define shared config to be merged on every switch.

**After**: Switching providers now only replaces provider-related key-values (API keys, endpoints, models, etc.), leaving all other settings intact. The "Common Config Snippet" feature is therefore no longer needed and has been removed.

**Impact & Migration**:
- If you **didn't use** Common Config Snippets, this change is fully transparent — switching just works better now
- If you **used** Common Config Snippets to preserve custom settings (MCP config, permissions, etc.), those settings are now automatically preserved during switches — no action needed
- If you used Common Config Snippets for other purposes (e.g., injecting extra config on every switch), please manually add those settings to your live config file after upgrading

This refactoring removed 6 frontend files (3 components + 3 hooks) and ~150 lines of backend dead code.

### Manual Import Replaces Auto-Import

Startup no longer auto-imports external configurations. Users now click "Import Current Config" manually, preventing accidental data overwrites.

### OmoVariant Parameterization

Eliminated ~250 lines of duplicated code in the OMO module via `OmoVariant` struct parameterization.

### OMO Common Config Removal

Removed the two-layer merge system, reducing ~1,733 lines of code and simplifying the architecture.

### ProviderForm Decomposition

Reduced ProviderForm component from 2,227 lines to 1,526 lines by extracting 5 independent modules (opencodeFormUtils, useOmoModelSource, useOpencodeFormState, useOmoDraftState, useOpenclawFormState), significantly improving maintainability.

### Shared MCP/Skills Components

Extracted AppCountBar, AppToggleGroup, and ListItemRow shared components to reduce duplication across MCP and Skills panels (#897, thanks @PeanutSplash).

### Settings Page Refactoring

Refactored settings page to a 5-tab layout (General | Proxy | Advanced | Usage | About), reducing SettingsPage code from ~716 to ~426 lines.

### Other Improvements

- Unified terminal selection via global settings with WezTerm support added
- Updated Claude model references from 4.5 to 4.6

---

## Bug Fixes

### Critical Fixes

- **Windows Home Dir Regression**: Restored default home directory resolution to prevent providers/settings "disappearing" when `HOME` env var differs from the real user profile directory in Git/MSYS environments
- **Linux White Screen**: Disabled WebKitGTK hardware acceleration on AMD GPUs (Cezanne/Radeon Vega) to prevent blank screen on startup (#986, thanks @ThendCN)
- **OpenAI Beta Parameter**: Stopped appending `?beta=true` to `/v1/chat/completions` endpoints, fixing request failures for Nvidia and other `apiFormat="openai_chat"` providers (#1052, thanks @jnorthrup)
- **Health Check Auth**: Health check now respects provider's `auth_mode` setting, preventing failures for proxy services that only support Bearer authentication (#824, thanks @Jassy930)

### Provider Preset Fixes

- Fixed OpenClaw `/v1` prefix causing double path (/v1/v1/messages)
- Corrected Opus pricing ($15/$75 → $5/$25) and upgraded to 4.6
- Unified AIGoCode URL to `https://api.aigocode.com` across all apps
- Removed outdated partner status from Zhipu GLM presets
- Restored API Key input visibility when creating new Claude providers
- Hide quick toggles for non-active providers, show context-aware JSON editor hints

### OMO Fixes

- Added missing omo-slim category checks across add/form/mutation paths
- Fixed OMO Slim query cache invalidation after provider mutations
- Synced OMO agent/category recommended models with upstream sources
- Added toast feedback for "Fill Recommended" button silent failures
- Removed last-provider deletion restriction for OMO/OMO Slim
- Reject saving OpenCode providers without configured models (#932, thanks @yovinchen)

### OpenClaw Fixes

- Fixed 25 missing i18n keys, replaced key={index} with stable IDs, added deep link additive merge, and other code review issues
- Enhanced EnvPanel robustness (NaN guards, entry key names instead of array indices)
- Merged duplicate i18n keys to restore provider form translations

### Platform Fixes

- Windows silent startup window flicker (#901, thanks @funnytime75)
- Title bar dark mode theme following (#903, thanks @funnytime75)
- Windows Skills path separator matching (#868, thanks @stmoonar)
- WSL helper functions conditional compilation

### UI Fixes

- Toolbar height clipping causing AppSwitcher to be obscured
- Show update badge instead of green checkmark when newer version available
- Session Manager button only visible for Claude/Codex apps
- Unified SQL import/export card dark mode styling (#1067, thanks @SaladDay)

### Other Fixes

- Replaced hardcoded Chinese strings in Session Manager with i18n keys
- Fixed Skill documentation URL branch and path resolution (#977, thanks @yovinchen)
- Added missing OpenCode install.sh installation path detection (#988, thanks @zhu-jl18)
- Fixed Skill ZIP symlink resolution (#1040, thanks @yovinchen)
- Added missing OpenCode checkbox in MCP add/edit form (#1026, thanks @yovinchen)
- Removed auto-import side effect from useProvidersQuery queryFn

---

## Performance

- Parallel directory scanning + head-tail JSONL reading for session panel, significantly improving session list loading speed
- Removed unnecessary TanStack Query cache overhead for Tauri local IPC calls

---

## Documentation

- Sponsor updates: SSSAiCode, Crazyrouter, AICoding, Right Code, MiniMax
- Added user manual documentation (#979, thanks @yovinchen)

---

## Notes & Considerations

- **OpenClaw is a newly supported app**: OpenClaw CLI must be installed first to use related features.
- **⚠️ Common Config Snippet feature has been removed**: Since provider switching now uses partial key-field merging (only replacing API keys, endpoints, models, etc.), user's other settings are automatically preserved, making Common Config Snippets unnecessary. See the "Architecture Improvements" section above for migration details.
- **Auto-import changed to manual**: External configurations are no longer auto-imported on startup. Click "Import Current Config" manually when needed.
- **OMO and OMO Slim are mutually exclusive**: Only one can be active at a time. Switching to one automatically disables the other.
- **Backup is enabled by default**: Automatic hourly backup during runtime. Adjust the policy in the Backup panel.

---

## Special Thanks

Thanks to all contributors for their contributions to this release!

@TinsFox @keithyt06 @kv-chiu @SaladDay @jnorthrup @JIA-ss @clx20000410 @ThendCN @yovinchen @zhu-jl18 @myjustify @funnytime75 @PeanutSplash @Jassy930 @stmoonar

---

## Download & Installation

Visit [Releases](https://github.com/farion1231/cc-switch/releases/latest) to download the appropriate version.

### System Requirements

| System  | Minimum Version                 | Architecture                        |
| ------- | ------------------------------- | ----------------------------------- |
| Windows | Windows 10 or later             | x64                                 |
| macOS   | macOS 10.15 (Catalina) or later | Intel (x64) / Apple Silicon (arm64) |
| Linux   | See table below                 | x64                                 |

### Windows

| File                                     | Description                                          |
| ---------------------------------------- | ---------------------------------------------------- |
| `CC-Switch-v3.11.0-Windows.msi`          | **Recommended** - MSI installer with auto-update     |
| `CC-Switch-v3.11.0-Windows-Portable.zip` | Portable version, extract and run, no registry write |

### macOS

| File                             | Description                                                          |
| -------------------------------- | -------------------------------------------------------------------- |
| `CC-Switch-v3.11.0-macOS.zip`    | **Recommended** - Extract and drag to Applications, Universal Binary |
| `CC-Switch-v3.11.0-macOS.tar.gz` | For Homebrew installation and auto-update                            |

> **Note**: Since the author doesn't have an Apple Developer account, you may see an "unidentified developer" warning on first launch. Please close it, then go to "System Settings" → "Privacy & Security" → click "Open Anyway", and it will open normally afterwards.

### Homebrew (macOS)

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

Update:

```bash
brew upgrade --cask cc-switch
```

### Linux

| Distribution                            | Recommended Format | Installation Method                                                    |
| --------------------------------------- | ------------------ | ---------------------------------------------------------------------- |
| Ubuntu / Debian / Linux Mint / Pop!\_OS | `.deb`             | `sudo dpkg -i CC-Switch-*.deb` or `sudo apt install ./CC-Switch-*.deb` |
| Fedora / RHEL / CentOS / Rocky Linux    | `.rpm`             | `sudo rpm -i CC-Switch-*.rpm` or `sudo dnf install ./CC-Switch-*.rpm`  |
| openSUSE                                | `.rpm`             | `sudo zypper install ./CC-Switch-*.rpm`                                |
| Arch Linux / Manjaro                    | `.AppImage`        | Add execute permission and run directly, or use AUR                    |
| Other distributions / Unsure            | `.AppImage`        | `chmod +x CC-Switch-*.AppImage && ./CC-Switch-*.AppImage`              |
