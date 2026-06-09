<div align="center">

# CC Switch · Multi-Account Usage Monitor Edition

### A personal enhanced fork of [farion1231/cc-switch](https://github.com/farion1231/cc-switch), focused on multi-account management, usage visibility, and Codex workflows

[![Version](https://img.shields.io/badge/version-3.16.2-blue.svg)](https://github.com/ajia1206/cc-switch/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey.svg)](https://github.com/ajia1206/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[中文](README.md) | English | [Upstream](https://github.com/farion1231/cc-switch)

</div>

---

## 📌 About This Fork

This is a personal enhanced version of [farion1231/cc-switch](https://github.com/farion1231/cc-switch). The upstream project is an excellent account-switching manager for Claude Code / Codex / Gemini CLI / OpenCode / OpenClaw. This fork **focuses on solving usage-visibility and Codex workflow pain points for users with multiple accounts**.

If you manage multiple Codex official accounts, frequently hit the 5-hour or 7-day rate limits, need to know which account still has capacity at a glance, or want CLI session usage, cache hits, and real token consumption in one dashboard, this fork is for you.

---

## ✨ Key Enhancements Over Upstream

### Feature Overview

| Scenario | Upstream | This Fork | Where to find it |
|----------|:--------:|:---------:|------------------|
| Codex official account snapshots | ✅ | ✅ enhanced | Codex tab |
| Concurrent multi-account quota queries | ❌ | ✅ | Codex official snapshots |
| Per-account 5h / 7d quota and reset time | ❌ | ✅ | Codex account cards |
| Global Codex quota summary | ❌ | ✅ | Home / Codex quota panel |
| Yearly usage activity heatmap | ❌ | ✅ | Usage dashboard |
| Provider / model real-token stats | Basic stats | ✅ includes cache hits and cache creation | Usage dashboard |
| Notch usage overlay | ❌ | ✅ | Notch overlay window |
| Codex proxy takeover consistency | Basic behavior | ✅ enhanced | Background service |

### 1. Real-time Multi-Account Codex Usage

- **🔄 Concurrent all-account querying** — Walk through every Codex snapshot and query official usage without switching accounts first
- **📊 Per-card quota display** — Each account separately shows `5h` and `7d` remaining quota, reset time, and status color
- **⏱ Configurable refresh interval** — Configure refresh cadence in settings, or refresh all account snapshots manually from the account panel
- **💾 Query cache and event sync** — Backend quota cache updates are bridged into the frontend React Query cache after tray or background refreshes

### 2. Usage Dashboard Enhancements

- **📈 Real token semantics** — Provider / model `Tokens` use `fresh input + output + cache read + cache creation`, avoiding undercounts for cache-heavy usage
- **🟩 Yearly heatmap** — GitHub-style activity view for active days, session counts, and real token consumption across the past year
- **🧮 Multi-source deduplication** — Effective usage dedupes proxy logs and session logs for Codex / Gemini / Claude / OpenCode
- **🗄 Historical rollups** — Older detail logs roll up into daily aggregates to keep the database lean while preserving trends and stats

### 3. Codex Workflow Polish

- **🔐 Official account snapshot management** — Save, switch, and restore multiple Codex official account snapshots
- **🔁 Reliable Codex restart** — Account switches restart related Codex processes so credentials take effect quickly
- **🧭 Proxy takeover consistency** — Codex proxy takeover / restore preserves live config and OAuth details to reduce configuration drift
- **⌁ Notch quota summary** — A lightweight overlay shows the best available quota across your Codex account set

---

## 📦 Installation

### Option 1: Download DMG (macOS Apple Silicon)

Grab the latest `CC Switch` macOS package from the [Releases](https://github.com/ajia1206/cc-switch/releases) page.

```bash
# Double-click the dmg → drag to Applications
# On first launch if macOS says "cannot verify developer",
# go to System Settings → Privacy & Security → Open Anyway
```

### Option 2: Build from Source

```bash
# 1. Clone the repository
git clone https://github.com/ajia1206/cc-switch.git
cd cc-switch

# 2. Install dependencies (requires Node 22+ and the Rust toolchain)
pnpm install

# 3. Run in dev mode
pnpm dev

# 4. Build for release
pnpm tauri build
# Output: src-tauri/target/release/bundle/dmg/
```

**Prerequisites:**
- Node.js 22.12+ (recommend [fnm](https://github.com/Schniz/fnm) for version management)
- Rust 1.85+ (`rustup install stable`)
- pnpm 9+ (`npm i -g pnpm`)
- macOS: Xcode Command Line Tools

---

## 🎮 Usage

### View Multi-Account Codex Usage

1. Open CC Switch → switch to the **Codex** tab on top
2. Click **"Codex Official Account Snapshots"**
3. Each account card automatically shows its usage:

```
┌─────────────────────────────────────┐
│ 📦 My Primary Account     [Active]  │
│  ⏱ 5h Remaining: 73%  · resets in 2h│
│  📅 7d Remaining: 45%  · resets in 5d│
└─────────────────────────────────────┘
```

### View Global Usage Stats

1. Open **Usage Dashboard**
2. Pick the app scope and time range at the top
3. Use **Provider Stats** / **Model Stats** to inspect real Tokens including cache usage
4. Use the heatmap to spot active days, session counts, and token peaks across the past year

### Adjust Refresh Strategy

- **Settings → Usage Dashboard**: configure the refresh interval
- **Account panel refresh button**: trigger a concurrent query of all Codex snapshots immediately

### Switch Accounts

Click "Switch to this account" on any account card. The app will:
1. Back up the current `~/.codex/auth.json` to the active account's snapshot
2. Restore the target account's snapshot to `~/.codex/auth.json`
3. Restart Codex-related processes so the credentials take effect

---

## 🛠 Technical Implementation

| Module | File | Description |
|--------|------|-------------|
| Multi-account query | `src-tauri/src/codex_accounts.rs` | `get_all_account_quotas` queries all snapshots concurrently |
| Tauri command | `src-tauri/src/commands/codex_accounts.rs` | `get_all_codex_quotas` exposed to frontend |
| Usage cache | `src-tauri/src/services/subscription.rs` | Per-account independent TTL cache |
| Stats aggregation | `src-tauri/src/services/usage_stats.rs` | Real tokens, deduplication, provider / model stats |
| Historical rollups | `src-tauri/src/database/dao/usage_rollup.rs` | Detail-log rollups and retention |
| Settings persistence | `src-tauri/src/settings.rs` | `codex_quota_refresh_interval` field |
| Frontend query | `src/lib/query/subscription.ts` | Codex quota React Query hooks |
| Account panel | `src/components/codex/CodexAccountsPanel.tsx` | Card usage rendering + refresh controls |
| Usage heatmap | `src/components/usage/UsageActivityHeatmap.tsx` | Yearly activity and token visualization |
| Notch overlay | `src/components/NotchOverlay.tsx` | Lightweight Codex usage summary |

---

## 🙏 Acknowledgements

- Upstream: [@farion1231/cc-switch](https://github.com/farion1231/cc-switch) by Jason Young
- For full upstream features (Claude/Codex/Gemini/OpenCode/OpenClaw multi-provider switching), see the [original README](https://github.com/farion1231/cc-switch/blob/main/README.md)
- The usage-query approach in this fork was inspired by [ericjypark/codex-island](https://github.com/ericjypark/codex-island)

---

## 📄 License

This project inherits the upstream [MIT License](LICENSE). Copyright belongs to Jason Young and respective contributors.

---

## 🔗 Links

- This fork: [github.com/ajia1206/cc-switch](https://github.com/ajia1206/cc-switch)
- Upstream: [github.com/farion1231/cc-switch](https://github.com/farion1231/cc-switch)
- Releases: [github.com/ajia1206/cc-switch/releases](https://github.com/ajia1206/cc-switch/releases)
- Issues: [github.com/ajia1206/cc-switch/issues](https://github.com/ajia1206/cc-switch/issues)
