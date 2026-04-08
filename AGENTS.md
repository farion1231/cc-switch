# AGENTS.md

Guidance for AI coding assistants (Claude Code, Codex, Gemini CLI, …) working in this
repository. Claude Code reads this file automatically; a local `CLAUDE.md` (gitignored
per `.gitignore`) may override or extend it per-developer.

## Project Overview

**CC Switch** is a cross-platform Tauri 2 desktop application that manages configurations
for multiple AI coding CLIs: **Claude Code, Codex, Gemini CLI, OpenCode, and OpenClaw**.
It provides provider switching, unified MCP/Prompts/Skills management, a local proxy with
failover, usage tracking, session browsing, and cloud sync — all backed by a SQLite SSOT.

- **Frontend**: React 18 + TypeScript + Vite + TailwindCSS 3.4 + shadcn/ui
- **Backend**: Rust (Tauri 2.8) with SQLite (`rusqlite`) persistence
- **State/cache**: TanStack Query v5 on the frontend; `Mutex<Connection>` on the backend
- **IPC**: Tauri commands (camelCase names) wrapped by a typed frontend API layer
- **i18n**: `react-i18next` with `zh` / `en` / `ja` locales (Chinese is the primary UI language)

## Repository Layout

```
├── src/                        # Frontend (React + TypeScript)
│   ├── App.tsx                 # Root shell — view routing, headers, dialogs
│   ├── main.tsx                # Bootstrap, providers, config-error handling
│   ├── components/
│   │   ├── providers/          # Provider CRUD (cards, forms, dialogs)
│   │   ├── mcp/                # Unified MCP panel + wizard
│   │   ├── prompts/            # Prompts panel (Markdown editor)
│   │   ├── skills/             # Skills install/management + repo manager
│   │   ├── sessions/           # Session manager (history browser)
│   │   ├── proxy/              # Proxy + failover panels
│   │   ├── openclaw/           # OpenClaw-specific config panels
│   │   ├── settings/           # Settings pages (theme, dir, webdav, proxy, about…)
│   │   ├── deeplink/           # ccswitch:// import confirmation dialogs
│   │   ├── env/                # Env conflict warning banner
│   │   ├── universal/          # Cross-app (universal) provider UI
│   │   ├── usage/              # Usage dashboard, charts, pricing
│   │   ├── workspace/          # OpenClaw workspace/agent file editor
│   │   └── ui/                 # shadcn/ui primitives (button, dialog, ...)
│   ├── hooks/                  # Custom React hooks (business logic glue)
│   ├── lib/
│   │   ├── api/                # Typed Tauri IPC wrappers (one module per domain)
│   │   ├── query/              # TanStack Query config + query keys
│   │   ├── schemas/            # Zod schemas (provider/mcp/settings/common)
│   │   ├── errors/             # Error parsing helpers
│   │   ├── utils/              # Small helpers (base64, ...)
│   │   ├── authBinding.ts      # Auth binding helpers
│   │   ├── clipboard.ts        # Clipboard utils
│   │   ├── platform.ts         # OS detection (isMac/isWin/isLinux)
│   │   └── updater.ts          # Updater helpers
│   ├── contexts/UpdateContext.tsx
│   ├── i18n/                   # i18next init + locales (en/zh/ja)
│   ├── config/                 # Static presets (providers, mcp)
│   ├── icons/                  # Provider icon index
│   ├── types.ts, types/        # Shared TypeScript types
│   └── utils/                  # DOM/error helpers
│
├── src-tauri/                  # Backend (Rust + Tauri 2)
│   ├── Cargo.toml              # rust-version = 1.85
│   ├── tauri.conf.json         # Deep link, updater, bundling config
│   ├── capabilities/           # Tauri permission manifests
│   └── src/
│       ├── lib.rs              # App entry, tray, deep-link, setup
│       ├── main.rs             # Binary entry delegating to lib
│       ├── commands/           # Tauri #[command] layer (by domain, mod.rs re-exports *)
│       │                       # auth, provider, mcp, prompt, skill, proxy,
│       │                       # session_manager, settings, usage, webdav_sync, …
│       ├── services/           # Business-logic layer
│       │   ├── provider/       # ProviderService (CRUD, switch, live sync, auth, usage)
│       │   ├── mcp.rs          # McpService
│       │   ├── prompt.rs       # PromptService
│       │   ├── skill.rs        # SkillService
│       │   ├── proxy.rs        # ProxyService (hot-switching local proxy)
│       │   ├── config.rs       # ConfigService (import/export, backups)
│       │   ├── speedtest.rs    # Endpoint latency
│       │   ├── webdav*.rs      # WebDAV sync engine + auto-sync
│       │   └── usage_stats.rs  # Usage aggregation
│       ├── database/
│       │   ├── mod.rs          # Database struct, Mutex<Connection>, hooks
│       │   ├── schema.rs       # Schema + migration (SCHEMA_VERSION = 6)
│       │   ├── migration.rs    # JSON → SQLite migration
│       │   ├── backup.rs       # Snapshot + SQL export
│       │   └── dao/            # providers, mcp, prompts, skills, settings, proxy,
│       │                       # failover, stream_check, usage_rollup, universal_providers
│       ├── proxy/              # Local HTTP proxy (forwarder, circuit breaker, SSE,
│       │                       #   failover, model mapping, thinking rectifier, …)
│       ├── mcp/                # MCP live-file sync per app
│       ├── session_manager/    # Conversation history browser
│       ├── deeplink/           # ccswitch:// URL parser + importer
│       ├── store.rs            # AppState (Arc<Database>, caches)
│       ├── config.rs           # Paths helper (get_app_config_dir, …)
│       ├── app_config.rs       # AppType, MultiAppConfig, domain models
│       ├── provider.rs         # Provider model
│       ├── {claude,codex,gemini,opencode,openclaw}_config.rs  # Per-app live-file IO
│       ├── {claude_mcp,claude_plugin,gemini_mcp}.rs           # App-specific helpers
│       ├── settings.rs         # AppSettings
│       ├── tray.rs             # System tray + quick switch
│       ├── error.rs            # AppError (thiserror)
│       ├── panic_hook.rs
│       └── ...
│   └── tests/                  # Rust integration tests (provider, mcp, deeplink, skill, …)
│
├── tests/                      # Frontend test suite (vitest)
│   ├── setupGlobals.ts, setupTests.ts
│   ├── msw/                    # MSW handlers + tauri IPC mocks + state
│   ├── components/             # Component tests
│   ├── hooks/                  # Hook tests
│   ├── integration/            # App-level flows
│   ├── config/                 # Preset sanity tests
│   └── utils/                  # testQueryClient + helpers
│
├── docs/                       # User manual, release notes, proxy guide
├── scripts/                    # Icon extraction & index generation
├── assets/                     # Screenshots, partner logos
├── flatpak/                    # Flatpak build instructions
├── package.json                # pnpm scripts (dev/build/typecheck/test/format)
├── vite.config.ts              # root = src, alias @ → src
├── vitest.config.ts            # jsdom + setup files
├── tsconfig.json               # strict; noUnusedLocals/Parameters
├── tailwind.config.cjs, postcss.config.cjs, components.json (shadcn)
└── README.md / README_ZH.md / README_JA.md / CHANGELOG.md / CONTRIBUTING.md
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Frontend (React + TS)                    │
│  Components → Hooks (business logic) → TanStack Query       │
│                             │                               │
│                 src/lib/api/* (typed invoke wrappers)       │
└────────────────────────────┬────────────────────────────────┘
                             │ Tauri IPC (camelCase commands)
┌────────────────────────────▼────────────────────────────────┐
│                 Backend (Rust + Tauri 2.8)                  │
│  commands/* (#[tauri::command])                             │
│         │                                                   │
│         ▼                                                   │
│  services/*  (ProviderService, McpService, PromptService,   │
│               SkillService, ProxyService, ConfigService…)   │
│         │                                                   │
│         ▼                                                   │
│  database/dao/*  →  Mutex<rusqlite::Connection>             │
│                                                             │
│  + per-app live-file writers (claude/codex/gemini/…)        │
│  + proxy/ (hyper + rustls local HTTP proxy)                 │
│  + session_manager/, deeplink/, mcp/, tray, updater         │
└─────────────────────────────────────────────────────────────┘
```

