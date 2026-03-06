# CC-Switch - AI Coding Agents Guide

## Project Overview
CC-Switch is a Tauri 2.8+ desktop application for managing API provider configurations across Claude Code, Codex, Gemini CLI, and OpenCode. Built with React + TypeScript frontend and Rust backend using SQLite for data persistence.

**Key Features**: Provider switching, MCP server management, Skills/Prompts management, proxy failover, usage tracking, deep link imports, and i18n support (EN/ZH/JA).

## Technology Stack
**Frontend**: React 18, TypeScript, Vite 7.3, TailwindCSS 3.4, TanStack Query v5, react-hook-form + zod, shadcn/ui, @dnd-kit, framer-motion, react-i18next, Sonner toast

**Backend**: Rust 1.85+, Tauri 2.8, serde + tokio, rusqlite with bundled SQLite, axum/tower (proxy server), reqwest with rustls

**Testing**: vitest + @testing-library/react + MSW for frontend; `cargo test` for Rust backend

## Build & Test Commands
```bash
# Frontend
pnpm install          # Install dependencies
pnpm dev              # Start Tauri dev mode with hot reload
pnpm build            # Build production app
pnpm typecheck        # TypeScript type checking
pnpm format           # Format with Prettier
pnpm format:check     # Check Prettier formatting
pnpm test:unit        # Run frontend unit tests (vitest)
pnpm test:unit:watch  # Watch mode for tests

# Backend (from src-tauri/)
cargo fmt             # Format Rust code
cargo clippy          # Lint with Clippy
cargo test            # Run all Rust tests
cargo test test_name  # Run specific test
cargo test --features test-hooks  # Run with test-hooks feature
```

## Architecture
**Frontend** (`src/`):
- `components/` - UI components organized by feature (providers, mcp, skills, settings, ui)
- `hooks/` - Custom hooks for business logic (useProviderActions, useSettings, useProxyConfig, etc.)
- `lib/api/` - Type-safe Tauri API wrappers (providers, mcp, settings, proxy, usage, etc.)
- `lib/query/` - TanStack Query configurations and mutations
- `i18n/locales/` - Translation files (en.json, ja.json, zh.json)
- `config/` - Provider presets (claudeProviderPresets, universalProviderPresets, etc.)

**Backend** (`src-tauri/src/`):
- `commands/` - Tauri command handlers organized by domain (provider, mcp, proxy, settings, skill, etc.)
- `services/` - Business logic layer (ProviderService, McpService, PromptService, ProxyService, etc.)
- `database/` - SQLite DAO layer with schema migrations
- `proxy/` - Built-in proxy server with health checks and failover
- `mcp/` - MCP server configuration handling for all apps
- `app_config.rs`, `provider.rs` - Core data models

## Data Persistence
- **SQLite** (`~/.cc-switch/cc-switch.db`) - Providers, MCP servers, Prompts, Skills, settings (SSOT)
- **JSON** (`~/.cc-switch/settings.json`) - Device-level settings only
- **Live configs** - App-specific files synced on provider switch (`~/.claude/settings.json`, `~/.codex/auth.json`, `~/.gemini/.env`)

## Code Style Guidelines
**Frontend**:
- Use `@/*` path aliases for imports
- Component files: PascalCase (`ProviderCard.tsx`, `McpFormModal.tsx`)
- Hook files: camelCase with `use` prefix (`useProviderActions.ts`)
- Test files: same name with `.test.tsx` suffix
- Imports: external libs first, then internal modules, sorted
- No comments unless explicitly requested
- Use TanStack Query for data fetching with proper query keys: `['providers', appId]`
- Form validation with react-hook-form + zod, use shadcn/ui Form components
- Toasts via Sonner: `toast.success()`, `toast.error()`
- Use `extractErrorMessage()` for consistent error handling

**Backend (Rust)**:
- Modules: `snake_case` (`provider_service.rs`, `mcp_config.rs`)
- Tests: `#[cfg(test)] mod tests { ... }` inside modules
- Error handling: Use `AppError` enum from `error.rs` with thiserror
- Logging: `log::info!`, `log::warn!`, `log::error!` with structured messages
- Async: tokio runtime, use `?` operator for error propagation
- Database access via DAO methods with Mutex-protected connection

## Testing Instructions
**Frontend Unit Tests**:
```bash
pnpm test:unit                # Run all tests
pnpm test:unit -- coverage    # With coverage report
pnpm test:unit:watch          # Watch mode for development
```
- Uses vitest with jsdom environment
- Mock Tauri API calls via MSW handlers in `tests/msw/handlers.ts`
- Test utilities in `tests/setupTests.ts`
- Aim for 100% hooks coverage (useProviderActions, useSettings, etc.)

**Backend Tests**:
```bash
cd src-tauri
cargo test                    # Run all tests
cargo test provider_service    # Run module-specific tests
cargo test test_name           # Run specific test function
```
- Tests in `tests/` directory with temporary filesystem handling
- Use `test_mutex()` for test isolation
- Test helpers in `support.rs`

## Security Considerations
- API keys stored in encrypted format in SQLite (never logged)
- Deep link URLs validated and sanitized before import (`redact_url_for_log()`)
- Atomic writes with temp files for config changes
- Mutex-protected database connections prevent race conditions
- Never commit secrets or keys to repository
- Proxy configurations validated before activation

## Important Notes
- **No ESLint/Prettier config files** - use `pnpm format` (default Prettier settings)
- **TypeScript strict mode enabled** - all types must be properly defined
- **SSOT principle** - SQLite is single source of truth, live files are derived
- **Schema migrations** - Update `SCHEMA_VERSION` constant and add migration in `database/schema.rs` when changing tables
- **I18n** - Use `useTranslation()` hook with `t()` function, fallback to English if key missing
- **Component updates** - When adding features, update both frontend UI and corresponding Tauri commands
- **Run typecheck and lint** before committing: `pnpm typecheck` + `cargo clippy`
