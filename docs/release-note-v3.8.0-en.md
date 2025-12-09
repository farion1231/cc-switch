# CC Switch v3.8.0

> Persistence Architecture Upgrade, Laying the Foundation for Cloud Sync

**[中文版 →](release-note-v3.8.0-zh.md) | [日本語版 →](release-note-v3.8.0-ja.md)**

---

## Overview

CC Switch v3.8.0 is a major architectural upgrade that restructures the data persistence layer and user interface, laying the foundation for future cloud sync and local proxy features.

**Release Date**: 2025-11-28
**Commits**: 51 commits since v3.7.1
**Code Changes**: 207 files, +17,297 / -6,870 lines

---

## Major Updates

### Persistence Architecture Upgrade

Migrated from single JSON file storage to SQLite + JSON dual-layer architecture for hierarchical data management.

**Architecture Changes**:

```
v3.7.x (Old)                     v3.8.0 (New)
┌─────────────────┐              ┌─────────────────────────────────┐
│  config.json    │              │  SQLite (Syncable Data)         │
│  ┌───────────┐  │              │  ├─ providers     Provider cfg  │
│  │ providers │  │              │  ├─ mcp_servers   MCP servers   │
│  │ mcp       │  │     ──>      │  ├─ prompts       Prompts       │
│  │ prompts   │  │              │  ├─ skills        Skills        │
│  │ settings  │  │              │  └─ settings      General cfg   │
│  └───────────┘  │              ├─────────────────────────────────┤
└─────────────────┘              │  JSON (Device-level Data)       │
                                 │  └─ settings.json Local settings│
                                 │     ├─ Window position          │
                                 │     ├─ Path overrides           │
                                 │     └─ Current provider ID      │
                                 └─────────────────────────────────┘
```

**Dual-layer Structure Design**:

| Layer      | Storage | Data Types                      | Sync Strategy   |
| ---------- | ------- | ------------------------------- | --------------- |
| Cloud Sync | SQLite  | Providers, MCP, Prompts, Skills | Future syncable |
| Device     | JSON    | Window state, local paths       | Stays local     |

**Technical Implementation**:

- **Schema Version Management** - Supports database structure upgrade migrations
- **SQL Import/Export** - `backup.rs` supports SQL dump for cloud storage
- **Transaction Support** - SQLite native transactions ensure data consistency
- **Auto Migration** - Automatically migrates from `config.json` on first launch

**Modular Refactoring**:

```
database/
├── mod.rs              Core Database struct and initialization
├── schema.rs           Table definitions, schema version migrations
├── backup.rs           SQL import/export, binary snapshot backup
├── migration.rs        JSON → SQLite data migration engine
└── dao/                Data Access Object layer
    ├── providers.rs    Provider CRUD
    ├── mcp.rs          MCP server CRUD
    ├── prompts.rs      Prompts CRUD
    ├── skills.rs       Skills CRUD
    └── settings.rs     Key-value settings storage
```

---

### Brand New User Interface

Completely redesigned UI providing a more modern visual experience.

**Visual Improvements**:

- Redesigned interface layout
- Unified component styles
- Smoother transition animations
- Optimized visual hierarchy

**Interaction Enhancements**:

- Redesigned header toolbar
- Unified ConfirmDialog styling
- Disabled overscroll bounce effect on main view
- Improved form validation feedback

**Compatibility Adjustments**:

- Downgraded Tailwind CSS from v4 to v3.4 for better browser compatibility

---

### Japanese Language Support

Added Japanese interface support, expanding internationalization to three languages.

**Supported Languages**:

- Simplified Chinese
- English
- Japanese (New)

---

## New Features

### Skills Recursive Scanning

Skills management system now supports recursive scanning of repository directories, automatically discovering nested skill files.

**Improvements**:

- Support for multi-level directory structures
- Automatic discovery of all `SKILL.md` files
- Allow same-named skills from different repositories (using full path for deduplication)

---

### Provider Icon Configuration

Provider presets now support custom icon configuration.

**Features**:

- Preset providers include default icons
- Icon settings preserved when duplicating providers
- Custom icon colors

---

### Enhanced Form Validation

Provider forms now include required field validation with friendlier error messages.

**Improvements**:

- Real-time validation for required fields
- Unified Toast notifications for validation errors
- Clearer error messages

---

### Auto Launch on Startup

Added auto-launch functionality supporting Windows, macOS, and Linux platforms.

**Features**:

- One-click enable/disable in settings
- Implemented using platform-native APIs
- Windows uses Registry, macOS uses LaunchAgent, Linux uses XDG autostart

---

### New Provider Presets

- **MiniMax** - Official partner

---

## Bug Fixes

### Critical Fixes

**Custom Endpoints Lost Issue**

Fixed an issue where custom request URLs were unexpectedly lost when updating providers.