### Core Design Principles

- **Single Source of Truth (SSOT)** — SQLite at `~/.cc-switch/cc-switch.db` holds providers,
  MCP, prompts, skills, settings. Syncable state lives in the DB; device-level UI
  preferences live in `~/.cc-switch/settings.json`.
- **Dual-way live sync** — On switch, services write the active provider into the CLI's
  real config files (e.g. `~/.claude/settings.json`, `~/.codex/config.toml`). When editing
  the currently active provider, changes are backfilled from the live file first to avoid
  losing edits the user made outside the app.
- **Atomic writes** — Write to a temp file and rename. Never overwrite a live config
  in-place.
- **Concurrency safety** — `Database` wraps `rusqlite::Connection` in a `Mutex`, exposed
  through `AppState` as `Arc<Database>`. Use the `lock_conn!` macro (see
  `src-tauri/src/database/mod.rs`) instead of raw `.lock().unwrap()`.
- **Layered backend** — `commands → services → dao → database`. Commands must stay thin;
  put business logic in services. DAOs are the only layer that touches SQL.
- **Auto backups** — `~/.cc-switch/backups/` keeps the 10 most recent snapshots;
  `~/.cc-switch/skill-backups/` keeps up to 20 before skill uninstall.

### Key Services

| Service            | Responsibility                                                          |
| ------------------ | ----------------------------------------------------------------------- |
| `ProviderService`  | Provider CRUD, switching, live-file sync, backfill, sort, auth, usage   |
| `McpService`       | MCP server CRUD + bidirectional sync across Claude/Codex/Gemini/OpenCode|
| `PromptService`    | Prompt presets, active sync to `CLAUDE.md` / `AGENTS.md` / `GEMINI.md`  |
| `SkillService`     | Skill install from GitHub/ZIP, symlink or copy mode, repo management    |
| `ProxyService`     | Local HTTP proxy (hyper+rustls) with hot-switch, failover, rectifiers   |
| `ConfigService`    | Import/export, backup rotation                                          |
| `SpeedtestService` | API endpoint latency probing                                            |

