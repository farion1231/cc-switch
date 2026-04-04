<div align="center">

# CC Switch Legacy

### Compatibility Fork for macOS 10.15 Catalina

[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#compatibility)
[![macOS](https://img.shields.io/badge/macOS-10.15%2B-blue.svg)](#compatibility)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

English | [Compatibility Notes (ZH)](docs/macos-10.15-compat.md) | [Upstream Project](https://github.com/farion1231/cc-switch) | [Changelog](CHANGELOG.md)

</div>

## Overview

This repository is a compatibility-focused fork of [CC Switch](https://github.com/farion1231/cc-switch).

The goal of this fork is simple: keep the original CC Switch experience, while making the desktop app buildable and usable on **macOS 10.15 Catalina**, which is not supported by the upstream project anymore.

If you are on macOS 12+ and do not specifically need Catalina support, the upstream project is still the better default choice.

## What Changed In This Fork

Compared with upstream CC Switch, this fork adds or adjusts the following areas for older macOS compatibility:

- Lowered the macOS deployment target from `12.0` to `10.15`
- Updated Tauri bundle config to allow `macOS 10.15+`
- Added an `objc2` dev-mode workaround for older WebKit protocol methods
- Pinned `esbuild` to `0.21.5` to avoid newer macOS-only symbols
- Lowered frontend build targets for older Safari / WKWebView behavior
- Replaced `smol-toml` with `@iarna/toml` to avoid `BigInt` syntax issues on Safari 13
- Added `MediaQueryList.addListener` fallback for Safari 13 theme change handling

Detailed adaptation notes are documented in [docs/macos-10.15-compat.md](docs/macos-10.15-compat.md).

## Features

This fork keeps the main CC Switch feature set:

- Manage **Claude Code**, **Codex**, **Gemini CLI**, **OpenCode**, and **OpenClaw** in one desktop app
- Import and switch providers without manually editing JSON, TOML, or `.env` files
- Manage **MCP**, **Prompts**, and **Skills** from a unified interface
- Use **system tray quick switching** for fast provider changes
- Track usage and cost data with built-in statistics views
- Sync data through custom config directories or WebDAV
- Browse and restore session history across supported apps

## Screenshots

| Main Interface | Add Provider |
| :---: | :---: |
| ![Main Interface](assets/screenshots/main-en.png) | ![Add Provider](assets/screenshots/add-en.png) |

## Compatibility

### Primary Target

- macOS 10.15 Catalina

### Also Expected To Work

- macOS 11+
- Windows
- Linux

This fork is primarily maintained for Catalina compatibility. Other platforms should remain close to upstream behavior, but Catalina support is the reason this repository exists.

## Quick Start

### Use The App

1. Add a provider from the main UI
2. Enable the provider you want to use
3. Restart the related CLI tool or terminal if required
4. Use the tray menu for quick switching when needed

### Documentation

- [Compatibility Notes for macOS 10.15](docs/macos-10.15-compat.md)
- [User Manual (English)](docs/user-manual/en/README.md)
- [User Manual (Chinese)](docs/user-manual/zh/README.md)
- [User Manual (Japanese)](docs/user-manual/ja/README.md)

## Build From Source

### Requirements

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### Commands

```bash
pnpm install
pnpm dev
pnpm typecheck
pnpm test:unit
pnpm build
```

### Rust Backend

```bash
cd src-tauri
cargo fmt
cargo clippy
cargo test
```

## macOS 10.15 Notes

This fork already includes the key Catalina-related project changes:

- `.cargo/config.toml` sets `MACOSX_DEPLOYMENT_TARGET=10.15`
- `src-tauri/tauri.conf.json` sets `minimumSystemVersion` to `10.15`
- `vite.config.ts` targets older Safari-compatible output
- `package.json` pins `esbuild` to a Catalina-safe version

If you are troubleshooting Catalina-specific issues, start with [docs/macos-10.15-compat.md](docs/macos-10.15-compat.md).

## Project Structure

```text
src/              Frontend (React + TypeScript)
src-tauri/        Backend (Tauri + Rust)
assets/           Screenshots and static assets
docs/             Compatibility notes, manuals, and release notes
tests/            Frontend tests
```

## Acknowledgements

- Original project: [farion1231/cc-switch](https://github.com/farion1231/cc-switch)
- This repository is a compatibility fork, not the official upstream release channel

## License

This project continues to use the [MIT License](LICENSE).
