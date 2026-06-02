# AGENTS.md

## Quick Start
- `pnpm install`
- `pnpm dev` (Full app)
- `pnpm dev:renderer` (Fast UI dev)

## Commands
### Frontend
- `pnpm typecheck`: Verify types.
- `pnpm format:check`: Verify formatting.
- `pnpm test:unit`: Run unit tests.

### Backend (Rust)
- `cd src-tauri && cargo clippy`: Check linting.
- `cd src-tauri && cargo test`: Run backend tests.

## Architecture & Conventions
- **IPC**: Frontend communicates with Rust via Tauri commands. Use `src/lib/api/` for type-safe calls.
- **Storage**: SQLite database at `~/.cc-switch/cc-switch.db`.
- **Structure**:
  - `src/components/`: Domain-specific UI components.
  - `src/hooks/`: Business logic.
  - `src-tauri/src/commands/`: Tauri command entry points.
  - `src-tauri/src/services/`: Core backend logic.
- **Testing**: Use MSW to mock Tauri API in frontend tests.