### Data Locations

- `~/.cc-switch/cc-switch.db` — SQLite SSOT (schema version 6)
- `~/.cc-switch/settings.json` — device-level UI preferences
- `~/.cc-switch/backups/` — auto-rotated DB snapshots (keeps 10)
- `~/.cc-switch/skills/` — skills (symlinked into each app by default)
- `~/.cc-switch/skill-backups/` — pre-uninstall skill backups (keeps 20)

## Development Workflow

### Prerequisites

- **Node.js 22.12** (see `.node-version`) — 18+ works but CI pins 20
- **pnpm 10.12.3** (pinned in CI; pnpm-workspace)
- **Rust 1.85+** (pinned in `Cargo.toml`)
- **Tauri 2.0 system deps** — see https://v2.tauri.app/start/prerequisites/

### Common Commands

```bash
pnpm install               # Install frontend deps
pnpm dev                   # Run full app (tauri dev with hot reload)
pnpm dev:renderer          # Vite-only (no Tauri shell) — useful for UI-only work
pnpm build                 # Production build (tauri build)
pnpm typecheck             # tsc --noEmit (strict)
pnpm format                # Prettier write on src/**
pnpm format:check          # Prettier check (CI)
pnpm test:unit             # vitest run
pnpm test:unit:watch       # vitest in watch mode
```

Rust backend (from `src-tauri/`):

```bash
cargo fmt                  # Format
cargo fmt --check          # CI format check
cargo clippy -- -D warnings
cargo test                 # Backend + integration tests
cargo test --features test-hooks
```

### Pre-submission Checklist

CI will run these; run locally before opening a PR:

```bash
pnpm typecheck && pnpm format:check && pnpm test:unit
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

### Testing

- **Frontend**: `vitest` + `jsdom` + `@testing-library/react`. Tauri `invoke` is mocked via
  `tests/msw/tauriMocks.ts`; network requests are mocked with MSW. Shared state
  (providers etc.) is reset between tests in `tests/setupTests.ts`.
- **Test query client**: use `tests/utils/testQueryClient.ts` instead of the app client —
  it disables retries/cache for deterministic tests.
- **Backend**: integration tests live in `src-tauri/tests/`; unit tests are co-located in
  modules. Many tests use `serial_test::serial` because they mutate `HOME`/env — do not
  run them with parallelism hacks, and don't remove the `#[serial]` attribute.
- **Rust test-only hooks**: the `test-hooks` cargo feature gates extra test instrumentation.

### CI (`.github/workflows/ci.yml`)

Two jobs on PRs and pushes to `main`:

1. **Frontend Checks** (ubuntu-latest): `pnpm typecheck`, `pnpm format:check`, `pnpm test:unit`
2. **Backend Checks** (ubuntu-22.04): installs GTK/WebKit deps, then
   `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`

## Conventions

### Tauri 2.0 IPC

- **Command names are camelCase** on the JS side (e.g. `getProviders`, `switchProvider`).
  On the Rust side, the `#[tauri::command]` functions use snake_case with the
  `#![allow(non_snake_case)]` at the crate boundary in `commands/mod.rs`.
- **Never call `invoke` directly in components** — add the call to `src/lib/api/*.ts`
  with a typed signature, then import from `@/lib/api`. See `src/lib/api/providers.ts`
  for the pattern.
- **Payloads use camelCase**: Rust types carry `#[serde(rename_all = "camelCase")]` where
  they cross the IPC boundary.

### Frontend

- **Import alias**: `@/` resolves to `src/` (configured in `vite.config.ts`, `tsconfig.json`,
  `vitest.config.ts`). Use `@/components/...`, `@/lib/...`, `@/hooks/...`.
- **Data access**: Prefer TanStack Query hooks from `src/lib/query/` (e.g.
  `useProvidersQuery`, `useSettingsQuery`) rather than calling the API layer ad-hoc —
  they own cache keys and invalidation.
