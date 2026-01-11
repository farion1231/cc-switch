# CC Switch v3.9.0

> Local API Proxy, Auto Failover, Universal Provider, and a more complete multi-app workflow

**[中文版 →](release-note-v3.9.0-zh.md) | [日本語版 →](release-note-v3.9.0-ja.md)**

---

## Overview

CC Switch v3.9.0 is the stable release of the v3.9 beta series (`3.9.0-1`, `3.9.0-2`, `3.9.0-3`).
It introduces a local API proxy with per-app takeover, automatic failover, universal providers, and many stability and UX improvements across Claude Code, Codex, and Gemini CLI.

**Release Date**: 2026-01-07

---

## Highlights

- Local API Proxy for Claude Code / Codex / Gemini CLI
- Auto Failover with circuit breaker and per-app failover queues
- Universal Provider: one shared config synced across apps (ideal for API gateways like NewAPI)
- Skills improvements: multi-app support, unified management with SSOT + React Query
- Common config snippets: extract reusable snippets from the editor or the current provider
- MCP import: import MCP servers from installed apps
- Usage improvements: auto-refresh, cache hit/creation metrics, and timezone fixes
- Linux packaging: RPM and Flatpak artifacts

---

## Major Features

### Local API Proxy

- Runs a local high-performance HTTP proxy server (Axum-based)
- Supports Claude Code, Codex, and Gemini CLI with a unified proxy
- Per-app takeover: you can independently decide which app routes through the proxy
- Live config takeover: backs up and redirects the CLI live config to the local proxy when takeover is enabled
- Monitoring: request logging and usage statistics for easier debugging and cost tracking
- Error request logging: keep detailed logs for failed proxy requests to simplify debugging (#401, thanks @yovinchen)

### Auto Failover (Circuit Breaker)

- Automatically detects provider failures and triggers protection (circuit breaker)
- Automatically switches to a backup provider when the current one is unhealthy
- Tracks provider health in real time, and keeps independent failover queues per app
- When failover is disabled, timeout/retry related settings no longer affect normal request flow

### Skills Management

- Multi-app Skills support for Claude Code and Codex, with smoother migration from older skill layouts (#365, #378, thanks @yovinchen)
- Unified Skills management architecture (SSOT + React Query) for more consistent state and refresh behavior
- Better discovery UX and performance:
  - Skip hidden directories during discovery
  - Faster discovery with long-lived caching for discoverable skills
  - Clear loading indicators and more discoverable header actions (import/refresh)
  - Fix wrong skill repo branch (#505, thanks @kjasn)

### Universal Provider

- Add a shared provider configuration that can sync to Claude/Codex/Gemini (#348, thanks @Calcium-Ion)
- Designed for API gateways that support multiple protocols (e.g., NewAPI)
- Allows per-app default model mapping under a single provider

### Common Config Snippets (Claude/Codex/Gemini)

- Maintain a reusable "common config" snippet and merge/append it into providers that enable it
- New extraction workflow:
  - Extract from the editor content (what you are currently editing)
  - Or extract from the current active provider when the editor content is not provided
- Codex extraction is safer:
  - Removes provider-specific sections like `model_provider`, `model`, and the entire `model_providers` table
  - Preserves `base_url` under `[mcp_servers.*]` so MCP configs are not accidentally broken

### MCP Management

- Import MCP servers from installed apps
- Improve robustness: skip sync when the target CLI app is not installed; handle invalid Codex `config.toml` gracefully (#461, thanks @majiayu000)
- Windows compatibility: wrap npx/npm commands with `cmd /c` for MCP export

### Usage & Pricing

- Usage & pricing improvements: auto-refresh, cache hit/creation metrics, timezone handling fixes, and refreshed built-in pricing table (#508, thanks @yovinchen)
- DeepLink support: import usage query configuration via deeplink (#400, thanks @qyinter)
- Model extraction for usage statistics (#455, thanks @yovinchen)
- Usage query credentials can fall back to provider config (#360, thanks @Sirhexs)

---

## UX Improvements

- Provider search filter: quickly find providers by name (#435, thanks @TinsFox)
- Provider icon colors: customize provider icon colors for quicker visual identification (#385, thanks @yovinchen)
- Keyboard shortcut: `Cmd/Ctrl + ,` opens Settings (#436, thanks @TinsFox)
- Skip Claude Code first-run confirmation dialog (optional)
- Closable toasts: close buttons for switch toast and all success toasts (#350, thanks @ForteScarlet)
- Update badge navigation: clicking the update badge opens the About tab
- Settings page tab style improvements (#342, thanks @wenyuanw)
- Smoother transitions: fade transitions for app/view switching and exit animations for panels
- Proxy takeover active theme: apply an emerald theme while takeover is active
- Dark mode readability improvements for forms and labels
- Better window dragging area for full-screen panels (#525, thanks @zerob13)

---

## Platform Notes

### Windows

- Prevent terminal windows from appearing during version checks
- Improve window sizing defaults (minimum width/height)
- Fix black screen on startup by using the system titlebar
- Add a fallback for `crypto.randomUUID()` on older WebViews

### macOS

- Use `.app` bundle path for autostart to avoid terminal window popups (#462, thanks @majiayu000)
- Improve tray/icon behavior and header alignment

---

## Packaging

- Linux: RPM and Flatpak packaging targets are now available for building release artifacts

---

## Notes

- Security improvements for the JavaScript executor and usage script execution (#151, thanks @luojiyin1987).
- SQL import is restricted to CC Switch exported backups to reduce the risk of importing unsafe or incompatible SQL dumps.
- Proxy takeover modifies CLI live configs; CC Switch will back up the live config before redirecting it to the local proxy. If you want to revert, disable takeover/stop the proxy and restore from the backup when needed.

## Special Thanks

Special thanks to @xunyu @deijing @su-fen for their support and contributions. This release wouldn't be possible without you!

## Download & Installation

Visit [Releases](https://github.com/farion1231/cc-switch/releases/latest) to download the appropriate version.

### System Requirements

| System  | Minimum Version                 | Architecture                        |
| ------- | ------------------------------- | ----------------------------------- |
| Windows | Windows 10 or later             | x64                                 |
| macOS   | macOS 10.15 (Catalina) or later | Intel (x64) / Apple Silicon (arm64) |
| Linux   | See table below                 | x64                                 |

### Windows

| File                                    | Description                                        |
| --------------------------------------- | -------------------------------------------------- |
| `CC-Switch-v3.9.0-Windows.msi`          | **Recommended** - MSI installer with auto-update support |
| `CC-Switch-v3.9.0-Windows-Portable.zip` | Portable version, no installation required         |

### macOS

| File                            | Description                                                       |
| ------------------------------- | ----------------------------------------------------------------- |
| `CC-Switch-v3.9.0-macOS.zip`    | **Recommended** - Extract and drag to Applications, Universal Binary |
| `CC-Switch-v3.9.0-macOS.tar.gz` | For Homebrew installation and auto-update                         |

> **Note**: Since the author does not have an Apple Developer account, you may see an "unidentified developer" warning on first launch. Close the app, then go to "System Settings" → "Privacy & Security" → click "Open Anyway", and it will open normally afterwards.

### Homebrew (MacOS)

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

Update:

```bash
brew upgrade --cask cc-switch
```

### Linux

| Distribution                            | Recommended Format | Installation                                                           |
| --------------------------------------- | ------------------ | ---------------------------------------------------------------------- |
| Ubuntu / Debian / Linux Mint / Pop!\_OS | `.deb`             | `sudo dpkg -i CC-Switch-*.deb` or `sudo apt install ./CC-Switch-*.deb` |
| Fedora / RHEL / CentOS / Rocky Linux    | `.rpm`             | `sudo rpm -i CC-Switch-*.rpm` or `sudo dnf install ./CC-Switch-*.rpm`  |
| openSUSE                                | `.rpm`             | `sudo zypper install ./CC-Switch-*.rpm`                                |
| Arch Linux / Manjaro                    | `.AppImage`        | Make executable and run directly, or use AUR                           |
| Other distros / Unsure                  | `.AppImage`        | `chmod +x CC-Switch-*.AppImage && ./CC-Switch-*.AppImage`              |
| Sandboxed installation                  | `.flatpak`         | `flatpak install CC-Switch-*.flatpak`                                  |
