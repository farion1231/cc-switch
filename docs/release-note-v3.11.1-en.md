# CC Switch v3.11.1

> Revert Partial Key-Field Merging, Restore Common Config Snippet & Bug Fixes

**[中文版 →](release-note-v3.11.1-zh.md) | [日本語版 →](release-note-v3.11.1-ja.md)**

---

## Overview

CC Switch v3.11.1 is a hotfix release that reverts the **Partial Key-Field Merging** architecture introduced in v3.11.0, restoring the proven "**full config overwrite + Common Config Snippet**" mechanism. It also includes several UI and platform compatibility fixes.

**Release Date**: 2026-02-28

**Update Scale**: 8 commits | 52 files changed | +3,948 / -1,411 lines

---

## Highlights

- **Restore Full Config Overwrite + Common Config Snippet**: Reverted partial key-field merging due to critical data loss issues; restores full config snapshot write and Common Config Snippet UI
- **Proxy Panel Improvements**: Proxy toggle moved into panel body for better discoverability of takeover options
- **Theme & Compact Mode Fixes**: "Follow System" theme now auto-updates; compact mode exit works correctly
- **Windows Compatibility**: Disabled env check and one-click install to prevent protocol handler side effects

---

## Reverted

### Restore Full Config Overwrite + Common Config Snippet

Reverted the partial key-field merging refactoring introduced in v3.11.0 (revert 992dda5c).

**Why reverted**: The partial key-field merging approach had three critical issues:
1. **Data loss on switch**: Non-whitelisted custom fields were silently dropped during provider switching
2. **Permanent backfill stripping**: Backfill permanently removed non-key fields from the database, causing irreversible data loss
3. **Maintenance burden**: The whitelist of "key fields" required constant maintenance as new config keys were added

**What's restored**:
- Full config snapshot write on provider switch (predictable, complete overwrite)
- Common Config Snippet UI and backend commands
- 6 frontend components/hooks (3 components + 3 hooks)

**Migration**:
- If you upgraded to v3.11.0 and your providers lost custom fields, re-import your config or manually re-add the missing fields
- Common Config Snippet is available again — use it to define shared config that should persist across provider switches

---

## Changed

- **Proxy Panel Layout**: Moved proxy on/off toggle from accordion header into panel content area, placed directly above app takeover options. This ensures users see takeover configuration immediately after enabling the proxy, avoiding the common mistake of enabling the proxy without configuring takeover
- **Manual Import for OpenCode/OpenClaw**: Removed auto-import on startup; empty state now shows an "Import Current Config" button, consistent with Claude/Codex/Gemini behavior

---

## Fixed

- **"Follow System" Theme Not Auto-Updating**: Delegated to Tauri's native theme tracking (`set_window_theme(None)`) so the WebView's `prefers-color-scheme` media query stays in sync with OS theme changes
- **Compact Mode Cannot Exit**: Restored `flex-1` on `toolbarRef` so `useAutoCompact`'s exit condition triggers correctly based on available width instead of content width
- **Proxy Takeover Toast Shows {{app}}**: Added missing `app` interpolation parameter to i18next `t()` calls for proxy takeover enabled/disabled messages
- **Windows Protocol Handler Side Effects**: Disabled environment check and one-click install on Windows to prevent unintended protocol handler registration

---

## Notes & Considerations

- **Common Config Snippet is back**: If you relied on this feature in v3.10.x and earlier, it works the same way again. Define shared config that should persist across all provider switches.
- **v3.11.0 Partial Key-Field Merging users**: If you noticed missing config fields after switching providers in v3.11.0, re-import your config to restore them.

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
| `CC-Switch-v3.11.1-Windows.msi`          | **Recommended** - MSI installer with auto-update     |
| `CC-Switch-v3.11.1-Windows-Portable.zip` | Portable version, extract and run, no registry write |

### macOS

| File                             | Description                                                          |
| -------------------------------- | -------------------------------------------------------------------- |
| `CC-Switch-v3.11.1-macOS.zip`    | **Recommended** - Extract and drag to Applications, Universal Binary |
| `CC-Switch-v3.11.1-macOS.tar.gz` | For Homebrew installation and auto-update                            |

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