- Root Cause: `INSERT OR REPLACE` executes `DELETE + INSERT` under the hood in SQLite, triggering foreign key cascade deletion
- Fix: Changed to use `UPDATE` statement for existing providers

**Gemini Configuration Issues**

- Fixed custom provider environment variables not correctly written to `.env` file
- Fixed security auth config incorrectly written to other config files

**Provider Validation Issues**

- Fixed validation error when current provider ID doesn't exist
- Fixed icon fields lost when duplicating providers

### Platform Compatibility

**Linux**

- Resolved WebKitGTK DMA-BUF rendering issue
- Preserve user `.desktop` file customizations

### Other Fixes

- Fixed redundant usage queries when switching apps
- Fixed DMXAPI preset using wrong auth token field
- Fixed missing translation keys in deeplink components
- Fixed usage script template initialization logic

---

## Technical Improvements

### Architecture Refactoring

**Provider Service Modularization**:

```
services/provider/
├── mod.rs          Core service - add/update/delete/switch/validate
├── live.rs         Live config file operations
├── gemini_auth.rs  Gemini auth type detection
├── endpoints.rs    Custom endpoint management
└── usage.rs        Usage script execution
```

**Deeplink Modularization**:

```
deeplink/
├── mod.rs       Module exports
├── parser.rs    URL parsing
├── provider.rs  Provider import logic
├── mcp.rs       MCP import logic
├── prompt.rs    Prompt import
├── skill.rs     Skills import
└── utils.rs     Utility functions
```

### Code Quality

**Cleanup**:

- Removed legacy JSON-era import/export dead code
- Removed unused MCP type exports
- Unified error handling approach

**Test Updates**:

- Migrated tests to SQLite database architecture
- Updated component tests to match current implementation
- Fixed MSW handlers to adapt to new API

---

## Technical Statistics

```
Overall Changes:
- Commits: 51
- Files: 207 files changed
- Additions: +17,297 lines
- Deletions: -6,870 lines
- Net: +10,427 lines

Commit Type Distribution:
- fix: 25 (Bug fixes)
- refactor: 11 (Code refactoring)
- feat: 9 (New features)
- test: 1 (Testing)
- other: 5

Change Area Distribution:
- Frontend source: 112 files
- Rust backend: 63 files
- Test files: 20 files
- i18n files: 3 files
```

---

## Migration Guide

### Upgrading from v3.7.x

**Auto Migration** - Executes automatically on first launch:

1. Detects if `config.json` exists
2. Migrates all data to SQLite within a transaction
3. Migrates device-level settings to `settings.json`
4. Shows migration success notification

**Data Safety**:

- Original `config.json` file is preserved (not deleted)
- Error dialog displayed on migration failure, `config.json` preserved
- Supports Dry-run mode to verify migration logic

---

## Download & Installation

### System Requirements

- **Windows**: Windows 10+
- **macOS**: macOS 10.15 (Catalina)+
- **Linux**: Ubuntu 22.04+ / Debian 11+ / Fedora 34+

### Download Links

Visit [Releases](https://github.com/farion1231/cc-switch/releases/latest) to download:

- **Windows**: `CC-Switch-v3.8.0-Windows.msi` or `-Portable.zip`
- **macOS**: `CC-Switch-v3.8.0-macOS.tar.gz` or `.zip`
- **Linux**: `CC-Switch-v3.8.0-Linux.AppImage` or `.deb`

### Homebrew (macOS)

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

Update:

```bash
brew upgrade --cask cc-switch
```

## Acknowledgments

### Contributors

Thanks to all contributors who made this release possible:

- [@YoVinchen](https://github.com/YoVinchen) - UI and database refactoring
- [@farion1231](https://github.com/farion1231) - Bug fixes and feature enhancements
- Community members for testing and feedback

### Sponsors

**Zhipu AI** - GLM CODING PLAN Sponsor
[Get 10% off with this link](https://z.ai/subscribe?ic=8JVLJQFSKB)

**PackyCode** - API Relay Service Partner
[Use code "cc-switch" for 10% off registration](https://www.packyapi.com/register?aff=cc-switch)

**ShandianShuo** - Local-first AI Voice Input
[Free download](https://shandianshuo.cn) for Mac/Windows

**MiniMax** - MiniMax M2 CODING PLAN Sponsor
[Black Friday sale, plans starting at $2](https://platform.minimax.io/subscribe/coding-plan)

---

## Feedback & Support

- **Issue Reports**: [GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- **Discussions**: [GitHub Discussions](https://github.com/farion1231/cc-switch/discussions)
- **Documentation**: [README](../README.md)
- **Changelog**: [CHANGELOG.md](../CHANGELOG.md)

---

## Future Roadmap

**v3.9.0 Preview** (Tentative):

- Local proxy feature

Stay tuned for more updates!

---

**Happy Coding!**