- **Forms**: `react-hook-form` + `zod` resolvers; schemas live in `src/lib/schemas/`.
- **UI kit**: shadcn/ui primitives under `src/components/ui/`. Configure new primitives
  via `components.json` (`npx shadcn add ...`). Icons: `lucide-react`.
- **Styling**: Tailwind utility classes; use the `cn()` helper from `@/lib/utils`.
  Dark/light/system theme is controlled by `ThemeProvider`.
- **State strictness**: `noUnusedLocals` and `noUnusedParameters` are on — prefix
  intentionally-unused args with `_`.

### Backend (Rust)

- **Errors**: return `Result<T, AppError>` (from `src-tauri/src/error.rs`, built on
  `thiserror`). Do not `unwrap()` outside tests; use `?` and map into `AppError`.
- **Concurrency**: never hold a DB lock across an `.await`. Use the `lock_conn!` macro
  from `database/mod.rs` for short critical sections.
- **JSON serialization**: use `database::to_json_string` for DB payloads to avoid panics.
- **Live-file IO**: always go through the per-app writer modules
  (`claude_config.rs`, `codex_config.rs`, etc.) — they implement atomic temp+rename.
- **Adding a new Tauri command**:
  1. Implement logic in the appropriate `services/*` module.
  2. Add a thin `#[tauri::command]` wrapper in `src-tauri/src/commands/<domain>.rs`.
  3. Register it in the `tauri::generate_handler!` list in `src-tauri/src/lib.rs`.
  4. Add the typed wrapper to `src/lib/api/<domain>.ts` and re-export from
     `src/lib/api/index.ts`.
  5. If it touches DB schema, bump `SCHEMA_VERSION` in `database/mod.rs` and add a
     migration step in `database/schema.rs` or `database/migration.rs`.

### Internationalization

CC Switch ships **three locales** and requires all of them to stay in sync:

- `src/i18n/locales/en.json`
- `src/i18n/locales/zh.json` (primary)
- `src/i18n/locales/ja.json`

Rules:

1. Never hardcode user-visible strings. Always use `t('namespace.key')` from
   `react-i18next`.
2. When adding/renaming a key, update **all three** files.
3. When removing a key, delete it from all three files.
4. Chinese is the authoritative source for meaning — follow the tone of existing zh
   strings when writing new ones.

### Commit Style

[Conventional Commits](https://www.conventionalcommits.org/):

```
feat(provider): add AWS Bedrock preset
fix(tray): resolve menu not refreshing after switch
docs(readme): update install instructions
ci: add format check workflow
chore(deps): bump tauri to 2.8.2
```

Scope should usually match the subsystem (`provider`, `mcp`, `prompt`, `skill`, `proxy`,
`session`, `tray`, `deeplink`, `usage`, `settings`, `i18n`, `backend`, `frontend`, …).

### Pull Requests

- **Open an issue first** for new features — drive-by feature PRs can be closed.
- **Keep PRs small and focused.** One issue, one PR.
- `main` is the base branch; use `feat/…` or `fix/…` branches.
- The repo enforces "explain every line" for AI-assisted PRs — see `CONTRIBUTING.md`.

## Things to Avoid

- **Don't bypass the service/DAO layers.** Commands must not call `rusqlite` directly,
  and components must not call `invoke` directly.
- **Don't mutate live CLI config files outside the dedicated writer modules.** They
  guarantee atomicity and backfill semantics.
- **Don't add fields to the Tauri IPC boundary without `#[serde(rename_all = "camelCase")]`.**
- **Don't remove `#[serial]` from backend tests that touch HOME / env** — they'll race.
- **Don't add a new i18n key to only one language file** — CI doesn't catch it, but users will.
- **Don't add emojis to source files / commits / UI copy** unless the user explicitly asks.
- **Don't create new top-level docs** (README variants, wiki pages) unless asked — prefer
  editing `docs/user-manual/` or the existing README.
- **Don't touch `CHANGELOG.md` by hand** for routine changes — it's maintained per release.

## Quick References

- **Main app shell**: `src/App.tsx` (view routing + header)
- **Bootstrap / providers**: `src/main.tsx`
- **Tauri entry**: `src-tauri/src/lib.rs`
- **Command registration**: search for `tauri::generate_handler!` in `src-tauri/src/lib.rs`
- **DB schema + migrations**: `src-tauri/src/database/schema.rs`,
  `src-tauri/src/database/migration.rs`
- **Per-app live config IO**: `src-tauri/src/{claude,codex,gemini,opencode,openclaw}_config.rs`
- **Local proxy**: `src-tauri/src/proxy/` (entry `mod.rs` → `server.rs`)
- **Frontend API layer**: `src/lib/api/*` re-exported from `src/lib/api/index.ts`
- **Query keys & hooks**: `src/lib/query/`
- **Test IPC mocks**: `tests/msw/tauriMocks.ts` + `tests/msw/state.ts`
