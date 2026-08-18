# Repository Guidelines

## Project Structure & Module Organization

CC Switch is a Tauri 2 desktop application. The React/TypeScript renderer lives in `src/`: domain UI is under `components/`, reusable behavior under `hooks/`, Tauri IPC clients under `lib/api/`, query state under `lib/query/`, and provider presets under `config/`. The Rust backend is in `src-tauri/src/`, organized into `commands/`, `services/`, `database/`, `proxy/`, and other domain modules. Frontend tests are grouped in `tests/`; Rust integration tests live in `src-tauri/tests/`, with unit tests beside Rust modules. Documentation belongs in `docs/`; application assets belong in `src/assets/` or `src/icons/`, while README media belongs in top-level `assets/`.

## Build, Test, and Development Commands

Use the pinned Node, pnpm, and Rust versions from `.node-version`, `package.json`, and `rust-toolchain.toml`.

- `pnpm install --frozen-lockfile`: install exact frontend dependencies.
- `pnpm dev`: launch the full Tauri app with hot reload.
- `pnpm dev:renderer`: run only the Vite renderer on port 3000.
- `pnpm build`: create a production desktop build.
- `pnpm typecheck && pnpm format:check && pnpm test:unit`: run frontend CI checks.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: verify Rust formatting.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: lint Rust strictly.
- `cargo test --manifest-path src-tauri/Cargo.toml`: run backend tests.

## Coding Style & Naming Conventions

Prettier 3 formats frontend files with two-space indentation, double quotes, and semicolons. TypeScript is strict; prefer the `@/` alias for imports. Name React components and files in `PascalCase`, hooks as `useThing`, and utilities in `camelCase`. Rust follows `rustfmt`: modules and functions use `snake_case`, types use `PascalCase`. Keep Tauri command names exposed to the renderer in `camelCase`. Never hardcode user-facing strings; use i18next and update all files in `src/i18n/locales/` (`en`, `zh`, `zh-TW`, and `ja`).

## Testing Guidelines

Frontend tests use Vitest, jsdom, Testing Library, and MSW. Name them `*.test.ts` or `*.test.tsx`, mirroring the relevant domain under `tests/` or colocating focused tests in `src/`. Add regression coverage for behavior changes; no numeric coverage threshold is configured. Rust integration tests use descriptive `snake_case` filenames in `src-tauri/tests/`.

## Commit & Pull Request Guidelines

Follow Conventional Commits, usually `type(scope): imperative summary`, for example `fix(tray): refresh menu after switch`. Open an issue before feature work, branch from `main` (for example `feat/provider-filter`), and keep each PR focused. Complete the PR template with a summary, `Fixes #123`, verification details, and before/after screenshots for UI changes. Run the applicable checks above and update every locale for visible text. Report vulnerabilities through `SECURITY.md`, not public issues.
