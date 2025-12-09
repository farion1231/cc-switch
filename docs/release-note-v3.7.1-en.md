# CC Switch v3.7.1

> Stability Enhancements and User Experience Improvements

**[中文更新说明 Chinese Documentation →](release-note-v3.7.1-zh.md)**

---

## v3.7.1 Updates

**Release Date**: 2025-11-22
**Code Changes**: 17 files, +524 / -81 lines

### Bug Fixes

- **Fix Third-Party Skills Installation Failure** (#268)
  Fixed installation issues for skills repositories with custom subdirectories, now supports repos like `ComposioHQ/awesome-claude-skills` with subdirectories

- **Fix Gemini Configuration Persistence Issue**
  Resolved the issue where settings.json edits in Gemini form were lost when switching providers

- **Prevent Dialogs from Closing on Overlay Click**
  Added protection against clicking overlay/backdrop, preventing accidental form data loss across all 11 dialog components

### New Features

- **Gemini Configuration Directory Support** (#255)
  Added Gemini configuration directory option in settings, supports customizing `~/.gemini/` path

- **ArchLinux Installation Support** (#259)
  Added AUR installation method: `paru -S cc-switch-bin`

### Improvements

- **Skills Error Message i18n Enhancement**
  Added 28+ detailed error messages (English & Chinese) with specific resolution suggestions, extended download timeout from 15s to 60s

- **Code Formatting**
  Applied unified Rust and TypeScript code formatting standards

### Download

Visit [Releases](https://github.com/farion1231/cc-switch/releases/latest) to download the latest version

---

## v3.7.0 Complete Release Notes

> From Provider Switcher to All-in-One AI CLI Management Platform

**Release Date**: 2025-11-19
**Commits**: 85 from v3.6.0
**Code Changes**: 152 files, +18,104 / -3,732 lines

---

## New Features

### Gemini CLI Integration

Complete support for Google Gemini CLI, becoming the third supported application (Claude Code, Codex, Gemini).

**Core Capabilities**:

- **Dual-file configuration** - Support for both `.env` and `settings.json` formats
- **Auto-detection** - Automatically detect `GOOGLE_GEMINI_BASE_URL`, `GEMINI_MODEL`, etc.
- **Full MCP support** - Complete MCP server management for Gemini
- **Deep link integration** - Import via `ccswitch://` protocol
- **System tray** - Quick-switch from tray menu

**Provider Presets**:

- **Google Official** - OAuth authentication support
- **PackyCode** - Partner integration
- **Custom** - Full customization support

**Technical Implementation**:

- New backend modules: `gemini_config.rs` (20KB), `gemini_mcp.rs`
- Form synchronization with environment editor
- Dual-file atomic writes

---

### MCP v3.7.0 Unified Architecture

Complete refactoring of MCP management system for cross-application unification.

**Architecture Improvements**:

- **Unified panel** - Single interface for Claude/Codex/Gemini MCP servers
- **SSE transport** - New Server-Sent Events support
- **Smart parser** - Fault-tolerant JSON parsing
- **Format correction** - Auto-fix Codex `[mcp_servers]` format
- **Extended fields** - Preserve custom TOML fields

**User Experience**:

- Default app selection in forms
- JSON formatter for validation
- Improved visual hierarchy
- Better error messages

**Import/Export**:

- Unified import from all three apps
- Bidirectional synchronization
- State preservation

---

### Claude Skills Management System

**Approximately 2,000 lines of code** - A complete skill ecosystem platform.

**GitHub Integration**:

- Auto-scan skills from GitHub repositories
- Pre-configured repos:
  - `ComposioHQ/awesome-claude-skills` - Curated collection
  - `anthropics/skills` - Official Anthropic skills
  - `cexll/myclaude` - Community contributions
- Add custom repositories
- Subdirectory scanning support (`skillsPath`)

**Lifecycle Management**:

- **Discover** - Auto-detect `SKILL.md` files
- **Install** - One-click to `~/.claude/skills/`
- **Uninstall** - Safe removal with tracking
- **Update** - Check for updates (infrastructure ready)

**Technical Architecture**:

- **Backend**: `SkillService` (526 lines) with GitHub API integration
- **Frontend**: SkillsPage, SkillCard, RepoManager
- **UI Components**: Badge, Card, Table (shadcn/ui)
- **State**: Persistent storage in `config.json`
- **i18n**: 47+ translation keys

---

### Prompts Management System

**Approximately 1,300 lines of code** - Complete system prompt management.

**Multi-Preset Management**:

- Create unlimited prompt presets
- Quick switch between presets
- One active prompt at a time
- Delete protection for active prompts

**Cross-App Support**:

- **Claude**: `~/.claude/CLAUDE.md`
- **Codex**: `~/.codex/AGENTS.md`
- **Gemini**: `~/.gemini/GEMINI.md`

**Markdown Editor**:

- Full-featured CodeMirror 6 integration
- Syntax highlighting
- Dark theme (One Dark)
- Real-time preview

**Smart Synchronization**:

- **Auto-write** - Immediately write to live files
- **Backfill protection** - Save current content before switching
- **Auto-import** - Import from live files on first launch
- **Modification protection** - Preserve manual modifications

**Technical Implementation**:

- **Backend**: `PromptService` (213 lines)
- **Frontend**: PromptPanel (177), PromptFormModal (160), MarkdownEditor (159)
- **Hooks**: usePromptActions (152 lines)
- **i18n**: 41+ translation keys

---

### Deep Link Protocol (ccswitch://)

One-click provider configuration import via URL scheme.

**Features**:

- Protocol registration on all platforms
- Import from shared links
- Lifecycle integration
- Security validation

---

### Environment Variable Conflict Detection

Intelligent detection and management of configuration conflicts.

**Detection Scope**:

- **Claude & Codex** - Cross-app conflicts
- **Gemini** - Auto-discovery
- **MCP** - Server configuration conflicts

**Management Features**:

- Visual conflict indicators
- Resolution suggestions
- Override warnings
- Backup before changes

---

## Improvements

### Provider Management

**New Presets**:

- **DouBaoSeed** - ByteDance's DouBao
- **Kimi For Coding** - Moonshot AI
- **BaiLing** - BaiLing AI
- **Removed AnyRouter** - To avoid confusion

**Enhancements**:

- Model name configuration for Codex and Gemini
- Provider notes field for organization
- Enhanced preset metadata

### Configuration Management

- **Common config migration** - From localStorage to `config.json`
- **Unified persistence** - Shared across all apps
- **Auto-import** - First launch configuration import
- **Backfill priority** - Correct handling of live files

### UI/UX Improvements

**Design System**:

- **macOS native** - System-aligned color scheme
- **Window centering** - Default centered position
- **Visual polish** - Improved spacing and hierarchy

**Interactions**:

- **Password input** - Fixed Edge/IE reveal buttons
- **URL overflow** - Fixed card overflow
- **Error copying** - Copy-to-clipboard errors
- **Tray sync** - Real-time drag-and-drop sync

---

## Bug Fixes

### Critical Fixes

- **Usage script validation** - Boundary checks
- **Gemini validation** - Relaxed constraints
- **TOML parsing** - CJK quote handling
- **MCP fields** - Custom field preservation
- **White screen** - FormLabel crash fix

### Stability

- **Tray safety** - Pattern matching instead of unwrap
- **Error isolation** - Tray failures don't block operations
- **Import classification** - Correct category assignment

### UI Fixes

- **Model placeholders** - Removed misleading hints
- **Base URL** - Auto-fill for third-party providers
- **Drag sort** - Tray menu synchronization

---

## Technical Improvements

### Architecture

**MCP v3.7.0**:

- Removed legacy code (~1,000 lines)
- Unified initialization structure
- Backward compatibility maintained
- Comprehensive code formatting

**Platform Compatibility**:

- Windows winreg API fix (v0.52)
- Safe pattern matching (no `unwrap()`)
- Cross-platform tray handling

### Configuration

**Synchronization**:

- MCP sync across all apps
- Gemini form-editor sync
- Dual-file reading (.env + settings.json)

**Validation**:

- Input boundary checks
- TOML quote normalization (CJK)
- Custom field preservation
- Enhanced error messages

### Code Quality

**Type Safety**:

- Complete TypeScript coverage
- Rust type refinements
- API contract validation

**Testing**:

- Simplified assertions
- Better test coverage
- Integration test updates

**Dependencies**:

- Tauri 2.8.x
- Rust: `anyhow`, `zip`, `serde_yaml`, `tempfile`
- Frontend: CodeMirror 6 packages
- winreg 0.52 (Windows)

---

## Technical Statistics

```
Total Changes:
- Commits: 85
- Files: 152 changed
- Additions: +18,104 lines
- Deletions: -3,732 lines

New Modules:
- Skills Management: 2,034 lines (21 files)
- Prompts Management: 1,302 lines (20 files)
- Gemini Integration: ~1,000 lines
- MCP Refactor: ~3,000 lines refactored

Code Distribution:
- Backend (Rust): ~4,500 lines new
- Frontend (React): ~3,000 lines new
- Configuration: ~1,500 lines refactored
- Tests: ~500 lines
```

---

## Strategic Positioning

### From Tool to Platform

v3.7.0 represents a shift in CC Switch's positioning:

| Aspect            | v3.6                     | v3.7.0                       |
| ----------------- | ------------------------ | ---------------------------- |
| **Identity**      | Provider Switcher        | AI CLI Management Platform   |
| **Scope**         | Configuration Management | Ecosystem Management         |
| **Applications**  | Claude + Codex           | Claude + Codex + Gemini      |
| **Capabilities**  | Switch configs           | Extend capabilities (Skills) |
| **Customization** | Manual editing           | Visual management (Prompts)  |
| **Integration**   | Isolated apps            | Unified management (MCP)     |

### Six Pillars of AI CLI Management

1. **Configuration Management** - Provider switching and management
2. **Capability Extension** - Skills installation and lifecycle
3. **Behavior Customization** - System prompt presets
4. **Ecosystem Integration** - Deep links and sharing
5. **Multi-AI Support** - Claude/Codex/Gemini
6. **Intelligent Detection** - Conflict prevention

---

## Download & Installation

### System Requirements

- **Windows**: Windows 10+
- **macOS**: macOS 10.15 (Catalina)+
- **Linux**: Ubuntu 22.04+ / Debian 11+ / Fedora 34+ / ArchLinux

### Download Links

Visit [Releases](https://github.com/farion1231/cc-switch/releases/latest) to download:

- **Windows**: `CC-Switch-Windows.msi` or `-Portable.zip`
- **macOS**: `CC-Switch-macOS.tar.gz` or `.zip`
- **Linux**: `CC-Switch-Linux.AppImage` or `.deb`
- **ArchLinux**: `paru -S cc-switch-bin`

### Homebrew (macOS)

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

Update:

```bash
brew upgrade --cask cc-switch
```

---

## Migration Notes

### From v3.6.x

**Automatic migration** - No action required, configs are fully compatible

### From v3.1.x or Earlier

**Two-step migration required**:

1. First upgrade to v3.2.x (performs one-time migration)
2. Then upgrade to v3.7.0

### New Features

- **Skills**: No migration needed, start fresh
- **Prompts**: Auto-import from live files on first launch
- **Gemini**: Install Gemini CLI separately if needed
- **MCP v3.7.0**: Backward compatible with previous configs

---

## Acknowledgments

### Contributors

Thanks to all contributors who made this release possible:

- [@YoVinchen](https://github.com/YoVinchen) - Skills & Prompts & Gemini integration implementation
- [@farion1231](https://github.com/farion1231) - From developer to issue responder
- Community members for testing and feedback

### Sponsors

**Z.ai** - GLM CODING PLAN sponsor
[Get 10% OFF with this link](https://z.ai/subscribe?ic=8JVLJQFSKB)

**PackyCode** - API relay service partner
[Register with "cc-switch" code for 10% discount](https://www.packyapi.com/register?aff=cc-switch)

**ShanDianShuo** - Local-first AI voice input
[Free download](https://shandianshuo.cn) for Mac/Win

---

## Feedback & Support

- **Issues**: [GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- **Discussions**: [GitHub Discussions](https://github.com/farion1231/cc-switch/discussions)
- **Documentation**: [README](../README.md)
- **Changelog**: [CHANGELOG.md](../CHANGELOG.md)

---

## What's Next

**v3.8.0 Preview** (Tentative):

- Local proxy functionality

Stay tuned for more updates!

---

**Happy Coding!**
